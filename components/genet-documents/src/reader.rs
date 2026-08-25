/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! The shared `genet.reader` lane: held HTML bytes -> fleece article ->
//! portable `EngineDocument` -> the existing document canvas.

use std::any::Any;

use fleece::{Article, ExtractionLineage, Inline, RootSelector};
use inker::{
    Block, ContentLineage, ContentReport, DocumentProvenance, DocumentSession, DocumentTrustState,
    EngineDocument, InlineSpan, SessionClick, SessionEngine, SessionError, SessionLink,
    SessionScrollKey, SessionSpawnRequest,
};
use netrender::Scene;

use crate::{SmolwebDocument, SmolwebTheme, resolve_href};

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
        }))
    }
}

/// Retained reader rendering plus the fleece derivation that made it.
pub struct ReaderDocumentSession {
    doc: SmolwebDocument,
    viewport: (u32, u32),
    lineage: ExtractionLineage,
}

impl ReaderDocumentSession {
    pub fn document(&self) -> &EngineDocument {
        self.doc.document()
    }

    pub fn lineage(&self) -> &ExtractionLineage {
        &self.lineage
    }
}

impl DocumentSession<Scene> for ReaderDocumentSession {
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
    use inker::{SessionRegistry, SessionSpawnRequest};

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
