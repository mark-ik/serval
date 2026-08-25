use std::any::Any;
use std::sync::{Arc, Mutex};

use inker::{
    ContentReport, DocumentSession, SessionButtonState, SessionClick, SessionEngine, SessionError,
    SessionFormMethod, SessionFormSubmission, SessionInput, SessionModifiers,
    SessionNavigationCommand, SessionPointerButton, SessionRegistry, SessionScrollKey,
    SessionSpawnRequest, SurfaceEngineRegistry,
};
use pelt_core::{PeltClock, PeltController, PeltControllerConfig};

struct FakeEngine {
    spawns: Arc<Mutex<Vec<SessionSpawnRequest>>>,
    pumps: Arc<Mutex<Vec<f64>>>,
}

impl SessionEngine<String> for FakeEngine {
    fn engine_id(&self) -> &str {
        "fake"
    }

    fn spawn(
        &self,
        request: &SessionSpawnRequest,
    ) -> Result<Box<dyn DocumentSession<String>>, SessionError> {
        self.spawns.lock().unwrap().push(request.clone());
        Ok(Box::new(FakeSession {
            address: request.address.clone(),
            pumps: self.pumps.clone(),
        }))
    }
}

struct FakeSession {
    address: String,
    pumps: Arc<Mutex<Vec<f64>>>,
}

impl DocumentSession<String> for FakeSession {
    fn frame(&mut self, width: u32, height: u32) -> String {
        format!("{}@{width}x{height}", self.address)
    }

    fn scroll_by(&mut self, _dx: f32, _dy: f32) -> bool {
        false
    }

    fn scroll_for_key(&mut self, _key: SessionScrollKey) -> bool {
        false
    }

    fn click_at(&mut self, x: f32, _y: f32) -> SessionClick {
        if x < 10.0 {
            SessionClick::Navigate("next.html".to_owned())
        } else {
            SessionClick::Submit("result.html".to_owned())
        }
    }

    fn form_submission(&mut self, action: &str) -> SessionFormSubmission {
        SessionFormSubmission {
            action: action.to_owned(),
            method: SessionFormMethod::Get,
            fields: vec![("note".to_owned(), "cedar & ash".to_owned())],
        }
    }

    fn pump(&mut self, now_ms: f64) {
        self.pumps.lock().unwrap().push(now_ms);
    }

    fn inspect(&self) -> Option<ContentReport> {
        Some(ContentReport {
            title: Some(self.address.clone()),
            ..Default::default()
        })
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

struct TestClock(f64);

impl PeltClock for TestClock {
    fn now_ms(&self) -> f64 {
        self.0
    }
}

#[derive(Default)]
struct RecordingTarget {
    frames: Vec<String>,
}

impl RecordingTarget {
    fn present(&mut self, frame: String) {
        self.frames.push(frame);
    }
}

fn press(x: f32) -> SessionInput {
    SessionInput::PointerButton {
        x,
        y: 1.0,
        button: SessionPointerButton::Primary,
        state: SessionButtonState::Pressed,
        modifiers: SessionModifiers::default(),
    }
}

#[test]
fn caller_owned_target_drives_the_same_retained_controller_as_desktop() {
    let spawns = Arc::new(Mutex::new(Vec::new()));
    let pumps = Arc::new(Mutex::new(Vec::new()));
    let mut sessions = SessionRegistry::new();
    sessions.register(Box::new(FakeEngine {
        spawns: spawns.clone(),
        pumps: pumps.clone(),
    }));
    let mut controller = PeltController::new(
        sessions,
        SurfaceEngineRegistry::new(),
        PeltControllerConfig::from_request(
            "fake",
            SessionSpawnRequest::new("docs/index.html")
                .with_body("held reader source")
                .with_content_type("text/html")
                .with_viewport(800, 600),
        ),
        TestClock(42.0),
    )
    .unwrap();
    let mut target = RecordingTarget::default();

    assert!(!controller.pump());
    target.present(controller.frame(800, 600));
    assert_eq!(pumps.lock().unwrap().as_slice(), [42.0]);
    assert_eq!(target.frames, ["docs/index.html@800x600"]);
    assert_eq!(controller.title().as_deref(), Some("docs/index.html"));
    assert_eq!(controller.surface_engines().engine_ids().count(), 0);

    let link = controller.input(press(1.0));
    assert!(link.handled && link.redraw && link.navigated);
    assert_eq!(link.pointer_capture, Some(true));
    assert_eq!(controller.address(), "docs/next.html");
    assert!(controller.can_go_back());

    assert!(
        controller
            .command(SessionNavigationCommand::Reload)
            .navigated
    );
    assert!(controller.command(SessionNavigationCommand::Back).navigated);
    assert_eq!(controller.address(), "docs/index.html");
    assert!(
        controller
            .command(SessionNavigationCommand::Forward)
            .navigated
    );
    assert_eq!(controller.address(), "docs/next.html");

    let submission = controller.input(press(20.0));
    assert!(submission.handled && submission.redraw && submission.navigated);
    assert_eq!(controller.address(), "docs/result.html?note=cedar+%26+ash");
    target.present(controller.frame(640, 480));
    assert_eq!(
        target.frames.last().map(String::as_str),
        Some("docs/result.html?note=cedar+%26+ash@640x480")
    );
    let spawns = spawns.lock().unwrap();
    let addresses = spawns
        .iter()
        .map(|request| request.address.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        addresses,
        [
            "docs/index.html",
            "docs/next.html",
            "docs/next.html",
            "docs/index.html",
            "docs/next.html",
            "docs/result.html?note=cedar+%26+ash",
        ]
    );
    assert_eq!(spawns[0].body.as_deref(), Some("held reader source"));
    assert_eq!(spawns[2].body, None);
    assert_eq!(spawns[3].body.as_deref(), Some("held reader source"));
    assert_eq!(spawns[3].viewport, (800, 600));
}
