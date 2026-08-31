use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use inker::routing::{
    EngineRouteDecision, EngineRoutePolicy, EngineRouteRequest, SurfaceContractMode,
    WorkspaceRouteId, is_surface_engine,
};
use inker::{
    A11yCapability, ContentReport, DocumentClipArtifactRole, EngineProfileBinding, FocusReason,
    KeyboardEvent, KeyboardModifiers, MouseButton, MouseEvent, MouseEventKind, PhysicalPosition,
    SessionButtonState, SessionInput, SessionKey, SessionNavigationCommand, SessionPointerButton,
    SessionRegistry, SessionScrollKey, SessionSpawnRequest, SurfaceEngineRegistry, SurfaceFrame,
    SurfaceProducer, SurfaceSpawnRequest,
};
use workbench::{
    ContentSource, Tile, TileEvent, TileId, TileTree, Workbench, WorkbenchEffect, WorkbenchOutcome,
};

use crate::{PeltClock, PeltController, PeltControllerConfig, PeltHostEffect};

/// Host-long-lived engine factories and routing policy shared by every tile.
pub struct PeltRegistries<F> {
    sessions: Arc<SessionRegistry<F>>,
    surfaces: Arc<SurfaceEngineRegistry>,
    policy: EngineRoutePolicy,
    workspace_id: WorkspaceRouteId,
    fallback_document_engine: String,
    surface_profile: EngineProfileBinding,
}

impl<F> Clone for PeltRegistries<F> {
    fn clone(&self) -> Self {
        Self {
            sessions: self.sessions.clone(),
            surfaces: self.surfaces.clone(),
            policy: self.policy.clone(),
            workspace_id: self.workspace_id.clone(),
            fallback_document_engine: self.fallback_document_engine.clone(),
            surface_profile: self.surface_profile.clone(),
        }
    }
}

impl<F> PeltRegistries<F> {
    pub fn new(
        sessions: SessionRegistry<F>,
        surfaces: SurfaceEngineRegistry,
        policy: EngineRoutePolicy,
        workspace_id: impl Into<String>,
        fallback_document_engine: impl Into<String>,
        surface_profile: EngineProfileBinding,
    ) -> Self {
        Self {
            sessions: Arc::new(sessions),
            surfaces: Arc::new(surfaces),
            policy,
            workspace_id: WorkspaceRouteId::new(workspace_id),
            fallback_document_engine: fallback_document_engine.into(),
            surface_profile,
        }
    }

    pub fn sessions(&self) -> &SessionRegistry<F> {
        &self.sessions
    }

    pub fn surfaces(&self) -> &SurfaceEngineRegistry {
        &self.surfaces
    }

    fn route(&self, tile: TileId, request: &PeltTileRequest) -> PeltTileRoute {
        let route_request = EngineRouteRequest {
            workspace_id: self.workspace_id.clone(),
            view: None,
            node: None,
            address: request.request.address.clone(),
            content_type: request.request.content_type.clone(),
            pinned_engine: request.engine_override.clone(),
        };
        let source = if request.engine_override.is_some() {
            PeltRouteSource::UserOverride
        } else {
            PeltRouteSource::Automatic
        };
        let decision = if request.engine_override.is_some() {
            // Keep an unavailable user choice visible so the host can explain
            // the active fallback. Automatic routing filters unavailable lanes.
            self.policy.route(&route_request)
        } else {
            self.policy.route_filtered(&route_request, |engine| {
                self.sessions.contains(engine) || self.surfaces.contains(engine)
            })
        };
        PeltTileRoute {
            tile,
            decision,
            source,
            state: PeltRouteState::Document,
        }
    }
}

/// Inputs that select one tile's lane. A source-capable document session can
/// refresh the held body after navigation, so routing never needs to refetch
/// merely to switch engines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeltTileRequest {
    pub request: SessionSpawnRequest,
    pub engine_override: Option<String>,
}

impl PeltTileRequest {
    pub fn new(address: impl Into<String>, viewport: (u32, u32)) -> Self {
        Self {
            request: SessionSpawnRequest::new(address).with_viewport(viewport.0, viewport.1),
            engine_override: None,
        }
    }

    pub fn from_request(request: SessionSpawnRequest) -> Self {
        Self {
            request,
            engine_override: None,
        }
    }

    pub fn with_engine_override(mut self, engine_id: impl Into<String>) -> Self {
        self.engine_override = Some(engine_id.into());
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeltRouteSource {
    Automatic,
    UserOverride,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeltRouteState {
    Document,
    Surface,
    Fallback {
        active_engine: String,
        reason: String,
    },
}

/// Selected route plus the lane that is actually active for the tile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeltTileRoute {
    pub tile: TileId,
    pub decision: EngineRouteDecision,
    pub source: PeltRouteSource,
    pub state: PeltRouteState,
}

impl PeltTileRoute {
    pub fn selected_engine(&self) -> &str {
        &self.decision.engine_id
    }

    pub fn active_engine(&self) -> &str {
        match &self.state {
            PeltRouteState::Fallback { active_engine, .. } => active_engine,
            PeltRouteState::Document | PeltRouteState::Surface => &self.decision.engine_id,
        }
    }
}

/// The active tile's declared semantic capability and any structural report
/// its engine provides. Surface lanes retain their declared capability even
/// when they cannot expose a report, so hosts never guess from route kind.
#[derive(Clone, Debug, PartialEq)]
pub struct PeltTileInspection {
    pub capability: A11yCapability,
    pub report: Option<ContentReport>,
}

/// One Frisket content hole in workspace coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WorkspaceRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl WorkspaceRect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x
            && y >= self.y
            && x < self.x + self.width.max(0.0)
            && y < self.y + self.height.max(0.0)
    }

    fn local(self, x: f32, y: f32) -> (f32, f32) {
        (x - self.x, y - self.y)
    }

    fn viewport(self) -> (u32, u32) {
        (
            self.width.max(1.0).ceil() as u32,
            self.height.max(1.0).ceil() as u32,
        )
    }
}

/// One active document layer returned by [`PeltWorkspace::frame`].
pub struct PeltTileFrame<F> {
    pub tile: TileId,
    pub rect: WorkspaceRect,
    pub frame: F,
}

/// One surface-engine layer. A native frame remains an Inker handle until the
/// embedding host imports it on its own wgpu device.
pub struct PeltSurfaceLayer {
    pub tile: TileId,
    pub rect: WorkspaceRect,
    pub route: PeltTileRoute,
    pub frame: Result<Option<SurfaceFrame>, String>,
}

/// The document layers for one workspace frame. Frisket's frame scene remains
/// host-owned because the reusable core is generic over the document frame.
pub struct PeltWorkspaceFrame<F> {
    pub tiles: Vec<PeltTileFrame<F>>,
    pub surfaces: Vec<PeltSurfaceLayer>,
}

/// Pelt's result for a workspace command.
///
/// The embedded [`WorkbenchOutcome`] reports the shared arrangement result or
/// a host request. `focus_changed` covers Pelt's retained-controller focus,
/// which can change even when an already-active tab leaves the tree intact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeltWorkspaceOutcome {
    workbench: WorkbenchOutcome,
    focus_changed: bool,
}

impl PeltWorkspaceOutcome {
    pub const fn changed(self) -> bool {
        self.workbench.changed() || self.focus_changed
    }

    pub const fn effect(self) -> Option<WorkbenchEffect> {
        self.workbench.effect()
    }

    pub const fn workbench(self) -> WorkbenchOutcome {
        self.workbench
    }
}

struct PeltSurfaceController {
    producer: Box<dyn SurfaceProducer>,
    viewport: (u32, u32),
    offset: Option<(i32, i32)>,
}

impl PeltSurfaceController {
    fn frame(
        &mut self,
        rect: WorkspaceRect,
        scale_factor: f32,
    ) -> Result<Option<SurfaceFrame>, String> {
        let viewport = (
            physical_extent(rect.width, scale_factor),
            physical_extent(rect.height, scale_factor),
        );
        if viewport != self.viewport {
            self.producer
                .resize(viewport.0, viewport.1)
                .map_err(|error| format!("surface resize failed: {error}"))?;
            self.viewport = viewport;
        }
        let offset = (
            physical_offset(rect.x, scale_factor),
            physical_offset(rect.y, scale_factor),
        );
        if Some(offset) != self.offset {
            self.producer
                .set_offset(offset.0, offset.1)
                .map_err(|error| format!("surface placement failed: {error}"))?;
            self.offset = Some(offset);
        }
        self.producer
            .acquire_frame()
            .map_err(|error| format!("surface frame failed: {error}"))
    }

    fn mouse(&mut self, event: MouseEvent) -> PeltHostEffect {
        match self.producer.send_mouse_input(event) {
            Ok(()) => PeltHostEffect {
                handled: true,
                redraw: true,
                ..Default::default()
            },
            Err(error) => PeltHostEffect {
                error: Some(format!("surface input failed: {error}")),
                ..Default::default()
            },
        }
    }

    fn keyboard(&mut self, event: KeyboardEvent) -> PeltHostEffect {
        match self.producer.send_keyboard_input(event) {
            Ok(()) => PeltHostEffect {
                handled: true,
                redraw: true,
                ..Default::default()
            },
            Err(error) => PeltHostEffect {
                error: Some(format!("surface input failed: {error}")),
                ..Default::default()
            },
        }
    }

    fn focus(&mut self) {
        let _ = self.producer.move_focus(FocusReason::Programmatic);
    }
}

struct RoutedWorkspace<F> {
    registries: PeltRegistries<F>,
    requests: HashMap<TileId, PeltTileRequest>,
    base_titles: HashMap<TileId, String>,
    clock_for: Arc<dyn Fn() -> Box<dyn PeltClock>>,
}

/// Pelt's window-neutral recursive workspace.
///
/// `Workbench` wraps the arrangement authority. Every document tile owns a
/// live controller, including inactive tabs; Frisket content-hole rectangles
/// arrive from the embedding host and are the only geometry used for routing
/// and frame sizing.
pub struct PeltWorkspace<F> {
    workbench: Workbench,
    controllers: HashMap<TileId, PeltController<F>>,
    surfaces: HashMap<TileId, PeltSurfaceController>,
    routes: HashMap<TileId, PeltTileRoute>,
    content_rects: HashMap<TileId, WorkspaceRect>,
    focused: Option<TileId>,
    pointer_capture: Option<TileId>,
    surface_scale_factor: f32,
    routed: Option<RoutedWorkspace<F>>,
}

impl<F: 'static> PeltWorkspace<F> {
    /// Compatibility constructor for callers that already select and build
    /// one document controller per tile. New hosts use [`Self::try_routed`].
    pub fn try_new(
        tree: TileTree,
        mut controller_for: impl FnMut(&Tile) -> Result<PeltController<F>, String>,
    ) -> Result<Self, String> {
        let mut controllers = HashMap::new();
        let mut tile_ids = HashSet::new();
        for tile in tree.tiles() {
            if !tile_ids.insert(tile.id) {
                return Err(format!("duplicate tile id {}", tile.id.0));
            }
            if matches!(tile.content, ContentSource::Document(_)) {
                let controller = controller_for(tile)
                    .map_err(|error| format!("could not open tile {}: {error}", tile.id.0))?;
                controllers.insert(tile.id, controller);
            }
        }
        let focused = active_tiles(&tree)
            .into_iter()
            .find(|id| controllers.contains_key(id));
        let routes = controllers
            .iter()
            .map(|(tile, controller)| {
                (
                    *tile,
                    PeltTileRoute {
                        tile: *tile,
                        decision: EngineRouteDecision {
                            engine_id: controller.engine_id().to_owned(),
                            surface_contract: inker::routing::SurfaceContract {
                                target: inker::routing::SurfaceTargetId::new(format!(
                                    "pelt:tile:{}",
                                    tile.0
                                )),
                                mode: SurfaceContractMode::CompositedTexture,
                            },
                        },
                        source: PeltRouteSource::UserOverride,
                        state: PeltRouteState::Document,
                    },
                )
            })
            .collect();
        let mut workspace = Self {
            workbench: Workbench::new(tree),
            controllers,
            surfaces: HashMap::new(),
            routes,
            content_rects: HashMap::new(),
            focused,
            pointer_capture: None,
            surface_scale_factor: 1.0,
            routed: None,
        };
        workspace.sync_tile_metadata();
        workspace.sync_visibility();
        Ok(workspace)
    }

    /// Route every document tile through one pair of shared registries. A
    /// registered surface engine becomes a retained producer; an unavailable
    /// or unattached surface contract stays selected and visibly falls back to
    /// the configured document engine.
    pub fn try_routed(
        tree: TileTree,
        registries: PeltRegistries<F>,
        mut request_for: impl FnMut(&Tile) -> Result<PeltTileRequest, String>,
        clock_for: impl Fn() -> Box<dyn PeltClock> + 'static,
    ) -> Result<Self, String> {
        let mut tile_ids = HashSet::new();
        let mut requests = HashMap::new();
        let mut base_titles = HashMap::new();
        for tile in tree.tiles() {
            if !tile_ids.insert(tile.id) {
                return Err(format!("duplicate tile id {}", tile.id.0));
            }
            if matches!(tile.content, ContentSource::Document(_)) {
                requests.insert(
                    tile.id,
                    request_for(tile)
                        .map_err(|error| format!("could not route tile {}: {error}", tile.id.0))?,
                );
                base_titles.insert(tile.id, tile.title.clone());
            }
        }
        let active = active_tiles(&tree);
        let focused = active.iter().copied().find(|id| requests.contains_key(id));
        let mut workspace = Self {
            workbench: Workbench::new(tree),
            controllers: HashMap::new(),
            surfaces: HashMap::new(),
            routes: HashMap::new(),
            content_rects: HashMap::new(),
            focused,
            pointer_capture: None,
            surface_scale_factor: 1.0,
            routed: Some(RoutedWorkspace {
                registries,
                requests,
                base_titles,
                clock_for: Arc::new(clock_for),
            }),
        };
        let ids = workspace
            .routed
            .as_ref()
            .expect("routed workspace")
            .requests
            .keys()
            .copied()
            .collect::<Vec<_>>();
        for id in ids {
            workspace.install_route(id)?;
        }
        workspace.sync_tile_metadata();
        workspace.sync_visibility();
        Ok(workspace)
    }

    pub fn tree(&self) -> &TileTree {
        self.workbench.tree()
    }

    pub fn focused_tile(&self) -> Option<TileId> {
        self.focused
    }

    /// Whether an ordinary physical pointer gesture still owns workspace
    /// routing. Synthetic semantic activation must wait for that gesture to
    /// finish rather than borrowing its capture target.
    pub fn has_active_pointer_capture(&self) -> bool {
        self.pointer_capture.is_some()
    }

    pub fn controller(&self, tile: TileId) -> Option<&PeltController<F>> {
        self.controllers.get(&tile)
    }

    pub fn controller_mut(&mut self, tile: TileId) -> Option<&mut PeltController<F>> {
        self.controllers.get_mut(&tile)
    }

    /// Identity generation of a tile's current successfully opened document
    /// session. Surface-only tiles return `None`.
    ///
    /// Hosts retaining engine-specific observations can pair this with the
    /// tile id and clear that state when a successful navigation, reload, or
    /// history traversal replaces the session.
    pub fn document_session_generation(&self, tile: TileId) -> Option<u64> {
        self.controllers
            .get(&tile)
            .map(PeltController::session_generation)
    }

    pub fn route(&self, tile: TileId) -> Option<&PeltTileRoute> {
        self.routes.get(&tile)
    }

    pub fn routes(&self) -> impl Iterator<Item = &PeltTileRoute> {
        self.routes.values()
    }

    /// Inspect the provider actually active for `tile`. A selected surface may
    /// have fallen back to a document controller, in which case the document's
    /// capability and report are authoritative. A live surface instead exposes
    /// only the capability declared by its registered surface engine.
    pub fn inspection(&self, tile: TileId) -> Option<PeltTileInspection> {
        let route = self.routes.get(&tile)?;
        match &route.state {
            PeltRouteState::Document | PeltRouteState::Fallback { .. } => {
                let controller = self.controllers.get(&tile)?;
                Some(PeltTileInspection {
                    capability: controller.a11y_capability(),
                    report: controller.inspect(),
                })
            },
            PeltRouteState::Surface => {
                let routed = self.routed.as_ref()?;
                let capability = routed
                    .registries
                    .surfaces
                    .engine(route.active_engine())?
                    .a11y_capability();
                Some(PeltTileInspection {
                    capability,
                    report: None,
                })
            },
        }
    }

    /// Evaluate a script through one routed surface's optional web control
    /// plane. Document tiles return `Ok(None)` so hosts can probe a mixed
    /// workspace without duplicating route-state checks.
    pub fn execute_surface_script(
        &mut self,
        tile: TileId,
        script: &str,
    ) -> Result<Option<String>, String> {
        let Some(surface) = self.surfaces.get_mut(&tile) else {
            return Ok(None);
        };
        let web = surface
            .producer
            .as_web_surface()
            .ok_or_else(|| format!("tile {} surface has no web scripting plane", tile.0))?;
        web.execute_script_with_result(script)
            .map(Some)
            .map_err(|error| format!("tile {} surface script failed: {error}", tile.0))
    }

    /// Replace or clear the user engine choice for one live tile. The selected
    /// address/body stay held while the tile is reconstructed through the same
    /// shared registry pair.
    pub fn set_route_override(
        &mut self,
        tile: TileId,
        engine_id: Option<String>,
    ) -> Result<bool, String> {
        let Some(routed) = self.routed.as_mut() else {
            return Err("this workspace was built without capability routing".to_owned());
        };
        let Some(request) = routed.requests.get_mut(&tile) else {
            return Err(format!("tile {} has no routed content", tile.0));
        };
        if request.engine_override == engine_id {
            return Ok(false);
        }
        let previous_request = request.clone();
        request.engine_override = engine_id;
        let previous_controller = self.controllers.remove(&tile);
        let previous_surface = self.surfaces.remove(&tile);
        let previous_route = self.routes.remove(&tile);
        if let Err(error) = self.install_route(tile) {
            self.routed
                .as_mut()
                .expect("routed workspace")
                .requests
                .insert(tile, previous_request);
            if let Some(controller) = previous_controller {
                self.controllers.insert(tile, controller);
            }
            if let Some(surface) = previous_surface {
                self.surfaces.insert(tile, surface);
            }
            if let Some(route) = previous_route {
                self.routes.insert(tile, route);
            }
            return Err(error);
        }
        self.sync_one_tile_metadata(tile);
        self.sync_visibility();
        Ok(true)
    }

    fn install_route(&mut self, tile: TileId) -> Result<(), String> {
        let routed = self.routed.as_ref().expect("routed workspace");
        let request = routed
            .requests
            .get(&tile)
            .cloned()
            .ok_or_else(|| format!("tile {} has no route request", tile.0))?;
        let mut route = routed.registries.route(tile, &request);
        let selected = route.decision.engine_id.clone();
        if routed.registries.sessions.contains(&selected) {
            let controller = PeltController::new_shared_boxed(
                routed.registries.sessions.clone(),
                routed.registries.surfaces.clone(),
                PeltControllerConfig::from_request(selected, request.request),
                (routed.clock_for)(),
            )?;
            self.controllers.insert(tile, controller);
            self.routes.insert(tile, route);
            return Ok(());
        }

        if routed.registries.surfaces.contains(&selected)
            && route.decision.surface_contract.mode == SurfaceContractMode::CompositedTexture
        {
            let viewport = request.request.viewport;
            let spawn = SurfaceSpawnRequest {
                url: request.request.address.clone(),
                width: viewport.0,
                height: viewport.1,
                profile: routed.registries.surface_profile.clone(),
                fence_handle: None,
            };
            let producer = routed
                .registries
                .surfaces
                .spawn(&route.decision, &spawn)
                .map_err(|error| format!("could not spawn surface {selected}: {error}"))?;
            route.state = PeltRouteState::Surface;
            self.surfaces.insert(
                tile,
                PeltSurfaceController {
                    producer,
                    viewport,
                    offset: None,
                },
            );
            self.routes.insert(tile, route);
            return Ok(());
        }

        let fallback = routed.registries.fallback_document_engine.clone();
        if !routed.registries.sessions.contains(&fallback) {
            return Err(format!(
                "selected engine {selected} is unavailable and fallback engine {fallback} is not registered"
            ));
        }
        let reason = if routed.registries.surfaces.contains(&selected) {
            format!(
                "surface contract {:?} needs an embedding adapter",
                route.decision.surface_contract.mode
            )
        } else if is_surface_engine(&selected) {
            "surface engine is not registered on this host".to_owned()
        } else {
            "document engine is not registered on this host".to_owned()
        };
        route.state = PeltRouteState::Fallback {
            active_engine: fallback.clone(),
            reason,
        };
        let controller = PeltController::new_shared_boxed(
            routed.registries.sessions.clone(),
            routed.registries.surfaces.clone(),
            PeltControllerConfig::from_request(fallback, request.request),
            (routed.clock_for)(),
        )?;
        self.controllers.insert(tile, controller);
        self.routes.insert(tile, route);
        Ok(())
    }

    pub fn content_rect(&self, tile: TileId) -> Option<WorkspaceRect> {
        self.content_rects.get(&tile).copied()
    }

    /// Physical pixels per workspace unit for native surface producers.
    /// Document sessions continue to receive logical content-hole extents.
    pub fn set_surface_scale_factor(&mut self, scale_factor: f32) {
        self.surface_scale_factor = scale_factor.max(1.0);
    }

    /// Replace the content-hole geometry read from the latest Frisket layout.
    /// Rectangles for inactive or closed tiles are deliberately discarded.
    pub fn set_content_rects(&mut self, rects: impl IntoIterator<Item = (TileId, WorkspaceRect)>) {
        self.content_rects = rects.into_iter().collect();
    }

    /// Apply a standalone Pelt arrangement gesture through the shared
    /// Workbench reducer, preserving Pelt's controller and focus custody.
    pub fn apply(&mut self, event: &TileEvent) -> bool {
        self.apply_outcome(event).changed()
    }

    /// Apply a workspace gesture and expose any request that needs a desktop
    /// host decision. A tearout leaves the tree and Pelt controller custody
    /// unchanged until that host accepts it.
    pub fn apply_outcome(&mut self, event: &TileEvent) -> PeltWorkspaceOutcome {
        let workbench = self.workbench.apply(event);
        if !workbench.changed() {
            let mut focus_changed = false;
            if let TileEvent::Activated(id) = event {
                if active_tiles(self.workbench.tree()).contains(id) && self.has_content(*id) {
                    focus_changed = self.focused != Some(*id);
                    self.focus(*id);
                }
            }
            return PeltWorkspaceOutcome {
                workbench,
                focus_changed,
            };
        }

        let retained = self
            .workbench
            .tree()
            .tiles()
            .into_iter()
            .map(|tile| tile.id)
            .collect::<HashSet<_>>();
        self.controllers.retain(|id, _| retained.contains(id));
        self.surfaces.retain(|id, _| retained.contains(id));
        self.routes.retain(|id, _| retained.contains(id));
        self.content_rects.retain(|id, _| retained.contains(id));
        if let Some(routed) = &mut self.routed {
            routed.requests.retain(|id, _| retained.contains(id));
            routed.base_titles.retain(|id, _| retained.contains(id));
        }
        if self
            .pointer_capture
            .is_some_and(|id| !retained.contains(&id))
        {
            self.pointer_capture = None;
        }

        let next_focus = match event {
            TileEvent::Activated(id) | TileEvent::Dragged { tile: id, .. }
                if self.has_content(*id) =>
            {
                Some(*id)
            },
            _ if self.focused.is_some_and(|id| retained.contains(&id)) => self.focused,
            _ => active_tiles(self.workbench.tree())
                .into_iter()
                .find(|id| self.has_content(*id)),
        };
        match next_focus {
            Some(id) => self.focus(id),
            None => self.focused = None,
        }
        self.sync_visibility();
        PeltWorkspaceOutcome {
            workbench,
            focus_changed: false,
        }
    }

    fn has_content(&self, tile: TileId) -> bool {
        self.controllers.contains_key(&tile) || self.surfaces.contains_key(&tile)
    }

    /// Produce one frame for every active document hole, sized to that hole.
    pub fn frame(&mut self) -> PeltWorkspaceFrame<F> {
        self.frame_with_surface_polling(true)
    }

    /// Produce document frames and the active surface layers without polling
    /// their producers.
    ///
    /// A host can use this for a bounded compositor capture after it has
    /// already imported a native surface frame. The returned surface layers
    /// preserve their routes and geometry, while their `frame` is `Ok(None)`
    /// so the host reuses its cached imported view instead of advancing an
    /// external producer.
    pub fn frame_with_cached_surfaces(&mut self) -> PeltWorkspaceFrame<F> {
        self.frame_with_surface_polling(false)
    }

    fn frame_with_surface_polling(&mut self, poll_surfaces: bool) -> PeltWorkspaceFrame<F> {
        let active = active_tiles(self.workbench.tree());
        let mut tiles = Vec::with_capacity(active.len());
        let mut surfaces = Vec::new();
        for tile in active {
            let Some(rect) = self.content_rects.get(&tile).copied() else {
                continue;
            };
            if let Some(controller) = self.controllers.get_mut(&tile) {
                let (width, height) = rect.viewport();
                tiles.push(PeltTileFrame {
                    tile,
                    rect,
                    frame: controller.frame(width, height),
                });
            } else if let Some(surface) = self.surfaces.get_mut(&tile) {
                let frame = if poll_surfaces {
                    surface.frame(rect, self.surface_scale_factor)
                } else {
                    Ok(None)
                };
                if let Some(route) = self.routes.get(&tile).cloned() {
                    surfaces.push(PeltSurfaceLayer {
                        tile,
                        rect,
                        route,
                        frame,
                    });
                }
            }
        }
        self.sync_tile_metadata();
        PeltWorkspaceFrame { tiles, surfaces }
    }

    /// Advance visible sessions. Hidden tabs retain state without driving the
    /// foreground frame loop.
    pub fn pump(&mut self) -> bool {
        let active = active_tiles(self.workbench.tree());
        // Surface producers do not expose a settled bit. Keep polling every
        // visible producer so a frame that arrives after the first acquire is
        // not stranded until unrelated document activity causes a redraw.
        let mut more = active.iter().any(|id| self.surfaces.contains_key(id));
        for id in active {
            if let Some(controller) = self.controllers.get_mut(&id) {
                more |= controller.pump();
            }
        }
        more
    }

    /// Mark every visible document session as composed by the embedding host.
    ///
    /// The host calls this after its presentation boundary. Hidden tabs retain
    /// their loading state until their own first visible composition.
    pub fn mark_visible_documents_presented(&mut self) {
        for id in active_tiles(self.workbench.tree()) {
            if let Some(controller) = self.controllers.get_mut(&id) {
                controller.mark_document_presented();
            }
        }
    }

    /// Route neutral input. Pointer coordinates are workspace coordinates and
    /// are translated into the selected Frisket content hole; keyboard, text,
    /// IME, and focus route to the focused tile.
    pub fn input(&mut self, input: SessionInput) -> PeltHostEffect {
        let (target, local_input) = match input {
            SessionInput::PointerMoved { x, y, modifiers } => {
                let target = self.pointer_capture.or_else(|| self.tile_at(x, y));
                let Some(target) = target else {
                    return PeltHostEffect::default();
                };
                let Some(rect) = self.content_rect(target) else {
                    return PeltHostEffect::default();
                };
                let (x, y) = rect.local(x, y);
                (target, SessionInput::PointerMoved { x, y, modifiers })
            },
            SessionInput::PointerButton {
                x,
                y,
                button,
                state,
                modifiers,
            } => {
                let target = self.pointer_capture.or_else(|| self.tile_at(x, y));
                let Some(target) = target else {
                    return PeltHostEffect::default();
                };
                let Some(rect) = self.content_rect(target) else {
                    return PeltHostEffect::default();
                };
                self.focus(target);
                let (x, y) = rect.local(x, y);
                (
                    target,
                    SessionInput::PointerButton {
                        x,
                        y,
                        button,
                        state,
                        modifiers,
                    },
                )
            },
            other => {
                let Some(target) = self.focused else {
                    return PeltHostEffect::default();
                };
                (target, other)
            },
        };

        let effect = if let Some(controller) = self.controllers.get_mut(&target) {
            controller.input(local_input)
        } else if let Some(surface) = self.surfaces.get_mut(&target) {
            surface_input(surface, local_input, self.surface_scale_factor)
        } else {
            return PeltHostEffect::default();
        };
        if let Some(capture) = effect.pointer_capture {
            self.pointer_capture = capture.then_some(target);
        }
        if effect.navigated {
            self.sync_one_tile_metadata(target);
            self.sync_visibility();
        }
        effect
    }

    pub fn scroll_at(&mut self, x: f32, y: f32, dx: f32, dy: f32) -> bool {
        let Some(tile) = self.tile_at(x, y) else {
            return false;
        };
        let Some(rect) = self.content_rect(tile) else {
            return false;
        };
        let (x, y) = rect.local(x, y);
        if let Some(controller) = self.controllers.get_mut(&tile) {
            return controller.scroll_at(x, y, dx, dy);
        }
        self.surfaces.get_mut(&tile).is_some_and(|surface| {
            surface
                .mouse(MouseEvent {
                    position: PhysicalPosition {
                        x: x * self.surface_scale_factor,
                        y: y * self.surface_scale_factor,
                    },
                    button: None,
                    kind: MouseEventKind::ScrollPixels {
                        delta_x: dx * self.surface_scale_factor,
                        delta_y: dy * self.surface_scale_factor,
                    },
                })
                .handled
        })
    }

    pub fn scroll_for_key(&mut self, key: SessionScrollKey) -> bool {
        let Some(tile) = self.focused else {
            return false;
        };
        self.controllers
            .get_mut(&tile)
            .is_some_and(|controller| controller.scroll_for_key(key))
    }

    pub fn command(&mut self, command: SessionNavigationCommand) -> PeltHostEffect {
        let Some(tile) = self.focused else {
            return PeltHostEffect::default();
        };
        self.command_for(tile, command)
    }

    pub fn command_for(
        &mut self,
        tile: TileId,
        command: SessionNavigationCommand,
    ) -> PeltHostEffect {
        let Some(controller) = self.controllers.get_mut(&tile) else {
            let Some(surface) = self.surfaces.get_mut(&tile) else {
                return PeltHostEffect::default();
            };
            let result = match command {
                SessionNavigationCommand::Address(address) => surface
                    .producer
                    .as_web_surface()
                    .ok_or_else(|| "surface has no web navigation plane".to_owned())
                    .and_then(|web| {
                        web.navigate_to_url(&address)
                            .map_err(|error| error.to_string())
                    }),
                SessionNavigationCommand::Reload => surface
                    .producer
                    .as_web_surface()
                    .ok_or_else(|| "surface has no web navigation plane".to_owned())
                    .and_then(|web| web.reload().map_err(|error| error.to_string())),
                SessionNavigationCommand::Back => surface
                    .producer
                    .as_web_surface()
                    .ok_or_else(|| "surface has no web navigation plane".to_owned())
                    .and_then(|web| web.go_back().map_err(|error| error.to_string())),
                SessionNavigationCommand::Forward => surface
                    .producer
                    .as_web_surface()
                    .ok_or_else(|| "surface has no web navigation plane".to_owned())
                    .and_then(|web| web.go_forward().map_err(|error| error.to_string())),
                SessionNavigationCommand::Stop => surface
                    .producer
                    .as_web_surface()
                    .ok_or_else(|| "surface has no web navigation plane".to_owned())
                    .and_then(|web| web.stop().map_err(|error| error.to_string())),
            };
            return match result {
                Ok(()) => PeltHostEffect {
                    handled: true,
                    redraw: true,
                    navigated: true,
                    ..Default::default()
                },
                Err(error) => PeltHostEffect {
                    error: Some(error),
                    ..Default::default()
                },
            };
        };
        let effect = controller.command(command);
        if effect.navigated {
            self.sync_one_tile_metadata(tile);
            self.sync_visibility();
        }
        effect
    }

    fn tile_at(&self, x: f32, y: f32) -> Option<TileId> {
        active_tiles(self.workbench.tree()).into_iter().find(|id| {
            self.content_rects
                .get(id)
                .is_some_and(|rect| rect.contains(x, y))
        })
    }

    fn focus(&mut self, tile: TileId) {
        if self.focused == Some(tile) {
            return;
        }
        if let Some(old) = self.focused.and_then(|id| self.controllers.get_mut(&id)) {
            let _ = old.input(SessionInput::Focus(false));
        }
        self.focused = Some(tile);
        if let Some(new) = self.controllers.get_mut(&tile) {
            let _ = new.input(SessionInput::Focus(true));
        } else if let Some(new) = self.surfaces.get_mut(&tile) {
            new.focus();
        }
    }

    fn sync_visibility(&mut self) {
        let active = active_tiles(self.workbench.tree())
            .into_iter()
            .collect::<HashSet<_>>();
        for (id, controller) in &mut self.controllers {
            controller.set_hidden(!active.contains(id));
        }
    }

    fn sync_tile_metadata(&mut self) {
        let ids = self.routes.keys().copied().collect::<Vec<_>>();
        for id in ids {
            self.sync_one_tile_metadata(id);
        }
    }

    fn sync_one_tile_metadata(&mut self, id: TileId) {
        let controller_metadata = self.controllers.get(&id).map(|controller| {
            let request = controller.request().clone();
            let source = request.body.is_none().then(|| {
                controller.clip().and_then(|clip| {
                    clip.artifacts
                        .into_iter()
                        .find(|artifact| artifact.role == DocumentClipArtifactRole::SourceResponse)
                })
            });
            (request, controller.title(), source.flatten())
        });
        if let Some((request, _, source)) = &controller_metadata
            && let Some(routed) = &mut self.routed
            && let Some(tile_request) = routed.requests.get_mut(&id)
        {
            tile_request.request = request.clone();
            if let Some(source) = source {
                tile_request.request.address =
                    retained_source_address(&source.canonical_uri, &tile_request.request.address);
                tile_request.request.body =
                    Some(String::from_utf8_lossy(&source.bytes).into_owned());
                tile_request.request.content_type = Some(source.media_type.clone());
            }
        }
        let route = self.routes.get(&id);
        let base_title = controller_metadata
            .as_ref()
            .and_then(|(_, title, _)| title.clone())
            .filter(|title| !title.trim().is_empty())
            .or_else(|| {
                self.routed
                    .as_ref()
                    .and_then(|routed| routed.base_titles.get(&id).cloned())
            });
        if let Some(tile) = self.workbench.tree_mut().tile_mut(id) {
            if let Some((request, _, _)) = controller_metadata {
                tile.content = ContentSource::Document(workbench::DocumentRef(request.address));
            }
            if let Some(route) = route {
                let selected = route.selected_engine();
                let suffix = match &route.state {
                    PeltRouteState::Fallback { active_engine, .. } => {
                        format!("[{selected} → {active_engine}]")
                    },
                    PeltRouteState::Document | PeltRouteState::Surface => {
                        format!("[{selected}]")
                    },
                };
                tile.title = format!("{} {suffix}", base_title.as_deref().unwrap_or(selected));
            } else if let Some(title) = base_title {
                tile.title = title;
            }
        }
    }
}

fn retained_source_address(canonical_uri: &str, requested_address: &str) -> String {
    if canonical_uri.contains('#') {
        return canonical_uri.to_owned();
    }
    requested_address.split_once('#').map_or_else(
        || canonical_uri.to_owned(),
        |(_, fragment)| format!("{canonical_uri}#{fragment}"),
    )
}

fn surface_input(
    surface: &mut PeltSurfaceController,
    input: SessionInput,
    scale_factor: f32,
) -> PeltHostEffect {
    match input {
        SessionInput::PointerMoved { x, y, .. } => surface.mouse(MouseEvent {
            position: PhysicalPosition {
                x: x * scale_factor,
                y: y * scale_factor,
            },
            button: None,
            kind: MouseEventKind::Moved,
        }),
        SessionInput::PointerButton {
            x,
            y,
            button,
            state,
            ..
        } => surface.mouse(MouseEvent {
            position: PhysicalPosition {
                x: x * scale_factor,
                y: y * scale_factor,
            },
            button: Some(match button {
                SessionPointerButton::Primary => MouseButton::Left,
                SessionPointerButton::Secondary => MouseButton::Right,
                SessionPointerButton::Auxiliary => MouseButton::Middle,
            }),
            kind: match state {
                SessionButtonState::Pressed => MouseEventKind::Pressed,
                SessionButtonState::Released => MouseEventKind::Released,
            },
        }),
        SessionInput::Key {
            key,
            state,
            modifiers,
            ..
        } => surface.keyboard(KeyboardEvent {
            key_code: surface_key_code(&key),
            scan_code: 0,
            modifiers: KeyboardModifiers {
                shift: modifiers.shift,
                ctrl: modifiers.control,
                alt: modifiers.alt,
                meta: modifiers.meta,
            },
            pressed: state == SessionButtonState::Pressed,
            text: match (state, key) {
                (SessionButtonState::Pressed, SessionKey::Character(text)) => Some(text),
                (SessionButtonState::Pressed, SessionKey::Space) => Some(" ".to_owned()),
                _ => None,
            },
        }),
        SessionInput::Text(text) => surface.keyboard(KeyboardEvent {
            key_code: 0,
            scan_code: 0,
            modifiers: KeyboardModifiers::default(),
            pressed: true,
            text: Some(text),
        }),
        SessionInput::Focus(true) => {
            surface.focus();
            PeltHostEffect {
                handled: true,
                ..Default::default()
            }
        },
        SessionInput::Focus(false)
        | SessionInput::FocusMove(_)
        | SessionInput::Ime(_)
        | SessionInput::Cancel => PeltHostEffect::default(),
    }
}

fn surface_key_code(key: &SessionKey) -> u32 {
    match key {
        SessionKey::Enter => 13,
        SessionKey::Tab => 9,
        SessionKey::Backspace => 8,
        SessionKey::Delete => 46,
        SessionKey::Escape => 27,
        SessionKey::Space => 32,
        SessionKey::ArrowLeft => 37,
        SessionKey::ArrowUp => 38,
        SessionKey::ArrowRight => 39,
        SessionKey::ArrowDown => 40,
        SessionKey::Home => 36,
        SessionKey::End => 35,
        SessionKey::PageUp => 33,
        SessionKey::PageDown => 34,
        SessionKey::Character(_) | SessionKey::Unidentified => 0,
    }
}

fn physical_extent(logical: f32, scale_factor: f32) -> u32 {
    ((logical.max(1.0) * scale_factor.max(1.0)).round() as u32).max(1)
}

fn physical_offset(logical: f32, scale_factor: f32) -> i32 {
    (logical * scale_factor.max(1.0)).round() as i32
}

fn active_tiles(tree: &TileTree) -> Vec<TileId> {
    fn visit(tree: &TileTree, active: &mut Vec<TileId>) {
        match tree {
            TileTree::Split { children, .. } => {
                for branch in children {
                    visit(&branch.tree, active);
                }
            },
            TileTree::Stack(stack) => {
                if let Some(tile) = stack.tabs.get(stack.active) {
                    active.push(tile.id);
                }
            },
        }
    }

    let mut active = Vec::new();
    visit(tree, &mut active);
    active
}
