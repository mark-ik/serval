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

use cambium_rootstock::TitlebarInsets;
use genet_scripted_dom::NodeId;
use layout_dom_api::{LayoutDom, LocalName, Namespace};

use crate::{AppRegion, WindowCommand, WindowGeometry};

fn remember_geometry(previous: Option<WindowGeometry>, current: WindowGeometry) -> WindowGeometry {
    if current.maximized {
        let mut restored = previous.unwrap_or(current);
        restored.maximized = true;
        restored
    } else {
        current
    }
}

#[cfg(test)]
use crate::WindowCommands;

/// Detects double-clicks the way the platforms do: two presses close together
/// in both time and space.
///
/// winit reports presses, not clicks, and no platform-independent
/// double-click signal exists, so the host keeps this small clock itself.
pub struct ClickCadence {
    last: Option<(cambium_rootstock::Instant, (f32, f32))>,
}

impl ClickCadence {
    /// Windows' default double-click time; macOS and the common Linux
    /// desktops all sit near it.
    const INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);
    /// Slop, in logical pixels. Generous enough for a shaky hand, tight
    /// enough that two deliberate clicks on different controls never merge.
    const SLOP: f32 = 4.0;

    pub fn new() -> Self {
        Self { last: None }
    }

    /// Record a press and report whether it completed a double-click. A
    /// double-click consumes the cadence, so a third press starts over rather
    /// than reporting a second double.
    pub fn press(&mut self, at: (f32, f32), now: cambium_rootstock::Instant) -> bool {
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
pub(crate) fn app_region_of(layout: &cambium_rootstock::OwnedLayout, node: NodeId) -> AppRegion {
    layout
        .computed_value(node, "app-region")
        .or_else(|| layout.computed_custom_property(node, "app-region"))
        .map(|v| AppRegion::parse(&v))
        .unwrap_or_default()
}

/// Locate an accessible window control.
///
/// Reuses W2's button semantics and accessible label rather than adding a
/// second native-only marker. A stylesheet remains free to size, order, or
/// relocate its frame; the bridge follows the same retained box that paint,
/// accessibility and pointer routing use.
fn window_control_node<D>(dom: &D, root: NodeId, accessible_label: &str) -> Option<NodeId>
where
    D: LayoutDom<NodeId = NodeId>,
{
    fn find<D>(dom: &D, node: NodeId, button: &LocalName, accessible_label: &str) -> Option<NodeId>
    where
        D: LayoutDom<NodeId = NodeId>,
    {
        let role = dom.attribute(node, &Namespace::from(""), &LocalName::from("role"));
        let label = dom.attribute(node, &Namespace::from(""), &LocalName::from("aria-label"));
        let button_element = dom
            .element_name(node)
            .is_some_and(|name| &name.local == button);
        if (button_element || role.is_some_and(|value| value.eq_ignore_ascii_case("button")))
            && label.is_some_and(|value| value == accessible_label)
        {
            return Some(node);
        }
        dom.dom_children(node)
            .find_map(|child| find(dom, child, button, accessible_label))
    }

    find(dom, root, &LocalName::from("button"), accessible_label)
}

/// The current painted client rect of the named window control.
pub(crate) fn window_control_rect<D>(
    dom: &D,
    root: NodeId,
    layout: &cambium_rootstock::OwnedLayout,
    accessible_label: &str,
) -> Option<(f32, f32, f32, f32)>
where
    D: LayoutDom<NodeId = NodeId>,
{
    let node = window_control_node(dom, root, accessible_label)?;
    layout.painted_rect(dom, node)
}

/// The client-side window frame. Inherent on the winit wrapper: every method
/// here needs a native window, which is exactly what the wrapper adds to the
/// host. Nothing here has a browser meaning.
impl<State, Logic, V> crate::WinitHost<State, Logic, V>
where
    State: 'static,
    Logic: FnMut(&State) -> V + 'static,
    V: cambium_rootstock::meristem_bounds::RootView<State>,
{
    /// What the window frame makes of the point `(x, y)` in logical
    /// coordinates.
    pub(crate) fn app_region_at(&self, x: f32, y: f32) -> AppRegion {
        // The hit test is the host's; what the frame makes of the node is ours.
        let (Some(node), Some(layout)) = (self.hit_at(x, y), self.layout()) else {
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
        let (x, y) = self.cursor();
        let region = self.app_region_at(x, y);
        let doubled = self
            .cadence
            .press((x, y), cambium_rootstock::Instant::now());

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

    /// A right press.
    ///
    /// Shaped exactly like [`press_left`](Self::press_left): the DOM sees it
    /// first — the `on_pointer` element under the cursor gets a `Down` marked
    /// [`PointerButton::Secondary`](cambium::PointerButton::Secondary), which
    /// is how a view opens a context menu — and only then does the frame act.
    /// On a drag surface that means the platform's own window menu, the way
    /// right-clicking a real title bar does, unless a handler called
    /// `prevent_default` to keep the press for itself.
    ///
    /// The press is one-shot: it begins no capture, so there is no right
    /// release path to match it. See
    /// [`Host::secondary_press`](cambium_rootstock::Host::secondary_press).
    pub(crate) fn press_right(&mut self) {
        let (x, y) = self.cursor();
        let region = self.app_region_at(x, y);

        self.secondary_press();

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
        self.perform(WindowCommand::ShowSystemMenu);
    }

    /// Run one window verb against the real window.
    pub(crate) fn perform(&mut self, command: WindowCommand) {
        self.performed.push(command);
        if let WindowCommand::Resize(w, h) = command {
            if let Some(window) = self.native_window.as_ref() {
                let _ = window.request_inner_size(winit::dpi::LogicalSize::new(w, h));
                window.request_redraw();
            }
            return;
        }
        if matches!(command, WindowCommand::Close) {
            self.refresh_geometry();
            self.request_close(crate::CloseRequest::Command);
            return;
        }
        let Some(window) = self.native_window.as_ref() else {
            // Windowless (the `Harness`). Verbs are still drained so a test
            // can assert an application asked for one; there is nothing to
            // act on.
            if matches!(command, WindowCommand::Show) {
                self.s.hidden = false;
            }
            return;
        };
        match command {
            // Handled above, before the window is unwrapped.
            WindowCommand::Resize(..) => {},
            WindowCommand::Show => {
                window.set_visible(true);
                window.set_minimized(false);
                window.request_redraw();
                self.s.hidden = false;
            },
            WindowCommand::Minimize => window.set_minimized(true),
            WindowCommand::ToggleMaximize => window.set_maximized(!window.is_maximized()),
            // Both of these hand control to the platform for the duration of
            // the gesture; they only mean anything while the press that asked
            // for them is still down, which is why the press path calls them
            // inline rather than queueing.
            WindowCommand::Drag => {
                let _ = window.drag_window();
            },
            WindowCommand::ShowSystemMenu => {
                // The cursor is in layout coordinates and winit's `Logical`
                // position is the platform's, which differ by the zoom.
                let (x, y) = self.cursor();
                let zoom = f64::from(self.ui_zoom());
                window.show_window_menu(winit::dpi::Position::Logical(
                    winit::dpi::LogicalPosition::new(x as f64 * zoom, y as f64 * zoom),
                ));
            },
            WindowCommand::Close => unreachable!("close returned above"),
        }
    }

    /// Drain and run whatever the application queued this dispatch.
    pub(crate) fn run_window_commands(&mut self) {
        for command in self.s.commands.drain() {
            self.perform(command);
        }
    }

    /// Where the native window is now. Maximized state is folded onto the last
    /// floating rectangle by [`Self::refresh_geometry`].
    pub(crate) fn geometry(&self) -> Option<WindowGeometry> {
        let window = self.native_window.as_ref()?;
        // Windows reports a minimized window at a sentinel off-screen
        // position. Keep the last useful snapshot instead of persisting it.
        if window.is_minimized() == Some(true) {
            return None;
        }
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

    /// Refresh the application-facing snapshot without replacing a maximized
    /// window's floating restore rectangle with its screen-filling rectangle.
    pub(crate) fn refresh_geometry(&mut self) {
        let Some(current) = self.geometry() else {
            return;
        };
        self.restored_geometry = Some(remember_geometry(self.restored_geometry, current));
        self.core.s.geometry = self.restored_geometry;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genet_scripted_dom::ScriptedDom;
    use layout_dom_api::{LayoutDomMut, QualName};

    fn qual(local: &str) -> QualName {
        QualName::new(None, Namespace::from(""), LocalName::from(local))
    }

    #[test]
    fn native_control_lookup_uses_html_button_semantics_and_accessible_name() {
        let mut dom = ScriptedDom::new();
        let root = dom.document();
        let minimize = dom.create_element(qual("button"));
        dom.set_attribute(minimize, qual("aria-label"), "Minimize");
        dom.append_child(root, minimize);
        let maximize = dom.create_element(qual("button"));
        dom.set_attribute(maximize, qual("aria-label"), "Maximize");
        dom.append_child(root, maximize);

        assert_eq!(window_control_node(&dom, root, "Maximize"), Some(maximize));
        assert_eq!(window_control_node(&dom, root, "Agrandir"), None);
    }

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
        let base = cambium_rootstock::Instant::now();
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
        let base = cambium_rootstock::Instant::now();
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

        let invalid = WindowGeometry {
            position: (100.0, 100.0),
            size: (800.0, -1.0),
            maximized: false,
        };
        assert!(!invalid.is_reachable_on(&[laptop]));
    }

    #[test]
    fn maximized_window_keeps_its_floating_restore_rectangle() {
        let floating = WindowGeometry {
            position: (120.0, 80.0),
            size: (900.0, 640.0),
            maximized: false,
        };
        let maximized = WindowGeometry {
            position: (0.0, 0.0),
            size: (1920.0, 1040.0),
            maximized: true,
        };
        assert_eq!(
            remember_geometry(Some(floating), maximized),
            WindowGeometry {
                maximized: true,
                ..floating
            }
        );
    }
}

/// What this platform reserves along the window's top edge.
///
/// Windows and Linux under CSD reserve nothing: the host draws every caption
/// control itself, inside the page, so the whole top edge is the stylesheet's.
/// macOS is the exception and the reason W1 exists — its traffic lights stay
/// system-drawn and system-placed even under a full-size content view, so a
/// stylesheet that puts a control at the top-left would put it underneath
/// them.
#[cfg(not(target_os = "macos"))]
pub(crate) fn titlebar_insets(_window: &winit::window::Window) -> TitlebarInsets {
    TitlebarInsets::NONE
}

/// macOS: the traffic lights, measured rather than assumed.
///
/// Their size and spacing are the system's to change (and did change between
/// releases), so the rect is read off the buttons themselves. A window without
/// them — one created with the standard title bar rather than the full-size
/// content view — reserves nothing, because the page is not drawing up there
/// at all.
#[cfg(target_os = "macos")]
pub(crate) fn titlebar_insets(window: &winit::window::Window) -> TitlebarInsets {
    use objc2_app_kit::{NSWindow, NSWindowButton};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = window.window_handle() else {
        return TitlebarInsets::NONE;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return TitlebarInsets::NONE;
    };
    // The handle names the content view; its window owns the buttons.
    let view: &objc2_app_kit::NSView = unsafe { handle.ns_view.cast().as_ref() };
    let Some(ns_window) = view.window() else {
        return TitlebarInsets::NONE;
    };
    let mut right = 0.0f32;
    let mut bottom = 0.0f32;
    for which in [
        NSWindowButton::CloseButton,
        NSWindowButton::MiniaturizeButton,
        NSWindowButton::ZoomButton,
    ] {
        let Some(button) = ns_window.standardWindowButton(which) else {
            continue;
        };
        // Buttons are laid out in the window's flipped-y coordinates; convert
        // to the content view's so the values mean what a stylesheet means by
        // "from the top".
        let frame = button.frame();
        let in_view =
            view.convertRect_fromView(frame, unsafe { ns_window.contentView() }.as_deref());
        right = right.max((in_view.origin.x + in_view.size.width) as f32);
        bottom = bottom.max((in_view.origin.y + in_view.size.height) as f32);
    }
    if right <= 0.0 {
        return TitlebarInsets::NONE;
    }
    // A margin past the last button, so a control butted against the reserved
    // region does not sit flush with the zoom button.
    const GUTTER: f32 = 8.0;
    TitlebarInsets {
        left: right + GUTTER,
        right: 0.0,
        height: bottom.max(28.0),
    }
}

#[cfg(test)]
mod titlebar_tests {
    use super::*;

    /// The published area is what is left after the platform's own controls.
    #[test]
    fn the_area_is_the_window_minus_the_reserved_edges() {
        let insets = TitlebarInsets {
            left: 78.0,
            right: 0.0,
            height: 28.0,
        };
        assert_eq!(insets.titlebar_area(1000.0), (78.0, 0.0, 922.0, 28.0));
    }

    /// A window narrower than its own controls publishes an empty area rather
    /// than a negative width, which would lay a reserving stylesheet out
    /// inside out.
    #[test]
    fn a_window_narrower_than_its_controls_publishes_nothing() {
        let insets = TitlebarInsets {
            left: 78.0,
            right: 40.0,
            height: 28.0,
        };
        let (x, _, width, _) = insets.titlebar_area(50.0);
        assert_eq!(x, 78.0);
        assert_eq!(width, 0.0);
    }

    /// Reserving nothing is the honest answer where the host draws every
    /// control, and it publishes a zero-height strip so a stylesheet's
    /// reservation collapses instead of leaving a gap.
    #[test]
    fn reserving_nothing_publishes_a_full_width_zero_height_area() {
        assert_eq!(
            TitlebarInsets::NONE.titlebar_area(800.0),
            (0.0, 0.0, 800.0, 0.0)
        );
    }

    /// The declarations are the four custom properties a stylesheet reads.
    #[test]
    fn the_declarations_name_all_four_properties() {
        let text = TitlebarInsets {
            left: 78.0,
            right: 0.0,
            height: 28.0,
        }
        .declarations(1000.0);
        for property in [
            "--titlebar-area-x: 78px",
            "--titlebar-area-y: 0px",
            "--titlebar-area-width: 922px",
            "--titlebar-area-height: 28px",
        ] {
            assert!(text.contains(property), "{property} in {text}");
        }
    }
}
