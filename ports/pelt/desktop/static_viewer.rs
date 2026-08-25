/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Pelt's headed single-document adapter.
//!
//! A thin winit shell over an engine-owned document session, presented through
//! the shared [`SurfaceHost`](genet_winit_host::SurfaceHost):
//! the second instance of the orrery-host pattern (a window-agnostic content lib
//! plus a thin shell that maps winit events onto the content's semantic input and
//! rasterizes + composites its scene per frame). The document is the content;
//! wheel scrolling is its only interaction in V1, fed through the shared
//! default-action helper into the document's viewport.

use crate::{DesktopHostProfile, WindowingMode};
use genet_host_api::EngineProfile;

/// Configuration for one single-document host run.
pub struct StaticViewerConfig {
    pub profile: DesktopHostProfile,
    pub url: String,
    pub title: String,
    /// Requested physical client size. `None` keeps the profile's established size.
    pub size: Option<(u32, u32)>,
    /// Exit after this many presented frames. `None` keeps the window interactive.
    pub frames: Option<u32>,
}

impl StaticViewerConfig {
    pub fn new(engine: EngineProfile, windowing: WindowingMode, url: impl Into<String>) -> Self {
        let url = url.into();
        Self {
            profile: DesktopHostProfile::new(engine, windowing),
            title: "Pelt".into(),
            url,
            size: None,
            frames: None,
        }
    }

    /// Request a physical client size for a headed run.
    pub fn with_size(mut self, width: u32, height: u32) -> Self {
        self.size = Some((width.max(1), height.max(1)));
        self
    }

    /// Exit after presenting `frames` frames, for deterministic headed smoke runs.
    pub fn with_frame_limit(mut self, frames: u32) -> Self {
        self.frames = Some(frames.max(1));
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticViewerOutcome {
    pub url: String,
    pub created_window: bool,
    pub redraws: u32,
    /// The physical client size the headed run actually achieved, or `(0, 0)` when
    /// no window was created.
    pub size: (u32, u32),
}

/// The single-content history owned by Pelt's present host. P3 moves this
/// state into one entry per tile, but even the first adapter must not let a
/// document session silently replace itself.
#[derive(Clone, Debug, Eq, PartialEq)]
struct NavigationState {
    entries: Vec<String>,
    active: usize,
}

impl NavigationState {
    fn new(address: String) -> Self {
        Self {
            entries: vec![address],
            active: 0,
        }
    }

    fn current(&self) -> &str {
        &self.entries[self.active]
    }

    fn commit(&mut self, address: String) {
        self.entries.truncate(self.active + 1);
        self.entries.push(address);
        self.active = self.entries.len() - 1;
    }

    fn back(&mut self) -> Option<&str> {
        (self.active > 0).then(|| {
            self.active -= 1;
            self.current()
        })
    }

    fn previous(&self) -> Option<&str> {
        self.active
            .checked_sub(1)
            .map(|index| self.entries[index].as_str())
    }

    fn forward(&mut self) -> Option<&str> {
        (self.active + 1 < self.entries.len()).then(|| {
            self.active += 1;
            self.current()
        })
    }

    fn next(&self) -> Option<&str> {
        self.entries.get(self.active + 1).map(String::as_str)
    }
}

/// Presentation-level keyboard scroll actions. Engine adapters translate this
/// vocabulary at their boundary; the window shell does not own a layout engine.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ViewerScrollKey {
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
}

/// Turn a document title into the stable native-window title, falling back to
/// the loaded URL's host.
///
/// Gemini, gopher, finger and nex carry no title element, so without the
/// fallback every capsule opens a window called plain "Pelt" -- indistinguishable
/// in the taskbar from every other one.
#[cfg(feature = "present")]
pub(crate) fn pelt_window_title(document_title: Option<&str>, url: Option<&str>) -> String {
    let named = document_title
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .or_else(|| url.and_then(crate::url_host));
    match named {
        Some(title) => format!("Pelt — {title}"),
        None => "Pelt".into(),
    }
}

/// Pelt's native window treatment, shared by all three headed profiles. The icon is
/// intentionally generated here: it is a real taskbar/title-bar icon without adding a
/// platform-specific asset pipeline to the reference host.
#[cfg(feature = "present")]
pub(crate) fn pelt_window_attributes(
    title: impl Into<String>,
    width: u32,
    height: u32,
) -> winit::window::WindowAttributes {
    use winit::dpi::PhysicalSize;
    use winit::window::{Icon, Window};

    const EDGE: u32 = 32;
    let mut rgba = vec![0; (EDGE * EDGE * 4) as usize];
    for y in 0..EDGE {
        for x in 0..EDGE {
            let i = ((y * EDGE + x) * 4) as usize;
            let (r, g, b) = if (4..28).contains(&x) && (4..28).contains(&y) {
                if (9..23).contains(&x) && (9..15).contains(&y)
                    || (9..15).contains(&x) && (9..24).contains(&y)
                    || (14..23).contains(&x) && (18..24).contains(&y)
                {
                    (133, 202, 255)
                } else {
                    (43, 43, 51)
                }
            } else {
                (0, 0, 0)
            };
            rgba[i..i + 4].copy_from_slice(&[
                r,
                g,
                b,
                if r == 0 && g == 0 && b == 0 { 0 } else { 255 },
            ]);
        }
    }
    let icon = Icon::from_rgba(rgba, EDGE, EDGE).expect("the fixed Pelt icon is valid RGBA");
    let attributes = Window::default_attributes()
        .with_title(title)
        .with_inner_size(PhysicalSize::new(width.max(1), height.max(1)))
        .with_window_icon(Some(icon));
    #[cfg(windows)]
    let attributes = {
        use winit::platform::windows::{Color, WindowAttributesExtWindows};
        attributes
            .with_title_background_color(Some(Color::from_rgb(43, 43, 51)))
            .with_title_text_color(Color::from_rgb(245, 245, 247))
    };
    attributes
}

/// Convert a physical window extent to the logical CSS/layout extent used by
/// Pelt's scenes. Keep this conversion beside the window attributes so every
/// headed profile shares the same DPI convention.
pub(crate) fn logical_extent(physical: u32, scale_factor: f32) -> u32 {
    ((physical.max(1) as f32 / scale_factor.max(1.0)).round() as u32).max(1)
}

/// Convert a physical winit pointer coordinate to the matching logical scene
/// coordinate. Layout, painting, and hit tests all use this one space.
pub(crate) fn logical_position(physical: f32, scale_factor: f32) -> f32 {
    physical / scale_factor.max(1.0)
}

#[cfg(test)]
mod dpi_tests {
    use super::{logical_extent, logical_position};

    #[test]
    fn physical_window_space_maps_to_one_logical_scene_space() {
        assert_eq!(logical_extent(1600, 2.0), 800);
        assert_eq!(logical_extent(900, 1.5), 600);
        assert!((logical_position(640.0, 2.0) - 320.0).abs() < f32::EPSILON);
    }
}

#[cfg(all(test, feature = "present"))]
mod title_tests {
    use super::pelt_window_title;

    /// A titled document names the window. The smolweb formats carry no title
    /// element at all, so without a fallback every capsule opened a window
    /// called plain "Pelt" and the taskbar could not tell two of them apart.
    #[test]
    fn a_window_is_named_by_its_document_then_by_its_host() {
        let named = Some("Merely | Local-first software");
        assert_eq!(
            pelt_window_title(named, Some("https://merelyllc.com")),
            "Pelt — Merely | Local-first software"
        );
        for url in [
            "gemini://geminiprotocol.net/",
            "gemini://user@geminiprotocol.net:1965/page",
        ] {
            assert_eq!(
                pelt_window_title(None, Some(url)),
                "Pelt — geminiprotocol.net",
                "naming a window for {url}"
            );
        }
        // A blank title is as absent as no title.
        assert_eq!(
            pelt_window_title(Some("   "), Some("gopher://gopher.floodgap.com/")),
            "Pelt — gopher.floodgap.com"
        );
        // Nothing to fall back on: a local file has no authority.
        assert_eq!(pelt_window_title(None, Some("C:\\docs\\a.html")), "Pelt");
        assert_eq!(pelt_window_title(None, None), "Pelt");
    }
}

/// Product-level local receipt for the explicit Livery pin. It uses the same
/// registry construction as `run_livery_viewer`, but keeps scene inspection
/// GPU-free so the resource and interaction assertions are stable in CI.
#[cfg(all(test, feature = "livery"))]
mod livery_route_tests {
    use genet_documents::{LiveryDocumentSession, LiverySessionEngine, LocalFetcher};
    use inker::{SessionRegistry, SessionScrollKey, SessionSpawnRequest};
    use layout_dom_api::{LayoutDom, LocalName, Namespace, NodeKind};
    use netrender::{Scene, SceneOp};

    fn node_by_id(
        dom: &genet_static_dom::StaticDocument,
        expected: &str,
    ) -> genet_static_dom::StaticNodeId {
        fn find(
            dom: &genet_static_dom::StaticDocument,
            node: genet_static_dom::StaticNodeId,
            expected: &str,
        ) -> Option<genet_static_dom::StaticNodeId> {
            if dom.kind(node) == NodeKind::Element
                && dom.attribute(node, &Namespace::default(), &LocalName::from("id"))
                    == Some(expected)
            {
                return Some(node);
            }
            dom.dom_children(node)
                .find_map(|child| find(dom, child, expected))
        }

        find(dom, dom.document(), expected).expect("fixture element by id")
    }

    #[test]
    fn local_livery_route_keeps_resource_identity_and_interaction_after_resize() {
        let fixture = format!(
            r"{}\..\examples\livery-route\index.html",
            env!("CARGO_MANIFEST_DIR")
        );
        let mut registry: SessionRegistry<Scene> = SessionRegistry::new();
        registry.register(Box::new(LiverySessionEngine::new(LocalFetcher)));
        let request = SessionSpawnRequest::new(&fixture).with_viewport(960, 640);
        let mut session = registry
            .spawn(inker::routing::ENGINE_GENET_LIVERY, &request)
            .expect("Pelt can spawn the explicit Livery pin");

        let first = session.frame(960, 640);
        assert!(
            first
                .ops
                .iter()
                .any(|operation| matches!(operation, SceneOp::Image(_))),
            "the linked CSS background or HTML image reaches the product scene"
        );
        let collapsed_caption_runs = first
            .ops
            .iter()
            .filter_map(|operation| match operation {
                SceneOp::GlyphRun(run)
                    if run.color
                        == [
                            f32::from(0x6b_u8) / 255.0,
                            f32::from(0x1f_u8) / 255.0,
                            f32::from(0x2d_u8) / 255.0,
                            1.0,
                        ] =>
                {
                    run.glyphs
                        .first()
                        .filter(|glyph| glyph.y > 200.0)
                        .map(|glyph| (glyph.x, glyph.y, run.glyphs.len()))
                },
                _ => None,
            })
            .collect::<Vec<_>>();
        let collapsed_cell_runs = first
            .ops
            .iter()
            .filter_map(|operation| match operation {
                SceneOp::GlyphRun(run)
                    if run.color
                        == [
                            f32::from(0x3d_u8) / 255.0,
                            f32::from(0x2b_u8) / 255.0,
                            f32::from(0x1f_u8) / 255.0,
                            1.0,
                        ] =>
                {
                    run.glyphs
                        .first()
                        .filter(|glyph| glyph.y > 400.0 && run.glyphs.len() == 3)
                        .map(|glyph| (glyph.x, glyph.y))
                },
                _ => None,
            })
            .collect::<Vec<_>>();
        let caption_last_baseline = collapsed_caption_runs
            .iter()
            .map(|(_, baseline, _)| *baseline)
            .max_by(f32::total_cmp)
            .expect("collapsed caption paints its wrapped text");
        let cell_first_baseline = collapsed_cell_runs
            .iter()
            // `CSS` in the preceding separate-border table is also a three
            // glyph brown cell run. The collapsed table is the first such
            // cell after its own caption, which is the relationship this
            // receipt is checking.
            .filter(|(_, baseline)| *baseline > caption_last_baseline)
            .map(|(_, baseline)| *baseline)
            .min_by(f32::total_cmp)
            .expect("collapsed table paints its cells");
        assert!(
            caption_last_baseline + 16.0 <= cell_first_baseline,
            "the collapsed table grid must begin after every caption line: \
             captions={collapsed_caption_runs:?} cells={collapsed_cell_runs:?}"
        );

        let concrete = session
            .as_any()
            .downcast_mut::<LiveryDocumentSession>()
            .expect("the registry returned a Livery document session");
        let resources = concrete.resource_set();
        assert_eq!(
            resources.stylesheets.len(),
            4,
            "two inline and two linked sheets"
        );
        assert!(
            resources.stylesheets.iter().any(|sheet| sheet
                .source_url
                .as_deref()
                .is_some_and(|url| url.replace('\\', "/").ends_with("assets/route.css"))),
            "the linked stylesheet retains its own local identity"
        );
        assert!(
            resources.resources.iter().any(|resource| resource
                .resolved_url
                .replace('\\', "/")
                .ends_with("resources/servo_64.png")),
            "the linked image remains attributed to its source-relative URL"
        );
        assert!(
            resources.resources.iter().any(|resource| resource
                .resolved_url
                .replace('\\', "/")
                .ends_with("assets/../../Ahem.ttf")),
            "the linked font remains attributed to its stylesheet-relative URL"
        );
        assert!(
            resources.diagnostics.is_empty(),
            "the product fixture has no missing or deferred resources: {:?}",
            resources.diagnostics
        );

        let body = node_by_id(concrete.document().dom(), "route-body");
        assert_eq!(
            concrete
                .document()
                .computed_style(body, "background-color")
                .as_deref(),
            Some("rgb(243, 236, 220)"),
            "the print-media sheet does not apply on the screen route"
        );
        let source_order = node_by_id(concrete.document().dom(), "source-order");
        assert_eq!(
            concrete
                .document()
                .computed_style(source_order, "color")
                .as_deref(),
            Some("rgb(107, 31, 45)"),
            "the later inline stylesheet wins the linked sheet at equal specificity"
        );

        // A second frame at a different viewport is the same resize path the
        // headed viewer uses. Link geometry must survive it and drive fragment
        // navigation before ordinary viewport scrolling resumes.
        let _resized = session.frame(640, 480);
        assert!(
            session.content_height(640, 480) > 480,
            "fixture is scrollable"
        );
        let link = session
            .links()
            .into_iter()
            .find(|link| link.url == "#resource-target")
            .expect("fixture jump link");
        assert!(matches!(
            session.click_at(link.rect[0] + 2.0, link.rect[1] + 2.0),
            inker::SessionClick::Handled
        ));
        assert!(session.scroll_for_key(SessionScrollKey::Home));
        assert!(session.scroll_by(0.0, 120.0));
        assert!(session.scroll_at(8.0, 8.0, 0.0, -80.0));
    }
}

/// Compatibility entrypoint for callers of the former static viewer. Script-free
/// HTML is always routed to Livery/Buckram.
pub fn run_static_viewer(config: StaticViewerConfig) -> Result<StaticViewerOutcome, String> {
    match config.profile.windowing {
        WindowingMode::Headless => Ok(StaticViewerOutcome {
            url: config.url,
            created_window: false,
            redraws: 0,
            size: (0, 0),
        }),
        WindowingMode::Headed => run_livery_viewer(config),
    }
}

/// Run the owned Livery engine through its inker registry entry.
#[cfg(feature = "livery")]
pub fn run_livery_viewer(config: StaticViewerConfig) -> Result<StaticViewerOutcome, String> {
    use genet_documents::LiverySessionEngine;
    use inker::{SessionEngine, SessionSpawnRequest};
    use netrender::Scene;

    if matches!(config.profile.windowing, WindowingMode::Headless) {
        return Ok(StaticViewerOutcome {
            url: config.url,
            created_window: false,
            redraws: 0,
            size: (0, 0),
        });
    }
    let (width, height) = config.size.unwrap_or((800, 600));
    let engine: Box<dyn SessionEngine<Scene>> =
        Box::new(LiverySessionEngine::new(genet_documents::LocalFetcher));
    let request = SessionSpawnRequest::new(&config.url).with_viewport(width, height);
    let session = engine
        .spawn(&request)
        .map_err(|error| format!("could not spawn engine genet.livery: {error}"))?;
    run_headed_with(
        config,
        SessionViewerContent {
            session,
            posture: None,
            replacement_engine: Some(engine),
            primary_pointer_down: false,
        },
    )
}

#[cfg(not(feature = "livery"))]
pub fn run_livery_viewer(_config: StaticViewerConfig) -> Result<StaticViewerOutcome, String> {
    Err("the document host requires the livery feature".to_string())
}

/// Run held HTML through the shared fleece reader lane.
#[cfg(feature = "reader")]
pub fn run_reader_viewer(config: StaticViewerConfig) -> Result<StaticViewerOutcome, String> {
    use genet_documents::{ReaderSessionEngine, ResourceFetcher, SmolwebTheme};
    use inker::{SessionRegistry, SessionSpawnRequest};
    use netrender::Scene;

    if matches!(config.profile.windowing, WindowingMode::Headless) {
        return Ok(StaticViewerOutcome {
            url: config.url,
            created_window: false,
            redraws: 0,
            size: (0, 0),
        });
    }
    let source = ResourceFetcher::fetch(&genet_documents::LocalFetcher, &config.url)
        .ok_or_else(|| format!("could not load held reader source {}", config.url))?;
    let source = String::from_utf8_lossy(&source).into_owned();
    let (width, height) = config.size.unwrap_or((800, 600));
    let mut registry: SessionRegistry<Scene> = SessionRegistry::new();
    registry.register(Box::new(ReaderSessionEngine::new(SmolwebTheme::System)));
    let request = SessionSpawnRequest::new(&config.url)
        .with_body(source)
        .with_viewport(width, height);
    let session = registry
        .spawn(inker::routing::ENGINE_GENET_READER, &request)
        .map_err(|error| format!("could not spawn engine genet.reader: {error}"))?;
    let posture = session
        .inspect()
        .and_then(|report| report.lineage)
        .map(|lineage| {
            let score = lineage
                .score
                .map(|score| format!(" score {score}"))
                .unwrap_or_default();
            format!(
                "Reader · {} {} · {}{} · {} blocks",
                lineage.tool, lineage.version, lineage.selector, score, lineage.block_count
            )
        });
    run_headed_with(
        config,
        SessionViewerContent {
            session,
            posture,
            replacement_engine: None,
            primary_pointer_down: false,
        },
    )
}

#[cfg(any(feature = "livery", feature = "reader"))]
struct SessionViewerContent {
    session: Box<dyn inker::DocumentSession<netrender::Scene>>,
    posture: Option<String>,
    /// The P1 single-document replacement factory. P2 lifts this out of the
    /// desktop adapter into the embeddable host core.
    replacement_engine: Option<Box<dyn inker::SessionEngine<netrender::Scene>>>,
    primary_pointer_down: bool,
}

#[cfg(any(feature = "livery", feature = "reader"))]
impl windowed::ViewerContent for SessionViewerContent {
    fn title(&self) -> Option<String> {
        self.session.inspect().and_then(|report| report.title)
    }

    fn frame(&mut self, width: u32, height: u32) -> netrender::Scene {
        self.session.frame(width, height)
    }

    fn scroll_by(&mut self, dx: f32, dy: f32) -> bool {
        self.session.scroll_by(dx, dy)
    }

    fn scroll_at(&mut self, x: f32, y: f32, dx: f32, dy: f32) -> bool {
        self.session.scroll_at(x, y, dx, dy)
    }

    fn scroll_for_key(&mut self, key: ViewerScrollKey) -> bool {
        let key = match key {
            ViewerScrollKey::Up => inker::SessionScrollKey::LineUp,
            ViewerScrollKey::Down => inker::SessionScrollKey::LineDown,
            ViewerScrollKey::PageUp => inker::SessionScrollKey::PageUp,
            ViewerScrollKey::PageDown => inker::SessionScrollKey::PageDown,
            ViewerScrollKey::Home => inker::SessionScrollKey::Home,
            ViewerScrollKey::End => inker::SessionScrollKey::End,
            ViewerScrollKey::Left | ViewerScrollKey::Right => return false,
        };
        self.session.scroll_for_key(key)
    }

    fn click_at(&mut self, x: f32, y: f32) -> bool {
        matches!(self.session.click_at(x, y), inker::SessionClick::Handled)
    }

    fn handle_input(
        &mut self,
        input: genet_host_api::HostInput,
    ) -> Vec<genet_host_api::HostEffect> {
        use genet_host_api::{ButtonState, HostEffect, HostInput, PointerButton};

        fn click_effect(click: inker::SessionClick) -> Option<HostEffect> {
            match click {
                inker::SessionClick::Navigate(target) => Some(HostEffect::Navigate { target }),
                inker::SessionClick::Submit(target) => Some(HostEffect::Submit { target }),
                inker::SessionClick::Handled => Some(HostEffect::Redraw),
                inker::SessionClick::Miss => None,
            }
        }

        match input {
            HostInput::PointerMoved { x, y } if self.primary_pointer_down => self
                .session
                .pointer_move(x, y)
                .then_some(HostEffect::Redraw)
                .into_iter()
                .collect(),
            HostInput::PointerButton {
                button: PointerButton::Primary,
                state: ButtonState::Pressed,
                x,
                y,
            } => {
                self.primary_pointer_down = true;
                click_effect(self.session.pointer_down(x, y))
                    .into_iter()
                    .collect()
            },
            HostInput::PointerButton {
                button: PointerButton::Primary,
                state: ButtonState::Released,
                x,
                y,
            } => {
                if !std::mem::take(&mut self.primary_pointer_down) {
                    return Vec::new();
                }
                click_effect(self.session.pointer_up(x, y))
                    .into_iter()
                    .collect()
            },
            other => {
                let redraw = match other {
                    HostInput::Wheel { x, y, dx, dy } => self.session.scroll_at(x, y, dx, dy),
                    HostInput::Key { key, modifiers } => {
                        let key = match key {
                            genet_host_api::HostKey::ArrowUp => {
                                Some(inker::SessionScrollKey::LineUp)
                            },
                            genet_host_api::HostKey::ArrowDown => {
                                Some(inker::SessionScrollKey::LineDown)
                            },
                            genet_host_api::HostKey::PageUp => {
                                Some(inker::SessionScrollKey::PageUp)
                            },
                            genet_host_api::HostKey::PageDown => {
                                Some(inker::SessionScrollKey::PageDown)
                            },
                            genet_host_api::HostKey::Home => Some(inker::SessionScrollKey::Home),
                            genet_host_api::HostKey::End => Some(inker::SessionScrollKey::End),
                            genet_host_api::HostKey::Space if modifiers.shift => {
                                Some(inker::SessionScrollKey::PageUp)
                            },
                            genet_host_api::HostKey::Space => {
                                Some(inker::SessionScrollKey::PageDown)
                            },
                            _ => None,
                        };
                        key.is_some_and(|key| self.session.scroll_for_key(key))
                    },
                    _ => false,
                };
                redraw.then_some(HostEffect::Redraw).into_iter().collect()
            },
        }
    }

    fn replace_session(&mut self, address: &str, width: u32, height: u32) -> Result<(), String> {
        let engine = self
            .replacement_engine
            .as_ref()
            .ok_or_else(|| "this document route cannot create a replacement session".to_string())?;
        self.session = engine
            .spawn(&inker::SessionSpawnRequest::new(address).with_viewport(width, height))
            .map_err(|error| error.to_string())?;
        self.primary_pointer_down = false;
        Ok(())
    }

    fn posture(&self) -> Option<&str> {
        self.posture.as_deref()
    }
}

/// Open a window and present `content` (any [`ViewerContent`](windowed::ViewerContent))
/// through the shared winit shell until the window closes. The Livery viewer and the
/// scripted viewer ([`crate::scripted`]) are the two callers — same shell, different
/// document. Kept generic (not a trait object) so each content type monomorphizes and
/// the scripted profile can pick its JS engine at the call site.
#[cfg(feature = "present")]
pub(crate) fn run_headed_with<C: windowed::ViewerContent + 'static>(
    config: StaticViewerConfig,
    content: C,
) -> Result<StaticViewerOutcome, String> {
    use winit::event_loop::EventLoop;

    let event_loop =
        EventLoop::new().map_err(|error| format!("could not create event loop: {error}"))?;
    let mut app = windowed::ViewerApp::new(config, content);
    event_loop
        .run_app(&mut app)
        .map_err(|error| format!("viewer event loop failed: {error}"))?;
    Ok(app.outcome())
}

#[cfg(feature = "present")]
pub(crate) mod windowed {
    use std::sync::Arc;
    use std::time::Instant;

    use genet_winit_host::{SurfaceHost, wheel_delta_from_winit};
    use netrender::external_texture::ExternalTexturePlacement;
    use netrender::{ColorLoad, NetrenderOptions, Scene};
    use winit::application::ApplicationHandler;
    use winit::event::{ElementState, MouseButton, WindowEvent};
    use winit::event_loop::ActiveEventLoop;
    use winit::keyboard::{Key, NamedKey};
    use winit::window::{Window, WindowId};

    use genet_host_api::{
        ButtonState, HostEffect, HostInput, HostKey, InputModifiers, NavigationCommand,
        PointerButton,
    };

    use super::{NavigationState, StaticViewerConfig, StaticViewerOutcome, ViewerScrollKey};

    /// A document the viewer can present: render at a size, scroll, click, and (for
    /// scripted content) advance time-based work. Livery-backed static and scripted
    /// documents implement it, so they share this one winit shell. The Pelt
    /// host-reconstruction lane replaces this private seam with a public host core.
    pub(crate) trait ViewerContent {
        /// The document title for native window chrome, when this content has one.
        fn title(&self) -> Option<String> {
            None
        }
        /// Optional engine posture shown in native window chrome.
        fn posture(&self) -> Option<&str> {
            None
        }
        /// Render at `width`×`height` at the current scroll.
        fn frame(&mut self, width: u32, height: u32) -> Scene;
        /// Scroll by a device-px wheel delta; return whether the offset moved.
        fn scroll_by(&mut self, dx: f32, dy: f32) -> bool;
        /// Scroll by a device-px wheel delta at scene point `(x, y)`: the wheel default
        /// action routes to the nearest `overflow: scroll/auto` container under the
        /// pointer, falling through to the document viewport. Returns whether anything
        /// moved. The default ignores the position and scrolls the viewport (the
        /// behaviour for content with no retained per-element scroll. The retained
        /// Livery session overrides it with position-aware nested scrolling.
        fn scroll_at(&mut self, _x: f32, _y: f32, dx: f32, dy: f32) -> bool {
            self.scroll_by(dx, dy)
        }
        /// Apply a keyboard scroll default; return whether the offset moved.
        fn scroll_for_key(&mut self, key: ViewerScrollKey) -> bool;
        /// Handle a left click at a scene point; return whether the document scrolled.
        fn click_at(&mut self, x: f32, y: f32) -> bool;
        /// Translate a host-neutral event without exposing the concrete document
        /// type to the winit adapter. Engines that can distinguish a link or
        /// submission override this and return the corresponding host effect.
        fn handle_input(&mut self, input: HostInput) -> Vec<HostEffect> {
            let redraw = match input {
                HostInput::Wheel { x, y, dx, dy } => self.scroll_at(x, y, dx, dy),
                HostInput::PointerButton {
                    button: PointerButton::Primary,
                    state: ButtonState::Pressed,
                    x,
                    y,
                } => self.click_at(x, y),
                HostInput::Key { key, modifiers } => {
                    host_key_to_scroll(key, modifiers).is_some_and(|key| self.scroll_for_key(key))
                },
                _ => false,
            };
            redraw.then_some(HostEffect::Redraw).into_iter().collect()
        }
        /// Replace the retained session after the host resolved an address.
        /// Content that is not a browsable document keeps the default refusal.
        fn replace_session(
            &mut self,
            _address: &str,
            _width: u32,
            _height: u32,
        ) -> Result<(), String> {
            Err("this Pelt content does not support document navigation".to_string())
        }
        /// Advance time-based work (script timers + GC) to `now_ms`; return whether
        /// more is pending, so the shell keeps requesting frames. Static content has
        /// none — the default returns `false` and the shell redraws only on input.
        fn pump(&mut self, _now_ms: f64) -> bool {
            false
        }
    }

    /// Map a winit key (with the shift state) to a [`ViewerScrollKey`] default action, or
    /// `None` for keys that do not scroll. `Space` / `Shift+Space` are
    /// `PageDown` / `PageUp` (scope doc rule 5's key list). Pelt-inline for now; this
    /// lifts to `genet-winit-host` when meerkat shares the decode.
    fn scroll_key_from_winit(key: &Key, shift: bool) -> Option<ViewerScrollKey> {
        Some(match key {
            Key::Named(NamedKey::ArrowUp) => ViewerScrollKey::Up,
            Key::Named(NamedKey::ArrowDown) => ViewerScrollKey::Down,
            Key::Named(NamedKey::ArrowLeft) => ViewerScrollKey::Left,
            Key::Named(NamedKey::ArrowRight) => ViewerScrollKey::Right,
            Key::Named(NamedKey::PageUp) => ViewerScrollKey::PageUp,
            Key::Named(NamedKey::PageDown) => ViewerScrollKey::PageDown,
            Key::Named(NamedKey::Home) => ViewerScrollKey::Home,
            Key::Named(NamedKey::End) => ViewerScrollKey::End,
            Key::Named(NamedKey::Space) => {
                if shift {
                    ViewerScrollKey::PageUp
                } else {
                    ViewerScrollKey::PageDown
                }
            },
            _ => return None,
        })
    }

    fn host_key_to_scroll(key: HostKey, modifiers: InputModifiers) -> Option<ViewerScrollKey> {
        Some(match key {
            HostKey::ArrowUp => ViewerScrollKey::Up,
            HostKey::ArrowDown => ViewerScrollKey::Down,
            HostKey::ArrowLeft => ViewerScrollKey::Left,
            HostKey::ArrowRight => ViewerScrollKey::Right,
            HostKey::PageUp => ViewerScrollKey::PageUp,
            HostKey::PageDown => ViewerScrollKey::PageDown,
            HostKey::Home => ViewerScrollKey::Home,
            HostKey::End => ViewerScrollKey::End,
            HostKey::Space if modifiers.shift => ViewerScrollKey::PageUp,
            HostKey::Space => ViewerScrollKey::PageDown,
            _ => return None,
        })
    }

    fn host_key_from_winit(key: &Key) -> HostKey {
        match key {
            Key::Named(NamedKey::ArrowUp) => HostKey::ArrowUp,
            Key::Named(NamedKey::ArrowDown) => HostKey::ArrowDown,
            Key::Named(NamedKey::ArrowLeft) => HostKey::ArrowLeft,
            Key::Named(NamedKey::ArrowRight) => HostKey::ArrowRight,
            Key::Named(NamedKey::PageUp) => HostKey::PageUp,
            Key::Named(NamedKey::PageDown) => HostKey::PageDown,
            Key::Named(NamedKey::Home) => HostKey::Home,
            Key::Named(NamedKey::End) => HostKey::End,
            Key::Named(NamedKey::Enter) => HostKey::Enter,
            Key::Named(NamedKey::Escape) => HostKey::Escape,
            Key::Named(NamedKey::Tab) => HostKey::Tab,
            Key::Named(NamedKey::Backspace) => HostKey::Backspace,
            Key::Named(NamedKey::Delete) => HostKey::Delete,
            Key::Named(NamedKey::Space) => HostKey::Space,
            Key::Character(value) => HostKey::Character(value.to_string()),
            other => HostKey::Other(format!("{other:?}")),
        }
    }

    /// The viewer application: a [`ViewerContent`] document plus the window + shared
    /// present stack that drives it. Generic over the content so the static and
    /// scripted profiles share the shell.
    pub(crate) struct ViewerApp<C: ViewerContent> {
        config: StaticViewerConfig,
        // A live scripted document carries its DOM, JS runtime, and retained
        // Livery session. Keep the generic viewer payload off the Windows UI
        // thread's stack; static content remains source-compatible through Box's
        // transparent dereference.
        doc: Box<C>,
        window: Option<Arc<Window>>,
        host: Option<SurfaceHost>,
        width: u32,
        height: u32,
        /// Physical device pixels per logical CSS/layout pixel.
        scale_factor: f32,
        redraws: u32,
        /// Shift state, tracked from `ModifiersChanged`, so `Shift+Space` pages up.
        shift: bool,
        /// Last cursor position in physical px (winit's `MouseInput` carries none),
        /// so a click can hit-test the document for in-page link navigation.
        cursor: (f32, f32),
        navigation: NavigationState,
        /// Frame-loop clock origin, supplying the `now_ms` virtual clock that drives
        /// scripted content's timers (a no-op for static content).
        start: Instant,
    }

    impl<C: ViewerContent> ViewerApp<C> {
        pub(crate) fn new(config: StaticViewerConfig, doc: C) -> Self {
            let navigation = NavigationState::new(config.url.clone());
            Self {
                width: config.size.map_or(800, |size| size.0),
                height: config.size.map_or(600, |size| size.1),
                scale_factor: 1.0,
                config,
                doc: Box::new(doc),
                window: None,
                host: None,
                redraws: 0,
                shift: false,
                cursor: (0.0, 0.0),
                navigation,
                start: Instant::now(),
            }
        }

        pub(crate) fn outcome(&self) -> StaticViewerOutcome {
            StaticViewerOutcome {
                url: self.navigation.current().to_owned(),
                created_window: self.window.is_some(),
                redraws: self.redraws,
                size: if self.window.is_some() {
                    (self.width, self.height)
                } else {
                    (0, 0)
                },
            }
        }

        fn window_title(&self) -> String {
            let mut title = super::pelt_window_title(
                self.doc.title().as_deref(),
                Some(self.navigation.current()),
            );
            if let Some(posture) = self.doc.posture() {
                title.push_str(" — ");
                title.push_str(posture);
            }
            title
        }

        fn logical_size(&self) -> (u32, u32) {
            (
                super::logical_extent(self.width, self.scale_factor),
                super::logical_extent(self.height, self.scale_factor),
            )
        }

        /// Render the document at the current size + scroll and present it. The
        /// per-frame shape `genet-winit-host` documents: rasterize the scene into a
        /// texture, acquire the backbuffer, composite the texture onto it, present.
        fn render(&mut self, event_loop: &ActiveEventLoop) {
            // Advance script time-based work (timers + GC) against the frame clock
            // before laying out; `more` is true while the content has pending work
            // (scripted timers), so the shell keeps the frame loop running.
            let now_ms = self.start.elapsed().as_secs_f64() * 1000.0;
            let more = self.doc.pump(now_ms);
            let Some(host) = self.host.as_ref() else {
                return;
            };
            let (w, h) = self.logical_size();
            let scene = self.doc.frame(w, h);
            // White canvas: a document with no root/body background paints over white
            // (the page background), as a browser does.
            let (_tex, view) = host.rasterize_scaled(
                &scene,
                self.width.max(1),
                self.height.max(1),
                ColorLoad::Clear(wgpu::Color::WHITE),
                self.scale_factor,
            );
            let Some(frame) = host.acquire() else { return };
            let target = frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            host.renderer().compose_external_texture(
                &view,
                &target,
                host.format(),
                self.width,
                self.height,
                ExternalTexturePlacement::new([0.0, 0.0, self.width as f32, self.height as f32]),
            );
            // wgpu 30 moved presentation from SurfaceTexture to Queue.
            host.queue().present(frame);
            self.redraws += 1;
            if let Some(limit) = self.config.frames {
                if self.redraws >= limit {
                    event_loop.exit();
                    return;
                }
                // Static documents settle after their first paint. A bounded
                // headed smoke still needs the requested number of presented
                // frames, so it owns the next redraw until the limit is met.
                self.request_redraw();
            } else if more {
                self.request_redraw();
            }
        }

        fn request_redraw(&self) {
            if let Some(window) = self.window.as_ref() {
                window.request_redraw();
            }
        }

        fn resolve_address(&self, target: &str) -> String {
            #[cfg(any(feature = "livery", feature = "reader"))]
            {
                genet_documents::resolve_href(self.navigation.current(), target)
            }
            #[cfg(not(any(feature = "livery", feature = "reader")))]
            {
                target.to_owned()
            }
        }

        fn replace_at(&mut self, address: &str) -> bool {
            let (width, height) = self.logical_size();
            match self.doc.replace_session(address, width, height) {
                Ok(()) => {
                    self.request_redraw();
                    true
                },
                Err(error) => {
                    eprintln!("[pelt-viewer] navigation to {address} failed: {error}");
                    false
                },
            }
        }

        fn sync_window_title(&self) {
            if let Some(window) = self.window.as_ref() {
                window.set_title(&self.window_title());
            }
        }

        fn navigate(&mut self, target: &str) {
            let address = self.resolve_address(target);
            if self.replace_at(&address) {
                self.navigation.commit(address);
                self.sync_window_title();
            }
        }

        fn handle_navigation_command(&mut self, command: NavigationCommand) {
            match command {
                NavigationCommand::Address(target) => self.navigate(&target),
                NavigationCommand::Reload => {
                    let address = self.navigation.current().to_owned();
                    if self.replace_at(&address) {
                        self.sync_window_title();
                    }
                },
                NavigationCommand::Stop => {
                    // P1 sessions are synchronous. The command is deliberately
                    // accepted now so async engines can attach cancellation in P2.
                },
                NavigationCommand::Back => {
                    if let Some(address) = self.navigation.previous().map(str::to_owned)
                        && self.replace_at(&address)
                    {
                        let _ = self.navigation.back();
                        self.sync_window_title();
                    }
                },
                NavigationCommand::Forward => {
                    if let Some(address) = self.navigation.next().map(str::to_owned)
                        && self.replace_at(&address)
                    {
                        let _ = self.navigation.forward();
                        self.sync_window_title();
                    }
                },
            }
        }

        fn consume_effects(&mut self, effects: Vec<HostEffect>) {
            for effect in effects {
                match effect {
                    HostEffect::Redraw => self.request_redraw(),
                    HostEffect::Cursor(_) => {
                        // Cursor application remains platform chrome; P6 gives it
                        // a visible status surface. Keeping this semantic effect
                        // here prevents a document lane from seeing winit types.
                    },
                    HostEffect::Navigate { target } => self.navigate(&target),
                    HostEffect::Submit { target } => {
                        // A session has named an endpoint, but P1's Livery lane
                        // does not yet expose editable values or a serialized
                        // body. Do not silently turn this into a GET navigation.
                        eprintln!(
                            "[pelt-viewer] form submission to {target} needs a document-provided body"
                        );
                    },
                }
            }
        }

        /// The one semantic ingress for the desktop adapter. Native events are
        /// lowered before this point, and history commands never reach a
        /// concrete document session.
        fn dispatch_host_input(&mut self, input: HostInput) {
            match input {
                HostInput::Navigation(command) => self.handle_navigation_command(command),
                input => {
                    let effects = self.doc.handle_input(input);
                    self.consume_effects(effects);
                },
            }
        }
    }

    impl<C: ViewerContent> ApplicationHandler for ViewerApp<C> {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.window.is_some() {
                return;
            }
            let attributes =
                super::pelt_window_attributes(self.window_title(), self.width, self.height);
            let window = match event_loop.create_window(attributes) {
                Ok(window) => Arc::new(window),
                Err(err) => {
                    eprintln!("[pelt-viewer] could not create window: {err}");
                    event_loop.exit();
                    return;
                },
            };
            let size = window.inner_size();
            self.width = size.width.max(1);
            self.height = size.height.max(1);
            self.scale_factor = window.scale_factor() as f32;
            window.set_title(&self.window_title());
            let options = NetrenderOptions {
                tile_cache_size: Some(64),
                enable_vello: true,
                ..Default::default()
            };
            match SurfaceHost::boot(window.clone(), self.width, self.height, options) {
                Ok(host) => self.host = Some(host),
                Err(err) => {
                    eprintln!("[pelt-viewer] {err}");
                    event_loop.exit();
                    return;
                },
            }
            window.request_redraw();
            self.window = Some(window);
        }

        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            window_id: WindowId,
            event: WindowEvent,
        ) {
            if self.window.as_ref().map(|w| w.id()) != Some(window_id) {
                return;
            }
            match event {
                WindowEvent::CloseRequested => event_loop.exit(),
                WindowEvent::Resized(size) => {
                    self.width = size.width.max(1);
                    self.height = size.height.max(1);
                    if let Some(host) = self.host.as_mut() {
                        host.resize(self.width, self.height);
                    }
                    // The session rebuilds at the new size on the next frame
                    // (re-resolving %-height + viewport units).
                    self.request_redraw();
                },
                WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                    self.scale_factor = scale_factor as f32;
                    if let Some(window) = self.window.as_ref() {
                        let size = window.inner_size();
                        self.width = size.width.max(1);
                        self.height = size.height.max(1);
                    }
                    if let Some(host) = self.host.as_mut() {
                        host.resize(self.width, self.height);
                    }
                    self.request_redraw();
                },
                WindowEvent::MouseWheel { delta, .. } => {
                    // The shared wheel default action (scope doc rule 5): map the wheel
                    // to a device-px delta and scroll at the cursor — a nested
                    // `overflow: scroll/auto` container under the pointer takes it first,
                    // else the document viewport. The viewer fills the window, so the
                    // cursor is already in document space. Redraw only when something
                    // moved (not at an edge).
                    let (dx, dy) = wheel_delta_from_winit(delta);
                    let (dx, dy) = (dx / self.scale_factor, dy / self.scale_factor);
                    self.dispatch_host_input(HostInput::Wheel {
                        x: self.cursor.0,
                        y: self.cursor.1,
                        dx,
                        dy,
                    });
                },
                WindowEvent::ModifiersChanged(mods) => {
                    self.shift = mods.state().shift_key();
                },
                WindowEvent::CursorMoved { position, .. } => {
                    self.cursor = (
                        super::logical_position(position.x as f32, self.scale_factor),
                        super::logical_position(position.y as f32, self.scale_factor),
                    );
                    self.dispatch_host_input(HostInput::PointerMoved {
                        x: self.cursor.0,
                        y: self.cursor.1,
                    });
                },
                WindowEvent::MouseInput { state, button, .. } => {
                    // A left click on an in-page link (`<a href="#id">`) scrolls its
                    // target into view (anchor-fragment navigation, scope doc rule 5).
                    let button = match button {
                        MouseButton::Left => PointerButton::Primary,
                        MouseButton::Middle => PointerButton::Auxiliary,
                        MouseButton::Right => PointerButton::Secondary,
                        MouseButton::Back => PointerButton::Other(8),
                        MouseButton::Forward => PointerButton::Other(9),
                        MouseButton::Other(value) => PointerButton::Other(value),
                    };
                    let state = match state {
                        ElementState::Pressed => ButtonState::Pressed,
                        ElementState::Released => ButtonState::Released,
                    };
                    let (x, y) = self.cursor;
                    self.dispatch_host_input(HostInput::PointerButton {
                        button,
                        state,
                        x,
                        y,
                    });
                },
                WindowEvent::KeyboardInput { event, .. } => {
                    // The keyboard scroll defaults (scope doc rule 5): map the key to
                    // a `ScrollKey` and scroll the document viewport. (No editable
                    // gate yet — pelt has no focusable fields in V1/V2; add the "focus
                    // not in an editable" check when it gains them.)
                    if event.state == ElementState::Pressed {
                        self.dispatch_host_input(HostInput::Key {
                            key: host_key_from_winit(&event.logical_key),
                            modifiers: InputModifiers {
                                shift: self.shift,
                                ..InputModifiers::default()
                            },
                        });
                    }
                },
                WindowEvent::RedrawRequested => self.render(event_loop),
                _ => {},
            }
        }
    }
}

#[cfg(all(test, feature = "livery"))]
mod browser_loop_tests {
    use std::any::Any;
    use std::sync::{Arc, Mutex};

    use genet_host_api::{ButtonState, HostInput, NavigationCommand, PointerButton};
    use inker::{
        DocumentSession, SessionClick, SessionEngine, SessionError, SessionLink, SessionScrollKey,
        SessionSpawnRequest,
    };
    use netrender::Scene;

    use super::windowed::{ViewerApp, ViewerContent};
    use super::{SessionViewerContent, StaticViewerConfig, WindowingMode};
    use genet_host_api::EngineProfile;

    #[derive(Clone)]
    struct RecordingEngine(Arc<Mutex<Vec<String>>>);

    impl SessionEngine<Scene> for RecordingEngine {
        fn engine_id(&self) -> &str {
            "test.browser-loop"
        }

        fn spawn(
            &self,
            request: &SessionSpawnRequest,
        ) -> Result<Box<dyn DocumentSession<Scene>>, SessionError> {
            self.0.lock().unwrap().push(request.address.clone());
            Ok(Box::new(TestSession))
        }
    }

    struct TestSession;

    impl DocumentSession<Scene> for TestSession {
        fn frame(&mut self, width: u32, height: u32) -> Scene {
            Scene::new(width, height)
        }

        fn scroll_by(&mut self, _dx: f32, _dy: f32) -> bool {
            false
        }

        fn scroll_for_key(&mut self, _key: SessionScrollKey) -> bool {
            false
        }

        fn click_at(&mut self, _x: f32, _y: f32) -> SessionClick {
            SessionClick::Navigate("next.html".to_string())
        }

        fn links(&self) -> Vec<SessionLink> {
            Vec::new()
        }

        fn as_any_ref(&self) -> &dyn Any {
            self
        }

        fn as_any(&mut self) -> &mut dyn Any {
            self
        }
    }

    #[test]
    fn host_consumes_relative_navigation_and_replaces_history_sessions() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let engine: Box<dyn SessionEngine<Scene>> = Box::new(RecordingEngine(requests.clone()));
        let initial = "https://example.test/docs/start.html";
        let session = engine
            .spawn(&SessionSpawnRequest::new(initial).with_viewport(800, 600))
            .expect("initial session");
        let content = SessionViewerContent {
            session,
            posture: None,
            replacement_engine: Some(engine),
            primary_pointer_down: false,
        };
        let config = StaticViewerConfig::new(EngineProfile::Livery, WindowingMode::Headed, initial);
        let mut app = ViewerApp::new(config, content);

        app.dispatch_host_input(HostInput::PointerButton {
            button: PointerButton::Primary,
            state: ButtonState::Pressed,
            x: 10.0,
            y: 10.0,
        });
        // The press navigates and replaces the session. The paired release is
        // not a click on the replacement page.
        app.dispatch_host_input(HostInput::PointerButton {
            button: PointerButton::Primary,
            state: ButtonState::Released,
            x: 10.0,
            y: 10.0,
        });
        assert_eq!(
            app.navigation.current(),
            "https://example.test/docs/next.html"
        );
        app.dispatch_host_input(HostInput::Navigation(NavigationCommand::Address(
            "https://next.test/next.html".to_string(),
        )));
        assert_eq!(app.navigation.current(), "https://next.test/next.html");
        assert_eq!(app.window_title(), "Pelt — next.test");

        app.dispatch_host_input(HostInput::Navigation(NavigationCommand::Back));
        assert_eq!(
            app.navigation.current(),
            "https://example.test/docs/next.html"
        );
        app.dispatch_host_input(HostInput::Navigation(NavigationCommand::Back));
        assert_eq!(app.navigation.current(), initial);
        app.dispatch_host_input(HostInput::Navigation(NavigationCommand::Forward));
        assert_eq!(
            app.navigation.current(),
            "https://example.test/docs/next.html"
        );
        app.dispatch_host_input(HostInput::Navigation(NavigationCommand::Forward));
        assert_eq!(app.navigation.current(), "https://next.test/next.html");
        app.dispatch_host_input(HostInput::Navigation(NavigationCommand::Reload));

        assert_eq!(
            requests.lock().unwrap().as_slice(),
            [
                "https://example.test/docs/start.html",
                "https://example.test/docs/next.html",
                "https://next.test/next.html",
                "https://example.test/docs/next.html",
                "https://example.test/docs/start.html",
                "https://example.test/docs/next.html",
                "https://next.test/next.html",
                "https://next.test/next.html",
            ]
        );
    }
}
