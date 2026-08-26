/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::hash::Hash;

use accesskit::{
    Action, Node as AccessNode, NodeId as AccessNodeId, Rect, Role, Tree, TreeId, TreeUpdate,
};
use genet_livery::LiveryLayout;
use layout_dom_api::{LayoutDom, LocalName, Namespace, NodeKind};

fn access_id<D: LayoutDom>(dom: &D, node: D::NodeId) -> AccessNodeId {
    AccessNodeId(dom.opaque_id(node))
}

fn role_for<D: LayoutDom>(dom: &D, node: D::NodeId) -> Role {
    if let Some(role) = dom.attribute(node, &Namespace::default(), &LocalName::from("role")) {
        match role {
            "button" => return Role::Button,
            "checkbox" => return Role::CheckBox,
            "radio" => return Role::RadioButton,
            "switch" => return Role::Switch,
            "tab" => return Role::Tab,
            "tablist" => return Role::TabList,
            "progressbar" => return Role::ProgressIndicator,
            "slider" => return Role::Slider,
            "textbox" => return Role::TextInput,
            "link" => return Role::Link,
            "document" => return Role::Document,
            "main" => return Role::Main,
            "navigation" => return Role::Navigation,
            "region" => return Role::Region,
            "heading" => return Role::Heading,
            "alert" => return Role::Alert,
            "status" => return Role::Status,
            _ => {},
        }
    }
    match dom.kind(node) {
        NodeKind::Document => Role::Window,
        NodeKind::Element => match dom.element_name(node).map(|name| name.local.as_ref()) {
            Some("button") => Role::Button,
            Some("input" | "textarea") => Role::TextInput,
            Some("p") => Role::Paragraph,
            Some("label") => Role::Label,
            Some("html") => Role::Document,
            _ => Role::GenericContainer,
        },
        _ => Role::GenericContainer,
    }
}

fn direct_text<D: LayoutDom>(dom: &D, node: D::NodeId) -> String {
    dom.dom_children(node)
        .filter_map(|child| {
            (dom.kind(child) == NodeKind::Text)
                .then(|| dom.text(child))
                .flatten()
        })
        .collect()
}

/// One ARIA numeric attribute, when it parses. A malformed value is left unset
/// rather than projected as zero: a reader is better told nothing than told a
/// wrong position.
fn aria_number<D: LayoutDom>(dom: &D, node: D::NodeId, name: &str) -> Option<f64> {
    dom.attribute(node, &Namespace::default(), &LocalName::from(name))
        .and_then(|value| value.trim().parse::<f64>().ok())
}

/// One ARIA boolean attribute, when it is explicitly true. An absent or
/// malformed value is left unset rather than inferring an interaction state.
fn aria_true<D: LayoutDom>(dom: &D, node: D::NodeId, name: &str) -> bool {
    dom.attribute(node, &Namespace::default(), &LocalName::from(name))
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("true"))
}

fn is_content_editable<D: LayoutDom>(dom: &D, node: D::NodeId) -> bool {
    dom.attribute(
        node,
        &Namespace::default(),
        &LocalName::from("contenteditable"),
    )
    .is_some_and(|value| {
        let value = value.trim();
        value.is_empty()
            || value.eq_ignore_ascii_case("true")
            || value.eq_ignore_ascii_case("plaintext-only")
    })
}

fn has_tabindex<D: LayoutDom>(dom: &D, node: D::NodeId) -> bool {
    dom.attribute(node, &Namespace::default(), &LocalName::from("tabindex"))
        .is_some_and(|value| value.trim().parse::<i32>().is_ok())
}

fn is_native_control<D: LayoutDom>(dom: &D, node: D::NodeId) -> bool {
    matches!(
        dom.element_name(node).map(|name| name.local.as_ref()),
        Some("button" | "input" | "select" | "textarea")
    ) || (dom.element_name(node).map(|name| name.local.as_ref()) == Some("a")
        && dom
            .attribute(node, &Namespace::default(), &LocalName::from("href"))
            .is_some())
}

fn supports_semantic_action(role: Role) -> bool {
    matches!(
        role,
        Role::Button
            | Role::CheckBox
            | Role::RadioButton
            | Role::Switch
            | Role::Tab
            | Role::Slider
            | Role::TextInput
            | Role::Link
    )
}

fn walk<D>(
    dom: &D,
    fragments: &LiveryLayout<D::NodeId>,
    node: D::NodeId,
    out: &mut Vec<(AccessNodeId, AccessNode)>,
) -> AccessNodeId
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let id = access_id(dom, node);
    let role = role_for(dom, node);
    let mut access = AccessNode::new(role);
    let label = dom
        .attribute(node, &Namespace::default(), &LocalName::from("aria-label"))
        .map(str::to_owned)
        .unwrap_or_else(|| direct_text(dom, node));
    if !label.is_empty() {
        access.set_label(label);
    }
    if aria_true(dom, node, "aria-readonly") {
        access.set_read_only();
    }
    let semantic_control = is_native_control(dom, node) || supports_semantic_action(role);
    let focusable = semantic_control || has_tabindex(dom, node) || is_content_editable(dom, node);
    if semantic_control || is_content_editable(dom, node) {
        access.add_action(Action::Click);
    }
    if focusable {
        access.add_action(Action::Focus);
    }
    // A progress bar or slider whose value never reaches the tree is
    // decoration: the reader is told it exists but not how far along it is.
    if let Some(value) = aria_number(dom, node, "aria-valuenow") {
        access.set_numeric_value(value);
    }
    if let Some(value) = aria_number(dom, node, "aria-valuemin") {
        access.set_min_numeric_value(value);
    }
    if let Some(value) = aria_number(dom, node, "aria-valuemax") {
        access.set_max_numeric_value(value);
    }
    if let Some(fragment) = fragments.get(node) {
        access.set_bounds(Rect::new(
            fragment.x as f64,
            fragment.y as f64,
            (fragment.x + fragment.width) as f64,
            (fragment.y + fragment.height) as f64,
        ));
    }
    let children: Vec<_> = dom
        .dom_children(node)
        .filter(|child| dom.kind(*child) == NodeKind::Element)
        .map(|child| walk(dom, fragments, child, out))
        .collect();
    access.set_children(children);
    out.push((id, access));
    id
}

/// Project a Livery/Buckram document into an AccessKit tree.
pub fn accesskit_tree<D>(
    dom: &D,
    fragments: &LiveryLayout<D::NodeId>,
    focus: Option<D::NodeId>,
) -> TreeUpdate
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let root = dom.document();
    let mut nodes = Vec::new();
    walk(dom, fragments, root, &mut nodes);
    TreeUpdate {
        nodes,
        tree: Some(Tree::new(access_id(dom, root))),
        tree_id: TreeId::ROOT,
        focus: access_id(dom, focus.unwrap_or(root)),
    }
}

#[cfg(test)]
mod tests {
    use accesskit::{Action, Node as AccessNode, Role};
    use genet_scripted_dom::ScriptedDom;
    use layout_dom_api::{LayoutDom, LayoutDomMut};

    use super::accesskit_tree;
    use crate::fragments_from_scripted_dom;

    const SHEET: &[&str] = &["div { display: block; }"];

    fn nodes_for(html: &str) -> Vec<AccessNode> {
        let mut dom = ScriptedDom::new();
        let root = dom.document();
        dom.set_inner_html(root, html);
        let fragments = fragments_from_scripted_dom(&dom, SHEET, 400, 300).expect("layout");
        accesskit_tree(&dom, &fragments, None)
            .nodes
            .into_iter()
            .map(|(_, node)| node)
            .collect()
    }

    fn with_role(html: &str, role: Role) -> AccessNode {
        nodes_for(html)
            .into_iter()
            .find(|node| node.role() == role)
            .unwrap_or_else(|| panic!("no node projected with role {role:?}"))
    }

    #[test]
    fn a_refusal_projects_as_an_alert() {
        let nodes = nodes_for("<div role=\"alert\">Cannot flash this board</div>");
        assert!(
            nodes.iter().any(|node| node.role() == Role::Alert),
            "role=alert must reach the reader as an alert, not a generic container",
        );
    }

    #[test]
    fn a_progress_bar_carries_its_value() {
        let bar = with_role(
            "<div role=\"progressbar\" aria-valuenow=\"50\" aria-valuemin=\"0\" aria-valuemax=\"100\"></div>",
            Role::ProgressIndicator,
        );
        assert_eq!(bar.numeric_value(), Some(50.0));
        assert_eq!(bar.min_numeric_value(), Some(0.0));
        assert_eq!(bar.max_numeric_value(), Some(100.0));
    }

    #[test]
    fn a_malformed_value_is_left_unset() {
        let bar = with_role(
            "<div role=\"progressbar\" aria-valuenow=\"soon\"></div>",
            Role::ProgressIndicator,
        );
        assert_eq!(bar.numeric_value(), None);
    }

    #[test]
    fn a_read_only_document_keeps_its_document_semantics() {
        let document = with_role(
            "<div role=\"document\" aria-readonly=\"true\">Read-only notes</div>",
            Role::Document,
        );
        assert!(document.is_read_only());
    }

    #[test]
    fn landmarks_and_status_reach_the_reader() {
        let nodes = nodes_for(
            "<section role=\"region\" aria-label=\"Related notes\"></section>\
             <div role=\"status\">Synced</div>",
        );
        assert!(nodes.iter().any(|node| node.role() == Role::Region));
        assert!(nodes.iter().any(|node| node.role() == Role::Status));
    }

    #[test]
    fn controls_advertise_click_and_focus_separately() {
        let button = with_role("<button>Open</button>", Role::Button);
        assert!(button.supports_action(Action::Click));
        assert!(button.supports_action(Action::Focus));

        let focusable = nodes_for("<div tabindex=\"0\">Focus only</div>")
            .into_iter()
            .find(|node| node.role() == Role::GenericContainer)
            .expect("focusable div");
        assert!(focusable.supports_action(Action::Focus));
        assert!(!focusable.supports_action(Action::Click));
    }

    #[test]
    fn contenteditable_nodes_are_clickable_and_focusable() {
        let editor = with_role(
            "<div role=\"textbox\" contenteditable>Notes</div>",
            Role::TextInput,
        );
        assert!(editor.supports_action(Action::Click));
        assert!(editor.supports_action(Action::Focus));
    }
}
