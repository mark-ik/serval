// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Per-element cascade property readers (line-height, text decoration,
//! color, font metrics) backing inline run styling.

use super::*;

/// An element's cascaded `line-height` mapped to a [`LineHeightSpec`]. `normal`
/// (and the un-cascaded case) keeps parley's font-metric default; a `<number>`
/// becomes a font-size multiple, a `<length>` an absolute px height.
pub(crate) fn line_height_of<NodeId: Copy + Eq + Hash>(
    styles: &StylePlane<NodeId>,
    id: NodeId,
) -> Option<LineHeightSpec> {
    use style::values::computed::LineHeight;
    let entry = styles.get(id)?;
    let data = entry.borrow_data()?;
    Some(match &data.styles.primary().get_font().line_height {
        LineHeight::Normal => LineHeightSpec::Normal,
        LineHeight::Number(num) => LineHeightSpec::Factor(num.0),
        LineHeight::Length(l) => LineHeightSpec::Px(l.px()),
        // `-moz-block-height` is gecko-only (cfg'd out here); treat as normal.
        #[allow(unreachable_patterns)]
        _ => LineHeightSpec::Normal,
    })
}

/// Whether an element's cascaded `text-decoration-line` includes `underline`.
/// `None` when the cascade hasn't run.
pub(crate) fn text_underline_of<NodeId: Copy + Eq + Hash>(
    styles: &StylePlane<NodeId>,
    id: NodeId,
) -> Option<bool> {
    use style::values::computed::TextDecorationLine;
    let entry = styles.get(id)?;
    let data = entry.borrow_data()?;
    Some(
        data.styles
            .primary()
            .get_text()
            .text_decoration_line
            .contains(TextDecorationLine::UNDERLINE),
    )
}

/// Whether an element's cascaded `text-decoration-line` includes `line-through`.
/// `None` when the cascade hasn't run.
pub(crate) fn text_strikethrough_of<NodeId: Copy + Eq + Hash>(
    styles: &StylePlane<NodeId>,
    id: NodeId,
) -> Option<bool> {
    use style::values::computed::TextDecorationLine;
    let entry = styles.get(id)?;
    let data = entry.borrow_data()?;
    Some(
        data.styles
            .primary()
            .get_text()
            .text_decoration_line
            .contains(TextDecorationLine::LINE_THROUGH),
    )
}

/// Whether an element's cascaded `text-decoration-line` includes `overline`.
/// `None` when the cascade hasn't run.
pub(crate) fn text_overline_of<NodeId: Copy + Eq + Hash>(
    styles: &StylePlane<NodeId>,
    id: NodeId,
) -> Option<bool> {
    use style::values::computed::TextDecorationLine;
    let entry = styles.get(id)?;
    let data = entry.borrow_data()?;
    Some(
        data.styles
            .primary()
            .get_text()
            .text_decoration_line
            .contains(TextDecorationLine::OVERLINE),
    )
}

/// Read an element's cascaded text `color` as straight RGBA in
/// `[0, 1]`. `None` when the cascade hasn't run.
pub(crate) fn text_color_of<NodeId: Copy + Eq + Hash>(
    styles: &StylePlane<NodeId>,
    id: NodeId,
) -> Option<[f32; 4]> {
    let entry = styles.get(id)?;
    let data = entry.borrow_data()?;
    let absolute = data.styles.primary().get_inherited_text().color;
    let srgb = absolute.into_srgb_legacy();
    Some(*srgb.raw_components())
}

/// Definite CSS `width` / `height` (px) of an inline-block, or `None` per axis
/// for `auto` / percentage / intrinsic (→ shrink-to-fit / content height).
pub(crate) fn inline_block_css_size<NodeId: Copy + Eq + Hash>(
    styles: &StylePlane<NodeId>,
    id: NodeId,
) -> (Option<f32>, Option<f32>) {
    let Some(entry) = styles.get(id) else {
        return (None, None);
    };
    let Some(data) = entry.borrow_data() else {
        return (None, None);
    };
    let pos = data.styles.primary().get_position();
    (definite_px(&pos.width), definite_px(&pos.height))
}

/// An inline-block's cascaded box-model rings as `(padding, border, margin)` in
/// px. An atomic inline-level box reserves its **margin** box in the line (CSS
/// 2.2 §10.6.6, §10.8), and `width` names only its content (§10.2, §10.3.9), so
/// these rings are what turn a content measurement into the box parley places.
///
/// Definite lengths only. Percentage padding and margin resolve against the
/// containing block's inline size, which construction does not have; they read
/// as zero here, the same residual [`inline_block_css_size`] carries for a
/// percentage `width`. `auto` margins compute to zero on an atomic inline (there
/// is no free inline space to distribute, so no centring). A side whose
/// `border-style` is `none` or `hidden` contributes no border width whatever the
/// specified width (CSS 2.1 §8.5.1), matching `stylo_taffy::convert::border` and
/// the paint path's own clamp.
pub(crate) fn inline_block_box_edges<NodeId: Copy + Eq + Hash>(
    styles: &StylePlane<NodeId>,
    id: NodeId,
) -> (EdgeSizes, EdgeSizes, EdgeSizes) {
    let zero = (
        EdgeSizes::default(),
        EdgeSizes::default(),
        EdgeSizes::default(),
    );
    let Some(entry) = styles.get(id) else {
        return zero;
    };
    let Some(data) = entry.borrow_data() else {
        return zero;
    };
    let cv = data.styles.primary();

    let p = cv.get_padding();
    let padding = EdgeSizes {
        top: definite_lp_px(&p.padding_top.0),
        right: definite_lp_px(&p.padding_right.0),
        bottom: definite_lp_px(&p.padding_bottom.0),
        left: definite_lp_px(&p.padding_left.0),
    };

    let b = cv.get_border();
    let border = EdgeSizes {
        top: border_side_px(&b.border_top_width, b.border_top_style),
        right: border_side_px(&b.border_right_width, b.border_right_style),
        bottom: border_side_px(&b.border_bottom_width, b.border_bottom_style),
        left: border_side_px(&b.border_left_width, b.border_left_style),
    };

    let m = cv.get_margin();
    let margin = EdgeSizes {
        top: definite_margin_px(&m.margin_top),
        right: definite_margin_px(&m.margin_right),
        bottom: definite_margin_px(&m.margin_bottom),
        left: definite_margin_px(&m.margin_left),
    };

    (padding, border, margin)
}

/// A computed `LengthPercentage` as px, with percentages (no containing-block
/// basis here) and calc reading as zero.
fn definite_lp_px(lp: &style::values::computed::LengthPercentage) -> f32 {
    lp.to_length().map(|l| l.px()).unwrap_or(0.0)
}

/// A computed margin as px; `auto` and percentages read as zero (see
/// [`inline_block_box_edges`]). The catch-all covers `auto` and the
/// anchor-positioning variants, which are flagged off.
fn definite_margin_px(m: &style::values::computed::Margin) -> f32 {
    use style::values::generics::length::GenericMargin as Gm;
    match m {
        Gm::LengthPercentage(lp) => definite_lp_px(lp),
        _ => 0.0,
    }
}

/// One border side's used width: zero when the side's style is `none` or
/// `hidden`, whatever width was specified (CSS 2.1 §8.5.1).
fn border_side_px(
    width: &style::values::computed::BorderSideWidth,
    border_style: style::values::computed::BorderStyle,
) -> f32 {
    if border_style.none_or_hidden() {
        return 0.0;
    }
    width.0.to_f32_px()
}

/// An inline-block's cascaded `background-color` as straight RGBA, resolving
/// `currentColor` against its own `color`. Transparent when no cascade data.
pub(crate) fn inline_block_bg_of<NodeId: Copy + Eq + Hash>(
    styles: &StylePlane<NodeId>,
    id: NodeId,
) -> [f32; 4] {
    let Some(entry) = styles.get(id) else {
        return [0.0; 4];
    };
    let Some(data) = entry.borrow_data() else {
        return [0.0; 4];
    };
    let primary = data.styles.primary();
    let current = primary.get_inherited_text().color;
    let absolute = primary
        .get_background()
        .background_color
        .resolve_to_absolute(&current);
    *absolute.into_srgb_legacy().raw_components()
}

/// Read an element's cascaded `text-decoration-color` as straight RGBA in
/// `[0, 1]`, resolving the default `currentColor` against the element's own
/// `color`. `None` when the cascade hasn't run.
pub(crate) fn text_decoration_color_of<NodeId: Copy + Eq + Hash>(
    styles: &StylePlane<NodeId>,
    id: NodeId,
) -> Option<[f32; 4]> {
    let entry = styles.get(id)?;
    let data = entry.borrow_data()?;
    let primary = data.styles.primary();
    let current = primary.get_inherited_text().color;
    let absolute = primary
        .get_text()
        .text_decoration_color
        .resolve_to_absolute(&current);
    let srgb = absolute.into_srgb_legacy();
    Some(*srgb.raw_components())
}

/// How a list item's marker is rendered, from its cascaded `list-style-type`.
/// A bullet carries its glyph; the counter kinds format the item's ordinal.
pub(crate) enum MarkerKind {
    Bullet(&'static str),
    Decimal,
    LowerAlpha,
    UpperAlpha,
    LowerRoman,
    UpperRoman,
}

/// Read an element's cascaded `font-size` in CSS px. Returns `None`
/// when the cascade hasn't been applied to that element (hand-rolled
/// style fixtures); the caller defaults to `DEFAULT_FONT_SIZE`.
pub(crate) fn font_size_of<NodeId: Copy + Eq + Hash>(
    styles: &StylePlane<NodeId>,
    id: NodeId,
) -> Option<f32> {
    let entry = styles.get(id)?;
    let data = entry.borrow_data()?;
    Some(
        data.styles
            .primary()
            .get_font()
            .font_size
            .computed_size()
            .px(),
    )
}

/// An element's cascaded `letter-spacing` in CSS px (`normal` resolves to 0).
/// Percentages resolve against the font size. `None` when the cascade hasn't run.
pub(crate) fn letter_spacing_of<NodeId: Copy + Eq + Hash>(
    styles: &StylePlane<NodeId>,
    id: NodeId,
) -> Option<f32> {
    use style::values::computed::Length;
    let entry = styles.get(id)?;
    let data = entry.borrow_data()?;
    let primary = data.styles.primary();
    let fs = primary.get_font().font_size.computed_size().px();
    Some(
        primary
            .get_inherited_text()
            .letter_spacing
            .0
            .resolve(Length::new(fs))
            .px(),
    )
}

/// An element's cascaded `word-spacing` in CSS px (`normal` resolves to 0).
/// Percentages resolve against the font size. `None` when the cascade hasn't run.
pub(crate) fn word_spacing_of<NodeId: Copy + Eq + Hash>(
    styles: &StylePlane<NodeId>,
    id: NodeId,
) -> Option<f32> {
    use style::values::computed::Length;
    let entry = styles.get(id)?;
    let data = entry.borrow_data()?;
    let primary = data.styles.primary();
    let fs = primary.get_font().font_size.computed_size().px();
    Some(
        primary
            .get_inherited_text()
            .word_spacing
            .resolve(Length::new(fs))
            .px(),
    )
}

/// Read an element's cascaded `font-family` and collapse the family
/// list to its first entry (probe scope — no fallback-chain walking).
/// Returns `None` when the cascade hasn't run for this element.
pub(crate) fn font_family_of<NodeId: Copy + Eq + Hash>(
    styles: &StylePlane<NodeId>,
    id: NodeId,
) -> Option<FontFamilySpec> {
    use style::values::computed::font::{GenericFontFamily, SingleFontFamily};

    let entry = styles.get(id)?;
    let data = entry.borrow_data()?;
    let primary = data.styles.primary();
    let first = primary.get_font().font_family.families.iter().next()?;
    let spec = match first {
        SingleFontFamily::FamilyName(name) => FontFamilySpec::Named(name.name.to_string()),
        SingleFontFamily::Generic(g) => {
            let kind = match g {
                GenericFontFamily::Serif => GenericFamilyKind::Serif,
                GenericFontFamily::SansSerif => GenericFamilyKind::SansSerif,
                GenericFontFamily::Monospace => GenericFamilyKind::Monospace,
                GenericFontFamily::Cursive => GenericFamilyKind::Cursive,
                GenericFontFamily::Fantasy => GenericFamilyKind::Fantasy,
                // None / SystemUi / other internal generics → sans-serif.
                _ => GenericFamilyKind::SansSerif,
            };
            FontFamilySpec::Generic(kind)
        },
    };
    Some(spec)
}

/// Read an element's cascaded numeric `font-weight` (400 normal, 700
/// bold). `None` when the cascade hasn't run.
pub(crate) fn font_weight_of<NodeId: Copy + Eq + Hash>(
    styles: &StylePlane<NodeId>,
    id: NodeId,
) -> Option<f32> {
    let entry = styles.get(id)?;
    let data = entry.borrow_data()?;
    Some(data.styles.primary().get_font().font_weight.value())
}

/// Whether an element's cascaded `font-style` is non-normal
/// (italic / oblique). `None` when the cascade hasn't run.
pub(crate) fn font_italic_of<NodeId: Copy + Eq + Hash>(
    styles: &StylePlane<NodeId>,
    id: NodeId,
) -> Option<bool> {
    use style::values::computed::font::FontStyle;
    let entry = styles.get(id)?;
    let data = entry.borrow_data()?;
    Some(data.styles.primary().get_font().font_style != FontStyle::NORMAL)
}
