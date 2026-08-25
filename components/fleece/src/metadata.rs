/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use layout_dom_api::LayoutDom;

use crate::{attr, local_name};

const HTML_NAMESPACE: &str = "http://www.w3.org/1999/xhtml";

/// One grouped Open Graph root property and the structured properties that
/// immediately follow it in the observed DOM order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenGraphGroup {
    pub property: String,
    pub value: String,
    pub structured: Vec<(String, String)>,
}

/// A DOM `<link>` observed by Fleece. Values remain raw and unresolved;
/// HTTP `Link` headers are outside this extraction contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentLink {
    /// HTML-space-tokenized relations. Registered names are ASCII-folded;
    /// extension relation IRIs retain their identity.
    pub rel: Vec<String>,
    pub href: Option<String>,
    pub type_: Option<String>,
    pub hreflang: Option<String>,
    pub title: Option<String>,
    pub media: Option<String>,
    /// Other observable attributes in DOM attribute order.
    pub other: Vec<(String, String)>,
}

impl DocumentLink {
    /// Compare a relation token without changing the spelling retained in
    /// [`Self::rel`]. Registered names and extension relation IRIs are both
    /// compared ASCII-case-insensitively.
    pub fn has_relation(&self, relation: &str) -> bool {
        self.rel
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(relation))
    }
}

/// The document's self-description. Fleece observes DOM metadata only and
/// resolves no URLs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Metadata {
    pub description: Option<String>,
    /// First qualifying DOM link's raw canonical href.
    pub canonical: Option<String>,
    /// Ordered raw Open Graph pairs with `og:` stripped.
    pub open_graph: Vec<(String, String)>,
    pub open_graph_groups: Vec<OpenGraphGroup>,
    /// HTML `<link>` elements in document order. HTTP `Link` headers are absent.
    pub links: Vec<DocumentLink>,
}

/// Extract page metadata, Open Graph pairs/groups, and HTML DOM links.
pub fn extract_metadata<D: LayoutDom>(dom: &D) -> Metadata {
    let mut md = Metadata::default();
    walk_metadata(dom, dom.document(), &mut md);
    md.open_graph_groups = group_open_graph(&md.open_graph);
    md
}

fn walk_metadata<D: LayoutDom>(dom: &D, id: D::NodeId, md: &mut Metadata) {
    if is_html_element(dom, id) {
        match local_name(dom, id) {
            Some("meta") => {
                if let Some(prop) = attr(dom, id, "property") {
                    if let Some(key) = prop.strip_prefix("og:") {
                        if let Some(content) = attr(dom, id, "content") {
                            md.open_graph.push((key.to_owned(), content));
                        }
                    }
                } else if attr(dom, id, "name").is_some_and(|name| name == "description")
                    && md.description.is_none()
                {
                    md.description = attr(dom, id, "content").filter(|value| !value.is_empty());
                }
            },
            Some("link") => {
                let link = document_link(dom, id);
                if md.canonical.is_none() && link.rel.iter().any(|relation| relation == "canonical")
                {
                    md.canonical = link.href.clone().filter(|href| !href.is_empty());
                }
                md.links.push(link);
            },
            _ => {},
        }
    }
    for child in dom.dom_children(id) {
        walk_metadata(dom, child, md);
    }
}

fn document_link<D: LayoutDom>(dom: &D, id: D::NodeId) -> DocumentLink {
    let known = ["rel", "href", "type", "hreflang", "title", "media"];
    let other = dom
        .attributes(id)
        .filter_map(|attribute| {
            let name = attribute.name.local.as_ref();
            (!known.contains(&name)).then(|| (name.to_owned(), attribute.value.to_owned()))
        })
        .collect();
    DocumentLink {
        rel: attr(dom, id, "rel")
            .map(|value| html_space_tokens(&value).map(normalize_relation).collect())
            .unwrap_or_default(),
        href: attr(dom, id, "href"),
        type_: attr(dom, id, "type"),
        hreflang: attr(dom, id, "hreflang"),
        title: attr(dom, id, "title"),
        media: attr(dom, id, "media"),
        other,
    }
}

/// HTML space-separated tokens use exactly U+0009, U+000A, U+000C, U+000D,
/// and U+0020, rather than every Unicode whitespace character.
fn html_space_tokens(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(|character| {
            matches!(
                character,
                '\u{0009}' | '\u{000A}' | '\u{000C}' | '\u{000D}' | ' '
            )
        })
        .filter(|token| !token.is_empty())
}

fn normalize_relation(value: &str) -> String {
    if value.contains(':') {
        value.to_owned()
    } else {
        value.to_ascii_lowercase()
    }
}

fn group_open_graph(pairs: &[(String, String)]) -> Vec<OpenGraphGroup> {
    let mut groups: Vec<OpenGraphGroup> = Vec::new();
    for (property, value) in pairs {
        if let Some((root, suffix)) = property.split_once(':') {
            if let Some(group) = groups.last_mut() {
                if group.property == root {
                    group.structured.push((suffix.to_owned(), value.clone()));
                }
            }
            // Keep malformed/orphan structured properties in raw evidence,
            // without fabricating a grouped root for them.
            continue;
        }
        groups.push(OpenGraphGroup {
            property: property.clone(),
            value: value.clone(),
            structured: Vec::new(),
        });
    }
    groups
}

fn is_html_element<D: LayoutDom>(dom: &D, id: D::NodeId) -> bool {
    dom.element_name(id)
        .is_some_and(|name| name.ns.as_ref() == HTML_NAMESPACE)
}
