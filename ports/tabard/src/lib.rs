//! Tabard owns portable theme artifacts: Tinct seeds in, typed design tokens
//! and a Livery stylesheet out.
//!
//! The first slice intentionally exposes only Tinct's base palette. It has no
//! host theme struct, icon policy, syntax palette, persistence, or Pelt
//! preview. Those consumers can share the artifact once it is real instead of
//! each inventing their own theme format.

#![doc(html_no_source)]
#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tinct::{Palette, Seeds, Srgb, color_to_hex, derive_palette};

/// DTCG's stable Design Tokens Format Module schema for this output.
pub const DTCG_2025_10_SCHEMA: &str = "https://www.designtokens.org/schemas/2025.10/format.json";

/// Reverse-DNS extension namespace for Tabard's source provenance.
pub const TABARD_EXTENSION_KEY: &str = "org.merely.tabard";

/// An authored theme: the small Tinct seed set plus a human-facing name.
///
/// Tabard derives the normal-contrast palette selected by Seeds::dark.
/// High-contrast profiles, syntax palettes, and product-specific roles remain
/// separate follow-on work.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Theme {
    pub name: String,
    pub seeds: Seeds,
}

impl Theme {
    /// Create a theme from its authored Tinct seed set.
    pub fn new(name: impl Into<String>, seeds: Seeds) -> Self {
        Self {
            name: name.into(),
            seeds,
        }
    }

    /// Derive the base palette owned by the current Tabard artifact.
    pub fn palette(&self) -> Palette {
        derive_palette(&self.seeds)
    }

    /// Emit the typed DTCG 2025.10 document for this theme.
    ///
    /// Every color token carries an explicit color type. The source seed set
    /// and the narrow derivation choice live under Tabard's reverse-DNS
    /// extension so a consumer can retain the provenance without mistaking it
    /// for an interchange requirement.
    pub fn design_tokens(&self) -> DtcgDocument {
        DtcgDocument {
            schema: DTCG_2025_10_SCHEMA.to_owned(),
            color: DtcgColorGroup::from_theme(self, self.palette()),
        }
    }

    /// Serialize the DTCG document as deterministic, pretty JSON.
    pub fn design_tokens_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self.design_tokens())
    }

    /// Emit a Livery author stylesheet with the derived palette at :root.
    ///
    /// The property names intentionally mirror the owned Tinct base roles:
    /// --tabard-color-bg, --tabard-color-surface-2, and so on. A host may
    /// append ordinary author rules which use them through var().
    pub fn css_custom_properties(&self) -> String {
        let mut stylesheet = String::from(":root {\n");
        for role in color_roles(self.palette()) {
            stylesheet.push_str("  --tabard-color-");
            stylesheet.push_str(role.name);
            stylesheet.push_str(": ");
            stylesheet.push_str(&css_color(role.value));
            stylesheet.push_str(";\n");
        }
        stylesheet.push_str("}\n");
        stylesheet
    }
}

/// A typed DTCG document containing Tabard's current color group.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DtcgDocument {
    #[serde(rename = "$schema")]
    pub schema: String,
    pub color: DtcgColorGroup,
}

/// The DTCG color group emitted by Tabard.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DtcgColorGroup {
    #[serde(rename = "$description")]
    pub description: String,
    #[serde(rename = "$extensions")]
    pub extensions: BTreeMap<String, DtcgProvenance>,
    pub bg: DtcgColorToken,
    pub surface: DtcgColorToken,
    #[serde(rename = "surface-2")]
    pub surface_2: DtcgColorToken,
    #[serde(rename = "surface-hover")]
    pub surface_hover: DtcgColorToken,
    #[serde(rename = "text-header")]
    pub text_header: DtcgColorToken,
    pub text: DtcgColorToken,
    #[serde(rename = "text-dim")]
    pub text_dim: DtcgColorToken,
    #[serde(rename = "text-disabled")]
    pub text_disabled: DtcgColorToken,
    pub primary: DtcgColorToken,
    #[serde(rename = "on-primary")]
    pub on_primary: DtcgColorToken,
    pub secondary: DtcgColorToken,
    #[serde(rename = "on-secondary")]
    pub on_secondary: DtcgColorToken,
    pub tertiary: DtcgColorToken,
    #[serde(rename = "on-tertiary")]
    pub on_tertiary: DtcgColorToken,
    pub success: DtcgColorToken,
    pub danger: DtcgColorToken,
}

impl DtcgColorGroup {
    fn from_theme(theme: &Theme, palette: Palette) -> Self {
        let mut extensions = BTreeMap::new();
        extensions.insert(
            TABARD_EXTENSION_KEY.to_owned(),
            DtcgProvenance {
                theme: DtcgThemeSource {
                    name: theme.name.clone(),
                    seeds: theme.seeds,
                },
                derivation: DtcgDerivation {
                    crate_name: "tinct".to_owned(),
                    function: "derive_palette".to_owned(),
                    profile: "normal-contrast".to_owned(),
                },
            },
        );

        Self {
            description: "Tinct-derived base palette authored by Tabard.".to_owned(),
            extensions,
            bg: palette.bg.into(),
            surface: palette.surface.into(),
            surface_2: palette.surface_2.into(),
            surface_hover: palette.surface_hover.into(),
            text_header: palette.text_header.into(),
            text: palette.text.into(),
            text_dim: palette.text_dim.into(),
            text_disabled: palette.text_disabled.into(),
            primary: palette.primary.into(),
            on_primary: palette.on_primary.into(),
            secondary: palette.secondary.into(),
            on_secondary: palette.on_secondary.into(),
            tertiary: palette.tertiary.into(),
            on_tertiary: palette.on_tertiary.into(),
            success: palette.success.into(),
            danger: palette.danger.into(),
        }
    }
}

/// A DTCG color token with an explicit type.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DtcgColorToken {
    #[serde(rename = "$type")]
    pub token_type: DtcgTokenType,
    #[serde(rename = "$value")]
    pub value: DtcgColorValue,
}

impl From<Srgb> for DtcgColorToken {
    fn from(color: Srgb) -> Self {
        Self {
            token_type: DtcgTokenType::Color,
            value: color.into(),
        }
    }
}

/// The only DTCG token type emitted by the first Tabard slice.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum DtcgTokenType {
    #[serde(rename = "color")]
    Color,
}

/// A DTCG sRGB color value with a CSS hexadecimal fallback.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DtcgColorValue {
    #[serde(rename = "colorSpace")]
    pub color_space: String,
    pub components: [f64; 3],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha: Option<f64>,
    pub hex: String,
}

impl From<Srgb> for DtcgColorValue {
    fn from(color: Srgb) -> Self {
        Self {
            color_space: "srgb".to_owned(),
            components: [
                f64::from(color.r) / f64::from(u8::MAX),
                f64::from(color.g) / f64::from(u8::MAX),
                f64::from(color.b) / f64::from(u8::MAX),
            ],
            alpha: (color.a != u8::MAX).then(|| f64::from(color.a) / f64::from(u8::MAX)),
            hex: color_to_hex(color),
        }
    }
}

/// Tabard-specific metadata which records how a DTCG color group was derived.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DtcgProvenance {
    pub theme: DtcgThemeSource,
    pub derivation: DtcgDerivation,
}

/// The authored source carried in the Tabard extension.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DtcgThemeSource {
    pub name: String,
    pub seeds: Seeds,
}

/// The fixed derivation choice behind this first palette artifact.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DtcgDerivation {
    #[serde(rename = "crate")]
    pub crate_name: String,
    pub function: String,
    pub profile: String,
}

#[derive(Clone, Copy)]
struct ColorRole {
    name: &'static str,
    value: Srgb,
}

fn color_roles(palette: Palette) -> [ColorRole; 16] {
    [
        ColorRole {
            name: "bg",
            value: palette.bg,
        },
        ColorRole {
            name: "surface",
            value: palette.surface,
        },
        ColorRole {
            name: "surface-2",
            value: palette.surface_2,
        },
        ColorRole {
            name: "surface-hover",
            value: palette.surface_hover,
        },
        ColorRole {
            name: "text-header",
            value: palette.text_header,
        },
        ColorRole {
            name: "text",
            value: palette.text,
        },
        ColorRole {
            name: "text-dim",
            value: palette.text_dim,
        },
        ColorRole {
            name: "text-disabled",
            value: palette.text_disabled,
        },
        ColorRole {
            name: "primary",
            value: palette.primary,
        },
        ColorRole {
            name: "on-primary",
            value: palette.on_primary,
        },
        ColorRole {
            name: "secondary",
            value: palette.secondary,
        },
        ColorRole {
            name: "on-secondary",
            value: palette.on_secondary,
        },
        ColorRole {
            name: "tertiary",
            value: palette.tertiary,
        },
        ColorRole {
            name: "on-tertiary",
            value: palette.on_tertiary,
        },
        ColorRole {
            name: "success",
            value: palette.success,
        },
        ColorRole {
            name: "danger",
            value: palette.danger,
        },
    ]
}

fn css_color(color: Srgb) -> String {
    if color.a == u8::MAX {
        return color_to_hex(color);
    }

    format!(
        "rgba({}, {}, {}, {})",
        color.r,
        color.g,
        color.b,
        css_alpha(color.a)
    )
}

fn css_alpha(alpha: u8) -> String {
    let mut value = format!("{:.6}", f64::from(alpha) / f64::from(u8::MAX));
    while value.ends_with('0') {
        value.pop();
    }
    if value.ends_with('.') {
        value.pop();
    }
    value
}
