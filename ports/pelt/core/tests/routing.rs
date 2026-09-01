use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use inker::routing::{ENGINE_SCRYING_WEB, EngineRoutePolicy, EngineRouteRule, SurfaceContractMode};
use inker::{
    A11yCapability, DocumentClip, DocumentClipArtifact, DocumentClipArtifactRole, DocumentSession,
    EngineProfileBinding, FocusReason, KeyboardEvent, MouseEvent, SessionClick, SessionEngine,
    SessionError, SessionRegistry, SessionScrollKey, SessionSpawnRequest, SurfaceEngine,
    SurfaceEngineRegistry, SurfaceError, SurfaceFrame, SurfaceProducer, SurfaceSettings,
    SurfaceSpawnRequest,
};
use pelt_core::{
    PeltClock, PeltDocumentState, PeltRegistries, PeltRouteSource, PeltRouteState, PeltTileRequest,
    PeltWorkspace, WorkspaceRect,
};
use workbench::{ContentSource, DocumentRef, SplitAxis, Tile, TileBranch, TileId, TileTree};

#[derive(Default)]
struct DocumentProbe {
    spawns: Vec<(String, String)>,
}

struct FakeDocumentEngine {
    id: &'static str,
    probe: Arc<Mutex<DocumentProbe>>,
    fail: bool,
    fail_addresses: Arc<Mutex<HashSet<String>>>,
    capability: A11yCapability,
}

impl SessionEngine<String> for FakeDocumentEngine {
    fn engine_id(&self) -> &str {
        self.id
    }

    fn spawn(
        &self,
        request: &SessionSpawnRequest,
    ) -> Result<Box<dyn DocumentSession<String>>, SessionError> {
        if self.fail {
            return Err(SessionError::SpawnFailed("forced route failure".to_owned()));
        }
        if self
            .fail_addresses
            .lock()
            .expect("fake failed-addresses lock")
            .contains(&request.address)
        {
            return Err(SessionError::SpawnFailed("forced load failure".to_owned()));
        }
        self.probe
            .lock()
            .unwrap()
            .spawns
            .push((self.id.to_owned(), request.address.clone()));
        Ok(Box::new(FakeDocument {
            id: self.id,
            address: request.address.clone(),
        }))
    }

    fn a11y_capability(&self) -> A11yCapability {
        self.capability
    }
}

struct FakeDocument {
    id: &'static str,
    address: String,
}

impl DocumentSession<String> for FakeDocument {
    fn frame(&mut self, width: u32, height: u32) -> String {
        format!("{}:{}@{width}x{height}", self.id, self.address)
    }

    fn scroll_by(&mut self, _dx: f32, _dy: f32) -> bool {
        false
    }

    fn scroll_for_key(&mut self, _key: SessionScrollKey) -> bool {
        false
    }

    fn click_at(&mut self, _x: f32, _y: f32) -> SessionClick {
        SessionClick::Miss
    }

    fn links(&self) -> Vec<inker::SessionLink> {
        Vec::new()
    }

    fn as_any_ref(&self) -> &dyn Any {
        self
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

struct SourceArtifactEngine {
    requests: Arc<Mutex<Vec<SessionSpawnRequest>>>,
}

impl SessionEngine<String> for SourceArtifactEngine {
    fn engine_id(&self) -> &str {
        "fake.source"
    }

    fn spawn(
        &self,
        request: &SessionSpawnRequest,
    ) -> Result<Box<dyn DocumentSession<String>>, SessionError> {
        self.requests.lock().unwrap().push(request.clone());
        Ok(Box::new(SourceArtifactDocument {
            address: request.address.clone(),
        }))
    }
}

struct SourceArtifactDocument {
    address: String,
}

impl DocumentSession<String> for SourceArtifactDocument {
    fn frame(&mut self, width: u32, height: u32) -> String {
        format!("source:{}@{width}x{height}", self.address)
    }

    fn scroll_by(&mut self, _dx: f32, _dy: f32) -> bool {
        false
    }

    fn scroll_for_key(&mut self, _key: SessionScrollKey) -> bool {
        false
    }

    fn click_at(&mut self, _x: f32, _y: f32) -> SessionClick {
        SessionClick::Miss
    }

    fn links(&self) -> Vec<inker::SessionLink> {
        Vec::new()
    }

    fn clip(&self) -> Option<DocumentClip> {
        Some(DocumentClip {
            source_url: self.address.clone(),
            title: None,
            text: "Held source".to_owned(),
            selector: None,
            links: Vec::new(),
            artifacts: vec![DocumentClipArtifact {
                role: DocumentClipArtifactRole::SourceResponse,
                media_type: "text/html; charset=utf-8".to_owned(),
                canonical_uri: "https://reader.test/final.html".to_owned(),
                bytes: b"<main><h1>Held source</h1></main>".to_vec(),
            }],
        })
    }

    fn as_any_ref(&self) -> &dyn Any {
        self
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

struct HeldBodyEngine {
    requests: Arc<Mutex<Vec<SessionSpawnRequest>>>,
}

impl SessionEngine<String> for HeldBodyEngine {
    fn engine_id(&self) -> &str {
        "fake.reader"
    }

    fn spawn(
        &self,
        request: &SessionSpawnRequest,
    ) -> Result<Box<dyn DocumentSession<String>>, SessionError> {
        self.requests.lock().unwrap().push(request.clone());
        Ok(Box::new(FakeDocument {
            id: "fake.reader",
            address: request.address.clone(),
        }))
    }
}

#[derive(Default)]
struct SurfaceProbe {
    spawns: Vec<String>,
    resizes: Vec<(u32, u32)>,
    offsets: Vec<(i32, i32)>,
    frames: usize,
}

struct FakeSurfaceEngine(Arc<Mutex<SurfaceProbe>>);

impl SurfaceEngine for FakeSurfaceEngine {
    fn engine_id(&self) -> &str {
        "fake.surface"
    }

    fn spawn(
        &self,
        request: &SurfaceSpawnRequest,
    ) -> Result<Box<dyn SurfaceProducer>, SurfaceError> {
        self.0.lock().unwrap().spawns.push(request.url.clone());
        Ok(Box::new(FakeSurface(self.0.clone())))
    }
}

struct FakeSurface(Arc<Mutex<SurfaceProbe>>);

impl SurfaceProducer for FakeSurface {
    fn resize(&mut self, width: u32, height: u32) -> Result<(), SurfaceError> {
        self.0.lock().unwrap().resizes.push((width, height));
        Ok(())
    }

    fn set_offset(&mut self, x: i32, y: i32) -> Result<(), SurfaceError> {
        self.0.lock().unwrap().offsets.push((x, y));
        Ok(())
    }

    fn acquire_frame(&mut self) -> Result<Option<SurfaceFrame>, SurfaceError> {
        self.0.lock().unwrap().frames += 1;
        Ok(None)
    }

    fn send_mouse_input(&mut self, _event: MouseEvent) -> Result<(), SurfaceError> {
        Ok(())
    }

    fn send_pointer_input(&mut self, _event: inker::PointerEvent) -> Result<(), SurfaceError> {
        Ok(())
    }

    fn send_keyboard_input(&mut self, _event: KeyboardEvent) -> Result<(), SurfaceError> {
        Ok(())
    }

    fn move_focus(&mut self, _reason: FocusReason) -> Result<(), SurfaceError> {
        Ok(())
    }

    fn poll_cursor_shape(&mut self) -> Option<inker::CursorShape> {
        None
    }

    fn apply_settings(&mut self, _settings: &SurfaceSettings) -> Result<(), SurfaceError> {
        Ok(())
    }
}

struct TestClock;

impl PeltClock for TestClock {
    fn now_ms(&self) -> f64 {
        0.0
    }
}

fn tile(id: u64) -> Tile {
    Tile {
        id: TileId(id),
        title: format!("tile-{id}"),
        content: ContentSource::Document(DocumentRef(format!("tile-{id}.html"))),
        accent: None,
    }
}

#[test]
fn shared_registries_route_documents_surfaces_overrides_and_visible_fallbacks() {
    let documents = Arc::new(Mutex::new(DocumentProbe::default()));
    let surfaces = Arc::new(Mutex::new(SurfaceProbe::default()));
    let mut sessions = SessionRegistry::new();
    sessions.register(Box::new(FakeDocumentEngine {
        id: "fake.static",
        probe: documents.clone(),
        fail: false,
        fail_addresses: Arc::new(Mutex::new(HashSet::new())),
        capability: A11yCapability::Full,
    }));
    sessions.register(Box::new(FakeDocumentEngine {
        id: "fake.scripted",
        probe: documents.clone(),
        fail: false,
        fail_addresses: Arc::new(Mutex::new(HashSet::new())),
        capability: A11yCapability::Partial,
    }));
    sessions.register(Box::new(FakeDocumentEngine {
        id: "fake.fail",
        probe: documents.clone(),
        fail: true,
        fail_addresses: Arc::new(Mutex::new(HashSet::new())),
        capability: A11yCapability::Partial,
    }));
    let mut surface_engines = SurfaceEngineRegistry::new();
    surface_engines.register(Box::new(FakeSurfaceEngine(surfaces.clone())));
    let policy = EngineRoutePolicy {
        rules: vec![
            EngineRouteRule::new(
                ["overlay"],
                "fake.surface",
                SurfaceContractMode::NativeOverlay,
            ),
            EngineRouteRule::new(["embed"], "fake.surface", SurfaceContractMode::EmbeddedHost),
            EngineRouteRule::new(
                ["fake"],
                "fake.static",
                SurfaceContractMode::CompositedTexture,
            ),
        ],
        fallback: EngineRouteRule::new(
            std::iter::empty::<&str>(),
            "fake.static",
            SurfaceContractMode::CompositedTexture,
        ),
        per_host_overrides: HashMap::new(),
    };
    let registries = PeltRegistries::new(
        sessions,
        surface_engines,
        policy,
        "pelt-test",
        "fake.static",
        EngineProfileBinding {
            user_data_dir: "pelt-test-profile".to_owned(),
        },
    );
    let tree = TileTree::split(
        SplitAxis::Row,
        (1..=6)
            .map(|id| TileBranch::new(1.0 / 6.0, TileTree::single(tile(id))))
            .collect(),
    );
    let mut workspace = PeltWorkspace::try_routed(
        tree,
        registries,
        |tile| {
            let ContentSource::Document(DocumentRef(address)) = &tile.content else {
                unreachable!()
            };
            let address = match tile.id.0 {
                5 => "overlay://tile-5",
                6 => "embed://tile-6",
                _ => address,
            };
            let request = PeltTileRequest::new(address, (800, 600));
            Ok(match tile.id.0 {
                1 => request.with_engine_override("fake.static"),
                2 => request.with_engine_override("fake.scripted"),
                3 => request.with_engine_override("fake.surface"),
                4 => request.with_engine_override(ENGINE_SCRYING_WEB),
                5 => request,
                6 => request,
                _ => unreachable!(),
            })
        },
        || Box::new(TestClock),
    )
    .unwrap();

    let tile1 = workspace.controller(TileId(1)).unwrap();
    let tile2 = workspace.controller(TileId(2)).unwrap();
    let tile4 = workspace.controller(TileId(4)).unwrap();
    assert!(tile1.shares_registries_with(tile2));
    assert!(tile1.shares_registries_with(tile4));
    assert_eq!(
        workspace.route(TileId(2)).unwrap().source,
        PeltRouteSource::UserOverride
    );
    assert_eq!(
        workspace.route(TileId(3)).unwrap().state,
        PeltRouteState::Surface
    );
    assert!(matches!(
        workspace.route(TileId(4)).unwrap().state,
        PeltRouteState::Fallback { ref active_engine, .. } if active_engine == "fake.static"
    ));
    assert_eq!(
        workspace.route(TileId(5)).unwrap().source,
        PeltRouteSource::Automatic
    );
    assert!(matches!(
        workspace.route(TileId(5)).unwrap().state,
        PeltRouteState::Fallback { ref reason, .. } if reason.contains("NativeOverlay")
    ));
    assert!(matches!(
        workspace.route(TileId(6)).unwrap().state,
        PeltRouteState::Fallback { ref reason, .. } if reason.contains("EmbeddedHost")
    ));
    let surface_inspection = workspace
        .inspection(TileId(3))
        .expect("live surface exposes its declared capability");
    assert_eq!(surface_inspection.capability, A11yCapability::Opaque);
    assert_eq!(surface_inspection.report, None);
    let fallback_inspection = workspace
        .inspection(TileId(4))
        .expect("fallback exposes its active document capability");
    assert_eq!(fallback_inspection.capability, A11yCapability::Full);
    assert_eq!(fallback_inspection.report, None);
    assert!(
        workspace
            .tree()
            .tiles()
            .into_iter()
            .find(|tile| tile.id == TileId(4))
            .unwrap()
            .title
            .contains("scrying.web → fake.static")
    );
    assert!(
        workspace
            .tree()
            .tiles()
            .into_iter()
            .find(|tile| tile.id == TileId(3))
            .unwrap()
            .title
            .contains("fake.surface")
    );

    workspace.set_content_rects([
        (TileId(1), WorkspaceRect::new(0.0, 10.0, 100.0, 90.0)),
        (TileId(2), WorkspaceRect::new(100.0, 10.0, 100.0, 90.0)),
        (TileId(3), WorkspaceRect::new(200.0, 10.0, 120.0, 90.0)),
        (TileId(4), WorkspaceRect::new(320.0, 10.0, 100.0, 90.0)),
        (TileId(5), WorkspaceRect::new(420.0, 10.0, 100.0, 90.0)),
        (TileId(6), WorkspaceRect::new(520.0, 10.0, 100.0, 90.0)),
    ]);
    workspace.set_surface_scale_factor(2.0);
    let frame = workspace.frame();
    assert_eq!(frame.tiles.len(), 5);
    assert_eq!(frame.surfaces.len(), 1);
    assert!(frame.surfaces[0].frame.as_ref().unwrap().is_none());
    let surface_probe = surfaces.lock().unwrap();
    assert_eq!(surface_probe.spawns, ["tile-3.html"]);
    assert_eq!(surface_probe.resizes, [(240, 180)]);
    assert_eq!(surface_probe.offsets, [(400, 20)]);
    assert_eq!(surface_probe.frames, 1);
    drop(surface_probe);
    let cached_frame = workspace.frame_with_cached_surfaces();
    assert_eq!(cached_frame.tiles.len(), 5);
    assert_eq!(cached_frame.surfaces.len(), 1);
    assert!(cached_frame.surfaces[0].frame.as_ref().unwrap().is_none());
    assert_eq!(surfaces.lock().unwrap().frames, 1);
    assert!(workspace.pump());
    assert_eq!(workspace.execute_surface_script(TileId(1), "1"), Ok(None));
    assert!(
        workspace
            .execute_surface_script(TileId(3), "1")
            .unwrap_err()
            .contains("no web scripting plane")
    );

    assert!(
        workspace
            .command_for(
                TileId(1),
                inker::SessionNavigationCommand::Address("next.html".to_owned()),
            )
            .navigated
    );
    assert!(
        workspace
            .set_route_override(TileId(1), Some("fake.scripted".to_owned()))
            .unwrap()
    );
    assert_eq!(
        workspace.route(TileId(1)).unwrap().active_engine(),
        "fake.scripted"
    );
    assert_eq!(
        workspace.controller(TileId(1)).unwrap().address(),
        "next.html"
    );
    assert!(
        workspace
            .tree()
            .tiles()
            .into_iter()
            .find(|tile| tile.id == TileId(1))
            .unwrap()
            .title
            .contains("fake.scripted")
    );
    assert!(
        workspace
            .set_route_override(TileId(2), Some("fake.fail".to_owned()))
            .is_err()
    );
    assert_eq!(
        workspace.route(TileId(2)).unwrap().active_engine(),
        "fake.scripted"
    );
    assert_eq!(
        workspace.controller(TileId(2)).unwrap().engine_id(),
        "fake.scripted"
    );
}

#[test]
fn failed_document_navigation_keeps_the_prior_session_and_records_a_recoverable_error() {
    let documents = Arc::new(Mutex::new(DocumentProbe::default()));
    let failed_addresses = Arc::new(Mutex::new(HashSet::from(["missing.html".to_owned()])));
    let mut sessions = SessionRegistry::new();
    sessions.register(Box::new(FakeDocumentEngine {
        id: "fake.static",
        probe: documents,
        fail: false,
        fail_addresses: failed_addresses.clone(),
        capability: A11yCapability::Full,
    }));
    let registries = PeltRegistries::new(
        sessions,
        SurfaceEngineRegistry::new(),
        EngineRoutePolicy {
            rules: Vec::new(),
            fallback: EngineRouteRule::new(
                std::iter::empty::<&str>(),
                "fake.static",
                SurfaceContractMode::CompositedTexture,
            ),
            per_host_overrides: HashMap::new(),
        },
        "pelt-load-state-test",
        "fake.static",
        EngineProfileBinding {
            user_data_dir: "pelt-load-state-test-profile".to_owned(),
        },
    );
    let mut workspace = PeltWorkspace::try_routed(
        TileTree::single(tile(1)),
        registries,
        |tile| {
            let ContentSource::Document(DocumentRef(address)) = &tile.content else {
                unreachable!()
            };
            Ok(PeltTileRequest::new(address, (800, 600)))
        },
        || Box::new(TestClock),
    )
    .expect("seed document opens");

    let failed = workspace.command_for(
        TileId(1),
        inker::SessionNavigationCommand::Address("missing.html".to_owned()),
    );
    assert!(
        failed.handled,
        "a visible error document consumes the action"
    );
    assert!(
        failed.redraw,
        "a visible error document requests composition"
    );
    assert!(!failed.navigated);
    assert!(
        failed
            .error
            .as_deref()
            .is_some_and(|error| error.contains("could not load missing.html"))
    );
    let controller = workspace
        .controller(TileId(1))
        .expect("seed controller remains");
    assert_eq!(controller.address(), "tile-1.html");
    assert_eq!(controller.session_generation(), 1);
    assert_eq!(workspace.document_session_generation(TileId(1)), Some(1));
    assert_eq!(
        controller
            .session_as_any_ref()
            .downcast_ref::<FakeDocument>()
            .expect("fake document stays observable through Pelt")
            .address,
        "tile-1.html"
    );
    assert!(!controller.can_go_back());
    assert_eq!(
        controller.document_state(),
        &PeltDocumentState::Error {
            address: "missing.html".to_owned(),
            message: failed.error.clone().expect("load error"),
        }
    );

    let recovered = workspace.command_for(
        TileId(1),
        inker::SessionNavigationCommand::Address("recovered.html".to_owned()),
    );
    assert!(recovered.navigated);
    let controller = workspace
        .controller(TileId(1))
        .expect("replacement controller");
    assert_eq!(controller.address(), "recovered.html");
    assert_eq!(controller.session_generation(), 2);
    assert_eq!(workspace.document_session_generation(TileId(1)), Some(2));
    assert_eq!(
        controller
            .session_as_any_ref()
            .downcast_ref::<FakeDocument>()
            .expect("successful navigation replaces the observable session")
            .address,
        "recovered.html"
    );
    assert!(controller.can_go_back());
    assert_eq!(
        controller.document_state(),
        &PeltDocumentState::Loading {
            address: "recovered.html".to_owned(),
        }
    );

    workspace.mark_visible_documents_presented();
    assert_eq!(
        workspace
            .controller(TileId(1))
            .expect("visible controller")
            .document_state(),
        &PeltDocumentState::Ready
    );

    failed_addresses
        .lock()
        .expect("fake failed-addresses lock")
        .insert("tile-1.html".to_owned());
    let failed_back = workspace.command_for(TileId(1), inker::SessionNavigationCommand::Back);
    assert!(failed_back.handled, "a failed history traversal is visible");
    assert!(failed_back.redraw, "a failed history traversal is composed");
    assert!(
        !failed_back.navigated,
        "the history cursor stays on its prior entry"
    );
    let controller = workspace
        .controller(TileId(1))
        .expect("failed Back retains its active controller");
    assert_eq!(controller.address(), "recovered.html");
    assert_eq!(
        controller.session_generation(),
        2,
        "failed traversal does not replace the active session"
    );
    assert_eq!(workspace.document_session_generation(TileId(1)), Some(2));
    assert!(
        controller.can_go_back(),
        "the failed target remains reachable"
    );
    assert!(!controller.can_go_forward());
    assert!(matches!(
        controller.document_state(),
        PeltDocumentState::Error { address, .. } if address == "tile-1.html"
    ));
}

#[test]
fn source_artifact_is_held_across_a_live_route_switch() {
    let source_requests = Arc::new(Mutex::new(Vec::new()));
    let reader_requests = Arc::new(Mutex::new(Vec::new()));
    let mut sessions = SessionRegistry::new();
    sessions.register(Box::new(SourceArtifactEngine {
        requests: source_requests.clone(),
    }));
    sessions.register(Box::new(HeldBodyEngine {
        requests: reader_requests.clone(),
    }));
    let registries = PeltRegistries::new(
        sessions,
        SurfaceEngineRegistry::new(),
        EngineRoutePolicy {
            rules: Vec::new(),
            fallback: EngineRouteRule::new(
                std::iter::empty::<&str>(),
                "fake.source",
                SurfaceContractMode::CompositedTexture,
            ),
            per_host_overrides: HashMap::new(),
        },
        "pelt-source-retention-test",
        "fake.source",
        EngineProfileBinding {
            user_data_dir: "pelt-source-retention-test-profile".to_owned(),
        },
    );
    let mut workspace = PeltWorkspace::try_routed(
        TileTree::single(tile(1)),
        registries,
        |tile| {
            let ContentSource::Document(DocumentRef(address)) = &tile.content else {
                unreachable!()
            };
            Ok(PeltTileRequest::new(
                format!("{address}#reader-source"),
                (800, 600),
            ))
        },
        || Box::new(TestClock),
    )
    .expect("source document opens");
    let source_identity = workspace
        .document_session_identity(TileId(1))
        .expect("source controller identity");

    assert!(
        workspace
            .set_route_override(TileId(1), Some("fake.reader".to_owned()))
            .expect("reader route uses the retained source")
    );
    let reader_identity = workspace
        .document_session_identity(TileId(1))
        .expect("reader controller identity");
    assert_ne!(
        reader_identity, source_identity,
        "a reconstructed controller receives a distinct session identity even at generation one"
    );
    let reader_request = reader_requests
        .lock()
        .expect("reader request lock")
        .pop()
        .expect("reader was spawned from the source artifact");
    assert_eq!(
        reader_request.address,
        "https://reader.test/final.html#reader-source"
    );
    assert_eq!(
        reader_request.body.as_deref(),
        Some("<main><h1>Held source</h1></main>")
    );
    assert_eq!(
        reader_request.content_type.as_deref(),
        Some("text/html; charset=utf-8")
    );

    assert!(
        workspace
            .set_route_override(TileId(1), None)
            .expect("automatic route reconstructs from the retained source")
    );
    let source_requests = source_requests.lock().expect("source request lock");
    assert_eq!(source_requests.len(), 2);
    assert_eq!(source_requests[0].body, None);
    assert_eq!(
        source_requests[1].body.as_deref(),
        Some("<main><h1>Held source</h1></main>")
    );
    assert_eq!(
        source_requests[1].content_type.as_deref(),
        Some("text/html; charset=utf-8")
    );
}
