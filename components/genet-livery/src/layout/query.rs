// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! The query plane of a finished layout: fragment and rect lookup, caret,
//! selection and text-range geometry, the table paint queries paint reads,
//! and the identifier reconciliation and positioned-subtree replacement the
//! retained document performs between passes.

use super::*;

impl<Id> LiveryLayout<Id>
where
    Id: Copy + Eq + Hash,
{
    pub(in crate::layout) fn new(
        buckram: LayoutResult<Id>,
        text_frame: Option<TextFrame<Id>>,
        block_algorithms: BlockAlgorithmCounts,
        table_paint: TablePaintPlane,
        table_shadow: TableShadowLedger,
    ) -> Self {
        Self {
            buckram,
            text_frame,
            block_algorithms,
            table_paint,
            table_shadow,
        }
    }

    pub fn buckram(&self) -> &LayoutResult<Id> {
        &self.buckram
    }

    pub fn boxes(&self) -> &buckram::CssBoxTree<Id> {
        self.buckram.boxes()
    }

    pub fn fragments(&self) -> &FragmentTree {
        self.buckram.fragments()
    }

    /// Retain stable Buckram identities across a freshly recomputed layout.
    /// Geometry, text shaping, and paint inputs remain from the new pass.
    pub(crate) fn reconcile_identifiers(&mut self, previous: &Self) {
        let identities = self.buckram.reconcile_identifiers(&previous.buckram);
        self.table_paint.remap_box_ids(&identities);
        self.table_shadow
            .remap_box_ids(|box_id| identities.box_id(box_id));
    }

    /// Every DOM source attached to the selected generated-box subtree. The
    /// retained text frame needs this old set as well as the final DOM set so
    /// a removed text node cannot retain a prepared run or selection cluster.
    pub(crate) fn generated_subtree_nodes(&self, node: Id) -> HashSet<Id> {
        fn visit<Id>(boxes: &buckram::CssBoxTree<Id>, box_id: BoxId, nodes: &mut HashSet<Id>)
        where
            Id: Copy + Eq + Hash,
        {
            if let Some(node) = boxes.origin_node(box_id) {
                nodes.insert(node);
            }
            for child in boxes[box_id].children() {
                visit(boxes, *child, nodes);
            }
        }

        let mut nodes = HashSet::new();
        if let Some(root) = self.buckram.boxes().principal_box(node) {
            visit(self.buckram.boxes(), root, &mut nodes);
        }
        nodes
    }

    /// Publish one freshly formatted, reconciled flex or grid root into this
    /// retained layout. Its root box must retain identity, but descendants
    /// may gain or retire boxes; the fresh box tree replaces node ownership
    /// only after the fragment splice has accepted that compatible root.
    ///
    /// The fragment splice preserves the selected root identity but gives its
    /// descendants fresh identities. Fresh text and table planes accompany it
    /// as one publication unit, so paint cannot read a stale side model.
    pub(crate) fn replace_reconciled_formatting_subtree_from(
        &mut self,
        fresh: &Self,
        node: Id,
    ) -> bool {
        let Some(root_box) = self
            .buckram
            .boxes()
            .boxes_for_node(node)
            .iter()
            .copied()
            .find(|box_id| {
                matches!(
                    self.buckram.boxes()[*box_id].formatting_context,
                    Some(FormattingContextKind::Flex | FormattingContextKind::Grid)
                )
            })
        else {
            return false;
        };
        let Some(fresh_root_box) = fresh
            .buckram
            .boxes()
            .boxes_for_node(node)
            .iter()
            .copied()
            .find(|box_id| *box_id == root_box)
        else {
            return false;
        };
        let root = match self.buckram.fragments().fragment_ids_for_box(root_box) {
            [root] => *root,
            _ => return false,
        };
        let fresh_root = match fresh
            .buckram
            .fragments()
            .fragment_ids_for_box(fresh_root_box)
        {
            [root] => *root,
            _ => return false,
        };
        if self
            .buckram
            .fragments_mut()
            .replace_subtree(root, fresh.buckram.fragments(), fresh_root)
            .is_none()
        {
            return false;
        }
        self.buckram.replace_box_tree(fresh.buckram.boxes().clone());
        self.text_frame = fresh.text_frame.clone();
        self.block_algorithms = fresh.block_algorithms;
        self.table_paint = fresh.table_paint.clone();
        self.table_shadow = fresh.table_shadow.clone();
        true
    }

    /// Publish a local formatting result. Unlike the complete publication
    /// route, `fresh` contains one selected subtree only, so table planes
    /// outside that subtree stay authoritative while its text frame replaces
    /// only the selected DOM sources.
    pub(crate) fn replace_reconciled_local_formatting_subtree_from(
        &mut self,
        fresh: &Self,
        node: Id,
        replaced_nodes: &HashSet<Id>,
        dom_text_order: &[Id],
    ) -> bool {
        let Some(root_box) = retained_root_box(self.buckram.boxes(), node) else {
            return false;
        };
        let Some(fresh_root_box) = retained_root_box(fresh.buckram.boxes(), node) else {
            return false;
        };
        if fresh_root_box != root_box {
            return false;
        }
        let table_root = self.buckram.boxes()[root_box].display.internal_table
            == Some(InternalTableRole::Wrapper);
        if table_root
            && (self.table_paint.tables.is_empty()
                || fresh.table_paint.tables.is_empty()
                || !self
                    .table_paint
                    .tables
                    .keys()
                    .any(|grid| box_is_descendant_of(self.buckram.boxes(), *grid, root_box))
                || !fresh
                    .table_paint
                    .tables
                    .keys()
                    .all(|grid| box_is_descendant_of(fresh.buckram.boxes(), *grid, fresh_root_box))
                || !fresh
                    .table_paint
                    .tables
                    .keys()
                    .any(|grid| box_is_descendant_of(fresh.buckram.boxes(), *grid, fresh_root_box))
                || !self.table_shadow.can_replace_subtree(
                    &fresh.table_shadow,
                    self.buckram.boxes(),
                    fresh.buckram.boxes(),
                    root_box,
                    fresh_root_box,
                ))
        {
            return false;
        }
        let root = match self.buckram.fragments().fragment_ids_for_box(root_box) {
            [root] => *root,
            _ => return false,
        };
        let fresh_root = match fresh
            .buckram
            .fragments()
            .fragment_ids_for_box(fresh_root_box)
        {
            [root] => *root,
            _ => return false,
        };
        if self.text_frame.is_none() || fresh.text_frame.is_none() {
            return false;
        }
        if self
            .buckram
            .fragments_mut()
            .replace_subtree(root, fresh.buckram.fragments(), fresh_root)
            .is_none()
        {
            return false;
        }
        if table_root {
            self.table_paint.replace_subtree_from(
                &fresh.table_paint,
                self.buckram.boxes(),
                fresh.buckram.boxes(),
                root_box,
                fresh_root_box,
            );
            self.table_shadow.replace_subtree_from(
                &fresh.table_shadow,
                self.buckram.boxes(),
                fresh.buckram.boxes(),
                root_box,
                fresh_root_box,
            );
        }
        self.buckram.replace_box_tree(fresh.buckram.boxes().clone());
        self.text_frame
            .as_mut()
            .expect("a checked retained text frame is present")
            .replace_subtree_from(
                fresh
                    .text_frame
                    .as_ref()
                    .expect("a checked fresh text frame is present"),
                replaced_nodes,
                dom_text_order,
            );
        true
    }

    /// Publish a disjoint K5h damage set as one retained-layout update. A
    /// failed root leaves `self` untouched, so callers can safely fall back
    /// to the complete fresh result without exposing a partial publication.
    pub(crate) fn replace_reconciled_formatting_subtrees_from(
        &mut self,
        fresh: &Self,
        roots: &[Id],
    ) -> bool {
        if roots.is_empty() {
            return false;
        }
        let mut replacement = self.clone();
        for root in roots {
            if !replacement.replace_reconciled_formatting_subtree_from(fresh, *root) {
                return false;
            }
        }
        *self = replacement;
        true
    }

    /// Apply retained scroll-dependent sticky constraints to this otherwise
    /// normal-flow layout snapshot. Callers clone the static layout first, so
    /// scroll changes never accumulate into the next frame's base geometry.
    pub(crate) fn apply_sticky_positioning(
        &mut self,
        styles: &StylePlane<Id>,
        viewport_width: f32,
        viewport_height: f32,
        mut scrollport_for: impl FnMut(Id) -> Option<StickyScrollport>,
    ) {
        let placements = self
            .buckram
            .boxes()
            .iter()
            .filter_map(|(box_id, css_box)| {
                if css_box.positioning != PositioningScheme::Sticky
                    || css_box
                        .display
                        .internal_table
                        .is_some_and(|role| !supports_retained_sticky_table_part(role))
                {
                    return None;
                }
                let node = css_box.origin.node()?;
                let scrollport = scrollport_for(node)?;
                let root = self
                    .buckram
                    .fragments()
                    .fragment_ids_for_box(box_id)
                    .iter()
                    .copied()
                    .find(|fragment_id| {
                        self.buckram
                            .fragments()
                            .get(*fragment_id)
                            .and_then(TreeFragment::parent)
                            .and_then(|parent| self.buckram.fragments().get(parent))
                            .is_none_or(|parent| parent.box_id() != box_id)
                    })?;
                let current = self.buckram.fragments().get(root)?.physical_rect();
                // A table-internal box's generated parent can be a row or
                // row group that is only as tall as that part. It would clamp
                // a sticky translation to zero. Its sticky containing block
                // is the table wrapper: the nearest block-level table
                // ancestor that owns the table's full scrollable extent.
                let table_wrapper = if css_box.display.internal_table.is_some_and(|role| {
                    role != InternalTableRole::Wrapper && supports_retained_sticky_table_part(role)
                }) {
                    let mut ancestor = css_box.parent();
                    loop {
                        let candidate = ancestor?;
                        if self.buckram.boxes()[candidate].display.internal_table
                            == Some(InternalTableRole::Wrapper)
                        {
                            break Some(candidate);
                        }
                        ancestor = self.buckram.boxes()[candidate].parent();
                    }
                } else {
                    None
                };
                let containing = match table_wrapper
                    .map(ContainingBlock::Box)
                    .unwrap_or(css_box.containing_block)
                {
                    ContainingBlock::Initial => PhysicalRect {
                        x: 0.0,
                        y: 0.0,
                        width: viewport_width,
                        height: viewport_height,
                    },
                    ContainingBlock::Box(containing) => self
                        .buckram
                        .fragments()
                        .fragment_ids_for_box(containing)
                        .first()
                        .and_then(|fragment_id| self.buckram.fragments().get(*fragment_id))
                        .map(TreeFragment::physical_rect)?,
                };
                let computed = styles.get(node)?;
                let computed = if css_box.display.internal_table == Some(InternalTableRole::Wrapper)
                {
                    wrapper_style(computed)
                } else {
                    computed.clone()
                };
                let font_size = font_size_px(&computed.font_size, LIVE_ROOT_FONT_SIZE);
                let style =
                    to_block_style(self.buckram.boxes(), styles, box_id, &computed, font_size);
                let percentage_basis = style
                    .containing_flow
                    .logical_size(PhysicalSize {
                        width: scrollport.rect.width,
                        height: scrollport.rect.height,
                    })
                    .inline;
                Some((
                    root,
                    current,
                    containing,
                    scrollport,
                    style.inset.left.resolve(percentage_basis),
                    style.inset.right.resolve(percentage_basis),
                    style.inset.top.resolve(percentage_basis),
                    style.inset.bottom.resolve(percentage_basis),
                ))
            })
            .collect::<Vec<_>>();

        for (root, current, containing, scrollport, left, right, top, bottom) in placements {
            let x = buckram::solve_sticky_axis(buckram::StickyAxisInput {
                normal_start: current.x,
                box_size: current.width,
                scrollport_start: scrollport.rect.x,
                scrollport_size: scrollport.rect.width,
                scroll_offset: scrollport.offset.x,
                containing_start: containing.x,
                containing_size: containing.width,
                start_inset: left,
                end_inset: right,
            });
            let y = buckram::solve_sticky_axis(buckram::StickyAxisInput {
                normal_start: current.y,
                box_size: current.height,
                scrollport_start: scrollport.rect.y,
                scrollport_size: scrollport.rect.height,
                scroll_offset: scrollport.offset.y,
                containing_start: containing.y,
                containing_size: containing.height,
                start_inset: top,
                end_inset: bottom,
            });
            self.buckram
                .fragments_mut()
                .translate_subtree(root, PhysicalOffset { x, y });
        }
    }

    /// Reposition one retained absolute or fixed fragment subtree when its
    /// computed insets are the only style change and Buckram proves its used
    /// border-box size is unchanged. General dirty-root formatting still
    /// rebuilds; this bounded K5h route owns only the final K5d translation.
    pub(crate) fn reposition_stable_positioned_subtree<D>(
        &mut self,
        dom: &D,
        styles: &StylePlane<D::NodeId>,
        image_sources: &ImageSources,
        node: D::NodeId,
        viewport_width: f32,
        viewport_height: f32,
    ) -> bool
    where
        D: LayoutDom<NodeId = Id>,
        D::NodeId: Copy + Eq + Hash,
    {
        let Some(placement) = self.positioned_placement_for_node(
            dom,
            styles,
            image_sources,
            node,
            viewport_width,
            viewport_height,
        ) else {
            return false;
        };
        let target = placement.target_rect();
        if (target.width - placement.current.width).abs() > 0.001
            || (target.height - placement.current.height).abs() > 0.001
        {
            return false;
        }
        let offset = PhysicalOffset {
            x: placement.containing_rect.x + target.x - placement.current.x,
            y: placement.containing_rect.y + target.y - placement.current.y,
        };
        if offset.x == 0.0 && offset.y == 0.0 {
            return false;
        }
        {
            let fragments = self.buckram.fragments_mut();
            fragments.translate_subtree(placement.root, offset);
            fragments.set_containing_fragment(placement.root, placement.containing_fragment);
        }
        if let Some(text_frame) = self.text_frame.as_mut() {
            text_frame.translate_subtree(dom, node, (offset.x, offset.y));
        }
        true
    }

    /// Resize and reposition one retained absolute or fixed leaf after its
    /// declared width or height changed. The leaf-only precondition prevents
    /// a stale child containing block: any subtree with descendants continues
    /// through the ordinary full-layout path.
    pub(crate) fn resize_positioned_leaf<D>(
        &mut self,
        dom: &D,
        styles: &StylePlane<D::NodeId>,
        image_sources: &ImageSources,
        node: D::NodeId,
        viewport_width: f32,
        viewport_height: f32,
    ) -> bool
    where
        D: LayoutDom<NodeId = Id>,
        D::NodeId: Copy + Eq + Hash,
    {
        let Some(placement) = self.positioned_placement_for_node(
            dom,
            styles,
            image_sources,
            node,
            viewport_width,
            viewport_height,
        ) else {
            return false;
        };
        let target = placement.target_rect();
        let offset = PhysicalOffset {
            x: placement.containing_rect.x + target.x - placement.current.x,
            y: placement.containing_rect.y + target.y - placement.current.y,
        };
        let size_changed = (target.width - placement.current.width).abs() > 0.001
            || (target.height - placement.current.height).abs() > 0.001;
        if size_changed
            && self
                .text_frame
                .as_ref()
                .is_some_and(|text_frame| text_frame.subtree_has_prepared_text(dom, node))
        {
            return false;
        }
        let fragments = self.buckram.fragments_mut();
        if !fragments.resize_leaf(
            placement.root,
            PhysicalSize {
                width: target.width,
                height: target.height,
            },
        ) {
            return false;
        }
        fragments.translate_subtree(placement.root, offset);
        fragments.set_containing_fragment(placement.root, placement.containing_fragment);
        if let Some(text_frame) = self.text_frame.as_mut() {
            text_frame.translate_subtree(dom, node, (offset.x, offset.y));
        }
        true
    }

    fn positioned_placement_for_node<D>(
        &self,
        dom: &D,
        styles: &StylePlane<D::NodeId>,
        image_sources: &ImageSources,
        node: D::NodeId,
        viewport_width: f32,
        viewport_height: f32,
    ) -> Option<PositionedPlacement>
    where
        D: LayoutDom<NodeId = Id>,
        D::NodeId: Copy + Eq + Hash,
    {
        let intrinsic_sizes = HashMap::new();
        let mut placements = positioned_placements(
            self.buckram.fragments(),
            self.buckram.boxes(),
            styles,
            dom,
            image_sources,
            &intrinsic_sizes,
            viewport_width,
            viewport_height,
        )
        .into_iter()
        .filter(|placement| self.buckram.boxes()[placement.box_id].origin.node() == Some(node));
        let placement = placements.next()?;
        (placements.next().is_none()
            && self.buckram.boxes()[placement.box_id]
                .display
                .internal_table
                .is_none()
            && self
                .buckram
                .fragments()
                .fragment_ids_for_box(placement.box_id)
                .len()
                == 1)
            .then_some(placement)
    }

    pub fn fragments_for_node(&self, node: Id) -> impl Iterator<Item = &TreeFragment> {
        self.buckram.fragments_for_node(node)
    }

    pub fn get(&self, node: Id) -> Option<&TreeFragment> {
        self.buckram.get(node)
    }

    /// Compatibility name for callers that only need a node's outermost
    /// retained fragment.
    pub fn rect_of(&self, node: Id) -> Option<&TreeFragment> {
        self.get(node)
    }

    /// A retained caret rectangle in document coordinates.
    pub fn caret_rect(&self, node: Id, byte: usize) -> Option<crate::TextRect> {
        self.text_frame()?
            .caret_rect(node, byte, |_, fragment| crate::TextRect {
                x: fragment.x,
                y: fragment.y,
                width: fragment.width,
                height: fragment.height,
            })
    }

    /// The shaped source position nearest a document point.
    pub fn text_position_at_point(&self, x: f32, y: f32) -> Option<(Id, usize)> {
        self.text_frame()?
            .text_position_at_point(x, y, |_, fragment| crate::TextRect {
                x: fragment.x,
                y: fragment.y,
                width: fragment.width,
                height: fragment.height,
            })
    }

    /// Retained geometry and text for a directed source range.
    pub fn text_selection(&self, range: crate::TextRange<Id>) -> Option<crate::TextSelection<Id>> {
        self.text_frame()?
            .text_selection(range, |_, fragment| crate::TextRect {
                x: fragment.x,
                y: fragment.y,
                width: fragment.width,
                height: fragment.height,
            })
    }

    /// Resolve the first shaped occurrence of `text` to its source range.
    ///
    /// Retained hosts use this to turn find results into ordinary pointer
    /// selection gestures without reaching into Livery's text-frame storage.
    pub fn text_range_for_text(&self, text: &str) -> Option<crate::TextRange<Id>> {
        self.text_frame()?.find_text_range(text)
    }

    /// Resolve a parsed URL Text Directive against retained logical text.
    pub fn text_range_for_text_directive(
        &self,
        directive: &crate::TextDirective,
    ) -> Option<crate::TextRange<Id>> {
        self.text_frame()?.find_text_directive_range(directive)
    }

    /// The node's principal box's fragment: a table element's grid box, which
    /// owns background, borders, and used `width`/`height` under CSS 2.1
    /// section 17.4. Rectangle queries and paint-effect anchors use
    /// [`Self::get`], whose first box is the outermost - the wrapper.
    pub fn principal_fragment(&self, node: Id) -> Option<&TreeFragment> {
        self.buckram.principal_fragment(node)
    }

    pub fn len(&self) -> usize {
        self.buckram.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buckram.is_empty()
    }

    pub fn block_algorithm_counts(&self) -> BlockAlgorithmCounts {
        self.block_algorithms
    }

    /// K4f's retained table paint model. Structural table boxes are emitted by
    /// Buckram, but their background phase cannot be reconstructed from DOM
    /// traversal once row and column boxes have been flattened away.
    pub(crate) fn table_paint_for_node(&self, node: Id) -> Option<&TablePaintModel> {
        self.buckram
            .boxes()
            .boxes_for_node(node)
            .iter()
            .find_map(|box_id| self.table_paint.table(*box_id))
    }

    /// Whether a node's own decoration is painted by the separated-table
    /// phase, rather than the ordinary DOM walk.
    pub(crate) fn table_paint_manages_node(&self, node: Id) -> bool {
        self.buckram
            .boxes()
            .boxes_for_node(node)
            .iter()
            .copied()
            .any(|box_id| self.table_paint.manages(box_id))
    }

    /// A collapsed table's grid and cells retain their normal background
    /// phase, but its generic border command must yield to K4g5's one-winner
    /// segment model.
    pub(crate) fn table_paint_uses_collapsed_borders(&self, node: Id) -> bool {
        self.table_paint_for_node(node)
            .is_some_and(TablePaintModel::is_collapsed)
    }

    /// Whether the node's descendants must clip at the accepted edge of a
    /// cell spanning a collapsed track.
    pub(crate) fn table_cell_requires_clip(&self, node: Id) -> bool {
        self.buckram
            .boxes()
            .boxes_for_node(node)
            .iter()
            .copied()
            .any(|box_id| self.table_paint.clips_cell(box_id))
    }

    /// K4c5a's shadow comparison of Buckram's fixed sizing against the live
    /// path. K4c5b may only make Buckram authoritative once this is silent.
    pub fn table_shadow_ledger(&self) -> &TableShadowLedger {
        &self.table_shadow
    }

    pub(crate) fn text_frame(&self) -> Option<&TextFrame<Id>> {
        self.text_frame.as_ref()
    }
}
