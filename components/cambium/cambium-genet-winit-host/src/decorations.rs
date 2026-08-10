//! Client-side decorations: the window frame an application draws itself.
//!
//! Three seams, no pixels. The host owns the native calls and the platform
//! quirks; the application owns what the frame *looks* like, because a frame
//! is product identity. See `genet/docs/2026-08-10_window_decorations_brief.md`.
//!
//! 1. **Drag surfaces are CSS.** An application marks a title bar with
//!    `--app-region: drag`, and a control inside it with
//!    `--app-region: no-drag`. The host reads the value off the cascade at the
//!    hit node. Custom properties inherit, so a declaration on the bar reaches
//!    everything inside it and a descendant can carve a hole — the containment
//!    behaviour the Window Controls Overlay spec gives `app-region`, obtained
//!    without an ancestor walk.
//!
//!    **Why a custom property and not the real longhand.** The live cascade is
//!    the stylo fork, which livery is meant to retire; adding a longhand there
//!    would deepen exactly the divergence the decomposition is unwinding. The
//!    custom property cascades, respects selectors and media queries, and
//!    costs the fork nothing. When livery owns the cascade it gains
//!    `app-region` proper and [`app_region_of`] reads the longhand first,
//!    falling back to the custom property. Stylesheets written today keep
//!    working; see the brief's §5.
//!
//! 2. **Window verbs are a handle.** [`WindowCommands`] is a cheap cloneable
//!    queue the application stores in its own state and pushes to from an
//!    ordinary click handler. The host drains it after every dispatch. This
//!    replaces the four `bool` flags woodshed carried, which made every app
//!    add fields to its state and made the host know an app's shape.
//!
//! 3. **The muscle memory is free.** Once drag regions are known, the host can
//!    honour double-click-to-maximize and the system menu itself, on every
//!    platform, with no application involvement at all.

use std::cell::RefCell;
use std::rc::Rc;

use genet_scripted_dom::NodeId;

/// What a hit landed on, as far as the window frame is concerned.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AppRegion {
    /// Ordinary content. Not a drag surface.
    #[default]
    NoDrag,
    /// A window-drag surface: pressing here moves the window, double-clicking
    /// toggles maximize, right-clicking raises the system menu.
    Drag,
}

impl AppRegion {
    /// Parse a computed `--app-region` value. Anything unrecognized is
    /// [`AppRegion::NoDrag`]: a frame that drags when the author did not ask
    /// is worse than one that does not drag when they did.
    pub fn parse(value: &str) -> Self {
        match value.trim() {
            "drag" => Self::Drag,
            _ => Self::NoDrag,
        }
    }

    /// Whether this region drags the window.
    pub fn is_drag(self) -> bool {
        matches!(self, Self::Drag)
    }
}

/// A window verb an application asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowCommand {
    /// Minimize to the taskbar/dock.
    Minimize,
    /// Maximize, or restore if already maximized.
    ToggleMaximize,
    /// Begin a window move. Only valid while a pointer button is down; the
    /// host also issues this itself for a press on an [`AppRegion::Drag`]
    /// surface, so an application rarely needs it.
    Drag,
    /// Raise the platform's own window menu at the cursor.
    ShowSystemMenu,
    /// Close the window and end the application.
    Close,
}

/// The application's end of the window-verb seam.
///
/// Cheap to clone (one `Rc`), so an application stores it in its state and a
/// click handler calls a method on it:
///
/// ```ignore
/// clickable(el("div", text("×")), |ui: &mut Ui, _| ui.window.close())
/// ```
///
/// The host holds the other end and drains the queue after each dispatch, so
/// no window verb ever becomes a field the host has to know about.
#[derive(Clone, Default)]
pub struct WindowCommands(Rc<RefCell<Vec<WindowCommand>>>);

impl WindowCommands {
    /// A fresh, empty queue.
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a verb.
    pub fn push(&self, command: WindowCommand) {
        self.0.borrow_mut().push(command);
    }

    /// Minimize the window.
    pub fn minimize(&self) {
        self.push(WindowCommand::Minimize);
    }

    /// Maximize, or restore if already maximized.
    pub fn toggle_maximize(&self) {
        self.push(WindowCommand::ToggleMaximize);
    }

    /// Begin a window move (only meaningful with a button down).
    pub fn drag(&self) {
        self.push(WindowCommand::Drag);
    }

    /// Raise the platform's window menu.
    pub fn show_system_menu(&self) {
        self.push(WindowCommand::ShowSystemMenu);
    }

    /// Close the window.
    pub fn close(&self) {
        self.push(WindowCommand::Close);
    }

    /// Take everything queued, leaving the queue empty.
    pub fn drain(&self) -> Vec<WindowCommand> {
        std::mem::take(&mut *self.0.borrow_mut())
    }

    /// Whether anything is queued (for tests).
    pub fn is_empty(&self) -> bool {
        self.0.borrow().is_empty()
    }
}

impl std::fmt::Debug for WindowCommands {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("WindowCommands").field(&*self.0.borrow()).finish()
    }
}

/// Where a window was, so it can open there again.
///
/// The host validates a restored geometry against the monitors that actually
/// exist before using it, so a window last seen on a since-unplugged display
/// does not open off-screen. Persisting it is the application's job — the host
/// has no storage and should not grow one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowGeometry {
    /// Logical position of the top-left corner.
    pub position: (f64, f64),
    /// Logical size.
    pub size: (f64, f64),
    /// Whether the window was maximized. Position and size then describe the
    /// restored (un-maximized) geometry, which is what the platform restores
    /// to.
    pub maximized: bool,
}

impl WindowGeometry {
    /// Whether this geometry puts a usable amount of the window's title area
    /// inside at least one of `monitors`, each given as a logical
    /// `(x, y, width, height)` work area.
    ///
    /// The test is deliberately about the *top* strip rather than the whole
    /// rectangle: a window whose title bar is reachable can be moved back by
    /// hand, and one whose title bar is off-screen cannot.
    pub fn is_reachable_on(&self, monitors: &[(f64, f64, f64, f64)]) -> bool {
        /// Horizontal run of title bar that must be on screen: about a
        /// grabbable width.
        const NEEDED_ACROSS: f64 = 48.0;
        /// Vertical slice that must be on screen. A single visible pixel of
        /// title bar is not something a person can grab, so this is a real
        /// strip rather than a nonzero test.
        const NEEDED_DOWN: f64 = 8.0;
        /// A plausible title-bar height to measure against; the geometry does
        /// not record the app's own bar height and does not need to.
        const BAR: f64 = 32.0;

        let (x, y) = self.position;
        let (w, _h) = self.size;
        monitors.iter().any(|&(mx, my, mw, mh)| {
            let across = (x + w).min(mx + mw) - x.max(mx);
            let down = (y + BAR).min(my + mh) - y.max(my);
            across >= NEEDED_ACROSS && down >= NEEDED_DOWN
        })
    }
}

/// Detects double-clicks the way the platforms do: two presses close together
/// in both time and space.
///
/// winit reports presses, not clicks, and no platform-independent
/// double-click signal exists, so the host keeps this small clock itself.
pub(crate) struct ClickCadence {
    last: Option<(std::time::Instant, (f32, f32))>,
}

impl ClickCadence {
    /// Windows' default double-click time; macOS and the common Linux
    /// desktops all sit near it.
    const INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);
    /// Slop, in logical pixels. Generous enough for a shaky hand, tight
    /// enough that two deliberate clicks on different controls never merge.
    const SLOP: f32 = 4.0;

    pub(crate) fn new() -> Self {
        Self { last: None }
    }

    /// Record a press and report whether it completed a double-click. A
    /// double-click consumes the cadence, so a third press starts over rather
    /// than reporting a second double.
    pub(crate) fn press(&mut self, at: (f32, f32), now: std::time::Instant) -> bool {
        let doubled = self.last.is_some_and(|(t, p)| {
            now.duration_since(t) <= Self::INTERVAL
                && (p.0 - at.0).abs() <= Self::SLOP
                && (p.1 - at.1).abs() <= Self::SLOP
        });
        self.last = if doubled { None } else { Some((now, at)) };
        doubled
    }
}

/// Read `--app-region` for a laid-out node.
///
/// Prefers the standard `app-region` longhand and falls back to the custom
/// property, so the day livery implements the real property this keeps
/// working and stylesheets written against either spelling are honoured.
pub(crate) fn app_region_of(
    layout: &genet_layout::IncrementalLayout<NodeId>,
    node: NodeId,
) -> AppRegion {
    layout
        .computed_value(node, "app-region")
        .or_else(|| layout.computed_custom_property(node, "app-region"))
        .map(|v| AppRegion::parse(&v))
        .unwrap_or_default()
}

impl<State, Logic, V> crate::Host<State, Logic, V>
where
    State: 'static,
    Logic: FnMut(&State) -> V + 'static,
    V: crate::meristem_bounds::RootView<State>,
{
    /// What the window frame makes of the point `(x, y)` in logical
    /// coordinates.
    pub(crate) fn app_region_at(&self, x: f32, y: f32) -> AppRegion {
        let (Some(runner), Some(layout)) = (self.s.runner.as_ref(), self.s.layout.as_ref())
        else {
            return AppRegion::NoDrag;
        };
        let dom = runner.dom();
        let dom_ref = dom.borrow();
        let Some(node) =
            layout.hit_test(&*dom_ref, x, y, &genet_layout::ScrollOffsets::default())
        else {
            return AppRegion::NoDrag;
        };
        app_region_of(layout, node)
    }

    /// A left press that was not an edge grab.
    ///
    /// The DOM sees it first in every case, so a drag surface can still take
    /// focus and a `no-drag` control inside a title bar keeps its click. Only
    /// then does the frame act, and only if no handler called
    /// `prevent_default` — which is how an application vetoes a drag it does
    /// not want (a slider living inside the bar, say).
    pub(crate) fn press_left(&mut self) {
        let (x, y) = self.s.cursor;
        let region = self.app_region_at(x, y);
        let doubled = self.s.cadence.press((x, y), std::time::Instant::now());

        self.click();

        if !region.is_drag() {
            return;
        }
        if self
            .s
            .runner
            .as_ref()
            .is_some_and(|runner| runner.default_prevented())
        {
            return;
        }
        if doubled {
            // Double-click a title bar to maximize: universal muscle memory,
            // and free once drag regions are known.
            self.perform(WindowCommand::ToggleMaximize);
        } else {
            self.perform(WindowCommand::Drag);
        }
    }

    /// A right press. On a drag surface this raises the platform's own window
    /// menu, the way right-clicking a real title bar does.
    pub(crate) fn press_right(&mut self) {
        let (x, y) = self.s.cursor;
        if self.app_region_at(x, y).is_drag() {
            self.perform(WindowCommand::ShowSystemMenu);
        }
    }

    /// Run one window verb against the real window.
    pub(crate) fn perform(&mut self, command: WindowCommand) {
        self.s.performed.push(command);
        let Some(window) = self.s.window.as_ref() else {
            // Windowless (the `Harness`). Verbs are still drained so a test
            // can assert an application asked for one; there is nothing to
            // act on.
            if matches!(command, WindowCommand::Close) {
                self.s.close_requested = true;
            }
            return;
        };
        match command {
            WindowCommand::Minimize => window.set_minimized(true),
            WindowCommand::ToggleMaximize => window.set_maximized(!window.is_maximized()),
            // Both of these hand control to the platform for the duration of
            // the gesture; they only mean anything while the press that asked
            // for them is still down, which is why the press path calls them
            // inline rather than queueing.
            WindowCommand::Drag => {
                let _ = window.drag_window();
            }
            WindowCommand::ShowSystemMenu => {
                let (x, y) = self.s.cursor;
                window.show_window_menu(winit::dpi::Position::Logical(
                    winit::dpi::LogicalPosition::new(x as f64, y as f64),
                ));
            }
            WindowCommand::Close => self.s.close_requested = true,
        }
    }

    /// Drain and run whatever the application queued this dispatch.
    pub(crate) fn run_window_commands(&mut self) {
        for command in self.s.commands.drain() {
            self.perform(command);
        }
    }

    /// The application's end of the window-verb seam, for storing in state.
    pub(crate) fn commands(&self) -> WindowCommands {
        self.s.commands.clone()
    }

    /// Where the window is now, for an application that wants to persist it.
    ///
    /// Reports the *restored* geometry when maximized, because that is what
    /// the platform will restore to and therefore what is worth remembering.
    pub(crate) fn geometry(&self) -> Option<WindowGeometry> {
        let window = self.s.window.as_ref()?;
        let scale = window.scale_factor();
        let maximized = window.is_maximized();
        let position = window
            .outer_position()
            .map(|p| (p.x as f64 / scale, p.y as f64 / scale))
            .unwrap_or((0.0, 0.0));
        let size = window.inner_size();
        Some(WindowGeometry {
            position,
            size: (size.width as f64 / scale, size.height as f64 / scale),
            maximized,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_regions_do_not_drag() {
        assert_eq!(AppRegion::parse("drag"), AppRegion::Drag);
        assert_eq!(AppRegion::parse(" drag "), AppRegion::Drag);
        assert_eq!(AppRegion::parse("no-drag"), AppRegion::NoDrag);
        assert_eq!(AppRegion::parse("inherit"), AppRegion::NoDrag);
        assert_eq!(AppRegion::parse(""), AppRegion::NoDrag);
    }

    #[test]
    fn commands_queue_and_drain_once() {
        let commands = WindowCommands::new();
        assert!(commands.is_empty());
        commands.minimize();
        commands.close();

        assert_eq!(
            commands.drain(),
            vec![WindowCommand::Minimize, WindowCommand::Close]
        );
        assert!(commands.is_empty(), "a drain empties the queue");
        assert!(commands.drain().is_empty());
    }

    #[test]
    fn a_clone_shares_the_queue() {
        // The whole point: the application stores one clone in its state and
        // the host keeps another.
        let host_side = WindowCommands::new();
        let app_side = host_side.clone();
        app_side.toggle_maximize();
        assert_eq!(host_side.drain(), vec![WindowCommand::ToggleMaximize]);
    }

    #[test]
    fn double_click_needs_both_time_and_proximity() {
        let base = std::time::Instant::now();
        let soon = base + std::time::Duration::from_millis(120);
        let late = base + std::time::Duration::from_millis(900);

        let mut cadence = ClickCadence::new();
        assert!(!cadence.press((10.0, 10.0), base), "one press is not two");
        assert!(cadence.press((11.0, 10.0), soon), "close in time and space");

        let mut cadence = ClickCadence::new();
        cadence.press((10.0, 10.0), base);
        assert!(!cadence.press((10.0, 10.0), late), "too slow");

        let mut cadence = ClickCadence::new();
        cadence.press((10.0, 10.0), base);
        assert!(!cadence.press((40.0, 10.0), soon), "too far");
    }

    #[test]
    fn a_double_click_does_not_chain_into_a_third() {
        let base = std::time::Instant::now();
        let mut cadence = ClickCadence::new();
        cadence.press((0.0, 0.0), base);
        assert!(cadence.press((0.0, 0.0), base + std::time::Duration::from_millis(50)));
        assert!(
            !cadence.press((0.0, 0.0), base + std::time::Duration::from_millis(100)),
            "the third press starts a new cadence",
        );
    }

    #[test]
    fn geometry_off_every_monitor_is_unreachable() {
        let laptop = (0.0, 0.0, 1920.0, 1040.0);

        let on = WindowGeometry {
            position: (100.0, 100.0),
            size: (800.0, 600.0),
            maximized: false,
        };
        assert!(on.is_reachable_on(&[laptop]));

        // The classic: last seen on a second monitor that is now unplugged.
        let orphaned = WindowGeometry {
            position: (2600.0, 300.0),
            size: (800.0, 600.0),
            maximized: false,
        };
        assert!(!orphaned.is_reachable_on(&[laptop]));
        assert!(
            orphaned.is_reachable_on(&[laptop, (1920.0, 0.0, 1920.0, 1040.0)]),
            "plug the monitor back in and it is fine",
        );

        // Dragged almost entirely off the bottom: the title bar is gone, so
        // there is nothing left to grab.
        let sunk = WindowGeometry {
            position: (100.0, 1039.0),
            size: (800.0, 600.0),
            maximized: false,
        };
        assert!(!sunk.is_reachable_on(&[laptop]));
    }
}
