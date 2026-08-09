//! The host's input routing, asserted against a real retained DOM and a real
//! retained layout, with no window and no GPU.
//!
//! Every case here is a claim the G1 scope makes about the host: pointer
//! Down/Move/Up carry the *captured* element's local coordinates; wheel
//! handlers run before the layout's own scrolling and can cancel it; Tab is a
//! routed key rather than a swallowed one; and `prevent_default` really does
//! suppress the host defaults it is documented to suppress.
//!
//! The view below is laid out with absolute positions so every coordinate in
//! the assertions is arithmetic rather than a guess.

use std::cell::RefCell;
use std::rc::Rc;

use cambium::{
    AnyView, GenetCtx, GenetElement, PointerEvent, PointerPhase, TextInput, WheelEvent, clickable,
    el, focusable, on_key, on_pointer, on_wheel, text,
};
use cambium_genet_winit_host::{FocusedTextSlot, Harness, HostHooks, Init, inert_hooks};
use genet_probe::Selector;
use winit::keyboard::{Key, ModifiersState, NamedKey};

// ---------------------------------------------------------------- the app

#[derive(Default)]
struct App {
    clicks: Vec<&'static str>,
    /// Every pointer phase the drag element saw, with the coordinates it was
    /// handed. The whole point of the drag receipt.
    drag: Vec<(PointerPhase, (f32, f32), (f32, f32))>,
    wheel: Vec<((f32, f32), (f32, f32))>,
    /// When set, the wheel handler cancels the host's scrolling default.
    wheel_cancels: bool,
    /// When set, the Alpha button's key handler swallows Tab.
    traps_tab: bool,
    keys: Vec<String>,
}

type Child = Box<dyn AnyView<App, (), GenetCtx, GenetElement>>;

/// Absolute boxes, so the test's coordinates are arithmetic:
///
/// | element   | box                |
/// |-----------|--------------------|
/// | `alpha`   | (0, 0, 100, 40)    |
/// | `beta`    | (0, 50, 100, 40)   |
/// | `drag`    | (0, 100, 200, 20)  |
/// | `wheel`   | (0, 150, 200, 60)  |
/// | `scroll`  | (0, 220, 200, 60)  |
fn place(x: i32, y: i32, w: i32, h: i32) -> String {
    format!("position:absolute;left:{x}px;top:{y}px;width:{w}px;height:{h}px;")
}

fn root(state: &App) -> Child {
    let cancels = state.wheel_cancels;
    let traps = state.traps_tab;
    Box::new(
        el(
            "div",
            (
                focusable(on_key(
                    clickable(
                        el("button", text("Alpha")).attr("style", place(0, 0, 100, 40)),
                        |s: &mut App, _| s.clicks.push("alpha"),
                    ),
                    move |s: &mut App, e: cambium::KeyEvent| {
                        s.keys.push(format!("{:?}", e.key));
                        // A view that owns Tab: it can only do this because the
                        // host routes Tab instead of consuming it.
                        if traps && matches!(e.key, cambium::Key::Named(cambium::NamedKey::Tab)) {
                            e.prevent_default();
                        }
                    },
                )),
                focusable(clickable(
                    el("button", text("Beta")).attr("style", place(0, 50, 100, 40)),
                    |s: &mut App, _| s.clicks.push("beta"),
                )),
                on_pointer(
                    el("div", text("drag"))
                        .attr("class", "drag")
                        .attr("style", place(0, 100, 200, 20)),
                    |s: &mut App, e: PointerEvent| {
                        s.drag.push((e.phase, e.local, e.size));
                    },
                ),
                on_wheel(
                    el("div", text("wheel"))
                        .attr("class", "wheelbox")
                        .attr("style", place(0, 150, 200, 60)),
                    move |s: &mut App, e: WheelEvent| {
                        s.wheel.push((e.delta, e.local));
                        if cancels {
                            e.prevent_default();
                        }
                    },
                ),
                // A real overflow container, for the host's scrolling default.
                el(
                    "div",
                    el("div", text("tall")).attr("style", "width:100px;height:600px;"),
                )
                .attr("class", "scroller")
                .attr(
                    "style",
                    format!("{}overflow:auto;", place(0, 220, 200, 60)),
                ),
            ),
        )
        .attr("style", "position:relative;width:400px;height:400px;"),
    )
}

const SHEET: &str = "";

fn harness() -> Harness<App, fn(&App) -> Child, Child> {
    let mut h = Harness::new(SHEET, App::default(), root as fn(&App) -> Child);
    h.layout_at(400.0, 400.0);
    h
}

// ------------------------------------------------------------- the claims

/// A press resolves through the retained layout and dispatches a click that
/// carries the hit point in the target's own coordinates — not the `(0, 0)`
/// placeholder the extracted host shipped with.
#[test]
fn a_press_clicks_the_element_under_it() {
    let mut h = harness();
    // Alpha's box is (0, 0, 100, 40); press 10px in from its top-left.
    h.click_at(10.0, 12.0);
    assert_eq!(h.state().clicks, vec!["alpha"]);
    assert!(h.focus().is_some(), "clicking a focusable control focuses it");

    h.click_at(10.0, 62.0);
    assert_eq!(h.state().clicks, vec!["alpha", "beta"]);
}

/// A drag routes Down / Move / Up to the capturing element with coordinates in
/// **its** space, and keeps routing after the cursor leaves its box.
#[test]
fn a_drag_carries_the_captured_elements_local_coordinates() {
    let mut h = harness();
    // The drag element's box is (0, 100, 200, 20). Press at its midpoint.
    h.press_at(100.0, 110.0);
    assert_eq!(
        h.state().drag,
        vec![(PointerPhase::Down, (100.0, 10.0), (200.0, 20.0))],
        "the press point is element-local, and the size is the element's box",
    );
    assert!(
        h.pointer_capture().is_some(),
        "the press begins a capture, so later moves route here",
    );

    // Move within the box.
    h.move_to(150.0, 115.0);
    assert_eq!(
        h.state().drag.last().copied(),
        Some((PointerPhase::Move, (150.0, 15.0), (200.0, 20.0))),
    );

    // Move far outside it: capture means this still routes here, with the
    // (now out-of-range) local coordinate reported honestly rather than clamped.
    h.move_to(320.0, 5.0);
    assert_eq!(
        h.state().drag.last().copied(),
        Some((PointerPhase::Move, (320.0, -95.0), (200.0, 20.0))),
    );

    h.release_at(320.0, 5.0);
    assert_eq!(
        h.state().drag.last().copied(),
        Some((PointerPhase::Up, (320.0, -95.0), (200.0, 20.0))),
    );
    assert_eq!(h.pointer_capture(), None, "release ends the capture");
}

/// A press somewhere with no `on_pointer` ancestor starts no drag, so a later
/// move is not misrouted to whatever was dragged last.
#[test]
fn a_press_outside_a_pointer_element_starts_no_drag() {
    let mut h = harness();
    h.press_at(10.0, 12.0);
    assert_eq!(h.pointer_capture(), None);
    h.move_to(150.0, 115.0);
    assert!(
        h.state().drag.is_empty(),
        "moving over the drag element without a press must not route to it",
    );
}

/// The wheel reaches a view handler, with cursor-local coordinates, *before*
/// the host scrolls anything.
#[test]
fn a_wheel_notch_reaches_the_view_handler_first() {
    let mut h = harness();
    // The wheel box is (0, 150, 200, 60); park the cursor 20px into it.
    h.move_to(40.0, 170.0);
    h.wheel(0.0, 30.0);
    assert_eq!(
        h.state().wheel,
        vec![((0.0, 30.0), (40.0, 20.0))],
        "the handler gets the delta and the cursor in its own coordinates",
    );
}

/// `prevent_default` on a wheel handler cancels the host's scrolling default.
/// Proven where it is observable: over a real overflow container.
#[test]
fn a_wheel_handler_can_cancel_the_scrolling_default() {
    // First, the container scrolls when nothing cancels.
    let mut h = harness();
    h.move_to(100.0, 250.0);
    h.wheel(0.0, 40.0);
    let scrolled = h.element_scroll_total();
    assert!(
        scrolled > 0.0,
        "an overflow container under the cursor scrolls by default (got {scrolled})",
    );

    // Now the same gesture over the handler that cancels: no scrolling.
    let mut h = harness();
    h.update(|s| s.wheel_cancels = true);
    h.move_to(40.0, 170.0);
    h.wheel(0.0, 40.0);
    assert_eq!(h.state().wheel.len(), 1, "the handler still ran");
    assert_eq!(
        h.element_scroll_total(),
        0.0,
        "and the host's own scrolling was suppressed",
    );
}

/// Tab goes through `dispatch_key`: the focused element's key handler sees it,
/// and only then does the runner traverse.
#[test]
fn tab_is_routed_before_it_traverses() {
    let mut h = harness();
    h.click_at(10.0, 12.0); // focus Alpha
    let alpha = h.focus().expect("Alpha focused");

    h.tab(true);
    assert!(
        h.state().keys.iter().any(|k| k.contains("Tab")),
        "the focused view's key handler saw Tab: {:?}",
        h.state().keys,
    );
    assert_ne!(h.focus(), Some(alpha), "and focus then moved on");

    h.tab(false);
    assert_eq!(h.focus(), Some(alpha), "Shift+Tab traverses back");
}

/// A view that calls `prevent_default` on Tab keeps focus — the escape hatch a
/// swallowed Tab made unreachable.
#[test]
fn a_view_can_prevent_the_tab_default() {
    let mut h = harness();
    h.update(|s| s.traps_tab = true);
    h.click_at(10.0, 12.0);
    let alpha = h.focus().expect("Alpha focused");
    h.tab(true);
    assert_eq!(
        h.focus(),
        Some(alpha),
        "prevent_default on Tab suppresses the traversal default",
    );
}

/// Enter activates a focused button through the runner's own default — the
/// keyboard route a mouse-only host would leave broken.
#[test]
fn enter_activates_the_focused_control() {
    let mut h = harness();
    h.click_at(10.0, 62.0); // Beta: clickable + focusable, no key handler
    let before = h.state().clicks.len();
    h.key_named(NamedKey::Enter);
    assert_eq!(
        h.state().clicks.len(),
        before + 1,
        "Enter on a focused button activates it",
    );
}

/// Semantic targeting: the harness resolves a control by role and label and
/// clicks it, which is the same resolver a `genet-probe` scenario uses.
#[test]
fn a_control_can_be_clicked_by_role_and_label() {
    let mut h = harness();
    assert!(
        h.click_on(&Selector::role("button").containing("Beta")),
        "the Beta button must resolve by role and label",
    );
    assert_eq!(h.state().clicks, vec!["beta"]);
}

/// Text the platform could not name a key for still types.
///
/// Windows delivers injected text as `VK_PACKET`, which winit surfaces as
/// `Key::Unidentified`, and the host used to drop it. Injected text is not an
/// exotic case: on-screen keyboards, keyboard remappers, and several assistive
/// input tools all type that way, so dropping it meant a person using one could
/// not type into the application at all. Found by tracing a live headed run.
#[test]
fn injected_text_types_even_though_the_key_is_unnamed() {
    let mut h = harness();
    h.click_at(10.0, 12.0); // focus Alpha, which has a key handler
    let before = h.state().keys.len();

    h.key_injected("q");
    assert_eq!(
        h.state().keys.len(),
        before + 1,
        "an unnamed key carrying text still reaches the focused element: {:?}",
        h.state().keys,
    );
    assert!(
        h.state().keys.last().is_some_and(|k| k.contains('q')),
        "and it arrives as the character it produces: {:?}",
        h.state().keys,
    );
}

/// A modifier chord is a command, not typing, so injected text under Ctrl is
/// still dropped — otherwise Ctrl+S would insert an "s".
#[test]
fn an_injected_chord_is_not_typed() {
    let mut h = harness();
    h.click_at(10.0, 12.0);
    let before = h.state().keys.len();
    h.set_modifiers(ModifiersState::CONTROL);
    h.key_injected("s");
    assert_eq!(
        h.state().keys.len(),
        before,
        "Ctrl + injected text is a shortcut, not an insertion",
    );
}

/// The caret default: clicking into a text field places the caret, and an arrow
/// key moves it — both gated on nothing having prevented the default.
#[test]
fn the_caret_defaults_run_through_the_focused_text_seam() {
    struct Field {
        text: TextInput,
    }
    type FieldChild = Box<dyn AnyView<Field, (), GenetCtx, GenetElement>>;
    fn field_root(_state: &Field) -> FieldChild {
        Box::new(el(
            "div",
            cambium::lens(
                |s: &mut TextInput| cambium::text_field(s),
                |f: &mut Field| &mut f.text,
            ),
        ))
    }
    const FIELD_SHEET: &str = "input { position:absolute; left:0px; top:0px; \
         width:200px; height:30px; font-size:16px; }";
    let node = Rc::new(RefCell::new(None));
    let seen = node.clone();
    let hooks: HostHooks<Field, fn(&Field) -> FieldChild, FieldChild> = HostHooks {
        focused_text: Box::new(move |runner| {
            let focused = runner.focus()?;
            let dom = runner.dom();
            let dom_ref = dom.borrow();
            if layout_dom_api::LayoutDom::element_name(&*dom_ref, focused)?
                .local
                .as_ref()
                != "input"
            {
                return None;
            }
            *seen.borrow_mut() = Some(focused);
            Some(FocusedTextSlot {
                node: focused,
                get: Box::new(|s: &Field| &s.text),
                get_mut: Box::new(|s: &mut Field| &mut s.text),
            })
        }),
        ..inert_hooks()
    };
    let mut h = Harness::with_hooks(
        Init {
            state: Field {
                text: TextInput::new("hello world"),
            },
            logic: field_root as fn(&Field) -> FieldChild,
            sheet: FIELD_SHEET.to_string(),
        },
        hooks,
    );
    h.layout_at(300.0, 100.0);
    h.click_at(4.0, 15.0);
    assert!(
        node.borrow().is_some(),
        "the click focused the field through the text seam",
    );
    let after_click = h.state().text.caret_position().byte;
    h.set_modifiers(ModifiersState::empty());
    h.key(Key::Named(NamedKey::ArrowRight));
    assert!(
        h.state().text.caret_position().byte > after_click,
        "the host's visual caret default moved the caret forward",
    );
}
