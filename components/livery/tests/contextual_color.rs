//! C0 receipts for the contextual color seam.
//!
//! These assert at the cascade layer on purpose. A `SpecifiedColor` CSSOM
//! round trip proves the parser retains a contextual expression; it does not
//! prove the expression survives into a computed value. The plan for the rest
//! of the seam is `docs/2026-07-28_livery_contextual_color_computation_plan.md`.

use livery::ComputedValues;
use livery::cascade::{
    CascadeLayer, ColorComputeContext, MatchedCustomDeclaration, MatchedDeclaration, Origin,
    Specificity, cascade, cascade_with_color_context, cascade_with_custom, parse_declaration_block,
};
use livery::media::SystemPalette;
use livery::values::{BackgroundImage, BoxShadow, Color, ColorScheme, ComputedColor, SystemColor};

/// Every contextual family named by the plan, written so its correct result is
/// exactly the element's used foreground.
const FOREGROUND_EQUIVALENT: &[(&str, &str)] = &[
    (
        "color-mix()",
        "color-mix(in srgb, currentcolor 100%, white)",
    ),
    ("relative color", "rgb(from currentcolor r g b)"),
    ("alpha()", "alpha(from currentcolor / 1)"),
    ("color-layers()", "color-layers(currentcolor)"),
];

fn matched(css: &str, source_order: u64) -> MatchedDeclaration {
    let mut block = parse_declaration_block(css);
    assert!(block.errors.is_empty(), "{css}: {:?}", block.errors);
    assert_eq!(block.declarations.len(), 1, "{css}");
    MatchedDeclaration {
        declaration: block.declarations.remove(0),
        origin: Origin::Author,
        layer: CascadeLayer::Unlayered,
        specificity: Specificity(1),
        source_order,
    }
}

fn computed(parent: Option<&ComputedValues>, css: &[&str]) -> ComputedValues {
    let declarations: Vec<MatchedDeclaration> = css
        .iter()
        .enumerate()
        .map(|(index, one)| matched(one, index as u64))
        .collect();
    cascade(parent, declarations)
}

fn computed_with_color_context(
    parent: Option<&ComputedValues>,
    context: ColorComputeContext,
    css: &[&str],
) -> ComputedValues {
    let declarations: Vec<MatchedDeclaration> = css
        .iter()
        .enumerate()
        .map(|(index, one)| matched(one, index as u64))
        .collect();
    cascade_with_color_context(parent, declarations, context)
}

fn palette_context() -> ColorComputeContext {
    let mut palette = SystemPalette::default();
    palette.set(
        ColorScheme::Light,
        SystemColor::Canvas,
        "#102030".parse().unwrap(),
    );
    palette.set(
        ColorScheme::Dark,
        SystemColor::Canvas,
        "#d0e0f0".parse().unwrap(),
    );
    palette.set(
        ColorScheme::Light,
        SystemColor::CanvasText,
        "#304050".parse().unwrap(),
    );
    palette.set(
        ColorScheme::Dark,
        SystemColor::CanvasText,
        "#f0e0d0".parse().unwrap(),
    );
    ColorComputeContext::new(ColorScheme::Light, palette)
}

/// A valid contextual declaration must reach the cascade at all. This is the
/// weakest possible claim in the seam and the one that gates every other test
/// here, which is why it fails first: measured on 2026-07-31, every family
/// above is rejected as `DeclarationError { kind: InvalidValue }`, because the
/// declaration parser targets the eager `Color` leaf and that leaf cannot hold
/// a contextual expression.
///
/// The same expressions round trip correctly through `SpecifiedColor` in
/// `tests/color.rs`. That is the false receipt this file exists to rule out.
#[test]
fn contextual_declarations_are_not_discarded_at_parse_time() {
    for (family, value) in FOREGROUND_EQUIVALENT {
        let css = format!("background-color: {value}");
        let block = parse_declaration_block(&css);
        assert!(block.errors.is_empty(), "{family}: {:?}", block.errors);
        assert_eq!(
            block.declarations.len(),
            1,
            "{family} produced no declaration"
        );
    }

    let block = parse_declaration_block("background-color: contrast-color(currentcolor)");
    assert!(
        block.errors.is_empty(),
        "contrast-color(): {:?}",
        block.errors
    );
    assert_eq!(block.declarations.len(), 1);
}

#[test]
fn c1_keeps_one_contextual_value_through_cascade_paths_and_nested_owners() {
    let expression = "color-mix(in srgb, currentcolor 100%, white)";
    let parent = computed(None, &[&format!("background-color: {expression}")]);
    assert!(matches!(
        &parent.background_color,
        ComputedColor::Expression(_)
    ));

    let inherited = computed(
        Some(&parent),
        &["color: #b83535", "background-color: inherit"],
    );
    assert_eq!(inherited.background_color, parent.background_color);

    let foreground_parent = computed(None, &[&format!("color: {expression}")]);
    let inherited_foreground = computed(Some(&foreground_parent), &["color: inherit"]);
    let unset_foreground = computed(Some(&foreground_parent), &["color: unset"]);
    assert_eq!(inherited_foreground.color, foreground_parent.color);
    assert_eq!(unset_foreground.color, foreground_parent.color);

    let mut block = parse_declaration_block(&format!(
        "--accent: {expression}; background-color: var(--accent)"
    ));
    assert!(block.errors.is_empty(), "{:#?}", block.errors);
    let declarations = block
        .declarations
        .drain(..)
        .enumerate()
        .map(|(index, declaration)| MatchedDeclaration {
            declaration,
            origin: Origin::Author,
            layer: CascadeLayer::Unlayered,
            specificity: Specificity(1),
            source_order: index as u64,
        });
    let custom = block
        .custom
        .drain(..)
        .enumerate()
        .map(|(index, declaration)| MatchedCustomDeclaration {
            declaration,
            origin: Origin::Author,
            layer: CascadeLayer::Unlayered,
            specificity: Specificity(1),
            source_order: index as u64,
        });
    let (substituted, _) = cascade_with_custom(None, None, declarations, custom);
    assert!(matches!(
        substituted.background_color,
        ComputedColor::Expression(_)
    ));

    let gradient = "linear-gradient(color-mix(in srgb, currentcolor 100%, white), red)"
        .parse::<BackgroundImage>()
        .unwrap();
    assert!(matches!(
        gradient,
        BackgroundImage::LinearGradient {
            from: ComputedColor::Expression(_),
            ..
        }
    ));
    let shadow = "0 0 alpha(from currentcolor / 0.5)"
        .parse::<BoxShadow>()
        .unwrap();
    assert!(matches!(
        shadow,
        BoxShadow::Value(ref value) if matches!(value.color, ComputedColor::Expression(_))
    ));

    let decoration = computed(None, &["text-decoration-color: color-layers(currentcolor)"]);
    assert!(matches!(
        decoration.text_decoration_color,
        ComputedColor::Expression(_)
    ));
}

/// C2's discriminating receipt: direct system colors select the child's used
/// scheme, while an inherited value is already the parent's absolute result.
#[test]
fn c2_system_colors_resolve_under_the_elements_used_scheme_before_inheritance() {
    let context = palette_context();
    let parent = computed_with_color_context(
        None,
        context,
        &[
            "color-scheme: light",
            "color: CanvasText",
            "background-color: Canvas",
        ],
    );
    assert_eq!(parent.color, "#304050".parse::<Color>().unwrap());
    assert_eq!(parent.background_color, "#102030".parse::<Color>().unwrap());

    let direct = computed_with_color_context(
        Some(&parent),
        context,
        &[
            "color-scheme: only dark",
            "color: CanvasText",
            "background-color: Canvas",
            "border-top-color: CanvasText",
            "background-image: linear-gradient(Canvas, CanvasText)",
            "box-shadow: 0 0 Canvas",
        ],
    );
    assert_eq!(direct.color_scheme.to_string(), "only dark");
    assert_eq!(direct.color, "#f0e0d0".parse::<Color>().unwrap());
    assert_eq!(direct.background_color, "#d0e0f0".parse::<Color>().unwrap());
    assert!(matches!(
        direct.border_top_color,
        ComputedColor::Absolute(_)
    ));
    assert!(matches!(
        direct.background_image,
        BackgroundImage::LinearGradient {
            from: ComputedColor::Absolute(_),
            to: ComputedColor::Absolute(_),
        }
    ));
    assert!(matches!(
        direct.box_shadow,
        BoxShadow::Value(ref shadow) if matches!(shadow.color, ComputedColor::Absolute(_))
    ));

    let inherited = computed_with_color_context(
        Some(&parent),
        context,
        &["color-scheme: dark", "background-color: inherit"],
    );
    assert_eq!(inherited.background_color, parent.background_color);
    assert_ne!(inherited.background_color, direct.background_color);
    assert!(matches!(
        inherited.background_color,
        ComputedColor::Absolute(_)
    ));
}

#[test]
#[ignore = "C1: computed color-bearing properties still store the eager Color leaf"]
fn a_contextual_background_resolves_against_the_used_foreground() {
    for (family, value) in FOREGROUND_EQUIVALENT {
        let values = computed(
            None,
            &["color: #3568b8", &format!("background-color: {value}")],
        );
        assert_eq!(
            values.background_color,
            "#3568b8".parse::<Color>().unwrap(),
            "{family} did not resolve against the element's foreground"
        );
    }
}

/// The distinguishing case for `ColorRole`. A non-foreground `currentcolor`
/// expression must stay contextual across inheritance so it re-resolves on a
/// descendant, rather than freezing to the parent's RGBA.
#[test]
#[ignore = "C1/C2: inherited contextual expressions do not survive as expressions"]
fn an_inherited_contextual_background_re_resolves_on_the_child() {
    for (family, value) in FOREGROUND_EQUIVALENT {
        let parent = computed(
            None,
            &["color: #3568b8", &format!("background-color: {value}")],
        );
        let child = computed(Some(&parent), &["color: #b83535"]);
        assert_eq!(
            child.background_color,
            "#b83535".parse::<Color>().unwrap(),
            "{family} froze to the parent's foreground instead of re-resolving"
        );
    }
}

/// `ColorRole::Foreground` is the exception: a `currentcolor` expression on the
/// `color` property resolves against the *inherited* foreground, before the new
/// foreground is itself inherited.
#[test]
fn a_foreground_contextual_color_resolves_against_the_inherited_foreground() {
    for (family, value) in FOREGROUND_EQUIVALENT {
        let parent = computed(None, &["color: #3568b8"]);
        let child = computed(Some(&parent), &[&format!("color: {value}")]);
        assert_eq!(
            child.color.to_srgb(),
            "#3568b8".parse::<Color>().unwrap().to_srgb(),
            "{family} did not resolve against the inherited foreground"
        );
    }
}

/// `contrast-color()` is tested by contrast rather than by a pinned value: its
/// result must track the foreground it is given.
#[test]
#[ignore = "C1: contrast-color() cannot see the element's foreground"]
fn contrast_color_tracks_the_elements_foreground() {
    let on_dark = computed(
        None,
        &[
            "color: #000000",
            "background-color: contrast-color(currentcolor)",
        ],
    );
    let on_light = computed(
        None,
        &[
            "color: #ffffff",
            "background-color: contrast-color(currentcolor)",
        ],
    );
    assert_ne!(
        on_dark.background_color, on_light.background_color,
        "contrast-color() produced the same result for opposite foregrounds"
    );
}

/// The plan's stop rule: a valid expression must never silently become black or
/// the property's initial value.
#[test]
#[ignore = "C3: paint and cascade still fall back for unresolved valid colors"]
fn a_valid_contextual_declaration_never_falls_back_to_the_initial_value() {
    for (family, value) in FOREGROUND_EQUIVALENT {
        let values = computed(
            None,
            &["color: #3568b8", &format!("background-color: {value}")],
        );
        assert_ne!(
            values.background_color,
            Color::TRANSPARENT,
            "{family} fell back to the initial background-color"
        );
    }
}
