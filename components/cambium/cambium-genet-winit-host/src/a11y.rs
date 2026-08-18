/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Incumbent layout projection kept beside the incumbent Cambium host.

use std::collections::HashMap;

use accesskit::{NodeId as A11yNodeId, TreeUpdate};
use genet_scripted_dom::{NodeId, ScriptedDom};
use layout_dom_api::{LayoutDom as _, LocalName, Namespace, NodeKind};
use sprigging::LeafRegistry;

pub(crate) fn project_tree(
    dom: &ScriptedDom,
    layout: &crate::owned_layout::OwnedLayout,
    leaves: &mut LeafRegistry<u64>,
    focus: Option<u64>,
) -> (TreeUpdate, HashMap<A11yNodeId, NodeId>) {
    let root = dom.document();
    let id_of = |d: &ScriptedDom, n: NodeId| A11yNodeId(d.opaque_id(n));
    let focused = focus.and_then(|opaque| find_opaque(dom, root, opaque));
    let mut tree = genet_render::accesskit_tree(dom, layout.fragments(), focused);
    let mut action_map = HashMap::new();
    walk(dom, root, &mut |node| {
        let id = id_of(dom, node);
        action_map.insert(id, node);
        if let Some(key) = custom_leaf_key(dom, node)
            && let Some(leaf) = leaves.get_mut(&key)
            && let Some((_, access)) = tree
                .nodes
                .iter_mut()
                .find(|(candidate, _)| *candidate == id)
        {
            leaf.accessibility(access);
        }
    });
    (tree, action_map)
}

fn walk(dom: &ScriptedDom, node: NodeId, visit: &mut impl FnMut(NodeId)) {
    visit(node);
    for child in dom.dom_children(node) {
        walk(dom, child, visit);
    }
}

fn find_opaque(dom: &ScriptedDom, node: NodeId, opaque: u64) -> Option<NodeId> {
    if dom.opaque_id(node) == opaque {
        return Some(node);
    }
    dom.dom_children(node)
        .find_map(|child| find_opaque(dom, child, opaque))
}

fn custom_leaf_key(dom: &ScriptedDom, node: NodeId) -> Option<u64> {
    if dom.kind(node) != NodeKind::Element
        || !matches!(
            dom.element_name(node)?.local.as_ref(),
            "custom-leaf" | "chisel-leaf"
        )
    {
        return None;
    }
    dom.attribute(node, &Namespace::default(), &LocalName::from("key"))?
        .parse()
        .ok()
}
