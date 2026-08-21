/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! A two-axis resize handle with retained pointer-grab state.
//!
//! The caller owns the durable size. The component retains only the grab point
//! while a pointer is captured, reports clamped sizes, and exposes the same
//! operation to the keyboard. This is the two-dimensional sibling of
//! [`crate::split`]: shared interaction furniture around caller-owned geometry.

use crate::component::{ComponentView, component};
use crate::{
    Action, GenetCtx, GenetElement, Key, NamedKey, OptionalAction, PointerEvent, PointerPhase,
    View, el, focusable, on_key, on_pointer,
};

/// Structural styling for [`resize_handle`]. A consumer may override palette,
/// but the inline geometry remains authoritative for layout and hit testing.
pub const RESIZE_HANDLE_CSS: &str = r#"
.resize-handle {
    border: 0;
    bottom: 0;
    cursor: nwse-resize;
    pointer-events: auto;
    position: absolute;
    right: 0;
    touch-action: none;
    user-select: none;
}
.resize-handle:focus-visible {
    outline: 1px solid currentColor;
    outline-offset: 1px;
}
"#;

/// Inclusive pixel bounds for a resizable surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResizeBounds {
    pub min_width: u32,
    pub min_height: u32,
    pub max_width: u32,
    pub max_height: u32,
    /// Keyboard step in pixels.
    pub step: u32,
}

impl ResizeBounds {
    pub fn new(min_width: u32, min_height: u32, max_width: u32, max_height: u32) -> Self {
        Self {
            min_width,
            min_height,
            max_width,
            max_height,
            step: 16,
        }
    }

    #[must_use]
    pub fn with_step(mut self, step: u32) -> Self {
        self.step = step.max(1);
        self
    }

    pub fn clamp(self, width: f32, height: f32) -> (u32, u32) {
        let (min_width, max_width) = ordered(self.min_width, self.max_width);
        let (min_height, max_height) = ordered(self.min_height, self.max_height);
        (
            width.round().clamp(min_width as f32, max_width as f32) as u32,
            height.round().clamp(min_height as f32, max_height as f32) as u32,
        )
    }
}

fn ordered(a: u32, b: u32) -> (u32, u32) {
    (a.min(b), a.max(b))
}

/// A size emitted while dragging or after keyboard resizing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeHandleEvent {
    Resize { width: u32, height: u32 },
    ResizeEnd { width: u32, height: u32 },
}

impl ResizeHandleEvent {
    pub fn size(self) -> (u32, u32) {
        match self {
            Self::Resize { width, height } | Self::ResizeEnd { width, height } => (width, height),
        }
    }
}

impl Action for ResizeHandleEvent {}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ResizeHandleProps {
    width: u32,
    height: u32,
    bounds: ResizeBounds,
    label: String,
}

#[derive(Clone, Debug)]
struct ResizeHandleLocal {
    width: u32,
    height: u32,
    grab: Option<(f32, f32)>,
}

fn resize_handle_body(
    props: &ResizeHandleProps,
    local: &ResizeHandleLocal,
) -> ComponentView<ResizeHandleLocal, ResizeHandleEvent> {
    let pointer_bounds = props.bounds;
    let key_bounds = props.bounds;
    let label = props.label.clone();
    let value = format!("{} by {} pixels", local.width, local.height);
    Box::new(focusable(on_key(
        on_pointer(
            el("div", ())
                .attr("class", "resize-handle")
                .attr("role", "button")
                .attr("aria-label", label)
                .attr("aria-valuetext", value)
                .attr("data-resize-width", local.width.to_string())
                .attr("data-resize-height", local.height.to_string())
                .attr("tabindex", "0")
                .attr(
                    "style",
                    "position:absolute;right:0;bottom:0;width:18px;height:18px;",
                ),
            move |local: &mut ResizeHandleLocal, event: PointerEvent| {
                event.prop.prevent_default();
                match event.phase {
                    PointerPhase::Down => {
                        local.grab = Some(event.local);
                        None
                    },
                    PointerPhase::Move => {
                        let grab = local.grab?;
                        let (width, height) = pointer_bounds.clamp(
                            local.width as f32 + event.local.0 - grab.0,
                            local.height as f32 + event.local.1 - grab.1,
                        );
                        local.width = width;
                        local.height = height;
                        Some(ResizeHandleEvent::Resize { width, height })
                    },
                    PointerPhase::Up => {
                        local.grab = None;
                        Some(ResizeHandleEvent::ResizeEnd {
                            width: local.width,
                            height: local.height,
                        })
                    },
                }
            },
        ),
        move |local: &mut ResizeHandleLocal, event| {
            let step = key_bounds.step.max(1) as f32;
            let next = match &event.key {
                Key::Named(NamedKey::ArrowLeft) => {
                    Some((local.width as f32 - step, local.height as f32))
                },
                Key::Named(NamedKey::ArrowRight) => {
                    Some((local.width as f32 + step, local.height as f32))
                },
                Key::Named(NamedKey::ArrowUp) => {
                    Some((local.width as f32, local.height as f32 - step))
                },
                Key::Named(NamedKey::ArrowDown) => {
                    Some((local.width as f32, local.height as f32 + step))
                },
                Key::Named(NamedKey::Home) => {
                    Some((key_bounds.min_width as f32, key_bounds.min_height as f32))
                },
                Key::Named(NamedKey::End) => {
                    Some((key_bounds.max_width as f32, key_bounds.max_height as f32))
                },
                _ => None,
            };
            next.map(|(width, height)| {
                let (width, height) = key_bounds.clamp(width, height);
                local.width = width;
                local.height = height;
                event.prevent_default();
                ResizeHandleEvent::Resize { width, height }
            })
        },
    )))
}

/// Render a bottom-right resize handle over a caller-sized positioned surface.
///
/// Pointer motion changes both axes. Arrow keys change one axis at a time;
/// Home and End select the minimum and maximum extents. The caller writes the
/// emitted size into its own state and supplies it again on rebuild.
pub fn resize_handle<State, A, Output, F>(
    size: (u32, u32),
    bounds: ResizeBounds,
    label: impl Into<String>,
    on_resize: F,
) -> impl View<State, A, GenetCtx, Element = GenetElement>
where
    State: 'static,
    A: 'static,
    Output: OptionalAction<A> + 'static,
    F: Fn(&mut State, ResizeHandleEvent) -> Output + 'static,
{
    let (width, height) = bounds.clamp(size.0 as f32, size.1 as f32);
    component(
        ResizeHandleProps {
            width,
            height,
            bounds,
            label: label.into(),
        },
        |props: &ResizeHandleProps| ResizeHandleLocal {
            width: props.width,
            height: props.height,
            grab: None,
        },
        |_: &ResizeHandleProps, next: &ResizeHandleProps, local: &mut ResizeHandleLocal| {
            local.width = next.width;
            local.height = next.height;
        },
        resize_handle_body,
        on_resize,
    )
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use genet_scripted_dom::ScriptedDom;
    use layout_dom_api::{LayoutDom, LocalName, Namespace};

    use super::*;
    use crate::{DomHandle, GenetAppRunner, KeyEvent};

    #[derive(Clone, Debug)]
    struct State {
        size: (u32, u32),
        events: Vec<ResizeHandleEvent>,
    }

    fn view(state: &State) -> impl View<State, (), GenetCtx, Element = GenetElement> + use<> {
        resize_handle(
            state.size,
            ResizeBounds::new(360, 240, 960, 720).with_step(20),
            "Resize graph",
            |state: &mut State, event| {
                state.size = event.size();
                state.events.push(event);
            },
        )
    }

    fn attr<'a>(dom: &'a ScriptedDom, node: genet_scripted_dom::NodeId, name: &str) -> &'a str {
        dom.attribute(node, &Namespace::from(""), &LocalName::from(name))
            .expect("attribute")
    }

    #[test]
    fn pointer_drag_and_keyboard_resize_share_clamped_caller_state() {
        let dom: DomHandle = Rc::new(RefCell::new(ScriptedDom::new()));
        let mut runner = GenetAppRunner::<_, _, _, ()>::new(
            dom.clone(),
            view,
            State {
                size: (520, 260),
                events: Vec::new(),
            },
        );
        let root = runner.root();
        runner.dispatch_pointer_down(
            root,
            PointerEvent::new(PointerPhase::Down, (9.0, 9.0), (18.0, 18.0)),
        );
        runner.dispatch_pointer_move(PointerEvent::new(
            PointerPhase::Move,
            (49.0, 29.0),
            (18.0, 18.0),
        ));
        runner.dispatch_pointer_up(PointerEvent::new(
            PointerPhase::Up,
            (9.0, 9.0),
            (18.0, 18.0),
        ));
        assert_eq!(runner.state().size, (560, 280));
        assert_eq!(
            attr(&dom.borrow(), root, "aria-valuetext"),
            "560 by 280 pixels"
        );
        assert_eq!(runner.pointer_capture(), None);

        runner.set_focus(Some(root));
        runner.dispatch_key(KeyEvent::new(Key::Named(NamedKey::ArrowRight)));
        runner.dispatch_key(KeyEvent::new(Key::Named(NamedKey::ArrowUp)));
        assert_eq!(runner.state().size, (580, 260));
        assert_eq!(attr(&dom.borrow(), root, "data-resize-width"), "580");
        assert_eq!(attr(&dom.borrow(), root, "data-resize-height"), "260");

        runner.dispatch_key(KeyEvent::new(Key::Named(NamedKey::End)));
        assert_eq!(runner.state().size, (960, 720));
        runner.dispatch_pointer_down(
            root,
            PointerEvent::new(PointerPhase::Down, (9.0, 9.0), (18.0, 18.0)),
        );
        runner.dispatch_pointer_move(PointerEvent::new(
            PointerPhase::Move,
            (1009.0, 1009.0),
            (18.0, 18.0),
        ));
        assert_eq!(runner.state().size, (960, 720), "pointer motion clamps too");
    }

    #[test]
    fn inverted_bounds_are_ordered_before_clamping() {
        assert_eq!(
            ResizeBounds::new(900, 700, 300, 200).clamp(0.0, 0.0),
            (300, 200)
        );
    }
}
