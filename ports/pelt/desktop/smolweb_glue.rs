/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Windowed-shell glue for the smolweb lane: the [`SmolwebDocument`]
//! (a `genet-documents` type since the session-engines split) as pelt's
//! `ViewerContent` / `BrowsableContent`, plus the standalone viewer entry.
//! Pelt impls its own local traits for the foreign type.

use genet_documents::{LocalFetcher, SmolwebDocument, SmolwebSessionEngine, SmolwebTheme};
use genet_host_api::ResourceFetcher;
use inker::{SessionRegistry, SessionScrollKey, SessionSpawnRequest, SurfaceEngineRegistry};
use netrender::Scene;
use pelt_core::{PeltController, PeltControllerConfig};

use crate::static_viewer::ViewerScrollKey;

const GEMTEXT_RECEIPT_START_URL: &str = "gemini://pelt.test/p5-gemtext/index.gmi";
const GEMTEXT_RECEIPT_NEXT_URL: &str = "gemini://pelt.test/p5-gemtext/next.gmi";

/// Deterministic transport for the receipt's post-click navigation. The first
/// document is host-held in `SessionSpawnRequest::body`; this fetcher is only
/// consulted when the retained controller opens another Gemtext resource.
#[derive(Clone, Copy, Debug, Default)]
struct GemtextReceiptFetcher;

impl ResourceFetcher for GemtextReceiptFetcher {
    fn fetch(&self, url: &str) -> Option<Vec<u8>> {
        match url {
            GEMTEXT_RECEIPT_START_URL => Some(
                include_str!("../examples/p5-gemtext/index.gmi")
                    .as_bytes()
                    .to_vec(),
            ),
            GEMTEXT_RECEIPT_NEXT_URL => Some(
                include_str!("../examples/p5-gemtext/next.gmi")
                    .as_bytes()
                    .to_vec(),
            ),
            _ => None,
        }
    }
}

fn gemtext_receipt_controller(
    config: &crate::StaticViewerConfig,
    body: &str,
) -> Result<PeltController<Scene>, String> {
    let (width, height) = config.size.unwrap_or((800, 600));
    let mut registry: SessionRegistry<Scene> = SessionRegistry::new();
    registry.register(Box::new(SmolwebSessionEngine::new(
        inker::routing::ENGINE_NEMATIC_GEMTEXT,
        GemtextReceiptFetcher,
        SmolwebTheme::default(),
    )));
    let request = SessionSpawnRequest::new(&config.url)
        .with_body(body)
        .with_content_type("text/gemini")
        .with_viewport(width, height);
    PeltController::new(
        registry,
        SurfaceEngineRegistry::new(),
        PeltControllerConfig::from_request(inker::routing::ENGINE_NEMATIC_GEMTEXT, request),
        crate::static_viewer::ViewerClock::new(),
    )
}

fn viewer_scroll_key(key: ViewerScrollKey) -> Option<SessionScrollKey> {
    Some(match key {
        ViewerScrollKey::Up => SessionScrollKey::LineUp,
        ViewerScrollKey::Down => SessionScrollKey::LineDown,
        ViewerScrollKey::PageUp => SessionScrollKey::PageUp,
        ViewerScrollKey::PageDown => SessionScrollKey::PageDown,
        ViewerScrollKey::Home => SessionScrollKey::Home,
        ViewerScrollKey::End => SessionScrollKey::End,
        ViewerScrollKey::Left | ViewerScrollKey::Right => return None,
    })
}

/// The smolweb document as windowed [`ViewerContent`](crate::static_viewer::windowed::ViewerContent),
/// so it plugs into the shared winit shell like the static document. v1 is read-only:
/// no scroll yet, and in-window link navigation is the chrome/tile lanes' job (the
/// bare viewer has no history), so a click is a no-op here.
impl crate::static_viewer::windowed::ViewerContent for SmolwebDocument {
    fn frame(&mut self, width: u32, height: u32) -> Scene {
        SmolwebDocument::frame(self, width, height)
    }
    fn scroll_by(&mut self, dx: f32, dy: f32) -> bool {
        SmolwebDocument::scroll_by(self, dx, dy)
    }
    fn scroll_at(&mut self, x: f32, y: f32, dx: f32, dy: f32) -> bool {
        SmolwebDocument::scroll_at(self, x, y, dx, dy)
    }
    fn scroll_for_key(&mut self, key: ViewerScrollKey) -> bool {
        viewer_scroll_key(key).is_some_and(|key| SmolwebDocument::scroll_for_key(self, key))
    }
    fn click_at(&mut self, _x: f32, _y: f32) -> bool {
        // The bare viewer has no history; navigation is the chrome browser's job
        // (see the `BrowsableContent` impl below), so a click is a no-op here.
        false
    }
}

/// Open a window and present the smolweb capsule at `config.url`, themed per-site by
/// default (the Lagrange look). The smolweb twin of
/// [`run_static_viewer`](crate::run_static_viewer); a bad URL fails fast before the
/// window opens.
pub fn run_smolweb_viewer(
    config: crate::StaticViewerConfig,
) -> Result<crate::StaticViewerOutcome, String> {
    let doc = SmolwebDocument::load(&LocalFetcher, &config.url, SmolwebTheme::default())?;
    crate::static_viewer::run_headed_with(config, doc)
}

/// Present the protocol-native P5 receipt through the registered Gemtext
/// session engine and Pelt's retained controller. `body` is caller-held source;
/// only the receipt's subsequent navigation reaches the fixed fetcher above.
pub fn run_smolweb_receipt(
    config: crate::StaticViewerConfig,
    body: &str,
) -> Result<crate::StaticViewerOutcome, String> {
    if config.product_receipt != Some(crate::ProductReceipt::Gemtext) {
        return Err("the smolweb receipt entrypoint requires product receipt gemtext".to_owned());
    }
    let controller = gemtext_receipt_controller(&config, body)?;
    crate::static_viewer::run_headed_with(
        config,
        crate::static_viewer::ControllerViewerContent::new(
            controller,
            Some("Nematic Gemtext · held source".to_owned()),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::static_viewer::windowed::ViewerContent;
    use crate::{ProductReceipt, StaticViewerConfig, WindowingMode};
    use genet_host_api::EngineProfile;

    #[test]
    fn gemtext_product_receipt_routes_held_body_and_controller_navigation() {
        let body = include_str!("../examples/p5-gemtext/index.gmi");
        let config = StaticViewerConfig::new(
            EngineProfile::Livery,
            WindowingMode::Headed,
            GEMTEXT_RECEIPT_START_URL,
        )
        .with_product_receipt(ProductReceipt::Gemtext, "unused.png");
        let mut controller =
            gemtext_receipt_controller(&config, body).expect("Gemtext controller should spawn");

        assert_eq!(
            controller.engine_id(),
            inker::routing::ENGINE_NEMATIC_GEMTEXT
        );
        assert_eq!(controller.request().body.as_deref(), Some(body));
        assert_eq!(
            controller.request().content_type.as_deref(),
            Some("text/gemini")
        );
        assert_eq!(
            controller.title().as_deref(),
            Some("P5 native Gemtext receipt")
        );
        let report = controller.inspect().expect("initial Gemtext report");
        assert_eq!(report.title.as_deref(), Some("P5 native Gemtext receipt"));
        assert!(
            report
                .links
                .iter()
                .any(|url| url == GEMTEXT_RECEIPT_NEXT_URL)
        );
        let scene = controller.frame(960, 640);
        assert!(scene.ops.iter().any(|operation| matches!(
            operation,
            netrender::SceneOp::GlyphRun(run) if !run.glyphs.is_empty()
        )));
        assert!(controller.links().iter().any(|link| {
            link.url == GEMTEXT_RECEIPT_NEXT_URL
                && link.rect.into_iter().all(f32::is_finite)
                && link.rect[2] > 0.0
                && link.rect[3] > 0.0
        }));

        let mut content = crate::static_viewer::ControllerViewerContent::new(
            controller,
            Some("Nematic Gemtext · held source".to_owned()),
        );
        let _ = content.frame(960, 640);
        assert_eq!(
            content
                .drive_product_receipt(ProductReceipt::Gemtext)
                .expect("Gemtext receipt should pass"),
            "held Gemtext body lowered through Nematic; retained native link navigated through PeltController"
        );
        assert_eq!(content.address(), Some(GEMTEXT_RECEIPT_NEXT_URL));
        assert_eq!(
            content.title().as_deref(),
            Some("P5 native navigation arrived")
        );
    }
}
