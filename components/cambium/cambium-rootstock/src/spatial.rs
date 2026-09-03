// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Spatial focus navigation: hold Tab, then steer with the arrow keys.
//!
//! Tab traversal walks *document order*, which is the wrong shape for anything
//! laid out in two dimensions. Woodshed's fretboard puts sixty focusable notes
//! between you and the search field; a six-by-six grid of buttons takes
//! thirty-five presses to cross diagonally. Document order is a list, and a list
//! is a bad map.
//!
//! So: **hold Tab and the arrow keys move focus to the nearest control in that
//! direction.** Tap Tab and nothing changes — it traverses exactly as before.
//! The mode is entered by the key-repeat that holding produces, so the OS's own
//! repeat delay is the "held" threshold and no timer is needed.
//!
//! This belongs to the host and nowhere else. It needs the focusable set, which
//! only the runner knows, and the laid-out geometry, which only the layout
//! knows. No application has both, and the view layer has no layout at all.

use genet_scripted_dom::NodeId;

use crate::Host;
use crate::meristem_bounds::RootView;
use crate::{Box2, Direction, score};

impl<State, Logic, V> Host<State, Logic, V>
where
    State: 'static,
    Logic: FnMut(&State) -> V + 'static,
    V: RootView<State>,
{
    /// The painted box of every focusable that has one, plus the focused node.
    fn focusable_boxes(&self) -> (Option<NodeId>, Vec<(NodeId, Box2)>) {
        let (Some(runner), Some(layout)) = (self.s.runner.as_ref(), self.s.layout.as_ref()) else {
            return (None, Vec::new());
        };
        let dom = runner.dom();
        let dom_ref = dom.borrow();
        let boxes = runner
            .focusables()
            .into_iter()
            .filter_map(|node| {
                let (x, y, w, h) = layout.painted_rect(&*dom_ref, node)?;
                // A zero-area control is not somewhere focus can usefully go.
                (w > 0.0 && h > 0.0).then_some((node, Box2 { x, y, w, h }))
            })
            .collect();
        (runner.focus(), boxes)
    }

    /// Move focus to the nearest focusable in `dir`. Returns whether it moved.
    ///
    /// With nothing focused this takes the topmost-leftmost control, so the
    /// first arrow after entering the mode always lands somewhere rather than
    /// doing nothing.
    pub fn focus_spatial(&mut self, dir: Direction) -> bool {
        let (focused, boxes) = self.focusable_boxes();
        if boxes.is_empty() {
            return false;
        }
        let target = match focused.and_then(|f| boxes.iter().find(|(n, _)| *n == f)) {
            Some(&(_, from)) => boxes
                .iter()
                .filter(|(node, _)| Some(*node) != focused)
                .filter_map(|&(node, b)| score(from, b, dir).map(|s| (node, s)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(node, _)| node),
            // Nothing focused yet: start at the top-left.
            None => boxes
                .iter()
                .min_by(|a, b| {
                    let (ax, ay) = a.1.centre();
                    let (bx, by) = b.1.centre();
                    (ay, ax)
                        .partial_cmp(&(by, bx))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(node, _)| *node),
        };
        let Some(target) = target else {
            return false;
        };
        if Some(target) == focused {
            return false;
        }
        if let Some(runner) = self.s.runner.as_mut() {
            runner.set_focus(Some(target));
        }
        // Focus moved without a pointer, so refresh the `:focus` restyle and let
        // the application see the dispatch tail as it would for any other input.
        self.hover();
        self.after_dispatch();
        true
    }
}
