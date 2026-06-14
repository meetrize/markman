//! Velocity-based circle physics for the knowledge graph canvas.

use super::graph_layout::{LayoutPoint, LayoutSimulation, MIN_DISTANCE};
use super::graph_view::GraphViewport;

pub(crate) const GRAPH_PHYSICS_FRAME_MS: u64 = 16;
pub(crate) const GRAPH_PHYSICS_DT: f32 = 1.0 / 60.0;
const GRAPH_PHYSICS_VELOCITY_THRESHOLD: f32 = 0.35;
const GRAPH_PHYSICS_VELOCITY_THRESHOLD_SQ: f32 =
    GRAPH_PHYSICS_VELOCITY_THRESHOLD * GRAPH_PHYSICS_VELOCITY_THRESHOLD;
const SPATIAL_GRID_THRESHOLD: usize = 8;
const SPATIAL_GRID_CELL_SCALE: f32 = 2.5;
const SPATIAL_GRID_MIN_CELL: f32 = 28.0;
const SPATIAL_GRID_MAX_DIM: usize = 256;

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

impl GraphPhysicsConfig {
    pub(crate) fn for_node_count(node_count: usize) -> Self {
        let mut config = Self::default();
        if node_count > 20 {
            config.solver_iterations = 4;
        }
        if node_count > 40 {
            config.solver_iterations = 3;
        }
        if node_count > 80 {
            config.solver_iterations = 2;
        }
        config
    }

    pub(crate) fn for_interactive_drag(node_count: usize) -> Self {
        let mut config = Self::for_node_count(node_count);
        config.solver_iterations = config.solver_iterations.min(2);
        config
    }
}

pub(crate) fn physics_frame_ms(_node_count: usize) -> u64 {
    GRAPH_PHYSICS_FRAME_MS
}

pub(crate) fn viewport_physics_bounds(
    viewport: &GraphViewport,
    panel_width: f32,
    panel_height: f32,
    padding: f32,
) -> GraphPhysicsBounds {
    if viewport.scale <= 0.0 || !viewport.scale.is_finite() {
        return GraphPhysicsBounds {
            min_x: -512.0,
            min_y: -512.0,
            max_x: 512.0,
            max_y: 512.0,
        };
    }

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
    }

    for _ in 0..config.solver_iterations {
        resolve_node_collisions(simulation, drag, config.restitution);
        resolve_boundary_collisions(simulation, bounds, config.boundary_restitution);
        for index in 0..node_count {
            sanitize_node_state(simulation, index);
        }
    }

    graph_physics_has_motion(simulation)
}

struct CollisionSpatialGrid {
    cols: usize,
    rows: usize,
}

fn populate_collision_grid(simulation: &mut LayoutSimulation) -> Option<CollisionSpatialGrid> {
    let node_count = simulation.positions.len();
    if node_count == 0 {
        return None;
    }

    let max_radius = simulation
        .sizes
        .iter()
        .copied()
        .fold(0.0f32, f32::max);
    let base_cell_size = (max_radius * SPATIAL_GRID_CELL_SCALE).max(SPATIAL_GRID_MIN_CELL);

    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    for (index, position) in simulation.positions.iter().enumerate() {
        let radius = simulation.sizes[index];
        min_x = min_x.min(position.x - radius);
        min_y = min_y.min(position.y - radius);
        max_x = max_x.max(position.x + radius);
        max_y = max_y.max(position.y + radius);
    }

    if !min_x.is_finite()
        || !min_y.is_finite()
        || !max_x.is_finite()
        || !max_y.is_finite()
    {
        return None;
    }

    let (cols, rows, cell_size) =
        spatial_grid_cell_size(min_x, min_y, max_x, max_y, base_cell_size);

    let grid_len = cols * rows;
    let mut grid = std::mem::take(&mut simulation.collision_grid);
    if grid.len() < grid_len {
        grid.resize(grid_len, Vec::new());
    }
    for cell in grid.iter_mut().take(grid_len) {
        cell.clear();
    }

    for index in 0..node_count {
        let position = simulation.positions[index];
        let col = ((position.x - min_x) / cell_size).floor() as usize;
        let row = ((position.y - min_y) / cell_size).floor() as usize;
        let col = col.min(cols.saturating_sub(1));
        let row = row.min(rows.saturating_sub(1));
        grid[row * cols + col].push(index);
    }

    simulation.collision_grid = grid;

    Some(CollisionSpatialGrid { cols, rows })
}

fn sanitize_node_state(simulation: &mut LayoutSimulation, index: usize) {
    if !simulation.positions[index].is_finite() {
        simulation.positions[index] = LayoutPoint { x: 0.0, y: 0.0 };
    }
    if !simulation.velocities[index].is_finite() {
        simulation.velocities[index] = LayoutPoint { x: 0.0, y: 0.0 };
    }
}

fn node_has_motion(velocity: LayoutPoint) -> bool {
    velocity.x * velocity.x + velocity.y * velocity.y > GRAPH_PHYSICS_VELOCITY_THRESHOLD_SQ
}

fn pair_needs_collision_check(
    simulation: &LayoutSimulation,
    left: usize,
    right: usize,
    drag: Option<GraphPhysicsDragState>,
) -> bool {
    if simulation.pinned[left]
        || simulation.pinned[right]
        || drag.is_some_and(|state| state.node_index == left || state.node_index == right)
        || node_has_motion(simulation.velocities[left])
        || node_has_motion(simulation.velocities[right])
    {
        return true;
    }

    let delta = point_delta(simulation.positions[left], simulation.positions[right]);
    let distance_sq = delta.x * delta.x + delta.y * delta.y;
    let min_distance = simulation.sizes[left] + simulation.sizes[right];
    distance_sq < min_distance * min_distance
}

fn resolve_node_collisions(
    simulation: &mut LayoutSimulation,
    drag: Option<GraphPhysicsDragState>,
    restitution: f32,
) {
    let node_count = simulation.positions.len();
    if node_count <= 1 {
        return;
    }

    if node_count <= SPATIAL_GRID_THRESHOLD {
        for left in 0..node_count {
            for right in left + 1..node_count {
                if pair_needs_collision_check(simulation, left, right, drag) {
                    resolve_collision_pair(simulation, left, right, drag, restitution);
                }
            }
        }
        return;
    }

    resolve_node_collisions_spatial(simulation, drag, restitution);
}

fn spatial_grid_cell_size(
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
    base_cell_size: f32,
) -> (usize, usize, f32) {
    let mut cell_size = base_cell_size;
    for _ in 0..16 {
        let cols = ((max_x - min_x) / cell_size).ceil() as usize + 1;
        let rows = ((max_y - min_y) / cell_size).ceil() as usize + 1;
        if cols <= SPATIAL_GRID_MAX_DIM && rows <= SPATIAL_GRID_MAX_DIM {
            return (cols.max(1), rows.max(1), cell_size);
        }
        cell_size *= 1.75;
    }

    let span = (max_x - min_x).max(max_y - min_y).max(SPATIAL_GRID_MIN_CELL);
    (1, 1, span)
}

fn resolve_node_collisions_spatial(
    simulation: &mut LayoutSimulation,
    drag: Option<GraphPhysicsDragState>,
    restitution: f32,
) {
    let Some(grid_info) = populate_collision_grid(simulation) else {
        return;
    };
    let cols = grid_info.cols;
    let rows = grid_info.rows;
    let grid = std::mem::take(&mut simulation.collision_grid);

    for row in 0..rows {
        for col in 0..cols {
            let cell = row * cols + col;
            let cell_nodes = &grid[cell];
            if cell_nodes.is_empty() {
                continue;
            }

            for left_index in 0..cell_nodes.len() {
                for right_index in left_index + 1..cell_nodes.len() {
                    let left = cell_nodes[left_index];
                    let right = cell_nodes[right_index];
                    if pair_needs_collision_check(simulation, left, right, drag) {
                        resolve_collision_pair(simulation, left, right, drag, restitution);
                    }
                }
            }

            if col + 1 < cols {
                resolve_cell_pairs(
                    simulation,
                    cell_nodes,
                    &grid[cell + 1],
                    drag,
                    restitution,
                );
            }
            if row + 1 < rows {
                resolve_cell_pairs(
                    simulation,
                    cell_nodes,
                    &grid[(row + 1) * cols + col],
                    drag,
                    restitution,
                );
            }
            if col + 1 < cols && row + 1 < rows {
                resolve_cell_pairs(
                    simulation,
                    cell_nodes,
                    &grid[(row + 1) * cols + col + 1],
                    drag,
                    restitution,
                );
            }
            if col > 0 && row + 1 < rows {
                resolve_cell_pairs(
                    simulation,
                    cell_nodes,
                    &grid[(row + 1) * cols + col - 1],
                    drag,
                    restitution,
                );
            }
        }
    }

    simulation.collision_grid = grid;
}

fn resolve_cell_pairs(
    simulation: &mut LayoutSimulation,
    left_nodes: &[usize],
    right_nodes: &[usize],
    drag: Option<GraphPhysicsDragState>,
    restitution: f32,
) {
    for &left in left_nodes {
        for &right in right_nodes {
            if left == right {
                continue;
            }
            let (left, right) = if left < right {
                (left, right)
            } else {
                (right, left)
            };
            if pair_needs_collision_check(simulation, left, right, drag) {
                resolve_collision_pair(simulation, left, right, drag, restitution);
            }
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
    let distance_sq = delta.x * delta.x + delta.y * delta.y;
    let min_distance = simulation.sizes[left] + simulation.sizes[right];
    let min_distance_sq = min_distance * min_distance;
    if distance_sq >= min_distance_sq {
        return;
    }

    let distance = distance_sq.sqrt().max(MIN_DISTANCE);
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

pub(crate) fn settle_graph_within_viewport_bounds(
    simulation: &mut LayoutSimulation,
    viewport: &GraphViewport,
    panel_width: f32,
    panel_height: f32,
    padding: f32,
) {
    if viewport.scale <= 0.0 || panel_width <= 0.0 || panel_height <= 0.0 {
        return;
    }

    clear_graph_physics_velocities(simulation);
    let bounds = viewport_physics_bounds(viewport, panel_width, panel_height, padding);
    let config = GraphPhysicsConfig::for_node_count(simulation.positions.len());
    let iterations = config.solver_iterations * 4;

    for _ in 0..iterations {
        resolve_node_collisions(simulation, None, 0.0);
        clamp_boundary_positions(simulation, bounds);
        for index in 0..simulation.positions.len() {
            sanitize_node_state(simulation, index);
        }
    }
}

fn clamp_boundary_positions(simulation: &mut LayoutSimulation, bounds: GraphPhysicsBounds) {
    for index in 0..simulation.positions.len() {
        clamp_boundary_index(simulation, bounds, index);
    }
}

fn clamp_boundary_index(
    simulation: &mut LayoutSimulation,
    bounds: GraphPhysicsBounds,
    index: usize,
) {
    if index >= simulation.positions.len() || simulation.pinned[index] {
        return;
    }

    let radius = simulation.sizes[index];
    let position = &mut simulation.positions[index];

    if position.x - radius < bounds.min_x {
        position.x = bounds.min_x + radius;
    } else if position.x + radius > bounds.max_x {
        position.x = bounds.max_x - radius;
    }

    if position.y - radius < bounds.min_y {
        position.y = bounds.min_y + radius;
    } else if position.y + radius > bounds.max_y {
        position.y = bounds.max_y - radius;
    }
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
    }
}

#[derive(Clone, Copy)]
struct PointDelta {
    x: f32,
    y: f32,
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

    #[test]
    fn adaptive_config_reduces_iterations_for_large_graphs() {
        let config = GraphPhysicsConfig::for_node_count(50);
        assert!(config.solver_iterations < GraphPhysicsConfig::default().solver_iterations);
    }
}
