// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Rootstock-owned retained Livery session over Cambium's externally owned DOM.

use std::collections::HashMap;

use genet_livery::{
    Device, InteractionStates, LiveryLayout, LiveryPaintList, StatePseudoClass, StylePlane,
    StyleSet, TextRange, TextSystem, ViewportSizes,
    emit_paint_list_with_text_system_scrolled_with_images, hit_test_with_scroll,
    layout_with_text_system, resolve_styles,
};
use genet_render::{VisualAffinity, VisualCaret, VisualMovement, VisualSelection};
use genet_scripted_dom::NodeId;
use layout_dom_api::{LayoutDom, LocalName, Namespace, NodeKind};
use paint_list_api::{ColorF, DeviceIntSize, LayoutPoint, LayoutRect, LayoutSize};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ScrollTarget {
    Document,
    Element(NodeId),
}

pub struct OwnedLayout {
    style_set: StyleSet,
    device: Device,
    interactions: InteractionStates<NodeId>,
    hovered: Option<NodeId>,
    focused: Option<NodeId>,
    styles: StylePlane<NodeId>,
    fragments: LiveryLayout<NodeId>,
    text: TextSystem,
    viewport: (f32, f32),
    viewport_scroll: (f32, f32),
    element_scroll: HashMap<NodeId, (f32, f32)>,
    content_extent: (f32, f32),
    generation: u64,
}

impl OwnedLayout {
    pub(crate) fn new<D: LayoutDom<NodeId = NodeId>>(
        dom: &D,
        sheets: &[&str],
        width: f32,
        height: f32,
    ) -> Self {
        let style_set = StyleSet::cambium(sheets);
        let device = Device::screen(width, height);
        let interactions = InteractionStates::default();
        let styles = resolve_styles(dom, &style_set, &device, &interactions);
        let mut text = TextSystem::new();
        let (styles, fragments) = layout_with_text_system(
            dom,
            &styles,
            width,
            height,
            ViewportSizes::uniform(width, height),
            &mut text,
            &HashMap::new(),
        )
        .expect("Cambium's authored Livery layout must resolve");
        let content_extent = content_extent(dom, &fragments);
        Self {
            style_set,
            device,
            interactions,
            hovered: None,
            focused: None,
            styles,
            fragments,
            text,
            viewport: (width, height),
            viewport_scroll: (0.0, 0.0),
            element_scroll: HashMap::new(),
            content_extent,
            generation: 1,
        }
    }

    pub(crate) fn rebuild<D: LayoutDom<NodeId = NodeId>>(
        &mut self,
        dom: &D,
        width: f32,
        height: f32,
    ) {
        self.viewport = (width, height);
        self.device.set_viewport_size(width, height);
        let styles = resolve_styles(dom, &self.style_set, &self.device, &self.interactions);
        let (styles, fragments) = layout_with_text_system(
            dom,
            &styles,
            width,
            height,
            self.device.viewport_sizes,
            &mut self.text,
            &HashMap::new(),
        )
        .expect("Cambium's authored Livery layout must resolve");
        self.styles = styles;
        self.fragments = fragments;
        self.content_extent = content_extent(dom, &self.fragments);
        self.clamp_viewport_scroll();
        self.generation = self.generation.saturating_add(1);
    }

    pub fn fragments(&self) -> &LiveryLayout<NodeId> {
        &self.fragments
    }

    pub(crate) fn has_active_animations(&self) -> bool {
        false
    }

    pub(crate) fn tick_animations<D: LayoutDom<NodeId = NodeId>>(
        &mut self,
        _dom: &D,
        _now: f64,
    ) -> bool {
        false
    }

    pub(crate) fn set_interaction<D: LayoutDom<NodeId = NodeId>>(
        &mut self,
        dom: &D,
        hovered: Option<NodeId>,
        focused: Option<NodeId>,
    ) -> bool {
        let mut changed = false;
        if self.hovered != hovered {
            if let Some(old) = self.hovered {
                changed |= self.interactions.set(old, StatePseudoClass::Hover, false);
            }
            if let Some(next) = hovered {
                changed |= self.interactions.set(next, StatePseudoClass::Hover, true);
            }
            self.hovered = hovered;
        }
        if self.focused != focused {
            if let Some(old) = self.focused {
                changed |= self.interactions.set(old, StatePseudoClass::Focus, false);
            }
            if let Some(next) = focused {
                changed |= self.interactions.set(next, StatePseudoClass::Focus, true);
            }
            self.focused = focused;
        }
        if changed {
            self.rebuild(dom, self.viewport.0, self.viewport.1);
        }
        changed
    }

    pub fn element_scroll(&self) -> &HashMap<NodeId, (f32, f32)> {
        &self.element_scroll
    }

    pub(crate) fn set_element_scroll(&mut self, scroll: HashMap<NodeId, (f32, f32)>) {
        self.element_scroll = scroll;
    }

    pub fn viewport_scroll(&self) -> (f32, f32) {
        self.viewport_scroll
    }

    pub(crate) fn set_viewport_scroll(&mut self, scroll: (f32, f32)) {
        self.viewport_scroll = scroll;
        self.clamp_viewport_scroll();
    }

    pub(crate) fn custom_leaf_boxes<D: LayoutDom<NodeId = NodeId>>(
        &self,
        dom: &D,
    ) -> Vec<(u64, (f32, f32))> {
        let mut leaves = Vec::new();
        walk(dom, dom.document(), &mut |node| {
            if let Some(key) = custom_leaf_key(dom, node)
                && let Some(fragment) = self.fragments.get(node)
            {
                leaves.push((key, (fragment.width, fragment.height)));
            }
        });
        leaves
    }

    pub(crate) fn hit_test<D: LayoutDom<NodeId = NodeId>>(
        &self,
        dom: &D,
        x: f32,
        y: f32,
    ) -> Option<NodeId> {
        hit_test_with_scroll(
            dom,
            &self.styles,
            &self.fragments,
            &self.element_scroll,
            x + self.viewport_scroll.0,
            y + self.viewport_scroll.1,
        )
    }

    pub fn painted_rect<D: LayoutDom<NodeId = NodeId>>(
        &self,
        dom: &D,
        node: NodeId,
    ) -> Option<(f32, f32, f32, f32)> {
        let fragment = self.fragments.get(node)?;
        let (nested_x, nested_y) = ancestor_scroll(dom, node, &self.element_scroll);
        Some((
            fragment.x - self.viewport_scroll.0 - nested_x,
            fragment.y - self.viewport_scroll.1 - nested_y,
            fragment.width,
            fragment.height,
        ))
    }

    pub(crate) fn caret_position_at_point<D: LayoutDom<NodeId = NodeId>>(
        &self,
        _dom: &D,
        _node: NodeId,
        x: f32,
        y: f32,
    ) -> Option<VisualCaret> {
        self.fragments
            .text_position_at_point(x + self.viewport_scroll.0, y + self.viewport_scroll.1)
            .map(|(_, byte)| VisualCaret {
                byte,
                affinity: VisualAffinity::Downstream,
            })
    }

    pub(crate) fn caret_rect_for_position<D: LayoutDom<NodeId = NodeId>>(
        &self,
        _dom: &D,
        node: NodeId,
        caret: VisualCaret,
        width: f32,
    ) -> Option<genet_livery::TextRect> {
        let mut rect = self.fragments.caret_rect(node, caret.byte)?;
        rect.x -= self.viewport_scroll.0;
        rect.y -= self.viewport_scroll.1;
        rect.width = width;
        Some(rect)
    }

    pub(crate) fn selection_rects<D: LayoutDom<NodeId = NodeId>>(
        &self,
        _dom: &D,
        node: NodeId,
        start: usize,
        end: usize,
    ) -> Vec<genet_livery::TextRect> {
        self.fragments
            .text_selection(TextRange {
                anchor_node: node,
                anchor_offset: start,
                focus_node: node,
                focus_offset: end,
            })
            .map(|mut selection| {
                for rect in &mut selection.rects {
                    rect.x -= self.viewport_scroll.0;
                    rect.y -= self.viewport_scroll.1;
                }
                selection.rects
            })
            .unwrap_or_default()
    }

    pub(crate) fn selection_visual_move<D: LayoutDom<NodeId = NodeId>>(
        &self,
        dom: &D,
        node: NodeId,
        selection: VisualSelection,
        movement: VisualMovement,
        extend: bool,
    ) -> Option<VisualSelection> {
        let text = node_text(dom, node);
        let len = text.len();
        let mut byte = selection.focus.byte.min(len);
        byte = match movement {
            VisualMovement::PreviousCluster => previous_boundary(&text, byte),
            VisualMovement::NextCluster => next_boundary(&text, byte),
            VisualMovement::PreviousWord => previous_word(&text, byte),
            VisualMovement::NextWord => next_word(&text, byte),
            VisualMovement::LineStart => 0,
            VisualMovement::LineEnd => len,
            VisualMovement::PreviousLine | VisualMovement::NextLine => return None,
        };
        let focus = VisualCaret {
            byte,
            affinity: VisualAffinity::Downstream,
        };
        Some(VisualSelection {
            anchor: if extend { selection.anchor } else { focus },
            focus,
        })
    }

    pub fn computed_value(&self, node: NodeId, property: &str) -> Option<String> {
        self.styles.computed_style(node, property)
    }

    pub fn computed_custom_property(&self, node: NodeId, name: &str) -> Option<String> {
        let properties = self.styles.custom_properties(node)?;
        properties
            .get(&format!("--{name}"))
            .or_else(|| properties.get(name))
            .cloned()
    }

    pub(crate) fn selection_style<D: LayoutDom<NodeId = NodeId>>(
        &self,
        _dom: &D,
        _node: NodeId,
    ) -> Option<([f32; 4], [f32; 4])> {
        None
    }

    pub(crate) fn caret_color<D: LayoutDom<NodeId = NodeId>>(
        &self,
        _dom: &D,
        _node: NodeId,
    ) -> Option<[f32; 4]> {
        None
    }

    pub(crate) fn emit_paint_list_with_leaves<D, F, G>(
        &mut self,
        dom: &D,
        viewport: DeviceIntSize,
        mut commands: F,
        mut fragment: G,
    ) -> LiveryPaintList
    where
        D: LayoutDom<NodeId = NodeId>,
        F: FnMut(u64) -> Option<Vec<paint_list_api::PaintCmd>>,
        G: FnMut(u64) -> Option<u64>,
    {
        let mut list = emit_paint_list_with_text_system_scrolled_with_images(
            dom,
            &self.styles,
            &self.fragments,
            viewport,
            self.generation,
            &mut self.text,
            &self.element_scroll,
            &HashMap::new(),
        );
        // Slots were recorded against the un-translated list. Fill them before
        // the document viewport transform is added so their indices and their
        // CSS paint context stay aligned.
        list.splice_host_leaf_slots(&mut commands, &mut fragment);
        list.translated(-self.viewport_scroll.0, -self.viewport_scroll.1)
    }

    pub(crate) fn push_rect(
        list: &mut LiveryPaintList,
        rect: genet_livery::TextRect,
        color: ColorF,
    ) {
        list.push_overlay_rect(
            LayoutRect::from_origin_and_size(
                LayoutPoint::new(rect.x, rect.y),
                LayoutSize::new(rect.width, rect.height),
            ),
            color,
        );
    }

    pub(crate) fn append_scrollbars<D: LayoutDom<NodeId = NodeId>>(
        &self,
        _dom: &D,
        _list: &mut LiveryPaintList,
        _alpha: &impl Fn(ScrollTarget) -> f32,
    ) {
    }

    pub(crate) fn scroll_at_target<D: LayoutDom<NodeId = NodeId>>(
        &mut self,
        dom: &D,
        x: f32,
        y: f32,
        dx: f32,
        dy: f32,
    ) -> Option<ScrollTarget> {
        let mut candidate = self.hit_test(dom, x, y);
        while let Some(node) = candidate {
            let overflow_x = self.styles.computed_style(node, "overflow-x");
            let overflow_y = self.styles.computed_style(node, "overflow-y");
            let scrolls_x = overflow_x
                .as_deref()
                .is_some_and(|value| matches!(value, "auto" | "scroll"));
            let scrolls_y = overflow_y
                .as_deref()
                .is_some_and(|value| matches!(value, "auto" | "scroll"));
            if scrolls_x || scrolls_y {
                let range = element_scroll_range(dom, &self.fragments, node);
                let current = self.element_scroll.get(&node).copied().unwrap_or_default();
                let next = (
                    if scrolls_x {
                        (current.0 + dx).clamp(0.0, range.0)
                    } else {
                        current.0
                    },
                    if scrolls_y {
                        (current.1 + dy).clamp(0.0, range.1)
                    } else {
                        current.1
                    },
                );
                if next != current {
                    self.element_scroll.insert(node, next);
                    return Some(ScrollTarget::Element(node));
                }
            }
            candidate = dom.parent(node);
        }
        let before = self.viewport_scroll;
        self.viewport_scroll.0 += dx;
        self.viewport_scroll.1 += dy;
        self.clamp_viewport_scroll();
        (self.viewport_scroll != before).then_some(ScrollTarget::Document)
    }

    fn clamp_viewport_scroll(&mut self) {
        self.viewport_scroll.0 = self
            .viewport_scroll
            .0
            .clamp(0.0, (self.content_extent.0 - self.viewport.0).max(0.0));
        self.viewport_scroll.1 = self
            .viewport_scroll
            .1
            .clamp(0.0, (self.content_extent.1 - self.viewport.1).max(0.0));
    }
}

fn element_scroll_range<D: LayoutDom<NodeId = NodeId>>(
    dom: &D,
    fragments: &LiveryLayout<NodeId>,
    node: NodeId,
) -> (f32, f32) {
    let Some(container) = fragments.get(node) else {
        return (0.0, 0.0);
    };
    let mut extent = (
        container.x + container.width,
        container.y + container.height,
    );
    for child in dom.dom_children(node) {
        walk(dom, child, &mut |descendant| {
            if let Some(fragment) = fragments.get(descendant) {
                extent.0 = extent.0.max(fragment.x + fragment.width);
                extent.1 = extent.1.max(fragment.y + fragment.height);
            }
        });
    }
    (
        (extent.0 - container.x - container.width).max(0.0),
        (extent.1 - container.y - container.height).max(0.0),
    )
}

fn content_extent<D: LayoutDom<NodeId = NodeId>>(
    dom: &D,
    fragments: &LiveryLayout<NodeId>,
) -> (f32, f32) {
    let mut extent: (f32, f32) = (0.0, 0.0);
    walk(dom, dom.document(), &mut |node| {
        if let Some(fragment) = fragments.get(node) {
            extent.0 = extent.0.max(fragment.x + fragment.width);
            extent.1 = extent.1.max(fragment.y + fragment.height);
        }
    });
    extent
}

fn walk<D: LayoutDom<NodeId = NodeId>>(dom: &D, node: NodeId, visit: &mut impl FnMut(NodeId)) {
    visit(node);
    for child in dom.dom_children(node) {
        walk(dom, child, visit);
    }
}

fn custom_leaf_key<D: LayoutDom<NodeId = NodeId>>(dom: &D, node: NodeId) -> Option<u64> {
    if dom.kind(node) != NodeKind::Element
        || !matches!(
            dom.element_name(node)?.local.as_ref(),
            "custom-leaf" | "chisel-leaf"
        )
    {
        return None;
    }
    dom.attribute(node, &Namespace::default(), &LocalName::from("key"))?
        .parse()
        .ok()
}

fn ancestor_scroll<D: LayoutDom<NodeId = NodeId>>(
    dom: &D,
    node: NodeId,
    scroll: &HashMap<NodeId, (f32, f32)>,
) -> (f32, f32) {
    let mut total = (0.0, 0.0);
    let mut current = dom.parent(node);
    while let Some(parent) = current {
        if let Some((x, y)) = scroll.get(&parent) {
            total.0 += x;
            total.1 += y;
        }
        current = dom.parent(parent);
    }
    total
}

fn node_text<D: LayoutDom<NodeId = NodeId>>(dom: &D, node: NodeId) -> String {
    if let Some(text) = dom.text(node) {
        return text.to_owned();
    }
    let mut text = String::new();
    for child in dom.dom_children(node) {
        text.push_str(&node_text(dom, child));
    }
    text
}

fn previous_boundary(text: &str, byte: usize) -> usize {
    text[..byte]
        .char_indices()
        .next_back()
        .map_or(0, |(index, _)| index)
}

fn next_boundary(text: &str, byte: usize) -> usize {
    text[byte..]
        .char_indices()
        .nth(1)
        .map_or(text.len(), |(index, _)| byte + index)
}

fn previous_word(text: &str, byte: usize) -> usize {
    let prefix = &text[..byte];
    let trimmed = prefix.trim_end_matches(char::is_whitespace);
    trimmed
        .rfind(char::is_whitespace)
        .map_or(0, |index| index + 1)
}

fn next_word(text: &str, byte: usize) -> usize {
    let suffix = &text[byte..];
    let skipped = suffix
        .char_indices()
        .find(|(_, ch)| !ch.is_whitespace())
        .map_or(suffix.len(), |(index, _)| index);
    let rest = &suffix[skipped..];
    byte + skipped
        + rest
            .char_indices()
            .find(|(_, ch)| ch.is_whitespace())
            .map_or(rest.len(), |(index, _)| index)
}
