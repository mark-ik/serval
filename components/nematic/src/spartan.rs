// Copyright 2026 Mark AB (markik)
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Spartan engine — body-shape parser for Spartan (`spartan://`), a
//! deliberately-simpler sibling of Gemini (<https://spartan.mozz.us>).
//!
//! A Spartan transaction is a single request line plus an optional upload
//! block, answered by a status line and a content-typed body. Building and
//! sending the request is the host transport's job; this engine receives
//! the response body with the declared content-type already in
//! [`EngineInput::content_type`] when known.
//!
//! Body content-type → delegate:
//!
//! - `text/gemini` (the Spartan default) → [`crate::GemtextEngine`]
//! - `text/markdown` / `text/x-markdown` → [`crate::MarkdownEngine`]
//!
//! Spartan has no signature/envelope layer (that is Scroll's concern), so
//! the engine adds no integrity diagnostic; trust defaults to `Unknown`.

use inker::{
    DocumentProvenance, DocumentTrustState, Engine, EngineDocument, EngineError, EngineInput,
};

use errand::parse::spartan::{SpartanLine, parse as parse_spartan};

use crate::{MarkdownEngine, gemtext::Lowering};

/// Stable engine identifier.
pub const ENGINE_ID: &str = "nematic.spartan";

/// Spartan body engine. Owns inner gemtext / markdown engines for body
/// dispatch.
pub struct SpartanEngine {
    markdown: MarkdownEngine,
}

impl SpartanEngine {
    pub fn new() -> Self {
        Self {
            markdown: MarkdownEngine::new(),
        }
    }
}

impl Default for SpartanEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine for SpartanEngine {
    fn engine_id(&self) -> &str {
        ENGINE_ID
    }

    fn render(&self, input: &EngineInput) -> Result<EngineDocument, EngineError> {
        let (mut doc, inner_kind) = match input.content_type.as_deref() {
            Some(ct) if matches_markdown(ct) => {
                let doc = self.markdown.render(input)?;
                (doc, "nematic.markdown")
            },
            _ => {
                let mut lowering = Lowering::default();
                for line in parse_spartan(&input.body) {
                    match line {
                        SpartanLine::Gemtext(line) => lowering.handle(&line),
                        SpartanLine::Prompt { target, label } => {
                            lowering.push_submit(&target, &label)
                        },
                    }
                }
                let (blocks, title) = lowering.finish();
                (
                    EngineDocument {
                        address: input.address.clone(),
                        title,
                        content_type: input
                            .content_type
                            .clone()
                            .unwrap_or_else(|| "text/gemini".to_string()),
                        lang: None,
                        provenance: DocumentProvenance::for_engine(
                            "nematic.gemtext",
                            &input.address,
                        ),
                        trust: DocumentTrustState::Unknown,
                        diagnostics: Vec::new(),
                        blocks,
                    },
                    "nematic.gemtext",
                )
            },
        };

        // Re-tag provenance so consumers see "nematic.spartan" as the
        // source kind while the inner engine ID stays visible as the label.
        doc.provenance = DocumentProvenance {
            source_kind: Some(self.engine_id().to_string()),
            canonical_uri: Some(input.address.clone()),
            fetched_at: None,
            source_label: Some(inner_kind.to_string()),
        };
        doc.trust = DocumentTrustState::Unknown;

        Ok(doc)
    }
}

fn matches_markdown(content_type: &str) -> bool {
    let primary = content_type
        .split(';')
        .next()
        .unwrap_or(content_type)
        .trim()
        .to_ascii_lowercase();
    primary == "text/markdown" || primary == "text/x-markdown"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_id_is_stable() {
        assert_eq!(SpartanEngine::new().engine_id(), "nematic.spartan");
    }

    #[test]
    fn default_body_treated_as_gemtext() {
        let doc = SpartanEngine::new()
            .render(&EngineInput::new(
                "spartan://capsule.test/",
                "# Hello\n\n=> spartan://capsule.test/next Next\n",
            ))
            .expect("render");
        assert_eq!(doc.title.as_deref(), Some("Hello"));
        assert_eq!(doc.outgoing_links(), vec!["spartan://capsule.test/next"]);
        assert_eq!(
            doc.provenance.source_kind.as_deref(),
            Some("nematic.spartan")
        );
        assert_eq!(
            doc.provenance.source_label.as_deref(),
            Some("nematic.gemtext")
        );
    }

    #[test]
    fn markdown_content_type_routes_to_markdown_engine() {
        let doc = SpartanEngine::new()
            .render(
                &EngineInput::new("spartan://capsule.test/", "# Hello\n\n*emphasis*\n")
                    .with_content_type("text/markdown"),
            )
            .expect("render");
        assert_eq!(doc.content_type, "text/markdown");
        assert_eq!(
            doc.provenance.source_label.as_deref(),
            Some("nematic.markdown")
        );
    }

    #[test]
    fn prompt_line_stays_a_typed_submission() {
        let doc = SpartanEngine::new()
            .render(&EngineInput::new(
                "spartan://capsule.test/",
                "=: /guestbook Sign the guestbook\n",
            ))
            .expect("render");
        let inker::Block::Paragraph { spans } = &doc.blocks[0] else {
            panic!("expected prompt paragraph");
        };
        assert!(matches!(
            &spans[0],
            inker::InlineSpan::Submit { target, spans }
                if target == "/guestbook"
                    && inker::inline_text(spans) == "Sign the guestbook"
        ));
        assert!(doc.outgoing_links().is_empty());
    }

    #[test]
    fn dispatches_through_inker_registry() {
        use inker::EngineRegistry;
        use inker::routing::{
            EngineRouteDecision, SurfaceContract, SurfaceContractMode, SurfaceTargetId,
        };

        let mut registry = EngineRegistry::new();
        registry.register(Box::new(SpartanEngine::new()));
        let decision = EngineRouteDecision {
            engine_id: ENGINE_ID.to_string(),
            surface_contract: SurfaceContract {
                target: SurfaceTargetId::new("spartan:1"),
                mode: SurfaceContractMode::CompositedTexture,
            },
        };
        let doc = registry
            .dispatch(&decision, &EngineInput::new("spartan://t/", "# T\n"))
            .expect("dispatch");
        assert_eq!(doc.title.as_deref(), Some("T"));
    }
}
