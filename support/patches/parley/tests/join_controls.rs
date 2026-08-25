//! Genet receipts for join-control font fallback and spacing.

use std::{borrow::Cow, sync::Arc};

use parley::{
    FontContext, FontFamily, FontFeature, FontFeatures, LayoutContext, StyleProperty,
    fontique::{Blob, FontInfoOverride},
};

fn feature(tag: [u8; 4], value: u16) -> FontFeature {
    FontFeature::new(parley::setting::Tag::from_bytes(tag), value)
}

fn metrics(
    font_context: &mut FontContext,
    layout_context: &mut LayoutContext,
    text: &str,
    features: Vec<FontFeature>,
    letter_spacing: f32,
) -> (f32, f32) {
    let mut builder = layout_context.ranged_builder(font_context, text, 1.0, false);
    builder.push_default(StyleProperty::FontSize(32.0));
    builder.push_default(StyleProperty::FontFamily(FontFamily::Source(
        Cow::Borrowed("face"),
    )));
    builder.push_default(StyleProperty::FontFeatures(FontFeatures::List(Cow::Owned(
        features,
    ))));
    builder.push_default(StyleProperty::LetterSpacing(letter_spacing));
    let mut layout = builder.build(text);
    layout.break_all_lines(None);
    let line = layout.lines().next().expect("one text line");
    let metrics = line.metrics();
    (metrics.line_height, metrics.advance)
}

#[test]
fn join_controls_keep_the_selected_face_and_do_not_receive_letter_spacing() {
    let mut font_context = FontContext::new();
    font_context.collection.register_fonts(
        Blob::new(Arc::new(
            include_bytes!("../../../../tests/wpt/tests/fonts/Lato-Medium-Liga.ttf").to_vec(),
        )),
        Some(FontInfoOverride {
            family_name: Some("face"),
            ..Default::default()
        }),
    );
    let mut layout_context = LayoutContext::new();

    let plain = metrics(
        &mut font_context,
        &mut layout_context,
        "st",
        vec![feature(*b"dlig", 0)],
        3.2,
    );
    let joined = metrics(
        &mut font_context,
        &mut layout_context,
        "s\u{200c}t",
        vec![feature(*b"dlig", 0)],
        3.2,
    );

    assert!(
        (plain.0 - joined.0).abs() < 0.001,
        "ZWNJ forced a fallback face"
    );
    assert!(
        (plain.1 - joined.1).abs() < 0.001,
        "ZWNJ received letter spacing: plain={}, joined={}",
        plain.1,
        joined.1
    );
}
