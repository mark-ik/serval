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

/// The screen-reader request vocabulary, re-exported from the neutral seam.
///
/// It lives there rather than here because it never named winit or AccessKit:
/// an action and a DOM node are the same two things whichever platform asked.
/// Re-exported so callers that already import it from this crate keep working.
pub use cambium_genet_host::{A11yAction, A11yRequest, Accessibility};

impl Accessibility for A11yHost {
    fn sync(
        &mut self,
        dom: &ScriptedDom,
        layout: &IncrementalLayout<NodeId>,
        leaves: &mut LeafRegistry<u64>,
        focus: Option<u64>,
    ) -> Vec<A11yRequest> {
        self.sync_inner(dom, layout, leaves, focus)
    }
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
        layout: &IncrementalLayout<NodeId>,
        leaves: &mut LeafRegistry<u64>,
        focus: Option<u64>,
    ) -> Vec<A11yRequest> {
        let Some(window) = self.window.clone() else {
            // No window yet: nothing to install against, and no reader to ask.
            return Vec::new();
        };
        let (tree, action_map) = project_tree(dom, layout, leaves, focus);
        self.action_map = action_map;
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
