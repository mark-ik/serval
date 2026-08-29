//! The document's clocks: CSS transitions and keyframe animations.
//!
//! `pump` advances them against a host-supplied millisecond clock, and
//! `settled` reports when nothing is left to advance.

use super::*;

impl<D> LiveryDocument<D>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    /// Start a host-driven opacity transition for one retained element. This
    /// is the runtime clock seam. CSS transitions use the same clock when the bounded transition
    /// longhands are present; this explicit method remains useful to hosts
    /// that need a direct paint-only animation.
    pub fn animate_opacity(
        &mut self,
        node: D::NodeId,
        from: f32,
        to: f32,
        start_ms: f64,
        duration_ms: f64,
    ) -> bool {
        if !from.is_finite()
            || !to.is_finite()
            || !start_ms.is_finite()
            || !duration_ms.is_finite()
            || duration_ms < 0.0
        {
            return false;
        }
        self.clock_ms = start_ms;
        self.transitions
            .retain(|transition| transition.property != PropertyId::Opacity);
        self.transitions.push(PropertyTransition {
            node,
            property: PropertyId::Opacity,
            from: PropertyValue::Opacity(Opacity::from_value(from.clamp(0.0, 1.0))),
            to: PropertyValue::Opacity(Opacity::from_value(to.clamp(0.0, 1.0))),
            start_ms,
            duration_ms,
            automatic: false,
        });
        self.cached = None;
        true
    }

    /// Advance retained animation time. A following frame samples the
    /// interpolated style through the ordinary retained-layout path, so text
    /// shaping and every paint consumer see the same frame value.
    pub fn pump(&mut self, now_ms: f64) -> bool {
        if (self.transitions.is_empty() && self.keyframe_animation.is_none()) || !now_ms.is_finite()
        {
            return false;
        }
        let next = now_ms.max(self.clock_ms);
        let changed = next != self.clock_ms;
        self.clock_ms = next;
        if changed {
            self.cached = None;
            self.layout_dirty = true;
        }
        changed
    }

    pub fn settled(&self) -> bool {
        let transitions_settled = self
            .transitions
            .iter()
            .all(|transition| self.clock_ms >= transition.start_ms + transition.duration_ms);
        let keyframe_settled = self
            .keyframe_animation
            .as_ref()
            .is_none_or(|animation| self.clock_ms >= animation.start_ms + animation.duration_ms);
        transitions_settled && keyframe_settled
    }

    pub(in crate::document) fn apply_transitions(&self, styles: &mut StylePlane<D::NodeId>) {
        for transition in &self.transitions {
            let progress = if transition.duration_ms == 0.0 {
                1.0
            } else {
                ((self.clock_ms - transition.start_ms) / transition.duration_ms).clamp(0.0, 1.0)
                    as f32
            };
            let value = transition.from.interpolate(&transition.to, progress);
            if let Some(style) = styles.get_mut(transition.node) {
                let _ = style.set(transition.property, value);
            }
        }
    }

    pub(in crate::document) fn apply_keyframe_animation(&self, styles: &mut StylePlane<D::NodeId>) {
        let Some(animation) = self.keyframe_animation.as_ref() else {
            return;
        };
        let Some(keyframes) = self.style_set.keyframes(&animation.name) else {
            return;
        };
        let progress = if animation.duration_ms == 0.0 {
            1.0
        } else {
            ((self.clock_ms - animation.start_ms) / animation.duration_ms).clamp(0.0, 1.0) as f32
        };
        if self.clock_ms < animation.start_ms {
            return;
        }
        let progress = animation.timing.sample(progress);
        let Some(base) = styles.get(animation.node).cloned() else {
            return;
        };
        let Some(context) = styles.used_color_context(animation.node) else {
            return;
        };
        let updates = keyframe_properties(keyframes)
            .into_iter()
            .filter_map(|property| {
                keyframe_value(keyframes, property, progress, base.get(property), context)
                    .map(|value| (property, value))
            })
            .collect::<Vec<_>>();
        if let Some(style) = styles.get_mut(animation.node) {
            for (property, value) in updates {
                let _ = style.set(property, value);
            }
        }
    }

    pub(in crate::document) fn schedule_keyframe_animation(
        &mut self,
        styles: &StylePlane<D::NodeId>,
    ) {
        let candidate = self.find_keyframe_animation(self.dom.document(), styles);
        let Some((node, name, duration_ms, delay_ms, timing)) = candidate else {
            self.keyframe_animation = None;
            return;
        };
        if self.keyframe_animation.as_ref().is_some_and(|animation| {
            animation.node == node
                && animation.name.as_ref() == name.as_str()
                && animation.duration_ms == duration_ms
                && animation.delay_ms == delay_ms
                && animation.timing == timing
        }) {
            return;
        }
        self.keyframe_animation = Some(KeyframeAnimation {
            node,
            name: name.into_boxed_str(),
            start_ms: self.clock_ms + delay_ms,
            duration_ms,
            delay_ms,
            timing,
        });
    }

    pub(in crate::document) fn find_keyframe_animation(
        &self,
        id: D::NodeId,
        styles: &StylePlane<D::NodeId>,
    ) -> Option<(D::NodeId, String, f64, f64, TimingFunction)> {
        if let Some(style) = styles.get(id)
            && let AnimationName::Name(name) = &style.animation_name
        {
            let duration_ms = f64::from(style.animation_duration.milliseconds());
            if duration_ms > 0.0 && self.style_set.keyframes(name).is_some() {
                return Some((
                    id,
                    name.to_string(),
                    duration_ms,
                    f64::from(style.animation_delay.milliseconds()),
                    style.animation_timing_function,
                ));
            }
        }
        self.dom
            .dom_children(id)
            .find_map(|child| self.find_keyframe_animation(child, styles))
    }

    pub(in crate::document) fn finish_completed_transitions(&mut self) {
        let clock_ms = self.clock_ms;
        let mut finished = Vec::new();
        self.transitions.retain(|transition| {
            let done =
                transition.automatic && clock_ms >= transition.start_ms + transition.duration_ms;
            if done {
                finished.push(transition.clone());
            }
            !done
        });
        if let Some(layout) = self.layout.as_mut() {
            for transition in finished {
                if let Some(style) = layout.styles.get_mut(transition.node) {
                    let _ = style.set(transition.property, transition.to);
                }
            }
        }
    }

    pub(in crate::document) fn schedule_transitions(&mut self, styles: &StylePlane<D::NodeId>) {
        let Some(layout) = self.layout.as_ref().or(self.identity_source.as_ref()) else {
            return;
        };
        // One live transition per longhand at a time, as the per-property
        // clock had it; the first differing node in DOM order wins.
        let mut scheduled = Vec::new();
        for &property in TransitionProperty::TRANSITIONABLE {
            if self
                .transitions
                .iter()
                .any(|transition| transition.property == property)
            {
                continue;
            }
            if let Some(transition) =
                self.find_property_transition(self.dom.document(), &layout.styles, styles, property)
            {
                scheduled.push(transition);
            }
        }
        self.transitions.extend(scheduled);
    }

    pub(in crate::document) fn find_property_transition(
        &self,
        id: D::NodeId,
        previous: &StylePlane<D::NodeId>,
        styles: &StylePlane<D::NodeId>,
        property: PropertyId,
    ) -> Option<PropertyTransition<D::NodeId>> {
        if let (Some(old), Some(new)) = (previous.get(id), styles.get(id)) {
            let duration_ms = f64::from(new.transition_duration.milliseconds());
            if duration_ms > 0.0 && new.transition_property.includes_property(property) {
                let from = previous.resolve_used_color_value(id, old.get(property));
                let to = styles.resolve_used_color_value(id, new.get(property));
                if from != to {
                    return Some(PropertyTransition {
                        node: id,
                        property,
                        from,
                        to,
                        start_ms: self.clock_ms,
                        duration_ms,
                        automatic: true,
                    });
                }
            }
        }
        self.dom
            .dom_children(id)
            .find_map(|child| self.find_property_transition(child, previous, styles, property))
    }
}

fn keyframe_properties(keyframes: &Keyframes) -> Vec<PropertyId> {
    let mut properties = Vec::new();
    for declaration in keyframes
        .frames()
        .iter()
        .flat_map(|frame| &frame.declarations().declarations)
    {
        if declaration.property.metadata().animation != AnimationClass::None
            && matches!(declaration.value, DeclaredValue::Value(_))
            && !properties.contains(&declaration.property)
        {
            properties.push(declaration.property);
        }
    }
    properties
}

fn keyframe_value(
    keyframes: &Keyframes,
    property: PropertyId,
    progress: f32,
    fallback: PropertyValue,
    context: livery::values::UsedColorContext,
) -> Option<PropertyValue> {
    let samples = keyframes
        .frames()
        .iter()
        .filter_map(|frame| {
            frame
                .declarations()
                .declarations
                .iter()
                .rev()
                .find(|declaration| declaration.property == property)
                .and_then(|declaration| match &declaration.value {
                    DeclaredValue::Value(value) => Some((
                        frame.offset(),
                        resolve_keyframe_color_value(value.clone(), context),
                    )),
                    _ => None,
                })
        })
        .collect::<Vec<_>>();
    let first_offset = samples.first().map(|(offset, _)| *offset)?;
    let mut samples = samples;
    if first_offset > 0.0 {
        samples.insert(
            0,
            (0.0, resolve_keyframe_color_value(fallback.clone(), context)),
        );
    }
    if samples.last().is_some_and(|(offset, _)| *offset < 1.0) {
        samples.push((1.0, resolve_keyframe_color_value(fallback, context)));
    }
    if progress <= samples[0].0 {
        return Some(samples[0].1.clone());
    }
    for pair in samples.windows(2) {
        let [(left_offset, left_value), (right_offset, right_value)] = pair else {
            continue;
        };
        if progress <= *right_offset {
            let span = (*right_offset - *left_offset).max(f32::EPSILON);
            let local = ((progress - *left_offset) / span).clamp(0.0, 1.0);
            return Some(left_value.interpolate(right_value, local));
        }
    }
    samples.last().map(|(_, value)| value.clone())
}

fn resolve_keyframe_color_value(
    value: PropertyValue,
    context: livery::values::UsedColorContext,
) -> PropertyValue {
    match value {
        PropertyValue::Color(color) => PropertyValue::Color(
            livery::values::ComputedColor::Absolute(color.resolve_used(context)),
        ),
        PropertyValue::BackgroundImage(livery::values::BackgroundImage::LinearGradient {
            from,
            to,
        }) => PropertyValue::BackgroundImage(livery::values::BackgroundImage::LinearGradient {
            from: livery::values::ComputedColor::Absolute(from.resolve_used(context)),
            to: livery::values::ComputedColor::Absolute(to.resolve_used(context)),
        }),
        PropertyValue::BoxShadow(livery::values::BoxShadow::Value(mut shadow)) => {
            shadow.color =
                livery::values::ComputedColor::Absolute(shadow.color.resolve_used(context));
            PropertyValue::BoxShadow(livery::values::BoxShadow::Value(shadow))
        },
        value => value,
    }
}
