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

/// A Pelt-owned action target for one node in the composite tree.
#[derive(Clone, Debug)]
pub(super) enum WorkspaceA11yActionTarget {
    Frisket(genet_scripted_dom::NodeId),
    Livery(LiveryA11yAction),
    #[cfg(feature = "reader")]
    Reader(ReaderA11yAction),
}

impl WorkspaceA11yActionTarget {
    fn is_document(&self) -> bool {
        match self {
            Self::Frisket(_) => false,
            Self::Livery(_) => true,
            #[cfg(feature = "reader")]
            Self::Reader(_) => true,
        }
    }
}

/// The session identity and workspace point needed to route a retained Livery
/// semantic action through the ordinary Pelt input path.
#[derive(Clone, Copy, Debug)]
pub(super) struct LiveryA11yAction {
    pub(super) tile: TileId,
    pub(super) session_generation: u64,
    pub(super) local_node: AccessNodeId,
    pub(super) content_rect: WorkspaceRect,
    /// Whether the composite tree advertised Click for this action. A pointer
    /// point alone is never authority to activate a disabled or hidden node.
    pub(super) click_enabled: bool,
    /// Whether the current composite tree advertised Focus for this node.
    pub(super) focus_enabled: bool,
    /// Whether the current composite tree advertised ScrollIntoView for this
    /// node.
    pub(super) scroll_enabled: bool,
    /// Whether the current composite tree advertised SetValue for this
    /// retained node. Accessibility actions are checked against this snapshot
    /// before Pelt asks Livery to mutate a value.
    pub(super) set_value_enabled: bool,
    /// The point published with the composite tree proves that Click was
    /// advertised. Dispatch queries Livery again instead of trusting these
    /// coordinates after a later scroll in the same session.
    pub(super) click_point: Option<(f32, f32)>,
}

/// The session identity and logical Reader link needed to keep a retained
/// semantic Focus action virtual and stale-safe.
#[cfg(feature = "reader")]
#[derive(Clone, Debug)]
pub(super) struct ReaderA11yAction {
    pub(super) tile: TileId,
    pub(super) session_generation: u64,
    pub(super) local_node: AccessNodeId,
    /// The retained Reader record carries the opaque document-canvas identity.
    /// It is compared against the current partial snapshot before Pelt accepts
    /// a virtual Focus action.
    pub(super) link: genet_documents::ReaderAccessibilityLink,
    pub(super) focus_enabled: bool,
}

/// One tile-local namespace inside Pelt's root AccessKit tree.
///
/// AccessKit node IDs are only local-tree unique. Pelt owns these assignments
/// so independently constructed `ScriptedDom`s cannot collide when their
/// retained trees become siblings below Frisket's content apertures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DocumentA11ySource {
    Livery,
    #[cfg(feature = "reader")]
    Reader,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct DocumentA11ySession {
    pub(super) source: DocumentA11ySource,
    pub(super) generation: u64,
}

pub(super) struct DocumentA11yNamespace {
    pub(super) source: DocumentA11ySource,
    pub(super) session_generation: u64,
    pub(super) global_ids: HashMap<AccessNodeId, AccessNodeId>,
    /// Reader's opaque link tokens cannot be represented as accesskit IDs and
    /// must not be replaced with output order. Retain a tile/session-local
    /// association so a removed link's global ID becomes inert rather than
    /// being aliased to its next sibling after a reflow.
    #[cfg(feature = "reader")]
    pub(super) reader_local_ids: Vec<(genet_documents::ReaderAccessibilityLink, AccessNodeId)>,
    #[cfg(feature = "reader")]
    pub(super) next_reader_local_node_id: u64,
}

impl DocumentA11yNamespace {
    fn new(source: DocumentA11ySource, session_generation: u64) -> Self {
        Self {
            source,
            session_generation,
            global_ids: HashMap::new(),
            #[cfg(feature = "reader")]
            reader_local_ids: Vec::new(),
            #[cfg(feature = "reader")]
            // The Reader root owns local ID 1. Link tokens begin at 2.
            next_reader_local_node_id: 2,
        }
    }
}

/// The tree and action map delivered together to the one-tree platform bridge.
pub(super) struct WorkspaceA11yProjection {
    pub(super) tree: TreeUpdate,
    pub(super) root: AccessNodeId,
    pub(super) actions: HashMap<AccessNodeId, WorkspaceA11yActionTarget>,
}

/// A completed retained Livery tree ready to attach below one Frisket hole.
pub(super) struct LiveryA11yChild {
    pub(super) tile: TileId,
    pub(super) session_generation: u64,
    pub(super) aperture: AccessNodeId,
    pub(super) root: AccessNodeId,
    pub(super) tree: TreeUpdate,
    pub(super) transform: Affine,
    pub(super) content_rect: WorkspaceRect,
    pub(super) content_origin: (f32, f32),
    pub(super) page_zoom: f32,
    /// Livery-owned CSS hit points for semantic Click targets. AccessKit
    /// bounds remain presentation data and are not a pointer-routing oracle.
    pub(super) pointer_targets: HashMap<AccessNodeId, (f32, f32)>,
}

/// A partial Reader semantic tree assembled from its already-presented,
/// renderer-neutral snapshot. It exposes only visible links and never claims a
/// complete semantic rendering of Fleece's article.
#[cfg(feature = "reader")]
pub(super) struct ReaderA11yChild {
    pub(super) tile: TileId,
    pub(super) session_generation: u64,
    pub(super) aperture: AccessNodeId,
    pub(super) root: AccessNodeId,
    pub(super) nodes: Vec<(AccessNodeId, AccessNode)>,
    pub(super) transform: Affine,
    pub(super) links: HashMap<AccessNodeId, genet_documents::ReaderAccessibilityLink>,
}

#[cfg(feature = "reader")]
fn reader_link_bounds(rects: &[[f32; 4]]) -> Option<AccessRect> {
    let mut bounds: Option<(f64, f64, f64, f64)> = None;
    for &[x, y, width, height] in rects {
        if !x.is_finite()
            || !y.is_finite()
            || !width.is_finite()
            || !height.is_finite()
            || width <= 0.0
            || height <= 0.0
        {
            continue;
        }
        let right = x + width;
        let bottom = y + height;
        if !right.is_finite() || !bottom.is_finite() {
            continue;
        }
        let rect = (
            f64::from(x),
            f64::from(y),
            f64::from(right),
            f64::from(bottom),
        );
        bounds = Some(match bounds {
            Some((left, top, old_right, old_bottom)) => (
                left.min(rect.0),
                top.min(rect.1),
                old_right.max(rect.2),
                old_bottom.max(rect.3),
            ),
            None => rect,
        });
    }
    bounds.map(|(left, top, right, bottom)| AccessRect::new(left, top, right, bottom))
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

    pub(super) fn prepare(&mut self, projection: WorkspaceA11yProjection, scale_factor: f64) -> TreeUpdate {
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

    fn sync(
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

    fn set_focus(&mut self, target: WorkspaceA11yFocus) -> bool {
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
                    source: namespace.source,
                    generation: namespace.session_generation,
                })
        });
    }

    pub(super) fn child_global_id(
        &mut self,
        tile: TileId,
        source: DocumentA11ySource,
        session_generation: u64,
        local_id: AccessNodeId,
        shell_ids: &HashSet<AccessNodeId>,
    ) -> AccessNodeId {
        let reset_namespace = self.child_namespaces.get(&tile).is_some_and(|namespace| {
            namespace.source != source
                || namespace.session_generation != session_generation
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
            .or_insert_with(|| DocumentA11yNamespace::new(source, session_generation));
        debug_assert_eq!(namespace.source, source);
        debug_assert_eq!(namespace.session_generation, session_generation);
        namespace.global_ids.insert(local_id, global_id);
        global_id
    }

    /// Give one opaque Reader link token a stable local AccessKit ID for the
    /// life of its tile session. This map intentionally retains disappeared
    /// links until the session is replaced, so a queued action cannot acquire
    /// a different link merely because the next snapshot emitted it earlier.
    #[cfg(feature = "reader")]
    fn reader_local_node_id(
        &mut self,
        tile: TileId,
        session_generation: u64,
        link: &genet_documents::ReaderAccessibilityLink,
    ) -> AccessNodeId {
        let reset_namespace = self.child_namespaces.get(&tile).is_some_and(|namespace| {
            namespace.source != DocumentA11ySource::Reader
                || namespace.session_generation != session_generation
        });
        if reset_namespace {
            self.child_namespaces.remove(&tile);
        }
        let namespace = self.child_namespaces.entry(tile).or_insert_with(|| {
            DocumentA11yNamespace::new(DocumentA11ySource::Reader, session_generation)
        });
        debug_assert_eq!(namespace.source, DocumentA11ySource::Reader);
        debug_assert_eq!(namespace.session_generation, session_generation);
        if let Some((_, id)) = namespace
            .reader_local_ids
            .iter()
            .find(|(known, _)| known.identity == link.identity)
        {
            return *id;
        }
        let id = AccessNodeId(namespace.next_reader_local_node_id);
        namespace.next_reader_local_node_id = namespace
            .next_reader_local_node_id
            .checked_add(1)
            .expect("Pelt Reader accessibility local node IDs exhausted");
        namespace.reader_local_ids.push((link.clone(), id));
        id
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


impl WorkspaceApp {
    pub(in crate::workspace_viewer) fn refresh_accessibility_content_regions(&mut self) {
        let regions = self
            .workspace
            .tree()
            .tiles()
            .into_iter()
            .map(|tile| {
                let controller = self.workspace.controller(tile.id);
                let is_livery = controller.is_some_and(|controller| {
                    controller
                        .session_as_any_ref()
                        .is::<genet_documents::LiveryDocumentSession>()
                });
                let description = match controller.map(PeltController::a11y_capability) {
                    Some(A11yCapability::Opaque) => {
                        "The engine declares opaque accessibility. Pelt cannot inspect or compose this content's semantics."
                    },
                    Some(A11yCapability::Partial) if is_livery => {
                        "The engine declares partial accessibility. Pelt composes this Livery document's available retained semantics into the workspace tree."
                    },
                    Some(A11yCapability::Partial) => {
                        "The engine declares partial accessibility. Pelt does not compose its document semantics into this workspace tree yet."
                    },
                    Some(A11yCapability::Full) => {
                        "The engine declares a full semantic tree. Pelt does not compose that child tree into this workspace tree yet."
                    },
                    None => {
                        "Pelt has not received an accessibility declaration for this content."
                    },
                };
                FrisketContentA11y {
                    tile: tile.id,
                    label: format!("{} content", tile.title),
                    description: description.to_owned(),
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

    /// Gather every live Livery session and every visible one with completed
    /// retained geometry. A session without a finished frame remains an honest
    /// labelled aperture until its engine supplies its own tree.
    pub(in crate::workspace_viewer) fn livery_accessibility_children(
        &self,
        projection: &FrisketA11yProjection,
    ) -> (HashMap<TileId, DocumentA11ySession>, Vec<LiveryA11yChild>) {
        let live_sessions = self
            .workspace
            .tree()
            .tiles()
            .into_iter()
            .filter_map(|tile| {
                let controller = self.workspace.controller(tile.id)?;
                controller
                    .session_as_any_ref()
                    .is::<genet_documents::LiveryDocumentSession>()
                    .then_some((
                        tile.id,
                        DocumentA11ySession {
                            source: DocumentA11ySource::Livery,
                            generation: controller.session_generation(),
                        },
                    ))
            })
            .collect::<HashMap<_, _>>();
        let mut children = Vec::new();
        for (&tile, &aperture) in &projection.content_nodes {
            let Some(controller) = self.workspace.controller(tile) else {
                continue;
            };
            let Some(session) = controller
                .session_as_any_ref()
                .downcast_ref::<genet_documents::LiveryDocumentSession>()
            else {
                continue;
            };
            let document = session.document();
            let Some(layout) = document.retained_layout() else {
                continue;
            };
            let Some(content) = self.workspace.content_rect(tile) else {
                continue;
            };
            let page_zoom = session.page_zoom();
            if !page_zoom.is_finite() || page_zoom <= 0.0 {
                continue;
            }
            let tree = genet_render::accesskit_tree_with_scroll(
                document.dom(),
                layout,
                None,
                document.element_scroll(),
            );
            let Some(root) = tree.tree.as_ref().map(|tree| tree.root) else {
                continue;
            };
            let pointer_targets = tree
                .nodes
                .iter()
                .filter_map(|(local_node, node)| {
                    (node.supports_action(Action::Click)
                        || node.supports_action(Action::ScrollIntoView))
                    .then(|| {
                        session
                            .accessible_pointer_target(local_node.0)
                            .map(|point| (*local_node, point))
                    })?
                })
                .collect();
            let scroll = document.scroll();
            let transform = Affine::translate((f64::from(content.x), f64::from(content.y)))
                * Affine::scale(f64::from(page_zoom))
                * Affine::translate((-f64::from(scroll.0), -f64::from(scroll.1)));
            children.push(LiveryA11yChild {
                tile,
                session_generation: controller.session_generation(),
                aperture,
                root,
                tree,
                transform,
                content_rect: content,
                content_origin: (content.x, content.y),
                page_zoom,
                pointer_targets,
            });
        }
        (live_sessions, children)
    }

    /// Gather Reader's independently retained partial snapshot. Reader does
    /// not publish an AccessKit tree or a generic DocumentSession contract:
    /// its visible logical links are adapted here, where Pelt owns the one
    /// platform tree and tile/session namespace.
    #[cfg(feature = "reader")]
    pub(in crate::workspace_viewer) fn reader_accessibility_children(
        &mut self,
        projection: &FrisketA11yProjection,
    ) -> (HashMap<TileId, DocumentA11ySession>, Vec<ReaderA11yChild>) {
        let live_sessions = self
            .workspace
            .tree()
            .tiles()
            .into_iter()
            .filter_map(|tile| {
                let controller = self.workspace.controller(tile.id)?;
                controller
                    .session_as_any_ref()
                    .is::<genet_documents::ReaderDocumentSession>()
                    .then_some((
                        tile.id,
                        DocumentA11ySession {
                            source: DocumentA11ySource::Reader,
                            generation: controller.session_generation(),
                        },
                    ))
            })
            .collect::<HashMap<_, _>>();
        let mut children = Vec::new();
        for (&tile, &aperture) in &projection.content_nodes {
            let Some((session_generation, snapshot, content)) = (|| {
                let controller = self.workspace.controller(tile)?;
                let session = controller
                    .session_as_any_ref()
                    .downcast_ref::<genet_documents::ReaderDocumentSession>()?;
                let snapshot = session.accessibility_snapshot()?;
                let content = self.workspace.content_rect(tile)?;
                Some((controller.session_generation(), snapshot, content))
            })() else {
                continue;
            };

            let root = AccessNodeId(1);
            let mut root_node = AccessNode::new(Role::Document);
            root_node.set_label(
                snapshot
                    .root_title
                    .filter(|title| !title.trim().is_empty())
                    .unwrap_or_else(|| "Reader document".to_owned()),
            );
            let mut nodes = Vec::new();
            let mut links = HashMap::new();
            let mut child_ids = Vec::new();

            for link in snapshot.links {
                let Some(bounds) = reader_link_bounds(&link.rects) else {
                    continue;
                };
                let local_id =
                    self.accessibility
                        .reader_local_node_id(tile, session_generation, &link);
                let mut node = AccessNode::new(Role::Link);
                node.set_label(link.label.clone());
                node.set_bounds(bounds);
                node.add_action(Action::Focus);
                child_ids.push(local_id);
                links.insert(local_id, link);
                nodes.push((local_id, node));
            }
            root_node.set_children(child_ids);
            nodes.insert(0, (root, root_node));
            children.push(ReaderA11yChild {
                tile,
                session_generation,
                aperture,
                root,
                nodes,
                transform: Affine::translate((f64::from(content.x), f64::from(content.y))),
                links,
            });
        }
        (live_sessions, children)
    }

    /// Merge Livery's local retained trees into Pelt's one-tree platform
    /// bridge. The current bridge cannot preserve AccessKit graft tree IDs on
    /// actions, so Pelt flattens only this supported lane with fresh IDs.
    pub(in crate::workspace_viewer) fn workspace_accessibility_projection(
        &mut self,
    ) -> Result<WorkspaceA11yProjection, String> {
        let frisket_focus = self.accessibility.frisket_focus().cloned();
        let projection = self
            .frisket
            .accessibility_projection(frisket_focus.as_ref())
            .ok_or_else(|| {
                "Frisket has no completed retained layout for accessibility".to_owned()
            })?;
        let (livery_sessions, livery_children) = self.livery_accessibility_children(&projection);
        #[cfg(feature = "reader")]
        let (live_sessions, reader_children) = {
            let mut live_sessions = livery_sessions;
            let (reader_sessions, reader_children) =
                self.reader_accessibility_children(&projection);
            live_sessions.extend(reader_sessions);
            (live_sessions, reader_children)
        };
        #[cfg(not(feature = "reader"))]
        let live_sessions = livery_sessions;
        self.accessibility
            .retain_document_namespaces(&live_sessions);

        let shell_ids = projection
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
        } = projection;
        let mut actions = nodes
            .into_iter()
            // If a later Frisket rebuild happens to reuse a retired child ID,
            // retaining that shell node is safe, but routing an old platform
            // action to it is not. Leave that rare shell action inert until a
            // future host-wide shell remapper owns both namespaces.
            .filter(|(id, _)| !self.accessibility.child_id_is_reserved(*id))
            .map(|(id, node)| (id, WorkspaceA11yActionTarget::Frisket(node)))
            .collect::<HashMap<_, _>>();

        for child in livery_children {
            let LiveryA11yChild {
                tile,
                session_generation,
                aperture,
                root: local_root,
                tree: child_tree,
                transform,
                content_rect,
                content_origin,
                page_zoom,
                pointer_targets,
            } = child;
            let mut child_ids = HashMap::new();
            for (local_id, _) in &child_tree.nodes {
                let global_id = self.accessibility.child_global_id(
                    tile,
                    DocumentA11ySource::Livery,
                    session_generation,
                    *local_id,
                    &shell_ids,
                );
                child_ids.insert(*local_id, global_id);
            }
            let Some(&child_root) = child_ids.get(&local_root) else {
                continue;
            };
            let Some((_, aperture)) = tree.nodes.iter_mut().find(|(id, _)| *id == aperture) else {
                continue;
            };
            aperture.set_children([child_root]);

            for (local_id, mut node) in child_tree.nodes {
                let global_id = child_ids
                    .get(&local_id)
                    .copied()
                    .expect("all Livery nodes receive a Pelt accessibility ID");
                let descendants = node
                    .children()
                    .iter()
                    .map(|local_id| {
                        child_ids
                            .get(local_id)
                            .copied()
                            .expect("Livery child tree references only its own nodes")
                    })
                    .collect::<Vec<_>>();
                node.set_children(descendants);
                if local_id == local_root {
                    node.set_transform(transform);
                }
                let click_point = pointer_targets
                    .get(&local_id)
                    .copied()
                    .map(|(x, y)| {
                        (
                            content_origin.0 + x * page_zoom,
                            content_origin.1 + y * page_zoom,
                        )
                    })
                    .filter(|&(x, y)| content_rect.contains(x, y));
                if !node.is_disabled()
                    && click_point.is_some()
                    && node.supports_action(Action::ScrollIntoView)
                {
                    // A revealed nested target may advertise only
                    // ScrollIntoView in the renderer's conservative tree.
                    // Promote Click only after Livery supplies a clip-aware
                    // point that remains in this tile's content hole.
                    node.add_action(Action::Click);
                }
                if node.is_disabled()
                    || (node.supports_action(Action::Click) && click_point.is_none())
                {
                    // A node's AccessKit bounds are descriptive, not a safe
                    // activation point. Livery owns clip-aware target
                    // selection; Pelt only promotes Click when that query
                    // returns a point that remains inside this tile's hole.
                    node.remove_action(Action::Click);
                }
                if node.supports_action(Action::Focus)
                    || click_point.is_some()
                    || node.supports_action(Action::SetValue)
                    || node.supports_action(Action::ScrollIntoView)
                {
                    let click_enabled = node.supports_action(Action::Click);
                    let focus_enabled = node.supports_action(Action::Focus);
                    let scroll_enabled = node.supports_action(Action::ScrollIntoView);
                    let set_value_enabled = node.supports_action(Action::SetValue);
                    actions.insert(
                        global_id,
                        WorkspaceA11yActionTarget::Livery(LiveryA11yAction {
                            tile,
                            session_generation,
                            local_node: local_id,
                            content_rect,
                            click_enabled,
                            focus_enabled,
                            scroll_enabled,
                            set_value_enabled,
                            click_point,
                        }),
                    );
                }
                tree.nodes.push((global_id, node));
            }
        }
        #[cfg(feature = "reader")]
        for child in reader_children {
            let ReaderA11yChild {
                tile,
                session_generation,
                aperture,
                root: local_root,
                nodes: child_nodes,
                transform,
                links,
            } = child;
            let mut child_ids = HashMap::new();
            for (local_id, _) in &child_nodes {
                let global_id = self.accessibility.child_global_id(
                    tile,
                    DocumentA11ySource::Reader,
                    session_generation,
                    *local_id,
                    &shell_ids,
                );
                child_ids.insert(*local_id, global_id);
            }
            let Some(&child_root) = child_ids.get(&local_root) else {
                continue;
            };
            let Some((_, aperture)) = tree.nodes.iter_mut().find(|(id, _)| *id == aperture) else {
                continue;
            };
            aperture.set_children([child_root]);

            for (local_id, mut node) in child_nodes {
                let global_id = child_ids
                    .get(&local_id)
                    .copied()
                    .expect("all Reader nodes receive a Pelt accessibility ID");
                let descendants = node
                    .children()
                    .iter()
                    .map(|local_id| {
                        child_ids
                            .get(local_id)
                            .copied()
                            .expect("Reader child tree references only its own nodes")
                    })
                    .collect::<Vec<_>>();
                node.set_children(descendants);
                if local_id == local_root {
                    node.set_transform(transform);
                }
                if let Some(link) = links.get(&local_id) {
                    let focus_enabled = node.supports_action(Action::Focus);
                    if focus_enabled {
                        actions.insert(
                            global_id,
                            WorkspaceA11yActionTarget::Reader(ReaderA11yAction {
                                tile,
                                session_generation,
                                local_node: local_id,
                                link: link.clone(),
                                focus_enabled,
                            }),
                        );
                    }
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

    /// Revalidate a queued Livery Click against current retained geometry.
    ///
    /// `click_point` in the action map proves the old tree advertised Click,
    /// but a scroll can move or clip the node without replacing its session.
    /// Livery returns a fresh viewport CSS point only after its own clip-aware
    /// hit test succeeds; Pelt applies the current presentation transform and
    /// keeps tile/session/content-hole authority here.
    pub(in crate::workspace_viewer) fn current_livery_accessibility_click_point(
        &self,
        target: LiveryA11yAction,
    ) -> Option<(f32, f32)> {
        let (css_x, css_y, page_zoom) = {
            let controller = self.workspace.controller(target.tile)?;
            if controller.session_generation() != target.session_generation {
                return None;
            }
            let session = controller
                .session_as_any_ref()
                .downcast_ref::<genet_documents::LiveryDocumentSession>()?;
            let page_zoom = session.page_zoom();
            if !page_zoom.is_finite() || page_zoom <= 0.0 {
                return None;
            }
            let (css_x, css_y) = session.accessible_pointer_target(target.local_node.0)?;
            (css_x, css_y, page_zoom)
        };
        let content_rect = self.workspace.content_rect(target.tile)?;
        if content_rect != target.content_rect {
            return None;
        }
        let point = (
            content_rect.x + css_x * page_zoom,
            content_rect.y + css_y * page_zoom,
        );
        content_rect.contains(point.0, point.1).then_some(point)
    }

    /// Rebuild the retained composite action map before mutating a Livery
    /// control. Platform actions can be queued across an ordinary wheel turn,
    /// so the snapshot that produced a request is not sufficient to authorize
    /// SetValue.
    pub(in crate::workspace_viewer) fn current_livery_action(
        &mut self,
        global_node: AccessNodeId,
        target: &LiveryA11yAction,
        action: Action,
    ) -> bool {
        let Ok(projection) = self.workspace_accessibility_projection() else {
            return false;
        };
        matches!(
            projection.actions.get(&global_node),
            Some(WorkspaceA11yActionTarget::Livery(current))
                if current.tile == target.tile
                    && current.session_generation == target.session_generation
                    && current.local_node == target.local_node
                    && match action {
                        Action::Focus => current.focus_enabled,
                        Action::ScrollIntoView => current.scroll_enabled,
                        Action::SetValue => current.set_value_enabled,
                        Action::Click => current.click_enabled,
                        _ => false,
                    }
        )
    }

    #[cfg(feature = "reader")]
    pub(in crate::workspace_viewer) fn current_reader_action(
        &mut self,
        global_node: AccessNodeId,
        target: &ReaderA11yAction,
        action: Action,
    ) -> bool {
        let Ok(projection) = self.workspace_accessibility_projection() else {
            return false;
        };
        matches!(
            projection.actions.get(&global_node),
            Some(WorkspaceA11yActionTarget::Reader(current))
                if current.tile == target.tile
                    && current.session_generation == target.session_generation
                    && current.local_node == target.local_node
                    && current.link.identity == target.link.identity
                    && match action {
                        Action::Focus => current.focus_enabled,
                        _ => false,
                    }
        )
    }

    /// Send one accessibility-validated semantic activation through exactly
    /// the physical primary-button route that ordinary content uses. Callers
    /// provide a freshly revalidated point, but Pelt retains tile, session,
    /// content-hole, and capture authority across both transitions.
    pub(in crate::workspace_viewer) fn route_accessibility_pointer_click(
        &mut self,
        tile: TileId,
        session_generation: u64,
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
            || self.workspace.document_session_generation(tile) != Some(session_generation)
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
            (Action::Click, WorkspaceA11yActionTarget::Frisket(node)) => {
                match self.frisket.accessibility_target(node) {
                    Some(FrisketA11yTarget::ChromeAction(action)) => {
                        self.apply_chrome_action(action)
                    },
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
                }
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
            (Action::Focus, WorkspaceA11yActionTarget::Livery(target)) => (target.focus_enabled
                && self.current_livery_action(request.target_node, &target, Action::Focus)
                && self.workspace.document_session_generation(target.tile)
                    == Some(target.session_generation))
            .then(|| {
                self.accessibility
                    .set_focus(WorkspaceA11yFocus::Document(request.target_node))
            })
            .unwrap_or(false),
            (Action::Click, WorkspaceA11yActionTarget::Livery(target)) => {
                if self.workspace.document_session_generation(target.tile)
                    != Some(target.session_generation)
                {
                    return false;
                }
                if !target.click_enabled
                    || target.click_point.is_none()
                    || self.workspace.has_active_pointer_capture()
                {
                    return false;
                }
                let Some((x, y)) = self.current_livery_accessibility_click_point(target) else {
                    return false;
                };
                self.route_accessibility_pointer_click(
                    target.tile,
                    target.session_generation,
                    target.content_rect,
                    x,
                    y,
                )
            },
            #[cfg(feature = "reader")]
            (Action::Focus, WorkspaceA11yActionTarget::Reader(target)) => (target.focus_enabled
                && self.current_reader_action(request.target_node, &target, Action::Focus)
                && self.workspace.document_session_generation(target.tile)
                    == Some(target.session_generation))
            .then(|| {
                self.accessibility
                    .set_focus(WorkspaceA11yFocus::Document(request.target_node))
            })
            .unwrap_or(false),
            (Action::SetValue, WorkspaceA11yActionTarget::Livery(target)) => {
                if self.workspace.document_session_generation(target.tile)
                    != Some(target.session_generation)
                    || !target.set_value_enabled
                    || !self.current_livery_action(request.target_node, &target, Action::SetValue)
                {
                    return false;
                }
                let Some(ActionData::Value(value)) = request.data else {
                    return false;
                };
                let Some(controller) = self.workspace.controller_mut(target.tile) else {
                    return false;
                };
                if controller.session_generation() != target.session_generation {
                    return false;
                }
                controller
                    .session_as_any_mut()
                    .downcast_mut::<genet_documents::LiveryDocumentSession>()
                    .is_some_and(|session| {
                        session.replace_accessible_text_value(target.local_node.0, value.as_ref())
                    })
            },
            (Action::ScrollIntoView, WorkspaceA11yActionTarget::Livery(target)) => {
                if self.workspace.document_session_generation(target.tile)
                    != Some(target.session_generation)
                    || !target.scroll_enabled
                    || !self.current_livery_action(
                        request.target_node,
                        &target,
                        Action::ScrollIntoView,
                    )
                {
                    return false;
                }
                let Some(controller) = self.workspace.controller_mut(target.tile) else {
                    return false;
                };
                if controller.session_generation() != target.session_generation {
                    return false;
                }
                controller
                    .session_as_any_mut()
                    .downcast_mut::<genet_documents::LiveryDocumentSession>()
                    .is_some_and(|session| {
                        session.scroll_accessible_node_into_view(target.local_node.0)
                    })
            },
            _ => false,
        }
    }
}
