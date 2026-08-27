/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Desktop host contracts for Pelt.
//!
//! This crate is the destination for winit windows, input translation, native
//! dialogs, filesystem integration, and platform event-loop glue. It stays
//! above `genet-host-api` and below the UI chrome crate.

#[cfg(all(feature = "livery", target_os = "windows"))]
mod dx12_surface;
#[cfg(feature = "livery")]
mod frisket_surface;
mod profile;
#[cfg(feature = "present")]
mod receipt_capture;
#[cfg(all(feature = "livery", target_os = "windows"))]
mod scrying_receipt;
mod static_viewer;
#[cfg(feature = "livery")]
mod workspace_viewer;

/// The host of an absolute URL, without userinfo or port. `None` for anything
/// without an authority, which includes every local filesystem path.
///
/// Pelt names things after the document, and falls back to the URL when a
/// format carries no title: gemini, gopher, finger and nex carry none at all.
/// Both fallbacks (a tab in `tile_surface`, a window in `static_viewer`) want
/// the host, so it lives here rather than in whichever one is compiled in.
pub(crate) fn url_host(url: &str) -> Option<&str> {
    let authority = url.split_once("://")?.1.split(['/', '\\']).next()?;
    let authority = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    // An IPv6 literal is bracketed and its colons are part of the address.
    let host = if authority.starts_with('[') {
        authority.split_inclusive(']').next().unwrap_or(authority)
    } else {
        authority
            .split_once(':')
            .map_or(authority, |(host, _)| host)
    };
    (!host.is_empty()).then_some(host)
}

#[cfg(any(feature = "scripted", feature = "smolweb"))]
pub(crate) mod href {
    pub use genet_documents::resolve_href;
}

#[cfg(feature = "smolweb")]
mod smolweb_glue;
#[cfg(feature = "smolweb")]
pub use genet_documents::SmolwebDocument;
// Re-exported so a host that builds a `SmolwebDocument` can name its compatibility
// theme and, for the App theme, supply a palette.
#[cfg(feature = "smolweb")]
pub use genet_documents::{SmolwebPalette, SmolwebTheme};
#[cfg(feature = "smolweb")]
pub use smolweb_glue::{run_smolweb_receipt, run_smolweb_viewer};

#[cfg(feature = "scripted")]
mod scripted;

#[cfg(all(feature = "present", feature = "scripted"))]
mod scripted_viewer;

// (STRUCTURAL_SHEET moved to genet-documents with the lanes; genet-scripted
// keeps its own copy, as before.)

#[cfg(feature = "macos-present")]
mod smoke_macos;
#[cfg(feature = "netrender")]
mod smoke_netrender;
#[cfg(feature = "linux-present")]
mod smoke_wayland;
#[cfg(feature = "netrender")]
mod smoke_webgl;
#[cfg(feature = "windows-present")]
mod smoke_windows;

#[cfg(any(feature = "scripted", feature = "smolweb"))]
pub use href::resolve_href;
pub use pelt_core::{PeltClock, PeltController, PeltControllerConfig, PeltHostEffect};
pub use profile::{DesktopHostProfile, WindowingMode};
#[cfg(feature = "macos-present")]
pub use smoke_macos::{
    MacosCALayerPresentSmokeConfig, MacosCALayerPresentSmokeOutcome,
    run_macos_calayer_present_smoke,
};
#[cfg(feature = "netrender")]
pub use smoke_netrender::{NetrenderSmokeOutcome, run_netrender_smoke};
#[cfg(feature = "linux-present")]
pub use smoke_wayland::{
    WaylandPresentSmokeConfig, WaylandPresentSmokeOutcome, run_wayland_subsurface_present_smoke,
};
#[cfg(feature = "netrender")]
pub use smoke_webgl::{WebGlWgpuSmokeOutcome, run_webgl_wgpu_smoke};
#[cfg(feature = "windows-present")]
pub use smoke_windows::{
    WindowsDxgiPresentSmokeConfig, WindowsDxgiPresentSmokeOutcome, run_windows_dxgi_present_smoke,
};
#[cfg(feature = "livery")]
pub use static_viewer::run_livery_viewer;
#[cfg(feature = "reader")]
pub use static_viewer::run_reader_viewer;
pub use static_viewer::{
    ProductReceipt, ProductReceiptOutcome, StaticProductReceipt, StaticProductReceiptOutcome,
    StaticViewerConfig, StaticViewerOutcome, run_static_viewer,
};
#[cfg(feature = "livery")]
pub use workspace_viewer::{
    WorkspaceViewerConfig, WorkspaceViewerOutcome, run_livery_workspace_viewer,
};
// `ScriptResourceFetcher` is `genet_scripted::ResourceFetcher` (the external-script
// byte seam `ScriptedDocument::from_body` takes), distinct from `genet_host_api::
// ResourceFetcher` (the shell-level fetch contract); re-exported so a host can impl
// it without a direct `genet-scripted` dep.
#[cfg(feature = "scripted")]
pub use scripted::{ScriptResourceFetcher, ScriptedDocument, ScriptedEngine};
// The host installs a cookie store on a scripted document (e.g. meerkat's session jar)
// for `document.cookie`; re-export the seam so the host can name it without a direct
// `script-runtime-api` dep. (Render ladder 2c.)
#[cfg(feature = "scripted")]
pub use script_runtime_api::CookieProvider;
// The headless-scripted-DOM scrape (`ScriptedDocument::extract`) returns these; re-export
// so the host names the post-JS extract without exposing its scripted runtime. (Phase 4.)
#[cfg(feature = "scripted")]
pub use fleece::{Article, Block as ArticleBlock, Heading, Inline, Link, Metadata, PageExtract};
#[cfg(all(feature = "present", feature = "scripted"))]
pub use scripted_viewer::run_scripted_viewer;
