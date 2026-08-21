/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! A bounded, interactive viewport over Sprigging's shared graph canvas.
//!
//! The graph remains one paint leaf. Each node also gets a small, absolutely
//! positioned native button in the view layer, projected through the same
//! [`GraphViewport`] as the leaf. Paint, hit targets, keyboard focus, and hover
//! therefore stay aligned without teaching a paint leaf about app actions.

use sprigging::{ColorF, GraphCanvas, GraphGlyphNode, GraphGlyphRelation, GraphViewport, Size};

use crate::component::{ComponentView, component};
use crate::{
    FocusEvent, FocusPhase, GenetCtx, GenetElement, HoverEvent, HoverPhase, OptionalAction,
    PointerClick, PointerEvent, PointerPhase, View, WheelEvent, custom_leaf, el, focusable,
    on_click, on_focus, on_hover, on_pointer, on_wheel,
};

/// Structural classes emitted by [`graph_canvas_swatch`]. Hosts own the palette.
pub const GRAPH_CANVAS_SWATCH_CSS: &str = r#"
.graph-canvas-swatch {
    background-color: rgba(127, 127, 127, 0.06);
    border: 1px solid rgba(127, 127, 127, 0.28);
    border-radius: 7px;
}
.graph-canvas-swatch-node {
    background-color: transparent;
    border: 0;
    border-radius: 999px;
    cursor: pointer;
    padding: 0;
    touch-action: none;
    user-select: none;
}
.graph-canvas-swatch-node:focus-visible {
    outline: 1px solid currentColor;
    outline-offset: 1px;
}
.graph-canvas-swatch-relation {
    background: transparent;
    border: 0;
    cursor: pointer;
    padding: 0;
    touch-action: manipulation;
}
.graph-canvas-swatch-relation:focus-visible {
    outline: 1px solid currentColor;
    outline-offset: 1px;
}
.graph-canvas-swatch-relation.emphasized::after {
    background-color: currentColor;
    content: "";
    height: 3px;
    left: 0;
    opacity: 0.72;
    position: absolute;
    top: 50%;
    transform: translateY(-50%);
    width: 100%;
}
.graph-canvas-swatch-label {
    display: block;
    font-size: 10px;
    line-height: 1;
    white-space: nowrap;
    opacity: 0.85;
    pointer-events: none;
    z-index: 1;
}
.graph-canvas-swatch-labels,
.graph-canvas-swatch-relation-targets,
.graph-canvas-swatch-targets {
    pointer-events: none;
}
.graph-canvas-swatch-relation,
.graph-canvas-swatch-node {
    pointer-events: auto;
}
.graph-canvas-swatch-label.selected {
    font-weight: bold;
    opacity: 1;
}
.graph-canvas-swatch-label.focused,
.graph-canvas-swatch-label.hovered {
    opacity: 1;
}
.graph-canvas-swatch-expand {
    background-color: rgba(127, 127, 127, 0.10);
    border: 0;
    border-radius: 4px;
    cursor: pointer;
    font-size: 10px;
    padding: 2px 5px;
}
"#;

/// One node in the app-facing subgraph contract.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphCanvasNode<Id, Kind> {
    pub id: Id,
    pub kind: Kind,
    /// Normalized `0..1` scene position.
    pub position: (f32, f32),
    /// Accessible name for the node's native hit target.
    pub label: String,
    /// An optional stable key emitted as `data-key` on the node's hit target,
    /// for targeting by identity when the `label` is not unique (two nodes may
    /// share a title). The component does not interpret it — a driver or test
    /// selects on it; a screen reader still reads `label`.
    pub key: Option<String>,
}

/// One captured node-motion event from a [`GraphCanvasSwatch`].
///
/// `position` is in the graph's normalized `0..=1` coordinate system, after
/// the Swatch has inverted its current viewport. A consumer owns the response:
/// it may write a local position override, hand the position to a solver, or
/// ignore the gesture. The component never edits graph truth.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphCanvasNodeDrag<Id> {
    pub id: Id,
    pub phase: PointerPhase,
    pub position: (f32, f32),
}

/// A leaf-local rectangular region anchored to one projected graph node.
///
/// Consumers use this for richer overlays whose contents remain app-owned: a
/// document preview, inspector, or editable card can occupy the node without
/// duplicating the canvas viewport math. The rectangle is clamped inside the
/// current canvas even when the node sits on an edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GraphCanvasNodeRegion {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

/// An app-owned rectangular overlay that occupies one graph node.
///
/// Cambium uses the requested size to derive the same clamped region exposed
/// by [`GraphCanvasSwatch::projected_node_footprint`], then terminates incident
/// relation routes at that region's perimeter. The overlay contents remain the
/// consumer's responsibility.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphCanvasNodeFootprint<Id> {
    pub id: Id,
    pub width: f32,
    pub height: f32,
}

/// One app-facing edge. Endpoints that are absent from the subgraph are skipped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphCanvasEdge<Id> {
    pub from: Id,
    pub to: Id,
}

/// One independently curatable relation cell in a graph-canvas Swatch.
///
/// `id` is owned by the consumer and must remain stable for the lifetime of
/// the source relation. Unlike [`GraphCanvasEdge`], it deliberately does not
/// collapse two relations that share endpoints. Visibility and emphasis are
/// projection state: they do not change graph truth.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphCanvasRelation<Id> {
    pub id: String,
    pub from: Id,
    pub to: Id,
    pub kind: String,
    pub label: String,
    /// Authored normalized route. Two points name a straight route and may be
    /// fanned when sibling relation cells share its endpoints; three or more
    /// points are preserved as an explicit polyline.
    pub route: Vec<(f32, f32)>,
    pub visible: bool,
    pub emphasized: bool,
}

/// The bounded graph data, independent of view state and palette.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphCanvasSubgraph<Id, Kind> {
    pub nodes: Vec<GraphCanvasNode<Id, Kind>>,
    pub edges: Vec<GraphCanvasEdge<Id>>,
}

/// A card-sized graph viewport. The consumer stores this beside app state,
/// rebuilds its leaf with [`GraphCanvasSwatch::paint_leaf`], and renders the
/// matching view with [`graph_canvas_swatch`].
#[derive(Clone, Debug, PartialEq)]
pub struct GraphCanvasSwatch<Id, Kind> {
    pub leaf_key: u64,
    pub graph: GraphCanvasSubgraph<Id, Kind>,
    /// Relation cells rendered in preference to the legacy endpoint-only
    /// [`GraphCanvasSubgraph::edges`] when nonempty.
    pub relations: Vec<GraphCanvasRelation<Id>>,
    /// App-owned rectangular overlays currently occupying graph nodes.
    pub node_footprints: Vec<GraphCanvasNodeFootprint<Id>>,
    pub selected: Option<Id>,
    pub focus: Option<Id>,
    pub hovered: Option<Id>,
    pub viewport: GraphViewport,
    pub width: u32,
    pub height: u32,
    pub node_radius: f32,
    pub edge_width: f32,
    pub hit_size: f32,
    pub label: String,
    /// Whether the Expand affordance renders. Defaults on (the original
    /// behavior); a consumer whose swatch has no "fuller view" to expand into
    /// switches it off rather than wiring a lying no-op chip.
    pub show_expand: bool,
    /// Whether each node's `label` also renders as visible text beside its hit
    /// target (it is always the accessible name). Defaults off — a dense
    /// minimap reads better bare; an overview where identity is the point
    /// (sessions, clusters) switches it on.
    ///
    /// Visible labels carry the same `selected` / `focused` / `hovered`
    /// modifiers as the node buttons, so a consumer can emphasize the label of
    /// the node that matters without hand-rolling a parallel label layer.
    pub show_labels: bool,
    /// Let captured `Move` events update the model used by the custom leaf
    /// without rebuilding the retained DOM until release. Defaults off because
    /// the consumer must refresh that leaf independently each presented frame.
    pub defer_drag_rebuild: bool,
}

impl<Id, Kind> GraphCanvasSwatch<Id, Kind> {
    /// Build a quiet, card-sized viewport. Dimensions remain configurable so a
    /// host can match its panel density without forking the component.
    pub fn new(leaf_key: u64, graph: GraphCanvasSubgraph<Id, Kind>) -> Self {
        Self {
            leaf_key,
            graph,
            relations: Vec::new(),
            node_footprints: Vec::new(),
            selected: None,
            focus: None,
            hovered: None,
            viewport: GraphViewport::default(),
            width: 260,
            height: 128,
            node_radius: 5.0,
            edge_width: 1.0,
            // Keep the painted dot compact while leaving a touch-sized native
            // target around it. Consumers can opt into a larger visual radius
            // without sacrificing a dependable base interaction area.
            hit_size: 44.0,
            label: "Related graph".to_string(),
            show_expand: true,
            show_labels: false,
            defer_drag_rebuild: false,
        }
    }

    #[must_use]
    pub fn with_size(mut self, width: u32, height: u32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    #[must_use]
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Show or hide the Expand affordance (see [`Self::show_expand`]).
    #[must_use]
    pub fn with_expand(mut self, on: bool) -> Self {
        self.show_expand = on;
        self
    }

    /// Render each node's label as visible text (see [`Self::show_labels`]).
    #[must_use]
    pub fn with_node_labels(mut self, on: bool) -> Self {
        self.show_labels = on;
        self
    }

    /// Paint live drag positions through the custom leaf and settle native hit
    /// targets on release instead of rebuilding them for every sampled move.
    #[must_use]
    pub fn with_deferred_drag_rebuild(mut self, on: bool) -> Self {
        self.defer_drag_rebuild = on;
        self
    }

    /// Supply identity-bearing relation cells. When present, these replace the
    /// legacy endpoint-only edges in the retained paint projection.
    #[must_use]
    pub fn with_relations(mut self, relations: Vec<GraphCanvasRelation<Id>>) -> Self {
        self.relations = relations;
        self
    }
}

impl<Id: PartialEq, Kind> GraphCanvasSwatch<Id, Kind> {
    /// Declare that an app-owned rectangular overlay occupies this node.
    ///
    /// Repeating an id replaces its earlier footprint, which makes the builder
    /// safe to apply after consumer-specific view state has been reconciled.
    #[must_use]
    pub fn with_node_footprint(mut self, id: Id, width: f32, height: f32) -> Self {
        self.node_footprints.retain(|footprint| footprint.id != id);
        self.node_footprints
            .push(GraphCanvasNodeFootprint { id, width, height });
        self
    }

    fn node_index(&self, id: Option<&Id>) -> Option<u16> {
        let id = id?;
        self.graph
            .nodes
            .iter()
            .position(|node| &node.id == id)
            .and_then(|index| u16::try_from(index).ok())
    }

    /// Build the Sprigging leaf for this viewport. Node kind is kept in the
    /// contract; the caller's palette resolves it to paint, so product-specific
    /// kinds do not leak into Cambium.
    pub fn paint_leaf(&self, color_for_kind: impl Fn(&Kind) -> ColorF) -> GraphCanvas {
        let nodes = self
            .graph
            .nodes
            .iter()
            .map(|node| GraphGlyphNode {
                x: node.position.0,
                y: node.position.1,
                color: color_for_kind(&node.kind),
            })
            .collect();
        let edges = if self.relations.is_empty() {
            self.graph
                .edges
                .iter()
                .filter_map(|edge| {
                    let from = self.node_index(Some(&edge.from))?;
                    let to = self.node_index(Some(&edge.to))?;
                    Some((from, to))
                })
                .collect()
        } else {
            Vec::new()
        };
        let mut leaf = GraphCanvas::new(
            nodes,
            edges,
            Size {
                width: self.width as f32,
                height: self.height as f32,
            },
        );
        leaf.node_radius = self.node_radius;
        leaf.edge_width = self.edge_width;
        if !self.relations.is_empty() {
            leaf.set_relations(
                self.resolved_relation_routes()
                    .into_iter()
                    .map(|(relation, points)| GraphGlyphRelation {
                        points,
                        emphasized: relation.emphasized,
                    })
                    .collect(),
            );
        }
        leaf.set_viewport(self.viewport);
        leaf.set_emphasis(
            self.node_index(self.selected.as_ref()),
            self.node_index(self.focus.as_ref()),
            self.node_index(self.hovered.as_ref()),
        );
        leaf
    }

    /// The exact leaf-local point for every node. Useful to hosts that need a
    /// second overlay beside the built-in hit targets.
    pub fn projected_positions(&self) -> Vec<(&Id, (f32, f32))> {
        let size = Size {
            width: self.width as f32,
            height: self.height as f32,
        };
        let inset = self.node_radius + self.edge_width;
        self.graph
            .nodes
            .iter()
            .map(|node| (&node.id, self.viewport.project(node.position, size, inset)))
            .collect()
    }

    /// Place a rectangular overlay around one node using the canvas's exact
    /// viewport projection. Oversized requests shrink to the canvas bounds.
    pub fn projected_node_region(
        &self,
        id: &Id,
        width: f32,
        height: f32,
    ) -> Option<GraphCanvasNodeRegion> {
        let (_, center) = self
            .projected_positions()
            .into_iter()
            .find(|(candidate, _)| *candidate == id)?;
        let canvas_width = self.width as f32;
        let canvas_height = self.height as f32;
        let width = width.max(1.0).min(canvas_width);
        let height = height.max(1.0).min(canvas_height);
        Some(GraphCanvasNodeRegion {
            left: (center.0 - width / 2.0).clamp(0.0, canvas_width - width),
            top: (center.1 - height / 2.0).clamp(0.0, canvas_height - height),
            width,
            height,
        })
    }

    /// Resolve the declared rectangular footprint for one node through this
    /// canvas's current viewport and clamping rules.
    pub fn projected_node_footprint(&self, id: &Id) -> Option<GraphCanvasNodeRegion> {
        let footprint = self
            .node_footprints
            .iter()
            .rev()
            .find(|footprint| &footprint.id == id)?;
        self.projected_node_region(id, footprint.width, footprint.height)
    }

    /// Put a visible node label across the quieter axis at that node. A
    /// horizontal lane gets labels above or below it; a vertical lane gets
    /// labels beside it. This avoids the old failure where every label sat to
    /// the right and a right-going relation was therefore painted through its
    /// source label.
    fn label_placement(&self, index: usize) -> GraphLabelPlacement {
        let Some(node) = self.graph.nodes.get(index) else {
            return GraphLabelPlacement::Right;
        };
        let mut horizontal = 0.0_f32;
        let mut vertical = 0.0_f32;
        let mut add_neighbor = |neighbor: &Id| {
            let Some(peer) = self.graph.nodes.iter().find(|peer| &peer.id == neighbor) else {
                return;
            };
            horizontal += (peer.position.0 - node.position.0).abs();
            vertical += (peer.position.1 - node.position.1).abs();
        };
        if self.relations.is_empty() {
            for edge in &self.graph.edges {
                if edge.from == node.id {
                    add_neighbor(&edge.to);
                } else if edge.to == node.id {
                    add_neighbor(&edge.from);
                }
            }
        } else {
            for relation in self.relations.iter().filter(|relation| relation.visible) {
                if relation.from == node.id {
                    add_neighbor(&relation.to);
                } else if relation.to == node.id {
                    add_neighbor(&relation.from);
                }
            }
        }

        let size = Size {
            width: self.width as f32,
            height: self.height as f32,
        };
        let position =
            self.viewport
                .project(node.position, size, self.node_radius + self.edge_width);
        if horizontal > 0.0 && horizontal >= vertical {
            if position.1 >= self.height as f32 * 0.28 {
                GraphLabelPlacement::Above
            } else {
                GraphLabelPlacement::Below
            }
        } else if vertical > 0.0 {
            if position.0 <= self.width as f32 * 0.72 {
                GraphLabelPlacement::Right
            } else {
                GraphLabelPlacement::Left
            }
        } else if position.0 <= self.width as f32 * 0.72 {
            GraphLabelPlacement::Right
        } else {
            GraphLabelPlacement::Left
        }
    }

    /// Visible relation cells with their projected polylines. This is shared by
    /// retained paint and native relation targets, so a relation is neither
    /// hittable when hidden nor represented at stale endpoint geometry.
    pub fn projected_relations(&self) -> Vec<(&GraphCanvasRelation<Id>, Vec<(f32, f32)>)> {
        let size = Size {
            width: self.width as f32,
            height: self.height as f32,
        };
        let inset = self.node_radius + self.edge_width;
        self.resolved_relation_routes()
            .into_iter()
            .map(|(relation, route)| {
                (
                    relation,
                    route
                        .into_iter()
                        .map(|point| self.viewport.project(point, size, inset))
                        .collect(),
                )
            })
            .collect()
    }

    /// Apply view-local node footprints to the normalized relation routes.
    /// Clipping happens in leaf pixels so the perimeter remains exact under a
    /// non-square canvas, pan, or zoom, then returns to graph coordinates for
    /// the shared Sprigging paint path.
    fn resolved_relation_routes(&self) -> Vec<(&GraphCanvasRelation<Id>, Vec<(f32, f32)>)> {
        let size = Size {
            width: self.width as f32,
            height: self.height as f32,
        };
        let inset = self.node_radius + self.edge_width;
        self.relation_routes()
            .into_iter()
            .map(|(relation, route)| {
                let from_region = self.projected_node_footprint(&relation.from);
                let to_region = self.projected_node_footprint(&relation.to);
                if from_region.is_none() && to_region.is_none() {
                    return (relation, route);
                }
                let mut projected = route
                    .into_iter()
                    .map(|point| self.viewport.project(point, size, inset))
                    .collect::<Vec<_>>();
                if projected.len() >= 2 {
                    if let Some(region) = from_region {
                        let center = self.projected_position(&relation.from);
                        if let Some(center) = center {
                            projected[0] = rectangle_ray_exit(center, projected[1], region);
                        }
                    }
                    if let Some(region) = to_region {
                        let center = self.projected_position(&relation.to);
                        if let Some(center) = center {
                            let last = projected.len() - 1;
                            projected[last] =
                                rectangle_ray_exit(center, projected[last - 1], region);
                        }
                    }
                }
                let route = projected
                    .into_iter()
                    .map(|point| graph_position_at_unclamped(self.viewport, size, inset, point))
                    .collect();
                (relation, route)
            })
            .collect()
    }

    fn projected_position(&self, id: &Id) -> Option<(f32, f32)> {
        let node = self.graph.nodes.iter().find(|node| &node.id == id)?;
        Some(self.viewport.project(
            node.position,
            Size {
                width: self.width as f32,
                height: self.height as f32,
            },
            self.node_radius + self.edge_width,
        ))
    }

    /// Resolve authored routes and endpoint-only relation cells into distinct
    /// normalized polylines. Parallel cells fan around their shared chord;
    /// authored routes with a real interior are left alone.
    fn relation_routes(&self) -> Vec<(&GraphCanvasRelation<Id>, Vec<(f32, f32)>)> {
        const FAN_GAP_PX: f32 = 12.0;
        let visible = self
            .relations
            .iter()
            .filter(|relation| relation.visible)
            .filter_map(|relation| {
                let from = self
                    .graph
                    .nodes
                    .iter()
                    .position(|node| node.id == relation.from)?;
                let to = self
                    .graph
                    .nodes
                    .iter()
                    .position(|node| node.id == relation.to)?;
                Some((relation, from, to))
            })
            .collect::<Vec<_>>();

        visible
            .iter()
            .enumerate()
            .map(|(visible_index, (relation, from, to))| {
                if relation.route.len() > 2 {
                    return (*relation, relation.route.clone());
                }

                let a = self.graph.nodes[*from].position;
                let b = self.graph.nodes[*to].position;
                if from == to {
                    let rank = visible[..visible_index]
                        .iter()
                        .filter(|(_, peer_from, peer_to)| peer_from == from && peer_to == to)
                        .count() as f32;
                    let radius = 0.08 + rank * 0.035;
                    return (
                        *relation,
                        vec![
                            a,
                            (a.0 + radius, a.1 - radius),
                            (a.0 + radius * 1.35, a.1 + radius),
                            a,
                        ],
                    );
                }

                let same_pair = |peer_from: usize, peer_to: usize| {
                    (peer_from == *from && peer_to == *to) || (peer_from == *to && peer_to == *from)
                };
                let peers = visible
                    .iter()
                    .filter(|(_, peer_from, peer_to)| same_pair(*peer_from, *peer_to))
                    .collect::<Vec<_>>();
                if peers.len() == 1 {
                    return (*relation, vec![a, b]);
                }
                let rank = peers
                    .iter()
                    .position(|(peer, _, _)| std::ptr::eq(*peer, *relation))
                    .unwrap_or_default() as f32;
                let offset = (rank - (peers.len() as f32 - 1.0) * 0.5) * FAN_GAP_PX;
                let size = Size {
                    width: self.width as f32,
                    height: self.height as f32,
                };
                let inset = self.node_radius + self.edge_width;
                let a_px = self.viewport.project(a, size, inset);
                let b_px = self.viewport.project(b, size, inset);
                let dx = b_px.0 - a_px.0;
                let dy = b_px.1 - a_px.1;
                let length = (dx * dx + dy * dy).sqrt().max(0.0001);
                let normal = (-dy / length * offset, dx / length * offset);
                let first = (a_px.0 + dx * 0.28 + normal.0, a_px.1 + dy * 0.28 + normal.1);
                let second = (a_px.0 + dx * 0.72 + normal.0, a_px.1 + dy * 0.72 + normal.1);
                (
                    *relation,
                    vec![
                        a,
                        graph_position_at_unclamped(self.viewport, size, inset, first),
                        graph_position_at_unclamped(self.viewport, size, inset, second),
                        b,
                    ],
                )
            })
            .collect()
    }

    /// Convert a leaf-local pointer position back into the graph's normalized
    /// coordinate system. This is the inverse of the projection used by paint
    /// and native node targets, clamped to the graph's visible bounds so a
    /// captured drag cannot create an unreachable position.
    pub fn graph_position_at(&self, local: (f32, f32)) -> (f32, f32) {
        graph_position_at(
            self.viewport,
            Size {
                width: self.width as f32,
                height: self.height as f32,
            },
            self.node_radius + self.edge_width,
            local,
        )
    }
}

/// Follow a ray from a point inside a rectangle to the first perimeter it
/// exits. A clamped node can already sit on that perimeter, in which case the
/// unchanged origin is the truthful endpoint.
fn rectangle_ray_exit(
    origin: (f32, f32),
    toward: (f32, f32),
    region: GraphCanvasNodeRegion,
) -> (f32, f32) {
    const EPSILON: f32 = 0.0001;
    let dx = toward.0 - origin.0;
    let dy = toward.1 - origin.1;
    if dx.abs() < EPSILON && dy.abs() < EPSILON {
        return origin;
    }

    let right = region.left + region.width;
    let bottom = region.top + region.height;
    let tx = if dx > EPSILON {
        (right - origin.0) / dx
    } else if dx < -EPSILON {
        (region.left - origin.0) / dx
    } else {
        f32::INFINITY
    };
    let ty = if dy > EPSILON {
        (bottom - origin.1) / dy
    } else if dy < -EPSILON {
        (region.top - origin.1) / dy
    } else {
        f32::INFINITY
    };
    let distance = tx.max(0.0).min(ty.max(0.0));
    if !distance.is_finite() {
        return origin;
    }
    (
        (origin.0 + dx * distance).clamp(region.left, right),
        (origin.1 + dy * distance).clamp(region.top, bottom),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GraphLabelPlacement {
    Above,
    Below,
    Left,
    Right,
}

impl GraphLabelPlacement {
    fn name(self) -> &'static str {
        match self {
            Self::Above => "above",
            Self::Below => "below",
            Self::Left => "left",
            Self::Right => "right",
        }
    }

    fn style(self, x: f32, y: f32, clearance: f32, canvas: Size) -> String {
        const LABEL_WIDTH: f32 = 160.0;
        const LABEL_HEIGHT: f32 = 14.0;
        let centred_left =
            (x - LABEL_WIDTH * 0.5).clamp(0.0, (canvas.width - LABEL_WIDTH).max(0.0));
        let centred_top =
            (y - LABEL_HEIGHT * 0.5).clamp(0.0, (canvas.height - LABEL_HEIGHT).max(0.0));
        match self {
            Self::Above => format!(
                "position:absolute;left:{centred_left}px;top:{}px;width:{LABEL_WIDTH}px;text-align:center;overflow:hidden;text-overflow:ellipsis;",
                (y - clearance - LABEL_HEIGHT).max(0.0)
            ),
            Self::Below => format!(
                "position:absolute;left:{centred_left}px;top:{}px;width:{LABEL_WIDTH}px;text-align:center;overflow:hidden;text-overflow:ellipsis;",
                (y + clearance).min((canvas.height - LABEL_HEIGHT).max(0.0))
            ),
            Self::Left => {
                let width = (x - clearance).clamp(0.0, LABEL_WIDTH);
                format!(
                    "position:absolute;left:{}px;top:{centred_top}px;width:{width}px;text-align:right;overflow:hidden;text-overflow:ellipsis;",
                    (x - clearance - width).max(0.0)
                )
            },
            Self::Right => {
                let left = (x + clearance).min(canvas.width);
                let width = (canvas.width - left).clamp(0.0, LABEL_WIDTH);
                format!(
                    "position:absolute;left:{left}px;top:{centred_top}px;width:{width}px;text-align:left;overflow:hidden;text-overflow:ellipsis;"
                )
            },
        }
    }
}

fn graph_position_at(
    viewport: GraphViewport,
    size: Size,
    inset: f32,
    local: (f32, f32),
) -> (f32, f32) {
    let width = (size.width - 2.0 * inset).max(0.0);
    let height = (size.height - 2.0 * inset).max(0.0);
    let zoom = viewport.zoom.max(0.01);
    let x = if width > 0.0 {
        (local.0 - inset) / width
    } else {
        0.5
    };
    let y = if height > 0.0 {
        (local.1 - inset) / height
    } else {
        0.5
    };
    (
        ((x - 0.5 - viewport.pan.0) / zoom + 0.5).clamp(0.0, 1.0),
        ((y - 0.5 - viewport.pan.1) / zoom + 0.5).clamp(0.0, 1.0),
    )
}

fn graph_position_at_unclamped(
    viewport: GraphViewport,
    size: Size,
    inset: f32,
    local: (f32, f32),
) -> (f32, f32) {
    let width = (size.width - 2.0 * inset).max(0.0);
    let height = (size.height - 2.0 * inset).max(0.0);
    let zoom = viewport.zoom.max(0.01);
    let x = if width > 0.0 {
        (local.0 - inset) / width
    } else {
        0.5
    };
    let y = if height > 0.0 {
        (local.1 - inset) / height
    } else {
        0.5
    };
    (
        (x - 0.5 - viewport.pan.0) / zoom + 0.5,
        (y - 0.5 - viewport.pan.1) / zoom + 0.5,
    )
}

/// The segment crossing a polyline's half-length point. Relation hit targets
/// sit on this segment, which keeps parallel fanned cells independently
/// reachable instead of stacking every target on the shared endpoint chord.
fn route_midpoint_segment(points: &[(f32, f32)]) -> Option<((f32, f32), (f32, f32))> {
    let segments = points
        .windows(2)
        .map(|pair| {
            let dx = pair[1].0 - pair[0].0;
            let dy = pair[1].1 - pair[0].1;
            (pair[0], pair[1], (dx * dx + dy * dy).sqrt())
        })
        .collect::<Vec<_>>();
    let total = segments.iter().map(|(_, _, length)| *length).sum::<f32>();
    let mut walked = 0.0;
    for (from, to, length) in &segments {
        if walked + length >= total * 0.5 {
            return Some((*from, *to));
        }
        walked += length;
    }
    segments.last().map(|(from, to, _)| (*from, *to))
}

/// Render a bounded graph canvas with one native node target per painted node.
///
/// `on_node_click` owns navigation or staging. `on_node_hover` normally writes
/// the supplied id into [`GraphCanvasSwatch::hovered`] (and clears it on leave),
/// after which the host refreshes the registered leaf. `on_expand` switches to
/// the app's full-canvas route. The component does not invent those policies.
pub fn graph_canvas_swatch<State, AppAction, Id, Kind, Click, ClickOut, Hover, Expand, ExpandOut>(
    swatch: &GraphCanvasSwatch<Id, Kind>,
    on_node_click: Click,
    on_node_hover: Hover,
    on_expand: Expand,
) -> impl View<State, AppAction, GenetCtx, Element = GenetElement>
where
    State: 'static,
    AppAction: 'static,
    Id: Clone + PartialEq + 'static,
    ClickOut: OptionalAction<AppAction>,
    Click: Fn(&mut State, Id) -> ClickOut + Clone + 'static,
    Hover: Fn(&mut State, Option<Id>) + Clone + 'static,
    ExpandOut: OptionalAction<AppAction>,
    Expand: Fn(&mut State) -> ExpandOut + Clone + 'static,
{
    graph_canvas_swatch_with_focus_and_drag_and_relations(
        swatch,
        on_node_click,
        on_node_hover,
        |_state: &mut State, _id: Option<Id>| {},
        |_state: &mut State, _event: GraphCanvasNodeDrag<Id>| {},
        |_state: &mut State, _id: String| {},
        |_state: &mut State, _id: Option<String>| {},
        on_expand,
        |_state: &mut State, _event: PointerEvent| {},
        |_state: &mut State, _event: WheelEvent| {},
    )
}

/// Render a graph-canvas Swatch with captured node motion. This is the
/// interaction variant of [`graph_canvas_swatch`]: click and hover retain their
/// existing meanings while `on_node_drag` receives Down/Move/Up in normalized
/// graph coordinates.
pub fn graph_canvas_swatch_with_drag<
    State,
    AppAction,
    Id,
    Kind,
    Click,
    ClickOut,
    Hover,
    Drag,
    DragOut,
    Expand,
    ExpandOut,
>(
    swatch: &GraphCanvasSwatch<Id, Kind>,
    on_node_click: Click,
    on_node_hover: Hover,
    on_node_drag: Drag,
    on_expand: Expand,
) -> impl View<State, AppAction, GenetCtx, Element = GenetElement>
where
    State: 'static,
    AppAction: 'static,
    Id: Clone + PartialEq + 'static,
    ClickOut: OptionalAction<AppAction>,
    Click: Fn(&mut State, Id) -> ClickOut + Clone + 'static,
    Hover: Fn(&mut State, Option<Id>) + Clone + 'static,
    DragOut: OptionalAction<AppAction>,
    Drag: Fn(&mut State, GraphCanvasNodeDrag<Id>) -> DragOut + Clone + 'static,
    ExpandOut: OptionalAction<AppAction>,
    Expand: Fn(&mut State) -> ExpandOut + Clone + 'static,
{
    graph_canvas_swatch_with_focus_and_drag_and_relations(
        swatch,
        on_node_click,
        on_node_hover,
        |_state: &mut State, _id: Option<Id>| {},
        on_node_drag,
        |_state: &mut State, _id: String| {},
        |_state: &mut State, _id: Option<String>| {},
        on_expand,
        |_state: &mut State, _event: PointerEvent| {},
        |_state: &mut State, _event: WheelEvent| {},
    )
}

/// Render a graph-canvas swatch whose retained paint focus follows the native
/// node button that pointer, keyboard, or programmatic focus selected.
///
/// `on_node_focus` normally writes its value into
/// [`GraphCanvasSwatch::focus`] before rebuilding the matching paint leaf.
pub fn graph_canvas_swatch_with_focus<
    State,
    AppAction,
    Id,
    Kind,
    Click,
    ClickOut,
    Hover,
    Focus,
    Expand,
    ExpandOut,
>(
    swatch: &GraphCanvasSwatch<Id, Kind>,
    on_node_click: Click,
    on_node_hover: Hover,
    on_node_focus: Focus,
    on_expand: Expand,
) -> impl View<State, AppAction, GenetCtx, Element = GenetElement>
where
    State: 'static,
    AppAction: 'static,
    Id: Clone + PartialEq + 'static,
    ClickOut: OptionalAction<AppAction>,
    Click: Fn(&mut State, Id) -> ClickOut + Clone + 'static,
    Hover: Fn(&mut State, Option<Id>) + Clone + 'static,
    Focus: Fn(&mut State, Option<Id>) + Clone + 'static,
    ExpandOut: OptionalAction<AppAction>,
    Expand: Fn(&mut State) -> ExpandOut + Clone + 'static,
{
    graph_canvas_swatch_with_focus_and_drag_and_relations(
        swatch,
        on_node_click,
        on_node_hover,
        on_node_focus,
        |_state: &mut State, _event: GraphCanvasNodeDrag<Id>| {},
        |_state: &mut State, _id: String| {},
        |_state: &mut State, _id: Option<String>| {},
        on_expand,
        |_state: &mut State, _event: PointerEvent| {},
        |_state: &mut State, _event: WheelEvent| {},
    )
}

/// Render a graph-canvas Swatch with captured node motion and independently
/// targetable relation cells. Relation callbacks receive a consumer-owned
/// stable relation id, so equal endpoint pairs remain separate cells.
pub fn graph_canvas_swatch_with_drag_and_relations<
    State,
    AppAction,
    Id,
    Kind,
    Click,
    ClickOut,
    Hover,
    Drag,
    DragOut,
    RelationClick,
    RelationClickOut,
    RelationHover,
    Expand,
    ExpandOut,
>(
    swatch: &GraphCanvasSwatch<Id, Kind>,
    on_node_click: Click,
    on_node_hover: Hover,
    on_node_drag: Drag,
    on_relation_click: RelationClick,
    on_relation_hover: RelationHover,
    on_expand: Expand,
) -> impl View<State, AppAction, GenetCtx, Element = GenetElement>
where
    State: 'static,
    AppAction: 'static,
    Id: Clone + PartialEq + 'static,
    ClickOut: OptionalAction<AppAction>,
    Click: Fn(&mut State, Id) -> ClickOut + Clone + 'static,
    Hover: Fn(&mut State, Option<Id>) + Clone + 'static,
    DragOut: OptionalAction<AppAction>,
    Drag: Fn(&mut State, GraphCanvasNodeDrag<Id>) -> DragOut + Clone + 'static,
    RelationClickOut: OptionalAction<AppAction>,
    RelationClick: Fn(&mut State, String) -> RelationClickOut + Clone + 'static,
    RelationHover: Fn(&mut State, Option<String>) + Clone + 'static,
    ExpandOut: OptionalAction<AppAction>,
    Expand: Fn(&mut State) -> ExpandOut + Clone + 'static,
{
    graph_canvas_swatch_with_focus_and_drag_and_relations(
        swatch,
        on_node_click,
        on_node_hover,
        |_state: &mut State, _id: Option<Id>| {},
        on_node_drag,
        on_relation_click,
        on_relation_hover,
        on_expand,
        |_state: &mut State, _event: PointerEvent| {},
        |_state: &mut State, _event: WheelEvent| {},
    )
}

/// Render a graph-canvas Swatch with focus and captured node-motion callbacks.
///
/// The component emits motion only; the consumer keeps ownership of position,
/// solver, and persistence policy. Existing callers can keep using
/// [`graph_canvas_swatch`] or [`graph_canvas_swatch_with_focus`].
pub fn graph_canvas_swatch_with_focus_and_drag<
    State,
    AppAction,
    Id,
    Kind,
    Click,
    ClickOut,
    Hover,
    Focus,
    Drag,
    DragOut,
    Expand,
    ExpandOut,
>(
    swatch: &GraphCanvasSwatch<Id, Kind>,
    on_node_click: Click,
    on_node_hover: Hover,
    on_node_focus: Focus,
    on_node_drag: Drag,
    on_expand: Expand,
) -> impl View<State, AppAction, GenetCtx, Element = GenetElement>
where
    State: 'static,
    AppAction: 'static,
    Id: Clone + PartialEq + 'static,
    ClickOut: OptionalAction<AppAction>,
    Click: Fn(&mut State, Id) -> ClickOut + Clone + 'static,
    Hover: Fn(&mut State, Option<Id>) + Clone + 'static,
    Focus: Fn(&mut State, Option<Id>) + Clone + 'static,
    DragOut: OptionalAction<AppAction>,
    Drag: Fn(&mut State, GraphCanvasNodeDrag<Id>) -> DragOut + Clone + 'static,
    ExpandOut: OptionalAction<AppAction>,
    Expand: Fn(&mut State) -> ExpandOut + Clone + 'static,
{
    graph_canvas_swatch_with_focus_and_drag_and_relations(
        swatch,
        on_node_click,
        on_node_hover,
        on_node_focus,
        on_node_drag,
        |_state: &mut State, _id: String| {},
        |_state: &mut State, _id: Option<String>| {},
        on_expand,
        |_state: &mut State, _event: PointerEvent| {},
        |_state: &mut State, _event: WheelEvent| {},
    )
}

/// The full graph-canvas interaction surface, including independently
/// targetable relation cells.
pub fn graph_canvas_swatch_with_focus_and_drag_and_relations<
    State,
    AppAction,
    Id,
    Kind,
    Click,
    ClickOut,
    Hover,
    Focus,
    Drag,
    DragOut,
    RelationClick,
    RelationClickOut,
    RelationHover,
    Expand,
    ExpandOut,
    Pan,
    PanOut,
    Zoom,
    ZoomOut,
>(
    swatch: &GraphCanvasSwatch<Id, Kind>,
    on_node_click: Click,
    on_node_hover: Hover,
    on_node_focus: Focus,
    on_node_drag: Drag,
    on_relation_click: RelationClick,
    on_relation_hover: RelationHover,
    on_expand: Expand,
    on_background_pointer: Pan,
    on_wheel_event: Zoom,
) -> impl View<State, AppAction, GenetCtx, Element = GenetElement>
where
    State: 'static,
    AppAction: 'static,
    Id: Clone + PartialEq + 'static,
    // The activating handlers may emit an action (returning `()` keeps them
    // silent, which is what every state-mutating consumer does). The hover and
    // focus handlers stay silent by construction: emphasis is presentation
    // state, never an app-facing event.
    ClickOut: OptionalAction<AppAction>,
    Click: Fn(&mut State, Id) -> ClickOut + Clone + 'static,
    Hover: Fn(&mut State, Option<Id>) + Clone + 'static,
    Focus: Fn(&mut State, Option<Id>) + Clone + 'static,
    DragOut: OptionalAction<AppAction>,
    Drag: Fn(&mut State, GraphCanvasNodeDrag<Id>) -> DragOut + Clone + 'static,
    RelationClickOut: OptionalAction<AppAction>,
    RelationClick: Fn(&mut State, String) -> RelationClickOut + Clone + 'static,
    RelationHover: Fn(&mut State, Option<String>) + Clone + 'static,
    ExpandOut: OptionalAction<AppAction>,
    Expand: Fn(&mut State) -> ExpandOut + Clone + 'static,
    // The background pointer and wheel handlers receive the raw events: pan
    // anchoring and zoom stepping are caller policy ([`graph_canvas`] turns
    // them into [`GraphCanvasEvent::Pan`] / [`GraphCanvasEvent::Zoom`]).
    // Returning `()` keeps a caller that has no viewport events silent.
    PanOut: OptionalAction<AppAction>,
    Pan: Fn(&mut State, PointerEvent) -> PanOut + Clone + 'static,
    ZoomOut: OptionalAction<AppAction>,
    Zoom: Fn(&mut State, WheelEvent) -> ZoomOut + Clone + 'static,
{
    let positions = swatch.projected_positions();
    let hit_size = swatch.hit_size.max(1.0);
    let size = Size {
        width: swatch.width as f32,
        height: swatch.height as f32,
    };
    let inset = swatch.node_radius + swatch.edge_width;
    let viewport = swatch.viewport;
    let defer_drag_rebuild = swatch.defer_drag_rebuild;
    // Visible node labels (opt-in): plain positioned text beside each node,
    // aria-hidden (the button already carries the accessible name) and
    // pointer-transparent (they must not steal the node's clicks).
    let labels: Vec<_> = if swatch.show_labels {
        swatch
            .graph
            .nodes
            .iter()
            .zip(swatch.projected_positions())
            .enumerate()
            .map(|(index, (node, (_, (x, y))))| {
                // Same state modifiers the node buttons carry. A label layer
                // that cannot say which node is selected forces a consumer to
                // keep hand-rolling the whole layer for the sake of one
                // emphasis, which is what Isometry's pointcrawl was doing: the
                // party's location is the first thing that layer has to say.
                let mut class = String::from("graph-canvas-swatch-label");
                if swatch.selected.as_ref() == Some(&node.id) {
                    class.push_str(" selected");
                }
                if swatch.focus.as_ref() == Some(&node.id) {
                    class.push_str(" focused");
                }
                if swatch.hovered.as_ref() == Some(&node.id) {
                    class.push_str(" hovered");
                }
                let placement = swatch.label_placement(index);
                el::<_, State, AppAction>("span", node.label.clone())
                    .attr("class", class)
                    .attr("aria-hidden", "true")
                    .attr("data-label-placement", placement.name())
                    .attr(
                        "style",
                        placement.style(x, y, swatch.node_radius + 7.0, size),
                    )
            })
            .collect()
    } else {
        Vec::new()
    };
    let relation_targets: Vec<_> = swatch
        .projected_relations()
        .into_iter()
        .filter_map(|(relation, route)| {
            let ((from_x, from_y), (to_x, to_y)) = route_midpoint_segment(&route)?;
            let dx = to_x - from_x;
            let dy = to_y - from_y;
            let length = (dx * dx + dy * dy).sqrt().max(1.0);
            let midpoint_x = (from_x + to_x) / 2.0;
            let midpoint_y = (from_y + to_y) / 2.0;
            let angle = dy.atan2(dx).to_degrees();
            let mut class = String::from("graph-canvas-swatch-relation");
            if relation.emphasized {
                class.push_str(" emphasized");
            }
            let mut target = el::<_, State, AppAction>("button", ())
                .attr("class", class)
                .attr("type", "button")
                .attr("aria-label", format!("{}: {}", relation.kind, relation.label))
                .attr("data-relation-id", relation.id.clone())
                .attr(
                    "style",
                    format!(
                        "position:absolute;left:{}px;top:{}px;width:{length}px;height:20px;transform:rotate({angle}deg);",
                        midpoint_x - length / 2.0,
                        midpoint_y - 10.0,
                    ),
                );
            if relation.emphasized {
                target = target.attr("aria-current", "true");
            }
            let click = on_relation_click.clone();
            let click_id = relation.id.clone();
            let hover = on_relation_hover.clone();
            let hover_id = relation.id.clone();
            Some(on_hover(
                on_click(target, move |state: &mut State, _: PointerClick| {
                    click(state, click_id.clone())
                }),
                move |state: &mut State, event: HoverEvent| match event.phase {
                    HoverPhase::Enter => hover(state, Some(hover_id.clone())),
                    HoverPhase::Leave => hover(state, None),
                    HoverPhase::Move => {}
                },
            ))
        })
        .collect();
    let targets: Vec<_> = swatch
        .graph
        .nodes
        .iter()
        .zip(positions)
        .enumerate()
        .map(|(index, (node, (_, (x, y))))| {
            let selected = swatch.selected.as_ref() == Some(&node.id);
            let focused = swatch.focus.as_ref() == Some(&node.id);
            let hovered = swatch.hovered.as_ref() == Some(&node.id);
            let mut class = String::from("graph-canvas-swatch-node");
            if selected {
                class.push_str(" selected");
            }
            if focused {
                class.push_str(" focused");
            }
            if hovered {
                class.push_str(" hovered");
            }
            let mut target = el::<_, State, AppAction>("button", ())
                .attr("class", class)
                .attr("type", "button")
                .attr("aria-label", node.label.clone())
                .attr("data-node-index", index.to_string())
                .attr(
                    "style",
                    format!(
                        "position:absolute;left:{}px;top:{}px;width:{hit_size}px;height:{hit_size}px;",
                        x - hit_size / 2.0,
                        y - hit_size / 2.0,
                    ),
                );
            if selected {
                target = target.attr("aria-current", "true");
            }
            if let Some(key) = &node.key {
                target = target.attr("data-key", key.clone());
            }

            let click = on_node_click.clone();
            let click_id = node.id.clone();
            let hover = on_node_hover.clone();
            let enter_id = node.id.clone();
            let focus = on_node_focus.clone();
            let focus_id = node.id.clone();
            let drag = on_node_drag.clone();
            let drag_id = node.id.clone();
            let target_left = x - hit_size / 2.0;
            let target_top = y - hit_size / 2.0;
            on_pointer(focusable(on_focus(
                on_hover(
                    on_click(target, move |state: &mut State, _: PointerClick| {
                        click(state, click_id.clone())
                    }),
                    move |state: &mut State, event: HoverEvent| match event.phase {
                        HoverPhase::Enter => hover(state, Some(enter_id.clone())),
                        HoverPhase::Leave => hover(state, None),
                        HoverPhase::Move => {}
                    },
                ),
                move |state: &mut State, event: FocusEvent| match event.phase {
                    FocusPhase::Gained => focus(state, Some(focus_id.clone())),
                    FocusPhase::Lost => focus(state, None),
                },
            )), move |state: &mut State, event: PointerEvent| {
                if defer_drag_rebuild && matches!(event.phase, PointerPhase::Move) {
                    event.defer_rebuild();
                }
                let position = graph_position_at(
                    viewport,
                    size,
                    inset,
                    (target_left + event.local.0, target_top + event.local.1),
                );
                drag(
                    state,
                    GraphCanvasNodeDrag {
                        id: drag_id.clone(),
                        phase: event.phase,
                        position,
                    },
                )
            })
        })
        .collect();

    let expand = on_expand.clone();
    let expand_buttons: Vec<_> = if swatch.show_expand {
        vec![on_click(
            el::<_, State, AppAction>("button", "Expand")
                .attr("class", "graph-canvas-swatch-expand")
                .attr("type", "button")
                .attr("aria-label", "Expand graph")
                .attr("style", "position:absolute;right:5px;top:5px;"),
            move |state: &mut State, _: PointerClick| expand(state),
        )]
    } else {
        Vec::new()
    };

    let root = el(
        "div",
        (
            custom_leaf::<State, AppAction>(swatch.leaf_key, swatch.width, swatch.height)
                .attr("aria-hidden", "true"),
            el("div", labels)
                .attr("class", "graph-canvas-swatch-labels")
                .attr(
                    "style",
                    format!(
                        "position:absolute;left:0;top:0;width:{}px;height:{}px;",
                        swatch.width, swatch.height
                    ),
                ),
            el("div", relation_targets)
                .attr("class", "graph-canvas-swatch-relation-targets")
                .attr(
                    "style",
                    format!(
                        "position:absolute;left:0;top:0;width:{}px;height:{}px;",
                        swatch.width, swatch.height
                    ),
                ),
            el("div", targets)
                .attr("class", "graph-canvas-swatch-targets")
                .attr(
                    "style",
                    format!(
                        "position:absolute;left:0;top:0;width:{}px;height:{}px;",
                        swatch.width, swatch.height
                    ),
                ),
            expand_buttons,
        ),
    )
    .attr("class", "graph-canvas-swatch")
    .attr("role", "group")
    .attr("aria-label", swatch.label.clone())
    // `overflow:hidden` crops the native node, label, and relation targets a
    // panned or zoomed viewport projects outside this box; the swatch is their
    // `position:relative` containing block, so the clip applies to them. The
    // painted leaf clips itself (see sprigging's GraphCanvas paint).
    .attr(
        "style",
        format!(
            "position:relative;display:block;overflow:hidden;width:{}px;height:{}px;max-width:100%;",
            swatch.width, swatch.height
        ),
    );
    // Background drags and wheel notches reach these root handlers only when
    // no inner target claims the event: a node's own pointer handler is the
    // innermost element under a node drag, so panning never fights dragging.
    on_wheel(on_pointer(root, on_background_pointer), on_wheel_event)
}

/// What a [`graph_canvas`] reports to its parent. Hover, focus, and relation
/// emphasis are deliberately absent: they are presentation state the component
/// owns, not events an application needs.
#[derive(Clone, Debug, PartialEq)]
pub enum GraphCanvasEvent<Id> {
    /// A node's hit target was activated.
    Activate(Id),
    /// The Expand affordance was activated.
    Expand,
    /// A node was dragged. `position` is in normalized graph coordinates.
    Drag(GraphCanvasNodeDrag<Id>),
    /// A relation cell was activated.
    RelationActivate(String),
    /// The canvas background was dragged. `delta` is in viewport pan units —
    /// the fraction of the canvas the pointer moved on each axis — so a
    /// consumer applies it to [`GraphViewport::pan`] directly. The component
    /// does not mutate the viewport itself: the viewport stays consumer-owned
    /// state, exactly like selection.
    Pan { delta: (f32, f32) },
    /// The wheel turned over the canvas. Multiply the consumer's zoom by
    /// `factor` (greater than one zooms in).
    Zoom { factor: f32 },
}

impl<Id> crate::Action for GraphCanvasEvent<Id> {}

/// Component-owned interaction state for [`graph_canvas`].
struct GraphCanvasLocal<Id> {
    /// Background-drag anchor for pointer panning: the last pointer position,
    /// leaf-local device px, while a background drag is held.
    pan_anchor: Option<(f32, f32)>,
    hovered: Option<Id>,
    focus: Option<Id>,
    hovered_relation: Option<String>,
}

/// A graph-canvas Swatch that owns its own emphasis.
///
/// Selection stays application truth and arrives on the `swatch` props (a node
/// is selected because the graph says so). Hover, keyboard focus emphasis, and
/// relation hover are pointer-and-focus presentation state, so they live in the
/// component: an application no longer stores a hover field purely to route it
/// back into the view on the next rebuild. The `hovered` and `focus` fields of
/// the passed `swatch` are ignored for that reason.
///
/// The parent receives only [`GraphCanvasEvent`]s. For the callback-per-axis
/// form (an application that genuinely wants to own emphasis, e.g. to mirror it
/// across two views), the `graph_canvas_swatch*` family remains. Woodshed's
/// Related panel is that case: its hover is shared with a neighbor list, and
/// the two cross-highlight.
///
/// This is a state-ownership boundary, not a compile-time one. Measured with
/// `cargo llvm-lines`, the swatch costs the same either way (~1.6k lines, 11
/// instantiations); the boundary moves that instantiation from the app's state
/// type onto `GraphCanvasLocal<Id>` rather than removing it.
pub fn graph_canvas<State, A, Output, Id, Kind, F>(
    swatch: &GraphCanvasSwatch<Id, Kind>,
    on_event: F,
) -> impl View<State, A, GenetCtx, Element = GenetElement> + use<State, A, Output, Id, Kind, F>
where
    State: 'static,
    A: 'static,
    Output: OptionalAction<A> + 'static,
    Id: Clone + PartialEq + 'static,
    Kind: Clone + PartialEq + 'static,
    F: Fn(&mut State, GraphCanvasEvent<Id>) -> Output + 'static,
{
    component(
        swatch.clone(),
        |_props: &GraphCanvasSwatch<Id, Kind>| GraphCanvasLocal {
            pan_anchor: None,
            hovered: None,
            focus: None,
            hovered_relation: None,
        },
        // Emphasis is never parent-controlled: a props change (new graph data,
        // a new selection) leaves the pointer where it is.
        |_prev: &GraphCanvasSwatch<Id, Kind>,
         _next: &GraphCanvasSwatch<Id, Kind>,
         _local: &mut GraphCanvasLocal<Id>| {},
        |props: &GraphCanvasSwatch<Id, Kind>, local: &GraphCanvasLocal<Id>| {
            let mut swatch = props.clone();
            swatch.hovered = local.hovered.clone();
            swatch.focus = local.focus.clone();
            if let Some(id) = &local.hovered_relation {
                for relation in &mut swatch.relations {
                    if &relation.id == id {
                        relation.emphasized = true;
                    }
                }
            }
            Box::new(graph_canvas_swatch_with_focus_and_drag_and_relations(
                &swatch,
                |_: &mut GraphCanvasLocal<Id>, id: Id| GraphCanvasEvent::Activate(id),
                |local: &mut GraphCanvasLocal<Id>, id: Option<Id>| local.hovered = id,
                |local: &mut GraphCanvasLocal<Id>, id: Option<Id>| local.focus = id,
                |_: &mut GraphCanvasLocal<Id>, drag: GraphCanvasNodeDrag<Id>| {
                    GraphCanvasEvent::Drag(drag)
                },
                |_: &mut GraphCanvasLocal<Id>, id: String| GraphCanvasEvent::RelationActivate(id),
                |local: &mut GraphCanvasLocal<Id>, id: Option<String>| {
                    local.hovered_relation = id;
                },
                |_: &mut GraphCanvasLocal<Id>| GraphCanvasEvent::Expand,
                {
                    let width = (props.width as f32).max(1.0);
                    let height = (props.height as f32).max(1.0);
                    move |local: &mut GraphCanvasLocal<Id>, event: PointerEvent| match event.phase {
                        PointerPhase::Down => {
                            local.pan_anchor = Some(event.local);
                            None
                        },
                        PointerPhase::Move => {
                            let Some(anchor) = local.pan_anchor else {
                                return None;
                            };
                            let delta = (event.local.0 - anchor.0, event.local.1 - anchor.1);
                            local.pan_anchor = Some(event.local);
                            if delta == (0.0, 0.0) {
                                return None;
                            }
                            Some(GraphCanvasEvent::Pan {
                                delta: (delta.0 / width, delta.1 / height),
                            })
                        },
                        PointerPhase::Up => {
                            local.pan_anchor = None;
                            None
                        },
                    }
                },
                |_: &mut GraphCanvasLocal<Id>, event: WheelEvent| {
                    // A notch toward the top zooms in, matching every map
                    // surface; the host's own scroll default is suppressed so
                    // the page does not scroll under the zoom.
                    if event.delta.1 == 0.0 {
                        return None;
                    }
                    event.prevent_default();
                    Some(GraphCanvasEvent::Zoom {
                        factor: if event.delta.1 > 0.0 { 1.2 } else { 1.0 / 1.2 },
                    })
                },
            )) as ComponentView<GraphCanvasLocal<Id>, GraphCanvasEvent<Id>>
        },
        on_event,
    )
    .memo()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AnyView, DomHandle, GenetAppRunner};
    use genet_scripted_dom::{NodeId, ScriptedDom};
    use layout_dom_api::{LayoutDom, LocalName, Namespace};
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Default)]
    struct State {
        clicked: Vec<u8>,
        hovered: Option<u8>,
        focused: Option<u8>,
        dragged: Vec<GraphCanvasNodeDrag<u8>>,
        relation_clicked: Vec<String>,
        relation_hovered: Option<String>,
        expanded: bool,
    }

    type TestView = Box<dyn AnyView<State, (), GenetCtx, GenetElement>>;

    fn model(hovered: Option<u8>, focused: Option<u8>) -> GraphCanvasSwatch<u8, &'static str> {
        let mut swatch = GraphCanvasSwatch::new(
            77,
            GraphCanvasSubgraph {
                nodes: vec![
                    GraphCanvasNode {
                        id: 1,
                        kind: "document",
                        position: (0.1, 0.5),
                        label: "First node".into(),
                        key: None,
                    },
                    GraphCanvasNode {
                        id: 2,
                        kind: "person",
                        position: (0.9, 0.5),
                        label: "Second node".into(),
                        key: None,
                    },
                ],
                edges: vec![GraphCanvasEdge { from: 1, to: 2 }],
            },
        )
        .with_size(240, 112);
        swatch.selected = Some(1);
        swatch.focus = focused;
        swatch.hovered = hovered;
        swatch
    }

    fn view(state: &State) -> TestView {
        let swatch = model(state.hovered, state.focused);
        Box::new(graph_canvas_swatch_with_focus(
            &swatch,
            |state: &mut State, id| state.clicked.push(id),
            |state: &mut State, id| state.hovered = id,
            |state: &mut State, id| state.focused = id,
            |state: &mut State| state.expanded = true,
        ))
    }

    #[derive(Default)]
    struct ViewportState {
        pans: Vec<(f32, f32)>,
        zooms: Vec<f32>,
    }

    type ViewportView = Box<dyn AnyView<ViewportState, (), GenetCtx, GenetElement>>;

    fn viewport_view(_state: &ViewportState) -> ViewportView {
        let swatch = model(None, None);
        Box::new(graph_canvas(
            &swatch,
            |state: &mut ViewportState, event: GraphCanvasEvent<u8>| match event {
                GraphCanvasEvent::Pan { delta } => state.pans.push(delta),
                GraphCanvasEvent::Zoom { factor } => state.zooms.push(factor),
                _ => {},
            },
        ))
    }

    /// The viewport stays consumer state: a background drag reports pan deltas
    /// in canvas fractions, a wheel notch reports a zoom factor, and neither
    /// mutates the swatch.
    #[test]
    fn background_drag_pans_and_wheel_zooms() {
        let dom: DomHandle = Rc::new(RefCell::new(ScriptedDom::new()));
        let mut runner = GenetAppRunner::<_, _, _, ()>::new(
            dom.clone(),
            viewport_view,
            ViewportState::default(),
        );
        let root = runner.root();
        let canvas = find_attr(&dom.borrow(), root, "class", "graph-canvas-swatch")
            .expect("canvas root element");

        let size = (240.0, 112.0);
        runner.dispatch_pointer_down(
            canvas,
            PointerEvent::new(PointerPhase::Down, (10.0, 10.0), size),
        );
        runner.dispatch_pointer_move(PointerEvent::new(PointerPhase::Move, (34.0, 24.0), size));
        runner.dispatch_pointer_up(PointerEvent::new(PointerPhase::Up, (34.0, 24.0), size));
        assert_eq!(runner.state().pans, vec![(24.0 / 240.0, 14.0 / 112.0)]);

        runner.dispatch_wheel(canvas, WheelEvent::new((0.0, 3.0), (5.0, 5.0), size));
        runner.dispatch_wheel(canvas, WheelEvent::new((0.0, -3.0), (5.0, 5.0), size));
        assert_eq!(runner.state().zooms, vec![1.2, 1.0 / 1.2]);
    }

    fn attr<'a>(dom: &'a ScriptedDom, node: NodeId, name: &str) -> Option<&'a str> {
        dom.attribute(node, &Namespace::from(""), &LocalName::from(name))
    }

    fn find_attr(dom: &ScriptedDom, root: NodeId, name: &str, value: &str) -> Option<NodeId> {
        if attr(dom, root, name) == Some(value) {
            return Some(root);
        }
        dom.dom_children(root)
            .find_map(|child| find_attr(dom, child, name, value))
    }

    #[test]
    fn node_targets_route_click_hover_and_expand() {
        let dom: DomHandle = Rc::new(RefCell::new(ScriptedDom::new()));
        let mut runner = GenetAppRunner::<_, _, _, ()>::new(dom.clone(), view, State::default());
        let root = runner.root();
        let first =
            find_attr(&dom.borrow(), root, "aria-label", "First node").expect("first node target");
        runner.dispatch_hover(
            first,
            HoverEvent::new(HoverPhase::Enter, (2.0, 2.0), (20.0, 20.0)),
        );
        assert_eq!(runner.state().hovered, Some(1));
        runner.dispatch_click(first, PointerClick::at((2.0, 2.0)));
        assert_eq!(runner.state().clicked, [1]);
        assert_eq!(runner.focus(), Some(first));
        assert_eq!(runner.state().focused, Some(1));

        let expand =
            find_attr(&dom.borrow(), root, "aria-label", "Expand graph").expect("expand target");
        runner.dispatch_click(expand, PointerClick::at((2.0, 2.0)));
        assert!(runner.state().expanded);
    }

    fn labelled_view(state: &State) -> TestView {
        let swatch = model(state.hovered, state.focused).with_node_labels(true);
        Box::new(graph_canvas_swatch_with_focus(
            &swatch,
            |state: &mut State, id| state.clicked.push(id),
            |state: &mut State, id| state.hovered = id,
            |state: &mut State, id| state.focused = id,
            |state: &mut State| state.expanded = true,
        ))
    }

    /// Collect the class attribute of every element whose *base* class is
    /// `base`. Matching the first token rather than a prefix keeps the
    /// `-labels` container out of a search for `-label` elements.
    fn classes_of(dom: &ScriptedDom, root: NodeId, base: &str, out: &mut Vec<String>) {
        if let Some(class) = attr(dom, root, "class") {
            if class.split_whitespace().next() == Some(base) {
                out.push(class.to_string());
            }
        }
        for child in dom.dom_children(root) {
            classes_of(dom, child, base, out);
        }
    }

    /// Visible labels carry the node's state, not just its text.
    ///
    /// Without this a consumer that needs to emphasize one label (which node is
    /// "here") has to hand-roll the entire label layer to get it, which defeats
    /// the point of the component rendering labels at all.
    #[test]
    fn visible_labels_carry_node_state() {
        let dom: DomHandle = Rc::new(RefCell::new(ScriptedDom::new()));
        let state = State {
            hovered: Some(2),
            ..State::default()
        };
        let runner = GenetAppRunner::<_, _, _, ()>::new(dom.clone(), labelled_view, state);
        let root = runner.root();
        let mut found = Vec::new();
        classes_of(&dom.borrow(), root, "graph-canvas-swatch-label", &mut found);

        assert_eq!(found.len(), 2, "one label per node: {found:?}");
        // `model` selects node 1; the test state hovers node 2.
        assert!(
            found
                .iter()
                .any(|c| c == "graph-canvas-swatch-label selected"),
            "the selected node's label must say so: {found:?}"
        );
        assert!(
            found
                .iter()
                .any(|c| c == "graph-canvas-swatch-label hovered"),
            "the hovered node's label must say so: {found:?}"
        );

        let swatch = model(None, None).with_node_labels(true);
        assert_eq!(swatch.label_placement(0), GraphLabelPlacement::Above);
        assert_eq!(swatch.label_placement(1), GraphLabelPlacement::Above);
        assert!(
            find_attr(&dom.borrow(), root, "data-label-placement", "above").is_some(),
            "a horizontal graph publishes its label-clear placement"
        );
    }

    #[test]
    fn paint_leaf_and_hit_targets_share_projection() {
        let swatch = model(Some(1), Some(2));
        let leaf = swatch.paint_leaf(|kind| match *kind {
            "document" => ColorF {
                r: 0.2,
                g: 0.4,
                b: 0.8,
                a: 1.0,
            },
            _ => ColorF {
                r: 0.7,
                g: 0.4,
                b: 0.3,
                a: 1.0,
            },
        });
        let projected = swatch.projected_positions();
        assert_eq!(
            leaf.node_local_position(
                0,
                Size {
                    width: 240.0,
                    height: 112.0
                }
            ),
            Some(projected[0].1)
        );
    }

    #[test]
    fn node_regions_share_projection_and_clamp_inside_the_canvas() {
        let swatch = model(None, None);
        let projected = swatch.projected_positions();
        let region = swatch
            .projected_node_region(&1, 120.0, 80.0)
            .expect("node region");
        let center = projected[0].1;
        assert!(center.0 >= region.left && center.0 <= region.left + region.width);
        assert!(center.1 >= region.top && center.1 <= region.top + region.height);
        assert!(region.left >= 0.0 && region.top >= 0.0);
        assert!(region.left + region.width <= swatch.width as f32);
        assert!(region.top + region.height <= swatch.height as f32);

        let full = swatch
            .projected_node_region(&2, 10_000.0, 10_000.0)
            .expect("oversized region");
        assert_eq!(
            full,
            GraphCanvasNodeRegion {
                left: 0.0,
                top: 0.0,
                width: swatch.width as f32,
                height: swatch.height as f32,
            }
        );
        assert!(swatch.projected_node_region(&99, 10.0, 10.0).is_none());
    }

    #[test]
    fn node_footprints_clip_incident_relation_routes_to_the_card_perimeter() {
        let relations = vec![
            GraphCanvasRelation {
                id: "outgoing".into(),
                from: 1,
                to: 2,
                kind: "First".into(),
                label: "Outgoing relation".into(),
                route: Vec::new(),
                visible: true,
                emphasized: false,
            },
            GraphCanvasRelation {
                id: "incoming".into(),
                from: 2,
                to: 1,
                kind: "Second".into(),
                label: "Incoming relation".into(),
                route: Vec::new(),
                visible: true,
                emphasized: false,
            },
        ];
        let compact = model(None, None).with_relations(relations.clone());
        let compact_routes = compact.projected_relations();
        let swatch = model(None, None)
            .with_relations(relations)
            .with_node_footprint(1, 80.0, 60.0);
        let region = swatch
            .projected_node_footprint(&1)
            .expect("declared footprint");
        let routes = swatch.projected_relations();
        let on_perimeter = |point: (f32, f32)| {
            let right = region.left + region.width;
            let bottom = region.top + region.height;
            let on_vertical =
                (point.0 - region.left).abs() < 0.01 || (point.0 - right).abs() < 0.01;
            let on_horizontal =
                (point.1 - region.top).abs() < 0.01 || (point.1 - bottom).abs() < 0.01;
            (on_vertical && point.1 >= region.top && point.1 <= bottom)
                || (on_horizontal && point.0 >= region.left && point.0 <= right)
        };

        assert_eq!(routes.len(), 2);
        assert!(on_perimeter(routes[0].1[0]), "outgoing source is clipped");
        assert!(
            on_perimeter(*routes[1].1.last().expect("incoming endpoint")),
            "incoming target is clipped"
        );
        assert_eq!(routes[0].1[1], compact_routes[0].1[1]);
        assert_eq!(routes[0].1[2], compact_routes[0].1[2]);
        assert_eq!(routes[1].1[1], compact_routes[1].1[1]);
        assert_eq!(routes[1].1[2], compact_routes[1].1[2]);
        assert_eq!(
            *routes[0].1.last().expect("unoccupied target"),
            *compact_routes[0].1.last().expect("compact target")
        );
        assert_eq!(routes[1].1[0], compact_routes[1].1[0]);
    }

    #[test]
    fn graph_position_inverts_the_viewport_projection() {
        let mut swatch = model(None, None);
        swatch.viewport = GraphViewport {
            pan: (0.12, -0.08),
            zoom: 1.35,
        };
        let point = (0.73, 0.21);
        let size = Size {
            width: swatch.width as f32,
            height: swatch.height as f32,
        };
        let leaf = swatch
            .viewport
            .project(point, size, swatch.node_radius + swatch.edge_width);
        let recovered = swatch.graph_position_at(leaf);
        assert!((recovered.0 - point.0).abs() < 1e-5, "{recovered:?}");
        assert!((recovered.1 - point.1).abs() < 1e-5, "{recovered:?}");
    }

    fn drag_view(state: &State) -> TestView {
        let swatch = model(state.hovered, state.focused);
        Box::new(graph_canvas_swatch_with_drag(
            &swatch,
            |state: &mut State, id| state.clicked.push(id),
            |state: &mut State, id| state.hovered = id,
            |state: &mut State, event| state.dragged.push(event),
            |state: &mut State| state.expanded = true,
        ))
    }

    fn relation_view(state: &State) -> TestView {
        let swatch = model(state.hovered, state.focused).with_relations(vec![
            GraphCanvasRelation {
                id: "cites".into(),
                from: 1,
                to: 2,
                kind: "Citation".into(),
                label: "First node cites Second node".into(),
                route: Vec::new(),
                visible: true,
                emphasized: true,
            },
            GraphCanvasRelation {
                id: "quotes".into(),
                from: 1,
                to: 2,
                kind: "Quotation".into(),
                label: "First node quotes Second node".into(),
                route: Vec::new(),
                visible: true,
                emphasized: false,
            },
            GraphCanvasRelation {
                id: "hidden".into(),
                from: 1,
                to: 2,
                kind: "Hidden".into(),
                label: "This relation is hidden".into(),
                route: Vec::new(),
                visible: false,
                emphasized: false,
            },
        ]);
        Box::new(graph_canvas_swatch_with_drag_and_relations(
            &swatch,
            |state: &mut State, id| state.clicked.push(id),
            |state: &mut State, id| state.hovered = id,
            |state: &mut State, event| state.dragged.push(event),
            |state: &mut State, id| state.relation_clicked.push(id),
            |state: &mut State, id| state.relation_hovered = id,
            |state: &mut State| state.expanded = true,
        ))
    }

    #[test]
    fn relation_cells_keep_parallel_endpoints_independent_and_hideable() {
        let dom: DomHandle = Rc::new(RefCell::new(ScriptedDom::new()));
        let mut runner =
            GenetAppRunner::<_, _, _, ()>::new(dom.clone(), relation_view, State::default());
        let root = runner.root();
        let cites = find_attr(&dom.borrow(), root, "data-relation-id", "cites")
            .expect("citation relation target");
        let quotes = find_attr(&dom.borrow(), root, "data-relation-id", "quotes")
            .expect("quotation relation target");
        assert_ne!(cites, quotes, "parallel cells must get separate targets");
        assert!(
            find_attr(&dom.borrow(), root, "data-relation-id", "hidden").is_none(),
            "hidden cells must not remain hittable"
        );
        assert_eq!(attr(&dom.borrow(), cites, "aria-current"), Some("true"));

        runner.dispatch_hover(
            quotes,
            HoverEvent::new(HoverPhase::Enter, (2.0, 2.0), (20.0, 20.0)),
        );
        runner.dispatch_click(cites, PointerClick::at((2.0, 2.0)));
        assert_eq!(runner.state().relation_hovered.as_deref(), Some("quotes"));
        assert_eq!(runner.state().relation_clicked, ["cites"]);

        let swatch = model(None, None).with_relations(vec![GraphCanvasRelation {
            id: "hidden".into(),
            from: 1,
            to: 2,
            kind: "Hidden".into(),
            label: "This relation is hidden".into(),
            route: Vec::new(),
            visible: false,
            emphasized: false,
        }]);
        assert!(swatch.projected_relations().is_empty());
    }

    #[test]
    fn parallel_relation_cells_fan_into_distinct_painted_routes() {
        let swatch = model(None, None).with_relations(vec![
            GraphCanvasRelation {
                id: "one".into(),
                from: 1,
                to: 2,
                kind: "First".into(),
                label: "First relation".into(),
                route: Vec::new(),
                visible: true,
                emphasized: false,
            },
            GraphCanvasRelation {
                id: "two".into(),
                from: 1,
                to: 2,
                kind: "Second".into(),
                label: "Second relation".into(),
                route: Vec::new(),
                visible: true,
                emphasized: false,
            },
        ]);
        let routes = swatch.projected_relations();
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].1.len(), 4);
        assert_eq!(routes[1].1.len(), 4);
        assert_eq!(routes[0].1[0], routes[1].1[0]);
        assert_eq!(routes[0].1[3], routes[1].1[3]);
        let gap = (routes[0].1[1].1 - routes[1].1[1].1).abs();
        assert!((gap - 12.0).abs() < 0.01, "pixel-stable lane gap: {gap}");
        assert_eq!(
            routes[0].1[1].1, routes[0].1[2].1,
            "each relation holds its own lane through the graph interior"
        );
    }

    #[test]
    fn node_drag_captures_and_reports_normalized_graph_positions() {
        let dom: DomHandle = Rc::new(RefCell::new(ScriptedDom::new()));
        let mut runner =
            GenetAppRunner::<_, _, _, ()>::new(dom.clone(), drag_view, State::default());
        let root = runner.root();
        let first =
            find_attr(&dom.borrow(), root, "aria-label", "First node").expect("first node target");

        runner.dispatch_pointer_down(
            first,
            PointerEvent::new(PointerPhase::Down, (10.0, 10.0), (20.0, 20.0)),
        );
        assert_eq!(runner.pointer_capture(), Some(first));
        runner.dispatch_pointer_move(PointerEvent::new(
            PointerPhase::Move,
            (120.0, 10.0),
            (20.0, 20.0),
        ));
        runner.dispatch_pointer_up(PointerEvent::new(
            PointerPhase::Up,
            (120.0, 10.0),
            (20.0, 20.0),
        ));

        let dragged = &runner.state().dragged;
        assert_eq!(
            dragged.iter().map(|event| event.phase).collect::<Vec<_>>(),
            vec![PointerPhase::Down, PointerPhase::Move, PointerPhase::Up]
        );
        assert!(
            dragged[1].position.0 > dragged[0].position.0,
            "a rightward drag advances the normalized x position: {dragged:?}"
        );
        assert!(
            dragged.iter().all(|event| event.id == 1
                && (0.0..=1.0).contains(&event.position.0)
                && (0.0..=1.0).contains(&event.position.1)),
            "every emitted event keeps the node id and clamps graph coordinates: {dragged:?}"
        );
        assert_eq!(runner.pointer_capture(), None, "Up releases capture");
    }

    /// The component form's app state: selection truth in, events out. There
    /// is deliberately no hover or focus field here — that is the whole point.
    #[derive(Default)]
    struct AppState {
        selected: Option<u8>,
        events: Vec<GraphCanvasEvent<u8>>,
    }

    type AppView = Box<dyn AnyView<AppState, (), GenetCtx, GenetElement>>;

    fn component_view(state: &AppState) -> AppView {
        let mut swatch = model(None, None);
        swatch.selected = state.selected;
        Box::new(graph_canvas(
            &swatch,
            |state: &mut AppState, event: GraphCanvasEvent<u8>| {
                if let GraphCanvasEvent::Activate(id) = &event {
                    state.selected = Some(*id);
                }
                state.events.push(event);
            },
        ))
    }

    #[test]
    fn component_owns_emphasis_and_reports_only_events() {
        let dom: DomHandle = Rc::new(RefCell::new(ScriptedDom::new()));
        let mut runner = GenetAppRunner::<_, _, _, ()>::new(
            dom.clone(),
            component_view,
            AppState {
                selected: Some(1),
                events: Vec::new(),
            },
        );
        let root = runner.root();
        let second =
            find_attr(&dom.borrow(), root, "aria-label", "Second node").expect("second node");

        // Hover renders emphasis without the application storing anything.
        runner.dispatch_hover(
            second,
            HoverEvent::new(HoverPhase::Enter, (2.0, 2.0), (20.0, 20.0)),
        );
        assert!(
            runner.state().events.is_empty(),
            "emphasis is not an application event"
        );
        assert_eq!(
            attr(&dom.borrow(), second, "class"),
            Some("graph-canvas-swatch-node hovered"),
            "the component rendered its own hover emphasis"
        );

        runner.dispatch_hover(
            second,
            HoverEvent::new(HoverPhase::Leave, (2.0, 2.0), (20.0, 20.0)),
        );
        assert_eq!(
            attr(&dom.borrow(), second, "class"),
            Some("graph-canvas-swatch-node"),
            "leaving clears the component's emphasis"
        );

        // Activation is an event, and the selection it drives is parent truth
        // that comes back through props.
        runner.dispatch_click(second, PointerClick::at((2.0, 2.0)));
        assert_eq!(runner.state().events, [GraphCanvasEvent::Activate(2)]);
        assert_eq!(runner.state().selected, Some(2));
        assert_eq!(
            attr(&dom.borrow(), second, "class"),
            Some("graph-canvas-swatch-node selected focused"),
            "parent-owned selection renders beside the component's own focus emphasis"
        );

        let expand =
            find_attr(&dom.borrow(), root, "aria-label", "Expand graph").expect("expand target");
        runner.dispatch_click(expand, PointerClick::at((2.0, 2.0)));
        assert_eq!(
            runner.state().events,
            [GraphCanvasEvent::Activate(2), GraphCanvasEvent::Expand]
        );
    }
}
