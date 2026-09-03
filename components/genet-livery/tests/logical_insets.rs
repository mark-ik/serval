// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Logical inset, size, and margin longhands reach used geometry.
//!
//! Found 2026-08-22 while measuring the K5 residual matrix: the 32
//! `css-shapes/shape-outside` files that turned red when the 2026-08-21
//! repairs landed are false passes exposed, not regressions. Their
//! *references* place marker boxes with `inset-block-start`,
//! `inset-inline-start`, `block-size`, and `margin-block-end`, all of which
//! sat in Livery's `[[unimplemented]]` catalog table. Every such box resolved
//! `auto` insets, stacked at its static position, and painted as one solid
//! block, which happened to match an equally wrong test side.
//!
//! These fixtures assert the projection end to end — cascade, computed style,
//! and used geometry — rather than only `PropertyId::to_physical`, because the
//! catalog entry alone would not have caught a cascade that never applied it.

use genet_livery::{Device, LiveryDocument, StyleSet};
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

/// Lay out `body` through the retained document session and return the
/// `(x, y, width, height)` of each id.
fn rects(body: &str, ids: &[&str]) -> Vec<(f32, f32, f32, f32)> {
    let html = format!("<html><body style=\"margin:0\">{body}</body></html>");
    let mut session = LiveryDocument::new(
        StaticDocument::parse(&html),
        StyleSet::cambium(&[]),
        Device::screen(800.0, 600.0),
    );
    session.frame(800, 600).expect("frame");
    ids.iter()
        .map(|name| {
            let id = find(session.dom(), session.dom().document(), name).expect(name);
            let [x, y, width, height] = session
                .fragment_rect(id)
                .unwrap_or_else(|| panic!("{name} has a fragment"));
            (x, y, width, height)
        })
        .collect()
}

/// One absolutely positioned box in a 200x200 containing block, styled by
/// `decl`, reported relative to that containing block.
fn positioned(writing_mode: &str, decl: &str) -> (f32, f32, f32, f32) {
    let body = format!(
        "<div id=\"cb\" style=\"position:relative; width:200px; height:200px; {writing_mode}\">\
           <div id=\"box\" style=\"position:absolute; width:40px; height:30px; {decl}\"></div>\
         </div>"
    );
    let got = rects(&body, &["cb", "box"]);
    let (cb, b) = (got[0], got[1]);
    (b.0 - cb.0, b.1 - cb.1, b.2, b.3)
}

#[test]
fn inset_block_start_matches_top_in_horizontal_tb() {
    let logical = positioned("", "inset-block-start: 50px");
    let physical = positioned("", "top: 50px");
    assert_eq!(
        logical, physical,
        "inset-block-start is top in horizontal-tb"
    );
    assert_eq!(logical.1, 50.0, "and it actually moved the box {logical:?}");
}

#[test]
fn inset_inline_start_follows_direction() {
    let ltr = positioned("direction:ltr", "inset-inline-start: 60px");
    assert_eq!(ltr.0, 60.0, "ltr inline-start is the left edge {ltr:?}");

    let rtl = positioned("direction:rtl", "inset-inline-start: 60px");
    // Inline-start is the right edge, so the box's left edge sits
    // 200 - 60 - 40 = 100 from the containing block's left.
    assert_eq!(rtl.0, 100.0, "rtl inline-start is the right edge {rtl:?}");
}

#[test]
fn inset_block_start_follows_writing_mode() {
    // vertical-rl stacks blocks right to left, so block-start is the right
    // edge: 200 - 50 - 40 = 110.
    let vertical = positioned("writing-mode:vertical-rl", "inset-block-start: 50px");
    assert_eq!(
        vertical.0, 110.0,
        "vertical-rl block-start is the right edge {vertical:?}"
    );
    assert_eq!(
        vertical.1, 0.0,
        "and it did not move in the physical block axis"
    );

    let vertical_lr = positioned("writing-mode:vertical-lr", "inset-block-start: 50px");
    assert_eq!(
        vertical_lr.0, 50.0,
        "vertical-lr block-start is the left edge {vertical_lr:?}"
    );
}

#[test]
fn block_size_maps_to_the_block_axis() {
    let horizontal = positioned("", "block-size: 90px");
    assert_eq!(
        horizontal.3, 90.0,
        "horizontal-tb block axis is height {horizontal:?}"
    );

    let vertical = positioned("writing-mode:vertical-rl", "block-size: 90px");
    assert_eq!(
        vertical.2, 90.0,
        "vertical-rl block axis is width {vertical:?}"
    );
}

#[test]
fn margin_block_end_maps_to_the_block_end_side() {
    // An in-flow box whose block-end margin pushes the following sibling.
    let body = "<div id=\"first\" style=\"height:20px; margin-block-end:30px\"></div>\
                <div id=\"second\" style=\"height:20px\"></div>";
    let got = rects(body, &["first", "second"]);
    assert_eq!(
        got[1].1, 50.0,
        "margin-block-end is margin-bottom in horizontal-tb {got:?}"
    );
}

#[test]
fn a_later_physical_declaration_wins_over_an_earlier_logical_one() {
    // CSS Logical Properties resolves same-group declarations in cascade
    // order, so the physical longhand written second must win.
    let both = positioned("", "inset-block-start: 50px; top: 10px");
    assert_eq!(both.1, 10.0, "the later physical declaration wins {both:?}");

    let reversed = positioned("", "top: 10px; inset-block-start: 50px");
    assert_eq!(
        reversed.1, 50.0,
        "the later logical declaration wins {reversed:?}"
    );
}

/// The exact shape the 32 `shape-outside` references are built from: boxes
/// positioned by logical insets inside an absolutely positioned vertical-rl
/// container. Before the catalog entries landed, every box collapsed onto the
/// same static position.
#[test]
fn shape_reference_shape_places_each_box_distinctly() {
    let body = "<div id=\"container\" style=\"writing-mode:vertical-rl; position:absolute; inline-size:200px\">\
        <div id=\"a\" style=\"position:absolute; inline-size:60px; block-size:20px; inset-block-start:0; inset-inline-start:0\"></div>\
        <div id=\"b\" style=\"position:absolute; inline-size:60px; block-size:12px; inset-block-start:20px; inset-inline-start:96px\"></div>\
        <div id=\"c\" style=\"position:absolute; inline-size:60px; block-size:36px; inset-block-start:44px; inset-inline-start:120px\"></div>\
      </div>";
    let got = rects(body, &["a", "b", "c"]);
    assert_ne!(got[0], got[1], "a and b must not coincide: {got:?}");
    assert_ne!(got[1], got[2], "b and c must not coincide: {got:?}");
    // vertical-rl: the inline axis is vertical, so inset-inline-start is a y
    // offset and block-size is a width.
    assert_eq!(got[0].1, 0.0, "a at inline-start 0 {:?}", got[0]);
    assert_eq!(got[1].1, 96.0, "b at inline-start 96 {:?}", got[1]);
    assert_eq!(got[2].1, 120.0, "c at inline-start 120 {:?}", got[2]);
    assert_eq!(got[1].2, 12.0, "b's block-size is its width {:?}", got[1]);
}

/// `inset: 0` with `margin: auto` centres a definite-inline-size box, whatever
/// resolves its block size.
///
/// Recorded 2026-08-22 from `css-sizing/div-*-auto-margin*.tentative.html`.
/// The first diagnosis blamed the positioned solver, wrongly:
/// `solve_positioned_box` centres identically for `Length` and `MaxContent`
/// when the insets are definite. The real cause was that the `inset`
/// shorthand was unimplemented, so `inset: 0` never reached the longhands and
/// every box fell back to its static position, which *does* differ between
/// the definite and intrinsic block-size paths.
#[test]
fn auto_margins_centre_under_inset_zero_for_every_block_size() {
    for decl in [
        "height: 200px",
        "height: max-content",
        "block-size: 200px",
        "block-size: max-content",
    ] {
        let body = format!(
            "<div id=\"cb\" style=\"position:relative; width:400px; height:400px\">\
               <div id=\"box\" style=\"position:absolute; inset:0; margin:auto; width:100px; {decl}\">\
                 <div style=\"height:200px\"></div>\
               </div>\
             </div>"
        );
        let got = rects(&body, &["cb", "box"]);
        let offset = got[1].0 - got[0].0;
        eprintln!(
            "{decl:24} -> x offset {offset}, size {:?}",
            (got[1].2, got[1].3)
        );
        assert_eq!(
            offset, 150.0,
            "{decl}: auto margins centre a 100px box in 400px"
        );
    }
}

/// The `inset` shorthand expands to all four physical longhands, in the
/// one-to-four value pattern the box shorthands share.
#[test]
fn inset_shorthand_expands_to_four_sides() {
    let one = positioned("", "inset: 10px");
    assert_eq!(
        (one.0, one.1),
        (10.0, 10.0),
        "one value sets every side {one:?}"
    );

    // Two values are block then inline: `inset: 10px 40px` puts the box 10
    // from the top and 40 from the left.
    let two = positioned("", "inset: 10px 40px");
    assert_eq!(
        (two.0, two.1),
        (40.0, 10.0),
        "two values are block/inline {two:?}"
    );

    // Four values run top, right, bottom, left.
    let four = positioned("", "inset: 5px 0 0 25px");
    assert_eq!(
        (four.0, four.1),
        (25.0, 5.0),
        "four values are TRBL {four:?}"
    );

    // An invalid component leaves the whole declaration out rather than
    // applying a partial expansion.
    let invalid = positioned("", "inset: 10px nonsense");
    let untouched = positioned("", "");
    assert_eq!(
        invalid, untouched,
        "an invalid inset is dropped whole {invalid:?}"
    );
}
