//! GPUI rendering and interaction for the workspace knowledge graph.

use std::collections::HashSet;
use std::path::PathBuf;

use gpui::*;

use super::graph_layout::{
    node_display_label, node_layout_radius, settle_graph_layout, smallest_node_layout_radius,
    GraphLayout, LayoutBounds, LayoutConfig, LayoutPoint, LayoutSimulation,
};
use super::graph_physics::{settle_graph_within_viewport_bounds, GraphPhysicsConfig};
use super::graph_model::{
    apply_graph_filter, GraphEdge, GraphEdgeKind, GraphFilter, GraphNode, GraphNodeId,
    GraphNodeKind, KnowledgeGraph,
};
use super::Editor;
use crate::theme::{Theme, ThemeManager};

const VIEWPORT_PADDING: f32 = 28.0;
const MIN_VIEWPORT_SCALE: f32 = 0.08;
const MAX_VIEWPORT_SCALE: f32 = 4.0;
const MIN_SCREEN_NODE_RADIUS: f32 = 16.0;
const MIN_SCREEN_LABEL_SIZE: f32 = 11.0;
const NODE_LABEL_PADDING: f32 = 8.0;
const CLICK_DRAG_THRESHOLD_PX: f32 = 4.0;
const ZOOM_LINE_SCALE: f32 = 0.08;
const GRAPH_EDGE_STROKE_BASE: f32 = 1.5;
const GRAPH_NODE_BORDER_BASE: f32 = 1.5;
const VIEWPORT_CULL_PADDING: f32 = 48.0;
const MAX_EDGES_WITHOUT_HOVER_CULL: usize = 3000;
pub(crate) const GRAPH_ANIMATION_FRAMES: u32 = 90;
pub(crate) const GRAPH_ANIMATION_FRAME_MS: u64 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GraphDragMode {
    None,
    Pan,
    Node,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GraphInteractionDrag {
    pub mode: GraphDragMode,
    pub node_id: Option<GraphNodeId>,
    pub start_screen: LayoutPoint,
    pub last_screen: LayoutPoint,
    pub drag_velocity: LayoutPoint,
}

impl Default for GraphInteractionDrag {
    fn default() -> Self {
        Self {
            mode: GraphDragMode::None,
            node_id: None,
            start_screen: LayoutPoint { x: 0.0, y: 0.0 },
            last_screen: LayoutPoint { x: 0.0, y: 0.0 },
            drag_velocity: LayoutPoint { x: 0.0, y: 0.0 },
        }
    }
}

impl GraphInteractionDrag {
    pub(crate) fn active(&self) -> bool {
        !matches!(self.mode, GraphDragMode::None)
    }

    fn movement_distance(&self) -> f32 {
        let dx = self.last_screen.x - self.start_screen.x;
        let dy = self.last_screen.y - self.start_screen.y;
        (dx * dx + dy * dy).sqrt()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct KnowledgeGraphViewState {
    pub raw_graph: KnowledgeGraph,
    pub graph: KnowledgeGraph,
    pub layout: GraphLayout,
    pub viewport: GraphViewport,
    pub filter: GraphFilter,
    pub simulation: Option<LayoutSimulation>,
    pub animating: bool,
    pub animation_progress: f32,
    pub hovered_node_id: Option<GraphNodeId>,
    pub pinned: HashSet<GraphNodeId>,
    pub drag: GraphInteractionDrag,
    pub last_bounds: Bounds<Pixels>,
    pub viewport_bounds_clamped: bool,
}

impl KnowledgeGraphViewState {
    pub(crate) fn new(raw_graph: KnowledgeGraph, filter: GraphFilter) -> Self {
        let graph = apply_graph_filter(&raw_graph, filter);
        let mut simulation = LayoutSimulation::new(&graph, LayoutConfig::default());
        settle_graph_layout(&mut simulation, LayoutConfig::default().iterations);
        let layout = simulation.to_layout();
        Self {
            raw_graph,
            graph,
            layout,
            viewport: GraphViewport::default(),
            filter,
            simulation: Some(simulation),
            animating: true,
            animation_progress: 0.0,
            hovered_node_id: None,
            pinned: HashSet::new(),
            drag: GraphInteractionDrag::default(),
            last_bounds: Bounds::default(),
            viewport_bounds_clamped: false,
        }
    }

    pub(crate) fn invalidate_viewport_bounds_clamp(&mut self) {
        self.viewport_bounds_clamped = false;
    }

    pub(crate) fn try_clamp_to_viewport_bounds(&mut self) {
        if self.viewport_bounds_clamped {
            return;
        }
        if self.viewport.scale <= 0.0 {
            return;
        }
        let panel_width = f32::from(self.last_bounds.size.width);
        let panel_height = f32::from(self.last_bounds.size.height);
        if panel_width <= 0.0 || panel_height <= 0.0 {
            return;
        }
        let Some(simulation) = self.simulation.as_mut() else {
            return;
        };
        let padding = GraphPhysicsConfig::default().boundary_padding;
        settle_graph_within_viewport_bounds(
            simulation,
            &self.viewport,
            panel_width,
            panel_height,
            padding,
        );
        simulation.sync_positions_to_layout(&mut self.layout);
        self.viewport_bounds_clamped = true;
    }

    pub(crate) fn refresh_filtered_graph(&mut self) {
        self.graph = apply_graph_filter(&self.raw_graph, self.filter);
        let mut simulation = LayoutSimulation::new(&self.graph, LayoutConfig::default());
        for (node_id, position) in &self.layout.positions {
            simulation.set_node_position(node_id, *position);
        }
        settle_graph_layout(&mut simulation, LayoutConfig::default().iterations);
        self.layout = simulation.to_layout();
        self.simulation = Some(simulation);
        self.animating = true;
        self.animation_progress = 0.0;
        self.viewport_bounds_clamped = false;
    }

    pub(crate) fn sync_layout_from_simulation(&mut self) {
        if let Some(simulation) = self.simulation.as_ref() {
            self.layout = simulation.to_layout();
        }
    }

    pub(crate) fn reset_viewport_fit(&mut self, viewport: Size<Pixels>) {
        self.viewport
            .fit_to_bounds(&self.layout.bounds, viewport, &self.graph);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GraphViewport {
    pub offset: LayoutPoint,
    pub scale: f32,
}

impl Default for GraphViewport {
    fn default() -> Self {
        Self {
            offset: LayoutPoint { x: 0.0, y: 0.0 },
            scale: 0.0,
        }
    }
}

impl GraphViewport {
    pub(crate) fn fit_to_bounds(
        &mut self,
        layout_bounds: &LayoutBounds,
        viewport: Size<Pixels>,
        graph: &KnowledgeGraph,
    ) {
        let content_width = (layout_bounds.max_x - layout_bounds.min_x).max(1.0);
        let content_height = (layout_bounds.max_y - layout_bounds.min_y).max(1.0);
        let available_width = f32::from(viewport.width) - VIEWPORT_PADDING * 2.0;
        let available_height = f32::from(viewport.height) - VIEWPORT_PADDING * 2.0;
        if available_width <= 0.0 || available_height <= 0.0 {
            self.scale = 1.0;
            return;
        }

        let fit_scale = (available_width / content_width)
            .min(available_height / content_height);
        let smallest_world_radius = smallest_node_layout_radius(graph);
        let readability_scale = MIN_SCREEN_NODE_RADIUS / smallest_world_radius;
        self.scale = fit_scale
            .max(readability_scale)
            .clamp(MIN_VIEWPORT_SCALE, MAX_VIEWPORT_SCALE);
        self.offset = LayoutPoint {
            x: VIEWPORT_PADDING - layout_bounds.min_x * self.scale,
            y: VIEWPORT_PADDING - layout_bounds.min_y * self.scale,
        };
    }

    pub(crate) fn world_to_screen_local(&self, world: LayoutPoint) -> LayoutPoint {
        LayoutPoint {
            x: world.x * self.scale + self.offset.x,
            y: world.y * self.scale + self.offset.y,
        }
    }

    pub(crate) fn screen_to_world(&self, screen: LayoutPoint) -> LayoutPoint {
        LayoutPoint {
            x: (screen.x - self.offset.x) / self.scale,
            y: (screen.y - self.offset.y) / self.scale,
        }
    }

    pub(crate) fn world_to_screen(
        &self,
        world: LayoutPoint,
        bounds: Bounds<Pixels>,
    ) -> Point<Pixels> {
        let local = self.world_to_screen_local(world);
        point(
            bounds.left() + px(local.x),
            bounds.top() + px(local.y),
        )
    }

    pub(crate) fn world_radius_to_pixels(&self, radius: f32) -> Pixels {
        px(radius * self.scale)
    }

    pub(crate) fn pan_by(&mut self, delta: LayoutPoint) {
        self.offset.x += delta.x;
        self.offset.y += delta.y;
    }

    pub(crate) fn zoom_at(&mut self, factor: f32, focus: LayoutPoint) {
        if self.scale <= 0.0 {
            return;
        }
        let world = self.screen_to_world(focus);
        self.scale = (self.scale * factor).clamp(MIN_VIEWPORT_SCALE, MAX_VIEWPORT_SCALE);
        let screen = self.world_to_screen_local(world);
        self.offset.x += focus.x - screen.x;
        self.offset.y += focus.y - screen.y;
    }
}

pub(crate) fn pointer_to_screen_local(
    position: Point<Pixels>,
    bounds: Bounds<Pixels>,
) -> LayoutPoint {
    LayoutPoint {
        x: f32::from(position.x) - f32::from(bounds.left()),
        y: f32::from(position.y) - f32::from(bounds.top()),
    }
}

pub(crate) fn hit_test_graph_node<'a>(
    graph: &'a KnowledgeGraph,
    layout: &GraphLayout,
    viewport: &GraphViewport,
    screen_local: LayoutPoint,
) -> Option<&'a GraphNode> {
    if viewport.scale <= 0.0 {
        return None;
    }

    let world = viewport.screen_to_world(screen_local);
    let mut best: Option<(f32, &'a GraphNode)> = None;

    for node in &graph.nodes {
        let Some(position) = layout.positions.get(&node.id) else {
            continue;
        };
        let radius = node_layout_radius(&node.kind);
        let dx = world.x - position.x;
        let dy = world.y - position.y;
        let distance = (dx * dx + dy * dy).sqrt();
        if distance <= radius {
            match best {
                Some((best_distance, _)) if distance >= best_distance => {}
                _ => best = Some((distance, node)),
            }
        }
    }

    best.map(|(_, node)| node)
}

#[cfg(test)]
pub(crate) fn count_visible_graph_items(
    graph: &KnowledgeGraph,
    layout: &GraphLayout,
) -> (usize, usize) {
    let edge_count = graph
        .edges
        .iter()
        .filter(|edge| {
            layout.positions.contains_key(&edge.source)
                && layout.positions.contains_key(&edge.target)
        })
        .count();
    let node_count = graph
        .nodes
        .iter()
        .filter(|node| layout.positions.contains_key(&node.id))
        .count();
    (edge_count, node_count)
}

struct KnowledgeGraphElement {
    editor: WeakEntity<Editor>,
}

struct GraphPrepaintState {
    background: PaintQuad,
    edges: Vec<(Path<Pixels>, Hsla)>,
    nodes: Vec<PaintQuad>,
    labels: Vec<(ShapedLine, Point<Pixels>, Pixels)>,
    hitbox: Hitbox,
}

#[derive(Default)]
struct GraphLayoutState;

fn empty_graph_prepaint_state(
    bounds: Bounds<Pixels>,
    theme: &Theme,
    window: &mut Window,
) -> GraphPrepaintState {
    GraphPrepaintState {
        background: fill(bounds, theme.colors.graph_background),
        edges: Vec::new(),
        nodes: Vec::new(),
        labels: Vec::new(),
        hitbox: window.insert_hitbox(bounds, HitboxBehavior::Normal),
    }
}

fn world_point_visible_in_viewport(
    world: LayoutPoint,
    world_radius: f32,
    viewport: &GraphViewport,
    bounds: Bounds<Pixels>,
) -> bool {
    let center = viewport.world_to_screen(world, bounds);
    let radius = f32::from(viewport.world_radius_to_pixels(world_radius)) + VIEWPORT_CULL_PADDING;
    let cx = f32::from(center.x);
    let cy = f32::from(center.y);
    cx + radius >= f32::from(bounds.left())
        && cx - radius <= f32::from(bounds.right())
        && cy + radius >= f32::from(bounds.top())
        && cy - radius <= f32::from(bounds.bottom())
}

fn pixel_point_is_finite(point: Point<Pixels>) -> bool {
    f32::from(point.x).is_finite() && f32::from(point.y).is_finite()
}

fn node_border_color(fill: Hsla) -> Hsla {
    Hsla {
        h: fill.h,
        s: fill.s,
        l: (fill.l * 0.55).clamp(0.0, 1.0),
        a: fill.a,
    }
}

fn build_graph_edge_path(
    source: Point<Pixels>,
    target: Point<Pixels>,
    stroke_width: Pixels,
) -> Option<Path<Pixels>> {
    if !pixel_point_is_finite(source) || !pixel_point_is_finite(target) {
        return None;
    }
    let mut builder = PathBuilder::stroke(stroke_width);
    builder.move_to(source);
    builder.line_to(target);
    builder.build().ok()
}

fn edge_visible_for_render(
    edge: &GraphEdge,
    positions: &std::collections::HashMap<GraphNodeId, LayoutPoint>,
    viewport: &GraphViewport,
    bounds: Bounds<Pixels>,
    graph: &KnowledgeGraph,
    hovered_node_id: Option<&GraphNodeId>,
) -> bool {
    let Some(source_world) = positions.get(&edge.source) else {
        return false;
    };
    let Some(target_world) = positions.get(&edge.target) else {
        return false;
    };

    let source_radius = graph
        .nodes
        .iter()
        .find(|node| node.id == edge.source)
        .map(|node| node_layout_radius(&node.kind))
        .unwrap_or(0.0);
    let target_radius = graph
        .nodes
        .iter()
        .find(|node| node.id == edge.target)
        .map(|node| node_layout_radius(&node.kind))
        .unwrap_or(0.0);

    let source_visible =
        world_point_visible_in_viewport(*source_world, source_radius, viewport, bounds);
    let target_visible =
        world_point_visible_in_viewport(*target_world, target_radius, viewport, bounds);
    if !source_visible && !target_visible {
        return false;
    }

    if graph.edges.len() <= MAX_EDGES_WITHOUT_HOVER_CULL {
        return true;
    }

    let Some(hovered_node_id) = hovered_node_id else {
        return false;
    };

    edge.source == *hovered_node_id || edge.target == *hovered_node_id
}

fn prepaint_graph(
    bounds: Bounds<Pixels>,
    state: &mut KnowledgeGraphViewState,
    theme: &Theme,
    window: &mut Window,
    active_document_node_id: Option<&GraphNodeId>,
) -> GraphPrepaintState {
    if state.viewport.scale <= 0.0 {
        state
            .viewport
            .fit_to_bounds(&state.layout.bounds, bounds.size, &state.graph);
    }
    state.last_bounds = bounds;

    let background = fill(bounds, theme.colors.graph_background);
    let positions = &state.layout.positions;
    let viewport = &state.viewport;
    let edge_alpha = if state.animating {
        0.35 + 0.65 * state.animation_progress.clamp(0.0, 1.0)
    } else {
        1.0
    };
    let stroke_width = px(
        (GRAPH_EDGE_STROKE_BASE * viewport.scale.max(0.25))
            .clamp(1.0, 3.0),
    );
    let node_border_width = px(
        (GRAPH_NODE_BORDER_BASE * viewport.scale.max(0.35))
            .clamp(1.0, 2.0),
    );

    let mut edges = Vec::new();
    for edge in &state.graph.edges {
        if !edge_visible_for_render(
            edge,
            positions,
            viewport,
            bounds,
            &state.graph,
            state.hovered_node_id.as_ref(),
        ) && !state.animating
        {
            continue;
        }

        let Some(source_world) = positions.get(&edge.source) else {
            continue;
        };
        let Some(target_world) = positions.get(&edge.target) else {
            continue;
        };
        if !source_world.is_finite() || !target_world.is_finite() {
            continue;
        }
        let source = viewport.world_to_screen(*source_world, bounds);
        let target = viewport.world_to_screen(*target_world, bounds);
        let Some(path) = build_graph_edge_path(source, target, stroke_width) else {
            continue;
        };
        let edge_color = match edge.kind {
            GraphEdgeKind::WikiLink => theme.colors.graph_edge.opacity(edge_alpha),
            GraphEdgeKind::Tagged => theme.colors.graph_edge.opacity(0.75 * edge_alpha),
        };
        edges.push((path, edge_color));
    }

    let mut nodes = Vec::new();
    let mut labels = Vec::new();
    let window_style = window.text_style();

    for node in &state.graph.nodes {
        let Some(world) = positions.get(&node.id) else {
            continue;
        };
        if !world.is_finite() {
            continue;
        }
        let world_radius = node_layout_radius(&node.kind);
        if !state.animating
            && !world_point_visible_in_viewport(*world, world_radius, viewport, bounds)
        {
            continue;
        }
        let radius = viewport.world_radius_to_pixels(world_radius).max(px(4.0));
        let center = viewport.world_to_screen(*world, bounds);
        let node_bounds = Bounds::new(
            point(center.x - radius, center.y - radius),
            size(radius * 2.0, radius * 2.0),
        );
        let color = match &node.kind {
            GraphNodeKind::Document { .. } => theme.colors.graph_node_document,
            GraphNodeKind::Tag { .. } => theme.colors.graph_node_tag,
        };
        let is_active_document =
            active_document_node_id.is_some_and(|active_id| active_id == &node.id);
        let mut quad = fill(node_bounds, color);
        quad.corner_radii = Corners::all(radius);
        quad.border_widths = Edges::all(if is_active_document {
            node_border_width * 1.5
        } else {
            node_border_width
        });
        quad.border_color = if is_active_document {
            theme.colors.selection.opacity(0.85)
        } else {
            node_border_color(color)
        };
        nodes.push(quad);

        let label_text: SharedString = node_display_label(&node.kind).into();
        let max_label_width = (radius * 2.0 * 0.82 - px(NODE_LABEL_PADDING)).max(px(8.0));
        let label_font_size = (f32::from(radius) * 0.48)
            .max(MIN_SCREEN_LABEL_SIZE);
        let font_size = px(label_font_size);
        let mut runs = vec![TextRun {
            len: label_text.len(),
            font: window_style.font(),
            color: white(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }];
        let shaped = if max_label_width > px(0.0) {
            let mut wrapper = window.text_system().line_wrapper(window_style.font(), font_size);
            wrapper.truncate_line(label_text, max_label_width, "…", &mut runs)
        } else {
            label_text
        };
        let shaped_line = window
            .text_system()
            .shape_line(shaped, font_size, &runs, None);
        let label_origin = point(
            center.x - px(f32::from(shaped_line.width) / 2.0),
            center.y - font_size / 2.0,
        );
        labels.push((shaped_line, label_origin, font_size));
    }

    let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);

    GraphPrepaintState {
        background,
        edges,
        nodes,
        labels,
        hitbox,
    }
}

pub(crate) fn render_knowledge_graph_panel(editor: WeakEntity<Editor>) -> impl IntoElement {
    let interaction_editor = editor.clone();
    div()
        .id("knowledge-graph-panel")
        .relative()
        .size_full()
        .child(KnowledgeGraphElement { editor })
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .occlude()
                .on_mouse_down(MouseButton::Left, {
                    let editor = interaction_editor.clone();
                    move |event, window, cx| {
                        cx.stop_propagation();
                        let _ = editor.update(cx, |editor, cx| {
                            editor.on_knowledge_graph_mouse_down(event, window, cx);
                        });
                    }
                })
                .on_scroll_wheel({
                    let editor = interaction_editor.clone();
                    move |event, window, cx| {
                        cx.stop_propagation();
                        let _ = editor.update(cx, |editor, cx| {
                            editor.on_knowledge_graph_scroll_wheel(event, window, cx);
                        });
                    }
                })
                .child(
                    canvas(
                        move |_, _, _| (),
                        move |_, _, window, _| {
                            window.on_mouse_event({
                                let editor = interaction_editor.clone();
                                move |event: &MouseMoveEvent, phase, window, cx| {
                                    if !phase.bubble() {
                                        return;
                                    }
                                    let _ = editor.update(cx, |editor, cx| {
                                        editor.on_knowledge_graph_mouse_move(event, window, cx);
                                    });
                                }
                            });

                            window.on_mouse_event({
                                let editor = interaction_editor;
                                move |event: &MouseUpEvent, phase, window, cx| {
                                    if !phase.bubble() {
                                        return;
                                    }
                                    let _ = editor.update(cx, |editor, cx| {
                                        editor.on_knowledge_graph_mouse_up(event, window, cx);
                                    });
                                }
                            });
                        },
                    )
                    .size_full(),
                ),
        )
}

impl IntoElement for KnowledgeGraphElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for KnowledgeGraphElement {
    type RequestLayoutState = GraphLayoutState;
    type PrepaintState = GraphPrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), GraphLayoutState)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let theme = cx.global::<ThemeManager>().current_arc();
        self.editor
            .update(cx, |editor, cx| {
                let active_id = editor.active_knowledge_graph_document_node_id();
                let Some(state) = editor.knowledge_graph_view.as_mut() else {
                    return empty_graph_prepaint_state(bounds, &theme, window);
                };

                let needs_initial_fit = state.viewport.scale <= 0.0;
                let panel_resized = state.last_bounds.size.width != bounds.size.width
                    || state.last_bounds.size.height != bounds.size.height;

                if needs_initial_fit {
                    state.viewport.fit_to_bounds(
                        &state.layout.bounds,
                        bounds.size,
                        &state.graph,
                    );
                    state.invalidate_viewport_bounds_clamp();
                } else if panel_resized && !state.drag.active() {
                    state.viewport.fit_to_bounds(
                        &state.layout.bounds,
                        bounds.size,
                        &state.graph,
                    );
                    state.invalidate_viewport_bounds_clamp();
                }

                state.last_bounds = bounds;
                if !matches!(state.drag.mode, GraphDragMode::Node) {
                    let clamped_before = state.viewport_bounds_clamped;
                    state.try_clamp_to_viewport_bounds();
                    if state.viewport_bounds_clamped && !clamped_before {
                        cx.notify();
                    }
                }

                prepaint_graph(
                    bounds,
                    state,
                    &theme,
                    window,
                    active_id.as_ref(),
                )
            })
            .unwrap_or_else(|_| empty_graph_prepaint_state(bounds, &theme, window))
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.paint_quad(prepaint.background.clone());

        for (path, color) in prepaint.edges.drain(..) {
            window.paint_path(path, color);
        }

        for node in prepaint.nodes.drain(..) {
            window.paint_quad(node);
        }

        for (line, origin, font_size) in prepaint.labels.drain(..) {
            let _ = line.paint(origin, font_size, window, cx);
        }

        if prepaint.hitbox.is_hovered(window) {
            if let Ok(Some(cursor)) = self
                .editor
                .update(cx, |editor, _| editor.knowledge_graph_cursor_style())
            {
                window.set_cursor_style(cursor, &prepaint.hitbox);
            }
        }
    }
}

pub(super) enum GraphNodeClickAction {
    Document(PathBuf),
    Tag(String),
}

impl Editor {
    pub(super) fn active_knowledge_graph_document_node_id(&self) -> Option<GraphNodeId> {
        let root = self.effective_workspace_root()?;
        let file_path = self.file_path.as_ref()?;
        let node_id = super::graph_model::workspace_document_node_id(&root, file_path)?;
        self.knowledge_graph_view
            .as_ref()?
            .graph
            .nodes
            .iter()
            .any(|node| node.id == node_id)
            .then_some(node_id)
    }

    pub(super) fn knowledge_graph_cursor_style(&self) -> Option<CursorStyle> {
        let state = self.knowledge_graph_view.as_ref()?;
        if state.drag.active() {
            return Some(match state.drag.mode {
                GraphDragMode::Node => CursorStyle::PointingHand,
                GraphDragMode::Pan | GraphDragMode::None => CursorStyle::ClosedHand,
            });
        }

        if hit_test_graph_node(
            &state.graph,
            &state.layout,
            &state.viewport,
            state.drag.last_screen,
        )
        .is_some()
        {
            Some(CursorStyle::PointingHand)
        } else {
            Some(CursorStyle::OpenHand)
        }
    }

    pub(super) fn on_knowledge_graph_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.knowledge_graph_view.as_mut() else {
            return;
        };
        let screen = pointer_to_screen_local(event.position, state.last_bounds);
        state.drag.start_screen = screen;
        state.drag.last_screen = screen;

        if let Some(node) = hit_test_graph_node(&state.graph, &state.layout, &state.viewport, screen)
        {
            state.drag.mode = GraphDragMode::Node;
            state.drag.node_id = Some(node.id.clone());
            state.pinned.insert(node.id.clone());
            if let Some(simulation) = state.simulation.as_mut() {
                simulation.pin_node(&node.id);
            }
        } else {
            state.drag.mode = GraphDragMode::Pan;
            state.drag.node_id = None;
        }

        cx.notify();
    }

    pub(super) fn on_knowledge_graph_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.knowledge_graph_view.as_mut() else {
            return;
        };

        let screen = pointer_to_screen_local(event.position, state.last_bounds);
        if !state.drag.active() {
            state.hovered_node_id = hit_test_graph_node(
                &state.graph,
                &state.layout,
                &state.viewport,
                screen,
            )
            .map(|node| node.id.clone());
            state.drag.last_screen = screen;
            cx.notify();
            return;
        }

        if !event.dragging() {
            state.drag = GraphInteractionDrag::default();
            cx.notify();
            return;
        }

        let delta = LayoutPoint {
            x: screen.x - state.drag.last_screen.x,
            y: screen.y - state.drag.last_screen.y,
        };
        state.drag.last_screen = screen;

        match state.drag.mode {
            GraphDragMode::Pan => {
                state.viewport.pan_by(delta);
            }
            GraphDragMode::Node => {
                let Some(node_id) = state.drag.node_id.clone() else {
                    cx.notify();
                    return;
                };
                if state.viewport.scale <= 0.0 {
                    cx.notify();
                    return;
                }
                let world_delta = LayoutPoint {
                    x: delta.x / state.viewport.scale,
                    y: delta.y / state.viewport.scale,
                };
                if !world_delta.is_finite() {
                    cx.notify();
                    return;
                }
                state.drag.drag_velocity = LayoutPoint {
                    x: world_delta.x / super::graph_physics::GRAPH_PHYSICS_DT,
                    y: world_delta.y / super::graph_physics::GRAPH_PHYSICS_DT,
                };
                if let Some(position) = state.layout.positions.get_mut(&node_id) {
                    position.x += world_delta.x;
                    position.y += world_delta.y;
                    if !position.is_finite() {
                        cx.notify();
                        return;
                    }
                }
                if let Some(simulation) = state.simulation.as_mut() {
                    if let Some(position) = state.layout.positions.get(&node_id) {
                        simulation.set_node_position(&node_id, *position);
                    }
                }
                let _ = state;
                self.run_knowledge_graph_physics_step(cx);
                cx.notify();
                return;
            }
            GraphDragMode::None => {}
        }

        cx.notify();
    }

    pub(super) fn on_knowledge_graph_mouse_up(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.knowledge_graph_view.as_mut() else {
            return;
        };

        if !state.drag.active() {
            return;
        }

        let screen = pointer_to_screen_local(event.position, state.last_bounds);
        state.drag.last_screen = screen;

        let clicked_node = if matches!(state.drag.mode, GraphDragMode::Node)
            && state.drag.movement_distance() <= CLICK_DRAG_THRESHOLD_PX
        {
            state.drag.node_id.clone()
        } else {
            None
        };

        if matches!(state.drag.mode, GraphDragMode::Node) {
            if let Some(node_id) = state.drag.node_id.clone() {
                let release_velocity = state.drag.drag_velocity;
                if let Some(simulation) = state.simulation.as_mut() {
                    simulation.set_node_velocity(&node_id, release_velocity);
                }
                state.pinned.remove(&node_id);
                if let Some(simulation) = state.simulation.as_mut() {
                    simulation.unpin_node(&node_id);
                }
            }
        }

        state.drag = GraphInteractionDrag::default();
        cx.notify();

        self.ensure_knowledge_graph_physics_loop(cx);

        let Some(node_id) = clicked_node else {
            return;
        };
        let click_action = self.knowledge_graph_view.as_ref().and_then(|state| {
            state.graph.nodes.iter().find(|node| node.id == node_id).map(|node| {
                match &node.kind {
                    GraphNodeKind::Document { path, .. } => {
                        GraphNodeClickAction::Document(path.clone())
                    }
                    GraphNodeKind::Tag { name, .. } => GraphNodeClickAction::Tag(name.clone()),
                }
            })
        });
        let Some(action) = click_action else {
            return;
        };
        self.apply_knowledge_graph_node_click(action, window, cx);
    }

    pub(super) fn on_knowledge_graph_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.knowledge_graph_view.as_mut() else {
            return;
        };

        let line_height = px(16.0);
        let delta = event.delta.pixel_delta(line_height);
        let factor = (1.0 - f32::from(delta.y) * ZOOM_LINE_SCALE).clamp(0.8, 1.25);
        let focus = pointer_to_screen_local(event.position, state.last_bounds);
        state.viewport.zoom_at(factor, focus);
        cx.notify();
    }

    pub(super) fn apply_knowledge_graph_node_click(
        &mut self,
        action: GraphNodeClickAction,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.graph_only_window {
            if let Some(parent) = self.graph_popout_parent.clone() {
                let _ = parent.update(cx, move |parent_editor, cx| {
                    parent_editor.pending_graph_popout_action = Some(action);
                    cx.notify();
                });
            }
            return;
        }

        match action {
            GraphNodeClickAction::Document(path) => {
                self.open_workspace_path(path, _window, cx);
            }
            GraphNodeClickAction::Tag(name) => {
                self.filter_workspace_by_tag(name, cx);
            }
        }
    }
}

#[cfg(test)]
mod graph_view_tests {
    use std::path::PathBuf;

    use gpui::{point, px, size, Bounds};

    use crate::editor::graph_layout::{
        compute_graph_layout, smallest_node_layout_radius, GraphLayout, LayoutBounds, LayoutConfig,
        LayoutPoint,
    };
    use crate::editor::graph_model::{
        build_knowledge_graph, GraphFilter, GraphNode, GraphNodeId, GraphNodeKind, KnowledgeGraph,
    };
    use crate::editor::graph_view::{
        count_visible_graph_items, hit_test_graph_node, GraphViewport, KnowledgeGraphViewState,
        MIN_SCREEN_NODE_RADIUS,
    };
    use crate::editor::link_index::{refresh_link_index_for_file, WorkspaceLinkIndex};
    use crate::editor::tag_index::{refresh_tag_index_for_file, WorkspaceTagIndex};

    fn sample_graph_and_layout() -> (KnowledgeGraph, GraphLayout) {
        let root = std::env::temp_dir().join(format!(
            "markman-graph-view-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create temp dir");
        let note_a = root.join("a.md");
        let note_b = root.join("b.md");
        std::fs::write(&note_a, "topic #shared\nsee [[b.md]]\n").expect("write a");
        std::fs::write(&note_b, "also #shared\n").expect("write b");

        let mut tag_index = WorkspaceTagIndex::default();
        refresh_tag_index_for_file(&mut tag_index, &note_a, "topic #shared\nsee [[b.md]]\n");
        refresh_tag_index_for_file(&mut tag_index, &note_b, "also #shared\n");
        let mut link_index = WorkspaceLinkIndex::default();
        refresh_link_index_for_file(
            &mut link_index,
            &note_a,
            "topic #shared\nsee [[b.md]]\n",
        );

        let graph = build_knowledge_graph(&root, &tag_index, &link_index);
        let layout = compute_graph_layout(&graph, &LayoutConfig::default());
        let _ = std::fs::remove_dir_all(&root);
        (graph, layout)
    }

    #[test]
    fn viewport_fit_produces_positive_scale() {
        let (graph, layout) = sample_graph_and_layout();
        let mut viewport = GraphViewport::default();
        viewport.fit_to_bounds(&layout.bounds, size(px(320.0), px(240.0)), &graph);
        assert!(viewport.scale > 0.0);
    }

    #[test]
    fn viewport_fit_keeps_smallest_node_readable() {
        let (graph, layout) = sample_graph_and_layout();
        let mut viewport = GraphViewport::default();
        viewport.fit_to_bounds(&layout.bounds, size(px(320.0), px(240.0)), &graph);
        let smallest = smallest_node_layout_radius(&graph);
        let screen_radius = smallest * viewport.scale;
        assert!(screen_radius >= MIN_SCREEN_NODE_RADIUS - 0.01);
    }

    #[test]
    fn visible_item_counts_match_graph() {
        let (graph, layout) = sample_graph_and_layout();
        let (edge_count, node_count) = count_visible_graph_items(&graph, &layout);
        assert_eq!(node_count, graph.nodes.len());
        assert_eq!(edge_count, graph.edges.len());
    }

    #[test]
    fn screen_to_world_roundtrip_is_consistent() {
        let viewport = GraphViewport {
            offset: LayoutPoint { x: 20.0, y: 30.0 },
            scale: 1.5,
        };
        let screen = LayoutPoint { x: 140.0, y: 90.0 };
        let world = viewport.screen_to_world(screen);
        let roundtrip = viewport.world_to_screen_local(world);
        assert!((roundtrip.x - screen.x).abs() < 0.001);
        assert!((roundtrip.y - screen.y).abs() < 0.001);
    }

    #[test]
    fn zoom_at_keeps_focus_point_stable() {
        let mut viewport = GraphViewport {
            offset: LayoutPoint { x: 10.0, y: 20.0 },
            scale: 1.0,
        };
        let focus = LayoutPoint { x: 100.0, y: 80.0 };
        let world_before = viewport.screen_to_world(focus);
        viewport.zoom_at(1.4, focus);
        let world_after = viewport.screen_to_world(focus);
        assert!((world_before.x - world_after.x).abs() < 0.001);
        assert!((world_before.y - world_after.y).abs() < 0.001);
    }

    #[test]
    fn hit_test_selects_node_under_pointer() {
        let (graph, _layout) = sample_graph_and_layout();
        let mut state = KnowledgeGraphViewState::new(graph, GraphFilter::ConnectedOnly);
        state.viewport = GraphViewport {
            offset: LayoutPoint { x: 0.0, y: 0.0 },
            scale: 1.0,
        };
        let tag_id = GraphNodeId::tag("shared");
        let tag_world = state.layout.positions[&tag_id];
        let screen = state.viewport.world_to_screen_local(tag_world);
        let hit = hit_test_graph_node(&state.graph, &state.layout, &state.viewport, screen);
        assert!(hit.is_some());
        assert_eq!(hit.unwrap().id, tag_id);
    }

    #[test]
    fn triangle_layout_world_to_screen_stays_inside_viewport() {
        let _nodes = vec![
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
        ];
        let layout = GraphLayout {
            positions: [
                (
                    GraphNodeId::document("a.md"),
                    LayoutPoint { x: 0.0, y: 0.0 },
                ),
                (
                    GraphNodeId::document("b.md"),
                    LayoutPoint { x: 100.0, y: 80.0 },
                ),
            ]
            .into_iter()
            .collect(),
            bounds: LayoutBounds {
                min_x: 0.0,
                min_y: 0.0,
                max_x: 100.0,
                max_y: 80.0,
            },
        };
        let graph = KnowledgeGraph {
            nodes: _nodes,
            edges: Vec::new(),
            broken_wiki_links: 0,
            revision: 1,
        };
        let mut viewport = GraphViewport::default();
        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(200.0), px(160.0)));
        viewport.fit_to_bounds(&layout.bounds, bounds.size, &graph);
        let first = viewport.world_to_screen(LayoutPoint { x: 0.0, y: 0.0 }, bounds);
        let second = viewport.world_to_screen(LayoutPoint { x: 100.0, y: 80.0 }, bounds);
        assert!(f32::from(first.x) >= f32::from(bounds.left()));
        assert!(f32::from(second.x) <= f32::from(bounds.right()));
    }
}
