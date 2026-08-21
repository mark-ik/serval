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
