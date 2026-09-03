//! The window-frame seam, driven headlessly.
//!
//! Every assertion here goes through the host's real press path
//! (`Harness::press_at` calls `press_left`), so what these prove is what the
//! winit build does. The window verbs are recorded rather than enacted
//! because a harness has no window — which is the only reason a frame test
//! can run in `cargo test` at all.

use cambium::{
    AnyView, GenetCtx, GenetElement, PointerButton, PointerEvent, clickable, el, on_pointer, text,
};
use cambium_genet_winit_host::{AppRegion, Harness, WindowCommand, WindowCommands};

#[derive(Default)]
struct App {
    window: WindowCommands,
    body_clicks: usize,
    /// Buttons the title's own `on_pointer` handler saw. The frame and the
    /// view both want the press; this records that the view gets it first.
    title_presses: Vec<PointerButton>,
    /// When set, that handler keeps the press for itself.
    title_vetoes: bool,
}

type Child = Box<dyn AnyView<App, (), GenetCtx, GenetElement>>;
type Logic = fn(&App) -> Child;

/// A title bar with one caption button, over an ordinary body. The shape
/// every CSD application has.
fn root(state: &App) -> Child {
    let vetoes = state.title_vetoes;
    Box::new(el(
        "div",
        (
            el(
                "div",
                (
                    on_pointer(
                        el("div", text("title")).attr("class", "caption-title"),
                        move |s: &mut App, e: PointerEvent| {
                            s.title_presses.push(e.button);
                            if vetoes {
                                e.prop.prevent_default();
                            }
                        },
                    ),
                    clickable(
                        el("button", text("x"))
                            .attr("class", "caption")
                            .attr("aria-label", "Close"),
                        |s: &mut App, _| s.window.close(),
                    ),
                ),
            )
            .attr("class", "bar"),
            clickable(
                el("div", text("body")).attr("class", "body"),
                |s: &mut App, _| s.body_clicks += 1,
            ),
        ),
    ))
}

// Flex, so the caption button sits beside the title rather than wrapping
// under it — the button has to be *inside* the bar's box for the carve-out to
// mean anything.
const SHEET: &str = "
.bar { --app-region: drag; display: flex; width: 400px; height: 30px; }
.caption-title { width: 300px; height: 30px; }
.caption { --app-region: no-drag; width: 40px; height: 30px; }
.body { width: 400px; height: 200px; }
";

fn harness() -> Harness<App, Logic, Child> {
    let mut h = Harness::with_commands(SHEET, root as Logic, |commands| App {
        window: commands.clone(),
        ..App::default()
    });
    h.layout_at(400.0, 300.0);
    h
}

/// The title strip and the body must disagree, or nothing below means
/// anything.
#[test]
fn the_bar_is_a_drag_surface_and_the_body_is_not() {
    let h = harness();
    assert_eq!(h.app_region_at(100.0, 10.0), AppRegion::Drag);
    assert_eq!(h.app_region_at(100.0, 150.0), AppRegion::NoDrag);
}

/// The `no-drag` carve-out is the whole reason the property inherits: a
/// caption button sits inside the drag surface and must not drag the window.
#[test]
fn a_caption_button_carves_a_hole_in_the_bar() {
    let h = harness();
    assert_eq!(
        h.app_region_at(320.0, 10.0),
        AppRegion::NoDrag,
        "the button is inside the bar but must not drag it",
    );
}

#[test]
fn pressing_the_bar_drags_the_window() {
    let mut h = harness();
    h.press_at(100.0, 10.0);
    assert_eq!(h.performed(), &[WindowCommand::Drag]);
}

#[test]
fn pressing_the_body_drags_nothing() {
    let mut h = harness();
    h.press_at(100.0, 150.0);
    assert!(h.performed().is_empty());
    assert_eq!(h.state().body_clicks, 1, "the click still reached the DOM");
}

/// A press on a `no-drag` control inside the bar runs its own verb and moves
/// no window. Both halves matter: the button has to work, and the bar
/// underneath it must not also grab the press.
#[test]
fn pressing_a_caption_button_runs_its_verb_without_dragging() {
    let mut h = harness();
    h.press_at(320.0, 10.0);

    assert_eq!(
        h.performed(),
        &[WindowCommand::Close],
        "the button's verb ran and nothing dragged",
    );
}

/// Double-clicking a title bar maximizes. Two presses inside the cadence, and
/// the second must produce maximize rather than a second drag.
#[test]
fn double_clicking_the_bar_toggles_maximize() {
    let mut h = harness();
    h.press_at(100.0, 10.0);
    h.release_at(100.0, 10.0);
    h.press_at(100.0, 10.0);

    assert_eq!(
        h.performed(),
        &[WindowCommand::Drag, WindowCommand::ToggleMaximize],
        "the first press drags, the second completes a double-click",
    );
}

/// Right-clicking a title bar raises the platform menu; right-clicking
/// content does not.
#[test]
fn right_click_raises_the_system_menu_only_on_the_bar() {
    let mut h = harness();
    h.right_press_at(100.0, 150.0);
    assert!(h.performed().is_empty(), "content has no system menu");

    h.right_press_at(100.0, 10.0);
    assert_eq!(h.performed(), &[WindowCommand::ShowSystemMenu]);
}

/// The application's end of the seam is one cloneable handle, so a verb
/// queued from a handler reaches the host without a state field per verb.
#[test]
fn the_command_handle_is_shared_not_copied() {
    let mut h = harness();
    h.update(|s| s.window.minimize());
    // Queued, not yet drained: nothing has dispatched.
    assert!(h.performed().is_empty());

    // Any dispatch drains it.
    h.press_at(100.0, 150.0);
    assert!(h.performed().contains(&WindowCommand::Minimize));
}

/// M4's frame half. A right press on the title bar reaches the view *first* —
/// the same order [`press_left`] uses — and only then raises the platform
/// menu. Both must happen: an application that draws a context menu on its own
/// title bar needs the press, and the window still needs its system menu.
#[test]
fn a_right_press_on_the_bar_reaches_the_view_and_still_raises_the_menu() {
    let mut h = harness();
    h.right_click_at(100.0, 10.0);

    assert_eq!(
        h.state().title_presses,
        vec![PointerButton::Secondary],
        "the view saw the press, marked as the right button",
    );
    assert_eq!(
        h.performed(),
        &[WindowCommand::ShowSystemMenu],
        "and the frame default still ran",
    );
    assert_eq!(h.pointer_capture(), None, "no drag began");
}

/// The veto, which is why the order matters: a handler that calls
/// `prevent_default` on the secondary press keeps it, exactly as one can on
/// the primary press.
#[test]
fn a_view_can_prevent_the_system_menu() {
    let mut h = harness();
    h.update(|s| s.title_vetoes = true);
    h.right_click_at(100.0, 10.0);

    assert_eq!(h.state().title_presses, vec![PointerButton::Secondary]);
    assert!(
        h.performed().is_empty(),
        "prevent_default suppresses the frame's system menu",
    );
}

/// A right press on ordinary content reaches no `on_pointer` handler here and
/// raises nothing — and, crucially, does not activate the control under it.
#[test]
fn a_right_press_on_the_body_activates_nothing() {
    let mut h = harness();
    h.right_click_at(100.0, 150.0);

    assert!(h.performed().is_empty());
    assert_eq!(
        h.state().body_clicks,
        0,
        "the right button is not a click on the body's clickable",
    );
}
