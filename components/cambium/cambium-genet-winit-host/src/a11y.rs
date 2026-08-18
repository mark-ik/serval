/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Incumbent layout projection kept beside the incumbent Cambium host.

use std::collections::HashMap;

use accesskit::{NodeId as A11yNodeId, Tree, TreeId, TreeUpdate};
use genet_layout::{IncrementalLayout, LeafA11ySource, project};
use genet_scripted_dom::{NodeId, ScriptedDom};
use layout_dom_api::LayoutDom as _;
use sprigging::LeafRegistry;

struct SpriggingA11y<'a>(&'a mut LeafRegistry<u64>);

impl LeafA11ySource for SpriggingA11y<'_> {
    fn describe_leaf(&mut self, key: u64, node: &mut accesskit::Node) {
        if let Some(leaf) = self.0.get_mut(&key) {
            leaf.accessibility(node);
        }
    }
}

pub(crate) fn project_tree(
    dom: &ScriptedDom,
    layout: &IncrementalLayout<NodeId>,
    leaves: &mut LeafRegistry<u64>,
    focus: Option<u64>,
) -> (TreeUpdate, HashMap<A11yNodeId, NodeId>) {
    let root = dom.document();
    let id_of = |d: &ScriptedDom, n: NodeId| A11yNodeId(d.opaque_id(n));
    let skip = |_: &ScriptedDom, _: NodeId| false;
    let projection = {
        let mut source = SpriggingA11y(leaves);
        project(
            dom,
            layout.fragments(),
            root,
            &id_of,
            &skip,
            &mut source,
            true,
        )
    };

    let mut nodes = Vec::with_capacity(projection.nodes.len());
    let mut action_map = HashMap::with_capacity(projection.nodes.len());
    for projected in projection.nodes {
        action_map.insert(projected.id, projected.dom);
        nodes.push((projected.id, projected.node));
    }
    let focus = focus
        .map(A11yNodeId)
        .filter(|id| action_map.contains_key(id))
        .unwrap_or(projection.root);
    let tree = TreeUpdate {
        nodes,
        tree: Some(Tree::new(projection.root)),
        tree_id: TreeId::ROOT,
        focus,
    };
    (tree, action_map)
}
