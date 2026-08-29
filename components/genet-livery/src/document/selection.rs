//! Text selection, caret geometry, links, and pointer activation.

use super::*;

impl<D> LiveryDocument<D>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    /// A retained caret rectangle in viewport coordinates.
    pub fn caret_rect(&self, node: D::NodeId, byte: usize) -> Option<TextRect> {
        if !self.dom.is_live(node) {
            return None;
        }
        let frame = self
            .layout
            .as_ref()
            .and_then(|layout| layout.fragments.text_frame())?;
        frame.caret_rect(node, byte, |source, fragment| {
            self.viewport_text_rect(source, fragment)
        })
    }

    /// Resolve a viewport CSS point to a retained shaped-text source and byte
    /// offset from the last frame. This is a read-only query: it does not
    /// create or replace the document selection.
    pub fn text_position_at_point(&self, x: f32, y: f32) -> Option<(D::NodeId, usize)> {
        let frame = self
            .layout
            .as_ref()
            .and_then(|layout| layout.fragments.text_frame())?;
        let position = frame.text_position_at_point(x, y, |source, fragment| {
            self.viewport_text_rect(source, fragment)
        })?;
        self.dom.is_live(position.0).then_some(position)
    }

    /// Retained viewport rectangles for a directed source range.
    pub fn selection_for_range(
        &self,
        range: TextRange<D::NodeId>,
    ) -> Option<TextSelection<D::NodeId>> {
        if !self.dom.is_live(range.anchor_node) || !self.dom.is_live(range.focus_node) {
            return None;
        }
        let frame = self
            .layout
            .as_ref()
            .and_then(|layout| layout.fragments.text_frame())?;
        frame.text_selection(range, |source, fragment| {
            self.viewport_text_rect(source, fragment)
        })
    }

    pub fn links(&self) -> Vec<LinkTarget> {
        let Some(layout) = self.layout.as_ref() else {
            return Vec::new();
        };
        let mut links = Vec::new();
        self.collect_links(self.dom.document(), layout, &mut links);
        links
    }

    /// Begin a primary-pointer text selection against the retained shaped
    /// clusters from the last frame.
    pub fn begin_text_selection(&mut self, x: f32, y: f32) -> bool {
        self.selection_range = None;
        self.selection_anchor = {
            let Some(frame) = self
                .layout
                .as_ref()
                .and_then(|layout| layout.fragments.text_frame())
            else {
                return false;
            };
            frame.text_position_at_point(x, y, |source, fragment| {
                self.viewport_text_rect(source, fragment)
            })
        };
        self.selection_anchor.is_some()
    }

    /// Extend the current primary-pointer selection.
    pub fn extend_text_selection(&mut self, x: f32, y: f32) -> bool {
        let Some(anchor) = self.selection_anchor else {
            return false;
        };
        let focus = {
            let Some(frame) = self
                .layout
                .as_ref()
                .and_then(|layout| layout.fragments.text_frame())
            else {
                return false;
            };
            frame.text_position_at_point(x, y, |source, fragment| {
                self.viewport_text_rect(source, fragment)
            })
        };
        let Some(focus) = focus else {
            return false;
        };
        let next = TextRange {
            anchor_node: anchor.0,
            anchor_offset: anchor.1,
            focus_node: focus.0,
            focus_offset: focus.1,
        };
        if self.selection_range == Some(next) {
            return false;
        }
        self.selection_range = Some(next);
        true
    }

    /// Finish the current selection. A collapsed gesture clears the range and
    /// lets the session perform the ordinary click action.
    pub fn finish_text_selection(&mut self, x: f32, y: f32) -> bool {
        self.extend_text_selection(x, y);
        self.selection_anchor = None;
        if self.text_selection().is_some() {
            true
        } else {
            self.selection_range = None;
            false
        }
    }

    /// Recompute the selected text and viewport geometry from the retained
    /// source range.
    pub fn text_selection(&self) -> Option<TextSelection<D::NodeId>> {
        let range = self.selection_range?;
        if !self.dom.is_live(range.anchor_node) || !self.dom.is_live(range.focus_node) {
            return None;
        }
        let frame = self
            .layout
            .as_ref()
            .and_then(|layout| layout.fragments.text_frame())?;
        frame.text_selection(range, |source, fragment| {
            self.viewport_text_rect(source, fragment)
        })
    }

    /// Replace the visible retained text range without synthesizing pointer
    /// input. Document-session capabilities such as find own this selection;
    /// ordinary gesture selection continues through `begin`/`extend`/`finish`.
    pub fn select_text_range(&mut self, range: Option<TextRange<D::NodeId>>) {
        self.selection_anchor = None;
        self.selection_range = range;
    }

    /// Resolve the first matching pending URL Text Directive, select its retained
    /// range, and reveal its first shaped rectangle. A successful match that is
    /// already visible still returns `true`; callers use that to avoid falling
    /// through to an ordinary element fragment.
    pub fn activate_text_directives(&mut self, directives: &[TextDirective]) -> bool {
        let Some(range) = self
            .layout
            .as_ref()
            .and_then(|layout| layout.fragments.text_frame())
            .and_then(|frame| {
                directives
                    .iter()
                    .find_map(|directive| frame.find_text_directive_range(directive))
            })
        else {
            return false;
        };
        let Some(selection) = self.selection_for_range(range) else {
            return false;
        };
        let Some(first) = selection.rects.first() else {
            return false;
        };
        let target_y = (self.scroll.1 + first.y - 24.0).max(0.0);
        self.select_text_range(Some(range));
        self.scroll_to(target_y);
        true
    }

    /// Reveal an ordinary element-id fragment. This remains the fallback when
    /// no pending Text Directive resolves against the retained document.
    pub fn scroll_to_element_fragment(&mut self, fragment: &str) -> bool {
        let Some(target) = find_id(&self.dom, self.dom.document(), fragment) else {
            return false;
        };
        // Link activation may focus the anchor first, which moves the live
        // layout into K5g's identity source before this fragment lookup.
        // That retained geometry is still the current hit-test frame.
        let Some(y) = self
            .layout
            .as_ref()
            .or(self.identity_source.as_ref())
            .and_then(|layout| layout.fragments.get(target).map(|fragment| fragment.y))
        else {
            return false;
        };
        self.scroll_to(y);
        true
    }

    /// Link URLs whose descendant text contributes to this selection.
    pub fn links_for_selection(&self, selection: &TextSelection<D::NodeId>) -> Vec<String> {
        let mut links = Vec::new();
        for source in &selection.source_nodes {
            if let Some(href) = self.link_ancestor(*source)
                && !links.contains(&href)
            {
                links.push(href);
            }
        }
        links
    }

    /// Resolve the first retained occurrence of `text` to viewport pointer
    /// endpoints for Genet Probe and find-to-select consumers.
    pub fn text_target(&self, text: &str) -> Option<([f32; 2], [f32; 2])> {
        let frame = self
            .layout
            .as_ref()
            .and_then(|layout| layout.fragments.text_frame())?;
        let range = frame.find_text_range(text)?;
        let anchor = frame.caret_rect(
            range.anchor_node,
            range.anchor_offset,
            |source, fragment| self.viewport_text_rect(source, fragment),
        )?;
        let focus =
            frame.caret_rect(range.focus_node, range.focus_offset, |source, fragment| {
                self.viewport_text_rect(source, fragment)
            })?;
        Some((
            [anchor.x, anchor.y + anchor.height * 0.5],
            [focus.x, focus.y + focus.height * 0.5],
        ))
    }

    pub fn click_at(&mut self, x: f32, y: f32) -> ClickOutcome {
        let Some(target) = self.hit_test(x, y) else {
            return ClickOutcome::None;
        };
        let focus_target = self.focusable_ancestor(target);
        let focused = focus_target.is_some_and(|id| self.focus(id));
        let href = self.link_ancestor(target);
        if let Some(href) = href {
            if let Some(fragment) = href
                .strip_prefix('#')
                .filter(|fragment| !fragment.is_empty())
                && self.scroll_to_element_fragment(fragment)
            {
                return ClickOutcome::Scrolled;
            }
            return ClickOutcome::Navigate(href);
        }
        if focused {
            ClickOutcome::Focused
        } else {
            ClickOutcome::None
        }
    }

    pub(in crate::document) fn viewport_text_rect(
        &self,
        source: D::NodeId,
        fragment: crate::layout::Fragment,
    ) -> TextRect {
        if !self.dom.is_live(source) {
            // A retained frame may briefly contain a source removed by a DOM
            // mutation. Public text queries remain read-only and must treat
            // that stale source as outside every viewport hit.
            return TextRect {
                x: f32::MAX * 0.25,
                y: f32::MAX * 0.25,
                width: 0.0,
                height: 0.0,
            };
        }
        let (nested_x, nested_y) = self.ancestor_scroll(source);
        TextRect {
            x: fragment.x - self.scroll.0 - nested_x,
            y: fragment.y - self.scroll.1 - nested_y,
            width: fragment.width,
            height: fragment.height,
        }
    }

    pub(in crate::document) fn link_ancestor(&self, mut id: D::NodeId) -> Option<String> {
        loop {
            if self.dom.kind(id) == NodeKind::Element
                && self
                    .dom
                    .element_name(id)
                    .is_some_and(|name| name.local.as_ref().eq_ignore_ascii_case("a"))
                && let Some(href) = self.attribute(id, "href")
            {
                return Some(href.to_owned());
            }
            id = self.dom.parent(id)?;
        }
    }

    pub(in crate::document) fn collect_links(
        &self,
        id: D::NodeId,
        layout: &LayoutState<D::NodeId>,
        links: &mut Vec<LinkTarget>,
    ) {
        if self.dom.kind(id) == NodeKind::Element
            && let Some(href) = self.attribute(id, "href")
            && let Some(fragment) = layout.fragments.get(id)
            && let Some(style) = layout.styles.get(id)
            && style.display != livery::values::Display::None
            && style.visibility == livery::values::Visibility::Visible
            && style.pointer_events == livery::values::PointerEvents::Auto
        {
            let (nested_x, nested_y) = self.ancestor_scroll(id);
            links.push(LinkTarget {
                url: href.to_owned(),
                rect: [
                    fragment.x - self.scroll.0 - nested_x,
                    fragment.y - self.scroll.1 - nested_y,
                    fragment.width,
                    fragment.height,
                ],
            });
        }
        for child in self.dom.dom_children(id) {
            self.collect_links(child, layout, links);
        }
    }
}

pub(in crate::document) fn find_id<D: LayoutDom>(
    dom: &D,
    id: D::NodeId,
    target: &str,
) -> Option<D::NodeId> {
    if dom.kind(id) == NodeKind::Element
        && dom
            .attribute(id, &Namespace::from(""), &LocalName::from("id"))
            .is_some_and(|value| value == target)
    {
        return Some(id);
    }
    dom.dom_children(id)
        .find_map(|child| find_id(dom, child, target))
}
