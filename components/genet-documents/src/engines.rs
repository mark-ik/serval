/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! The lanes as inker session engines: `SessionEngine<Scene>` /
//! `DocumentSession<Scene>` impls wrapping [`LoadedDocument`], the scripted
//! document, and [`SmolwebDocument`](crate::SmolwebDocument).
//!
//! Construction seams (fetchers, themes, cookie jars) live on the engine at
//! registration time; the spawn request stays plain data (session-engines
//! plan, review-resolved 2026-07-10).

use std::any::Any;

#[cfg(feature = "livery")]
use genet_document_resources::{
    ResolvedDocumentResources, ResolvedStylesheet, ResourceDelta, ResourceKind, ResourceLimits,
    StylesheetOwner,
};
use genet_host_api::ResourceFetcher;
#[cfg(feature = "livery")]
use genet_host_api::ResourceResponse;
use inker::session_engine::{
    DocumentClip, DocumentClipArtifact, DocumentClipArtifactRole, DocumentFindDirection,
    DocumentFindMatch, DocumentFindQuery, DocumentFindReveal, DocumentFindState, DocumentSession,
    SessionButtonState, SessionClick, SessionCursor, SessionEffect, SessionEngine, SessionError,
    SessionFocusDirection, SessionFormMethod, SessionFormSubmission, SessionIme, SessionKey,
    SessionLink, SessionModifiers, SessionScrollKey, SessionSpawnRequest, SessionTextTarget,
};
use inker::{DocumentCapabilities, DocumentCapabilityStatus};
use layout_dom_api::{LayoutDom, LayoutDomMut, LocalName, Namespace, NodeKind, QualName};
use netrender::Scene;
#[cfg(feature = "livery")]
use unicode_segmentation::UnicodeSegmentation;

fn retained_document_capabilities(find_reason: impl Into<String>) -> DocumentCapabilities {
    DocumentCapabilities {
        find_in_page: DocumentCapabilityStatus::unsupported(find_reason),
        page_zoom: DocumentCapabilityStatus::unsupported(
            "retained sessions do not expose page zoom",
        ),
        page_capture: DocumentCapabilityStatus::unsupported(
            "retained sessions do not capture rendered pages",
        ),
        navigation: DocumentCapabilityStatus::Partial {
            detail: "the host owns document lineage, policy, and refetch".into(),
        },
    }
}

/// Map the host-neutral scroll-key vocabulary onto the owned scripted lane.
#[cfg(feature = "scripted")]
pub(crate) fn scripted_scroll_key(key: SessionScrollKey) -> genet_scripted::ScrollKey {
    match key {
        SessionScrollKey::LineUp => genet_scripted::ScrollKey::Up,
        SessionScrollKey::LineDown => genet_scripted::ScrollKey::Down,
        SessionScrollKey::PageUp => genet_scripted::ScrollKey::PageUp,
        SessionScrollKey::PageDown => genet_scripted::ScrollKey::PageDown,
        SessionScrollKey::Home => genet_scripted::ScrollKey::Home,
        SessionScrollKey::End => genet_scripted::ScrollKey::End,
    }
}

// ── Script-free HTML lane (genet.livery) ─────────────────────────────────

/// Session engine for the owned Livery CSS and Buckram layout path.
#[cfg(feature = "livery")]
pub struct LiverySessionEngine<Fetch> {
    fetcher: Fetch,
    author_css: Vec<String>,
    resource_limits: ResourceLimits,
}

#[cfg(feature = "livery")]
impl<Fetch> LiverySessionEngine<Fetch> {
    pub fn new(fetcher: Fetch) -> Self {
        Self {
            fetcher,
            author_css: Vec::new(),
            resource_limits: ResourceLimits::default(),
        }
    }

    /// Add host-supplied author sheets before the document's own inline
    /// sheets. This keeps lane policy configurable at registration time.
    pub fn with_author_css(
        fetcher: Fetch,
        sheets: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            fetcher,
            author_css: sheets.into_iter().map(Into::into).collect(),
            resource_limits: ResourceLimits::default(),
        }
    }

    /// Select bounded recursive stylesheet resolution for sessions made by
    /// this host engine.
    pub fn with_resource_limits(mut self, resource_limits: ResourceLimits) -> Self {
        self.resource_limits = resource_limits;
        self
    }
}

#[cfg(feature = "livery")]
impl<Fetch: ResourceFetcher + Send + Sync> SessionEngine<Scene> for LiverySessionEngine<Fetch> {
    fn engine_id(&self) -> &str {
        inker::routing::ENGINE_GENET_LIVERY
    }

    fn spawn(
        &self,
        request: &SessionSpawnRequest,
    ) -> Result<Box<dyn DocumentSession<Scene>>, SessionError> {
        let navigation = genet_livery::NavigationFragment::parse(&request.address);
        let requested_resource = navigation.resource_url.as_str();
        let source_response = match &request.body {
            Some(body) => ResourceResponse {
                final_url: request.address.clone(),
                content_type: request
                    .content_type
                    .clone()
                    .or_else(|| Some("text/html".to_string())),
                bytes: body.as_bytes().to_vec(),
            },
            None => self
                .fetcher
                .fetch_response(requested_resource)
                .ok_or_else(|| {
                    SessionError::SpawnFailed(format!("could not load {requested_resource}"))
                })?,
        };
        let base_resource = source_response
            .final_url
            .split_once('#')
            .map_or(source_response.final_url.as_str(), |(resource, _)| resource)
            .to_owned();
        let source = String::from_utf8_lossy(&source_response.bytes).into_owned();
        // Script-free HTML still uses the mutable DOM backing. Form controls
        // need one retained value plane even when JavaScript is disabled;
        // ScriptedDom supplies LayoutDomMut without constructing a JS runtime.
        let dom = genet_scripted_dom::ScriptedDom::from_serialized_document(&source);
        self.spawn_livery_document(request, dom, base_resource, source_response, navigation)
    }
}

#[cfg(feature = "livery")]
impl<Fetch: ResourceFetcher + Send + Sync> LiverySessionEngine<Fetch> {
    fn spawn_livery_document(
        &self,
        request: &SessionSpawnRequest,
        dom: genet_scripted_dom::ScriptedDom,
        base_resource: String,
        source_response: ResourceResponse,
        navigation: genet_livery::NavigationFragment,
    ) -> Result<Box<dyn DocumentSession<Scene>>, SessionError> {
        let resources = ResolvedDocumentResources::resolve_with_limits(
            &dom,
            Some(&base_resource),
            &self.fetcher,
            self.resource_limits,
        );
        let mut sheets = self
            .author_css
            .iter()
            .enumerate()
            .map(|(document_order, text)| ResolvedStylesheet {
                // Resource-resolved sheet ids begin at zero. Keep injected
                // host sheets in a disjoint range when the vectors join.
                sheet_id: u64::MAX.saturating_sub(document_order as u64),
                owner: StylesheetOwner::Inline,
                owner_node: None,
                source_url: None,
                requested_url: None,
                content_type: None,
                media: None,
                imports: Vec::new(),
                import_parent: None,
                text: text.clone(),
                document_order: document_order as u64,
            })
            .collect::<Vec<_>>();
        sheets.extend(resources.stylesheets.iter().cloned());
        let (width, height) = request.viewport;
        let mut doc = genet_livery::LiveryDocument::new(
            dom,
            genet_livery::StyleSet::cambium_resources(&sheets),
            genet_livery::Device::screen(width as f32, height as f32),
        );
        for resource in &resources.resources {
            match resource.kind {
                ResourceKind::Image => {
                    doc.set_image_resource(resource.authored_url.clone(), resource.bytes.clone());
                    if resource.resolved_url != resource.authored_url {
                        doc.set_image_resource(
                            resource.resolved_url.clone(),
                            resource.bytes.clone(),
                        );
                    }
                },
                ResourceKind::Font => {
                    doc.set_font_resource(resource.resolved_url.clone(), resource.bytes.clone());
                },
            }
        }
        Ok(Box::new(LiveryDocumentSession {
            doc,
            address: navigation.script_visible_url.clone(),
            focused_node: None,
            editor: None,
            active_form: None,
            pressed_submit: None,
            last_error: None,
            resources,
            source_response,
            find_state: DocumentFindState::empty(DocumentFindQuery::default()),
            find_ranges: Vec::new(),
            pending_fragment: (!navigation.text_directives.is_empty()
                || navigation.element_fragment.is_some())
            .then_some(navigation),
        }))
    }
}

/// Retained Livery document session. The document owns the resolved style and
/// fragment planes, so this adapter only translates the session contract.
#[cfg(feature = "livery")]
pub struct LiveryDocumentSession {
    doc: genet_livery::LiveryDocument<genet_scripted_dom::ScriptedDom>,
    address: String,
    focused_node: Option<genet_scripted_dom::NodeId>,
    editor: Option<EditableControl>,
    active_form: Option<genet_scripted_dom::NodeId>,
    pressed_submit: Option<genet_scripted_dom::NodeId>,
    last_error: Option<String>,
    resources: ResolvedDocumentResources,
    source_response: ResourceResponse,
    find_state: DocumentFindState,
    find_ranges: Vec<genet_livery::TextRange<genet_scripted_dom::NodeId>>,
    pending_fragment: Option<genet_livery::NavigationFragment>,
}

#[cfg(feature = "livery")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EditableKind {
    Input,
    Textarea,
}

#[cfg(feature = "livery")]
struct EditableControl {
    node: genet_scripted_dom::NodeId,
    kind: EditableKind,
    value: String,
    caret: usize,
    composition: Option<String>,
}

#[cfg(feature = "livery")]
impl LiveryDocumentSession {
    pub fn document(&self) -> &genet_livery::LiveryDocument<genet_scripted_dom::ScriptedDom> {
        &self.doc
    }

    fn attribute(&self, node: genet_scripted_dom::NodeId, name: &str) -> Option<&str> {
        self.doc
            .dom()
            .attribute(node, &Namespace::default(), &LocalName::from(name))
    }

    fn tag(&self, node: genet_scripted_dom::NodeId) -> Option<&str> {
        self.doc
            .dom()
            .element_name(node)
            .map(|name| name.local.as_ref())
    }

    fn ancestor_matching(
        &self,
        mut node: genet_scripted_dom::NodeId,
        predicate: impl Fn(genet_scripted_dom::NodeId, &str) -> bool,
    ) -> Option<genet_scripted_dom::NodeId> {
        loop {
            if let Some(tag) = self.tag(node)
                && predicate(node, tag)
            {
                return Some(node);
            }
            node = self.doc.dom().parent(node)?;
        }
    }

    fn form_ancestor(
        &self,
        node: genet_scripted_dom::NodeId,
    ) -> Option<genet_scripted_dom::NodeId> {
        self.ancestor_matching(node, |_node, tag| tag.eq_ignore_ascii_case("form"))
    }

    fn editable_kind(&self, node: genet_scripted_dom::NodeId) -> Option<EditableKind> {
        match self.tag(node)? {
            tag if tag.eq_ignore_ascii_case("textarea") => Some(EditableKind::Textarea),
            tag if tag.eq_ignore_ascii_case("input") => {
                let kind = self.attribute(node, "type").unwrap_or("text");
                (!matches!(
                    kind.to_ascii_lowercase().as_str(),
                    "button"
                        | "checkbox"
                        | "file"
                        | "hidden"
                        | "image"
                        | "radio"
                        | "reset"
                        | "submit"
                ))
                .then_some(EditableKind::Input)
            },
            _ => None,
        }
    }

    fn editable_ancestor(
        &self,
        node: genet_scripted_dom::NodeId,
    ) -> Option<(genet_scripted_dom::NodeId, EditableKind)> {
        let node = self.ancestor_matching(node, |node, _tag| self.editable_kind(node).is_some())?;
        Some((node, self.editable_kind(node)?))
    }

    fn is_submit_control(&self, node: genet_scripted_dom::NodeId) -> bool {
        match self.tag(node) {
            Some(tag) if tag.eq_ignore_ascii_case("button") => {
                !self.attribute(node, "type").is_some_and(|kind| {
                    matches!(kind.to_ascii_lowercase().as_str(), "button" | "reset")
                })
            },
            Some(tag) if tag.eq_ignore_ascii_case("input") => {
                self.attribute(node, "type").is_some_and(|kind| {
                    matches!(kind.to_ascii_lowercase().as_str(), "submit" | "image")
                })
            },
            _ => false,
        }
    }

    fn submit_ancestor(
        &self,
        node: genet_scripted_dom::NodeId,
    ) -> Option<genet_scripted_dom::NodeId> {
        self.ancestor_matching(node, |node, _tag| self.is_submit_control(node))
    }

    fn text_content(&self, node: genet_scripted_dom::NodeId) -> String {
        fn append(
            dom: &genet_scripted_dom::ScriptedDom,
            node: genet_scripted_dom::NodeId,
            out: &mut String,
        ) {
            if dom.kind(node) == NodeKind::Text
                && let Some(text) = dom.text(node)
            {
                out.push_str(text);
            }
            for child in dom.dom_children(node) {
                append(dom, child, out);
            }
        }

        let mut value = String::new();
        append(self.doc.dom(), node, &mut value);
        value
    }

    fn activate_editable(&mut self, node: genet_scripted_dom::NodeId, kind: EditableKind) {
        let value = match kind {
            EditableKind::Input => self.attribute(node, "value").unwrap_or("").to_owned(),
            EditableKind::Textarea => self.text_content(node),
        };
        let caret = value.len();
        self.focused_node = Some(node);
        self.active_form = self.form_ancestor(node);
        self.editor = Some(EditableControl {
            node,
            kind,
            value,
            caret,
            composition: None,
        });
    }

    fn activate_hit(&mut self, x: f32, y: f32) {
        let Some(hit) = self.doc.hit_test(x, y) else {
            self.focused_node = None;
            self.editor = None;
            self.active_form = None;
            return;
        };
        if let Some((node, kind)) = self.editable_ancestor(hit) {
            self.activate_editable(node, kind);
            return;
        }
        self.focused_node = self.ancestor_matching(hit, |node, tag| {
            tag.eq_ignore_ascii_case("button")
                || tag.eq_ignore_ascii_case("select")
                || tag.eq_ignore_ascii_case("a") && self.attribute(node, "href").is_some()
                || self.attribute(node, "tabindex").is_some()
        });
        self.editor = None;
        self.active_form = self.focused_node.and_then(|node| self.form_ancestor(node));
    }

    fn submission_for_form(
        &self,
        form: genet_scripted_dom::NodeId,
        fallback_action: &str,
    ) -> SessionFormSubmission {
        let action = self
            .attribute(form, "action")
            .filter(|action| !action.is_empty())
            .unwrap_or(fallback_action)
            .to_owned();
        let method = if self
            .attribute(form, "method")
            .is_some_and(|method| method.eq_ignore_ascii_case("post"))
        {
            SessionFormMethod::Post
        } else {
            SessionFormMethod::Get
        };
        SessionFormSubmission {
            action,
            method,
            fields: self.doc.dom().form_values(form),
        }
    }

    fn submit_click(&mut self, submit: genet_scripted_dom::NodeId) -> SessionClick {
        let Some(form) = self.form_ancestor(submit) else {
            return SessionClick::Handled;
        };
        self.active_form = Some(form);
        let action = self
            .attribute(form, "action")
            .unwrap_or(&self.address)
            .to_owned();
        SessionClick::Submit(action)
    }

    fn apply_editor(&mut self) {
        let Some(editor) = self.editor.as_ref() else {
            return;
        };
        let (node, kind, value) = (editor.node, editor.kind, editor.value.clone());
        let _ = self.doc.mutate_dom(|dom| match kind {
            EditableKind::Input => dom.set_attribute(
                node,
                QualName::new(None, Namespace::default(), LocalName::from("value")),
                &value,
            ),
            EditableKind::Textarea => dom.set_text_content(node, &value),
        });
    }

    fn insert_text(&mut self, text: &str) -> bool {
        if text.is_empty() {
            return false;
        }
        let Some(editor) = self.editor.as_mut() else {
            return false;
        };
        editor.value.insert_str(editor.caret, text);
        editor.caret += text.len();
        editor.composition = None;
        self.apply_editor();
        true
    }

    fn delete_backward(&mut self) -> bool {
        let Some(editor) = self.editor.as_mut() else {
            return false;
        };
        if editor.caret == 0 {
            return true;
        }
        let previous = editor.value[..editor.caret]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(index, _)| index);
        editor.value.replace_range(previous..editor.caret, "");
        editor.caret = previous;
        self.apply_editor();
        true
    }

    fn delete_forward(&mut self) -> bool {
        let Some(editor) = self.editor.as_mut() else {
            return false;
        };
        if editor.caret == editor.value.len() {
            return true;
        }
        let next = editor.value[editor.caret..]
            .grapheme_indices(true)
            .nth(1)
            .map_or(editor.value.len(), |(offset, _)| editor.caret + offset);
        editor.value.replace_range(editor.caret..next, "");
        self.apply_editor();
        true
    }

    fn move_caret(&mut self, direction: i8) -> bool {
        let Some(editor) = self.editor.as_mut() else {
            return false;
        };
        editor.caret = if direction < 0 {
            editor.value[..editor.caret]
                .grapheme_indices(true)
                .next_back()
                .map_or(0, |(index, _)| index)
        } else if editor.caret == editor.value.len() {
            editor.caret
        } else {
            editor.value[editor.caret..]
                .grapheme_indices(true)
                .nth(1)
                .map_or(editor.value.len(), |(offset, _)| editor.caret + offset)
        };
        true
    }

    fn collect_focusable(
        &self,
        node: genet_scripted_dom::NodeId,
        out: &mut Vec<genet_scripted_dom::NodeId>,
    ) {
        if let Some(tag) = self.tag(node)
            && (matches!(
                tag.to_ascii_lowercase().as_str(),
                "button" | "input" | "select" | "textarea"
            ) || tag.eq_ignore_ascii_case("a") && self.attribute(node, "href").is_some()
                || self.attribute(node, "tabindex").is_some())
        {
            out.push(node);
        }
        for child in self.doc.dom().dom_children(node) {
            self.collect_focusable(child, out);
        }
    }

    /// The immutable host-owned inputs used to construct this Livery session.
    /// Hosts can inspect this ledger for a product receipt without gaining a
    /// path to mutate resources or fetch behind the host boundary.
    pub fn resource_set(&self) -> &ResolvedDocumentResources {
        &self.resources
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Replace only the host-fetched image and font ledger. The retained
    /// stylesheet graph must be unchanged; a stylesheet change belongs to the
    /// CSSOM/live-style reconciliation path, not an asset update hidden under
    /// a static document session.
    pub fn replace_resource_bytes(
        &mut self,
        next: ResolvedDocumentResources,
    ) -> Result<ResourceDelta, String> {
        if next.stylesheets != self.resources.stylesheets {
            return Err("resource replacement cannot change the stylesheet graph".to_owned());
        }
        let delta = next.resource_delta_from(&self.resources);
        if delta.is_empty() {
            self.resources = next;
            return Ok(delta);
        }
        let images = next
            .resources
            .iter()
            .filter(|resource| resource.kind == ResourceKind::Image)
            .flat_map(|resource| {
                let mut keys = vec![(resource.authored_url.clone(), resource.bytes.clone())];
                if resource.resolved_url != resource.authored_url {
                    keys.push((resource.resolved_url.clone(), resource.bytes.clone()));
                }
                keys
            });
        let fonts = next
            .resources
            .iter()
            .filter(|resource| resource.kind == ResourceKind::Font)
            .map(|resource| (resource.resolved_url.clone(), resource.bytes.clone()));
        self.doc.replace_image_resources(images);
        self.doc.replace_font_resources(fonts);
        self.resources = next;
        Ok(delta)
    }

    /// Missing or deferred dependencies observed while assembling this
    /// document. This is the Livery ledger for the selected product route.
    pub fn resource_diagnostics(&self) -> &[genet_document_resources::ResourceDiagnostic] {
        &self.resources.diagnostics
    }

    fn find_occurrences(text: &str, query: &DocumentFindQuery) -> Vec<std::ops::Range<usize>> {
        if query.text.is_empty() {
            return Vec::new();
        }
        if query.match_case {
            return text
                .match_indices(&query.text)
                .map(|(start, matched)| start..start + matched.len())
                .collect();
        }
        if text.is_ascii() && query.text.is_ascii() {
            let haystack = text.to_ascii_lowercase();
            let needle = query.text.to_ascii_lowercase();
            return haystack
                .match_indices(&needle)
                .map(|(start, matched)| start..start + matched.len())
                .collect();
        }

        let needle = query.text.to_lowercase();
        let width = query.text.chars().count();
        let boundaries: Vec<_> = text
            .char_indices()
            .map(|(index, _)| index)
            .chain(std::iter::once(text.len()))
            .collect();
        (0..boundaries.len().saturating_sub(1))
            .filter_map(|start_index| {
                let end_index = start_index.checked_add(width)?;
                let end = *boundaries.get(end_index)?;
                let start = boundaries[start_index];
                (text[start..end].to_lowercase() == needle).then_some(start..end)
            })
            .collect()
    }

    fn find_structural_context(
        &self,
        source: genet_scripted_dom::NodeId,
        matched: &str,
    ) -> (Option<String>, String) {
        let mut node = self.doc.dom().parent(source);
        let mut fallback = None;
        while let Some(candidate) = node {
            if let Some(tag) = self.tag(candidate) {
                let role = match tag {
                    "a" => "link",
                    "button" => "button",
                    "input" | "textarea" => "textbox",
                    "p" => "paragraph",
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => "heading",
                    "li" => "listitem",
                    "img" => "image",
                    "label" => "label",
                    "nav" => "navigation",
                    "header" => "banner",
                    "footer" => "contentinfo",
                    "main" => "main",
                    "section" | "article" => "region",
                    _ => "group",
                };
                let label = self.text_content(candidate).trim().to_string();
                let context = (
                    Some(role.to_string()),
                    if label.is_empty() {
                        matched.to_string()
                    } else {
                        label
                    },
                );
                if role != "group" {
                    return context;
                }
                fallback.get_or_insert(context);
            }
            node = self.doc.dom().parent(candidate);
        }
        fallback.unwrap_or_else(|| (None, matched.to_string()))
    }

    fn find_matches(
        &self,
        query: &DocumentFindQuery,
    ) -> (
        Vec<DocumentFindMatch>,
        Vec<genet_livery::TextRange<genet_scripted_dom::NodeId>>,
    ) {
        fn collect_text_nodes(
            dom: &genet_scripted_dom::ScriptedDom,
            node: genet_scripted_dom::NodeId,
            out: &mut Vec<genet_scripted_dom::NodeId>,
        ) {
            if dom.kind(node) == NodeKind::Text {
                out.push(node);
            }
            for child in dom.dom_children(node) {
                collect_text_nodes(dom, child, out);
            }
        }

        let mut sources = Vec::new();
        collect_text_nodes(self.doc.dom(), self.doc.dom().document(), &mut sources);
        let mut matches = Vec::new();
        let mut ranges = Vec::new();
        for source in sources {
            let Some(text) = self.doc.dom().text(source) else {
                continue;
            };
            for range in Self::find_occurrences(text, query) {
                let Some(caret) = self.doc.caret_rect(source, range.start) else {
                    continue;
                };
                let matched = &text[range.clone()];
                let (role, label) = self.find_structural_context(source, matched);
                matches.push(DocumentFindMatch {
                    label,
                    role,
                    reveal: DocumentFindReveal::ScrollY((self.doc.scroll().1 + caret.y).max(0.0)),
                });
                ranges.push(genet_livery::TextRange {
                    anchor_node: source,
                    anchor_offset: range.start,
                    focus_node: source,
                    focus_offset: range.end,
                });
            }
        }
        (matches, ranges)
    }

    fn reveal_find_current(&mut self) {
        let Some(index) = self.find_state.current else {
            self.doc.select_text_range(None);
            return;
        };
        let Some(range) = self.find_ranges.get(index).copied() else {
            return;
        };
        if let Some(DocumentFindMatch {
            reveal: DocumentFindReveal::ScrollY(y),
            ..
        }) = self.find_state.matches.get(index)
        {
            self.doc.scroll_to((*y - 24.0).max(0.0));
        }
        self.doc.select_text_range(Some(range));
    }
}

#[cfg(feature = "livery")]
impl DocumentSession<Scene> for LiveryDocumentSession {
    fn document_capabilities(&self) -> DocumentCapabilities {
        DocumentCapabilities {
            find_in_page: DocumentCapabilityStatus::Supported,
            page_zoom: DocumentCapabilityStatus::unsupported(
                "Livery sessions do not expose page zoom",
            ),
            page_capture: DocumentCapabilityStatus::unsupported(
                "Livery sessions do not capture rendered pages",
            ),
            navigation: DocumentCapabilityStatus::Partial {
                detail: "the host owns document lineage, policy, and refetch".into(),
            },
        }
    }

    fn frame(&mut self, width: u32, height: u32) -> Scene {
        let mut list = match self.doc.frame(width, height) {
            Ok(list) => {
                self.last_error = None;
                list
            },
            Err(error) => {
                self.last_error = Some(error.to_string());
                return Scene::new(width, height);
            },
        };
        if let Some(navigation) = self.pending_fragment.take() {
            let text_activated = self
                .doc
                .activate_text_directives(&navigation.text_directives);
            if !text_activated && let Some(fragment) = navigation.element_fragment.as_deref() {
                self.doc.scroll_to_element_fragment(fragment);
            }
            // The activation only changes retained selection/scroll state. It
            // deliberately reuses this session's already-loaded source.
            list = match self.doc.frame(width, height) {
                Ok(list) => list,
                Err(error) => {
                    self.last_error = Some(error.to_string());
                    return Scene::new(width, height);
                },
            };
        }
        let mut scene = paint_list_render::translate_paint_list(&list);
        if let Some(selection) = self.doc.text_selection() {
            for rect in selection.rects {
                let x0 = rect.x.max(0.0);
                let y0 = rect.y.max(0.0);
                let x1 = (rect.x + rect.width).min(width as f32);
                let y1 = (rect.y + rect.height).min(height as f32);
                if x0 < x1 && y0 < y1 {
                    scene.push_rect(x0, y0, x1, y1, [0.18, 0.46, 0.95, 0.34]);
                }
            }
        }
        scene
    }

    fn scroll_by(&mut self, dx: f32, dy: f32) -> bool {
        self.doc.scroll_by(dx, dy)
    }

    fn scroll_at(&mut self, x: f32, y: f32, dx: f32, dy: f32) -> bool {
        self.doc.scroll_at(x, y, dx, dy)
    }

    fn scroll_for_key(&mut self, key: SessionScrollKey) -> bool {
        match key {
            SessionScrollKey::LineUp => self.doc.scroll_line(-1),
            SessionScrollKey::LineDown => self.doc.scroll_line(1),
            SessionScrollKey::PageUp => self.doc.scroll_page(-1),
            SessionScrollKey::PageDown => self.doc.scroll_page(1),
            SessionScrollKey::Home => {
                let before = self.doc.scroll();
                self.doc.scroll_to(0.0);
                before != self.doc.scroll()
            },
            SessionScrollKey::End => {
                let before = self.doc.scroll();
                self.doc.scroll_to(f32::MAX);
                before != self.doc.scroll()
            },
        }
    }

    fn scroll_to(&mut self, y: f32) {
        self.doc.scroll_to(y);
    }

    fn click_at(&mut self, x: f32, y: f32) -> SessionClick {
        let hit = self.doc.hit_test(x, y);
        let submit = hit.and_then(|node| self.submit_ancestor(node));
        self.activate_hit(x, y);
        match self.doc.click_at(x, y) {
            genet_livery::ClickOutcome::None => SessionClick::Miss,
            genet_livery::ClickOutcome::Focused | genet_livery::ClickOutcome::Scrolled => {
                submit.map_or(SessionClick::Handled, |submit| self.submit_click(submit))
            },
            genet_livery::ClickOutcome::Navigate(href) => SessionClick::Navigate(href),
        }
    }

    fn pointer_down(&mut self, x: f32, y: f32) -> SessionClick {
        self.pressed_submit = self
            .doc
            .hit_test(x, y)
            .and_then(|node| self.submit_ancestor(node));
        self.activate_hit(x, y);
        if self.doc.begin_text_selection(x, y) {
            SessionClick::Handled
        } else if self.editor.is_some() {
            let _ = self.doc.click_at(x, y);
            SessionClick::Handled
        } else if self.pressed_submit.is_some() || self.focused_node.is_some() {
            // Links and buttons activate on the matching release. Keeping the
            // press handled starts the host's logical pointer capture without
            // replacing the session before its release arrives.
            SessionClick::Handled
        } else {
            SessionClick::Miss
        }
    }

    fn pointer_move(&mut self, x: f32, y: f32) -> bool {
        self.doc.extend_text_selection(x, y)
    }

    fn pointer_up(&mut self, x: f32, y: f32) -> SessionClick {
        if self.doc.finish_text_selection(x, y) {
            self.pressed_submit = None;
            SessionClick::Handled
        } else {
            let released_submit = self
                .doc
                .hit_test(x, y)
                .and_then(|node| self.submit_ancestor(node));
            let pressed_submit = self.pressed_submit.take();
            if let Some(submit) = released_submit
                && Some(submit) == pressed_submit
            {
                let _ = self.doc.click_at(x, y);
                self.activate_hit(x, y);
                self.submit_click(submit)
            } else {
                self.click_at(x, y)
            }
        }
    }

    fn key_input(
        &mut self,
        key: SessionKey,
        state: SessionButtonState,
        modifiers: SessionModifiers,
        _repeat: bool,
    ) -> SessionEffect {
        if state == SessionButtonState::Released {
            return SessionEffect::Ignored;
        }
        match key {
            SessionKey::Character(text)
                if !modifiers.control && !modifiers.meta && !modifiers.alt =>
            {
                if self.insert_text(&text) {
                    SessionEffect::Handled
                } else {
                    SessionEffect::Ignored
                }
            },
            SessionKey::Space if self.insert_text(" ") => SessionEffect::Handled,
            SessionKey::Backspace if self.delete_backward() => SessionEffect::Handled,
            SessionKey::Delete if self.delete_forward() => SessionEffect::Handled,
            SessionKey::ArrowLeft if self.move_caret(-1) => SessionEffect::Handled,
            SessionKey::ArrowRight if self.move_caret(1) => SessionEffect::Handled,
            SessionKey::Home if let Some(editor) = self.editor.as_mut() => {
                editor.caret = 0;
                SessionEffect::Handled
            },
            SessionKey::End if let Some(editor) = self.editor.as_mut() => {
                editor.caret = editor.value.len();
                SessionEffect::Handled
            },
            SessionKey::Enter => {
                if self
                    .editor
                    .as_ref()
                    .is_some_and(|editor| editor.kind == EditableKind::Textarea)
                {
                    if self.insert_text("\n") {
                        SessionEffect::Handled
                    } else {
                        SessionEffect::Ignored
                    }
                } else if let Some(form) = self.active_form {
                    SessionEffect::Submit(self.submission_for_form(form, &self.address))
                } else {
                    SessionEffect::Ignored
                }
            },
            SessionKey::Tab => {
                let direction = if modifiers.shift {
                    SessionFocusDirection::Backward
                } else {
                    SessionFocusDirection::Forward
                };
                if self.focus_move(direction) {
                    SessionEffect::Handled
                } else {
                    SessionEffect::Ignored
                }
            },
            SessionKey::Escape => {
                if self.cancel_input() {
                    SessionEffect::Cancelled
                } else {
                    SessionEffect::Ignored
                }
            },
            _ => SessionEffect::Ignored,
        }
    }

    fn text_input(&mut self, text: &str) -> bool {
        self.insert_text(text)
    }

    fn ime_input(&mut self, ime: SessionIme) -> bool {
        match ime {
            SessionIme::Enabled => self.editor.is_some(),
            SessionIme::Preedit { text, .. } => {
                let Some(editor) = self.editor.as_mut() else {
                    return false;
                };
                editor.composition = Some(text);
                true
            },
            SessionIme::Commit(text) => self.insert_text(&text),
            SessionIme::Disabled => {
                let Some(editor) = self.editor.as_mut() else {
                    return false;
                };
                editor.composition = None;
                true
            },
        }
    }

    fn focus_input(&mut self, focused: bool) {
        if !focused {
            self.focused_node = None;
            self.editor = None;
            self.active_form = None;
        }
    }

    fn focus_move(&mut self, direction: SessionFocusDirection) -> bool {
        let mut focusable = Vec::new();
        self.collect_focusable(self.doc.dom().document(), &mut focusable);
        if focusable.is_empty() {
            return false;
        }
        let current = self
            .focused_node
            .and_then(|focused| focusable.iter().position(|node| *node == focused));
        let next = match direction {
            SessionFocusDirection::Forward => {
                current.map_or(0, |index| (index + 1) % focusable.len())
            },
            SessionFocusDirection::Backward => current.map_or(focusable.len() - 1, |index| {
                index.checked_sub(1).unwrap_or(focusable.len() - 1)
            }),
        };
        let node = focusable[next];
        let Some([x, y, width, height]) = self.doc.fragment_rect(node) else {
            return false;
        };
        let _ = self.doc.click_at(x + width * 0.5, y + height * 0.5);
        if let Some(kind) = self.editable_kind(node) {
            self.activate_editable(node, kind);
        } else {
            self.focused_node = Some(node);
            self.editor = None;
            self.active_form = self.form_ancestor(node);
        }
        true
    }

    fn cancel_input(&mut self) -> bool {
        let Some(editor) = self.editor.as_mut() else {
            return false;
        };
        editor.composition.take().is_some()
    }

    fn editable_focus(&self) -> bool {
        self.editor.is_some()
    }

    fn cursor_at(&self, x: f32, y: f32) -> SessionCursor {
        let Some(hit) = self.doc.hit_test(x, y) else {
            return SessionCursor::Default;
        };
        if self.editable_ancestor(hit).is_some() {
            SessionCursor::Text
        } else if self.submit_ancestor(hit).is_some()
            || self
                .ancestor_matching(hit, |node, tag| {
                    tag.eq_ignore_ascii_case("a") && self.attribute(node, "href").is_some()
                })
                .is_some()
        {
            SessionCursor::Pointer
        } else {
            SessionCursor::Default
        }
    }

    fn form_submission(&mut self, action: &str) -> SessionFormSubmission {
        self.active_form.map_or_else(
            || SessionFormSubmission {
                action: action.to_owned(),
                ..Default::default()
            },
            |form| self.submission_for_form(form, action),
        )
    }

    fn text_target(&self, text: &str) -> Option<SessionTextTarget> {
        let (anchor, focus) = self.doc.text_target(text)?;
        Some(SessionTextTarget { anchor, focus })
    }

    fn document_find(
        &mut self,
        query: &DocumentFindQuery,
    ) -> Result<DocumentFindState, SessionError> {
        if query.text.is_empty() {
            self.clear_document_find()?;
            self.find_state.query = query.clone();
            return Ok(self.find_state.clone());
        }
        let (matches, ranges) = self.find_matches(query);
        self.find_ranges = ranges;
        self.find_state = DocumentFindState {
            query: query.clone(),
            current: (!matches.is_empty()).then_some(0),
            count: matches.len(),
            matches,
            complete: true,
        };
        self.reveal_find_current();
        Ok(self.find_state.clone())
    }

    fn document_find_step(
        &mut self,
        direction: DocumentFindDirection,
    ) -> Result<DocumentFindState, SessionError> {
        let count = self.find_state.count;
        if count == 0 {
            return Ok(self.find_state.clone());
        }
        let current = self.find_state.current.unwrap_or(0);
        self.find_state.current = Some(match direction {
            DocumentFindDirection::Previous => (current + count - 1) % count,
            DocumentFindDirection::Next => (current + 1) % count,
        });
        self.reveal_find_current();
        Ok(self.find_state.clone())
    }

    fn clear_document_find(&mut self) -> Result<(), SessionError> {
        self.find_ranges.clear();
        self.find_state = DocumentFindState::empty(DocumentFindQuery::default());
        self.doc.select_text_range(None);
        Ok(())
    }

    fn links(&self) -> Vec<SessionLink> {
        self.doc
            .links()
            .into_iter()
            .map(|link| SessionLink {
                url: link.url,
                rect: link.rect,
            })
            .collect()
    }

    fn content_height(&mut self, _width: u32, height: u32) -> u32 {
        self.doc.content_height(height)
    }

    fn pump(&mut self, now_ms: f64) {
        self.doc.pump(now_ms);
    }

    fn settled(&mut self) -> bool {
        self.doc.settled()
    }

    /// The structural report through the trait, off the Livery document's own
    /// DOM — the same read the static lane serves, so a viewer-pinned livery
    /// session inspects (and a11y-projects) instead of answering "none for
    /// this lane".
    fn inspect(&self) -> Option<inker::ContentReport> {
        Some(content_report(self.doc.dom()))
    }

    fn clip(&self) -> Option<DocumentClip> {
        let selection = self.doc.text_selection();
        let mut clip = match selection {
            Some(selection) => {
                let links = self.doc.links_for_selection(&selection);
                semantic_clip_from_selection_with_links(
                    &self.address,
                    self.doc.dom(),
                    ClipSelection {
                        range: ClipRange {
                            anchor_node: selection.range.anchor_node,
                            anchor_offset: selection.range.anchor_offset,
                            focus_node: selection.range.focus_node,
                            focus_offset: selection.range.focus_offset,
                        },
                        text: selection.text,
                    },
                    links,
                )
            },
            None => semantic_clip_from_dom(&self.address, self.doc.dom()),
        }?;
        clip.artifacts.push(DocumentClipArtifact {
            role: DocumentClipArtifactRole::SourceResponse,
            media_type: self
                .source_response
                .content_type
                .as_deref()
                .unwrap_or("text/html")
                .to_string(),
            canonical_uri: self.source_response.final_url.clone(),
            bytes: self.source_response.bytes.clone(),
        });
        Some(clip)
    }

    fn as_any_ref(&self) -> &dyn Any {
        self
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

#[derive(Clone, Copy)]
struct ClipRange<Id> {
    anchor_node: Id,
    anchor_offset: usize,
    focus_node: Id,
    focus_offset: usize,
}

struct ClipSelection<Id> {
    range: ClipRange<Id>,
    text: String,
}

fn content_report<D: LayoutDom>(dom: &D) -> inker::ContentReport {
    fn direct_text<D: LayoutDom>(dom: &D, node: D::NodeId) -> String {
        let mut name = String::new();
        for child in dom.dom_children(node) {
            if dom.kind(child) == NodeKind::Text
                && let Some(text) = dom.text(child)
            {
                name.push_str(text);
            }
        }
        name.trim().to_string()
    }

    fn role_of(tag: &str) -> &'static str {
        match tag {
            "a" => "link",
            "button" => "button",
            "input" | "textarea" => "textbox",
            "p" => "paragraph",
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => "heading",
            "ul" | "ol" => "list",
            "li" => "listitem",
            "img" => "image",
            "label" => "label",
            "nav" => "navigation",
            "header" => "banner",
            "footer" => "contentinfo",
            "main" => "main",
            "section" | "article" => "region",
            _ => "group",
        }
    }

    fn walk<D: LayoutDom>(
        dom: &D,
        node: D::NodeId,
        depth: usize,
        report: &mut inker::ContentReport,
    ) {
        let mut child_depth = depth;
        if let Some(tag) = dom.element_name(node).map(|name| name.local.to_string()) {
            if !matches!(
                tag.as_str(),
                "head" | "style" | "script" | "title" | "meta" | "link" | "base" | "html"
            ) {
                report.outline.push(inker::OutlineEntry {
                    depth,
                    role: role_of(&tag),
                    name: direct_text(dom, node),
                });
                child_depth = depth + 1;
            }
            match tag.as_str() {
                "title" => {
                    let text = direct_text(dom, node);
                    if !text.is_empty() {
                        report.title = Some(text);
                    }
                },
                "a" => {
                    if let Some(href) =
                        dom.attribute(node, &Namespace::default(), &LocalName::from("href"))
                    {
                        report.links.push(href.to_string());
                    }
                },
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    let text = direct_text(dom, node);
                    if !text.is_empty() {
                        report.headings.push(text);
                    }
                },
                _ => {},
            }
        }
        for child in dom.dom_children(node) {
            walk(dom, child, child_depth, report);
        }
    }

    let mut report = inker::ContentReport::default();
    walk(dom, dom.document(), 0, &mut report);
    report
}

fn semantic_clip_from_dom<D: LayoutDom>(address: &str, dom: &D) -> Option<DocumentClip> {
    let report = content_report(dom);
    let text = fleece::extract_main_text(dom).unwrap_or_else(|| report.headings.join("\n"));
    let text = text.trim().to_string();
    (!text.is_empty()).then(|| DocumentClip {
        source_url: address.to_string(),
        title: report.title,
        text,
        selector: None,
        links: report.links,
        artifacts: Vec::new(),
    })
}

fn semantic_clip_from_selection_with_links<D>(
    address: &str,
    dom: &D,
    selection: ClipSelection<D::NodeId>,
    links: Vec<String>,
) -> Option<DocumentClip>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq,
{
    if selection.text.is_empty() {
        return None;
    }
    let anchor_path = dom_path(dom, selection.range.anchor_node)?;
    let focus_path = dom_path(dom, selection.range.focus_node)?;
    let selector = serde_json::json!({
        "type": "dom-range",
        "version": 1,
        "anchor": {
            "path": anchor_path,
            "offset": selection.range.anchor_offset,
        },
        "focus": {
            "path": focus_path,
            "offset": selection.range.focus_offset,
        },
        "quote": selection.text,
    })
    .to_string();
    let report = content_report(dom);
    Some(DocumentClip {
        source_url: address.to_string(),
        title: report.title,
        text: selection.text,
        selector: Some(selector),
        links,
        artifacts: Vec::new(),
    })
}

fn dom_path<D>(dom: &D, mut node: D::NodeId) -> Option<Vec<usize>>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq,
{
    let mut path = Vec::new();
    while node != dom.document() {
        let parent = dom.parent(node)?;
        let index = dom.dom_children(parent).position(|child| child == node)?;
        path.push(index);
        node = parent;
    }
    path.reverse();
    Some(path)
}

#[cfg(feature = "scripted")]
fn links_for_source_nodes<D>(dom: &D, sources: &[D::NodeId]) -> Vec<String>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq,
{
    let mut links = Vec::new();
    for source in sources {
        let mut node = Some(*source);
        while let Some(current) = node {
            if dom.kind(current) == NodeKind::Element
                && dom
                    .element_name(current)
                    .is_some_and(|name| name.local.as_ref().eq_ignore_ascii_case("a"))
                && let Some(href) =
                    dom.attribute(current, &Namespace::default(), &LocalName::from("href"))
            {
                if !links.iter().any(|seen| seen == href) {
                    links.push(href.to_owned());
                }
                break;
            }
            node = dom.parent(current);
        }
    }
    links
}

// ── Scripted lane (genet.scripted / genet.scripted.nova) ────────────────

/// Session engine for the scripted lane, generic over the JS engine `E` (the
/// per-engine monomorphization genet-scripted already uses: the host
/// registers `ScriptedSessionEngine::<BoaEngine, _>` under `genet.scripted`
/// and, on 64-bit targets with the `scripted-nova` feature,
/// `ScriptedSessionEngine::<NovaEngine, _>` under `genet.scripted.nova`).
/// Holds the shell's fetcher for external `<script src>` resolution.
#[cfg(feature = "scripted")]
pub struct ScriptedSessionEngine<E, Fetch> {
    engine_id: String,
    fetcher: Fetch,
    _engine: std::marker::PhantomData<fn() -> E>,
}

#[cfg(feature = "scripted")]
impl<E, Fetch> ScriptedSessionEngine<E, Fetch> {
    pub fn new(engine_id: impl Into<String>, fetcher: Fetch) -> Self {
        Self {
            engine_id: engine_id.into(),
            fetcher,
            _engine: std::marker::PhantomData,
        }
    }
}

#[cfg(feature = "scripted")]
impl<E, Fetch> SessionEngine<Scene> for ScriptedSessionEngine<E, Fetch>
where
    E: script_engine_api::ScriptEngine + 'static,
    Fetch: genet_scripted::ResourceFetcher + Clone + Send + Sync + 'static,
{
    fn engine_id(&self) -> &str {
        &self.engine_id
    }

    fn spawn(
        &self,
        request: &SessionSpawnRequest,
    ) -> Result<Box<dyn DocumentSession<Scene>>, SessionError> {
        let navigation = genet_livery::NavigationFragment::parse(&request.address);
        let doc = match &request.body {
            Some(body) => genet_scripted::LiveryScriptedDocument::<E>::from_body(
                body,
                self.fetcher.clone(),
                &request.address,
            ),
            None => genet_scripted::LiveryScriptedDocument::<E>::load(
                self.fetcher.clone(),
                &request.address,
            ),
        }
        .map_err(SessionError::SpawnFailed)?;
        let mut session = ScriptedDocumentSession {
            doc,
            address: navigation.script_visible_url,
            pressed_target: None,
            pointer_active: false,
        };
        if request.hidden {
            session.doc.set_hidden(true);
        }
        Ok(Box::new(session))
    }
}

/// The scripted document as a session. Public so a host with richer
/// construction seams (per-spawn fetchers, cookie jars) builds the document
/// itself and wraps it; the engine above is the simple-seam path.
#[cfg(feature = "scripted")]
pub struct ScriptedDocumentSession<E: script_engine_api::ScriptEngine> {
    doc: genet_scripted::LiveryScriptedDocument<E>,
    address: String,
    pressed_target: Option<genet_scripted_dom::NodeId>,
    pointer_active: bool,
}

#[cfg(feature = "scripted")]
impl<E: script_engine_api::ScriptEngine + 'static> ScriptedDocumentSession<E> {
    pub fn new(doc: genet_scripted::LiveryScriptedDocument<E>) -> Self {
        Self::new_at(doc, "about:blank")
    }

    pub fn new_at(
        doc: genet_scripted::LiveryScriptedDocument<E>,
        address: impl Into<String>,
    ) -> Self {
        Self {
            doc,
            address: address.into(),
            pressed_target: None,
            pointer_active: false,
        }
    }
}

#[cfg(feature = "scripted")]
impl<E: script_engine_api::ScriptEngine + 'static> DocumentSession<Scene>
    for ScriptedDocumentSession<E>
{
    fn document_capabilities(&self) -> DocumentCapabilities {
        retained_document_capabilities("scripted sessions do not expose document find")
    }

    fn frame(&mut self, width: u32, height: u32) -> Scene {
        self.doc.frame(width, height)
    }
    fn scroll_by(&mut self, dx: f32, dy: f32) -> bool {
        self.doc.scroll_by(dx, dy)
    }
    fn scroll_for_key(&mut self, key: SessionScrollKey) -> bool {
        self.doc.scroll_for_key(scripted_scroll_key(key))
    }
    fn click_at(&mut self, x: f32, y: f32) -> SessionClick {
        match self.doc.click_at_result(x, y) {
            genet_scripted::ScriptedClick::Miss => SessionClick::Miss,
            genet_scripted::ScriptedClick::Handled => SessionClick::Handled,
            genet_scripted::ScriptedClick::Navigate(target) => SessionClick::Navigate(target),
        }
    }
    fn pointer_down(&mut self, x: f32, y: f32) -> SessionClick {
        let pressed_target = self.doc.click_target_at(x, y);
        self.pressed_target = pressed_target;
        self.pointer_active = self.doc.begin_text_selection(x, y) || pressed_target.is_some();
        if self.pointer_active {
            SessionClick::Handled
        } else {
            SessionClick::Miss
        }
    }
    fn pointer_move(&mut self, x: f32, y: f32) -> bool {
        self.doc.extend_text_selection(x, y)
    }
    fn pointer_up(&mut self, x: f32, y: f32) -> SessionClick {
        if !std::mem::replace(&mut self.pointer_active, false) {
            self.pressed_target = None;
            return SessionClick::Miss;
        }
        let pressed_target = self.pressed_target.take();
        if self.doc.finish_text_selection(x, y) {
            SessionClick::Handled
        } else if pressed_target.is_some() && self.doc.click_target_at(x, y) == pressed_target {
            self.click_at(x, y)
        } else {
            SessionClick::Miss
        }
    }
    fn focus_input(&mut self, focused: bool) {
        if !focused && self.pointer_active {
            self.pointer_active = false;
            self.pressed_target = None;
            let _ = self.doc.cancel_text_selection();
        }
    }
    fn cancel_input(&mut self) -> bool {
        let had_pointer = std::mem::replace(&mut self.pointer_active, false)
            || self.pressed_target.take().is_some();
        self.pressed_target = None;
        self.doc.cancel_text_selection() || had_pointer
    }
    fn text_target(&self, text: &str) -> Option<SessionTextTarget> {
        let (anchor, focus) = self.doc.text_target(text)?;
        Some(SessionTextTarget { anchor, focus })
    }
    fn links(&self) -> Vec<SessionLink> {
        self.doc
            .links()
            .into_iter()
            .map(|(url, rect)| SessionLink { url, rect })
            .collect()
    }
    fn pump(&mut self, now_ms: f64) {
        let _ = self.doc.pump(now_ms);
    }
    fn settled(&mut self) -> bool {
        !self.doc.has_pending_work()
    }
    fn set_hidden(&mut self, hidden: bool) {
        self.doc.set_hidden(hidden);
    }
    fn inspect(&self) -> Option<inker::ContentReport> {
        Some(self.doc.with_dom(content_report))
    }
    fn clip(&self) -> Option<DocumentClip> {
        let selection = self.doc.text_selection();
        self.doc.with_dom(|dom| match selection {
            Some(selection) => {
                let selected_links = links_for_source_nodes(dom, &selection.source_nodes);
                semantic_clip_from_selection_with_links(
                    &self.address,
                    dom,
                    ClipSelection {
                        range: ClipRange {
                            anchor_node: selection.range.anchor_node,
                            anchor_offset: selection.range.anchor_offset,
                            focus_node: selection.range.focus_node,
                            focus_offset: selection.range.focus_offset,
                        },
                        text: selection.text,
                    },
                    selected_links,
                )
            },
            None => semantic_clip_from_dom(&self.address, dom),
        })
    }
    /// Observation extras (extract, dom_snapshot, dispatch_event, dom stats)
    /// stay on the concrete type until the observation contract lands
    /// (session-engines plan phase 3 rescope); hosts reach them here.
    fn as_any_ref(&self) -> &dyn Any {
        self
    }
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(feature = "scripted")]
impl<E: script_engine_api::ScriptEngine> ScriptedDocumentSession<E> {
    /// The concrete document, for observation downcasts (phase 3 rescope:
    /// extract / dom_snapshot / dispatch_event stay concrete until the
    /// observation contract lands).
    pub fn document_mut(&mut self) -> &mut genet_scripted::LiveryScriptedDocument<E> {
        &mut self.doc
    }
}

// ── Smolweb engine-native document lane (per-format ids) ──────────────────

/// Session engine for the smolweb native lane. One instance per format id
/// (`nematic.gemtext` / `nematic.gopher` / `nematic.feed` today) so routing
/// decisions map directly; the same ids keep their block engines for cards —
/// the kind index reports both and the host picks by surface context.
#[cfg(feature = "smolweb")]
pub struct SmolwebSessionEngine<Fetch> {
    engine_id: String,
    fetcher: Fetch,
    theme: crate::SmolwebTheme,
    inline_media: crate::SmolwebInlineMediaPolicy,
}

#[cfg(feature = "smolweb")]
impl<Fetch> SmolwebSessionEngine<Fetch> {
    pub fn new(engine_id: impl Into<String>, fetcher: Fetch, theme: crate::SmolwebTheme) -> Self {
        Self {
            engine_id: engine_id.into(),
            fetcher,
            theme,
            inline_media: crate::SmolwebInlineMediaPolicy::default(),
        }
    }

    /// Apply a host-owned inline-media policy to documents this engine spawns.
    pub fn with_inline_media(mut self, policy: crate::SmolwebInlineMediaPolicy) -> Self {
        self.inline_media = policy;
        self
    }
}

#[cfg(feature = "smolweb")]
impl<Fetch: ResourceFetcher + Send + Sync> SessionEngine<Scene> for SmolwebSessionEngine<Fetch> {
    fn engine_id(&self) -> &str {
        &self.engine_id
    }

    fn spawn(
        &self,
        request: &SessionSpawnRequest,
    ) -> Result<Box<dyn DocumentSession<Scene>>, SessionError> {
        let doc = match &request.body {
            Some(body) => crate::SmolwebDocument::parse_with_inline_media(
                &request.address,
                body,
                self.theme.clone(),
                self.inline_media,
            ),
            None => crate::SmolwebDocument::load_with_inline_media(
                &self.fetcher,
                &request.address,
                self.theme.clone(),
                self.inline_media,
            )
            .map_err(SessionError::SpawnFailed)?,
        };
        Ok(Box::new(SmolwebDocumentSession {
            doc,
            viewport: request.viewport,
        }))
    }
}

/// The smolweb document as a session. Public so a host that themes per
/// content (meerkat's palette-derived themes) parses the document itself and
/// wraps it; the engine above is the fixed-theme path.
#[cfg(feature = "smolweb")]
pub struct SmolwebDocumentSession {
    doc: crate::SmolwebDocument,
    /// Last framed size: the lane's click/content-height APIs take the
    /// viewport, which the trait carries implicitly through `frame`.
    viewport: (u32, u32),
}

#[cfg(feature = "smolweb")]
impl SmolwebDocumentSession {
    pub fn new(doc: crate::SmolwebDocument, viewport: (u32, u32)) -> Self {
        Self { doc, viewport }
    }

    /// The concrete document, for observation downcasts and host-side
    /// banding/link-table inspection.
    pub fn document_mut(&mut self) -> &mut crate::SmolwebDocument {
        &mut self.doc
    }

    /// Replace an incrementally received body while retaining this session's
    /// viewport and host-owned presentation policy.
    pub fn replace_body(&mut self, url: &str, body: &str) {
        self.doc.replace_body(url, body);
    }
}

#[cfg(feature = "smolweb")]
impl DocumentSession<Scene> for SmolwebDocumentSession {
    fn document_capabilities(&self) -> DocumentCapabilities {
        retained_document_capabilities("Smolweb sessions do not expose document find")
    }

    fn frame(&mut self, width: u32, height: u32) -> Scene {
        self.viewport = (width, height);
        self.doc.frame(width, height)
    }
    fn scroll_by(&mut self, dx: f32, dy: f32) -> bool {
        self.doc.scroll_by(dx, dy)
    }
    fn scroll_at(&mut self, x: f32, y: f32, dx: f32, dy: f32) -> bool {
        self.doc.scroll_at(x, y, dx, dy)
    }
    fn scroll_for_key(&mut self, key: SessionScrollKey) -> bool {
        self.doc.scroll_for_key(key)
    }
    fn scroll_to(&mut self, y: f32) {
        self.doc.scroll_to(y);
    }
    fn click_at(&mut self, x: f32, y: f32) -> SessionClick {
        let (w, h) = self.viewport;
        match self.doc.click_at(x, y, w, h) {
            Some(document_canvas::InteractionKind::Link { url }) => SessionClick::Navigate(url),
            Some(document_canvas::InteractionKind::Submit { target }) => {
                SessionClick::Submit(target)
            },
            None => SessionClick::Miss,
        }
    }
    fn links(&self) -> Vec<SessionLink> {
        self.doc
            .links()
            .into_iter()
            .map(|(url, rect)| SessionLink { url, rect })
            .collect()
    }
    fn content_height(&mut self, width: u32, height: u32) -> u32 {
        self.doc.content_height(width, height)
    }
    fn subresources(&self) -> Vec<String> {
        self.doc.subresources()
    }
    fn provide_subresource(&mut self, url: &str, bytes: &[u8]) -> bool {
        self.doc.provide_subresource(url, bytes)
    }
    fn inspect(&self) -> Option<inker::ContentReport> {
        Some(inker::ContentReport {
            title: self.doc.document().title.clone(),
            links: self
                .doc
                .document()
                .outgoing_links()
                .into_iter()
                .map(str::to_string)
                .collect(),
            ..Default::default()
        })
    }
    fn as_any_ref(&self) -> &dyn Any {
        self
    }
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inker::session_engine::SessionRegistry;
    #[cfg(feature = "livery")]
    use std::sync::{Arc, Mutex};

    /// Byte source for spawn-with-body tests; never fetches.
    #[derive(Clone)]
    struct NoFetch;
    impl ResourceFetcher for NoFetch {
        fn fetch(&self, _url: &str) -> Option<Vec<u8>> {
            None
        }
    }
    #[cfg(feature = "livery")]
    struct ImageFetch {
        bytes: Vec<u8>,
        requests: Arc<Mutex<Vec<String>>>,
    }

    #[cfg(feature = "livery")]
    impl ResourceFetcher for ImageFetch {
        fn fetch(&self, url: &str) -> Option<Vec<u8>> {
            self.requests.lock().unwrap().push(url.to_owned());
            Some(self.bytes.clone())
        }
    }

    #[cfg(feature = "smolweb")]
    #[test]
    fn smolweb_session_body_route_requests_and_accepts_inline_images() {
        let image = image::RgbaImage::from_pixel(2, 1, image::Rgba([0, 128, 255, 255]));
        let mut image_bytes = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut image_bytes),
                image::ImageFormat::Png,
            )
            .expect("encode PNG fixture");
        let engine = SmolwebSessionEngine::new(
            inker::routing::ENGINE_NEMATIC_GEMTEXT,
            NoFetch,
            crate::SmolwebTheme::Plain,
        )
        .with_inline_media(crate::SmolwebInlineMediaPolicy::images());
        let request = SessionSpawnRequest::new("gemini://x.test/docs/index.gmi")
            .with_body("=> picture.png Picture\n")
            .with_viewport(320, 240);
        let mut session = engine.spawn(&request).expect("smolweb session spawns");

        assert_eq!(session.subresources(), ["gemini://x.test/docs/picture.png"]);
        assert!(session.provide_subresource("gemini://x.test/docs/picture.png", &image_bytes));
        let scene = session.frame(320, 240);
        assert!(
            scene
                .ops
                .iter()
                .any(|operation| matches!(operation, netrender::SceneOp::Image(_)))
        );
        assert!(session.subresources().is_empty());
        assert_eq!(session.links()[0].url, "gemini://x.test/docs/picture.png");
    }

    #[cfg(feature = "livery")]
    struct LinkedResourceFetch {
        image: Vec<u8>,
        font: Vec<u8>,
        requests: Arc<Mutex<Vec<String>>>,
    }

    #[cfg(feature = "livery")]
    impl ResourceFetcher for LinkedResourceFetch {
        fn fetch(&self, url: &str) -> Option<Vec<u8>> {
            self.requests.lock().unwrap().push(url.to_owned());
            match url {
                "https://example.test/docs/styles/site.css" => Some(
                    br#".card { display: block; width: 80px; height: 40px; background-image: url(images/hero.png); }
@font-face { font-family: linked; src: url(../fonts/text.woff2); }"#
                        .to_vec(),
                ),
                "https://example.test/docs/styles/images/hero.png" => Some(self.image.clone()),
                "https://example.test/docs/fonts/text.woff2" => Some(self.font.clone()),
                _ => None,
            }
        }
    }

    #[cfg(feature = "livery")]
    struct ImportedSheetFetch;

    #[cfg(feature = "livery")]
    impl ResourceFetcher for ImportedSheetFetch {
        fn fetch(&self, _url: &str) -> Option<Vec<u8>> {
            None
        }

        fn fetch_response(&self, url: &str) -> Option<genet_host_api::ResourceResponse> {
            match url {
                "https://example.test/docs/styles/root.css" => Some(
                    genet_host_api::ResourceResponse::new(
                        "https://cdn.example.test/styles/root.css",
                        br#"@import "palette.css"; .card { color: rgb(255, 0, 0); }"#.to_vec(),
                    )
                    .with_content_type("text/css"),
                ),
                "https://cdn.example.test/styles/palette.css" => Some(
                    genet_host_api::ResourceResponse::new(
                        url,
                        br#".card { color: rgb(0, 0, 255); }"#.to_vec(),
                    )
                    .with_content_type("text/css; charset=utf-8"),
                ),
                _ => None,
            }
        }
    }

    #[cfg(feature = "livery")]
    struct RedirectedDocumentFetch;

    #[cfg(feature = "livery")]
    impl ResourceFetcher for RedirectedDocumentFetch {
        fn fetch(&self, _url: &str) -> Option<Vec<u8>> {
            None
        }

        fn fetch_response(&self, url: &str) -> Option<genet_host_api::ResourceResponse> {
            match url {
                "https://example.test/start" => Some(
                    genet_host_api::ResourceResponse::new(
                        "https://cdn.example.test/final/index.html",
                        br#"<link rel="stylesheet" href="site.css"><main class="card"><h1>final base</h1></main>"#
                            .to_vec(),
                    )
                    .with_content_type("text/html"),
                ),
                "https://cdn.example.test/final/site.css" => Some(
                    genet_host_api::ResourceResponse::new(
                        url,
                        br#".card { color: rgb(0, 128, 0); }"#.to_vec(),
                    )
                    .with_content_type("text/css"),
                ),
                _ => None,
            }
        }
    }

    #[cfg(feature = "scripted")]
    #[test]
    fn scripted_session_selects_and_clips_the_live_dom() {
        let engine = ScriptedSessionEngine::<script_engine_boa::BoaEngine, _>::new(
            "genet.scripted",
            NoFetch,
        );
        let request = SessionSpawnRequest::new("https://example.test/report")
            .with_body(
                "<html><head><title>Live Page</title></head><body style=\"margin:0\">\
                 <p style=\"margin:0\">before <a id=\"choice\" href=\"/chosen\"></a> and \
                 <a href=\"/also\">second link</a> after <a href=\"/outside\">outside</a></p>\
                 <script>document.getElementById('choice').appendChild(\
                 document.createTextNode('selected link'));</script>\
                 </body></html>",
            )
            .with_viewport(640, 200);
        let mut session = engine.spawn(&request).expect("scripted lane spawns");
        let unselected = session.frame(640, 200);
        let unselected_rects = unselected
            .ops
            .iter()
            .filter(|op| matches!(op, netrender::SceneOp::Rect(_)))
            .count();
        let report = session.inspect().expect("live DOM is inspectable");
        assert_eq!(report.title.as_deref(), Some("Live Page"));

        let target = session
            .text_target("selected link")
            .expect("post-script first link resolves to pointer endpoints");
        let second = session
            .text_target("second link")
            .expect("second link resolves to pointer endpoints");
        assert_eq!(
            session.pointer_down(target.anchor[0], target.anchor[1]),
            SessionClick::Handled
        );
        assert!(
            session.pointer_move(second.focus[0], second.focus[1]),
            "the live range extends through ordinary pointer input"
        );
        assert_eq!(
            session.pointer_up(second.focus[0], second.focus[1]),
            SessionClick::Handled
        );
        let selected = session.frame(640, 200);
        let selected_rects = selected
            .ops
            .iter()
            .filter(|op| matches!(op, netrender::SceneOp::Rect(_)))
            .count();
        assert!(
            selected_rects > unselected_rects,
            "the retained live range paints selection geometry"
        );

        let clip = session.clip().expect("live selection supplies a clip");
        assert_eq!(clip.source_url, "https://example.test/report");
        assert_eq!(clip.text, "selected link and second link");
        assert_eq!(clip.links, vec!["/chosen", "/also"]);
        let selector: serde_json::Value =
            serde_json::from_str(clip.selector.as_deref().expect("range selector"))
                .expect("selector is typed JSON");
        assert_eq!(selector["type"], "dom-range");
        assert_eq!(selector["version"], 1);
        assert_eq!(selector["quote"], "selected link and second link");
        assert!(selector["anchor"]["path"].is_array());
        assert!(selector["focus"]["path"].is_array());
    }

    #[cfg(feature = "scripted")]
    #[test]
    fn scripted_session_returns_only_uncancelled_external_navigation() {
        let engine = ScriptedSessionEngine::<script_engine_boa::BoaEngine, _>::new(
            "genet.scripted",
            NoFetch,
        );
        let request = SessionSpawnRequest::new("https://example.test/start")
            .with_body(
                r#"<body style="margin:0">
                    <style>a { display:block; width:180px; padding:20px; }</style>
                    <p>Selection start</p>
                    <a href="next.html"><span>Open next</span></a>
                    <a id="blocked" href="blocked.html">Stay here</a>
                    <a id="changed" href="old.html">Change target</a>
                    <script>
                      document.addEventListener('click', function (event) {
                        if (event.target.id === 'blocked') event.preventDefault();
                        if (event.target.id === 'changed') {
                          event.target.setAttribute('href', 'changed.html');
                        }
                      });
                    </script>
                </body>"#,
            )
            .with_viewport(640, 200);
        let mut session = engine.spawn(&request).expect("scripted lane spawns");
        let _scene = session.frame(640, 200);
        session.pump(1.0);

        let right_padding = |target: SessionTextTarget| {
            (
                target.focus[0] + 8.0,
                (target.anchor[1] + target.focus[1]) * 0.5,
            )
        };
        let next_text = session
            .text_target("Open next")
            .expect("next link geometry");
        let next_padding = right_padding(next_text);
        assert_eq!(
            session.pointer_down(next_padding.0, next_padding.1),
            SessionClick::Handled,
            "an anchor-padding press starts capture without navigating"
        );
        assert_eq!(
            session.pointer_up(
                next_text.focus[0] - 0.5,
                (next_text.anchor[1] + next_text.focus[1]) * 0.5,
            ),
            SessionClick::Navigate("next.html".to_owned()),
            "release on the same anchor's inline child keeps one activation target"
        );

        assert_eq!(
            session.pointer_down(next_padding.0, next_padding.1),
            SessionClick::Handled
        );
        assert_eq!(
            session.pointer_up(500.0, 190.0),
            SessionClick::Handled,
            "release outside becomes a selection instead of navigation"
        );
        assert_eq!(
            session.pointer_up(next_padding.0, next_padding.1),
            SessionClick::Miss,
            "a mismatched release clears the retained press"
        );

        let blocked = right_padding(
            session
                .text_target("Stay here")
                .expect("cancelled link geometry"),
        );
        assert_eq!(
            session.pointer_down(blocked.0, blocked.1),
            SessionClick::Handled
        );
        assert_eq!(
            session.pointer_up(blocked.0, blocked.1),
            SessionClick::Handled,
            "preventDefault cancels navigation through the ordinary release path"
        );

        let changed = right_padding(
            session
                .text_target("Change target")
                .expect("mutating link geometry"),
        );
        assert_eq!(
            session.pointer_down(changed.0, changed.1),
            SessionClick::Handled
        );
        assert_eq!(
            session.pointer_up(changed.0, changed.1),
            SessionClick::Navigate("changed.html".to_owned()),
            "the default action reads href after uncancelled listeners run"
        );

        assert_eq!(
            session.pointer_down(blocked.0, blocked.1),
            SessionClick::Handled
        );
        let cancelled = session.input(inker::SessionInput::Cancel);
        assert_eq!(cancelled.effect, SessionEffect::Cancelled);
        assert_eq!(
            session.pointer_up(blocked.0, blocked.1),
            SessionClick::Miss,
            "cancelled capture cannot activate on a later release"
        );

        assert_eq!(
            session.pointer_down(changed.0, changed.1),
            SessionClick::Handled
        );
        let blurred = session.input(inker::SessionInput::Focus(false));
        assert_eq!(blurred.effect, SessionEffect::Handled);
        assert_eq!(
            session.pointer_up(changed.0, changed.1),
            SessionClick::Miss,
            "focus loss clears a captured press before its later release"
        );

        let drag_start = session
            .text_target("Selection start")
            .expect("selection start geometry");
        assert_eq!(
            session.pointer_down(drag_start.anchor[0], drag_start.anchor[1]),
            SessionClick::Handled
        );
        assert!(session.pointer_move(next_text.focus[0], next_text.focus[1]));
        assert_eq!(
            session.pointer_up(next_text.focus[0], next_text.focus[1]),
            SessionClick::Handled,
            "a drag selection ending on a link wins over navigation"
        );
    }

    #[cfg(feature = "livery")]
    #[test]
    fn livery_session_routes_retained_structural_and_text_paint() {
        let mut registry: SessionRegistry<Scene> = SessionRegistry::new();
        registry.register(Box::new(LiverySessionEngine::new(NoFetch)));
        assert!(registry.contains(inker::routing::ENGINE_GENET_LIVERY));

        let request = SessionSpawnRequest::new("https://example.test/")
            .with_body(
                r#"<html><head><style>.card { background-color: navy; color: white; width: 120px; }</style></head><body><div class="card">Livery <span>session</span></div></body></html>"#,
            )
            .with_viewport(320, 240);
        let mut session = registry
            .spawn(inker::routing::ENGINE_GENET_LIVERY, &request)
            .expect("registered Livery lane spawns from body");

        let first = session.frame(320, 240);
        assert!(
            first
                .ops
                .iter()
                .any(|operation| matches!(operation, netrender::SceneOp::Rect(_)))
        );
        assert!(
            first
                .ops
                .iter()
                .any(|operation| matches!(operation, netrender::SceneOp::GlyphRun(_)))
        );
        let concrete = session
            .as_any()
            .downcast_mut::<LiveryDocumentSession>()
            .expect("session keeps its concrete Livery owner");
        let generation = concrete.document().generation();
        let shape_count = concrete.document().text_system().shape_count();
        assert_eq!(concrete.last_error(), None);

        let _cached = session.frame(320, 240);
        let concrete = session
            .as_any()
            .downcast_mut::<LiveryDocumentSession>()
            .expect("session keeps its concrete Livery owner");
        assert_eq!(concrete.document().generation(), generation);
        assert_eq!(concrete.document().text_system().shape_count(), shape_count);
        assert!(!session.scroll_by(0.0, 100.0));
        assert_eq!(session.click_at(20.0, 20.0), SessionClick::Miss);
    }

    #[cfg(feature = "livery")]
    #[test]
    fn livery_session_activates_the_first_matching_text_directive() {
        let engine = LiverySessionEngine::new(NoFetch);
        let request = SessionSpawnRequest::new(
            "https://example.test/article#:~:text=missing&text=prefix-,target,end,-suffix",
        )
        .with_body(
            r#"<html><head><style>body { margin: 0; }</style></head><body>
                <div style="height: 900px"></div><p>prefix target end suffix</p>
                </body></html>"#,
        )
        .with_viewport(320, 160);
        let mut session = engine.spawn(&request).expect("livery session spawns");

        let scene = session.frame(320, 160);
        let concrete = session
            .as_any()
            .downcast_mut::<LiveryDocumentSession>()
            .expect("retained static session");
        let selection = concrete
            .document()
            .text_selection()
            .expect("the second directive matched in source order");
        assert_eq!(selection.text, "target end");
        assert!(
            concrete.document().scroll().1 > 0.0,
            "activation reveals the retained match"
        );
        assert!(
            scene
                .ops
                .iter()
                .any(|operation| matches!(operation, netrender::SceneOp::Rect(_))),
            "the selection is emitted as scene indication geometry"
        );
    }

    #[cfg(feature = "livery")]
    #[test]
    fn livery_session_falls_back_to_the_ordinary_element_fragment() {
        let engine = LiverySessionEngine::new(NoFetch);
        let request =
            SessionSpawnRequest::new("https://example.test/article#fallback:~:text=not-present")
                .with_body(
                    r#"<html><head><style>body { margin: 0; }</style></head><body>
                <div style="height: 900px"></div><p id="fallback">ordinary fallback</p>
                </body></html>"#,
                )
                .with_viewport(320, 160);
        let mut session = engine.spawn(&request).expect("livery session spawns");

        let _scene = session.frame(320, 160);
        let concrete = session
            .as_any()
            .downcast_mut::<LiveryDocumentSession>()
            .expect("retained static session");
        assert!(concrete.document().text_selection().is_none());
        assert!(
            concrete.document().scroll().1 > 0.0,
            "an unmatched text directive falls through to #fallback"
        );
    }

    /// The livery lane's structural report through the trait — the same
    /// contract the static lane serves, so a viewer override to livery keeps
    /// the Inspector/a11y read instead of degrading to "none for this lane".
    #[cfg(feature = "livery")]
    #[test]
    fn livery_session_reports_structure_through_the_trait() {
        let engine = LiverySessionEngine::new(NoFetch);
        let request = SessionSpawnRequest::new("https://example.test/")
            .with_body(
                "<html><head><title>The Page</title></head>\
                 <body><h1>Heading</h1><a href=\"/next\">next</a></body></html>",
            )
            .with_viewport(640, 480);
        let mut session = engine.spawn(&request).expect("livery lane spawns");
        let report = session
            .inspect()
            .expect("the livery lane has a structural read");
        assert_eq!(report.title.as_deref(), Some("The Page"));
        assert_eq!(report.headings, vec!["Heading"]);
        assert_eq!(report.links, vec!["/next"]);

        let _scene = session.frame(640, 480);
        let link = session.links().into_iter().next().expect("retained link");
        let pointer = |state| inker::SessionInput::PointerButton {
            x: link.rect[0] + 2.0,
            y: link.rect[1] + 2.0,
            button: inker::SessionPointerButton::Primary,
            state,
            modifiers: SessionModifiers::default(),
        };
        let pressed = session.input(pointer(SessionButtonState::Pressed));
        assert_eq!(pressed.effect, SessionEffect::Handled);
        assert_eq!(pressed.capture, Some(true));
        let released = session.input(pointer(SessionButtonState::Released));
        assert_eq!(released.effect, SessionEffect::Navigate("/next".to_owned()));
        assert_eq!(released.capture, Some(false));
    }

    #[cfg(feature = "livery")]
    #[test]
    fn livery_session_retains_structural_find_and_wraps_reveal() {
        let engine = LiverySessionEngine::new(NoFetch);
        let request = SessionSpawnRequest::new("https://example.test/find")
            .with_body(
                "<html><body><h1>Finding heading</h1><p>First finding.</p>\
                 <div style=\"height: 1200px\"></div><p>Last finding.</p></body></html>",
            )
            .with_viewport(480, 240);
        let mut session = engine.spawn(&request).expect("livery lane spawns");
        let _ = session.frame(480, 240);

        let capabilities = session.document_capabilities();
        assert_eq!(
            capabilities.find_in_page,
            DocumentCapabilityStatus::Supported
        );
        assert!(matches!(
            capabilities.page_zoom,
            DocumentCapabilityStatus::Unsupported { .. }
        ));
        assert!(matches!(
            capabilities.page_capture,
            DocumentCapabilityStatus::Unsupported { .. }
        ));
        assert!(matches!(
            capabilities.navigation,
            DocumentCapabilityStatus::Partial { .. }
        ));

        let state = session
            .document_find(&DocumentFindQuery::new("finding"))
            .expect("livery supplies retained find");
        assert_eq!(state.matches.len(), 3);
        assert_eq!(state.count, 3);
        assert_eq!(state.current, Some(0));
        assert_eq!(
            state.current_match().and_then(|item| item.role.as_deref()),
            Some("heading")
        );
        assert_eq!(
            state.current_match().map(|item| item.label.as_str()),
            Some("Finding heading")
        );

        let state = session
            .document_find_step(DocumentFindDirection::Previous)
            .expect("previous wraps");
        assert_eq!(state.current, Some(2));
        assert_eq!(
            state.current_match().map(|item| item.label.as_str()),
            Some("Last finding.")
        );
        let concrete = session
            .as_any_ref()
            .downcast_ref::<LiveryDocumentSession>()
            .expect("livery session remains concrete");
        assert!(
            concrete.document().scroll().1 > 0.0,
            "wrapped match is revealed"
        );
        assert!(concrete.document().text_selection().is_some());

        let state = session
            .document_find_step(DocumentFindDirection::Next)
            .expect("next wraps");
        assert_eq!(state.current, Some(0));

        let changed = session
            .document_find(&DocumentFindQuery::new("LAST"))
            .expect("query replacement recomputes the model");
        assert_eq!(changed.matches.len(), 1);
        assert_eq!(changed.current, Some(0));
        assert_eq!(changed.matches[0].role.as_deref(), Some("paragraph"));
    }

    #[cfg(feature = "livery")]
    #[test]
    fn livery_session_clip_retains_the_source_response() {
        let engine = LiverySessionEngine::new(NoFetch);
        let body = "<html><head><title>The Page</title></head><body><main>\
                    <h1>Heading</h1><p>A useful finding.</p></main></body></html>";
        let request = SessionSpawnRequest::new("https://example.test/report")
            .with_body(body)
            .with_content_type("text/html; charset=utf-8");
        let session = engine.spawn(&request).expect("livery lane spawns");
        let clip = session.clip().expect("the livery lane can supply a clip");

        assert_eq!(clip.artifacts.len(), 1);
        assert_eq!(
            clip.artifacts[0].role,
            DocumentClipArtifactRole::SourceResponse
        );
        assert_eq!(clip.artifacts[0].media_type, "text/html; charset=utf-8");
        assert_eq!(
            clip.artifacts[0].canonical_uri,
            "https://example.test/report"
        );
        assert_eq!(clip.artifacts[0].bytes, body.as_bytes());
    }

    #[cfg(feature = "livery")]
    #[test]
    fn livery_session_pointer_selection_scopes_clip_and_selector() {
        let engine = LiverySessionEngine::new(NoFetch);
        let request = SessionSpawnRequest::new("https://example.test/report")
            .with_body(
                "<html><head><title>The Page</title><style>\
                 html, body, p { margin: 0; padding: 0; }\
                 </style></head><body><p>before \
                 <a href=\"/chosen\">selected link</a> after \
                 <a href=\"/outside\">outside</a></p></body></html>",
            )
            .with_viewport(640, 200);
        let mut session = engine.spawn(&request).expect("spawns");
        let unselected = session.frame(640, 200);
        let unselected_rects = unselected
            .ops
            .iter()
            .filter(|op| matches!(op, netrender::SceneOp::Rect(_)))
            .count();
        let target = session
            .text_target("selected link")
            .expect("Livery source ranges resolve to pointer endpoints");

        assert_eq!(
            session.pointer_down(target.anchor[0], target.anchor[1]),
            SessionClick::Handled
        );
        assert!(
            session.pointer_move(target.focus[0], target.focus[1]),
            "the range extends through ordinary pointer input"
        );
        assert_eq!(
            session.pointer_up(target.focus[0], target.focus[1]),
            SessionClick::Handled
        );

        let selected = session.frame(640, 200);
        let selected_rects = selected
            .ops
            .iter()
            .filter(|op| matches!(op, netrender::SceneOp::Rect(_)))
            .count();
        assert!(
            selected_rects > unselected_rects,
            "the retained Livery range paints selection geometry"
        );

        let clip = session.clip().expect("selection supplies a clip");
        assert_eq!(clip.source_url, "https://example.test/report");
        assert_eq!(clip.text, "selected link");
        assert_eq!(clip.links, vec!["/chosen"]);
        let selector: serde_json::Value =
            serde_json::from_str(clip.selector.as_deref().expect("range selector"))
                .expect("selector is typed JSON");
        assert_eq!(selector["type"], "dom-range");
        assert_eq!(selector["version"], 1);
        assert_eq!(selector["quote"], "selected link");
        assert!(selector["anchor"]["path"].is_array());
        assert!(selector["focus"]["path"].is_array());
    }

    #[cfg(feature = "livery")]
    #[test]
    fn livery_session_routes_scroll_focus_and_links() {
        let engine = LiverySessionEngine::new(NoFetch);
        let request = SessionSpawnRequest::new("https://example.test/")
            .with_body(
                r##"<html><head><style>
                    html, body { margin: 0; padding: 0; }
                    .link, .target { display: block; width: 100px; height: 20px; }
                    .spacer { height: 500px; }
                </style></head><body>
                    <a class="link" href="#target">top</a>
                    <div class="spacer"></div>
                    <div id="target" class="target">target</div>
                </body></html>"##,
            )
            .with_viewport(320, 240);
        let mut session = engine.spawn(&request).expect("spawns");
        let _scene = session.frame(320, 240);

        assert!(session.content_height(320, 240) > 240);
        let link = session.links().into_iter().next().expect("retained link");
        let click = session.click_at(link.rect[0] + 5.0, link.rect[1] + 5.0);
        assert_eq!(click, SessionClick::Handled);
        assert!(session.scroll_for_key(SessionScrollKey::Home));
        assert!(session.scroll_by(0.0, 100.0));
        assert!(session.scroll_for_key(SessionScrollKey::Home));
        assert!(session.scroll_at(10.0, 10.0, 0.0, 100.0));
    }

    #[cfg(feature = "livery")]
    #[test]
    fn livery_session_edits_and_submits_a_retained_get_form() {
        let engine = LiverySessionEngine::new(NoFetch);
        let request = SessionSpawnRequest::new("fixtures/form/index.html")
            .with_body(
                r#"<html><head><style>
                    html, body { margin: 0; padding: 0; }
                    textarea, button { display: block; width: 220px; min-height: 40px; margin: 8px; }
                </style></head><body><form action="result.html" method="get">
                    <textarea id="note" name="note">cedar</textarea>
                    <button id="submit" type="submit">send</button>
                </form></body></html>"#,
            )
            .with_viewport(400, 240);
        let mut session = engine.spawn(&request).expect("form session spawns");
        let initial = session.frame(400, 240);
        let initial_glyphs = initial
            .ops
            .iter()
            .filter_map(|operation| match operation {
                netrender::SceneOp::GlyphRun(run) => Some(run.glyphs.len()),
                _ => None,
            })
            .sum::<usize>();
        let target = session
            .text_target("cedar")
            .expect("the textarea value has a retained text target");
        let point = (target.anchor[0] + 2.0, target.anchor[1]);
        let pointer = |state| inker::SessionInput::PointerButton {
            x: point.0,
            y: point.1,
            button: inker::SessionPointerButton::Primary,
            state,
            modifiers: SessionModifiers::default(),
        };
        assert!(
            session
                .input(pointer(SessionButtonState::Pressed))
                .effect
                .is_handled()
        );
        let released = session.input(pointer(SessionButtonState::Released));
        assert!(released.editable);
        assert_eq!(released.cursor, Some(SessionCursor::Text));

        let edited = session.input(inker::SessionInput::Text(" and ash".to_owned()));
        assert_eq!(edited.effect, SessionEffect::Handled);
        let edited_scene = session.frame(400, 240);
        let edited_glyphs = edited_scene
            .ops
            .iter()
            .filter_map(|operation| match operation {
                netrender::SceneOp::GlyphRun(run) => Some(run.glyphs.len()),
                _ => None,
            })
            .sum::<usize>();
        assert!(
            edited_glyphs > initial_glyphs,
            "the textarea edit reaches paint"
        );
        assert!(
            session
                .inspect()
                .expect("form remains inspectable")
                .outline
                .iter()
                .any(|entry| entry.role == "textbox" && entry.name == "cedar and ash")
        );

        let tabbed = session.input(inker::SessionInput::Key {
            key: SessionKey::Tab,
            state: SessionButtonState::Pressed,
            modifiers: SessionModifiers::default(),
            repeat: false,
        });
        assert_eq!(tabbed.effect, SessionEffect::Handled);
        assert!(!tabbed.editable, "Tab moves focus to the submit button");
        let submitted = session.input(inker::SessionInput::Key {
            key: SessionKey::Enter,
            state: SessionButtonState::Pressed,
            modifiers: SessionModifiers::default(),
            repeat: false,
        });
        let SessionEffect::Submit(submission) = submitted.effect else {
            panic!("Enter on the focused submit button must submit: {submitted:?}");
        };
        assert_eq!(submission.action, "result.html");
        assert_eq!(submission.method, SessionFormMethod::Get);
        assert_eq!(
            submission.fields,
            [("note".to_owned(), "cedar and ash".to_owned())]
        );
    }

    #[cfg(feature = "livery")]
    #[test]
    fn livery_session_fetches_remote_image_resources_through_the_host() {
        let image = image::RgbaImage::from_pixel(2, 3, image::Rgba([0, 0, 255, 255]));
        let mut bytes = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .expect("encode test PNG");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let engine = LiverySessionEngine::new(ImageFetch {
            bytes,
            requests: requests.clone(),
        });
        let request = SessionSpawnRequest::new("https://example.test/docs/index.html")
            .with_body(
                r#"<html><head><style>
                    .card { display: block; width: 80px; height: 40px;
                            background-repeat: no-repeat;
                            background-image: url(hero.png); }
                </style></head><body><div class="card"></div></body></html>"#,
            )
            .with_viewport(320, 240);
        let mut session = engine.spawn(&request).expect("Livery lane spawns");

        let scene = session.frame(320, 240);
        assert!(
            scene
                .ops
                .iter()
                .any(|operation| matches!(operation, netrender::SceneOp::Image(_))),
            "host-fetched image reaches the scene"
        );
        assert_eq!(
            requests.lock().unwrap().as_slice(),
            ["https://example.test/docs/hero.png"]
        );
    }

    #[cfg(feature = "livery")]
    #[test]
    fn livery_session_uses_linked_sheet_identity_and_sheet_relative_resources() {
        let image = image::RgbaImage::from_pixel(2, 3, image::Rgba([0, 0, 255, 255]));
        let mut image_bytes = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut image_bytes),
                image::ImageFormat::Png,
            )
            .expect("encode test PNG");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let engine = LiverySessionEngine::new(LinkedResourceFetch {
            image: image_bytes,
            // The retained text system accepts host-provided bytes; rendering
            // the fake fixture font is outside this source-attribution test.
            font: b"not-a-real-font".to_vec(),
            requests: requests.clone(),
        });
        let request = SessionSpawnRequest::new("https://example.test/docs/index.html")
            .with_body(
                r#"<html><head><link rel="stylesheet" href="styles/site.css" media="screen"></head>
<body><div class="card">linked resource</div></body></html>"#,
            )
            .with_viewport(320, 240);
        let mut session = engine.spawn(&request).expect("linked Livery route spawns");
        let _ = session.frame(320, 240);
        let concrete = session
            .as_any()
            .downcast_mut::<LiveryDocumentSession>()
            .expect("session keeps its resource ledger");
        assert_eq!(concrete.resources.stylesheets.len(), 1);
        let sheet = &concrete.resources.stylesheets[0];
        assert_eq!(sheet.media.as_deref(), Some("screen"));
        assert_eq!(
            sheet.source_url.as_deref(),
            Some("https://example.test/docs/styles/site.css")
        );
        assert!(concrete.resources.resources.iter().any(|resource| {
            resource.kind == ResourceKind::Image
                && resource.resolved_url == "https://example.test/docs/styles/images/hero.png"
        }));
        assert!(concrete.resources.resources.iter().any(|resource| {
            resource.kind == ResourceKind::Font
                && resource.resolved_url == "https://example.test/docs/fonts/text.woff2"
        }));
        assert!(concrete.resource_diagnostics().is_empty());
        assert_eq!(
            requests.lock().unwrap().as_slice(),
            [
                "https://example.test/docs/styles/site.css",
                "https://example.test/docs/styles/images/hero.png",
                "https://example.test/docs/fonts/text.woff2",
            ]
        );
    }

    #[cfg(feature = "livery")]
    #[test]
    fn livery_session_applies_imports_before_their_redirected_parent_sheet() {
        let engine = LiverySessionEngine::new(ImportedSheetFetch);
        let request = SessionSpawnRequest::new("https://example.test/docs/index.html")
            .with_body(
                r#"<html><head><link rel="stylesheet" href="styles/root.css"></head>
<body><p class="card">cascade</p></body></html>"#,
            )
            .with_viewport(320, 240);
        let mut session = engine
            .spawn(&request)
            .expect("imported Livery route spawns");
        let scene = session.frame(320, 240);
        assert!(
            scene.ops.iter().any(|operation| {
                matches!(operation, netrender::SceneOp::GlyphRun(run) if run.color == [1.0, 0.0, 0.0, 1.0])
            }),
            "the parent sheet follows the imported sheet in the author cascade"
        );
        let concrete = session
            .as_any()
            .downcast_mut::<LiveryDocumentSession>()
            .expect("session keeps its resource ledger");
        assert!(concrete.resource_diagnostics().is_empty());
        assert_eq!(concrete.resources.stylesheets.len(), 2);
        assert_eq!(
            concrete.resources.stylesheets[0].owner,
            StylesheetOwner::Imported
        );
        assert_eq!(
            concrete.resources.stylesheets[1].source_url.as_deref(),
            Some("https://cdn.example.test/styles/root.css")
        );
        assert_eq!(
            concrete.resources.stylesheets[1].requested_url.as_deref(),
            Some("https://example.test/docs/styles/root.css")
        );
    }

    #[cfg(feature = "livery")]
    #[test]
    fn livery_session_applies_host_selected_import_limits() {
        let engine =
            LiverySessionEngine::new(ImportedSheetFetch).with_resource_limits(ResourceLimits {
                max_import_depth: 0,
                max_stylesheet_bytes: 2 * 1024 * 1024,
            });
        let request = SessionSpawnRequest::new("https://example.test/docs/index.html").with_body(
            r#"<html><head><link rel="stylesheet" href="styles/root.css"></head>
<body><p class="card">bounded</p></body></html>"#,
        );
        let session = engine.spawn(&request).expect("bounded Livery route spawns");
        let concrete = session
            .as_any_ref()
            .downcast_ref::<LiveryDocumentSession>()
            .expect("session keeps its resource ledger");
        assert_eq!(concrete.resource_set().stylesheets.len(), 1);
        assert!(matches!(
            concrete.resource_diagnostics(),
            [
                genet_document_resources::ResourceDiagnostic::ImportRuleDepthLimit {
                    max_depth: 0,
                    ..
                }
            ]
        ));
    }

    #[cfg(feature = "livery")]
    #[test]
    fn livery_session_resolves_links_against_a_redirected_document_identity() {
        let engine = LiverySessionEngine::new(RedirectedDocumentFetch);
        let request =
            SessionSpawnRequest::new("https://example.test/start").with_viewport(320, 240);
        let mut session = engine.spawn(&request).expect("redirected document spawns");
        let scene = session.frame(320, 240);
        assert!(
            scene.ops.iter().any(|operation| {
                matches!(operation, netrender::SceneOp::GlyphRun(run) if run.color == [0.0, 128.0 / 255.0, 0.0, 1.0])
            }),
            "the final document identity supplies the linked stylesheet base"
        );
        let clip = session.clip().expect("redirected document supplies a clip");
        assert_eq!(
            clip.artifacts[0].canonical_uri,
            "https://cdn.example.test/final/index.html"
        );
        assert_eq!(clip.artifacts[0].media_type, "text/html");
        let concrete = session
            .as_any()
            .downcast_mut::<LiveryDocumentSession>()
            .expect("session keeps its resource ledger");
        assert_eq!(
            concrete.resources.document_url.as_deref(),
            Some("https://cdn.example.test/final/index.html")
        );
        assert_eq!(
            concrete.resources.stylesheets[0].source_url.as_deref(),
            Some("https://cdn.example.test/final/site.css")
        );
    }
}
