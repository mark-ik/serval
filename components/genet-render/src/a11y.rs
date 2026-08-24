/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::hash::Hash;

use accesskit::{Node as AccessNode, NodeId as AccessNodeId, Rect, Role, Tree, TreeId, TreeUpdate};
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
            "main" => return Role::Main,
            "navigation" => return Role::Navigation,
            "heading" => return Role::Heading,
            "alert" => return Role::Alert,
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
    let mut access = AccessNode::new(role_for(dom, node));
    let label = dom
        .attribute(node, &Namespace::default(), &LocalName::from("aria-label"))
        .map(str::to_owned)
        .unwrap_or_else(|| direct_text(dom, node));
    if !label.is_empty() {
        access.set_label(label);
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
    use accesskit::{Node as AccessNode, Role};
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
}
