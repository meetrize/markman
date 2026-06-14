//! Workspace knowledge graph index sync and sidebar panel rendering.

use std::path::Path;

use gpui::*;
use gpui::prelude::FluentBuilder;

use super::graph_layout::{LayoutConfig, LayoutSimulation, uncross_graph_layout};
use super::graph_physics::{
    clear_graph_physics_velocities, step_graph_physics, viewport_physics_bounds,
    GraphPhysicsConfig, GraphPhysicsDragState, GRAPH_PHYSICS_DT, GRAPH_PHYSICS_FRAME_MS,
};
use super::graph_model::{apply_graph_filter, build_knowledge_graph, GraphFilter};
use super::graph_view::{
    render_knowledge_graph_panel, GraphViewport, KnowledgeGraphViewState,
    ACTIVE_NODE_PULSE_FRAME_MS, ACTIVE_NODE_PULSE_PHASE_STEP, GRAPH_ANIMATION_FRAME_MS,
    GRAPH_ANIMATION_FRAMES,
};
use super::link_index::{
    build_workspace_link_index, refresh_link_index_for_file, WorkspaceLinkIndex,
};
use super::tag_index::WorkspaceTagIndex;
use super::Editor;
use crate::app_identity::app_window_title;
use crate::i18n::I18nManager;
use crate::i18n::I18nStrings;
use crate::theme::{Theme, ThemeColors, ThemeTypography};
use crate::window_chrome::velotype_window_options;

const GRAPH_REPEL_ICON: &str = "icon/workspace/graph-repel.svg";
const GRAPH_PHYSICS_ICON: &str = "icon/workspace/graph-physics.svg";
const GRAPH_UNCROSS_ICON: &str = "icon/workspace/graph-uncross.svg";
const GRAPH_FIT_ICON: &str = "icon/workspace/graph-fit.svg";
const GRAPH_RESET_ICON: &str = "icon/workspace/graph-reset.svg";
const GRAPH_POPOUT_ICON: &str = "icon/workspace/graph-popout.svg";
const GRAPH_TOOLBAR_ICON_SIZE: f32 = 12.0;

fn graph_source_revision(tag_index: &WorkspaceTagIndex, link_index: &WorkspaceLinkIndex) -> u64 {
    tag_index
        .revision
        .wrapping_mul(1_000)
        .wrapping_add(link_index.revision)
}

impl Editor {
    pub(super) fn clear_workspace_graph_state(&mut self) {
        self.workspace.state.link_index = None;
        self.workspace.state.link_index_root = None;
        self.workspace.state.link_index_busy = false;
        self.workspace.state.graph_revision = None;
        self.workspace.state.graph_busy = false;
        self.graph_animation_task = None;
        self.graph_active_node_pulse_task = None;
        self.graph_physics_task = None;
        self.knowledge_graph_view = None;
    }

    pub(super) fn sync_workspace_link_index(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.effective_workspace_root() else {
            self.workspace.state.link_index = None;
            self.workspace.state.link_index_root = None;
            self.workspace.state.link_index_busy = false;
            self.sync_knowledge_graph(cx);
            return;
        };

        if self.workspace.state.link_index_root.as_deref() == Some(root.as_path())
            && self.workspace.state.link_index.is_some()
        {
            return;
        }

        self.workspace.state.link_index = None;
        self.workspace.state.link_index_root = Some(root.clone());
        self.workspace.state.link_index_busy = true;

        let editor = cx.entity().downgrade();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let index = build_workspace_link_index(&root);
            let _ = editor.update(cx, |editor, cx| {
                if editor.effective_workspace_root().as_deref() != Some(root.as_path()) {
                    return;
                }
                editor.workspace.state.link_index = Some(index);
                editor.workspace.state.link_index_busy = false;
                editor.sync_knowledge_graph(cx);
            });
        })
        .detach();
    }

    pub(super) fn sync_knowledge_graph(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.effective_workspace_root() else {
            self.knowledge_graph_view = None;
            self.workspace.state.graph_revision = None;
            self.workspace.state.graph_busy = false;
            self.graph_animation_task = None;
            return;
        };

        if self.workspace.state.tag_index_busy || self.workspace.state.link_index_busy {
            return;
        }

        let (Some(tag_index), Some(link_index)) = (
            self.workspace.state.tag_index.as_ref(),
            self.workspace.state.link_index.as_ref(),
        ) else {
            self.knowledge_graph_view = None;
            self.workspace.state.graph_revision = None;
            self.workspace.state.graph_busy = false;
            self.graph_animation_task = None;
            return;
        };

        let revision = graph_source_revision(tag_index, link_index);
        if self.workspace.state.graph_revision == Some(revision)
            && self.knowledge_graph_view.is_some()
        {
            return;
        }

        let filter = self
            .knowledge_graph_view
            .as_ref()
            .map(|state| state.filter)
            .unwrap_or_default();
        let tag_index = tag_index.clone();
        let link_index = link_index.clone();
        self.workspace.state.graph_busy = true;
        self.graph_animation_task = None;

        let editor = cx.entity().downgrade();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let raw_graph = build_knowledge_graph(&root, &tag_index, &link_index);
            let _ = editor.update(cx, |editor, cx| {
                if editor.effective_workspace_root().as_deref() != Some(root.as_path()) {
                    return;
                }
                editor.workspace.state.graph_busy = false;
                editor.apply_built_knowledge_graph(raw_graph, revision, filter, cx);
            });
        })
        .detach();
    }

    pub(super) fn apply_built_knowledge_graph(
        &mut self,
        raw_graph: super::graph_model::KnowledgeGraph,
        revision: u64,
        filter: GraphFilter,
        cx: &mut Context<Self>,
    ) {
        if raw_graph.nodes.is_empty() || apply_graph_filter(&raw_graph, filter).nodes.is_empty() {
            self.knowledge_graph_view = None;
            self.workspace.state.graph_revision = Some(revision);
            cx.notify();
            return;
        }

        let mutual_repulsion = self
            .knowledge_graph_view
            .as_ref()
            .map(|state| state.mutual_repulsion)
            .unwrap_or(false);
        let physics_collisions = self
            .knowledge_graph_view
            .as_ref()
            .map(|state| state.physics_collisions)
            .unwrap_or(false);
        self.knowledge_graph_view = Some(KnowledgeGraphViewState::new(raw_graph, filter));
        if let Some(state) = self.knowledge_graph_view.as_mut() {
            state.mutual_repulsion = mutual_repulsion;
            state.physics_collisions = physics_collisions;
            if state.mutual_repulsion {
                state.apply_mutual_repulsion(None);
            }
        }
        self.workspace.state.graph_revision = Some(revision);
        self.start_knowledge_graph_animation(cx);
        cx.notify();
    }

    pub(super) fn start_knowledge_graph_animation(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.knowledge_graph_view.as_mut() else {
            return;
        };

        if state.simulation.is_none() {
            let mut simulation = LayoutSimulation::new(&state.graph, LayoutConfig::default());
            for (node_id, position) in &state.layout.positions {
                simulation.set_node_position(node_id, *position);
            }
            state.simulation = Some(simulation);
        }

        state.animating = true;
        state.animation_progress = 0.0;
        self.graph_animation_task = None;

        let editor = cx.entity().downgrade();
        self.graph_animation_task = Some(cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
                for frame in 0..GRAPH_ANIMATION_FRAMES {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(GRAPH_ANIMATION_FRAME_MS))
                        .await;

                    let finished = editor
                        .update(cx, |editor, cx| {
                            let Some(state) = editor.knowledge_graph_view.as_mut() else {
                                return true;
                            };
                            let Some(_simulation) = state.simulation.as_mut() else {
                                state.animating = false;
                                return true;
                            };

                            state.animation_progress =
                                (frame as f32 + 1.0) / GRAPH_ANIMATION_FRAMES as f32;
                            state.animating = frame + 1 < GRAPH_ANIMATION_FRAMES;
                            cx.notify();
                            !state.animating
                        })
                        .unwrap_or(true);

                    if finished {
                        break;
                    }
                }

                let _ = editor.update(cx, |editor, cx| {
                    editor.graph_animation_task = None;
                    if let Some(state) = editor.knowledge_graph_view.as_mut() {
                        state.animating = false;
                        state.animation_progress = 1.0;
                        state.sync_layout_from_simulation();
                        if state.last_bounds.size.width > px(0.0)
                            && state.last_bounds.size.height > px(0.0)
                        {
                            state.reset_viewport_fit(state.last_bounds.size);
                        } else {
                            state.viewport = GraphViewport::default();
                        }
                    }
                    cx.notify();
                });
            },
        ));
    }

    pub(super) fn ensure_knowledge_graph_active_node_pulse(&mut self, cx: &mut Context<Self>) {
        if self.knowledge_graph_view.is_none() || self.graph_active_node_pulse_task.is_some() {
            return;
        }

        let phase_step = ACTIVE_NODE_PULSE_PHASE_STEP;
        let editor = cx.entity().downgrade();
        self.graph_active_node_pulse_task = Some(cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(
                            ACTIVE_NODE_PULSE_FRAME_MS,
                        ))
                        .await;

                    let keep_going = editor
                        .update(cx, |editor, cx| {
                            let Some(state) = editor.knowledge_graph_view.as_mut() else {
                                return false;
                            };
                            state.active_node_pulse_phase =
                                (state.active_node_pulse_phase + phase_step)
                                    % std::f32::consts::TAU;
                            cx.notify();
                            true
                        })
                        .unwrap_or(false);

                    if !keep_going {
                        break;
                    }
                }

                let _ = editor.update(cx, |editor, _| {
                    editor.graph_active_node_pulse_task = None;
                });
            },
        ));
    }

    pub(super) fn set_knowledge_graph_filter(
        &mut self,
        filter: GraphFilter,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.knowledge_graph_view.as_mut() else {
            return;
        };
        if state.filter == filter {
            return;
        }

        state.filter = filter;
        state.refresh_filtered_graph();
        if state.graph.nodes.is_empty() {
            self.knowledge_graph_view = None;
            cx.notify();
            return;
        }

        self.start_knowledge_graph_animation(cx);
        cx.notify();
    }

    pub(super) fn refresh_workspace_link_index_for_saved_file(
        &mut self,
        path: &Path,
        content: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.effective_workspace_root() else {
            return;
        };
        if !path.starts_with(&root) {
            return;
        }

        if self.workspace.state.link_index.is_none() {
            self.sync_workspace_link_index(cx);
            return;
        }

        if self.workspace.state.link_index_root.as_deref() != Some(root.as_path()) {
            self.sync_workspace_link_index(cx);
            return;
        }

        if let Some(index) = self.workspace.state.link_index.as_mut() {
            refresh_link_index_for_file(index, path, content);
            self.sync_knowledge_graph(cx);
        }
    }

    pub(super) fn reset_knowledge_graph_layout(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.knowledge_graph_view.as_mut() else {
            return;
        };
        state.reset_layout_from_simulation();
        if state.mutual_repulsion {
            state.apply_mutual_repulsion(None);
        }
        self.start_knowledge_graph_animation(cx);
        cx.notify();
    }

    pub(super) fn toggle_knowledge_graph_mutual_repulsion(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.knowledge_graph_view.as_mut() else {
            return;
        };
        state.mutual_repulsion = !state.mutual_repulsion;
        if state.mutual_repulsion {
            state.apply_mutual_repulsion(None);
        }
        cx.notify();
    }

    pub(super) fn toggle_knowledge_graph_physics_collisions(&mut self, cx: &mut Context<Self>) {
        let enabled = {
            let Some(state) = self.knowledge_graph_view.as_mut() else {
                return;
            };
            state.physics_collisions = !state.physics_collisions;
            state.physics_collisions
        };
        if enabled {
            self.ensure_knowledge_graph_physics_loop(cx);
        } else {
            self.stop_knowledge_graph_physics_loop();
            if let Some(simulation) = self
                .knowledge_graph_view
                .as_mut()
                .and_then(|state| state.simulation.as_mut())
            {
                clear_graph_physics_velocities(simulation);
            }
        }
        cx.notify();
    }

    pub(super) fn run_knowledge_graph_physics_step(&mut self, cx: &mut Context<Self>) -> bool {
        let drag = self.knowledge_graph_physics_drag_state();
        let Some(state) = self.knowledge_graph_view.as_mut() else {
            return false;
        };
        if !state.physics_collisions {
            return false;
        }
        let Some(simulation) = state.simulation.as_mut() else {
            return false;
        };

        let panel_width = f32::from(state.last_bounds.size.width);
        let panel_height = f32::from(state.last_bounds.size.height);
        if panel_width <= 0.0 || panel_height <= 0.0 {
            return false;
        }

        let config = GraphPhysicsConfig::default();
        let bounds = viewport_physics_bounds(
            &state.viewport,
            panel_width,
            panel_height,
            config.boundary_padding,
        );
        let still_moving = step_graph_physics(
            simulation,
            bounds,
            &config,
            drag,
            GRAPH_PHYSICS_DT,
        );
        state.layout = simulation.to_layout();
        if still_moving {
            cx.notify();
        }
        still_moving
    }

    fn knowledge_graph_physics_drag_state(&self) -> Option<GraphPhysicsDragState> {
        let state = self.knowledge_graph_view.as_ref()?;
        if !matches!(state.drag.mode, super::graph_view::GraphDragMode::Node) {
            return None;
        }
        let node_id = state.drag.node_id.as_ref()?;
        let simulation = state.simulation.as_ref()?;
        let node_index = simulation.node_index(node_id)?;
        Some(GraphPhysicsDragState {
            node_index,
            velocity: state.drag.drag_velocity,
        })
    }

    pub(super) fn ensure_knowledge_graph_physics_loop(&mut self, cx: &mut Context<Self>) {
        if self.graph_physics_task.is_some() {
            return;
        }
        let Some(state) = self.knowledge_graph_view.as_ref() else {
            return;
        };
        if !state.physics_collisions {
            return;
        }

        let editor = cx.entity().downgrade();
        self.graph_physics_task = Some(cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(GRAPH_PHYSICS_FRAME_MS))
                        .await;

                    let keep_going = editor
                        .update(cx, |editor, cx| {
                            if !editor
                                .knowledge_graph_view
                                .as_ref()
                                .is_some_and(|state| state.physics_collisions)
                            {
                                return false;
                            }
                            editor.run_knowledge_graph_physics_step(cx)
                        })
                        .unwrap_or(false);

                    if !keep_going {
                        break;
                    }
                }

                let _ = editor.update(cx, |editor, _| {
                    editor.graph_physics_task = None;
                });
            },
        ));
    }

    pub(super) fn stop_knowledge_graph_physics_loop(&mut self) {
        self.graph_physics_task = None;
    }

    pub(super) fn uncross_knowledge_graph_layout(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.knowledge_graph_view.as_mut() else {
            return;
        };
        let Some(simulation) = state.simulation.as_mut() else {
            return;
        };
        let resolve_collisions = state.mutual_repulsion;
        uncross_graph_layout(simulation, resolve_collisions);
        state.layout = simulation.to_layout();
        cx.notify();
    }

    pub(super) fn fit_knowledge_graph_viewport(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.knowledge_graph_view.as_mut() else {
            return;
        };
        if state.last_bounds.size.width > px(0.0) && state.last_bounds.size.height > px(0.0) {
            state.reset_viewport_fit(state.last_bounds.size);
        } else {
            state.viewport = GraphViewport::default();
        }
        cx.notify();
    }

    pub(super) fn popout_knowledge_graph(&mut self, cx: &mut Context<Self>) {
        if self.knowledge_graph_view.is_none() {
            return;
        }

        let workspace_root = self.effective_workspace_root();
        let original_editor = cx.entity().clone();
        let title = cx.global::<I18nManager>().strings().workspace_graph_window_title.clone();
        let bounds = Bounds::centered(None, size(px(1080.0), px(720.0)), cx);
        let mut options = velotype_window_options(app_window_title(Some(&title)).into(), bounds);
        options.focus = true;
        options.show = true;

        let handle = match cx.open_window(options, move |_window, cx| {
            cx.new(move |cx| {
                let mut editor = Editor::empty(cx);
                editor.graph_only_window = true;
                editor.graph_popout_parent = Some(original_editor);
                editor
            })
        }) {
            Ok(handle) => handle,
            Err(err) => {
                eprintln!("failed to open knowledge graph window: {err}");
                return;
            }
        };

        let _ = handle.update(cx, move |editor, window, cx| {
            if let Some(root) = workspace_root {
                editor.open_workspace_folder(root, window, cx);
            }
            editor.force_install_close_guard(cx, window);
            window.activate_window();
        });
    }

    pub(super) fn render_workspace_graph_panel(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        editor: &WeakEntity<Editor>,
    ) -> AnyElement {
        if self.effective_workspace_root().is_none() {
            return self.render_workspace_empty_state(
                "",
                &strings.workspace_search_no_root,
                theme,
            );
        }

        if self.workspace.state.tag_index_busy
            || self.workspace.state.link_index_busy
            || self.workspace.state.graph_busy
        {
            return self.render_workspace_empty_state("", &strings.workspace_graph_building, theme);
        }

        if self.knowledge_graph_view.is_none() {
            return self.render_workspace_empty_state("", &strings.workspace_graph_empty, theme);
        }

        let c = &theme.colors;
        let t = &theme.typography;
        let tooltip_colors = theme.colors.clone();
        let tooltip_typography = theme.typography.clone();
        let fit_editor = editor.clone();
        let reset_editor = editor.clone();
        let repulsion_editor = editor.clone();
        let physics_editor = editor.clone();
        let uncross_editor = editor.clone();
        let popout_editor = editor.clone();
        let filter = self
            .knowledge_graph_view
            .as_ref()
            .map(|state| state.filter)
            .unwrap_or_default();
        let mutual_repulsion = self
            .knowledge_graph_view
            .as_ref()
            .map(|state| state.mutual_repulsion)
            .unwrap_or(false);
        let physics_collisions = self
            .knowledge_graph_view
            .as_ref()
            .map(|state| state.physics_collisions)
            .unwrap_or(false);
        let filter_all_editor = editor.clone();
        let filter_connected_editor = editor.clone();

        div()
            .id("workspace-graph-panel")
            .w_full()
            .h_full()
            .min_h(px(240.0))
            .flex()
            .flex_col()
            .child(
                div()
                    .id("workspace-graph-toolbar")
                    .w_full()
                    .px(px(4.0))
                    .pb(px(4.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(6.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .child(graph_filter_button(
                                "workspace-graph-filter-connected",
                                &strings.workspace_graph_filter_connected,
                                filter == GraphFilter::ConnectedOnly,
                                c,
                                t,
                                filter_connected_editor,
                                GraphFilter::ConnectedOnly,
                            ))
                            .child(graph_filter_button(
                                "workspace-graph-filter-all",
                                &strings.workspace_graph_filter_all,
                                filter == GraphFilter::All,
                                c,
                                t,
                                filter_all_editor,
                                GraphFilter::All,
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(graph_toggle_button(
                                "workspace-graph-mutual-repulsion",
                                GRAPH_REPEL_ICON,
                                mutual_repulsion,
                                tooltip_colors.clone(),
                                tooltip_typography.clone(),
                                strings.workspace_graph_mutual_repulsion.clone(),
                                repulsion_editor,
                                |editor, cx| editor.toggle_knowledge_graph_mutual_repulsion(cx),
                            ))
                            .child(graph_toggle_button(
                                "workspace-graph-physics-collisions",
                                GRAPH_PHYSICS_ICON,
                                physics_collisions,
                                tooltip_colors.clone(),
                                tooltip_typography.clone(),
                                strings.workspace_graph_physics_collisions.clone(),
                                physics_editor,
                                |editor, cx| editor.toggle_knowledge_graph_physics_collisions(cx),
                            ))
                            .child(graph_toolbar_button(
                                "workspace-graph-uncross-edges",
                                GRAPH_UNCROSS_ICON,
                                tooltip_colors.clone(),
                                tooltip_typography.clone(),
                                strings.workspace_graph_uncross_crossings.clone(),
                                uncross_editor,
                                |editor, cx| editor.uncross_knowledge_graph_layout(cx),
                            ))
                            .child(graph_toolbar_button(
                                "workspace-graph-fit-view",
                                GRAPH_FIT_ICON,
                                tooltip_colors.clone(),
                                tooltip_typography.clone(),
                                strings.workspace_graph_fit_view.clone(),
                                fit_editor,
                                |editor, cx| editor.fit_knowledge_graph_viewport(cx),
                            ))
                            .child(graph_toolbar_button(
                                "workspace-graph-reset-layout",
                                GRAPH_RESET_ICON,
                                tooltip_colors.clone(),
                                tooltip_typography.clone(),
                                strings.workspace_graph_reset_layout.clone(),
                                reset_editor,
                                |editor, cx| editor.reset_knowledge_graph_layout(cx),
                            ))
                            .when(!self.graph_only_window, |this| {
                                this.child(graph_toolbar_button(
                                    "workspace-graph-popout",
                                    GRAPH_POPOUT_ICON,
                                    tooltip_colors,
                                    tooltip_typography,
                                    strings.workspace_graph_popout.clone(),
                                    popout_editor,
                                    |editor, cx| editor.popout_knowledge_graph(cx),
                                ))
                            }),
                    ),
            )
            .child(
                div()
                    .id("workspace-graph-canvas-host")
                    .flex_1()
                    .min_h(px(0.0))
                    .rounded(px(6.0))
                    .border(px(1.0))
                    .border_color(c.dialog_border.opacity(0.75))
                    .overflow_hidden()
                    .child(render_knowledge_graph_panel(editor.clone())),
            )
            .into_any_element()
    }
}

fn graph_filter_button(
    id: &'static str,
    label: &str,
    active: bool,
    c: &ThemeColors,
    t: &ThemeTypography,
    editor: WeakEntity<Editor>,
    filter: GraphFilter,
) -> AnyElement {
    let label = label.to_string();
    let mut element = div()
        .id(id)
        .px(px(6.0))
        .py(px(2.0))
        .rounded(px(4.0))
        .text_size(px(t.text_size * 0.75))
        .cursor_pointer()
        .child(label);

    element = if active {
        element
            .bg(c.dialog_secondary_button_hover)
            .text_color(c.text_default)
    } else {
        element.text_color(c.dialog_muted).hover(|this| {
            this.bg(c.dialog_secondary_button_hover)
                .text_color(c.text_default)
        })
    };

    element
        .on_click(move |_event, _window, cx| {
            let _ = editor.update(cx, |editor, cx| {
                editor.set_knowledge_graph_filter(filter, cx);
            });
        })
        .into_any_element()
}

fn graph_toolbar_icon(icon: &'static str, text_color: Hsla) -> impl IntoElement {
    svg()
        .path(icon)
        .size(px(GRAPH_TOOLBAR_ICON_SIZE))
        .flex_shrink_0()
        .text_color(text_color)
}

struct GraphToolbarTooltip {
    text: SharedString,
    colors: ThemeColors,
    typography: ThemeTypography,
}

impl Render for GraphToolbarTooltip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let c = &self.colors;
        let t = &self.typography;
        div()
            .px(px(8.0))
            .py(px(4.0))
            .rounded(px(4.0))
            .bg(c.dialog_surface)
            .border(px(1.0))
            .border_color(c.dialog_border.opacity(0.75))
            .text_size(px(t.text_size * 0.75))
            .text_color(c.text_default)
            .child(self.text.clone())
    }
}

fn graph_toolbar_tooltip(
    text: impl Into<SharedString>,
    colors: ThemeColors,
    typography: ThemeTypography,
) -> impl Fn(&mut Window, &mut App) -> AnyView + Clone + 'static {
    let text = text.into();
    move |_window, cx| {
        cx.new(|_cx| GraphToolbarTooltip {
            text: text.clone(),
            colors: colors.clone(),
            typography: typography.clone(),
        })
        .into()
    }
}

fn graph_toggle_button(
    id: &'static str,
    icon: &'static str,
    active: bool,
    colors: ThemeColors,
    typography: ThemeTypography,
    tooltip: impl Into<SharedString>,
    editor: WeakEntity<Editor>,
    action: fn(&mut Editor, &mut Context<Editor>),
) -> AnyElement {
    let icon_color = if active {
        colors.text_default
    } else {
        colors.dialog_muted
    };
    let mut element = div()
        .id(id)
        .p(px(4.0))
        .rounded(px(4.0))
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_center()
        .child(graph_toolbar_icon(icon, icon_color));

    element = if active {
        element.bg(colors.dialog_secondary_button_hover)
    } else {
        element.hover(|this| this.bg(colors.dialog_secondary_button_hover))
    };

    element
        .tooltip(graph_toolbar_tooltip(tooltip, colors, typography))
        .on_click(move |_event, _window, cx| {
            let _ = editor.update(cx, |editor, cx| {
                action(editor, cx);
            });
        })
        .into_any_element()
}

fn graph_toolbar_button(
    id: &'static str,
    icon: &'static str,
    colors: ThemeColors,
    typography: ThemeTypography,
    tooltip: impl Into<SharedString>,
    editor: WeakEntity<Editor>,
    action: fn(&mut Editor, &mut Context<Editor>),
) -> AnyElement {
    div()
        .id(id)
        .p(px(4.0))
        .rounded(px(4.0))
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_center()
        .hover(|this| this.bg(colors.dialog_secondary_button_hover))
        .child(graph_toolbar_icon(icon, colors.dialog_muted))
        .tooltip(graph_toolbar_tooltip(tooltip, colors, typography))
        .on_click(move |_event, _window, cx| {
            let _ = editor.update(cx, |editor, cx| {
                action(editor, cx);
            });
        })
        .into_any_element()
}
