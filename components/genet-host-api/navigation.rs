/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Host-owned navigation address resolution.
//!
//! This stays dependency-free and deliberately handles bare local paths and
//! Windows drive paths alongside web-like URLs. Document engines consume the
//! resolved address; they do not own browser history or navigation policy.

/// Resolve a link `href` against the `base` address the document was loaded
/// from. Absolute hrefs pass through. Remote, root-relative, scheme-relative,
/// and bare local paths retain Pelt's established local-first behavior.
pub fn resolve_href(base: &str, href: &str) -> String {
    if has_scheme(href) {
        return href.to_string();
    }

    if let Some((scheme, authority_end)) = remote_origin(base) {
        if let Some(network_path) = href.strip_prefix("//") {
            return format!("{scheme}://{network_path}");
        }
        if href.starts_with('/') {
            return format!("{}{}", &base[..authority_end], href);
        }

        let page_end = base.find(['?', '#']).unwrap_or(base.len());
        if href.starts_with('?') || href.starts_with('#') {
            return format!("{}{}", &base[..page_end], href);
        }
        let page = &base[..page_end];
        let path_start = authority_end.min(page.len());
        if let Some(index) = page[path_start..].rfind('/') {
            let cut = path_start + index + 1;
            return format!("{}{}", &page[..cut], href);
        }
        return format!("{page}/{href}");
    }

    if href.starts_with('/') || href.starts_with('\\') {
        return href.to_string();
    }
    let cut = base.rfind(['/', '\\']).map_or(0, |i| i + 1);
    format!("{}{}", &base[..cut], href)
}

fn remote_origin(base: &str) -> Option<(&str, usize)> {
    let scheme_end = base.find("://")?;
    let scheme = &base[..scheme_end];
    if scheme.is_empty()
        || !scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        return None;
    }
    let after_authority = &base[scheme_end + 3..];
    let authority_len = after_authority
        .find(['/', '?', '#'])
        .unwrap_or(after_authority.len());
    Some((scheme, scheme_end + 3 + authority_len))
}

fn has_scheme(url: &str) -> bool {
    match url.find(':') {
        Some(i) if i > 0 => url[..i]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_local_and_remote_addresses() {
        assert_eq!(resolve_href("docs/a.html", "b.html"), "docs/b.html");
        assert_eq!(resolve_href("a.html", "sub/c.html"), "sub/c.html");
        assert_eq!(
            resolve_href("file:///x/a.html", "b.html"),
            "file:///x/b.html"
        );
        assert_eq!(
            resolve_href("https://example.org/page/index.html", "/site.css?v=1"),
            "https://example.org/site.css?v=1"
        );
        assert_eq!(
            resolve_href("https://example.org", "site.css"),
            "https://example.org/site.css"
        );
        assert_eq!(
            resolve_href("https://example.org/a/b.html?x=1", "image.png"),
            "https://example.org/a/image.png"
        );
        assert_eq!(
            resolve_href(
                "https://example.org/a/b.html",
                "//cdn.example.org/style.css"
            ),
            "https://cdn.example.org/style.css"
        );
    }

    #[test]
    fn passes_absolute_addresses_through() {
        assert_eq!(
            resolve_href("a.html", "https://example.org/p"),
            "https://example.org/p"
        );
        assert_eq!(
            resolve_href("a.html", "data:text/html,<p>x</p>"),
            "data:text/html,<p>x</p>"
        );
        assert_eq!(resolve_href("docs/a.html", "/root.html"), "/root.html");
        assert_eq!(
            resolve_href("docs/a.html", "C:\\pages\\root.html"),
            "C:\\pages\\root.html"
        );
    }
}
