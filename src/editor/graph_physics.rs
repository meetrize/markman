//! Velocity-based circle physics for the knowledge graph canvas.

use super::graph_layout::{LayoutPoint, LayoutSimulation, MIN_DISTANCE};
use super::graph_view::GraphViewport;

pub(crate) const GRAPH_PHYSICS_FRAME_MS: u64 = 16;
pub(crate) const GRAPH_PHYSICS_DT: f32 = 1.0 / 60.0;
const GRAPH_PHYSICS_VELOCITY_THRESHOLD: f32 = 0.35;
const GRAPH_PHYSICS_VELOCITY_THRESHOLD_SQ: f32 =
    GRAPH_PHYSICS_VELOCITY_THRESHOLD * GRAPH_PHYSICS_VELOCITY_THRESHOLD;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GraphPhysicsBounds {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GraphPhysicsDragState {
    pub node_index: usize,
    pub velocity: LayoutPoint,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GraphPhysicsConfig {
    pub restitution: f32,
    pub linear_damping: f32,
    pub boundary_restitution: f32,
    pub boundary_padding: f32,
    pub solver_iterations: usize,
}

impl Default for GraphPhysicsConfig {
    fn default() -> Self {
        Self {
            restitution: 0.82,
            linear_damping: 0.972,
            boundary_restitution: 0.72,
            boundary_padding: 12.0,
            solver_iterations: 6,
        }
    }
}

pub(crate) fn viewport_physics_bounds(
    viewport: &GraphViewport,
    panel_width: f32,
    panel_height: f32,
    padding: f32,
) -> GraphPhysicsBounds {
    let top_left = viewport.screen_to_world(LayoutPoint { x: 0.0, y: 0.0 });
    let bottom_right = viewport.screen_to_world(LayoutPoint {
        x: panel_width,
        y: panel_height,
    });
    let min_x = top_left.x.min(bottom_right.x) + padding;
    let min_y = top_left.y.min(bottom_right.y) + padding;
    let max_x = top_left.x.max(bottom_right.x) - padding;
    let max_y = top_left.y.max(bottom_right.y) - padding;
    GraphPhysicsBounds {
        min_x,
        min_y,
        max_x: max_x.max(min_x + MIN_DISTANCE),
        max_y: max_y.max(min_y + MIN_DISTANCE),
    }
}

pub(crate) fn clear_graph_physics_velocities(simulation: &mut LayoutSimulation) {
    for velocity in &mut simulation.velocities {
        *velocity = LayoutPoint { x: 0.0, y: 0.0 };
    }
}

pub(crate) fn graph_physics_has_motion(simulation: &LayoutSimulation) -> bool {
    simulation.velocities.iter().any(|velocity| {
        velocity.x * velocity.x + velocity.y * velocity.y > GRAPH_PHYSICS_VELOCITY_THRESHOLD_SQ
    })
}

pub(crate) fn step_graph_physics(
    simulation: &mut LayoutSimulation,
    bounds: GraphPhysicsBounds,
    config: &GraphPhysicsConfig,
    drag: Option<GraphPhysicsDragState>,
    dt: f32,
) -> bool {
    let node_count = simulation.positions.len();
    if node_count == 0 {
        return false;
    }

    for index in 0..node_count {
        if simulation.pinned[index] {
            continue;
        }
        simulation.positions[index].x += simulation.velocities[index].x * dt;
        simulation.positions[index].y += simulation.velocities[index].y * dt;
        simulation.velocities[index].x *= config.linear_damping;
        simulation.velocities[index].y *= config.linear_damping;
        sanitize_node_state(simulation, index);
    }

    for _ in 0..config.solver_iterations {
        resolve_node_collisions(simulation, drag, config.restitution);
        resolve_boundary_collisions(simulation, bounds, config.boundary_restitution);
    }

    graph_physics_has_motion(simulation)
}

fn sanitize_node_state(simulation: &mut LayoutSimulation, index: usize) {
    if !simulation.positions[index].is_finite() {
        simulation.positions[index] = LayoutPoint { x: 0.0, y: 0.0 };
    }
    if !simulation.velocities[index].is_finite() {
        simulation.velocities[index] = LayoutPoint { x: 0.0, y: 0.0 };
    }
}

fn resolve_node_collisions(
    simulation: &mut LayoutSimulation,
    drag: Option<GraphPhysicsDragState>,
    restitution: f32,
) {
    let node_count = simulation.positions.len();
    for left in 0..node_count {
        for right in left + 1..node_count {
            resolve_collision_pair(simulation, left, right, drag, restitution);
        }
    }
}

fn resolve_collision_pair(
    simulation: &mut LayoutSimulation,
    left: usize,
    right: usize,
    drag: Option<GraphPhysicsDragState>,
    restitution: f32,
) {
    let delta = point_delta(simulation.positions[left], simulation.positions[right]);
    let distance = delta.length().max(MIN_DISTANCE);
    let min_distance = simulation.sizes[left] + simulation.sizes[right];
    if distance >= min_distance {
        return;
    }

    let nx = delta.x / distance;
    let ny = delta.y / distance;
    let overlap = min_distance - distance;
    let left_pinned = simulation.pinned[left];
    let right_pinned = simulation.pinned[right];

    if left_pinned && right_pinned {
        return;
    }

    let left_velocity = simulation.velocities[left];
    let right_velocity = simulation.velocities[right];

    if left_pinned && !right_pinned {
        simulation.positions[right].x += nx * overlap;
        simulation.positions[right].y += ny * overlap;
        apply_kinematic_impulse(
            &mut simulation.velocities[right],
            kinematic_velocity(left, drag, &left_velocity),
            nx,
            ny,
            restitution,
        );
    } else if right_pinned && !left_pinned {
        simulation.positions[left].x -= nx * overlap;
        simulation.positions[left].y -= ny * overlap;
        apply_kinematic_impulse(
            &mut simulation.velocities[left],
            kinematic_velocity(right, drag, &right_velocity),
            -nx,
            -ny,
            restitution,
        );
    } else {
        let half_overlap = overlap * 0.5;
        simulation.positions[left].x -= nx * half_overlap;
        simulation.positions[left].y -= ny * half_overlap;
        simulation.positions[right].x += nx * half_overlap;
        simulation.positions[right].y += ny * half_overlap;

        let left_normal_velocity = left_velocity.x * nx + left_velocity.y * ny;
        let right_normal_velocity = right_velocity.x * nx + right_velocity.y * ny;
        let relative_normal = left_normal_velocity - right_normal_velocity;
        if relative_normal > 0.0 {
            let impulse = relative_normal * (1.0 + restitution) * 0.5;
            simulation.velocities[left].x -= nx * impulse;
            simulation.velocities[left].y -= ny * impulse;
            simulation.velocities[right].x += nx * impulse;
            simulation.velocities[right].y += ny * impulse;
        }
    }

    sanitize_node_state(simulation, left);
    sanitize_node_state(simulation, right);
}

fn kinematic_velocity(
    index: usize,
    drag: Option<GraphPhysicsDragState>,
    fallback: &LayoutPoint,
) -> LayoutPoint {
    if drag.is_some_and(|state| state.node_index == index) {
        drag.unwrap().velocity
    } else {
        *fallback
    }
}

fn apply_kinematic_impulse(
    target_velocity: &mut LayoutPoint,
    source_velocity: LayoutPoint,
    nx: f32,
    ny: f32,
    restitution: f32,
) {
    let target_normal = target_velocity.x * nx + target_velocity.y * ny;
    let source_normal = source_velocity.x * nx + source_velocity.y * ny;
    let relative_normal = source_normal - target_normal;
    if relative_normal <= 0.0 {
        return;
    }
    let impulse = relative_normal * (1.0 + restitution);
    target_velocity.x += nx * impulse;
    target_velocity.y += ny * impulse;
}

fn resolve_boundary_collisions(
    simulation: &mut LayoutSimulation,
    bounds: GraphPhysicsBounds,
    restitution: f32,
) {
    for index in 0..simulation.positions.len() {
        if simulation.pinned[index] {
            continue;
        }

        let radius = simulation.sizes[index];
        let mut position = simulation.positions[index];
        let mut velocity = simulation.velocities[index];

        if position.x - radius < bounds.min_x {
            position.x = bounds.min_x + radius;
            velocity.x = velocity.x.abs() * restitution;
        } else if position.x + radius > bounds.max_x {
            position.x = bounds.max_x - radius;
            velocity.x = -velocity.x.abs() * restitution;
        }

        if position.y - radius < bounds.min_y {
            position.y = bounds.min_y + radius;
            velocity.y = velocity.y.abs() * restitution;
        } else if position.y + radius > bounds.max_y {
            position.y = bounds.max_y - radius;
            velocity.y = -velocity.y.abs() * restitution;
        }

        simulation.positions[index] = position;
        simulation.velocities[index] = velocity;
        sanitize_node_state(simulation, index);
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
    use crate::editor::graph_layout::LayoutConfig;
    use crate::editor::graph_model::{
        GraphEdge, GraphEdgeKind, GraphNode, GraphNodeId, GraphNodeKind, KnowledgeGraph,
    };

    fn two_node_graph() -> KnowledgeGraph {
        KnowledgeGraph {
            nodes: vec![
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
            ],
            edges: vec![GraphEdge {
                source: GraphNodeId::document("a.md"),
                target: GraphNodeId::document("b.md"),
                kind: GraphEdgeKind::WikiLink,
            }],
            broken_wiki_links: 0,
            revision: 1,
        }
    }

    #[test]
    fn pinned_node_pushes_free_node_apart() {
        let graph = two_node_graph();
        let mut simulation = LayoutSimulation::new(&graph, LayoutConfig::default());
        simulation.set_node_position(
            &GraphNodeId::document("a.md"),
            LayoutPoint { x: 0.0, y: 0.0 },
        );
        simulation.set_node_position(
            &GraphNodeId::document("b.md"),
            LayoutPoint { x: 10.0, y: 0.0 },
        );
        simulation.pin_node(&GraphNodeId::document("a.md"));

        let bounds = GraphPhysicsBounds {
            min_x: -200.0,
            min_y: -200.0,
            max_x: 200.0,
            max_y: 200.0,
        };
        let config = GraphPhysicsConfig::default();
        let drag = GraphPhysicsDragState {
            node_index: 0,
            velocity: LayoutPoint { x: 40.0, y: 0.0 },
        };

        step_graph_physics(
            &mut simulation,
            bounds,
            &config,
            Some(drag),
            GRAPH_PHYSICS_DT,
        );

        let separation = simulation.positions[1].x - simulation.positions[0].x;
        assert!(
            separation >= simulation.sizes[0] + simulation.sizes[1] - 0.5,
            "free node should be pushed out of overlap"
        );
        assert!(simulation.velocities[1].x > 0.0, "free node should rebound");
    }

    #[test]
    fn free_node_bounces_off_boundary() {
        let graph = two_node_graph();
        let mut simulation = LayoutSimulation::new(&graph, LayoutConfig::default());
        simulation.set_node_position(
            &GraphNodeId::document("b.md"),
            LayoutPoint { x: 0.0, y: 0.0 },
        );
        simulation.velocities[1] = LayoutPoint { x: -80.0, y: 0.0 };

        let bounds = GraphPhysicsBounds {
            min_x: -20.0,
            min_y: -20.0,
            max_x: 20.0,
            max_y: 20.0,
        };
        let config = GraphPhysicsConfig::default();

        step_graph_physics(
            &mut simulation,
            bounds,
            &config,
            None,
            GRAPH_PHYSICS_DT,
        );

        assert!(simulation.positions[1].x >= bounds.min_x + simulation.sizes[1] - 0.01);
        assert!(simulation.velocities[1].x > 0.0);
    }
}
