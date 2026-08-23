/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Retained navigation state around one engine-owned document session.
//!
//! The controller owns replacement-session spawning and history. It consumes
//! Inker's host-neutral effects, leaving the winit shell to translate platform
//! events without downcasting or learning a concrete document type.

use inker::{
    DocumentSession, SessionEffect, SessionFormMethod, SessionInput, SessionInputResult,
    SessionNavigationCommand, SessionRegistry, SessionScrollKey, SessionSpawnRequest,
};

use crate::static_viewer::windowed::ViewerAction;

pub(crate) struct BrowserSession<F> {
    registry: SessionRegistry<F>,
    engine_id: String,
    session: Box<dyn DocumentSession<F>>,
    history: Vec<String>,
    history_index: usize,
    viewport: (u32, u32),
}

impl<F: 'static> BrowserSession<F> {
    pub(crate) fn new(
        registry: SessionRegistry<F>,
        engine_id: impl Into<String>,
        address: impl Into<String>,
        viewport: (u32, u32),
    ) -> Result<Self, String> {
        let engine_id = engine_id.into();
        let address = address.into();
        let session = registry
            .spawn(
                &engine_id,
                &SessionSpawnRequest::new(&address).with_viewport(viewport.0, viewport.1),
            )
            .map_err(|error| format!("could not spawn engine {engine_id}: {error}"))?;
        Ok(Self {
            registry,
            engine_id,
            session,
            history: vec![address],
            history_index: 0,
            viewport,
        })
    }

    pub(crate) fn address(&self) -> &str {
        &self.history[self.history_index]
    }

    pub(crate) fn title(&self) -> Option<String> {
        self.session.inspect().and_then(|report| report.title)
    }

    pub(crate) fn frame(&mut self, width: u32, height: u32) -> F {
        self.viewport = (width, height);
        self.session.frame(width, height)
    }

    pub(crate) fn scroll_by(&mut self, dx: f32, dy: f32) -> bool {
        self.session.scroll_by(dx, dy)
    }

    pub(crate) fn scroll_at(&mut self, x: f32, y: f32, dx: f32, dy: f32) -> bool {
        self.session.scroll_at(x, y, dx, dy)
    }

    pub(crate) fn scroll_for_key(&mut self, key: SessionScrollKey) -> bool {
        self.session.scroll_for_key(key)
    }

    pub(crate) fn input(&mut self, input: SessionInput) -> ViewerAction {
        let SessionInputResult {
            effect,
            cursor,
            capture,
            editable,
        } = self.session.input(input);
        let mut action = ViewerAction {
            handled: effect.is_handled(),
            redraw: matches!(effect, SessionEffect::Handled | SessionEffect::Cancelled),
            cursor,
            capture,
            editable,
            navigated: false,
            error: None,
        };
        match effect {
            SessionEffect::Navigate(target) => self.navigate_effect(target, &mut action),
            SessionEffect::Submit(submission) => match submission.method {
                SessionFormMethod::Get => {
                    let target = get_submission_target(&submission.action, &submission.fields);
                    self.navigate_effect(target, &mut action);
                },
                SessionFormMethod::Post => {
                    action.error = Some(
                        "POST form submission needs an injected request-body transport".to_owned(),
                    );
                },
            },
            SessionEffect::Ignored | SessionEffect::Handled | SessionEffect::Cancelled => {},
        }
        action
    }

    pub(crate) fn command(&mut self, command: SessionNavigationCommand) -> ViewerAction {
        let mut action = ViewerAction::default();
        match command {
            SessionNavigationCommand::Address(address) => {
                self.navigate_effect(address, &mut action);
            },
            SessionNavigationCommand::Reload => {
                let address = self.address().to_owned();
                match self.spawn(&address) {
                    Ok(session) => {
                        self.session = session;
                        action.handled = true;
                        action.redraw = true;
                        action.navigated = true;
                    },
                    Err(error) => action.error = Some(error),
                }
            },
            SessionNavigationCommand::Back => {
                if self.history_index > 0 {
                    self.traverse_to(self.history_index - 1, &mut action);
                }
            },
            SessionNavigationCommand::Forward => {
                if self.history_index + 1 < self.history.len() {
                    self.traverse_to(self.history_index + 1, &mut action);
                }
            },
            SessionNavigationCommand::Stop => {
                // Session spawning is synchronous today, so there is no
                // in-flight request to cancel. Keeping Stop as a handled host
                // command makes the contract ready for the async transport lane.
                action.handled = true;
            },
        }
        action
    }

    fn navigate_effect(&mut self, target: String, action: &mut ViewerAction) {
        let target = genet_documents::resolve_href(self.address(), &target);
        match self.spawn(&target) {
            Ok(session) => {
                self.session = session;
                self.history.truncate(self.history_index + 1);
                self.history.push(target);
                self.history_index += 1;
                action.handled = true;
                action.redraw = true;
                action.navigated = true;
                action.editable = false;
            },
            Err(error) => action.error = Some(error),
        }
    }

    fn traverse_to(&mut self, index: usize, action: &mut ViewerAction) {
        let address = self.history[index].clone();
        match self.spawn(&address) {
            Ok(session) => {
                self.session = session;
                self.history_index = index;
                action.handled = true;
                action.redraw = true;
                action.navigated = true;
                action.editable = false;
            },
            Err(error) => action.error = Some(error),
        }
    }

    fn spawn(&self, address: &str) -> Result<Box<dyn DocumentSession<F>>, String> {
        self.registry
            .spawn(
                &self.engine_id,
                &SessionSpawnRequest::new(address).with_viewport(self.viewport.0, self.viewport.1),
            )
            .map_err(|error| format!("could not load {address}: {error}"))
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

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::sync::{Arc, Mutex};

    use inker::{
        DocumentSession, SessionButtonState, SessionClick, SessionEngine, SessionError,
        SessionFormSubmission, SessionInput, SessionModifiers, SessionPointerButton,
        SessionSpawnRequest,
    };

    use super::*;

    struct FakeEngine {
        spawns: Arc<Mutex<Vec<String>>>,
    }

    impl SessionEngine<String> for FakeEngine {
        fn engine_id(&self) -> &str {
            "fake"
        }

        fn spawn(
            &self,
            request: &SessionSpawnRequest,
        ) -> Result<Box<dyn DocumentSession<String>>, SessionError> {
            self.spawns.lock().unwrap().push(request.address.clone());
            Ok(Box::new(FakeSession(request.address.clone())))
        }
    }

    struct FakeSession(String);

    impl DocumentSession<String> for FakeSession {
        fn frame(&mut self, _width: u32, _height: u32) -> String {
            self.0.clone()
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

    fn browser() -> (BrowserSession<String>, Arc<Mutex<Vec<String>>>) {
        let spawns = Arc::new(Mutex::new(Vec::new()));
        let mut registry = SessionRegistry::new();
        registry.register(Box::new(FakeEngine {
            spawns: spawns.clone(),
        }));
        (
            BrowserSession::new(registry, "fake", "docs/index.html", (800, 600)).unwrap(),
            spawns,
        )
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
    fn link_navigation_reload_and_history_replace_sessions() {
        let (mut browser, spawns) = browser();
        assert!(browser.input(press(1.0)).navigated);
        assert_eq!(browser.address(), "docs/next.html");
        assert!(browser.command(SessionNavigationCommand::Reload).navigated);
        assert!(browser.command(SessionNavigationCommand::Back).navigated);
        assert_eq!(browser.address(), "docs/index.html");
        assert!(browser.command(SessionNavigationCommand::Forward).navigated);
        assert_eq!(browser.address(), "docs/next.html");
        assert_eq!(
            spawns.lock().unwrap().as_slice(),
            [
                "docs/index.html",
                "docs/next.html",
                "docs/next.html",
                "docs/index.html",
                "docs/next.html",
            ]
        );
    }

    #[test]
    fn address_navigation_truncates_forward_history() {
        let (mut browser, _) = browser();
        assert!(browser.input(press(1.0)).navigated);
        assert!(browser.command(SessionNavigationCommand::Back).navigated);
        assert!(
            browser
                .command(SessionNavigationCommand::Address(
                    "replacement.html".to_owned()
                ))
                .navigated
        );
        assert_eq!(browser.address(), "docs/replacement.html");
        assert!(!browser.command(SessionNavigationCommand::Forward).handled);
    }

    #[test]
    fn get_form_submission_is_encoded_then_navigated() {
        let (mut browser, _) = browser();
        let action = browser.input(press(20.0));
        assert!(action.navigated);
        assert_eq!(browser.address(), "docs/result.html?note=cedar+%26+ash");
    }
}
