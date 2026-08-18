//! The single-root Cambium desktop host.
//!
//! Every Cambium desktop application so far has hand-assembled the same
//! machinery: a winit `ApplicationHandler` owning one window, a genet
//! [`SurfaceHost`] presenting a `netrender` scene, a retained
//! [`IncrementalLayout`] over the runner's `ScriptedDom`, logical-coordinate
//! hit testing, pointer/keyboard/IME/wheel routing into a
//! [`GenetAppRunner`], overlay-scrollbar fade policy, and the
//! [`A11yHost`] install-before-show lifecycle. This crate is that machinery,
//! extracted once, from the woodshed-genet donor.
//!
//! Deliberately **not** here, per the Signalman desktop scope (retinue,
//! 2026-08-09): no async runtime, no application trait, no multi-window,
//! docking, navigation, persistence, or command system. The application
//! supplies plain closures ([`HostHooks`]) and keeps its own state in their
//! environment; the host owns lifecycle, layout, paint, input routing, and
//! accessibility synchronization, and nothing above them.
//!
//! ```ignore
//! let options = HostOptions { title: "App".into(), ..Default::default() };
//! run(options, |window, commands, wake| Init { state, logic, sheet }, hooks)
//! ```

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cambium::{GenetAppRunner, TextInput};
use cambium_winit::ScrollbarFade;
use cambium_winit_a11y::A11yHost;
use genet_layout::{IncrementalLayout, ScrollTarget};
use genet_scripted_dom::{NodeId, ScriptedDom};
use genet_winit_host::SurfaceHost;
use meristem_bounds::RootView;
use netrender::NetrenderOptions;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowId};

mod capture;
mod decorations;
mod frame;
mod harness;
mod input;
mod spatial;
mod wake;

pub use capture::{Frame, read_frame};
pub use decorations::{AppRegion, WindowCommand, WindowCommands, WindowGeometry};
pub use harness::{Harness, inert_hooks};
pub use spatial::Direction;
pub use wake::HostWake;

use decorations::ClickCadence;

mod a11y;

/// An application-level close request. Native window chrome and an app's own
/// Close command deliberately use the same path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseRequest {
    /// The operating system asked the window to close.
    Native,
    /// The application queued [`WindowCommand::Close`].
    Command,
}

/// What the application decided to do with a [`CloseRequest`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseDisposition {
    /// Keep the window visible and keep the event loop alive.
    KeepVisible,
    /// Hide the window but keep the event loop and application state alive.
    Hide,
    /// End the event loop.
    Exit,
}

#[derive(Clone, Copy, Debug)]
enum HostEvent {
    Wake,
}

/// The bound every root view satisfies. A module rather than a trait alias so
/// the host's signatures stay readable: `V: RootView<State>` means
/// `meristem::View<State, (), GenetCtx, Element = GenetElement>`.
mod meristem_bounds {
    use cambium::{GenetCtx, GenetElement};
    use meristem::View;

    pub trait RootView<State: 'static>:
        View<State, (), GenetCtx, Element = GenetElement> + 'static
    {
    }
    impl<State: 'static, V> RootView<State> for V where
        V: View<State, (), GenetCtx, Element = GenetElement> + 'static
    {
    }
}

/// The runner shape this host drives: one state, one tree, unit actions.
pub type Runner<State, Logic, V> = GenetAppRunner<State, Logic, V, ()>;

/// A capture armed by the application, run inside the next frame while the
/// rasterized view is still alive (scenario screenshots).
pub type CaptureFn = Box<dyn FnOnce(&SurfaceHost, &wgpu::TextureView, u32, u32) + 'static>;

/// Window and pipeline configuration.
pub struct HostOptions {
    /// The window title.
    pub title: String,
    /// `false` for client-side decorations: the application draws its own
    /// chrome, and the host supplies edge-resize grab margins and cursors.
    pub decorations: bool,
    /// Logical size to open at when no environment override is present.
    pub initial_logical_size: (f64, f64),
    /// Environment variable names overriding the logical width and height,
    /// for reproducible scenario receipts (`WOODSHED_WIDTH`-shaped).
    pub size_env: Option<(String, String)>,
    /// Renderer options for [`SurfaceHost::boot`]. A factory rather than a
    /// value: `NetrenderOptions` is not `Clone`, and the surface is booted
    /// again every time the platform takes it away and hands it back
    /// (suspend/resume), which must not silently fall back to defaults.
    pub netrender: Box<dyn Fn() -> NetrenderOptions>,
    /// Hold Tab and steer focus with the arrow keys (see [`spatial`]).
    ///
    /// On by default: it is additive, costs nothing until Tab is *held*, and
    /// tapping Tab behaves exactly as it always did. Turn it off for an
    /// application whose arrow keys mean something while a control is focused
    /// and which would rather not share them.
    ///
    /// [`spatial`]: crate::Direction
    pub spatial_focus: bool,
}

impl Default for HostOptions {
    fn default() -> Self {
        Self {
            title: String::new(),
            decorations: true,
            initial_logical_size: (1_100.0, 664.0),
            size_env: None,
            netrender: Box::new(|| NetrenderOptions {
                tile_cache_size: Some(1024),
                enable_vello: true,
                ..Default::default()
            }),
            spatial_focus: true,
        }
    }
}

/// The focused text field, as the application maps it: which DOM node carries
/// the caret, and how to reach the [`TextInput`] inside the state. Returning
/// `Some` enables IME, the caret/selection overlay, visual caret movement,
/// and drag selection for that node.
pub struct FocusedTextSlot<State> {
    /// The focused `<input>`-carrying node.
    pub node: NodeId,
    /// Borrow the field's [`TextInput`] from the state.
    pub get: Box<dyn Fn(&State) -> &TextInput>,
    /// Borrow it mutably.
    pub get_mut: Box<dyn Fn(&mut State) -> &mut TextInput>,
}

/// A key press as the host routes it: winit's logical key, plus the modifier
/// state at the time.
///
/// Small on purpose. `winit::event::KeyEvent` cannot be constructed outside
/// winit, so a host whose keyboard path took one could never be driven from a
/// test — and a keyboard-order receipt that cannot run in `cargo test` is a
/// receipt nobody collects. This carries exactly what routing reads.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyPress {
    /// The logical key, after the layout and modifiers the OS applied.
    pub key: winit::keyboard::Key,
    /// The text this press produces, when the platform reports any.
    ///
    /// Carried because `key` alone is not always enough. Windows delivers
    /// injected text as `VK_PACKET`, which winit surfaces as
    /// `Key::Unidentified` — and injected text is not an exotic case: on-screen
    /// keyboards, keyboard remappers, and several assistive input tools all
    /// type that way. Without this field a person using one of them cannot type
    /// into the application at all.
    pub text: Option<String>,
    /// Modifiers held at the time of the press.
    pub modifiers: ModifiersState,
    /// Whether this is an auto-repeat rather than a fresh press.
    pub repeat: bool,
}

impl KeyPress {
    /// A press with no modifiers held.
    pub fn new(key: winit::keyboard::Key) -> Self {
        let text = match &key {
            winit::keyboard::Key::Character(c) => Some(c.to_string()),
            _ => None,
        };
        Self {
            key,
            text,
            modifiers: ModifiersState::empty(),
            repeat: false,
        }
    }

    /// A named-key press (Tab, Enter, ArrowLeft, …).
    pub fn named(named: winit::keyboard::NamedKey) -> Self {
        Self::new(winit::keyboard::Key::Named(named))
    }

    /// Hold these modifiers for the press.
    #[must_use]
    pub fn with_modifiers(mut self, modifiers: ModifiersState) -> Self {
        self.modifiers = modifiers;
        self
    }

    /// The character this press should insert when the platform could not name
    /// the key but did report text. `None` for a named key, a control
    /// character, or a chord — a shortcut must not become typed text.
    pub fn injected_text(&self) -> Option<&str> {
        if !matches!(self.key, winit::keyboard::Key::Unidentified(_)) {
            return None;
        }
        // A modifier chord is a command, not typing.
        if self.modifiers.control_key() || self.modifiers.super_key() {
            return None;
        }
        let text = self.text.as_deref()?;
        if text.is_empty() || text.chars().any(char::is_control) {
            return None;
        }
        Some(text)
    }
}

/// A pointer event an application asks the host to deliver to itself, in
/// logical window coordinates.
///
/// The host owns hit testing, capture, and the dispatch order, so an
/// application that drives itself — a `genet-probe` scenario clicking a
/// resolved element, a demo replaying a gesture — must not re-roll that
/// routing. It queues one of these instead and the host runs it through the
/// same path a real mouse takes, so a self-driven receipt exercises the
/// production code rather than a parallel one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HostPointer {
    /// Move the cursor (hover restyle, Enter/Leave, drag tracking).
    Moved(f32, f32),
    /// Press the left button at this point.
    Press(f32, f32),
    /// Release the left button at this point.
    Release(f32, f32),
}

/// What the application sees inside a hook. One shape for every hook so the
/// application-side plumbing stays boring.
pub struct AppCtx<'a, State: 'static, Logic, V>
where
    Logic: FnMut(&State) -> V,
    V: RootView<State>,
{
    /// The runner: state access, updates, and dispatch.
    pub runner: &'a mut Runner<State, Logic, V>,
    /// The native window (chrome requests, redraws, cursor, IME area), when
    /// there is one. `None` under [`Harness`], the windowless test host — an
    /// application that asks for window chrome must tolerate its absence
    /// rather than assume a window exists.
    pub window: Option<&'a Window>,
    /// The logical (DPI-independent) size of the surface being laid out — the
    /// coordinate space the layout, the cursor, and [`HostPointer`] all use.
    pub logical_size: (f32, f32),
    /// The custom-paint leaf registry the paint pass renders from.
    pub leaves: &'a mut sprigging::LeafRegistry<u64>,
    /// Set to swap the stylesheet; the host relayouts under the new sheet.
    pub set_sheet: &'a mut Option<String>,
    /// Set to end the application after this event.
    pub close: &'a mut bool,
    /// The cross-thread wake handle for application-owned workers. It has the
    /// same callback shape Armillary consumes, without making this host depend
    /// on Armillary or any task runtime.
    pub wake: &'a HostWake,
    /// Arm a capture of the next presented frame.
    pub capture: &'a mut Option<CaptureFn>,
    /// Pointer events for the host to deliver to itself once this hook
    /// returns, in order. See [`HostPointer`].
    pub pointer: &'a mut Vec<HostPointer>,
    /// The window-verb seam, for a hook that wants to minimize/maximize/close
    /// without routing through application state. The same handle `init`
    /// received.
    pub window_commands: &'a WindowCommands,
    /// Where the window is now, for persisting across launches. `None` under
    /// [`Harness`], which has no window.
    pub geometry: Option<WindowGeometry>,
}

/// A per-frame hook: return `true` to keep frames coming.
pub type FrameHook<State, Logic, V> = Box<dyn FnMut(&mut AppCtx<'_, State, Logic, V>) -> bool>;
/// A plain application hook over the standard context.
pub type AppHook<State, Logic, V> = Box<dyn FnMut(&mut AppCtx<'_, State, Logic, V>)>;
/// A request from the OS or an application command to close the root window.
pub type CloseRequestHook<State, Logic, V> =
    Box<dyn FnMut(&mut AppCtx<'_, State, Logic, V>, CloseRequest) -> CloseDisposition>;
/// The text-seam query: which text field has focus, if any.
pub type FocusedTextHook<State, Logic, V> =
    Box<dyn Fn(&Runner<State, Logic, V>) -> Option<FocusedTextSlot<State>>>;
/// A pre-dispatch keyboard intercept: return `true` to consume the event.
pub type KeyInterceptHook<State, Logic, V> =
    Box<dyn FnMut(&mut Runner<State, Logic, V>, &KeyPress) -> bool>;

/// The application's hooks. Plain closures, owned state lives in their
/// captured environment (an `Rc<RefCell<...>>` for anything shared).
pub struct HostHooks<State: 'static, Logic, V>
where
    Logic: FnMut(&State) -> V,
    V: RootView<State>,
{
    /// Runs at the top of every frame, before layout and paint: drive
    /// animations, sync leaves, poll backends. Return `true` to keep frames
    /// coming (an animation is live).
    pub frame: FrameHook<State, Logic, V>,
    /// The tail of every input dispatch: persist, push state to backends,
    /// drain window-chrome requests.
    pub after_dispatch: AppHook<State, Logic, V>,
    /// Runs after a frame is presented and the accessibility tree synced:
    /// scenario pumping and other per-presented-frame work.
    pub after_frame: AppHook<State, Logic, V>,
    /// Runs after an application-owned worker wakes the host, before the
    /// redraw it requested. Drain the application's own channel here.
    pub after_wake: AppHook<State, Logic, V>,
    /// Decides what a native or application-requested close means. The host
    /// owns the resulting visibility or exit; the application owns policy.
    pub close_request: CloseRequestHook<State, Logic, V>,
    /// The text seam: which text field has focus, if any.
    pub focused_text: FocusedTextHook<State, Logic, V>,
    /// Pre-dispatch keyboard intercept (Escape policy and friends). Return
    /// `true` to consume the event; `after_dispatch` runs either way when
    /// consumed.
    pub key_intercept: KeyInterceptHook<State, Logic, V>,
}

/// What `init` hands back once the window exists.
pub struct Init<State, Logic> {
    /// The application state the runner owns.
    pub state: State,
    /// The view logic (`fn(&State) -> V` or a closure).
    pub logic: Logic,
    /// The stylesheet the layout runs under.
    pub sheet: String,
}

type InitFn<State, Logic> =
    Box<dyn FnOnce(&Window, &WindowCommands, &HostWake) -> Init<State, Logic>>;

/// A logical window dimension from the environment.
fn env_size(key: &str) -> Option<f64> {
    std::env::var(key)
        .ok()?
        .parse::<f64>()
        .ok()
        .filter(|v| *v > 0.0)
}

pub(crate) struct HostState<State: 'static, Logic, V>
where
    Logic: FnMut(&State) -> V,
    V: RootView<State>,
{
    pub(crate) window: Option<Arc<Window>>,
    pub(crate) surface: Option<SurfaceHost>,
    pub(crate) runner: Option<Runner<State, Logic, V>>,
    /// Retained layout session in logical coordinates — hit-test target and
    /// incremental-apply subject.
    pub(crate) layout: Option<IncrementalLayout<NodeId>>,
    pub(crate) layout_size: (f32, f32),
    pub(crate) sheet: String,
    pub(crate) leaves: sprigging::LeafRegistry<u64>,
    pub(crate) rendered: sprigging::RenderedLeaves,
    /// Netrender roadmap E4 — leaf key → (retained `FragmentId`, epoch it was
    /// translated at). Synced against `rendered` each redraw while a surface
    /// (and therefore a renderer registry) exists; cleared when the surface is
    /// gone, since the registry died with it. When a key is here,
    /// `emit_paint_list_with_leaves` places a marker instead of splicing the
    /// leaf's commands, and the renderer composes the cached lowering.
    pub(crate) leaf_fragments: std::collections::HashMap<u64, (u64, u64)>,
    /// Cursor position in logical coordinates.
    pub(crate) cursor: (f32, f32),
    pub(crate) modifiers: ModifiersState,
    /// The node whose text field anchors an active drag selection.
    pub(crate) text_drag: Option<NodeId>,
    /// Opaque ids for `:hover` / `:focus` restyles on target change.
    pub(crate) last_hover: Option<u64>,
    pub(crate) last_focus: Option<u64>,
    /// The hovered hit node, for `on_hover` Enter/Leave routing.
    pub(crate) last_hover_hit: Option<NodeId>,
    /// Last resize-edge the cursor was over (CSD), to dedup cursor sets.
    pub(crate) resize_hint: Option<winit::window::ResizeDirection>,
    /// The application's end of the window-verb seam; the host drains it
    /// after every dispatch.
    pub(crate) commands: WindowCommands,
    /// Double-click detection for the title bar, which winit does not provide.
    pub(crate) cadence: ClickCadence,
    /// Every window verb performed this run, for tests. Cheap (a handful of
    /// enum values over an application's lifetime) and the only way a
    /// windowless harness can prove the frame did what the gesture asked.
    pub(crate) performed: Vec<WindowCommand>,
    /// Monotonic base for the CSS-transition animation clock.
    pub(crate) anim_base: std::time::Instant,
    pub(crate) a11y: Option<A11yHost>,
    pub(crate) a11y_wake: Arc<AtomicBool>,
    /// Set by [`HostWake`] until the event loop gives the application one drain
    /// turn. Kept in host state so the harness can exercise the same coalescing
    /// contract without a native event loop.
    pub(crate) wake_pending: Arc<AtomicBool>,
    pub(crate) scrollbar_fade: ScrollbarFade<ScrollTarget<NodeId>>,
    pub(crate) close_requested: bool,
    /// Whether a close disposition hid the root window. The native handle stays
    /// alive, so a later product extension may restore it without rebuilding
    /// canonical application state.
    pub(crate) hidden: bool,
    pub(crate) pending_sheet: Option<String>,
    pub(crate) pending_capture: Option<CaptureFn>,
    /// Pointer events an application hook asked the host to deliver to itself,
    /// drained through the real input path once the hook returns.
    pub(crate) pending_pointer: Vec<HostPointer>,
    /// Tab is being held: the arrow keys steer focus instead of reaching the
    /// focused element. Set by Tab's first key-repeat, cleared on its release.
    pub(crate) tab_held: bool,
}

impl<State, Logic, V> HostState<State, Logic, V>
where
    State: 'static,
    Logic: FnMut(&State) -> V,
    V: RootView<State>,
{
    pub(crate) fn new() -> Self {
        Self {
            window: None,
            surface: None,
            runner: None,
            layout: None,
            layout_size: (0.0, 0.0),
            sheet: String::new(),
            leaves: sprigging::LeafRegistry::new(),
            rendered: sprigging::RenderedLeaves::new(),
            leaf_fragments: std::collections::HashMap::new(),
            cursor: (0.0, 0.0),
            modifiers: ModifiersState::empty(),
            text_drag: None,
            last_hover: None,
            last_focus: None,
            last_hover_hit: None,
            resize_hint: None,
            commands: WindowCommands::new(),
            cadence: ClickCadence::new(),
            performed: Vec::new(),
            anim_base: std::time::Instant::now(),
            a11y: None,
            a11y_wake: Arc::new(AtomicBool::new(false)),
            wake_pending: Arc::new(AtomicBool::new(false)),
            scrollbar_fade: ScrollbarFade::new(),
            close_requested: false,
            hidden: false,
            pending_sheet: None,
            pending_capture: None,
            pending_pointer: Vec::new(),
            tab_held: false,
        }
    }
}

/// The host: options, hooks, and everything the donor's `App` owned that was
/// not application policy.
pub struct Host<State: 'static, Logic, V>
where
    Logic: FnMut(&State) -> V,
    V: RootView<State>,
{
    pub(crate) options: HostOptions,
    pub(crate) init: Option<InitFn<State, Logic>>,
    pub(crate) hooks: HostHooks<State, Logic, V>,
    pub(crate) s: HostState<State, Logic, V>,
    pub(crate) wake: HostWake,
}

/// What the event loop should do on an idle turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdlePolicy {
    /// Nothing is pending; sleep until the next event.
    Wait,
    /// Something time-based is live (an overlay scrollbar mid-fade): repaint
    /// and come back after this long.
    Animate(std::time::Duration),
    /// A screen reader acted while the app was idle: repaint so the queued
    /// accessibility action is drained.
    A11yWake,
}

/// Run a single-root Cambium application to completion.
pub fn run<State, Logic, V>(
    options: HostOptions,
    init: impl FnOnce(&Window, &WindowCommands, &HostWake) -> Init<State, Logic> + 'static,
    hooks: HostHooks<State, Logic, V>,
) -> Result<(), winit::error::EventLoopError>
where
    State: 'static,
    Logic: FnMut(&State) -> V + 'static,
    V: RootView<State>,
{
    let event_loop = EventLoop::<HostEvent>::with_user_event().build()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let s = HostState::new();
    let pending = s.wake_pending.clone();
    let proxy = event_loop.create_proxy();
    let wake = HostWake::new(
        pending,
        Arc::new(move || {
            let _ = proxy.send_event(HostEvent::Wake);
        }),
    );
    let mut host = Host {
        options,
        init: Some(Box::new(init)),
        hooks,
        s,
        wake,
    };
    event_loop.run_app(&mut host)
}

impl<State, Logic, V> Host<State, Logic, V>
where
    State: 'static,
    Logic: FnMut(&State) -> V + 'static,
    V: RootView<State>,
{
    pub(crate) fn scale_factor(&self) -> f64 {
        self.s.window.as_ref().map_or(1.0, |w| w.scale_factor())
    }

    /// The logical size being laid out: the window's, or — windowless, under
    /// [`Harness`] — the size the retained layout was last built at.
    pub(crate) fn logical_size(&self) -> (f32, f32) {
        match self.s.window.as_ref() {
            Some(window) => {
                let size = window.inner_size();
                let scale = window.scale_factor() as f32;
                (
                    size.width.max(1) as f32 / scale,
                    size.height.max(1) as f32 / scale,
                )
            },
            None => self.s.layout_size,
        }
    }

    /// Run one application hook with the standard context, then apply any
    /// host-owned requests it made (sheet swap → relayout, queued pointer
    /// events → the real input path).
    pub(crate) fn with_ctx(&mut self, which: Hook) {
        {
            let logical_size = self.logical_size();
            let geometry = self.geometry();
            let commands = self.s.commands.clone();
            let window = self.s.window.as_deref();
            let Some(runner) = self.s.runner.as_mut() else {
                return;
            };
            let mut ctx = AppCtx {
                runner,
                window,
                logical_size,
                leaves: &mut self.s.leaves,
                set_sheet: &mut self.s.pending_sheet,
                close: &mut self.s.close_requested,
                wake: &self.wake,
                capture: &mut self.s.pending_capture,
                pointer: &mut self.s.pending_pointer,
                window_commands: &commands,
                geometry,
            };
            match which {
                Hook::AfterDispatch => (self.hooks.after_dispatch)(&mut ctx),
                Hook::AfterFrame => (self.hooks.after_frame)(&mut ctx),
                Hook::AfterWake => (self.hooks.after_wake)(&mut ctx),
            }
        }
        self.apply_pending();
    }

    /// Apply requests a hook made after its temporary borrows of the runner and
    /// view state ended. Shared by normal hooks and close negotiation.
    fn apply_pending(&mut self) {
        if let Some(sheet) = self.s.pending_sheet.take() {
            self.s.sheet = sheet;
            // Force a full relayout under the new sheet.
            self.s.layout = None;
            self.s.layout_size = (0.0, 0.0);
        }
        self.drain_pointer();
    }

    /// Drain one application wake. The caller is the native `UserEvent` path
    /// or the windowless harness; either way, a worker owns its channel and
    /// this host only gives it a UI-thread drain turn and a redraw.
    pub(crate) fn process_wake(&mut self) -> bool {
        if !self.wake.take_pending() {
            return false;
        }
        self.with_ctx(Hook::AfterWake);
        if !self.s.hidden {
            if let Some(window) = self.s.window.as_ref() {
                window.request_redraw();
            }
        }
        true
    }

    /// Ask the application whether a close should keep the window visible,
    /// hide it while its work continues, or terminate the event loop.
    pub(crate) fn request_close(&mut self, request: CloseRequest) {
        let disposition = {
            let logical_size = self.logical_size();
            let geometry = self.geometry();
            let commands = self.s.commands.clone();
            let window = self.s.window.as_deref();
            let Some(runner) = self.s.runner.as_mut() else {
                self.s.close_requested = true;
                return;
            };
            let mut ctx = AppCtx {
                runner,
                window,
                logical_size,
                leaves: &mut self.s.leaves,
                set_sheet: &mut self.s.pending_sheet,
                close: &mut self.s.close_requested,
                wake: &self.wake,
                capture: &mut self.s.pending_capture,
                pointer: &mut self.s.pending_pointer,
                window_commands: &commands,
                geometry,
            };
            (self.hooks.close_request)(&mut ctx, request)
        };
        self.apply_pending();
        if self.s.close_requested {
            return;
        }
        match disposition {
            CloseDisposition::KeepVisible => {
                if let Some(window) = self.s.window.as_ref() {
                    window.request_redraw();
                }
            },
            CloseDisposition::Hide => {
                if let Some(window) = self.s.window.as_ref() {
                    window.set_visible(false);
                }
                self.s.hidden = true;
            },
            CloseDisposition::Exit => self.s.close_requested = true,
        }
    }

    /// Deliver the pointer events an application queued, in order, through the
    /// same routing a real mouse takes. Drained outside the hook's borrow of
    /// the runner, so the delivery can hit-test, capture, and dispatch exactly
    /// as `window_event` does.
    ///
    /// `after_dispatch` is *not* re-entered from here for the hook that queued
    /// the events — each delivery runs its own, and the queue is taken whole so
    /// an event queued by that dispatch lands on the next drain rather than
    /// looping.
    pub(crate) fn drain_pointer(&mut self) {
        if self.s.pending_pointer.is_empty() {
            return;
        }
        for event in std::mem::take(&mut self.s.pending_pointer) {
            match event {
                HostPointer::Moved(x, y) => {
                    self.s.cursor = (x, y);
                    self.hover();
                    self.hover_dispatch();
                    self.pointer_move();
                    self.drag_text_selection();
                },
                HostPointer::Press(x, y) => {
                    self.s.cursor = (x, y);
                    self.click();
                },
                HostPointer::Release(x, y) => {
                    self.s.cursor = (x, y);
                    self.release();
                },
            }
        }
    }

    /// The tail of every input dispatch: the application hook, then the
    /// host-owned IME and repaint policy.
    pub(crate) fn after_dispatch(&mut self) {
        self.with_ctx(Hook::AfterDispatch);
        // Window verbs an application queued from a click handler. Drained
        // here rather than inside the hook so a verb runs after the state
        // change that asked for it, and so `Drag` still happens while the
        // press that requested it is down.
        self.run_window_commands();
        let Some(window) = self.s.window.as_ref() else {
            return;
        };
        window.set_ime_allowed(
            self.s
                .runner
                .as_ref()
                .is_some_and(|runner| (self.hooks.focused_text)(runner).is_some()),
        );
        window.request_redraw();
    }
}

pub(crate) enum Hook {
    AfterDispatch,
    AfterFrame,
    AfterWake,
}

impl<State, Logic, V> Host<State, Logic, V>
where
    State: 'static,
    Logic: FnMut(&State) -> V + 'static,
    V: RootView<State>,
{
    /// The callback handed to the AccessKit adapter. It fires on the adapter's
    /// own thread when a screen reader acts on an otherwise idle application,
    /// and its only job is to raise the flag [`idle_policy`](Self::idle_policy)
    /// reads — the wake path a test can prove end to end without an OS
    /// adapter to stand in for the screen reader.
    pub(crate) fn a11y_waker(&self) -> impl Fn() + Send + Sync + 'static {
        let wake = self.s.a11y_wake.clone();
        move || wake.store(true, Ordering::Relaxed)
    }

    /// Decide what the event loop does on an idle turn, consuming the
    /// accessibility wake flag if one is set. Factored out of `about_to_wait`
    /// so the wake path is assertable: `Wait` really means nothing is pending,
    /// and a raised wake really becomes a repaint rather than being swallowed.
    pub(crate) fn idle_policy(&mut self, now: std::time::Instant) -> IdlePolicy {
        if self.s.a11y_wake.swap(false, Ordering::Relaxed) {
            return IdlePolicy::A11yWake;
        }
        // Overlay scrollbars mid-hold/mid-fade keep frames coming until
        // hidden; `Wait` never wakes without an event, so ask for a timed wake.
        if self.s.scrollbar_fade.any_visible(now) {
            return IdlePolicy::Animate(std::time::Duration::from_millis(33));
        }
        IdlePolicy::Wait
    }
}

/// Which resize edge a point near the window border maps to, in logical
/// coordinates with an 8px grab margin. `None` in the interior.
fn resize_edge(x: f32, y: f32, w: f32, h: f32) -> Option<winit::window::ResizeDirection> {
    use winit::window::ResizeDirection as R;
    const M: f32 = 8.0;
    let left = x <= M;
    let right = x >= w - M;
    let top = y <= M;
    let bottom = y >= h - M;
    match (left, right, top, bottom) {
        (true, _, true, _) => Some(R::NorthWest),
        (_, true, true, _) => Some(R::NorthEast),
        (true, _, _, true) => Some(R::SouthWest),
        (_, true, _, true) => Some(R::SouthEast),
        (true, ..) => Some(R::West),
        (_, true, ..) => Some(R::East),
        (_, _, true, _) => Some(R::North),
        (_, _, _, true) => Some(R::South),
        _ => None,
    }
}

/// The resize cursor icon for a border direction — an undecorated (CSD)
/// window gets no OS resize cursors, so the host supplies the affordance.
fn edge_cursor(dir: winit::window::ResizeDirection) -> winit::window::CursorIcon {
    use winit::window::{CursorIcon as C, ResizeDirection as R};
    match dir {
        R::East | R::West => C::EwResize,
        R::North | R::South => C::NsResize,
        R::NorthEast | R::SouthWest => C::NeswResize,
        R::NorthWest | R::SouthEast => C::NwseResize,
    }
}

impl<State, Logic, V> Host<State, Logic, V>
where
    State: 'static,
    Logic: FnMut(&State) -> V + 'static,
    V: RootView<State>,
{
    /// CSD only: the resize edge under the cursor, when the window is
    /// floating.
    fn edge_under_cursor(&self) -> Option<winit::window::ResizeDirection> {
        if self.options.decorations {
            return None;
        }
        let window = self.s.window.as_ref()?;
        if window.is_maximized() {
            return None;
        }
        let size = window.inner_size();
        let s = window.scale_factor() as f32;
        resize_edge(
            self.s.cursor.0,
            self.s.cursor.1,
            size.width as f32 / s,
            size.height as f32 / s,
        )
    }

    /// CSD only: show the matching resize arrow near the border, deduped on
    /// transitions.
    fn update_resize_cursor(&mut self) {
        if self.options.decorations {
            return;
        }
        let dir = self.edge_under_cursor();
        if dir != self.s.resize_hint {
            self.s.resize_hint = dir;
            if let Some(window) = self.s.window.as_ref() {
                window.set_cursor(
                    dir.map(edge_cursor)
                        .unwrap_or(winit::window::CursorIcon::Default),
                );
            }
        }
    }

    /// Refresh the runner's viewport after a resize or DPI change, if the
    /// application state tracks one. The host cannot know the state's shape,
    /// so it routes through an ordinary update; applications that track the
    /// viewport read the window inside `frame`.
    fn note_resize(&mut self) {
        if let Some(window) = self.s.window.as_ref() {
            window.request_redraw();
        }
    }
}

impl<State, Logic, V> ApplicationHandler<HostEvent> for Host<State, Logic, V>
where
    State: 'static,
    Logic: FnMut(&State) -> V + 'static,
    V: RootView<State>,
{
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Resume after a suspend. The window, the runner, the retained layout,
        // and every scrap of application state survived; only the drawing
        // surface was taken away, so boot a new one against the same window
        // and repaint. Booting from the options factory rather than a stashed
        // value is why `HostOptions::netrender` is a closure: the second
        // surface must have the same renderer configuration as the first.
        if let Some(window) = self.s.window.clone() {
            if self.s.surface.is_none() {
                let size = window.inner_size();
                match SurfaceHost::boot(
                    window.clone(),
                    size.width.max(1),
                    size.height.max(1),
                    (self.options.netrender)(),
                ) {
                    Ok(surface) => {
                        self.s.surface = Some(surface);
                        // The surface is new, so nothing is cached in it: force
                        // a full repaint rather than an incremental one.
                        self.redraw();
                    },
                    Err(e) => eprintln!("[cambium-host] surface re-boot failed: {e}"),
                }
            }
            return;
        }
        // Start comfortably on a large desktop, but never assume one.
        let want = self
            .options
            .size_env
            .as_ref()
            .map(|(w_key, h_key)| {
                (
                    env_size(w_key).unwrap_or(self.options.initial_logical_size.0),
                    env_size(h_key).unwrap_or(self.options.initial_logical_size.1),
                )
            })
            .unwrap_or(self.options.initial_logical_size);
        let (initial_pos, initial_size) = event_loop
            .primary_monitor()
            .map(|monitor| {
                let scale = monitor.scale_factor();
                let size = monitor.size();
                let pos = monitor.position();
                let logical_w = size.width as f64 / scale;
                let logical_h = size.height as f64 / scale;
                let width = want.0.min((logical_w - 48.0).max(480.0));
                let height = want.1.min((logical_h - 48.0).max(360.0));
                let x = pos.x as f64 / scale + ((logical_w - width) / 2.0).max(8.0);
                let y = pos.y as f64 / scale + ((logical_h - height) / 2.0).max(8.0);
                (
                    winit::dpi::LogicalPosition::new(x, y),
                    winit::dpi::LogicalSize::new(width, height),
                )
            })
            .unwrap_or((
                winit::dpi::LogicalPosition::new(40.0, 8.0),
                winit::dpi::LogicalSize::new(want.0, want.1),
            ));
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title(self.options.title.clone())
                        .with_decorations(self.options.decorations)
                        // Hidden until the a11y adapter is installed on the
                        // first frame — the Windows AccessKit adapter must
                        // attach before the window is shown. Revealed in
                        // `sync_a11y`.
                        .with_visible(false)
                        .with_position(initial_pos)
                        .with_inner_size(initial_size),
                )
                .expect("create window"),
        );
        let size = window.inner_size();
        let surface = SurfaceHost::boot(
            window.clone(),
            size.width.max(1),
            size.height.max(1),
            (self.options.netrender)(),
        )
        .expect("boot genet host");
        let init = self.init.take().expect("resumed once");
        // The application takes its end of the window-verb seam here, stores
        // it in its own state, and calls it from ordinary click handlers.
        let Init {
            state,
            logic,
            sheet,
        } = init(&window, &self.s.commands.clone(), &self.wake);
        let dom = Rc::new(RefCell::new(ScriptedDom::new()));
        let runner = Runner::new(dom, logic, state);
        self.s.a11y = Some(A11yHost::new(self.a11y_waker()));
        self.s.sheet = sheet;
        self.s.window = Some(window);
        self.s.surface = Some(surface);
        self.s.runner = Some(runner);
        // Drive the first frame synchronously while the window is hidden:
        // lay out, install the a11y tree, then reveal. A hidden winit window
        // may not receive a deferred redraw.
        self.redraw();
        self.sync_a11y();
    }

    /// The platform is taking the drawing surface away (Android, iOS; never on
    /// the desktop backends). Drop the surface and nothing else: the window
    /// handle, the runner's state, the retained layout, the accessibility tree,
    /// and the leaf registry all survive, so `resumed` re-boots a surface and
    /// repaints the same application rather than restarting it.
    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        self.s.surface = None;
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        match self.idle_policy(std::time::Instant::now()) {
            IdlePolicy::A11yWake => {
                if let Some(window) = self.s.window.as_ref() {
                    window.request_redraw();
                }
                // Come straight back: the flag was consumed, and the next idle
                // turn settles on the real policy once the action is drained.
                event_loop.set_control_flow(ControlFlow::Poll);
            },
            IdlePolicy::Animate(after) => {
                if let Some(window) = self.s.window.as_ref() {
                    window.request_redraw();
                }
                event_loop.set_control_flow(ControlFlow::wait_duration(after));
            },
            IdlePolicy::Wait => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: HostEvent) {
        match event {
            HostEvent::Wake => {
                self.process_wake();
            },
        }
        if self.s.close_requested {
            event_loop.exit();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => self.request_close(CloseRequest::Native),
            WindowEvent::Resized(size) => {
                if let Some(surface) = self.s.surface.as_mut() {
                    surface.resize(size.width.max(1), size.height.max(1));
                }
                self.note_resize();
            },
            WindowEvent::ScaleFactorChanged { .. } => self.note_resize(),
            WindowEvent::ModifiersChanged(mods) => {
                self.s.modifiers = mods.state();
            },
            WindowEvent::CursorMoved { position, .. } => {
                let scale = self.scale_factor();
                self.s.cursor = ((position.x / scale) as f32, (position.y / scale) as f32);
                self.update_resize_cursor();
                self.hover();
                self.hover_dispatch();
                // A captured drag gets the move before the text selection: an
                // `on_pointer` element that took the press owns the gesture.
                self.pointer_move();
                self.drag_text_selection();
            },
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                // Edge grab beats content when a CSD window is floating.
                match self.edge_under_cursor() {
                    Some(dir) => {
                        if let Some(w) = self.s.window.as_ref() {
                            let _ = w.drag_resize_window(dir);
                        }
                    },
                    // Then the window frame: a press on an `--app-region: drag`
                    // surface moves the window, and a double-click there
                    // toggles maximize. Both still dispatch into the DOM first,
                    // so a drag surface can also be a focus target and a
                    // `no-drag` control inside it keeps its click.
                    None => self.press_left(),
                }
            },
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } => self.press_right(),
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => self.release(),
            WindowEvent::MouseWheel { delta, .. } => self.wheel(delta),
            WindowEvent::Ime(ime) => self.ime(&ime),
            WindowEvent::KeyboardInput { event, .. } => match event.state {
                ElementState::Pressed => self.key(&KeyPress {
                    key: event.logical_key,
                    text: event.text.as_ref().map(|t| t.to_string()),
                    modifiers: self.s.modifiers,
                    repeat: event.repeat,
                }),
                // Releases matter for exactly one thing: letting go of Tab
                // leaves spatial focus navigation.
                ElementState::Released => {
                    if matches!(
                        event.logical_key,
                        winit::keyboard::Key::Named(winit::keyboard::NamedKey::Tab)
                    ) {
                        self.s.tab_held = false;
                    }
                },
            },
            WindowEvent::RedrawRequested => {
                self.redraw();
                // After the frame is laid out and presented, refresh the
                // accessibility tree and drain any screen-reader actions,
                // then let the application pump per-presented-frame work.
                self.sync_a11y();
                self.with_ctx(Hook::AfterFrame);
            },
            _ => {},
        }
        if self.s.close_requested {
            event_loop.exit();
        }
    }
}
