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
use cambium_rootstock::Accessibility;
use cambium_winit::ScrollbarFade;
use cambium_winit_a11y::A11yHost;
use genet_layout::{IncrementalLayout, ScrollTarget};
use genet_scripted_dom::{NodeId, ScriptedDom};
use genet_winit_host::SurfaceHost;
use netrender::NetrenderOptions;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowId};

mod decorations;
mod harness;

pub use cambium_rootstock::{
    AppCtx, AppHook, AppRegion, CaptureFn, CloseDisposition, CloseRequest, CloseRequestHook,
    Direction, FocusedTextHook, FocusedTextSlot, Frame, FrameHook, Host, HostHooks, HostOptions,
    HostPointer, HostWake, HostWindow, IdlePolicy, Init, Key, KeyInterceptHook, KeyPress,
    Modifiers, NamedKey, Runner, Surface, WindowCommand, WindowCommands, WindowGeometry,
    read_frame,
};
pub use harness::{Harness, inert_hooks};

use cambium_rootstock::meristem_bounds::RootView;
use cambium_rootstock::{Hook, HostState, env_size};
use decorations::ClickCadence;
#[derive(Clone, Copy, Debug)]
enum HostEvent {
    Wake,
}
/// The winit window, as the neutral seam sees it.
///
/// A newtype for the same reason as [`WinitSurface`]: the trait is not local to
/// this crate and neither is `Window`. It carries an `Arc` so the adapter keeps
/// its own handle for the window management the seam deliberately omits.
#[derive(Clone)]
pub struct WinitWindow(pub Arc<Window>);
impl HostWindow for WinitWindow {
    fn request_redraw(&self) {
        self.0.request_redraw();
    }

    fn inner_size(&self) -> (u32, u32) {
        let size = self.0.inner_size();
        (size.width, size.height)
    }

    fn scale_factor(&self) -> f64 {
        self.0.scale_factor()
    }

    fn set_ime_allowed(&self, allowed: bool) {
        self.0.set_ime_allowed(allowed);
    }

    fn set_ime_cursor_area(&self, x: f64, y: f64, width: f64, height: f64) {
        self.0.set_ime_cursor_area(
            winit::dpi::LogicalPosition::new(x, y),
            winit::dpi::LogicalSize::new(width, height),
        );
    }
}
/// The winit window's presentation surface, as the neutral seam sees it.
///
/// A newtype because neither [`Surface`] nor `SurfaceHost` is local to this
/// crate, so the implementation cannot be written directly on it. The wrapper
/// costs nothing and gives the winit surface a name on this side of the seam.
pub struct WinitSurface(pub SurfaceHost);
impl Surface for WinitSurface {
    fn core(&self) -> &genet_render_host::RenderCore {
        self.0.core()
    }

    fn format(&self) -> wgpu::TextureFormat {
        self.0.format()
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.0.resize(width, height);
    }

    fn acquire(&self) -> Option<wgpu::SurfaceTexture> {
        self.0.acquire()
    }
}
/// Convert a winit named key into the host's neutral vocabulary.
///
/// Keys this vocabulary does not special-case become [`NamedKey::Other`], the
/// same way `cambium_winit`'s mapping treats them.
fn named_from_winit(named: &winit::keyboard::NamedKey) -> NamedKey {
    use winit::keyboard::NamedKey as N;
    match named {
        N::Backspace => NamedKey::Backspace,
        N::Enter => NamedKey::Enter,
        N::Tab => NamedKey::Tab,
        N::Space => NamedKey::Space,
        N::Escape => NamedKey::Escape,
        N::ArrowLeft => NamedKey::ArrowLeft,
        N::ArrowRight => NamedKey::ArrowRight,
        N::ArrowUp => NamedKey::ArrowUp,
        N::ArrowDown => NamedKey::ArrowDown,
        N::Delete => NamedKey::Delete,
        N::Home => NamedKey::Home,
        N::End => NamedKey::End,
        N::PageUp => NamedKey::PageUp,
        N::PageDown => NamedKey::PageDown,
        _ => NamedKey::Other,
    }
}
/// Winit's live modifier state, in the host's neutral vocabulary.
///
/// `super_key` is winit's name for the platform command key, which is `meta`
/// here and in Cambium.
pub(crate) fn modifiers_from_winit_state(state: ModifiersState) -> Modifiers {
    Modifiers {
        shift: state.shift_key(),
        ctrl: state.control_key(),
        alt: state.alt_key(),
        meta: state.super_key(),
    }
}
/// A winit key event as the host routes it.
///
/// The one place winit's keyboard vocabulary is read. `Dead` and `Unidentified`
/// stay distinct: an unidentified key may still carry injected text and should
/// type, a dead key is an accent awaiting composition and must not.
pub(crate) fn key_from_winit(key: &winit::keyboard::Key) -> Key {
    use winit::keyboard::Key as W;
    match key {
        W::Character(c) => Key::Character(c.to_string()),
        W::Named(named) => Key::Named(named_from_winit(named)),
        W::Dead(_) => Key::Dead,
        W::Unidentified(_) => Key::Unidentified,
    }
}
fn key_press_from_winit(event: &winit::event::KeyEvent, modifiers: Modifiers) -> KeyPress {
    KeyPress {
        key: key_from_winit(&event.logical_key),
        text: event.text.as_ref().map(|t| t.to_string()),
        modifiers,
        repeat: event.repeat,
    }
}
/// The winit event source: the host, plus everything only a desktop window
/// can answer.
///
/// Derefs to [`Host`], because the wrapper is not an abstraction boundary; it
/// is the same host with a native window attached. Adapter code reads core
/// state through the deref and its own state directly, and the split stays
/// visible in the field list rather than in every call site.
pub struct WinitHost<State: 'static, Logic, V>
where
    Logic: FnMut(&State) -> V,
    V: RootView<State>,
{
    pub(crate) core: Host<State, Logic, V>,
    /// The native window, for the management the neutral seam omits. The one
    /// thing a browser tab cannot supply.
    pub(crate) native_window: Option<Arc<Window>>,
    /// Last resize-edge the cursor was over (CSD), to dedup cursor sets.
    pub(crate) resize_hint: Option<winit::window::ResizeDirection>,
    /// Double-click detection for the title bar, which winit does not provide.
    pub(crate) cadence: ClickCadence,
    /// Every window verb performed this run, for tests. The only way a
    /// windowless harness can prove the frame did what the gesture asked.
    pub(crate) performed: Vec<WindowCommand>,
}
impl<State, Logic, V> std::ops::Deref for WinitHost<State, Logic, V>
where
    Logic: FnMut(&State) -> V,
    V: RootView<State>,
{
    type Target = Host<State, Logic, V>;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}
impl<State, Logic, V> std::ops::DerefMut for WinitHost<State, Logic, V>
where
    Logic: FnMut(&State) -> V,
    V: RootView<State>,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}
impl<State, Logic, V> WinitHost<State, Logic, V>
where
    State: 'static,
    Logic: FnMut(&State) -> V + 'static,
    V: RootView<State>,
{
    /// Wrap a host for the desktop event loop.
    pub(crate) fn new(core: Host<State, Logic, V>) -> Self {
        Self {
            core,
            native_window: None,
            resize_hint: None,
            cadence: ClickCadence::new(),
            performed: Vec::new(),
        }
    }

    /// Run the application's close policy, then do what only a desktop window
    /// can: hide on [`CloseDisposition::Hide`], repaint on
    /// [`CloseDisposition::KeepVisible`].
    pub(crate) fn request_close(&mut self, request: CloseRequest) {
        match self.core.decide_close(request) {
            Some(CloseDisposition::KeepVisible) => {
                if let Some(window) = self.core.s.window.as_ref() {
                    window.request_redraw();
                }
            },
            Some(CloseDisposition::Hide) => {
                if let Some(window) = self.native_window.as_ref() {
                    window.set_visible(false);
                }
                self.core.s.hidden = true;
            },
            Some(CloseDisposition::Exit) | None => {},
        }
    }
}
/// Run a single-root Cambium application to completion.
pub fn run<State, Logic, V>(
    options: HostOptions,
    init: impl FnOnce(&dyn HostWindow, &WindowCommands, &HostWake) -> Init<State, Logic> + 'static,
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
    let mut host = WinitHost::new(Host {
        options,
        init: Some(Box::new(init)),
        hooks,
        s,
        wake,
    });
    event_loop.run_app(&mut host)
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
impl<State, Logic, V> WinitHost<State, Logic, V>
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
        let window = self.native_window.as_ref()?;
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
        if dir != self.resize_hint {
            self.resize_hint = dir;
            if let Some(window) = self.native_window.as_ref() {
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
impl<State, Logic, V> ApplicationHandler<HostEvent> for WinitHost<State, Logic, V>
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
        if let Some(window) = self.native_window.clone() {
            if self.s.surface.is_none() {
                let size = window.inner_size();
                match SurfaceHost::boot(
                    window.clone(),
                    size.width.max(1),
                    size.height.max(1),
                    (self.options.netrender)(),
                ) {
                    Ok(surface) => {
                        self.s.surface = Some(Box::new(WinitSurface(surface)));
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
        } = init(
            &WinitWindow(window.clone()),
            &self.s.commands.clone(),
            &self.wake,
        );
        let dom = Rc::new(RefCell::new(ScriptedDom::new()));
        let runner = Runner::new(dom, logic, state);
        let mut a11y = A11yHost::new(self.a11y_waker());
        a11y.attach(window.clone());
        self.s.a11y = Some(Box::new(a11y));
        self.s.sheet = sheet;
        self.native_window = Some(window.clone());
        self.s.window = Some(Box::new(WinitWindow(window)));
        self.s.surface = Some(Box::new(WinitSurface(surface)));
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
        match self.idle_policy(cambium_rootstock::Instant::now()) {
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
        self.run_window_commands();
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
                self.s.modifiers = modifiers_from_winit_state(mods.state());
            },
            WindowEvent::CursorMoved { position, .. } => {
                let scale = self.scale_factor();
                self.pointer_moved((position.x / scale) as f32, (position.y / scale) as f32);
                // The resize-edge cursor is decoration, and reads only the
                // pointer position, so it follows rather than interleaves.
                self.update_resize_cursor();
            },
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                // Edge grab beats content when a CSD window is floating.
                match self.edge_under_cursor() {
                    Some(dir) => {
                        if let Some(w) = self.native_window.as_ref() {
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
            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = genet_winit_host::wheel_delta_from_winit(delta);
                self.wheel(dx, dy);
            },
            WindowEvent::Ime(ime) => self.ime(cambium_winit::composition_from_winit(&ime)),
            WindowEvent::KeyboardInput { event, .. } => match event.state {
                ElementState::Pressed => {
                    let press = key_press_from_winit(&event, self.core.s.modifiers);
                    self.key(&press);
                },
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
                // The snapshot hooks read; refreshed here so a frame always
                // sees where the window is now, as the live query used to.
                self.core.s.geometry = self.geometry();
                self.redraw();
                // After the frame is laid out and presented, refresh the
                // accessibility tree and drain any screen-reader actions,
                // then let the application pump per-presented-frame work.
                self.sync_a11y();
                self.with_ctx(Hook::AfterFrame);
            },
            _ => {},
        }
        // Verbs queued during this event's dispatch run before the turn ends:
        // after the state change that asked for them, and for `Drag`, while
        // the press that requested it is still down. The queue is host data;
        // performing a verb needs the native window, so the drain lives here.
        self.run_window_commands();
        if self.s.close_requested {
            event_loop.exit();
        }
    }
}
