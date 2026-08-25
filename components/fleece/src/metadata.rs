/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use layout_dom_api::LayoutDom;

use crate::{attr, local_name};

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
