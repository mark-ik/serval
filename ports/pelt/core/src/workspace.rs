use std::collections::{HashMap, HashSet};

use genet_host_api::tile::{ContentSource, Tile, TileEvent, TileId, TileTree};
use inker::{SessionInput, SessionNavigationCommand, SessionScrollKey};

use crate::{PeltController, PeltHostEffect};

/// One Frisket content hole in workspace coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WorkspaceRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl WorkspaceRect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x
            && y >= self.y
            && x < self.x + self.width.max(0.0)
            && y < self.y + self.height.max(0.0)
    }

    fn local(self, x: f32, y: f32) -> (f32, f32) {
        (x - self.x, y - self.y)
    }

    fn viewport(self) -> (u32, u32) {
        (
            self.width.max(1.0).ceil() as u32,
            self.height.max(1.0).ceil() as u32,
        )
    }
}

/// One active document layer returned by [`PeltWorkspace::frame`].
pub struct PeltTileFrame<F> {
    pub tile: TileId,
    pub rect: WorkspaceRect,
    pub frame: F,
}

/// The document layers for one workspace frame. Frisket's frame scene remains
/// host-owned because the reusable core is generic over the document frame.
pub struct PeltWorkspaceFrame<F> {
    pub tiles: Vec<PeltTileFrame<F>>,
}

/// Pelt's window-neutral recursive workspace.
///
/// `TileTree` is the arrangement authority. Every document tile owns a live
/// controller, including inactive tabs; Frisket content-hole rectangles arrive
/// from the embedding host and are the only geometry used for routing and
/// frame sizing.
pub struct PeltWorkspace<F> {
    tree: TileTree,
    controllers: HashMap<TileId, PeltController<F>>,
    content_rects: HashMap<TileId, WorkspaceRect>,
    focused: Option<TileId>,
    pointer_capture: Option<TileId>,
}

impl<F: 'static> PeltWorkspace<F> {
    /// Build one controller for every document tile. Non-document content
    /// holes remain in the tree for the P4 surface-composition lane.
    pub fn try_new(
        tree: TileTree,
        mut controller_for: impl FnMut(&Tile) -> Result<PeltController<F>, String>,
    ) -> Result<Self, String> {
        let mut controllers = HashMap::new();
        let mut tile_ids = HashSet::new();
        for tile in tree.tiles() {
            if !tile_ids.insert(tile.id) {
                return Err(format!("duplicate tile id {}", tile.id.0));
            }
            if matches!(tile.content, ContentSource::Document(_)) {
                let controller = controller_for(tile)
                    .map_err(|error| format!("could not open tile {}: {error}", tile.id.0))?;
                controllers.insert(tile.id, controller);
            }
        }
        let focused = active_tiles(&tree)
            .into_iter()
            .find(|id| controllers.contains_key(id));
        let mut workspace = Self {
            tree,
            controllers,
            content_rects: HashMap::new(),
            focused,
            pointer_capture: None,
        };
        workspace.sync_tile_metadata();
        workspace.sync_visibility();
        Ok(workspace)
    }

    pub fn tree(&self) -> &TileTree {
        &self.tree
    }

    pub fn focused_tile(&self) -> Option<TileId> {
        self.focused
    }

    pub fn controller(&self, tile: TileId) -> Option<&PeltController<F>> {
        self.controllers.get(&tile)
    }

    pub fn controller_mut(&mut self, tile: TileId) -> Option<&mut PeltController<F>> {
        self.controllers.get_mut(&tile)
    }

    pub fn content_rect(&self, tile: TileId) -> Option<WorkspaceRect> {
        self.content_rects.get(&tile).copied()
    }

    /// Replace the content-hole geometry read from the latest Frisket layout.
    /// Rectangles for inactive or closed tiles are deliberately discarded.
    pub fn set_content_rects(&mut self, rects: impl IntoIterator<Item = (TileId, WorkspaceRect)>) {
        self.content_rects = rects.into_iter().collect();
    }

    /// Apply a standalone Pelt arrangement gesture through the shared reducer.
    pub fn apply(&mut self, event: &TileEvent) -> bool {
        let changed = self.tree.apply(event);
        if !changed {
            if let TileEvent::Activated(id) = event {
                if active_tiles(&self.tree).contains(id) && self.controllers.contains_key(id) {
                    let focus_changed = self.focused != Some(*id);
                    self.focus(*id);
                    return focus_changed;
                }
            }
            return false;
        }

        let retained = self
            .tree
            .tiles()
            .into_iter()
            .map(|tile| tile.id)
            .collect::<HashSet<_>>();
        self.controllers.retain(|id, _| retained.contains(id));
        self.content_rects.retain(|id, _| retained.contains(id));
        if self
            .pointer_capture
            .is_some_and(|id| !retained.contains(&id))
        {
            self.pointer_capture = None;
        }

        let next_focus = match event {
            TileEvent::Activated(id) | TileEvent::Dragged { tile: id, .. }
                if self.controllers.contains_key(id) =>
            {
                Some(*id)
            },
            _ if self.focused.is_some_and(|id| retained.contains(&id)) => self.focused,
            _ => active_tiles(&self.tree)
                .into_iter()
                .find(|id| self.controllers.contains_key(id)),
        };
        match next_focus {
            Some(id) => self.focus(id),
            None => self.focused = None,
        }
        self.sync_visibility();
        true
    }

    /// Produce one frame for every active document hole, sized to that hole.
    pub fn frame(&mut self) -> PeltWorkspaceFrame<F> {
        let active = active_tiles(&self.tree);
        let mut tiles = Vec::with_capacity(active.len());
        for tile in active {
            let Some(rect) = self.content_rects.get(&tile).copied() else {
                continue;
            };
            let Some(controller) = self.controllers.get_mut(&tile) else {
                continue;
            };
            let (width, height) = rect.viewport();
            tiles.push(PeltTileFrame {
                tile,
                rect,
                frame: controller.frame(width, height),
            });
        }
        self.sync_tile_metadata();
        PeltWorkspaceFrame { tiles }
    }

    /// Advance visible sessions. Hidden tabs retain state without driving the
    /// foreground frame loop.
    pub fn pump(&mut self) -> bool {
        let mut more = false;
        for id in active_tiles(&self.tree) {
            if let Some(controller) = self.controllers.get_mut(&id) {
                more |= controller.pump();
            }
        }
        more
    }

    /// Route neutral input. Pointer coordinates are workspace coordinates and
    /// are translated into the selected Frisket content hole; keyboard, text,
    /// IME, and focus route to the focused tile.
    pub fn input(&mut self, input: SessionInput) -> PeltHostEffect {
        let (target, local_input) = match input {
            SessionInput::PointerMoved { x, y, modifiers } => {
                let target = self.pointer_capture.or_else(|| self.tile_at(x, y));
                let Some(target) = target else {
                    return PeltHostEffect::default();
                };
                let Some(rect) = self.content_rect(target) else {
                    return PeltHostEffect::default();
                };
                let (x, y) = rect.local(x, y);
                (target, SessionInput::PointerMoved { x, y, modifiers })
            },
            SessionInput::PointerButton {
                x,
                y,
                button,
                state,
                modifiers,
            } => {
                let target = self.pointer_capture.or_else(|| self.tile_at(x, y));
                let Some(target) = target else {
                    return PeltHostEffect::default();
                };
                let Some(rect) = self.content_rect(target) else {
                    return PeltHostEffect::default();
                };
                self.focus(target);
                let (x, y) = rect.local(x, y);
                (
                    target,
                    SessionInput::PointerButton {
                        x,
                        y,
                        button,
                        state,
                        modifiers,
                    },
                )
            },
            other => {
                let Some(target) = self.focused else {
                    return PeltHostEffect::default();
                };
                (target, other)
            },
        };

        let Some(controller) = self.controllers.get_mut(&target) else {
            return PeltHostEffect::default();
        };
        let effect = controller.input(local_input);
        if let Some(capture) = effect.pointer_capture {
            self.pointer_capture = capture.then_some(target);
        }
        if effect.navigated {
            self.sync_one_tile_metadata(target);
            self.sync_visibility();
        }
        effect
    }

    pub fn scroll_at(&mut self, x: f32, y: f32, dx: f32, dy: f32) -> bool {
        let Some(tile) = self.tile_at(x, y) else {
            return false;
        };
        let Some(rect) = self.content_rect(tile) else {
            return false;
        };
        let (x, y) = rect.local(x, y);
        self.controllers
            .get_mut(&tile)
            .is_some_and(|controller| controller.scroll_at(x, y, dx, dy))
    }

    pub fn scroll_for_key(&mut self, key: SessionScrollKey) -> bool {
        let Some(tile) = self.focused else {
            return false;
        };
        self.controllers
            .get_mut(&tile)
            .is_some_and(|controller| controller.scroll_for_key(key))
    }

    pub fn command(&mut self, command: SessionNavigationCommand) -> PeltHostEffect {
        let Some(tile) = self.focused else {
            return PeltHostEffect::default();
        };
        self.command_for(tile, command)
    }

    pub fn command_for(
        &mut self,
        tile: TileId,
        command: SessionNavigationCommand,
    ) -> PeltHostEffect {
        let Some(controller) = self.controllers.get_mut(&tile) else {
            return PeltHostEffect::default();
        };
        let effect = controller.command(command);
        if effect.navigated {
            self.sync_one_tile_metadata(tile);
            self.sync_visibility();
        }
        effect
    }

    fn tile_at(&self, x: f32, y: f32) -> Option<TileId> {
        active_tiles(&self.tree).into_iter().find(|id| {
            self.content_rects
                .get(id)
                .is_some_and(|rect| rect.contains(x, y))
        })
    }

    fn focus(&mut self, tile: TileId) {
        if self.focused == Some(tile) {
            return;
        }
        if let Some(old) = self.focused.and_then(|id| self.controllers.get_mut(&id)) {
            let _ = old.input(SessionInput::Focus(false));
        }
        self.focused = Some(tile);
        if let Some(new) = self.controllers.get_mut(&tile) {
            let _ = new.input(SessionInput::Focus(true));
        }
    }

    fn sync_visibility(&mut self) {
        let active = active_tiles(&self.tree).into_iter().collect::<HashSet<_>>();
        for (id, controller) in &mut self.controllers {
            controller.set_hidden(!active.contains(id));
        }
    }

    fn sync_tile_metadata(&mut self) {
        let ids = self.controllers.keys().copied().collect::<Vec<_>>();
        for id in ids {
            self.sync_one_tile_metadata(id);
        }
    }

    fn sync_one_tile_metadata(&mut self, id: TileId) {
        let Some(controller) = self.controllers.get(&id) else {
            return;
        };
        let address = controller.address().to_owned();
        let title = controller.title();
        if let Some(tile) = self.tree.tile_mut(id) {
            tile.content = ContentSource::Document(genet_host_api::tile::DocumentRef(address));
            if let Some(title) = title.filter(|title| !title.trim().is_empty()) {
                tile.title = title;
            }
        }
    }
}

fn active_tiles(tree: &TileTree) -> Vec<TileId> {
    fn visit(tree: &TileTree, active: &mut Vec<TileId>) {
        match tree {
            TileTree::Split { children, .. } => {
                for branch in children {
                    visit(&branch.tree, active);
                }
            },
            TileTree::Stack(stack) => {
                if let Some(tile) = stack.tabs.get(stack.active) {
                    active.push(tile.id);
                }
            },
        }
    }

    let mut active = Vec::new();
    visit(tree, &mut active);
    active
}
