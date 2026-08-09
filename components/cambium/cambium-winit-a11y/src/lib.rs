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
//! genet-layout, and paints its custom visuals with Sprigging leaves. This
//! module turns that into a live accessibility tree for the OS screen reader,
//! so no app has to hand-roll the wiring:
//!
//! - [`SpriggingA11y`] lets the layout walk ask each `<custom-leaf>` for its own
//!   semantics ([`sprigging::Leaf::accessibility`]) — the mirror of the paint
//!   registry, for meaning rather than pixels.
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

use accesskit::{Action, NodeId as A11yNodeId, Tree, TreeId, TreeUpdate};
use genet_layout::{IncrementalLayout, LeafA11ySource, project};
use genet_scripted_dom::{NodeId, ScriptedDom};
use genet_winit_host::{AccessKitBridge, BridgeStatus};
use layout_dom_api::LayoutDom as _;
use sprigging::LeafRegistry;
use winit::window::Window;

/// What a screen reader asked for, kept as the action it actually requested.
///
/// AccessKit's `Click` and `Focus` are different requests and a host that
/// collapses them lies to the reader: navigating a list with a virtual cursor
/// issues `Focus`, and turning that into a click activates every control the
/// reader merely moves across. They stay apart here so the host can route
/// `Click` through its activation path and `Focus` through `set_focus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum A11yAction {
    /// Activate the element — the reader's equivalent of a pointer click.
    Click,
    /// Move focus to the element, without activating it.
    Focus,
}

/// One drained screen-reader request: which action, on which DOM node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct A11yRequest {
    pub action: A11yAction,
    pub node: NodeId,
}

/// Bridges genet-layout's a11y walk to a Sprigging leaf registry: when the walk
/// reaches a `<custom-leaf>`, the registered leaf fills its own AccessKit node
/// (a knob announces as a slider, a fretboard as a graphic). Mirrors the paint
/// registry's role, for semantics.
pub struct SpriggingA11y<'a>(pub &'a mut LeafRegistry<u64>);

impl LeafA11ySource for SpriggingA11y<'_> {
    fn describe_leaf(&mut self, key: u64, node: &mut accesskit::Node) {
        if let Some(leaf) = self.0.get_mut(&key) {
            leaf.accessibility(node);
        }
    }
}

/// Owns the OS AccessKit adapter and the per-frame tree lifecycle for a
/// genet-backed Cambium app. Create it in `resumed` (with a wake callback that
/// nudges the event loop), then call [`A11yHost::sync`] after every frame.
pub struct A11yHost {
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
    pub fn sync(
        &mut self,
        window: &Window,
        dom: &ScriptedDom,
        layout: &IncrementalLayout<NodeId>,
        leaves: &mut LeafRegistry<u64>,
        focus: Option<u64>,
    ) -> Vec<A11yRequest> {
        let (tree, action_map) = project_tree(dom, layout, leaves, focus);
        self.action_map = action_map;
        let node_count = tree.nodes.len();

        if !self.installed {
            match self.bridge.install(window, tree) {
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
    for p in projection.nodes {
        action_map.insert(p.id, p.dom);
        nodes.push((p.id, p.node));
    }
    // A stale focus id would point the reader at nothing, so it only stands
    // when the node is really in this frame's tree.
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
