//! Interface zoom: one layout scale, and everything the host exchanges with
//! the application expressed in it.
//!
//! The claims here are the ones the zoom plan makes. Layout runs at
//! `window / zoom`, so `logical_size` shrinks as the interface grows. A point
//! the platform reports is divided by the layout scale, so a click lands on the
//! control the person actually saw. The accessibility root transform carries
//! the same scale, so a screen reader is told where the control is on screen
//! rather than where it would be unzoomed. And the keyboard ladder is the
//! browser's, with the application's `key_intercept` able to veto it.
//!
//! The boxes below are absolute or in explicit pixels, so every coordinate in
//! an assertion is arithmetic rather than a guess.

use std::cell::RefCell;
use std::rc::Rc;

use cambium::{
    AnyView, GenetCtx, GenetElement, WheelEvent, clickable, el, on_wheel, text,
};
use cambium_genet_winit_host::{
    Harness, HostHooks, HostOptions, Init, Modifiers, ZOOM_LADDER, fit_zoom, inert_hooks,
};
use genet_probe::Selector;

// ---------------------------------------------------------------- the app

#[derive(Default)]
struct App {
    clicks: Vec<&'static str>,
    /// When set, the wheel handler cancels the host's default — which now
    /// includes the Ctrl+wheel zoom step.
    wheel_cancels: bool,
}

type Child = Box<dyn AnyView<App, (), GenetCtx, GenetElement>>;

/// | element    | box (layout px)      | why it is here                    |
/// |------------|----------------------|-----------------------------------|
/// | `.target`  | (100, 100, 120, 40)  | a control to click through zoom   |
/// | `.wheelbox`| (0, 300, 200, 60)    | the Ctrl+wheel veto               |
/// | `.prose`   | full width, flowing  | text relaid out, never resampled  |
/// | `.run`     | shrink-to-fit at 0,0 | one text run's advance width      |
fn root(state: &App) -> Child {
    let cancels = state.wheel_cancels;
    Box::new(
        el(
            "div",
            (
                clickable(
                    el("button", text("Target"))
                        .attr("class", "target")
                        .attr(
                            "style",
                            "display:block;position:absolute;left:100px;top:100px;\
                             width:120px;height:40px;",
                        ),
                    |s: &mut App, _| s.clicks.push("target"),
                ),
                on_wheel(
                    el("div", text("wheel"))
                        .attr("class", "wheelbox")
                        .attr(
                            "style",
                            "position:absolute;left:0px;top:300px;width:200px;height:60px;",
                        ),
                    move |_: &mut App, e: WheelEvent| {
                        if cancels {
                            e.prevent_default();
                        }
                    },
                ),
                el(
                    "p",
                    text(
                        "The rootstock is the body a scion is fitted to: one root \
                         system, and tops that can be exchanged without regrowing \
                         it. That is the relation here, and it is why the machinery \
                         below knows nothing about a window.",
                    ),
                )
                .attr("class", "prose")
                .attr(
                    "style",
                    "position:absolute;left:0px;top:400px;width:100%;font-size:16px;",
                ),
                el("span", text("Advance"))
                    .attr("class", "run")
                    .attr(
                        "style",
                        "display:block;position:absolute;left:0px;top:0px;font-size:16px;",
                    ),
            ),
        )
        .attr("style", "position:relative;width:100%;height:100%;"),
    )
}

const SHEET: &str = "";

fn options(zoom: f32, fit: Option<(f32, f32)>) -> HostOptions {
    HostOptions {
        ui_zoom: zoom,
        fit_design: fit,
        ..Default::default()
    }
}

fn init() -> Init<App, fn(&App) -> Child> {
    Init {
        state: App::default(),
        logic: root as fn(&App) -> Child,
        sheet: SHEET.into(),
    }
}

/// A harness at a chosen zoom, laid out for a 1200x800 window.
fn harness_at(zoom: f32) -> Harness<App, fn(&App) -> Child, Child> {
    let mut h = Harness::with_hooks_and_options(init(), inert_hooks(), options(zoom, None));
    h.layout_at(1200.0, 800.0);
    h
}

fn painted(h: &Harness<App, fn(&App) -> Child, Child>, class: &str) -> (f32, f32, f32, f32) {
    let node = h
        .with_dom(|dom| genet_probe::matching(dom, &Selector::class(class)).first().copied())
        .unwrap_or_else(|| panic!("no element with class {class}"));
    h.painted_rect(node)
        .unwrap_or_else(|| panic!("{class} has no painted box"))
}

fn ctrl() -> Modifiers {
    Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    }
}

// ------------------------------------------------------- Z0: one scale

/// Layout runs at `window / zoom`, so a zoomed interface fits *less*. That
/// division is the whole mechanism; everything else follows from it.
#[test]
fn logical_size_is_the_window_over_the_zoom() {
    let h = harness_at(1.0);
    assert_eq!(h.ui_zoom(), 1.0);
    assert_eq!(h.logical_size(), (1200.0, 800.0));

    let h = harness_at(1.5);
    assert_eq!(h.ui_zoom(), 1.5);
    assert_eq!(h.logical_size(), (800.0, 800.0 / 1.5));
    assert_eq!(
        h.layout_scale(),
        1.5,
        "windowless the device scale is 1.0, so the layout scale is the zoom",
    );
}

/// Zoom 1.0 is the identity, exactly: the layout scale is the device scale,
/// the logical size is the window, and a platform point passes through
/// unchanged.
///
/// This is the guard on the whole change. Every consumer that names no zoom
/// runs down this path, and the plan's stop rule is that a behaviour change at
/// zoom 1.0 stops the work.
#[test]
fn zoom_one_is_the_identity() {
    let h = harness_at(1.0);
    assert_eq!(h.ui_zoom(), 1.0);
    assert_eq!(h.layout_scale(), 1.0);
    assert_eq!(h.logical_size(), (1200.0, 800.0), "the window, undivided");
    assert_eq!(h.layout_point(240.0, 180.0), (240.0, 180.0));
    // And the boxes are where they were laid out, not where a scale put them.
    assert_eq!(painted(&h, "target"), (100.0, 100.0, 120.0, 40.0));
}

/// A point the platform reports is in window pixels; the control is drawn at
/// `layout * zoom`. The host divides, so a click lands where the person aimed.
#[test]
fn a_click_in_window_coordinates_hits_the_scaled_element() {
    let mut h = harness_at(1.5);
    // The target's layout box is (100, 100, 120, 40) at any zoom — layout
    // coordinates do not move, the window's relation to them does.
    assert_eq!(painted(&h, "target"), (100.0, 100.0, 120.0, 40.0));

    // On screen it is drawn at (150, 150)-(330, 210): its centre is (240, 180).
    h.click_at_window(240.0, 180.0);
    assert_eq!(h.state().clicks, vec!["target"], "the scaled centre hits");
    assert_eq!(
        h.cursor(),
        (160.0, 120.0),
        "and arrives in the layout's own coordinates",
    );

    // The unzoomed centre — where the control would have been drawn at zoom 1
    // — is now empty space above and left of it.
    let mut h = harness_at(1.5);
    h.click_at_window(160.0, 120.0);
    assert!(
        h.state().clicks.is_empty(),
        "the pre-zoom centre is no longer the control",
    );
}

/// A screen reader is told physical client coordinates. The transform that
/// carries the projected boxes there is the layout scale, zoom included.
#[test]
fn the_accessibility_root_transform_carries_the_layout_scale() {
    use accesskit::{Affine, NodeId as A11yNodeId};
    use layout_dom_api::LayoutDom as _;

    let mut h = harness_at(1.5);
    let root = h.with_dom(|dom| A11yNodeId(dom.opaque_id(dom.document())));
    let tree = h.scaled_a11y_tree();
    let (_, node) = tree
        .nodes
        .iter()
        .find(|(id, _)| *id == root)
        .expect("the projected tree has a root");
    assert_eq!(node.transform(), Some(&Affine::scale(1.5)));
}

// ------------------------------------------------------- Z1: the knobs

/// `fit_design` scales the interface until the design fits, on the binding
/// axis. The other axis keeps its slack — that is what `min` means, and it is
/// why the design is never cropped.
#[test]
fn fit_design_scales_the_interface_until_the_design_fits() {
    let mut h = Harness::with_hooks_and_options(
        init(),
        inert_hooks(),
        options(1.0, Some((1100.0, 820.0))),
    );
    h.layout_at(1100.0, 752.0);

    let expected = 752.0 / 820.0;
    assert!((h.ui_zoom() - expected).abs() < 1e-5, "{}", h.ui_zoom());
    assert_eq!(
        h.ui_zoom(),
        fit_zoom((1100.0, 820.0), (1100.0, 752.0)),
        "the public helper computes the host's own number",
    );
    let (lw, lh) = h.logical_size();
    assert!((lh - 820.0).abs() < 0.01, "the binding axis lands exactly: {lh}");
    assert!(
        (lw - 1100.0 / expected).abs() < 0.01,
        "the slack axis gets more room than the design asked for: {lw}",
    );
    assert!(lw > 1100.0, "which is more, not less: {lw}");
}

/// Resizing recomputes it, with no application involvement at all.
#[test]
fn resizing_recomputes_the_fit() {
    let mut h = Harness::with_hooks_and_options(
        init(),
        inert_hooks(),
        options(1.0, Some((1100.0, 820.0))),
    );
    h.layout_at(1100.0, 820.0);
    assert_eq!(h.ui_zoom(), 1.0, "the design fits exactly");

    h.layout_at(550.0, 410.0);
    assert!((h.ui_zoom() - 0.5).abs() < 1e-5, "{}", h.ui_zoom());
    let (lw, lh) = h.logical_size();
    assert!((lw - 1100.0).abs() < 0.01 && (lh - 820.0).abs() < 0.01, "{lw}x{lh}");
}

/// Without a design the explicit knob is the whole answer. With one it is an
/// offset multiplied onto the fit.
#[test]
fn the_explicit_knob_stands_alone_and_composes_with_the_fit() {
    let mut h = harness_at(1.0);
    h.set_ui_zoom(1.25);
    assert_eq!(h.ui_zoom(), 1.25);
    assert_eq!(h.logical_size(), (1200.0 / 1.25, 800.0 / 1.25));

    let mut h = Harness::with_hooks_and_options(
        init(),
        inert_hooks(),
        options(1.25, Some((1100.0, 820.0))),
    );
    h.layout_at(1100.0, 820.0);
    assert!(
        (h.ui_zoom() - 1.25).abs() < 1e-5,
        "fit 1.0 times the offset: {}",
        h.ui_zoom(),
    );
    h.layout_at(550.0, 410.0);
    assert!(
        (h.ui_zoom() - 0.625).abs() < 1e-5,
        "fit 0.5 times the offset: {}",
        h.ui_zoom(),
    );
}

/// A zoom change reaches the application through the hook context, once —
/// the edge, not the level, so a consumer persisting the preference writes on
/// the change rather than every frame.
#[test]
fn a_hook_is_told_the_zoom_once_per_change() {
    let seen: Rc<RefCell<Vec<(f32, bool)>>> = Rc::new(RefCell::new(Vec::new()));
    let recorder = seen.clone();
    let hooks = HostHooks {
        after_dispatch: Box::new(move |ctx| {
            recorder.borrow_mut().push((ctx.ui_zoom, ctx.zoom_changed));
        }),
        ..inert_hooks()
    };
    let mut h = Harness::with_hooks_and_options(init(), hooks, options(1.0, None));
    h.layout_at(1200.0, 800.0);

    h.after_dispatch();
    assert_eq!(seen.borrow().as_slice(), &[(1.0, false)]);

    h.set_ui_zoom(1.25);
    h.after_dispatch();
    assert_eq!(
        seen.borrow().last().copied(),
        Some((1.25, true)),
        "the first hook after the change is told about it",
    );

    h.after_dispatch();
    assert_eq!(
        seen.borrow().last().copied(),
        Some((1.25, false)),
        "and the next one is not told again",
    );
}

/// The setter is reachable from inside a hook, the way a stylesheet swap is:
/// set the field, and the host applies it once the hook's borrows end.
#[test]
fn a_hook_can_ask_for_a_zoom_through_the_context() {
    let asked = Rc::new(RefCell::new(false));
    let flag = asked.clone();
    let hooks = HostHooks {
        after_dispatch: Box::new(move |ctx| {
            if !*flag.borrow() {
                *flag.borrow_mut() = true;
                *ctx.set_ui_zoom = Some(0.8);
            }
        }),
        ..inert_hooks()
    };
    let mut h = Harness::with_hooks_and_options(init(), hooks, options(1.0, None));
    h.layout_at(1200.0, 800.0);
    h.after_dispatch();
    assert_eq!(h.ui_zoom(), 0.8);
}

// -------------------------------------------------- Z2: laid out, not scaled

/// Text is *laid out* at the new logical size, not drawn small and stretched.
///
/// A bitmap scale cannot change where a line breaks. This paragraph is as wide
/// as the window, so zooming in narrows it in CSS pixels, the same 16px text
/// wraps sooner, and the block gets taller. That is layout, and nothing else
/// produces it.
#[test]
fn text_is_relaid_out_at_the_new_logical_width() {
    let plain = painted(&harness_at(1.0), "prose").3;
    let zoomed = painted(&harness_at(2.0), "prose").3;
    assert!(
        zoomed > plain,
        "the same prose wraps to more lines in a narrower CSS viewport: \
         {plain} -> {zoomed}",
    );
}

/// One text run's advance: constant in layout pixels, and therefore scaling
/// with zoom in the window the person is looking at.
#[test]
fn a_text_runs_advance_scales_with_zoom_on_screen() {
    let at_one = painted(&harness_at(1.0), "run").2;
    let at_zoom = painted(&harness_at(1.25), "run").2;
    // Bounded rather than merely nonzero, so the box cannot have quietly
    // become the containing block's width and passed the equality below by
    // being wrong in both runs.
    assert!(
        (20.0..300.0).contains(&at_one),
        "the run shrink-wraps its own text rather than filling the 1200px \
         viewport: {at_one}",
    );
    assert!(
        (at_one - at_zoom).abs() < 0.01,
        "the CSS advance is a property of the text and the font size, not of \
         zoom: {at_one} vs {at_zoom}",
    );
    let on_screen = |advance: f32, zoom: f32| advance * zoom;
    assert!(
        (on_screen(at_zoom, 1.25) - on_screen(at_one, 1.0) * 1.25).abs() < 0.02,
        "so on screen it is exactly 1.25x wider",
    );
}

// -------------------------------------------------- Z3: keys and the wheel

/// Ctrl+plus and Ctrl+minus walk the browser ladder; Ctrl+0 comes home.
#[test]
fn the_keyboard_walks_the_ladder_both_ways() {
    let mut h = harness_at(1.0);
    h.set_modifiers(ctrl());

    h.key_char("=");
    assert_eq!(h.ui_zoom(), 1.1);
    h.key_char("+");
    assert_eq!(h.ui_zoom(), 1.25, "the shifted spelling means the same thing");
    h.key_char("-");
    assert_eq!(h.ui_zoom(), 1.1);
    h.key_char("0");
    assert_eq!(h.ui_zoom(), 1.0);
    assert_eq!(h.logical_size(), (1200.0, 800.0), "and the layout came back");
}

/// The ladder clamps at both ends rather than wrapping or running away.
#[test]
fn the_ladder_clamps_at_its_ends() {
    let mut h = harness_at(1.0);
    h.set_modifiers(ctrl());
    for _ in 0..20 {
        h.key_char("=");
    }
    assert_eq!(h.ui_zoom(), *ZOOM_LADDER.last().expect("a ladder has rungs"));
    for _ in 0..40 {
        h.key_char("-");
    }
    assert_eq!(h.ui_zoom(), ZOOM_LADDER[0]);
}

/// The application's `key_intercept` runs first, so consuming the chord vetoes
/// the host's default — the same veto Escape and every other global shortcut
/// already has.
#[test]
fn an_application_can_veto_the_zoom_chord() {
    let hooks = HostHooks {
        key_intercept: Box::new(|_, press| press.modifiers.ctrl),
        ..inert_hooks()
    };
    let mut h = Harness::with_hooks_and_options(init(), hooks, options(1.0, None));
    h.layout_at(1200.0, 800.0);
    h.set_modifiers(ctrl());

    h.key_char("=");
    assert_eq!(h.ui_zoom(), 1.0, "the intercept consumed it, so nothing moved");
    h.key_char("0");
    assert_eq!(h.ui_zoom(), 1.0);
}

/// Ctrl+wheel steps the same ladder, in the browser's direction: up enlarges.
#[test]
fn ctrl_wheel_steps_the_ladder() {
    let mut h = harness_at(1.0);
    h.set_modifiers(ctrl());
    // A positive dy advances toward the end of the document — scrolling down,
    // which shrinks.
    h.wheel(0.0, 30.0);
    assert_eq!(h.ui_zoom(), 0.9);
    h.wheel(0.0, -30.0);
    assert_eq!(h.ui_zoom(), 1.0);
    h.wheel(0.0, -30.0);
    assert_eq!(h.ui_zoom(), 1.1);
}

/// It is a host *default*, so a view that wants Ctrl+wheel for itself keeps it
/// by preventing the default — exactly as it already suppresses scrolling.
#[test]
fn a_wheel_handler_can_prevent_the_zoom_default() {
    let mut h = harness_at(1.0);
    h.update(|s| s.wheel_cancels = true);
    h.set_modifiers(ctrl());
    // The wheel box's layout rect is (0, 300, 200, 60): put the cursor in it.
    h.move_to(100.0, 330.0);
    h.wheel(0.0, -30.0);
    assert_eq!(h.ui_zoom(), 1.0, "the handler consumed the notch");

    // Away from the handler there is nothing to consume it.
    h.move_to(600.0, 600.0);
    h.wheel(0.0, -30.0);
    assert_eq!(h.ui_zoom(), 1.1);
}

/// With `fit_design` set, a keyboard step is a user offset on the fit factor,
/// and Ctrl+0 clears the offset rather than forcing the interface to 1.0.
#[test]
fn a_step_under_fit_design_is_an_offset_and_ctrl_zero_clears_it() {
    let mut h = Harness::with_hooks_and_options(
        init(),
        inert_hooks(),
        options(1.0, Some((1100.0, 820.0))),
    );
    h.layout_at(550.0, 410.0);
    assert!((h.ui_zoom() - 0.5).abs() < 1e-5);

    h.set_modifiers(ctrl());
    h.key_char("=");
    assert!(
        (h.ui_zoom() - 0.55).abs() < 1e-5,
        "fit 0.5 times the 1.1 rung: {}",
        h.ui_zoom(),
    );

    h.key_char("0");
    assert!(
        (h.ui_zoom() - 0.5).abs() < 1e-5,
        "reset means back to what fits, not back to unscaled: {}",
        h.ui_zoom(),
    );
}
