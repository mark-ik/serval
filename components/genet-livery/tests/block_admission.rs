//! Block-formatter admission of independent formatting contexts (lane 8,
//! 2026-08-21).
//!
//! A table, flow-root, or scroll container under ordinary blocks is an opaque
//! block child of Buckram's owned block formatter: it is laid out by its own
//! algorithm and contributes its border box and margins to the parent's
//! flow, so its block ancestors no longer fall back to Taffy. Numbers come
//! from the same `layout` entry the runner uses.

use genet_livery::{
    BlockAlgorithmCounts, Device, InteractionStates, StyleSet, layout, resolve_styles,
};
use genet_static_dom::StaticDocument;
use layout_dom_api::{LayoutDom, LocalName, Namespace, NodeKind};

fn find(
    dom: &StaticDocument,
    node: <StaticDocument as LayoutDom>::NodeId,
    needle: &str,
) -> Option<<StaticDocument as LayoutDom>::NodeId> {
    if dom.kind(node) == NodeKind::Element
        && dom.attribute(node, &Namespace::from(""), &LocalName::from("id")) == Some(needle)
    {
        return Some(node);
    }
    dom.dom_children(node)
        .find_map(|child| find(dom, child, needle))
}

/// Lay out `html` and return the block algorithm census plus each id's
/// outermost physical rectangle as `(x, y, width, height)`.
fn census_and_rects(html: &str, ids: &[&str]) -> (BlockAlgorithmCounts, Vec<(f32, f32, f32, f32)>) {
    let document = StaticDocument::parse(html);
    let styles = resolve_styles(
        &document,
        &StyleSet::cambium(&[]),
        &Device::screen(800.0, 600.0),
        &InteractionStates::default(),
    );
    let fragments = layout(&document, &styles, 800.0, 600.0).expect("layout");
    let rects = ids
        .iter()
        .map(|name| {
            let id = find(&document, document.document(), name).expect(name);
            let rect = fragments
                .get(id)
                .map(|fragment| fragment.physical_rect())
                .unwrap_or_else(|| panic!("{name} has a fragment"));
            (rect.x, rect.y, rect.width, rect.height)
        })
        .collect();
    (fragments.block_algorithm_counts(), rects)
}

fn assert_no_css_facing_taffy(census: BlockAlgorithmCounts) {
    assert_eq!(census.css_facing_taffy(), 0, "{census:?}");
    assert_eq!(
        census.taffy, census.backend_sizing,
        "every remaining Taffy block is scratch measurement {census:?}"
    );
}

/// The shape `plain_in_flow_table_block_algorithm_census` recorded as
/// evidence: every block ancestor of an in-flow table ran on Taffy. Now none
/// does, and the sibling after the table still sits below it.
#[test]
fn a_table_under_nested_blocks_keeps_every_block_ancestor_on_buckram() {
    let html = "<html><body style=\"margin:0\">\
        <div id=\"outer\"><div id=\"inner\">\
          <table id=\"table\" style=\"border-collapse:collapse\"><tbody>\
            <tr><td style=\"padding:0\"><div style=\"width:50px; height:20px\"></div></td></tr>\
          </tbody></table>\
        </div><div id=\"after\" style=\"height:10px\"></div></div>\
        </body></html>";
    let (census, got) = census_and_rects(html, &["outer", "inner", "table", "after"]);
    assert_no_css_facing_taffy(census);
    assert!(census.buckram >= 4, "{census:?}");
    let (_outer, inner, table, after) = (got[0], got[1], got[2], got[3]);
    assert_eq!(
        table.1, inner.1,
        "the table starts at its parent's top {table:?}"
    );
    assert_eq!(table.3, 20.0, "{table:?}");
    assert_eq!(
        after.1,
        table.1 + 20.0,
        "the sibling follows the table {after:?}"
    );
}

/// CSS Tables 3 section 2.2.1: the wrapper is as wide as the grid's border
/// edge, so the table's `margin: 0 auto` centers it in a 400px host.
#[test]
fn an_auto_margin_table_is_centered_by_its_grid_width() {
    let html = "<html><body style=\"margin:0\">\
        <div id=\"host\" style=\"width:400px\">\
          <table id=\"table\" style=\"margin:0 auto; border-collapse:collapse\"><tbody>\
            <tr><td style=\"padding:0\"><div style=\"width:100px; height:20px\"></div></td></tr>\
          </tbody></table>\
        </div></body></html>";
    let (census, got) = census_and_rects(html, &["host", "table"]);
    assert_no_css_facing_taffy(census);
    let (host, table) = (got[0], got[1]);
    assert_eq!(
        table.2, 100.0,
        "the wrapper takes the grid's width {table:?}"
    );
    assert_eq!(
        table.0,
        host.0 + 150.0,
        "auto margins split the remaining 300px {table:?}"
    );
}

/// A caption's containing block is the wrapper, so it is exactly as wide as
/// the grid, not as wide as the wrapper's containing block.
#[test]
fn a_caption_is_as_wide_as_the_table_wrapper() {
    let html = "<html><body style=\"margin:0\">\
        <table id=\"table\" style=\"border-collapse:collapse\">\
          <caption id=\"caption\" style=\"margin:0; padding:0; text-align:left\">c</caption>\
          <tbody><tr><td style=\"padding:0\"><div style=\"width:100px; height:20px\"></div></td></tr></tbody>\
        </table></body></html>";
    let (census, got) = census_and_rects(html, &["table", "caption"]);
    assert_no_css_facing_taffy(census);
    let (table, caption) = (got[0], got[1]);
    assert_eq!(table.2, 100.0, "{table:?}");
    assert_eq!(caption.2, 100.0, "{caption:?}");
}

/// A flow-root whose own contents defer (an intrinsic keyword width) runs on
/// Taffy by itself; its parent stays on Buckram and places it with its
/// margins collapsed against the previous sibling.
#[test]
fn a_deferred_flow_root_does_not_defer_its_block_ancestors() {
    let html = "<html><body style=\"margin:0\">\
        <div id=\"before\" style=\"height:40px; margin-bottom:30px\"></div>\
        <div id=\"root\" style=\"display:flow-root; margin-top:10px; margin-bottom:20px\">\
          <div style=\"width:min-content; height:30px\">x</div>\
        </div>\
        <div id=\"after\" style=\"height:10px\"></div>\
        </body></html>";
    let (census, got) = census_and_rects(html, &["before", "root", "after"]);
    assert!(
        census.buckram >= 2,
        "html and body stay on Buckram {census:?}"
    );
    let (before, root, after) = (got[0], got[1], got[2]);
    assert_eq!(before.1, 0.0, "{before:?}");
    assert_eq!(root.1, 70.0, "40px + max(30px, 10px) {root:?}");
    assert_eq!(root.3, 30.0, "{root:?}");
    assert_eq!(after.1, 120.0, "70px + 30px + 20px {after:?}");
}
