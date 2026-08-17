//! [`Harness`]: the host with no window and no GPU.
//!
//! Every receipt this host owes — "a click on the element labelled *Install*
//! advances the page", "Tab reaches the controls in this order", "a screen
//! reader's Focus request does not activate the control" — is about routing,
//! not about pixels. Routing needs a real retained DOM and a real laid-out
//! tree, and needs neither a window nor a swapchain.
//!
//! So the harness is the same [`Host`], constructed with `window: None`, driven
//! through the same `click` / `key` / `wheel` / `relayout` methods the winit
//! event loop calls. A test that passes here exercised production routing; it
//! did not re-implement it. That is deliberately the opposite of the usual
//! "mock the host" arrangement, where the tested path is the one nobody ships.
//!
//! ```ignore
//! let mut h = Harness::new(sheet, state, logic);
//! h.layout_at(800.0, 600.0);
//! assert!(h.click_on(&Selector::role("button").containing("Install")));
//! assert_eq!(h.state().stage, Stage::Install);
//! ```
//!
//! Applications get it too: it is how `signalman-desktop` proves its page
//! states and its keyboard order without a display.

use crate::decorations::Decorations;
use cambium_winit_a11y::{A11yAction, A11yRequest};
use genet_probe::{ProbeSurface, Selector, resolve};
use genet_scripted_dom::NodeId;
use winit::keyboard::{Key as WinitKey, NamedKey};

use crate::meristem_bounds::RootView;
use crate::{
    CloseRequest, Host, HostHooks, HostOptions, HostState, HostWake, Init, KeyPress, Runner,
    WindowCommands,
};

/// A windowless host for deterministic tests. See the module docs.
pub struct Harness<State: 'static, Logic, V>
where
    Logic: FnMut(&State) -> V,
    V: RootView<State>,
{
    host: Host<State, Logic, V>,
}

/// Hooks that do nothing — the starting point for a test that only cares about
/// routing. Override the fields you need.
pub fn inert_hooks<State: 'static, Logic, V>() -> HostHooks<State, Logic, V>
where
    Logic: FnMut(&State) -> V + 'static,
    V: RootView<State>,
{
    HostHooks {
        frame: Box::new(|_| false),
        after_dispatch: Box::new(|_| {}),
        after_frame: Box::new(|_| {}),
        after_wake: Box::new(|_| {}),
        close_request: Box::new(|_, _| crate::CloseDisposition::Exit),
        focused_text: Box::new(|_| None),
        key_intercept: Box::new(|_, _| false),
    }
}

impl<State, Logic, V> Harness<State, Logic, V>
where
    State: 'static,
    Logic: FnMut(&State) -> V + 'static,
    V: RootView<State>,
{
    /// Build a harness over an application's real state, view logic, and sheet,
    /// with inert hooks.
    pub fn new(sheet: impl Into<String>, state: State, logic: Logic) -> Self {
        Self::with_hooks(
            Init {
                state,
                logic,
                sheet: sheet.into(),
            },
            inert_hooks(),
        )
    }

    /// Build a harness whose state receives the window-verb handle, mirroring
    /// what [`run`](crate::run) hands `init`.
    ///
    /// Without this an application that stores a [`WindowCommands`] in its
    /// state would construct its own under test, orphaned from the host's —
    /// so every window verb it queued would vanish and the test would prove
    /// the opposite of the truth.
    pub fn with_commands(
        sheet: impl Into<String>,
        logic: Logic,
        make_state: impl FnOnce(&WindowCommands) -> State,
    ) -> Self {
        let commands = WindowCommands::new();
        let state = make_state(&commands);
        let mut harness = Self::with_hooks(
            Init {
                state,
                logic,
                sheet: sheet.into(),
            },
            inert_hooks(),
        );
        harness.host.commands = commands;
        harness
    }

    /// The host's end of the window-verb seam.
    pub fn commands(&self) -> WindowCommands {
        self.host.commands.clone()
    }

    /// The thread-safe wake handle an actor or worker would receive from the
    /// headed host. Call [`process_wake`](Self::process_wake) to give the
    /// windowless event loop its turn.
    pub fn wake(&self) -> HostWake {
        self.host.wake.clone()
    }

    /// Process one queued application wake, then relayout as the headed host's
    /// redraw would. Returns whether a wake was pending.
    pub fn process_wake(&mut self) -> bool {
        let woke = self.host.process_wake();
        if woke {
            self.relayout();
        }
        woke
    }

    /// Deliver a native or application-requested close through the same policy
    /// hook the headed host uses.
    pub fn request_close(&mut self, request: CloseRequest) {
        self.host.request_close(request);
    }

    /// Whether the close policy hid the window while retaining the host state.
    pub fn hidden(&self) -> bool {
        self.host.hidden
    }

    /// Run the ordinary post-dispatch path, including any queued window
    /// commands. This is useful when a test needs to prove an app-command
    /// close shares the native close policy.
    pub fn after_dispatch(&mut self) {
        self.host.after_dispatch();
    }

    /// Build a harness with the application's own hooks — the form a consumer
    /// uses when the behavior under test includes its `after_dispatch` sync or
    /// its `focused_text` seam.
    ///
    /// `AppCtx::window` is `None` throughout, so a hook that asks for window
    /// chrome simply finds no window; nothing else differs from a real run.
    pub fn with_hooks(init: Init<State, Logic>, hooks: HostHooks<State, Logic, V>) -> Self {
        Self::with_hooks_and_options(init, hooks, HostOptions::default())
    }

    /// [`with_hooks`](Self::with_hooks) with the host options spelled out — for
    /// behaviour a test needs to configure, such as `spatial_focus`.
    pub fn with_hooks_and_options(
        init: Init<State, Logic>,
        hooks: HostHooks<State, Logic, V>,
        options: HostOptions,
    ) -> Self {
        let Init {
            state,
            logic,
            sheet,
        } = init;
        let mut s = HostState::new();
        let dom = std::rc::Rc::new(std::cell::RefCell::new(
            genet_scripted_dom::ScriptedDom::new(),
        ));
        s.sheet = sheet;
        s.runner = Some(Runner::new(dom, logic, state));
        let wake = HostWake::new(s.wake_pending.clone(), std::sync::Arc::new(|| {}));
        Self {
            host: Host {
                options,
                init: None,
                hooks,
                s,
                resize_hint: None,
                commands: crate::WindowCommands::new(),
                cadence: crate::decorations::ClickCadence::new(),
                performed: Vec::new(),
                native_window: None,
                hidden: false,
                wake,
            },
        }
    }

    /// Lay out at a logical size. Call after any state change that should be
    /// visible to hit testing — the winit host does this once per frame, so a
    /// test does it once per step.
    pub fn layout_at(&mut self, width: f32, height: f32) {
        self.host.relayout(width, height);
    }

    /// Re-lay out at the size already in use.
    pub fn relayout(&mut self) {
        let (w, h) = self.host.s.layout_size;
        if w > 0.0 && h > 0.0 {
            self.host.relayout(w, h);
        }
    }

    /// The application state the runner owns.
    pub fn state(&self) -> &State {
        self.runner().state()
    }

    /// Mutate the state through the runner (rebuilding the view) and re-lay
    /// out — the test-side equivalent of an action a handler would take.
    pub fn update(&mut self, f: impl FnOnce(&mut State)) {
        if let Some(runner) = self.host.s.runner.as_mut() {
            runner.update(f);
        }
        self.relayout();
    }

    /// How far every nested scroll container is scrolled, summed. A blunt
    /// single number on purpose: a scrolling assertion wants "did the host's
    /// default run", not which container absorbed it.
    pub fn element_scroll_total(&self) -> f32 {
        let Some(layout) = self.host.s.layout.as_ref() else {
            return 0.0;
        };
        let element: f32 = layout
            .element_scroll()
            .values()
            .map(|(x, y)| x.abs() + y.abs())
            .sum();
        let (vx, vy) = layout.viewport_scroll();
        element + vx.abs() + vy.abs()
    }

    /// The runner, for assertions the state alone cannot make (focus, capture).
    pub fn runner(&self) -> &Runner<State, Logic, V> {
        self.host.s.runner.as_ref().expect("harness has a runner")
    }

    /// The focused DOM node, if any.
    pub fn focus(&self) -> Option<NodeId> {
        self.runner().focus()
    }

    /// The element currently capturing a pointer drag, if any.
    pub fn pointer_capture(&self) -> Option<NodeId> {
        self.runner().pointer_capture()
    }

    /// The node the cursor is currently over.
    pub fn hit(&self) -> Option<NodeId> {
        self.host.hit_at_cursor()
    }

    /// Where a node actually paints, in the coordinates the cursor uses:
    /// `(x, y, w, h)`. The same rect the host measures pointer-local
    /// coordinates against.
    pub fn painted_rect(&self, node: NodeId) -> Option<(f32, f32, f32, f32)> {
        let layout = self.host.s.layout.as_ref()?;
        let dom = self.runner().dom();
        let dom_ref = dom.borrow();
        layout.painted_rect(&*dom_ref, node)
    }

    /// The cursor position the harness last delivered.
    pub fn cursor(&self) -> (f32, f32) {
        self.host.s.cursor
    }

    /// Run `f` over the retained DOM — for assertions about text, roles, and
    /// attributes that the state does not carry.
    pub fn with_dom<R>(&self, f: impl FnOnce(&genet_scripted_dom::ScriptedDom) -> R) -> R {
        let dom = self.runner().dom();
        let dom_ref = dom.borrow();
        f(&dom_ref)
    }

    /// Present the retained DOM to a `genet-probe` visitor as this app's single
    /// surface, so `resolve` / `text_present` work against it unchanged.
    pub fn with_surfaces<R>(&self, f: impl FnOnce(&[ProbeSurface<'_>]) -> R) -> R {
        let dom = self.runner().dom();
        let dom_ref = dom.borrow();
        let (w, h) = self.host.s.layout_size;
        f(&[ProbeSurface {
            name: "harness",
            dom: &dom_ref,
            rect: [0.0, 0.0, w, h],
            sheet: &self.host.s.sheet,
        }])
    }

    /// Resolve a semantic selector to the centre of the matching element.
    pub fn resolve(&self, selector: &Selector) -> Option<(f32, f32)> {
        self.with_surfaces(|surfaces| resolve(surfaces, selector).map(|hit| hit.point))
    }

    /// Move the cursor: hover restyle, Enter/Leave, captured-drag tracking.
    pub fn move_to(&mut self, x: f32, y: f32) {
        self.host.s.cursor = (x, y);
        self.host.hover();
        self.host.hover_dispatch();
        self.host.pointer_move();
        self.host.drag_text_selection();
    }

    /// Press the left button at a point, through the host's real routing.
    pub fn press_at(&mut self, x: f32, y: f32) {
        self.host.s.cursor = (x, y);
        // The frame-aware path, not bare `click`: a press on a title bar has
        // to reach the same drag/maximize logic the winit host runs, or a
        // receipt proves something the shipping build does not do.
        self.host.press_left();
    }

    /// Press the right button at a point — the system-menu gesture.
    pub fn right_press_at(&mut self, x: f32, y: f32) {
        self.host.s.cursor = (x, y);
        self.host.press_right();
    }

    /// What the window frame makes of a point: whether pressing there drags
    /// the window.
    pub fn app_region_at(&self, x: f32, y: f32) -> crate::AppRegion {
        self.host.app_region_at(x, y)
    }

    /// The window verbs the host has performed, oldest first.
    ///
    /// A windowless harness has no window to minimize, so the verbs are
    /// recorded rather than enacted — which is exactly what a test wants to
    /// assert against.
    pub fn performed(&self) -> &[crate::WindowCommand] {
        &self.host.performed
    }

    /// Release the left button at a point.
    pub fn release_at(&mut self, x: f32, y: f32) {
        self.host.s.cursor = (x, y);
        self.host.release();
    }

    /// Press and release at a point — an ordinary click.
    pub fn click_at(&mut self, x: f32, y: f32) {
        self.press_at(x, y);
        self.release_at(x, y);
    }

    /// Resolve a selector and click it. `false` when nothing matched, so a test
    /// fails on the miss rather than on its consequence.
    pub fn click_on(&mut self, selector: &Selector) -> bool {
        let Some((x, y)) = self.resolve(selector) else {
            return false;
        };
        self.click_at(x, y);
        self.relayout();
        true
    }

    /// Scroll the wheel at the current cursor: view handlers first, then the
    /// host's layout-scrolling default.
    ///
    /// `(dx, dy)` is the **scroll** delta, in the direction the content moves —
    /// a positive `dy` advances toward the end of the document, which is what a
    /// wheel handler and `scroll_at_target` both see. (Winit reports the
    /// opposite sign; the negation here is the same one `wheel_delta_from_winit`
    /// applies, so a test states the delta it means.)
    pub fn wheel(&mut self, dx: f32, dy: f32) {
        self.host.wheel(winit::event::MouseScrollDelta::PixelDelta(
            winit::dpi::PhysicalPosition::new(-dx as f64, -dy as f64),
        ));
        self.relayout();
    }

    /// Set the modifier state subsequent keys are delivered with.
    pub fn set_modifiers(&mut self, modifiers: crate::Modifiers) {
        self.host.s.modifiers = modifiers;
    }

    /// Deliver a key press through the host's real key path, with whatever
    /// modifiers [`set_modifiers`](Self::set_modifiers) last set.
    pub fn key(&mut self, key: WinitKey) {
        let modifiers = self.host.s.modifiers;
        self.press_key(&KeyPress::new(crate::key_from_winit(&key)).with_modifiers(modifiers));
    }

    /// Deliver text the way an on-screen keyboard or a remapper does: the
    /// platform could not name the key, but reported what it produces. On
    /// Windows this is `VK_PACKET`, which every `SendInput`-based assistive
    /// tool uses.
    pub fn key_injected(&mut self, text: &str) {
        let modifiers = self.host.s.modifiers;
        self.press_key(&KeyPress {
            key: crate::Key::Unidentified,
            text: Some(text.to_string()),
            modifiers,
            repeat: false,
        });
    }

    /// Deliver a fully specified key press.
    pub fn press_key(&mut self, press: &KeyPress) {
        self.host.key(press);
        self.relayout();
    }

    /// Deliver a named key (Tab, Enter, ArrowLeft, …).
    pub fn key_named(&mut self, named: NamedKey) {
        self.key(WinitKey::Named(named));
    }

    /// Type a single character.
    pub fn key_char(&mut self, c: &str) {
        self.key(WinitKey::Character(c.into()));
    }

    /// Tap Tab — press and release. Forward, or backward with `Shift`. Routed
    /// through `dispatch_key`, so a view that handles Tab still gets it first.
    pub fn tab(&mut self, forward: bool) {
        self.hold_tab(forward);
        self.release_tab();
    }

    /// Press Tab and keep it down: arrow keys now steer focus spatially, until
    /// [`release_tab`](Self::release_tab). The press itself traverses once,
    /// exactly as a tap does.
    pub fn hold_tab(&mut self, forward: bool) {
        self.press_key(
            &KeyPress::named(crate::NamedKey::Tab).with_modifiers(crate::Modifiers {
                shift: !forward,
                ..crate::Modifiers::NONE
            }),
        );
    }

    /// Let go of Tab, leaving spatial focus navigation.
    pub fn release_tab(&mut self) {
        self.host.s.tab_held = false;
    }

    /// Whether spatial focus navigation is currently engaged.
    pub fn tab_held(&self) -> bool {
        self.host.s.tab_held
    }

    /// Project this frame's layout into an AccessKit tree, exactly as the
    /// accessibility host does, and return it with the id map a drained action
    /// resolves through. No adapter, so no screen reader is required to assert
    /// what one would be told.
    pub fn a11y_tree(
        &mut self,
    ) -> (
        accesskit::TreeUpdate,
        std::collections::HashMap<accesskit::NodeId, NodeId>,
    ) {
        let (Some(runner), Some(layout)) =
            (self.host.s.runner.as_ref(), self.host.s.layout.as_ref())
        else {
            panic!("a11y_tree needs a laid-out harness: call layout_at first");
        };
        let dom = runner.dom();
        let dom_ref = dom.borrow();
        cambium_winit_a11y::project_tree(
            &dom_ref,
            layout,
            &mut self.host.s.leaves,
            self.host.s.last_focus,
        )
    }

    /// The DOM node a projected AccessKit node came from.
    pub fn a11y_dom_node(&mut self, id: accesskit::NodeId) -> Option<NodeId> {
        let (_, map) = self.a11y_tree();
        map.get(&id).copied()
    }

    /// Route a screen-reader request through the host's accessibility path —
    /// the same routing [`Host::sync_a11y`] performs on a drained request,
    /// minus the OS adapter no test can supply.
    pub fn a11y_request(&mut self, action: A11yAction, node: NodeId) {
        self.host
            .apply_a11y_requests(&[A11yRequest { action, node }]);
        self.relayout();
    }

    /// Fire the exact wake callback the host hands the AccessKit adapter — the
    /// signal a screen reader raises when it acts on an idle app.
    pub fn signal_a11y_wake(&self) {
        (self.host.a11y_waker())();
    }

    /// What the event loop would do on its next idle turn. Pairs with
    /// [`signal_a11y_wake`](Self::signal_a11y_wake) to prove the wake reaches
    /// a redraw rather than being swallowed.
    pub fn idle_policy(&mut self) -> crate::IdlePolicy {
        self.host.idle_policy(cambium_rootstock::Instant::now())
    }

    /// Whether the application asked to close (a hook setting `ctx.close`).
    pub fn close_requested(&self) -> bool {
        self.host.s.close_requested
    }

    /// Drain any pointer events an application hook queued, delivering them
    /// through the real input path.
    pub fn drain_pointer(&mut self) {
        self.host.drain_pointer();
    }

    /// Run the application's `after_frame` hook, as a presented frame would.
    pub fn after_frame(&mut self) {
        self.host.with_ctx(crate::Hook::AfterFrame);
    }
}
