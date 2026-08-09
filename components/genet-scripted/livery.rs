/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Livery's scripted CSSOM adapter.
//!
//! The JS runtime owns the live mutable DOM. Livery owns retained author rule
//! objects beside it, and resolves a style plane on demand for
//! `getComputedStyle`. This is intentionally a style/session bridge rather than
//! a second DOM copy: script mutations are visible to the next read immediately.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use genet_document_resources::{ResolvedDocumentResources, ResourceFetcher, ResourceLimits};
use genet_livery::{
    Device, IncrementalStyle, InteractionStates, RestyleStats, RuleMutationError, StyleSet,
    ViewportSizes, canonicalize_specified_value, content_box_size, layout,
    resolve_container_query_styles, resolve_styles,
    used_value_context as layout_used_value_context,
};
use genet_scripted_dom::{NodeId, ScriptedDom};
use layout_dom_api::{
    AttributeView, LayoutDom, LocalName, Namespace, NodeKind, QualName, QuirksMode,
};
use script_engine_api::ScriptEngine;
use script_runtime_api::{
    ComputedStyleHandler, HostState, InlineStyleHandler, InlineStyleValueResult, Runtime,
    StyleSheetHandler, StyleSheetImportOwner, StyleSheetImportRule, StyleSheetMutationError,
};

struct LiveryState {
    styles: StyleSet,
    document_sheets: Vec<usize>,
    device: Device,
    interactions: InteractionStates<NodeId>,
    session: IncrementalStyle<NodeId>,
    mutation_cursor: u64,
    live_sheets: Option<LiveStylesheetSource>,
}

struct LiveStylesheetSource {
    document_url: String,
    fetcher: Rc<dyn ResourceFetcher>,
    limits: ResourceLimits,
    resources: ResolvedDocumentResources,
    mutation_cursor: u64,
}

impl LiveryState {
    fn sheet_key_at(&self, list_index: usize) -> Option<String> {
        let author_index = *self.document_sheets.get(list_index)?;
        self.styles.cssom_key(author_index).map(str::to_owned)
    }

    fn author_index_for_key(&self, key: &str) -> Option<usize> {
        self.styles.author_index_by_cssom_key(key)
    }
}

fn synchronize_live_styles(host: &HostState, state: &mut LiveryState) {
    let Some(live) = state.live_sheets.as_mut() else {
        return;
    };
    let (base, pending) = host.dom.pending_mutations();
    let end = base.saturating_add(pending.len() as u64);
    if live.mutation_cursor >= base && live.mutation_cursor == end {
        return;
    }
    let resources = ResolvedDocumentResources::resolve_with_limits(
        &host.dom,
        Some(&live.document_url),
        live.fetcher.as_ref(),
        live.limits,
    );
    if resources != live.resources {
        state.styles.replace_author_sheets(&resources.stylesheets);
        state.document_sheets = state.styles.document_sheet_indexes();
        live.resources = resources;
    }
    live.mutation_cursor = end;
}

/// A retained Livery stylesheet session installed on one scripted runtime.
///
/// Keep this handle when the host needs to update the media device. The runtime
/// itself retains the handler state for JS reads and mutations.
#[derive(Clone)]
pub struct LiveryCssom {
    state: Rc<RefCell<LiveryState>>,
}

impl LiveryCssom {
    /// Install Livery as the runtime's `document.styleSheets` and
    /// `getComputedStyle` provider. `author_sheets` are ordered exactly as they
    /// appear in the document.
    pub fn install<E: ScriptEngine>(
        runtime: &mut Runtime<E>,
        author_sheets: &[&str],
        device: Device,
    ) -> Self {
        let styles = StyleSet::cambium(author_sheets);
        let state = Rc::new(RefCell::new(LiveryState {
            document_sheets: styles.document_sheet_indexes(),
            styles,
            device,
            interactions: InteractionStates::default(),
            session: IncrementalStyle::new(),
            mutation_cursor: 0,
            live_sheets: None,
        }));
        Self::install_state(runtime, state)
    }

    /// Install a live resource-backed stylesheet set. The shared resolver runs
    /// again after DOM mutations and reconciles retained direct-sheet CSSOM
    /// objects by their owner node, while imported sheets remain cascade-only
    /// children of their parent source.
    pub fn install_live<E, Fetch>(
        runtime: &mut Runtime<E>,
        fetcher: Fetch,
        document_url: impl Into<String>,
        limits: ResourceLimits,
        device: Device,
    ) -> Self
    where
        E: ScriptEngine,
        Fetch: ResourceFetcher + 'static,
    {
        let document_url = document_url.into();
        let fetcher: Rc<dyn ResourceFetcher> = Rc::new(fetcher);
        let (resources, mutation_cursor) = {
            let host = runtime.host().borrow();
            let resources = ResolvedDocumentResources::resolve_with_limits(
                &host.dom,
                Some(&document_url),
                fetcher.as_ref(),
                limits,
            );
            let (base, pending) = host.dom.pending_mutations();
            (resources, base.saturating_add(pending.len() as u64))
        };
        let styles = StyleSet::cambium_resources(&resources.stylesheets);
        let state = Rc::new(RefCell::new(LiveryState {
            document_sheets: styles.document_sheet_indexes(),
            styles,
            device,
            interactions: InteractionStates::default(),
            session: IncrementalStyle::new(),
            mutation_cursor,
            live_sheets: Some(LiveStylesheetSource {
                document_url,
                fetcher,
                limits,
                resources,
                mutation_cursor,
            }),
        }));
        Self::install_state(runtime, state)
    }

    fn install_state<E: ScriptEngine>(
        runtime: &mut Runtime<E>,
        state: Rc<RefCell<LiveryState>>,
    ) -> Self {
        let host = Rc::downgrade(runtime.host());
        runtime.set_computed_style_handler(Box::new(LiveryComputedStyle {
            host: host.clone(),
            state: state.clone(),
        }));
        runtime.set_inline_style_handler(Box::new(LiveryInlineStyle));
        runtime.set_stylesheet_handler(Box::new(LiveryStyleSheets {
            state: state.clone(),
            host,
        }));
        Self { state }
    }

    /// Update the device used by media queries and computed-value resolution.
    pub fn set_viewport_size(&self, width: f32, height: f32) {
        let mut state = self.state.borrow_mut();
        state.device.set_viewport_size(width, height);
    }

    /// Supply distinct small, large, and dynamic viewport sizes.
    pub fn set_viewport_sizes(&self, sizes: ViewportSizes) {
        self.state.borrow_mut().device.set_viewport_sizes(sizes);
    }

    /// The retained generation stamp for one author sheet.
    pub fn generation(&self, sheet: usize) -> Option<u64> {
        let state = self.state.borrow();
        let sheet = *state.document_sheets.get(sheet)?;
        state
            .styles
            .author_sheets()
            .get(sheet)
            .map(|sheet| sheet.generation())
    }

    /// The shared resource ledger for a live CSSOM installation. Static
    /// `install` sessions have no resource owner and return `None`.
    pub fn resource_set(&self) -> Option<ResolvedDocumentResources> {
        self.state
            .borrow()
            .live_sheets
            .as_ref()
            .map(|live| live.resources.clone())
    }

    /// Work performed by the latest scripted computed-style read.
    pub fn last_restyle_stats(&self) -> RestyleStats {
        self.state.borrow().session.last_stats()
    }
}

struct LiveryInlineStyle;

impl InlineStyleHandler for LiveryInlineStyle {
    fn canonicalize(&self, property: &str, value: &str) -> InlineStyleValueResult {
        if let Some(value) = canonicalize_specified_value(property, value) {
            InlineStyleValueResult::Canonical(value)
        } else if genet_livery::PropertyId::from_css_name(&property.to_ascii_lowercase()).is_some()
            && !value.to_ascii_lowercase().contains("var(")
        {
            InlineStyleValueResult::Invalid
        } else {
            InlineStyleValueResult::PassThrough
        }
    }
}

struct LiveryComputedStyle {
    host: Weak<RefCell<HostState>>,
    state: Rc<RefCell<LiveryState>>,
}

fn needs_used_values(property: &str) -> bool {
    matches!(
        property.to_ascii_lowercase().as_str(),
        "width" | "height" | "margin-top" | "margin-right" | "margin-bottom" | "margin-left"
    )
}

impl ComputedStyleHandler for LiveryComputedStyle {
    fn computed_value(&self, node: u64, property: &str) -> Option<String> {
        let host = self.host.upgrade()?;
        let host = host.borrow();
        let (base, pending) = host.dom.pending_mutations();
        let end = base.saturating_add(pending.len() as u64);
        let mut state = self.state.borrow_mut();
        synchronize_live_styles(&host, &mut state);
        if state.mutation_cursor < base || state.mutation_cursor > end {
            state.session.invalidate();
            state.mutation_cursor = base;
        }
        let start = state.mutation_cursor.saturating_sub(base) as usize;
        let LiveryState {
            styles,
            device,
            interactions,
            session,
            ..
        } = &mut *state;
        session.update(&host.dom, styles, device, interactions, &pending[start..]);
        let node = NodeId::from_raw(node as usize);
        let container_resolved = resolve_container_query_styles(
            &host.dom,
            session.styles(),
            styles,
            device,
            interactions,
        )
        .ok();
        let computed_styles = container_resolved
            .as_ref()
            .unwrap_or_else(|| session.styles());
        let used = needs_used_values(property)
            .then(|| {
                layout_used_value_context(
                    &host.dom,
                    computed_styles,
                    device.viewport_width,
                    device.viewport_height,
                    node,
                )
                .ok()
                .flatten()
            })
            .flatten();
        let value = computed_styles.computed_style_with_used_values(node, property, used);
        state.mutation_cursor = end;
        value
    }

    fn computed_value_in_context(&self, context: u64, node: u64, property: &str) -> Option<String> {
        let host = self.host.upgrade()?;
        let host = host.borrow();
        let (base, pending) = host.dom.pending_mutations();
        let end = base.saturating_add(pending.len() as u64);
        let mut state = self.state.borrow_mut();
        synchronize_live_styles(&host, &mut state);
        if state.mutation_cursor < base || state.mutation_cursor > end {
            state.session.invalidate();
            state.mutation_cursor = base;
        }
        let start = state.mutation_cursor.saturating_sub(base) as usize;
        let LiveryState {
            styles,
            device,
            interactions,
            session,
            mutation_cursor,
            ..
        } = &mut *state;
        session.update(&host.dom, styles, device, interactions, &pending[start..]);
        *mutation_cursor = end;

        let primary = resolve_container_query_styles(
            &host.dom,
            session.styles(),
            styles,
            device,
            interactions,
        )
        .ok();
        let primary = primary.as_ref().unwrap_or_else(|| session.styles());
        let context = NodeId::from_raw(context as usize);
        let fragments = layout(
            &host.dom,
            primary,
            device.viewport_width,
            device.viewport_height,
        )
        .ok()?;
        let frame_style = primary.get(context)?;
        let frame_fragment = fragments.get(context)?;
        let (width, height) = content_box_size(frame_style, frame_fragment);

        let node = NodeId::from_raw(node as usize);
        let document = owning_document(&host.dom, node)?;
        if document == host.dom.document() {
            let used = needs_used_values(property)
                .then(|| {
                    layout_used_value_context(
                        &host.dom,
                        primary,
                        device.viewport_width,
                        device.viewport_height,
                        node,
                    )
                    .ok()
                    .flatten()
                })
                .flatten();
            return primary.computed_style_with_used_values(node, property, used);
        }
        let scoped = ScopedDom {
            dom: &host.dom,
            document,
        };
        let sheets = inline_stylesheets(&scoped);
        let sheet_refs = sheets.iter().map(String::as_str).collect::<Vec<_>>();
        let child_styles = StyleSet::cambium(&sheet_refs);
        let child_device = Device::screen(width, height);
        let child_interactions = InteractionStates::default();
        let child_plane =
            resolve_styles(&scoped, &child_styles, &child_device, &child_interactions);
        let child_plane = resolve_container_query_styles(
            &scoped,
            &child_plane,
            &child_styles,
            &child_device,
            &child_interactions,
        )
        .ok()?;
        let used = needs_used_values(property)
            .then(|| {
                layout_used_value_context(&scoped, &child_plane, width, height, node)
                    .ok()
                    .flatten()
            })
            .flatten();
        child_plane.computed_style_with_used_values(node, property, used)
    }
}

struct ScopedDom<'a> {
    dom: &'a ScriptedDom,
    document: NodeId,
}

impl LayoutDom for ScopedDom<'_> {
    type NodeId = NodeId;

    fn document(&self) -> Self::NodeId {
        self.document
    }

    fn is_live(&self, id: Self::NodeId) -> bool {
        self.dom.is_live(id)
    }

    fn quirks_mode(&self) -> QuirksMode {
        self.dom.quirks_mode()
    }

    fn parent(&self, id: Self::NodeId) -> Option<Self::NodeId> {
        self.dom.parent(id)
    }

    fn prev_sibling(&self, id: Self::NodeId) -> Option<Self::NodeId> {
        self.dom.prev_sibling(id)
    }

    fn next_sibling(&self, id: Self::NodeId) -> Option<Self::NodeId> {
        self.dom.next_sibling(id)
    }

    fn dom_children(&self, id: Self::NodeId) -> impl Iterator<Item = Self::NodeId> + '_ {
        self.dom.dom_children(id)
    }

    fn kind(&self, id: Self::NodeId) -> NodeKind {
        self.dom.kind(id)
    }

    fn opaque_id(&self, id: Self::NodeId) -> u64 {
        self.dom.opaque_id(id)
    }

    fn element_name(&self, id: Self::NodeId) -> Option<&QualName> {
        self.dom.element_name(id)
    }

    fn attribute(
        &self,
        id: Self::NodeId,
        namespace: &Namespace,
        local: &LocalName,
    ) -> Option<&str> {
        self.dom.attribute(id, namespace, local)
    }

    fn attributes(&self, id: Self::NodeId) -> impl Iterator<Item = AttributeView<'_>> + '_ {
        self.dom.attributes(id)
    }

    fn text(&self, id: Self::NodeId) -> Option<&str> {
        self.dom.text(id)
    }
}

fn owning_document(dom: &ScriptedDom, mut node: NodeId) -> Option<NodeId> {
    loop {
        if dom.kind(node) == NodeKind::Document {
            return Some(node);
        }
        node = dom.parent(node)?;
    }
}

fn inline_stylesheets(dom: &impl LayoutDom) -> Vec<String> {
    fn text_content<D: LayoutDom>(dom: &D, node: D::NodeId, output: &mut String) {
        if dom.kind(node) == NodeKind::Text {
            output.push_str(dom.text(node).unwrap_or(""));
        }
        for child in dom.dom_children(node) {
            text_content(dom, child, output);
        }
    }

    fn collect<D: LayoutDom>(dom: &D, node: D::NodeId, output: &mut Vec<String>) {
        if dom
            .element_name(node)
            .is_some_and(|name| name.local.as_ref() == "style")
        {
            let mut sheet = String::new();
            text_content(dom, node, &mut sheet);
            output.push(sheet);
        }
        for child in dom.dom_children(node) {
            collect(dom, child, output);
        }
    }

    let mut sheets = Vec::new();
    collect(dom, dom.document(), &mut sheets);
    sheets
}

struct LiveryStyleSheets {
    state: Rc<RefCell<LiveryState>>,
    host: Weak<RefCell<HostState>>,
}

impl LiveryStyleSheets {
    fn synchronize(&self) {
        let Some(host) = self.host.upgrade() else {
            return;
        };
        let host = host.borrow();
        synchronize_live_styles(&host, &mut self.state.borrow_mut());
    }

    fn author_index(&self, sheet: usize) -> Option<usize> {
        self.state.borrow().document_sheets.get(sheet).copied()
    }

    fn author_index_by_key(&self, key: &str) -> Option<usize> {
        self.state.borrow().author_index_for_key(key)
    }
}

impl StyleSheetHandler for LiveryStyleSheets {
    fn sheet_count(&self) -> usize {
        self.synchronize();
        self.state.borrow().document_sheets.len()
    }

    fn rule_count(&self, sheet: usize) -> Option<usize> {
        self.synchronize();
        let sheet = self.author_index(sheet)?;
        self.state.borrow().styles.cssom_rule_count(sheet)
    }

    fn insert_rule(
        &self,
        sheet: usize,
        rule: &str,
        index: usize,
    ) -> Result<usize, StyleSheetMutationError> {
        self.synchronize();
        let sheet = self
            .author_index(sheet)
            .ok_or(StyleSheetMutationError::IndexSize)?;
        self.state
            .borrow_mut()
            .styles
            .insert_cssom_rule(sheet, rule, index)
            .map_err(mutation_error)
    }

    fn delete_rule(&self, sheet: usize, index: usize) -> Result<(), StyleSheetMutationError> {
        self.synchronize();
        let sheet = self
            .author_index(sheet)
            .ok_or(StyleSheetMutationError::IndexSize)?;
        self.state
            .borrow_mut()
            .styles
            .delete_cssom_rule(sheet, index)
            .map_err(mutation_error)
    }

    fn sheet_key(&self, sheet: usize) -> Option<String> {
        self.synchronize();
        self.state.borrow().sheet_key_at(sheet)
    }

    fn rule_count_by_key(&self, key: &str) -> Option<usize> {
        self.synchronize();
        let sheet = self.author_index_by_key(key)?;
        self.state.borrow().styles.cssom_rule_count(sheet)
    }

    fn insert_rule_by_key(
        &self,
        key: &str,
        rule: &str,
        index: usize,
    ) -> Result<usize, StyleSheetMutationError> {
        self.synchronize();
        let sheet = self
            .author_index_by_key(key)
            .ok_or(StyleSheetMutationError::IndexSize)?;
        self.state
            .borrow_mut()
            .styles
            .insert_cssom_rule(sheet, rule, index)
            .map_err(mutation_error)
    }

    fn delete_rule_by_key(&self, key: &str, index: usize) -> Result<(), StyleSheetMutationError> {
        self.synchronize();
        let sheet = self
            .author_index_by_key(key)
            .ok_or(StyleSheetMutationError::IndexSize)?;
        self.state
            .borrow_mut()
            .styles
            .delete_cssom_rule(sheet, index)
            .map_err(mutation_error)
    }

    fn owner_node_by_key(&self, key: &str) -> Option<u64> {
        self.synchronize();
        let sheet = self.author_index_by_key(key)?;
        self.state
            .borrow()
            .styles
            .author_sheets()
            .get(sheet)
            .and_then(|sheet| {
                (sheet.owner() != genet_document_resources::StylesheetOwner::Imported)
                    .then(|| sheet.owner_node())
                    .flatten()
            })
    }

    fn import_rule_by_key(&self, key: &str, index: usize) -> Option<StyleSheetImportRule> {
        self.synchronize();
        let sheet = self.author_index_by_key(key)?;
        self.state
            .borrow()
            .styles
            .cssom_import_rule(sheet, index)
            .map(|rule| StyleSheetImportRule {
                href: rule.href,
                media: rule.media,
                child_sheet_key: rule.child_sheet_key,
            })
    }

    fn import_owner_by_key(&self, key: &str) -> Option<StyleSheetImportOwner> {
        self.synchronize();
        let sheet = self.author_index_by_key(key)?;
        self.state
            .borrow()
            .styles
            .cssom_import_owner(sheet)
            .map(|owner| StyleSheetImportOwner {
                parent_sheet_key: owner.parent_sheet_key,
                import_index: owner.import_index,
            })
    }
}

fn mutation_error(error: RuleMutationError) -> StyleSheetMutationError {
    match error {
        RuleMutationError::IndexSize => StyleSheetMutationError::IndexSize,
        RuleMutationError::Syntax(message) => StyleSheetMutationError::Syntax(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genet_livery::ViewportSize;
    use genet_static_dom::StaticDocument;
    use layout_dom_api::LayoutDomMut;
    use script_engine_boa::BoaEngine;

    struct LiveSheetFetch;

    impl ResourceFetcher for LiveSheetFetch {
        fn fetch(&self, url: &str) -> Option<Vec<u8>> {
            match url {
                "https://example.test/docs/theme.css" => Some(b".card { color: green; }".to_vec()),
                "https://example.test/docs/replacement.css" => {
                    Some(b".card { color: orange; }".to_vec())
                },
                "https://example.test/docs/parent.css" => {
                    Some(b"@import url('child.css') screen; .host { display: block; }".to_vec())
                },
                "https://example.test/docs/child.css" => Some(b".card { color: blue; }".to_vec()),
                _ => None,
            }
        }
    }

    #[test]
    fn boa_live_cssom_reconciles_inserted_removed_and_media_gated_sheets() {
        let mut runtime = Runtime::<BoaEngine>::new().expect("runtime");
        runtime.load_dom(&StaticDocument::parse(
            "<html><head><style id='base'>.card { color: red; }</style></head>\
             <body><div id='card' class='card'></div></body></html>",
        ));
        let cssom = LiveryCssom::install_live(
            &mut runtime,
            LiveSheetFetch,
            "https://example.test/docs/index.html",
            ResourceLimits::default(),
            Device::screen(800.0, 600.0),
        );

        runtime
            .eval(
                "var card = document.getElementById('card');\
                 var head = document.head;\
                 var base = document.styleSheets[0];\
                 console.log(document.styleSheets.length + '|' + base.ownerNode.id + '|' + getComputedStyle(card).color);\
                 base.insertRule('.card { color: purple; }', base.cssRules.length);\
                 console.log(base.cssRules.length + '|' + getComputedStyle(card).color);\
                 var dynamic = document.createElement('style');\
                 dynamic.id = 'dynamic'; dynamic.textContent = '.card { color: blue; }'; head.appendChild(dynamic);\
                 console.log(document.styleSheets.length + '|' + String(document.styleSheets[0] === base));\
                 console.log(document.styleSheets[1].ownerNode.id + '|' + getComputedStyle(card).color);\
                 var link = document.createElement('link');\
                 link.id = 'theme'; link.setAttribute('rel', 'stylesheet'); link.setAttribute('href', 'theme.css'); link.setAttribute('media', 'screen'); head.appendChild(link);\
                 console.log(document.styleSheets.length + '|' + document.styleSheets[2].ownerNode.id + '|' + getComputedStyle(card).color);\
                 var theme = document.styleSheets[2];\
                 link.setAttribute('media', 'print');\
                 console.log(document.styleSheets.length + '|' + getComputedStyle(card).color);\
                 head.removeChild(dynamic);\
                 console.log(document.styleSheets.length + '|' + String(document.styleSheets[0] === base) + '|' + getComputedStyle(card).color);\
                 link.setAttribute('media', 'screen');\
                 console.log(String(document.styleSheets[1] === theme) + '|' + getComputedStyle(card).color);\
                 link.setAttribute('href', 'replacement.css');\
                 console.log(String(document.styleSheets[1] === theme) + '|' + getComputedStyle(card).color);\
                 link.setAttribute('href', 'missing.css');\
                 console.log(document.styleSheets.length + '|' + String(theme.ownerNode === null) + '|' + getComputedStyle(card).color);\
                 head.removeChild(link);\
                 console.log(document.styleSheets.length + '|' + String(document.styleSheets[0] === base) + '|' + getComputedStyle(card).color);",
            )
            .expect("live Livery CSSOM script");

        assert_eq!(
            runtime.host().borrow().console,
            vec![
                "1|base|rgb(255, 0, 0)",
                "2|rgb(128, 0, 128)",
                "2|true",
                "dynamic|rgb(0, 0, 255)",
                "3|theme|rgb(0, 128, 0)",
                "3|rgb(0, 0, 255)",
                "2|true|rgb(128, 0, 128)",
                "true|rgb(0, 128, 0)",
                "true|rgb(255, 165, 0)",
                "1|true|rgb(128, 0, 128)",
                "1|true|rgb(128, 0, 128)",
            ],
        );
        let resources = cssom.resource_set().expect("live resource ledger");
        assert_eq!(resources.stylesheets.len(), 1);
        assert!(resources.stylesheets[0].owner_node.is_some());
    }

    #[test]
    fn boa_live_cssom_exposes_imported_sheet_owner_graph() {
        let mut runtime = Runtime::<BoaEngine>::new().expect("runtime");
        runtime.load_dom(&StaticDocument::parse(
            "<html><head><link id='theme' rel='stylesheet' href='parent.css'></head>\
             <body><div id='card' class='card'></div></body></html>",
        ));
        let _cssom = LiveryCssom::install_live(
            &mut runtime,
            LiveSheetFetch,
            "https://example.test/docs/index.html",
            ResourceLimits::default(),
            Device::screen(800.0, 600.0),
        );

        runtime
            .eval(
                "var card = document.getElementById('card');\
                 var parent = document.styleSheets[0];\
                 var rule = parent.cssRules.item(0);\
                 var child = rule.styleSheet;\
                 console.log(document.styleSheets.length + '|' + parent.ownerNode.id + '|' + parent.cssRules.length + '|' + String(child.ownerNode === null));\
                 console.log(rule.href + '|' + rule.media.mediaText + '|' + String(rule.parentStyleSheet === parent) + '|' + String(child.ownerRule === rule) + '|' + String(rule instanceof CSSImportRule) + '|' + child.cssRules.length + '|' + getComputedStyle(card).color);\
                 console.log(child.insertRule('.card { color: green; }', child.cssRules.length) + '|' + getComputedStyle(card).color);",
            )
            .expect("import ownership CSSOM script");

        assert_eq!(
            runtime.host().borrow().console,
            vec![
                "1|theme|2|true",
                "child.css|screen|true|true|true|1|rgb(0, 0, 255)",
                "1|rgb(0, 128, 0)",
            ],
        );
    }

    #[test]
    fn boa_reaches_livery_stylesheets_mutation_and_computed_values() {
        let mut runtime = Runtime::<BoaEngine>::new().expect("runtime");
        runtime.load_dom(&StaticDocument::parse(
            "<html><body><div id='card' class='card'></div></body></html>",
        ));
        let cssom = LiveryCssom::install(
            &mut runtime,
            &[".card { --accent: #ff0000; color: var(--accent); }"],
            Device::screen(800.0, 600.0),
        );
        let initial_generation = cssom.generation(0).expect("author sheet");

        runtime
            .eval(
                "var card = document.getElementById('card');\
                 var sheet = document.styleSheets[0];\
                 console.log(document.styleSheets.length + '|' + sheet.cssRules.length + '|' +\
                   getComputedStyle(card).color + '|' +\
                   getComputedStyle(card).getPropertyValue('--accent'));\
                 console.log(sheet.insertRule('.card { --accent: #0000ff; }', 1));\
                 console.log(sheet.cssRules.length + '|' + getComputedStyle(card).color + '|' +\
                   getComputedStyle(card).getPropertyValue('--accent'));\
                 try { sheet.insertRule('.bad {}', 9); } catch (e) { console.log(e.name); }\
                 try { sheet.insertRule('not a rule', 2); } catch (e) { console.log(e.name); }\
                 console.log(sheet.cssRules.length + '|' + getComputedStyle(card).color);\
                 sheet.deleteRule(1);\
                 console.log(sheet.cssRules.length + '|' + getComputedStyle(card).color);",
            )
            .expect("Livery CSSOM script");

        assert_eq!(
            runtime.host().borrow().console,
            vec![
                "1|1|rgb(255, 0, 0)|#ff0000",
                "1",
                "2|rgb(0, 0, 255)|#0000ff",
                "IndexSizeError",
                "SyntaxError",
                "2|rgb(0, 0, 255)",
                "1|rgb(255, 0, 0)",
            ],
        );
        assert_eq!(cssom.generation(0), Some(initial_generation + 2));
    }

    #[test]
    fn boa_canonicalizes_nested_calc_through_livery_inline_cssom() {
        let mut runtime = Runtime::<BoaEngine>::new().expect("runtime");
        runtime.load_dom(&StaticDocument::parse(
            "<html><body><div id='target'></div></body></html>",
        ));
        LiveryCssom::install(&mut runtime, &[], Device::screen(800.0, 600.0));

        runtime
            .eval(
                "var s = document.getElementById('target').style;\
                 var values = [\
                   'calc(20px + calc(80px))',\
                   'calc(calc(100px))',\
                   'calc(calc(2) * calc(50px))',\
                   'calc(calc(150px*2/3))',\
                   'calc(calc(2 * calc(calc(3)) + 4) * 10px)',\
                   'calc(50px + calc(40%))'\
                 ];\
                 for (var i = 0; i < values.length; i++) {\
                   s.left = values[i]; console.log(s.left);\
                 }\
                 s.border = 'calc(calc(10px)) solid pink';\
                 console.log(s.border);",
            )
            .expect("Livery inline CSSOM script");

        assert_eq!(
            runtime.host().borrow().console,
            vec![
                "calc(100px)",
                "calc(100px)",
                "calc(100px)",
                "calc(100px)",
                "calc(100px)",
                "calc(40% + 50px)",
                "calc(10px) solid pink",
            ]
        );
    }

    #[test]
    fn boa_resolves_nested_calc_widths_through_livery_layout() {
        let mut runtime = Runtime::<BoaEngine>::new().expect("runtime");
        runtime.load_dom(&StaticDocument::parse(
            "<html><body><div id='parent'>\
             <div id='div1'></div><div id='div2'></div>\
             <div id='div3'></div><div id='div4'></div>\
             </div></body></html>",
        ));
        LiveryCssom::install(
            &mut runtime,
            &["#parent { width: 200px; }\
                 #div1 { width: calc(calc(50px)); }\
                 #div2 { width: calc(calc(60%) - 20px); }\
                 #div3 { width: calc(calc(3 * 25%)); }\
                 #div4 { --width: calc(10% + 30px); width: calc(2 * var(--width)); }"],
            Device::screen(800.0, 600.0),
        );

        runtime
            .eval(
                "console.log(getComputedStyle(div1).width);\
                 console.log(getComputedStyle(div2).width);\
                 console.log(getComputedStyle(div3).width);\
                 console.log(getComputedStyle(div4).width);",
            )
            .expect("Livery used width script");

        assert_eq!(
            runtime.host().borrow().console,
            vec!["50px", "100px", "150px", "100px"]
        );
    }

    #[test]
    fn boa_reads_advanced_math_through_livery_used_values() {
        let mut runtime = Runtime::<BoaEngine>::new().expect("runtime");
        runtime.load_dom(&StaticDocument::parse(
            "<html><body><div id='parent'><div id='target'></div></div></body></html>",
        ));
        LiveryCssom::install(
            &mut runtime,
            &["#parent { width: 75px; }"],
            Device::screen(800.0, 600.0),
        );

        runtime
            .eval(
                "target.style = '';\
                 target.style.marginLeft = 'round(10%, 5px)';\
                 console.log(getComputedStyle(target).marginLeft);\
                 target.style.marginLeft = 'mod(-18px, 100% / 10)';\
                 console.log(getComputedStyle(target).marginLeft);\
                 target.style.marginLeft = 'calc(10px * exp(log(2)))';\
                 console.log(getComputedStyle(target).marginLeft);\
                 target.style.scale = 'sin(30deg)';\
                 console.log(getComputedStyle(target).scale);\
                 target.style.rotate = 'atan2(1px, -1px)';\
                 console.log(getComputedStyle(target).rotate);",
            )
            .expect("advanced CSS math script");

        assert_eq!(
            runtime.host().borrow().console,
            vec!["10px", "4.5px", "20px", "0.5", "2.3561945rad"]
        );
    }

    #[test]
    fn boa_restyles_viewport_units_when_the_livery_device_changes() {
        let mut runtime = Runtime::<BoaEngine>::new().expect("runtime");
        runtime.load_dom(&StaticDocument::parse(
            "<html><body><div id='target'></div></body></html>",
        ));
        let cssom = LiveryCssom::install(
            &mut runtime,
            &["#target { width: 10vw; }"],
            Device::screen(800.0, 600.0),
        );

        runtime
            .eval("console.log(getComputedStyle(target).width);")
            .expect("initial viewport width");
        cssom.set_viewport_size(400.0, 300.0);
        runtime
            .eval("console.log(getComputedStyle(target).width);")
            .expect("resized viewport width");

        assert_eq!(runtime.host().borrow().console, vec!["80px", "40px"]);
        assert!(cssom.last_restyle_stats().device_invalidated);
    }

    #[test]
    fn boa_scopes_iframe_documents_to_the_laid_out_frame_viewport() {
        let mut runtime = Runtime::<BoaEngine>::new().expect("runtime");
        runtime.load_dom(&StaticDocument::parse(
            "<html><body><iframe id='frame'></iframe></body></html>",
        ));
        LiveryCssom::install(
            &mut runtime,
            &["iframe { display: inline-block; width: 200px; height: 100px; }"],
            Device::screen(800.0, 600.0),
        );

        runtime
            .eval(
                "var doc = frame.contentDocument;\
                 doc.body.innerHTML = '<style>* { margin: 0; } body { height: 100%; } div { height: calc(1dvw + 1dvh); }</style><div></div>';\
                 console.log(doc.body.innerHTML);\
                 console.log(getComputedStyle(frame).width + 'x' + getComputedStyle(frame).height);\
                 console.log(frame.contentWindow.getComputedStyle(doc.querySelector('div')).height);",
            )
            .expect("iframe style script");

        assert_eq!(
            runtime.host().borrow().console,
            vec![
                "<style>* { margin: 0; } body { height: 100%; } div { height: calc(1dvw + 1dvh); }</style><div></div>",
                "200pxx100px",
                "3px",
            ]
        );
    }

    #[test]
    fn boa_mutates_named_container_queries_through_cssom() {
        let mut runtime = Runtime::<BoaEngine>::new().expect("runtime");
        runtime.load_dom(&StaticDocument::parse(
            "<html><body>\
             <div class='panel wide'><div id='wide' class='card'></div></div>\
             <div class='panel narrow'><div id='narrow' class='card'></div></div>\
             </body></html>",
        ));
        LiveryCssom::install(
            &mut runtime,
            &[
                ".panel { container-type: size; container-name: sidebar; height: 100px; }\
                 .wide { width: 320px; } .narrow { width: 200px; }\
                 .card { color: red; }",
            ],
            Device::screen(800.0, 600.0),
        );

        runtime
            .eval(
                "var sheet = document.styleSheets[0];\
                 console.log(getComputedStyle(wide).color + '|' + getComputedStyle(narrow).color);\
                 console.log(sheet.insertRule(\
                   '@container sidebar (width >= 300px) { .card { color: green; } }',\
                   sheet.cssRules.length));\
                 console.log(getComputedStyle(wide).color + '|' + getComputedStyle(narrow).color);\
                 sheet.deleteRule(sheet.cssRules.length - 1);\
                 console.log(getComputedStyle(wide).color + '|' + getComputedStyle(narrow).color);",
            )
            .expect("container query CSSOM script");

        assert_eq!(
            runtime.host().borrow().console,
            vec![
                "rgb(255, 0, 0)|rgb(255, 0, 0)",
                "4",
                "rgb(0, 128, 0)|rgb(255, 0, 0)",
                "rgb(255, 0, 0)|rgb(255, 0, 0)",
            ]
        );
    }

    #[test]
    fn boa_resolves_tiered_viewports_comparison_math_and_container_axes() {
        let mut runtime = Runtime::<BoaEngine>::new().expect("runtime");
        runtime.load_dom(&StaticDocument::parse(
            "<html><body>\
             <div class='inline outer'><div class='size outer'>\
             <div class='inline inner'><div id='target'></div></div>\
             </div></div></body></html>",
        ));
        let mut device = Device::screen(450.0, 250.0);
        device.set_viewport_sizes(ViewportSizes {
            small: ViewportSize::new(300.0, 200.0),
            large: ViewportSize::new(600.0, 400.0),
            dynamic: ViewportSize::new(450.0, 250.0),
        });
        let cssom = LiveryCssom::install(
            &mut runtime,
            &[".inline { container-type: inline-size; }\
               .size { container-type: size; }\
               .inline.outer { width: 500px; }\
               .size.outer { height: 400px; }\
               .inline.inner { width: 300px; }\
               #target { width: max(10cqi, 5cqb); height: min(10cqi, 10cqb);\
                         margin-left: 10svw; margin-right: 10lvw;\
                         top: 10dvh; }"],
            device,
        );

        runtime
            .eval(
                "console.log(getComputedStyle(target).width);\
                 console.log(getComputedStyle(target).height);\
                 console.log(getComputedStyle(target).marginLeft);\
                 console.log(getComputedStyle(target).marginRight);\
                 console.log(getComputedStyle(target).top);",
            )
            .expect("tiered and container unit reads");
        assert_eq!(
            runtime.host().borrow().console,
            vec!["30px", "30px", "30px", "60px", "25px"]
        );

        cssom.set_viewport_sizes(ViewportSizes {
            small: ViewportSize::new(200.0, 100.0),
            large: ViewportSize::new(800.0, 500.0),
            dynamic: ViewportSize::new(500.0, 300.0),
        });
        runtime
            .eval("console.log(getComputedStyle(target).marginLeft);")
            .expect("updated small viewport tier");
        assert!(cssom.last_restyle_stats().device_invalidated);
        runtime
            .eval(
                "console.log(getComputedStyle(target).marginRight);\
                 console.log(getComputedStyle(target).top);",
            )
            .expect("updated large and dynamic viewport tiers");
        assert_eq!(
            runtime.host().borrow().console,
            vec![
                "30px", "30px", "30px", "60px", "25px", "20px", "80px", "30px"
            ]
        );
    }

    #[test]
    fn scripted_attribute_change_restyles_only_its_livery_subtree() {
        let mut runtime = Runtime::<BoaEngine>::new().expect("runtime");
        runtime.load_dom(&StaticDocument::parse(
            "<html><body>\
             <main id='branch' class='branch'><span id='leaf' class='leaf'></span></main>\
             <aside><span class='unrelated'></span></aside>\
             </body></html>",
        ));
        let cssom = LiveryCssom::install(
            &mut runtime,
            &[".branch.on .leaf { color: #0000ff; }"],
            Device::screen(800.0, 600.0),
        );

        runtime
            .eval(
                "var leaf = document.getElementById('leaf');\
                 console.log(getComputedStyle(leaf).color);\
                 document.getElementById('branch').className = 'branch on';\
                 console.log(getComputedStyle(leaf).color);",
            )
            .expect("scoped Livery restyle");

        assert_eq!(
            runtime.host().borrow().console,
            vec!["rgb(0, 0, 0)", "rgb(0, 0, 255)"]
        );
        let stats = cssom.last_restyle_stats();
        assert_eq!(stats.snapshots, 1);
        assert_eq!(stats.hints, 1);
        assert_eq!(stats.restyled_elements, 2);
        assert!(stats.restyled_elements < stats.total_elements);
        assert!(!stats.full_document);
    }

    #[test]
    fn scripted_style_read_recovers_when_layout_drained_an_unseen_batch() {
        let mut runtime = Runtime::<BoaEngine>::new().expect("runtime");
        runtime.load_dom(&StaticDocument::parse(
            "<html><body><main id='branch'><span id='leaf'></span></main></body></html>",
        ));
        let cssom = LiveryCssom::install(
            &mut runtime,
            &[".on span { color: #0000ff; }"],
            Device::screen(800.0, 600.0),
        );

        runtime
            .eval("console.log(getComputedStyle(document.getElementById('leaf')).color);")
            .expect("initial style read");
        runtime
            .eval("document.getElementById('branch').className = 'on';")
            .expect("mutation");
        let mut drained = Vec::new();
        runtime
            .host()
            .borrow_mut()
            .dom
            .drain_mutations(&mut drained);
        assert!(!drained.is_empty());
        runtime
            .eval("console.log(getComputedStyle(document.getElementById('leaf')).color);")
            .expect("style read after layout drain");

        assert_eq!(
            runtime.host().borrow().console,
            vec!["rgb(0, 0, 0)", "rgb(0, 0, 255)"]
        );
        assert!(cssom.last_restyle_stats().full_document);
    }
}
