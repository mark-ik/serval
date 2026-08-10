/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! `LayoutDom` + layout fragments -> AccessKit tree emission.
//!
//! The builder lives beside `GenetLaneView` so every consumer with a laid-out
//! Genet lane can ask for the same accessibility tree. The host still owns the
//! platform adapter; this module only emits the engine-side `TreeUpdate`.

use std::collections::HashMap;
use std::hash::Hash;

use accesskit::{
    Action, Node as AccessNode, NodeId as AccessNodeId, Rect, Role, Toggled, Tree, TreeId,
    TreeUpdate,
};
use layout_dom_api::{LayoutDom, LocalName, Namespace, NodeKind};

use crate::construct::custom_leaf_key_of;
use crate::fragment::FragmentPlane;

/// Host-side source of custom-leaf accessibility semantics.
///
/// Mirrors [`LeafPaintSource`](crate::LeafPaintSource): genet-layout knows
/// `<custom-leaf key="…">` as an element, not a toolkit's types, so the host
/// bridges a leaf key to its registered leaf. A leaf fills its own AccessKit
/// node (a knob announces as a slider carrying its value) and may override the
/// role the tag walk assigned, since a leaf's interior is invisible to the DOM.
///
/// Two things are not the leaf's to decide:
///
/// - **Geometry.** The walk stamps absolute bounds after the leaf has spoken, so
///   a leaf can never disagree with layout about where it is.
/// - **A name the author gave it.** The walk resolves `aria-label` (then direct
///   text) *before* calling the leaf, so an author who wrote
///   `<custom-leaf aria-label="Session graph">` has said what this instance is,
///   and a leaf's generic self-description must not overwrite that. A leaf that
///   wants a fallback name checks [`AccessNode::label`] first and only fills a
///   gap. Roles and values carry no such rule: those are facts about the widget,
///   not editorial choices, so a leaf always wins on them.
pub trait LeafA11ySource {
    /// Fill `node` with the semantics of the leaf registered under `key`. An
    /// absent key must leave `node` untouched (the leaf stays an opaque box).
    fn describe_leaf(&mut self, key: u64, node: &mut AccessNode);
}

/// A source with no leaves: every `<custom-leaf>` stays an opaque container.
/// What [`accesskit_tree`] and [`build_subtree`] use when the caller has none.
pub struct NoLeafA11y;

impl LeafA11ySource for NoLeafA11y {
    fn describe_leaf(&mut self, _key: u64, _node: &mut AccessNode) {}
}

/// The actions a host can route back to a node. A node advertising any of these
/// is handed back to the caller, whether it acquired the action from its role
/// (a `<button>` takes `Click`) or from a leaf declaring its own (a slider takes
/// `SetValue` / `Increment` / `Decrement`).
pub const ROUTABLE_ACTIONS: [Action; 5] = [
    Action::Click,
    Action::Focus,
    Action::SetValue,
    Action::Increment,
    Action::Decrement,
];

/// One projected node: the AccessKit node, its id, and the DOM node it came from.
///
/// Keeping the DOM node is what lets a caller act on what it found. Automation
/// queries the same tree a screen reader reads, then routes back through the DOM,
/// so the two can never disagree about what is on screen.
pub struct ProjectedNode<Id> {
    pub dom: Id,
    pub id: AccessNodeId,
    pub node: AccessNode,
}

/// A laid-out subtree projected once: every node, in insertion order (children
/// before parents), paired with its DOM origin.
///
/// This is the single semantic projection. [`accesskit_tree`] and
/// [`build_subtree`] are views onto it, and element queries read it rather than
/// re-deriving roles from the DOM, so a query can never drift from what assistive
/// tech sees.
pub struct Projection<Id> {
    pub root: AccessNodeId,
    pub nodes: Vec<ProjectedNode<Id>>,
}

impl<Id: Copy> Projection<Id> {
    /// The DOM nodes advertising an action a host can route (see
    /// [`ROUTABLE_ACTIONS`]), whether it came from a role or from a leaf.
    pub fn actionable(&self) -> Vec<Id> {
        self.nodes
            .iter()
            .filter(|projected| {
                ROUTABLE_ACTIONS
                    .iter()
                    .any(|action| projected.node.supports_action(*action))
            })
            .map(|projected| projected.dom)
            .collect()
    }
}

fn access_id<D: LayoutDom>(dom: &D, node: D::NodeId) -> AccessNodeId {
    AccessNodeId(dom.opaque_id(node))
}

fn attr<'a, D>(dom: &'a D, node: D::NodeId, name: &str) -> Option<&'a str>
where
    D: LayoutDom,
{
    dom.attribute(node, &Namespace::default(), &LocalName::from(name))
}

/// Whether `node` is `aria-hidden="true"` — removed from the accessibility tree
/// along with everything under it.
///
/// Decorative structure is the ordinary case, not an exotic one: a window's
/// drag surface, a spacer, an icon whose meaning is already in its control's
/// label. Without this every one of them becomes a focus stop or a node a
/// screen reader reads out, and an application has no way to say otherwise.
fn aria_hidden<D>(dom: &D, node: D::NodeId) -> bool
where
    D: LayoutDom,
{
    attr(dom, node, "aria-hidden") == Some("true")
}

fn role_for<D>(dom: &D, node: D::NodeId) -> Role
where
    D: LayoutDom,
{
    if dom.kind(node) == NodeKind::Element {
        if let Some(role) = attr(dom, node, "role") {
            match role {
                "button" => return Role::Button,
                "checkbox" => return Role::CheckBox,
                "radio" => return Role::RadioButton,
                "radiogroup" => return Role::RadioGroup,
                "switch" => return Role::Switch,
                "tab" => return Role::Tab,
                "tablist" => return Role::TabList,
                // Value-bearing and announcement roles. Without these an
                // authored `role="alert"` projects as a generic container, so a
                // screen reader is never told the thing happened, and a
                // `role="progressbar"` announces as "group" with no value.
                "alert" => return Role::Alert,
                "status" => return Role::Status,
                "log" => return Role::Log,
                "progressbar" => return Role::ProgressIndicator,
                "slider" => return Role::Slider,
                "spinbutton" => return Role::SpinButton,
                "textbox" => return Role::TextInput,
                "note" => return Role::Note,
                "list" => return Role::List,
                "listitem" => return Role::ListItem,
                "main" => return Role::Main,
                "navigation" => return Role::Navigation,
                "region" => return Role::Region,
                "heading" => return Role::Heading,
                _ => {},
            }
        }
    }

    match dom.kind(node) {
        NodeKind::Document => Role::Window,
        NodeKind::Element => match dom.element_name(node).map(|q| q.local.as_ref()) {
            Some("button") => Role::Button,
            Some("input") => Role::TextInput,
            Some("p") => Role::Paragraph,
            Some("label") => Role::Label,
            Some("html") => Role::Document,
            _ => Role::GenericContainer,
        },
        _ => Role::GenericContainer,
    }
}

/// The text a `<label>` contributes as a name: everything under it, **except**
/// the text inside the control it wraps.
///
/// Direct text alone is not enough — a caption is routinely wrapped for
/// styling, as `<label><div>Board revision</div><input></label>`, and the label
/// would then contribute nothing. Excluding embedded controls is what keeps the
/// name stable: a `TextInput` renders its buffer as the `<input>`'s text, so
/// folding that in would rename the field to whatever had been typed into it.
fn label_text<D>(dom: &D, node: D::NodeId) -> String
where
    D: LayoutDom,
{
    fn collect<D: LayoutDom>(dom: &D, node: D::NodeId, out: &mut String) {
        for child in dom.dom_children(node) {
            match dom.kind(child) {
                NodeKind::Text => {
                    if let Some(text) = dom.text(child) {
                        out.push_str(text);
                    }
                },
                NodeKind::Element => {
                    let is_control = dom.element_name(child).is_some_and(|q| {
                        matches!(q.local.as_ref(), "input" | "textarea" | "select" | "button")
                    });
                    if !is_control {
                        collect(dom, child, out);
                    }
                },
                _ => {},
            }
        }
    }
    let mut out = String::new();
    collect(dom, node, &mut out);
    out.trim().to_string()
}

fn direct_text<D>(dom: &D, node: D::NodeId) -> String
where
    D: LayoutDom,
{
    let mut name = String::new();
    for child in dom.dom_children(node) {
        if dom.kind(child) == NodeKind::Text {
            if let Some(text) = dom.text(child) {
                name.push_str(text);
            }
        }
    }
    name
}

/// The shared subtree walk behind both [`accesskit_tree`] (the sealed engine
/// tree) and [`build_subtree`] (a host stitching several subtrees). `id_of`
/// assigns each node its id, `skip` prunes element subtrees the caller projects
/// elsewhere, `leaves` fills in each `<custom-leaf>`'s interior semantics, and
/// `advertise_actions` gates whether controls declare the host action they
/// accept (recording them in `actionable`) — off for the engine tree so hosts
/// that don't route actions don't promise affordances they can't honor.
///
/// `label_ctx` carries the text of an enclosing `<label>` down to the control it
/// wraps, which is how HTML names a field without an `id` anywhere.
#[allow(clippy::too_many_arguments)]
fn walk<D, I, S>(
    dom: &D,
    fragments: &FragmentPlane<D::NodeId>,
    origins: &HashMap<D::NodeId, (f32, f32)>,
    node: D::NodeId,
    id_of: &I,
    skip: &S,
    leaves: &mut dyn LeafA11ySource,
    advertise_actions: bool,
    label_ctx: Option<&str>,
    out: &mut Vec<ProjectedNode<D::NodeId>>,
) -> AccessNodeId
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
    I: Fn(&D, D::NodeId) -> AccessNodeId,
    S: Fn(&D, D::NodeId) -> bool,
{
    let id = id_of(dom, node);
    let mut access = AccessNode::new(role_for(dom, node));

    // Accessible name: `aria-label` wins (ARIA semantics), else the node's direct
    // text, else the text of an enclosing `<label>`. Icon-only or nested controls
    // carry no direct text, so `aria-label` is how a host names them — and a
    // wrapped `<label>Board revision <input></label>` is how HTML names a field
    // with no `id` in sight, which is the shape a generated control leaves you.
    let name = attr(dom, node, "aria-label")
        .map(str::to_string)
        .or_else(|| Some(direct_text(dom, node)).filter(|t| !t.is_empty()))
        .or_else(|| label_ctx.map(str::to_string))
        .unwrap_or_default();
    if !name.is_empty() {
        access.set_label(name);
    }

    if let Some(toggled) = attr(dom, node, "aria-checked").and_then(|v| match v {
        "true" => Some(Toggled::True),
        "false" => Some(Toggled::False),
        "mixed" => Some(Toggled::Mixed),
        _ => None,
    }) {
        access.set_toggled(toggled);
    }

    // `aria-selected` maps to AccessKit's first-class selection flag (a tab, a
    // listbox option, a roster row), so AT announces and queries selection as
    // state rather than parsing it out of a description string.
    match attr(dom, node, "aria-selected") {
        Some("true") => access.set_selected(true),
        Some("false") => access.set_selected(false),
        _ => {},
    }

    // A pressed toggle button reports as toggled, the same as a checkbox: an
    // `aria-pressed` row whose selection is only visible in CSS is invisible to
    // a reader.
    if let Some(pressed) = attr(dom, node, "aria-pressed").and_then(|v| match v {
        "true" => Some(Toggled::True),
        "false" => Some(Toggled::False),
        "mixed" => Some(Toggled::Mixed),
        _ => None,
    }) {
        access.set_toggled(pressed);
    }

    // The value range for a progress bar, slider, or spin button. A progress
    // indicator with no value announces only that something is happening, which
    // is exactly the "fake spinner" a real transfer must not be reduced to.
    for (name, set) in [
        ("aria-valuenow", 0u8),
        ("aria-valuemin", 1),
        ("aria-valuemax", 2),
    ] {
        if let Some(value) = attr(dom, node, name).and_then(|v| v.parse::<f64>().ok()) {
            match set {
                0 => access.set_numeric_value(value),
                1 => access.set_min_numeric_value(value),
                _ => access.set_max_numeric_value(value),
            }
        }
    }

    // A chisel leaf is a replaced element: its interior is invisible to the DOM,
    // so the leaf speaks for itself here. It may override the role the tag walk
    // assigned, name itself, carry a value, and declare its own actions. It runs
    // after the DOM-derived semantics (so a leaf wins) and before bounds (so
    // layout wins on geometry).
    if let Some(key) = custom_leaf_key_of(dom, node) {
        leaves.describe_leaf(key, &mut access);
    }

    if advertise_actions {
        // Toggle controls (switch / checkbox / radio) are invoked via `Click` in
        // AccessKit, same as a button; a text field takes `Focus`. Read the role
        // back off the node, not from the tag, so a leaf that promoted itself to
        // a control is treated as one.
        let action = match access.role() {
            Role::Button | Role::Switch | Role::CheckBox | Role::RadioButton | Role::Tab => {
                Some(Action::Click)
            },
            Role::TextInput => Some(Action::Focus),
            _ => None,
        };
        if let Some(action) = action {
            access.add_action(action);
        }
        // Whether this node is routable is read back off the finished node by
        // `Projection::actionable`, so a leaf that declared `SetValue` on itself
        // counts exactly like a `<button>` that got `Click` from its role.
    }

    // Bounds. An inline-level element (the UA sheet makes every form control
    // `inline-block`) establishes no Taffy box, so it takes its rect from the
    // plane's inline-box table — an offset from its formatting leaf's origin,
    // which `origins` already holds. Without it a screen reader's virtual cursor
    // had nothing to land on for an unstyled `<button>`, and where an anonymous
    // wrapper borrowed the control's key for its own entry the announced bounds
    // were the whole line box. See `crate::inline_fragment`.
    let bounds = match fragments.inline_box_of(node) {
        Some(f) => origins
            .get(&f.leaf)
            .map(|&(lx, ly)| (lx + f.x, ly + f.y, f.width, f.height)),
        None => match (origins.get(&node), fragments.rect_of(node)) {
            (Some(&(x0, y0)), Some(layout)) => {
                Some((x0, y0, layout.size.width, layout.size.height))
            },
            _ => None,
        },
    };
    if let Some((x0, y0, w, h)) = bounds {
        let (x0, y0) = (x0 as f64, y0 as f64);
        access.set_bounds(Rect::new(x0, y0, x0 + w as f64, y0 + h as f64));
    }

    // Entering a `<label>` names everything it wraps; otherwise the context
    // passes straight through, so a label nested a level or two above a
    // generated `<input>` still reaches it.
    let is_label = dom
        .element_name(node)
        .is_some_and(|q| q.local.as_ref() == "label");
    let own_text = if is_label {
        label_text(dom, node)
    } else {
        String::new()
    };
    let child_label: Option<&str> = if is_label && !own_text.is_empty() {
        Some(own_text.as_str())
    } else {
        label_ctx
    };
    let mut children = Vec::new();
    for child in dom.dom_children(node) {
        if dom.kind(child) == NodeKind::Element && !skip(dom, child) && !aria_hidden(dom, child) {
            children.push(walk(
                dom,
                fragments,
                origins,
                child,
                id_of,
                skip,
                leaves,
                advertise_actions,
                child_label,
                out,
            ));
        }
    }
    access.set_children(children);

    out.push(ProjectedNode {
        dom: node,
        id,
        node: access,
    });
    id
}

/// Project a laid-out subtree once. Everything else in this module is a view onto
/// the result: the sealed engine tree, a host's stitchable subtree, and element
/// queries all read the same nodes.
#[allow(clippy::too_many_arguments)]
pub fn project<D, I, S>(
    dom: &D,
    fragments: &FragmentPlane<D::NodeId>,
    root: D::NodeId,
    id_of: &I,
    skip: &S,
    leaves: &mut dyn LeafA11ySource,
    advertise_actions: bool,
) -> Projection<D::NodeId>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
    I: Fn(&D, D::NodeId) -> AccessNodeId,
    S: Fn(&D, D::NodeId) -> bool,
{
    let origins = origins_of(dom, fragments);
    let mut nodes = Vec::new();
    let root_id = walk(
        dom,
        fragments,
        &origins,
        root,
        id_of,
        skip,
        leaves,
        advertise_actions,
        None,
        &mut nodes,
    );
    Projection {
        root: root_id,
        nodes,
    }
}

fn origins_of<D>(dom: &D, fragments: &FragmentPlane<D::NodeId>) -> HashMap<D::NodeId, (f32, f32)>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    crate::genet_lane::accumulate_origins(dom, fragments)
        .into_iter()
        .map(|(id, p)| (id, (p.x, p.y)))
        .collect()
}

/// Emit an AccessKit tree for a laid-out Genet DOM.
pub fn accesskit_tree<D>(
    dom: &D,
    fragments: &FragmentPlane<D::NodeId>,
    focus: Option<D::NodeId>,
) -> TreeUpdate
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let root = dom.document();
    let projection = project(
        dom,
        fragments,
        root,
        &|d: &D, n: D::NodeId| access_id(d, n),
        &|_d: &D, _n: D::NodeId| false,
        &mut NoLeafA11y,
        false,
    );

    TreeUpdate {
        nodes: projection
            .nodes
            .into_iter()
            .map(|projected| (projected.id, projected.node))
            .collect(),
        tree: Some(Tree::new(access_id(dom, root))),
        tree_id: TreeId::ROOT,
        focus: access_id(dom, focus.unwrap_or(root)),
    }
}

/// Walk a laid-out subtree into AccessKit nodes for a host that stitches several
/// subtrees (chrome, content panes, host root) into one tree before converting
/// once. Returns the `(id, node)` pairs in insertion order, the subtree root's
/// id, and the DOM nodes that advertise a host action (buttons, text fields) so
/// the host can route an AccessKit request back to its activation path.
///
/// `id_of` assigns each node its id: a stitching host salts ids into a range
/// disjoint from its other subtrees, where [`accesskit_tree`] uses the DOM's
/// opaque id. `skip` prunes element subtrees the host projects elsewhere (a pane
/// it gives richer, actionable a11y of its own). Roles honor ARIA `role=` then
/// tag, and `aria-checked` sets toggled state — the same leaf logic as the
/// engine tree, so a host subtree never drifts behind on standards support.
///
/// Chisel leaves stay opaque containers here. A host with leaves calls
/// [`build_subtree_with_leaves`] instead.
#[allow(clippy::type_complexity)]
pub fn build_subtree<D, I, S>(
    dom: &D,
    fragments: &FragmentPlane<D::NodeId>,
    root: D::NodeId,
    id_of: &I,
    skip: &S,
) -> (
    Vec<(AccessNodeId, AccessNode)>,
    AccessNodeId,
    Vec<D::NodeId>,
)
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
    I: Fn(&D, D::NodeId) -> AccessNodeId,
    S: Fn(&D, D::NodeId) -> bool,
{
    build_subtree_with_leaves(dom, fragments, root, id_of, skip, &mut NoLeafA11y)
}

/// [`build_subtree`], with each `<custom-leaf>`'s interior filled in by `leaves`.
///
/// A leaf is a replaced element, so nothing about its interior reaches the DOM;
/// without a source it projects as an unlabeled container. With one, a `Knob`
/// announces as a slider carrying its value, a `Meter` as a meter, and a leaf
/// that declares an action (`SetValue`, `Click`) is handed back in the
/// actionable list exactly like a `<button>`, so one routing path serves DOM
/// controls and leaf interiors alike.
#[allow(clippy::type_complexity)]
pub fn build_subtree_with_leaves<D, I, S>(
    dom: &D,
    fragments: &FragmentPlane<D::NodeId>,
    root: D::NodeId,
    id_of: &I,
    skip: &S,
    leaves: &mut dyn LeafA11ySource,
) -> (
    Vec<(AccessNodeId, AccessNode)>,
    AccessNodeId,
    Vec<D::NodeId>,
)
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
    I: Fn(&D, D::NodeId) -> AccessNodeId,
    S: Fn(&D, D::NodeId) -> bool,
{
    let projection = project(dom, fragments, root, id_of, skip, leaves, true);
    let actionable = projection.actionable();
    let root_id = projection.root;
    let nodes = projection
        .nodes
        .into_iter()
        .map(|projected| (projected.id, projected.node))
        .collect();
    (nodes, root_id, actionable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ImagePlane, StylePlane, layout, run_cascade};
    use genet_scripted_dom::{NodeId, ScriptedDom};
    use layout_dom_api::{LayoutDomMut, QualName};

    const SHEET: &[&str] = &["div, p, button { display: block; }"];

    fn html(local: &str) -> QualName {
        QualName::new(
            None,
            Namespace::from("http://www.w3.org/1999/xhtml"),
            LocalName::from(local),
        )
    }

    fn attr_name(local: &str) -> QualName {
        QualName::new(None, Namespace::from(""), LocalName::from(local))
    }

    fn fragments_from_scripted_dom(dom: &ScriptedDom) -> FragmentPlane<NodeId> {
        let mut styles = StylePlane::new();
        run_cascade(
            dom,
            &mut styles,
            euclid::Size2D::new(800.0, 600.0),
            SHEET,
            None,
        );
        let viewport = taffy::Size {
            width: taffy::AvailableSpace::Definite(800.0),
            height: taffy::AvailableSpace::Definite(600.0),
        };
        layout(dom, &styles, &ImagePlane::new(), viewport).0
    }

    /// ARIA tab semantics: `role="tab"` / `"tablist"` map to their AccessKit
    /// roles, and `aria-selected` becomes the first-class selection flag — state
    /// AT queries, not prose parsed out of a description.
    #[test]
    fn aria_tab_and_selected_reach_the_tree() {
        let mut dom = ScriptedDom::new();
        let root = dom.document();
        let bar = dom.create_element(html("div"));
        dom.set_attribute(bar, attr_name("role"), "tablist");
        dom.append_child(root, bar);
        let tab = dom.create_element(html("div"));
        dom.set_attribute(tab, attr_name("role"), "tab");
        dom.set_attribute(tab, attr_name("aria-selected"), "true");
        dom.append_child(bar, tab);

        let fragments = fragments_from_scripted_dom(&dom);
        let (nodes, _, actionable) = build_subtree(
            &dom,
            &fragments,
            root,
            &|d: &ScriptedDom, n: NodeId| access_id(d, n),
            &|_d: &ScriptedDom, _n: NodeId| false,
        );
        let node = |n: NodeId| {
            nodes
                .iter()
                .find(|(id, _)| *id == access_id(&dom, n))
                .map(|(_, node)| node)
                .expect("node in tree")
        };

        assert_eq!(node(bar).role(), Role::TabList);
        assert_eq!(node(tab).role(), Role::Tab);
        assert_eq!(node(tab).is_selected(), Some(true));
        assert!(
            node(tab).supports_action(Action::Click),
            "a tab is invoked via Click, like a button"
        );
        assert_eq!(actionable, vec![tab], "the tab is routable; the bar is not");
    }

    /// A `LeafA11ySource` standing in for chisel's registry: key 7 is a knob.
    struct KnobAt7;

    impl LeafA11ySource for KnobAt7 {
        fn describe_leaf(&mut self, key: u64, node: &mut AccessNode) {
            if key != 7 {
                return;
            }
            node.set_role(Role::Slider);
            node.set_label("Gain");
            node.set_numeric_value(0.25);
            node.add_action(Action::SetValue);
        }
    }

    /// A leaf offering a *fallback* name, the way `GraphGlyph` does: it fills the
    /// gap only when the author named nothing.
    struct FallbackNamed;

    impl LeafA11ySource for FallbackNamed {
        fn describe_leaf(&mut self, _key: u64, node: &mut AccessNode) {
            node.set_role(Role::GraphicsObject);
            if node.label().is_none() {
                node.set_label("graph: 3 nodes, 2 links");
            }
        }
    }

    /// The author placing a leaf knows what this instance depicts; the leaf only
    /// knows what kind of thing it is. So `aria-label` outranks a leaf's generic
    /// self-description, while the leaf still wins on role, which is a fact about
    /// the widget rather than an editorial choice.
    #[test]
    fn a_leaf_fallback_name_never_overwrites_the_authors_aria_label() {
        let mut dom = ScriptedDom::new();
        let root = dom.document();

        let named = dom.create_element(html("custom-leaf"));
        dom.set_attribute(named, attr_name("key"), "1");
        dom.set_attribute(named, attr_name("aria-label"), "Session graph");
        dom.append_child(root, named);

        let unnamed = dom.create_element(html("custom-leaf"));
        dom.set_attribute(unnamed, attr_name("key"), "2");
        dom.append_child(root, unnamed);

        let fragments = fragments_from_scripted_dom(&dom);
        let (nodes, _, _) = build_subtree_with_leaves(
            &dom,
            &fragments,
            root,
            &|d: &ScriptedDom, n: NodeId| access_id(d, n),
            &|_d: &ScriptedDom, _n: NodeId| false,
            &mut FallbackNamed,
        );
        let node = |n: NodeId| {
            nodes
                .iter()
                .find(|(id, _)| *id == access_id(&dom, n))
                .map(|(_, node)| node)
                .expect("leaf in tree")
        };

        assert_eq!(
            node(named).label(),
            Some("Session graph"),
            "the author's aria-label survives the leaf"
        );
        assert_eq!(
            node(unnamed).label(),
            Some("graph: 3 nodes, 2 links"),
            "the leaf fills a name only where the author left none"
        );
        assert_eq!(
            node(named).role(),
            Role::GraphicsObject,
            "the leaf still wins on role, which is not the author's to state"
        );
    }

    /// A `<custom-leaf>` is a replaced element, so nothing about its interior
    /// reaches the DOM. Without a source it projects as an opaque, unlabeled
    /// container; with one, the leaf names itself, promotes its own role, carries
    /// its value, and lands in the routable set on the strength of the action it
    /// declared — the same handback a `<button>` gets. Layout still owns bounds.
    #[test]
    fn chisel_leaf_interior_reaches_the_tree_through_its_source() {
        let mut dom = ScriptedDom::new();
        let root = dom.document();
        let leaf = dom.create_element(html("custom-leaf"));
        dom.set_attribute(leaf, attr_name("key"), "7");
        dom.append_child(root, leaf);

        let fragments = fragments_from_scripted_dom(&dom);
        let id_of = |d: &ScriptedDom, n: NodeId| access_id(d, n);
        let no_skip = |_d: &ScriptedDom, _n: NodeId| false;

        // Without a source: opaque. The leaf is not a control and not routable.
        let (nodes, _, actionable) = build_subtree(&dom, &fragments, root, &id_of, &no_skip);
        let bare = nodes
            .iter()
            .find(|(id, _)| *id == access_id(&dom, leaf))
            .map(|(_, n)| n)
            .expect("leaf node present");
        assert_eq!(
            bare.role(),
            Role::GenericContainer,
            "opaque without a source"
        );
        assert_eq!(bare.label(), None);
        assert!(actionable.is_empty(), "an opaque leaf advertises nothing");

        // With a source: the leaf speaks for itself.
        let (nodes, _, actionable) =
            build_subtree_with_leaves(&dom, &fragments, root, &id_of, &no_skip, &mut KnobAt7);
        let knob = nodes
            .iter()
            .find(|(id, _)| *id == access_id(&dom, leaf))
            .map(|(_, n)| n)
            .expect("leaf node present");
        assert_eq!(knob.role(), Role::Slider, "the leaf promoted its own role");
        assert_eq!(knob.label(), Some("Gain"));
        assert_eq!(knob.numeric_value(), Some(0.25));
        assert!(knob.supports_action(Action::SetValue));
        assert!(
            knob.bounds().is_some(),
            "layout owns geometry, not the leaf"
        );
        assert_eq!(
            actionable,
            vec![leaf],
            "a leaf that declares an action is routable, like a button"
        );
    }

    #[test]
    fn dom_maps_to_accessibility_tree() {
        let mut dom = ScriptedDom::new();
        let root = dom.document();
        let div = dom.create_element(html("div"));
        dom.append_child(root, div);
        let p = dom.create_element(html("p"));
        dom.append_child(div, p);
        let count = dom.create_text("13");
        dom.append_child(p, count);
        let button = dom.create_element(html("button"));
        dom.append_child(div, button);
        let plus = dom.create_text("+");
        dom.append_child(button, plus);

        let fragments = fragments_from_scripted_dom(&dom);
        let tree = accesskit_tree(&dom, &fragments, Some(button));

        assert_eq!(tree.tree_id, TreeId::ROOT);
        assert_eq!(tree.tree.as_ref().unwrap().root, access_id(&dom, root));
        assert_eq!(tree.focus, access_id(&dom, button));

        let node = |n: NodeId| {
            tree.nodes
                .iter()
                .find(|(id, _)| *id == access_id(&dom, n))
                .map(|(_, node)| node)
                .unwrap_or_else(|| panic!("node missing from a11y tree"))
        };

        let root_node = node(root);
        assert_eq!(root_node.role(), Role::Window);
        assert!(root_node.children().contains(&access_id(&dom, div)));

        let button_node = node(button);
        assert_eq!(button_node.role(), Role::Button);
        assert_eq!(button_node.label(), Some("+"));
        assert!(button_node.bounds().is_some(), "laid-out node has bounds");
        assert!(button_node.children().is_empty());

        let p_node = node(p);
        assert_eq!(p_node.role(), Role::Paragraph);
        assert_eq!(p_node.label(), Some("13"));

        assert!(
            tree.nodes
                .iter()
                .all(|(id, _)| *id != access_id(&dom, plus)),
            "text nodes are folded into element labels"
        );
    }

    /// A screen reader's virtual cursor needs a box to land on, and an
    /// **inline-level** control — which, per the UA sheet, is every unstyled
    /// `<button>` and `<input>` — establishes no Taffy box. Its bounds come from
    /// the plane's inline-box table instead (`crate::inline_fragment`).
    ///
    /// Before that table the two buttons here announced badly in different ways:
    /// the second had no bounds at all, and the first inherited the whole line box
    /// — the anonymous wrapper's rect, which the plane keys under its borrowed
    /// first member. A reader would have reported one full-width control where
    /// there are two side by side.
    ///
    /// Note this fixture does NOT use the module's block-display sheet: the point
    /// is that a control reachable to a reader no longer has to be styled for it.
    #[test]
    fn inline_level_controls_announce_their_own_bounds() {
        let mut dom = ScriptedDom::new();
        let root = dom.document();
        let div = dom.create_element(html("div"));
        dom.append_child(root, div);
        // A block sibling ahead of the buttons is what makes the box tree wrap
        // the button run in an anonymous block box.
        let title = dom.create_element(html("p"));
        let title_text = dom.create_text("Controls");
        dom.append_child(title, title_text);
        dom.append_child(div, title);
        let mut buttons = Vec::new();
        for label in ["Alpha", "Bravo"] {
            let b = dom.create_element(html("button"));
            let t = dom.create_text(label);
            dom.append_child(b, t);
            dom.append_child(div, b);
            buttons.push(b);
        }

        let mut styles = StylePlane::new();
        run_cascade(
            &dom,
            &mut styles,
            euclid::Size2D::new(800.0, 600.0),
            // Only the container is blockified; the buttons keep the UA
            // `inline-block`.
            &["div, p { display: block; }"],
            None,
        );
        let viewport = taffy::Size {
            width: taffy::AvailableSpace::Definite(800.0),
            height: taffy::AvailableSpace::Definite(600.0),
        };
        let fragments = layout(&dom, &styles, &ImagePlane::new(), viewport).0;
        let tree = accesskit_tree(&dom, &fragments, None);

        let bounds = |n: NodeId| {
            tree.nodes
                .iter()
                .find(|(id, _)| *id == access_id(&dom, n))
                .and_then(|(_, node)| node.bounds())
                .unwrap_or_else(|| panic!("an inline-level control needs bounds to be reachable"))
        };
        let (a, b) = (bounds(buttons[0]), bounds(buttons[1]));
        for (name, r) in [("Alpha", a), ("Bravo", b)] {
            assert!(
                r.width() > 0.0 && r.height() > 0.0,
                "{name} has positive area, got {r:?}"
            );
            assert!(
                r.width() < 400.0,
                "{name}'s box is its own, not the 800px line box, got {r:?}"
            );
        }
        assert!(
            b.x0 >= a.x1,
            "the two controls occupy separate boxes on the line, got {a:?} then {b:?}"
        );
    }

    #[test]
    fn aria_role_and_checked_reach_the_tree() {
        let mut dom = ScriptedDom::new();
        let root = dom.document();
        let div = dom.create_element(html("div"));
        dom.append_child(root, div);

        let radio = dom.create_element(html("div"));
        dom.set_attribute(radio, attr_name("role"), "radio");
        dom.set_attribute(radio, attr_name("aria-checked"), "true");
        dom.append_child(div, radio);

        let switch = dom.create_element(html("button"));
        dom.set_attribute(switch, attr_name("role"), "switch");
        dom.set_attribute(switch, attr_name("aria-checked"), "false");
        dom.append_child(div, switch);

        let fragments = fragments_from_scripted_dom(&dom);
        let tree = accesskit_tree(&dom, &fragments, None);
        let node = |n: NodeId| {
            tree.nodes
                .iter()
                .find(|(id, _)| *id == access_id(&dom, n))
                .map(|(_, node)| node)
                .unwrap_or_else(|| panic!("node missing from a11y tree"))
        };

        let radio_node = node(radio);
        assert_eq!(
            radio_node.role(),
            Role::RadioButton,
            "role attr overrides the div tag"
        );
        assert_eq!(
            radio_node.toggled(),
            Some(Toggled::True),
            "aria-checked=true is checked"
        );

        let switch_node = node(switch);
        assert_eq!(
            switch_node.role(),
            Role::Switch,
            "role attr overrides the button tag"
        );
        assert_eq!(
            switch_node.toggled(),
            Some(Toggled::False),
            "aria-checked=false is unchecked"
        );
    }

    /// A wrapping `<label>` names the control inside it. This is how HTML names
    /// a field when no `id` is available to point `for` at — the shape you get
    /// whenever the control is generated by a component rather than authored.
    #[test]
    fn a_wrapping_label_names_the_control_inside_it() {
        let mut dom = ScriptedDom::new();
        let root = dom.document();

        let label = dom.create_element(html("label"));
        dom.append_child(root, label);
        let caption = dom.create_text("Board revision");
        dom.append_child(label, caption);
        // A wrapper between the label and the field, as a component's markup
        // would produce: the name still has to reach through it.
        let wrap = dom.create_element(html("div"));
        dom.append_child(label, wrap);
        let field = dom.create_element(html("input"));
        dom.append_child(wrap, field);

        // And a control with its own name is not overwritten by the label.
        let named = dom.create_element(html("input"));
        dom.set_attribute(named, attr_name("aria-label"), "Its own name");
        dom.append_child(label, named);

        let fragments = fragments_from_scripted_dom(&dom);
        let tree = accesskit_tree(&dom, &fragments, None);
        let node = |n: NodeId| {
            tree.nodes
                .iter()
                .find(|(id, _)| *id == access_id(&dom, n))
                .map(|(_, node)| node)
                .expect("node missing from a11y tree")
        };

        assert_eq!(
            node(field).label().map(|l| l.to_string()),
            Some("Board revision".to_string()),
            "the field takes its name from the label wrapping it",
        );
        assert_eq!(
            node(named).label().map(|l| l.to_string()),
            Some("Its own name".to_string()),
            "a control that names itself keeps its own name",
        );
    }

    /// `aria-hidden="true"` removes a node and its subtree from the tree.
    /// Decoration a screen reader should not meet: a drag surface, a spacer, an
    /// icon whose meaning is already in its control's label.
    #[test]
    fn aria_hidden_removes_a_subtree() {
        let mut dom = ScriptedDom::new();
        let root = dom.document();
        let div = dom.create_element(html("div"));
        dom.append_child(root, div);

        let shown = dom.create_element(html("button"));
        dom.set_attribute(shown, attr_name("aria-label"), "Close");
        dom.append_child(div, shown);

        let decoration = dom.create_element(html("div"));
        dom.set_attribute(decoration, attr_name("aria-hidden"), "true");
        dom.append_child(div, decoration);
        // Something inside it that would otherwise project on its own.
        let buried = dom.create_element(html("button"));
        dom.set_attribute(buried, attr_name("aria-label"), "Drag");
        dom.append_child(decoration, buried);

        let fragments = fragments_from_scripted_dom(&dom);
        let tree = accesskit_tree(&dom, &fragments, None);
        let has = |n: NodeId| tree.nodes.iter().any(|(id, _)| *id == access_id(&dom, n));

        assert!(has(shown), "an ordinary control still projects");
        assert!(!has(decoration), "the hidden node is gone");
        assert!(!has(buried), "and so is everything under it");
    }

    /// The announcement and value-bearing roles. Each of these was a generic
    /// container before: an authored `role="alert"` never reached the reader as
    /// an alert, and a `role="progressbar"` announced with no value at all —
    /// which is the "fake spinner" a real transfer must never be reduced to.
    #[test]
    fn announcement_and_value_roles_reach_the_tree() {
        let mut dom = ScriptedDom::new();
        let root = dom.document();
        let div = dom.create_element(html("div"));
        dom.append_child(root, div);

        let alert = dom.create_element(html("div"));
        dom.set_attribute(alert, attr_name("role"), "alert");
        dom.append_child(div, alert);

        let bar = dom.create_element(html("div"));
        dom.set_attribute(bar, attr_name("role"), "progressbar");
        dom.set_attribute(bar, attr_name("aria-label"), "Transfer");
        dom.set_attribute(bar, attr_name("aria-valuenow"), "42");
        dom.set_attribute(bar, attr_name("aria-valuemin"), "0");
        dom.set_attribute(bar, attr_name("aria-valuemax"), "100");
        dom.append_child(div, bar);

        let row = dom.create_element(html("button"));
        dom.set_attribute(row, attr_name("aria-pressed"), "true");
        dom.append_child(div, row);

        let log = dom.create_element(html("ul"));
        dom.set_attribute(log, attr_name("role"), "log");
        dom.append_child(div, log);

        let fragments = fragments_from_scripted_dom(&dom);
        let tree = accesskit_tree(&dom, &fragments, None);
        let node = |n: NodeId| {
            tree.nodes
                .iter()
                .find(|(id, _)| *id == access_id(&dom, n))
                .map(|(_, node)| node)
                .unwrap_or_else(|| panic!("node missing from a11y tree"))
        };

        assert_eq!(node(alert).role(), Role::Alert, "a refusal announces");
        assert_eq!(node(log).role(), Role::Log, "an event log announces");

        let bar_node = node(bar);
        assert_eq!(bar_node.role(), Role::ProgressIndicator);
        assert_eq!(
            bar_node.numeric_value(),
            Some(42.0),
            "and it carries how far along it is",
        );
        assert_eq!(bar_node.min_numeric_value(), Some(0.0));
        assert_eq!(bar_node.max_numeric_value(), Some(100.0));

        assert_eq!(
            node(row).toggled(),
            Some(Toggled::True),
            "a pressed toggle button reports as toggled, not only in CSS",
        );
    }

    #[test]
    fn genet_lane_view_emits_accessibility_tree() {
        let mut dom = ScriptedDom::new();
        let root = dom.document();
        let button = dom.create_element(html("button"));
        dom.append_child(root, button);
        let label = dom.create_text("Go");
        dom.append_child(button, label);

        let mut styles = StylePlane::new();
        run_cascade(
            &dom,
            &mut styles,
            euclid::Size2D::new(800.0, 600.0),
            SHEET,
            None,
        );
        let viewport = taffy::Size {
            width: taffy::AvailableSpace::Definite(800.0),
            height: taffy::AvailableSpace::Definite(600.0),
        };
        let (fragments, _, _) = layout(&dom, &styles, &ImagePlane::new(), viewport);
        let view = crate::GenetLaneView::new(&dom, &styles, &fragments);

        let tree = view.accesskit_tree(Some(button));
        assert_eq!(tree.focus, access_id(&dom, button));
        assert!(
            tree.nodes
                .iter()
                .any(|(id, node)| *id == access_id(&dom, button)
                    && node.role() == Role::Button
                    && node.label() == Some("Go"))
        );
    }

    #[test]
    fn focus_falls_back_to_root() {
        let mut dom = ScriptedDom::new();
        let root = dom.document();
        let div = dom.create_element(html("div"));
        dom.append_child(root, div);

        let fragments = fragments_from_scripted_dom(&dom);
        let tree = accesskit_tree(&dom, &fragments, None);
        assert_eq!(tree.focus, access_id(&dom, root));
    }
}
