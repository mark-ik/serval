//! Clip semantics shared by the document lanes that retain a DOM:
//! selection ranges, the structural content report, and the semantic
//! clip projections the Livery and scripted sessions both serve.

use document_session_api::session_engine::DocumentClip;
use layout_dom_api::{LayoutDom, LocalName, Namespace, NodeKind};

#[derive(Clone, Copy)]
pub(crate) struct ClipRange<Id> {
    pub(crate) anchor_node: Id,
    pub(crate) anchor_offset: usize,
    pub(crate) focus_node: Id,
    pub(crate) focus_offset: usize,
}

pub(crate) struct ClipSelection<Id> {
    pub(crate) range: ClipRange<Id>,
    pub(crate) text: String,
}

pub(crate) fn content_report<D: LayoutDom>(dom: &D) -> document_session_api::ContentReport {
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
        report: &mut document_session_api::ContentReport,
    ) {
        let mut child_depth = depth;
        if let Some(tag) = dom.element_name(node).map(|name| name.local.to_string()) {
            if !matches!(
                tag.as_str(),
                "head" | "style" | "script" | "title" | "meta" | "link" | "base" | "html"
            ) {
                report.outline.push(document_session_api::OutlineEntry {
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

    let mut report = document_session_api::ContentReport::default();
    walk(dom, dom.document(), 0, &mut report);
    report
}

pub(crate) fn semantic_clip_from_dom<D: LayoutDom>(address: &str, dom: &D) -> Option<DocumentClip> {
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

pub(crate) fn semantic_clip_from_selection_with_links<D>(
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
pub(crate) fn links_for_source_nodes<D>(dom: &D, sources: &[D::NodeId]) -> Vec<String>
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
