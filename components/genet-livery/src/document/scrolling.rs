// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Scroll geometry: the document scrollport and per-element nested
//! scrollers, their extents, clamping, and the scroll-into-view routes.

use super::*;

impl<D> LiveryDocument<D>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    /// Return the current viewport scroll offset.
    pub fn scroll(&self) -> (f32, f32) {
        self.scroll
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
            // Focus, CSSOM, and DOM mutations can invalidate the position-aware
            // layout between input and the next frame. Viewport scrolling still
            // has retained extents and must not become a one-frame no-op.
            return self.scroll_by(dx, dy);
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

    /// Reveal one retained node within every currently active nested
    /// scrollport that contains it.
    ///
    /// This is the document-local half of the narrow accessibility
    /// `ScrollIntoView` route. It intentionally works only with scrollports
    /// that already have a nonzero retained offset: the accessibility
    /// projection advertises the action under that same condition, so no
    /// style-plane or host-wide scrolling contract is needed. The operation
    /// uses nearest-edge placement and leaves root viewport scrolling to its
    /// existing host path.
    pub fn scroll_accessible_node_into_view(&mut self, node: D::NodeId) -> bool {
        let Some(layout) = self.layout.as_ref() else {
            return false;
        };
        if layout.fragments.get(node).is_none() {
            return false;
        }

        // Visit the innermost scrollport first. Its new offset changes the
        // target's position seen by every outer scrollport, which makes the
        // following outer adjustments use the final retained geometry.
        let mut active_scrollports = Vec::new();
        let mut parent = self.dom.parent(node);
        while let Some(ancestor) = parent {
            if self
                .nested_scroll
                .get(&ancestor)
                .is_some_and(|&(x, y)| x != 0.0 || y != 0.0)
            {
                active_scrollports.push(ancestor);
            }
            parent = self.dom.parent(ancestor);
        }

        let mut changed = false;
        for scrollport in active_scrollports {
            let Some(style) = layout.styles.get(scrollport) else {
                continue;
            };
            let Some(container) = layout.fragments.get(scrollport) else {
                continue;
            };
            let Some(target) = layout.fragments.get(node) else {
                continue;
            };
            let current = self
                .nested_scroll
                .get(&scrollport)
                .copied()
                .unwrap_or_default();
            let target_ancestor_scroll = self.ancestor_scroll(node);
            let container_ancestor_scroll = self.ancestor_scroll(scrollport);
            let local_x =
                target.x - container.x - (target_ancestor_scroll.0 - container_ancestor_scroll.0);
            let local_y =
                target.y - container.y - (target_ancestor_scroll.1 - container_ancestor_scroll.1);
            let (max_x, max_y) = self.scroll_extent(layout, scrollport);
            let next = (
                if self.scrolls_x(style) {
                    reveal_scroll_offset(current.0, local_x, target.width, container.width, max_x)
                } else {
                    current.0
                },
                if self.scrolls_y(style) {
                    reveal_scroll_offset(current.1, local_y, target.height, container.height, max_y)
                } else {
                    current.1
                },
            );
            if next != current {
                self.nested_scroll.insert(scrollport, next);
                changed = true;
            }
        }
        if changed {
            self.cached = None;
        }
        changed
    }

    /// Return a retained CSS-space point that can activate one accessible
    /// element through ordinary pointer input.
    ///
    /// The point is the visible intersection of the element, every clipping
    /// ancestor, and the viewport. It is accepted only when Livery's current
    /// scroll-aware hit test resolves inside `node`, so a host cannot turn a
    /// stale accessibility bound into a pointer action after nested scrolling.
    /// The result intentionally stays in CSS space; the host applies its
    /// presentation-scale and content-hole transform at the session boundary.
    pub fn accessible_pointer_target(&self, node: D::NodeId) -> Option<(f32, f32)> {
        let layout = self.layout.as_ref()?;
        let active = self.sticky_layout(layout);
        let target = active.get(node)?;
        let target_scroll = self.ancestor_scroll(node);
        let mut visible = [
            target.x - self.scroll.0 - target_scroll.0,
            target.y - self.scroll.1 - target_scroll.1,
            target.x + target.width - self.scroll.0 - target_scroll.0,
            target.y + target.height - self.scroll.1 - target_scroll.1,
        ];
        visible = intersect_viewport_rect(
            visible,
            [0.0, 0.0, layout.viewport.0 as f32, layout.viewport.1 as f32],
        )?;

        let mut parent = self.dom.parent(node);
        while let Some(ancestor) = parent {
            if let Some(style) = layout.styles.get(ancestor)
                && self.clips_content(style)
            {
                let clip = active.get(ancestor)?;
                let clip_scroll = self.ancestor_scroll(ancestor);
                visible = intersect_viewport_rect(
                    visible,
                    [
                        clip.x - self.scroll.0 - clip_scroll.0,
                        clip.y - self.scroll.1 - clip_scroll.1,
                        clip.x + clip.width - self.scroll.0 - clip_scroll.0,
                        clip.y + clip.height - self.scroll.1 - clip_scroll.1,
                    ],
                )?;
            }
            parent = self.dom.parent(ancestor);
        }

        let point = (
            (visible[0] + visible[2]) * 0.5,
            (visible[1] + visible[3]) * 0.5,
        );
        let hit = self.hit_test(point.0, point.1)?;
        self.node_contains(hit, node).then_some(point)
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

    pub(in crate::document) fn clamp_scroll(&mut self) {
        let Some(layout) = self.layout.as_ref().or(self.identity_source.as_ref()) else {
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

    pub(in crate::document) fn document_content_extent(
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

    pub(in crate::document) fn extend_content_extent(
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

    pub(in crate::document) fn clamp_nested_scroll(&mut self) {
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

    pub(in crate::document) fn scroll_step(
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

    pub(in crate::document) fn scroll_extent(
        &self,
        layout: &LayoutState<D::NodeId>,
        node: D::NodeId,
    ) -> (f32, f32) {
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

    pub(in crate::document) fn extend_nested_extent(
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

    pub(in crate::document) fn is_scroll_container(&self, style: &livery::ComputedValues) -> bool {
        self.scrolls_x(style) || self.scrolls_y(style)
    }

    pub(in crate::document) fn clips_content(&self, style: &livery::ComputedValues) -> bool {
        style.overflow_x != Overflow::Visible || style.overflow_y != Overflow::Visible
    }

    pub(in crate::document) fn scrolls_x(&self, style: &livery::ComputedValues) -> bool {
        matches!(style.overflow_x, Overflow::Auto | Overflow::Scroll)
    }

    pub(in crate::document) fn scrolls_y(&self, style: &livery::ComputedValues) -> bool {
        matches!(style.overflow_y, Overflow::Auto | Overflow::Scroll)
    }

    pub(in crate::document) fn scrollable_axes(
        &self,
        layout: &LayoutState<D::NodeId>,
    ) -> (bool, bool) {
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

    pub(in crate::document) fn ancestor_scroll(&self, id: D::NodeId) -> (f32, f32) {
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
}

fn intersect_viewport_rect(left: [f32; 4], right: [f32; 4]) -> Option<[f32; 4]> {
    let intersection = [
        left[0].max(right[0]),
        left[1].max(right[1]),
        left[2].min(right[2]),
        left[3].min(right[3]),
    ];
    (intersection.iter().all(|value| value.is_finite())
        && intersection[0] < intersection[2]
        && intersection[1] < intersection[3])
        .then_some(intersection)
}

/// Move a retained scroll offset only as far as needed to show one axis of a
/// target. Oversized targets anchor their leading edge, matching the least
/// surprising result when neither edge can fit simultaneously.
fn reveal_scroll_offset(
    current: f32,
    target_start: f32,
    target_extent: f32,
    viewport_extent: f32,
    max: f32,
) -> f32 {
    let target_end = target_start + target_extent;
    let next = if target_start < 0.0 {
        current + target_start
    } else if target_end > viewport_extent {
        current + target_end - viewport_extent
    } else {
        current
    };
    next.clamp(0.0, max)
}
