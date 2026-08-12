//! Retained Livery document ownership.

use std::{collections::{HashMap, HashSet}, hash::Hash};

use layout_dom_api::{DomMutation, LayoutDom, LayoutDomMut, LocalName, Namespace, NodeKind};
use livery::cascade::DeclaredValue;
use livery::media::{Device, SystemPalette};
use livery::{AnimationClass, PropertyId};
use livery::{
    PropertyValue,
    selector::StatePseudoClass,
    stylesheet::Keyframes,
    values::{AnimationName, ColorScheme, Opacity, Overflow, TimingFunction, TransitionProperty},
};
use paint_list_api::DeviceIntSize;

use crate::{
    IncrementalStyle, InteractionStates, LayoutError, LiveryLayout, LiveryPaintList, RestyleStats,
    StylePlane, StyleSet, TextRange, TextRect, TextSelection, TextSystem,
    emit_paint_list_with_text_system_scrolled_with_images, hit_test_with_scroll,
    layout::{
        RetainedRootFormatting, StickyScrollport, layout_retained_formatting_root,
        layout_with_text_system, retained_table_owner,
        resolve_container_query_styles,
        resolve_container_query_styles_with_images,
    },
    resolve_styles,
};

/// What a Livery click resolved to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClickOutcome {
    None,
    Focused,
    Scrolled,
    Navigate(String),
}

/// A link rectangle retained from the last layout pass.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkTarget {
    pub url: String,
    pub rect: [f32; 4],
}

/// The event class that invalidated retained layout geometry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LayoutDamageKind {
    Dom,
    Stylesheet,
    Device,
    Resource,
    Interaction,
    Viewport,
}

/// Conservative formatting-context candidates for the next retained-layout
/// replacement. K5h records this separately from style invalidation so a
/// caller can inspect exactly why a frame was rebuilt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutDamage<Id> {
    pub kind: LayoutDamageKind,
    pub roots: Vec<Id>,
    pub full_document: bool,
}

struct LayoutState<Id> {
    viewport: (u32, u32),
    styles: StylePlane<Id>,
    fragments: LiveryLayout<Id>,
    content_width: f32,
    content_height: f32,
}

fn nodes_in_subtree<D>(dom: &D, root: D::NodeId) -> HashSet<D::NodeId>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    fn visit<D>(dom: &D, node: D::NodeId, nodes: &mut HashSet<D::NodeId>)
    where
        D: LayoutDom,
        D::NodeId: Copy + Eq + Hash,
    {
        nodes.insert(node);
        for child in dom.dom_children(node) {
            visit(dom, child, nodes);
        }
    }

    let mut nodes = HashSet::new();
    visit(dom, root, &mut nodes);
    nodes
}

fn text_sources_in_dom_order<D>(dom: &D) -> Vec<D::NodeId>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    fn visit<D>(dom: &D, node: D::NodeId, sources: &mut Vec<D::NodeId>)
    where
        D: LayoutDom,
        D::NodeId: Copy + Eq + Hash,
    {
        if dom.kind(node) == NodeKind::Text {
            sources.push(node);
        }
        for child in dom.dom_children(node) {
            visit(dom, child, sources);
        }
    }

    let mut sources = Vec::new();
    visit(dom, dom.document(), &mut sources);
    sources
}

/// One retained transition on the generic clock (harvest H2): any
/// transitionable longhand, sampled through the generated
/// `PropertyValue::interpolate` dispatch.
#[derive(Clone)]
struct PropertyTransition<Id> {
    node: Id,
    property: PropertyId,
    from: PropertyValue,
    to: PropertyValue,
    start_ms: f64,
    duration_ms: f64,
    automatic: bool,
}

#[derive(Clone)]
struct KeyframeAnimation<Id> {
    node: Id,
    name: Box<str>,
    start_ms: f64,
    duration_ms: f64,
    delay_ms: f64,
    timing: TimingFunction,
}

/// A retained DOM plus the Livery state that should survive between frames.
///
/// Equal-size frames reuse the complete paint list. Resizes recascade media
/// queries, relayout, and repaint while retaining Parley's font database,
/// shaping scratch space, and shared font-resource allocations.
pub struct LiveryDocument<D>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    dom: D,
    style_set: StyleSet,
    device: Device,
    interactions: InteractionStates<D::NodeId>,
    style_session: IncrementalStyle<D::NodeId>,
    text: TextSystem,
    generation: u64,
    layout_generation: u64,
    #[cfg(test)]
    retained_root_relayout_generation: u64,
    cached: Option<((u32, u32), LiveryPaintList)>,
    layout: Option<LayoutState<D::NodeId>>,
    identity_source: Option<LayoutState<D::NodeId>>,
    last_layout_damage: Option<LayoutDamage<D::NodeId>>,
    layout_dirty: bool,
    viewport: (u32, u32),
    scroll: (f32, f32),
    focused_chain: Vec<D::NodeId>,
    clock_ms: f64,
    transitions: Vec<PropertyTransition<D::NodeId>>,
    keyframe_animation: Option<KeyframeAnimation<D::NodeId>>,
    nested_scroll: HashMap<D::NodeId, (f32, f32)>,
    image_sources: HashMap<String, Vec<u8>>,
    font_sources: HashMap<String, Vec<u8>>,
    selection_anchor: Option<(D::NodeId, usize)>,
    selection_range: Option<TextRange<D::NodeId>>,
}

impl<D> LiveryDocument<D>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    pub fn new(dom: D, style_set: StyleSet, device: Device) -> Self {
        let viewport = (
            device.viewport_width.max(0.0) as u32,
            device.viewport_height.max(0.0) as u32,
        );
        Self {
            dom,
            style_set,
            device,
            interactions: InteractionStates::default(),
            style_session: IncrementalStyle::new(),
            text: TextSystem::new(),
            generation: 0,
            layout_generation: 0,
            #[cfg(test)]
            retained_root_relayout_generation: 0,
            cached: None,
            layout: None,
            identity_source: None,
            last_layout_damage: None,
            layout_dirty: true,
            viewport,
            scroll: (0.0, 0.0),
            focused_chain: Vec::new(),
            clock_ms: 0.0,
            transitions: Vec::new(),
            keyframe_animation: None,
            nested_scroll: HashMap::new(),
            image_sources: HashMap::new(),
            font_sources: HashMap::new(),
            selection_anchor: None,
            selection_range: None,
        }
    }

    pub fn dom(&self) -> &D {
        &self.dom
    }

    pub fn text_system(&self) -> &TextSystem {
        &self.text
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The number of completed geometry passes. Paint-only and scroll-only
    /// frames advance [`Self::generation`] but not this counter.
    pub fn layout_generation(&self) -> u64 {
        self.layout_generation
    }

    pub fn interactions_mut(&mut self) -> &mut InteractionStates<D::NodeId> {
        self.record_full_layout_damage(LayoutDamageKind::Interaction);
        self.retain_layout_identity();
        &mut self.interactions
    }

    pub fn invalidate(&mut self) {
        self.invalidate_with_layout_damage(LayoutDamageKind::Device);
    }

    fn invalidate_with_layout_damage(&mut self, kind: LayoutDamageKind) {
        self.record_full_layout_damage(kind);
        self.retain_layout_identity();
        self.style_session.invalidate();
    }

    /// The latest retained-layout damage classification. The current K5g
    /// correctness path still rebuilds a full layout; K5h will replace these
    /// named roots rather than treating `RestyleStats` as geometry proof.
    pub fn last_layout_damage(&self) -> Option<&LayoutDamage<D::NodeId>> {
        self.last_layout_damage.as_ref()
    }

    fn record_full_layout_damage(&mut self, kind: LayoutDamageKind) {
        self.last_layout_damage = Some(LayoutDamage {
            kind,
            roots: vec![self.dom.document()],
            full_document: true,
        });
    }

    fn record_dom_layout_damage(&mut self, mutations: &[DomMutation<D::NodeId>]) {
        let mut roots = Vec::new();
        for mutation in mutations {
            match *mutation {
                DomMutation::Inserted { parent, .. }
                | DomMutation::Removed {
                    former_parent: parent,
                    ..
                }
                | DomMutation::SubtreeReplaced { node: parent }
                | DomMutation::AttributeChanged { node: parent, .. } => {
                    self.insert_damage_root(&mut roots, self.formatting_damage_root(parent));
                },
                DomMutation::CharacterDataChanged { node } => {
                    let parent = self.dom.parent(node).unwrap_or(node);
                    self.insert_damage_root(&mut roots, self.formatting_damage_root(parent));
                },
                DomMutation::Moved {
                    from_parent,
                    to_parent,
                    ..
                } => {
                    self.insert_damage_root(&mut roots, self.formatting_damage_root(from_parent));
                    self.insert_damage_root(&mut roots, self.formatting_damage_root(to_parent));
                },
            }
        }
        self.last_layout_damage = Some(LayoutDamage {
            kind: LayoutDamageKind::Dom,
            roots,
            full_document: false,
        });
    }

    fn formatting_damage_root(&self, node: D::NodeId) -> D::NodeId {
        let Some(layout) = self.layout.as_ref() else {
            return self.dom.document();
        };
        let mut candidate = Some(node);
        while let Some(current) = candidate {
            if layout
                .fragments
                .boxes()
                .boxes_for_node(current)
                .iter()
                .any(|box_id| {
                    layout.fragments.boxes()[*box_id]
                        .formatting_context
                        .is_some()
                })
            {
                return current;
            }
            candidate = self.dom.parent(current);
        }
        self.dom.document()
    }

    fn insert_damage_root(&self, roots: &mut Vec<D::NodeId>, candidate: D::NodeId) {
        if roots
            .iter()
            .copied()
            .any(|root| self.is_dom_ancestor(root, candidate))
        {
            return;
        }
        roots.retain(|root| !self.is_dom_ancestor(candidate, *root));
        roots.push(candidate);
    }

    fn is_dom_ancestor(&self, ancestor: D::NodeId, node: D::NodeId) -> bool {
        let mut current = Some(node);
        while let Some(candidate) = current {
            if candidate == ancestor {
                return true;
            }
            current = self.dom.parent(candidate);
        }
        false
    }

    /// Discard visible derived output while retaining the immediately prior
    /// layout as K5g's identity source. The next frame recomputes geometry and
    /// reconciles only compatible generated boxes and fragments against it.
    fn retain_layout_identity(&mut self) {
        self.mark_layout_dirty();
        if let Some(layout) = self.layout.take() {
            self.identity_source = Some(layout);
        }
    }

    fn mark_layout_dirty(&mut self) {
        self.cached = None;
        self.layout_dirty = true;
    }

    /// Apply one exact DOM-mutation batch before the next frame.
    ///
    /// A retained frame is otherwise eligible for paint-list reuse. A table
    /// mutation must not reuse its old candidates, winner grid, metrics, or
    /// collapsed-border paint, so any nonempty batch discards every derived
    /// frame artifact and updates the retained style plane first. The next
    /// [`Self::frame`] then rebuilds layout and paint from that same style
    /// generation. The current correctness path deliberately rebuilds
    /// geometry; K5g reconciles only compatible identities from the retained
    /// prior generation, and callers must not infer incremental table geometry
    /// from `RestyleStats`.
    pub fn apply_dom_mutations(&mut self, mutations: &[DomMutation<D::NodeId>]) -> RestyleStats {
        if mutations.is_empty() {
            return self.style_session.last_stats();
        }

        self.record_dom_layout_damage(mutations);
        self.mark_layout_dirty();
        self.transitions.clear();
        self.keyframe_animation = None;
        self.style_session.update(
            &self.dom,
            &self.style_set,
            &self.device,
            &self.interactions,
            mutations,
        )
    }

    /// Mutate the owned DOM and atomically hand its exact recorded batch to
    /// [`Self::apply_dom_mutations`]. This is the retained-document entry point
    /// for script-hosted tables: a caller cannot accidentally paint a cached
    /// frame after changing table structure or a participating border style.
    pub fn mutate_dom<R>(&mut self, mutate: impl FnOnce(&mut D) -> R) -> (R, RestyleStats)
    where
        D: LayoutDomMut,
    {
        let result = mutate(&mut self.dom);
        let mut mutations = Vec::new();
        self.dom.drain_mutations(&mut mutations);
        let stats = self.apply_dom_mutations(&mutations);
        (result, stats)
    }

    /// Set the host preference that media queries and an element's supported
    /// `color-scheme` list consult. A real change invalidates style, layout,
    /// and paint together; a repeated setting is intentionally a no-op.
    pub fn set_preferred_color_scheme(&mut self, scheme: ColorScheme) -> bool {
        if self.device.preferred_color_scheme() == scheme {
            return false;
        }
        self.device.set_preferred_color_scheme(scheme);
        self.invalidate();
        true
    }

    /// Replace the host system palette. Palette values are consumed while
    /// styles compute, so this has the same full retained invalidation as a
    /// changed media preference.
    pub fn set_system_palette(&mut self, palette: SystemPalette) -> bool {
        if self.device.system_palette == palette {
            return false;
        }
        self.device.set_system_palette(palette);
        self.invalidate();
        true
    }

    pub fn last_restyle_stats(&self) -> RestyleStats {
        self.style_session.last_stats()
    }

    /// The table dispatch record from the most recent completed layout.
    ///
    /// A retained document has no layout before its first frame, and an
    /// invalidation deliberately clears the prior record with the rest of its
    /// derived state. Hosts can therefore observe a ledger only for the exact
    /// frame they rendered.
    pub fn table_shadow_ledger(&self) -> Option<&crate::table_shadow::TableShadowLedger> {
        self.layout
            .as_ref()
            .map(|layout| layout.fragments.table_shadow_ledger())
    }

    /// Supply host-resolved image bytes for a non-data URL. The CSS engine
    /// still owns decoding and paint-key allocation; the host owns URL
    /// resolution and fetching.
    pub fn set_image_resource(&mut self, url: impl Into<String>, bytes: Vec<u8>) {
        let url = url.into();
        if self.image_sources.get(&url) == Some(&bytes) {
            return;
        }
        self.image_sources.insert(url, bytes);
        self.invalidate_with_layout_damage(LayoutDamageKind::Resource);
    }

    /// Supply host-resolved font bytes for a non-data URL. The host owns URL
    /// resolution and fetching. A source identity replaces prior bytes rather
    /// than registering another competing face in Parley's collection.
    pub fn set_font_resource(&mut self, url: impl Into<String>, bytes: Vec<u8>) {
        let url = url.into();
        if self.font_sources.get(&url) == Some(&bytes) {
            return;
        }
        self.font_sources.insert(url, bytes);
        self.rebuild_font_resources();
        self.invalidate_with_layout_damage(LayoutDamageKind::Resource);
    }

    /// Replace the complete host image ledger. A missing prior key is removed,
    /// so a failed or deleted live image cannot remain painted from stale bytes.
    pub fn replace_image_resources(
        &mut self,
        resources: impl IntoIterator<Item = (String, Vec<u8>)>,
    ) {
        let next = resources.into_iter().collect::<HashMap<_, _>>();
        if self.image_sources == next {
            return;
        }
        self.image_sources = next;
        self.invalidate_with_layout_damage(LayoutDamageKind::Resource);
    }

    /// Replace the complete host font ledger. Fontique has no per-blob removal
    /// operation, so a changed or removed source rebuilds this document's font
    /// context from the surviving ledger before the next layout.
    pub fn replace_font_resources(
        &mut self,
        resources: impl IntoIterator<Item = (String, Vec<u8>)>,
    ) {
        let next = resources.into_iter().collect::<HashMap<_, _>>();
        if self.font_sources == next {
            return;
        }
        self.font_sources = next;
        self.rebuild_font_resources();
        self.invalidate_with_layout_damage(LayoutDamageKind::Resource);
    }

    fn rebuild_font_resources(&mut self) {
        self.text = TextSystem::new();
        for bytes in self.font_sources.values().cloned() {
            self.text.register_font_bytes(bytes);
        }
    }

    pub fn frame(&mut self, width: u32, height: u32) -> Result<LiveryPaintList, LayoutError> {
        if let Some((viewport, list)) = &self.cached
            && *viewport == (width, height)
        {
            return Ok(list.clone().translated(-self.scroll.0, -self.scroll.1));
        }

        if !self.layout_dirty
            && self
                .layout
                .as_ref()
                .is_some_and(|layout| layout.viewport == (width, height))
        {
            self.generation = self.generation.saturating_add(1);
            return self.paint_active_layout(width, height);
        }

        let viewport_changed = self.viewport != (width, height);
        if viewport_changed {
            self.record_full_layout_damage(LayoutDamageKind::Viewport);
        }
        self.viewport = (width, height);
        self.device.set_viewport_size(width as f32, height as f32);
        self.finish_completed_transitions();
        self.style_session.update(
            &self.dom,
            &self.style_set,
            &self.device,
            &self.interactions,
            &[],
        );
        let mut styles = resolve_container_query_styles_with_images(
            &self.dom,
            self.style_session.styles(),
            &self.style_set,
            &self.device,
            &self.interactions,
            &self.image_sources,
        )?;
        if !viewport_changed
            && self.layout_dirty
            && self.transitions.is_empty()
            && self.keyframe_animation.is_none()
            && let Some(node) = self.layout.as_ref().and_then(|layout| {
                styles.only_positioned_insets_changed(&layout.styles)
            })
            && self.layout.as_mut().is_some_and(|layout| {
                layout.fragments.reposition_stable_positioned_subtree(
                    &self.dom,
                    &styles,
                    &self.image_sources,
                    node,
                    width as f32,
                    height as f32,
                )
            })
        {
            let (content_width, content_height) = self
                .layout
                .as_ref()
                .map(|layout| self.document_content_extent(&styles, &layout.fragments))
                .expect("a repositioned retained layout is still live");
            let layout = self
                .layout
                .as_mut()
                .expect("a repositioned retained layout is still live");
            layout.styles = styles;
            layout.content_width = content_width;
            layout.content_height = content_height;
            self.identity_source = None;
            self.layout_dirty = false;
            self.layout_generation = self.layout_generation.saturating_add(1);
            self.clamp_scroll();
            self.clamp_nested_scroll();
            self.generation = self.generation.saturating_add(1);
            return self.paint_active_layout(width, height);
        }
        if !viewport_changed
            && self.layout_dirty
            && self.transitions.is_empty()
            && self.keyframe_animation.is_none()
            && let Some(node) = self.layout.as_ref().and_then(|layout| {
                styles.only_positioned_leaf_geometry_changed(&layout.styles)
            })
            && self.layout.as_mut().is_some_and(|layout| {
                layout.fragments.resize_positioned_leaf(
                    &self.dom,
                    &styles,
                    &self.image_sources,
                    node,
                    width as f32,
                    height as f32,
                )
            })
        {
            let (content_width, content_height) = self
                .layout
                .as_ref()
                .map(|layout| self.document_content_extent(&styles, &layout.fragments))
                .expect("a resized retained layout is still live");
            let layout = self
                .layout
                .as_mut()
                .expect("a resized retained layout is still live");
            layout.styles = styles;
            layout.content_width = content_width;
            layout.content_height = content_height;
            self.identity_source = None;
            self.layout_dirty = false;
            self.layout_generation = self.layout_generation.saturating_add(1);
            self.clamp_scroll();
            self.clamp_nested_scroll();
            self.generation = self.generation.saturating_add(1);
            return self.paint_active_layout(width, height);
        }
        if !viewport_changed
            && self.layout_dirty
            && self.transitions.is_empty()
            && self.keyframe_animation.is_none()
            && self.layout.as_ref().is_some_and(|layout| {
                styles.differs_only_in_background_color(&layout.styles)
            })
        {
            self.layout
                .as_mut()
                .expect("a checked retained layout is still live")
                .styles = styles;
            self.identity_source = None;
            self.layout_dirty = false;
            self.generation = self.generation.saturating_add(1);
            return self.paint_active_layout(width, height);
        }

        let local_root = self.last_layout_damage.as_ref().and_then(|damage| {
            (!viewport_changed
                && damage.kind == LayoutDamageKind::Dom
                && !damage.full_document
                && damage.roots.len() == 1)
                .then(|| damage.roots[0])
        });
        if self.transitions.is_empty()
            && self.keyframe_animation.is_none()
            && !self.style_set.has_container_queries()
            && let (Some(root), Some(previous)) = (local_root, self.layout.as_ref())
        {
            let previous_styles = previous.styles.clone();
            let previous_fragments = previous.fragments.clone();
            let dom_text_order = text_sources_in_dom_order(&self.dom);
            let mut candidate = retained_table_owner(previous_fragments.boxes(), root).unwrap_or(root);
            loop {
                let mut replaced_nodes = nodes_in_subtree(&self.dom, candidate);
                replaced_nodes.extend(previous_fragments.generated_subtree_nodes(candidate));
                match layout_retained_formatting_root(
                    &self.dom,
                    &styles,
                    &previous_styles,
                    &previous_fragments,
                    candidate,
                    &mut self.text,
                    &self.image_sources,
                )? {
                    RetainedRootFormatting::Formatted(mut local) => {
                        local.reconcile_identifiers(&previous_fragments);
                        let mut fragments = previous_fragments.clone();
                        if fragments.replace_reconciled_local_formatting_subtree_from(
                            &local,
                            candidate,
                            &replaced_nodes,
                            &dom_text_order,
                        ) {
                            let (content_width, content_height) =
                                self.document_content_extent(&styles, &fragments);
                            let layout = self
                                .layout
                                .as_mut()
                                .expect("the retained root formatter had a source layout");
                            layout.styles = styles;
                            layout.fragments = fragments;
                            layout.content_width = content_width;
                            layout.content_height = content_height;
                            self.identity_source = None;
                            self.layout_dirty = false;
                            self.layout_generation = self.layout_generation.saturating_add(1);
                            #[cfg(test)]
                            {
                                self.retained_root_relayout_generation = self
                                    .retained_root_relayout_generation
                                    .saturating_add(1);
                            }
                            self.clamp_scroll();
                            self.clamp_nested_scroll();
                            self.generation = self.generation.saturating_add(1);
                            return self.paint_active_layout(width, height);
                        }
                        break;
                    }
                    RetainedRootFormatting::PromoteParent => {
                        let Some(parent) = self.dom.parent(candidate) else {
                            break;
                        };
                        candidate = parent;
                    }
                    RetainedRootFormatting::Unsupported => break,
                }
            }
        }

        self.retain_layout_identity();
        self.schedule_transitions(&styles);
        self.schedule_keyframe_animation(&styles);
        self.apply_transitions(&mut styles);
        self.apply_keyframe_animation(&mut styles);
        let (styles, mut fragments) = layout_with_text_system(
            &self.dom,
            &styles,
            width as f32,
            height as f32,
            self.device.viewport_sizes,
            &mut self.text,
            &self.image_sources,
        )?;
        let previous = self.identity_source.take();
        if let Some(previous) = previous.as_ref() {
            fragments.reconcile_identifiers(&previous.fragments);
        }
        let replacement_roots = self.last_layout_damage.as_ref().and_then(|damage| {
            (!damage.full_document && damage.kind == LayoutDamageKind::Dom && !damage.roots.is_empty())
                .then(|| damage.roots.clone())
        });
        if let (Some(mut previous), Some(roots)) = (previous, replacement_roots)
            && previous
                .fragments
                .replace_reconciled_formatting_subtrees_from(&fragments, &roots)
        {
            fragments = previous.fragments;
        }
        let (content_width, content_height) = self.document_content_extent(&styles, &fragments);
        self.layout = Some(LayoutState {
            viewport: (width, height),
            styles: styles.clone(),
            fragments: fragments.clone(),
            content_width,
            content_height,
        });
        self.layout_dirty = false;
        self.layout_generation = self.layout_generation.saturating_add(1);
        self.clamp_scroll();
        self.clamp_nested_scroll();
        self.generation = self.generation.saturating_add(1);
        self.paint_active_layout(width, height)
    }

    fn paint_active_layout(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<LiveryPaintList, LayoutError> {
        let (styles, mut fragments) = self
            .layout
            .as_ref()
            .map(|layout| (layout.styles.clone(), layout.fragments.clone()))
            .ok_or_else(|| LayoutError::retained_state("no retained layout to paint"))?;
        self.apply_sticky_positioning(&mut fragments, &styles);
        let list = emit_paint_list_with_text_system_scrolled_with_images(
            &self.dom,
            &styles,
            &fragments,
            DeviceIntSize::new(width as i32, height as i32),
            self.generation,
            &mut self.text,
            &self.nested_scroll,
            &self.image_sources,
        );
        self.cached = Some(((width, height), list.clone()));
        Ok(list.translated(-self.scroll.0, -self.scroll.1))
    }

    fn sticky_layout(&self, layout: &LayoutState<D::NodeId>) -> LiveryLayout<D::NodeId> {
        let mut active = layout.fragments.clone();
        self.apply_sticky_positioning(&mut active, &layout.styles);
        active
    }

    fn apply_sticky_positioning(
        &self,
        fragments: &mut LiveryLayout<D::NodeId>,
        styles: &StylePlane<D::NodeId>,
    ) {
        let scrollport_geometry = fragments.clone();
        fragments.apply_sticky_positioning(
            styles,
            self.viewport.0 as f32,
            self.viewport.1 as f32,
            |node| self.sticky_scrollport(node, styles, &scrollport_geometry),
        );
    }

    fn sticky_scrollport(
        &self,
        node: D::NodeId,
        styles: &StylePlane<D::NodeId>,
        fragments: &LiveryLayout<D::NodeId>,
    ) -> Option<StickyScrollport> {
        let mut ancestor = self.dom.parent(node);
        while let Some(candidate) = ancestor {
            if styles
                .get(candidate)
                .is_some_and(|style| self.is_scroll_container(style))
                && let Some(fragment) = fragments.get(candidate)
            {
                return Some(StickyScrollport {
                    rect: fragment.physical_rect(),
                    offset: self
                        .nested_scroll
                        .get(&candidate)
                        .copied()
                        .map_or(buckram::PhysicalOffset::default(), |(x, y)| {
                            buckram::PhysicalOffset { x, y }
                        }),
                });
            }
            ancestor = self.dom.parent(candidate);
        }
        Some(StickyScrollport {
            rect: buckram::PhysicalRect {
                x: 0.0,
                y: 0.0,
                width: self.viewport.0 as f32,
                height: self.viewport.1 as f32,
            },
            offset: buckram::PhysicalOffset {
                x: self.scroll.0,
                y: self.scroll.1,
            },
        })
    }

    fn has_sticky_positioning(&self) -> bool {
        self.layout.as_ref().is_some_and(|layout| {
            layout
                .fragments
                .boxes()
                .iter()
                .any(|(_, css_box)| css_box.positioning == buckram::PositioningScheme::Sticky)
        })
    }

    /// Return the current viewport scroll offset.
    pub fn scroll(&self) -> (f32, f32) {
        self.scroll
    }

    /// CSSOM `insertRule` on one retained author sheet (harvest H3). The
    /// next frame restyles, relays out, and repaints.
    pub fn insert_author_rule(
        &mut self,
        sheet: usize,
        rule: &str,
        index: usize,
    ) -> Result<usize, livery::stylesheet::RuleMutationError> {
        let inserted = self.style_set.insert_author_rule(sheet, rule, index)?;
        self.record_full_layout_damage(LayoutDamageKind::Stylesheet);
        self.retain_layout_identity();
        Ok(inserted)
    }

    /// CSSOM `deleteRule` on one retained author sheet (harvest H3).
    pub fn delete_author_rule(
        &mut self,
        sheet: usize,
        index: usize,
    ) -> Result<(), livery::stylesheet::RuleMutationError> {
        self.style_set.delete_author_rule(sheet, index)?;
        self.record_full_layout_damage(LayoutDamageKind::Stylesheet);
        self.retain_layout_identity();
        Ok(())
    }

    /// The retained style set, for CSSOM reads (rule counts, object model).
    pub fn style_set(&self) -> &StyleSet {
        &self.style_set
    }

    /// getComputedStyle backing (harvest H3): serialize one longhand or
    /// custom property of one element from the retained style plane. With
    /// no retained layout yet, styles resolve on demand at the current
    /// device. Unknown names and unstyled nodes return None.
    pub fn computed_style(&self, node: D::NodeId, property: &str) -> Option<String> {
        let resolved;
        let container_resolved;
        let plane = match self.layout.as_ref() {
            Some(layout) => &layout.styles,
            None => {
                resolved =
                    resolve_styles(&self.dom, &self.style_set, &self.device, &self.interactions);
                container_resolved = resolve_container_query_styles(
                    &self.dom,
                    &resolved,
                    &self.style_set,
                    &self.device,
                    &self.interactions,
                )
                .ok();
                container_resolved.as_ref().unwrap_or(&resolved)
            },
        };
        plane.computed_style(node, property)
    }

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
    /// interpolated value without re-running layout.
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

    /// Scroll the document viewport by device pixels. Wheel deltas that need
    /// position-aware nested routing go through [`Self::scroll_at`].
    pub fn scroll_by(&mut self, dx: f32, dy: f32) -> bool {
        let before = self.scroll;
        self.scroll.0 += dx;
        self.scroll.1 += dy;
        self.clamp_scroll();
        let changed = before != self.scroll;
        if changed && self.has_sticky_positioning() {
            self.cached = None;
        }
        changed
    }

    pub fn scroll_at(&mut self, x: f32, y: f32, dx: f32, dy: f32) -> bool {
        let Some(layout) = self.layout.as_ref() else {
            return false;
        };
        let active = self.sticky_layout(layout);
        let mut node = hit_test_with_scroll(
            &self.dom,
            &layout.styles,
            &active,
            &self.nested_scroll,
            x + self.scroll.0,
            y + self.scroll.1,
        );
        while let Some(candidate) = node {
            if let Some(next) = self.scroll_step(layout, candidate, dx, dy) {
                self.nested_scroll.insert(candidate, next);
                self.cached = None;
                return true;
            }
            node = self.dom.parent(candidate);
        }
        self.scroll_by(dx, dy)
    }

    pub fn scroll_to(&mut self, y: f32) {
        let before = self.scroll;
        self.scroll.1 = y;
        self.clamp_scroll();
        if self.scroll != before && self.has_sticky_positioning() {
            self.cached = None;
        }
    }

    pub fn scroll_line(&mut self, direction: i8) -> bool {
        self.scroll_by(0.0, 40.0 * f32::from(direction))
    }

    pub fn scroll_page(&mut self, direction: i8) -> bool {
        let amount = self.viewport.1 as f32 * 0.9;
        self.scroll_by(0.0, amount * f32::from(direction))
    }

    pub fn content_height(&self, fallback: u32) -> u32 {
        self.layout
            .as_ref()
            .map_or(fallback, |layout| layout.content_height.ceil() as u32)
    }

    /// Retained per-element scroll offsets for hosts that draw their own
    /// scrollbar or accessibility overlay.
    pub fn element_scroll(&self) -> &HashMap<D::NodeId, (f32, f32)> {
        &self.nested_scroll
    }

    pub fn hit_test(&self, x: f32, y: f32) -> Option<D::NodeId> {
        let layout = self.layout.as_ref()?;
        let active = self.sticky_layout(layout);
        hit_test_with_scroll(
            &self.dom,
            &layout.styles,
            &active,
            &self.nested_scroll,
            x + self.scroll.0,
            y + self.scroll.1,
        )
    }

    pub fn links(&self) -> Vec<LinkTarget> {
        let Some(layout) = self.layout.as_ref() else {
            return Vec::new();
        };
        let mut links = Vec::new();
        self.collect_links(self.dom.document(), layout, &mut links);
        links
    }

    /// Begin a primary-pointer text selection against the retained shaped
    /// clusters from the last frame.
    pub fn begin_text_selection(&mut self, x: f32, y: f32) -> bool {
        self.selection_range = None;
        self.selection_anchor = {
            let Some(frame) = self
                .layout
                .as_ref()
                .and_then(|layout| layout.fragments.text_frame())
            else {
                return false;
            };
            frame.text_position_at_point(x, y, |source, fragment| {
                self.viewport_text_rect(source, fragment)
            })
        };
        self.selection_anchor.is_some()
    }

    /// Extend the current primary-pointer selection.
    pub fn extend_text_selection(&mut self, x: f32, y: f32) -> bool {
        let Some(anchor) = self.selection_anchor else {
            return false;
        };
        let focus = {
            let Some(frame) = self
                .layout
                .as_ref()
                .and_then(|layout| layout.fragments.text_frame())
            else {
                return false;
            };
            frame.text_position_at_point(x, y, |source, fragment| {
                self.viewport_text_rect(source, fragment)
            })
        };
        let Some(focus) = focus else {
            return false;
        };
        let next = TextRange {
            anchor_node: anchor.0,
            anchor_offset: anchor.1,
            focus_node: focus.0,
            focus_offset: focus.1,
        };
        if self.selection_range == Some(next) {
            return false;
        }
        self.selection_range = Some(next);
        true
    }

    /// Finish the current selection. A collapsed gesture clears the range and
    /// lets the session perform the ordinary click action.
    pub fn finish_text_selection(&mut self, x: f32, y: f32) -> bool {
        self.extend_text_selection(x, y);
        self.selection_anchor = None;
        if self.text_selection().is_some() {
            true
        } else {
            self.selection_range = None;
            false
        }
    }

    /// Recompute the selected text and viewport geometry from the retained
    /// source range.
    pub fn text_selection(&self) -> Option<TextSelection<D::NodeId>> {
        let frame = self
            .layout
            .as_ref()
            .and_then(|layout| layout.fragments.text_frame())?;
        frame.text_selection(self.selection_range?, |source, fragment| {
            self.viewport_text_rect(source, fragment)
        })
    }

    /// Link URLs whose descendant text contributes to this selection.
    pub fn links_for_selection(&self, selection: &TextSelection<D::NodeId>) -> Vec<String> {
        let mut links = Vec::new();
        for source in &selection.source_nodes {
            if let Some(href) = self.link_ancestor(*source)
                && !links.contains(&href)
            {
                links.push(href);
            }
        }
        links
    }

    /// Resolve the first retained occurrence of `text` to viewport pointer
    /// endpoints for Genet Probe and find-to-select consumers.
    pub fn text_target(&self, text: &str) -> Option<([f32; 2], [f32; 2])> {
        let frame = self
            .layout
            .as_ref()
            .and_then(|layout| layout.fragments.text_frame())?;
        let range = frame.find_text_range(text)?;
        let anchor = frame.caret_rect(
            range.anchor_node,
            range.anchor_offset,
            |source, fragment| self.viewport_text_rect(source, fragment),
        )?;
        let focus =
            frame.caret_rect(range.focus_node, range.focus_offset, |source, fragment| {
                self.viewport_text_rect(source, fragment)
            })?;
        Some((
            [anchor.x, anchor.y + anchor.height * 0.5],
            [focus.x, focus.y + focus.height * 0.5],
        ))
    }

    pub fn click_at(&mut self, x: f32, y: f32) -> ClickOutcome {
        let Some(target) = self.hit_test(x, y) else {
            return ClickOutcome::None;
        };
        let focus_target = self.focusable_ancestor(target);
        let focused = focus_target.is_some_and(|id| self.focus(id));
        let href = self.link_ancestor(target);
        if let Some(href) = href {
            if let Some(fragment) = href
                .strip_prefix('#')
                .filter(|fragment| !fragment.is_empty())
                && self.scroll_to_fragment(fragment)
            {
                return ClickOutcome::Scrolled;
            }
            return ClickOutcome::Navigate(href);
        }
        if focused {
            ClickOutcome::Focused
        } else {
            ClickOutcome::None
        }
    }

    fn viewport_text_rect(&self, source: D::NodeId, fragment: crate::layout::Fragment) -> TextRect {
        let (nested_x, nested_y) = self.ancestor_scroll(source);
        TextRect {
            x: fragment.x - self.scroll.0 - nested_x,
            y: fragment.y - self.scroll.1 - nested_y,
            width: fragment.width,
            height: fragment.height,
        }
    }

    fn clamp_scroll(&mut self) {
        let Some(layout) = self.layout.as_ref() else {
            self.scroll = (0.0, 0.0);
            return;
        };
        let (scroll_x, scroll_y) = self.scrollable_axes(layout);
        let max_x = if scroll_x {
            (layout.content_width - layout.viewport.0 as f32).max(0.0)
        } else {
            0.0
        };
        let max_y = if scroll_y {
            (layout.content_height - layout.viewport.1 as f32).max(0.0)
        } else {
            0.0
        };
        self.scroll.0 = self.scroll.0.clamp(0.0, max_x);
        self.scroll.1 = self.scroll.1.clamp(0.0, max_y);
    }

    fn document_content_extent(
        &self,
        styles: &StylePlane<D::NodeId>,
        fragments: &LiveryLayout<D::NodeId>,
    ) -> (f32, f32) {
        let mut extent = (0.0, 0.0);
        for child in self.dom.dom_children(self.dom.document()) {
            self.extend_content_extent(child, styles, fragments, &mut extent, false);
        }
        extent
    }

    fn extend_content_extent(
        &self,
        id: D::NodeId,
        styles: &StylePlane<D::NodeId>,
        fragments: &LiveryLayout<D::NodeId>,
        extent: &mut (f32, f32),
        nested: bool,
    ) {
        let Some(style) = styles.get(id) else {
            return;
        };
        if style.display == livery::values::Display::None {
            return;
        }
        if let Some(fragment) = fragments.get(id) {
            extent.0 = extent.0.max(fragment.x + fragment.width);
            extent.1 = extent.1.max(fragment.y + fragment.height);
        }
        if nested && self.clips_content(style) {
            return;
        }
        for child in self.dom.dom_children(id) {
            self.extend_content_extent(child, styles, fragments, extent, true);
        }
    }

    fn clamp_nested_scroll(&mut self) {
        let Some(layout) = self.layout.as_ref() else {
            self.nested_scroll.clear();
            return;
        };
        let keys = self.nested_scroll.keys().copied().collect::<Vec<_>>();
        for node in keys {
            let Some(style) = layout.styles.get(node) else {
                self.nested_scroll.remove(&node);
                continue;
            };
            if !self.is_scroll_container(style) {
                self.nested_scroll.remove(&node);
                continue;
            }
            let (max_x, max_y) = self.scroll_extent(layout, node);
            if let Some(offset) = self.nested_scroll.get_mut(&node) {
                offset.0 = offset.0.clamp(0.0, max_x);
                offset.1 = offset.1.clamp(0.0, max_y);
            }
        }
    }

    fn scroll_step(
        &self,
        layout: &LayoutState<D::NodeId>,
        node: D::NodeId,
        dx: f32,
        dy: f32,
    ) -> Option<(f32, f32)> {
        let style = layout.styles.get(node)?;
        if !self.is_scroll_container(style) {
            return None;
        }
        let (max_x, max_y) = self.scroll_extent(layout, node);
        let current = self.nested_scroll.get(&node).copied().unwrap_or((0.0, 0.0));
        let next = (
            if self.scrolls_x(style) {
                (current.0 + dx).clamp(0.0, max_x)
            } else {
                current.0
            },
            if self.scrolls_y(style) {
                (current.1 + dy).clamp(0.0, max_y)
            } else {
                current.1
            },
        );
        if next == current { None } else { Some(next) }
    }

    fn scroll_extent(&self, layout: &LayoutState<D::NodeId>, node: D::NodeId) -> (f32, f32) {
        let Some(container) = layout.fragments.get(node) else {
            return (0.0, 0.0);
        };
        let mut extent = (0.0, 0.0);
        for child in self.dom.dom_children(node) {
            self.extend_nested_extent(child, node, layout, &mut extent);
        }
        (
            (extent.0 - container.width).max(0.0),
            (extent.1 - container.height).max(0.0),
        )
    }

    fn extend_nested_extent(
        &self,
        id: D::NodeId,
        container: D::NodeId,
        layout: &LayoutState<D::NodeId>,
        extent: &mut (f32, f32),
    ) {
        let Some(style) = layout.styles.get(id) else {
            return;
        };
        if style.display == livery::values::Display::None {
            return;
        }
        if let (Some(container), Some(fragment)) =
            (layout.fragments.get(container), layout.fragments.get(id))
        {
            extent.0 = extent.0.max(fragment.x + fragment.width - container.x);
            extent.1 = extent.1.max(fragment.y + fragment.height - container.y);
        }
        if self.clips_content(style) {
            return;
        }
        for child in self.dom.dom_children(id) {
            self.extend_nested_extent(child, container, layout, extent);
        }
    }

    fn is_scroll_container(&self, style: &livery::ComputedValues) -> bool {
        self.scrolls_x(style) || self.scrolls_y(style)
    }

    fn clips_content(&self, style: &livery::ComputedValues) -> bool {
        style.overflow_x != Overflow::Visible || style.overflow_y != Overflow::Visible
    }

    fn scrolls_x(&self, style: &livery::ComputedValues) -> bool {
        matches!(style.overflow_x, Overflow::Auto | Overflow::Scroll)
    }

    fn scrolls_y(&self, style: &livery::ComputedValues) -> bool {
        matches!(style.overflow_y, Overflow::Auto | Overflow::Scroll)
    }

    fn apply_transitions(&self, styles: &mut StylePlane<D::NodeId>) {
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

    fn apply_keyframe_animation(&self, styles: &mut StylePlane<D::NodeId>) {
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

    fn schedule_keyframe_animation(&mut self, styles: &StylePlane<D::NodeId>) {
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

    fn find_keyframe_animation(
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

    fn finish_completed_transitions(&mut self) {
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

    fn schedule_transitions(&mut self, styles: &StylePlane<D::NodeId>) {
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

    fn find_property_transition(
        &self,
        id: D::NodeId,
        previous: &StylePlane<D::NodeId>,
        styles: &StylePlane<D::NodeId>,
        property: PropertyId,
    ) -> Option<PropertyTransition<D::NodeId>> {
        if let (Some(old), Some(new)) = (previous.get(id), styles.get(id)) {
            let duration_ms = f64::from(new.transition_duration.milliseconds());
            if duration_ms > 0.0 && new.transition_property.includes_property(property) {
                let from = old.get(property);
                let to = new.get(property);
                if from != to {
                    return Some(PropertyTransition {
                        node: id,
                        property,
                        from: previous.resolve_used_color_value(id, from),
                        to: styles.resolve_used_color_value(id, to),
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

    fn scrollable_axes(&self, layout: &LayoutState<D::NodeId>) -> (bool, bool) {
        let root = self
            .dom
            .dom_children(self.dom.document())
            .find(|id| self.dom.kind(*id) == NodeKind::Element);
        let Some(root) = root else {
            return (true, true);
        };
        let Some(style) = layout.styles.get(root) else {
            return (true, true);
        };
        (
            !matches!(style.overflow_x, Overflow::Hidden | Overflow::Clip),
            !matches!(style.overflow_y, Overflow::Hidden | Overflow::Clip),
        )
    }

    fn focus(&mut self, id: D::NodeId) -> bool {
        for old in self.focused_chain.drain(..) {
            self.interactions.set(old, StatePseudoClass::Focus, false);
            self.interactions
                .set(old, StatePseudoClass::FocusWithin, false);
        }
        self.interactions.set(id, StatePseudoClass::Focus, true);
        let mut chain = vec![id];
        let mut parent = self.dom.parent(id);
        while let Some(ancestor) = parent {
            if self.dom.kind(ancestor) == NodeKind::Element {
                self.interactions
                    .set(ancestor, StatePseudoClass::FocusWithin, true);
                chain.push(ancestor);
            }
            parent = self.dom.parent(ancestor);
        }
        self.focused_chain = chain;
        self.record_full_layout_damage(LayoutDamageKind::Interaction);
        self.retain_layout_identity();
        true
    }

    fn focusable_ancestor(&self, mut id: D::NodeId) -> Option<D::NodeId> {
        loop {
            if self.is_focusable(id) {
                return Some(id);
            }
            id = self.dom.parent(id)?;
        }
    }

    fn is_focusable(&self, id: D::NodeId) -> bool {
        if self.dom.kind(id) != NodeKind::Element {
            return false;
        }
        let Some(name) = self.dom.element_name(id) else {
            return false;
        };
        let local = name.local.as_ref();
        local.eq_ignore_ascii_case("a") && self.attribute(id, "href").is_some()
            || matches!(
                local.to_ascii_lowercase().as_str(),
                "button" | "input" | "select" | "textarea"
            )
            || self.attribute(id, "tabindex").is_some()
    }

    fn link_ancestor(&self, mut id: D::NodeId) -> Option<String> {
        loop {
            if self.dom.kind(id) == NodeKind::Element
                && self
                    .dom
                    .element_name(id)
                    .is_some_and(|name| name.local.as_ref().eq_ignore_ascii_case("a"))
                && let Some(href) = self.attribute(id, "href")
            {
                return Some(href.to_owned());
            }
            id = self.dom.parent(id)?;
        }
    }

    fn scroll_to_fragment(&mut self, fragment: &str) -> bool {
        let Some(target) = find_id(&self.dom, self.dom.document(), fragment) else {
            return false;
        };
        let Some(y) = self
            .layout
            .as_ref()
            .and_then(|layout| layout.fragments.get(target).map(|fragment| fragment.y))
        else {
            return false;
        };
        self.scroll_to(y);
        true
    }

    fn collect_links(
        &self,
        id: D::NodeId,
        layout: &LayoutState<D::NodeId>,
        links: &mut Vec<LinkTarget>,
    ) {
        if self.dom.kind(id) == NodeKind::Element
            && let Some(href) = self.attribute(id, "href")
            && let Some(fragment) = layout.fragments.get(id)
            && let Some(style) = layout.styles.get(id)
            && style.display != livery::values::Display::None
            && style.visibility == livery::values::Visibility::Visible
            && style.pointer_events == livery::values::PointerEvents::Auto
        {
            let (nested_x, nested_y) = self.ancestor_scroll(id);
            links.push(LinkTarget {
                url: href.to_owned(),
                rect: [
                    fragment.x - self.scroll.0 - nested_x,
                    fragment.y - self.scroll.1 - nested_y,
                    fragment.width,
                    fragment.height,
                ],
            });
        }
        for child in self.dom.dom_children(id) {
            self.collect_links(child, layout, links);
        }
    }

    fn ancestor_scroll(&self, id: D::NodeId) -> (f32, f32) {
        let mut offset = (0.0, 0.0);
        let mut parent = self.dom.parent(id);
        while let Some(ancestor) = parent {
            if let Some(scroll) = self.nested_scroll.get(&ancestor) {
                offset.0 += scroll.0;
                offset.1 += scroll.1;
            }
            parent = self.dom.parent(ancestor);
        }
        offset
    }

    fn attribute(&self, id: D::NodeId, local: &str) -> Option<&str> {
        self.dom
            .attribute(id, &Namespace::from(""), &LocalName::from(local))
    }

    pub fn into_dom(self) -> D {
        self.dom
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

fn find_id<D: LayoutDom>(dom: &D, id: D::NodeId, target: &str) -> Option<D::NodeId> {
    if dom.kind(id) == NodeKind::Element
        && dom
            .attribute(id, &Namespace::from(""), &LocalName::from("id"))
            .is_some_and(|value| value == target)
    {
        return Some(id);
    }
    dom.dom_children(id)
        .find_map(|child| find_id(dom, child, target))
}

#[cfg(test)]
mod tests {
    use super::*;
    use genet_scripted_dom::{NodeId, ScriptedDom};
    use layout_dom_api::QualName;
    use paint_list_api::PaintList;

    fn attr(name: &str) -> QualName {
        QualName::new(None, Namespace::from(""), LocalName::from(name))
    }

    fn by_id(dom: &ScriptedDom, id: &str) -> NodeId {
        find_id(dom, dom.document(), id).expect("fixture node")
    }

    fn generated_ids(
        document: &LiveryDocument<ScriptedDom>,
        node: NodeId,
    ) -> Vec<(buckram::BoxId, Vec<buckram::FragmentId>)> {
        let layout = document.layout.as_ref().expect("completed frame");
        layout
            .fragments
            .boxes()
            .boxes_for_node(node)
            .iter()
            .copied()
            .map(|box_id| {
                (
                    box_id,
                    layout
                        .fragments
                        .fragments()
                        .fragment_ids_for_box(box_id)
                        .to_vec(),
                )
            })
            .collect()
    }

    fn assert_table_paint_sources_are_live(document: &LiveryDocument<ScriptedDom>, node: NodeId) {
        let layout = document.layout.as_ref().expect("completed frame");
        let paint = layout
            .fragments
            .table_paint_for_node(node)
            .expect("retained table paint model");
        for source in paint.fragments().iter().filter_map(|fragment| fragment.box_id) {
            assert!(
                !layout
                    .fragments
                    .fragments()
                    .fragment_ids_for_box(source)
                    .is_empty(),
                "each retained table paint source names a live reconciled box",
            );
        }
    }

    fn table_wrapper_fragment_id(
        document: &LiveryDocument<ScriptedDom>,
        node: NodeId,
    ) -> buckram::FragmentId {
        let layout = document.layout.as_ref().expect("completed frame");
        let grid = layout
            .fragments
            .boxes()
            .principal_box(node)
            .expect("table grid box");
        let wrapper = layout.fragments.boxes()[grid]
            .parent()
            .expect("table wrapper box");
        assert_eq!(
            layout.fragments.boxes()[wrapper].display.internal_table,
            Some(buckram::InternalTableRole::Wrapper),
        );
        match layout.fragments.fragments().fragment_ids_for_box(wrapper) {
            [fragment] => *fragment,
            fragments => panic!("one table wrapper fragment, got {fragments:?}"),
        }
    }

    #[test]
    fn retained_relayout_keeps_unrelated_and_table_generated_ids_after_sibling_insertion() {
        let mut dom = ScriptedDom::from_serialized_document(
            "<html><body id=body><div id=changed>changed</div><table id=table><tbody><tr><td>cell</td></tr></tbody></table><div id=outside>outside</div></body></html>",
        );
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut document = LiveryDocument::new(
            dom,
            StyleSet::cambium(&[
                "body { margin: 0; } table { display: table; border-spacing: 0; } \
                 tbody { display: table-row-group; } tr { display: table-row; } \
                 td { display: table-cell; width: 40px; height: 20px; }",
            ]),
            Device::screen(240.0, 160.0),
        );
        document.frame(240, 160).expect("initial frame");
        let table = by_id(document.dom(), "table");
        let outside = by_id(document.dom(), "outside");
        let table_before = generated_ids(&document, table);
        let outside_before = generated_ids(&document, outside);
        assert_table_paint_sources_are_live(&document, table);
        assert!(
            table_before.len() >= 2,
            "the table receipt includes its retained wrapper and grid boxes",
        );

        document.mutate_dom(|dom| {
            let body = by_id(dom, "body");
            let changed = by_id(dom, "changed");
            let inserted = dom.create_element(QualName::new(
                None,
                Namespace::from(""),
                LocalName::from("div"),
            ));
            dom.set_attribute(inserted, attr("id"), "inserted");
            let text = dom.create_text("inserted");
            dom.append_child(inserted, text);
            dom.insert_before(body, inserted, Some(changed));
        });
        document.frame(240, 160).expect("inserted-sibling frame");

        assert_eq!(generated_ids(&document, table), table_before);
        assert_eq!(generated_ids(&document, outside), outside_before);
        assert_table_paint_sources_are_live(&document, table);
        assert!(
            !generated_ids(&document, by_id(document.dom(), "inserted")).is_empty(),
            "the inserted sibling receives separate live identities",
        );
    }

    #[test]
    fn retained_mutation_paints_like_a_fresh_final_document() {
        let initial = "<html><body id=body><div id=before>before</div><div id=after>after</div></body></html>";
        let final_document = "<html><body id=body><div id=before>before</div><div id=inserted>inserted</div><div id=after>after</div></body></html>";
        let styles = || {
            StyleSet::cambium(&[
                "html, body { margin: 0; padding: 0; } div { width: 100px; height: 20px; } \
                 #inserted { background: blue; }",
            ])
        };
        let mut dom = ScriptedDom::from_serialized_document(initial);
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut retained = LiveryDocument::new(dom, styles(), Device::screen(160.0, 120.0));
        retained.frame(160, 120).expect("initial retained frame");

        retained.mutate_dom(|dom| {
            let body = by_id(dom, "body");
            let after = by_id(dom, "after");
            let inserted = dom.create_element(QualName::new(
                None,
                Namespace::from(""),
                LocalName::from("div"),
            ));
            dom.set_attribute(inserted, attr("id"), "inserted");
            let text = dom.create_text("inserted");
            dom.append_child(inserted, text);
            dom.insert_before(body, inserted, Some(after));
        });
        let retained_paint = retained.frame(160, 120).expect("retained final frame");

        let mut fresh_dom = ScriptedDom::from_serialized_document(final_document);
        let mut fresh_mutations = Vec::new();
        fresh_dom.drain_mutations(&mut fresh_mutations);
        let mut fresh = LiveryDocument::new(fresh_dom, styles(), Device::screen(160.0, 120.0));
        let fresh_paint = fresh.frame(160, 120).expect("fresh final frame");

        assert_eq!(
            format!("{:?}", retained_paint.commands()),
            format!("{:?}", fresh_paint.commands()),
            "retained mutation and fresh final-document layout must emit the same paint commands",
        );
        assert_eq!(
            retained.content_height(0),
            fresh.content_height(0),
            "the same final DOM has the same retained document extent",
        );
    }

    #[test]
    fn dom_mutation_records_its_nearest_formatting_context_root() {
        let mut dom = ScriptedDom::from_serialized_document(
            "<html><body><div id=flex><div id=existing>existing</div></div></body></html>",
        );
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut document = LiveryDocument::new(
            dom,
            StyleSet::cambium(&[
                "html, body { margin: 0; padding: 0; } #flex { display: flex; width: 120px; }",
            ]),
            Device::screen(160.0, 120.0),
        );
        document.frame(160, 120).expect("initial frame");
        let flex = by_id(document.dom(), "flex");

        document.mutate_dom(|dom| {
            let flex = by_id(dom, "flex");
            let inserted = dom.create_element(QualName::new(
                None,
                Namespace::from(""),
                LocalName::from("div"),
            ));
            dom.append_child(flex, inserted);
        });

        assert_eq!(
            document.last_layout_damage(),
            Some(&LayoutDamage {
                kind: LayoutDamageKind::Dom,
                roots: vec![flex],
                full_document: false,
            }),
            "K5h records the flex root whose child list changed",
        );
    }

    #[test]
    fn retained_formatting_root_splice_refreshes_descendants_and_keeps_outside_identity() {
        let initial = "<html><body><div id=flex><div id=child>child</div></div><div id=outside>outside</div></body></html>";
        let final_document = "<html><body><div id=flex style=\"width: 180px\"><div id=child>child</div></div><div id=outside>outside</div></body></html>";
        let styles = || {
            StyleSet::cambium(&[
                "html, body { margin: 0; padding: 0; } \
                 #flex { display: flex; width: 100px; height: 40px; background: red; } \
                 #child { width: 40px; height: 20px; background: blue; } \
                 #outside { width: 80px; height: 20px; background: green; }",
            ])
        };
        let mut dom = ScriptedDom::from_serialized_document(initial);
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut retained = LiveryDocument::new(dom, styles(), Device::screen(240.0, 120.0));
        retained.frame(240, 120).expect("initial retained frame");
        let flex = by_id(retained.dom(), "flex");
        let child = by_id(retained.dom(), "child");
        let outside = by_id(retained.dom(), "outside");
        let flex_before = generated_ids(&retained, flex);
        let child_before = generated_ids(&retained, child);
        let outside_before = generated_ids(&retained, outside);
        let layout_generation = retained.layout_generation();

        retained.mutate_dom(|dom| {
            dom.set_attribute(by_id(dom, "flex"), attr("style"), "width: 180px");
        });
        assert_eq!(
            retained.last_layout_damage(),
            Some(&LayoutDamage {
                kind: LayoutDamageKind::Dom,
                roots: vec![flex],
                full_document: false,
            })
        );
        let retained_paint = retained.frame(240, 120).expect("spliced retained frame");

        assert_eq!(retained.layout_generation(), layout_generation + 1);
        assert_eq!(generated_ids(&retained, flex), flex_before);
        assert_ne!(
            generated_ids(&retained, child),
            child_before,
            "the selected formatting root receives fresh descendant fragments"
        );
        assert_eq!(generated_ids(&retained, outside), outside_before);
        assert!(
            retained.identity_source.is_none() && !retained.layout_dirty,
            "the selected root was published into the retained layout"
        );

        let mut fresh_dom = ScriptedDom::from_serialized_document(final_document);
        let mut fresh_mutations = Vec::new();
        fresh_dom.drain_mutations(&mut fresh_mutations);
        let mut fresh = LiveryDocument::new(fresh_dom, styles(), Device::screen(240.0, 120.0));
        let fresh_paint = fresh.frame(240, 120).expect("fresh final frame");
        assert_eq!(
            format!("{:?}", retained_paint.commands()),
            format!("{:?}", fresh_paint.commands()),
            "the spliced root must paint like a fresh final document",
        );
        assert_eq!(retained.content_height(0), fresh.content_height(0));
    }

    #[test]
    fn retained_flex_root_splice_accepts_an_inserted_child_box() {
        let initial = "<html><body><div id=flex><div id=existing>existing</div></div><div id=outside>outside</div></body></html>";
        let final_document = "<html><body><div id=flex><div id=existing>existing</div><div id=inserted>inserted</div></div><div id=outside>outside</div></body></html>";
        let styles = || {
            StyleSet::cambium(&[
                "html, body { margin: 0; padding: 0; } \
                 #flex { display: flex; width: 180px; height: 40px; background: red; } \
                 #existing, #inserted { width: 60px; height: 20px; background: blue; } \
                 #outside { width: 80px; height: 20px; background: green; }",
            ])
        };
        let mut dom = ScriptedDom::from_serialized_document(initial);
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut retained = LiveryDocument::new(dom, styles(), Device::screen(240.0, 120.0));
        retained.frame(240, 120).expect("initial retained frame");
        let flex = by_id(retained.dom(), "flex");
        let existing = by_id(retained.dom(), "existing");
        let outside = by_id(retained.dom(), "outside");
        let flex_before = generated_ids(&retained, flex);
        let existing_before = generated_ids(&retained, existing);
        let outside_before = generated_ids(&retained, outside);
        let local_generation = retained.retained_root_relayout_generation;

        retained.mutate_dom(|dom| {
            let flex = by_id(dom, "flex");
            let inserted = dom.create_element(QualName::new(
                None,
                Namespace::from(""),
                LocalName::from("div"),
            ));
            dom.set_attribute(inserted, attr("id"), "inserted");
            let text = dom.create_text("inserted");
            dom.append_child(inserted, text);
            dom.append_child(flex, inserted);
        });
        assert_eq!(
            retained.last_layout_damage(),
            Some(&LayoutDamage {
                kind: LayoutDamageKind::Dom,
                roots: vec![flex],
                full_document: false,
            })
        );
        let retained_paint = retained.frame(240, 120).expect("spliced retained frame");

        assert_eq!(
            retained.retained_root_relayout_generation,
            local_generation + 1,
            "the text-bearing flex root takes the selected-root formatter",
        );
        assert_eq!(generated_ids(&retained, flex), flex_before);
        assert_ne!(generated_ids(&retained, existing), existing_before);
        assert_eq!(generated_ids(&retained, outside), outside_before);
        assert!(
            !generated_ids(&retained, by_id(retained.dom(), "inserted")).is_empty(),
            "the new child is published through the fresh selected-root box tree",
        );
        assert!(retained.text_target("existing").is_some());
        assert!(retained.text_target("inserted").is_some());
        assert!(retained.text_target("outside").is_some());

        let mut fresh_dom = ScriptedDom::from_serialized_document(final_document);
        let mut fresh_mutations = Vec::new();
        fresh_dom.drain_mutations(&mut fresh_mutations);
        let mut fresh = LiveryDocument::new(fresh_dom, styles(), Device::screen(240.0, 120.0));
        let fresh_paint = fresh.frame(240, 120).expect("fresh final frame");
        assert_eq!(
            format!("{:?}", retained_paint.commands()),
            format!("{:?}", fresh_paint.commands()),
            "the selected flex-root splice must paint like a fresh structural mutation",
        );
        assert_eq!(retained.content_height(0), fresh.content_height(0));
    }

    #[test]
    fn retained_grid_root_splice_accepts_an_inserted_child_box() {
        let initial = "<html><body><div id=grid><div id=existing>existing</div></div><div id=outside>outside</div></body></html>";
        let final_document = "<html><body><div id=grid><div id=existing>existing</div><div id=inserted>inserted</div></div><div id=outside>outside</div></body></html>";
        let styles = || {
            StyleSet::cambium(&[
                "html, body { margin: 0; padding: 0; } \
                 #grid { display: grid; grid-template-columns: 60px 60px; width: 180px; height: 40px; background: red; } \
                 #existing, #inserted { width: 60px; height: 20px; background: blue; } \
                 #outside { width: 80px; height: 20px; background: green; }",
            ])
        };
        let mut dom = ScriptedDom::from_serialized_document(initial);
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut retained = LiveryDocument::new(dom, styles(), Device::screen(240.0, 120.0));
        retained.frame(240, 120).expect("initial retained frame");
        let grid = by_id(retained.dom(), "grid");
        let existing = by_id(retained.dom(), "existing");
        let outside = by_id(retained.dom(), "outside");
        let grid_before = generated_ids(&retained, grid);
        let existing_before = generated_ids(&retained, existing);
        let outside_before = generated_ids(&retained, outside);
        let local_generation = retained.retained_root_relayout_generation;

        retained.mutate_dom(|dom| {
            let grid = by_id(dom, "grid");
            let inserted = dom.create_element(QualName::new(
                None,
                Namespace::from(""),
                LocalName::from("div"),
            ));
            dom.set_attribute(inserted, attr("id"), "inserted");
            let text = dom.create_text("inserted");
            dom.append_child(inserted, text);
            dom.append_child(grid, inserted);
        });
        assert_eq!(
            retained.last_layout_damage(),
            Some(&LayoutDamage {
                kind: LayoutDamageKind::Dom,
                roots: vec![grid],
                full_document: false,
            })
        );
        let retained_paint = retained.frame(240, 120).expect("spliced retained frame");

        assert_eq!(
            retained.retained_root_relayout_generation,
            local_generation + 1,
            "the text-bearing grid root takes the selected-root formatter",
        );
        assert_eq!(generated_ids(&retained, grid), grid_before);
        assert_ne!(generated_ids(&retained, existing), existing_before);
        assert_eq!(generated_ids(&retained, outside), outside_before);
        assert!(
            !generated_ids(&retained, by_id(retained.dom(), "inserted")).is_empty(),
            "the new child is published through the fresh selected-root box tree",
        );
        assert!(retained.text_target("existing").is_some());
        assert!(retained.text_target("inserted").is_some());
        assert!(retained.text_target("outside").is_some());

        let mut fresh_dom = ScriptedDom::from_serialized_document(final_document);
        let mut fresh_mutations = Vec::new();
        fresh_dom.drain_mutations(&mut fresh_mutations);
        let mut fresh = LiveryDocument::new(fresh_dom, styles(), Device::screen(240.0, 120.0));
        let fresh_paint = fresh.frame(240, 120).expect("fresh final frame");
        assert_eq!(
            format!("{:?}", retained_paint.commands()),
            format!("{:?}", fresh_paint.commands()),
            "the selected grid-root splice must paint like a fresh structural mutation",
        );
        assert_eq!(retained.content_height(0), fresh.content_height(0));
    }

    #[test]
    fn retained_root_formatter_adds_its_first_text_source_in_dom_order() {
        let initial = "<html><body><div id=flex><div id=existing></div></div><div id=outside>outside</div></body></html>";
        let final_document = "<html><body><div id=flex><div id=existing></div><div id=inserted>inside</div></div><div id=outside>outside</div></body></html>";
        let styles = || {
            StyleSet::cambium(&[
                "html, body { margin: 0; padding: 0; } \
                 #flex { display: flex; width: 180px; height: 40px; background: red; } \
                 #existing, #inserted { width: 60px; height: 20px; background: blue; } \
                 #outside { width: 80px; height: 20px; background: green; }",
            ])
        };
        let mut dom = ScriptedDom::from_serialized_document(initial);
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut retained = LiveryDocument::new(dom, styles(), Device::screen(240.0, 120.0));
        retained.frame(240, 120).expect("initial retained frame");
        let flex = by_id(retained.dom(), "flex");
        let existing = by_id(retained.dom(), "existing");
        let outside = by_id(retained.dom(), "outside");
        let flex_before = generated_ids(&retained, flex);
        let existing_before = generated_ids(&retained, existing);
        let outside_before = generated_ids(&retained, outside);
        let local_generation = retained.retained_root_relayout_generation;

        retained.mutate_dom(|dom| {
            let flex = by_id(dom, "flex");
            let inserted = dom.create_element(QualName::new(
                None,
                Namespace::from(""),
                LocalName::from("div"),
            ));
            dom.set_attribute(inserted, attr("id"), "inserted");
            let text = dom.create_text("inside");
            dom.append_child(inserted, text);
            dom.append_child(flex, inserted);
        });
        let retained_paint = retained.frame(240, 120).expect("locally formatted frame");

        assert_eq!(
            retained.retained_root_relayout_generation,
            local_generation + 1,
            "the first text source takes the selected-root formatter instead of the complete-layout publication path",
        );
        assert_eq!(generated_ids(&retained, flex), flex_before);
        assert_ne!(generated_ids(&retained, existing), existing_before);
        assert_eq!(generated_ids(&retained, outside), outside_before);
        assert!(
            !generated_ids(&retained, by_id(retained.dom(), "inserted")).is_empty(),
            "the selected-root formatter publishes the inserted descendant",
        );
        assert!(retained.text_target("inside").is_some());
        assert!(retained.text_target("outside").is_some());
        let inserted_text = retained
            .dom()
            .dom_children(by_id(retained.dom(), "inserted"))
            .find(|node| retained.dom().kind(*node) == NodeKind::Text)
            .expect("inserted text source");
        let outside_text = retained
            .dom()
            .dom_children(by_id(retained.dom(), "outside"))
            .find(|node| retained.dom().kind(*node) == NodeKind::Text)
            .expect("outside text source");
        let text_order = retained
            .layout
            .as_ref()
            .and_then(|layout| layout.fragments.text_frame())
            .expect("retained text frame")
            .text_order();
        let inserted_index = text_order
            .iter()
            .position(|source| *source == inserted_text)
            .expect("inserted source stays ordered");
        let outside_index = text_order
            .iter()
            .position(|source| *source == outside_text)
            .expect("outside source stays ordered");
        assert!(inserted_index < outside_index);

        let mut fresh_dom = ScriptedDom::from_serialized_document(final_document);
        let mut fresh_mutations = Vec::new();
        fresh_dom.drain_mutations(&mut fresh_mutations);
        let mut fresh = LiveryDocument::new(fresh_dom, styles(), Device::screen(240.0, 120.0));
        let fresh_paint = fresh.frame(240, 120).expect("fresh final frame");
        assert_eq!(
            format!("{:?}", retained_paint.commands()),
            format!("{:?}", fresh_paint.commands()),
            "the locally formatted flex root paints like a fresh final document",
        );
        assert_eq!(retained.content_height(0), fresh.content_height(0));
    }

    #[test]
    fn retained_root_formatter_reflows_a_text_free_grid_subtree() {
        let initial = "<html><body><div id=grid><div id=existing></div></div><div id=outside></div></body></html>";
        let final_document = "<html><body><div id=grid><div id=existing></div><div id=inserted></div></div><div id=outside></div></body></html>";
        let styles = || {
            StyleSet::cambium(&[
                "html, body { margin: 0; padding: 0; } \
                 #grid { display: grid; grid-template-columns: 60px 60px; width: 180px; height: 40px; background: red; } \
                 #existing, #inserted { width: 60px; height: 20px; background: blue; } \
                 #outside { width: 80px; height: 20px; background: green; }",
            ])
        };
        let mut dom = ScriptedDom::from_serialized_document(initial);
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut retained = LiveryDocument::new(dom, styles(), Device::screen(240.0, 120.0));
        retained.frame(240, 120).expect("initial retained frame");
        let grid = by_id(retained.dom(), "grid");
        let existing = by_id(retained.dom(), "existing");
        let outside = by_id(retained.dom(), "outside");
        let grid_before = generated_ids(&retained, grid);
        let existing_before = generated_ids(&retained, existing);
        let outside_before = generated_ids(&retained, outside);
        let local_generation = retained.retained_root_relayout_generation;

        retained.mutate_dom(|dom| {
            let grid = by_id(dom, "grid");
            let inserted = dom.create_element(QualName::new(
                None,
                Namespace::from(""),
                LocalName::from("div"),
            ));
            dom.set_attribute(inserted, attr("id"), "inserted");
            dom.append_child(grid, inserted);
        });
        let retained_paint = retained.frame(240, 120).expect("locally formatted frame");

        assert_eq!(
            retained.retained_root_relayout_generation,
            local_generation + 1,
            "the text-free grid mutation takes the selected-root formatter instead of the complete-layout publication path",
        );
        assert_eq!(generated_ids(&retained, grid), grid_before);
        assert_ne!(generated_ids(&retained, existing), existing_before);
        assert_eq!(generated_ids(&retained, outside), outside_before);
        assert!(
            !generated_ids(&retained, by_id(retained.dom(), "inserted")).is_empty(),
            "the selected-root formatter publishes the inserted descendant",
        );

        let mut fresh_dom = ScriptedDom::from_serialized_document(final_document);
        let mut fresh_mutations = Vec::new();
        fresh_dom.drain_mutations(&mut fresh_mutations);
        let mut fresh = LiveryDocument::new(fresh_dom, styles(), Device::screen(240.0, 120.0));
        let fresh_paint = fresh.frame(240, 120).expect("fresh final frame");
        assert_eq!(
            format!("{:?}", retained_paint.commands()),
            format!("{:?}", fresh_paint.commands()),
            "the locally formatted grid root paints like a fresh final document",
        );
        assert_eq!(retained.content_height(0), fresh.content_height(0));
    }

    #[test]
    fn retained_root_formatter_drops_retired_text_sources() {
        let initial = "<html><body><div id=flex><div id=removed>remove me</div><div id=survives>survives</div></div><div id=outside>outside</div></body></html>";
        let final_document = "<html><body><div id=flex><div id=survives>survives</div></div><div id=outside>outside</div></body></html>";
        let styles = || {
            StyleSet::cambium(&[
                "html, body { margin: 0; padding: 0; } \
                 #flex { display: flex; width: 180px; height: 40px; background: red; } \
                 #removed, #survives { width: 60px; height: 20px; background: blue; } \
                 #outside { width: 80px; height: 20px; background: green; }",
            ])
        };
        let mut dom = ScriptedDom::from_serialized_document(initial);
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut retained = LiveryDocument::new(dom, styles(), Device::screen(240.0, 120.0));
        retained.frame(240, 120).expect("initial retained frame");
        let flex = by_id(retained.dom(), "flex");
        let survives = by_id(retained.dom(), "survives");
        let outside = by_id(retained.dom(), "outside");
        let flex_before = generated_ids(&retained, flex);
        let survives_before = generated_ids(&retained, survives);
        let outside_before = generated_ids(&retained, outside);
        let local_generation = retained.retained_root_relayout_generation;

        retained.mutate_dom(|dom| dom.remove_child(by_id(dom, "removed")));
        let retained_paint = retained.frame(240, 120).expect("locally formatted frame");

        assert_eq!(
            retained.retained_root_relayout_generation,
            local_generation + 1,
            "removing text takes the selected-root formatter",
        );
        assert_eq!(generated_ids(&retained, flex), flex_before);
        assert_ne!(generated_ids(&retained, survives), survives_before);
        assert_eq!(generated_ids(&retained, outside), outside_before);
        assert!(retained.text_target("remove me").is_none());
        assert!(retained.text_target("survives").is_some());
        assert!(retained.text_target("outside").is_some());

        let mut fresh_dom = ScriptedDom::from_serialized_document(final_document);
        let mut fresh_mutations = Vec::new();
        fresh_dom.drain_mutations(&mut fresh_mutations);
        let mut fresh = LiveryDocument::new(fresh_dom, styles(), Device::screen(240.0, 120.0));
        let fresh_paint = fresh.frame(240, 120).expect("fresh final frame");
        assert_eq!(
            format!("{:?}", retained_paint.commands()),
            format!("{:?}", fresh_paint.commands()),
            "the retained frame drops text whose selected subtree retired",
        );
        assert_eq!(
            format!("{:?}", retained_paint.fonts()),
            format!("{:?}", fresh_paint.fonts()),
            "retired text cannot retain a font resource in the paint list",
        );
        assert_eq!(retained.content_height(0), fresh.content_height(0));
    }

    #[test]
    fn retained_root_formatter_updates_fixed_root_overflow() {
        let initial = "<html><body><div id=flex><div id=existing></div></div><div id=outside></div></body></html>";
        let final_document = "<html><body><div id=flex><div id=existing></div><div id=inserted></div></div><div id=outside></div></body></html>";
        let styles = || {
            StyleSet::cambium(&[
                "html, body { margin: 0; padding: 0; } \
                 #flex { display: flex; flex-direction: column; width: 100px; height: 40px; background: red; } \
                 #existing { flex-shrink: 0; width: 100px; height: 20px; background: blue; } \
                 #inserted { flex-shrink: 0; width: 100px; height: 100px; background: blue; } \
                 #outside { width: 80px; height: 20px; background: green; }",
            ])
        };
        let mut dom = ScriptedDom::from_serialized_document(initial);
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut retained = LiveryDocument::new(dom, styles(), Device::screen(160.0, 120.0));
        retained.frame(160, 120).expect("initial retained frame");
        let flex = by_id(retained.dom(), "flex");
        let outside = by_id(retained.dom(), "outside");
        let flex_before = generated_ids(&retained, flex);
        let outside_before = generated_ids(&retained, outside);
        let content_height = retained.content_height(0);
        let local_generation = retained.retained_root_relayout_generation;

        retained.mutate_dom(|dom| {
            let flex = by_id(dom, "flex");
            let inserted = dom.create_element(QualName::new(
                None,
                Namespace::from(""),
                LocalName::from("div"),
            ));
            dom.set_attribute(inserted, attr("id"), "inserted");
            dom.append_child(flex, inserted);
        });
        let retained_paint = retained.frame(160, 120).expect("locally formatted frame");

        assert_eq!(
            retained.retained_root_relayout_generation,
            local_generation + 1,
            "fixed-size overflow takes the selected-root formatter",
        );
        assert_eq!(generated_ids(&retained, flex), flex_before);
        assert_eq!(generated_ids(&retained, outside), outside_before);
        assert!(retained.content_height(0) > content_height);

        let mut fresh_dom = ScriptedDom::from_serialized_document(final_document);
        let mut fresh_mutations = Vec::new();
        fresh_dom.drain_mutations(&mut fresh_mutations);
        let mut fresh = LiveryDocument::new(fresh_dom, styles(), Device::screen(160.0, 120.0));
        let fresh_paint = fresh.frame(160, 120).expect("fresh final frame");
        assert_eq!(
            format!("{:?}", retained_paint.commands()),
            format!("{:?}", fresh_paint.commands()),
            "overflow from the locally formatted root paints like a fresh document",
        );
        assert_eq!(retained.content_height(0), fresh.content_height(0));
    }

    #[test]
    fn retained_root_formatter_promotes_a_changed_size_to_its_block_parent() {
        let initial = "<html><body><div id=host><div id=flex><div id=existing></div></div><div id=after></div></div><div id=outside></div></body></html>";
        let final_document = "<html><body><div id=host><div id=flex><div id=existing></div><div id=inserted></div></div><div id=after></div></div><div id=outside></div></body></html>";
        let styles = || {
            StyleSet::cambium(&[
                "html, body { margin: 0; padding: 0; } \
                 #host { width: 160px; height: 120px; background: red; } \
                 #flex { display: flex; flex-direction: column; width: 100px; background: blue; } \
                 #existing { flex-shrink: 0; width: 100px; height: 20px; } \
                 #inserted { flex-shrink: 0; width: 100px; height: 60px; } \
                 #after { width: 100px; height: 20px; background: yellow; } \
                 #outside { width: 80px; height: 20px; background: green; }",
            ])
        };
        let mut dom = ScriptedDom::from_serialized_document(initial);
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut retained = LiveryDocument::new(dom, styles(), Device::screen(240.0, 180.0));
        retained.frame(240, 180).expect("initial retained frame");
        let host = by_id(retained.dom(), "host");
        let flex = by_id(retained.dom(), "flex");
        let after = by_id(retained.dom(), "after");
        let outside = by_id(retained.dom(), "outside");
        let host_before = generated_ids(&retained, host);
        let flex_before = generated_ids(&retained, flex);
        let after_before = generated_ids(&retained, after);
        let outside_before = generated_ids(&retained, outside);
        let local_generation = retained.retained_root_relayout_generation;

        retained.mutate_dom(|dom| {
            let flex = by_id(dom, "flex");
            let inserted = dom.create_element(QualName::new(
                None,
                Namespace::from(""),
                LocalName::from("div"),
            ));
            dom.set_attribute(inserted, attr("id"), "inserted");
            dom.append_child(flex, inserted);
        });
        assert_eq!(
            retained.last_layout_damage(),
            Some(&LayoutDamage {
                kind: LayoutDamageKind::Dom,
                roots: vec![flex],
                full_document: false,
            })
        );
        let retained_paint = retained.frame(240, 180).expect("promoted retained frame");

        assert_eq!(
            retained.retained_root_relayout_generation,
            local_generation + 1,
            "a changed flex used size promotes to its block formatting parent",
        );
        assert_eq!(generated_ids(&retained, host), host_before);
        assert_ne!(generated_ids(&retained, flex), flex_before);
        assert_ne!(generated_ids(&retained, after), after_before);
        assert_eq!(generated_ids(&retained, outside), outside_before);

        let mut fresh_dom = ScriptedDom::from_serialized_document(final_document);
        let mut fresh_mutations = Vec::new();
        fresh_dom.drain_mutations(&mut fresh_mutations);
        let mut fresh = LiveryDocument::new(fresh_dom, styles(), Device::screen(240.0, 180.0));
        let fresh_paint = fresh.frame(240, 180).expect("fresh final frame");
        assert_eq!(
            format!("{:?}", retained_paint.commands()),
            format!("{:?}", fresh_paint.commands()),
            "the promoted block root paints like a fresh final document",
        );
        assert_eq!(retained.content_height(0), fresh.content_height(0));
    }

    #[test]
    fn retained_root_formatter_promotes_through_a_changed_parent_to_a_stable_ancestor() {
        let initial = "<html><body><div id=host><div id=parent><div id=flex><div id=existing></div></div><div id=after></div></div></div><div id=outside></div></body></html>";
        let final_document = "<html><body><div id=host><div id=parent><div id=flex><div id=existing></div><div id=inserted></div></div><div id=after></div></div></div><div id=outside></div></body></html>";
        let styles = || {
            StyleSet::cambium(&[
                "html, body { margin: 0; padding: 0; } \
                 #host { width: 160px; height: 160px; background: red; } \
                 #parent { width: 160px; background: orange; } \
                 #flex { display: flex; flex-direction: column; width: 100px; background: blue; } \
                 #existing { flex-shrink: 0; width: 100px; height: 20px; } \
                 #inserted { flex-shrink: 0; width: 100px; height: 60px; } \
                 #after { width: 100px; height: 20px; background: yellow; } \
                 #outside { width: 80px; height: 20px; background: green; }",
            ])
        };
        let mut dom = ScriptedDom::from_serialized_document(initial);
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut retained = LiveryDocument::new(dom, styles(), Device::screen(240.0, 220.0));
        retained.frame(240, 220).expect("initial retained frame");
        let host = by_id(retained.dom(), "host");
        let parent = by_id(retained.dom(), "parent");
        let flex = by_id(retained.dom(), "flex");
        let after = by_id(retained.dom(), "after");
        let outside = by_id(retained.dom(), "outside");
        let host_before = generated_ids(&retained, host);
        let parent_before = generated_ids(&retained, parent);
        let flex_before = generated_ids(&retained, flex);
        let after_before = generated_ids(&retained, after);
        let outside_before = generated_ids(&retained, outside);
        let local_generation = retained.retained_root_relayout_generation;

        retained.mutate_dom(|dom| {
            let flex = by_id(dom, "flex");
            let inserted = dom.create_element(QualName::new(
                None,
                Namespace::from(""),
                LocalName::from("div"),
            ));
            dom.set_attribute(inserted, attr("id"), "inserted");
            dom.append_child(flex, inserted);
        });
        assert_eq!(
            retained.last_layout_damage(),
            Some(&LayoutDamage {
                kind: LayoutDamageKind::Dom,
                roots: vec![flex],
                full_document: false,
            })
        );
        let retained_paint = retained
            .frame(240, 220)
            .expect("ancestor-promoted retained frame");

        assert_eq!(
            retained.retained_root_relayout_generation,
            local_generation + 1,
            "the formatter promotes only after each selected root grows",
        );
        assert_eq!(generated_ids(&retained, host), host_before);
        assert_ne!(generated_ids(&retained, parent), parent_before);
        assert_ne!(generated_ids(&retained, flex), flex_before);
        assert_ne!(generated_ids(&retained, after), after_before);
        assert_eq!(generated_ids(&retained, outside), outside_before);

        let mut fresh_dom = ScriptedDom::from_serialized_document(final_document);
        let mut fresh_mutations = Vec::new();
        fresh_dom.drain_mutations(&mut fresh_mutations);
        let mut fresh = LiveryDocument::new(fresh_dom, styles(), Device::screen(240.0, 220.0));
        let fresh_paint = fresh.frame(240, 220).expect("fresh final frame");
        assert_eq!(
            format!("{:?}", retained_paint.commands()),
            format!("{:?}", fresh_paint.commands()),
            "the stable ancestor paints like a fresh final document",
        );
        assert_eq!(retained.content_height(0), fresh.content_height(0));
    }

    #[test]
    fn retained_root_formatter_replaces_a_fixed_size_table_and_its_paint_plane() {
        let initial = "<html><body><table id=table><tbody><tr id=row><td id=first></td></tr></tbody></table><div id=outside></div></body></html>";
        let final_document = "<html><body><table id=table><tbody><tr id=row><td id=first></td><td id=second></td></tr></tbody></table><div id=outside></div></body></html>";
        let styles = || {
            StyleSet::cambium(&[
                "html, body { margin: 0; padding: 0; } \
                 table { display: table; table-layout: fixed; width: 120px; height: 80px; border-spacing: 0; background: blue; } \
                 tbody { display: table-row-group; } tr { display: table-row; } \
                 td { display: table-cell; width: 40px; height: 20px; background: yellow; } \
                 #outside { width: 80px; height: 20px; background: green; }",
            ])
        };
        let mut dom = ScriptedDom::from_serialized_document(initial);
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut retained = LiveryDocument::new(dom, styles(), Device::screen(240.0, 180.0));
        retained.frame(240, 180).expect("initial retained frame");
        let table = by_id(retained.dom(), "table");
        let row = by_id(retained.dom(), "row");
        let first = by_id(retained.dom(), "first");
        let outside = by_id(retained.dom(), "outside");
        let wrapper_before = table_wrapper_fragment_id(&retained, table);
        let table_before = generated_ids(&retained, table);
        let first_before = generated_ids(&retained, first);
        let outside_before = generated_ids(&retained, outside);
        let local_generation = retained.retained_root_relayout_generation;
        assert_table_paint_sources_are_live(&retained, table);

        retained.mutate_dom(|dom| {
            let row = by_id(dom, "row");
            let cell = dom.create_element(QualName::new(
                None,
                Namespace::from(""),
                LocalName::from("td"),
            ));
            dom.set_attribute(cell, attr("id"), "second");
            dom.append_child(row, cell);
        });
        assert_eq!(
            retained.last_layout_damage(),
            Some(&LayoutDamage {
                kind: LayoutDamageKind::Dom,
                roots: vec![row],
                full_document: false,
            })
        );
        let retained_paint = retained.frame(240, 180).expect("retained table frame");

        assert_eq!(
            retained.retained_root_relayout_generation,
            local_generation + 1,
            "the fixed-size table uses the selected-root formatter",
        );
        assert_eq!(table_wrapper_fragment_id(&retained, table), wrapper_before);
        assert_ne!(generated_ids(&retained, table), table_before);
        assert_ne!(generated_ids(&retained, first), first_before);
        assert_eq!(generated_ids(&retained, outside), outside_before);
        assert_table_paint_sources_are_live(&retained, table);

        let mut fresh_dom = ScriptedDom::from_serialized_document(final_document);
        let mut fresh_mutations = Vec::new();
        fresh_dom.drain_mutations(&mut fresh_mutations);
        let mut fresh = LiveryDocument::new(fresh_dom, styles(), Device::screen(240.0, 180.0));
        let fresh_paint = fresh.frame(240, 180).expect("fresh final frame");
        assert_eq!(
            format!("{:?}", retained_paint.commands()),
            format!("{:?}", fresh_paint.commands()),
            "the retained table paint plane matches a fresh final document",
        );
        assert_eq!(retained.content_height(0), fresh.content_height(0));
    }

    #[test]
    fn retained_root_formatter_replaces_a_captioned_fixed_size_table() {
        let initial = "<html><body><table id=table><caption id=caption>caption</caption><tbody><tr id=row><td id=first></td></tr></tbody></table><div id=outside></div></body></html>";
        let final_document = "<html><body><table id=table><caption id=caption>caption</caption><tbody><tr id=row><td id=first></td><td id=second></td></tr></tbody></table><div id=outside></div></body></html>";
        let styles = || {
            StyleSet::cambium(&[
                "html, body { margin: 0; padding: 0; } \
                 table { display: table; table-layout: fixed; width: 120px; height: 80px; border-spacing: 0; background: blue; } \
                 caption { display: table-caption; width: 120px; height: 20px; background: red; } \
                 tbody { display: table-row-group; } tr { display: table-row; } \
                 td { display: table-cell; width: 40px; height: 20px; background: yellow; } \
                 #outside { width: 80px; height: 20px; background: green; }",
            ])
        };
        let mut dom = ScriptedDom::from_serialized_document(initial);
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut retained = LiveryDocument::new(dom, styles(), Device::screen(240.0, 200.0));
        retained.frame(240, 200).expect("initial retained frame");
        let table = by_id(retained.dom(), "table");
        let row = by_id(retained.dom(), "row");
        let caption = by_id(retained.dom(), "caption");
        let outside = by_id(retained.dom(), "outside");
        let wrapper_before = table_wrapper_fragment_id(&retained, table);
        let caption_before = generated_ids(&retained, caption);
        let outside_before = generated_ids(&retained, outside);
        let local_generation = retained.retained_root_relayout_generation;

        retained.mutate_dom(|dom| {
            let row = by_id(dom, "row");
            let cell = dom.create_element(QualName::new(
                None,
                Namespace::from(""),
                LocalName::from("td"),
            ));
            dom.set_attribute(cell, attr("id"), "second");
            dom.append_child(row, cell);
        });
        assert_eq!(
            retained.last_layout_damage(),
            Some(&LayoutDamage {
                kind: LayoutDamageKind::Dom,
                roots: vec![row],
                full_document: false,
            })
        );
        let retained_paint = retained.frame(240, 200).expect("retained table frame");

        assert_eq!(
            retained.retained_root_relayout_generation,
            local_generation + 1,
            "a stable table wrapper admits its caption and grid together",
        );
        assert_eq!(table_wrapper_fragment_id(&retained, table), wrapper_before);
        assert_ne!(generated_ids(&retained, caption), caption_before);
        assert_eq!(generated_ids(&retained, outside), outside_before);

        let mut fresh_dom = ScriptedDom::from_serialized_document(final_document);
        let mut fresh_mutations = Vec::new();
        fresh_dom.drain_mutations(&mut fresh_mutations);
        let mut fresh = LiveryDocument::new(fresh_dom, styles(), Device::screen(240.0, 200.0));
        let fresh_paint = fresh.frame(240, 200).expect("fresh final frame");
        assert_eq!(
            format!("{:?}", retained_paint.commands()),
            format!("{:?}", fresh_paint.commands()),
            "the captioned table paints like a fresh final document",
        );
        assert_eq!(retained.content_height(0), fresh.content_height(0));
    }

    #[test]
    fn retained_table_root_keeps_an_unrelated_table_paint_plane_live() {
        let initial = "<html><body><table id=changed><tbody><tr id=row><td id=first></td></tr></tbody></table><table id=other><tbody><tr><td id=other-cell></td></tr></tbody></table></body></html>";
        let final_document = "<html><body><table id=changed><tbody><tr id=row><td id=first></td><td id=second></td></tr></tbody></table><table id=other><tbody><tr><td id=other-cell></td></tr></tbody></table></body></html>";
        let styles = || {
            StyleSet::cambium(&[
                "html, body { margin: 0; padding: 0; } \
                 table { display: table; table-layout: fixed; width: 120px; height: 80px; border-spacing: 0; background: blue; } \
                 tbody { display: table-row-group; } tr { display: table-row; } \
                 td { display: table-cell; width: 40px; height: 20px; background: yellow; } \
                 #other-cell { position: absolute; top: 0; }",
            ])
        };
        let mut dom = ScriptedDom::from_serialized_document(initial);
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut retained = LiveryDocument::new(dom, styles(), Device::screen(240.0, 220.0));
        retained.frame(240, 220).expect("initial retained frame");
        let changed = by_id(retained.dom(), "changed");
        let row = by_id(retained.dom(), "row");
        let other = by_id(retained.dom(), "other");
        let changed_wrapper_before = table_wrapper_fragment_id(&retained, changed);
        let other_wrapper_before = table_wrapper_fragment_id(&retained, other);
        let other_before = generated_ids(&retained, other);
        let local_generation = retained.retained_root_relayout_generation;
        assert_table_paint_sources_are_live(&retained, changed);
        assert_table_paint_sources_are_live(&retained, other);
        let initial_ledger = retained.table_shadow_ledger().expect("completed table ledger");
        assert_eq!(initial_ledger.assigned, 2, "one contribution per live table");
        assert_eq!(initial_ledger.honored, 2, "both tables remain verified");
        assert!(
            !initial_ledger.positioning_gaps.is_empty(),
            "the untouched table keeps a noncanonical K5 table-part record",
        );

        retained.mutate_dom(|dom| {
            let row = by_id(dom, "row");
            let cell = dom.create_element(QualName::new(
                None,
                Namespace::from(""),
                LocalName::from("td"),
            ));
            dom.set_attribute(cell, attr("id"), "second");
            dom.append_child(row, cell);
        });
        assert_eq!(
            retained.last_layout_damage(),
            Some(&LayoutDamage {
                kind: LayoutDamageKind::Dom,
                roots: vec![row],
                full_document: false,
            })
        );
        let retained_paint = retained.frame(240, 220).expect("retained table frame");

        assert_eq!(
            retained.retained_root_relayout_generation,
            local_generation + 1,
            "one canonical table contribution can be replaced in place",
        );
        assert_eq!(
            table_wrapper_fragment_id(&retained, changed),
            changed_wrapper_before,
        );
        assert_eq!(table_wrapper_fragment_id(&retained, other), other_wrapper_before);
        assert_eq!(generated_ids(&retained, other), other_before);
        assert_table_paint_sources_are_live(&retained, changed);
        assert_table_paint_sources_are_live(&retained, other);
        let retained_ledger = retained.table_shadow_ledger().expect("retained table ledger");
        assert_eq!(retained_ledger.assigned, 2, "aggregate keeps both table entries");
        assert_eq!(retained_ledger.honored, 2, "both table entries remain verified");
        assert!(
            !retained_ledger.positioning_gaps.is_empty(),
            "the untouched table's absolute record remains in the aggregate ledger",
        );

        let mut fresh_dom = ScriptedDom::from_serialized_document(final_document);
        let mut fresh_mutations = Vec::new();
        fresh_dom.drain_mutations(&mut fresh_mutations);
        let mut fresh = LiveryDocument::new(fresh_dom, styles(), Device::screen(240.0, 220.0));
        let fresh_paint = fresh.frame(240, 220).expect("fresh final frame");
        assert_eq!(
            format!("{:?}", retained_paint.commands()),
            format!("{:?}", fresh_paint.commands()),
            "both retained table paint planes match a fresh final document",
        );
        assert_eq!(retained.content_height(0), fresh.content_height(0));
    }

    #[test]
    fn sticky_table_cell_uses_its_nested_scrollport_without_relayout() {
        let mut dom = ScriptedDom::from_serialized_document(
            "<html><body><div id=scroller><div id=content><div id=spacer></div><table id=table><tbody><tr><td id=sticky>sticky</td></tr><tr><td id=tail></td></tr></tbody></table></div></div></body></html>",
        );
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut document = LiveryDocument::new(
            dom,
            StyleSet::cambium(&[
                "html, body { margin: 0; padding: 0; } \
                 #scroller { height: 80px; overflow-y: auto; } \
                 #spacer { height: 120px; } \
                 table { display: table; table-layout: fixed; width: 100px; border-spacing: 0; } \
                 tbody { display: table-row-group; } tr { display: table-row; } \
                 td { display: table-cell; width: 100px; } \
                 #sticky { position: sticky; top: 0; height: 20px; background: red; } \
                 #tail { height: 180px; background: blue; }",
            ]),
            Device::screen(160.0, 120.0),
        );
        document.frame(160, 120).expect("initial table frame");
        let scroller = by_id(document.dom(), "scroller");
        let table = by_id(document.dom(), "table");
        let sticky = by_id(document.dom(), "sticky");
        let ids_before = generated_ids(&document, sticky);
        let table_wrapper_before = table_wrapper_fragment_id(&document, table);
        let static_rect = document
            .layout
            .as_ref()
            .and_then(|layout| layout.fragments.get(sticky))
            .map(|fragment| fragment.physical_rect())
            .expect("static table cell fragment");
        assert_eq!(static_rect.y, 120.0);
        assert!(
            !document
                .table_shadow_ledger()
                .expect("table ledger")
                .positioning_gaps
                .iter()
                .any(|record| record.gap == crate::table_shadow::TablePositioningGap::Sticky),
            "table-cell sticky uses the shared retained sticky solver",
        );

        assert!(document.scroll_at(10.0, 10.0, 0.0, 150.0));
        assert_eq!(document.element_scroll().get(&scroller), Some(&(0.0, 150.0)));
        document.frame(160, 120).expect("scrolled table frame");

        let active = document.sticky_layout(document.layout.as_ref().expect("retained layout"));
        let sticky_rect = active
            .get(sticky)
            .map(|fragment| fragment.physical_rect())
            .expect("active table cell fragment");
        assert_eq!(sticky_rect.y, 150.0);
        assert_eq!(sticky_rect.y - document.element_scroll()[&scroller].1, 0.0);
        assert_eq!(generated_ids(&document, sticky), ids_before);
        assert_eq!(table_wrapper_fragment_id(&document, table), table_wrapper_before);
        assert_eq!(
            document
                .layout
                .as_ref()
                .and_then(|layout| layout.fragments.get(sticky))
                .map(|fragment| fragment.physical_rect()),
            Some(static_rect),
            "scrolling keeps the retained table base layout unchanged",
        );
    }

    #[test]
    fn sticky_table_row_moves_its_cell_subtree_without_relayout() {
        let mut dom = ScriptedDom::from_serialized_document(
            "<html><body><div id=scroller><div id=content><div id=spacer></div><table id=table><tbody><tr id=sticky-row><td id=sticky-cell>sticky</td></tr><tr><td id=tail-cell></td></tr></tbody></table></div></div></body></html>",
        );
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut document = LiveryDocument::new(
            dom,
            StyleSet::cambium(&[
                "html, body { margin: 0; padding: 0; } \
                 #scroller { height: 80px; overflow-y: auto; } \
                 #spacer { height: 120px; } \
                 table { display: table; table-layout: fixed; width: 100px; border-spacing: 0; } \
                 tbody { display: table-row-group; } tr { display: table-row; } \
                 td { display: table-cell; width: 100px; } \
                 #sticky-row { position: sticky; top: 0; height: 20px; background: red; } \
                 #tail-cell { height: 180px; background: blue; }",
            ]),
            Device::screen(160.0, 120.0),
        );
        document.frame(160, 120).expect("initial table frame");
        let scroller = by_id(document.dom(), "scroller");
        let table = by_id(document.dom(), "table");
        let sticky_row = by_id(document.dom(), "sticky-row");
        let sticky_cell = by_id(document.dom(), "sticky-cell");
        let row_ids_before = generated_ids(&document, sticky_row);
        let cell_ids_before = generated_ids(&document, sticky_cell);
        let table_wrapper_before = table_wrapper_fragment_id(&document, table);
        let static_row = document
            .layout
            .as_ref()
            .and_then(|layout| layout.fragments.get(sticky_row))
            .map(|fragment| fragment.physical_rect())
            .expect("static table row fragment");
        assert_eq!(static_row.y, 120.0);
        assert!(
            !document
                .table_shadow_ledger()
                .expect("table ledger")
                .positioning_gaps
                .iter()
                .any(|record| record.gap == crate::table_shadow::TablePositioningGap::Sticky),
            "row sticky uses the shared retained sticky solver",
        );

        assert!(document.scroll_at(10.0, 10.0, 0.0, 150.0));
        document.frame(160, 120).expect("scrolled table frame");

        let active = document.sticky_layout(document.layout.as_ref().expect("retained layout"));
        let row_rect = active
            .get(sticky_row)
            .map(|fragment| fragment.physical_rect())
            .expect("active table row fragment");
        let cell_rect = active
            .get(sticky_cell)
            .map(|fragment| fragment.physical_rect())
            .expect("active table cell fragment");
        assert_eq!(row_rect.y, 150.0);
        assert_eq!(cell_rect.y, 150.0);
        assert_eq!(row_rect.y - document.element_scroll()[&scroller].1, 0.0);
        assert_eq!(generated_ids(&document, sticky_row), row_ids_before);
        assert_eq!(generated_ids(&document, sticky_cell), cell_ids_before);
        assert_eq!(table_wrapper_fragment_id(&document, table), table_wrapper_before);
        assert_eq!(
            document
                .layout
                .as_ref()
                .and_then(|layout| layout.fragments.get(sticky_row))
                .map(|fragment| fragment.physical_rect()),
            Some(static_row),
            "scrolling keeps the retained table row base layout unchanged",
        );
    }

    #[test]
    fn sticky_table_row_group_moves_its_row_subtree_without_relayout() {
        let mut dom = ScriptedDom::from_serialized_document(
            "<html><body><div id=scroller><div id=content><div id=spacer></div><table id=table><tbody id=sticky-group><tr id=sticky-row><td id=sticky-cell>sticky</td></tr></tbody><tbody><tr><td id=tail-cell></td></tr></tbody></table></div></div></body></html>",
        );
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut document = LiveryDocument::new(
            dom,
            StyleSet::cambium(&[
                "html, body { margin: 0; padding: 0; } \
                 #scroller { height: 80px; overflow-y: auto; } \
                 #spacer { height: 120px; } \
                 table { display: table; table-layout: fixed; width: 100px; border-spacing: 0; } \
                 tbody { display: table-row-group; } tr { display: table-row; } \
                 td { display: table-cell; width: 100px; } \
                 #sticky-group { position: sticky; top: 0; } \
                 #sticky-row { height: 20px; background: red; } \
                 #tail-cell { height: 180px; background: blue; }",
            ]),
            Device::screen(160.0, 120.0),
        );
        document.frame(160, 120).expect("initial table frame");
        let scroller = by_id(document.dom(), "scroller");
        let table = by_id(document.dom(), "table");
        let sticky_group = by_id(document.dom(), "sticky-group");
        let sticky_row = by_id(document.dom(), "sticky-row");
        let sticky_cell = by_id(document.dom(), "sticky-cell");
        let group_ids_before = generated_ids(&document, sticky_group);
        let row_ids_before = generated_ids(&document, sticky_row);
        let cell_ids_before = generated_ids(&document, sticky_cell);
        let table_wrapper_before = table_wrapper_fragment_id(&document, table);
        let static_group = document
            .layout
            .as_ref()
            .and_then(|layout| layout.fragments.get(sticky_group))
            .map(|fragment| fragment.physical_rect())
            .expect("static table row-group fragment");
        assert_eq!(static_group.y, 120.0);
        assert!(
            !document
                .table_shadow_ledger()
                .expect("table ledger")
                .positioning_gaps
                .iter()
                .any(|record| record.gap == crate::table_shadow::TablePositioningGap::Sticky),
            "row-group sticky uses the shared retained sticky solver",
        );

        assert!(document.scroll_at(10.0, 10.0, 0.0, 150.0));
        document.frame(160, 120).expect("scrolled table frame");

        let active = document.sticky_layout(document.layout.as_ref().expect("retained layout"));
        let group_rect = active
            .get(sticky_group)
            .map(|fragment| fragment.physical_rect())
            .expect("active table row-group fragment");
        let row_rect = active
            .get(sticky_row)
            .map(|fragment| fragment.physical_rect())
            .expect("active table row fragment");
        let cell_rect = active
            .get(sticky_cell)
            .map(|fragment| fragment.physical_rect())
            .expect("active table cell fragment");
        assert_eq!(group_rect.y, 150.0);
        assert_eq!(row_rect.y, 150.0);
        assert_eq!(cell_rect.y, 150.0);
        assert_eq!(group_rect.y - document.element_scroll()[&scroller].1, 0.0);
        assert_eq!(generated_ids(&document, sticky_group), group_ids_before);
        assert_eq!(generated_ids(&document, sticky_row), row_ids_before);
        assert_eq!(generated_ids(&document, sticky_cell), cell_ids_before);
        assert_eq!(table_wrapper_fragment_id(&document, table), table_wrapper_before);
        assert_eq!(
            document
                .layout
                .as_ref()
                .and_then(|layout| layout.fragments.get(sticky_group))
                .map(|fragment| fragment.physical_rect()),
            Some(static_group),
            "scrolling keeps the retained table row-group base layout unchanged",
        );
    }

    #[test]
    fn sticky_table_header_group_moves_its_row_subtree_without_relayout() {
        let mut dom = ScriptedDom::from_serialized_document(
            "<html><body><div id=scroller><div id=content><div id=spacer></div><table id=table><thead id=sticky-group><tr id=sticky-row><td id=sticky-cell>sticky</td></tr></thead><tbody><tr><td id=tail-cell></td></tr></tbody></table></div></div></body></html>",
        );
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut document = LiveryDocument::new(
            dom,
            StyleSet::cambium(&[
                "html, body { margin: 0; padding: 0; } \
                 #scroller { height: 80px; overflow-y: auto; } \
                 #spacer { height: 120px; } \
                 table { display: table; table-layout: fixed; width: 100px; border-spacing: 0; } \
                 thead { display: table-header-group; } tbody { display: table-row-group; } \
                 tr { display: table-row; } td { display: table-cell; width: 100px; } \
                 #sticky-group { position: sticky; top: 0; } \
                 #sticky-row { height: 20px; background: red; } \
                 #tail-cell { height: 180px; background: blue; }",
            ]),
            Device::screen(160.0, 120.0),
        );
        document.frame(160, 120).expect("initial table frame");
        let scroller = by_id(document.dom(), "scroller");
        let table = by_id(document.dom(), "table");
        let sticky_group = by_id(document.dom(), "sticky-group");
        let sticky_row = by_id(document.dom(), "sticky-row");
        let sticky_cell = by_id(document.dom(), "sticky-cell");
        let group_ids_before = generated_ids(&document, sticky_group);
        let row_ids_before = generated_ids(&document, sticky_row);
        let cell_ids_before = generated_ids(&document, sticky_cell);
        let table_wrapper_before = table_wrapper_fragment_id(&document, table);
        let static_group = document
            .layout
            .as_ref()
            .and_then(|layout| layout.fragments.get(sticky_group))
            .map(|fragment| fragment.physical_rect())
            .expect("static table header-group fragment");
        assert_eq!(static_group.y, 120.0);

        assert!(document.scroll_at(10.0, 10.0, 0.0, 150.0));
        document.frame(160, 120).expect("scrolled table frame");

        let active = document.sticky_layout(document.layout.as_ref().expect("retained layout"));
        let group_rect = active
            .get(sticky_group)
            .map(|fragment| fragment.physical_rect())
            .expect("active table header-group fragment");
        let row_rect = active
            .get(sticky_row)
            .map(|fragment| fragment.physical_rect())
            .expect("active table row fragment");
        let cell_rect = active
            .get(sticky_cell)
            .map(|fragment| fragment.physical_rect())
            .expect("active table cell fragment");
        assert_eq!(group_rect.y, 150.0);
        assert_eq!(row_rect.y, 150.0);
        assert_eq!(cell_rect.y, 150.0);
        assert_eq!(group_rect.y - document.element_scroll()[&scroller].1, 0.0);
        assert_eq!(generated_ids(&document, sticky_group), group_ids_before);
        assert_eq!(generated_ids(&document, sticky_row), row_ids_before);
        assert_eq!(generated_ids(&document, sticky_cell), cell_ids_before);
        assert_eq!(table_wrapper_fragment_id(&document, table), table_wrapper_before);
        assert_eq!(
            document
                .layout
                .as_ref()
                .and_then(|layout| layout.fragments.get(sticky_group))
                .map(|fragment| fragment.physical_rect()),
            Some(static_group),
            "scrolling keeps the retained header-group base layout unchanged",
        );
    }

    #[test]
    fn sticky_table_footer_group_uses_the_scrollport_end_without_relayout() {
        let mut dom = ScriptedDom::from_serialized_document(
            "<html><body><div id=scroller><div id=content><div id=spacer></div><table id=table><tbody><tr><td id=tail-cell></td></tr></tbody><tfoot id=sticky-group><tr id=sticky-row><td id=sticky-cell>sticky</td></tr></tfoot></table></div></div></body></html>",
        );
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut document = LiveryDocument::new(
            dom,
            StyleSet::cambium(&[
                "html, body { margin: 0; padding: 0; } \
                 #scroller { height: 80px; overflow-y: auto; } \
                 #spacer { height: 120px; } \
                 table { display: table; table-layout: fixed; width: 100px; border-spacing: 0; } \
                 tbody { display: table-row-group; } tfoot { display: table-footer-group; } \
                 tr { display: table-row; } td { display: table-cell; width: 100px; } \
                 #tail-cell { height: 180px; background: blue; } \
                 #sticky-group { position: sticky; bottom: 0; } \
                 #sticky-row { height: 20px; background: red; }",
            ]),
            Device::screen(160.0, 120.0),
        );
        document.frame(160, 120).expect("initial table frame");
        let scroller = by_id(document.dom(), "scroller");
        let table = by_id(document.dom(), "table");
        let sticky_group = by_id(document.dom(), "sticky-group");
        let sticky_row = by_id(document.dom(), "sticky-row");
        let sticky_cell = by_id(document.dom(), "sticky-cell");
        let group_ids_before = generated_ids(&document, sticky_group);
        let row_ids_before = generated_ids(&document, sticky_row);
        let cell_ids_before = generated_ids(&document, sticky_cell);
        let table_wrapper_before = table_wrapper_fragment_id(&document, table);
        let static_group = document
            .layout
            .as_ref()
            .and_then(|layout| layout.fragments.get(sticky_group))
            .map(|fragment| fragment.physical_rect())
            .expect("static table footer-group fragment");
        assert!(
            static_group.y > 120.0,
            "the footer group starts below the table's ordinary content"
        );

        assert!(document.scroll_at(10.0, 10.0, 0.0, 150.0));
        document.frame(160, 120).expect("scrolled table frame");

        let active = document.sticky_layout(document.layout.as_ref().expect("retained layout"));
        let group_rect = active
            .get(sticky_group)
            .map(|fragment| fragment.physical_rect())
            .expect("active table footer-group fragment");
        let row_rect = active
            .get(sticky_row)
            .map(|fragment| fragment.physical_rect())
            .expect("active table row fragment");
        let cell_rect = active
            .get(sticky_cell)
            .map(|fragment| fragment.physical_rect())
            .expect("active table cell fragment");
        assert_eq!(group_rect.y - document.element_scroll()[&scroller].1, 60.0);
        assert_eq!(row_rect.y, group_rect.y);
        assert_eq!(cell_rect.y, group_rect.y);
        assert_eq!(generated_ids(&document, sticky_group), group_ids_before);
        assert_eq!(generated_ids(&document, sticky_row), row_ids_before);
        assert_eq!(generated_ids(&document, sticky_cell), cell_ids_before);
        assert_eq!(table_wrapper_fragment_id(&document, table), table_wrapper_before);
        assert_eq!(
            document
                .layout
                .as_ref()
                .and_then(|layout| layout.fragments.get(sticky_group))
                .map(|fragment| fragment.physical_rect()),
            Some(static_group),
            "scrolling keeps the retained footer-group base layout unchanged",
        );
    }

    #[test]
    fn sticky_table_caption_uses_its_wrapper_scroll_extent_without_relayout() {
        let mut dom = ScriptedDom::from_serialized_document(
            "<html><body><div id=scroller><div id=content><div id=spacer></div><table id=table><caption id=sticky-caption>sticky</caption><tbody><tr><td id=tail-cell></td></tr></tbody></table></div></div></body></html>",
        );
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut document = LiveryDocument::new(
            dom,
            StyleSet::cambium(&[
                "html, body { margin: 0; padding: 0; } \
                 #scroller { height: 80px; overflow-y: auto; } \
                 #spacer { height: 120px; } \
                 table { display: table; table-layout: fixed; width: 100px; border-spacing: 0; } \
                 caption { display: table-caption; } \
                 tbody { display: table-row-group; } tr { display: table-row; } \
                 td { display: table-cell; width: 100px; } \
                 #sticky-caption { position: sticky; top: 0; height: 20px; background: red; } \
                 #tail-cell { height: 180px; background: blue; }",
            ]),
            Device::screen(160.0, 120.0),
        );
        document.frame(160, 120).expect("initial table frame");
        let scroller = by_id(document.dom(), "scroller");
        let table = by_id(document.dom(), "table");
        let caption = by_id(document.dom(), "sticky-caption");
        let caption_ids_before = generated_ids(&document, caption);
        let table_wrapper_before = table_wrapper_fragment_id(&document, table);
        let static_caption = document
            .layout
            .as_ref()
            .and_then(|layout| layout.fragments.get(caption))
            .map(|fragment| fragment.physical_rect())
            .expect("static table caption fragment");
        assert_eq!(static_caption.y, 120.0);

        assert!(document.scroll_at(10.0, 10.0, 0.0, 150.0));
        document.frame(160, 120).expect("scrolled table frame");

        let active = document.sticky_layout(document.layout.as_ref().expect("retained layout"));
        let caption_rect = active
            .get(caption)
            .map(|fragment| fragment.physical_rect())
            .expect("active table caption fragment");
        assert_eq!(caption_rect.y, 150.0);
        assert_eq!(caption_rect.y - document.element_scroll()[&scroller].1, 0.0);
        assert_eq!(generated_ids(&document, caption), caption_ids_before);
        assert_eq!(table_wrapper_fragment_id(&document, table), table_wrapper_before);
        assert_eq!(
            document
                .layout
                .as_ref()
                .and_then(|layout| layout.fragments.get(caption))
                .map(|fragment| fragment.physical_rect()),
            Some(static_caption),
            "scrolling keeps the retained caption base layout unchanged",
        );
    }

    #[test]
    fn retained_disjoint_formatting_roots_publish_atomically() {
        let initial = "<html><body><div id=first><div id=first-child>one</div></div><div id=second><div id=second-child>two</div></div><div id=outside>outside</div></body></html>";
        let final_document = "<html><body><div id=first style=\"width: 160px\"><div id=first-child>one</div></div><div id=second style=\"width: 180px\"><div id=second-child>two</div></div><div id=outside>outside</div></body></html>";
        let styles = || {
            StyleSet::cambium(&[
                "html, body { margin: 0; padding: 0; } \
                 #first, #second { display: flex; width: 100px; height: 30px; background: red; } \
                 #first-child, #second-child { width: 40px; height: 20px; background: blue; } \
                 #outside { width: 80px; height: 20px; background: green; }",
            ])
        };
        let mut dom = ScriptedDom::from_serialized_document(initial);
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut retained = LiveryDocument::new(dom, styles(), Device::screen(240.0, 160.0));
        retained.frame(240, 160).expect("initial retained frame");
        let first = by_id(retained.dom(), "first");
        let first_child = by_id(retained.dom(), "first-child");
        let second = by_id(retained.dom(), "second");
        let second_child = by_id(retained.dom(), "second-child");
        let outside = by_id(retained.dom(), "outside");
        let first_before = generated_ids(&retained, first);
        let first_child_before = generated_ids(&retained, first_child);
        let second_before = generated_ids(&retained, second);
        let second_child_before = generated_ids(&retained, second_child);
        let outside_before = generated_ids(&retained, outside);

        retained.mutate_dom(|dom| {
            dom.set_attribute(by_id(dom, "first"), attr("style"), "width: 160px");
            dom.set_attribute(by_id(dom, "second"), attr("style"), "width: 180px");
        });
        assert_eq!(
            retained.last_layout_damage(),
            Some(&LayoutDamage {
                kind: LayoutDamageKind::Dom,
                roots: vec![first, second],
                full_document: false,
            })
        );
        let retained_paint = retained.frame(240, 160).expect("spliced retained frame");

        assert_eq!(generated_ids(&retained, first), first_before);
        assert_ne!(generated_ids(&retained, first_child), first_child_before);
        assert_eq!(generated_ids(&retained, second), second_before);
        assert_ne!(generated_ids(&retained, second_child), second_child_before);
        assert_eq!(generated_ids(&retained, outside), outside_before);

        let mut fresh_dom = ScriptedDom::from_serialized_document(final_document);
        let mut fresh_mutations = Vec::new();
        fresh_dom.drain_mutations(&mut fresh_mutations);
        let mut fresh = LiveryDocument::new(fresh_dom, styles(), Device::screen(240.0, 160.0));
        let fresh_paint = fresh.frame(240, 160).expect("fresh final frame");
        assert_eq!(
            format!("{:?}", retained_paint.commands()),
            format!("{:?}", fresh_paint.commands()),
            "every spliced root must publish as one fresh final document",
        );
        assert_eq!(retained.content_height(0), fresh.content_height(0));
    }

    #[test]
    fn retained_root_splice_keeps_an_unrelated_table_paint_plane_live() {
        let initial = "<html><body><div id=flex><div id=child>child</div></div><table id=table><tbody><tr><td>cell</td></tr></tbody></table></body></html>";
        let final_document = "<html><body><div id=flex style=\"width: 180px\"><div id=child>child</div></div><table id=table><tbody><tr><td>cell</td></tr></tbody></table></body></html>";
        let styles = || {
            StyleSet::cambium(&[
                "html, body { margin: 0; padding: 0; } \
                 #flex { display: flex; width: 100px; height: 40px; background: red; } \
                 #child { width: 40px; height: 20px; background: blue; } \
                 table { display: table; border-spacing: 0; background: green; } \
                 tbody { display: table-row-group; } tr { display: table-row; } \
                 td { display: table-cell; width: 60px; height: 20px; background: yellow; }",
            ])
        };
        let mut dom = ScriptedDom::from_serialized_document(initial);
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut retained = LiveryDocument::new(dom, styles(), Device::screen(240.0, 160.0));
        retained.frame(240, 160).expect("initial retained frame");
        let flex = by_id(retained.dom(), "flex");
        let child = by_id(retained.dom(), "child");
        let table = by_id(retained.dom(), "table");
        let flex_before = generated_ids(&retained, flex);
        let child_before = generated_ids(&retained, child);
        let table_before = generated_ids(&retained, table);
        assert_table_paint_sources_are_live(&retained, table);

        retained.mutate_dom(|dom| {
            dom.set_attribute(by_id(dom, "flex"), attr("style"), "width: 180px");
        });
        let retained_paint = retained.frame(240, 160).expect("spliced retained frame");

        assert_eq!(generated_ids(&retained, flex), flex_before);
        assert_ne!(generated_ids(&retained, child), child_before);
        assert_eq!(generated_ids(&retained, table), table_before);
        assert_table_paint_sources_are_live(&retained, table);

        let mut fresh_dom = ScriptedDom::from_serialized_document(final_document);
        let mut fresh_mutations = Vec::new();
        fresh_dom.drain_mutations(&mut fresh_mutations);
        let mut fresh = LiveryDocument::new(fresh_dom, styles(), Device::screen(240.0, 160.0));
        let fresh_paint = fresh.frame(240, 160).expect("fresh final frame");
        assert_eq!(
            format!("{:?}", retained_paint.commands()),
            format!("{:?}", fresh_paint.commands()),
            "fresh table paint must agree with the retained fragment tree",
        );
        assert_eq!(retained.content_height(0), fresh.content_height(0));
    }

    #[test]
    fn background_color_mutation_repaints_without_a_geometry_pass() {
        let initial = "<html><body><div id=target>target</div></body></html>";
        let final_document =
            "<html><body><div id=target style=\"background-color: blue\">target</div></body></html>";
        let styles = || {
            StyleSet::cambium(&[
                "html, body { margin: 0; padding: 0; } #target { width: 100px; height: 20px; }",
            ])
        };
        let mut dom = ScriptedDom::from_serialized_document(initial);
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut retained = LiveryDocument::new(dom, styles(), Device::screen(160.0, 120.0));
        retained.frame(160, 120).expect("initial frame");
        let target = by_id(retained.dom(), "target");
        let ids_before = generated_ids(&retained, target);
        let layout_generation = retained.layout_generation();

        retained.mutate_dom(|dom| {
            dom.set_attribute(
                by_id(dom, "target"),
                attr("style"),
                "background-color: blue",
            );
        });
        let retained_paint = retained.frame(160, 120).expect("repainted frame");

        assert_eq!(retained.layout_generation(), layout_generation);
        assert_eq!(generated_ids(&retained, target), ids_before);
        assert!(
            retained.identity_source.is_none() && !retained.layout_dirty,
            "the retained geometry was repainted directly rather than rebuilt",
        );

        let mut fresh_dom = ScriptedDom::from_serialized_document(final_document);
        let mut fresh_mutations = Vec::new();
        fresh_dom.drain_mutations(&mut fresh_mutations);
        let mut fresh = LiveryDocument::new(fresh_dom, styles(), Device::screen(160.0, 120.0));
        let fresh_paint = fresh.frame(160, 120).expect("fresh final frame");
        assert_eq!(
            format!("{:?}", retained_paint.commands()),
            format!("{:?}", fresh_paint.commands()),
            "paint-only reuse must match a fresh final-document layout",
        );
    }

    #[test]
    fn positioned_inset_mutation_reuses_a_stable_fragment_subtree() {
        let initial = "<html><body><div id=containing><div id=positioned>target</div></div><div id=outside>outside</div></body></html>";
        let final_document = "<html><body><div id=containing><div id=positioned style=\"left: 70px\">target</div></div><div id=outside>outside</div></body></html>";
        let styles = || {
            StyleSet::cambium(&[
                "html, body { margin: 0; padding: 0; } \
                 #containing { position: relative; width: 200px; height: 60px; } \
                 #positioned { position: absolute; left: 10px; top: 5px; width: 40px; height: 20px; } \
                 #outside { width: 80px; height: 20px; }",
            ])
        };
        let mut dom = ScriptedDom::from_serialized_document(initial);
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut retained = LiveryDocument::new(dom, styles(), Device::screen(240.0, 120.0));
        retained.frame(240, 120).expect("initial retained frame");
        let positioned = by_id(retained.dom(), "positioned");
        let outside = by_id(retained.dom(), "outside");
        let positioned_ids = generated_ids(&retained, positioned);
        let outside_ids = generated_ids(&retained, outside);
        let layout_generation = retained.layout_generation();

        retained.mutate_dom(|dom| {
            dom.set_attribute(
                by_id(dom, "positioned"),
                attr("style"),
                "left: 70px",
            );
        });
        let retained_paint = retained.frame(240, 120).expect("repositioned frame");

        assert_eq!(retained.layout_generation(), layout_generation + 1);
        assert_eq!(generated_ids(&retained, positioned), positioned_ids);
        assert_eq!(generated_ids(&retained, outside), outside_ids);
        let rect = retained
            .layout
            .as_ref()
            .and_then(|layout| layout.fragments.get(positioned))
            .map(|fragment| fragment.physical_rect())
            .expect("repositioned fragment");
        assert_eq!((rect.x, rect.y, rect.width, rect.height), (70.0, 5.0, 40.0, 20.0));
        assert!(
            retained.identity_source.is_none() && !retained.layout_dirty,
            "the positioned fragment subtree was translated without a fresh layout",
        );

        let mut fresh_dom = ScriptedDom::from_serialized_document(final_document);
        let mut fresh_mutations = Vec::new();
        fresh_dom.drain_mutations(&mut fresh_mutations);
        let mut fresh = LiveryDocument::new(fresh_dom, styles(), Device::screen(240.0, 120.0));
        let fresh_paint = fresh.frame(240, 120).expect("fresh final frame");
        assert_eq!(
            format!("{:?}", retained_paint.commands()),
            format!("{:?}", fresh_paint.commands()),
            "the retained positioned result must match a fresh final-document layout",
        );
        assert_eq!(retained.content_height(0), fresh.content_height(0));
    }

    #[test]
    fn positioned_inset_reuse_updates_nested_scroll_range() {
        let initial = "<html><body><div id=scroller><div id=positioned>out of flow</div></div></body></html>";
        let final_document = "<html><body><div id=scroller><div id=positioned style=\"top: 300px\">out of flow</div></div></body></html>";
        let styles = || {
            StyleSet::cambium(&[
                "html, body, div { margin: 0; padding: 0; } \
                 #scroller { position: relative; width: 100px; height: 80px; overflow-y: auto; } \
                 #positioned { position: absolute; left: 0; top: 200px; width: 100px; height: 20px; }",
            ])
        };
        let mut dom = ScriptedDom::from_serialized_document(initial);
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut retained = LiveryDocument::new(dom, styles(), Device::screen(160.0, 120.0));
        retained.frame(160, 120).expect("initial frame");
        let positioned = by_id(retained.dom(), "positioned");
        let scroller = by_id(retained.dom(), "scroller");
        let positioned_ids = generated_ids(&retained, positioned);
        let layout_generation = retained.layout_generation();

        retained.mutate_dom(|dom| {
            dom.set_attribute(
                by_id(dom, "positioned"),
                attr("style"),
                "top: 300px",
            );
        });
        retained.frame(160, 120).expect("repositioned frame");

        assert_eq!(retained.layout_generation(), layout_generation + 1);
        assert_eq!(generated_ids(&retained, positioned), positioned_ids);
        let layout = retained.layout.as_ref().expect("repositioned layout");
        assert_eq!(retained.scroll_extent(layout, scroller), (0.0, 240.0));

        let mut fresh_dom = ScriptedDom::from_serialized_document(final_document);
        let mut fresh_mutations = Vec::new();
        fresh_dom.drain_mutations(&mut fresh_mutations);
        let mut fresh = LiveryDocument::new(fresh_dom, styles(), Device::screen(160.0, 120.0));
        fresh.frame(160, 120).expect("fresh final frame");
        let fresh_scroller = by_id(fresh.dom(), "scroller");
        let fresh_layout = fresh.layout.as_ref().expect("fresh final layout");
        assert_eq!(
            retained.scroll_extent(layout, scroller),
            fresh.scroll_extent(fresh_layout, fresh_scroller),
            "retained repositioning must keep nested scrolling equal to a fresh final layout",
        );
    }

    #[test]
    fn positioned_leaf_geometry_mutation_resizes_the_retained_fragment() {
        let initial = "<html><body><div id=containing><canvas id=positioned width=\"80\" height=\"40\"></canvas></div><div id=outside>outside</div></body></html>";
        let final_document = "<html><body><div id=containing><canvas id=positioned width=\"80\" height=\"40\" style=\"left: 70px; width: 120px; height: 60px\"></canvas></div><div id=outside>outside</div></body></html>";
        let styles = || {
            StyleSet::cambium(&[
                "html, body { margin: 0; padding: 0; } \
                 #containing { position: relative; width: 200px; height: 80px; } \
                 #positioned { position: absolute; left: 10px; top: 5px; } \
                 #outside { width: 80px; height: 20px; }",
            ])
        };
        let mut dom = ScriptedDom::from_serialized_document(initial);
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut retained = LiveryDocument::new(dom, styles(), Device::screen(240.0, 120.0));
        retained.frame(240, 120).expect("initial retained frame");
        let positioned = by_id(retained.dom(), "positioned");
        let outside = by_id(retained.dom(), "outside");
        let positioned_ids = generated_ids(&retained, positioned);
        let outside_ids = generated_ids(&retained, outside);
        let layout_generation = retained.layout_generation();

        retained.mutate_dom(|dom| {
            dom.set_attribute(
                by_id(dom, "positioned"),
                attr("style"),
                "left: 70px; width: 120px; height: 60px",
            );
        });
        let retained_paint = retained.frame(240, 120).expect("resized frame");

        assert_eq!(retained.layout_generation(), layout_generation + 1);
        assert_eq!(generated_ids(&retained, positioned), positioned_ids);
        assert_eq!(generated_ids(&retained, outside), outside_ids);
        let rect = retained
            .layout
            .as_ref()
            .and_then(|layout| layout.fragments.get(positioned))
            .map(|fragment| fragment.physical_rect())
            .expect("resized fragment");
        assert_eq!((rect.x, rect.y, rect.width, rect.height), (70.0, 5.0, 120.0, 60.0));

        let mut fresh_dom = ScriptedDom::from_serialized_document(final_document);
        let mut fresh_mutations = Vec::new();
        fresh_dom.drain_mutations(&mut fresh_mutations);
        let mut fresh = LiveryDocument::new(fresh_dom, styles(), Device::screen(240.0, 120.0));
        let fresh_paint = fresh.frame(240, 120).expect("fresh final frame");
        assert_eq!(
            format!("{:?}", retained_paint.commands()),
            format!("{:?}", fresh_paint.commands()),
            "retained leaf resize must match a fresh final-document layout",
        );
        assert_eq!(retained.content_height(0), fresh.content_height(0));
    }

    #[test]
    fn positioned_leaf_resize_updates_nested_scroll_range() {
        let initial = "<html><body><div id=scroller><canvas id=positioned width=\"100\" height=\"20\"></canvas></div></body></html>";
        let final_document = "<html><body><div id=scroller><canvas id=positioned width=\"100\" height=\"20\" style=\"top: 200px; width: 120px; height: 60px\"></canvas></div></body></html>";
        let styles = || {
            StyleSet::cambium(&[
                "html, body, div { margin: 0; padding: 0; } \
                 #scroller { position: relative; width: 100px; height: 80px; overflow-y: auto; } \
                 #positioned { position: absolute; left: 0; top: 100px; }",
            ])
        };
        let mut dom = ScriptedDom::from_serialized_document(initial);
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut retained = LiveryDocument::new(dom, styles(), Device::screen(160.0, 120.0));
        retained.frame(160, 120).expect("initial frame");
        let positioned = by_id(retained.dom(), "positioned");
        let scroller = by_id(retained.dom(), "scroller");
        let positioned_ids = generated_ids(&retained, positioned);
        let layout_generation = retained.layout_generation();

        retained.mutate_dom(|dom| {
            dom.set_attribute(
                by_id(dom, "positioned"),
                attr("style"),
                "top: 200px; width: 120px; height: 60px",
            );
        });
        retained.frame(160, 120).expect("resized frame");

        assert_eq!(retained.layout_generation(), layout_generation + 1);
        assert_eq!(generated_ids(&retained, positioned), positioned_ids);
        let layout = retained.layout.as_ref().expect("resized retained layout");
        assert_eq!(retained.scroll_extent(layout, scroller), (20.0, 180.0));

        let mut fresh_dom = ScriptedDom::from_serialized_document(final_document);
        let mut fresh_mutations = Vec::new();
        fresh_dom.drain_mutations(&mut fresh_mutations);
        let mut fresh = LiveryDocument::new(fresh_dom, styles(), Device::screen(160.0, 120.0));
        fresh.frame(160, 120).expect("fresh final frame");
        let fresh_scroller = by_id(fresh.dom(), "scroller");
        let fresh_layout = fresh.layout.as_ref().expect("fresh final layout");
        assert_eq!(
            retained.scroll_extent(layout, scroller),
            fresh.scroll_extent(fresh_layout, fresh_scroller),
            "retained leaf resize must keep nested scrolling equal to a fresh final layout",
        );
    }

    #[test]
    fn geometry_mutation_rejects_the_paint_only_reuse_path() {
        let mut dom = ScriptedDom::from_serialized_document(
            "<html><body><div id=target>target</div></body></html>",
        );
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut document = LiveryDocument::new(
            dom,
            StyleSet::cambium(&[
                "html, body { margin: 0; padding: 0; } #target { width: 100px; height: 20px; }",
            ]),
            Device::screen(160.0, 120.0),
        );
        document.frame(160, 120).expect("initial frame");
        let layout_generation = document.layout_generation();

        document.mutate_dom(|dom| {
            let target = by_id(dom, "target");
            dom.set_attribute(target, attr("style"), "width: 120px");
        });
        document.frame(160, 120).expect("resized frame");

        assert_eq!(document.layout_generation(), layout_generation + 1);
        assert_eq!(
            document
                .layout
                .as_ref()
                .and_then(|layout| layout.fragments.get(by_id(document.dom(), "target")))
                .map(|fragment| fragment.width),
            Some(120.0),
            "a geometry change stays on the full K5g reconciliation path",
        );
    }

    #[test]
    fn retained_document_uses_intrinsic_positioned_width_between_insets() {
        let mut dom = ScriptedDom::from_serialized_document(
            "<html><body><div id=containing><div id=positioned>MMMM MMMM MMMM MMMM MMMM MMMM MMMM MMMM</div></div></body></html>",
        );
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut document = LiveryDocument::new(
            dom,
            StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; } \
                 #containing { position: relative; width: 200px; } \
                 #positioned { position: absolute; left: 10px; right: 20px; }"]),
            Device::screen(320.0, 240.0),
        );

        document.frame(320, 240).expect("positioned frame");
        let positioned = by_id(document.dom(), "positioned");
        let rect = document
            .layout
            .as_ref()
            .and_then(|layout| layout.fragments.get(positioned))
            .map(|fragment| fragment.physical_rect())
            .expect("positioned fragment");

        assert_eq!((rect.x, rect.width), (10.0, 170.0));
        assert!(
            rect.height > 20.0,
            "the second formatter pass rewraps content at Buckram's used width"
        );
    }

    #[test]
    fn sticky_scrolls_its_retained_fragment_without_relayout() {
        let mut dom = ScriptedDom::from_serialized_document(
            "<html><body><div id=spacer></div><div id=sticky>sticky</div><div id=tail></div></body></html>",
        );
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut document = LiveryDocument::new(
            dom,
            StyleSet::cambium(&[
                "html, body { margin: 0; padding: 0; } #spacer { height: 120px; } \
                 #sticky { position: sticky; top: 0; height: 20px; } #tail { height: 180px; }",
            ]),
            Device::screen(160.0, 80.0),
        );
        document.frame(160, 80).expect("initial sticky frame");
        let sticky = by_id(document.dom(), "sticky");
        let before_ids = generated_ids(&document, sticky);
        let static_rect = document
            .layout
            .as_ref()
            .and_then(|layout| layout.fragments.get(sticky))
            .map(|fragment| fragment.physical_rect())
            .expect("static sticky fragment");
        assert_eq!(static_rect.y, 120.0);

        assert!(document.scroll_by(0.0, 150.0));
        document.frame(160, 80).expect("scrolled sticky frame");

        let active = document.sticky_layout(document.layout.as_ref().expect("retained layout"));
        let sticky_rect = active
            .get(sticky)
            .map(|fragment| fragment.physical_rect())
            .expect("active sticky fragment");
        assert_eq!(sticky_rect.y, 150.0);
        assert_eq!(sticky_rect.y - document.scroll().1, 0.0);
        assert_eq!(generated_ids(&document, sticky), before_ids);
        assert_eq!(
            document
                .layout
                .as_ref()
                .and_then(|layout| layout.fragments.get(sticky))
                .map(|fragment| fragment.physical_rect()),
            Some(static_rect),
            "scrolling never mutates the retained normal-flow base layout",
        );
        assert!(
            !document.layout_dirty,
            "scroll repaint did not trigger relayout"
        );
    }

    #[test]
    fn sticky_uses_the_nearest_nested_scrollport_offset() {
        let mut dom = ScriptedDom::from_serialized_document(
            "<html><body><div id=scroller><div id=content><div id=spacer></div><div id=sticky>sticky</div><div id=tail></div></div></div></body></html>",
        );
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut document = LiveryDocument::new(
            dom,
            StyleSet::cambium(&[
                "html, body { margin: 0; padding: 0; } #scroller { height: 80px; overflow-y: auto; } \
                 #spacer { height: 120px; } #sticky { position: sticky; top: 0; height: 20px; } \
                 #tail { height: 180px; }",
            ]),
            Device::screen(160.0, 120.0),
        );
        document
            .frame(160, 120)
            .expect("initial nested sticky frame");
        let scroller = by_id(document.dom(), "scroller");
        let sticky = by_id(document.dom(), "sticky");

        assert!(document.scroll_at(10.0, 10.0, 0.0, 150.0));
        assert_eq!(
            document.element_scroll().get(&scroller),
            Some(&(0.0, 150.0))
        );
        document.frame(160, 120).expect("nested sticky frame");

        let active = document.sticky_layout(document.layout.as_ref().expect("retained layout"));
        let sticky_rect = active
            .get(sticky)
            .map(|fragment| fragment.physical_rect())
            .expect("active nested sticky fragment");
        assert_eq!(sticky_rect.y, 150.0);
        assert_eq!(sticky_rect.y - document.element_scroll()[&scroller].1, 0.0);
    }

    #[test]
    fn positioned_descendant_extends_its_scroll_container_range() {
        let mut dom = ScriptedDom::from_serialized_document(
            "<html><body><div id=scroller><div id=positioned>out of flow</div></div></body></html>",
        );
        let mut initial_mutations = Vec::new();
        dom.drain_mutations(&mut initial_mutations);
        let mut document = LiveryDocument::new(
            dom,
            StyleSet::cambium(&[
                "html, body { margin: 0; padding: 0; } \
                 #scroller { position: relative; width: 100px; height: 80px; overflow-y: auto; } \
                 #positioned { position: absolute; top: 200px; width: 100px; height: 20px; }",
            ]),
            Device::screen(160.0, 120.0),
        );
        document.frame(160, 120).expect("positioned overflow frame");
        let scroller = by_id(document.dom(), "scroller");
        let layout = document.layout.as_ref().expect("retained layout");
        assert_eq!(document.scroll_extent(layout, scroller), (0.0, 140.0));

        assert!(document.scroll_at(10.0, 10.0, 0.0, 200.0));
        assert_eq!(
            document.element_scroll().get(&scroller),
            Some(&(0.0, 140.0)),
            "the positioned fragment contributes to the container's scrollable overflow"
        );
    }
}
