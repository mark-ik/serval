//! Pelt's composite accessibility projection.
//!
//! The workspace owns one AccessKit tree. Chrome nodes are authored here;
//! each focused document lane contributes a namespaced child subtree through
//! its content aperture, so a typed action routes to exactly one tile.

use super::*;

// ---------------------------------------------------------------------
// The accessibility vocabulary: the focus and action-target model, the
// per-lane action and child records, the namespaced document sessions,
// and the composite projection that owns them. Moved here from the
// parent on 2026-09-01; `pub(super)` is the ceiling because nothing
// outside `workspace_viewer` names any of it.
// ---------------------------------------------------------------------

/// One stable virtual focus target in the composite workspace tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum WorkspaceA11yFocus {
    Frisket(FrisketA11yTarget),
    /// A virtual focus target in an engine-owned child subtree. The child
    /// engine remains responsible for its semantics; Pelt owns only its
    /// one-tree namespace and host focus state.
    Document(AccessNodeId),
}

impl WorkspaceApp {
    fn document_action_is_current(
        &self,
        target: DocumentA11yActionTarget,
        action: DocumentA11yAction,
    ) -> bool {
        let Some(controller) = self.workspace.controller(target.tile) else {
            return false;
        };
        if self.workspace.document_session_identity(target.tile) != Some(target.session_identity)
            || self.workspace.content_rect(target.tile) != Some(target.content_rect)
            || !target.supports(action)
        {
            return false;
        }
        let Some(projection) = controller.accessibility_projection() else {
            return false;
        };
        projection.revision() == target.revision
            && projection.node(target.local_node).is_some_and(|node| {
                node.actions.contains(&action) && !node.state.disabled && !node.state.hidden
            })
    }

    fn current_document_click_point(&self, target: DocumentA11yActionTarget) -> Option<(f32, f32)> {
        if !self.document_action_is_current(target, DocumentA11yAction::Click) {
            return None;
        }
        let controller = self.workspace.controller(target.tile)?;
        let point = controller.accessibility_click_target(target.local_node)?;
        if point.revision != target.revision {
            return None;
        }
        let point = (
            target.content_rect.x + point.point.x,
            target.content_rect.y + point.point.y,
        );
        target
            .content_rect
            .contains(point.0, point.1)
            .then_some(point)
    }

    fn dispatch_document_action(
        &mut self,
        target: DocumentA11yActionTarget,
        action: DocumentA11yAction,
        data: Option<DocumentA11yActionData>,
    ) -> bool {
        if !self.document_action_is_current(target, action) {
            return false;
        }
        let Some(controller) = self.workspace.controller_mut(target.tile) else {
            return false;
        };
        controller.dispatch_accessibility_action(&DocumentA11yActionRequest {
            revision: target.revision,
            target: target.local_node,
            action,
            data,
        })
    }

    pub(in crate::workspace_viewer) fn apply_accessibility_request(
        &mut self,
        request: A11yActionRequest,
    ) -> bool {
        let Some(target) = self.accessibility.action_for(request.target_node) else {
            return false;
        };
        match (request.action, target) {
            (Action::Focus, WorkspaceA11yActionTarget::Frisket(node)) => self
                .frisket
                .accessibility_target(node)
                .is_some_and(|target| {
                    self.accessibility
                        .set_focus(WorkspaceA11yFocus::Frisket(target))
                }),
            (Action::Click, WorkspaceA11yActionTarget::Frisket(node)) => match self
                .frisket
                .accessibility_target(node)
            {
                Some(FrisketA11yTarget::ChromeAction(action)) => self.apply_chrome_action(action),
                Some(FrisketA11yTarget::Close(tile)) => {
                    self.clear_chrome_address();
                    self.clear_chrome_engine_menu();
                    self.clear_chrome_appearance();
                    self.apply_tile_event(TileEvent::Closed(tile), None)
                },
                Some(FrisketA11yTarget::Tab(tile)) => {
                    self.clear_chrome_address();
                    self.clear_chrome_engine_menu();
                    self.clear_chrome_appearance();
                    self.apply_tile_event(TileEvent::Activated(tile), None)
                },
                None => false,
            },
            (Action::SetValue, WorkspaceA11yActionTarget::Frisket(node)) => {
                if self.frisket.accessibility_target(node)
                    != Some(FrisketA11yTarget::ChromeAction(ChromeAction::Address))
                {
                    return false;
                }
                let Some(ActionData::Value(value)) = request.data else {
                    return false;
                };
                if !self.apply_chrome_action(ChromeAction::Address) {
                    return false;
                }
                let Some(input) = self.chrome_address.as_mut() else {
                    return false;
                };
                input.value = value.into();
                input.replace_on_insert = false;
                self.submit_chrome_address()
            },
            (Action::Focus, WorkspaceA11yActionTarget::Document(target)) => self
                .document_action_is_current(target, DocumentA11yAction::Focus)
                .then(|| {
                    self.accessibility
                        .set_focus(WorkspaceA11yFocus::Document(request.target_node))
                })
                .unwrap_or(false),
            (Action::Click, WorkspaceA11yActionTarget::Document(target)) => {
                if self.workspace.has_active_pointer_capture() {
                    return false;
                }
                let Some((x, y)) = self.current_document_click_point(target) else {
                    return false;
                };
                self.route_accessibility_pointer_click(
                    target.tile,
                    target.session_identity,
                    target.content_rect,
                    x,
                    y,
                )
            },
            (Action::SetValue, WorkspaceA11yActionTarget::Document(target)) => {
                let Some(ActionData::Value(value)) = request.data else {
                    return false;
                };
                self.dispatch_document_action(
                    target,
                    DocumentA11yAction::SetValue,
                    Some(DocumentA11yActionData::Value(value.into())),
                )
            },
            (Action::ScrollIntoView, WorkspaceA11yActionTarget::Document(target)) => {
                self.dispatch_document_action(target, DocumentA11yAction::ScrollIntoView, None)
            },
            (Action::Increment, WorkspaceA11yActionTarget::Document(target)) => {
                self.dispatch_document_action(target, DocumentA11yAction::Increment, None)
            },
            (Action::Decrement, WorkspaceA11yActionTarget::Document(target)) => {
                self.dispatch_document_action(target, DocumentA11yAction::Decrement, None)
            },
            _ => false,
        }
    }
}

/// A Pelt-owned action target for one node in the composite tree.
#[derive(Clone, Debug)]
pub(super) enum WorkspaceA11yActionTarget {
    Frisket(genet_scripted_dom::NodeId),
    Document(DocumentA11yActionTarget),
}

impl WorkspaceA11yActionTarget {
    fn is_document(&self) -> bool {
        match self {
            Self::Frisket(_) => false,
            Self::Document(_) => true,
        }
    }
}

/// The session identity and action snapshot for a renderer-neutral document
/// semantic node. The document owns semantic truth; Pelt owns namespacing,
/// tile/session validation, and its ordinary pointer route.
#[derive(Clone, Copy, Debug)]
pub(super) struct DocumentA11yActionTarget {
    pub(super) tile: TileId,
    pub(super) session_identity: PeltSessionIdentity,
    pub(super) revision: u64,
    pub(super) local_node: DocumentA11yNodeId,
    pub(super) content_rect: WorkspaceRect,
    pub(super) actions: u8,
}

impl DocumentA11yActionTarget {
    const CLICK: u8 = 1 << 0;
    const FOCUS: u8 = 1 << 1;
    const SET_VALUE: u8 = 1 << 2;
    const SCROLL_INTO_VIEW: u8 = 1 << 3;
    const INCREMENT: u8 = 1 << 4;
    const DECREMENT: u8 = 1 << 5;

    fn from_actions(actions: &[DocumentA11yAction]) -> u8 {
        actions.iter().fold(0, |bits, action| {
            bits | match action {
                DocumentA11yAction::Click => Self::CLICK,
                DocumentA11yAction::Focus => Self::FOCUS,
                DocumentA11yAction::SetValue => Self::SET_VALUE,
                DocumentA11yAction::ScrollIntoView => Self::SCROLL_INTO_VIEW,
                DocumentA11yAction::Increment => Self::INCREMENT,
                DocumentA11yAction::Decrement => Self::DECREMENT,
            }
        })
    }

    pub(super) fn supports(self, action: DocumentA11yAction) -> bool {
        let bit = match action {
            DocumentA11yAction::Click => Self::CLICK,
            DocumentA11yAction::Focus => Self::FOCUS,
            DocumentA11yAction::SetValue => Self::SET_VALUE,
            DocumentA11yAction::ScrollIntoView => Self::SCROLL_INTO_VIEW,
            DocumentA11yAction::Increment => Self::INCREMENT,
            DocumentA11yAction::Decrement => Self::DECREMENT,
        };
        self.actions & bit != 0
    }
}

/// One tile-local namespace inside Pelt's root AccessKit tree.
///
/// AccessKit node IDs are only local-tree unique. Pelt owns these assignments
/// so independently constructed `ScriptedDom`s cannot collide when their
/// retained trees become siblings below Frisket's content apertures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct DocumentA11ySession {
    pub(super) identity: PeltSessionIdentity,
}

pub(super) struct DocumentA11yNamespace {
    pub(super) session_identity: PeltSessionIdentity,
    pub(super) global_ids: HashMap<AccessNodeId, AccessNodeId>,
}

impl DocumentA11yNamespace {
    fn new(session_identity: PeltSessionIdentity) -> Self {
        Self {
            session_identity,
            global_ids: HashMap::new(),
        }
    }
}

/// The tree and action map delivered together to the one-tree platform bridge.
pub(super) struct WorkspaceA11yProjection {
    pub(super) tree: TreeUpdate,
    pub(super) root: AccessNodeId,
    pub(super) actions: HashMap<AccessNodeId, WorkspaceA11yActionTarget>,
}

/// A completed renderer-neutral document projection ready to attach below one
/// Frisket content aperture.
pub(super) struct DocumentA11yChild {
    pub(super) tile: TileId,
    pub(super) session_identity: PeltSessionIdentity,
    pub(super) aperture: AccessNodeId,
    pub(super) projection: DocumentA11yProjection,
    pub(super) content_rect: WorkspaceRect,
}

/// Per-window platform bridge and retained composite action map.
pub(super) struct WorkspaceAccessibility {
    pub(super) bridge: AccessKitBridge,
    pub(super) window_revealed: bool,
    pub(super) last_install_error: Option<String>,
    pub(super) action_map: HashMap<AccessNodeId, WorkspaceA11yActionTarget>,
    pub(super) focus: Option<WorkspaceA11yFocus>,
    pub(super) child_namespaces: HashMap<TileId, DocumentA11yNamespace>,
    pub(super) assigned_child_ids: HashSet<AccessNodeId>,
    pub(super) next_child_node_id: u64,
    pub(super) wake: Arc<AtomicBool>,
}

impl WorkspaceAccessibility {
    pub(super) fn new() -> Self {
        let wake = Arc::new(AtomicBool::new(false));
        let requested = Arc::clone(&wake);
        Self {
            bridge: AccessKitBridge::new(move || {
                requested.store(true, Ordering::Release);
            }),
            window_revealed: false,
            last_install_error: None,
            action_map: HashMap::new(),
            focus: None,
            child_namespaces: HashMap::new(),
            assigned_child_ids: HashSet::new(),
            // Keep Pelt-owned IDs in a distinct range while still checking
            // every shell ID. The allocator never recycles an issued child ID,
            // which makes a stale platform action inert rather than aliased.
            next_child_node_id: 1_u64 << 63,
            wake,
        }
    }

    pub(super) fn prepare(
        &mut self,
        projection: WorkspaceA11yProjection,
        scale_factor: f64,
    ) -> TreeUpdate {
        self.action_map = projection.actions;
        let mut tree = projection.tree;
        if let Some(WorkspaceA11yFocus::Document(id)) = self.focus.as_ref()
            && self
                .action_map
                .get(id)
                .is_some_and(WorkspaceA11yActionTarget::is_document)
        {
            tree.focus = *id;
        }
        if let Some((_, root)) = tree.nodes.iter_mut().find(|(id, _)| *id == projection.root) {
            // Livery lays the retained shell out in logical CSS pixels. The
            // platform tree needs physical client coordinates, matching the
            // raster and pointer conversion paths below.
            root.set_transform(Affine::scale(scale_factor));
        }
        tree
    }

    pub(super) fn sync(
        &mut self,
        window: &Window,
        projection: WorkspaceA11yProjection,
        scale_factor: f64,
    ) -> Vec<A11yActionRequest> {
        let node_count = projection.tree.nodes.len();
        let tree = self.prepare(projection, scale_factor);
        if self.bridge.status() != BridgeStatus::Installed {
            match self.bridge.install(window, tree) {
                Ok(()) => {
                    self.last_install_error = None;
                    eprintln!(
                        "[pelt] accessibility {:?}, {node_count} retained workspace nodes projected",
                        self.bridge.status()
                    );
                },
                Err(error) => {
                    if self.last_install_error.as_deref() != Some(error.as_str()) {
                        eprintln!("[pelt] accessibility install failed: {error}");
                    }
                    self.last_install_error = Some(error);
                },
            }
            // The Windows adapter has to attach before the first visible frame.
            // An initial failure still leaves ordinary Pelt usable and is tried
            // again on a later redraw instead of permanently disabling a11y.
            if !self.window_revealed {
                window.set_visible(true);
                self.window_revealed = true;
            }
            return self.bridge.drain_actions();
        }
        self.bridge.update(tree);
        self.bridge.drain_actions()
    }

    pub(super) fn action_for(&self, id: AccessNodeId) -> Option<WorkspaceA11yActionTarget> {
        self.action_map.get(&id).cloned()
    }

    pub(super) fn set_focus(&mut self, target: WorkspaceA11yFocus) -> bool {
        if self.focus.as_ref() == Some(&target) {
            return false;
        }
        self.focus = Some(target);
        true
    }

    fn frisket_focus(&self) -> Option<&FrisketA11yTarget> {
        match self.focus.as_ref() {
            Some(WorkspaceA11yFocus::Frisket(target)) => Some(target),
            Some(WorkspaceA11yFocus::Document(_)) | None => None,
        }
    }

    fn retain_document_namespaces(&mut self, live_sessions: &HashMap<TileId, DocumentA11ySession>) {
        self.child_namespaces.retain(|tile, namespace| {
            live_sessions.get(tile)
                == Some(&DocumentA11ySession {
                    identity: namespace.session_identity,
                })
        });
    }

    pub(super) fn child_global_id(
        &mut self,
        tile: TileId,
        session_identity: PeltSessionIdentity,
        local_id: AccessNodeId,
        shell_ids: &HashSet<AccessNodeId>,
    ) -> AccessNodeId {
        let reset_namespace = self.child_namespaces.get(&tile).is_some_and(|namespace| {
            namespace.session_identity != session_identity
                || namespace
                    .global_ids
                    .values()
                    .any(|id| shell_ids.contains(id))
        });
        if reset_namespace {
            self.child_namespaces.remove(&tile);
        }
        if let Some(id) = self
            .child_namespaces
            .get(&tile)
            .and_then(|namespace| namespace.global_ids.get(&local_id))
        {
            return *id;
        }

        let global_id = self.allocate_child_id(shell_ids);
        let namespace = self
            .child_namespaces
            .entry(tile)
            .or_insert_with(|| DocumentA11yNamespace::new(session_identity));
        debug_assert_eq!(namespace.session_identity, session_identity);
        namespace.global_ids.insert(local_id, global_id);
        global_id
    }

    pub(super) fn allocate_child_id(&mut self, shell_ids: &HashSet<AccessNodeId>) -> AccessNodeId {
        loop {
            let candidate = AccessNodeId(self.next_child_node_id);
            self.next_child_node_id = self
                .next_child_node_id
                .checked_add(1)
                .expect("Pelt accessibility child node IDs exhausted");
            if !shell_ids.contains(&candidate) && self.assigned_child_ids.insert(candidate) {
                return candidate;
            }
        }
    }

    fn clear_stale_document_focus(
        &mut self,
        actions: &HashMap<AccessNodeId, WorkspaceA11yActionTarget>,
    ) {
        let Some(WorkspaceA11yFocus::Document(id)) = self.focus.as_ref() else {
            return;
        };
        if !actions
            .get(id)
            .is_some_and(WorkspaceA11yActionTarget::is_document)
        {
            self.focus = None;
        }
    }

    pub(super) fn child_id_is_reserved(&self, id: AccessNodeId) -> bool {
        self.assigned_child_ids.contains(&id)
    }

    pub(super) fn status(&self) -> BridgeStatus {
        self.bridge.status()
    }

    pub(super) fn update_window_focus(&mut self, focused: bool) {
        self.bridge.update_window_focus(focused);
    }

    pub(super) fn take_wake(&self) -> bool {
        self.wake.swap(false, Ordering::AcqRel)
    }
}

/// Append one engine-owned document projection beneath its Pelt content
/// aperture. This is shared by the primary composite workspace and accepted
/// tearout windows. The engine supplies semantic truth; each Pelt window owns
/// its own namespace, action snapshot, focus, and platform bridge.
fn append_document_child(
    accessibility: &mut WorkspaceAccessibility,
    tree: &mut TreeUpdate,
    actions: &mut HashMap<AccessNodeId, WorkspaceA11yActionTarget>,
    shell_ids: &HashSet<AccessNodeId>,
    child: DocumentA11yChild,
) {
    let DocumentA11yChild {
        tile,
        session_identity,
        aperture,
        projection,
        content_rect,
    } = child;
    let local_ids = projection
        .nodes()
        .iter()
        .map(|node| {
            let local = AccessNodeId(node.id.get());
            let global = accessibility.child_global_id(tile, session_identity, local, shell_ids);
            (node.id, global)
        })
        .collect::<HashMap<_, _>>();
    let Some(&child_root) = local_ids.get(&projection.root()) else {
        return;
    };
    let Some((_, aperture_node)) = tree.nodes.iter_mut().find(|(id, _)| *id == aperture) else {
        return;
    };
    aperture_node.set_children([child_root]);
    for semantic in projection.nodes() {
        let Some(&global_id) = local_ids.get(&semantic.id) else {
            continue;
        };
        let mut node = AccessNode::new(WorkspaceApp::accesskit_role(semantic.role));
        if let DocumentA11yRole::Heading { level } = semantic.role
            && level > 0
        {
            node.set_level(usize::from(level));
        }
        if let Some(name) = &semantic.name {
            node.set_label(name.clone());
        }
        if let Some(value) = &semantic.value {
            node.set_value(value.clone());
        }
        if semantic.state.disabled {
            node.set_disabled();
        }
        if semantic.state.hidden {
            node.set_hidden();
        }
        if semantic.state.read_only {
            node.set_read_only();
        }
        if semantic.state.required {
            node.set_required();
        }
        if let Some(selected) = semantic.state.selected {
            node.set_selected(selected);
        }
        if let Some(expanded) = semantic.state.expanded {
            node.set_expanded(expanded);
        }
        if let Some(toggled) = semantic.state.toggled {
            node.set_toggled(match toggled {
                inker::DocumentA11yToggled::On => Toggled::True,
                inker::DocumentA11yToggled::Off => Toggled::False,
                inker::DocumentA11yToggled::Mixed => Toggled::Mixed,
            });
        } else if let Some(checked) = semantic.state.checked {
            node.set_toggled(if checked {
                Toggled::True
            } else {
                Toggled::False
            });
        }
        if let Some(live) = semantic.state.live {
            node.set_live(match live {
                inker::DocumentA11yLive::Off => Live::Off,
                inker::DocumentA11yLive::Polite => Live::Polite,
                inker::DocumentA11yLive::Assertive => Live::Assertive,
            });
        }
        if let Some(orientation) = semantic.state.orientation {
            node.set_orientation(match orientation {
                inker::DocumentA11yOrientation::Horizontal => Orientation::Horizontal,
                inker::DocumentA11yOrientation::Vertical => Orientation::Vertical,
            });
        }
        if let Some(has_popup) = semantic.state.has_popup {
            node.set_has_popup(match has_popup {
                inker::DocumentA11yHasPopup::Menu => HasPopup::Menu,
                inker::DocumentA11yHasPopup::ListBox => HasPopup::Listbox,
                inker::DocumentA11yHasPopup::Tree => HasPopup::Tree,
                inker::DocumentA11yHasPopup::Grid => HasPopup::Grid,
                inker::DocumentA11yHasPopup::Dialog => HasPopup::Dialog,
            });
        }
        if let Some(value) = semantic.numeric_value {
            node.set_numeric_value(value);
        }
        if let Some(value) = semantic.numeric_minimum {
            node.set_min_numeric_value(value);
        }
        if let Some(value) = semantic.numeric_maximum {
            node.set_max_numeric_value(value);
        }
        if let Some(bounds) = semantic.bounds {
            node.set_bounds(AccessRect::new(
                f64::from(bounds.x),
                f64::from(bounds.y),
                f64::from(bounds.x + bounds.width),
                f64::from(bounds.y + bounds.height),
            ));
        }
        let descendants = semantic
            .children
            .iter()
            .filter_map(|child| local_ids.get(child).copied())
            .collect::<Vec<_>>();
        node.set_children(descendants);
        if semantic.id == projection.root() {
            node.set_transform(Affine::translate((
                f64::from(content_rect.x),
                f64::from(content_rect.y),
            )));
        }
        WorkspaceApp::accesskit_actions(&semantic.actions, &mut node);
        if semantic.state.focused {
            tree.focus = global_id;
        }
        let action_bits = DocumentA11yActionTarget::from_actions(&semantic.actions);
        if action_bits != 0 {
            actions.insert(
                global_id,
                WorkspaceA11yActionTarget::Document(DocumentA11yActionTarget {
                    tile,
                    session_identity,
                    revision: projection.revision(),
                    local_node: semantic.id,
                    content_rect,
                    actions: action_bits,
                }),
            );
        }
        tree.nodes.push((global_id, node));
    }
}

/// Build a composite tree for an already-laid-out tearout. The returned
/// action map is local to this window and cannot route into the primary lane.
pub(super) fn secondary_accessibility_projection(
    accessibility: &mut WorkspaceAccessibility,
    frisket: &FrisketSurface,
    workspace: &PeltWorkspace<Scene>,
) -> Result<WorkspaceA11yProjection, String> {
    let shell = frisket
        .accessibility_projection(accessibility.frisket_focus())
        .ok_or_else(|| "tearout Frisket has no completed retained layout".to_owned())?;
    let live_sessions = workspace
        .tree()
        .tiles()
        .into_iter()
        .filter_map(|tile| {
            let controller = workspace.controller(tile.id)?;
            Some((
                tile.id,
                DocumentA11ySession {
                    identity: controller.session_identity(),
                },
            ))
        })
        .collect::<HashMap<_, _>>();
    accessibility.retain_document_namespaces(&live_sessions);
    let shell_ids = shell
        .tree
        .nodes
        .iter()
        .map(|(id, _)| *id)
        .collect::<HashSet<_>>();
    let FrisketA11yProjection {
        mut tree,
        root,
        nodes,
        content_nodes,
    } = shell;
    let mut actions = nodes
        .into_iter()
        .filter(|(id, _)| !accessibility.child_id_is_reserved(*id))
        .map(|(id, node)| (id, WorkspaceA11yActionTarget::Frisket(node)))
        .collect::<HashMap<_, _>>();
    for (tile, aperture) in content_nodes {
        let Some(controller) = workspace.controller(tile) else {
            continue;
        };
        let Some(projection) = controller.accessibility_projection() else {
            continue;
        };
        let Some(content_rect) = workspace.content_rect(tile) else {
            continue;
        };
        append_document_child(
            accessibility,
            &mut tree,
            &mut actions,
            &shell_ids,
            DocumentA11yChild {
                tile,
                session_identity: controller.session_identity(),
                aperture,
                projection,
                content_rect,
            },
        );
    }
    accessibility.clear_stale_document_focus(&actions);
    Ok(WorkspaceA11yProjection {
        tree,
        root,
        actions,
    })
}

impl WorkspaceApp {
    pub(in crate::workspace_viewer) fn refresh_accessibility_content_regions(&mut self) {
        let regions = self
            .workspace
            .tree()
            .tiles()
            .into_iter()
            .map(|tile| {
                let description = match self
                    .workspace
                    .controller(tile.id)
                    .and_then(PeltController::accessibility_projection)
                {
                    Some(projection) => {
                        let support = projection.support();
                        let limitations = support.limitations().join(" ");
                        match support.capability() {
                            A11yCapability::Partial => format!(
                                "The engine composes partial accessibility into this workspace tree. {limitations}"
                            ),
                            A11yCapability::Full => {
                                "The engine composes its full semantic tree into this workspace tree."
                                    .to_owned()
                            },
                            A11yCapability::Opaque => {
                                "The engine supplied an opaque accessibility projection."
                                    .to_owned()
                            },
                        }
                    },
                    None => match self
                        .workspace
                        .controller(tile.id)
                        .map(PeltController::a11y_capability)
                    {
                    Some(A11yCapability::Opaque) => {
                        "The engine declares opaque accessibility. Pelt cannot inspect or compose this content's semantics.".to_owned()
                    },
                    Some(A11yCapability::Partial) => {
                        "The engine declares partial accessibility, but has not published a completed semantic projection.".to_owned()
                    },
                    Some(A11yCapability::Full) => {
                        "The engine declares a full semantic tree, but has not published a completed projection.".to_owned()
                    },
                    None => {
                        "Pelt has not received an accessibility declaration for this content.".to_owned()
                    },
                    },
                };
                FrisketContentA11y {
                    tile: tile.id,
                    label: format!("{} content", tile.title),
                    description,
                }
            })
            .collect::<Vec<_>>();
        self.frisket.set_content_accessibility(regions);
    }

    /// Refresh Frisket's retained geometry before projecting its shell and
    /// attached document subtrees. The visual renderer does this same first
    /// phase before it asks each document session for a frame.
    pub(in crate::workspace_viewer) fn layout_accessibility_shell(&mut self) -> Result<(), String> {
        self.refresh_chrome();
        self.refresh_accessibility_content_regions();
        let (width, height) = self.logical_size();
        let mut pane = self
            .frisket
            .frame(width, height)
            .map_err(|error| format!("could not lay out Frisket for accessibility: {error}"))?;
        self.workspace
            .set_content_rects(pane.content_rects.iter().copied());
        if self.config.chrome && self.chrome_model().diagnostic.is_some() {
            self.refresh_chrome();
            pane = self.frisket.frame(width, height).map_err(|error| {
                format!("could not lay out diagnostic Frisket for accessibility: {error}")
            })?;
            self.workspace
                .set_content_rects(pane.content_rects.iter().copied());
        }
        Ok(())
    }

    /// Gather every engine-owned semantic projection through Pelt core's
    /// neutral controller seam. A session that has not published a completed
    /// projection remains an honest Frisket aperture.
    pub(in crate::workspace_viewer) fn document_accessibility_children(
        &self,
        shell: &FrisketA11yProjection,
    ) -> (HashMap<TileId, DocumentA11ySession>, Vec<DocumentA11yChild>) {
        let live_sessions = self
            .workspace
            .tree()
            .tiles()
            .into_iter()
            .filter_map(|tile| {
                let controller = self.workspace.controller(tile.id)?;
                Some((
                    tile.id,
                    DocumentA11ySession {
                        identity: controller.session_identity(),
                    },
                ))
            })
            .collect::<HashMap<_, _>>();
        let children = shell
            .content_nodes
            .iter()
            .filter_map(|(&tile, &aperture)| {
                let controller = self.workspace.controller(tile)?;
                let projection = controller.accessibility_projection()?;
                let content_rect = self.workspace.content_rect(tile)?;
                Some(DocumentA11yChild {
                    tile,
                    session_identity: controller.session_identity(),
                    aperture,
                    projection,
                    content_rect,
                })
            })
            .collect();
        (live_sessions, children)
    }

    fn accesskit_role(role: DocumentA11yRole) -> Role {
        match role {
            DocumentA11yRole::Window => Role::Window,
            DocumentA11yRole::Document => Role::Document,
            DocumentA11yRole::Article => Role::Article,
            DocumentA11yRole::Region => Role::Region,
            DocumentA11yRole::Group => Role::Group,
            DocumentA11yRole::Navigation => Role::Navigation,
            DocumentA11yRole::Main => Role::Main,
            DocumentA11yRole::Heading { .. } => Role::Heading,
            DocumentA11yRole::Paragraph => Role::Paragraph,
            DocumentA11yRole::StaticText => Role::TextRun,
            DocumentA11yRole::Link => Role::Link,
            DocumentA11yRole::Button => Role::Button,
            DocumentA11yRole::TextField => Role::TextInput,
            DocumentA11yRole::CheckBox => Role::CheckBox,
            DocumentA11yRole::RadioButton => Role::RadioButton,
            DocumentA11yRole::RadioGroup => Role::RadioGroup,
            DocumentA11yRole::Switch => Role::Switch,
            DocumentA11yRole::ComboBox => Role::ComboBox,
            DocumentA11yRole::List => Role::List,
            DocumentA11yRole::ListItem => Role::ListItem,
            DocumentA11yRole::ListBox => Role::ListBox,
            DocumentA11yRole::ListBoxOption => Role::ListBoxOption,
            DocumentA11yRole::Table => Role::Table,
            DocumentA11yRole::Row => Role::Row,
            DocumentA11yRole::Cell => Role::Cell,
            DocumentA11yRole::Image => Role::Image,
            DocumentA11yRole::Form => Role::Form,
            DocumentA11yRole::Dialog => Role::Dialog,
            DocumentA11yRole::Alert => Role::Alert,
            DocumentA11yRole::Menu => Role::Menu,
            DocumentA11yRole::MenuItem => Role::MenuItem,
            DocumentA11yRole::MenuItemCheckBox => Role::MenuItemCheckBox,
            DocumentA11yRole::MenuItemRadio => Role::MenuItemRadio,
            DocumentA11yRole::TabList => Role::TabList,
            DocumentA11yRole::Tab => Role::Tab,
            DocumentA11yRole::TabPanel => Role::TabPanel,
            DocumentA11yRole::Tree => Role::Tree,
            DocumentA11yRole::TreeItem => Role::TreeItem,
            DocumentA11yRole::Slider => Role::Slider,
            DocumentA11yRole::SpinButton => Role::SpinButton,
            DocumentA11yRole::Splitter => Role::Splitter,
            DocumentA11yRole::Toolbar => Role::Toolbar,
            DocumentA11yRole::ProgressIndicator => Role::ProgressIndicator,
            DocumentA11yRole::Label => Role::Label,
            DocumentA11yRole::Status => Role::Status,
            DocumentA11yRole::Log => Role::Log,
            DocumentA11yRole::Note => Role::Note,
            DocumentA11yRole::Unknown => Role::GenericContainer,
        }
    }

    fn accesskit_actions(actions: &[DocumentA11yAction], node: &mut AccessNode) {
        for action in actions {
            node.add_action(match action {
                DocumentA11yAction::Click => Action::Click,
                DocumentA11yAction::Focus => Action::Focus,
                DocumentA11yAction::SetValue => Action::SetValue,
                DocumentA11yAction::ScrollIntoView => Action::ScrollIntoView,
                DocumentA11yAction::Increment => Action::Increment,
                DocumentA11yAction::Decrement => Action::Decrement,
            });
        }
    }

    /// Flatten every supported child projection into Pelt's one AccessKit
    /// tree. Namespaces and action targets are host-global; semantic roles,
    /// actions, coverage, and revisions remain engine-owned.
    pub(in crate::workspace_viewer) fn workspace_accessibility_projection(
        &mut self,
    ) -> Result<WorkspaceA11yProjection, String> {
        let frisket_focus = self.accessibility.frisket_focus().cloned();
        let shell = self
            .frisket
            .accessibility_projection(frisket_focus.as_ref())
            .ok_or_else(|| {
                "Frisket has no completed retained layout for accessibility".to_owned()
            })?;
        let (live_sessions, children) = self.document_accessibility_children(&shell);
        self.accessibility
            .retain_document_namespaces(&live_sessions);
        let shell_ids = shell
            .tree
            .nodes
            .iter()
            .map(|(id, _)| *id)
            .collect::<HashSet<_>>();
        let FrisketA11yProjection {
            mut tree,
            root,
            nodes,
            ..
        } = shell;
        let mut actions = nodes
            .into_iter()
            .filter(|(id, _)| !self.accessibility.child_id_is_reserved(*id))
            .map(|(id, node)| (id, WorkspaceA11yActionTarget::Frisket(node)))
            .collect::<HashMap<_, _>>();

        for child in children {
            let DocumentA11yChild {
                tile,
                session_identity,
                aperture,
                projection,
                content_rect,
            } = child;
            let local_ids = projection
                .nodes()
                .iter()
                .map(|node| {
                    let local = AccessNodeId(node.id.get());
                    let global = self.accessibility.child_global_id(
                        tile,
                        session_identity,
                        local,
                        &shell_ids,
                    );
                    (node.id, global)
                })
                .collect::<HashMap<_, _>>();
            let Some(&child_root) = local_ids.get(&projection.root()) else {
                continue;
            };
            let Some((_, aperture_node)) = tree.nodes.iter_mut().find(|(id, _)| *id == aperture)
            else {
                continue;
            };
            aperture_node.set_children([child_root]);
            for semantic in projection.nodes() {
                let Some(&global_id) = local_ids.get(&semantic.id) else {
                    continue;
                };
                let mut node = AccessNode::new(Self::accesskit_role(semantic.role));
                if let DocumentA11yRole::Heading { level } = semantic.role
                    && level > 0
                {
                    node.set_level(usize::from(level));
                }
                if let Some(name) = &semantic.name {
                    node.set_label(name.clone());
                }
                if let Some(value) = &semantic.value {
                    node.set_value(value.clone());
                }
                if semantic.state.disabled {
                    node.set_disabled();
                }
                if semantic.state.hidden {
                    node.set_hidden();
                }
                if semantic.state.read_only {
                    node.set_read_only();
                }
                if semantic.state.required {
                    node.set_required();
                }
                if let Some(selected) = semantic.state.selected {
                    node.set_selected(selected);
                }
                if let Some(expanded) = semantic.state.expanded {
                    node.set_expanded(expanded);
                }
                if let Some(toggled) = semantic.state.toggled {
                    node.set_toggled(match toggled {
                        inker::DocumentA11yToggled::On => Toggled::True,
                        inker::DocumentA11yToggled::Off => Toggled::False,
                        inker::DocumentA11yToggled::Mixed => Toggled::Mixed,
                    });
                } else if let Some(checked) = semantic.state.checked {
                    node.set_toggled(if checked {
                        Toggled::True
                    } else {
                        Toggled::False
                    });
                }
                if let Some(live) = semantic.state.live {
                    node.set_live(match live {
                        inker::DocumentA11yLive::Off => Live::Off,
                        inker::DocumentA11yLive::Polite => Live::Polite,
                        inker::DocumentA11yLive::Assertive => Live::Assertive,
                    });
                }
                if let Some(orientation) = semantic.state.orientation {
                    node.set_orientation(match orientation {
                        inker::DocumentA11yOrientation::Horizontal => Orientation::Horizontal,
                        inker::DocumentA11yOrientation::Vertical => Orientation::Vertical,
                    });
                }
                if let Some(has_popup) = semantic.state.has_popup {
                    node.set_has_popup(match has_popup {
                        inker::DocumentA11yHasPopup::Menu => HasPopup::Menu,
                        inker::DocumentA11yHasPopup::ListBox => HasPopup::Listbox,
                        inker::DocumentA11yHasPopup::Tree => HasPopup::Tree,
                        inker::DocumentA11yHasPopup::Grid => HasPopup::Grid,
                        inker::DocumentA11yHasPopup::Dialog => HasPopup::Dialog,
                    });
                }
                if let Some(value) = semantic.numeric_value {
                    node.set_numeric_value(value);
                }
                if let Some(value) = semantic.numeric_minimum {
                    node.set_min_numeric_value(value);
                }
                if let Some(value) = semantic.numeric_maximum {
                    node.set_max_numeric_value(value);
                }
                if let Some(bounds) = semantic.bounds {
                    node.set_bounds(AccessRect::new(
                        f64::from(bounds.x),
                        f64::from(bounds.y),
                        f64::from(bounds.x + bounds.width),
                        f64::from(bounds.y + bounds.height),
                    ));
                }
                let descendants = semantic
                    .children
                    .iter()
                    .filter_map(|child| local_ids.get(child).copied())
                    .collect::<Vec<_>>();
                node.set_children(descendants);
                if semantic.id == projection.root() {
                    node.set_transform(Affine::translate((
                        f64::from(content_rect.x),
                        f64::from(content_rect.y),
                    )));
                }
                Self::accesskit_actions(&semantic.actions, &mut node);
                if semantic.state.focused {
                    tree.focus = global_id;
                }
                let action_bits = DocumentA11yActionTarget::from_actions(&semantic.actions);
                if action_bits != 0 {
                    actions.insert(
                        global_id,
                        WorkspaceA11yActionTarget::Document(DocumentA11yActionTarget {
                            tile,
                            session_identity,
                            revision: projection.revision(),
                            local_node: semantic.id,
                            content_rect,
                            actions: action_bits,
                        }),
                    );
                }
                tree.nodes.push((global_id, node));
            }
        }
        self.accessibility.clear_stale_document_focus(&actions);
        Ok(WorkspaceA11yProjection {
            tree,
            root,
            actions,
        })
    }

    /// Build the currently retained composite tree without a window. This
    /// keeps semantic projection and its action map GPU-free and testable.
    pub(in crate::workspace_viewer) fn prepare_accessibility_tree(
        &mut self,
    ) -> Result<TreeUpdate, String> {
        self.layout_accessibility_shell()?;
        let projection = self.workspace_accessibility_projection()?;
        Ok(self
            .accessibility
            .prepare(projection, self.scale_factor as f64))
    }

    pub(in crate::workspace_viewer) fn install_accessibility_before_show(
        &mut self,
    ) -> Result<(), String> {
        self.layout_accessibility_shell()?;
        let Some(window) = self.window.clone() else {
            return Err("Pelt accessibility install needs its live window".to_owned());
        };
        let projection = self.workspace_accessibility_projection()?;
        let _ = self
            .accessibility
            .sync(&window, projection, self.scale_factor as f64);
        if matches!(
            self.config.workspace_receipt,
            Some(
                WorkspaceReceipt::Accessibility
                    | WorkspaceReceipt::AccessibilityAddress
                    | WorkspaceReceipt::AccessibilityChildren
                    | WorkspaceReceipt::AccessibilityEdit
                    | WorkspaceReceipt::AccessibilityScroll
                    | WorkspaceReceipt::AccessibilityClick
                    | WorkspaceReceipt::AccessibilityInput
                    | WorkspaceReceipt::ReaderAccessibility,
            )
        ) && self.accessibility.status() != BridgeStatus::Installed
        {
            return Err(
                "Pelt accessibility receipt could not install the platform AccessKit bridge"
                    .to_owned(),
            );
        }
        Ok(())
    }

    pub(in crate::workspace_viewer) fn sync_accessibility(&mut self) -> bool {
        let Some(window) = self.window.clone() else {
            return false;
        };
        self.refresh_accessibility_content_regions();
        let Ok(projection) = self.workspace_accessibility_projection() else {
            return false;
        };
        self.accessibility
            .sync(&window, projection, self.scale_factor as f64)
            .into_iter()
            .fold(false, |redraw, request| {
                self.apply_accessibility_request(request) || redraw
            })
    }

    /// Send one accessibility-validated semantic activation through exactly
    /// the physical primary-button route that ordinary content uses. Callers
    /// provide a freshly revalidated point, but Pelt retains tile, session,
    /// content-hole, and capture authority across both transitions.
    pub(in crate::workspace_viewer) fn route_accessibility_pointer_click(
        &mut self,
        tile: TileId,
        session_identity: PeltSessionIdentity,
        content_rect: WorkspaceRect,
        x: f32,
        y: f32,
    ) -> bool {
        self.clear_chrome_address();
        self.clear_chrome_engine_menu();
        self.clear_chrome_appearance();
        let pressed = self.workspace.input(SessionInput::PointerButton {
            x,
            y,
            button: SessionPointerButton::Primary,
            state: SessionButtonState::Pressed,
            modifiers: self.modifiers,
        });
        let mut redraw = pressed.redraw;
        let press_handled = pressed.handled;
        self.apply_effect(pressed);
        if !press_handled
            || self.workspace.document_session_identity(tile) != Some(session_identity)
            || self.workspace.content_rect(tile) != Some(content_rect)
        {
            return redraw;
        }
        let released = self.workspace.input(SessionInput::PointerButton {
            x,
            y,
            button: SessionPointerButton::Primary,
            state: SessionButtonState::Released,
            modifiers: self.modifiers,
        });
        redraw |= released.redraw;
        self.apply_effect(released);
        redraw
    }
}
