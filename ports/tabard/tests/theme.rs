// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

use genet_livery::{InteractionStates, StyleSet, resolve_styles};
use genet_static_dom::StaticDocument;
use layout_dom_api::LayoutDom;
use livery::{media::Device, values::Color};
use tabard::{DTCG_2025_10_SCHEMA, DtcgDocument, DtcgTokenType, TABARD_EXTENSION_KEY, Theme};
use tinct::{Seeds, Srgb, color_to_hex, contrast};

fn theme() -> Theme {
    Theme::new(
        "Ink",
        Seeds {
            primary: Srgb::rgb(0x33, 0x66, 0xC8),
            secondary: Srgb::rgb(0x2E, 0x9D, 0xA6),
            tertiary: Srgb::rgb(0xE0, 0xA8, 0x46),
            neutral: Srgb::rgb(0x10, 0x14, 0x22),
            text_header: None,
            text_body: None,
            success: Srgb::rgb(0x4F, 0xB3, 0x6E),
            danger: Srgb::rgb(0xD5, 0x4E, 0x4E),
            dark: true,
        },
    )
}

#[test]
fn dtcg_document_preserves_tabard_provenance() {
    let theme = theme();
    let expected = theme.design_tokens();
    let json = theme.design_tokens_json().expect("DTCG JSON");
    let document: DtcgDocument = serde_json::from_str(&json).expect("typed DTCG JSON");

    assert_eq!(document.schema, DTCG_2025_10_SCHEMA);
    assert_eq!(document.color.bg.token_type, DtcgTokenType::Color);
    assert_eq!(document.color.bg.value.color_space, "srgb");
    assert_eq!(
        document.color.bg.value.hex,
        color_to_hex(theme.palette().bg)
    );
    for (actual, expected) in document
        .color
        .bg
        .value
        .components
        .iter()
        .zip(expected.color.bg.value.components)
    {
        assert!((actual - expected).abs() < 1e-12);
    }
    assert_eq!(document.color.extensions, expected.color.extensions);

    let provenance = document
        .color
        .extensions
        .get(TABARD_EXTENSION_KEY)
        .expect("Tabard provenance extension");
    assert_eq!(provenance.theme.name, "Ink");
    assert_eq!(provenance.theme.seeds, theme.seeds);
    assert_eq!(provenance.derivation.crate_name, "tinct");
    assert_eq!(provenance.derivation.function, "derive_palette");
    assert_eq!(provenance.derivation.profile, "normal-contrast");
}

#[test]
fn token_and_css_output_are_deterministic_and_map_every_base_role() {
    let theme = theme();
    assert_eq!(
        theme.design_tokens_json().expect("first JSON"),
        theme.design_tokens_json().expect("second JSON")
    );
    assert_eq!(theme.css_custom_properties(), theme.css_custom_properties());

    let css = theme.css_custom_properties();
    let palette = theme.palette();
    let expected = [
        ("bg", palette.bg),
        ("surface", palette.surface),
        ("surface-2", palette.surface_2),
        ("surface-hover", palette.surface_hover),
        ("text-header", palette.text_header),
        ("text", palette.text),
        ("text-dim", palette.text_dim),
        ("text-disabled", palette.text_disabled),
        ("primary", palette.primary),
        ("on-primary", palette.on_primary),
        ("secondary", palette.secondary),
        ("on-secondary", palette.on_secondary),
        ("tertiary", palette.tertiary),
        ("on-tertiary", palette.on_tertiary),
        ("success", palette.success),
        ("danger", palette.danger),
    ];
    for (name, color) in expected {
        assert_eq!(
            css_value(&css, name),
            color_to_hex(color),
            "CSS custom property for {name}"
        );
    }
}

#[test]
fn exported_palette_keeps_tinct_contrast_roles() {
    let palette = theme().palette();
    assert!(contrast(palette.text, palette.surface) >= 4.5);
    assert!(contrast(palette.on_primary, palette.primary) >= 3.0);
    assert!(contrast(palette.on_secondary, palette.secondary) >= 3.0);
    assert!(contrast(palette.on_tertiary, palette.tertiary) >= 3.0);
}

#[test]
fn livery_resolves_the_emitted_custom_properties() {
    let theme = theme();
    let stylesheet = format!(
        "{}\n.probe {{ color: var(--tabard-color-text); background-color: var(--tabard-color-surface); }}",
        theme.css_custom_properties()
    );
    let styles = StyleSet::cambium(&[&stylesheet]);
    assert!(
        styles.diagnostics().is_empty(),
        "{:?}",
        styles.diagnostics()
    );

    let document = StaticDocument::parse("<html><body><p class=\"probe\">Tabard</p></body></html>");
    let probe = document
        .first_with_class(document.document(), "probe")
        .expect("probe element");
    let plane = resolve_styles(
        &document,
        &styles,
        &Device::screen(320.0, 200.0),
        &InteractionStates::default(),
    );
    let computed = plane.get(probe).expect("computed probe style");
    let palette = theme.palette();

    assert_eq!(
        computed.color,
        color_to_hex(palette.text).parse::<Color>().unwrap()
    );
    assert_eq!(
        computed.background_color,
        color_to_hex(palette.surface).parse::<Color>().unwrap()
    );
}

fn css_value<'a>(stylesheet: &'a str, name: &str) -> &'a str {
    let declaration = format!("--tabard-color-{name}: ");
    stylesheet
        .lines()
        .find_map(|line| line.trim().strip_prefix(&declaration))
        .and_then(|value| value.strip_suffix(';'))
        .unwrap_or_else(|| panic!("custom property {name}"))
}
