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

/// Which way the arrow key pointed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    /// The arrow key this direction comes from, if any.
    pub(crate) fn from_named(named: &winit::keyboard::NamedKey) -> Option<Self> {
        use winit::keyboard::NamedKey as N;
        Some(match named {
            N::ArrowUp => Self::Up,
            N::ArrowDown => Self::Down,
            N::ArrowLeft => Self::Left,
            N::ArrowRight => Self::Right,
            _ => return None,
        })
    }
}

/// A laid-out rect as this module reasons about it.
#[derive(Clone, Copy)]
struct Box2 {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Box2 {
    fn centre(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

/// How much an off-axis offset costs relative to on-axis distance. Above 1 so a
/// control that is roughly in line wins over a nearer one well off to the side —
/// which is what "down" means to a person looking at a column.
const OFF_AXIS_COST: f32 = 2.0;

/// The penalty for a candidate that does not overlap the current element's band
/// at all. Large enough that any aligned candidate beats any unaligned one,
/// finite so that a lone diagonal neighbour is still reachable rather than
/// stranding focus.
const NO_OVERLAP_PENALTY: f32 = 10_000.0;

/// Score `candidate` as a move from `from` in `dir`, or `None` when it is not in
/// that direction at all.
///
/// The rule is the ordinary spatial-navigation heuristic: travel along the axis,
/// penalized by how far off-axis you have to go, and penalized much harder for
/// leaving the current element's band entirely. Lower is better.
fn score(from: Box2, candidate: Box2, dir: Direction) -> Option<f32> {
    let (fx, fy) = from.centre();
    let (cx, cy) = candidate.centre();
    // A hair of tolerance, so two controls on the same visual row do not count
    // as being above or below each other through rounding.
    const EPS: f32 = 0.5;
    let (along, off, overlaps) = match dir {
        Direction::Right => (
            cx - fx,
            (cy - fy).abs(),
            candidate.y < from.y + from.h && from.y < candidate.y + candidate.h,
        ),
        Direction::Left => (
            fx - cx,
            (cy - fy).abs(),
            candidate.y < from.y + from.h && from.y < candidate.y + candidate.h,
        ),
        Direction::Down => (
            cy - fy,
            (cx - fx).abs(),
            candidate.x < from.x + from.w && from.x < candidate.x + candidate.w,
        ),
        Direction::Up => (
            fy - cy,
            (cx - fx).abs(),
            candidate.x < from.x + from.w && from.x < candidate.x + candidate.w,
        ),
    };
    if along <= EPS {
        return None;
    }
    let penalty = if overlaps { 0.0 } else { NO_OVERLAP_PENALTY };
    Some(along + off * OFF_AXIS_COST + penalty)
}

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
    pub(crate) fn focus_spatial(&mut self, dir: Direction) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn b(x: f32, y: f32, w: f32, h: f32) -> Box2 {
        Box2 { x, y, w, h }
    }

    /// A control directly below beats a nearer one off to the side: "down" means
    /// down the column you are looking at.
    #[test]
    fn alignment_beats_raw_distance() {
        let from = b(0.0, 0.0, 100.0, 40.0);
        let below = score(from, b(0.0, 60.0, 100.0, 40.0), Direction::Down).expect("in direction");
        let aside =
            score(from, b(300.0, 45.0, 100.0, 40.0), Direction::Down).expect("in direction");
        assert!(
            below < aside,
            "aligned {below} must beat off-to-the-side {aside}",
        );
    }

    /// Nothing behind you is a candidate.
    #[test]
    fn the_opposite_direction_is_never_a_candidate() {
        let from = b(0.0, 100.0, 100.0, 40.0);
        assert!(score(from, b(0.0, 0.0, 100.0, 40.0), Direction::Down).is_none());
        assert!(score(from, b(0.0, 0.0, 100.0, 40.0), Direction::Up).is_some());
    }

    /// A control on the same visual row is not "below" its neighbour, so Down
    /// from one column does not slide sideways.
    #[test]
    fn the_same_row_is_not_below() {
        let from = b(0.0, 0.0, 100.0, 40.0);
        assert!(score(from, b(120.0, 0.0, 100.0, 40.0), Direction::Down).is_none());
        assert!(score(from, b(120.0, 0.0, 100.0, 40.0), Direction::Right).is_some());
    }
}
