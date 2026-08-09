//! Pointer, keyboard, IME, and wheel routing. Extracted from the
//! woodshed-genet donor.

use cambium::{
    CaretPosition, CaretSelection, PointerClick, Propagation, TextCommand,
};
use cambium_winit::{ime_event_from_winit, key_event_from_winit, modifiers_from_winit, wheel_axes};
use genet_layout::{ScrollOffsets, VisualAffinity, VisualCaret, VisualMovement, VisualSelection};
use genet_scripted_dom::ScriptedDom;
use winit::event::{ElementState, KeyEvent as WinitKeyEvent};
use winit::keyboard::{Key as WinitKey, NamedKey as WinitNamedKey};

use crate::meristem_bounds::RootView;
use crate::Host;

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

impl<State, Logic, V> Host<State, Logic, V>
where
    State: 'static,
    Logic: FnMut(&State) -> V + 'static,
    V: RootView<State>,
{
    pub(crate) fn click(&mut self) {
        self.s.text_drag = None;
        let (Some(runner), Some(layout)) =
            (self.s.runner.as_mut(), self.s.layout.as_ref())
        else {
            return;
        };
        let (x, y) = self.s.cursor;
        let hit = {
            let dom = runner.dom();
            let dom_ref = dom.borrow();
            layout.hit_test(&*dom_ref, x, y, &ScrollOffsets::default())
        };
        let Some(node) = hit else { return };
        runner.dispatch_click(
            node,
            PointerClick {
                local: (0.0, 0.0),
                prop: Propagation::new(),
            },
        );
        // Click-to-caret: when a text field took focus, place the caret at
        // the press point and anchor a drag selection there.
        if let Some(slot) = (self.hooks.focused_text)(runner) {
            let caret = {
                let dom = runner.dom();
                let dom = dom.borrow();
                layout.caret_position_at_point(&*dom, slot.node, x, y)
            };
            if let Some(caret) = caret {
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
        }
        self.after_dispatch();
    }

    pub(crate) fn drag_text_selection(&mut self) {
        let Some(drag_node) = self.s.text_drag else {
            return;
        };
        let (Some(runner), Some(layout)) =
            (self.s.runner.as_mut(), self.s.layout.as_ref())
        else {
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

    fn move_focused_text_visual(&mut self, event: &WinitKeyEvent) -> bool {
        let WinitKey::Named(named) = &event.logical_key else {
            return false;
        };
        let word = self.s.modifiers.control_key() || self.s.modifiers.alt_key();
        let movement = match named {
            WinitNamedKey::ArrowLeft if word => VisualMovement::PreviousWord,
            WinitNamedKey::ArrowLeft => VisualMovement::PreviousCluster,
            WinitNamedKey::ArrowRight if word => VisualMovement::NextWord,
            WinitNamedKey::ArrowRight => VisualMovement::NextCluster,
            WinitNamedKey::ArrowUp => VisualMovement::PreviousLine,
            WinitNamedKey::ArrowDown => VisualMovement::NextLine,
            WinitNamedKey::Home => VisualMovement::LineStart,
            WinitNamedKey::End => VisualMovement::LineEnd,
            _ => return false,
        };
        let (Some(runner), Some(layout)) =
            (self.s.runner.as_mut(), self.s.layout.as_ref())
        else {
            return false;
        };
        let Some(slot) = (self.hooks.focused_text)(runner) else {
            return false;
        };
        let selection =
            to_visual_selection((slot.get)(runner.state()).caret_selection());
        let Some(moved) = layout.selection_visual_move::<ScriptedDom>(
            slot.node,
            selection,
            movement,
            self.s.modifiers.shift_key(),
        ) else {
            return false;
        };
        runner.update(|state| {
            (slot.get_mut)(state)
                .apply(TextCommand::SetSelection(from_visual_selection(moved)));
        });
        self.after_dispatch();
        true
    }

    pub(crate) fn key(&mut self, event: &WinitKeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }
        if self.move_focused_text_visual(event) {
            return;
        }
        {
            let Some(runner) = self.s.runner.as_mut() else {
                return;
            };
            if let WinitKey::Named(WinitNamedKey::Tab) = event.logical_key {
                runner.focus_traverse(!self.s.modifiers.shift_key());
                self.after_dispatch();
                self.hover(); // focus changed → refresh :focus styling
                return;
            }
            if (self.hooks.key_intercept)(runner, event) {
                self.after_dispatch();
                return;
            }
        }
        let mods = modifiers_from_winit(self.s.modifiers);
        if let Some(kev) = key_event_from_winit(&event.logical_key, mods)
            && self.s.runner.is_some()
        {
            if let Some(runner) = self.s.runner.as_mut() {
                runner.dispatch_key(kev);
            }
            self.after_dispatch();
        }
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
        // Wheel scrolls the nearest overflow container under the cursor (the
        // engine hit-tests, clamps, and chains). Desktop convention (shared
        // cambium-winit policy): Shift + vertical wheel scrolls horizontally.
        let (dx, dy) = genet_winit_host::wheel_delta_from_winit(delta);
        let (dx, dy) = wheel_axes(dx, dy, self.s.modifiers.shift_key());
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
