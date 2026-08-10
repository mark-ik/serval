/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Fragment plane — laid-out rects keyed by DOM `NodeId`.
//!
//! After Taffy runs, this plane stores the per-node layout result so consumers
//! (paint emission, hit-testing, the apparatus inspector,
//! `getBoundingClientRect`-shaped queries) can read positions back without
//! re-running layout. The plane is a `NodeId → taffy::Layout` map; richer
//! fragment data (line boxes, pseudo-element fragments, scroll-container
//! metadata) is a future extension per the planes doc.
//!
//! Per the Hekate doc's "publishing observables" rule the plane is `pub(crate)`,
//! and the public query surface is the `engine_observables_api` `FragmentQuery`
//! trait, implemented by `GenetLaneView` (`genet_lane.rs`).

use std::hash::Hash;

use rustc_hash::FxHashMap;
use taffy::Layout;

/// One **inline-level** element's box, recovered from the laid-out text of the
/// inline-formatting leaf it flows in.
///
/// An element that flows inline — an atomic inline (`inline-block`, an inline
/// replaced `<img>`, an inline `<custom-leaf>`) or a non-atomic `display:inline`
/// (`<a>`, `<span>`, `<label>`) — establishes no Taffy box, so it has no
/// [`FragmentPlane::rect_of`] entry of its own. Its geometry lives in its leaf's
/// `parley::Layout` (as a positioned inline box, or as the line boxes its text
/// occupies), which fragment readback distils into this record.
///
/// The offset is from `leaf`'s **border-box** origin — the leaf's own content
/// offset is already folded in — so a consumer resolves the absolute or painted
/// rect by adding whichever origin it already computes for `leaf`
/// (`absolute_origin`, `painted_origin`, an `accumulate_origins` entry). Keeping
/// the record leaf-relative rather than absolute is what lets the scroll-aware
/// and unscrolled walkers each stay correct without a second table.
///
/// A wrapped inline (a link broken across two lines) reports the **union** of its
/// line boxes, matching `getBoundingClientRect`. Per-line geometry, where hit
/// areas must not include the inter-line gutter, stays with
/// [`crate::caret::selection_rects`] / [`crate::link_harvest`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InlineFragment<NodeId> {
    /// The inline-formatting leaf hosting this box — the node whose origin the
    /// offset below is relative to. For a run of inline-level children wrapped in
    /// an anonymous block box this is that box's borrowed key (its first member),
    /// which is exactly the key the origin walkers answer for.
    pub leaf: NodeId,
    /// Offset from `leaf`'s border-box origin, in layout px.
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone)]
pub struct FragmentPlane<NodeId: Copy + Eq + Hash> {
    pub(crate) rects: FxHashMap<NodeId, Layout>,
    /// Inline-level elements' boxes (see [`InlineFragment`]) — the plane's
    /// answer for everything that flows in a line instead of establishing a
    /// Taffy box. Disjoint from `rects` in intent: readback records a node here
    /// only when it owns no box of its own, so a consumer that prefers this
    /// entry never shadows a real fragment. It does, though, deliberately win
    /// over the ALIAS an anonymous wrapper leaves in `rects` — that wrapper is
    /// keyed by its borrowed first member, so without this table
    /// `rect_of(first_button)` hands back the whole line box.
    pub(crate) inline_boxes: FxHashMap<NodeId, InlineFragment<NodeId>>,
    /// Absolute (layout-space) origins of boxes the box tree **hoisted** to a
    /// containing block that is not their DOM parent (position-containing-block
    /// plan: `fixed` to the ICB today, `absolute` to its positioned ancestor
    /// under F2). Their `Layout.location` is relative to the *hoist* parent, so
    /// DOM-driven origin accumulation (hit-testing, `absolute_origin`, a11y
    /// bounds) would add the DOM ancestors' offsets a second time; walkers that
    /// find a node here use this origin standalone instead. Filled from the box
    /// tree at fragment-readback time — the one source of truth, so walkers and
    /// paint agree by data rather than by re-derived predicates.
    pub(crate) hoisted_origins: FxHashMap<NodeId, (f32, f32)>,
    /// The reverse view: hoist target -> the boxes hoisted **to** it (the
    /// root's DOM id for `fixed`, the positioned ancestor's for `absolute`).
    /// The hit walk defers a hoisted box from its *target's* frame — the frame
    /// whose accumulated point mapping (scrolls above the containing block,
    /// clips on the containing-block chain) is the one that legitimately
    /// applies to it — rather than from its DOM parent's, where intermediate
    /// static clippers/scrollers would wrongly apply.
    pub(crate) hoisted_by_target: FxHashMap<NodeId, Vec<NodeId>>,
}

impl<NodeId: Copy + Eq + Hash> Default for FragmentPlane<NodeId> {
    fn default() -> Self {
        Self {
            rects: FxHashMap::default(),
            inline_boxes: FxHashMap::default(),
            hoisted_origins: FxHashMap::default(),
            hoisted_by_target: FxHashMap::default(),
        }
    }
}

impl<NodeId: Copy + Eq + Hash> FragmentPlane<NodeId> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, id: NodeId, layout: Layout) {
        self.rects.insert(id, layout);
    }

    /// Record an inline-level element's box (see [`InlineFragment`]).
    pub fn insert_inline_box(&mut self, id: NodeId, fragment: InlineFragment<NodeId>) {
        self.inline_boxes.insert(id, fragment);
    }

    /// Drop `id`'s inline-box entry, if any. The splice path calls this before
    /// merging a scoped subtree's entries, so a node that stopped flowing inline
    /// (restyled to `display: block`, or its text removed) does not keep a stale
    /// inline rect while its real fragment says otherwise.
    pub fn remove_inline_box(&mut self, id: NodeId) {
        self.inline_boxes.remove(&id);
    }

    /// The inline-level box of `id` (see [`InlineFragment`]), or `None` when `id`
    /// establishes a Taffy box of its own — read [`Self::rect_of`] for those — or
    /// was not laid out at all.
    pub fn inline_box_of(&self, id: NodeId) -> Option<InlineFragment<NodeId>> {
        self.inline_boxes.get(&id).copied()
    }

    /// Drop every entry, in both tables, whose node `live` rejects.
    ///
    /// A structural splice re-lays-out the mutated subtree and writes the result
    /// over the retained plane, but it walks the **live** DOM, so a node removed
    /// by the batch is never visited and its entry stays behind. The box tree's
    /// own graft already purges the departed subtree from `node_map`,
    /// `inline_sources`, and the shaped-text cache
    /// ([`BoxTree::graft_subtree`](crate::BoxTree::graft_subtree)); without this
    /// the plane is the one side table that does not keep step, so it grows
    /// without bound across a session's churn and `fragment_count` over-reports.
    ///
    /// The dangle contract that lets `hit_test` reject a retired id covers the
    /// window between a host publishing DOM and its next apply. It is not licence
    /// to keep the entry after an apply has processed the removal.
    pub fn retain_live(&mut self, live: impl Fn(NodeId) -> bool) {
        self.rects.retain(|id, _| live(*id));
        self.inline_boxes.retain(|id, _| live(*id));
        self.hoisted_origins.retain(|id, _| live(*id));
        self.hoisted_by_target.retain(|id, boxes| {
            boxes.retain(|b| live(*b));
            live(*id) && !boxes.is_empty()
        });
    }

    /// The absolute origin of a hoisted out-of-flow box (see
    /// [`Self::hoisted_origins`]), or `None` for every in-flow box.
    pub fn hoisted_origin(&self, id: NodeId) -> Option<(f32, f32)> {
        self.hoisted_origins.get(&id).copied()
    }

    /// The boxes hoisted **to** `id` (see [`Self::hoisted_by_target`]); empty
    /// for every node that is not a hoist target.
    pub fn hoisted_children(&self, id: NodeId) -> &[NodeId] {
        self.hoisted_by_target.get(&id).map_or(&[], Vec::as_slice)
    }

    /// Read the laid-out rect for a node, if it was reached by layout.
    /// Non-element nodes (text, comment, document) won't have entries
    /// in the probe — see `construct.rs`.
    pub fn rect_of(&self, id: NodeId) -> Option<&Layout> {
        self.rects.get(&id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&NodeId, &Layout)> {
        self.rects.iter()
    }

    /// Every inline-level element's box (see [`InlineFragment`]) — the
    /// inline-box table's twin of [`Self::iter`]. A consumer comparing two whole
    /// planes (the splice differential harness) needs the key set, not just
    /// per-node lookups.
    pub fn iter_inline_boxes(&self) -> impl Iterator<Item = (&NodeId, &InlineFragment<NodeId>)> {
        self.inline_boxes.iter()
    }

    pub fn len(&self) -> usize {
        self.rects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }
}
