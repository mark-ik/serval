/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Accessibility adapter lifecycle for a Cambium app.
//!
//! [`A11yHost`] owns the platform adapter and per-frame lifecycle: install a
//! caller-projected AccessKit tree the first frame, update it after, and drain a screen reader's
//!   actions, handing back the DOM nodes to activate so the app routes them
//!   through the same click path a mouse uses.
//!
//! The app keeps only what is app-specific: create the window hidden (the
//! adapter must attach before it is shown), drive the first frame synchronously
//! (a hidden window may not receive a deferred redraw), turn the wake callback
//! into a redraw, and dispatch the returned nodes.

use std::collections::HashMap;

use accesskit::{Action, NodeId as A11yNodeId, TreeUpdate};
use genet_scripted_dom::NodeId;
use genet_winit_host::{AccessKitBridge, BridgeStatus};
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

    /// Install the caller-projected AccessKit tree on the first call, revealing
    /// `window`, which must
    /// have been created hidden so the adapter attaches first — and update it
    /// after. Returns the screen reader's Click and Focus requests, in request
    /// order and still typed, for the caller to route each through the matching
    /// path (activation for `Click`, focus for `Focus`).
    ///
    pub fn sync(
        &mut self,
        window: &Window,
        tree: TreeUpdate,
        action_map: HashMap<A11yNodeId, NodeId>,
    ) -> Vec<A11yRequest> {
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
