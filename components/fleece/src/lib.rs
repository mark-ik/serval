/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Fleece: render-free content extraction over [`LayoutDom`].
//!
//! "We don't just want to render the web, we want to analyze it too." This crate
//! turns a parsed document into the structured content a crawler or the eidetic
//! browsing corpus wants — links, title, headings, main text, metadata, reader
//! blocks, and page-carried structured data —
//! with **no cascade, layout, or paint**. Its single dependency is the
//! profile-neutral [`layout_dom_api`], so the dep graph itself is the witness that
//! extraction pulls none of the render stack (the render ladder's witness
//! discipline, applied to the orthogonal extraction axis).
//!
//! Extraction is **not a lower render rung**: it is a different *output* (data, not
//! pixels) that can draw from any rung's DOM. The cheap path runs over a no-JS
//! [`genet_static_dom::StaticDocument`] (static-parse extract); the same functions
//! run over a script-mutated DOM for the post-JS / SPA case (headless-scripted-DOM
//! extract), since both are just `LayoutDom`s.
//!
//! All output is **unresolved and rect-free**: an `href` is the raw attribute value
//! (the caller owns the page URL and resolves it), and there is no geometry — this
//! is the counterpart to the layout-coupled `LinkHit` (`href` + rect), for code that
//! wants the link graph without laying the page out.

#![deny(unsafe_code)]

use layout_dom_api::{LayoutDom, LocalName, Namespace};

/// One extracted hyperlink — the rect-free counterpart to a laid-out `LinkHit`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// The raw `href` attribute value, **unresolved**: extraction owns no URL
    /// context, so the caller resolves it against the page URL.
    pub href: String,
    /// The anchor's visible text: its descendants' text, whitespace-collapsed.
    pub text: String,
    /// The `rel` token list, if present (`nofollow`, `noopener`, …). A crawler
    /// honours `nofollow` when building its frontier; extraction just reports it.
    pub rel: Option<String>,
}

/// One extracted heading: its level (`1`–`6` for `<h1>`–`<h6>`) and collapsed text.
/// The document outline — structure for the corpus and a summarization signal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    /// The heading level, `1`–`6`.
    pub level: u8,
    /// The heading's visible text, whitespace-collapsed.
    pub text: String,
}

/// The document's self-description: the metadata a page declares about itself. All
/// values are **unresolved** (a `canonical` href is the raw attribute). `Default` is
/// "nothing declared".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Metadata {
    /// `<meta name="description">` content — the page's own summary.
    pub description: Option<String>,
    /// `<link rel="canonical" href>` — the canonical URL the page claims (raw).
    pub canonical: Option<String>,
    /// OpenGraph `<meta property="og:*">` pairs with the `og:` prefix stripped, in
    /// document order: `("title", …)`, `("description", …)`, `("image", …)`,
    /// `("site_name", …)`, `("type", …)`, `("url", …)`, and the long tail.
    pub open_graph: Vec<(String, String)>,
}

/// One JSON value harvested from page-carried structured data.
///
/// Fleece keeps this small value model locally so JSON-LD does not add a
/// parser dependency to the render-free extraction cone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StructuredValue {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<StructuredValue>),
    Object(Vec<(String, StructuredValue)>),
}

impl StructuredValue {
    /// Return an object member by name.
    pub fn get(&self, name: &str) -> Option<&StructuredValue> {
        let Self::Object(entries) = self else {
            return None;
        };
        entries
            .iter()
            .find_map(|(key, value)| (key == name).then_some(value))
    }

    /// Return this value as a string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }
}

/// A typed block harvested from JSON-LD or HTML microdata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuredData {
    /// The declared schema type (`Recipe`, `Event`, `Person`, `Article`, ...).
    pub kind: String,
    /// The harvested value, retaining fields fleece does not interpret.
    pub value: StructuredValue,
    /// The page syntax that supplied the value.
    pub source: StructuredDataSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuredDataSource {
    JsonLd,
    Microdata,
}

/// A rich inline run in a reader article. URLs remain raw attributes; callers
/// resolve them against the source document address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inline {
    Text(String),
    Link { href: String, runs: Vec<Inline> },
    Emphasis { strong: bool, runs: Vec<Inline> },
    Code(String),
}

/// One table cell. `header` records whether the source used `<th>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableCell {
    pub header: bool,
    pub runs: Vec<Inline>,
}

/// One table row. A row is a header row when every non-empty cell is a header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRow {
    pub header: bool,
    pub cells: Vec<TableCell>,
}

/// Structured reader blocks, independent of layout and paint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Heading {
        level: u8,
        runs: Vec<Inline>,
    },
    Paragraph {
        runs: Vec<Inline>,
    },
    List {
        ordered: bool,
        items: Vec<Vec<Block>>,
    },
    Quote {
        blocks: Vec<Block>,
    },
    Code {
        language: Option<String>,
        text: String,
    },
    Table {
        rows: Vec<TableRow>,
    },
    Figure {
        src: String,
        alt: String,
        caption: Option<Vec<Inline>>,
    },
    Rule,
}

/// The root-selection evidence attached to a reader rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootSelector {
    Main,
    ScoredCandidate { tag: String, score: i32 },
}

/// Reproducibility information for a reader extraction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractionLineage {
    pub fleece_version: String,
    pub root_selector: RootSelector,
    pub block_count: usize,
}

/// The structured reader shape. Fleece returns it and retains no state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Article {
    pub title: Option<String>,
    pub byline: Option<String>,
    pub published: Option<String>,
    pub lang: Option<String>,
    pub site: Option<String>,
    pub canonical: Option<String>,
    pub lead_image: Option<String>,
    pub blocks: Vec<Block>,
    pub lineage: ExtractionLineage,
}

/// The two extraction views produced from one selected live document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedDocument {
    pub page: PageExtract,
    pub article: Option<Article>,
}

/// A render-free extraction of a parsed document: the structured content a crawler
/// or the eidetic corpus wants, with no cascade / layout / paint. `Default` is
/// the empty extract.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageExtract {
    /// The document `<title>` text, whitespace-collapsed, if present and non-empty.
    pub title: Option<String>,
    /// The page's declared metadata (description / canonical / OpenGraph).
    pub metadata: Metadata,
    /// The `<h1>`–`<h6>` outline in document order.
    pub headings: Vec<Heading>,
    /// The page's full **visible text**, whitespace-collapsed (non-rendered
    /// subtrees — `<script>` / `<style>` / `<head>` / … — excluded). This is the
    /// indexing/corpus text (everything the reader could see, chrome included);
    /// for the article body alone, see [`main_text`](Self::main_text).
    pub text: String,
    /// The **reader-mode article body**: the main content block by a
    /// readability heuristic (semantic landmarks + paragraph density + class/id
    /// signals), with page chrome (nav / header / footer / aside) dropped. `None`
    /// when no contentful block stands out (a link list, an app shell). This is the
    /// per-page payload for a reader-mode crawl.
    pub main_text: Option<String>,
    /// Every `<a href>` in document order — the crawl frontier's source.
    pub links: Vec<Link>,
    /// JSON-LD and microdata blocks carried by the page.
    pub structured_data: Vec<StructuredData>,
}

/// Extract the structured content of `dom` without rendering it. The one-call entry
/// for the eidetic sink; the field functions below are the à-la-carte pieces.
pub fn extract<D: LayoutDom>(dom: &D) -> PageExtract {
    extract_document(dom).page
}

/// Extract the flat index shape and structured reader shape together.
pub fn extract_document<D: LayoutDom>(dom: &D) -> ExtractedDocument {
    let selected = select_content(dom);
    let main_text = selected.as_ref().and_then(|selected| {
        let text = chrome_free_text(dom, &selected.roots);
        (!text.is_empty()).then_some(text)
    });
    let page = PageExtract {
        title: extract_title(dom),
        metadata: extract_metadata(dom),
        headings: extract_headings(dom),
        text: extract_text(dom),
        main_text,
        links: extract_links(dom),
        structured_data: extract_structured_data(dom),
    };
    let article = selected.and_then(|selected| extract_article_with_page(dom, &page, selected));
    ExtractedDocument { page, article }
}

/// Extract only the structured reader shape.
pub fn extract_article<D: LayoutDom>(dom: &D) -> Option<Article> {
    extract_document(dom).article
}

/// Whether the document carries a script element.
///
/// Hosts use this only as a fallback signal: a static parse that already
/// yields an [`Article`] stays on the cheap path, while an empty scripted shell
/// may be retried over its post-JS DOM.
pub fn carries_script<D: LayoutDom>(dom: &D) -> bool {
    find_first(dom, dom.document(), "script").is_some()
}

/// Every `<a href>` in the document, in document (pre-order) order. The **rect-free
/// anchor enumerator**: the link extractor for a crawl frontier, with no layout.
/// Anchors without an `href` (named anchors / placeholders) are skipped — they are
/// not navigable targets.
pub fn extract_links<D: LayoutDom>(dom: &D) -> Vec<Link> {
    let mut out = Vec::new();
    walk_links(dom, dom.document(), &mut out);
    out
}

fn walk_links<D: LayoutDom>(dom: &D, id: D::NodeId, out: &mut Vec<Link>) {
    if local_name(dom, id) == Some("a") {
        if let Some(href) = attr(dom, id, "href") {
            out.push(Link {
                href,
                text: text_of(dom, id),
                rel: attr(dom, id, "rel"),
            });
        }
    }
    for child in dom.dom_children(id) {
        walk_links(dom, child, out);
    }
}

/// The document `<title>` text (whitespace-collapsed), or `None` if absent/empty.
pub fn extract_title<D: LayoutDom>(dom: &D) -> Option<String> {
    let id = find_first(dom, dom.document(), "title")?;
    let text = text_of(dom, id);
    (!text.is_empty()).then_some(text)
}

/// The first element with local name `name` in pre-order, or `None`.
fn find_first<D: LayoutDom>(dom: &D, id: D::NodeId, name: &str) -> Option<D::NodeId> {
    if local_name(dom, id) == Some(name) {
        return Some(id);
    }
    for child in dom.dom_children(id) {
        if let Some(found) = find_first(dom, child, name) {
            return Some(found);
        }
    }
    None
}

/// The `<h1>`–`<h6>` outline in document (pre-order) order, each with its level and
/// collapsed text. Empty headings are skipped (no text to outline).
pub fn extract_headings<D: LayoutDom>(dom: &D) -> Vec<Heading> {
    let mut out = Vec::new();
    walk_headings(dom, dom.document(), &mut out);
    out
}

fn walk_headings<D: LayoutDom>(dom: &D, id: D::NodeId, out: &mut Vec<Heading>) {
    if let Some(level) = local_name(dom, id).and_then(heading_level) {
        let text = text_of(dom, id);
        if !text.is_empty() {
            out.push(Heading { level, text });
        }
    }
    for child in dom.dom_children(id) {
        walk_headings(dom, child, out);
    }
}

/// `1`–`6` for `h1`–`h6`, else `None`.
fn heading_level(name: &str) -> Option<u8> {
    match name.as_bytes() {
        [b'h', d @ b'1'..=b'6'] => Some(d - b'0'),
        _ => None,
    }
}

/// The page's declared [`Metadata`]: `<meta name="description">`, the
/// `<link rel="canonical">` href, and OpenGraph `<meta property="og:*">` pairs.
/// Walks the whole tree (not just `<head>`) since pages place these loosely.
pub fn extract_metadata<D: LayoutDom>(dom: &D) -> Metadata {
    let mut md = Metadata::default();
    walk_metadata(dom, dom.document(), &mut md);
    md
}

fn walk_metadata<D: LayoutDom>(dom: &D, id: D::NodeId, md: &mut Metadata) {
    match local_name(dom, id) {
        Some("meta") => {
            // OpenGraph (`property="og:*"`) takes precedence over `name`; a `<meta>`
            // carries one or the other. Only the *first* description wins.
            if let Some(prop) = attr(dom, id, "property") {
                if let Some(key) = prop.strip_prefix("og:") {
                    if let Some(content) = attr(dom, id, "content") {
                        md.open_graph.push((key.to_string(), content));
                    }
                }
            } else if attr(dom, id, "name").as_deref() == Some("description")
                && md.description.is_none()
            {
                md.description = attr(dom, id, "content").filter(|c| !c.is_empty());
            }
        },
        Some("link") if md.canonical.is_none() && rel_has(dom, id, "canonical") => {
            md.canonical = attr(dom, id, "href").filter(|h| !h.is_empty());
        },
        _ => {},
    }
    for child in dom.dom_children(id) {
        walk_metadata(dom, child, md);
    }
}

/// Whether `id`'s `rel` attribute contains the (space-separated, case-insensitive)
/// token `token` — `rel` is a token list (`"stylesheet preload"`, `"canonical"`).
fn rel_has<D: LayoutDom>(dom: &D, id: D::NodeId, token: &str) -> bool {
    attr(dom, id, "rel").is_some_and(|rel| {
        rel.split_whitespace()
            .any(|t| t.eq_ignore_ascii_case(token))
    })
}

/// The page's full **visible text**, whitespace-collapsed: every text node except
/// those under non-rendered elements (`<script>` / `<style>` / `<template>` /
/// `<noscript>` / the document `<head>`). The indexing/corpus text — deliberately
/// *not* a main-content heuristic (which would drop nav/footer chrome); that
/// readability pass is a later slice that can build on this.
pub fn extract_text<D: LayoutDom>(dom: &D) -> String {
    let mut out = String::new();
    collect_visible_text(dom, dom.document(), &mut out);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Names of subtrees that carry no visible page text and are skipped wholesale.
fn is_non_rendered(name: &str) -> bool {
    matches!(name, "script" | "style" | "template" | "noscript" | "head")
}

fn collect_visible_text<D: LayoutDom>(dom: &D, id: D::NodeId, out: &mut String) {
    if local_name(dom, id).is_some_and(is_non_rendered) {
        return; // skip the whole subtree
    }
    if let Some(t) = dom.text(id) {
        out.push_str(t);
        out.push(' '); // separator so adjacent inline runs don't fuse
    }
    for child in dom.dom_children(id) {
        collect_visible_text(dom, child, out);
    }
}

// ---- reader-mode / main-content extraction ------------------------------------
//
// A compact readability heuristic (technique borrowed from readability.js): a
// semantic `<main>` wins outright; otherwise score block containers by paragraph
// density and class/id signal and take the best. Then emit that block's text with
// chrome (nav / header / footer / aside) and non-rendered subtrees dropped. The
// per-page payload for a reader-mode crawl.

/// Positive class/id signals: an element whose `class`/`id` contains one of these is
/// likely the article body (readability.js's positive lexicon, trimmed).
const POSITIVE_HINTS: &[&str] = &[
    "article", "body", "content", "entry", "main", "page", "post", "text", "blog", "story",
    "column", "prose",
];

/// Negative class/id signals: chrome, boilerplate, furniture. An element matching one
/// is penalized as unlikely to be the article body.
const NEGATIVE_HINTS: &[&str] = &[
    "nav",
    "menu",
    "header",
    "footer",
    "sidebar",
    "comment",
    "ads",
    "banner",
    "sponsor",
    "social",
    "share",
    "related",
    "promo",
    "masthead",
    "widget",
    "byline",
    "breadcrumb",
];

/// The page's **reader-mode article body**: locate the main content block and return
/// its chrome-free text, whitespace-collapsed. `None` when nothing contentful stands
/// out (an app shell, a pure link list). The per-page payload for a reader-mode crawl.
pub fn extract_main_text<D: LayoutDom>(dom: &D) -> Option<String> {
    let selected = select_content(dom)?;
    let text = chrome_free_text(dom, &selected.roots);
    (!text.is_empty()).then_some(text)
}

struct SelectedContent<Id> {
    roots: Vec<Id>,
    selector: RootSelector,
}

/// Select the article root and absorb adjacent article-grade siblings. Several
/// `<article>` elements identify an index page rather than one readable article.
fn select_content<D: LayoutDom>(dom: &D) -> Option<SelectedContent<D::NodeId>> {
    if count_elements(dom, dom.document(), "article") > 1 {
        return None;
    }
    if let Some(main) = find_first(dom, dom.document(), "main")
        && is_article_grade(dom, main)
    {
        return Some(SelectedContent {
            roots: absorb_siblings(dom, main),
            selector: RootSelector::Main,
        });
    }
    let mut best: Option<(i32, D::NodeId)> = None;
    score_candidates(dom, dom.document(), &mut best);
    let (score, root) = best.filter(|(score, id)| *score > 0 && is_article_grade(dom, *id))?;
    Some(SelectedContent {
        roots: absorb_siblings(dom, root),
        selector: RootSelector::ScoredCandidate {
            tag: local_name(dom, root).unwrap_or("unknown").to_string(),
            score,
        },
    })
}

fn count_elements<D: LayoutDom>(dom: &D, id: D::NodeId, name: &str) -> usize {
    usize::from(local_name(dom, id) == Some(name))
        + dom
            .dom_children(id)
            .map(|child| count_elements(dom, child, name))
            .sum::<usize>()
}

fn is_article_grade<D: LayoutDom>(dom: &D, id: D::NodeId) -> bool {
    let text_len = chrome_free_text(dom, &[id]).len();
    text_len >= 40 && link_density_percent(dom, id) < 50
}

fn absorb_siblings<D: LayoutDom>(dom: &D, root: D::NodeId) -> Vec<D::NodeId> {
    let Some(parent) = dom.parent(root) else {
        return vec![root];
    };
    let siblings = dom.dom_children(parent).collect::<Vec<_>>();
    let Some(index) = siblings.iter().position(|candidate| *candidate == root) else {
        return vec![root];
    };
    let absorbable = |id| {
        is_candidate_block(dom, id)
            && paragraph_text_len(dom, id) >= 60
            && link_density_percent(dom, id) < 40
            && classid_signal(dom, id) >= 0
    };
    let mut start = index;
    while start > 0 && absorbable(siblings[start - 1]) {
        start -= 1;
    }
    let mut end = index + 1;
    while end < siblings.len() && absorbable(siblings[end]) {
        end += 1;
    }
    siblings[start..end].to_vec()
}

/// Score every candidate block in the tree, tracking the maximum.
fn score_candidates<D: LayoutDom>(dom: &D, id: D::NodeId, best: &mut Option<(i32, D::NodeId)>) {
    if is_candidate_block(dom, id) {
        let score = score_block(dom, id);
        let better = match *best {
            Some((s, _)) => score > s,
            None => true,
        };
        if better {
            *best = Some((score, id));
        }
    }
    for child in dom.dom_children(id) {
        score_candidates(dom, child, best);
    }
}

/// The block-level containers that can be the article root.
fn is_candidate_block<D: LayoutDom>(dom: &D, id: D::NodeId) -> bool {
    matches!(
        local_name(dom, id),
        Some("div" | "section" | "article" | "td")
    )
}

/// A block's readability score: a tag bonus, the class/id signal, and paragraph
/// density (the dominant term — an article body is mostly paragraph text).
fn score_block<D: LayoutDom>(dom: &D, id: D::NodeId) -> i32 {
    let mut score = classid_signal(dom, id);
    score += match local_name(dom, id) {
        Some("article") => 25,
        Some("section") => 8,
        Some("td") => 3,
        _ => 0,
    };
    // Paragraph density in ~50-char units, capped so one giant block doesn't wholly
    // swamp the class/tag signal.
    score += (paragraph_text_len(dom, id) / 50).min(50) as i32;
    // Link-heavy candidates are navigation/index furniture, even when their labels
    // happen to be long enough to resemble prose.
    score -= (link_density_percent(dom, id) / 2) as i32;
    score
}

fn link_density_percent<D: LayoutDom>(dom: &D, id: D::NodeId) -> usize {
    let total = text_of(dom, id).len();
    if total == 0 {
        return 100;
    }
    let mut linked = 0;
    linked_text_len(dom, id, false, &mut linked);
    linked.saturating_mul(100) / total
}

fn linked_text_len<D: LayoutDom>(dom: &D, id: D::NodeId, in_link: bool, total: &mut usize) {
    let in_link = in_link || local_name(dom, id) == Some("a");
    if in_link && let Some(text) = dom.text(id) {
        *total += text.len();
    }
    for child in dom.dom_children(id) {
        linked_text_len(dom, child, in_link, total);
    }
}

/// Sum of descendant `<p>` text lengths under `id` (tiny paragraphs ignored — UI
/// labels, not prose).
fn paragraph_text_len<D: LayoutDom>(dom: &D, id: D::NodeId) -> usize {
    let mut total = 0;
    walk_paragraph_len(dom, id, &mut total);
    total
}

fn walk_paragraph_len<D: LayoutDom>(dom: &D, id: D::NodeId, total: &mut usize) {
    if local_name(dom, id) == Some("p") {
        let len = text_of(dom, id).len();
        if len >= 25 {
            *total += len;
        }
    }
    for child in dom.dom_children(id) {
        walk_paragraph_len(dom, child, total);
    }
}

/// The class/id signal: `+25` if any positive hint and `-25` if any negative hint
/// appears in the element's `class` or `id` (substring, lowercased), as readability does.
fn classid_signal<D: LayoutDom>(dom: &D, id: D::NodeId) -> i32 {
    let haystack = format!(
        "{} {}",
        attr(dom, id, "class").unwrap_or_default(),
        attr(dom, id, "id").unwrap_or_default(),
    )
    .to_ascii_lowercase();
    let mut score = 0;
    if POSITIVE_HINTS.iter().any(|h| haystack.contains(h)) {
        score += 25;
    }
    if NEGATIVE_HINTS.iter().any(|h| haystack.contains(h)) {
        score -= 25;
    }
    score
}

/// Text under `root` with chrome (nav / header / footer / aside) and non-rendered
/// (script / style / …) subtrees dropped — the reader-mode body text.
fn chrome_free_text<D: LayoutDom>(dom: &D, roots: &[D::NodeId]) -> String {
    let mut out = String::new();
    for root in roots {
        collect_main_text(dom, *root, &mut out);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn collect_main_text<D: LayoutDom>(dom: &D, id: D::NodeId, out: &mut String) {
    if local_name(dom, id).is_some_and(is_chrome_or_non_rendered) {
        return; // skip chrome / non-rendered subtrees
    }
    if let Some(t) = dom.text(id) {
        out.push_str(t);
        out.push(' ');
    }
    for child in dom.dom_children(id) {
        collect_main_text(dom, child, out);
    }
}

/// Subtrees excluded from reader-mode body text: non-rendered content plus page
/// chrome (the landmarks that are not the article).
fn is_chrome_or_non_rendered(name: &str) -> bool {
    is_non_rendered(name) || matches!(name, "nav" | "header" | "footer" | "aside")
}

// ---- structured reader extraction --------------------------------------------

fn extract_article_with_page<D: LayoutDom>(
    dom: &D,
    page: &PageExtract,
    selected: SelectedContent<D::NodeId>,
) -> Option<Article> {
    let mut blocks = Vec::new();
    for root in &selected.roots {
        collect_blocks(dom, *root, &mut blocks, true);
    }
    if blocks.is_empty() {
        return None;
    }
    let title = open_graph_value(&page.metadata, "title")
        .map(str::to_string)
        .or_else(|| first_heading_text(dom, &selected.roots))
        .or_else(|| page.title.clone());
    let byline = first_meta_content(dom, &["author", "byl"])
        .or_else(|| first_class_text(dom, dom.document(), &["byline", "author"]));
    let published = first_meta_content(
        dom,
        &["article:published_time", "date", "datepublished", "pubdate"],
    )
    .or_else(|| first_time_value(dom, dom.document()));
    let lang = find_first(dom, dom.document(), "html").and_then(|html| attr(dom, html, "lang"));
    let site = open_graph_value(&page.metadata, "site_name").map(str::to_string);
    let canonical = page.metadata.canonical.clone();
    let lead_image = open_graph_value(&page.metadata, "image")
        .map(str::to_string)
        .or_else(|| first_attr_in_roots(dom, &selected.roots, "img", "src"));
    let block_count = blocks.iter().map(count_block_tree).sum();
    Some(Article {
        title,
        byline,
        published,
        lang,
        site,
        canonical,
        lead_image,
        blocks,
        lineage: ExtractionLineage {
            fleece_version: env!("CARGO_PKG_VERSION").to_string(),
            root_selector: selected.selector,
            block_count,
        },
    })
}

fn open_graph_value<'a>(metadata: &'a Metadata, key: &str) -> Option<&'a str> {
    metadata
        .open_graph
        .iter()
        .find_map(|(candidate, value)| (candidate == key).then_some(value.as_str()))
}

fn first_heading_text<D: LayoutDom>(dom: &D, roots: &[D::NodeId]) -> Option<String> {
    for root in roots {
        if let Some(heading) = find_first_heading(dom, *root) {
            let text = text_of(dom, heading);
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
}

fn find_first_heading<D: LayoutDom>(dom: &D, id: D::NodeId) -> Option<D::NodeId> {
    if local_name(dom, id).and_then(heading_level).is_some() {
        return Some(id);
    }
    dom.dom_children(id)
        .find_map(|child| find_first_heading(dom, child))
}

fn first_meta_content<D: LayoutDom>(dom: &D, names: &[&str]) -> Option<String> {
    fn walk<D: LayoutDom>(dom: &D, id: D::NodeId, names: &[&str]) -> Option<String> {
        if local_name(dom, id) == Some("meta") {
            let key = attr(dom, id, "property").or_else(|| attr(dom, id, "name"));
            if key
                .as_deref()
                .is_some_and(|key| names.iter().any(|name| key.eq_ignore_ascii_case(name)))
            {
                return attr(dom, id, "content").filter(|value| !value.is_empty());
            }
        }
        dom.dom_children(id)
            .find_map(|child| walk(dom, child, names))
    }
    walk(dom, dom.document(), names)
}

fn first_class_text<D: LayoutDom>(dom: &D, id: D::NodeId, hints: &[&str]) -> Option<String> {
    let class_id = format!(
        "{} {}",
        attr(dom, id, "class").unwrap_or_default(),
        attr(dom, id, "id").unwrap_or_default()
    )
    .to_ascii_lowercase();
    if hints.iter().any(|hint| class_id.contains(hint)) {
        let text = text_of(dom, id);
        if !text.is_empty() {
            return Some(text);
        }
    }
    dom.dom_children(id)
        .find_map(|child| first_class_text(dom, child, hints))
}

fn first_time_value<D: LayoutDom>(dom: &D, id: D::NodeId) -> Option<String> {
    if local_name(dom, id) == Some("time") {
        return attr(dom, id, "datetime").or_else(|| {
            let text = text_of(dom, id);
            (!text.is_empty()).then_some(text)
        });
    }
    dom.dom_children(id)
        .find_map(|child| first_time_value(dom, child))
}

fn first_attr_in_roots<D: LayoutDom>(
    dom: &D,
    roots: &[D::NodeId],
    tag: &str,
    attribute: &str,
) -> Option<String> {
    roots
        .iter()
        .find_map(|root| first_attr(dom, *root, tag, attribute))
}

fn first_attr<D: LayoutDom>(dom: &D, id: D::NodeId, tag: &str, attribute: &str) -> Option<String> {
    if local_name(dom, id) == Some(tag)
        && let Some(value) = attr(dom, id, attribute).filter(|value| !value.is_empty())
    {
        return Some(value);
    }
    dom.dom_children(id)
        .find_map(|child| first_attr(dom, child, tag, attribute))
}

fn count_block_tree(block: &Block) -> usize {
    1 + match block {
        Block::List { items, .. } => items.iter().flatten().map(count_block_tree).sum::<usize>(),
        Block::Quote { blocks } => blocks.iter().map(count_block_tree).sum(),
        _ => 0,
    }
}

fn collect_blocks<D: LayoutDom>(dom: &D, id: D::NodeId, out: &mut Vec<Block>, include_self: bool) {
    let name = local_name(dom, id);
    if name.is_some_and(is_chrome_or_non_rendered) {
        return;
    }
    if include_self {
        if let Some(level) = name.and_then(heading_level) {
            push_inline_block(
                out,
                |runs| Block::Heading { level, runs },
                inline_runs(dom, id),
            );
            return;
        }
        match name {
            Some("p") => {
                push_inline_block(out, |runs| Block::Paragraph { runs }, inline_runs(dom, id));
                return;
            },
            Some("ul" | "ol") => {
                let ordered = name == Some("ol");
                let items = list_items(dom, id);
                if !items.is_empty() {
                    out.push(Block::List { ordered, items });
                }
                return;
            },
            Some("blockquote") => {
                let mut blocks = Vec::new();
                for child in dom.dom_children(id) {
                    collect_blocks(dom, child, &mut blocks, true);
                }
                if blocks.is_empty() {
                    push_inline_block(
                        &mut blocks,
                        |runs| Block::Paragraph { runs },
                        inline_runs(dom, id),
                    );
                }
                if !blocks.is_empty() {
                    out.push(Block::Quote { blocks });
                }
                return;
            },
            Some("pre") => {
                let language = code_language(dom, id);
                let text = text_of(dom, id);
                if !text.is_empty() {
                    out.push(Block::Code { language, text });
                }
                return;
            },
            Some("table") => {
                let rows = table_rows(dom, id);
                if !rows.is_empty() {
                    out.push(Block::Table { rows });
                }
                return;
            },
            Some("figure") => {
                if let Some(figure) = figure_block(dom, id) {
                    out.push(figure);
                }
                return;
            },
            Some("img") => {
                if let Some(src) = attr(dom, id, "src").filter(|src| !src.is_empty()) {
                    out.push(Block::Figure {
                        src,
                        alt: attr(dom, id, "alt").unwrap_or_default(),
                        caption: None,
                    });
                }
                return;
            },
            Some("hr") => {
                out.push(Block::Rule);
                return;
            },
            _ => {},
        }
    }
    for child in dom.dom_children(id) {
        collect_blocks(dom, child, out, true);
    }
}

fn push_inline_block(
    out: &mut Vec<Block>,
    make: impl FnOnce(Vec<Inline>) -> Block,
    runs: Vec<Inline>,
) {
    if !inline_plain_text(&runs).trim().is_empty() {
        out.push(make(runs));
    }
}

fn list_items<D: LayoutDom>(dom: &D, list: D::NodeId) -> Vec<Vec<Block>> {
    dom.dom_children(list)
        .filter(|child| local_name(dom, *child) == Some("li"))
        .filter_map(|item| {
            let mut blocks = Vec::new();
            let shallow = inline_runs_shallow(dom, item);
            push_inline_block(&mut blocks, |runs| Block::Paragraph { runs }, shallow);
            for child in dom.dom_children(item) {
                if local_name(dom, child).is_some_and(is_structural_block) {
                    collect_blocks(dom, child, &mut blocks, true);
                }
            }
            (!blocks.is_empty()).then_some(blocks)
        })
        .collect()
}

fn is_structural_block(name: &str) -> bool {
    matches!(
        name,
        "h1" | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "p"
            | "ul"
            | "ol"
            | "blockquote"
            | "pre"
            | "table"
            | "figure"
            | "hr"
    )
}

fn inline_runs<D: LayoutDom>(dom: &D, id: D::NodeId) -> Vec<Inline> {
    let mut runs = Vec::new();
    for child in dom.dom_children(id) {
        collect_inline(dom, child, &mut runs, false);
    }
    trim_inline_edges(&mut runs);
    runs
}

fn inline_runs_shallow<D: LayoutDom>(dom: &D, id: D::NodeId) -> Vec<Inline> {
    let mut runs = Vec::new();
    for child in dom.dom_children(id) {
        if local_name(dom, child).is_some_and(is_structural_block) {
            continue;
        }
        collect_inline(dom, child, &mut runs, true);
    }
    trim_inline_edges(&mut runs);
    runs
}

fn collect_inline<D: LayoutDom>(dom: &D, id: D::NodeId, out: &mut Vec<Inline>, shallow: bool) {
    if let Some(text) = dom.text(id) {
        let collapsed = collapse_inline_text(text);
        if !collapsed.is_empty() {
            push_text_run(out, collapsed);
        }
        return;
    }
    let name = local_name(dom, id);
    if name.is_some_and(is_chrome_or_non_rendered)
        || shallow && name.is_some_and(is_structural_block)
    {
        return;
    }
    let nested = |dom: &D, id, shallow| {
        let mut runs = Vec::new();
        for child in dom.dom_children(id) {
            collect_inline(dom, child, &mut runs, shallow);
        }
        trim_inline_edges(&mut runs);
        runs
    };
    match name {
        Some("a") => {
            let runs = nested(dom, id, shallow);
            if let Some(href) = attr(dom, id, "href")
                && !runs.is_empty()
            {
                out.push(Inline::Link { href, runs });
            } else {
                out.extend(runs);
            }
        },
        Some("strong" | "b" | "em" | "i") => {
            let runs = nested(dom, id, shallow);
            if !runs.is_empty() {
                out.push(Inline::Emphasis {
                    strong: matches!(name, Some("strong" | "b")),
                    runs,
                });
            }
        },
        Some("code") => {
            let text = text_of(dom, id);
            if !text.is_empty() {
                out.push(Inline::Code(text));
            }
        },
        Some("br") => push_text_run(out, "\n".to_string()),
        _ => out.extend(nested(dom, id, shallow)),
    }
}

fn collapse_inline_text(text: &str) -> String {
    let leading = text.chars().next().is_some_and(char::is_whitespace);
    let trailing = text.chars().next_back().is_some_and(char::is_whitespace);
    let core = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if core.is_empty() {
        return if leading {
            " ".to_string()
        } else {
            String::new()
        };
    }
    format!(
        "{}{}{}",
        if leading { " " } else { "" },
        core,
        if trailing { " " } else { "" }
    )
}

fn push_text_run(out: &mut Vec<Inline>, text: String) {
    if let Some(Inline::Text(previous)) = out.last_mut() {
        previous.push_str(&text);
    } else {
        out.push(Inline::Text(text));
    }
}

fn trim_inline_edges(runs: &mut Vec<Inline>) {
    if let Some(Inline::Text(first)) = runs.first_mut() {
        *first = first.trim_start().to_string();
    }
    if let Some(Inline::Text(last)) = runs.last_mut() {
        *last = last.trim_end().to_string();
    }
    runs.retain(|run| !matches!(run, Inline::Text(text) if text.is_empty()));
}

fn inline_plain_text(runs: &[Inline]) -> String {
    let mut out = String::new();
    for run in runs {
        match run {
            Inline::Text(text) | Inline::Code(text) => out.push_str(text),
            Inline::Link { runs, .. } | Inline::Emphasis { runs, .. } => {
                out.push_str(&inline_plain_text(runs));
            },
        }
    }
    out
}

fn code_language<D: LayoutDom>(dom: &D, pre: D::NodeId) -> Option<String> {
    let code = find_first(dom, pre, "code")?;
    let class = attr(dom, code, "class")?;
    class.split_whitespace().find_map(|token| {
        token
            .strip_prefix("language-")
            .or_else(|| token.strip_prefix("lang-"))
            .map(str::to_string)
    })
}

fn table_rows<D: LayoutDom>(dom: &D, table: D::NodeId) -> Vec<TableRow> {
    fn walk<D: LayoutDom>(dom: &D, id: D::NodeId, rows: &mut Vec<TableRow>) {
        if local_name(dom, id) == Some("tr") {
            let cells = dom
                .dom_children(id)
                .filter(|cell| matches!(local_name(dom, *cell), Some("th" | "td")))
                .map(|cell| TableCell {
                    header: local_name(dom, cell) == Some("th"),
                    runs: inline_runs(dom, cell),
                })
                .collect::<Vec<_>>();
            if !cells.is_empty() {
                let header = cells.iter().all(|cell| cell.header);
                rows.push(TableRow { header, cells });
            }
            return;
        }
        for child in dom.dom_children(id) {
            walk(dom, child, rows);
        }
    }
    let mut rows = Vec::new();
    walk(dom, table, &mut rows);
    rows
}

fn figure_block<D: LayoutDom>(dom: &D, figure: D::NodeId) -> Option<Block> {
    let image = find_first(dom, figure, "img")?;
    let src = attr(dom, image, "src").filter(|src| !src.is_empty())?;
    let caption = find_first(dom, figure, "figcaption")
        .map(|node| inline_runs(dom, node))
        .filter(|runs| !runs.is_empty());
    Some(Block::Figure {
        src,
        alt: attr(dom, image, "alt").unwrap_or_default(),
        caption,
    })
}

// ---- structured-data harvest -------------------------------------------------

/// Harvest JSON-LD and microdata without interpreting consumer policy.
pub fn extract_structured_data<D: LayoutDom>(dom: &D) -> Vec<StructuredData> {
    let mut out = Vec::new();
    walk_structured_data(dom, dom.document(), &mut out);
    out
}

fn walk_structured_data<D: LayoutDom>(dom: &D, id: D::NodeId, out: &mut Vec<StructuredData>) {
    if local_name(dom, id) == Some("script")
        && attr(dom, id, "type").is_some_and(|value| {
            value
                .split(';')
                .next()
                .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/ld+json"))
        })
    {
        let raw = raw_text_of(dom, id);
        if let Some(value) = JsonParser::new(&raw).parse() {
            collect_json_ld(value, out);
        }
    }
    if attr(dom, id, "itemscope").is_some()
        && let Some(data) = microdata_item(dom, id)
    {
        out.push(data);
    }
    for child in dom.dom_children(id) {
        walk_structured_data(dom, child, out);
    }
}

fn collect_json_ld(value: StructuredValue, out: &mut Vec<StructuredData>) {
    match value {
        StructuredValue::Array(values) => {
            for value in values {
                collect_json_ld(value, out);
            }
        },
        StructuredValue::Object(entries) => {
            let value = StructuredValue::Object(entries);
            if let Some(kind) = json_ld_kind(&value) {
                out.push(StructuredData {
                    kind,
                    value: value.clone(),
                    source: StructuredDataSource::JsonLd,
                });
            }
            if let Some(StructuredValue::Array(graph)) = value.get("@graph") {
                for child in graph.clone() {
                    collect_json_ld(child, out);
                }
            }
        },
        _ => {},
    }
}

fn json_ld_kind(value: &StructuredValue) -> Option<String> {
    match value.get("@type")? {
        StructuredValue::String(kind) => Some(short_schema_type(kind)),
        StructuredValue::Array(kinds) => kinds
            .iter()
            .find_map(StructuredValue::as_str)
            .map(short_schema_type),
        _ => None,
    }
}

fn short_schema_type(value: &str) -> String {
    value.rsplit(['/', '#']).next().unwrap_or(value).to_string()
}

fn microdata_item<D: LayoutDom>(dom: &D, root: D::NodeId) -> Option<StructuredData> {
    let kind = attr(dom, root, "itemtype")
        .and_then(|types| types.split_whitespace().next().map(short_schema_type))?;
    let mut fields = Vec::new();
    collect_microdata_fields(dom, root, root, &mut fields);
    Some(StructuredData {
        kind,
        value: StructuredValue::Object(fields),
        source: StructuredDataSource::Microdata,
    })
}

fn collect_microdata_fields<D: LayoutDom>(
    dom: &D,
    root: D::NodeId,
    id: D::NodeId,
    fields: &mut Vec<(String, StructuredValue)>,
) {
    if id != root && attr(dom, id, "itemscope").is_some() {
        if let Some(property) = attr(dom, id, "itemprop")
            && let Some(item) = microdata_item(dom, id)
        {
            fields.push((property, item.value));
        }
        return;
    }
    if id != root
        && let Some(property) = attr(dom, id, "itemprop")
    {
        let value = attr(dom, id, "content")
            .or_else(|| attr(dom, id, "datetime"))
            .or_else(|| attr(dom, id, "href"))
            .or_else(|| attr(dom, id, "src"))
            .unwrap_or_else(|| text_of(dom, id));
        if !value.is_empty() {
            for property in property.split_whitespace() {
                fields.push((property.to_string(), StructuredValue::String(value.clone())));
            }
        }
    }
    for child in dom.dom_children(id) {
        collect_microdata_fields(dom, root, child, fields);
    }
}

fn raw_text_of<D: LayoutDom>(dom: &D, id: D::NodeId) -> String {
    let mut out = String::new();
    collect_text(dom, id, &mut out);
    out
}

struct JsonParser<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl<'a> JsonParser<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            bytes: source.as_bytes(),
            cursor: 0,
        }
    }

    fn parse(mut self) -> Option<StructuredValue> {
        self.skip_ws();
        let value = self.value()?;
        self.skip_ws();
        (self.cursor == self.bytes.len()).then_some(value)
    }

    fn value(&mut self) -> Option<StructuredValue> {
        self.skip_ws();
        match self.peek()? {
            b'n' => self.literal(b"null", StructuredValue::Null),
            b't' => self.literal(b"true", StructuredValue::Bool(true)),
            b'f' => self.literal(b"false", StructuredValue::Bool(false)),
            b'"' => self.string().map(StructuredValue::String),
            b'[' => self.array(),
            b'{' => self.object(),
            b'-' | b'0'..=b'9' => self.number().map(StructuredValue::Number),
            _ => None,
        }
    }

    fn literal(&mut self, literal: &[u8], value: StructuredValue) -> Option<StructuredValue> {
        let end = self.cursor.checked_add(literal.len())?;
        (self.bytes.get(self.cursor..end)? == literal).then(|| {
            self.cursor = end;
            value
        })
    }

    fn array(&mut self) -> Option<StructuredValue> {
        self.take(b'[')?;
        let mut values = Vec::new();
        self.skip_ws();
        if self.take(b']').is_some() {
            return Some(StructuredValue::Array(values));
        }
        loop {
            values.push(self.value()?);
            self.skip_ws();
            if self.take(b']').is_some() {
                break;
            }
            self.take(b',')?;
        }
        Some(StructuredValue::Array(values))
    }

    fn object(&mut self) -> Option<StructuredValue> {
        self.take(b'{')?;
        let mut entries = Vec::new();
        self.skip_ws();
        if self.take(b'}').is_some() {
            return Some(StructuredValue::Object(entries));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            self.take(b':')?;
            entries.push((key, self.value()?));
            self.skip_ws();
            if self.take(b'}').is_some() {
                break;
            }
            self.take(b',')?;
        }
        Some(StructuredValue::Object(entries))
    }

    fn string(&mut self) -> Option<String> {
        self.take(b'"')?;
        let mut out = String::new();
        while let Some(byte) = self.peek() {
            self.cursor += 1;
            match byte {
                b'"' => return Some(out),
                b'\\' => {
                    let escaped = self.peek()?;
                    self.cursor += 1;
                    match escaped {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => out.push(self.unicode_escape()?),
                        _ => return None,
                    }
                },
                0x00..=0x1f => return None,
                ascii if ascii.is_ascii() => out.push(ascii as char),
                _ => {
                    self.cursor -= 1;
                    let tail = std::str::from_utf8(&self.bytes[self.cursor..]).ok()?;
                    let character = tail.chars().next()?;
                    self.cursor += character.len_utf8();
                    out.push(character);
                },
            }
        }
        None
    }

    fn unicode_escape(&mut self) -> Option<char> {
        let first = self.hex_quad()?;
        if (0xd800..=0xdbff).contains(&first) {
            self.take(b'\\')?;
            self.take(b'u')?;
            let second = self.hex_quad()?;
            if !(0xdc00..=0xdfff).contains(&second) {
                return None;
            }
            let scalar =
                0x1_0000 + ((u32::from(first) - 0xd800) << 10) + (u32::from(second) - 0xdc00);
            char::from_u32(scalar)
        } else {
            char::from_u32(u32::from(first))
        }
    }

    fn hex_quad(&mut self) -> Option<u16> {
        let mut value = 0u16;
        for _ in 0..4 {
            let digit = (self.peek()? as char).to_digit(16)? as u16;
            self.cursor += 1;
            value = value.checked_mul(16)?.checked_add(digit)?;
        }
        Some(value)
    }

    fn number(&mut self) -> Option<String> {
        let start = self.cursor;
        if self.peek() == Some(b'-') {
            self.cursor += 1;
        }
        match self.peek()? {
            b'0' => self.cursor += 1,
            b'1'..=b'9' => {
                self.cursor += 1;
                while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                    self.cursor += 1;
                }
            },
            _ => return None,
        }
        if self.peek() == Some(b'.') {
            self.cursor += 1;
            let fraction = self.cursor;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.cursor += 1;
            }
            if self.cursor == fraction {
                return None;
            }
        }
        if self.peek().is_some_and(|byte| matches!(byte, b'e' | b'E')) {
            self.cursor += 1;
            if self.peek().is_some_and(|byte| matches!(byte, b'+' | b'-')) {
                self.cursor += 1;
            }
            let exponent = self.cursor;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.cursor += 1;
            }
            if self.cursor == exponent {
                return None;
            }
        }
        std::str::from_utf8(&self.bytes[start..self.cursor])
            .ok()
            .map(str::to_string)
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.cursor += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.cursor).copied()
    }

    fn take(&mut self, expected: u8) -> Option<()> {
        (self.peek()? == expected).then(|| self.cursor += 1)
    }
}

// ---- small DOM helpers (rect-free, allocation-light) --------------------------

/// `id`'s element local name as `&str`, or `None` for non-elements.
fn local_name<D: LayoutDom>(dom: &D, id: D::NodeId) -> Option<&str> {
    dom.element_name(id).map(|q| q.local.as_ref())
}

/// A null-namespace attribute (`href`, `rel`, `id`, … — the HTML common case).
fn attr<D: LayoutDom>(dom: &D, id: D::NodeId, name: &str) -> Option<String> {
    dom.attribute(id, &Namespace::from(""), &LocalName::from(name))
        .map(str::to_string)
}

/// The whitespace-collapsed concatenation of all descendant text under `id` — an
/// element's "visible text" for extraction (script/style content is parsed as text
/// children, but anchors and titles do not contain them, so no filtering is needed
/// at this slice; a main-text extractor will skip `<script>`/`<style>`).
fn text_of<D: LayoutDom>(dom: &D, id: D::NodeId) -> String {
    let mut raw = String::new();
    collect_text(dom, id, &mut raw);
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn collect_text<D: LayoutDom>(dom: &D, id: D::NodeId, out: &mut String) {
    if let Some(t) = dom.text(id) {
        out.push_str(t);
        out.push(' '); // a separator so adjacent inline text nodes don't fuse
    }
    for child in dom.dom_children(id) {
        collect_text(dom, child, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genet_static_dom::StaticDocument;

    #[test]
    fn extracts_anchors_with_text_and_rel() {
        let doc = StaticDocument::parse(
            "<html><body>\
                <a href=\"/one\">First</a>\
                <p>not a link</p>\
                <a href=\"https://example.com/two\" rel=\"nofollow\">Second <b>bold</b></a>\
                <a name=\"anchor\">no href, skipped</a>\
             </body></html>",
        );
        let links = extract_links(&doc);
        assert_eq!(
            links.len(),
            2,
            "two href anchors; the named anchor is skipped: {links:?}"
        );
        assert_eq!(
            links[0],
            Link {
                href: "/one".into(),
                text: "First".into(),
                rel: None
            }
        );
        assert_eq!(
            links[1],
            Link {
                href: "https://example.com/two".into(),
                text: "Second bold".into(), // descendant text concatenated + collapsed
                rel: Some("nofollow".into()),
            },
        );
    }

    #[test]
    fn anchor_href_is_unresolved_raw_attribute() {
        // Extraction owns no URL context: the relative href comes back verbatim, for
        // the caller to resolve against the page URL.
        let doc = StaticDocument::parse("<body><a href=\"../sub/page.html\">x</a></body>");
        assert_eq!(extract_links(&doc)[0].href, "../sub/page.html");
    }

    #[test]
    fn extracts_the_title_collapsed() {
        let doc = StaticDocument::parse(
            "<html><head><title>  Hello   World  </title></head><body></body></html>",
        );
        assert_eq!(extract_title(&doc).as_deref(), Some("Hello World"));
    }

    #[test]
    fn no_title_is_none() {
        let doc = StaticDocument::parse("<body><p>no title here</p></body>");
        assert_eq!(extract_title(&doc), None);
    }

    #[test]
    fn extract_bundles_title_and_links() {
        let doc = StaticDocument::parse(
            "<html><head><title>T</title></head><body><a href=\"/a\">A</a></body></html>",
        );
        let page = extract(&doc);
        assert_eq!(page.title.as_deref(), Some("T"));
        assert_eq!(page.links.len(), 1);
        assert_eq!(page.links[0].href, "/a");
    }

    #[test]
    fn empty_document_extracts_nothing() {
        let doc = StaticDocument::parse("");
        assert_eq!(extract(&doc), PageExtract::default());
    }

    #[test]
    fn extracts_the_heading_outline() {
        let doc = StaticDocument::parse(
            "<body>\
                <h1>Title</h1>\
                <h2>Section <em>one</em></h2>\
                <p>body</p>\
                <h3></h3>\
                <h2>Section two</h2>\
             </body>",
        );
        assert_eq!(
            extract_headings(&doc),
            vec![
                Heading {
                    level: 1,
                    text: "Title".into()
                },
                Heading {
                    level: 2,
                    text: "Section one".into()
                },
                // the empty <h3> is skipped
                Heading {
                    level: 2,
                    text: "Section two".into()
                },
            ],
        );
    }

    #[test]
    fn extracts_description_canonical_and_open_graph() {
        let doc = StaticDocument::parse(
            "<html><head>\
                <meta name=\"description\" content=\"A page about things.\">\
                <link rel=\"canonical\" href=\"https://example.com/page\">\
                <meta property=\"og:title\" content=\"Things\">\
                <meta property=\"og:image\" content=\"https://example.com/og.png\">\
             </head><body></body></html>",
        );
        let md = extract_metadata(&doc);
        assert_eq!(md.description.as_deref(), Some("A page about things."));
        assert_eq!(md.canonical.as_deref(), Some("https://example.com/page"));
        assert_eq!(
            md.open_graph,
            vec![
                ("title".to_string(), "Things".to_string()),
                (
                    "image".to_string(),
                    "https://example.com/og.png".to_string()
                ),
            ],
        );
    }

    #[test]
    fn canonical_rel_is_a_token_list() {
        // `rel` is a space-separated token list; `canonical` need not be the only token.
        let doc =
            StaticDocument::parse("<head><link rel=\"alternate canonical\" href=\"/c\"></head>");
        assert_eq!(extract_metadata(&doc).canonical.as_deref(), Some("/c"));
    }

    #[test]
    fn missing_metadata_is_all_none() {
        let doc = StaticDocument::parse("<body><p>no meta</p></body>");
        assert_eq!(extract_metadata(&doc), Metadata::default());
    }

    #[test]
    fn visible_text_excludes_script_and_style() {
        let doc = StaticDocument::parse(
            "<html><head><title>T</title><style>p{color:red}</style></head>\
             <body><h1>Heading</h1><p>Para one.</p>\
             <script>var x = 'not visible';</script>\
             <p>Para two.</p></body></html>",
        );
        // <head> (title + style) and the inline <script> are excluded; body text is
        // concatenated and whitespace-collapsed.
        assert_eq!(extract_text(&doc), "Heading Para one. Para two.");
    }

    #[test]
    fn full_extract_carries_text() {
        let doc = StaticDocument::parse(
            "<html><head><title>T</title></head><body><p>Hello world.</p></body></html>",
        );
        assert_eq!(extract(&doc).text, "Hello world.");
    }

    #[test]
    fn main_text_prefers_the_main_landmark_and_drops_chrome() {
        let doc = StaticDocument::parse(
            "<body>\
                <nav><a href='/'>Home</a> Menu Junk Links</nav>\
                <header>Site Title Boilerplate Banner</header>\
                <main>\
                    <h1>Article Heading</h1>\
                    <p>This is the first paragraph of the real article body, long enough to count.</p>\
                    <p>And a second paragraph continuing the genuine article content here.</p>\
                    <aside>Inline aside promo junk to drop</aside>\
                </main>\
                <footer>Copyright Footer Junk</footer>\
             </body>",
        );
        let main = extract_main_text(&doc).expect("an article body");
        assert!(
            main.contains("first paragraph of the real article body"),
            "{main}"
        );
        assert!(main.contains("second paragraph"), "{main}");
        // Chrome outside the landmark, and an aside *inside* it, are all dropped.
        assert!(!main.contains("Menu Junk"), "nav dropped: {main}");
        assert!(!main.contains("Footer Junk"), "footer dropped: {main}");
        assert!(
            !main.contains("Boilerplate Banner"),
            "header dropped: {main}"
        );
        assert!(
            !main.contains("aside promo junk"),
            "inline aside dropped: {main}"
        );
    }

    #[test]
    fn main_text_scores_content_over_sidebar() {
        // No <main>: the heuristic must pick the article div over the sidebar by
        // paragraph density + class signal, and drop an inline footer within it.
        let doc = StaticDocument::parse(
            "<body>\
                <div class='sidebar'><p>Ads and promo links and sponsor junk over here.</p></div>\
                <div class='article-content'>\
                    <p>The genuine article body paragraph one, with substantial readable prose.</p>\
                    <p>Paragraph two of the genuine article, with more substantial readable content.</p>\
                    <footer>inline footer junk to drop</footer>\
                </div>\
             </body>",
        );
        let main = extract_main_text(&doc).expect("an article body");
        assert!(
            main.contains("genuine article body paragraph one"),
            "{main}"
        );
        assert!(
            main.contains("Paragraph two of the genuine article"),
            "{main}"
        );
        assert!(
            !main.contains("Ads and promo"),
            "sidebar lost to scoring: {main}"
        );
        assert!(
            !main.contains("footer junk"),
            "inline footer dropped: {main}"
        );
    }

    #[test]
    fn main_text_is_none_for_a_link_list() {
        // A nav-only page (an app shell / index) has no article body.
        let doc = StaticDocument::parse(
            "<body><nav><a href='/a'>A</a><a href='/b'>B</a><a href='/c'>C</a></nav></body>",
        );
        assert_eq!(extract_main_text(&doc), None);
    }

    #[test]
    fn full_extract_carries_main_text() {
        let doc = StaticDocument::parse(
            "<body><main><p>The article body paragraph with enough prose to register.</p></main></body>",
        );
        assert!(
            extract(&doc)
                .main_text
                .as_deref()
                .is_some_and(|m| m.contains("article body paragraph")),
        );
    }

    #[test]
    fn article_keeps_structure_inline_runs_metadata_and_lineage() {
        let doc = StaticDocument::parse(
            r#"<html lang="en-GB"><head>
                <title>Fallback title</title>
                <link rel="canonical" href="/canonical">
                <meta property="og:title" content="Reader title">
                <meta property="og:site_name" content="The Example">
                <meta property="og:image" content="/lead.jpg">
                <meta name="author" content="Ada Example">
                <meta property="article:published_time" content="2026-08-22">
              </head><body><main><h1>On extraction</h1>
                <p>Keep <em>quiet emphasis</em>, <strong>strong emphasis</strong>,
                <a href="/source">linked words</a>, and <code>inline()</code> intact.</p>
                <ul><li>first item<ul><li>nested item</li></ul></li><li>second item</li></ul>
                <blockquote><p>A quoted paragraph with enough substance to remain readable.</p></blockquote>
                <pre><code class="language-rust">fn main() {}</code></pre>
                <table><thead><tr><th>Name</th><th>Value</th></tr></thead>
                <tbody><tr><td>fleece</td><td>reader</td></tr></tbody></table>
                <figure><img src="/figure.png" alt="diagram"><figcaption>The figure</figcaption></figure>
                <hr><p>A final paragraph makes this unmistakably article-grade prose.</p>
              </main></body></html>"#,
        );
        let article = extract_article(&doc).expect("structured article");
        assert_eq!(article.title.as_deref(), Some("Reader title"));
        assert_eq!(article.byline.as_deref(), Some("Ada Example"));
        assert_eq!(article.published.as_deref(), Some("2026-08-22"));
        assert_eq!(article.lang.as_deref(), Some("en-GB"));
        assert_eq!(article.site.as_deref(), Some("The Example"));
        assert_eq!(article.canonical.as_deref(), Some("/canonical"));
        assert_eq!(article.lead_image.as_deref(), Some("/lead.jpg"));
        assert!(matches!(article.lineage.root_selector, RootSelector::Main));
        assert_eq!(article.lineage.fleece_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(article.lineage.block_count, 14);
        let paragraph = article
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::Paragraph { runs } if inline_plain_text(runs).contains("linked words") => {
                    Some(runs)
                },
                _ => None,
            })
            .expect("rich paragraph");
        assert!(
            paragraph
                .iter()
                .any(|run| matches!(run, Inline::Link { href, .. } if href == "/source"))
        );
        assert!(
            paragraph
                .iter()
                .any(|run| matches!(run, Inline::Emphasis { strong: false, .. }))
        );
        assert!(
            paragraph
                .iter()
                .any(|run| matches!(run, Inline::Emphasis { strong: true, .. }))
        );
        assert!(
            paragraph
                .iter()
                .any(|run| matches!(run, Inline::Code(code) if code == "inline()"))
        );
        assert!(
            article
                .blocks
                .iter()
                .any(|block| matches!(block, Block::Table { rows } if rows[0].header))
        );
        assert!(article.blocks.iter().any(|block| matches!(block, Block::Figure { src, caption: Some(_), .. } if src == "/figure.png")));
    }

    #[test]
    fn sibling_absorption_joins_article_grade_neighbors() {
        let doc = StaticDocument::parse(
            "<body><div class='article-content'><p>The opening section carries substantial prose for the reader.</p></div>\
             <section><p>The adjacent continuation also carries substantial article-grade prose.</p></section>\
             <div class='related'><p>Related promotional furniture should not join the article.</p></div></body>",
        );
        let text = extract_main_text(&doc).expect("article");
        assert!(text.contains("opening section"));
        assert!(text.contains("adjacent continuation"));
        assert!(!text.contains("promotional furniture"));
    }

    #[test]
    fn multiple_articles_identify_an_index_page() {
        let doc = StaticDocument::parse(
            "<body><article><p>The first summary contains enough prose to score.</p></article>\
             <article><p>The second summary contains enough prose to score.</p></article></body>",
        );
        assert_eq!(extract_article(&doc), None);
        assert_eq!(extract_main_text(&doc), None);
    }

    #[test]
    fn harvests_json_ld_and_microdata_as_typed_values() {
        let doc = StaticDocument::parse(
            r#"<head><script type="application/ld+json">
              {"@context":"https://schema.org","@type":"Recipe","name":"Tea","servings":2}
              </script></head><body>
              <section itemscope itemtype="https://schema.org/Event">
                <meta itemprop="name" content="Reading group">
                <time itemprop="startDate" datetime="2026-09-01">September 1</time>
              </section></body>"#,
        );
        let data = extract_structured_data(&doc);
        assert_eq!(data.len(), 2);
        assert_eq!(data[0].kind, "Recipe");
        assert_eq!(data[0].source, StructuredDataSource::JsonLd);
        assert_eq!(
            data[0].value.get("name").and_then(StructuredValue::as_str),
            Some("Tea")
        );
        assert_eq!(data[1].kind, "Event");
        assert_eq!(data[1].source, StructuredDataSource::Microdata);
        assert_eq!(
            data[1]
                .value
                .get("startDate")
                .and_then(StructuredValue::as_str),
            Some("2026-09-01")
        );
        assert!(
            extract(&StaticDocument::parse(
                "<main><p>ordinary page prose long enough to read cleanly.</p></main>"
            ))
            .structured_data
            .is_empty()
        );
    }

    #[test]
    fn labelled_reader_corpus_reports_precision_and_recall() {
        struct Fixture {
            name: &'static str,
            html: &'static str,
            expected: Option<&'static str>,
        }
        let fixtures = [
            Fixture {
                name: "news",
                html: "<html><head><title>Metro Desk</title></head><body><header><nav><a href='/local'>Local</a><a href='/weather'>Weather</a></nav></header><div class='page'><aside>Most read and market data</aside><article class='story-body'><div class='byline'>By A Reporter</div><h1>City council approves the river plan</h1><p>Councillors approved the river restoration plan after a long public hearing.</p><div class='share-tools'>Share this report</div><section class='comments'>Reader comments</section></article></div><footer>Subscriptions and legal notices</footer></body></html>",
                expected: Some(
                    "City council approves the river plan Councillors approved the river restoration plan after a long public hearing.",
                ),
            },
            Fixture {
                name: "blog",
                html: "<html><body><header><a href='/'>Notebook home</a></header><div class='columns'><article class='post-content'><h1>A small garden in August</h1><p>The tomatoes finally reached the kitchen window after weeks of patient tying.</p><div class='related-posts'><a href='/spring'>Earlier garden notes</a></div></article><aside>Archive categories and newsletter signup</aside></div><footer>Copyright notice</footer></body></html>",
                expected: Some(
                    "A small garden in August The tomatoes finally reached the kitchen window after weeks of patient tying.",
                ),
            },
            Fixture {
                name: "documentation",
                html: "<html><body><header>Product documentation</header><div class='docs-shell'><nav><a href='/install'>Install</a><a href='/api'>API</a></nav><section class='documentation-content'><h1>Configure the reader lane</h1><p>Choose the reader engine from the tile menu to render held HTML as an article.</p></section><aside>On this page and version picker</aside></div><footer>Edit this page</footer></body></html>",
                expected: Some(
                    "Configure the reader lane Choose the reader engine from the tile menu to render held HTML as an article.",
                ),
            },
            Fixture {
                name: "recipe",
                html: "<html><body><header><nav>Breakfast Lunch Dinner</nav></header><main class='recipe-content'><h1>Weeknight lentil soup</h1><div class='promo'>Save this recipe and join the club</div><p>Simmer the lentils with onion and stock until tender, then finish with lemon.</p><section class='related-recipes'>More soups and stews</section></main><footer>Kitchen index</footer></body></html>",
                expected: Some(
                    "Weeknight lentil soup Simmer the lentils with onion and stock until tender, then finish with lemon.",
                ),
            },
            Fixture {
                name: "forum-thread",
                html: "<html><body><header><nav>Forums Members Search</nav></header><main id='thread-content'><h1>Repairing a noisy tape machine</h1><div class='post'><p>Cleaning the capstan solved the flutter, while a new belt fixed the remaining drift.</p></div><div class='comment controls'>Reply Quote Report</div></main><aside>Recent discussions and active users</aside></body></html>",
                expected: Some(
                    "Repairing a noisy tape machine Cleaning the capstan solved the flutter, while a new belt fixed the remaining drift.",
                ),
            },
            Fixture {
                name: "essay",
                html: "<html><body><header>Independent essays</header><div class='layout'><div class='article-content'><h1>Notes on durable interfaces</h1><p>An interface becomes durable when a second consumer proves the abstraction under different pressure.</p></div><div class='promo-card'>Support the magazine</div><aside>Contents and author archive</aside></div><footer>Colophon</footer></body></html>",
                expected: Some(
                    "Notes on durable interfaces An interface becomes durable when a second consumer proves the abstraction under different pressure.",
                ),
            },
            Fixture {
                name: "newspaper-column",
                html: "<html><body><header><nav>News Arts Books</nav></header><div id='column-layout'><section class='story-body'><div class='byline'>The Saturday columnist</div><h1>The library at closing time</h1><p>At closing time the reading room gathers its scattered silence into one place.</p><div class='share'>Email Print Share</div></section><aside>Recommended columns</aside></div><footer>Subscriber services</footer></body></html>",
                expected: Some(
                    "The library at closing time At closing time the reading room gathers its scattered silence into one place.",
                ),
            },
            Fixture {
                name: "api-guide",
                html: "<html><body><header>API guide</header><nav><a href='/types'>Types</a><a href='/traits'>Traits</a></nav><article class='documentation'><h1>Resolve links at the boundary</h1><p>Fleece returns raw attributes so each caller resolves addresses against its own source URL.</p><div class='related'>Related functions</div></article><footer>Source and license</footer></body></html>",
                expected: Some(
                    "Resolve links at the boundary Fleece returns raw attributes so each caller resolves addresses against its own source URL.",
                ),
            },
            Fixture {
                name: "food-column",
                html: "<html><body><header><nav>Recipes Techniques Equipment</nav></header><main><h1>Why toast needs patience</h1><div class='author byline'>Test kitchen</div><p>Moderate heat dries the surface before browning and leaves the center pleasantly tender.</p><aside>Recommended toaster settings</aside><div class='comments'>Join the discussion</div></main><footer>Magazine links</footer></body></html>",
                expected: Some(
                    "Why toast needs patience Moderate heat dries the surface before browning and leaves the center pleasantly tender.",
                ),
            },
            Fixture {
                name: "travel",
                html: "<html><body><header>Travel journal</header><div class='page-grid'><section class='post-content'><h1>Walking the old canal</h1><p>The towpath follows stone locks, quiet warehouses, and gardens built above the water.</p><div class='social-share'>Share itinerary</div></section><aside>Map, lodging, and nearby trips</aside></div><footer>Travel archive</footer></body></html>",
                expected: Some(
                    "Walking the old canal The towpath follows stone locks, quiet warehouses, and gardens built above the water.",
                ),
            },
            Fixture {
                name: "science",
                html: "<html><body><header><nav>Research Field notes Collections</nav></header><article id='research-story'><h1>A closer look at moth wings</h1><p>Microscopic scales scatter light in patterns that change as the viewing angle shifts.</p><section class='related-research'>More microscopy reports</section></article><aside>Institutional announcements</aside><footer>Data policy</footer></body></html>",
                expected: Some(
                    "A closer look at moth wings Microscopic scales scatter light in patterns that change as the viewing angle shifts.",
                ),
            },
            Fixture {
                name: "opinion",
                html: "<html><body><header>Opinion</header><main class='opinion-body'><div class='byline'>Guest essay</div><h1>Public benches are small infrastructure</h1><p>A place to pause changes who can cross a neighborhood and how long they can remain.</p><div class='share-bar'>Share Comment</div></main><aside>Other opinions</aside><footer>Editorial standards</footer></body></html>",
                expected: Some(
                    "Public benches are small infrastructure A place to pause changes who can cross a neighborhood and how long they can remain.",
                ),
            },
            Fixture {
                name: "changelog",
                html: "<html><body><header><nav>Guide Reference Releases</nav></header><div class='release-layout'><section class='content release-notes'><h1>Version two point four</h1><p>This release adds reader lineage, nested lists, and table headers to exported articles.</p></section><aside>Previous versions and downloads</aside></div><footer>Security policy</footer></body></html>",
                expected: Some(
                    "Version two point four This release adds reader lineage, nested lists, and table headers to exported articles.",
                ),
            },
            Fixture {
                name: "how-to",
                html: "<html><body><header>Practical testing</header><nav>Basics Tools Examples</nav><article class='how-to article-body'><h1>Label a reader fixture</h1><p>Keep the expected article words separate from navigation labels and promotional furniture.</p><div class='promo'>Download the complete handbook</div></article><aside>Table of contents</aside><footer>Feedback</footer></body></html>",
                expected: Some(
                    "Label a reader fixture Keep the expected article words separate from navigation labels and promotional furniture.",
                ),
            },
            Fixture {
                name: "profile",
                html: "<html><body><header><nav>People Places Work</nav></header><div id='feature-layout'><div id='story' class='article-content'><h1>The instrument repairer</h1><p>For thirty years she has restored concertinas whose reeds arrived bent, dusty, or silent.</p><div class='related'>More maker profiles</div></div><aside>Portrait gallery</aside></div><footer>About this series</footer></body></html>",
                expected: Some(
                    "The instrument repairer For thirty years she has restored concertinas whose reeds arrived bent, dusty, or silent.",
                ),
            },
            Fixture {
                name: "interview",
                html: "<html><body><header>Conversations</header><main><h1>A conversation about local software</h1><div class='byline'>Recorded interview</div><p>The useful question is where authority lives when the network disappears.</p><section class='comments'>Transcript corrections and reader notes</section></main><aside>Recent interviews</aside><footer>Podcast feeds</footer></body></html>",
                expected: Some(
                    "A conversation about local software The useful question is where authority lives when the network disappears.",
                ),
            },
            Fixture {
                name: "review",
                html: "<html><body><header><nav>Music Film Books</nav></header><div class='review-page'><section class='article-content review-body'><h1>A patient record of winter songs</h1><p>The album favors room sound, spare arrangements, and performances allowed to breathe.</p><div class='share'>Share review</div></section><aside>Score, credits, and related records</aside></div><footer>Review policy</footer></body></html>",
                expected: Some(
                    "A patient record of winter songs The album favors room sound, spare arrangements, and performances allowed to breathe.",
                ),
            },
            Fixture {
                name: "dispatch",
                html: "<html><body><header>Field dispatches</header><div class='site-grid'><article class='dispatch story-body'><h1>Dispatch from the shoreline</h1><p>Morning fog hid the opposite bank while fishing boats moved slowly through the channel.</p><div class='author byline'>Filed from the coast</div><div class='related'>Earlier dispatches</div></article><aside>Weather and tide table</aside></div><footer>Archive</footer></body></html>",
                expected: Some(
                    "Dispatch from the shoreline Morning fog hid the opposite bank while fishing boats moved slowly through the channel.",
                ),
            },
            Fixture {
                name: "spa-shell",
                html: "<html><head><title>Client application</title></head><body><header><nav><a href='/'>Home</a></nav></header><main id='app'><div class='loading'>Loading</div></main><script>renderArticleLater()</script><footer>Application shell</footer></body></html>",
                expected: None,
            },
            Fixture {
                name: "link-index",
                html: "<html><body><header>Topic directory</header><nav>Browse A to Z</nav><div class='content link-index'><h1>All destinations</h1><a href='/1'>A long directory label for the first destination</a><a href='/2'>A long directory label for the second destination</a><a href='/3'>A long directory label for the third destination</a><a href='/4'>A long directory label for the fourth destination</a></div><footer>Directory help</footer></body></html>",
                expected: None,
            },
        ];
        assert_eq!(fixtures.len(), 20);

        let mut true_positive = 0usize;
        let mut false_positive = 0usize;
        let mut false_negative = 0usize;
        for fixture in fixtures {
            let actual = extract_main_text(&StaticDocument::parse(fixture.html));
            match fixture.expected {
                None => assert_eq!(
                    actual, None,
                    "{} should not read as an article",
                    fixture.name
                ),
                Some(expected) => {
                    let actual =
                        actual.unwrap_or_else(|| panic!("{} lost its article", fixture.name));
                    let expected_words = word_set(expected);
                    let actual_words = word_set(&actual);
                    true_positive += actual_words.intersection(&expected_words).count();
                    false_positive += actual_words.difference(&expected_words).count();
                    false_negative += expected_words.difference(&actual_words).count();
                },
            }
        }
        let precision = true_positive as f64 / (true_positive + false_positive) as f64;
        let recall = true_positive as f64 / (true_positive + false_negative) as f64;
        eprintln!(
            "fleece labelled corpus: 20 pages; precision={precision:.3}; recall={recall:.3}; tp={true_positive}; fp={false_positive}; fn={false_negative}"
        );
        assert!(precision.is_finite() && recall.is_finite());
    }

    fn word_set(text: &str) -> std::collections::BTreeSet<String> {
        text.split(|character: char| !character.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .map(str::to_ascii_lowercase)
            .collect()
    }
}
