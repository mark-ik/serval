//! css-text lane (2026-08-21) fixtures: the text properties and white-space
//! rules `genet-livery/src/text.rs` carries to Parley. Each test names the
//! `css/css-text` family it stands for; the numbers come from the same
//! retained document session the WPT runner uses.

use genet_livery::{Device, LiveryDocument, StyleSet};
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

/// One painted glyph run: the glyph ids in visual order and their points.
#[derive(Clone, Debug)]
struct Run {
    ids: Vec<u32>,
    points: Vec<(f32, f32)>,
}

struct Rendered {
    session: LiveryDocument<StaticDocument>,
    commands: Vec<PaintCmd>,
}

impl Rendered {
    fn new(body: &str) -> Self {
        let html = format!("<html><body style=\"margin:0\">{body}</body></html>");
        let mut session = LiveryDocument::new(
            StaticDocument::parse(&html),
            StyleSet::cambium(&[]),
            Device::screen(800.0, 600.0),
        );
        let frame = session.frame(800, 600).expect("frame");
        let commands = frame.commands().to_vec();
        Self { session, commands }
    }

    /// `(x, y, width, height)` of the element's fragment.
    fn rect(&self, id: &str) -> (f32, f32, f32, f32) {
        let node = find(self.session.dom(), self.session.dom().document(), id).expect(id);
        let [x, y, width, height] = self
            .session
            .fragment_rect(node)
            .unwrap_or_else(|| panic!("{id} has a fragment"));
        (x, y, width, height)
    }

    /// The glyph runs painted in `color`, in paint order. Fixtures give each
    /// element under test its own colour so this selects one element's text.
    fn runs(&self, color: ColorF) -> Vec<Run> {
        self.commands
            .iter()
            .filter_map(|command| match command {
                PaintCmd::DrawText(run) if run.color == color => Some(Run {
                    ids: run.glyphs.iter().map(|glyph| glyph.index).collect(),
                    points: run
                        .glyphs
                        .iter()
                        .map(|glyph| (glyph.point.x, glyph.point.y))
                        .collect(),
                }),
                _ => None,
            })
            .collect()
    }

    /// Every glyph painted in `color`, flattened across runs.
    fn glyphs(&self, color: ColorF) -> Vec<(u32, f32, f32)> {
        self.runs(color)
            .into_iter()
            .flat_map(|run| {
                run.ids
                    .into_iter()
                    .zip(run.points)
                    .map(|(id, (x, y))| (id, x, y))
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

const RED: ColorF = ColorF {
    r: 1.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};
const BLUE: ColorF = ColorF {
    r: 0.0,
    g: 0.0,
    b: 1.0,
    a: 1.0,
};

fn glyph_ids(glyphs: &[(u32, f32, f32)]) -> Vec<u32> {
    glyphs.iter().map(|(id, _, _)| *id).collect()
}

fn distinct_line_ys(glyphs: &[(u32, f32, f32)]) -> Vec<f32> {
    let mut ys = Vec::<f32>::new();
    for (_, _, y) in glyphs {
        if !ys.iter().any(|known| (known - y).abs() < 0.5) {
            ys.push(*y);
        }
    }
    ys.sort_by(f32::total_cmp);
    ys
}

/// `css/css-text/i18n/*` references, `white-space/white-space-normal-011`,
/// `white-space/white-space-pre-031`: a forced line break inside text keeps
/// every line in the block's height. The block height was collapsing to one
/// line whenever a `<br>` atom was present.
#[test]
fn forced_line_break_keeps_both_lines_in_the_block_height() {
    let rendered = Rendered::new(
        "<div id=\"two\" style=\"font:16px/20px sans-serif; width:300px\">one<br>two</div>\
         <div id=\"three\" style=\"font:16px/20px sans-serif; width:300px\">one<br>two<br>three</div>\
         <div id=\"trailing\" style=\"font:16px/20px sans-serif; width:300px\">one<br></div>",
    );
    assert_eq!(rendered.rect("two").3, 40.0, "two lines");
    assert_eq!(rendered.rect("three").3, 60.0, "three lines");
    assert_eq!(
        rendered.rect("trailing").3,
        20.0,
        "a trailing <br> ends the line without opening another"
    );
}

/// `css/css-text/white-space/pre-line-051`,
/// `white-space-collapse-preserve-breaks-001`: `pre-line` collapses spaces
/// but keeps segment breaks as forced breaks, removing the spaces around
/// them.
#[test]
fn pre_line_preserves_segment_breaks_and_collapses_spaces() {
    let rendered = Rendered::new(
        "<div id=\"t\" style=\"font:16px/20px sans-serif; width:300px; white-space:pre-line; color:#f00\">ab  \n   cd</div>\
         <div id=\"r\" style=\"font:16px/20px sans-serif; width:300px; color:#00f\">ab<br>cd</div>",
    );
    assert_eq!(rendered.rect("t").3, 40.0, "two lines");
    let test = rendered.glyphs(RED);
    let reference = rendered.glyphs(BLUE);
    assert_eq!(glyph_ids(&test), glyph_ids(&reference), "no space survives");
    assert_eq!(
        test.iter().map(|(_, x, _)| *x).collect::<Vec<_>>(),
        reference.iter().map(|(_, x, _)| *x).collect::<Vec<_>>(),
        "both lines start at the content edge"
    );
}

/// `css/css-text/white-space/white-space-pre-031`, `tab-size/*`: a
/// preserved tab advances to the next tab stop, `tab-size` space advances
/// wide, instead of shaping U+0009 as a missing glyph.
#[test]
fn preserved_tabs_advance_to_tab_stops() {
    let rendered = Rendered::new(
        "<pre id=\"t\" style=\"font:16px/20px monospace; margin:0; color:#f00\">ab\tx\n\tx</pre>\
         <pre id=\"r\" style=\"font:16px/20px monospace; margin:0; color:#00f\">ab      x\n        x</pre>\
         <pre id=\"four\" style=\"font:16px/20px monospace; margin:0; tab-size:4; color:#f00\">ab\tx</pre>\
         <pre id=\"four_ref\" style=\"font:16px/20px monospace; margin:0; color:#00f\">ab  x</pre>",
    );
    let line_max_xs = |color| {
        let glyphs = rendered.glyphs(color);
        distinct_line_ys(&glyphs)
            .iter()
            .map(|line| {
                glyphs
                    .iter()
                    .filter(|(_, _, y)| (*y - line).abs() < 0.5)
                    .map(|(_, x, _)| *x)
                    .fold(f32::MIN, f32::max)
            })
            .collect::<Vec<_>>()
    };
    let test = line_max_xs(RED);
    let reference = line_max_xs(BLUE);
    assert_eq!(
        test.len(),
        3,
        "two lines in the first pre and one in the third"
    );
    assert_eq!(
        reference.len(),
        3,
        "each tab expansion has a space reference"
    );
    assert!(
        (test[0] - reference[0]).abs() < 0.5,
        "a tab after two characters reaches column 8: {test:?} vs {reference:?}"
    );
    assert!(
        (test[1] - reference[1]).abs() < 0.5,
        "a tab at the line start reaches column 8: {test:?} vs {reference:?}"
    );
    assert!(
        (test[2] - reference[2]).abs() < 0.5,
        "tab-size: 4 reaches column 4: {test:?} vs {reference:?}"
    );
}

#[test]
fn tab_stops_include_letter_and_word_spacing() {
    let rendered = Rendered::new(
        "<div id=\"ref\" style=\"position:absolute; font-family:monospace; margin-left:calc(8ch + 8 * 2px + 8 * 10px); width:20px; height:20px\"></div>\
         <div style=\"white-space:pre; tab-size:8; font-family:monospace; letter-spacing:2px; word-spacing:10px\">\t<span id=\"tab\" style=\"display:inline-block; width:20px; height:20px; background:#00ff00\"></span></div>",
    );
    let actual_rect = rendered.rect("tab");
    let expected_rect = rendered.rect("ref");
    let actual = actual_rect.0;
    let expected = expected_rect.0;
    assert_eq!(
        actual_rect.2, 20.0,
        "the inline box keeps its specified width"
    );
    assert!(
        expected > 150.0,
        "the reference offset resolves: {expected_rect:?}"
    );
    assert!(
        (actual - expected).abs() < 0.5,
        "tab stop {actual} should include the spacing terms and reach {expected}"
    );
    let painted = rendered
        .commands
        .iter()
        .find_map(|command| match command {
            PaintCmd::DrawRect(rect) if rect.color == ColorF::new(0.0, 1.0, 0.0, 1.0) => {
                Some(rect.placement.bounds)
            },
            _ => None,
        })
        .expect("the inline box background paints");
    assert!(
        (painted.min.x - actual).abs() < 0.5,
        "paint follows layout: {painted:?}"
    );
    assert_eq!(
        painted.max.x - painted.min.x,
        20.0,
        "paint keeps the box width"
    );
}

#[test]
fn first_opening_punctuation_hangs_by_its_shaped_advance() {
    let rendered = Rendered::new(
        "<div style=\"font:20px/24px monospace; hanging-punctuation:first; color:#f00\">(x</div>\
         <div style=\"font:20px/24px monospace; color:#00f\">(x</div>\
         <div style=\"font:20px/24px monospace; hanging-punctuation:first; color:#f00\"><span style=\"border-left:10px solid black\">(</span>x</div>",
    );
    let hanging = rendered.glyphs(RED);
    let plain = rendered.glyphs(BLUE);
    assert!(hanging.len() >= 4 && plain.len() >= 2);
    let punctuation_advance = plain[1].1 - plain[0].1;
    assert!(
        (hanging[0].1 - (plain[0].1 - punctuation_advance)).abs() < 0.5,
        "the first opening punctuation hangs: {hanging:?} vs {plain:?}"
    );
    assert!(
        (hanging[2].1 - 10.0).abs() < 0.5,
        "a non-zero inline edge blocks hanging: {hanging:?}"
    );
}

#[test]
fn ideographic_space_hangs_before_an_emergency_wrap() {
    let rendered = Rendered::new(
        "<div id=\"t\" style=\"font:25px/25px monospace; width:2ch; overflow-wrap:anywhere\">XX<span style=\"background:#00ff00\">\u{3000}</span>XX</div>",
    );
    assert_eq!(
        rendered.rect("t").3,
        50.0,
        "the hanging space does not open a third line"
    );
}

/// `css/css-text/text-transform/*`: case mapping, full-width, and
/// full-size-kana transforms apply before shaping.
#[test]
fn text_transform_maps_text_before_shaping() {
    let cases = [
        ("uppercase", "hello world", "HELLO WORLD"),
        ("lowercase", "HELLO World", "hello world"),
        (
            "capitalize",
            "hello wide-world (again) o'neil",
            "Hello Wide-World (Again) O'neil",
        ),
        (
            "full-width",
            "ab 1!",
            "\u{ff41}\u{ff42}\u{3000}\u{ff11}\u{ff01}",
        ),
        ("full-size-kana", "\u{3041}\u{3063}", "\u{3042}\u{3064}"),
        ("uppercase full-width", "ab", "\u{ff21}\u{ff22}"),
        (
            "capitalize",
            "\u{24d0}\u{24d0}\u{24d0}",
            "\u{24d0}\u{24d0}\u{24d0}",
        ),
        ("full-width", "x   x", "\u{ff58}\u{3000}\u{ff58}"),
    ];
    for (transform, source, expected) in cases {
        let rendered = Rendered::new(&format!(
            "<div style=\"font:16px/20px sans-serif; text-transform:{transform}; color:#f00\">{source}</div>\
             <div style=\"font:16px/20px sans-serif; color:#00f\">{expected}</div>"
        ));
        let test = rendered.glyphs(RED);
        let reference = rendered.glyphs(BLUE);
        assert!(!reference.is_empty(), "{transform}: reference paints");
        assert_eq!(
            glyph_ids(&test),
            glyph_ids(&reference),
            "{transform}: {source:?} shapes like {expected:?}"
        );
    }
}

/// `css/css-text/letter-spacing/letter-spacing-percent-001`,
/// `word-spacing/word-spacing-percent-001`: percentage spacing resolves
/// against the element's own font size.
#[test]
fn percentage_letter_and_word_spacing_resolve_against_font_size() {
    let rendered = Rendered::new(
        "<div style=\"font:20px/24px sans-serif; letter-spacing:50%; word-spacing:100%; color:#f00\">ab cd</div>\
         <div style=\"font:20px/24px sans-serif; letter-spacing:10px; word-spacing:20px; color:#00f\">ab cd</div>",
    );
    let test = rendered.glyphs(RED);
    let reference = rendered.glyphs(BLUE);
    assert_eq!(glyph_ids(&test), glyph_ids(&reference));
    for ((_, test_x, _), (_, reference_x, _)) in test.iter().zip(&reference) {
        assert!(
            (test_x - reference_x).abs() < 0.01,
            "glyph at {test_x} should sit at {reference_x}"
        );
    }
}

/// `css/css-text/overflow-wrap/overflow-wrap-001`, `-anywhere-001`,
/// `word-break/word-break-break-all-*`: emergency and all-character breaks
/// wrap an unbreakable word inside a narrow block.
#[test]
fn overflow_wrap_and_word_break_wrap_unbreakable_words() {
    let word = "MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM";
    let rendered = Rendered::new(&format!(
        "<div id=\"normal\" style=\"font:16px/20px sans-serif; width:100px\">{word}</div>\
         <div id=\"break_word\" style=\"font:16px/20px sans-serif; width:100px; overflow-wrap:break-word\">{word}</div>\
         <div id=\"anywhere\" style=\"font:16px/20px sans-serif; width:100px; overflow-wrap:anywhere\">{word}</div>\
         <div id=\"word_wrap\" style=\"font:16px/20px sans-serif; width:100px; word-wrap:break-word\">{word}</div>\
         <div id=\"break_all\" style=\"font:16px/20px sans-serif; width:100px; word-break:break-all\">{word}</div>\
         <div id=\"legacy\" style=\"font:16px/20px sans-serif; width:100px; word-break:break-word\">{word}</div>\
         <div id=\"nested\" style=\"font:16px/20px sans-serif; width:100px\"><span style=\"overflow-wrap:anywhere\">{word}</span></div>"
    ));
    assert_eq!(
        rendered.rect("normal").3,
        20.0,
        "no opportunity: the word overflows"
    );
    for id in [
        "break_word",
        "anywhere",
        "word_wrap",
        "break_all",
        "legacy",
        "nested",
    ] {
        assert!(
            rendered.rect(id).3 >= 40.0,
            "{id} wraps the word ({:?})",
            rendered.rect(id)
        );
    }
}

/// `css/css-text/word-break/word-break-keep-all-*`: `keep-all` suppresses
/// the implicit break opportunities between CJK characters.
#[test]
fn word_break_keep_all_keeps_cjk_runs_together() {
    let text = "\u{4e2d}\u{6587}\u{4e2d}\u{6587}\u{4e2d}\u{6587}\u{4e2d}\u{6587}";
    let rendered = Rendered::new(&format!(
        "<div id=\"normal\" style=\"font:16px/20px sans-serif; width:60px\">{text}</div>\
         <div id=\"keep\" style=\"font:16px/20px sans-serif; width:60px; word-break:keep-all\">{text}</div>"
    ));
    assert!(
        rendered.rect("normal").3 >= 40.0,
        "normal breaks between ideographs"
    );
    assert_eq!(rendered.rect("keep").3, 20.0, "keep-all does not");
}

/// `css/css-text/text-indent/text-indent-length-001`, `-percentage-001`,
/// `text-indent-each-line-hanging`: the first line starts after the indent,
/// a percentage resolves against the containing block, and `hanging` or
/// `each-line` select the other lines.
#[test]
fn text_indent_offsets_the_selected_lines() {
    let rendered = Rendered::new(
        "<div id=\"px\" style=\"font:16px/20px sans-serif; width:200px; text-indent:30px; color:#f00\">a<br>b</div>\
         <div id=\"pct\" style=\"font:16px/20px sans-serif; width:200px; text-indent:25%; color:#00f\">a<br>b</div>\
         <div id=\"hang\" style=\"font:16px/20px sans-serif; width:200px; text-indent:30px hanging; color:#0f0\">a<br>b</div>\
         <div id=\"each\" style=\"font:16px/20px sans-serif; width:200px; text-indent:30px each-line; color:#ff0\">a<br>b</div>",
    );
    let green = ColorF::new(0.0, 1.0, 0.0, 1.0);
    let yellow = ColorF::new(1.0, 1.0, 0.0, 1.0);
    let line_xs = |color| {
        let glyphs = rendered.glyphs(color);
        let lines = distinct_line_ys(&glyphs);
        lines
            .iter()
            .map(|line| {
                glyphs
                    .iter()
                    .filter(|(_, _, y)| (y - line).abs() < 0.5)
                    .map(|(_, x, _)| *x)
                    .fold(f32::MAX, f32::min)
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        line_xs(RED),
        vec![30.0, 0.0],
        "30px indents the first line only"
    );
    assert_eq!(line_xs(BLUE), vec![50.0, 0.0], "25% of 200px");
    assert_eq!(
        line_xs(green),
        vec![0.0, 30.0],
        "hanging indents the other lines"
    );
    assert_eq!(
        line_xs(yellow),
        vec![30.0, 30.0],
        "each-line indents after a forced break"
    );
}

/// `css/css-text/text-align/text-align-start-001`, `-end-001`,
/// `text-align-justify-001`: `start` and `end` follow the computed
/// `direction`, so an RTL block starts at its right edge.
#[test]
fn text_align_start_and_end_follow_direction() {
    let rendered = Rendered::new(
        "<div id=\"start\" style=\"font:16px/20px sans-serif; width:200px; direction:rtl; text-align:start; color:#f00\">AB</div>\
         <div id=\"end\" style=\"font:16px/20px sans-serif; width:200px; direction:rtl; text-align:end; color:#00f\">AB</div>",
    );
    let start = rendered.glyphs(RED);
    let end = rendered.glyphs(BLUE);
    assert!(
        start.iter().all(|(_, x, _)| *x > 150.0),
        "start in rtl is the right edge: {start:?}"
    );
    assert!(
        end.iter().all(|(_, x, _)| *x < 50.0),
        "end in rtl is the left edge: {end:?}"
    );
}

/// `css/css-text/text-align/text-align-last-simple`,
/// `text-align-*-last-*`, `text-align-justifyall-*`: the last line of a
/// block and the line before a forced break take `text-align-last`;
/// `justify-all` justifies them too.
#[test]
fn text_align_last_aligns_the_final_line() {
    let rendered = Rendered::new(
        "<div id=\"end\" style=\"font:16px/20px sans-serif; width:30px; text-align:start; text-align-last:end; color:#f00\">ab cd</div>\
         <div id=\"center\" style=\"font:16px/20px sans-serif; width:30px; text-align:end; text-align-last:center; color:#00f\">ab cd</div>",
    );
    let line_max_xs = |color| {
        let glyphs = rendered.glyphs(color);
        distinct_line_ys(&glyphs)
            .iter()
            .map(|line| {
                glyphs
                    .iter()
                    .filter(|(_, _, y)| (y - line).abs() < 0.5)
                    .map(|(_, x, _)| *x)
                    .fold(f32::MIN, f32::max)
            })
            .collect::<Vec<_>>()
    };
    let end = line_max_xs(RED);
    assert!(end[0] < 20.0 && end[1] > 20.0, "start then end: {end:?}");
    let center = line_max_xs(BLUE);
    assert!(
        center[0] > 20.0 && center[1] > 10.0 && center[1] < 30.0,
        "end then center: {center:?}"
    );
}

/// `css/css-text/text-align/text-align-justifyall-001`: `justify-all`
/// stretches the last line as well; `text-justify: none` disables
/// justification entirely.
#[test]
fn justify_all_and_text_justify_none() {
    let words = "aa bb cc dd ee ff gg hh ii jj kk ll mm nn oo pp qq rr ss tt";
    let rendered = Rendered::new(&format!(
        "<div id=\"justify\" style=\"font:16px/20px sans-serif; width:240px; text-align:justify; color:#f00\">{words}</div>\
         <div id=\"all\" style=\"font:16px/20px sans-serif; width:240px; text-align:justify-all; color:#00f\">{words}</div>\
         <div id=\"none\" style=\"font:16px/20px sans-serif; width:240px; text-align:justify; text-justify:none; color:#0f0\">{words}</div>\
         <div id=\"plain\" style=\"font:16px/20px sans-serif; width:240px; color:#ff0\">{words}</div>"
    ));
    let green = ColorF::new(0.0, 1.0, 0.0, 1.0);
    let yellow = ColorF::new(1.0, 1.0, 0.0, 1.0);
    let last_line_end = |color| {
        let glyphs = rendered.glyphs(color);
        let last = *distinct_line_ys(&glyphs).last().expect("lines");
        glyphs
            .iter()
            .filter(|(_, _, y)| (y - last).abs() < 0.5)
            .map(|(_, x, _)| *x)
            .fold(f32::MIN, f32::max)
    };
    let justified_last = last_line_end(RED);
    let all_last = last_line_end(BLUE);
    assert!(
        all_last > justified_last + 20.0,
        "justify-all stretches the last line: {all_last} vs {justified_last}"
    );
    let none = rendered.glyphs(green);
    let plain = rendered.glyphs(yellow);
    assert_eq!(
        none.iter().map(|(_, x, _)| *x).collect::<Vec<_>>(),
        plain.iter().map(|(_, x, _)| *x).collect::<Vec<_>>(),
        "text-justify: none lays out like start alignment"
    );
}

/// `css/css-text/hyphens/hyphens-none-*`: `hyphens: none` removes the
/// soft-hyphen break opportunities; the default `manual` keeps them.
#[test]
fn hyphens_none_suppresses_soft_hyphen_breaks() {
    let word = "ab\u{ad}cd\u{ad}ef\u{ad}gh\u{ad}ij\u{ad}kl\u{ad}mn\u{ad}op";
    let rendered = Rendered::new(&format!(
        "<div id=\"manual\" style=\"font:16px/20px sans-serif; width:40px\">{word}</div>\
         <div id=\"none\" style=\"font:16px/20px sans-serif; width:40px; hyphens:none\">{word}</div>"
    ));
    assert!(
        rendered.rect("manual").3 >= 40.0,
        "manual breaks at soft hyphens"
    );
    assert_eq!(rendered.rect("none").3, 20.0, "none keeps the word whole");
}

/// `css/css-text/white-space/white-space-wrap-after-nowrap-001`,
/// `line-breaking/*`: a `nowrap` inline inside a wrapping block keeps its
/// own text on one line while the block still wraps around it.
#[test]
fn nowrap_inline_inside_a_wrapping_block() {
    let rendered = Rendered::new(
        "<div id=\"t\" style=\"font:16px/20px sans-serif; width:120px\"><span style=\"white-space:nowrap; color:#f00\">aaaa bbbb cccc dddd</span> <span style=\"color:#00f\">eeee</span></div>",
    );
    let nowrap = rendered.glyphs(RED);
    assert_eq!(
        distinct_line_ys(&nowrap).len(),
        1,
        "the nowrap span stays on one line"
    );
    let wrapped = rendered.glyphs(BLUE);
    assert!(
        distinct_line_ys(&wrapped)[0] > distinct_line_ys(&nowrap)[0] + 0.5,
        "the following text wraps to the next line"
    );
}
