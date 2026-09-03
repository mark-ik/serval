// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! The smolweb lane as an inker session engine: protocol-native
//! documents over the errand transport.

use std::any::Any;

use genet_host_api::ResourceFetcher;
use inker::DocumentCapabilities;
use inker::DocumentCapabilityStatus;
use inker::session_engine::{
    DocumentSession, SessionClick, SessionEngine, SessionError, SessionLink, SessionScrollKey,
    SessionSpawnRequest,
};
use netrender::Scene;

use crate::{SmolwebDocument, SmolwebInlineMediaPolicy, SmolwebTheme};

#[cfg(feature = "smolweb")]
fn retained_document_capabilities(find_reason: impl Into<String>) -> DocumentCapabilities {
    DocumentCapabilities {
        find_in_page: DocumentCapabilityStatus::unsupported(find_reason),
        page_zoom: DocumentCapabilityStatus::unsupported(
            "smolweb sessions do not expose page zoom",
        ),
        page_capture: DocumentCapabilityStatus::unsupported(
            "retained sessions do not capture rendered pages",
        ),
        navigation: DocumentCapabilityStatus::Partial {
            detail: "the host owns document lineage, policy, and refetch".into(),
        },
    }
}

/// Session engine for the smolweb native lane. One instance per format id
/// (`nematic.gemtext` / `nematic.gopher` / `nematic.feed` today) so routing
/// decisions map directly; the same ids keep their block engines for cards —
/// the kind index reports both and the host picks by surface context.
#[cfg(feature = "smolweb")]
pub struct SmolwebSessionEngine<Fetch> {
    engine_id: String,
    fetcher: Fetch,
    theme: SmolwebTheme,
    inline_media: SmolwebInlineMediaPolicy,
}

#[cfg(feature = "smolweb")]
impl<Fetch> SmolwebSessionEngine<Fetch> {
    pub fn new(engine_id: impl Into<String>, fetcher: Fetch, theme: SmolwebTheme) -> Self {
        Self {
            engine_id: engine_id.into(),
            fetcher,
            theme,
            inline_media: SmolwebInlineMediaPolicy::default(),
        }
    }

    /// Apply a host-owned inline-media policy to documents this engine spawns.
    pub fn with_inline_media(mut self, policy: SmolwebInlineMediaPolicy) -> Self {
        self.inline_media = policy;
        self
    }
}

#[cfg(feature = "smolweb")]
impl<Fetch: ResourceFetcher + Send + Sync> SessionEngine<Scene> for SmolwebSessionEngine<Fetch> {
    fn engine_id(&self) -> &str {
        &self.engine_id
    }

    fn spawn(
        &self,
        request: &SessionSpawnRequest,
    ) -> Result<Box<dyn DocumentSession<Scene>>, SessionError> {
        let doc = match &request.body {
            Some(body) => SmolwebDocument::parse_with_inline_media(
                &request.address,
                body,
                self.theme.clone(),
                self.inline_media,
            ),
            None => SmolwebDocument::load_with_inline_media(
                &self.fetcher,
                &request.address,
                self.theme.clone(),
                self.inline_media,
            )
            .map_err(SessionError::SpawnFailed)?,
        };
        Ok(Box::new(SmolwebDocumentSession {
            doc,
            viewport: request.viewport,
        }))
    }
}

/// The smolweb document as a session. Public so a host that themes per
/// content (meerkat's palette-derived themes) parses the document itself and
/// wraps it; the engine above is the fixed-theme path.
#[cfg(feature = "smolweb")]
pub struct SmolwebDocumentSession {
    doc: SmolwebDocument,
    /// Last framed size: the lane's click/content-height APIs take the
    /// viewport, which the trait carries implicitly through `frame`.
    viewport: (u32, u32),
}

#[cfg(feature = "smolweb")]
impl SmolwebDocumentSession {
    pub fn new(doc: SmolwebDocument, viewport: (u32, u32)) -> Self {
        Self { doc, viewport }
    }

    /// The concrete document, for observation downcasts and host-side
    /// banding/link-table inspection.
    pub fn document_mut(&mut self) -> &mut SmolwebDocument {
        &mut self.doc
    }

    /// Replace an incrementally received body while retaining this session's
    /// viewport and host-owned presentation policy.
    pub fn replace_body(&mut self, url: &str, body: &str) {
        self.doc.replace_body(url, body);
    }
}

#[cfg(feature = "smolweb")]
impl DocumentSession<Scene> for SmolwebDocumentSession {
    fn document_capabilities(&self) -> DocumentCapabilities {
        retained_document_capabilities("Smolweb sessions do not expose document find")
    }

    fn frame(&mut self, width: u32, height: u32) -> Scene {
        self.viewport = (width, height);
        self.doc.frame(width, height)
    }
    fn scroll_by(&mut self, dx: f32, dy: f32) -> bool {
        self.doc.scroll_by(dx, dy)
    }
    fn scroll_at(&mut self, x: f32, y: f32, dx: f32, dy: f32) -> bool {
        self.doc.scroll_at(x, y, dx, dy)
    }
    fn scroll_for_key(&mut self, key: SessionScrollKey) -> bool {
        self.doc.scroll_for_key(key)
    }
    fn scroll_to(&mut self, y: f32) {
        self.doc.scroll_to(y);
    }
    fn click_at(&mut self, x: f32, y: f32) -> SessionClick {
        let (w, h) = self.viewport;
        match self.doc.click_at(x, y, w, h) {
            Some(document_canvas::InteractionKind::Link { url }) => SessionClick::Navigate(url),
            Some(document_canvas::InteractionKind::Submit { target }) => {
                SessionClick::Submit(target)
            },
            None => SessionClick::Miss,
        }
    }
    fn links(&self) -> Vec<SessionLink> {
        self.doc
            .links()
            .into_iter()
            .map(|(url, rect)| SessionLink { url, rect })
            .collect()
    }
    fn content_height(&mut self, width: u32, height: u32) -> u32 {
        self.doc.content_height(width, height)
    }
    fn subresources(&self) -> Vec<String> {
        self.doc.subresources()
    }
    fn provide_subresource(&mut self, url: &str, bytes: &[u8]) -> bool {
        self.doc.provide_subresource(url, bytes)
    }
    fn inspect(&self) -> Option<inker::ContentReport> {
        Some(inker::ContentReport {
            title: self.doc.document().title.clone(),
            links: self
                .doc
                .document()
                .outgoing_links()
                .into_iter()
                .map(str::to_string)
                .collect(),
            ..Default::default()
        })
    }
    fn as_any_ref(&self) -> &dyn Any {
        self
    }
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(all(test, feature = "smolweb"))]
mod tests {
    use genet_host_api::ResourceFetcher;
    use inker::session_engine::{DocumentSession, SessionEngine, SessionSpawnRequest};

    use super::SmolwebSessionEngine;

    /// Byte source for spawn-with-body tests; never fetches.
    #[derive(Clone)]
    struct NoFetch;
    impl ResourceFetcher for NoFetch {
        fn fetch(&self, _url: &str) -> Option<Vec<u8>> {
            None
        }
    }

    #[test]
    fn smolweb_session_body_route_requests_and_accepts_inline_images() {
        let image = image::RgbaImage::from_pixel(2, 1, image::Rgba([0, 128, 255, 255]));
        let mut image_bytes = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut image_bytes),
                image::ImageFormat::Png,
            )
            .expect("encode PNG fixture");
        let engine = SmolwebSessionEngine::new(
            inker::routing::ENGINE_NEMATIC_GEMTEXT,
            NoFetch,
            crate::SmolwebTheme::Plain,
        )
        .with_inline_media(crate::SmolwebInlineMediaPolicy::images());
        let request = SessionSpawnRequest::new("gemini://x.test/docs/index.gmi")
            .with_body("=> picture.png Picture\n")
            .with_viewport(320, 240);
        let mut session = engine.spawn(&request).expect("smolweb session spawns");

        assert_eq!(session.subresources(), ["gemini://x.test/docs/picture.png"]);
        assert!(session.provide_subresource("gemini://x.test/docs/picture.png", &image_bytes));
        let scene = session.frame(320, 240);
        assert!(
            scene
                .ops
                .iter()
                .any(|operation| matches!(operation, netrender::SceneOp::Image(_)))
        );
        assert!(session.subresources().is_empty());
        assert_eq!(session.links()[0].url, "gemini://x.test/docs/picture.png");
    }
}
