// Copyright 2026 Mark AB (markik)
// SPDX-License-Identifier: MIT OR Apache-2.0

//! PaintList producer.
//!
//! Walks a [`crate::DocumentRenderPacket`] and emits an [`InkerPaintList`]
//! — an implementation of [`paint_list_api::PaintList`], the engine-facing
//! display-list vocabulary shared across the Mere/Genet renderer
//! ecosystem. The list lowers to a `netrender::Scene` via
//! `paint_list_render` (see [`crate::netrender_backend`]); it can equally
//! cross IPC or sit in a capture/replay fixture, since the vocabulary is
//! fully serializable and self-contained.
//!
//! This is the portable half of the rendering path: it depends only on
//! `paint_list_api` (euclid + serde, no wgpu) plus parley's font handles,
//! so it builds everywhere the rest of document-canvas does. The Scene
//! lowering lives behind the `netrender` feature.
//!
//! ## Font handling
//!
//! Each glyph run carries a [`FontFaceId`](crate::FontFaceId) — parley's
//! *actually-shaped* face. The producer resolves it through the
//! [`FontTable`](crate::FontTable) sidecar (built during layout, carried
//! beside the packet), extracts the face bytes once per unique face into
//! the list's [`PaintList::fonts`] side-table, and references them from
//! each `DrawText` by `FontInstanceKey`.
//!
//! There is no `(family, weight, style)` re-resolution: the glyph ids and
//! the shipped face come from the *same* parley face, so a fallback can't
//! desync them (the bug this producer used to have). A run whose face is
//! somehow absent from the sidecar falls back to a placeholder rect —
//! defensive only; a table from the same layout pass always contains it.
//!
//! Interaction regions (`InteractionKind::Link`) are not represented in
//! the paint list — it carries pixels, not hit-test trees. The host
//! consumes the packet's `interactions` separately.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use paint_list_api::{
    AlphaType, ColorF, CommonPlacement, DeviceIntSize, EngineId, FontInstanceKey, FontResource,
    GlyphInstance, IdNamespace, ImageItem, ImageKey, ImageRendering, ImageResource, LayoutPoint,
    LayoutRect, LineItem, LineOrientation, LineStyle, PaintCmd, PaintList, RectItem, TextOptions,
    TextRunItem,
};

use crate::font_table::FontTable;
use crate::style::ColorVocabulary;
use crate::types::{
    DecodedImage, DocumentRenderPacket, FontFaceId, GlyphRun, Rect, RenderedBlock,
    RenderedBlockKind,
};

/// Half the hairline thickness used for [`RenderedBlockKind::Rule`]
/// (the rule fills a 1px strip centered on its mid-line).
const RULE_HALF_THICKNESS: f32 = 0.5;

/// A document-view paint list: the unit of paint output for one rendered
/// frame of an [`inker`](https://crates.io/crates/inker) document.
/// Implements [`PaintList`] so it lowers through the shared
/// `paint_list_render` translator like any other engine's output.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InkerPaintList {
    viewport: DeviceIntSize,
    commands: Vec<PaintCmd>,
    fonts: Vec<FontResource>,
    images: Vec<ImageResource>,
    generation: u64,
}

impl PaintList for InkerPaintList {
    fn engine_id(&self) -> EngineId {
        EngineId::INKER
    }
    fn viewport(&self) -> DeviceIntSize {
        self.viewport
    }
    fn generation_id(&self) -> u64 {
        self.generation
    }
    fn commands(&self) -> &[PaintCmd] {
        &self.commands
    }
    fn fonts(&self) -> &[FontResource] {
        &self.fonts
    }
    fn images(&self) -> &[ImageResource] {
        &self.images
    }
}

/// Build an [`InkerPaintList`] from a [`DocumentRenderPacket`] and its
/// [`FontTable`] sidecar (both produced by
/// [`layout_document`](crate::layout_document)).
///
/// `fonts` supplies the real face bytes parley shaped each run against;
/// `colors` supplies theme primitives. Every text run renders as real
/// glyphs — no host font wiring required, since the faces come from
/// parley's own shaping.
pub fn paint_list_from_packet(
    packet: &DocumentRenderPacket,
    fonts: &FontTable,
    colors: &ColorVocabulary,
) -> InkerPaintList {
    paint_list_from_packet_with_images(packet, fonts, colors, &HashMap::new())
}

/// Build a paint list while resolving image blocks against decoded host
/// resources keyed by the block URL.
pub fn paint_list_from_packet_with_images(
    packet: &DocumentRenderPacket,
    fonts: &FontTable,
    colors: &ColorVocabulary,
    images: &HashMap<String, DecodedImage>,
) -> InkerPaintList {
    let viewport = DeviceIntSize::new(
        packet.viewport.width.max(0.0).round() as i32,
        packet.viewport.height.max(0.0).round() as i32,
    );
    let mut builder = Builder::new(fonts, colors, images);
    for block in &packet.blocks {
        builder.emit_block(block);
    }
    InkerPaintList {
        viewport,
        commands: builder.commands,
        fonts: builder.fonts,
        images: builder.images,
        generation: 0,
    }
}

/// Walk state: accumulates `PaintCmd`s + the font side-table, interning
/// each referenced [`FontFaceId`] to a single `FontInstanceKey` (and
/// shipping its bytes from the sidecar once).
struct Builder<'a> {
    face_table: &'a FontTable,
    colors: &'a ColorVocabulary,
    decoded_images: &'a HashMap<String, DecodedImage>,
    commands: Vec<PaintCmd>,
    fonts: Vec<FontResource>,
    images: Vec<ImageResource>,
    face_keys: HashMap<FontFaceId, FontInstanceKey>,
    image_keys: HashMap<String, ImageKey>,
    next_font_key: u32,
    next_image_key: u32,
}

impl<'a> Builder<'a> {
    fn new(
        face_table: &'a FontTable,
        colors: &'a ColorVocabulary,
        decoded_images: &'a HashMap<String, DecodedImage>,
    ) -> Self {
        Self {
            face_table,
            colors,
            decoded_images,
            commands: Vec::new(),
            fonts: Vec::new(),
            images: Vec::new(),
            face_keys: HashMap::new(),
            image_keys: HashMap::new(),
            next_font_key: 0,
            next_image_key: 0,
        }
    }

    fn emit_block(&mut self, block: &RenderedBlock) {
        match &block.kind {
            RenderedBlockKind::Text { glyph_runs } => {
                for run in glyph_runs {
                    self.emit_glyph_run(run);
                }
            },
            RenderedBlockKind::Image { url, .. } => {
                if let Some((key, width, height)) = self.intern_image(url) {
                    self.commands.push(PaintCmd::DrawImage(ImageItem {
                        placement: CommonPlacement::new(layout_rect(contained_image_bounds(
                            block.bounds,
                            width,
                            height,
                        ))),
                        image_key: key,
                        image_rendering: ImageRendering::Auto,
                        alpha_type: AlphaType::Alpha,
                        color: ColorF::WHITE,
                    }));
                } else {
                    self.push_rect(block.bounds, self.colors.placeholder_image);
                }
            },
            RenderedBlockKind::Rule => {
                // Hairline: a 1px-tall strip centered on the rect's
                // vertical midpoint. Lowered as a (filled) line primitive.
                let mid_y = block.bounds.origin.y + block.bounds.size.height * 0.5;
                let strip = Rect::from_xywh(
                    block.bounds.origin.x,
                    mid_y - RULE_HALF_THICKNESS,
                    block.bounds.size.width,
                    RULE_HALF_THICKNESS * 2.0,
                );
                self.commands.push(PaintCmd::DrawLine(LineItem {
                    placement: CommonPlacement::new(layout_rect(strip)),
                    color: colorf(self.colors.rule),
                    style: LineStyle::Solid,
                    orientation: LineOrientation::Horizontal,
                    wavy_thickness: 0.0,
                }));
            },
            RenderedBlockKind::Group { children } => {
                for child in children {
                    self.emit_block(child);
                }
            },
        }
    }

    fn emit_glyph_run(&mut self, run: &GlyphRun) {
        let Some(font_instance) = self.intern_font(run.font_face) else {
            // The run's face isn't in the sidecar — shouldn't happen for a
            // table produced by the same layout pass. Placeholder rect so
            // the text-shaped region is still visible.
            self.push_rect(glyph_run_bounds(run), self.colors.placeholder_text);
            return;
        };

        // Translate each PositionedGlyph (relative to run.origin) into a
        // PaintList GlyphInstance (absolute in packet space, baseline-
        // anchored).
        let baseline_y = run.origin.y + run.baseline_y;
        let glyphs: Vec<GlyphInstance> = run
            .glyphs
            .iter()
            .map(|g| GlyphInstance {
                index: g.glyph_id,
                point: LayoutPoint::new(run.origin.x + g.x, baseline_y + g.y),
            })
            .collect();
        if glyphs.is_empty() {
            return;
        }

        self.commands.push(PaintCmd::DrawText(TextRunItem {
            placement: CommonPlacement::new(layout_rect(glyph_run_bounds(run))),
            font_instance,
            font_size: run.font_size,
            color: colorf(run.color),
            glyphs,
            options: TextOptions::default(),
        }));
    }

    /// Map a run's [`FontFaceId`] to the `FontInstanceKey` the side-table
    /// holds for it, minting the key + shipping the face bytes (from the
    /// sidecar) on first sight. `None` only if the sidecar lacks the face.
    fn intern_font(&mut self, face: FontFaceId) -> Option<FontInstanceKey> {
        if let Some(&key) = self.face_keys.get(&face) {
            return Some(key);
        }
        let font = self.face_table.get(face)?;
        let key = FontInstanceKey::new(IdNamespace(0), self.next_font_key);
        self.next_font_key += 1;
        self.fonts.push(FontResource {
            key,
            data: font.data.data().to_vec().into(),
            index: font.index,
        });
        self.face_keys.insert(face, key);
        Some(key)
    }

    fn intern_image(&mut self, url: &str) -> Option<(ImageKey, u32, u32)> {
        let image = self.decoded_images.get(url)?;
        if let Some(&key) = self.image_keys.get(url) {
            return Some((key, image.width, image.height));
        }
        let key = ImageKey::new(IdNamespace(0), self.next_image_key);
        self.next_image_key += 1;
        self.images.push(ImageResource {
            key,
            width: image.width,
            height: image.height,
            data: image.rgba8.clone(),
        });
        self.image_keys.insert(url.to_string(), key);
        Some((key, image.width, image.height))
    }

    fn push_rect(&mut self, bounds: Rect, color: [f32; 4]) {
        if bounds.size.width <= 0.0 || bounds.size.height <= 0.0 {
            return;
        }
        self.commands.push(PaintCmd::DrawRect(RectItem {
            placement: CommonPlacement::new(layout_rect(bounds)),
            color: colorf(color),
        }));
    }
}

fn contained_image_bounds(bounds: Rect, width: u32, height: u32) -> Rect {
    if width == 0 || height == 0 || bounds.size.width <= 0.0 || bounds.size.height <= 0.0 {
        return bounds;
    }
    let scale = (bounds.size.width / width as f32).min(bounds.size.height / height as f32);
    let fitted_width = width as f32 * scale;
    let fitted_height = height as f32 * scale;
    Rect::from_xywh(
        bounds.origin.x + (bounds.size.width - fitted_width) * 0.5,
        bounds.origin.y + (bounds.size.height - fitted_height) * 0.5,
        fitted_width,
        fitted_height,
    )
}

fn glyph_run_bounds(run: &GlyphRun) -> Rect {
    let advance: f32 = run.glyphs.iter().map(|g| g.advance).sum();
    Rect::from_xywh(
        run.origin.x,
        run.origin.y,
        advance.max(1.0),
        // Approximate run height as font size * a generous line-gap factor.
        run.font_size * 1.4,
    )
}

fn layout_rect(r: Rect) -> LayoutRect {
    LayoutRect::new(
        LayoutPoint::new(r.origin.x, r.origin.y),
        LayoutPoint::new(r.max_x(), r.max_y()),
    )
}

fn colorf(c: [f32; 4]) -> ColorF {
    ColorF::new(c[0], c[1], c[2], c[3])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font_table::FontTable;
    use crate::layout::layout_document;
    use crate::style_sheet::DocumentStyleSheet;
    use crate::types::Viewport;
    use inker::{Block, DocumentProvenance, DocumentTrustState, EngineDocument, InlineSpan};

    fn doc(blocks: Vec<Block>) -> EngineDocument {
        EngineDocument {
            address: "doc:paint-list-test".into(),
            title: None,
            content_type: "text/plain".into(),
            lang: None,
            provenance: DocumentProvenance::default(),
            trust: DocumentTrustState::Unknown,
            diagnostics: Vec::new(),
            blocks,
        }
    }

    /// Lay out + build the paint list, returning both the list and the
    /// font sidecar (so tests can assert shipped faces came from parley).
    fn list_for(blocks: Vec<Block>) -> (InkerPaintList, FontTable) {
        let laid = layout_document(
            &doc(blocks),
            Viewport::new(640.0, 480.0),
            &DocumentStyleSheet::default(),
        );
        let list = paint_list_from_packet(&laid.packet, &laid.fonts, &ColorVocabulary::default());
        (list, laid.fonts)
    }

    fn count<F: Fn(&PaintCmd) -> bool>(list: &InkerPaintList, pred: F) -> usize {
        list.commands().iter().filter(|c| pred(c)).count()
    }

    #[test]
    fn empty_packet_produces_no_commands() {
        let (list, _) = list_for(vec![]);
        assert_eq!(list.engine_id(), EngineId::INKER);
        assert!(list.commands().is_empty());
        assert!(list.fonts().is_empty());
        assert!(list.images().is_empty());
    }

    #[test]
    fn decoded_image_emits_draw_image_and_resource_sidecar() {
        let laid = layout_document(
            &doc(vec![Block::Image {
                url: "https://x.test/pic.png".into(),
                alt: "picture".into(),
            }]),
            Viewport::new(640.0, 480.0),
            &DocumentStyleSheet::default(),
        );
        let decoded = DecodedImage {
            width: 2,
            height: 1,
            rgba8: vec![255, 0, 0, 255, 0, 0, 255, 255],
        };
        let images = HashMap::from([("https://x.test/pic.png".to_string(), decoded.clone())]);
        let list = paint_list_from_packet_with_images(
            &laid.packet,
            &laid.fonts,
            &ColorVocabulary::default(),
            &images,
        );

        let draw = list
            .commands()
            .iter()
            .find_map(|command| match command {
                PaintCmd::DrawImage(image) => Some(image),
                _ => None,
            })
            .expect("decoded resource paints as an image");
        let resource = list
            .images()
            .iter()
            .find(|resource| resource.key == draw.image_key)
            .expect("draw key resolves through the image sidecar");
        assert_eq!((resource.width, resource.height), (2, 1));
        assert_eq!(resource.data, decoded.rgba8);
        assert!(
            !list
                .commands()
                .iter()
                .any(|command| matches!(command, PaintCmd::DrawRect(_))),
            "a resolved image does not fall back to its placeholder"
        );
    }

    #[test]
    fn text_emits_drawtext_and_populates_fonts() {
        // With parley's real face threaded through, a paragraph renders as
        // real text (DrawText) with zero host font wiring — no placeholder
        // rect, no resolver.
        let (list, _) = list_for(vec![Block::Paragraph {
            spans: vec![InlineSpan::Text("hello".into())],
        }]);
        assert!(count(&list, |c| matches!(c, PaintCmd::DrawText(_))) >= 1);
        assert!(!list.fonts().is_empty(), "real text ships a font face");
    }

    #[test]
    fn shipped_faces_come_from_the_sidecar() {
        // Regression guard for the wrong-face-on-fallback bug: every face
        // shipped in the side-table is byte-identical to a face in the
        // sidecar (parley's actually-shaped faces), never one re-resolved
        // from a (family, weight, style) label. Body + monospace exercises
        // more than one face.
        let (list, fonts) = list_for(vec![
            Block::Paragraph {
                spans: vec![InlineSpan::Text("body text".into())],
            },
            Block::CodeBlock {
                language: None,
                text: "fn main() {}".into(),
            },
        ]);
        let sidecar: Vec<(Vec<u8>, u32)> = fonts
            .iter()
            .map(|(_, f)| (f.data.data().to_vec(), f.index))
            .collect();
        assert!(!list.fonts().is_empty());
        for fr in list.fonts() {
            assert!(
                sidecar
                    .iter()
                    .any(|(d, i)| *d == *fr.data && *i == fr.index),
                "shipped face must originate from parley's sidecar, not a label re-resolve"
            );
        }
    }

    #[test]
    fn each_run_ships_the_face_it_was_shaped_against() {
        // The crux of the fix, stated per-run and deterministically: the
        // bytes shipped for a run's DrawText are exactly the bytes of the
        // sidecar face its FontFaceId points at — i.e. parley's
        // actually-shaped face. Glyph ids and shipped face now come from
        // the same place, so they can't desync (the old bug shipped a
        // label-re-resolved face that could differ on fallback).
        let laid = layout_document(
            &doc(vec![
                Block::Paragraph {
                    spans: vec![InlineSpan::Text("alpha".into())],
                },
                Block::CodeBlock {
                    language: None,
                    text: "beta()".into(),
                },
            ]),
            Viewport::new(640.0, 480.0),
            &DocumentStyleSheet::default(),
        );
        let list = paint_list_from_packet(&laid.packet, &laid.fonts, &ColorVocabulary::default());

        // Flat doc → glyph runs and DrawText commands correspond 1:1 in
        // emission order.
        let runs: Vec<GlyphRun> = laid
            .packet
            .blocks
            .iter()
            .flat_map(|b| match &b.kind {
                RenderedBlockKind::Text { glyph_runs } => glyph_runs.clone(),
                _ => Vec::new(),
            })
            .collect();
        let draws: Vec<_> = list
            .commands()
            .iter()
            .filter_map(|c| match c {
                PaintCmd::DrawText(t) => Some(t.clone()),
                _ => None,
            })
            .collect();
        assert!(!runs.is_empty());
        assert_eq!(runs.len(), draws.len(), "one DrawText per non-empty run");

        for (run, draw) in runs.iter().zip(draws.iter()) {
            let face = laid.fonts.get(run.font_face).expect("run face in sidecar");
            let shipped = list
                .fonts()
                .iter()
                .find(|fr| fr.key == draw.font_instance)
                .expect("DrawText key present in fonts() table");
            assert_eq!(
                *shipped.data,
                face.data.data().to_vec(),
                "shipped bytes must equal the shaped face's bytes"
            );
            assert_eq!(shipped.index, face.index, "collection index must match");
        }
    }

    #[test]
    fn rule_emits_line_command_in_rule_color() {
        let mut colors = ColorVocabulary::default();
        colors.rule = [1.0, 0.0, 0.0, 1.0];
        let laid = layout_document(
            &doc(vec![Block::Rule]),
            Viewport::new(640.0, 480.0),
            &DocumentStyleSheet::default(),
        );
        let list = paint_list_from_packet(&laid.packet, &laid.fonts, &colors);
        let line = list
            .commands()
            .iter()
            .find_map(|c| match c {
                PaintCmd::DrawLine(l) => Some(l),
                _ => None,
            })
            .expect("expected a DrawLine for the rule");
        assert_eq!(line.color, colorf([1.0, 0.0, 0.0, 1.0]));
    }

    #[test]
    fn draw_text_carries_per_run_role_color() {
        // Text color now rides on each glyph run (baked from the sheet at
        // layout), not a single ColorVocabulary lookup: a heading lowers in
        // heading_text, a paragraph in body_text.
        let palette = DocumentStyleSheet::default().colors;
        let laid = layout_document(
            &doc(vec![
                Block::Heading {
                    level: 1,
                    spans: vec![InlineSpan::Text("Title".into())],
                },
                Block::Paragraph {
                    spans: vec![InlineSpan::Text("body".into())],
                },
            ]),
            Viewport::new(640.0, 480.0),
            &DocumentStyleSheet::default(),
        );
        let list = paint_list_from_packet(&laid.packet, &laid.fonts, &ColorVocabulary::default());
        let text_colors: Vec<_> = list
            .commands()
            .iter()
            .filter_map(|c| match c {
                PaintCmd::DrawText(t) => Some(t.color),
                _ => None,
            })
            .collect();
        assert_ne!(palette.heading_text, palette.body_text, "roles must differ");
        assert!(
            text_colors.contains(&colorf(palette.heading_text)),
            "heading DrawText paints in heading_text"
        );
        assert!(
            text_colors.contains(&colorf(palette.body_text)),
            "body DrawText paints in body_text"
        );
    }

    #[test]
    fn group_recurses_into_children() {
        // Each list item is a paragraph → real text now (not a placeholder
        // rect), so at least two DrawText commands.
        let (list, _) = list_for(vec![Block::List {
            ordered: false,
            items: vec![
                vec![Block::Paragraph {
                    spans: vec![InlineSpan::Text("first".into())],
                }],
                vec![Block::Paragraph {
                    spans: vec![InlineSpan::Text("second".into())],
                }],
            ],
        }]);
        assert!(count(&list, |c| matches!(c, PaintCmd::DrawText(_))) >= 2);
    }

    #[test]
    fn repeated_runs_share_one_font_side_table_entry() {
        // Two body paragraphs share one parley face → interned once; both
        // DrawText runs reference the same FontInstanceKey.
        let (list, _) = list_for(vec![
            Block::Paragraph {
                spans: vec![InlineSpan::Text("first".into())],
            },
            Block::Paragraph {
                spans: vec![InlineSpan::Text("second".into())],
            },
        ]);
        assert_eq!(list.fonts().len(), 1, "shared face should intern once");
        let keys: Vec<_> = list
            .commands()
            .iter()
            .filter_map(|c| match c {
                PaintCmd::DrawText(t) => Some(t.font_instance),
                _ => None,
            })
            .collect();
        assert!(keys.len() >= 2);
        assert!(keys.iter().all(|k| *k == keys[0]), "all runs share one key");
    }

    #[test]
    fn viewport_rounds_to_device_int_size() {
        let laid = layout_document(
            &doc(vec![]),
            Viewport::new(640.4, 480.6),
            &DocumentStyleSheet::default(),
        );
        let list = paint_list_from_packet(&laid.packet, &laid.fonts, &ColorVocabulary::default());
        assert_eq!(list.viewport().width, 640);
        assert_eq!(list.viewport().height, 481);
    }
}
