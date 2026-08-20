use malloc_size_of_derive::MallocSizeOf;
use serde::{Deserialize, Serialize};

/// The three non-alpha components of an absolute CSS color.
#[derive(Clone, Copy, Debug, Deserialize, MallocSizeOf, PartialEq, Serialize)]
#[repr(C)]
pub struct ColorComponents(pub f32, pub f32, pub f32);

/// A color space supported by absolute CSS colors.
#[derive(Clone, Copy, Debug, Deserialize, Eq, MallocSizeOf, PartialEq, Serialize)]
#[repr(u8)]
pub enum ColorSpace {
    Srgb = 0,
    Hsl,
    Hwb,
    Lab,
    Lch,
    Oklab,
    Oklch,
    SrgbLinear,
    DisplayP3,
    DisplayP3Linear,
    A98Rgb,
    ProphotoRgb,
    Rec2020,
    XyzD50,
    XyzD65,
}

/// Serialization flags retained with an absolute CSS color.
#[derive(Clone, Copy, Debug, Default, Deserialize, MallocSizeOf, PartialEq, Serialize)]
#[repr(transparent)]
pub struct ColorFlags(pub u8);

impl ColorFlags {
    pub const C0_IS_NONE: u8 = 1 << 0;
    pub const C1_IS_NONE: u8 = 1 << 1;
    pub const C2_IS_NONE: u8 = 1 << 2;
    pub const ALPHA_IS_NONE: u8 = 1 << 3;
    pub const IS_LEGACY_SRGB: u8 = 1 << 4;
}

/// A resolved CSS color suitable for canvas messages and other paint-tier
/// boundaries.
#[derive(Clone, Copy, Debug, Deserialize, MallocSizeOf, PartialEq, Serialize)]
#[repr(C)]
pub struct AbsoluteColor {
    pub components: ColorComponents,
    pub alpha: f32,
    pub color_space: ColorSpace,
    pub flags: ColorFlags,
}

impl AbsoluteColor {
    pub const TRANSPARENT_BLACK: Self = Self {
        components: ColorComponents(0.0, 0.0, 0.0),
        alpha: 0.0,
        color_space: ColorSpace::Srgb,
        flags: ColorFlags(ColorFlags::IS_LEGACY_SRGB),
    };

    pub const BLACK: Self = Self {
        components: ColorComponents(0.0, 0.0, 0.0),
        alpha: 1.0,
        color_space: ColorSpace::Srgb,
        flags: ColorFlags(ColorFlags::IS_LEGACY_SRGB),
    };

    pub const WHITE: Self = Self {
        components: ColorComponents(1.0, 1.0, 1.0),
        alpha: 1.0,
        color_space: ColorSpace::Srgb,
        flags: ColorFlags(ColorFlags::IS_LEGACY_SRGB),
    };

    pub const fn from_parts(
        components: ColorComponents,
        alpha: f32,
        color_space: ColorSpace,
        flags: ColorFlags,
    ) -> Self {
        Self {
            components,
            alpha,
            color_space,
            flags,
        }
    }

    pub fn srgb_legacy(red: u8, green: u8, blue: u8, alpha: f32) -> Self {
        Self {
            components: ColorComponents(
                red as f32 / 255.0,
                green as f32 / 255.0,
                blue as f32 / 255.0,
            ),
            alpha: alpha.clamp(0.0, 1.0),
            color_space: ColorSpace::Srgb,
            flags: ColorFlags(ColorFlags::IS_LEGACY_SRGB),
        }
    }

    pub fn is_transparent(&self) -> bool {
        self.flags.0 & ColorFlags::ALPHA_IS_NONE != 0 || self.alpha == 0.0
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, MallocSizeOf, PartialEq, Serialize)]
pub struct ColorF {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl ColorF {
    pub const BLACK: Self = Self::new(0.0, 0.0, 0.0, 1.0);
    pub const TRANSPARENT: Self = Self::new(0.0, 0.0, 0.0, 0.0);
    pub const WHITE: Self = Self::new(1.0, 1.0, 1.0, 1.0);

    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }
}
