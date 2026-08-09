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
//! run(options, |window| Init { state, logic, sheet }, hooks)
//! ```

use std::rc::Rc;
use std::cell::RefCell;
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

mod frame;
mod input;

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
pub type CaptureFn =
    Box<dyn FnOnce(&SurfaceHost, &wgpu::TextureView, u32, u32) + 'static>;

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
    /// Renderer options handed to [`SurfaceHost::boot`]. `Option` because
    /// `NetrenderOptions` is not `Clone`; the host takes it at window
    /// creation.
    pub netrender: Option<NetrenderOptions>,
}

impl Default for HostOptions {
    fn default() -> Self {
        Self {
            title: String::new(),
            decorations: true,
            initial_logical_size: (1_100.0, 664.0),
            size_env: None,
            netrender: Some(NetrenderOptions {
                tile_cache_size: Some(1024),
                enable_vello: true,
                ..Default::default()
            }),
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

/// What the application sees inside a hook. One shape for every hook so the
/// application-side plumbing stays boring.
pub struct AppCtx<'a, State: 'static, Logic, V>
where
    Logic: FnMut(&State) -> V,
    V: RootView<State>,
{
    /// The runner: state access, updates, and dispatch.
    pub runner: &'a mut Runner<State, Logic, V>,
    /// The native window (chrome requests, redraws, cursor, IME area).
    pub window: &'a Window,
    /// The custom-paint leaf registry the paint pass renders from.
    pub leaves: &'a mut sprigging::LeafRegistry<u64>,
    /// Set to swap the stylesheet; the host relayouts under the new sheet.
    pub set_sheet: &'a mut Option<String>,
    /// Set to end the application after this event.
    pub close: &'a mut bool,
    /// Arm a capture of the next presented frame.
    pub capture: &'a mut Option<CaptureFn>,
}

/// A per-frame hook: return `true` to keep frames coming.
pub type FrameHook<State, Logic, V> =
    Box<dyn FnMut(&mut AppCtx<'_, State, Logic, V>) -> bool>;
/// A plain application hook over the standard context.
pub type AppHook<State, Logic, V> = Box<dyn FnMut(&mut AppCtx<'_, State, Logic, V>)>;
/// The text-seam query: which text field has focus, if any.
pub type FocusedTextHook<State, Logic, V> =
    Box<dyn Fn(&Runner<State, Logic, V>) -> Option<FocusedTextSlot<State>>>;
/// A pre-dispatch keyboard intercept: return `true` to consume the event.
pub type KeyInterceptHook<State, Logic, V> =
    Box<dyn FnMut(&mut Runner<State, Logic, V>, &winit::event::KeyEvent) -> bool>;

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

type InitFn<State, Logic> = Box<dyn FnOnce(&Window) -> Init<State, Logic>>;

/// A logical window dimension from the environment.
fn env_size(key: &str) -> Option<f64> {
    std::env::var(key).ok()?.parse::<f64>().ok().filter(|v| *v > 0.0)
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
    /// Monotonic base for the CSS-transition animation clock.
    pub(crate) anim_base: std::time::Instant,
    pub(crate) a11y: Option<A11yHost>,
    pub(crate) a11y_wake: Arc<AtomicBool>,
    pub(crate) scrollbar_fade: ScrollbarFade<ScrollTarget<NodeId>>,
    pub(crate) close_requested: bool,
    pub(crate) pending_sheet: Option<String>,
    pub(crate) pending_capture: Option<CaptureFn>,
}

/// The host: options, hooks, and everything the donor's `App` owned that was
/// not application policy.
pub struct Host<State: 'static, Logic, V>
where
    Logic: FnMut(&State) -> V,
    V: RootView<State>,
{
    options: HostOptions,
    init: Option<InitFn<State, Logic>>,
    pub(crate) hooks: HostHooks<State, Logic, V>,
    pub(crate) s: HostState<State, Logic, V>,
}

/// Run a single-root Cambium application to completion.
pub fn run<State, Logic, V>(
    options: HostOptions,
    init: impl FnOnce(&Window) -> Init<State, Logic> + 'static,
    hooks: HostHooks<State, Logic, V>,
) -> Result<(), winit::error::EventLoopError>
where
    State: 'static,
    Logic: FnMut(&State) -> V + 'static,
    V: RootView<State>,
{
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut host = Host {
        options,
        init: Some(Box::new(init)),
        hooks,
        s: HostState {
            window: None,
            surface: None,
            runner: None,
            layout: None,
            layout_size: (0.0, 0.0),
            sheet: String::new(),
            leaves: sprigging::LeafRegistry::new(),
            rendered: sprigging::RenderedLeaves::new(),
            cursor: (0.0, 0.0),
            modifiers: ModifiersState::empty(),
            text_drag: None,
            last_hover: None,
            last_focus: None,
            last_hover_hit: None,
            resize_hint: None,
            anim_base: std::time::Instant::now(),
            a11y: None,
            a11y_wake: Arc::new(AtomicBool::new(false)),
            scrollbar_fade: ScrollbarFade::new(),
            close_requested: false,
            pending_sheet: None,
            pending_capture: None,
        },
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

    /// Run one application hook with the standard context, then apply any
    /// host-owned requests it made (sheet swap → relayout).
    pub(crate) fn with_ctx(&mut self, which: Hook) {
        let (Some(window), Some(runner)) =
            (self.s.window.as_ref(), self.s.runner.as_mut())
        else {
            return;
        };
        let mut ctx = AppCtx {
            runner,
            window,
            leaves: &mut self.s.leaves,
            set_sheet: &mut self.s.pending_sheet,
            close: &mut self.s.close_requested,
            capture: &mut self.s.pending_capture,
        };
        match which {
            Hook::AfterDispatch => (self.hooks.after_dispatch)(&mut ctx),
            Hook::AfterFrame => (self.hooks.after_frame)(&mut ctx),
        }
        if let Some(sheet) = self.s.pending_sheet.take() {
            self.s.sheet = sheet;
            // Force a full relayout under the new sheet.
            self.s.layout = None;
            self.s.layout_size = (0.0, 0.0);
        }
    }

    /// The tail of every input dispatch: the application hook, then the
    /// host-owned IME and repaint policy.
    pub(crate) fn after_dispatch(&mut self) {
        self.with_ctx(Hook::AfterDispatch);
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

impl<State, Logic, V> ApplicationHandler for Host<State, Logic, V>
where
    State: 'static,
    Logic: FnMut(&State) -> V + 'static,
    V: RootView<State>,
{
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.s.window.is_some() {
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
            self.options.netrender.take().unwrap_or_default(),
        )
        .expect("boot genet host");
        let init = self.init.take().expect("resumed once");
        let Init { state, logic, sheet } = init(&window);
        let dom = Rc::new(RefCell::new(ScriptedDom::new()));
        let runner = Runner::new(dom, logic, state);
        let wake = self.s.a11y_wake.clone();
        self.s.a11y = Some(A11yHost::new(move || {
            wake.store(true, Ordering::Relaxed);
        }));
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

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // A screen reader acted while the app was idle: turn the adapter's
        // wake flag into a redraw so `sync_a11y` drains the action.
        if self.s.a11y_wake.swap(false, Ordering::Relaxed)
            && let Some(window) = self.s.window.as_ref()
        {
            window.request_redraw();
        }
        // Overlay scrollbars mid-hold/mid-fade keep frames coming until
        // hidden; `Wait` never wakes without an event, so ask for a timed
        // wake.
        if self.s.scrollbar_fade.any_visible(std::time::Instant::now()) {
            if let Some(window) = self.s.window.as_ref() {
                window.request_redraw();
            }
            event_loop.set_control_flow(ControlFlow::wait_duration(
                std::time::Duration::from_millis(33),
            ));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(surface) = self.s.surface.as_mut() {
                    surface.resize(size.width.max(1), size.height.max(1));
                }
                self.note_resize();
            }
            WindowEvent::ScaleFactorChanged { .. } => self.note_resize(),
            WindowEvent::ModifiersChanged(mods) => {
                self.s.modifiers = mods.state();
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale = self.scale_factor();
                self.s.cursor =
                    ((position.x / scale) as f32, (position.y / scale) as f32);
                self.update_resize_cursor();
                self.hover();
                self.hover_dispatch();
                self.drag_text_selection();
            }
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
                    }
                    None => self.click(),
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                self.s.text_drag = None;
            }
            WindowEvent::MouseWheel { delta, .. } => self.wheel(delta),
            WindowEvent::Ime(ime) => self.ime(&ime),
            WindowEvent::KeyboardInput { event, .. } => self.key(&event),
            WindowEvent::RedrawRequested => {
                self.redraw();
                // After the frame is laid out and presented, refresh the
                // accessibility tree and drain any screen-reader actions,
                // then let the application pump per-presented-frame work.
                self.sync_a11y();
                self.with_ctx(Hook::AfterFrame);
            }
            _ => {}
        }
        if self.s.close_requested {
            event_loop.exit();
        }
    }
}
