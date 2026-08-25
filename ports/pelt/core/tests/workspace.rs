use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use genet_host_api::tile::{
    ContentSource, DocumentRef, DropTarget, Edge, SplitAxis, Tile, TileBranch, TileEvent, TileId,
    TilePath, TileTree,
};
use inker::{
    ContentReport, DocumentSession, SessionButtonState, SessionClick, SessionEngine, SessionError,
    SessionInput, SessionModifiers, SessionNavigationCommand, SessionPointerButton,
    SessionRegistry, SessionScrollKey, SessionSpawnRequest, SurfaceEngineRegistry,
};
use pelt_core::{PeltClock, PeltController, PeltControllerConfig, PeltWorkspace, WorkspaceRect};

#[derive(Default)]
struct Probe {
    spawns: Vec<String>,
    hidden: HashMap<String, Vec<bool>>,
}

struct FakeEngine(Arc<Mutex<Probe>>);

impl SessionEngine<String> for FakeEngine {
    fn engine_id(&self) -> &str {
        "fake"
    }

    fn spawn(
        &self,
        request: &SessionSpawnRequest,
    ) -> Result<Box<dyn DocumentSession<String>>, SessionError> {
        self.0.lock().unwrap().spawns.push(request.address.clone());
        Ok(Box::new(FakeSession {
            address: request.address.clone(),
            scroll: 0.0,
            probe: self.0.clone(),
        }))
    }
}

struct FakeSession {
    address: String,
    scroll: f32,
    probe: Arc<Mutex<Probe>>,
}

impl DocumentSession<String> for FakeSession {
    fn frame(&mut self, width: u32, height: u32) -> String {
        format!("{}@{width}x{height}+{}", self.address, self.scroll)
    }

    fn scroll_by(&mut self, _dx: f32, dy: f32) -> bool {
        self.scroll += dy;
        true
    }

    fn scroll_for_key(&mut self, _key: SessionScrollKey) -> bool {
        self.scroll += 40.0;
        true
    }

    fn click_at(&mut self, x: f32, _y: f32) -> SessionClick {
        if x < 10.0 {
            SessionClick::Navigate("next.html".to_owned())
        } else {
            SessionClick::Handled
        }
    }

    fn links(&self) -> Vec<inker::SessionLink> {
        Vec::new()
    }

    fn set_hidden(&mut self, hidden: bool) {
        self.probe
            .lock()
            .unwrap()
            .hidden
            .entry(self.address.clone())
            .or_default()
            .push(hidden);
    }

    fn inspect(&self) -> Option<ContentReport> {
        Some(ContentReport {
            title: Some(format!("title:{}", self.address)),
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

struct TestClock;

impl PeltClock for TestClock {
    fn now_ms(&self) -> f64 {
        1.0
    }
}

fn tile(id: u64, address: &str) -> Tile {
    Tile {
        id: TileId(id),
        title: address.to_owned(),
        content: ContentSource::Document(DocumentRef(address.to_owned())),
        accent: None,
    }
}

fn controller(tile: &Tile, probe: Arc<Mutex<Probe>>) -> Result<PeltController<String>, String> {
    let ContentSource::Document(DocumentRef(address)) = &tile.content else {
        return Err("not a document".to_owned());
    };
    let mut sessions = SessionRegistry::new();
    sessions.register(Box::new(FakeEngine(probe)));
    PeltController::new(
        sessions,
        SurfaceEngineRegistry::new(),
        PeltControllerConfig::new("fake", address, (800, 600)),
        TestClock,
    )
}

fn press(x: f32, y: f32) -> SessionInput {
    SessionInput::PointerButton {
        x,
        y,
        button: SessionPointerButton::Primary,
        state: SessionButtonState::Pressed,
        modifiers: SessionModifiers::default(),
    }
}

#[test]
fn recursive_workspace_retains_each_tile_across_activation_drag_and_resize() {
    let tree = TileTree::split(
        SplitAxis::Row,
        vec![
            TileBranch::new(
                0.5,
                TileTree::stack(vec![tile(1, "a/index.html"), tile(2, "b/index.html")], 0),
            ),
            TileBranch::new(
                0.5,
                TileTree::split(
                    SplitAxis::Column,
                    vec![
                        TileBranch::new(0.5, TileTree::single(tile(3, "c/index.html"))),
                        TileBranch::new(0.5, TileTree::single(tile(4, "d/index.html"))),
                    ],
                ),
            ),
        ],
    );
    let probe = Arc::new(Mutex::new(Probe::default()));
    let mut workspace =
        PeltWorkspace::try_new(tree, |tile| controller(tile, probe.clone())).unwrap();

    assert_eq!(workspace.focused_tile(), Some(TileId(1)));
    assert_eq!(probe.lock().unwrap().spawns.len(), 4);
    assert_eq!(
        probe.lock().unwrap().hidden["b/index.html"].last(),
        Some(&true),
        "the inactive tab is hidden without being discarded"
    );

    workspace.set_content_rects([
        (TileId(1), WorkspaceRect::new(0.0, 44.0, 390.0, 556.0)),
        (TileId(3), WorkspaceRect::new(400.0, 44.0, 400.0, 251.0)),
        (TileId(4), WorkspaceRect::new(400.0, 349.0, 400.0, 251.0)),
    ]);
    assert!(workspace.scroll_at(410.0, 360.0, 0.0, 75.0));
    let first = workspace.frame();
    assert_eq!(first.tiles.len(), 3);
    assert_eq!(first.tiles[2].frame, "d/index.html@400x251+75");

    // The pointer is translated through tile 3's content-hole origin, so an
    // x=405 workspace press reaches the session at local x=5 and follows its
    // relative link. Tile 4's navigation remains independent.
    assert!(workspace.input(press(405.0, 50.0)).navigated);
    assert_eq!(
        workspace.controller(TileId(3)).unwrap().address(),
        "c/next.html"
    );
    assert_eq!(
        workspace.controller(TileId(4)).unwrap().address(),
        "d/index.html"
    );
    assert!(
        workspace
            .command_for(
                TileId(4),
                SessionNavigationCommand::Address("other.html".to_owned()),
            )
            .navigated
    );
    assert!(workspace.controller(TileId(3)).unwrap().can_go_back());
    assert!(workspace.controller(TileId(4)).unwrap().can_go_back());

    assert!(workspace.apply(&TileEvent::Activated(TileId(2))));
    assert_eq!(workspace.focused_tile(), Some(TileId(2)));
    assert_eq!(probe.lock().unwrap().spawns.len(), 6);
    assert_eq!(
        probe.lock().unwrap().hidden["a/index.html"].last(),
        Some(&true)
    );
    workspace.set_content_rects([
        (TileId(2), WorkspaceRect::new(0.0, 44.0, 390.0, 556.0)),
        (TileId(3), WorkspaceRect::new(400.0, 44.0, 400.0, 251.0)),
        (TileId(4), WorkspaceRect::new(400.0, 349.0, 400.0, 251.0)),
    ]);
    assert!(workspace.scroll_at(410.0, 360.0, 0.0, 75.0));

    // Divider resize and tab drag only change arrangement. All four retained
    // controllers and the hidden tile's history survive.
    assert!(workspace.apply(&TileEvent::DividerMoved {
        split: TilePath(Vec::new()),
        fractions: vec![0.65, 0.35],
    }));
    assert!(workspace.apply(&TileEvent::Dragged {
        tile: TileId(2),
        to: DropTarget::Edge {
            tile: TileId(4),
            edge: Edge::Left,
        },
    }));
    assert_eq!(probe.lock().unwrap().spawns.len(), 6);
    assert!(workspace.controller(TileId(3)).unwrap().can_go_back());
    assert_eq!(
        workspace.controller(TileId(4)).unwrap().address(),
        "d/other.html"
    );

    // Reactivating tile 4 proves its live session retained the pre-tab scroll.
    assert!(workspace.apply(&TileEvent::Activated(TileId(4))));
    workspace.set_content_rects([
        (TileId(1), WorkspaceRect::new(0.0, 44.0, 300.0, 556.0)),
        (TileId(3), WorkspaceRect::new(310.0, 44.0, 240.0, 251.0)),
        (TileId(4), WorkspaceRect::new(560.0, 349.0, 240.0, 251.0)),
    ]);
    let frames = workspace.frame();
    let tile4 = frames
        .tiles
        .iter()
        .find(|frame| frame.tile == TileId(4))
        .expect("tile 4 is visible");
    assert_eq!(tile4.frame, "d/other.html@240x251+75");

    assert!(workspace.apply(&TileEvent::Closed(TileId(3))));
    assert!(workspace.controller(TileId(3)).is_none());
    assert!(workspace.controller(TileId(1)).is_some());
    assert!(workspace.controller(TileId(2)).is_some());
    assert!(workspace.controller(TileId(4)).is_some());
}
