/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Per-inline-box fragment readback: give every inline-level element a rect.
//!
//! The Taffy plane holds one box per block-level / replaced / IFC-establishing
//! element. Everything that *flows in a line* — an `inline-block` `<button>`, an
//! inline `<img>`, a `display:inline` `<a>` or `<span>` — establishes no Taffy
//! box, so before this pass it had no [`FragmentPlane::rect_of`] entry at all,
//! and `absolute_rect` / `painted_rect` / a11y bounds answered `None` for it. The
//! genet UA sheet makes every form control `inline-block`, so that was every
//! unstyled `<button>` and `<input>` on the page: a screen reader's virtual
//! cursor had nothing to land on and `genet_probe::resolve` could not find a
//! control by role or label. Consumers worked around it by giving controls
//! `display: block`.
//!
//! Worse than absent, for one node it was *wrong*: an anonymous block box wrapping
//! a run of inline-level children is keyed in the plane by its **borrowed first
//! member** (`BoxSource::Anonymous`), so `rect_of(first_button)` handed back the
//! whole line box — full content width, not the button's shrink-to-fit box.
//!
//! The geometry was never missing, only unexposed: it lives in the leaf's cached
//! `parley::Layout`, the same place paint emission and
//! [`inline_hit`](crate::inline_hit) read it from. This pass distils it into
//! [`InlineFragment`] records at fragment-readback time, so hit, paint, and every
//! rect query mirror by reading one table rather than each re-deriving it.
//!
//! Two shapes, matching parley's own split:
//!
//! - **Atomic inlines** (`inline-block`, inline replaced `<img>`, inline
//!   `<custom-leaf>`) ride as `InlineBoxItem`s and parley reports each one's
//!   placed rect directly — one box, exactly the rect paint draws at.
//! - **Non-atomic `display:inline`** elements (`<a>`, `<span>`, `<label>`)
//!   establish N boxes across the N line boxes their text occupies. Their entry is
//!   the **union**, which is what `getBoundingClientRect` reports. Per-line
//!   geometry (hit areas, which must not swallow the inter-line gutter) stays with
//!   [`caret::selection_rects`](crate::caret::selection_rects) and
//!   [`link_harvest`](crate::link_harvest).
//!
//! An inline element's span covers its whole inline subtree, not just its direct
//! text: the leaf's byte-range → source map attributes each run to the *innermost*
//! enclosing inline, so each run is folded into every box-less inline ancestor up
//! to the leaf. That is what makes `<a><img></a>` — an anchor wrapping only a
//! replaced child — report the image's box as its own.
//!
//! Scope: **top-level** boxes of each leaf. An inline-block's own inline content
//! is measured into a sublayout keyed by `(leaf, box index)`, and only that first
//! level is cached ([`TextMeasureCtx::inline_block_layouts`]), so a control nested
//! *inside* an inline-block is not addressable here — the same depth paint can
//! place, so the table never claims geometry paint cannot draw.

use std::hash::Hash;
use std::ops::Range;

use layout_dom_api::LayoutDom;
use parley::PositionedLayoutItem;
use parley::layout::{Affinity, Cursor, Selection};

use crate::box_tree::BoxTree;
use crate::fragment::{FragmentPlane, InlineFragment};
use crate::text_measure::TextMeasureCtx;

/// One leaf's accumulating rects, in that leaf's border-box space. An element can
/// take geometry from more than one source — a wrapped `<a>` from each of its line
/// boxes, an `<a>` wrapping an `<img>` from the image's own placed box — so each is
/// unioned rather than overwritten. Reset per leaf: an element's inline content is
/// gathered into exactly one leaf, so the scan stays bounded by that leaf.
struct Unions<NodeId> {
    boxes: Vec<(NodeId, [f32; 4])>,
}

impl<NodeId: Copy + Eq> Unions<NodeId> {
    fn new() -> Self {
        Self { boxes: Vec::new() }
    }

    /// Union `(x, y, w, h)` into `node`'s rect, stored as `[x0, y0, x1, y1]`.
    fn add(&mut self, node: NodeId, x: f32, y: f32, w: f32, h: f32) {
        match self.boxes.iter_mut().find(|(id, _)| *id == node) {
            Some((_, r)) => {
                r[0] = r[0].min(x);
                r[1] = r[1].min(y);
                r[2] = r[2].max(x + w);
                r[3] = r[3].max(y + h);
            },
            None => self.boxes.push((node, [x, y, x + w, y + h])),
        }
    }
}

/// Fill `fragments`' inline-box table from the laid-out `tree` + its shaped text.
/// Called once per layout pass, right after the Taffy fragments are read back.
pub(crate) fn harvest<D>(
    dom: &D,
    tree: &BoxTree<D::NodeId>,
    text_ctx: &TextMeasureCtx,
    fragments: &mut FragmentPlane<D::NodeId>,
) where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    for arena in 0..tree.node_count() {
        let leaf_node = tree.node(arena);
        let Some(content) = leaf_node.inline_content.as_ref() else {
            continue;
        };
        let leaf = leaf_node.source.dom_id();
        let Some(layout) = text_ctx.layouts.get(&tree.arena_node_id(arena)) else {
            continue;
        };
        // parley positions runs and inline boxes in the leaf's CONTENT box; the
        // table is border-box relative (what the origin walkers answer for), so
        // fold the leaf's own border + padding in once here.
        let frame = &leaf_node.final_layout;
        let (cx, cy) = (
            frame.border.left + frame.padding.left,
            frame.border.top + frame.padding.top,
        );
        let mut unions = Unions::new();

        // Atomic inlines: parley placed each one, so its rect is read straight off.
        for line in layout.lines() {
            for item in line.items() {
                let PositionedLayoutItem::InlineBox(pbox) = item else {
                    continue;
                };
                let Some(box_item) = content.boxes.get(pbox.id as usize) else {
                    continue;
                };
                // parley placed the MARGIN box. The plane records the BORDER box,
                // which is what `getClientRects` is defined over, so an
                // inline-block's margin comes back off. A replaced `<img>` or an
                // inline `<custom-leaf>` carries no box-model record and needs no
                // adjustment.
                let (mx, my, mw, mh) = match &box_item.block {
                    Some(block) => (
                        block.margin.left,
                        block.margin.top,
                        block.margin.inline(),
                        block.margin.block(),
                    ),
                    None => (0.0, 0.0, 0.0, 0.0),
                };
                for owner in inline_owners(dom, tree, box_item.source, leaf) {
                    unions.add(
                        owner,
                        cx + pbox.x + mx,
                        cy + pbox.y + my,
                        pbox.width - mw,
                        pbox.height - mh,
                    );
                }
            }
        }

        // Non-atomic inlines: each element's byte span within the leaf, folded up
        // through its box-less inline ancestors, then measured as line boxes.
        if let Some(sources) = tree.inline_sources(leaf) {
            let mut spans: Vec<(D::NodeId, Range<usize>)> = Vec::new();
            for (range, src) in sources {
                for owner in inline_owners(dom, tree, *src, leaf) {
                    match spans.iter_mut().find(|(id, _)| *id == owner) {
                        Some((_, span)) => {
                            span.start = span.start.min(range.start);
                            span.end = span.end.max(range.end);
                        },
                        None => spans.push((owner, range.clone())),
                    }
                }
            }
            for (owner, span) in spans {
                if span.start >= span.end {
                    continue;
                }
                let anchor = Cursor::from_byte_index(layout, span.start, Affinity::default());
                let focus = Cursor::from_byte_index(layout, span.end, Affinity::default());
                for (bb, _line) in Selection::new(anchor, focus).geometry(layout) {
                    unions.add(
                        owner,
                        cx + bb.x0 as f32,
                        cy + bb.y0 as f32,
                        (bb.x1 - bb.x0) as f32,
                        (bb.y1 - bb.y0) as f32,
                    );
                }
            }
        }

        for (node, [x0, y0, x1, y1]) in unions.boxes {
            fragments.insert_inline_box(
                node,
                InlineFragment {
                    leaf,
                    x: x0,
                    y: y0,
                    width: x1 - x0,
                    height: y1 - y0,
                },
            );
        }
    }
}

/// The elements that own `src`'s inline geometry: `src` itself and each box-less
/// inline ancestor up to (exclusive) the first ancestor that establishes its own
/// box — an `<em>` inside an `<a>` inside a `<p>` yields `[em, a]`, and the `<p>`
/// keeps its Taffy rect. Stops at `leaf` regardless, since an anonymous block box
/// is keyed by a *sibling* of the run rather than an ancestor and the DOM walk
/// would otherwise run past it.
///
/// An element that owns a box is never yielded, so the inline table stays disjoint
/// from `rects` — except for an anonymous wrapper's borrowed key, which owns no box
/// of its own and is precisely the alias the table must override.
fn inline_owners<D>(
    dom: &D,
    tree: &BoxTree<D::NodeId>,
    src: D::NodeId,
    leaf: D::NodeId,
) -> Vec<D::NodeId>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let mut out = Vec::new();
    let mut cur = Some(src);
    while let Some(node) = cur {
        if tree.owns_box(node) {
            break;
        }
        out.push(node);
        if node == leaf {
            break;
        }
        cur = dom.parent(node);
    }
    out
}
