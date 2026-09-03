// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Titan engine — render side of Titan (`titan://`), the write/upload
//! companion to Gemini (<https://transjovian.org/titan>).
//!
//! Titan is an *upload* protocol: the client sends a body (with a size +
//! MIME token) to a capsule, and the server answers with an ordinary
//! Gemini response — almost always a redirect (`30`/`31`) to the newly
//! written resource. There is therefore no "Titan document" of its own to
//! render; what an engine sees is the server's Gemini response body, which
//! is gemtext-shaped. This engine delegates that body to
//! [`crate::GemtextEngine`] and re-tags provenance as `nematic.titan`.
//!
//! Building and sending the upload belongs to the transport/request layer,
//! not to this portable render engine. Trust defaults to `Unknown`.

use inker::{
    DocumentProvenance, DocumentTrustState, Engine, EngineDocument, EngineError, EngineInput,
};

use crate::GemtextEngine;

/// Stable engine identifier.
pub const ENGINE_ID: &str = "nematic.titan";

/// Titan response-body engine — wraps gemtext with titan provenance and a
/// note that upload/request construction is out of this engine's scope.
pub struct TitanEngine {
    gemtext: GemtextEngine,
}

impl TitanEngine {
    pub fn new() -> Self {
        Self {
            gemtext: GemtextEngine::new(),
        }
    }
}

impl Default for TitanEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine for TitanEngine {
    fn engine_id(&self) -> &str {
        ENGINE_ID
    }

    fn render(&self, input: &EngineInput) -> Result<EngineDocument, EngineError> {
        let mut doc = self.gemtext.render(input)?;

        doc.provenance = DocumentProvenance {
            source_kind: Some(self.engine_id().to_string()),
            canonical_uri: Some(input.address.clone()),
            fetched_at: None,
            source_label: Some("nematic.gemtext".to_string()),
        };
        doc.trust = DocumentTrustState::Unknown;
        Ok(doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(body: &str) -> EngineDocument {
        TitanEngine::new()
            .render(&EngineInput::new("titan://example.test/", body))
            .expect("render")
    }

    #[test]
    fn engine_id_is_stable() {
        assert_eq!(TitanEngine::new().engine_id(), "nematic.titan");
    }

    #[test]
    fn response_body_parses_as_gemtext() {
        let doc = render("# Uploaded\n\n=> titan://example.test/page Saved page\n");
        assert_eq!(doc.title.as_deref(), Some("Uploaded"));
        assert_eq!(doc.outgoing_links(), vec!["titan://example.test/page"]);
    }

    #[test]
    fn provenance_records_titan_with_gemtext_label() {
        let doc = render("body\n");
        assert_eq!(doc.provenance.source_kind.as_deref(), Some("nematic.titan"));
        assert_eq!(
            doc.provenance.source_label.as_deref(),
            Some("nematic.gemtext")
        );
    }
}
