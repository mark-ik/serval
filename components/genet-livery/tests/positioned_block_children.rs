//! Absolutely positioned block children of a relatively positioned block.
//!
//! Found 2026-08-21 while remeasuring `css/CSS2/tables/table-anonymous-objects-059`
//! through `-098`: the family's red/green overlay depends on an absolute box
//! with `top: 0` landing at the top of its relative parent regardless of its
//! sibling order, and on that box taking no normal-flow space whatever it
//! contains. Both fixtures here are the minimal shapes that exposed the two
//! defects; their numbers come from the same `layout` entry the runner uses.

use genet_livery::{Device, InteractionStates, LiveryDocument, StyleSet, layout, resolve_styles};
use genet_static_dom::StaticDocument;
use layout_dom_api::{LayoutDom, LocalName, Namespace, NodeKind};
use paint_list_api::{ColorF, PaintCmd, PaintList};

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

/// Lay out `html` and return each id's physical border rectangle as
/// `(x, y, width, height)`.
fn rects(html: &str, ids: &[&str]) -> Vec<(f32, f32, f32, f32)> {
    let document = StaticDocument::parse(html);
    let styles = resolve_styles(
        &document,
        &StyleSet::cambium(&[]),
        &Device::screen(800.0, 600.0),
        &InteractionStates::default(),
    );
    let fragments = layout(&document, &styles, 800.0, 600.0).expect("layout");
    eprintln!("block algorithms: {:?}", fragments.block_algorithm_counts());
    ids.iter()
        .map(|name| {
            let id = find(&document, document.document(), name).expect(name);
            let rect = fragments
                .get(id)
                .map(|fragment| fragment.physical_rect())
                .unwrap_or_else(|| panic!("{name} has a fragment"));
            (rect.x, rect.y, rect.width, rect.height)
        })
        .collect()
}

/// Lay out `html` through the retained document session the WPT runner and
/// the hosts use, and return each id's fragment rectangle as
/// `(x, y, width, height)`.
fn document_rects(html: &str, ids: &[&str]) -> Vec<(f32, f32, f32, f32)> {
    let mut session = LiveryDocument::new(
        StaticDocument::parse(html),
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

/// The painted glyphs of an absolutely positioned box must sit inside the
/// box's final fragment, not at the static position the inline formatter
/// shaped them at.
#[test]
fn document_session_paints_the_text_of_a_moved_absolute_block_inside_its_fragment() {
    // The second case puts a table in the sibling, which sends the whole
    // block tree through the Taffy fallback; the overlay must paint the same.
    // The remaining cases are the runner probe that first showed the text
    // painting below the sibling, then that probe with one ingredient removed.
    let probe = |body_style: &str, lead: &str, parent_style: &str| {
        format!(
            "<html><body style=\"{body_style}\">{lead}\
            <div id=\"parent\" style=\"position: relative; {parent_style} height: 300px\">\
              <div id=\"sibling\" style=\"color: red\">in-flow red text</div>\
              <div id=\"abs\" style=\"position: absolute; top: 0; color: #00ff00; background: yellow\">GREEN on yellow</div>\
            </div></body></html>"
        )
    };
    let lead = "<p style=\"margin: 0; height: 40px\">lead</p>";
    let cases = [
        (
            "buckram block flow",
            format!(
                "<html><body style=\"margin:0\">\
                <div id=\"parent\" style=\"position:relative; height:300px\">\
                  <div id=\"sibling\">in flow</div>\
                  <div id=\"abs\" style=\"position:absolute; top:0; color:#00ff00; background:#ffff00\">overlay</div>\
                </div></body></html>"
            ),
        ),
        (
            "taffy fallback",
            format!(
                "<html><body style=\"margin:0\">\
                <div id=\"parent\" style=\"position:relative; height:300px\">\
                  <div id=\"sibling\"><table><tr><td>in flow</td></tr></table></div>\
                  <div id=\"abs\" style=\"position:absolute; top:0; color:#00ff00; background:#ffff00\">overlay</div>\
                </div></body></html>"
            ),
        ),
        (
            "runner probe",
            probe(
                "font-family: monospace; margin: 0",
                lead,
                "font-size: 2em; background: #eee;",
            ),
        ),
        (
            "runner probe without the lead p",
            probe(
                "font-family: monospace; margin: 0",
                "",
                "font-size: 2em; background: #eee;",
            ),
        ),
        (
            "runner probe without font-size 2em",
            probe(
                "font-family: monospace; margin: 0",
                lead,
                "background: #eee;",
            ),
        ),
        (
            "runner probe without monospace",
            probe("margin: 0", lead, "font-size: 2em; background: #eee;"),
        ),
    ];
    for (label, html) in cases {
        let mut session = LiveryDocument::new(
            StaticDocument::parse(&html),
            StyleSet::cambium(&[]),
            Device::screen(800.0, 600.0),
        );
        let frame = session.frame(800, 600).expect("frame");
        let abs = find(session.dom(), session.dom().document(), "abs").expect("abs");
        let [_, fragment_y, _, fragment_height] = session.fragment_rect(abs).expect("abs fragment");
        let green = ColorF::new(0.0, 1.0, 0.0, 1.0);
        let glyph_ys = frame
            .commands()
            .iter()
            .filter_map(|command| match command {
                PaintCmd::DrawText(run) if run.color == green => {
                    run.glyphs.first().map(|glyph| glyph.point.y)
                },
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(!glyph_ys.is_empty(), "{label}: the overlay text paints");
        for y in glyph_ys {
            assert!(
                y >= fragment_y && y <= fragment_y + fragment_height,
                "{label}: glyph baseline {y} lies inside the absolute box's fragment {fragment_y}..{}",
                fragment_y + fragment_height
            );
        }
    }
}

/// `css/css-position/position-relative-006`, `-007`, `-010`, and `-013` in
/// one fixture: a block-axis percentage inset on a relatively positioned box
/// resolves against its containing block's specified block size, and is
/// `auto` when that size is indefinite.
#[test]
fn relative_block_percentage_insets_resolve_only_against_a_specified_height() {
    let cases = [
        (
            "min-height parent is indefinite: top -10000% is auto",
            "<div id=\"parent\" style=\"width:100px; min-height:100px\">\
               <div id=\"child\" style=\"width:100px; height:100px; top:-10000%; position:relative\"></div>\
             </div>",
            0.0,
        ),
        (
            "calc with a percentage against an indefinite height is auto",
            "<div id=\"parent\" style=\"width:100px; min-height:100px\">\
               <div id=\"child\" style=\"width:100px; height:100px; top:calc(10px + 10%); position:relative\"></div>\
             </div>",
            0.0,
        ),
        (
            "specified parent height resolves the percentage",
            "<div id=\"parent\" style=\"width:100px; height:200px\">\
               <div id=\"child\" style=\"width:100px; height:100px; top:25%; position:relative\"></div>\
             </div>",
            50.0,
        ),
        (
            "table cell: top 100% with no row height is auto",
            "<table id=\"parent\" style=\"border-spacing:0\"><tbody><tr>\
               <td id=\"child\" style=\"position:relative; top:100%; padding:0\">\
                 <div style=\"width:100px; height:100px\"></div>\
               </td></tr></tbody></table>",
            0.0,
        ),
        (
            "stretched flex item: its cross size is definite after layout (css-flexbox/position-relative-percentage-top-002)",
            "<div id=\"flex\" style=\"display:flex; width:100px\">\
               <div style=\"height:100px\"></div>\
               <div id=\"parent\" style=\"width:100px\">\
                 <div id=\"child\" style=\"height:100px; position:relative; top:-100%\"></div>\
               </div>\
             </div>",
            -100.0,
        ),
        (
            "percentage-height parent inside a definite flex container is definite (css-flexbox/position-relative-percentage-top-003)",
            "<div id=\"flex\" style=\"display:flex; width:100px; height:100px\">\
               <div>\
                 <div id=\"parent\" style=\"width:100%; height:100%\">\
                   <div style=\"width:100px; height:100px\"></div>\
                   <div id=\"child\" style=\"width:100px; height:100px; position:relative; top:-100%\"></div>\
                 </div>\
               </div>\
             </div>",
            0.0,
        ),
        (
            "percentage-height parent under an auto-height parent stays indefinite",
            "<div style=\"width:100px\">\
               <div id=\"parent\" style=\"width:100%; height:100%\">\
                 <div id=\"child\" style=\"width:100px; height:100px; position:relative; top:-100%\"></div>\
               </div>\
             </div>",
            0.0,
        ),
        (
            "table cell: top 100% resolves against the row's specified height",
            "<table id=\"parent\" style=\"border-spacing:0\"><tbody><tr style=\"height:10px\">\
               <td id=\"child\" style=\"position:relative; top:100%; padding:0\">\
                 <div style=\"width:100px; height:100px\"></div>\
               </td></tr></tbody></table>",
            10.0,
        ),
    ];
    let mut failures = Vec::new();
    for (label, body, expected) in cases {
        let html = format!("<html><body style=\"margin:0\">{body}</body></html>");
        let got = document_rects(&html, &["parent", "child"]);
        let (parent, child) = (got[0], got[1]);
        let offset = child.1 - parent.1;
        eprintln!("{label}: parent {parent:?} child {child:?} offset {offset}");
        if (offset - expected).abs() > 0.01 {
            failures.push(format!("{label}: expected {expected}, got {offset}"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// `css/css-position/position-relative-table-*-left` in miniature: an
/// absolutely positioned box with an `auto` block inset that precedes an
/// in-flow table keeps the static position it would have had before the
/// table, not one after it.
#[test]
fn absolute_box_before_a_table_keeps_its_static_position_above_the_table() {
    let html = "<html><body style=\"margin:0\">\
        <div id=\"group\" style=\"display:inline-block; position:relative; width:150px; height:200px\">\
          <div>\
            <div id=\"indicator\" style=\"position:absolute; left:100px; width:50px; height:50px\"></div>\
            <table id=\"table\" style=\"border-collapse:collapse\"><tbody>\
              <tr><td style=\"padding:0\"><div style=\"width:50px; height:50px\"></div></td></tr>\
              <tr><td style=\"padding:0\"><div style=\"width:50px; height:50px\"></div></td></tr>\
            </tbody></table>\
          </div>\
        </div></body></html>";
    let got = document_rects(html, &["group", "indicator", "table"]);
    let (group, indicator, table) = (got[0], got[1], got[2]);
    assert_eq!(
        table.1, group.1,
        "the table starts at the group's top {table:?}"
    );
    assert_eq!(
        indicator.1, group.1,
        "the indicator's static block position precedes the table {indicator:?}"
    );
    assert_eq!(indicator.0, group.0 + 100.0, "left: 100px {indicator:?}");
}

/// `css/css-position/position-relative-table-caption` in miniature: a
/// relatively positioned caption moves like any other relatively positioned
/// block inside the table wrapper.
#[test]
fn relative_caption_moves_by_its_inset() {
    let html = "<html><body style=\"margin:0\">\
        <table id=\"table\" style=\"border-collapse:collapse\">\
          <caption id=\"caption\" style=\"position:relative; top:100px; margin:0; padding:0\">\
            <div style=\"width:50px; height:50px\"></div>\
          </caption>\
          <tbody><tr><td style=\"padding:0\"><div style=\"width:50px; height:50px\"></div></td></tr></tbody>\
        </table></body></html>";
    let got = document_rects(html, &["table", "caption"]);
    let (table, caption) = (got[0], got[1]);
    assert_eq!(
        caption.1,
        table.1 + 100.0,
        "the caption's static position is the wrapper's top; top: 100px moves it down {caption:?} table {table:?}"
    );
}

#[test]
fn relative_table_parts_move_their_positioned_descendants() {
    for (label, part_markup, axis) in [
        (
            "row group inline offset",
            "<tbody id=part style='position:relative;left:50px'>\
               <tr><td id=cell style='padding:0'><div id=abs style='position:absolute;left:50px;width:50px;height:50px'></div></td></tr>\
             </tbody>",
            0,
        ),
        (
            "row group block offset",
            "<thead id=part style='position:relative;top:50px'>\
               <tr><td id=cell style='padding:0'><div id=abs style='position:absolute;top:50px;width:50px;height:50px'></div></td></tr>\
             </thead>\
             <tbody><tr><td style='padding:0'><div style='width:50px;height:50px'></div></td></tr></tbody>",
            1,
        ),
        (
            "row inline offset",
            "<tbody><tr id=part style='position:relative;left:50px'>\
               <td id=cell style='padding:0'><div id=abs style='position:absolute;left:50px;width:50px;height:50px'></div></td>\
             </tr></tbody>",
            0,
        ),
        (
            "row block offset",
            "<tbody><tr id=part style='position:relative;top:50px'>\
               <td id=cell style='padding:0'><div id=abs style='position:absolute;top:50px;width:50px;height:50px'></div></td>\
             </tr>\
             <tr><td style='padding:0'><div style='width:50px;height:50px'></div></td></tr></tbody>",
            1,
        ),
    ] {
        let html = format!(
            "<html><body style=\"margin:0\">\
             <table id=table style=\"border-collapse:collapse\">{part_markup}</table>\
             </body></html>"
        );
        let got = document_rects(&html, &["table", "part", "cell", "abs"]);
        let (table, part, cell, absolute) = (got[0], got[1], got[2], got[3]);
        let part_start = if axis == 0 { part.0 } else { part.1 };
        let part_size = if axis == 0 { part.2 } else { part.3 };
        let absolute_start = if axis == 0 { absolute.0 } else { absolute.1 };

        assert_eq!(
            part_size, 0.0,
            "{label}: an out-of-flow child does not size its table part; table={table:?}, part={part:?}, cell={cell:?}, absolute={absolute:?}"
        );
        assert_eq!(
            absolute_start - part_start,
            50.0,
            "{label}: the positioned descendant resolves from the translated table part; part={part:?}, absolute={absolute:?}"
        );
    }
}

/// Which block algorithm a page with an ordinary in-flow table runs on.
/// Recorded as evidence, not as an assertion: it explains why the
/// absolute-box flow-space defect only showed up with table content.
#[test]
fn plain_in_flow_table_block_algorithm_census() {
    let html = "<html><body style=\"margin:0\">\
        <div id=\"parent\"><table><tr><td>cell</td></tr></table><div id=\"sibling\">after</div></div>\
        </body></html>";
    let _ = rects(html, &["parent", "sibling"]);
}

#[test]
fn document_session_places_an_absolute_block_after_an_in_flow_sibling_at_top_zero() {
    let html = "<html><body style=\"margin:0\">\
        <div id=\"parent\" style=\"position:relative; height:300px\">\
          <div id=\"sibling\">in flow</div>\
          <div id=\"abs\" style=\"position:absolute; top:0\">overlay</div>\
        </div></body></html>";
    let got = document_rects(html, &["parent", "sibling", "abs"]);
    let (parent, sibling, abs) = (got[0], got[1], got[2]);
    assert!(
        sibling.3 > 0.0,
        "the sibling has a line of text {sibling:?}"
    );
    assert_eq!(
        abs.1, parent.1,
        "top: 0 places the absolute box at its containing block's top through the document session {abs:?}"
    );
}

/// The exact probe markup that fails in the WPT runner, then one feature
/// removed at a time, so the failing ingredient names itself.
#[test]
fn document_session_probe_bisection_for_top_zero_after_a_sibling() {
    let cases = [
        (
            "probe as rendered by the runner",
            "<html><body style=\"font-family: monospace; margin: 0\">\
            <p style=\"margin: 0; height: 40px\">lead</p>\
            <div id=\"parent\" style=\"position: relative; font-size: 2em; background: #eee; height: 300px\">\
              <div id=\"sibling\" style=\"color: red\">in-flow red text</div>\
              <div id=\"abs\" style=\"position: absolute; top: 0; color: green\">GREEN</div>\
            </div></body></html>",
        ),
        (
            "without the leading p",
            "<html><body style=\"font-family: monospace; margin: 0\">\
            <div id=\"parent\" style=\"position: relative; font-size: 2em; background: #eee; height: 300px\">\
              <div id=\"sibling\" style=\"color: red\">in-flow red text</div>\
              <div id=\"abs\" style=\"position: absolute; top: 0; color: green\">GREEN</div>\
            </div></body></html>",
        ),
        (
            "without font-size 2em",
            "<html><body style=\"font-family: monospace; margin: 0\">\
            <p style=\"margin: 0; height: 40px\">lead</p>\
            <div id=\"parent\" style=\"position: relative; background: #eee; height: 300px\">\
              <div id=\"sibling\" style=\"color: red\">in-flow red text</div>\
              <div id=\"abs\" style=\"position: absolute; top: 0; color: green\">GREEN</div>\
            </div></body></html>",
        ),
        (
            "without the background",
            "<html><body style=\"font-family: monospace; margin: 0\">\
            <p style=\"margin: 0; height: 40px\">lead</p>\
            <div id=\"parent\" style=\"position: relative; font-size: 2em; height: 300px\">\
              <div id=\"sibling\" style=\"color: red\">in-flow red text</div>\
              <div id=\"abs\" style=\"position: absolute; top: 0; color: green\">GREEN</div>\
            </div></body></html>",
        ),
        (
            "without monospace",
            "<html><body style=\"margin: 0\">\
            <p style=\"margin: 0; height: 40px\">lead</p>\
            <div id=\"parent\" style=\"position: relative; font-size: 2em; background: #eee; height: 300px\">\
              <div id=\"sibling\" style=\"color: red\">in-flow red text</div>\
              <div id=\"abs\" style=\"position: absolute; top: 0; color: green\">GREEN</div>\
            </div></body></html>",
        ),
    ];
    let mut failures = Vec::new();
    for (label, html) in cases {
        let got = document_rects(html, &["parent", "sibling", "abs"]);
        let (parent, sibling, abs) = (got[0], got[1], got[2]);
        eprintln!("{label}: parent {parent:?} sibling {sibling:?} abs {abs:?}");
        if abs.1 != parent.1 {
            failures.push(label);
        }
    }
    assert!(failures.is_empty(), "top: 0 ignored in: {failures:?}");
}

#[test]
fn document_session_gives_an_absolute_block_no_flow_space_whatever_it_contains() {
    for (label, content) in [
        ("text", "overlay"),
        (
            "html table",
            "<table cellpadding=\"0\" cellspacing=\"0\"><tr><td>overlay</td></tr></table>",
        ),
        (
            "css table",
            "<span style=\"display:table\"><span style=\"display:table-cell\">overlay</span></span>",
        ),
    ] {
        let html = format!(
            "<html><body style=\"margin:0\">\
            <div id=\"parent\" style=\"position:relative; height:300px\">\
              <div id=\"abs\" style=\"position:absolute; top:0\">{content}</div>\
              <div id=\"sibling\">in flow</div>\
            </div></body></html>"
        );
        let got = document_rects(&html, &["parent", "abs", "sibling"]);
        let (parent, abs, sibling) = (got[0], got[1], got[2]);
        assert_eq!(abs.1, parent.1, "{label}: absolute box at the top {abs:?}");
        assert_eq!(
            sibling.1, parent.1,
            "{label}: the in-flow sibling starts at the parent's top {sibling:?}"
        );
    }
}

#[test]
fn absolute_block_after_an_in_flow_sibling_honors_top_zero() {
    let html = "<html><body style=\"margin:0\">\
        <div id=\"parent\" style=\"position:relative; height:300px\">\
          <div id=\"sibling\" style=\"height:50px\">in flow</div>\
          <div id=\"abs\" style=\"position:absolute; top:0; height:20px\">overlay</div>\
        </div></body></html>";
    let got = rects(html, &["parent", "sibling", "abs"]);
    let (parent, sibling, abs) = (got[0], got[1], got[2]);
    assert_eq!((parent.0, parent.1), (0.0, 0.0), "parent at the origin");
    assert_eq!(
        (sibling.0, sibling.1),
        (0.0, 0.0),
        "sibling at the parent's top"
    );
    assert_eq!(
        abs.1, parent.1,
        "top: 0 places the absolute box at its containing block's top, not at its static position {abs:?}"
    );
}

#[test]
fn absolute_block_honors_a_top_inset_that_differs_from_its_static_position() {
    for (label, html) in [
        (
            "first child, top: 100px",
            "<html><body style=\"margin:0\">\
            <div id=\"parent\" style=\"position:relative; height:300px\">\
              <div id=\"abs\" style=\"position:absolute; top:100px; height:20px\">overlay</div>\
              <div id=\"sibling\" style=\"height:50px\">in flow</div>\
            </div></body></html>",
        ),
        (
            "second child, top: 100px",
            "<html><body style=\"margin:0\">\
            <div id=\"parent\" style=\"position:relative; height:300px\">\
              <div id=\"sibling\" style=\"height:50px\">in flow</div>\
              <div id=\"abs\" style=\"position:absolute; top:100px; height:20px\">overlay</div>\
            </div></body></html>",
        ),
    ] {
        let got = rects(html, &["parent", "abs"]);
        let (parent, abs) = (got[0], got[1]);
        assert_eq!(abs.1, parent.1 + 100.0, "{label}: {abs:?}");
    }
}

#[test]
fn absolute_block_takes_no_flow_space_whatever_it_contains() {
    for (label, content) in [
        ("text", "overlay"),
        (
            "html table",
            "<table cellpadding=\"0\" cellspacing=\"0\"><tr><td>overlay</td></tr></table>",
        ),
        (
            "css table",
            "<span style=\"display:table\"><span style=\"display:table-cell\">overlay</span></span>",
        ),
    ] {
        let html = format!(
            "<html><body style=\"margin:0\">\
            <div id=\"parent\" style=\"position:relative; height:300px\">\
              <div id=\"abs\" style=\"position:absolute; top:0\">{content}</div>\
              <div id=\"sibling\" style=\"height:50px\">in flow</div>\
            </div></body></html>"
        );
        let got = rects(&html, &["parent", "abs", "sibling"]);
        let (parent, abs, sibling) = (got[0], got[1], got[2]);
        assert_eq!(abs.1, parent.1, "{label}: absolute box at the top {abs:?}");
        assert_eq!(
            sibling.1, parent.1,
            "{label}: the in-flow sibling starts at the parent's top; the absolute box takes no flow space {sibling:?}"
        );
    }
}
