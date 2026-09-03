/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! The shared `genet.reader` lane: held HTML bytes -> fleece article ->
//! portable `EngineDocument` -> the existing document canvas.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;

use fleece::{Article, ExtractionLineage, Inline, RootSelector};
use inker::{
    Block, ContentLineage, ContentReport, DocumentA11yAction, DocumentA11yBounds, DocumentA11yNode,
    DocumentA11yNodeId, DocumentA11yProjection, DocumentA11yRole, DocumentA11yState,
    DocumentA11ySupport, DocumentCapabilities, DocumentCapabilityStatus, DocumentProvenance,
    DocumentSession, DocumentTrustState, EngineDocument, InlineSpan, SessionClick, SessionEngine,
    SessionError, SessionLink, SessionScrollKey, SessionSpawnRequest,
};
use netrender::Scene;

use genet_host_api::resolve_href;

use crate::smolweb::{SmolwebDocument, SmolwebTheme};

/// Renderer-neutral Reader semantics recovered from the current retained
/// document-canvas packet. It is absent until Reader has presented one frame.
#[derive(Clone, Debug, PartialEq)]
pub struct ReaderAccessibilitySnapshot {
    /// Reader's root name, if Fleece lowered a document title.
    pub root_title: Option<String>,
    /// Logical, currently-visible links. A wrapped link appears once with all
    /// of its clipped viewport-space rectangles.
    pub links: Vec<ReaderAccessibilityLink>,
}

/// One logical Reader link in a [`ReaderAccessibilitySnapshot`].
#[derive(Clone, Debug, PartialEq)]
pub struct ReaderAccessibilityLink {
    /// Opaque identity retained by document-canvas across reflow.
    pub identity: document_canvas::SemanticInteractionId,
    /// Author-lowered link text. Decorative visual arrows are excluded.
    pub label: String,
    pub url: String,
    /// Visible `[x, y, width, height]` rectangles in Reader viewport space.
    pub rects: Vec<[f32; 4]>,
}

fn reader_link_bounds(rects: &[[f32; 4]]) -> Option<DocumentA11yBounds> {
    let mut bounds: Option<(f32, f32, f32, f32)> = None;
    for &[x, y, width, height] in rects {
        if !x.is_finite()
            || !y.is_finite()
            || !width.is_finite()
            || !height.is_finite()
            || width <= 0.0
            || height <= 0.0
        {
            continue;
        }
        let right = x + width;
        let bottom = y + height;
        if !right.is_finite() || !bottom.is_finite() {
            continue;
        }
        bounds = Some(match bounds {
            Some((left, top, old_right, old_bottom)) => (
                left.min(x),
                top.min(y),
                old_right.max(right),
                old_bottom.max(bottom),
            ),
            None => (x, y, right, bottom),
        });
    }
    bounds.map(|(x, y, right, bottom)| DocumentA11yBounds {
        x,
        y,
        width: right - x,
        height: bottom - y,
    })
}

/// The only three outcomes of the cheap static reader pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticArticleOutcome {
    Article(Box<Article>),
    NeedsScriptedDom,
    NotReadable,
}

/// Select the scripted fallback only when static fleece extraction found no
/// article and the source actually carries script.
pub fn extract_static_article(source: &str) -> StaticArticleOutcome {
    let dom = genet_static_dom::StaticDocument::parse(source);
    if let Some(article) = fleece::extract_article(&dom) {
        StaticArticleOutcome::Article(Box::new(article))
    } else if fleece::carries_script(&dom) {
        StaticArticleOutcome::NeedsScriptedDom
    } else {
        StaticArticleOutcome::NotReadable
    }
}

fn resolve_reader_href(base: &str, href: &str) -> String {
    url::Url::parse(base)
        .ok()
        .and_then(|base| base.join(href).ok())
        .map(|url| url.to_string())
        .unwrap_or_else(|| resolve_href(base, href))
}

/// Lower a render-free article into the document engine's portable block model.
pub fn lower_article(address: &str, article: &Article) -> EngineDocument {
    let mut blocks = Vec::new();
    for block in &article.blocks {
        lower_block(address, block, &mut blocks);
    }
    EngineDocument {
        address: address.to_string(),
        title: article.title.clone(),
        content_type: "text/html; profile=reader".to_string(),
        lang: article.lang.clone(),
        provenance: DocumentProvenance {
            source_kind: Some(inker::routing::ENGINE_GENET_READER.to_string()),
            canonical_uri: article
                .canonical
                .as_deref()
                .map(|canonical| resolve_reader_href(address, canonical))
                .or_else(|| Some(address.to_string())),
            fetched_at: None,
            source_label: article.site.clone(),
        },
        trust: DocumentTrustState::Unknown,
        diagnostics: Vec::new(),
        blocks,
    }
}

/// Reader rendering is intentionally anchor-blind: Fleece selector evidence remains
/// available to annotation consumers, while this boundary preserves its portable
/// document content.
fn lower_block(address: &str, block: &fleece::AnchoredBlock, out: &mut Vec<Block>) {
    match &block.block {
        fleece::Block::Heading { level, runs } => out.push(Block::Heading {
            level: *level,
            spans: lower_inline(address, runs),
        }),
        fleece::Block::Paragraph { runs } => out.push(Block::Paragraph {
            spans: lower_inline(address, runs),
        }),
        fleece::Block::List { ordered, items } => out.push(Block::List {
            ordered: *ordered,
            items: items
                .iter()
                .map(|item| {
                    let mut blocks = Vec::new();
                    for block in item {
                        lower_block(address, block, &mut blocks);
                    }
                    blocks
                })
                .collect(),
        }),
        fleece::Block::Quote { blocks } => {
            let mut lowered = Vec::new();
            for block in blocks {
                lower_block(address, block, &mut lowered);
            }
            out.push(Block::Quote { blocks: lowered });
        },
        fleece::Block::Code { language, text } => out.push(Block::CodeBlock {
            language: language.clone(),
            text: text.clone(),
        }),
        fleece::Block::Table { table } => {
            let rows = &table.rows;
            let header_index = rows.iter().position(|row| row.header);
            let header = header_index
                .map(|index| {
                    rows[index]
                        .cells
                        .iter()
                        .map(|cell| lower_inline(address, &cell.runs))
                        .collect()
                })
                .unwrap_or_default();
            let rows = rows
                .iter()
                .enumerate()
                .filter(|(index, _)| Some(*index) != header_index)
                .map(|(_, row)| {
                    row.cells
                        .iter()
                        .map(|cell| lower_inline(address, &cell.runs))
                        .collect()
                })
                .collect();
            out.push(Block::Table {
                alignments: Vec::new(),
                header,
                rows,
            });
        },
        fleece::Block::Figure { src, alt, caption } => {
            out.push(Block::Image {
                url: resolve_reader_href(address, src),
                alt: alt.clone(),
            });
            if let Some(caption) = caption {
                out.push(Block::Paragraph {
                    spans: lower_inline(address, caption),
                });
            }
        },
        fleece::Block::Rule => out.push(Block::Rule),
    }
}

fn lower_inline(address: &str, runs: &[Inline]) -> Vec<InlineSpan> {
    runs.iter()
        .map(|run| match run {
            Inline::Text(text) => InlineSpan::Text(text.clone()),
            Inline::Code(code) => InlineSpan::Code(code.clone()),
            Inline::Link { href, runs } => InlineSpan::Link {
                url: resolve_reader_href(address, href),
                title: None,
                spans: lower_inline(address, runs),
                predicate: None,
            },
            Inline::Emphasis { strong, runs } => {
                let runs = lower_inline(address, runs);
                if *strong {
                    InlineSpan::Strong(runs)
                } else {
                    InlineSpan::Emphasis(runs)
                }
            },
        })
        .collect()
}

/// Engine registration for the shared reader lane. A spawn must carry held
/// source bytes; reader mode never refetches.
pub struct ReaderSessionEngine {
    theme: SmolwebTheme,
}

impl ReaderSessionEngine {
    pub fn new(theme: SmolwebTheme) -> Self {
        Self { theme }
    }
}

impl Default for ReaderSessionEngine {
    fn default() -> Self {
        Self::new(SmolwebTheme::default())
    }
}

impl SessionEngine<Scene> for ReaderSessionEngine {
    fn engine_id(&self) -> &str {
        inker::routing::ENGINE_GENET_READER
    }

    fn spawn(
        &self,
        request: &SessionSpawnRequest,
    ) -> Result<Box<dyn DocumentSession<Scene>>, SessionError> {
        let source = request.body.as_deref().ok_or_else(|| {
            SessionError::Unsupported(
                "genet.reader requires the source bytes already held by the host".to_string(),
            )
        })?;
        let article = match extract_static_article(source) {
            StaticArticleOutcome::Article(article) => article,
            StaticArticleOutcome::NeedsScriptedDom => {
                return Err(SessionError::Unsupported(
                    "static fleece extraction found an empty scripted shell; retry over the post-JS DOM"
                        .to_string(),
                ));
            },
            StaticArticleOutcome::NotReadable => {
                return Err(SessionError::SpawnFailed(
                    "fleece found no single readable article".to_string(),
                ));
            },
        };
        let lineage = article.lineage.clone();
        let document = lower_article(&request.address, &article);
        let doc = SmolwebDocument::from_document_with_theme(document, self.theme.clone());
        Ok(Box::new(ReaderDocumentSession {
            doc,
            viewport: request.viewport,
            lineage,
            accessibility_revision: 1,
            accessibility_nodes: RefCell::new(ReaderAccessibilityNodeMap::default()),
        }))
    }
}

/// Retained reader rendering plus the fleece derivation that made it.
pub struct ReaderDocumentSession {
    doc: SmolwebDocument,
    viewport: (u32, u32),
    lineage: ExtractionLineage,
    /// Every presentation-affecting turn revokes prior geometry and actions.
    /// The public projection carries this revision so Pelt makes queued
    /// virtual Focus inert rather than retargeting it after a reflow.
    accessibility_revision: u64,
    /// Document-canvas keeps link identities opaque. Retain an engine-local
    /// projection ID for each one instead of deriving identity from output
    /// order, URL, or visible rectangles.
    accessibility_nodes: RefCell<ReaderAccessibilityNodeMap>,
}

#[derive(Default)]
struct ReaderAccessibilityNodeMap {
    ids: HashMap<document_canvas::SemanticInteractionId, DocumentA11yNodeId>,
    next: u64,
}

impl ReaderDocumentSession {
    pub fn document(&self) -> &EngineDocument {
        self.doc.document()
    }

    pub fn lineage(&self) -> &ExtractionLineage {
        &self.lineage
    }

    /// Return the current renderer-neutral Reader semantic snapshot without
    /// forcing layout. It is absent before Reader has completed its first
    /// frame, then reflects the retained layout and current scroll position.
    pub fn accessibility_snapshot(&self) -> Option<ReaderAccessibilitySnapshot> {
        let links = self.doc.retained_accessible_links()?;
        Some(ReaderAccessibilitySnapshot {
            root_title: self.doc.document().title.clone(),
            links: links
                .into_iter()
                .map(|link| ReaderAccessibilityLink {
                    identity: link.identity,
                    label: link.label,
                    url: link.url,
                    rects: link.rects,
                })
                .collect(),
        })
    }

    fn accessibility_node_id(
        &self,
        identity: document_canvas::SemanticInteractionId,
    ) -> DocumentA11yNodeId {
        let mut nodes = self.accessibility_nodes.borrow_mut();
        if let Some(id) = nodes.ids.get(&identity) {
            return *id;
        }
        // Reserve 1 for the document root. Never recycle a local identity:
        // a queued virtual Focus may then only become stale, never point at a
        // later visible link after reflow.
        let next = nodes.next.max(2);
        nodes.next = next
            .checked_add(1)
            .expect("Reader accessibility node IDs exhausted");
        let id = DocumentA11yNodeId::new(next);
        nodes.ids.insert(identity, id);
        id
    }

    fn bump_accessibility_revision(&mut self) {
        self.accessibility_revision = self
            .accessibility_revision
            .checked_add(1)
            .expect("Reader accessibility revision exhausted");
    }

    /// Project the completed document-canvas presentation into Inker's
    /// renderer-neutral semantic contract. Reader currently exposes visible
    /// links only; Focus is intentionally virtual and activation remains
    /// unavailable until a host-held destination-body handoff exists.
    fn document_accessibility_projection(&self) -> Option<DocumentA11yProjection> {
        let snapshot = self.accessibility_snapshot()?;
        let root = DocumentA11yNodeId::new(1);
        let title = snapshot
            .root_title
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_else(|| "Reader document".to_owned());
        let mut links = Vec::new();
        for link in snapshot.links {
            let Some(bounds) = reader_link_bounds(&link.rects) else {
                continue;
            };
            links.push(DocumentA11yNode {
                id: self.accessibility_node_id(link.identity),
                parent: Some(root),
                children: Vec::new(),
                role: DocumentA11yRole::Link,
                name: Some(link.label),
                value: None,
                numeric_value: None,
                numeric_minimum: None,
                numeric_maximum: None,
                bounds: Some(bounds),
                state: DocumentA11yState::default(),
                actions: vec![DocumentA11yAction::Focus],
            });
        }
        let mut nodes = vec![DocumentA11yNode {
            id: root,
            parent: None,
            children: links.iter().map(|link| link.id).collect(),
            role: DocumentA11yRole::Document,
            name: Some(title),
            value: None,
            numeric_value: None,
            numeric_minimum: None,
            numeric_maximum: None,
            bounds: None,
            state: DocumentA11yState::default(),
            actions: Vec::new(),
        }];
        nodes.extend(links);
        let support = DocumentA11ySupport::new(
            inker::A11yCapability::Partial,
            ["Visible Reader links are composed with virtual Focus; activation remains unavailable until the host holds the destination body."],
        )
        .expect("Reader's partial accessibility limitation is non-empty");
        Some(DocumentA11yProjection::new(
            self.accessibility_revision,
            support,
            root,
            nodes,
        ))
    }

    /// Re-resolve a current visible point for a Reader semantic link token.
    /// The query reads retained geometry only, validates against current scroll,
    /// and returns no point after that link has left the content hole.
    pub fn accessibility_pointer_target(
        &self,
        identity: document_canvas::SemanticInteractionId,
    ) -> Option<(f32, f32)> {
        self.doc.retained_accessible_pointer_target(identity)
    }
}

impl DocumentSession<Scene> for ReaderDocumentSession {
    fn document_capabilities(&self) -> DocumentCapabilities {
        DocumentCapabilities {
            find_in_page: DocumentCapabilityStatus::unsupported(
                "reader sessions do not expose document find",
            ),
            page_zoom: DocumentCapabilityStatus::unsupported(
                "reader sessions do not expose page zoom",
            ),
            page_capture: DocumentCapabilityStatus::unsupported(
                "reader sessions do not capture rendered pages",
            ),
            navigation: DocumentCapabilityStatus::Partial {
                detail: "the host owns document lineage, policy, and refetch".into(),
            },
        }
    }

    fn frame(&mut self, width: u32, height: u32) -> Scene {
        let before = self.accessibility_snapshot();
        self.viewport = (width, height);
        let scene = self.doc.frame(width, height);
        if self.accessibility_snapshot() != before {
            self.bump_accessibility_revision();
        }
        scene
    }

    fn scroll_by(&mut self, dx: f32, dy: f32) -> bool {
        let changed = self.doc.scroll_by(dx, dy);
        if changed {
            self.bump_accessibility_revision();
        }
        changed
    }

    fn scroll_at(&mut self, x: f32, y: f32, dx: f32, dy: f32) -> bool {
        let changed = self.doc.scroll_at(x, y, dx, dy);
        if changed {
            self.bump_accessibility_revision();
        }
        changed
    }

    fn scroll_for_key(&mut self, key: SessionScrollKey) -> bool {
        let changed = self.doc.scroll_for_key(key);
        if changed {
            self.bump_accessibility_revision();
        }
        changed
    }

    fn scroll_to(&mut self, y: f32) {
        let before = self.accessibility_snapshot();
        self.doc.scroll_to(y);
        if self.accessibility_snapshot() != before {
            self.bump_accessibility_revision();
        }
    }

    fn click_at(&mut self, x: f32, y: f32) -> SessionClick {
        let (width, height) = self.viewport;
        match self.doc.click_at(x, y, width, height) {
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

    fn inspect(&self) -> Option<ContentReport> {
        let document = self.doc.document();
        let mut headings = Vec::new();
        collect_headings(&document.blocks, &mut headings);
        Some(ContentReport {
            title: document.title.clone(),
            links: document
                .outgoing_links()
                .into_iter()
                .map(str::to_string)
                .collect(),
            headings,
            lineage: Some(content_lineage(&self.lineage)),
            ..Default::default()
        })
    }

    fn accessibility_projection(&self) -> Option<DocumentA11yProjection> {
        self.document_accessibility_projection()
    }

    fn as_any_ref(&self) -> &dyn Any {
        self
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

fn collect_headings(blocks: &[Block], out: &mut Vec<String>) {
    for block in blocks {
        match block {
            Block::Heading { spans, .. } => out.push(inker::inline_text(spans)),
            Block::Quote { blocks } => collect_headings(blocks, out),
            Block::List { items, .. } => {
                for item in items {
                    collect_headings(item, out);
                }
            },
            _ => {},
        }
    }
}

fn content_lineage(lineage: &ExtractionLineage) -> ContentLineage {
    let (selector, score) = match &lineage.root_selector {
        RootSelector::Main => ("main".to_string(), None),
        RootSelector::ScoredCandidate { tag, score } => (format!("scored {tag}"), Some(*score)),
    };
    ContentLineage {
        tool: "fleece".to_string(),
        version: lineage.fleece_version.clone(),
        selector,
        score,
        block_count: lineage.block_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inker::{DocumentA11yAction, DocumentA11yRole, SessionRegistry, SessionSpawnRequest};

    fn reader_session(html: &str, width: u32, height: u32) -> Box<dyn DocumentSession<Scene>> {
        ReaderSessionEngine::new(SmolwebTheme::Plain)
            .spawn(
                &SessionSpawnRequest::new("https://example.test/story/index.html")
                    .with_body(html)
                    .with_viewport(width, height),
            )
            .expect("reader session")
    }

    fn reader(session: &dyn DocumentSession<Scene>) -> &ReaderDocumentSession {
        session
            .as_any_ref()
            .downcast_ref::<ReaderDocumentSession>()
            .expect("reader session type")
    }

    #[test]
    fn reader_route_lowers_renders_links_and_reports_lineage() {
        let mut registry = SessionRegistry::<Scene>::new();
        registry.register(Box::new(ReaderSessionEngine::new(SmolwebTheme::Plain)));
        let request = SessionSpawnRequest::new("https://example.test/story/index.html")
            .with_body(
                "<html lang='en'><head><title>Story</title></head><body><nav>chrome</nav>\
                 <main><h1>Readable story</h1><p>This substantial paragraph keeps a \
                 <a href='../source'>source link</a> in the shared reader lane.</p></main></body></html>",
            )
            .with_viewport(640, 480);
        let mut session = registry
            .spawn(inker::routing::ENGINE_GENET_READER, &request)
            .expect("reader session");
        let scene = session.frame(640, 480);
        assert!(
            scene
                .ops
                .iter()
                .any(|op| matches!(op, netrender::SceneOp::GlyphRun(_)))
        );
        let report = session.inspect().expect("reader report");
        assert_eq!(report.title.as_deref(), Some("Readable story"));
        assert_eq!(report.links, ["https://example.test/source"]);
        let lineage = report.lineage.expect("fleece lineage");
        assert_eq!(lineage.tool, "fleece");
        assert_eq!(lineage.selector, "main");
        assert_eq!(lineage.block_count, 2);
    }

    #[test]
    fn reader_requires_held_bytes_and_declines_non_articles() {
        let engine = ReaderSessionEngine::default();
        assert!(matches!(
            engine.spawn(&SessionSpawnRequest::new("https://example.test/")),
            Err(SessionError::Unsupported(_))
        ));
        assert!(matches!(
            engine.spawn(
                &SessionSpawnRequest::new("https://example.test/")
                    .with_body("<nav><a href='/one'>one</a></nav>")
            ),
            Err(SessionError::SpawnFailed(_))
        ));
        assert!(matches!(
            engine.spawn(
                &SessionSpawnRequest::new("https://example.test/")
                    .with_body("<main id='app'></main><script src='/app.js'></script>")
            ),
            Err(SessionError::Unsupported(message)) if message.contains("post-JS DOM")
        ));
    }

    #[test]
    fn reader_snapshot_waits_for_frame_and_groups_wrapped_links_across_reflow() {
        let label = "one two three four five six seven eight nine ten";
        let html = format!(
            "<html><head><title>Snapshot article</title></head><body><main>\
             <h1>Snapshot article</h1><p>This substantial reader paragraph introduces a \
             <a href='/go'>{label}</a> and keeps enough prose for Fleece to retain the article.\
             </p></main></body></html>"
        );
        let mut session = reader_session(&html, 130, 300);
        assert!(
            reader(session.as_ref()).accessibility_snapshot().is_none(),
            "sizing and construction do not publish a semantic snapshot"
        );

        let _ = session.frame(130, 300);
        let narrow = reader(session.as_ref())
            .accessibility_snapshot()
            .expect("completed frame publishes snapshot");
        assert_eq!(narrow.root_title.as_deref(), Some("Snapshot article"));
        let narrow_link = narrow
            .links
            .iter()
            .find(|link| link.url == "https://example.test/go")
            .expect("reader link");
        assert_eq!(narrow_link.label, label, "decorative arrow is excluded");
        assert!(
            narrow_link.rects.len() >= 2,
            "the narrow frame keeps one logical link across wrapped rectangles"
        );
        let identity = narrow_link.identity;

        let (x, y) = reader(session.as_ref())
            .accessibility_pointer_target(identity)
            .expect("visible semantic link resolves to a retained point");
        assert!(x >= 0.0 && x < 130.0 && y >= 0.0 && y < 300.0);

        // A sizing query can rebuild retained geometry but cannot publish it
        // as the next a11y snapshot before the corresponding frame finishes.
        let _ = session.content_height(640, 300);
        assert!(
            reader(session.as_ref()).accessibility_snapshot().is_none(),
            "a size-triggered layout rebuild revokes the prior presentation"
        );
        let _ = session.frame(640, 300);
        let wide = reader(session.as_ref())
            .accessibility_snapshot()
            .expect("reflowed completed frame publishes snapshot");
        let wide_link = wide
            .links
            .iter()
            .find(|link| link.url == "https://example.test/go")
            .expect("reader link survives reflow");
        assert_eq!(
            wide_link.identity, identity,
            "geometry does not define identity"
        );
    }

    #[test]
    fn reader_projection_is_partial_with_virtual_focus_only() {
        let html = "<html><head><title>Projection article</title></head><body><main>\
            <h1>Projection article</h1><p>A sufficiently long Reader paragraph has a \
            <a href='/next'>visible continuation</a> for the retained canvas.</p></main></body></html>";
        let mut session = reader_session(html, 480, 300);
        assert!(session.accessibility_projection().is_none());
        let _ = session.frame(480, 300);
        let first = session
            .accessibility_projection()
            .expect("completed Reader frame publishes a neutral projection");
        assert_eq!(first.support().capability(), inker::A11yCapability::Partial);
        assert!(
            first
                .support()
                .limitations()
                .iter()
                .any(|limitation| limitation.contains("activation remains unavailable"))
        );
        let root = first.node(first.root()).expect("Reader root");
        assert_eq!(root.role, DocumentA11yRole::Document);
        let link = first
            .nodes()
            .iter()
            .find(|node| node.role == DocumentA11yRole::Link)
            .expect("visible Reader link");
        assert_eq!(link.actions, [DocumentA11yAction::Focus]);
        assert!(link.bounds.is_some());
        let id = link.id;
        let revision = first.revision();
        let _ = session.frame(480, 300);
        let stable = session
            .accessibility_projection()
            .expect("same completed frame retains projection");
        assert_eq!(stable.revision(), revision);
        assert!(
            stable.node(id).is_some(),
            "link ID outlives a stable redraw"
        );
    }

    #[test]
    fn reader_pointer_target_rechecks_current_scroll_and_content_hole() {
        let body: String = (0..40)
            .map(|index| format!("<p>Reader body line {index} has enough text for extraction.</p>"))
            .collect();
        let html = format!(
            "<html><head><title>Scrolled snapshot</title></head><body><main>\
             <h1>Scrolled snapshot</h1>{body}\
             <p><a href='/tail'>tail link</a></p></main></body></html>"
        );
        let mut session = reader_session(&html, 400, 120);
        let _ = session.frame(400, 120);
        assert!(
            reader(session.as_ref())
                .accessibility_snapshot()
                .expect("completed frame")
                .links
                .is_empty(),
            "the tail is not initially in the content hole"
        );

        session.scroll_to(f32::MAX);
        let snapshot = reader(session.as_ref())
            .accessibility_snapshot()
            .expect("scroll reads retained geometry");
        let tail = snapshot
            .links
            .iter()
            .find(|link| link.url == "https://example.test/tail")
            .expect("tail link becomes visible after scroll");
        let identity = tail.identity;
        let point = reader(session.as_ref())
            .accessibility_pointer_target(identity)
            .expect("current visible tail has a pointer target");
        assert!(point.0 >= 0.0 && point.0 < 400.0 && point.1 >= 0.0 && point.1 < 120.0);

        session.scroll_to(0.0);
        assert!(
            reader(session.as_ref())
                .accessibility_pointer_target(identity)
                .is_none(),
            "the old snapshot token is not used once scroll hides its retained rect"
        );
    }

    #[test]
    fn article_lowering_preserves_table_header_and_figure_caption() {
        let dom = genet_static_dom::StaticDocument::parse(
            "<main><h1>Data report</h1><p>A substantial introduction makes this report readable.</p>\
             <table><tr><th>Name</th><th>Value</th></tr><tr><td>A</td><td>B</td></tr></table>\
             <figure><img src='/plot.png' alt='plot'><figcaption>Measured values</figcaption></figure></main>",
        );
        let article = fleece::extract_article(&dom).expect("article");
        let document = lower_article("https://example.test/report", &article);
        assert!(matches!(
            document.blocks.iter().find(|block| matches!(block, Block::Table { .. })),
            Some(Block::Table { header, rows, .. }) if header.len() == 2 && rows.len() == 1
        ));
        assert!(document.blocks.iter().any(|block| {
            matches!(block, Block::Image { url, .. } if url == "https://example.test/plot.png")
        }));
        assert!(document.blocks.iter().any(|block| {
            matches!(block, Block::Paragraph { spans } if inker::inline_text(spans) == "Measured values")
        }));
    }
}
