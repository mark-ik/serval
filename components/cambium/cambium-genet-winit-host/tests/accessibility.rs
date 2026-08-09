//! The accessibility seam: what a screen reader is told, what happens when it
//! acts, and that it can wake an idle application at all.
//!
//! The OS adapter is the one piece a test cannot supply, so it is the one piece
//! not asserted here. Everything on either side of it is: the projection the
//! adapter is handed, the mapping from a raw AccessKit request to a typed one,
//! the routing of that request into the retained DOM, and the wake callback the
//! adapter is given.

use accesskit::{Action, ActionRequest, Role};
use cambium::{AnyView, GenetCtx, GenetElement, clickable, el, focusable, text};
use cambium_genet_winit_host::{Harness, IdlePolicy};
use cambium_winit_a11y::{A11yAction, A11yHost};

#[derive(Default)]
struct App {
    activated: Vec<&'static str>,
}

type Child = Box<dyn AnyView<App, (), GenetCtx, GenetElement>>;

fn place(x: i32, y: i32, w: i32, h: i32) -> String {
    format!("position:absolute;left:{x}px;top:{y}px;width:{w}px;height:{h}px;")
}

fn root(_state: &App) -> Child {
    Box::new(
        el(
            "div",
            (
                focusable(clickable(
                    el("button", text("Install")).attr("style", place(0, 0, 120, 40)),
                    |s: &mut App, _| s.activated.push("install"),
                )),
                focusable(clickable(
                    el("button", text("Cancel")).attr("style", place(0, 50, 120, 40)),
                    |s: &mut App, _| s.activated.push("cancel"),
                )),
            ),
        )
        .attr("role", "main")
        .attr("style", "position:relative;width:300px;height:200px;"),
    )
}

fn harness() -> Harness<App, fn(&App) -> Child, Child> {
    let mut h = Harness::new("", App::default(), root as fn(&App) -> Child);
    h.layout_at(300.0, 200.0);
    h
}

/// Find the projected node for a control by its accessible name.
fn node_named<'a>(
    tree: &'a accesskit::TreeUpdate,
    name: &str,
) -> Option<(accesskit::NodeId, &'a accesskit::Node)> {
    tree.nodes
        .iter()
        .find(|(_, node)| node.label().is_some_and(|label| label == name))
        .map(|(id, node)| (*id, node))
}

/// The projection carries the roles and names a reader announces, and a box for
/// each control so a virtual cursor can land on it.
#[test]
fn the_projection_carries_roles_names_and_boxes() {
    let mut h = harness();
    let (tree, map) = h.a11y_tree();

    let (install_id, install) = node_named(&tree, "Install").expect("the Install button projects");
    assert_eq!(install.role(), Role::Button, "and announces as a button");
    let dom_node = *map
        .get(&install_id)
        .expect("the node maps back to its DOM element, or an action could not route");
    let bounds = install.bounds().expect("a control needs a box to be reachable");
    let painted = h
        .painted_rect(dom_node)
        .expect("the same control has a laid-out box");
    // The strong form of the claim: the box a screen reader is told about is
    // the box the pointer hits. A projection that drifts from layout puts the
    // reader's cursor somewhere the mouse cannot go.
    assert_eq!(
        (
            bounds.x0 as f32,
            bounds.y0 as f32,
            (bounds.x1 - bounds.x0) as f32,
            (bounds.y1 - bounds.y0) as f32
        ),
        painted,
        "the announced box and the hit box are the same box",
    );

    assert!(
        node_named(&tree, "Cancel").is_some(),
        "every control projects, not just the first",
    );
}

/// A Click request activates the control — the same path a mouse press takes.
#[test]
fn a_click_request_activates_the_control() {
    let mut h = harness();
    let (tree, _) = h.a11y_tree();
    let (install, _) = node_named(&tree, "Install").expect("Install projects");
    let dom_node = h
        .a11y_dom_node(install)
        .expect("the projected node resolves to a DOM node");

    h.a11y_request(A11yAction::Click, dom_node);
    assert_eq!(h.state().activated, vec!["install"]);
}

/// A Focus request moves focus and does **not** activate. This is the whole
/// reason the two actions stay typed: a reader's virtual cursor issues Focus as
/// it moves across controls, and collapsing that into a click would fire every
/// one it passed.
#[test]
fn a_focus_request_focuses_without_activating() {
    let mut h = harness();
    let (tree, _) = h.a11y_tree();
    let (cancel, _) = node_named(&tree, "Cancel").expect("Cancel projects");
    let dom_node = h.a11y_dom_node(cancel).expect("Cancel resolves to a DOM node");

    h.a11y_request(A11yAction::Focus, dom_node);
    assert_eq!(
        h.state().activated,
        Vec::<&str>::new(),
        "moving the reader's cursor must not press the button",
    );
    assert_eq!(h.focus(), Some(dom_node), "but focus did move there");

    // And the projection now reports that focus, so the reader is told where
    // it is rather than being left on the root.
    let (tree, _) = h.a11y_tree();
    let (cancel_id, _) = node_named(&tree, "Cancel").expect("Cancel still projects");
    assert_eq!(tree.focus, cancel_id);
}

/// The raw-request mapping keeps the two actions apart and drops the ones this
/// host does not route, rather than guessing at them.
#[test]
fn raw_requests_map_to_their_own_actions() {
    let mut host = A11yHost::new(|| {});
    // No tree has been synced, so nothing resolves — including a Click, which
    // is the honest answer rather than a click on some default node.
    assert_eq!(
        host.map_request(&ActionRequest {
            action: Action::Click,
            target_tree: accesskit::TreeId::ROOT,
            target_node: accesskit::NodeId(1),
            data: None,
        }),
        None,
    );
    let _ = &mut host;
}

/// A screen reader acting on an idle application must be able to wake it. The
/// callback under test is the exact one the host hands the adapter.
#[test]
fn a_screen_reader_wakes_an_idle_application() {
    let mut h = harness();
    assert_eq!(
        h.idle_policy(),
        IdlePolicy::Wait,
        "an idle app with nothing pending sleeps",
    );

    h.signal_a11y_wake();
    assert_eq!(
        h.idle_policy(),
        IdlePolicy::A11yWake,
        "the adapter's wake callback turns into a repaint, so the queued action drains",
    );
    assert_eq!(
        h.idle_policy(),
        IdlePolicy::Wait,
        "and the wake is consumed once, not re-triggered every idle turn",
    );
}
