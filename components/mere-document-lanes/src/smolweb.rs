/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Retained smolweb documents through Genet's engine-native document path.
//!
//! Nematic parses protocol content into an [`inker::EngineDocument`].
//! `document-canvas` owns layout, visible-band derivation, link regions, and
//! PaintList lowering. This module retains that packet plus viewport scroll
//! and exposes the existing session API to Pelt and Mere.

use std::collections::HashMap;
use std::io::Cursor;

use document_canvas::{
    ColorVocabulary, DecodedImage, DocumentStyleSheet, InteractionKind, LaidOutDocument, Rect,
    SemanticInteractionId, Viewport, layout_document,
    netrender_backend::scene_from_packet_with_images,
};
#[cfg(feature = "smolweb")]
use genet_host_api::ResourceFetcher;
use image::GenericImageView;
use inker::{Block, EngineDocument, SessionScrollKey};
#[cfg(feature = "smolweb")]
use inker::{Engine, EngineInput, InlineSpan, inline_text};
use netrender::Scene;

/// How an engine-native smolweb document is colored.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SmolwebTheme {
    /// A stable palette derived from the capsule host.
    #[default]
    Site,
    /// A neutral light palette.
    Plain,
    /// A warm fixed light palette.
    Light,
    /// A fixed dark palette.
    Dark,
    /// Colors supplied by the application host.
    App(SmolwebPalette),
    /// Host-resolved system theme. Light is the fallback.
    System,
}

/// Compatibility palette used by current Pelt and Mere hosts.
///
/// New engine-native callers may configure [`DocumentStyleSheet`] directly
/// through [`SmolwebDocument::from_document`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmolwebPalette {
    pub bg: String,
    pub fg: String,
    pub link: String,
    pub quote: String,
    pub pre_bg: String,
}

/// Host policy for promoting image-shaped gemtext links into inline images.
///
/// The generic Genet lane defaults this off: gemtext specifies links, while
/// embedding them is a browser presentation choice. Product hosts opt in at
/// engine registration and retain explicit fetch and decode budgets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SmolwebInlineMediaPolicy {
    pub enabled: bool,
    pub max_images: usize,
    pub max_encoded_bytes_per_image: usize,
    pub max_decoded_bytes_per_image: usize,
    pub max_dimension: u32,
}

impl SmolwebInlineMediaPolicy {
    pub fn images() -> Self {
        Self {
            enabled: true,
            max_images: 8,
            max_encoded_bytes_per_image: 8 * 1024 * 1024,
            max_decoded_bytes_per_image: 64 * 1024 * 1024,
            max_dimension: 8192,
        }
    }
}

impl Default for SmolwebInlineMediaPolicy {
    fn default() -> Self {
        let mut policy = Self::images();
        policy.enabled = false;
        policy
    }
}

/// A retained engine document, its document-canvas layout, and host viewport.
pub struct SmolwebDocument {
    document: EngineDocument,
    style: DocumentStyleSheet,
    background: [f32; 4],
    images: HashMap<String, DecodedImage>,
    inline_media: SmolwebInlineMediaPolicy,
    layout: Option<LaidOutDocument>,
    size: (u32, u32),
    scroll_y: f32,
    /// A semantic snapshot is only meaningful after the layout has crossed a
    /// completed presentation boundary. Layout may exist earlier for sizing or
    /// hit-testing, but that is not an a11y publication event.
    presented: bool,
}

/// One visible logical link recovered from the retained document-canvas
/// packet. This stays crate-local: Reader owns the public snapshot shape.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RetainedAccessibleLink {
    pub identity: SemanticInteractionId,
    pub label: String,
    pub url: String,
    pub rects: Vec<[f32; 4]>,
}

impl SmolwebDocument {
    /// Fetch `url` through the host fetcher, then lower and retain the body.
    #[cfg(feature = "smolweb")]
    pub fn load(
        fetcher: &impl ResourceFetcher,
        url: &str,
        theme: SmolwebTheme,
    ) -> Result<Self, String> {
        let bytes = fetcher
            .fetch(url)
            .ok_or_else(|| format!("could not load {url}"))?;
        Ok(Self::parse(url, &String::from_utf8_lossy(&bytes), theme))
    }

    /// Fetch a document and apply the host's inline-media presentation policy.
    #[cfg(feature = "smolweb")]
    pub fn load_with_inline_media(
        fetcher: &impl ResourceFetcher,
        url: &str,
        theme: SmolwebTheme,
        policy: SmolwebInlineMediaPolicy,
    ) -> Result<Self, String> {
        let bytes = fetcher
            .fetch(url)
            .ok_or_else(|| format!("could not load {url}"))?;
        Ok(Self::parse_with_inline_media(
            url,
            &String::from_utf8_lossy(&bytes),
            theme,
            policy,
        ))
    }

    /// Lower already-fetched content through the matching Nematic engine.
    #[cfg(feature = "smolweb")]
    pub fn parse(url: &str, body: &str, theme: SmolwebTheme) -> Self {
        let document = lower(url, body);
        let (style, background) = style_for_theme(&theme, url, &document.content_type);
        Self::from_document(document, style, background)
    }

    /// Lower an already-fetched body and expose eligible linked images for the
    /// host to resolve through [`SmolwebDocument::subresources`].
    #[cfg(feature = "smolweb")]
    pub fn parse_with_inline_media(
        url: &str,
        body: &str,
        theme: SmolwebTheme,
        policy: SmolwebInlineMediaPolicy,
    ) -> Self {
        let mut document = lower(url, body);
        promote_inline_image_links(&mut document, policy);
        let (style, background) = style_for_theme(&theme, url, &document.content_type);
        Self::from_document_with_media_policy(document, style, background, policy)
    }

    /// Retain an already-lowered document with an explicit host style.
    pub fn from_document(
        document: EngineDocument,
        style: DocumentStyleSheet,
        background: [f32; 4],
    ) -> Self {
        Self::from_document_with_media_policy(
            document,
            style,
            background,
            SmolwebInlineMediaPolicy::default(),
        )
    }

    /// Retain a portable document using the same host-theme mapping as the
    /// existing document lane.
    pub fn from_document_with_theme(document: EngineDocument, theme: SmolwebTheme) -> Self {
        let (style, background) =
            style_for_theme(&theme, &document.address, &document.content_type);
        Self::from_document(document, style, background)
    }

    fn from_document_with_media_policy(
        document: EngineDocument,
        style: DocumentStyleSheet,
        background: [f32; 4],
        inline_media: SmolwebInlineMediaPolicy,
    ) -> Self {
        Self {
            document,
            style,
            background,
            images: HashMap::new(),
            inline_media,
            layout: None,
            size: (0, 0),
            scroll_y: 0.0,
            presented: false,
        }
    }

    /// The portable document retained by this session.
    pub fn document(&self) -> &EngineDocument {
        &self.document
    }

    /// Replace an in-flight document body without replacing its live session.
    ///
    /// Host-selected styling, viewport, and scroll position remain stable.
    /// Lowered structure and inline-media requests are rebuilt from the exact
    /// prefix now available; decoded images survive only while still named by
    /// the replacement document.
    #[cfg(feature = "smolweb")]
    pub fn replace_body(&mut self, url: &str, body: &str) {
        let mut document = lower(url, body);
        promote_inline_image_links(&mut document, self.inline_media);
        self.images.retain(|url, _| {
            document.blocks.iter().any(
                |block| matches!(block, Block::Image { url: image_url, .. } if image_url == url),
            )
        });
        self.document = document;
        self.layout = None;
        self.presented = false;
    }

    /// Unresolved inline-image URLs for the host fetch actor.
    pub fn subresources(&self) -> Vec<String> {
        self.document
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Image { url, .. } if !self.images.contains_key(url) => Some(url.clone()),
                _ => None,
            })
            .collect()
    }

    /// Decode and retain one host-fetched inline image within this document's
    /// configured resource limits.
    pub fn provide_subresource(&mut self, url: &str, bytes: &[u8]) -> bool {
        if self.images.contains_key(url)
            || !self.subresources().iter().any(|pending| pending == url)
            || bytes.len() > self.inline_media.max_encoded_bytes_per_image
        {
            return false;
        }
        let Some(image) = decode_image(bytes, self.inline_media) else {
            return false;
        };
        self.images.insert(url.to_string(), image);
        true
    }

    fn ensure_layout(&mut self, width: u32, height: u32) {
        let size = (width.max(1), height.max(1));
        if self.layout.is_some() && self.size == size {
            return;
        }
        // A new viewport produces new geometry. Do not let a sizing, click,
        // or content-height query publish that geometry under the preceding
        // frame; `frame` marks it present only after painting completes.
        self.presented = false;
        self.layout = Some(layout_document(
            &self.document,
            Viewport::new(size.0 as f32, size.1 as f32),
            &self.style,
        ));
        self.size = size;
        self.scroll_y = self.scroll_y.min(self.max_scroll());
    }

    fn max_scroll(&self) -> f32 {
        let Some(layout) = &self.layout else {
            return 0.0;
        };
        (layout.packet.content_bounds.size.height - self.size.1 as f32).max(0.0)
    }

    /// Paint the visible document band at the retained scroll offset.
    pub fn frame(&mut self, width: u32, height: u32) -> Scene {
        self.ensure_layout(width, height);
        let layout = self.layout.as_ref().expect("layout built above");
        let packet = layout.packet.window(self.scroll_y, self.size.1 as f32);
        let mut scene =
            scene_from_packet_with_images(&packet, &layout.fonts, &self.style.colors, &self.images);
        scene.push_rect(
            0.0,
            0.0,
            self.size.0 as f32,
            self.size.1 as f32,
            self.background,
        );
        let background = scene.ops.pop().expect("push_rect appended an op");
        scene.ops.insert(0, background);
        self.presented = true;
        scene
    }

    /// Move the single host-owned document viewport.
    pub fn scroll_by(&mut self, _dx: f32, dy: f32) -> bool {
        if self.layout.is_none() {
            return false;
        }
        let before = self.scroll_y;
        self.scroll_y = (self.scroll_y + dy).clamp(0.0, self.max_scroll());
        self.scroll_y != before
    }

    /// document-canvas has one viewport scroller, so point routing delegates
    /// to [`scroll_by`](Self::scroll_by).
    pub fn scroll_at(&mut self, _x: f32, _y: f32, dx: f32, dy: f32) -> bool {
        self.scroll_by(dx, dy)
    }

    /// Apply the established Genet keyboard-scroll vocabulary.
    pub fn scroll_for_key(&mut self, key: SessionScrollKey) -> bool {
        if self.layout.is_none() {
            return false;
        }
        let before = self.scroll_y;
        let page = self.size.1 as f32 * 0.9;
        self.scroll_y = match key {
            SessionScrollKey::LineUp => self.scroll_y - 40.0,
            SessionScrollKey::LineDown => self.scroll_y + 40.0,
            SessionScrollKey::PageUp => self.scroll_y - page,
            SessionScrollKey::PageDown => self.scroll_y + page,
            SessionScrollKey::Home => 0.0,
            SessionScrollKey::End => self.max_scroll(),
        }
        .clamp(0.0, self.max_scroll());
        self.scroll_y != before
    }

    /// Jump to an absolute full-document offset.
    pub fn scroll_to(&mut self, y: f32) {
        if self.layout.is_some() {
            self.scroll_y = y.clamp(0.0, self.max_scroll());
        }
    }

    /// Full laid-out content height, floored to the viewport height.
    pub fn content_height(&mut self, width: u32, height: u32) -> u32 {
        self.ensure_layout(width, height);
        self.layout
            .as_ref()
            .expect("layout built above")
            .packet
            .content_bounds
            .size
            .height
            .ceil()
            .max(height.max(1) as f32) as u32
    }

    /// Viewport-space link rectangles as `[x, y, width, height]`, matching the
    /// shared retained-session contract.
    pub fn links(&self) -> Vec<(String, [f32; 4])> {
        let Some(layout) = &self.layout else {
            return Vec::new();
        };
        layout
            .packet
            .interactions
            .iter()
            .filter_map(|region| {
                let InteractionKind::Link { url } = &region.kind else {
                    return None;
                };
                let rect = region.bounds;
                Some((
                    url.clone(),
                    [
                        rect.origin.x,
                        rect.origin.y - self.scroll_y,
                        rect.size.width,
                        rect.size.height,
                    ],
                ))
            })
            .collect()
    }

    /// Return the current visible logical links without changing retained
    /// layout. `None` means no completed frame has made this document's
    /// geometry publishable yet.
    pub(crate) fn retained_accessible_links(&self) -> Option<Vec<RetainedAccessibleLink>> {
        if !self.presented {
            return None;
        }
        let layout = self.layout.as_ref()?;
        let mut links: Vec<RetainedAccessibleLink> = Vec::new();
        for region in &layout.packet.interactions {
            let InteractionKind::Link { url } = &region.kind else {
                continue;
            };
            let Some(semantics) = &region.link_semantics else {
                continue;
            };
            let Some(rect) = self.viewport_rect(region.bounds) else {
                continue;
            };
            if let Some(link) = links
                .iter_mut()
                .find(|link| link.identity == semantics.identity)
            {
                link.rects.push(rect);
            } else {
                links.push(RetainedAccessibleLink {
                    identity: semantics.identity,
                    label: semantics.accessible_label.clone(),
                    url: url.clone(),
                    rects: vec![rect],
                });
            }
        }
        Some(links)
    }

    /// Resolve a current, visible pointer point for one semantic link token.
    /// This recomputes from the retained packet and current scroll rather than
    /// accepting a snapshot rectangle, and never asks layout to rebuild.
    pub(crate) fn retained_accessible_pointer_target(
        &self,
        identity: SemanticInteractionId,
    ) -> Option<(f32, f32)> {
        if !self.presented {
            return None;
        }
        let layout = self.layout.as_ref()?;
        for region in layout.packet.interactions.iter().rev() {
            let Some(semantics) = &region.link_semantics else {
                continue;
            };
            if semantics.identity != identity {
                continue;
            }
            let Some([x, y, width, height]) = self.viewport_rect(region.bounds) else {
                continue;
            };
            let point = (x + width * 0.5, y + height * 0.5);
            let document_y = point.1 + self.scroll_y;
            let current_topmost = layout
                .packet
                .interactions
                .iter()
                .rev()
                .find(|candidate| rect_contains(candidate.bounds, point.0, document_y));
            if current_topmost
                .and_then(|candidate| candidate.link_semantics.as_ref())
                .is_some_and(|current| current.identity == identity)
            {
                return Some(point);
            }
        }
        None
    }

    /// Clip a retained full-document rect into this document's current
    /// viewport. The returned rectangle and all pointer targets derived from
    /// it remain inside the content hole.
    fn viewport_rect(&self, bounds: Rect) -> Option<[f32; 4]> {
        let left = bounds.origin.x.max(0.0);
        let right = bounds.max_x().min(self.size.0 as f32);
        let top = (bounds.origin.y - self.scroll_y).max(0.0);
        let bottom = (bounds.max_y() - self.scroll_y).min(self.size.1 as f32);
        (left < right && top < bottom).then_some([left, top, right - left, bottom - top])
    }

    /// Resolve a viewport-local click through the retained full-document packet.
    pub fn click_at(&mut self, x: f32, y: f32, width: u32, height: u32) -> Option<InteractionKind> {
        self.ensure_layout(width, height);
        self.layout
            .as_ref()?
            .packet
            .interaction_at(x, y + self.scroll_y)
            .cloned()
    }
}

fn rect_contains(rect: Rect, x: f32, y: f32) -> bool {
    x >= rect.origin.x && x <= rect.max_x() && y >= rect.origin.y && y <= rect.max_y()
}

#[cfg(feature = "smolweb")]
fn promote_inline_image_links(document: &mut EngineDocument, policy: SmolwebInlineMediaPolicy) {
    if !policy.enabled || !is_gemtext_document(document) {
        return;
    }

    let base_address = document.address.clone();
    let mut promoted = 0;
    for block in &mut document.blocks {
        if promoted >= policy.max_images {
            break;
        }
        let Some((href, alt)) = image_link(block) else {
            continue;
        };
        let resolved = genet_host_api::resolve_href(&base_address, &href);
        if !looks_like_image_url(&resolved) {
            continue;
        }
        *block = Block::Image { url: resolved, alt };
        promoted += 1;
    }
}

#[cfg(feature = "smolweb")]
fn is_gemtext_document(document: &EngineDocument) -> bool {
    document
        .content_type
        .split(';')
        .next()
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("text/gemini"))
}

#[cfg(feature = "smolweb")]
fn image_link(block: &Block) -> Option<(String, String)> {
    let Block::Paragraph { spans } = block else {
        return None;
    };
    let [InlineSpan::Link { url, spans, .. }] = spans.as_slice() else {
        return None;
    };
    Some((url.clone(), inline_text(spans)))
}

#[cfg(feature = "smolweb")]
fn looks_like_image_url(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let extension = path.rsplit_once('.').map(|(_, extension)| extension);
    extension.is_some_and(|extension| {
        matches!(
            extension.to_ascii_lowercase().as_str(),
            "avif" | "bmp" | "gif" | "ico" | "jfif" | "jpeg" | "jpg" | "png" | "webp"
        )
    })
}

fn decode_image(bytes: &[u8], policy: SmolwebInlineMediaPolicy) -> Option<DecodedImage> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(policy.max_dimension);
    limits.max_image_height = Some(policy.max_dimension);
    limits.max_alloc = Some(policy.max_decoded_bytes_per_image as u64);
    reader.limits(limits);
    let decoded = reader.decode().ok()?;
    let (width, height) = decoded.dimensions();
    let rgba_bytes = u64::from(width)
        .checked_mul(u64::from(height))?
        .checked_mul(4)?;
    if rgba_bytes > policy.max_decoded_bytes_per_image as u64 {
        return None;
    }
    Some(DecodedImage {
        width,
        height,
        rgba8: decoded.to_rgba8().into_raw(),
    })
}

#[cfg(feature = "smolweb")]
fn lower(url: &str, body: &str) -> EngineDocument {
    let scheme = url
        .split_once("://")
        .map(|(scheme, _)| scheme)
        .unwrap_or("");
    let engine: Box<dyn Engine> = match scheme {
        "gopher" => Box::new(nematic::GopherEngine::new()),
        "nex" => Box::new(nematic::NexEngine::new()),
        "finger" => Box::new(nematic::FingerEngine::new()),
        "spartan" => Box::new(nematic::SpartanEngine::new()),
        "titan" => Box::new(nematic::TitanEngine::new()),
        "misfin" => Box::new(nematic::MisfinEngine::new()),
        "guppy" => Box::new(nematic::GuppyEngine::new()),
        "scroll" => Box::new(nematic::ScrollEngine::new()),
        _ if looks_like_feed(body) => Box::new(nematic::FeedEngine::new()),
        _ => Box::new(nematic::GemtextEngine::new()),
    };
    let input = EngineInput::new(url, body);
    engine.render(&input).unwrap_or_else(|_| {
        nematic::GemtextEngine::new()
            .render(&input)
            .expect("gemtext lowering is infallible")
    })
}

#[cfg(feature = "smolweb")]
fn looks_like_feed(body: &str) -> bool {
    let body = body.trim_start();
    body.starts_with("<?xml") || body.starts_with("<rss") || body.starts_with("<feed")
}

/// The content types whose whole document is a fixed-width menu, so its body
/// font has to be the monospace one.
///
/// A gopher menu's informational lines lower to `Preformatted` and carry ASCII
/// art and column alignment, but its selector lines lower to a `Paragraph` with
/// a link span, because `Preformatted` holds text and cannot hold a link. Those
/// lines would otherwise take the body serif and break the very column grid the
/// lines above and below them establish. Nex listings have the same shape.
const FIXED_WIDTH_MENU_TYPES: &[&str] = &["application/gopher-menu", "application/x-nex-listing"];

fn style_for_theme(
    theme: &SmolwebTheme,
    url: &str,
    content_type: &str,
) -> (DocumentStyleSheet, [f32; 4]) {
    let palette = match theme {
        SmolwebTheme::Site => site_palette(url),
        SmolwebTheme::Plain => fixed_palette("#ffffff", "#1a1a1a", "#0b57d0", "#555555", "#f4f4f4"),
        SmolwebTheme::Light | SmolwebTheme::System => {
            fixed_palette("#fbfaf7", "#23211c", "#1a6e57", "#5b574e", "#f0eee8")
        },
        SmolwebTheme::Dark => fixed_palette("#16181c", "#e6e3dc", "#7db4ff", "#a8a49a", "#21242a"),
        SmolwebTheme::App(palette) => palette.clone(),
    };
    let defaults = ColorVocabulary::default();
    let background = parse_color(&palette.bg).unwrap_or([1.0, 1.0, 1.0, 1.0]);
    let foreground = parse_color(&palette.fg).unwrap_or(defaults.body_text);
    let link = parse_color(&palette.link).unwrap_or(defaults.link_text);
    let quote = parse_color(&palette.quote).unwrap_or(defaults.badge_text);
    let pre = parse_color(&palette.pre_bg).unwrap_or(defaults.placeholder_image);
    let mut style = DocumentStyleSheet::default();
    // These values are user-facing defaults rather than host chrome. Hosts
    // can still pass an explicit sheet through `from_document`.
    style.mono_font_family = "monospace".into();
    style.body_font_family = if FIXED_WIDTH_MENU_TYPES.contains(&content_type) {
        style.mono_font_family.clone()
    } else {
        "serif".into()
    };
    style.body_font_size = 16.0;
    style.line_height_ratio = 1.5;
    style.horizontal_padding = 32.0;
    style.max_content_width = Some(720.0);
    style.vertical_padding = 24.0;
    style.colors = ColorVocabulary {
        body_text: foreground,
        heading_text: foreground,
        link_text: link,
        code_text: foreground,
        badge_text: quote,
        rule: quote,
        placeholder_text: foreground,
        placeholder_image: pre,
    };
    (style, background)
}

fn fixed_palette(bg: &str, fg: &str, link: &str, quote: &str, pre_bg: &str) -> SmolwebPalette {
    SmolwebPalette {
        bg: bg.into(),
        fg: fg.into(),
        link: link.into(),
        quote: quote.into(),
        pre_bg: pre_bg.into(),
    }
}

fn site_palette(url: &str) -> SmolwebPalette {
    let hue = hue_from_host(url);
    let css = |saturation, lightness| {
        let [r, g, b, _] = hsl(hue as f32, saturation, lightness);
        format!(
            "rgb({}, {}, {})",
            (r * 255.0).round() as u8,
            (g * 255.0).round() as u8,
            (b * 255.0).round() as u8
        )
    };
    SmolwebPalette {
        // A restrained capsule tint: legibility stays stable while related
        // pages retain a little identity. `App` remains the host-theme path.
        bg: css(0.22, 0.975),
        fg: css(0.20, 0.14),
        link: css(0.62, 0.34),
        quote: css(0.15, 0.38),
        pre_bg: css(0.24, 0.93),
    }
}

fn hue_from_host(url: &str) -> u16 {
    let host = url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or("");
    let hash = host.bytes().fold(5381_u32, |hash, byte| {
        hash.wrapping_mul(33).wrapping_add(u32::from(byte))
    });
    (hash % 360) as u16
}

fn hsl(hue: f32, saturation: f32, lightness: f32) -> [f32; 4] {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let sector = (hue.rem_euclid(360.0)) / 60.0;
    let x = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match sector as u8 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = lightness - chroma / 2.0;
    [r + m, g + m, b + m, 1.0]
}

fn parse_color(value: &str) -> Option<[f32; 4]> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix('#') {
        let (r, g, b) = match hex.len() {
            3 => {
                let mut chars = hex.chars();
                let expand = |c: char| u8::from_str_radix(&format!("{c}{c}"), 16).ok();
                (
                    expand(chars.next()?)?,
                    expand(chars.next()?)?,
                    expand(chars.next()?)?,
                )
            },
            6 => (
                u8::from_str_radix(&hex[0..2], 16).ok()?,
                u8::from_str_radix(&hex[2..4], 16).ok()?,
                u8::from_str_radix(&hex[4..6], 16).ok()?,
            ),
            _ => return None,
        };
        return Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]);
    }
    let body = value.strip_prefix("rgb(")?.strip_suffix(')')?;
    let channels: Vec<u8> = body
        .split(',')
        .map(|channel| channel.trim().parse())
        .collect::<Result<_, _>>()
        .ok()?;
    (channels.len() == 3).then(|| {
        [
            channels[0] as f32 / 255.0,
            channels[1] as f32 / 255.0,
            channels[2] as f32 / 255.0,
            1.0,
        ]
    })
}

#[cfg(all(test, feature = "smolweb"))]
mod tests {
    use super::*;

    fn test_png() -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(2, 1, image::Rgba([20, 40, 60, 255]));
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .expect("encode PNG fixture");
        bytes
    }

    #[test]
    fn gemtext_uses_engine_document_and_paints_text() {
        let mut doc = SmolwebDocument::parse(
            "gemini://x.test/",
            "# Hello\n\nWorld.\n",
            SmolwebTheme::Site,
        );
        assert_eq!(
            doc.document().provenance.source_kind.as_deref(),
            Some(nematic::ENGINE_GEMTEXT)
        );
        let scene = doc.frame(800, 600);
        assert!(
            scene
                .ops
                .iter()
                .any(|op| matches!(op, netrender::SceneOp::GlyphRun(_)))
        );
        assert!(matches!(
            scene.ops.first(),
            Some(netrender::SceneOp::Rect(_))
        ));
    }

    #[test]
    fn opted_in_gemtext_image_link_fetches_decodes_paints_and_stays_clickable() {
        let mut doc = SmolwebDocument::parse_with_inline_media(
            "gemini://x.test/posts/index.gmi",
            "=> media/picture.png A picture\n",
            SmolwebTheme::Plain,
            SmolwebInlineMediaPolicy::images(),
        );
        assert!(matches!(
            &doc.document().blocks[0],
            Block::Image { url, alt }
                if url == "gemini://x.test/posts/media/picture.png" && alt == "A picture"
        ));
        assert_eq!(
            doc.subresources(),
            ["gemini://x.test/posts/media/picture.png"]
        );
        assert!(doc.provide_subresource("gemini://x.test/posts/media/picture.png", &test_png()));
        assert!(doc.subresources().is_empty());

        let scene = doc.frame(400, 300);
        assert!(
            scene
                .ops
                .iter()
                .any(|operation| matches!(operation, netrender::SceneOp::Image(_)))
        );
        let (url, [x, y, width, height]) =
            doc.links().into_iter().next().expect("image link region");
        assert_eq!(url, "gemini://x.test/posts/media/picture.png");
        assert!(matches!(
            doc.click_at(x + width / 2.0, y + height / 2.0, 400, 300),
            Some(InteractionKind::Link { url })
                if url == "gemini://x.test/posts/media/picture.png"
        ));
    }

    #[test]
    fn replacing_a_streamed_body_rebuilds_structure_and_invalidates_layout() {
        let mut doc = SmolwebDocument::parse_with_inline_media(
            "gemini://x.test/live",
            "# Prefix\n=> first.png First\n",
            SmolwebTheme::Plain,
            SmolwebInlineMediaPolicy::images(),
        );
        let _ = doc.frame(400, 300);
        assert!(doc.layout.is_some());

        doc.replace_body(
            "gemini://x.test/live",
            "# Complete\n=> second.png Second\nTail\n",
        );

        assert_eq!(doc.document().title.as_deref(), Some("Complete"));
        assert_eq!(doc.subresources(), ["gemini://x.test/second.png"]);
        assert!(doc.layout.is_none());
        assert!(
            doc.frame(400, 300)
                .ops
                .iter()
                .any(|operation| matches!(operation, netrender::SceneOp::GlyphRun(_)))
        );
    }

    #[test]
    fn streamed_replacement_scene_retains_prefix_runs_and_adds_tail_runs() {
        let url = "gemini://x.test/streaming.gmi";
        let prefix = "# Streaming prefix visible\nThis arrived before connection close.\n";
        let complete = concat!(
            "# Streaming prefix visible\n",
            "This arrived before connection close.\n",
            "## Streaming tail arrived\n",
            "The terminal body is complete.\n",
        );
        let mut doc = SmolwebDocument::parse(url, prefix, SmolwebTheme::Plain);
        let prefix_scene = doc.frame(614, 600);
        let prefix_runs: Vec<_> = prefix_scene
            .iter_glyph_runs()
            .map(|run| {
                (
                    run.font_size.to_bits(),
                    run.glyphs
                        .iter()
                        .map(|glyph| (glyph.id, glyph.x.to_bits(), glyph.y.to_bits()))
                        .collect::<Vec<_>>(),
                )
            })
            .collect();

        doc.replace_body(url, complete);
        let complete_scene = doc.frame(614, 600);
        let complete_runs: Vec<_> = complete_scene
            .iter_glyph_runs()
            .map(|run| {
                (
                    run.font_size.to_bits(),
                    run.glyphs
                        .iter()
                        .map(|glyph| (glyph.id, glyph.x.to_bits(), glyph.y.to_bits()))
                        .collect::<Vec<_>>(),
                )
            })
            .collect();

        assert!(complete_runs.len() > prefix_runs.len());
        assert!(
            prefix_runs
                .iter()
                .all(|prefix_run| complete_runs.contains(prefix_run)),
            "the full replacement scene must still carry every prefix glyph run"
        );
    }

    #[test]
    fn long_document_scrolls_and_windows() {
        let body: String = (0..200).map(|i| format!("Line {i}\n\n")).collect();
        let mut doc = SmolwebDocument::parse("gemini://x.test/", &body, SmolwebTheme::Plain);
        assert!(doc.content_height(400, 300) > 300);
        assert!(!doc.scroll_by(0.0, -50.0));
        assert!(doc.scroll_by(0.0, 240.0));
        assert_eq!(doc.frame(400, 300).viewport_height, 300);
    }

    #[test]
    fn links_and_clicks_share_viewport_coordinates() {
        let mut body: String = (0..30).map(|i| format!("Line {i}\n\n")).collect();
        body.push_str("=> gemini://x.test/page A link\n");
        let mut doc = SmolwebDocument::parse("gemini://x.test/", &body, SmolwebTheme::Plain);
        assert!(doc.links().is_empty());
        let _ = doc.frame(400, 300);
        let (url, [_, initial_y, _, _]) = doc.links().into_iter().next().expect("link region");
        assert_eq!(url, "gemini://x.test/page");
        doc.scroll_to(f32::MAX);
        let (url, [x, y, width, height]) = doc
            .links()
            .into_iter()
            .next()
            .expect("scrolled link region");
        assert_eq!(url, "gemini://x.test/page");
        assert!(y < initial_y, "scrolling must move the host hit table");
        assert!(width > 0.0 && height > 0.0);
        assert!(matches!(
            doc.click_at(x + width / 2.0, y + height / 2.0, 400, 300),
            Some(InteractionKind::Link { url }) if url == "gemini://x.test/page"
        ));
    }

    #[test]
    fn spartan_prompt_click_remains_a_submission() {
        let mut doc = SmolwebDocument::parse(
            "spartan://x.test/guestbook",
            "=: /guestbook/sign Sign it\n",
            SmolwebTheme::Plain,
        );
        let _ = doc.frame(400, 300);
        let region = doc
            .layout
            .as_ref()
            .expect("layout")
            .packet
            .interactions
            .iter()
            .find(|region| matches!(region.kind, InteractionKind::Submit { .. }))
            .expect("prompt region");
        let x0 = region.bounds.origin.x;
        let y0 = region.bounds.origin.y;
        let x1 = x0 + region.bounds.size.width;
        let y1 = y0 + region.bounds.size.height;
        assert!(matches!(
            doc.click_at((x0 + x1) / 2.0, (y0 + y1) / 2.0, 400, 300),
            Some(InteractionKind::Submit { target }) if target == "/guestbook/sign"
        ));
    }

    #[test]
    fn schemes_select_their_nematic_engines() {
        let gopher = SmolwebDocument::parse(
            "gopher://x.test/",
            "1Files\t/files\tx.test\t70\r\n",
            SmolwebTheme::Plain,
        );
        assert_eq!(
            gopher.document().provenance.source_kind.as_deref(),
            Some(nematic::ENGINE_GOPHER)
        );

        let feed = SmolwebDocument::parse(
            "gemini://x.test/feed",
            "<?xml version=\"1.0\"?><rss version=\"2.0\"><channel><title>Log</title></channel></rss>",
            SmolwebTheme::Dark,
        );
        assert_eq!(
            feed.document().provenance.source_kind.as_deref(),
            Some(nematic::ENGINE_FEED)
        );
    }

    /// A gopher menu is one fixed-width document. Its info lines lower to
    /// `Preformatted` and its selector lines to a paragraph with a link span,
    /// so a serif body font renders the links in a different typeface from the
    /// ASCII art directly above them and the columns stop lining up.
    #[test]
    fn a_fixed_width_menu_sets_its_body_font_to_the_monospace_one() {
        for content_type in ["application/gopher-menu", "application/x-nex-listing"] {
            let (style, _) = style_for_theme(&SmolwebTheme::Dark, "gopher://x.test/", content_type);
            assert_eq!(
                style.body_font_family, style.mono_font_family,
                "{content_type} must not mix typefaces inside one column grid"
            );
        }
        // A prose format keeps the body serif: gemtext is not column-aligned.
        for content_type in ["text/gemini", "text/plain"] {
            let (style, _) = style_for_theme(&SmolwebTheme::Dark, "gemini://x.test/", content_type);
            assert_eq!(style.body_font_family, "serif", "{content_type} is prose");
        }
    }

    #[test]
    fn app_rgb_palette_maps_into_document_style() {
        let theme = SmolwebTheme::App(SmolwebPalette {
            bg: "rgb(16, 32, 48)".into(),
            fg: "rgb(250, 250, 250)".into(),
            link: "rgb(51, 204, 255)".into(),
            quote: "rgb(153, 170, 187)".into(),
            pre_bg: "rgb(10, 22, 34)".into(),
        });
        let (style, background) = style_for_theme(&theme, "gemini://x.test/", "text/gemini");
        assert_eq!(background, [16.0 / 255.0, 32.0 / 255.0, 48.0 / 255.0, 1.0]);
        assert_eq!(
            style.colors.link_text,
            [51.0 / 255.0, 204.0 / 255.0, 1.0, 1.0]
        );
    }
}
