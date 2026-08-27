/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Windowed-shell glue for the smolweb lane: the [`SmolwebDocument`]
//! (a `genet-documents` type since the session-engines split) as pelt's
//! `ViewerContent` / `BrowsableContent`, plus the standalone viewer entry.
//! Pelt impls its own local traits for the foreign type.

use genet_documents::{LocalFetcher, SmolwebDocument, SmolwebTheme};
use inker::{Block, InlineSpan, SessionScrollKey};
use netrender::Scene;

use crate::static_viewer::ViewerScrollKey;

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
    fn drive_product_receipt(
        &mut self,
        receipt: crate::static_viewer::StaticProductReceipt,
    ) -> Result<String, String> {
        if receipt != crate::static_viewer::StaticProductReceipt::Gemtext {
            return Err(format!(
                "smolweb viewer does not implement product receipt {}",
                receipt.id()
            ));
        }
        let document = self.document();
        if document.provenance.source_kind.as_deref() != Some("nematic.gemtext") {
            return Err(format!(
                "gemtext receipt used unexpected source {:?}",
                document.provenance.source_kind
            ));
        }
        if document.content_type != "text/gemini" {
            return Err(format!(
                "gemtext receipt used unexpected content type {:?}",
                document.content_type
            ));
        }
        if document.title.as_deref() != Some("P5 native Gemtext receipt") {
            return Err(format!(
                "gemtext receipt lost its title marker: {:?}",
                document.title
            ));
        }
        let heading = document.blocks.iter().any(|block| {
            matches!(
                block,
                Block::Heading { spans, .. }
                    if spans.iter().any(|span| matches!(
                        span,
                        InlineSpan::Text(text) if text == "P5 native Gemtext receipt"
                    ))
            )
        });
        if !heading {
            return Err("gemtext receipt lost its heading marker".to_owned());
        }
        if !document.outgoing_links().contains(&"gemini://p5.test/next") {
            return Err("gemtext receipt lost its native link marker".to_owned());
        }
        Ok("held Gemtext bytes lowered by Nematic and painted through smolweb".to_owned())
    }

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

/// Open a headed smolweb viewer from source bytes already held by Pelt.
///
/// Product receipts use this path so Nematic lowers the supplied protocol body
/// without asking Fleece or a transport fetcher to acquire it.
pub fn run_smolweb_receipt(
    config: crate::StaticViewerConfig,
    body: &str,
) -> Result<crate::StaticViewerOutcome, String> {
    let doc = SmolwebDocument::parse(&config.url, body, SmolwebTheme::default());
    crate::static_viewer::run_headed_with(config, doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::static_viewer::windowed::ViewerContent;

    #[test]
    fn gemtext_product_receipt_uses_nematic_for_held_body() {
        let mut document = SmolwebDocument::parse(
            "gemini://pelt.test/p5-gemtext/index.gmi",
            include_str!("../examples/p5-gemtext/index.gmi"),
            SmolwebTheme::default(),
        );
        let scene = document.frame(960, 640);
        assert!(!scene.ops.is_empty());
        assert_eq!(
            document.drive_product_receipt(crate::ProductReceipt::Gemtext),
            Ok("held Gemtext bytes lowered by Nematic and painted through smolweb".to_owned())
        );
    }
}
