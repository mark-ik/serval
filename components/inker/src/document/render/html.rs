// Copyright 2026 Mark AB (markik)
// SPDX-License-Identifier: MIT OR Apache-2.0

//! HTML exporter — the universal-browser projection beside `to_markdown` /
//! `to_gemini` / `to_knot` / `to_gophermap` / `to_text`.
//!
//! Emits a body **fragment**, not a page: no doctype, `<html>`, `<head>`,
//! or styling. The caller owns the page shell. Like the sibling exporters
//! it renders blocks only — the title lives in frontmatter (`to_knot`) or
//! the document's own heading, never invented here.
//!
//! This is the projection that lets a knot travel to clients with no
//! nematic engine (a stock phone browser): protocol blocks that were
//! expanded from gemtext / gopher / feed fences render as ordinary HTML
//! while the authored knot keeps them in their idiomatic form.
//!
//! Mapping rules, documented where they bite:
//!
//! - All text and attribute values are HTML-escaped; a document can never
//!   inject markup through content.
//! - Semantic variants keep their intent as class names (`feed-header`,
//!   `feed-entry`, `metadata-row`, `badge`) so a page shell can style them;
//!   `MetadataRow` renders as a definition-list row, matching the
//!   projection guidance on the block itself.
//! - A `Link`'s open predicate (statements-over-schema `rel`) is carried as
//!   a `data-predicate` attribute — meaning is preserved without inventing
//!   an HTML `rel` token the IRI isn't.
//! - Table column alignment becomes an inline `text-align` style, the only
//!   styling this exporter emits.

use super::super::{Block, EngineDocument, InlineSpan, TableAlignment};

impl EngineDocument {
    /// Render the document as an HTML body fragment. See the module docs
    /// for the mapping rules.
    pub fn to_html(&self) -> String {
        let mut out = String::new();
        for block in &self.blocks {
            write_html_block(block, &mut out);
        }
        out
    }
}

/// Escape text content: `&`, `<`, `>`.
fn escape_text(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
}

/// Escape an attribute value: text escapes plus `"`.
fn escape_attr(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
}

fn write_html_block(block: &Block, out: &mut String) {
    match block {
        Block::Heading { level, spans } => {
            let level = (*level).clamp(1, 6);
            out.push_str(&format!("<h{level}>"));
            write_inline_html(spans, out);
            out.push_str(&format!("</h{level}>\n"));
        },
        Block::Paragraph { spans } => {
            out.push_str("<p>");
            write_inline_html(spans, out);
            out.push_str("</p>\n");
        },
        Block::CodeBlock { language, text } => {
            out.push_str("<pre><code");
            if let Some(lang) = language {
                out.push_str(" class=\"language-");
                escape_attr(lang, out);
                out.push('"');
            }
            out.push('>');
            escape_text(text, out);
            out.push_str("</code></pre>\n");
        },
        Block::Quote { blocks } => {
            out.push_str("<blockquote>\n");
            for inner in blocks {
                write_html_block(inner, out);
            }
            out.push_str("</blockquote>\n");
        },
        Block::List { ordered, items } => {
            out.push_str(if *ordered { "<ol>\n" } else { "<ul>\n" });
            for item in items {
                out.push_str("<li>");
                for inner in item {
                    write_html_block(inner, out);
                }
                out.push_str("</li>\n");
            }
            out.push_str(if *ordered { "</ol>\n" } else { "</ul>\n" });
        },
        Block::Image { url, alt } => {
            out.push_str("<img src=\"");
            escape_attr(url, out);
            out.push_str("\" alt=\"");
            escape_attr(alt, out);
            out.push_str("\">\n");
        },
        Block::Preformatted { text } => {
            out.push_str("<pre>");
            escape_text(text, out);
            out.push_str("</pre>\n");
        },
        Block::Rule => out.push_str("<hr>\n"),
        Block::FeedHeader {
            title,
            subtitle,
            summary,
            source_url,
        } => {
            out.push_str("<header class=\"feed-header\">\n<h1>");
            escape_text(title, out);
            out.push_str("</h1>\n");
            if let Some(subtitle) = subtitle {
                out.push_str("<h2>");
                escape_text(subtitle, out);
                out.push_str("</h2>\n");
            }
            if let Some(summary) = summary {
                out.push_str("<p>");
                escape_text(summary, out);
                out.push_str("</p>\n");
            }
            if let Some(url) = source_url {
                write_bare_link(url, "Open source", out);
            }
            out.push_str("</header>\n");
        },
        Block::FeedEntry {
            title,
            date,
            summary,
            article_url,
            source_url,
        } => {
            out.push_str("<article class=\"feed-entry\">\n<h2>");
            escape_text(title, out);
            out.push_str("</h2>\n");
            if let Some(date) = date {
                out.push_str("<p class=\"feed-date\"><em>");
                escape_text(date, out);
                out.push_str("</em></p>\n");
            }
            if let Some(summary) = summary {
                out.push_str("<p>");
                escape_text(summary, out);
                out.push_str("</p>\n");
            }
            if let Some(url) = article_url {
                write_bare_link(url, "Open article", out);
            }
            if let Some(url) = source_url {
                write_bare_link(url, "Open source", out);
            }
            out.push_str("</article>\n");
        },
        Block::MetadataRow { label, value } => {
            out.push_str("<dl class=\"metadata-row\"><dt>");
            escape_text(label, out);
            out.push_str("</dt><dd>");
            escape_text(value, out);
            out.push_str("</dd></dl>\n");
        },
        Block::Badge { text } => {
            out.push_str("<p class=\"badge\"><em>");
            escape_text(text, out);
            out.push_str("</em></p>\n");
        },
        Block::Table {
            alignments,
            header,
            rows,
        } => write_html_table(alignments, header, rows, out),
    }
}

/// A link on its own paragraph line — the feed blocks' "Open …" links.
fn write_bare_link(url: &str, label: &str, out: &mut String) {
    out.push_str("<p><a href=\"");
    escape_attr(url, out);
    out.push_str("\">");
    escape_text(label, out);
    out.push_str("</a></p>\n");
}

fn alignment_style(alignment: TableAlignment) -> Option<&'static str> {
    match alignment {
        TableAlignment::None => None,
        TableAlignment::Left => Some(" style=\"text-align:left\""),
        TableAlignment::Center => Some(" style=\"text-align:center\""),
        TableAlignment::Right => Some(" style=\"text-align:right\""),
    }
}

fn write_html_table(
    alignments: &[TableAlignment],
    header: &[Vec<InlineSpan>],
    rows: &[Vec<Vec<InlineSpan>>],
    out: &mut String,
) {
    let cell = |tag: &str, col: usize, spans: &[InlineSpan], out: &mut String| {
        out.push('<');
        out.push_str(tag);
        if let Some(style) = alignment_style(alignments.get(col).copied().unwrap_or_default()) {
            out.push_str(style);
        }
        out.push('>');
        write_inline_html(spans, out);
        out.push_str("</");
        out.push_str(tag);
        out.push('>');
    };
    out.push_str("<table>\n");
    if !header.is_empty() {
        out.push_str("<thead><tr>");
        for (i, spans) in header.iter().enumerate() {
            cell("th", i, spans, out);
        }
        out.push_str("</tr></thead>\n");
    }
    out.push_str("<tbody>\n");
    for row in rows {
        out.push_str("<tr>");
        for (i, spans) in row.iter().enumerate() {
            cell("td", i, spans, out);
        }
        out.push_str("</tr>\n");
    }
    out.push_str("</tbody>\n</table>\n");
}

fn write_inline_html(spans: &[InlineSpan], out: &mut String) {
    for span in spans {
        match span {
            InlineSpan::Text(t) => escape_text(t, out),
            InlineSpan::Code(t) => {
                out.push_str("<code>");
                escape_text(t, out);
                out.push_str("</code>");
            },
            InlineSpan::Emphasis(s) => {
                out.push_str("<em>");
                write_inline_html(s, out);
                out.push_str("</em>");
            },
            InlineSpan::Strong(s) => {
                out.push_str("<strong>");
                write_inline_html(s, out);
                out.push_str("</strong>");
            },
            InlineSpan::Link {
                url,
                title,
                spans,
                predicate,
            } => {
                out.push_str("<a href=\"");
                escape_attr(url, out);
                out.push('"');
                if let Some(title) = title {
                    out.push_str(" title=\"");
                    escape_attr(title, out);
                    out.push('"');
                }
                if let Some(predicate) = predicate {
                    out.push_str(" data-predicate=\"");
                    escape_attr(predicate, out);
                    out.push('"');
                }
                out.push('>');
                write_inline_html(spans, out);
                out.push_str("</a>");
            },
            InlineSpan::Submit { target, spans } => {
                out.push_str("<a href=\"");
                escape_attr(target, out);
                out.push_str("\" data-interaction=\"submit\">");
                write_inline_html(spans, out);
                out.push_str("</a>");
            },
            InlineSpan::SoftBreak => out.push('\n'),
            InlineSpan::LineBreak => out.push_str("<br>\n"),
        }
    }
}
