//! The host proper: lifecycle, state, hooks, and the application-facing
//! context, with no event source of its own.
//!
//! What a winit window and a browser canvas have in common lives here. What
//! differs is grafted on: an event source converts platform events into this
//! crate's vocabulary and drives the methods below.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cambium::{GenetAppRunner, TextInput};
use cambium_winit::ScrollbarFade;
use genet_layout::{IncrementalLayout, ScrollTarget};
use genet_scripted_dom::{NodeId, ScriptedDom};
use netrender::NetrenderOptions;

use crate::meristem_bounds::RootView;
use crate::wake::HostWake;
use crate::{
    Accessibility, HostWindow, Instant, Surface, WindowCommand, WindowCommands, WindowGeometry,
};
use crate::{KeyPress, Modifiers};

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
pub type CaptureFn = Box<dyn FnOnce(&dyn Surface, &wgpu::TextureView, u32, u32) + 'static>;

/// Which layer draws the window frame.
///
/// This is the application-visible boundary. A [`Host`](Self::Host) frame may
/// be compositor-drawn or supplied by the platform window library; the
/// application leaves both forms alone. An [`App`](Self::App) frame is part of
/// the application's own view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WindowFrame {
    /// Let the platform host draw and operate the frame.
    #[default]
    Host,
    /// Let the application draw the frame; the host supplies window verbs and
    /// resize affordances.
    App,
}

/// Transparent margins reserved inside an application-drawn window frame.
///
/// These are logical pixels, matching the application's CSS coordinate space.
/// The application draws its frame and effects inside them. The desktop host
/// supplies an alpha-capable surface and, on X11, publishes the corresponding
/// device-pixel `_GTK_FRAME_EXTENTS` so the window manager snaps and maximizes
/// against the visible frame rather than the transparent shadow boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AppFrameInsets {
    pub left: u32,
    pub right: u32,
    pub top: u32,
    pub bottom: u32,
}

impl AppFrameInsets {
    /// A frame with no transparent outer effect.
    pub const NONE: Self = Self {
        left: 0,
        right: 0,
        top: 0,
        bottom: 0,
    };

    /// The same margin on every edge.
    pub const fn uniform(value: u32) -> Self {
        Self {
            left: value,
            right: value,
            top: value,
            bottom: value,
        }
    }

    pub const fn is_empty(self) -> bool {
        self.left == 0 && self.right == 0 && self.top == 0 && self.bottom == 0
    }
}

/// Window and pipeline configuration.
pub struct HostOptions {
    /// The window title.
    pub title: String,
    /// The layer responsible for the window frame.
    pub window_frame: WindowFrame,
    /// Compatibility input for existing consumers. `false` selects
    /// [`WindowFrame::App`]; `true` leaves [`window_frame`](Self::window_frame)
    /// in control.
    ///
    /// New consumers should set `window_frame` directly. This field can retire
    /// after the existing Woodshed consumer moves off the boolean API.
    pub decorations: bool,
    /// Transparent margins occupied by effects around an app-drawn frame.
    ///
    /// This is geometry, not a shared visual style: the application still
    /// chooses and draws its own shadow. Keep these values equal to the outer
    /// transparent margins in that application stylesheet. They are ignored
    /// when the effective [`WindowFrame`] is [`WindowFrame::Host`].
    pub app_frame_insets: AppFrameInsets,
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
            window_frame: WindowFrame::Host,
            decorations: true,
            app_frame_insets: AppFrameInsets::NONE,
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

impl HostOptions {
    /// Resolve the named policy plus the source-compatible boolean input.
    pub fn effective_window_frame(&self) -> WindowFrame {
        if self.decorations {
            self.window_frame
        } else {
            WindowFrame::App
        }
    }

    /// Insets that actually participate in this window's frame policy.
    pub fn effective_app_frame_insets(&self) -> AppFrameInsets {
        if self.effective_window_frame() == WindowFrame::App {
            self.app_frame_insets
        } else {
            AppFrameInsets::NONE
        }
    }

    /// Whether the native window and presentation surface need alpha.
    pub fn app_frame_is_transparent(&self) -> bool {
        !self.effective_app_frame_insets().is_empty()
    }
}

#[cfg(test)]
mod window_frame_tests {
    use super::{AppFrameInsets, HostOptions, WindowFrame};

    #[test]
    fn host_frame_is_the_default() {
        assert_eq!(
            HostOptions::default().effective_window_frame(),
            WindowFrame::Host
        );
    }

    #[test]
    fn an_app_frame_is_explicit() {
        let options = HostOptions {
            window_frame: WindowFrame::App,
            ..Default::default()
        };
        assert_eq!(options.effective_window_frame(), WindowFrame::App);
    }

    #[test]
    fn the_old_false_input_still_selects_an_app_frame() {
        let options = HostOptions {
            decorations: false,
            ..Default::default()
        };
        assert_eq!(options.effective_window_frame(), WindowFrame::App);
    }

    #[test]
    fn app_frame_insets_only_apply_to_an_app_frame() {
        let insets = AppFrameInsets::uniform(12);
        let host = HostOptions {
            app_frame_insets: insets,
            ..Default::default()
        };
        assert_eq!(host.effective_app_frame_insets(), AppFrameInsets::NONE);
        assert!(!host.app_frame_is_transparent());

        let app = HostOptions {
            window_frame: WindowFrame::App,
            app_frame_insets: insets,
            ..Default::default()
        };
        assert_eq!(app.effective_app_frame_insets(), insets);
        assert!(app.app_frame_is_transparent());
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
    /// The frame, behind the neutral seam rather than as a winit handle.
    ///
    /// Applications ask it for redraws, size and scale, all of which a browser
    /// answers. Naming `&Window` here would have made the hook signature itself
    /// undeliverable on a second event source.
    pub window: Option<&'a dyn HostWindow>,
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
    Box<dyn FnOnce(&dyn HostWindow, &WindowCommands, &HostWake) -> Init<State, Logic>>;

/// A logical window dimension from the environment.
pub fn env_size(key: &str) -> Option<f64> {
    std::env::var(key)
        .ok()?
        .parse::<f64>()
        .ok()
        .filter(|v| *v > 0.0)
}

pub struct HostState<State: 'static, Logic, V>
where
    Logic: FnMut(&State) -> V,
    V: RootView<State>,
{
    /// The frame behind the neutral seam: redraw, size, scale, IME. A browser
    /// event source supplies a canvas here.
    pub window: Option<Box<dyn HostWindow>>,
    /// The last published titlebar area and the width it was computed for, so
    /// the sheet is rebuilt only when one of them actually moves.
    pub titlebar_published: Option<(crate::TitlebarInsets, f32)>,
    /// The `:root` rule carrying the published custom properties. Empty until
    /// the first publish, which is a valid stylesheet.
    pub titlebar_sheet: String,
    /// The presentation surface behind the neutral seam. A browser event
    /// source supplies the same pair against a canvas.
    pub surface: Option<Box<dyn Surface>>,
    pub runner: Option<Runner<State, Logic, V>>,
    /// Retained layout session in logical coordinates — hit-test target and
    /// incremental-apply subject.
    pub layout: Option<IncrementalLayout<NodeId>>,
    pub layout_size: (f32, f32),
    pub sheet: String,
    pub leaves: sprigging::LeafRegistry<u64>,
    pub rendered: sprigging::RenderedLeaves,
    /// Netrender roadmap E4 — leaf key → (retained `FragmentId`, epoch it was
    /// translated at). Synced against `rendered` each redraw while a surface
    /// (and therefore a renderer registry) exists; cleared when the surface is
    /// gone, since the registry died with it. When a key is here,
    /// `emit_paint_list_with_leaves` places a marker instead of splicing the
    /// leaf's commands, and the renderer composes the cached lowering.
    pub leaf_fragments: std::collections::HashMap<u64, (u64, u64)>,
    /// Cursor position in logical coordinates.
    pub cursor: (f32, f32),
    /// Live modifier state, in the neutral vocabulary. Winit's own state is
    /// converted once, where it arrives, so nothing downstream reads it.
    pub modifiers: Modifiers,
    /// The node whose text field anchors an active drag selection.
    pub text_drag: Option<NodeId>,
    /// Opaque ids for `:hover` / `:focus` restyles on target change.
    pub last_hover: Option<u64>,
    pub last_focus: Option<u64>,
    /// The hovered hit node, for `on_hover` Enter/Leave routing.
    pub last_hover_hit: Option<NodeId>,
    /// Monotonic base for the CSS-transition animation clock.
    pub anim_base: crate::Instant,
    /// The accessibility seam, behind the neutral trait rather than named as
    /// AccessKit. `HostState` holds no winit or AccessKit type through it, so
    /// this field is ready to move with the struct; a browser event source
    /// supplies a projection onto the live document instead.
    pub a11y: Option<Box<dyn Accessibility>>,
    pub(crate) a11y_wake: Arc<AtomicBool>,
    /// Whether presentation is suspended: a close policy hid the root window,
    /// or, on a future browser source, the tab went background. Portable fact;
    /// only *how* a host hides (set_visible, document.hidden) is platform.
    pub hidden: bool,
    /// The application's end of the window-verb seam. The queue is plain data
    /// (`Rc<RefCell<Vec<WindowCommand>>>`, no platform type in it), so it lives
    /// with the host and every event source shares it; *draining* it is the
    /// event source's job, since honouring a verb needs a real window.
    pub commands: WindowCommands,
    /// Where the window is and how big, refreshed by the event source. A
    /// snapshot rather than a live query so the host can hand it to hooks
    /// without asking a window it does not own.
    pub geometry: Option<WindowGeometry>,
    /// Set by [`HostWake`] until the event loop gives the application one drain
    /// turn. Kept in host state so the harness can exercise the same coalescing
    /// contract without a native event loop.
    pub wake_pending: Arc<AtomicBool>,
    pub scrollbar_fade: ScrollbarFade<ScrollTarget<NodeId>>,
    pub close_requested: bool,
    pub pending_sheet: Option<String>,
    pub pending_capture: Option<CaptureFn>,
    /// Pointer events an application hook asked the host to deliver to itself,
    /// drained through the real input path once the hook returns.
    pub pending_pointer: Vec<HostPointer>,
    /// Tab is being held: the arrow keys steer focus instead of reaching the
    /// focused element. Set by Tab's first key-repeat, cleared on its release.
    pub tab_held: bool,
}

impl<State, Logic, V> HostState<State, Logic, V>
where
    State: 'static,
    Logic: FnMut(&State) -> V,
    V: RootView<State>,
{
    pub fn new() -> Self {
        Self {
            window: None,
            titlebar_published: None,
            titlebar_sheet: String::new(),
            surface: None,
            runner: None,
            layout: None,
            layout_size: (0.0, 0.0),
            sheet: String::new(),
            leaves: sprigging::LeafRegistry::new(),
            rendered: sprigging::RenderedLeaves::new(),
            leaf_fragments: std::collections::HashMap::new(),
            cursor: (0.0, 0.0),
            modifiers: Modifiers::NONE,
            text_drag: None,
            last_hover: None,
            last_focus: None,
            last_hover_hit: None,
            anim_base: crate::Instant::now(),
            a11y: None,
            a11y_wake: Arc::new(AtomicBool::new(false)),
            hidden: false,
            commands: WindowCommands::new(),
            geometry: None,
            wake_pending: Arc::new(AtomicBool::new(false)),
            scrollbar_fade: ScrollbarFade::new(),
            close_requested: false,
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
    pub options: HostOptions,
    pub init: Option<InitFn<State, Logic>>,
    pub hooks: HostHooks<State, Logic, V>,
    pub s: HostState<State, Logic, V>,
    pub wake: HostWake,
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

impl<State, Logic, V> Host<State, Logic, V>
where
    State: 'static,
    Logic: FnMut(&State) -> V + 'static,
    V: RootView<State>,
{
    /// The application's handle on the window-verb queue. Core data: pushing a
    /// verb is portable, honouring one is the event source's problem.
    pub fn commands(&self) -> WindowCommands {
        self.s.commands.clone()
    }

    pub fn scale_factor(&self) -> f64 {
        self.s.window.as_ref().map_or(1.0, |w| w.scale_factor())
    }

    /// The logical size being laid out: the window's, or — windowless, under
    /// [`Harness`] — the size the retained layout was last built at.
    pub fn logical_size(&self) -> (f32, f32) {
        match self.s.window.as_ref() {
            Some(window) => {
                let size = window.inner_size();
                let scale = window.scale_factor() as f32;
                (size.0.max(1) as f32 / scale, size.1.max(1) as f32 / scale)
            },
            None => self.s.layout_size,
        }
    }

    /// Run one application hook with the standard context, then apply any
    /// host-owned requests it made (sheet swap → relayout, queued pointer
    /// events → the real input path).
    pub fn with_ctx(&mut self, which: Hook) {
        {
            let logical_size = self.logical_size();
            let geometry = self.s.geometry;
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
    pub fn process_wake(&mut self) -> bool {
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
    /// Run the application's close policy and record what the core can act on
    /// itself ([`CloseDisposition::Exit`] sets the exit flag). `None` when there
    /// is no runner yet, which also sets the flag: closing an app that never
    /// booted should succeed. Reacting to `Hide` and `KeepVisible` needs a real
    /// window, so that half lives with the event source.
    pub fn decide_close(&mut self, request: CloseRequest) -> Option<CloseDisposition> {
        let disposition = {
            let logical_size = self.logical_size();
            let geometry = self.s.geometry;
            let commands = self.s.commands.clone();
            let window = self.s.window.as_deref();
            let Some(runner) = self.s.runner.as_mut() else {
                self.s.close_requested = true;
                return None;
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
            return None;
        }
        if matches!(disposition, CloseDisposition::Exit) {
            self.s.close_requested = true;
        }
        Some(disposition)
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
    pub fn drain_pointer(&mut self) {
        if self.s.pending_pointer.is_empty() {
            return;
        }
        for event in std::mem::take(&mut self.s.pending_pointer) {
            match event {
                HostPointer::Moved(x, y) => {
                    self.pointer_moved(x, y);
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
    pub fn after_dispatch(&mut self) {
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

pub enum Hook {
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
    pub fn a11y_waker(&self) -> impl Fn() + Send + Sync + 'static {
        let wake = self.s.a11y_wake.clone();
        move || wake.store(true, Ordering::Relaxed)
    }

    /// Decide what the event loop does on an idle turn, consuming the
    /// accessibility wake flag if one is set. Factored out of `about_to_wait`
    /// so the wake path is assertable: `Wait` really means nothing is pending,
    /// and a raised wake really becomes a repaint rather than being swallowed.
    pub fn idle_policy(&mut self, now: crate::Instant) -> IdlePolicy {
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
