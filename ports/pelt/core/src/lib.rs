/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Window-neutral Pelt host controller.

mod workspace;

use std::sync::Arc;

use genet_host_api::resolve_href;
use inker::{
    A11yCapability, ContentReport, DocumentSession, DocumentZoomState, SessionEffect, SessionError,
    SessionFormMethod, SessionInput, SessionInputResult, SessionNavigationCommand, SessionRegistry,
    SessionScrollKey, SessionSpawnRequest, SurfaceEngineRegistry,
};

pub use workspace::{
    PeltRegistries, PeltRouteSource, PeltRouteState, PeltSurfaceLayer, PeltTileFrame,
    PeltTileInspection, PeltTileRequest, PeltTileRoute, PeltWorkspace, PeltWorkspaceFrame,
    WorkspaceRect,
};

/// Host-neutral state for a controller's document presentation.
///
/// A controller spawn is synchronous today, so [`Self::Loading`] does not
/// describe transport progress. It records that a replacement session needs
/// one host-composed frame before the host can call
/// [`PeltController::mark_document_presented`]. A failed replacement leaves
/// the current session and history intact while exposing the attempted address
/// and error to the host's own diagnostic document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PeltDocumentState {
    Ready,
    Loading { address: String },
    Error { address: String, message: String },
}

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
/// `session_engines`. The routed workspace shares both registry pairs across
/// its controllers and owns surface producers beside them. Frames remain
/// generic, so the embedding host owns every wgpu resource and presentation
/// target.
pub struct PeltController<F> {
    session_engines: Arc<SessionRegistry<F>>,
    surface_engines: Arc<SurfaceEngineRegistry>,
    engine_id: String,
    session: Box<dyn DocumentSession<F>>,
    history: Vec<SessionSpawnRequest>,
    history_index: usize,
    viewport: (u32, u32),
    clock: Box<dyn PeltClock>,
    document_state: PeltDocumentState,
    session_generation: u64,
}

impl<F: 'static> PeltController<F> {
    pub fn new(
        session_engines: SessionRegistry<F>,
        surface_engines: SurfaceEngineRegistry,
        config: PeltControllerConfig,
        clock: impl PeltClock,
    ) -> Result<Self, String> {
        Self::new_shared(
            Arc::new(session_engines),
            Arc::new(surface_engines),
            config,
            clock,
        )
    }

    /// Spawn a controller from host-long-lived registries. Every tile keeps its
    /// own session and history while sharing the immutable engine factories.
    pub fn new_shared(
        session_engines: Arc<SessionRegistry<F>>,
        surface_engines: Arc<SurfaceEngineRegistry>,
        config: PeltControllerConfig,
        clock: impl PeltClock,
    ) -> Result<Self, String> {
        Self::new_shared_boxed(session_engines, surface_engines, config, Box::new(clock))
    }

    pub(crate) fn new_shared_boxed(
        session_engines: Arc<SessionRegistry<F>>,
        surface_engines: Arc<SurfaceEngineRegistry>,
        config: PeltControllerConfig,
        clock: Box<dyn PeltClock>,
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
            clock,
            document_state: PeltDocumentState::Ready,
            // Generation zero is reserved as "no successfully opened
            // session" for hosts that retain child trees across tiles.
            session_generation: 1,
        })
    }

    pub fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub fn address(&self) -> &str {
        &self.history[self.history_index].address
    }

    pub fn request(&self) -> &SessionSpawnRequest {
        &self.history[self.history_index]
    }

    pub fn title(&self) -> Option<String> {
        self.session.inspect().and_then(|report| report.title)
    }

    pub fn inspect(&self) -> Option<ContentReport> {
        self.session.inspect()
    }

    /// The current host-neutral document presentation state.
    pub fn document_state(&self) -> &PeltDocumentState {
        &self.document_state
    }

    /// Identity generation of the current successfully opened session.
    ///
    /// This starts at one for the initial session and increases only after a
    /// replacement session has been successfully spawned and installed. Hosts
    /// that retain concrete session observations, such as accessibility child
    /// trees, pair this value with their tile identity and discard cached
    /// observations when it changes.
    pub fn session_generation(&self) -> u64 {
        self.session_generation
    }

    /// The active document session's concrete observation surface.
    ///
    /// Engine-specific behavior stays behind [`DocumentSession`]. This is for
    /// host-owned observation of a public concrete session type, paired with
    /// [`Self::session_generation`] so retained host state never outlives a
    /// successful replacement session.
    pub fn session_as_any_ref(&self) -> &dyn std::any::Any {
        self.session.as_any_ref()
    }

    /// The active document session's narrowly scoped concrete mutation surface.
    ///
    /// Engine-specific behavior stays behind [`DocumentSession`]. A host that
    /// owns a typed action route pairs a downcast here with tile identity and
    /// [`Self::session_generation`] so a stale retained node cannot mutate a
    /// replacement session.
    pub fn session_as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self.session.as_any()
    }

    /// Request an engine-owned page zoom through Pelt's neutral session seam.
    ///
    /// The active engine reports its supported bounds and applied factor. Pelt
    /// neither chooses a zoom policy nor reaches into an engine concrete type.
    pub fn set_page_zoom(&mut self, factor: f32) -> Result<DocumentZoomState, SessionError> {
        self.session.set_page_zoom(factor)
    }

    /// Mark a successfully replaced document as visibly composed by its host.
    ///
    /// Pelt deliberately does not choose a presentation loop. An embedding host
    /// calls this only after it has composed the replacement session, preserving
    /// one deterministic loading-document frame without pretending the current
    /// synchronous registry spawn is asynchronous transport.
    pub fn mark_document_presented(&mut self) {
        if matches!(self.document_state, PeltDocumentState::Loading { .. }) {
            self.document_state = PeltDocumentState::Ready;
        }
    }

    /// The semantic capability declared by this controller's active document
    /// engine. Construction requires that engine to remain in the immutable
    /// shared registry, so this cannot silently degrade after a session opens.
    pub fn a11y_capability(&self) -> A11yCapability {
        self.session_engines
            .get(&self.engine_id)
            .expect("a live Pelt controller keeps its registered session engine")
            .a11y_capability()
    }

    /// Semantic clip from the current retained selection or document.
    ///
    /// Product receipts use this to verify engine-owned selection without
    /// reaching through the controller to a concrete document session.
    pub fn clip(&self) -> Option<inker::DocumentClip> {
        self.session.clip()
    }

    /// Links in the current retained frame, in document-local coordinates.
    /// Product hosts use this for semantic receipts and accessibility-driven
    /// activation without reaching through the controller to a concrete engine.
    pub fn links(&self) -> Vec<inker::SessionLink> {
        self.session.links()
    }

    /// Resolve retained text to document-local pointer endpoints without
    /// exposing the concrete document engine's DOM or layout identities.
    pub fn text_target(&self, text: &str) -> Option<inker::SessionTextTarget> {
        self.session.text_target(text)
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

    /// Mutate an owned registry before sharing it. Returns `None` once another
    /// controller shares the registry.
    pub fn session_engines_mut(&mut self) -> Option<&mut SessionRegistry<F>> {
        Arc::get_mut(&mut self.session_engines)
    }

    pub fn surface_engines(&self) -> &SurfaceEngineRegistry {
        &self.surface_engines
    }

    /// Mutate an owned registry before sharing it. See
    /// [`Self::session_engines_mut`] for the routed-workspace rule.
    pub fn surface_engines_mut(&mut self) -> Option<&mut SurfaceEngineRegistry> {
        Arc::get_mut(&mut self.surface_engines)
    }

    /// Whether two live controllers use the same host registry pair.
    pub fn shares_registries_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.session_engines, &other.session_engines)
            && Arc::ptr_eq(&self.surface_engines, &other.surface_engines)
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

    /// Tell the retained session whether its tile is currently visible.
    /// Hidden sessions keep their navigation and scroll state while avoiding
    /// work that only contributes to a visible frame.
    pub fn set_hidden(&mut self, hidden: bool) {
        self.session.set_hidden(hidden);
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
                    let address = self.address().to_owned();
                    self.document_error(
                        address,
                        "POST form submission needs an injected request-body transport".to_owned(),
                        &mut host_effect,
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
                        self.install_session(session);
                        self.document_state = PeltDocumentState::Loading {
                            address: request.address.clone(),
                        };
                        host_effect.handled = true;
                        host_effect.redraw = true;
                        host_effect.navigated = true;
                    },
                    Err(error) => self.document_error(request.address, error, &mut host_effect),
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
                let address = request.address.clone();
                self.install_session(session);
                self.history.truncate(self.history_index + 1);
                self.history.push(request);
                self.history_index += 1;
                self.document_state = PeltDocumentState::Loading { address };
                host_effect.handled = true;
                host_effect.redraw = true;
                host_effect.navigated = true;
                host_effect.editable = false;
            },
            Err(error) => self.document_error(request.address, error, host_effect),
        }
    }

    fn traverse_to(&mut self, index: usize, host_effect: &mut PeltHostEffect) {
        let request = self.history[index].clone();
        match self.spawn(&request) {
            Ok(session) => {
                self.install_session(session);
                self.history_index = index;
                self.document_state = PeltDocumentState::Loading {
                    address: request.address.clone(),
                };
                host_effect.handled = true;
                host_effect.redraw = true;
                host_effect.navigated = true;
                host_effect.editable = false;
            },
            Err(error) => self.document_error(request.address, error, host_effect),
        }
    }

    fn spawn(&self, request: &SessionSpawnRequest) -> Result<Box<dyn DocumentSession<F>>, String> {
        let mut request = request.clone();
        request.viewport = self.viewport;
        self.session_engines
            .spawn(&self.engine_id, &request)
            .map_err(|error| format!("could not load {}: {error}", request.address))
    }

    /// Install a session only after its factory has completed successfully.
    /// Failed replacement attempts retain both this session and its generation.
    fn install_session(&mut self, session: Box<dyn DocumentSession<F>>) {
        self.session = session;
        self.session_generation = self
            .session_generation
            .checked_add(1)
            .expect("Pelt session generation exhausted");
    }

    fn document_error(
        &mut self,
        address: String,
        message: String,
        host_effect: &mut PeltHostEffect,
    ) {
        self.document_state = PeltDocumentState::Error {
            address,
            message: message.clone(),
        };
        host_effect.error = Some(message);
        // The host-owned diagnostic document is a visible transition even
        // though the active session and history stay unchanged.
        host_effect.handled = true;
        host_effect.redraw = true;
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
