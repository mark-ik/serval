/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Accessibility host for a genet-backed Cambium app.
//!
//! Split out of `cambium-winit` on 2026-07-26, code unchanged. It lives apart
//! for one reason: it needs the laid-out genet DOM and the platform adapter,
//! which cannot be published, and holding them here leaves `cambium-winit` with
//! only `cambium` and `winit` so its key translation is reachable from a
//! registry consumer. A host that wants both takes both crates.
//!
//! Every Cambium app emits a semantic, ARIA-attributed DOM laid out by
//! Livery/Buckram, and paints its custom visuals with Sprigging leaves. This
//! module turns that into a live accessibility tree for the OS screen reader,
//! so no app has to hand-roll the wiring:
//!
//! - [`A11yHost`] owns the platform adapter and the per-frame lifecycle: project
//!   the retained layout into an AccessKit tree (leaf semantics included),
//!   install it the first frame and update it after, and drain a screen reader's
//!   actions, handing back the DOM nodes to activate so the app routes them
//!   through the same click path a mouse uses.
//!
//! The app keeps only what is app-specific: create the window hidden (the
//! adapter must attach before it is shown), drive the first frame synchronously
//! (a hidden window may not receive a deferred redraw), turn the wake callback
//! into a redraw, and dispatch the returned nodes.

use std::collections::HashMap;

use accesskit::{Action, Affine, NodeId as A11yNodeId, TreeUpdate};
use genet_scripted_dom::{NodeId, ScriptedDom};
use genet_winit_host::{AccessKitBridge, BridgeStatus};
use layout_dom_api::{LayoutDom as _, LocalName, Namespace, NodeKind};
use sprigging::LeafRegistry;
use winit::window::Window;

/// The screen-reader request vocabulary, re-exported from the neutral seam.
///
/// It lives there rather than here because it never named winit or AccessKit:
/// an action and a DOM node are the same two things whichever platform asked.
/// Re-exported so callers that already import it from this crate keep working.
pub use cambium_rootstock::{A11yAction, A11yRequest, Accessibility};

impl Accessibility for A11yHost {
    fn sync(
        &mut self,
        dom: &ScriptedDom,
        layout: &cambium_rootstock::OwnedLayout,
        leaves: &mut LeafRegistry<u64>,
        focus: Option<u64>,
    ) -> Vec<A11yRequest> {
        self.sync_inner(dom, layout, leaves, focus)
    }
}

/// Owns the OS AccessKit adapter and the per-frame tree lifecycle for a
/// genet-backed Cambium app. Create it in `resumed` (with a wake callback that
/// nudges the event loop), then call [`A11yHost::sync`] after every frame.
pub struct A11yHost {
    /// The native handle AccessKit binds its parallel tree to. Held rather than
    /// passed per call: it is what makes this implementation winit's, and the
    /// neutral seam has no room for it.
    ///
    /// Optional so the host is constructible without one. A window cannot be
    /// made in a unit test, and requiring one here would put `map_request` and
    /// the projection out of reach of exactly the receipts that should cover
    /// them. Until [`attach`](Self::attach), `sync` installs nothing and
    /// returns no requests, which is the honest answer for a tree no reader can
    /// see yet.
    window: Option<std::sync::Arc<Window>>,
    bridge: AccessKitBridge,
    installed: bool,
    /// AccessKit node id -> its DOM node, rebuilt each frame, so a screen
    /// reader's action on a node routes back to the element it came from.
    action_map: HashMap<A11yNodeId, NodeId>,
}

impl A11yHost {
    /// Create the adapter. `wake` is called by the adapter when a screen reader
    /// acts while the app is idle; wire it to request a redraw so the queued
    /// action gets drained (e.g. set a flag honored in `about_to_wait`).
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            window: None,
            bridge: AccessKitBridge::new(wake),
            installed: false,
            action_map: HashMap::new(),
        }
    }

    /// Whether the platform adapter is live.
    pub fn status(&self) -> BridgeStatus {
        self.bridge.status()
    }

    /// Project the current layout into an AccessKit tree (with each leaf's own
    /// semantics), install it on the first call — revealing `window`, which must
    /// have been created hidden so the adapter attaches first — and update it
    /// after. Returns the screen reader's Click and Focus requests, in request
    /// order and still typed, for the caller to route each through the matching
    /// path (activation for `Click`, focus for `Focus`).
    ///
    /// `focus` is the app's currently-focused DOM node's opaque id (from
    /// `LayoutDom::opaque_id`), used as the tree's focus when it is really in the
    /// tree, so a stale id never points the reader at nothing.
    fn sync_inner(
        &mut self,
        dom: &ScriptedDom,
        layout: &cambium_rootstock::OwnedLayout,
        leaves: &mut LeafRegistry<u64>,
        focus: Option<u64>,
    ) -> Vec<A11yRequest> {
        let Some(window) = self.window.clone() else {
            // No window yet: nothing to install against, and no reader to ask.
            return Vec::new();
        };
        let (mut tree, action_map) = project_tree(dom, layout, leaves, focus);
        self.action_map = action_map;
        scale_tree_to_window(&mut tree, dom, window.scale_factor());
        let node_count = tree.nodes.len();

        if !self.installed {
            match self.bridge.install(&window, tree) {
                Ok(()) => eprintln!(
                    "[cambium-winit] accessibility {:?}, {node_count} nodes projected",
                    self.bridge.status()
                ),
                Err(e) => eprintln!("[cambium-winit] accessibility install failed: {e}"),
            }
            self.installed = true;
            window.set_visible(true);
            return Vec::new();
        }

        self.bridge.update(tree);
        // Route a screen reader's requests back to their DOM nodes, each still
        // carrying the action it asked for.
        self.bridge
            .drain_actions()
            .into_iter()
            .filter_map(|req| {
                let action = match req.action {
                    Action::Click => A11yAction::Click,
                    Action::Focus => A11yAction::Focus,
                    _ => return None,
                };
                let node = self.action_map.get(&req.target_node).copied()?;
                Some(A11yRequest { action, node })
            })
            .collect()
    }

    /// Give the host the window AccessKit installs against.
    ///
    /// Call once, as soon as the window exists. Before this the host projects
    /// nothing; after it, the first [`sync`](Accessibility::sync) installs the
    /// tree and reveals the window.
    pub fn attach(&mut self, window: std::sync::Arc<Window>) {
        self.window = Some(window);
    }

    /// Map a raw AccessKit request to a typed one against the tree that was
    /// last synced. `None` for an action this host does not route, or a target
    /// that is no longer in the tree.
    ///
    /// Public because it is the seam between "the OS asked for something" and
    /// "the app does it": a test can feed a request the same way the adapter
    /// does, without a screen reader.
    pub fn map_request(&self, request: &accesskit::ActionRequest) -> Option<A11yRequest> {
        let action = match request.action {
            Action::Click => A11yAction::Click,
            Action::Focus => A11yAction::Focus,
            _ => return None,
        };
        let node = self.action_map.get(&request.target_node).copied()?;
        Some(A11yRequest { action, node })
    }
}

/// Project a laid-out Cambium document into an AccessKit tree, with no window
/// and no platform adapter: the half of [`A11yHost::sync`] that is pure.
///
/// Returns the tree update and the AccessKit-id → DOM-node map a drained
/// action is resolved through. Split out so the projection is assertable in an
/// ordinary test — an accessibility regression that only a screen reader can
/// catch is one nobody catches.
pub fn project_tree(
    dom: &ScriptedDom,
    layout: &cambium_rootstock::OwnedLayout,
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

/// Stamp the window's DPI scale on the tree root.
///
/// Layout bounds are logical CSS pixels, and AccessKit expects the final
/// transformed coordinates to be the platform's physical client pixels. The
/// Windows adapter applies no DPI conversion of its own, so without this a
/// screen reader or UI Automation client at 125% is told every control sits
/// at four fifths of its true position.
pub fn scale_tree_to_window(tree: &mut TreeUpdate, dom: &ScriptedDom, scale_factor: f64) {
    let root = A11yNodeId(dom.opaque_id(dom.document()));
    if let Some((_, node)) = tree.nodes.iter_mut().find(|(id, _)| *id == root) {
        node.set_transform(Affine::scale(scale_factor));
    }
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

#[cfg(test)]
mod dpi_tests {
    use super::*;
    use accesskit::{Node, Role, Tree, TreeId};

    /// A 125% window must report physical coordinates to the platform: the
    /// logical layout bounds ride a root transform that scales them.
    #[test]
    fn tree_root_carries_the_window_scale() {
        let dom = ScriptedDom::new();
        let root = A11yNodeId(dom.opaque_id(dom.document()));
        let mut tree = TreeUpdate {
            nodes: vec![(root, Node::new(Role::Window))],
            tree: Some(Tree::new(root)),
            tree_id: TreeId::ROOT,
            focus: root,
        };
        scale_tree_to_window(&mut tree, &dom, 1.25);
        let (_, node) = tree.nodes.iter().find(|(id, _)| *id == root).unwrap();
        assert_eq!(node.transform(), Some(&Affine::scale(1.25)));
    }
}
