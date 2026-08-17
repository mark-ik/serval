//! The browser event source for a Cambium application host.
//!
//! The scion to [`cambium_rootstock`]'s stock. Everything a Cambium
//! application is made of already builds for `wasm32-unknown-unknown`: the
//! retained layout over the runner's DOM, hit testing, input routing, focus
//! and spatial navigation, frame pacing, accessibility projection. What was
//! missing was something to tell it what happened, and something to present
//! onto. That is this crate, and it is deliberately small.
//!
//! Three pieces, each answering one seam the host defines:
//!
//! | seam | here | on the desktop |
//! |---|---|---|
//! | [`Surface`] | [`WebSurface`], a canvas | a winit window's surface |
//! | [`HostWindow`] | [`WebWindow`] | the winit window |
//! | [`Accessibility`] | [`DomAccessibility`] | an AccessKit tree |
//!
//! ## The crate compiles to nothing off wasm
//!
//! Its lib root is `#![cfg(target_arch = "wasm32")]`, so a native
//! `cargo check --workspace` sees an empty crate rather than a red one. That
//! is deliberate and it has a cost worth stating: a green workspace check says
//! nothing about this crate, because a target cargo never builds cannot fail.
//! Check it for the target it is for:
//!
//! ```text
//! cargo check -p cambium-genet-web-host --target wasm32-unknown-unknown
//! ```
#![cfg(target_arch = "wasm32")]

mod a11y;
mod input;
mod mount;
mod surface;

pub use a11y::DomAccessibility;
pub use input::{
    CompositionKind, composition_from_dom, key_press_from_dom, modifiers_from_dom,
    wheel_delta_from_dom,
};
pub use mount::{Mounted, mount};
pub use surface::{WebSurface, WebWindow};

/// How far one wheel line scrolls, in logical pixels.
///
/// The DOM may report a wheel notch in lines rather than pixels and does not
/// say how tall a line is. Firefox reports lines by default where Chromium
/// reports pixels, so without this the same gesture scrolls a different
/// distance per browser.
pub const WHEEL_LINE_PX: f32 = 16.0;

/// How far one wheel page scrolls, in logical pixels.
///
/// A fallback: `DOM_DELTA_PAGE` is rare, and a viewport-relative figure would
/// need a viewport this constant does not have.
pub const WHEEL_PAGE_PX: f32 = 400.0;
