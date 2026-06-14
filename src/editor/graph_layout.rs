//! Force-directed layout for knowledge graphs.

use std::collections::HashMap;

use super::graph_model::{
    GraphEdgeKind, GraphNodeId, GraphNodeKind, KnowledgeGraph,
};

const DOCUMENT_NODE_RADIUS: f32 = 16.0;
const TAG_NODE_BASE_RADIUS: f32 = 12.0;
const TAG_NODE_RADIUS_SCALE: f32 = 4.0;
const MIN_NODE_RADIUS: f32 = 10.0;
const MAX_NODE_RADIUS: f32 = 52.0;
const CENTER_GRAVITY: f32 = 0.02;
const IDEAL_EDGE_LENGTH_SCALE: f32 = 2.5;
const MIN_DISTANCE: f32 = 0.01;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LayoutPoint {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LayoutBounds {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

impl LayoutPoint {
    fn scaled(self, factor: f32) -> Self {
        Self {
            x: self.x * factor,
            y: self.y * factor,
        }
    }

    pub(crate) fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

impl LayoutBounds {
    fn empty() -> Self {
        Self {
            min_x: 0.0,
            min_y: 0.0,
            max_x: 0.0,
            max_y: 0.0,
        }
    }

    fn include_point(&mut self, point: LayoutPoint, radius: f32) {
        self.min_x = self.min_x.min(point.x - radius);
        self.min_y = self.min_y.min(point.y - radius);
        self.max_x = self.max_x.max(point.x + radius);
        self.max_y = self.max_y.max(point.y + radius);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GraphLayout {
    pub positions: HashMap<GraphNodeId, LayoutPoint>,
    pub bounds: LayoutBounds,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LayoutConfig {
    pub iterations: usize,
    pub repulsion: f32,
    pub attraction: f32,
    pub damping: f32,
    pub seed: u64,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            iterations: 300,
            repulsion: 900.0,
            attraction: 0.06,
            damping: 0.86,
            seed: 42,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LayoutSimulation {
    node_ids: Vec<GraphNodeId>,
    edges: Vec<(usize, usize, GraphEdgeKind)>,
    sizes: Vec<f32>,
    positions: Vec<LayoutPoint>,
    velocities: Vec<LayoutPoint>,
    pinned: Vec<bool>,
    config: LayoutConfig,
}

pub(crate) fn node_display_label(kind: &GraphNodeKind) -> String {
    match kind {
        GraphNodeKind::Document { label, .. } => label.clone(),
        GraphNodeKind::Tag { name, .. } => format!("#{name}"),
    }
}

pub(crate) fn node_layout_radius(kind: &GraphNodeKind) -> f32 {
    match kind {
        GraphNodeKind::Document { .. } => DOCUMENT_NODE_RADIUS,
        GraphNodeKind::Tag { count, .. } => {
            let count = (*count).max(1) as f32;
            (TAG_NODE_BASE_RADIUS + TAG_NODE_RADIUS_SCALE * count.sqrt())
                .clamp(MIN_NODE_RADIUS, MAX_NODE_RADIUS)
        }
    }
}

pub(crate) fn layout_spread_for_graph(graph: &KnowledgeGraph) -> f32 {
    let node_count = graph.nodes.len().max(1) as f32;
    let average_radius = if graph.nodes.is_empty() {
        DOCUMENT_NODE_RADIUS
    } else {
        graph
            .nodes
            .iter()
            .map(|node| node_layout_radius(&node.kind))
            .sum::<f32>()
            / node_count
    };
    (node_count.sqrt() * average_radius * 3.0).max(120.0)
}

#[cfg(test)]
pub(crate) fn compute_graph_layout(
    graph: &KnowledgeGraph,
    config: &LayoutConfig,
) -> GraphLayout {
    let mut simulation = LayoutSimulation::new(graph, config.clone());
    for _ in 0..config.iterations {
        layout_tick(&mut simulation, 1.0);
    }
    simulation.to_layout()
}

pub(crate) fn layout_tick(simulation: &mut LayoutSimulation, dt: f32) {
    let node_count = simulation.positions.len();
    if node_count == 0 {
        return;
    }

    let mut forces = vec![LayoutPoint { x: 0.0, y: 0.0 }; node_count];

    for left in 0..node_count {
        for right in left + 1..node_count {
            let delta = point_delta(simulation.positions[left], simulation.positions[right]);
            let distance = delta.length().max(MIN_DISTANCE);
            let min_separation = simulation.sizes[left] + simulation.sizes[right];
            let repulsion_strength =
                simulation.config.repulsion * (min_separation / distance).powi(2);
            let repulsion = delta
                .normalized()
                .scaled(repulsion_strength / (distance * distance));

            forces[left].x -= repulsion.x;
            forces[left].y -= repulsion.y;
            forces[right].x += repulsion.x;
            forces[right].y += repulsion.y;
        }
    }

    for &(source, target, kind) in &simulation.edges {
        let delta = point_delta(simulation.positions[source], simulation.positions[target]);
        let distance = delta.length().max(MIN_DISTANCE);
        let min_separation = simulation.sizes[source] + simulation.sizes[target];
        let ideal_length = min_separation * IDEAL_EDGE_LENGTH_SCALE;
        let attraction_scale = match kind {
            GraphEdgeKind::WikiLink => simulation.config.attraction * 1.25,
            GraphEdgeKind::Tagged => simulation.config.attraction,
        };
        let attraction = delta
            .normalized()
            .scaled(attraction_scale * (distance - ideal_length));

        forces[source].x += attraction.x;
        forces[source].y += attraction.y;
        forces[target].x -= attraction.x;
        forces[target].y -= attraction.y;
    }

    for (index, force) in forces.iter_mut().enumerate() {
        force.x -= simulation.positions[index].x * CENTER_GRAVITY;
        force.y -= simulation.positions[index].y * CENTER_GRAVITY;
    }

    let damping = simulation.config.damping;
    for index in 0..node_count {
        if simulation.pinned[index] {
            simulation.velocities[index] = LayoutPoint { x: 0.0, y: 0.0 };
            continue;
        }

        simulation.velocities[index].x =
            (simulation.velocities[index].x + forces[index].x * dt) * damping;
        simulation.velocities[index].y =
            (simulation.velocities[index].y + forces[index].y * dt) * damping;
        simulation.positions[index].x += simulation.velocities[index].x * dt;
        simulation.positions[index].y += simulation.velocities[index].y * dt;
        if !simulation.positions[index].is_finite() {
            simulation.positions[index] = LayoutPoint { x: 0.0, y: 0.0 };
            simulation.velocities[index] = LayoutPoint { x: 0.0, y: 0.0 };
        }
    }
}

impl LayoutSimulation {
    pub(crate) fn new(graph: &KnowledgeGraph, config: LayoutConfig) -> Self {
        let mut rng = Rng::new(config.seed);
        let spread = layout_spread_for_graph(graph);

        let node_ids: Vec<GraphNodeId> = graph.nodes.iter().map(|node| node.id.clone()).collect();
        let id_to_index: HashMap<_, _> = node_ids
            .iter()
            .enumerate()
            .map(|(index, id)| (id.clone(), index))
            .collect();

        let sizes = graph
            .nodes
            .iter()
            .map(|node| node_layout_radius(&node.kind))
            .collect();

        let positions = (0..node_ids.len())
            .map(|_| LayoutPoint {
                x: rng.next_f32() * spread,
                y: rng.next_f32() * spread,
            })
            .collect();

        let node_count = node_ids.len();
        let edges = graph
            .edges
            .iter()
            .filter_map(|edge| {
                Some((
                    *id_to_index.get(&edge.source)?,
                    *id_to_index.get(&edge.target)?,
                    edge.kind,
                ))
            })
            .collect();

        Self {
            node_ids,
            edges,
            sizes,
            positions,
            velocities: vec![LayoutPoint { x: 0.0, y: 0.0 }; node_count],
            pinned: vec![false; node_count],
            config,
        }
    }

    pub(crate) fn pin_node(&mut self, node_id: &GraphNodeId) {
        if let Some(index) = self.node_ids.iter().position(|id| id == node_id) {
            self.pinned[index] = true;
        }
    }

    pub(crate) fn set_node_position(&mut self, node_id: &GraphNodeId, position: LayoutPoint) {
        if !position.is_finite() {
            return;
        }
        if let Some(index) = self.node_ids.iter().position(|id| id == node_id) {
            self.positions[index] = position;
            self.velocities[index] = LayoutPoint { x: 0.0, y: 0.0 };
        }
    }

    pub(crate) fn to_layout(&self) -> GraphLayout {
        let mut positions = HashMap::with_capacity(self.node_ids.len());
        let mut bounds = LayoutBounds::empty();

        for (index, node_id) in self.node_ids.iter().enumerate() {
            let point = self.positions[index];
            positions.insert(node_id.clone(), point);
            bounds.include_point(point, self.sizes[index]);
        }

        GraphLayout { positions, bounds }
    }
}

#[derive(Clone, Copy)]
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        value ^ (value >> 31)
    }

    fn next_f32(&mut self) -> f32 {
        let bits = (self.next_u64() >> 40) as u32;
        (bits as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

#[derive(Clone, Copy)]
struct PointDelta {
    x: f32,
    y: f32,
}

impl PointDelta {
    fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    fn normalized(self) -> LayoutPoint {
        let length = self.length().max(MIN_DISTANCE);
        LayoutPoint {
            x: self.x / length,
            y: self.y / length,
        }
    }
}

fn point_delta(from: LayoutPoint, to: LayoutPoint) -> PointDelta {
    PointDelta {
        x: to.x - from.x,
        y: to.y - from.y,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::editor::graph_model::{GraphEdge, GraphNode, KnowledgeGraph};

    fn triangle_graph() -> KnowledgeGraph {
        let nodes = vec![
            GraphNode {
                id: GraphNodeId::document("a.md"),
                kind: GraphNodeKind::Document {
                    path: PathBuf::from("/tmp/a.md"),
                    relative_path: "a.md".to_string(),
                    label: "a.md".to_string(),
                },
            },
            GraphNode {
                id: GraphNodeId::document("b.md"),
                kind: GraphNodeKind::Document {
                    path: PathBuf::from("/tmp/b.md"),
                    relative_path: "b.md".to_string(),
                    label: "b.md".to_string(),
                },
            },
            GraphNode {
                id: GraphNodeId::document("c.md"),
                kind: GraphNodeKind::Document {
                    path: PathBuf::from("/tmp/c.md"),
                    relative_path: "c.md".to_string(),
                    label: "c.md".to_string(),
                },
            },
        ];
        let edges = vec![
            GraphEdge {
                source: GraphNodeId::document("a.md"),
                target: GraphNodeId::document("b.md"),
                kind: GraphEdgeKind::WikiLink,
            },
            GraphEdge {
                source: GraphNodeId::document("b.md"),
                target: GraphNodeId::document("c.md"),
                kind: GraphEdgeKind::WikiLink,
            },
            GraphEdge {
                source: GraphNodeId::document("c.md"),
                target: GraphNodeId::document("a.md"),
                kind: GraphEdgeKind::WikiLink,
            },
        ];

        KnowledgeGraph {
            nodes,
            edges,
            broken_wiki_links: 0,
            revision: 1,
        }
    }

    fn nodes_overlap(layout: &GraphLayout, graph: &KnowledgeGraph) -> bool {
        let radii: HashMap<_, _> = graph
            .nodes
            .iter()
            .map(|node| (node.id.clone(), node_layout_radius(&node.kind)))
            .collect();

        for left in &graph.nodes {
            for right in &graph.nodes {
                if left.id >= right.id {
                    continue;
                }
                let Some(left_pos) = layout.positions.get(&left.id) else {
                    continue;
                };
                let Some(right_pos) = layout.positions.get(&right.id) else {
                    continue;
                };
                let dx = left_pos.x - right_pos.x;
                let dy = left_pos.y - right_pos.y;
                let distance = (dx * dx + dy * dy).sqrt();
                let min_distance = radii[&left.id] + radii[&right.id];
                if distance < min_distance * 0.95 {
                    return true;
                }
            }
        }
        false
    }

    #[test]
    fn triangle_graph_converges_without_overlap() {
        let graph = triangle_graph();
        let config = LayoutConfig {
            iterations: 400,
            ..LayoutConfig::default()
        };
        let layout = compute_graph_layout(&graph, &config);

        assert_eq!(layout.positions.len(), 3);
        assert!(!nodes_overlap(&layout, &graph));
        assert!(layout.bounds.max_x > layout.bounds.min_x);
        assert!(layout.bounds.max_y > layout.bounds.min_y);
    }

    #[test]
    fn same_seed_produces_identical_layout() {
        let graph = triangle_graph();
        let config = LayoutConfig::default();

        let first = compute_graph_layout(&graph, &config);
        let second = compute_graph_layout(&graph, &config);

        assert_eq!(first.positions, second.positions);
        assert_eq!(first.bounds, second.bounds);
    }

    #[test]
    fn layout_tick_moves_unpinned_nodes() {
        let graph = triangle_graph();
        let mut simulation = LayoutSimulation::new(&graph, LayoutConfig::default());
        let before = simulation.positions.clone();
        layout_tick(&mut simulation, 1.0);
        assert_ne!(simulation.positions, before);
    }

    #[test]
    fn tag_radius_scales_proportionally_with_count() {
        let name = "shared".to_string();
        let one = node_layout_radius(&GraphNodeKind::Tag {
            name: name.clone(),
            count: 1,
        });
        let four = node_layout_radius(&GraphNodeKind::Tag {
            name: name.clone(),
            count: 4,
        });
        let sixteen = node_layout_radius(&GraphNodeKind::Tag {
            name,
            count: 16,
        });
        assert!(four > one);
        assert!(sixteen > four);
        assert!((four - one - (sixteen - four) * 0.5).abs() < 0.01);
    }

    #[test]
    fn layout_spread_grows_with_node_count() {
        let small = layout_spread_for_graph(&triangle_graph());
        let mut many_nodes = triangle_graph();
        for index in 0..20 {
            let relative = format!("extra-{index}.md");
            many_nodes.nodes.push(GraphNode {
                id: GraphNodeId::document(&relative),
                kind: GraphNodeKind::Document {
                    path: PathBuf::from(format!("/tmp/{relative}")),
                    relative_path: relative.clone(),
                    label: relative,
                },
            });
        }
        let large = layout_spread_for_graph(&many_nodes);
        assert!(large > small);
    }
}
