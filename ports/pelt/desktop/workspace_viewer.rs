//! Headed recursive Pelt workspace over TileTree and Frisket.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use genet_host_api::tile::{
    ContentSource, DocumentRef, DropTarget, Edge, SplitAxis, Tile, TileBranch, TileEvent, TileId,
    TileTree,
};
use genet_winit_host::{SurfaceHost, wheel_delta_from_winit};
use inker::{
    EngineProfileBinding, SessionButtonState, SessionCursor, SessionIme, SessionInput, SessionKey,
    SessionModifiers, SessionNavigationCommand, SessionPointerButton, SessionRegistry,
    SessionScrollKey, SurfaceEngineRegistry, SurfaceFrame,
};
#[cfg(target_os = "windows")]
use inker::{FrameHandleOwnership, NativeTextureHandle};
use netrender::external_texture::ExternalTexturePlacement;
use netrender::{ColorLoad, NetrenderOptions, Scene};
use pelt_core::{
    PeltController, PeltHostEffect, PeltRegistries, PeltRouteState, PeltTileRequest, PeltWorkspace,
    WorkspaceRect,
};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use crate::frisket_surface::{FrisketHit, FrisketSurface};
use crate::{WindowingMode, static_viewer};

const RECEIPT_STEPS: u8 = 8;

/// Configuration for the recursive Livery workspace.
pub struct WorkspaceViewerConfig {
    pub urls: Vec<String>,
    pub windowing: WindowingMode,
    pub size: Option<(u32, u32)>,
    pub frames: Option<u32>,
    /// Drive the checked-in P3 interaction receipt through the same semantic
    /// pointer and navigation paths as the window.
    pub interaction_receipt: bool,
    /// Drive the mixed P4 routing receipt after the first shared frame.
    pub capability_receipt: bool,
    /// One-based tile number to explicit engine id.
    pub route_overrides: HashMap<u64, String>,
}

impl WorkspaceViewerConfig {
    pub fn new(urls: Vec<String>, windowing: WindowingMode) -> Self {
        Self {
            urls,
            windowing,
            size: None,
            frames: None,
            interaction_receipt: false,
            capability_receipt: false,
            route_overrides: HashMap::new(),
        }
    }

    pub fn with_size(mut self, width: u32, height: u32) -> Self {
        self.size = Some((width.max(1), height.max(1)));
        self
    }

    pub fn with_frame_limit(mut self, frames: u32) -> Self {
        self.frames = Some(frames.max(1));
        self
    }

    pub fn with_interaction_receipt(mut self) -> Self {
        self.interaction_receipt = true;
        self.frames = Some(self.frames.unwrap_or(0).max(u32::from(RECEIPT_STEPS) + 1));
        self
    }

    pub fn with_route_override(mut self, tile: u64, engine_id: impl Into<String>) -> Self {
        self.route_overrides.insert(tile, engine_id.into());
        self
    }

    pub fn with_capability_receipt(mut self) -> Self {
        self.capability_receipt = true;
        self.frames = Some(self.frames.unwrap_or(0).max(2));
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceViewerOutcome {
    pub first_url: String,
    pub created_window: bool,
    pub redraws: u32,
    pub size: (u32, u32),
    pub tile_count: usize,
    pub interaction_receipt: bool,
    pub capability_receipt: bool,
    pub routes: Vec<String>,
}

pub fn run_livery_workspace_viewer(
    config: WorkspaceViewerConfig,
) -> Result<WorkspaceViewerOutcome, String> {
    let tree = tree_from_urls(&config.urls);
    let tile_count = tree.tiles().len();
    if matches!(config.windowing, WindowingMode::Headless) {
        return Ok(WorkspaceViewerOutcome {
            first_url: config.urls.first().cloned().unwrap_or_default(),
            created_window: false,
            redraws: 0,
            size: (0, 0),
            tile_count,
            interaction_receipt: false,
            capability_receipt: false,
            routes: Vec::new(),
        });
    }

    let initial_size = config.size.unwrap_or((1100, 750));
    let registries = workspace_registries();
    let overrides = config.route_overrides.clone();
    let workspace = PeltWorkspace::try_routed(
        tree,
        registries,
        |tile| {
            let ContentSource::Document(DocumentRef(address)) = &tile.content else {
                return Err("standalone Pelt only routes document tile sources".to_owned());
            };
            let mut request = PeltTileRequest::new(address, initial_size);
            if let Some(engine) = overrides.get(&tile.id.0) {
                request = request.with_engine_override(engine);
            }
            Ok(request)
        },
        || Box::new(WorkspaceClock(Instant::now())),
    )?;
    let frisket = FrisketSurface::new(workspace.tree());
    let event_loop =
        EventLoop::new().map_err(|error| format!("could not create event loop: {error}"))?;
    let mut app = WorkspaceApp::new(config, workspace, frisket);
    event_loop
        .run_app(&mut app)
        .map_err(|error| format!("workspace event loop failed: {error}"))?;
    if let Some(error) = app.receipt_error.take() {
        return Err(error);
    }
    Ok(app.outcome())
}

struct WorkspaceClock(Instant);

impl pelt_core::PeltClock for WorkspaceClock {
    fn now_ms(&self) -> f64 {
        self.0.elapsed().as_secs_f64() * 1000.0
    }
}

fn workspace_registries() -> PeltRegistries<Scene> {
    let mut sessions: SessionRegistry<Scene> = SessionRegistry::new();
    let fetcher = genet_documents::LocalFetcher::with_resource_policy(
        genet_documents::ResourceFetchPolicy::default(),
    );
    sessions.register(Box::new(genet_documents::LiverySessionEngine::new(
        fetcher.clone(),
    )));
    #[cfg(feature = "reader")]
    sessions.register(Box::new(genet_documents::ReaderSessionEngine::default()));
    #[cfg(feature = "scripted")]
    sessions.register(Box::new(genet_documents::ScriptedSessionEngine::<
        script_engine_boa::BoaEngine,
        _,
    >::new(
        inker::routing::ENGINE_GENET_SCRIPTED,
        fetcher.clone(),
    )));
    #[cfg(feature = "scripted-nova")]
    sessions.register(Box::new(genet_documents::ScriptedSessionEngine::<
        script_engine_nova::NovaEngine,
        _,
    >::new(
        inker::routing::ENGINE_GENET_SCRIPTED_NOVA,
        fetcher.clone(),
    )));
    #[cfg(feature = "smolweb")]
    for engine_id in [
        inker::routing::ENGINE_NEMATIC_GEMTEXT,
        inker::routing::ENGINE_NEMATIC_GOPHER,
        inker::routing::ENGINE_NEMATIC_FEED,
        inker::routing::ENGINE_NEMATIC_NEX,
        inker::routing::ENGINE_NEMATIC_FINGER,
    ] {
        sessions.register(Box::new(genet_documents::SmolwebSessionEngine::new(
            engine_id,
            fetcher.clone(),
            genet_documents::SmolwebTheme::System,
        )));
    }

    let mut policy = inker::routing::EngineRoutePolicy::default();
    for rule in &mut policy.rules {
        if rule.engine_id == inker::routing::ENGINE_GENET_WEB {
            rule.engine_id = inker::routing::ENGINE_GENET_LIVERY.to_owned();
        }
    }
    policy.fallback.engine_id = inker::routing::ENGINE_GENET_LIVERY.to_owned();
    PeltRegistries::new(
        sessions,
        SurfaceEngineRegistry::new(),
        policy,
        "pelt.workspace",
        inker::routing::ENGINE_GENET_LIVERY,
        EngineProfileBinding {
            user_data_dir: "pelt-surface-profile".to_owned(),
        },
    )
}

fn tree_from_urls(urls: &[String]) -> TileTree {
    let urls = if urls.is_empty() {
        vec!["about:blank".to_owned()]
    } else {
        urls.to_vec()
    };
    let make_tile = |index: usize| Tile {
        id: TileId(index as u64 + 1),
        title: tile_title(&urls[index]),
        content: ContentSource::Document(DocumentRef(urls[index].clone())),
        accent: None,
    };
    match urls.len() {
        1 => TileTree::single(make_tile(0)),
        2 => TileTree::split(
            SplitAxis::Row,
            vec![
                TileBranch::new(0.5, TileTree::single(make_tile(0))),
                TileBranch::new(0.5, TileTree::single(make_tile(1))),
            ],
        ),
        3 => TileTree::split(
            SplitAxis::Row,
            vec![
                TileBranch::new(0.5, TileTree::stack(vec![make_tile(0), make_tile(1)], 0)),
                TileBranch::new(0.5, TileTree::single(make_tile(2))),
            ],
        ),
        _ => TileTree::split(
            SplitAxis::Row,
            vec![
                TileBranch::new(0.5, TileTree::stack(vec![make_tile(0), make_tile(1)], 0)),
                TileBranch::new(
                    0.5,
                    TileTree::split(
                        SplitAxis::Column,
                        vec![
                            TileBranch::new(0.5, TileTree::single(make_tile(2))),
                            TileBranch::new(
                                0.5,
                                TileTree::stack((3..urls.len()).map(&make_tile).collect(), 0),
                            ),
                        ],
                    ),
                ),
            ],
        ),
    }
}

fn tile_title(address: &str) -> String {
    address
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(address)
        .to_owned()
}

struct DividerDrag {
    target: cambium::DividerTarget,
    horizontal: bool,
    extent: f32,
    start: f32,
    init_first: f32,
    pair_total: f32,
}

struct TabDrag {
    tile: TileId,
    start: (f32, f32),
    moved: bool,
}

enum PointerGesture {
    Content,
    Divider(DividerDrag),
    Tab(TabDrag),
}

struct WorkspaceApp {
    config: WorkspaceViewerConfig,
    workspace: PeltWorkspace<Scene>,
    frisket: FrisketSurface,
    window: Option<Arc<Window>>,
    host: Option<SurfaceHost>,
    width: u32,
    height: u32,
    scale_factor: f32,
    redraws: u32,
    modifiers: SessionModifiers,
    cursor: (f32, f32),
    gesture: Option<PointerGesture>,
    receipt_step: u8,
    receipt_complete: bool,
    receipt_error: Option<String>,
}

impl WorkspaceApp {
    fn new(
        config: WorkspaceViewerConfig,
        workspace: PeltWorkspace<Scene>,
        frisket: FrisketSurface,
    ) -> Self {
        let (width, height) = config.size.unwrap_or((1100, 750));
        Self {
            config,
            workspace,
            frisket,
            window: None,
            host: None,
            width,
            height,
            scale_factor: 1.0,
            redraws: 0,
            modifiers: SessionModifiers::default(),
            cursor: (0.0, 0.0),
            gesture: None,
            receipt_step: 0,
            receipt_complete: false,
            receipt_error: None,
        }
    }

    fn outcome(&self) -> WorkspaceViewerOutcome {
        let mut routes = self
            .workspace
            .routes()
            .map(|route| {
                let state = match &route.state {
                    PeltRouteState::Document => "document".to_owned(),
                    PeltRouteState::Surface => {
                        format!("surface:{:?}", route.decision.surface_contract.mode)
                    },
                    PeltRouteState::Fallback {
                        active_engine,
                        reason,
                    } => {
                        format!("fallback:{active_engine}:{reason}")
                    },
                };
                format!("{}={}:{state}", route.tile.0, route.selected_engine())
            })
            .collect::<Vec<_>>();
        routes.sort();
        WorkspaceViewerOutcome {
            first_url: self.config.urls.first().cloned().unwrap_or_default(),
            created_window: self.window.is_some(),
            redraws: self.redraws,
            size: if self.window.is_some() {
                (self.width, self.height)
            } else {
                (0, 0)
            },
            tile_count: self.workspace.tree().tiles().len(),
            interaction_receipt: self.config.interaction_receipt && self.receipt_complete,
            capability_receipt: self.config.capability_receipt && self.receipt_complete,
            routes,
        }
    }

    fn window_title(&self) -> String {
        let controller = self
            .workspace
            .focused_tile()
            .and_then(|tile| self.workspace.controller(tile));
        static_viewer::pelt_window_title(
            controller.and_then(PeltController::title).as_deref(),
            controller.map(PeltController::address),
        )
    }

    fn logical_size(&self) -> (u32, u32) {
        (
            static_viewer::logical_extent(self.width, self.scale_factor),
            static_viewer::logical_extent(self.height, self.scale_factor),
        )
    }

    fn render(&mut self, event_loop: &ActiveEventLoop) {
        if self.config.capability_receipt && self.redraws > 0 && !self.receipt_complete {
            if let Err(error) = self.validate_capability_receipt() {
                self.receipt_error = Some(error);
                event_loop.exit();
                return;
            }
            self.receipt_complete = true;
        }
        if self.config.interaction_receipt && self.redraws > 0 && !self.receipt_complete {
            if let Err(error) = self.drive_receipt_step() {
                self.receipt_error = Some(error);
                event_loop.exit();
                return;
            }
        }

        let (logical_width, logical_height) = self.logical_size();
        let pane_frame = match self.frisket.frame(logical_width, logical_height) {
            Ok(frame) => frame,
            Err(error) => {
                self.receipt_error = Some(error);
                event_loop.exit();
                return;
            },
        };
        self.workspace
            .set_content_rects(pane_frame.content_rects.iter().copied());
        self.workspace.set_surface_scale_factor(self.scale_factor);
        let more = self.workspace.pump();
        let workspace_frame = self.workspace.frame();
        for surface in workspace_frame.surfaces {
            match surface.frame {
                Ok(None) => {},
                Ok(Some(frame)) => {
                    discard_unimported_surface_frame(frame);
                    self.receipt_error = Some(format!(
                        "tile {} produced a native surface frame, but this platform has no shared-handle importer",
                        surface.tile.0
                    ));
                    event_loop.exit();
                    return;
                },
                Err(error) => {
                    self.receipt_error =
                        Some(format!("tile {} surface failed: {error}", surface.tile.0));
                    event_loop.exit();
                    return;
                },
            }
        }
        let Some(host) = self.host.as_ref() else {
            return;
        };
        let (_frame_texture, frame_view) = host.rasterize_scaled(
            &pane_frame.scene,
            self.width,
            self.height,
            ColorLoad::Clear(wgpu::Color {
                r: 0.10,
                g: 0.10,
                b: 0.12,
                a: 1.0,
            }),
            self.scale_factor,
        );
        let tile_layers = workspace_frame
            .tiles
            .into_iter()
            .map(|layer| {
                let (width, height) = (
                    physical_extent(layer.rect.width, self.scale_factor),
                    physical_extent(layer.rect.height, self.scale_factor),
                );
                let (texture, view) = host.rasterize_scaled(
                    &layer.frame,
                    width,
                    height,
                    ColorLoad::Clear(wgpu::Color::WHITE),
                    self.scale_factor,
                );
                (texture, view, layer.rect)
            })
            .collect::<Vec<_>>();
        let Some(swap) = host.acquire() else {
            return;
        };
        let target = swap
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        host.renderer().compose_external_texture(
            &frame_view,
            &target,
            host.format(),
            self.width,
            self.height,
            ExternalTexturePlacement::new([0.0, 0.0, self.width as f32, self.height as f32]),
        );
        for (_texture, view, rect) in &tile_layers {
            host.renderer().compose_external_texture(
                view,
                &target,
                host.format(),
                self.width,
                self.height,
                placement(*rect, self.scale_factor),
            );
        }
        host.queue().present(swap);
        self.redraws += 1;

        if self.receipt_complete
            || self
                .config
                .frames
                .is_some_and(|limit| self.redraws >= limit)
        {
            event_loop.exit();
        } else if self.config.interaction_receipt || more || self.config.frames.is_some() {
            self.request_redraw();
        }
    }

    fn validate_capability_receipt(&self) -> Result<(), String> {
        let expected = [
            (1, inker::routing::ENGINE_NEMATIC_GEMTEXT),
            (2, inker::routing::ENGINE_GENET_LIVERY),
            (3, inker::routing::ENGINE_GENET_SCRIPTED),
            (4, inker::routing::ENGINE_SCRYING_WEB),
        ];
        for (tile, engine) in expected {
            let route = self
                .workspace
                .route(TileId(tile))
                .ok_or_else(|| format!("P4 receipt is missing tile {tile}"))?;
            if route.selected_engine() != engine {
                return Err(format!(
                    "P4 tile {tile} selected {} instead of {engine}",
                    route.selected_engine()
                ));
            }
        }
        if !matches!(
            self.workspace.route(TileId(4)).map(|route| &route.state),
            Some(PeltRouteState::Fallback { active_engine, .. })
                if active_engine == inker::routing::ENGINE_GENET_LIVERY
        ) {
            return Err("P4 external surface tile did not expose its Livery fallback".to_owned());
        }
        let first = self
            .workspace
            .controller(TileId(1))
            .ok_or("P4 smolweb tile is not live")?;
        let second = self
            .workspace
            .controller(TileId(2))
            .ok_or("P4 static tile is not live")?;
        let third = self
            .workspace
            .controller(TileId(3))
            .ok_or("P4 scripted tile is not live")?;
        if !first.shares_registries_with(second) || !second.shares_registries_with(third) {
            return Err("P4 document tiles did not share their long-lived registries".to_owned());
        }
        let native = first
            .inspect()
            .ok_or("P4 smolweb tile did not expose a structural report")?;
        if !native
            .links
            .iter()
            .any(|link| link.ends_with("static.html"))
        {
            return Err("P4 smolweb tile did not parse its gemtext link".to_owned());
        }
        let static_report = second
            .inspect()
            .ok_or("P4 static tile did not expose a structural report")?;
        if static_report.title.as_deref() != Some("Static Livery")
            || static_report.headings != ["Static Livery"]
        {
            return Err("P4 static tile did not retain its Livery semantics".to_owned());
        }
        let scripted = third
            .inspect()
            .ok_or("P4 scripted tile did not expose a structural report")?;
        if scripted.title.as_deref() != Some("Scripted Livery")
            || !scripted
                .outline
                .iter()
                .any(|entry| entry.name == "Boa mutated this retained DOM")
        {
            return Err("P4 scripted tile did not expose its post-Boa DOM".to_owned());
        }
        Ok(())
    }

    fn request_redraw(&self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn apply_effect(&mut self, effect: PeltHostEffect) {
        let navigated = effect.navigated;
        if let Some(error) = effect.error {
            eprintln!("[pelt-workspace] {error}");
        }
        if let Some(window) = &self.window {
            if let Some(cursor) = effect.cursor {
                window.set_cursor(match cursor {
                    SessionCursor::Default => winit::window::CursorIcon::Default,
                    SessionCursor::Pointer => winit::window::CursorIcon::Pointer,
                    SessionCursor::Text => winit::window::CursorIcon::Text,
                });
            }
            window.set_ime_allowed(effect.editable);
            if navigated {
                window.set_title(&self.window_title());
            }
        }
        if navigated {
            self.frisket.set_tree(self.workspace.tree());
        }
        if effect.redraw {
            self.request_redraw();
        }
    }

    fn apply_tile_event(&mut self, event: TileEvent) -> bool {
        if self.workspace.apply(&event) {
            self.frisket.set_tree(self.workspace.tree());
            if let Some(window) = &self.window {
                window.set_title(&self.window_title());
            }
            true
        } else {
            false
        }
    }

    fn pointer_move(&mut self, x: f32, y: f32) -> bool {
        self.cursor = (x, y);
        let mut redraw = false;
        if let Some(PointerGesture::Divider(drag)) = &self.gesture {
            let position = if drag.horizontal { x } else { y };
            let delta = (position - drag.start) / drag.extent.max(1.0);
            let minimum = drag.pair_total.min(0.05);
            let first =
                (drag.init_first + delta).clamp(minimum, (drag.pair_total - minimum).max(minimum));
            if let Some(mut fractions) = self.workspace.tree().fractions_at(&drag.target.path) {
                fractions[drag.target.index] = first;
                fractions[drag.target.index + 1] = drag.pair_total - first;
                let event = TileEvent::DividerMoved {
                    split: drag.target.path.clone(),
                    fractions,
                };
                redraw |= self.apply_tile_event(event);
            }
            return redraw;
        }
        if let Some(PointerGesture::Tab(drag)) = &mut self.gesture {
            if (x - drag.start.0).abs() + (y - drag.start.1).abs() > 6.0 {
                drag.moved = true;
            }
            return drag.moved;
        }

        let effect = self.workspace.input(SessionInput::PointerMoved {
            x,
            y,
            modifiers: self.modifiers,
        });
        redraw |= effect.redraw;
        self.apply_effect(effect);
        redraw
    }

    fn pointer_down(&mut self) -> bool {
        let (x, y) = self.cursor;
        match self.frisket.hit(x, y) {
            Some(FrisketHit::Close(tile)) => self.apply_tile_event(TileEvent::Closed(tile)),
            Some(FrisketHit::Divider { target, split_rect }) => {
                let Some(fractions) = self.workspace.tree().fractions_at(&target.path) else {
                    return false;
                };
                if target.index + 1 >= fractions.len() {
                    return false;
                }
                let index = target.index;
                let horizontal =
                    self.workspace.tree().axis_at(&target.path) == Some(SplitAxis::Row);
                self.gesture = Some(PointerGesture::Divider(DividerDrag {
                    target,
                    horizontal,
                    extent: if horizontal {
                        split_rect.width
                    } else {
                        split_rect.height
                    },
                    start: if horizontal { x } else { y },
                    init_first: fractions[index],
                    pair_total: fractions[index] + fractions[index + 1],
                }));
                true
            },
            Some(FrisketHit::Tab(tile)) => {
                self.gesture = Some(PointerGesture::Tab(TabDrag {
                    tile,
                    start: self.cursor,
                    moved: false,
                }));
                true
            },
            Some(FrisketHit::Content(_)) => {
                self.gesture = Some(PointerGesture::Content);
                let effect = self.workspace.input(SessionInput::PointerButton {
                    x,
                    y,
                    button: SessionPointerButton::Primary,
                    state: SessionButtonState::Pressed,
                    modifiers: self.modifiers,
                });
                let redraw = effect.redraw;
                self.apply_effect(effect);
                redraw
            },
            Some(FrisketHit::Chrome) | None => false,
        }
    }

    fn pointer_up(&mut self) -> bool {
        let gesture = self.gesture.take();
        match gesture {
            Some(PointerGesture::Divider(_)) => true,
            Some(PointerGesture::Tab(drag)) if drag.moved => {
                let to = self.resolve_drop(drag.tile);
                to.is_some_and(|to| {
                    self.apply_tile_event(TileEvent::Dragged {
                        tile: drag.tile,
                        to,
                    })
                })
            },
            Some(PointerGesture::Tab(drag)) => {
                self.apply_tile_event(TileEvent::Activated(drag.tile))
            },
            Some(PointerGesture::Content) => {
                let (x, y) = self.cursor;
                let effect = self.workspace.input(SessionInput::PointerButton {
                    x,
                    y,
                    button: SessionPointerButton::Primary,
                    state: SessionButtonState::Released,
                    modifiers: self.modifiers,
                });
                let redraw = effect.redraw;
                self.apply_effect(effect);
                redraw
            },
            None => false,
        }
    }

    fn resolve_drop(&self, dragged: TileId) -> Option<DropTarget> {
        let (x, y) = self.cursor;
        if let Some((stack, index)) = self.frisket.tabbar_drop(x, y) {
            return Some(DropTarget::Stack { stack, index });
        }
        let target = self.workspace.tree().tiles().into_iter().find_map(|tile| {
            self.workspace
                .content_rect(tile.id)
                .filter(|rect| rect.contains(x, y))
                .map(|rect| (tile.id, rect))
        });
        match target {
            Some((tile, _)) if tile == dragged => None,
            Some((tile, rect)) => Some(DropTarget::Edge {
                tile,
                edge: nearest_edge((x, y), rect),
            }),
            None => Some(DropTarget::Outside),
        }
    }

    fn drive_receipt_step(&mut self) -> Result<(), String> {
        match self.receipt_step {
            0 => {
                require_tile(self.workspace.tree(), 4)?;
                let tile1 = TileId(1);
                let first = self.workspace.command_for(
                    tile1,
                    SessionNavigationCommand::Address("next.html".to_owned()),
                );
                if !first.navigated {
                    return Err(format!(
                        "P3 receipt could not navigate tile 1: {:?}",
                        first.error
                    ));
                }
                self.apply_effect(first);
                if let Some(rect) = self.workspace.content_rect(tile1) {
                    let _ = self.workspace.scroll_at(
                        rect.x + rect.width / 2.0,
                        rect.y + rect.height / 2.0,
                        0.0,
                        120.0,
                    );
                }
            },
            1 => {
                self.click_tab(TileId(2))?;
            },
            2 => {
                self.click_tab(TileId(1))?;
                let controller = self
                    .workspace
                    .controller(TileId(1))
                    .ok_or("tile 1 closed")?;
                if !controller.can_go_back() || !controller.address().ends_with("next.html") {
                    return Err("tile 1 lost history across tab activation".to_owned());
                }
            },
            3 => {
                self.click_tab(TileId(2))?;
            },
            4 => {
                let target = cambium::DividerTarget {
                    path: genet_host_api::tile::TilePath(Vec::new()),
                    index: 0,
                };
                let rect = self
                    .frisket
                    .divider_rect(&target)
                    .ok_or("root divider has no Frisket geometry")?;
                let start = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
                self.pointer_move(start.0, start.1);
                self.pointer_down();
                self.pointer_move(start.0 + 60.0, start.1);
                self.pointer_up();
                let fractions = self
                    .workspace
                    .tree()
                    .fractions_at(&target.path)
                    .ok_or("root divider disappeared after resize")?;
                if (fractions[0] - 0.5).abs() < 0.01 {
                    return Err("root divider drag did not change its fractions".to_owned());
                }
            },
            5 => {
                let tab = self
                    .frisket
                    .tab_rect(TileId(2))
                    .ok_or("tile 2 tab has no Frisket geometry")?;
                let target = self
                    .workspace
                    .content_rect(TileId(4))
                    .ok_or("tile 4 content has no Frisket geometry")?;
                self.pointer_move(tab.x + 8.0, tab.y + tab.height / 2.0);
                self.pointer_down();
                self.pointer_move(
                    target.x + target.width * 0.9,
                    target.y + target.height / 2.0,
                );
                self.pointer_up();
            },
            6 => {
                let fourth = self.workspace.command_for(
                    TileId(4),
                    SessionNavigationCommand::Address("next.html".to_owned()),
                );
                if !fourth.navigated {
                    return Err(format!(
                        "P3 receipt could not navigate tile 4: {:?}",
                        fourth.error
                    ));
                }
                self.apply_effect(fourth);
            },
            7 => {
                let close = self
                    .frisket
                    .close_rect(TileId(3))
                    .ok_or("tile 3 close control has no Frisket geometry")?;
                self.pointer_move(close.x + close.width / 2.0, close.y + close.height / 2.0);
                self.pointer_down();
                if self.workspace.controller(TileId(3)).is_some() {
                    return Err("tile 3 remained live after its close control".to_owned());
                }
                if !self
                    .workspace
                    .controller(TileId(1))
                    .is_some_and(PeltController::can_go_back)
                    || !self
                        .workspace
                        .controller(TileId(4))
                        .is_some_and(PeltController::can_go_back)
                {
                    return Err("independent tile histories did not survive P3 gestures".to_owned());
                }
                self.receipt_complete = true;
            },
            _ => self.receipt_complete = true,
        }
        self.receipt_step = self.receipt_step.saturating_add(1);
        Ok(())
    }

    fn click_tab(&mut self, tile: TileId) -> Result<(), String> {
        let rect = self
            .frisket
            .tab_rect(tile)
            .ok_or_else(|| format!("tile {} tab has no Frisket geometry", tile.0))?;
        self.pointer_move(rect.x + 8.0, rect.y + rect.height / 2.0);
        self.pointer_down();
        self.pointer_up();
        Ok(())
    }
}

fn discard_unimported_surface_frame(frame: SurfaceFrame) {
    #[cfg(target_os = "windows")]
    if let NativeTextureHandle::D3d12Shared {
        handle,
        ownership: FrameHandleOwnership::Transferred,
    } = frame.texture
    {
        // SAFETY: Inker transferred this one-shot Win32 handle to the host,
        // and this rejection path consumes the frame without importing it.
        unsafe {
            let _ = windows_sys::Win32::Foundation::CloseHandle(handle as _);
        }
    }

    #[cfg(not(target_os = "windows"))]
    let _ = frame;
}

impl ApplicationHandler for WorkspaceApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes =
            static_viewer::pelt_window_attributes(self.window_title(), self.width, self.height);
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                self.receipt_error =
                    Some(format!("could not create Pelt workspace window: {error}"));
                event_loop.exit();
                return;
            },
        };
        let size = window.inner_size();
        self.width = size.width.max(1);
        self.height = size.height.max(1);
        self.scale_factor = window.scale_factor() as f32;
        let options = NetrenderOptions {
            tile_cache_size: Some(64),
            enable_vello: true,
            ..Default::default()
        };
        match SurfaceHost::boot(window.clone(), self.width, self.height, options) {
            Ok(host) => self.host = Some(host),
            Err(error) => {
                self.receipt_error = Some(error);
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
        if self.window.as_ref().map(|window| window.id()) != Some(window_id) {
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
                self.request_redraw();
            },
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.scale_factor = scale_factor as f32;
                if let Some(window) = &self.window {
                    let size = window.inner_size();
                    self.width = size.width.max(1);
                    self.height = size.height.max(1);
                }
                if let Some(host) = self.host.as_mut() {
                    host.resize(self.width, self.height);
                }
                self.request_redraw();
            },
            WindowEvent::ModifiersChanged(modifiers) => {
                let state = modifiers.state();
                self.modifiers = SessionModifiers {
                    shift: state.shift_key(),
                    control: state.control_key(),
                    alt: state.alt_key(),
                    meta: state.super_key(),
                };
            },
            WindowEvent::CursorMoved { position, .. } => {
                let redraw = self.pointer_move(
                    static_viewer::logical_position(position.x as f32, self.scale_factor),
                    static_viewer::logical_position(position.y as f32, self.scale_factor),
                );
                if redraw {
                    self.request_redraw();
                }
            },
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                let redraw = match state {
                    ElementState::Pressed => self.pointer_down(),
                    ElementState::Released => self.pointer_up(),
                };
                if redraw {
                    self.request_redraw();
                }
            },
            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = wheel_delta_from_winit(delta);
                if self.workspace.scroll_at(
                    self.cursor.0,
                    self.cursor.1,
                    dx / self.scale_factor,
                    dy / self.scale_factor,
                ) {
                    self.request_redraw();
                }
            },
            WindowEvent::KeyboardInput { event, .. } => {
                let navigation =
                    navigation_command(&event.logical_key, event.state, self.modifiers);
                if let Some(command) = navigation {
                    let effect = self.workspace.command(command);
                    self.apply_effect(effect);
                    return;
                }
                let effect = self.workspace.input(SessionInput::Key {
                    key: session_key(&event.logical_key),
                    state: button_state(event.state),
                    modifiers: self.modifiers,
                    repeat: event.repeat,
                });
                let handled = effect.handled;
                let editable = effect.editable;
                self.apply_effect(effect);
                if event.state == ElementState::Pressed
                    && !handled
                    && !editable
                    && let Some(key) = scroll_key(&event.logical_key, self.modifiers.shift)
                    && self.workspace.scroll_for_key(key)
                {
                    self.request_redraw();
                }
            },
            WindowEvent::Ime(ime) => {
                let effect = self.workspace.input(SessionInput::Ime(session_ime(ime)));
                self.apply_effect(effect);
            },
            WindowEvent::Focused(focused) => {
                let effect = self.workspace.input(SessionInput::Focus(focused));
                self.apply_effect(effect);
            },
            WindowEvent::RedrawRequested => self.render(event_loop),
            _ => {},
        }
    }
}

fn require_tile(tree: &TileTree, count: usize) -> Result<(), String> {
    if tree.tiles().len() >= count {
        Ok(())
    } else {
        Err(format!(
            "P3 interaction receipt needs at least {count} document URLs"
        ))
    }
}

fn nearest_edge(point: (f32, f32), rect: WorkspaceRect) -> Edge {
    let x = if rect.width > 0.0 {
        (point.0 - rect.x) / rect.width
    } else {
        0.5
    };
    let y = if rect.height > 0.0 {
        (point.1 - rect.y) / rect.height
    } else {
        0.5
    };
    let distances = [x, 1.0 - x, y, 1.0 - y];
    let index = distances
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| a.total_cmp(b))
        .map_or(0, |(index, _)| index);
    [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom][index]
}

fn physical_extent(logical: f32, scale_factor: f32) -> u32 {
    ((logical.max(1.0) * scale_factor.max(1.0)).round() as u32).max(1)
}

fn placement(rect: WorkspaceRect, scale_factor: f32) -> ExternalTexturePlacement {
    ExternalTexturePlacement::new([
        rect.x * scale_factor,
        rect.y * scale_factor,
        (rect.x + rect.width) * scale_factor,
        (rect.y + rect.height) * scale_factor,
    ])
}

fn button_state(state: ElementState) -> SessionButtonState {
    match state {
        ElementState::Pressed => SessionButtonState::Pressed,
        ElementState::Released => SessionButtonState::Released,
    }
}

fn session_key(key: &Key) -> SessionKey {
    match key {
        Key::Character(text) => SessionKey::Character(text.to_string()),
        Key::Named(NamedKey::Enter) => SessionKey::Enter,
        Key::Named(NamedKey::Tab) => SessionKey::Tab,
        Key::Named(NamedKey::Backspace) => SessionKey::Backspace,
        Key::Named(NamedKey::Delete) => SessionKey::Delete,
        Key::Named(NamedKey::Escape) => SessionKey::Escape,
        Key::Named(NamedKey::Space) => SessionKey::Space,
        Key::Named(NamedKey::ArrowLeft) => SessionKey::ArrowLeft,
        Key::Named(NamedKey::ArrowRight) => SessionKey::ArrowRight,
        Key::Named(NamedKey::ArrowUp) => SessionKey::ArrowUp,
        Key::Named(NamedKey::ArrowDown) => SessionKey::ArrowDown,
        Key::Named(NamedKey::Home) => SessionKey::Home,
        Key::Named(NamedKey::End) => SessionKey::End,
        Key::Named(NamedKey::PageUp) => SessionKey::PageUp,
        Key::Named(NamedKey::PageDown) => SessionKey::PageDown,
        _ => SessionKey::Unidentified,
    }
}

fn scroll_key(key: &Key, shift: bool) -> Option<SessionScrollKey> {
    Some(match key {
        Key::Named(NamedKey::ArrowUp) => SessionScrollKey::LineUp,
        Key::Named(NamedKey::ArrowDown) => SessionScrollKey::LineDown,
        Key::Named(NamedKey::PageUp) => SessionScrollKey::PageUp,
        Key::Named(NamedKey::PageDown) => SessionScrollKey::PageDown,
        Key::Named(NamedKey::Home) => SessionScrollKey::Home,
        Key::Named(NamedKey::End) => SessionScrollKey::End,
        Key::Named(NamedKey::Space) if shift => SessionScrollKey::PageUp,
        Key::Named(NamedKey::Space) => SessionScrollKey::PageDown,
        _ => return None,
    })
}

fn navigation_command(
    key: &Key,
    state: ElementState,
    modifiers: SessionModifiers,
) -> Option<SessionNavigationCommand> {
    if state != ElementState::Pressed {
        return None;
    }
    match key {
        Key::Named(NamedKey::BrowserBack) => Some(SessionNavigationCommand::Back),
        Key::Named(NamedKey::BrowserForward) => Some(SessionNavigationCommand::Forward),
        Key::Named(NamedKey::BrowserRefresh) | Key::Named(NamedKey::F5) => {
            Some(SessionNavigationCommand::Reload)
        },
        Key::Named(NamedKey::ArrowLeft) if modifiers.alt => Some(SessionNavigationCommand::Back),
        Key::Named(NamedKey::ArrowRight) if modifiers.alt => {
            Some(SessionNavigationCommand::Forward)
        },
        Key::Character(text)
            if (modifiers.control || modifiers.meta) && text.eq_ignore_ascii_case("r") =>
        {
            Some(SessionNavigationCommand::Reload)
        },
        _ => None,
    }
}

fn session_ime(ime: winit::event::Ime) -> SessionIme {
    match ime {
        winit::event::Ime::Enabled => SessionIme::Enabled,
        winit::event::Ime::Preedit(text, selection) => SessionIme::Preedit { text, selection },
        winit::event::Ime::Commit(text) => SessionIme::Commit(text),
        winit::event::Ime::Disabled => SessionIme::Disabled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_urls_build_tabs_beside_a_nested_split() {
        let urls = (1..=4)
            .map(|id| format!("tile-{id}.html"))
            .collect::<Vec<_>>();
        let tree = tree_from_urls(&urls);
        let TileTree::Split { axis, children } = tree else {
            panic!("root split");
        };
        assert_eq!(axis, SplitAxis::Row);
        assert_eq!(children.len(), 2);
        assert!(matches!(&children[0].tree, TileTree::Stack(stack) if stack.tabs.len() == 2));
        assert!(matches!(
            &children[1].tree,
            TileTree::Split {
                axis: SplitAxis::Column,
                ..
            }
        ));
    }

    #[test]
    fn nearest_edge_uses_the_content_hole_geometry() {
        let rect = WorkspaceRect::new(100.0, 100.0, 200.0, 100.0);
        assert_eq!(nearest_edge((105.0, 140.0), rect), Edge::Left);
        assert_eq!(nearest_edge((295.0, 140.0), rect), Edge::Right);
        assert_eq!(nearest_edge((200.0, 102.0), rect), Edge::Top);
        assert_eq!(nearest_edge((200.0, 198.0), rect), Edge::Bottom);
    }

    #[test]
    fn receipt_tree_exposes_every_tab_to_frisket_geometry() {
        let urls = [
            "a/index.html",
            "b/index.html",
            "c/index.html",
            "d/index.html",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
        let tree = tree_from_urls(&urls);
        let mut surface = FrisketSurface::new(&tree);
        surface.frame(1000, 700).unwrap();
        assert!(surface.tab_rect(TileId(1)).is_some());
        assert!(surface.tab_rect(TileId(2)).is_some());
        assert!(surface.tab_rect(TileId(3)).is_some());
        assert!(surface.tab_rect(TileId(4)).is_some());
    }
}
