// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

use fleece::{
    AnchoredBlock, Article, Block, ExtractionOptions, Inline, extract_document_with_options,
};
use genet_static_dom::StaticDocument;

fn article(html: &str) -> Article {
    let document =
        extract_document_with_options(&parsed(html), ExtractionOptions { quote_context: 8 });
    document.article.expect("fixture should produce an article")
}

fn parsed(body: &str) -> StaticDocument {
    let body = body.replace(
        "</main>",
        "<p>Fleece conformance fixture supplies enough ordinary prose for the reader selector.</p></main>",
    );
    StaticDocument::parse(&format!("<html><body>{body}</body></html>"))
}

fn all_blocks<'a>(blocks: &'a [AnchoredBlock], out: &mut Vec<&'a AnchoredBlock>) {
    for block in blocks {
        out.push(block);
        match &block.block {
            Block::List { items, .. } => {
                for item in items {
                    all_blocks(item, out);
                }
            },
            Block::Quote { blocks } => all_blocks(blocks, out),
            _ => {},
        }
    }
}

fn block_text(block: &Block) -> String {
    fn runs_text(runs: &[Inline], out: &mut String) {
        for run in runs {
            match run {
                Inline::Text(text) | Inline::Code(text) => out.push_str(text),
                Inline::Link { runs, .. } | Inline::Emphasis { runs, .. } => runs_text(runs, out),
            }
        }
    }

    let mut text = String::new();
    match block {
        Block::Heading { runs, .. } | Block::Paragraph { runs } => runs_text(runs, &mut text),
        Block::Code { text: value, .. } => text.push_str(value),
        Block::Figure { alt, caption, .. } => {
            text.push_str(alt);
            if let Some(caption) = caption {
                runs_text(caption, &mut text);
            }
        },
        Block::Table { table } => {
            for row in &table.rows {
                for cell in &row.cells {
                    runs_text(&cell.runs, &mut text);
                }
            }
        },
        Block::List { .. } | Block::Quote { .. } | Block::Rule => {},
    }
    text
}

#[test]
fn canonical_text_decodes_entities_collapses_whitespace_and_separates_adjacent_nodes() {
    let article = article(
        "<main id='content'><p><span>A&nbsp; &amp;</span><span>B</span></p><p>second</p></main>",
    );
    let text = article
        .blocks
        .iter()
        .filter_map(|block| block.anchor.as_ref())
        .map(|anchor| anchor.quote.exact.as_str())
        .collect::<Vec<_>>();
    assert_eq!(&text[..2], ["A & B", "second"]);
    assert!(
        article.blocks[0]
            .anchor
            .as_ref()
            .is_some_and(|anchor| anchor.quote.exact.contains('&'))
    );
}

#[test]
fn coordinates_count_astral_code_points_and_preserve_logical_bidi_order() {
    let article = article("<main id='content'><p>🙂 אבג</p><p>next</p></main>");
    let first = article.blocks[0].anchor.as_ref().expect("paragraph anchor");
    assert_eq!(first.quote.exact, "🙂 אבג");
    assert_eq!(first.position.start, 0);
    assert_eq!(
        first.position.end, 5,
        "emoji, space, and three Hebrew code points"
    );
    assert_eq!(first.quote.exact.chars().count(), 5);
    assert_eq!(article.blocks[1].anchor.as_ref().unwrap().position.start, 6);
}

#[test]
fn context_does_not_split_combining_graphemes() {
    let article = extract_document_with_options(
        &parsed("<main id='content'><p>ab e\u{301}</p><p>cd</p></main>"),
        ExtractionOptions { quote_context: 2 },
    )
    .article
    .unwrap();
    let paragraph = article.blocks[1].anchor.as_ref().unwrap();
    assert_eq!(paragraph.quote.exact, "cd");
    assert_eq!(
        paragraph.quote.prefix, " ",
        "the context budget retains the separator but not half a grapheme"
    );
}

#[test]
fn a_block_boundary_inside_a_cross_node_grapheme_is_not_anchored() {
    let article = article("<main id='content'><p>e</p><p>\u{301}mark</p></main>");
    let second = &article.blocks[1];
    assert_eq!(block_text(&second.block), "\u{301}mark");
    assert!(
        second.anchor.is_none(),
        "the inserted separator and following combining mark form one grapheme"
    );
}

#[test]
fn repeated_quotes_get_context_and_nested_ranges_are_parent_child_ranges() {
    let article =
        article("<main id='content'><blockquote><p>repeat</p></blockquote><p>repeat</p></main>");
    let mut blocks = Vec::new();
    all_blocks(&article.blocks, &mut blocks);
    let quote = blocks
        .iter()
        .find(|block| matches!(block.block, Block::Quote { .. }))
        .expect("quote block");
    let child = blocks
        .iter()
        .find(|block| matches!(block.block, Block::Paragraph { .. }))
        .expect("nested paragraph");
    let quote_anchor = quote.anchor.as_ref().unwrap();
    let child_anchor = child.anchor.as_ref().unwrap();
    assert!(quote_anchor.position.start <= child_anchor.position.start);
    assert!(child_anchor.position.end <= quote_anchor.position.end);
    assert_eq!(quote_anchor.quote.exact, child_anchor.quote.exact);
}

#[test]
fn every_position_slice_equals_its_quote_exact() {
    let extracted = extract_document_with_options(
        &parsed(
            "<main id='content'><h1>Title</h1><p>one <em>two</em> three</p><ul><li>item</li></ul><blockquote><p>quoted</p></blockquote></main>",
        ),
        ExtractionOptions { quote_context: 8 },
    );
    let canonical = extracted.page.text;
    let article = extracted
        .article
        .expect("fixture should produce an article");
    let mut blocks = Vec::new();
    all_blocks(&article.blocks, &mut blocks);
    for anchored in blocks {
        if let Some(anchor) = &anchored.anchor {
            let sliced = canonical
                .chars()
                .skip(anchor.position.start as usize)
                .take((anchor.position.end - anchor.position.start) as usize)
                .collect::<String>();
            assert_eq!(sliced, anchor.quote.exact);
            assert!(anchor.position.start < anchor.position.end);
        }
    }
}

#[test]
fn synthetic_rule_and_image_only_figure_are_unanchored() {
    let article = article(
        "<main id='content'><hr><figure><img src='image.png' alt='diagram'></figure><p>literal caption</p></main>",
    );
    let rule = article
        .blocks
        .iter()
        .find(|block| matches!(block.block, Block::Rule))
        .expect("rule block");
    assert!(rule.anchor.is_none());
    let figure = article
        .blocks
        .iter()
        .find(|block| matches!(block.block, Block::Figure { .. }))
        .expect("figure block");
    assert!(figure.anchor.is_none());
}

#[test]
fn shallow_list_text_around_a_nested_block_is_unanchored() {
    let article = article(
        "<main id='content'><ul><li>before <blockquote><p>nested</p></blockquote> after</li></ul></main>",
    );
    let list = article
        .blocks
        .iter()
        .find(|block| matches!(block.block, Block::List { .. }))
        .expect("list block");
    let Block::List { items, .. } = &list.block else {
        unreachable!();
    };
    let shallow = &items[0][0];
    assert_eq!(
        block_text(&shallow.block)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "),
        "before after"
    );
    assert!(
        shallow.anchor.is_none(),
        "the shallow paragraph skips the nested block and is discontinuous"
    );
}
