//! Pointer, keyboard, IME, and wheel routing. Extracted from the
//! woodshed-genet donor, then completed against Cambium's full native event
//! vocabulary (drag capture, wheel handlers, routed Tab).
//!
//! The routing rule everywhere below is the browser's: **the view layer sees
//! the event first, the host default runs second, and a handler that calls
//! `prevent_default` cancels the host default**. That is why Tab goes through
//! [`dispatch_key`](cambium::GenetAppRunner::dispatch_key) rather than straight
//! to `focus_traverse`, and why click-to-caret and wheel scrolling are gated on
//! [`default_prevented`](cambium::GenetAppRunner::default_prevented).

use cambium::{
    CaretPosition, CaretSelection, PointerClick, PointerEvent, PointerPhase, TextCommand,
    WheelEvent,
};
use cambium_winit::{ime_event_from_winit, key_event_from_winit, modifiers_from_winit, wheel_axes};
use genet_layout::{ScrollOffsets, VisualAffinity, VisualCaret, VisualMovement, VisualSelection};
use genet_scripted_dom::{NodeId, ScriptedDom};
use winit::keyboard::{Key as WinitKey, NamedKey as WinitNamedKey};

use crate::meristem_bounds::RootView;
use crate::{Host, KeyPress};

pub(crate) fn to_visual_caret(caret: CaretPosition) -> VisualCaret {
    VisualCaret {
        byte: caret.byte,
        affinity: match caret.affinity {
            cambium::CaretAffinity::Downstream => VisualAffinity::Downstream,
            cambium::CaretAffinity::Upstream => VisualAffinity::Upstream,
        },
    }
}

pub(crate) fn from_visual_caret(caret: VisualCaret) -> CaretPosition {
    CaretPosition {
        byte: caret.byte,
        affinity: match caret.affinity {
            VisualAffinity::Downstream => cambium::CaretAffinity::Downstream,
            VisualAffinity::Upstream => cambium::CaretAffinity::Upstream,
        },
    }
}

fn to_visual_selection(selection: CaretSelection) -> VisualSelection {
    VisualSelection {
        anchor: to_visual_caret(selection.anchor),
        focus: to_visual_caret(selection.focus),
    }
}

fn from_visual_selection(selection: VisualSelection) -> CaretSelection {
    CaretSelection {
        anchor: from_visual_caret(selection.anchor),
        focus: from_visual_caret(selection.focus),
    }
}

/// Which visual caret movement a key means, or `None` for a key the host has no
/// caret default for.
fn caret_movement(key: &WinitKey, word: bool) -> Option<VisualMovement> {
    let WinitKey::Named(named) = key else {
        return None;
    };
    Some(match named {
        WinitNamedKey::ArrowLeft if word => VisualMovement::PreviousWord,
        WinitNamedKey::ArrowLeft => VisualMovement::PreviousCluster,
        WinitNamedKey::ArrowRight if word => VisualMovement::NextWord,
        WinitNamedKey::ArrowRight => VisualMovement::NextCluster,
        WinitNamedKey::ArrowUp => VisualMovement::PreviousLine,
        WinitNamedKey::ArrowDown => VisualMovement::NextLine,
        WinitNamedKey::Home => VisualMovement::LineStart,
        WinitNamedKey::End => VisualMovement::LineEnd,
        _ => return None,
    })
}

impl<State, Logic, V> Host<State, Logic, V>
where
    State: 'static,
    Logic: FnMut(&State) -> V + 'static,
    V: RootView<State>,
{
    /// The topmost node under the cursor, in the retained layout's own
    /// coordinates. The one hit-test every pointer path goes through.
    pub(crate) fn hit_at_cursor(&self) -> Option<NodeId> {
        let runner = self.s.runner.as_ref()?;
        let layout = self.s.layout.as_ref()?;
        let (x, y) = self.s.cursor;
        let dom = runner.dom();
        let dom_ref = dom.borrow();
        layout.hit_test(&*dom_ref, x, y, &ScrollOffsets::default())
    }

    /// The cursor in `node`'s **local** coordinate space, plus `node`'s box
    /// size — the pair a Cambium pointer or wheel handler normalizes with
    /// (`local.0 / size.0` is a slider's value) without knowing layout.
    ///
    /// Read from the *painted* rect, so an element inside a scrolled container
    /// reports where it actually is rather than where it would be unscrolled.
    /// `((0, 0), (0, 0))` when the node has no laid-out box.
    fn local_in(&self, node: NodeId) -> ((f32, f32), (f32, f32)) {
        let rect = self.s.runner.as_ref().zip(self.s.layout.as_ref()).and_then(
            |(runner, layout)| {
                let dom = runner.dom();
                let dom_ref = dom.borrow();
                layout.painted_rect(&*dom_ref, node)
            },
        );
        match rect {
            Some((x, y, w, h)) => ((self.s.cursor.0 - x, self.s.cursor.1 - y), (w, h)),
            None => ((0.0, 0.0), (0.0, 0.0)),
        }
    }

    /// A left-button press in the content area: click, then drag capture, then
    /// the host's own click-to-caret default.
    pub(crate) fn click(&mut self) {
        self.s.text_drag = None;
        let Some(node) = self.hit_at_cursor() else {
            return;
        };
        // 1. The click, carrying the real hit point in the target's space.
        let (local, _) = self.local_in(node);
        let mut prevented = {
            let Some(runner) = self.s.runner.as_mut() else {
                return;
            };
            runner.dispatch_click(node, PointerClick::at(local));
            runner.default_prevented()
        };
        // 2. Begin a drag when the press lands under an `on_pointer` element.
        //    `pointer_target` resolves the element that *would* capture without
        //    starting the drag, so the coordinates are measured against the
        //    capturing element rather than the deeper node the cursor hit.
        let capture = self
            .s
            .runner
            .as_ref()
            .and_then(|runner| runner.pointer_target(node));
        if let Some(target) = capture {
            let (local, size) = self.local_in(target);
            if let Some(runner) = self.s.runner.as_mut() {
                runner.dispatch_pointer_down(
                    node,
                    PointerEvent::new(PointerPhase::Down, local, size),
                );
                prevented |= runner.default_prevented();
            }
        }
        // 3. Click-to-caret: a host default, so a handler that prevented the
        //    default keeps its own press semantics (a drag handle does not want
        //    the caret moved out from under it).
        if !prevented {
            self.place_caret_at_cursor();
        }
        self.after_dispatch();
    }

    /// Place the caret at the cursor when the press focused a text field, and
    /// anchor a drag selection there.
    fn place_caret_at_cursor(&mut self) {
        let (Some(runner), Some(layout)) = (self.s.runner.as_mut(), self.s.layout.as_ref()) else {
            return;
        };
        let Some(slot) = (self.hooks.focused_text)(runner) else {
            return;
        };
        let (x, y) = self.s.cursor;
        let caret = {
            let dom = runner.dom();
            let dom = dom.borrow();
            layout.caret_position_at_point(&*dom, slot.node, x, y)
        };
        let Some(caret) = caret else { return };
        runner.update(|state| {
            let input = (slot.get_mut)(state);
            let position = from_visual_caret(caret);
            input.apply(TextCommand::SetSelection(CaretSelection {
                anchor: position,
                focus: position,
            }));
        });
        self.s.text_drag = Some(slot.node);
    }

    /// A pointer move while a drag is captured. Routed to the capturing
    /// element even when the cursor has left its box — that is what capture
    /// means, and it is why the coordinates come from the captured element's
    /// rect rather than the hit test.
    pub(crate) fn pointer_move(&mut self) {
        let Some(target) = self
            .s
            .runner
            .as_ref()
            .and_then(|runner| runner.pointer_capture())
        else {
            return;
        };
        let (local, size) = self.local_in(target);
        if let Some(runner) = self.s.runner.as_mut() {
            runner.dispatch_pointer_move(PointerEvent::new(PointerPhase::Move, local, size));
        }
        self.after_dispatch();
    }

    /// A left-button release: end any captured drag, then the text-drag anchor.
    pub(crate) fn release(&mut self) {
        let capture = self
            .s
            .runner
            .as_ref()
            .and_then(|runner| runner.pointer_capture());
        if let Some(target) = capture {
            let (local, size) = self.local_in(target);
            if let Some(runner) = self.s.runner.as_mut() {
                runner.dispatch_pointer_up(PointerEvent::new(PointerPhase::Up, local, size));
            }
            self.after_dispatch();
        }
        self.s.text_drag = None;
    }

    pub(crate) fn drag_text_selection(&mut self) {
        let Some(drag_node) = self.s.text_drag else {
            return;
        };
        let (Some(runner), Some(layout)) = (self.s.runner.as_mut(), self.s.layout.as_ref()) else {
            return;
        };
        let Some(slot) = (self.hooks.focused_text)(runner) else {
            self.s.text_drag = None;
            return;
        };
        if slot.node != drag_node {
            self.s.text_drag = None;
            return;
        }
        let (x, y) = self.s.cursor;
        let caret = {
            let dom = runner.dom();
            let dom = dom.borrow();
            layout.caret_position_at_point(&*dom, slot.node, x, y)
        };
        let Some(caret) = caret else {
            return;
        };
        runner.update(|state| {
            let input = (slot.get_mut)(state);
            let anchor = input.caret_selection().anchor;
            input.apply(TextCommand::SetSelection(CaretSelection {
                anchor,
                focus: from_visual_caret(caret),
            }));
        });
        self.after_dispatch();
    }

    /// The focused text field's node and selection, before dispatch.
    fn focused_text_selection(&self) -> Option<(NodeId, CaretSelection)> {
        let runner = self.s.runner.as_ref()?;
        let slot = (self.hooks.focused_text)(runner)?;
        Some((slot.node, (slot.get)(runner.state()).caret_selection()))
    }

    /// The host's caret default: move the focused field's selection **visually**
    /// — through the laid-out lines, so wrapped and bidi text move the way they
    /// look — starting from `base`, the selection as it stood before dispatch.
    ///
    /// The field's own key handler has already applied its *logical* move by
    /// the time this runs (arrow keys route like any other key now), so this
    /// deliberately recomputes from `base` and overwrites it. The layout-aware
    /// answer wins; the field keeps working unchanged in a host that has no
    /// layout to consult.
    fn move_focused_text_visual(
        &mut self,
        press: &KeyPress,
        base: Option<(NodeId, CaretSelection)>,
    ) {
        let word = press.modifiers.control_key() || press.modifiers.alt_key();
        let Some(movement) = caret_movement(&press.key, word) else {
            return;
        };
        let Some((base_node, base_selection)) = base else {
            return;
        };
        let (Some(runner), Some(layout)) = (self.s.runner.as_mut(), self.s.layout.as_ref()) else {
            return;
        };
        let Some(slot) = (self.hooks.focused_text)(runner) else {
            return;
        };
        // Focus moved during dispatch: there is no caret context to continue.
        if slot.node != base_node {
            return;
        }
        let Some(moved) = layout.selection_visual_move::<ScriptedDom>(
            slot.node,
            to_visual_selection(base_selection),
            movement,
            press.modifiers.shift_key(),
        ) else {
            return;
        };
        runner.update(|state| {
            (slot.get_mut)(state).apply(TextCommand::SetSelection(from_visual_selection(moved)));
        });
    }

    pub(crate) fn key(&mut self, press: &KeyPress) {
        // The application's own policy first (an Escape that closes a popover, a
        // global shortcut): it can consume the event before the tree sees it.
        {
            let Some(runner) = self.s.runner.as_mut() else {
                return;
            };
            if (self.hooks.key_intercept)(runner, press) {
                self.after_dispatch();
                return;
            }
        }
        let mods = modifiers_from_winit(press.modifiers);
        let Some(kev) = key_event_from_winit(&press.key, mods) else {
            return;
        };
        // Snapshot the caret before dispatch: the host's visual movement below
        // recomputes from this rather than from whatever logical move the field
        // just applied.
        let base = self.focused_text_selection();
        let prevented = {
            let Some(runner) = self.s.runner.as_mut() else {
                return;
            };
            // Tab included: the runner routes it to the focused element's
            // handlers and applies its own traversal default only if none of
            // them prevented it.
            runner.dispatch_key(kev);
            runner.default_prevented()
        };
        if !prevented {
            self.move_focused_text_visual(press, base);
        }
        self.after_dispatch();
        // Focus may have moved (Tab, or a handler's `set_focus`): refresh the
        // `:hover` / `:focus` restyle. Cheap — it returns early unless the
        // hovered/focused pair actually changed.
        self.hover();
    }

    pub(crate) fn ime(&mut self, ime: &winit::event::Ime) {
        let Some(runner) = self.s.runner.as_mut() else {
            return;
        };
        if (self.hooks.focused_text)(runner).is_none() {
            return;
        }
        runner.dispatch_key(ime_event_from_winit(ime));
        self.after_dispatch();
    }

    pub(crate) fn wheel(&mut self, delta: winit::event::MouseScrollDelta) {
        // Desktop convention (shared cambium-winit policy): Shift + vertical
        // wheel scrolls horizontally.
        let (dx, dy) = genet_winit_host::wheel_delta_from_winit(delta);
        let (dx, dy) = wheel_axes(dx, dy, self.s.modifiers.shift_key());
        let hit = self.hit_at_cursor();
        // 1. The view layer first: the nearest `on_wheel` ancestor of the hit
        //    node gets the notch, with cursor-local coordinates so it can
        //    anchor a zoom under the pointer.
        let mut prevented = false;
        if let Some(node) = hit {
            let target = self
                .s
                .runner
                .as_ref()
                .and_then(|runner| runner.wheel_target(node));
            if let Some(target) = target {
                let (local, size) = self.local_in(target);
                if let Some(runner) = self.s.runner.as_mut() {
                    runner.dispatch_wheel(node, WheelEvent::new((dx, dy), local, size));
                    prevented = runner.default_prevented();
                }
                self.after_dispatch();
            }
        }
        // 2. The host default: scroll the nearest overflow container under the
        //    cursor (the engine hit-tests, clamps, and chains). Suppressed when
        //    a handler consumed the notch, so a canvas that pans on the wheel
        //    does not also scroll the page behind it.
        if prevented {
            return;
        }
        let (x, y) = self.s.cursor;
        let scrolled = if let (Some(runner), Some(layout)) =
            (self.s.runner.as_ref(), self.s.layout.as_mut())
        {
            let dom = runner.dom();
            let dom_ref = dom.borrow();
            layout.scroll_at_target(&*dom_ref, x, y, dx, dy)
        } else {
            None
        };
        if let Some(target) = scrolled {
            // Wake the scrolled target's overlay bar; the fade clock keeps
            // redraws coming until it hides again.
            self.s.scrollbar_fade.note(target, std::time::Instant::now());
            if let Some(window) = self.s.window.as_ref() {
                window.request_redraw();
            }
        }
    }
}
