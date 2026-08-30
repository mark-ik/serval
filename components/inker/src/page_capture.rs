// Copyright 2026 Mark AB (markik)
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Host-owned requests and engine-reported results for page capture.
//!
//! A host mints the request id and records its own navigation, node, and
//! requested-scale provenance beside it. Engines report only facts they
//! actually know about the captured viewport and applied scale.

use serde::{Deserialize, Serialize};

/// A host-minted id for one capture request on one document surface.
///
/// The id is scoped to its surface. Hosts driving several surfaces retain the
/// surface identity beside this value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PageCaptureRequestId(u64);

impl PageCaptureRequestId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// The portion of a page an engine is asked to capture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageCaptureScope {
    /// The visible viewport at the instant the engine accepts the request.
    Viewport,
}

/// A page-capture request. Its id is minted by the host, never the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageCaptureRequest {
    pub id: PageCaptureRequestId,
    pub scope: PageCaptureScope,
}

impl PageCaptureRequest {
    pub const fn viewport(id: PageCaptureRequestId) -> Self {
        Self {
            id,
            scope: PageCaptureScope::Viewport,
        }
    }
}

/// Coordinates in the document's CSS-pixel space.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CssPoint {
    pub x: f32,
    pub y: f32,
}

/// An extent in CSS pixels.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CssExtent {
    pub width: f32,
    pub height: f32,
}

/// Facts about the captured viewport, as reported by the engine.
///
/// CSS facts are optional because a black-box engine may produce pixels
/// without exposing its scroll position or CSS viewport. Output dimensions are
/// always required and are physical pixels in the returned image.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PageCaptureViewportFacts {
    pub css_scroll_origin: Option<CssPoint>,
    pub css_viewport_extent: Option<CssExtent>,
    pub output_width_pixels: u32,
    pub output_height_pixels: u32,
}

impl PageCaptureViewportFacts {
    /// Builds an honest report for an engine that knows output dimensions but
    /// cannot observe CSS scroll or viewport data.
    pub const fn unknown_css(output_width_pixels: u32, output_height_pixels: u32) -> Self {
        Self {
            css_scroll_origin: None,
            css_viewport_extent: None,
            output_width_pixels,
            output_height_pixels,
        }
    }
}

/// The admitted P1 image representations.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageCaptureImageArtifact {
    Png(Vec<u8>),
}

/// A successful engine-reported page capture.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PageCaptureOutput {
    /// The scope the engine actually produced. It repeats the request instead
    /// of asking hosts to infer result semantics from a pending record.
    pub scope: PageCaptureScope,
    pub image: PageCaptureImageArtifact,
    pub viewport: PageCaptureViewportFacts,
    /// The engine's actual page scale, when it can observe it. This is not the
    /// host's requested scale, which belongs in host provenance.
    pub applied_page_scale: Option<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_facts_keep_unobserved_css_explicit() {
        let facts = PageCaptureViewportFacts::unknown_css(1280, 720);
        assert_eq!(facts.css_scroll_origin, None);
        assert_eq!(facts.css_viewport_extent, None);
        assert_eq!(facts.output_width_pixels, 1280);
        assert_eq!(facts.output_height_pixels, 720);
    }

    #[test]
    fn viewport_request_keeps_the_host_minted_id() {
        let id = PageCaptureRequestId::new(42);
        let request = PageCaptureRequest::viewport(id);
        assert_eq!(request.id, id);
        assert_eq!(request.scope, PageCaptureScope::Viewport);
    }
}
