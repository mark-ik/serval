/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Window-neutral Pelt host controller.

use genet_host_api::resolve_href;
use inker::{
    ContentReport, DocumentSession, SessionEffect, SessionFormMethod, SessionInput,
    SessionInputResult, SessionNavigationCommand, SessionRegistry, SessionScrollKey,
    SessionSpawnRequest, SurfaceEngineRegistry,
};

/// A caller-owned monotonic clock. Pelt asks for the current time only while
/// pumping a retained session; it neither selects a system clock nor owns an
/// event loop.
pub trait PeltClock: 'static {
    fn now_ms(&self) -> f64;
}

/// Initial engine and document request for one retained Pelt controller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeltControllerConfig {
    pub engine_id: String,
    pub request: SessionSpawnRequest,
}

impl PeltControllerConfig {
    pub fn new(
        engine_id: impl Into<String>,
        address: impl Into<String>,
        viewport: (u32, u32),
    ) -> Self {
        Self::from_request(
            engine_id,
            SessionSpawnRequest::new(address).with_viewport(viewport.0, viewport.1),
        )
    }

    /// Preserve a caller-held body, content type, visibility, and viewport in
    /// the first spawn request. Reader hosts use this to supply fleeced source
    /// bytes without teaching the controller how to fetch them.
    pub fn from_request(engine_id: impl Into<String>, request: SessionSpawnRequest) -> Self {
        Self {
            engine_id: engine_id.into(),
            request,
        }
    }
}

/// Host work requested after session input or a navigation command.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PeltHostEffect {
    pub handled: bool,
    pub redraw: bool,
    pub cursor: Option<inker::SessionCursor>,
    pub pointer_capture: Option<bool>,
    pub editable: bool,
    pub navigated: bool,
    pub error: Option<String>,
}

/// Pelt's reusable one-session browser controller.
///
/// Concrete document engines and their resource policy arrive inside
/// `session_engines`. Surface engines are retained for the P4 composition
/// lane, but P2 does not select or spawn them. Frames remain generic, so the
/// embedding host owns every wgpu resource and presentation target.
pub struct PeltController<F> {
    session_engines: SessionRegistry<F>,
    surface_engines: SurfaceEngineRegistry,
    engine_id: String,
    session: Box<dyn DocumentSession<F>>,
    history: Vec<SessionSpawnRequest>,
    history_index: usize,
    viewport: (u32, u32),
    clock: Box<dyn PeltClock>,
}

impl<F: 'static> PeltController<F> {
    pub fn new(
        session_engines: SessionRegistry<F>,
        surface_engines: SurfaceEngineRegistry,
        config: PeltControllerConfig,
        clock: impl PeltClock,
    ) -> Result<Self, String> {
        let engine_id = config.engine_id;
        let viewport = config.request.viewport;
        let session = session_engines
            .spawn(&engine_id, &config.request)
            .map_err(|error| format!("could not spawn engine {engine_id}: {error}"))?;
        Ok(Self {
            session_engines,
            surface_engines,
            engine_id,
            session,
            history: vec![config.request],
            history_index: 0,
            viewport,
            clock: Box::new(clock),
        })
    }

    pub fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub fn address(&self) -> &str {
        &self.history[self.history_index].address
    }

    pub fn title(&self) -> Option<String> {
        self.session.inspect().and_then(|report| report.title)
    }

    pub fn inspect(&self) -> Option<ContentReport> {
        self.session.inspect()
    }

    pub fn can_go_back(&self) -> bool {
        self.history_index > 0
    }

    pub fn can_go_forward(&self) -> bool {
        self.history_index + 1 < self.history.len()
    }

    pub fn session_engines(&self) -> &SessionRegistry<F> {
        &self.session_engines
    }

    pub fn session_engines_mut(&mut self) -> &mut SessionRegistry<F> {
        &mut self.session_engines
    }

    pub fn surface_engines(&self) -> &SurfaceEngineRegistry {
        &self.surface_engines
    }

    pub fn surface_engines_mut(&mut self) -> &mut SurfaceEngineRegistry {
        &mut self.surface_engines
    }

    /// Advance session-owned time work using the injected clock. `true`
    /// requests another frame because the session has not settled.
    pub fn pump(&mut self) -> bool {
        self.session.pump(self.clock.now_ms());
        !self.session.settled()
    }

    pub fn frame(&mut self, width: u32, height: u32) -> F {
        self.viewport = (width.max(1), height.max(1));
        self.session.frame(self.viewport.0, self.viewport.1)
    }

    pub fn scroll_by(&mut self, dx: f32, dy: f32) -> bool {
        self.session.scroll_by(dx, dy)
    }

    pub fn scroll_at(&mut self, x: f32, y: f32, dx: f32, dy: f32) -> bool {
        self.session.scroll_at(x, y, dx, dy)
    }

    pub fn scroll_for_key(&mut self, key: SessionScrollKey) -> bool {
        self.session.scroll_for_key(key)
    }

    pub fn input(&mut self, input: SessionInput) -> PeltHostEffect {
        let SessionInputResult {
            effect,
            cursor,
            capture,
            editable,
        } = self.session.input(input);
        let mut host_effect = PeltHostEffect {
            handled: effect.is_handled(),
            redraw: matches!(effect, SessionEffect::Handled | SessionEffect::Cancelled),
            cursor,
            pointer_capture: capture,
            editable,
            navigated: false,
            error: None,
        };
        match effect {
            SessionEffect::Navigate(target) => self.navigate_effect(target, &mut host_effect),
            SessionEffect::Submit(submission) => match submission.method {
                SessionFormMethod::Get => {
                    let target = get_submission_target(&submission.action, &submission.fields);
                    self.navigate_effect(target, &mut host_effect);
                },
                SessionFormMethod::Post => {
                    host_effect.error = Some(
                        "POST form submission needs an injected request-body transport".to_owned(),
                    );
                },
            },
            SessionEffect::Ignored | SessionEffect::Handled | SessionEffect::Cancelled => {},
        }
        host_effect
    }

    pub fn command(&mut self, command: SessionNavigationCommand) -> PeltHostEffect {
        let mut host_effect = PeltHostEffect::default();
        match command {
            SessionNavigationCommand::Address(address) => {
                self.navigate_effect(address, &mut host_effect);
            },
            SessionNavigationCommand::Reload => {
                let request = self.history[self.history_index].clone();
                match self.spawn(&request) {
                    Ok(session) => {
                        self.session = session;
                        host_effect.handled = true;
                        host_effect.redraw = true;
                        host_effect.navigated = true;
                    },
                    Err(error) => host_effect.error = Some(error),
                }
            },
            SessionNavigationCommand::Back => {
                if self.can_go_back() {
                    self.traverse_to(self.history_index - 1, &mut host_effect);
                }
            },
            SessionNavigationCommand::Forward => {
                if self.can_go_forward() {
                    self.traverse_to(self.history_index + 1, &mut host_effect);
                }
            },
            SessionNavigationCommand::Stop => {
                // The current registry spawn contract is synchronous. Stop is
                // consumed here; cancellable transport remains a separate seam.
                host_effect.handled = true;
            },
        }
        host_effect
    }

    fn navigate_effect(&mut self, target: String, host_effect: &mut PeltHostEffect) {
        let target = resolve_href(self.address(), &target);
        let request =
            SessionSpawnRequest::new(target).with_viewport(self.viewport.0, self.viewport.1);
        match self.spawn(&request) {
            Ok(session) => {
                self.session = session;
                self.history.truncate(self.history_index + 1);
                self.history.push(request);
                self.history_index += 1;
                host_effect.handled = true;
                host_effect.redraw = true;
                host_effect.navigated = true;
                host_effect.editable = false;
            },
            Err(error) => host_effect.error = Some(error),
        }
    }

    fn traverse_to(&mut self, index: usize, host_effect: &mut PeltHostEffect) {
        let request = self.history[index].clone();
        match self.spawn(&request) {
            Ok(session) => {
                self.session = session;
                self.history_index = index;
                host_effect.handled = true;
                host_effect.redraw = true;
                host_effect.navigated = true;
                host_effect.editable = false;
            },
            Err(error) => host_effect.error = Some(error),
        }
    }

    fn spawn(&self, request: &SessionSpawnRequest) -> Result<Box<dyn DocumentSession<F>>, String> {
        let mut request = request.clone();
        request.viewport = self.viewport;
        self.session_engines
            .spawn(&self.engine_id, &request)
            .map_err(|error| format!("could not load {}: {error}", request.address))
    }
}

fn get_submission_target(action: &str, fields: &[(String, String)]) -> String {
    if fields.is_empty() {
        return action.to_owned();
    }
    let query = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(
            fields
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str())),
        )
        .finish();
    let (base, fragment) = action
        .split_once('#')
        .map_or((action, None), |(base, fragment)| (base, Some(fragment)));
    let separator = if base.contains('?') { '&' } else { '?' };
    let mut target = format!("{base}{separator}{query}");
    if let Some(fragment) = fragment {
        target.push('#');
        target.push_str(fragment);
    }
    target
}
