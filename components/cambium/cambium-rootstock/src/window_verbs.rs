//! The window-verb vocabulary: what an application can ask a frame for.
//!
//! Data, not behaviour. Pushing a verb is portable, which is why the queue
//! lives here and is handed to every hook; honouring one needs a real window,
//! so the draining and the doing belong to an event source. A browser host
//! carries a queue it never drains, and the hook signature does not change
//! between the two.

use std::cell::RefCell;
use std::rc::Rc;

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
///
/// `PartialEq` but not `Eq`: [`Resize`](WindowCommand::Resize) carries logical
/// pixels, which are floats.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WindowCommand {
    /// Reveal and redraw a root window that a close policy hid.
    Show,
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
    /// Ask for a new inner size, in logical pixels.
    ///
    /// A request, not a command: a window manager may refuse it, and a browser
    /// tab has nothing to resize. Verbs go through this queue rather than a raw
    /// handle so a host that cannot honour one simply does not, instead of the
    /// application holding a window type that only one host can supply.
    Resize(f64, f64),
    /// Ask the application to close. Its [`CloseDisposition`](crate::CloseDisposition)
    /// decides whether this exits, hides, or keeps the window visible.
    Close,
}

/// The application's end of the window-verb seam.
///
/// Cheap to clone (one `Rc`), so an application stores it in its state and a
/// click handler calls a method on it:
///
/// ```ignore
/// clickable(el("div", text("Ã—")), |ui: &mut Ui, _| ui.window.close())
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

    /// Reveal a window previously hidden by a close policy.
    pub fn show(&self) {
        self.push(WindowCommand::Show);
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

    /// Ask the application's close policy to close the window.
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
        f.debug_tuple("WindowCommands")
            .field(&*self.0.borrow())
            .finish()
    }
}

/// Where a window was, so it can open there again.
///
/// The host validates a restored geometry against the monitors that actually
/// exist before using it, so a window last seen on a since-unplugged display
/// does not open off-screen. Persisting it is the application's job â€” the host
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
        let (w, h) = self.size;
        if ![x, y, w, h].into_iter().all(f64::is_finite) || w <= 0.0 || h <= 0.0 {
            return false;
        }
        monitors.iter().any(|&(mx, my, mw, mh)| {
            let across = (x + w).min(mx + mw) - x.max(mx);
            let down = (y + BAR).min(my + mh) - y.max(my);
            across >= NEEDED_ACROSS && down >= NEEDED_DOWN
        })
    }
}
