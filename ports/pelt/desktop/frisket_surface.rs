//! Frisket rendered through the owned Livery/Buckram lane.

use std::cell::RefCell;
use std::rc::Rc;

use cambium::{
    AnyView, DividerTarget, DomHandle, FRISKET_CSS, FRISKET_TILE_ATTR, GenetAppRunner, GenetCtx,
    GenetElement, close_target, content_target, divider_target, frisket, stack_target,
    tab_drop_index, tab_target,
};
use genet_host_api::tile::{TileId, TilePath, TileTree};
use genet_livery::{Device, LiveryDocument, StyleSet};
use genet_scripted_dom::{NodeId, ScriptedDom};
use layout_dom_api::{LayoutDom, LocalName, Namespace};
use pelt_core::WorkspaceRect;

type FrameView = Box<dyn AnyView<FrameState, (), GenetCtx, GenetElement>>;
type FrameLogic = fn(&FrameState) -> FrameView;

struct FrameState {
    tree: TileTree,
}

fn frame_view(state: &FrameState) -> FrameView {
    Box::new(frisket(&state.tree, |_state: &mut FrameState, _event| {}))
}

/// One laid-out Frisket frame and the active content holes it authorizes.
pub(crate) struct FrisketFrame {
    pub scene: netrender::Scene,
    pub content_rects: Vec<(TileId, WorkspaceRect)>,
}

/// Semantic result of hit-testing the live Frisket DOM.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FrisketHit {
    Content(TileId),
    Close(TileId),
    Tab(TileId),
    Divider {
        target: DividerTarget,
        split_rect: WorkspaceRect,
    },
    Chrome,
}

/// A retained, GPU-free pane frame. Its DOM is produced by Cambium Frisket;
/// Livery/Buckram supplies both its scene and the geometry consumed by Pelt.
pub(crate) struct FrisketSurface {
    tree: TileTree,
    viewport: (u32, u32),
    document: LiveryDocument<ScriptedDom>,
}

impl FrisketSurface {
    pub fn new(tree: &TileTree) -> Self {
        let viewport = (800, 600);
        Self {
            tree: tree.clone(),
            viewport,
            document: document_for(tree, viewport.0, viewport.1),
        }
    }

    pub fn set_tree(&mut self, tree: &TileTree) {
        self.tree = tree.clone();
        self.document = document_for(tree, self.viewport.0, self.viewport.1);
    }

    pub fn frame(&mut self, width: u32, height: u32) -> Result<FrisketFrame, String> {
        let viewport = (width.max(1), height.max(1));
        if self.viewport != viewport {
            self.viewport = viewport;
            self.document = document_for(&self.tree, viewport.0, viewport.1);
        }
        let list = self
            .document
            .frame(viewport.0, viewport.1)
            .map_err(|error| format!("could not lay out Frisket: {error}"))?;
        let content_rects = nodes_with_attr(self.document.dom(), FRISKET_TILE_ATTR)
            .into_iter()
            .filter_map(|node| {
                let id = attr(self.document.dom(), node, FRISKET_TILE_ATTR)?
                    .parse::<u64>()
                    .ok()?;
                self.document
                    .fragment_rect(node)
                    .map(|rect| (TileId(id), workspace_rect(rect)))
            })
            .collect();
        Ok(FrisketFrame {
            scene: paint_list_render::translate_paint_list(&list),
            content_rects,
        })
    }

    pub fn hit(&self, x: f32, y: f32) -> Option<FrisketHit> {
        let node = self.document.hit_test(x, y)?;
        let dom = self.document.dom();
        if let Some(tile) = close_target(dom, node) {
            return Some(FrisketHit::Close(tile));
        }
        if let Some(target) = divider_target(dom, node) {
            let split_rect = divider_node(dom, node)
                .and_then(|divider| dom.parent(divider))
                .and_then(|split| self.document.fragment_rect(split))
                .map(workspace_rect)?;
            return Some(FrisketHit::Divider { target, split_rect });
        }
        if let Some(tile) = tab_target(dom, node) {
            return Some(FrisketHit::Tab(tile));
        }
        if let Some(tile) = content_target(dom, node) {
            return Some(FrisketHit::Content(tile));
        }
        Some(FrisketHit::Chrome)
    }

    pub fn tabbar_drop(&self, x: f32, y: f32) -> Option<(TilePath, usize)> {
        let hit = self.document.hit_test(x, y)?;
        stack_target(self.document.dom(), hit)?;
        tab_drop_index(self.document.dom(), hit, x, |node| {
            self.document
                .fragment_rect(node)
                .map(|rect| (rect[0], rect[1], rect[2], rect[3]))
        })
    }

    pub fn tab_rect(&self, tile: TileId) -> Option<WorkspaceRect> {
        self.rect_for_attr("data-tabid", &tile.0.to_string())
    }

    pub fn close_rect(&self, tile: TileId) -> Option<WorkspaceRect> {
        nodes_in_document(self.document.dom())
            .into_iter()
            .find(|node| close_target(self.document.dom(), *node) == Some(tile))
            .and_then(|node| self.document.fragment_rect(node))
            .map(workspace_rect)
    }

    pub fn divider_rect(&self, target: &DividerTarget) -> Option<WorkspaceRect> {
        nodes_with_attr(self.document.dom(), "data-divider")
            .into_iter()
            .find(|node| divider_target(self.document.dom(), *node).as_ref() == Some(target))
            .and_then(|node| self.document.fragment_rect(node))
            .map(workspace_rect)
    }

    fn rect_for_attr(&self, name: &str, value: &str) -> Option<WorkspaceRect> {
        nodes_with_attr(self.document.dom(), name)
            .into_iter()
            .find(|node| attr(self.document.dom(), *node, name).as_deref() == Some(value))
            .and_then(|node| self.document.fragment_rect(node))
            .map(workspace_rect)
    }
}

fn document_for(tree: &TileTree, width: u32, height: u32) -> LiveryDocument<ScriptedDom> {
    let handle: DomHandle = Rc::new(RefCell::new(ScriptedDom::new()));
    let runner = GenetAppRunner::new(
        handle.clone(),
        frame_view as FrameLogic,
        FrameState { tree: tree.clone() },
    );
    let html = handle.borrow().outer_html(runner.root());
    let dom = ScriptedDom::from_serialized_document(&html);
    let host_css = format!(
        "html, body {{ display: block; width: {width}px; height: {height}px; margin: 0; overflow: hidden; }} \
         * {{ box-sizing: border-box; }} \
         .frisket-body {{ width: {width}px; height: {height}px; }}"
    );
    LiveryDocument::new(
        dom,
        StyleSet::cambium(&[&host_css, FRISKET_CSS]),
        Device::screen(width as f32, height as f32),
    )
}

fn attr(dom: &ScriptedDom, node: NodeId, name: &str) -> Option<String> {
    dom.attribute(node, &Namespace::default(), &LocalName::from(name))
        .map(|value| value.to_string())
}

fn nodes_with_attr(dom: &ScriptedDom, name: &str) -> Vec<NodeId> {
    fn visit(dom: &ScriptedDom, node: NodeId, name: &str, out: &mut Vec<NodeId>) {
        if attr(dom, node, name).is_some() {
            out.push(node);
        }
        for child in dom.dom_children(node) {
            visit(dom, child, name, out);
        }
    }

    let mut nodes = Vec::new();
    visit(dom, dom.document(), name, &mut nodes);
    nodes
}

fn nodes_in_document(dom: &ScriptedDom) -> Vec<NodeId> {
    fn visit(dom: &ScriptedDom, node: NodeId, out: &mut Vec<NodeId>) {
        out.push(node);
        for child in dom.dom_children(node) {
            visit(dom, child, out);
        }
    }

    let mut nodes = Vec::new();
    visit(dom, dom.document(), &mut nodes);
    nodes
}

fn divider_node(dom: &ScriptedDom, hit: NodeId) -> Option<NodeId> {
    let mut node = hit;
    loop {
        if attr(dom, node, "data-divider").is_some() {
            return Some(node);
        }
        node = dom.parent(node)?;
    }
}

fn workspace_rect(rect: [f32; 4]) -> WorkspaceRect {
    WorkspaceRect::new(rect[0], rect[1], rect[2], rect[3])
}

#[cfg(test)]
mod tests {
    use genet_host_api::tile::{ContentSource, DocumentRef, SplitAxis, Tile, TileBranch};

    use super::*;

    fn tile(id: u64) -> Tile {
        Tile {
            id: TileId(id),
            title: format!("Tile {id}"),
            content: ContentSource::Document(DocumentRef(format!("tile-{id}.html"))),
            accent: None,
        }
    }

    fn nested_tree() -> TileTree {
        TileTree::split(
            SplitAxis::Row,
            vec![
                TileBranch::new(0.5, TileTree::stack(vec![tile(1), tile(2)], 0)),
                TileBranch::new(
                    0.5,
                    TileTree::split(
                        SplitAxis::Column,
                        vec![
                            TileBranch::new(0.5, TileTree::single(tile(3))),
                            TileBranch::new(0.5, TileTree::single(tile(4))),
                        ],
                    ),
                ),
            ],
        )
    }

    #[test]
    fn livery_geometry_is_the_content_hole_and_hit_authority() {
        let mut surface = FrisketSurface::new(&nested_tree());
        let frame = surface.frame(800, 600).unwrap();
        assert_eq!(frame.content_rects.len(), 3);
        let rects = frame
            .content_rects
            .into_iter()
            .collect::<std::collections::HashMap<_, _>>();
        let left = rects[&TileId(1)];
        let top_right = rects[&TileId(3)];
        let bottom_right = rects[&TileId(4)];
        assert!(left.x < top_right.x);
        assert!(top_right.y < bottom_right.y);
        assert!(
            left.height > top_right.height,
            "left={left:?} top_right={top_right:?} bottom_right={bottom_right:?}"
        );
        assert_eq!(
            surface.hit(left.x + left.width / 2.0, left.y + left.height / 2.0),
            Some(FrisketHit::Content(TileId(1)))
        );

        let tab = surface
            .rect_for_attr("data-tabid", "2")
            .expect("second tab");
        assert_eq!(
            surface.hit(tab.x + 4.0, tab.y + tab.height / 2.0),
            Some(FrisketHit::Tab(TileId(2)))
        );
        let divider = surface
            .rect_for_attr("data-divider", "")
            .expect("root divider");
        assert!(matches!(
            surface.hit(
                divider.x + divider.width / 2.0,
                divider.y + divider.height / 2.0
            ),
            Some(FrisketHit::Divider { .. })
        ));
    }
}
