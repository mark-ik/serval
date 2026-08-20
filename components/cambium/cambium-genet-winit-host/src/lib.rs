//! The single-root Cambium desktop host.
//!
//! Every Cambium desktop application so far has hand-assembled the same
//! machinery: a winit `ApplicationHandler` owning one window, a genet
//! [`SurfaceHost`] presenting a `netrender` scene, a retained
//! owned Livery/Buckram layout over the runner's `ScriptedDom`, logical-coordinate
//! hit testing, pointer/keyboard/IME/wheel routing into a
//! [`cambium::GenetAppRunner`], overlay-scrollbar fade policy, and the
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

use std::sync::Arc;

use cambium_winit_a11y::A11yHost;
use genet_winit_host::SurfaceHost;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowId};

#[cfg(test)]
use genet_scripted_dom::ScriptedDom;
#[cfg(test)]
use std::{cell::RefCell, rc::Rc};

mod decorations;
mod harness;
#[cfg(target_os = "windows")]
mod windows_snap;
#[cfg(target_os = "linux")]
mod x11_frame;

pub use cambium_rootstock::{
    AppCtx, AppFrameInsets, AppHook, AppRegion, CaptureFn, CloseDisposition, CloseRequest,
    CloseRequestHook, Direction, FocusedTextHook, FocusedTextSlot, Frame, FrameHook, FrameProfile,
    Host, HostHooks, HostOptions, HostPointer, HostWake, HostWindow, IdlePolicy, Init, Key,
    KeyInterceptHook, KeyPress, Modifiers, NamedKey, Runner, Surface, WindowCommand,
    WindowCommands, WindowFrame, WindowGeometry, read_frame,
};
pub use harness::{Harness, inert_hooks};

use cambium_rootstock::meristem_bounds::RootView;
use cambium_rootstock::{Hook, HostState, env_size};
use decorations::ClickCadence;
#[derive(Clone, Copy, Debug)]
enum HostEvent {
    Wake,
}

/// `CAMBIUM_HOST_FRAME_TRACE=1` publishes the platform's post-configure answer
/// to the frame-policy request. This is deliberately a diagnostic rather than
/// application state: both compositor SSD and winit CSD are a `Host` frame to
/// the application, so it has no reason to branch between them.
fn frame_trace() -> bool {
    std::env::var_os("CAMBIUM_HOST_FRAME_TRACE").is_some_and(|value| value != "0")
}

/// Opt-in CPU-side frame attribution. Kept separate from the frame-policy
/// trace because a performance run emits one line per presented frame.
fn perf_trace() -> bool {
    std::env::var_os("CAMBIUM_HOST_PERF_TRACE").is_some_and(|value| value != "0")
}

fn usable_restored_geometry(
    geometry: Option<WindowGeometry>,
    monitors: &[(f64, f64, f64, f64)],
    size_overridden: bool,
) -> Option<WindowGeometry> {
    if size_overridden {
        return None;
    }
    geometry.filter(|geometry| geometry.is_reachable_on(monitors))
}

#[cfg(test)]
mod geometry_tests {
    use super::*;

    const PRIMARY: (f64, f64, f64, f64) = (0.0, 0.0, 1920.0, 1040.0);

    fn geometry(position: (f64, f64)) -> WindowGeometry {
        WindowGeometry {
            position,
            size: (900.0, 640.0),
            maximized: true,
        }
    }

    #[test]
    fn reachable_restored_geometry_is_used() {
        assert_eq!(
            usable_restored_geometry(Some(geometry((120.0, 80.0))), &[PRIMARY], false),
            Some(geometry((120.0, 80.0)))
        );
    }

    #[test]
    fn unreachable_restored_geometry_is_discarded() {
        assert_eq!(
            usable_restored_geometry(Some(geometry((2400.0, 80.0))), &[PRIMARY], false),
            None
        );
    }

    #[test]
    fn receipt_size_override_wins_over_restored_geometry() {
        assert_eq!(
            usable_restored_geometry(Some(geometry((120.0, 80.0))), &[PRIMARY], true),
            None
        );
    }
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

    fn titlebar_insets(&self) -> cambium_rootstock::TitlebarInsets {
        crate::decorations::titlebar_insets(&self.0)
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
    /// Win32's `WM_NCHITTEST` bridge for the application-drawn maximize
    /// control. Declared before the window so its subclass is removed first.
    #[cfg(target_os = "windows")]
    pub(crate) snap_layout: Option<windows_snap::SnapLayoutBridge>,
    /// The native window, for the management the neutral seam omits. The one
    /// thing a browser tab cannot supply.
    pub(crate) native_window: Option<Arc<Window>>,
    /// Last resize-edge the cursor was over (CSD), to dedup cursor sets.
    pub(crate) resize_hint: Option<winit::window::ResizeDirection>,
    /// Double-click detection for the title bar, which winit does not provide.
    pub(crate) cadence: ClickCadence,
    /// Last floating geometry. Maximized windows keep this rectangle and only
    /// change its `maximized` bit, so persistence records the restore target.
    pub(crate) restored_geometry: Option<WindowGeometry>,
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
            #[cfg(target_os = "windows")]
            snap_layout: None,
            native_window: None,
            resize_hint: None,
            cadence: ClickCadence::new(),
            restored_geometry: None,
            performed: Vec::new(),
        }
    }

    fn boot_surface(
        &self,
        window: Arc<Window>,
        width: u32,
        height: u32,
    ) -> Result<SurfaceHost, String> {
        SurfaceHost::boot_with_transparency(
            window,
            width,
            height,
            (self.options.netrender)(),
            self.options.app_frame_is_transparent(),
        )
    }

    /// Draw one frame, then project any application-owned native hit regions
    /// from that exact retained layout.
    fn redraw(&mut self) {
        self.core.redraw();
        #[cfg(target_os = "windows")]
        self.refresh_snap_layout_rect();
    }

    #[cfg(target_os = "windows")]
    fn refresh_snap_layout_rect(&mut self) {
        let logical = (|| {
            let runner = self.s.runner.as_ref()?;
            let layout = self.s.layout.as_ref()?;
            let dom = runner.dom();
            let dom_ref = dom.borrow();
            decorations::window_control_rect(
                &*dom_ref,
                runner.root(),
                layout,
                &self.options.maximize_control_label,
            )
        })();
        let scale = self
            .native_window
            .as_ref()
            .map_or(1.0, |window| window.scale_factor());
        let Some(bridge) = self.snap_layout.as_ref() else {
            return;
        };
        let Some(changed) = bridge.update(logical, scale) else {
            return;
        };
        if frame_trace() {
            match changed {
                Some([left, top, right, bottom]) => eprintln!(
                    "[cambium-winit] snap-layout hit=HTMAXBUTTON rect=[{left},{top},{right},{bottom}] scale={scale}"
                ),
                None => eprintln!("[cambium-winit] snap-layout hit=absent scale={scale}"),
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn publish_x11_frame_extents(&self, window: &Window) {
        let insets = self.options.effective_app_frame_insets();
        match x11_frame::publish_gtk_frame_extents(window, insets) {
            Ok(Some(extents)) if frame_trace() => eprintln!(
                "[cambium-winit] _GTK_FRAME_EXTENTS left={} right={} top={} bottom={}",
                extents[0], extents[1], extents[2], extents[3]
            ),
            Ok(_) => {},
            Err(error) => eprintln!("[cambium-winit] could not publish X11 frame extents: {error}"),
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
        if self.options.effective_window_frame() == WindowFrame::Host {
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
        if self.options.effective_window_frame() == WindowFrame::Host {
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
                match self.boot_surface(window.clone(), size.width.max(1), size.height.max(1)) {
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
        // Start comfortably on a large desktop, but never assume one. A
        // receipt's explicit size wins over application-restored geometry.
        let size_overridden = self
            .options
            .size_env
            .as_ref()
            .is_some_and(|(w_key, h_key)| env_size(w_key).is_some() || env_size(h_key).is_some());
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
        let monitors = event_loop
            .available_monitors()
            .map(|monitor| {
                let scale = monitor.scale_factor();
                let size = monitor.size();
                let pos = monitor.position();
                (
                    pos.x as f64 / scale,
                    pos.y as f64 / scale,
                    size.width as f64 / scale,
                    size.height as f64 / scale,
                )
            })
            .collect::<Vec<_>>();
        let restored =
            usable_restored_geometry(self.options.initial_geometry, &monitors, size_overridden);
        let (initial_pos, initial_size, initial_maximized) = match restored {
            Some(geometry) => (
                winit::dpi::LogicalPosition::new(geometry.position.0, geometry.position.1),
                winit::dpi::LogicalSize::new(geometry.size.0, geometry.size.1),
                geometry.maximized,
            ),
            None => event_loop
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
                        false,
                    )
                })
                .unwrap_or((
                    winit::dpi::LogicalPosition::new(40.0, 8.0),
                    winit::dpi::LogicalSize::new(want.0, want.1),
                    false,
                )),
        };
        // macOS does not do client-side decorations the way Windows and Linux
        // do. Turning decorations off there removes the traffic lights along
        // with the title bar, which is not what a Mac user expects of a
        // window; the platform's own answer is a full-size content view --
        // content extends under a transparent title bar while the system keeps
        // drawing and placing the buttons. That is why this path exists and
        // why `titlebar_insets` is nonzero only here.
        //
        // Note what this does NOT do: pass `decorations = false` through. On
        // macOS that is borderless, which takes the traffic lights with it.
        // The frame stays decorated and the title bar is made transparent
        // instead, so the buttons remain exactly where a Mac user reaches for
        // them while the page draws behind them.
        #[cfg(target_os = "macos")]
        let (attributes, decorated) = {
            use winit::platform::macos::WindowAttributesExtMacOS;
            let attributes = Window::default_attributes();
            if self.options.effective_window_frame() == WindowFrame::Host {
                (attributes, true)
            } else {
                (
                    attributes
                        .with_titlebar_transparent(true)
                        .with_fullsize_content_view(true)
                        .with_title_hidden(true),
                    true,
                )
            }
        };
        #[cfg(not(target_os = "macos"))]
        let (attributes, decorated) = (
            Window::default_attributes(),
            self.options.effective_window_frame() == WindowFrame::Host,
        );
        let window = Arc::new(
            event_loop
                .create_window(
                    attributes
                        .with_title(self.options.title.clone())
                        .with_decorations(decorated)
                        .with_transparent(self.options.app_frame_is_transparent())
                        // Hidden until the a11y adapter is installed on the
                        // first frame — the Windows AccessKit adapter must
                        // attach before the window is shown. Revealed in
                        // `sync_a11y`.
                        .with_visible(false)
                        .with_position(initial_pos)
                        .with_inner_size(initial_size)
                        .with_maximized(initial_maximized),
                )
                .expect("create window"),
        );
        #[cfg(target_os = "windows")]
        if self.options.effective_window_frame() == WindowFrame::App {
            match windows_snap::SnapLayoutBridge::attach(&window) {
                Ok(bridge) => self.snap_layout = Some(bridge),
                Err(error) => {
                    eprintln!("[cambium-winit] could not install Snap Layout hit test: {error}")
                },
            }
        }
        #[cfg(target_os = "linux")]
        self.publish_x11_frame_extents(&window);
        if frame_trace() {
            #[cfg(target_os = "linux")]
            let backend = {
                use winit::platform::wayland::ActiveEventLoopExtWayland;
                if event_loop.is_wayland() {
                    "wayland"
                } else {
                    "x11"
                }
            };
            #[cfg(not(target_os = "linux"))]
            let backend = std::env::consts::OS;
            eprintln!(
                "[cambium-winit] window-frame backend={backend} policy={:?} decorated={} transparent={}",
                self.options.effective_window_frame(),
                window.is_decorated(),
                self.options.app_frame_is_transparent(),
            );
        }
        let size = window.inner_size();
        let surface = self
            .boot_surface(window.clone(), size.width.max(1), size.height.max(1))
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
        self.restored_geometry = restored;
        self.refresh_geometry();
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
            WindowEvent::CloseRequested => {
                self.refresh_geometry();
                self.request_close(CloseRequest::Native);
            },
            WindowEvent::Moved(_) => self.refresh_geometry(),
            WindowEvent::Resized(size) => {
                if let Some(surface) = self.s.surface.as_mut() {
                    surface.resize(size.width.max(1), size.height.max(1));
                }
                self.refresh_geometry();
                self.note_resize();
            },
            WindowEvent::ScaleFactorChanged { .. } => {
                #[cfg(target_os = "linux")]
                if let Some(window) = self.native_window.as_ref() {
                    self.publish_x11_frame_extents(window);
                }
                self.refresh_geometry();
                self.note_resize();
            },
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
                self.refresh_geometry();
                self.redraw();
                // After the frame is laid out and presented, refresh the
                // accessibility tree and drain any screen-reader actions,
                // then let the application pump per-presented-frame work.
                let a11y_started = std::time::Instant::now();
                self.sync_a11y();
                if let Some(profile) = self.s.last_frame_profile.as_mut() {
                    profile.a11y_us =
                        a11y_started.elapsed().as_micros().min(u64::MAX as u128) as u64;
                    profile.total_us = profile.total_us.saturating_add(profile.a11y_us);
                    if perf_trace() {
                        eprintln!("[cambium-host] frame {}", profile.summary());
                    }
                }
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
