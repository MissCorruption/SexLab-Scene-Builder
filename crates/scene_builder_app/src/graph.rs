//! Graph canvas with manual pan/zoom in screen space (crisp text at any zoom).
//! Custom edge engine: rounded orthogonal routing, hit-testing, arrowheads.
//! Nodes allocate screen-space interact rects so future in-card controls can
//! be added without a transform-layer rewrite.

use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use scene_builder_core::project::define::Node;
use scene_builder_core::project::scene::Scene;
use scene_builder_core::project::stage::Stage;
use scene_builder_core::project::NanoID;
use std::collections::HashMap;

// Stage node card size and mint fill (300×140, 6px double border).
pub const NODE_W: f32 = 300.0;
pub const NODE_H: f32 = 140.0;
const HEADER_H: f32 = 44.0;
const ROOT_BORDER: Color32 = Color32::from_rgb(0, 88, 0);
const FIXED_LEN_PINK: Color32 = Color32::from_rgb(255, 175, 175);
const FIXED_LEN_CYAN: Color32 = Color32::from_rgb(175, 235, 255);
const ICON_START: Color32 = Color32::from_rgb(17, 175, 17);
const ICON_ORGASM: Color32 = Color32::from_rgb(255, 20, 147);
const ICON_WARN: Color32 = Color32::from_rgb(255, 0, 0);
const ICON_FIXED: Color32 = Color32::from_rgb(0, 191, 255);
const BTN_DANGER: Color32 = Color32::from_rgb(0xcf, 0x13, 0x22);
// rgba(201, 225, 195, 0.3) premultiplied (from_rgba_unmultiplied is not const).
const PORT_FILL: Color32 = Color32::from_rgba_premultiplied(61, 68, 59, 77);
/// Soft major grid spacing in world units (no dense minors).
const GRID_STEP: f32 = 90.0;
const DRAG_THRESHOLD: f32 = 4.0;
const EDGE_HIT_DIST: f32 = 6.0;
const ZOOM_MIN: f32 = 0.25;
const ZOOM_MAX: f32 = 5.0;
/// Status / hover control size in world pixels (scales with zoom).
const ICON_HIT: f32 = 22.0;
/// Stage name size in world pixels.
const NAME_PX: f32 = 18.0;
const ROOT_FONT_PX: f32 = 12.0;
const TOOLBAR_BTN: f32 = 22.0;

#[derive(Debug, Clone)]
pub struct GraphSnapshot {
    pub stages: Vec<Stage>,
    pub nodes: Vec<(NanoID, Node)>,
    pub root: NanoID,
}

#[derive(Debug, Clone)]
pub struct GraphView {
    pub pan: Vec2,
    pub zoom: f32,
    pub selected: Option<NanoID>,
    pub locked: bool,
    needs_fit: bool,
    /// Last graph canvas rect — used by the header toolbar Fit action.
    last_canvas_rect: Rect,
    undo_stack: Vec<GraphSnapshot>,
    redo_stack: Vec<GraphSnapshot>,
    /// Port-drag connect in progress (source node).
    connect_drag: Option<NanoID>,
    dragging_node: Option<NanoID>,
    /// Node under pointer awaiting drag threshold before move.
    pending_node: Option<NanoID>,
    drag_accum: Vec2,
    panning_bg: bool,
    drag_snapshot: Option<GraphSnapshot>,
    /// Right-click menu on a stage node (id + screen pos).
    node_menu: Option<(NanoID, Pos2)>,
}

impl Default for GraphView {
    fn default() -> Self {
        Self {
            pan: Vec2::ZERO,
            zoom: 1.0,
            selected: None,
            locked: false,
            needs_fit: true,
            last_canvas_rect: Rect::NOTHING,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            connect_drag: None,
            dragging_node: None,
            pending_node: None,
            drag_accum: Vec2::ZERO,
            panning_bg: false,
            drag_snapshot: None,
            node_menu: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphAction {
    None,
    Select(NanoID),
    OpenEditor(NanoID),
    SetRoot(NanoID),
    DeleteStage(NanoID),
    CloneStage(NanoID),
    CloneStageTo(NanoID),
    Arrange,
    /// User asked to clear the canvas (app confirms before wiping).
    ClearCanvas,
    /// Scene graph mutated (edge or node position).
    Dirty,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NodeButton {
    Edit,
    Clone,
    CloneTo,
    Root,
    Delete,
}

#[derive(Clone, Copy)]
enum StatusKind {
    Start,
    Orgasm,
    Warn,
    Fixed,
}

impl GraphView {
    pub fn push_undo(&mut self, scene: &Scene) {
        self.undo_stack.push(snapshot_of(scene));
        self.redo_stack.clear();
        if self.undo_stack.len() > 64 {
            self.undo_stack.remove(0);
        }
    }

    pub fn undo(&mut self, scene: &mut Scene) -> bool {
        let Some(prev) = self.undo_stack.pop() else {
            return false;
        };
        self.redo_stack.push(snapshot_of(scene));
        apply_snapshot(scene, &prev);
        true
    }

    pub fn redo(&mut self, scene: &mut Scene) -> bool {
        let Some(next) = self.redo_stack.pop() else {
            return false;
        };
        self.undo_stack.push(snapshot_of(scene));
        apply_snapshot(scene, &next);
        true
    }

    /// Re-fit the view on the next frame (e.g. after switching scenes).
    pub fn request_fit(&mut self) {
        self.needs_fit = true;
    }

    pub fn zoom_by(&mut self, factor: f32) {
        self.zoom = (self.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
    }

    /// Zoom keeping the world point under `screen_pos` fixed.
    pub fn zoom_at(&mut self, rect: Rect, screen_pos: Pos2, factor: f32) {
        let world = self.screen_to_world(rect, screen_pos);
        self.zoom = (self.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
        self.pan = (screen_pos - rect.center()) / self.zoom - world.to_vec2();
    }

    pub fn center_view(&mut self, scene: &Scene) {
        let positions = layout_nodes(scene);
        if positions.is_empty() {
            self.pan = Vec2::ZERO;
            return;
        }
        let Some(bounds) = content_bounds_from(&positions) else {
            return;
        };
        self.pan = -bounds.center().to_vec2();
    }

    pub fn fit_view(&mut self, rect: Rect, scene: &Scene) {
        let positions = layout_nodes(scene);
        if positions.is_empty() {
            self.pan = Vec2::ZERO;
            self.zoom = 1.0;
            return;
        }
        let Some(bounds) = content_bounds_from(&positions) else {
            return;
        };
        let content_w = bounds.width().max(1.0);
        let content_h = bounds.height().max(1.0);
        let pad = 48.0;
        let avail_w = (rect.width() - pad * 2.0).max(1.0);
        let avail_h = (rect.height() - pad * 2.0).max(1.0);
        // Zoom so content fills the canvas (may zoom in for sparse scenes).
        self.zoom = (avail_w / content_w)
            .min(avail_h / content_h)
            .clamp(ZOOM_MIN, ZOOM_MAX.min(2.5));
        self.pan = -bounds.center().to_vec2();
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, scene: &mut Scene) -> GraphAction {
        let mut action = GraphAction::None;
        let mut dirty = false;
        let (response, mut painter) =
            ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = response.rect;
        if self.last_canvas_rect != Rect::NOTHING
            && self.last_canvas_rect.width() > 1.0
            && self.zoom > 0.0
        {
            let old_c = self.last_canvas_rect.center();
            let new_c = rect.center();
            let shift = old_c - new_c;
            if shift.length_sq() > 0.01 {
                self.pan += shift / self.zoom;
            }
        }
        self.last_canvas_rect = rect;
        painter.rect_filled(rect, 0.0, graph_bg(ui.visuals().dark_mode));
        painter.set_clip_rect(rect.intersect(ui.clip_rect()));

        if self.needs_fit {
            // Wait until the canvas has a real size (first frames after import can be tiny).
            if rect.width() >= 120.0 && rect.height() >= 80.0 {
                self.fit_view(rect, scene);
                self.needs_fit = false;
            }
        }

        draw_mesh_grid(&painter, rect, self.pan, self.zoom, ui.visuals().dark_mode);

        let dark = ui.visuals().dark_mode;
        let mut positions = layout_nodes(scene);

        let hover_pos = response.hover_pos();
        let pointer_pos = response.interact_pointer_pos();

        let node_rects: Vec<(NanoID, Rect)> = scene
            .stages
            .iter()
            .filter_map(|s| {
                positions.get(&s.id).map(|p| {
                    let tl = self.world_to_screen(rect, *p);
                    let br = self.world_to_screen(rect, *p + Vec2::new(NODE_W, NODE_H));
                    (s.id.clone(), Rect::from_min_max(tl, br))
                })
            })
            .collect();
        let topmost_node_at = |pos: Pos2| -> Option<NanoID> {
            node_rects
                .iter()
                .rev()
                .find(|(_, r)| r.contains(pos))
                .map(|(id, _)| id.clone())
        };

        let edge_color = if dark {
            Color32::from_gray(205)
        } else {
            Color32::BLACK
        };
        let edge_stroke = Stroke::new(1.6 * self.zoom.clamp(0.6, 1.6), edge_color);
        let mut edge_paths: Vec<(NanoID, NanoID, Vec<Pos2>)> = Vec::new();
        for (id, node) in &scene.graph {
            let Some(from) = positions.get(id).copied() else {
                continue;
            };
            for dest in &node.dest {
                let Some(to) = positions.get(dest).copied() else {
                    continue;
                };
                let path = self.edge_screen_path(rect, from, to);
                draw_edge_path(&painter, &path, edge_stroke, self.zoom);
                edge_paths.push((id.clone(), dest.clone(), path));
            }
        }

        if let (Some(from_id), Some(pointer)) = (&self.connect_drag, hover_pos) {
            if let Some(from) = positions.get(from_id).copied() {
                let start =
                    self.world_to_screen(rect, from + Vec2::new(NODE_W + 9.0, NODE_H * 0.5));
                draw_edge_path(&painter, &[start, pointer], edge_stroke, self.zoom);
            }
        }

        let mut opened_node_menu = false;
        if response.secondary_clicked() {
            if let Some(pos) = pointer_pos {
                if let Some(id) = topmost_node_at(pos) {
                    self.node_menu = Some((id, pos));
                    opened_node_menu = true;
                } else {
                    self.node_menu = None;
                    let hit = edge_paths
                        .iter()
                        .find(|(_, _, path)| dist_to_polyline(pos, path) <= EDGE_HIT_DIST)
                        .map(|(a, b, _)| (a.clone(), b.clone()));
                    if let Some((from, to)) = hit {
                        self.push_undo(scene);
                        if let Some(node) = scene.graph.get_mut(&from) {
                            node.dest.retain(|d| d != &to);
                        }
                        dirty = true;
                    }
                }
            }
        }

        let mut clicked: Option<NanoID> = None;
        let mut double: Option<NanoID> = None;
        let mut button_hit: Option<(NanoID, NodeButton)> = None;
        let mut hover_button = false;

        for stage in &scene.stages {
            let Some(pos) = positions.get(&stage.id).copied() else {
                continue;
            };
            let node_rect = {
                let tl = self.world_to_screen(rect, pos);
                let br = self.world_to_screen(rect, pos + Vec2::new(NODE_W, NODE_H));
                Rect::from_min_max(tl, br)
            };
            if !rect.intersects(node_rect.expand(60.0 * self.zoom)) {
                continue;
            }
            let is_root = scene.root == stage.id;
            let hovered = hover_pos.map(|p| node_rect.contains(p)).unwrap_or(false);
            let outgoing = scene
                .graph
                .get(&stage.id)
                .map(|n| n.dest.len())
                .unwrap_or(0);

            let buttons =
                self.draw_node(ui, &painter, stage, node_rect, is_root, hovered, outgoing);

            if hovered {
                if let Some(p) = hover_pos {
                    for (r, b) in &buttons {
                        if r.contains(p) {
                            hover_button = true;
                            if response.clicked() {
                                button_hit = Some((stage.id.clone(), *b));
                            }
                        }
                    }
                }
            }

            if let Some(p) = pointer_pos {
                if node_rect.contains(p) && button_hit.is_none() && !hover_button {
                    if response.double_clicked() {
                        double = Some(stage.id.clone());
                    } else if response.clicked() {
                        clicked = Some(stage.id.clone());
                    }
                }
            }
        }

        if response.drag_started_by(egui::PointerButton::Primary) {
            if let Some(p) = pointer_pos {
                let port_hit = scene.stages.iter().rev().find_map(|s| {
                    let pos = positions.get(&s.id)?;
                    let port = self.port_screen_rect(rect, *pos);
                    port.contains(p).then(|| s.id.clone())
                });
                if let Some(id) = port_hit {
                    self.connect_drag = Some(id);
                    self.pending_node = None;
                    self.panning_bg = false;
                } else if hover_button {
                    self.pending_node = None;
                    self.panning_bg = false;
                } else if let Some(id) = topmost_node_at(p) {
                    self.pending_node = Some(id.clone());
                    self.drag_accum = Vec2::ZERO;
                    self.drag_snapshot = Some(snapshot_of(scene));
                    self.selected = Some(id);
                    self.panning_bg = false;
                } else if !self.locked {
                    self.panning_bg = true;
                    self.pending_node = None;
                    self.dragging_node = None;
                }
            }
        }
        if response.drag_started_by(egui::PointerButton::Middle) {
            if !self.locked {
                self.panning_bg = true;
                self.pending_node = None;
            }
        }
        if response.drag_started_by(egui::PointerButton::Secondary) {
            let over_node = pointer_pos.and_then(|p| topmost_node_at(p)).is_some();
            if !self.locked && !over_node {
                self.panning_bg = true;
                self.pending_node = None;
            }
        }

        let primary_drag = response.dragged_by(egui::PointerButton::Primary);
        let mid_right_drag = response.dragged_by(egui::PointerButton::Middle)
            || response.dragged_by(egui::PointerButton::Secondary);

        if primary_drag || mid_right_drag {
            ui.ctx().request_repaint();
            let delta = response.drag_delta();

            if self.connect_drag.is_some() {
            } else if self.panning_bg && !self.locked {
                self.pan += delta / self.zoom;
            } else if let Some(id) = self.pending_node.clone() {
                self.drag_accum += delta;
                if self.drag_accum.length() >= DRAG_THRESHOLD {
                    self.dragging_node = Some(id);
                    self.pending_node = None;
                    let world_delta = self.drag_accum / self.zoom;
                    if let Some(node) = scene.graph.get_mut(self.dragging_node.as_ref().unwrap()) {
                        node.x += world_delta.x;
                        node.y += world_delta.y;
                        dirty = true;
                    }
                    self.drag_accum = Vec2::ZERO;
                }
            } else if let Some(id) = self.dragging_node.clone() {
                if primary_drag {
                    let world_delta = delta / self.zoom;
                    if let Some(pos) = positions.get_mut(&id) {
                        *pos += world_delta;
                    }
                    if let Some(node) = scene.graph.get_mut(&id) {
                        node.x += world_delta.x;
                        node.y += world_delta.y;
                        dirty = true;
                    } else if let Some(pos) = positions.get(&id).copied() {
                        ensure_graph_node(scene, &id, 0);
                        if let Some(node) = scene.graph.get_mut(&id) {
                            node.x = pos.x;
                            node.y = pos.y;
                            dirty = true;
                        }
                    }
                }
            }
        }

        if response.drag_stopped() {
            if let Some(from) = self.connect_drag.take() {
                if let Some(p) = hover_pos.or(pointer_pos) {
                    if let Some(target) = topmost_node_at(p) {
                        if target != from {
                            self.push_undo(scene);
                            ensure_graph_node(scene, &from, 0);
                            if let Some(node) = scene.graph.get_mut(&from) {
                                if !node.dest.iter().any(|d| d == &target) {
                                    node.dest.push(target.clone());
                                    dirty = true;
                                }
                            }
                            ensure_graph_node(scene, &target, 0);
                        }
                    }
                }
            }
            if dirty {
                if let Some(snap) = self.drag_snapshot.take() {
                    self.undo_stack.push(snap);
                    self.redo_stack.clear();
                }
            }
            self.dragging_node = None;
            self.pending_node = None;
            self.panning_bg = false;
            self.drag_accum = Vec2::ZERO;
            self.drag_snapshot = None;
        }

        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.0 {
                let factor = (scroll * 0.00105).exp();
                if let Some(pointer) = hover_pos {
                    self.zoom_at(rect, pointer, factor);
                } else {
                    self.zoom_by(factor);
                }
                ui.ctx().request_repaint();
            }
        }

        if let Some((id, btn)) = button_hit {
            self.selected = Some(id.clone());
            action = match btn {
                NodeButton::Edit => GraphAction::OpenEditor(id),
                NodeButton::Clone => GraphAction::CloneStage(id),
                NodeButton::CloneTo => GraphAction::CloneStageTo(id),
                NodeButton::Root => GraphAction::SetRoot(id),
                NodeButton::Delete => GraphAction::DeleteStage(id),
            };
        } else if let Some(id) = double {
            self.selected = Some(id.clone());
            action = GraphAction::OpenEditor(id);
        } else if let Some(id) = clicked {
            self.selected = Some(id.clone());
            action = GraphAction::Select(id);
        } else if response.clicked()
            && self.dragging_node.is_none()
            && self.pending_node.is_none()
            && !self.panning_bg
            && !hover_button
        {
            if let Some(pos) = pointer_pos {
                if topmost_node_at(pos).is_none() {
                    self.selected = None;
                }
            }
        }

        if let Some((id, pos)) = self.node_menu.clone() {
            let mut picked: Option<GraphAction> = None;
            let mut close = false;
            let mut remove_links = false;
            let popup = egui::Area::new(egui::Id::new("stage_node_ctx"))
                .order(egui::Order::Foreground)
                .fixed_pos(pos)
                .constrain(true)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(160.0);
                        if ui.button("Edit").clicked() {
                            picked = Some(GraphAction::OpenEditor(id.clone()));
                        }
                        if ui.button("Clone").clicked() {
                            picked = Some(GraphAction::CloneStage(id.clone()));
                        }
                        if ui.button("Clone to…").clicked() {
                            picked = Some(GraphAction::CloneStageTo(id.clone()));
                        }
                        if ui.button("Mark as root").clicked() {
                            picked = Some(GraphAction::SetRoot(id.clone()));
                        }
                        if ui.button("Remove connections").clicked() {
                            remove_links = true;
                        }
                        ui.separator();
                        if ui
                            .add(egui::Button::new(RichText::new("Delete").color(BTN_DANGER)))
                            .clicked()
                        {
                            picked = Some(GraphAction::DeleteStage(id.clone()));
                        }
                    });
                });
            if remove_links {
                self.push_undo(scene);
                if let Some(node) = scene.graph.get_mut(&id) {
                    node.dest.clear();
                }
                for node in scene.graph.values_mut() {
                    node.dest.retain(|d| d != &id);
                }
                dirty = true;
                close = true;
            }
            if picked.is_some() {
                close = true;
            }
            // The RMB that opened the menu is "elsewhere" on this frame.
            if popup.response.clicked_elsewhere() && !opened_node_menu {
                close = true;
            }
            if let Some(act) = picked {
                self.selected = Some(id);
                action = act;
            }
            if close {
                self.node_menu = None;
            }
        }

        if dirty {
            match action {
                GraphAction::OpenEditor(_)
                | GraphAction::SetRoot(_)
                | GraphAction::DeleteStage(_)
                | GraphAction::CloneStage(_)
                | GraphAction::CloneStageTo(_)
                | GraphAction::ClearCanvas
                | GraphAction::Arrange => {}
                _ => action = GraphAction::Dirty,
            }
        }

        action
    }

    /// Scene-header toolbar: Undo Redo | Center Fit Arrange Lock | Zoom | Clear.
    pub fn toolbar_ui(&mut self, ui: &mut egui::Ui, scene: &mut Scene) -> GraphAction {
        let mut action = GraphAction::None;
        let mut dirty = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if toolbar_glyph_button(ui, "↺", "Undo", None, !self.undo_stack.is_empty()).clicked()
                && self.undo(scene)
            {
                dirty = true;
            }
            if toolbar_glyph_button(ui, "↻", "Redo", None, !self.redo_stack.is_empty()).clicked()
                && self.redo(scene)
            {
                dirty = true;
            }
            ui.separator();
            if toolbar_icon_button(ui, ToolbarIcon::Center, "Center content").clicked() {
                self.center_view(scene);
            }
            if toolbar_icon_button(ui, ToolbarIcon::Fit, "Fit to screen").clicked() {
                if self.last_canvas_rect.width() >= 32.0 && self.last_canvas_rect.height() >= 32.0 {
                    self.fit_view(self.last_canvas_rect, scene);
                } else {
                    self.request_fit();
                }
            }
            if toolbar_icon_button(ui, ToolbarIcon::Arrange, "Arrange stages").clicked() {
                self.push_undo(scene);
                action = GraphAction::Arrange;
            }
            if toolbar_glyph_button(
                ui,
                if self.locked { "📌" } else { "✋" },
                "Lock canvas (disables panning)",
                None,
                true,
            )
            .clicked()
            {
                self.locked = !self.locked;
            }
            ui.separator();
            if toolbar_glyph_button(ui, "−", "Zoom out", None, true).clicked() {
                self.zoom_by(0.8);
            }
            if toolbar_glyph_button(ui, "+", "Zoom in", None, true).clicked() {
                self.zoom_by(1.2);
            }
            ui.separator();
            if toolbar_glyph_button(ui, "✕", "Clear canvas", Some(BTN_DANGER), true).clicked() {
                action = GraphAction::ClearCanvas;
            }
        });
        if dirty && matches!(action, GraphAction::None) {
            action = GraphAction::Dirty;
        }
        action
    }

    /// Draws the node card and returns header button rects (only populated when hovered).
    fn draw_node(
        &self,
        ui: &egui::Ui,
        painter: &egui::Painter,
        stage: &Stage,
        node_rect: Rect,
        is_root: bool,
        hovered: bool,
        outgoing: usize,
    ) -> Vec<(Rect, NodeButton)> {
        let z = self.zoom;
        let selected = self.selected.as_ref() == Some(&stage.id);
        let connect_src = self.connect_drag.as_ref() == Some(&stage.id);
        let has_climax = stage.positions.iter().any(|p| p.climax);
        let fixed_len = stage.extra.fixed_len;
        let missing_nav = outgoing > 1 && stage.extra.nav_text.trim().is_empty();

        let fill = if fixed_len > 0.0 {
            if fixed_len < 50.0 {
                FIXED_LEN_PINK
            } else {
                FIXED_LEN_CYAN
            }
        } else {
            crate::theme::SCENE_NODE_BG
        };

        let port_stroke_color = if is_root { ROOT_BORDER } else { Color32::BLACK };
        {
            let base_x = node_rect.right() - 1.0 * z;
            let cy = node_rect.center().y;
            let p1 = Pos2::new(base_x, cy - 40.0 * z);
            let p2 = Pos2::new(base_x + 10.0 * z, cy);
            let p3 = Pos2::new(base_x, cy + 40.0 * z);
            painter.add(egui::Shape::convex_polygon(
                vec![p1, p2, p3],
                if connect_src {
                    crate::theme::SCENE_NODE_CONNECT
                } else {
                    PORT_FILL
                },
                Stroke::new(1.0 * z.clamp(0.5, 1.5), port_stroke_color),
            ));
        }

        let border_color = if is_root {
            ROOT_BORDER
        } else {
            crate::theme::border_strong(false)
        };
        let rounding = 6.0 * z.clamp(0.5, 1.5);
        painter.rect_filled(node_rect, rounding, fill);
        let line_w = 2.0 * z.clamp(0.5, 1.5);
        painter.rect_stroke(
            node_rect,
            rounding,
            Stroke::new(line_w, border_color),
            egui::StrokeKind::Inside,
        );
        painter.rect_stroke(
            node_rect.shrink(2.0 * line_w),
            (rounding - 2.0 * line_w).max(0.0),
            Stroke::new(line_w, border_color),
            egui::StrokeKind::Inside,
        );
        if selected {
            painter.rect_stroke(
                node_rect.expand(2.0),
                rounding,
                Stroke::new(1.5, crate::theme::accent(false)),
                egui::StrokeKind::Outside,
            );
        }

        let header_bottom = node_rect.top() + HEADER_H * z;
        painter.line_segment(
            [
                Pos2::new(node_rect.left() + 3.0 * line_w, header_bottom),
                Pos2::new(node_rect.right() - 3.0 * line_w, header_bottom),
            ],
            Stroke::new(2.0 * z.clamp(0.5, 1.2), Color32::BLACK),
        );

        let header_h = (HEADER_H * z).max(1.0);
        let icon_y = node_rect.top() + header_h * 0.5;
        let mut icon_x = node_rect.left() + 10.0 * z;
        let mut status: Vec<(StatusKind, Color32, &str)> = Vec::new();
        if is_root {
            status.push((StatusKind::Start, ICON_START, "Start Animation"));
        }
        if has_climax {
            status.push((StatusKind::Orgasm, ICON_ORGASM, "Orgasm Stage"));
        }
        if missing_nav {
            status.push((
                StatusKind::Warn,
                ICON_WARN,
                "Missing choice label for this branch",
            ));
        }
        if fixed_len > 0.0 {
            status.push((StatusKind::Fixed, ICON_FIXED, "Fixed Length"));
        }
        let icon_draw = ICON_HIT * z;
        for (kind, color, tip) in &status {
            let icon_rect = Rect::from_center_size(
                Pos2::new(icon_x + icon_draw * 0.5, icon_y),
                Vec2::splat(icon_draw),
            );
            draw_status_icon(painter, *kind, icon_rect, *color);
            if let Some(p) = ui.ctx().pointer_hover_pos() {
                if icon_rect.expand(2.0).contains(p) {
                    egui::show_tooltip_at_pointer(
                        ui.ctx(),
                        ui.layer_id(),
                        egui::Id::new(("node_status_tip", stage.id.0.as_str(), *tip)),
                        |ui| {
                            ui.label(*tip);
                        },
                    );
                }
            }
            icon_x = icon_rect.right() + 6.0 * z.clamp(0.5, 1.2);
        }

        let mut buttons = Vec::new();
        if hovered {
            let pad = 6.0 * z;
            let gap = 3.0 * z;
            let btn_size = ICON_HIT * z;
            let root_font = egui::FontId::proportional(ROOT_FONT_PX * z);
            let mut root_w = painter
                .layout_no_wrap("Root".to_string(), root_font.clone(), Color32::BLACK)
                .size()
                .x
                + 6.0 * z;
            root_w = root_w.max(btn_size);

            let strip_w = btn_size * 4.0 + root_w + gap * 4.0;
            let max_strip = (node_rect.width() - pad * 2.0).max(btn_size);
            let (btn_size, root_w, root_font) = if strip_w > max_strip {
                let s = max_strip / strip_w;
                (
                    (btn_size * s).max(4.0),
                    (root_w * s).max(4.0),
                    egui::FontId::proportional((ROOT_FONT_PX * z * s).max(5.0)),
                )
            } else {
                (btn_size, root_w, root_font)
            };
            let strip_clip = node_rect.intersect(painter.clip_rect());
            let icon_painter = painter.with_clip_rect(strip_clip);

            let entries: [(NodeButton, &str, Color32); 5] = [
                (NodeButton::Edit, "Edit", crate::theme::SCENE_NODE_TEXT),
                (NodeButton::Clone, "Clone", crate::theme::SCENE_NODE_TEXT),
                (
                    NodeButton::CloneTo,
                    "Clone to…",
                    crate::theme::SCENE_NODE_TEXT,
                ),
                (
                    NodeButton::Root,
                    "Mark as root",
                    crate::theme::SCENE_NODE_TEXT,
                ),
                (NodeButton::Delete, "Delete", BTN_DANGER),
            ];
            let mut right = node_rect.right() - pad;
            for (btn, tip, color) in entries.iter().rev() {
                let w = if *btn == NodeButton::Root {
                    root_w
                } else {
                    btn_size
                };
                let h = btn_size;
                let btn_rect = Rect::from_min_max(
                    Pos2::new(right - w, icon_y - h * 0.5),
                    Pos2::new(right, icon_y + h * 0.5),
                );
                let over = ui
                    .ctx()
                    .pointer_hover_pos()
                    .map(|p| btn_rect.contains(p))
                    .unwrap_or(false);
                if over {
                    icon_painter.rect_filled(
                        btn_rect,
                        4.0,
                        Color32::from_rgba_unmultiplied(0, 0, 0, 20),
                    );
                    egui::show_tooltip_at_pointer(
                        ui.ctx(),
                        ui.layer_id(),
                        egui::Id::new(("node_btn_tip", stage.id.0.as_str(), *tip)),
                        |ui| {
                            ui.label(*tip);
                        },
                    );
                }
                match btn {
                    NodeButton::Root => {
                        icon_painter.text(
                            btn_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "Root",
                            root_font.clone(),
                            *color,
                        );
                    }
                    other => draw_ctrl_icon(&icon_painter, *other, btn_rect.shrink(2.0), *color),
                }
                buttons.push((btn_rect, *btn));
                right -= w + gap;
            }
        }

        let label = if stage.name.is_empty() {
            "Untitled".to_string()
        } else {
            stage.name.clone()
        };
        let pad = 8.0 * z;
        let body_h = (node_rect.bottom() - header_bottom).max(1.0);
        let name_font = egui::FontId::proportional(NAME_PX * z);
        let max_w = (node_rect.width() - pad).max(8.0);
        let text = truncate_to_width(painter, &label, &name_font, max_w);
        let name_area_center = Pos2::new(node_rect.center().x, header_bottom + body_h * 0.5);
        let name_clip = Rect::from_min_max(
            Pos2::new(node_rect.left() + pad * 0.5, header_bottom),
            node_rect.right_bottom(),
        )
        .intersect(painter.clip_rect());
        painter.with_clip_rect(name_clip).text(
            name_area_center,
            egui::Align2::CENTER_CENTER,
            text,
            name_font,
            crate::theme::SCENE_NODE_TEXT,
        );

        buttons
    }

    fn port_screen_rect(&self, rect: Rect, world_pos: Pos2) -> Rect {
        let base = self.world_to_screen(rect, world_pos + Vec2::new(NODE_W - 1.0, NODE_H * 0.5));
        Rect::from_min_max(
            Pos2::new(base.x - 4.0 * self.zoom, base.y - 40.0 * self.zoom),
            Pos2::new(base.x + 12.0 * self.zoom, base.y + 40.0 * self.zoom),
        )
    }

    /// Orthogonal edge path from `from`'s out-port to `to`'s in-port, in screen space.
    fn edge_screen_path(&self, rect: Rect, from: Pos2, to: Pos2) -> Vec<Pos2> {
        let start = from + Vec2::new(NODE_W + 9.0, NODE_H * 0.5);
        let end = to + Vec2::new(0.0, NODE_H * 0.5);
        let pad = 24.0;

        let world: Vec<Pos2> = if end.x >= start.x + pad {
            let mid_x = (start.x + end.x) * 0.5;
            vec![
                start,
                Pos2::new(mid_x, start.y),
                Pos2::new(mid_x, end.y),
                end,
            ]
        } else {
            let from_bottom = from.y + NODE_H;
            let to_bottom = to.y + NODE_H;
            let corridor_y = if to.y > from_bottom + pad {
                (from_bottom + to.y) * 0.5
            } else if from.y > to_bottom + pad {
                (to_bottom + from.y) * 0.5
            } else {
                from_bottom.max(to_bottom) + pad * 1.5
            };
            vec![
                start,
                Pos2::new(start.x + pad, start.y),
                Pos2::new(start.x + pad, corridor_y),
                Pos2::new(end.x - pad, corridor_y),
                Pos2::new(end.x - pad, end.y),
                end,
            ]
        };

        rounded_polyline(&world, 10.0)
            .into_iter()
            .map(|p| self.world_to_screen(rect, p))
            .collect()
    }

    fn world_to_screen(&self, rect: Rect, world: Pos2) -> Pos2 {
        rect.center() + (world.to_vec2() + self.pan) * self.zoom
    }

    fn screen_to_world(&self, rect: Rect, screen: Pos2) -> Pos2 {
        let v = (screen - rect.center()) / self.zoom - self.pan;
        Pos2::new(v.x, v.y)
    }
}

fn snapshot_of(scene: &Scene) -> GraphSnapshot {
    GraphSnapshot {
        stages: scene.stages.clone(),
        nodes: scene
            .graph
            .iter()
            .map(|(id, node)| (id.clone(), node.clone()))
            .collect(),
        root: scene.root.clone(),
    }
}

fn apply_snapshot(scene: &mut Scene, snap: &GraphSnapshot) {
    scene.stages = snap.stages.clone();
    scene.graph.clear();
    for (id, node) in &snap.nodes {
        scene.graph.insert(id.clone(), node.clone());
    }
    scene.root = snap.root.clone();
}

fn content_bounds_from(positions: &HashMap<NanoID, Pos2>) -> Option<Rect> {
    if positions.is_empty() {
        return None;
    }
    let mut min = Pos2::new(f32::MAX, f32::MAX);
    let mut max = Pos2::new(f32::MIN, f32::MIN);
    for p in positions.values() {
        min.x = min.x.min(p.x);
        min.y = min.y.min(p.y);
        max.x = max.x.max(p.x + NODE_W);
        max.y = max.y.max(p.y + NODE_H);
    }
    Some(Rect::from_min_max(min, max))
}

fn truncate_to_width(
    painter: &egui::Painter,
    text: &str,
    font: &egui::FontId,
    max_w: f32,
) -> String {
    let full_w = painter
        .layout_no_wrap(text.to_string(), font.clone(), Color32::BLACK)
        .size()
        .x;
    if full_w <= max_w {
        return text.to_string();
    }
    let mut out: String = text.to_string();
    while !out.is_empty() {
        out.pop();
        let candidate = format!("{out}…");
        let w = painter
            .layout_no_wrap(candidate.clone(), font.clone(), Color32::BLACK)
            .size()
            .x;
        if w <= max_w {
            return candidate;
        }
    }
    "…".to_string()
}

fn dist_to_polyline(p: Pos2, path: &[Pos2]) -> f32 {
    let mut best = f32::MAX;
    for w in path.windows(2) {
        let (a, b) = (w[0], w[1]);
        let ab = b - a;
        let len2 = ab.length_sq();
        let t = if len2 > 0.0 {
            ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let proj = a + ab * t;
        best = best.min((p - proj).length());
    }
    best
}

fn rounded_polyline(points: &[Pos2], radius: f32) -> Vec<Pos2> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut out = Vec::with_capacity(points.len() * 4);
    out.push(points[0]);
    for i in 1..points.len() - 1 {
        let prev = points[i - 1];
        let p = points[i];
        let next = points[i + 1];
        let in_len = (p - prev).length();
        let out_len = (next - p).length();
        let r = radius.min(in_len * 0.5).min(out_len * 0.5);
        if r < 0.5 {
            out.push(p);
            continue;
        }
        let dir_in = (p - prev) / in_len;
        let dir_out = (next - p) / out_len;
        let a = p - dir_in * r;
        let b = p + dir_out * r;
        const STEPS: usize = 6;
        for s in 0..=STEPS {
            let t = s as f32 / STEPS as f32;
            let q1 = a + (p - a) * t;
            let q2 = p + (b - p) * t;
            out.push(q1 + (q2 - q1) * t);
        }
    }
    out.push(*points.last().unwrap());
    out
}

fn draw_edge_path(painter: &egui::Painter, path: &[Pos2], stroke: Stroke, zoom: f32) {
    if path.len() < 2 {
        return;
    }
    painter.add(egui::Shape::line(path.to_vec(), stroke));

    let tip = *path.last().unwrap();
    let prev = path[path.len() - 2];
    let dir = tip - prev;
    if dir.length_sq() > 0.0 {
        let dir = dir.normalized();
        let side = Vec2::new(-dir.y, dir.x);
        let len = 8.0 * zoom.clamp(0.6, 1.5);
        let half_h = 3.0 * zoom.clamp(0.6, 1.5);
        let p1 = tip - dir * len + side * half_h;
        let p2 = tip - dir * len - side * half_h;
        painter.add(egui::Shape::convex_polygon(
            vec![tip, p1, p2],
            stroke.color,
            Stroke::NONE,
        ));
    }
}

/// Painter-drawn status glyphs — crisp at any zoom (no emoji atlas stretch).
fn draw_status_icon(painter: &egui::Painter, kind: StatusKind, rect: Rect, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) * 0.5;
    match kind {
        StatusKind::Start => {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(c.x - s * 0.45, c.y - s * 0.55),
                    Pos2::new(c.x + s * 0.55, c.y),
                    Pos2::new(c.x - s * 0.45, c.y + s * 0.55),
                ],
                color,
                Stroke::NONE,
            ));
        }
        StatusKind::Orgasm => {
            // Convex pieces only — PathShape fill tessellates poorly and can draw a
            // vertical "impale" seam through the cleft at some zoom levels.
            draw_heart(painter, c, s * 0.95, color);
        }
        StatusKind::Warn => {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(c.x, c.y - s * 0.75),
                    Pos2::new(c.x + s * 0.75, c.y + s * 0.65),
                    Pos2::new(c.x - s * 0.75, c.y + s * 0.65),
                ],
                color,
                Stroke::NONE,
            ));
            let bar = Color32::WHITE;
            painter.rect_filled(
                Rect::from_center_size(
                    Pos2::new(c.x, c.y + s * 0.05),
                    Vec2::new(s * 0.18, s * 0.55),
                ),
                1.0,
                bar,
            );
            painter.circle_filled(Pos2::new(c.x, c.y + s * 0.48), s * 0.1, bar);
        }
        StatusKind::Fixed => {
            let stroke = Stroke::new((s * 0.22).max(1.5), color);
            painter.line_segment(
                [
                    Pos2::new(c.x - s * 0.7, c.y),
                    Pos2::new(c.x + s * 0.35, c.y),
                ],
                stroke,
            );
            painter.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(c.x + s * 0.7, c.y),
                    Pos2::new(c.x + s * 0.15, c.y - s * 0.45),
                    Pos2::new(c.x + s * 0.15, c.y + s * 0.45),
                ],
                color,
                Stroke::NONE,
            ));
            painter.line_segment(
                [
                    Pos2::new(c.x - s * 0.7, c.y - s * 0.45),
                    Pos2::new(c.x - s * 0.7, c.y + s * 0.45),
                ],
                stroke,
            );
        }
    }
}

/// Heart from two circles + a triangle (all convex) — stable at every zoom.
fn draw_heart(painter: &egui::Painter, c: Pos2, s: f32, color: Color32) {
    let r = s * 0.48;
    let lobe_y = c.y - r * 0.22;
    painter.circle_filled(Pos2::new(c.x - r * 0.55, lobe_y), r, color);
    painter.circle_filled(Pos2::new(c.x + r * 0.55, lobe_y), r, color);
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(c.x - r * 1.12, lobe_y + r * 0.05),
            Pos2::new(c.x + r * 1.12, lobe_y + r * 0.05),
            Pos2::new(c.x, c.y + r * 1.25),
        ],
        color,
        Stroke::NONE,
    ));
}

#[derive(Clone, Copy)]
enum ToolbarIcon {
    Center,
    Fit,
    Arrange,
}

fn toolbar_icon_button(ui: &mut egui::Ui, icon: ToolbarIcon, tip: &str) -> egui::Response {
    let size = egui::vec2(TOOLBAR_BTN, TOOLBAR_BTN);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let visuals = ui.style().interact(&resp);
    ui.painter().rect(
        rect,
        2.0,
        visuals.weak_bg_fill,
        visuals.bg_stroke,
        egui::StrokeKind::Inside,
    );
    let color = visuals.fg_stroke.color;
    draw_toolbar_icon(ui.painter(), icon, rect.shrink(4.0), color);
    resp.on_hover_text(tip)
}

fn toolbar_glyph_button(
    ui: &mut egui::Ui,
    glyph: &str,
    tip: &str,
    color: Option<Color32>,
    enabled: bool,
) -> egui::Response {
    let size = egui::vec2(TOOLBAR_BTN, TOOLBAR_BTN);
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, mut resp) = ui.allocate_exact_size(size, sense);
    if !enabled {
        resp = resp.on_disabled_hover_text(tip);
    }
    let visuals = ui.style().interact(&resp);
    ui.painter().rect(
        rect,
        2.0,
        visuals.weak_bg_fill,
        visuals.bg_stroke,
        egui::StrokeKind::Inside,
    );
    let fg = color.unwrap_or(visuals.fg_stroke.color);
    let fg = if enabled { fg } else { Color32::from_gray(130) };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        egui::FontId::proportional(14.0),
        fg,
    );
    if enabled {
        resp.on_hover_text(tip)
    } else {
        resp
    }
}

fn draw_toolbar_icon(painter: &egui::Painter, icon: ToolbarIcon, rect: Rect, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) * 0.5;
    let stroke = Stroke::new((s * 0.22).max(1.1), color);
    match icon {
        ToolbarIcon::Center => {
            // Crosshair / compress-to-center
            painter.line_segment(
                [Pos2::new(c.x - s, c.y), Pos2::new(c.x - s * 0.25, c.y)],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(c.x + s * 0.25, c.y), Pos2::new(c.x + s, c.y)],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(c.x, c.y - s), Pos2::new(c.x, c.y - s * 0.25)],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(c.x, c.y + s * 0.25), Pos2::new(c.x, c.y + s)],
                stroke,
            );
            painter.circle_stroke(c, s * 0.28, stroke);
        }
        ToolbarIcon::Fit => {
            // Bounding box with corner ticks (fit-to-view)
            let r = Rect::from_center_size(c, egui::vec2(s * 1.6, s * 1.2));
            painter.rect_stroke(r, 1.0, stroke, egui::StrokeKind::Middle);
            let t = s * 0.35;
            for (x, y, dx, dy) in [
                (r.left(), r.top(), 1.0, 1.0),
                (r.right(), r.top(), -1.0, 1.0),
                (r.left(), r.bottom(), 1.0, -1.0),
                (r.right(), r.bottom(), -1.0, -1.0),
            ] {
                painter.line_segment([Pos2::new(x, y), Pos2::new(x + dx * t, y)], stroke);
                painter.line_segment([Pos2::new(x, y), Pos2::new(x, y + dy * t)], stroke);
            }
        }
        ToolbarIcon::Arrange => {
            // Three stacked nodes (arrange / hierarchy)
            let w = s * 1.1;
            let h = s * 0.45;
            let gap = s * 0.22;
            for i in 0..3 {
                let y = c.y - (h + gap) + i as f32 * (h + gap);
                let node = Rect::from_center_size(Pos2::new(c.x, y), egui::vec2(w, h));
                painter.rect_stroke(node, 1.0, stroke, egui::StrokeKind::Middle);
            }
        }
    }
}

/// Hover-control icons (Edit / Clone / CloneTo / Delete). Root is drawn as text by the caller.
fn draw_ctrl_icon(painter: &egui::Painter, btn: NodeButton, rect: Rect, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) * 0.5;
    let stroke = Stroke::new((s * 0.18).max(1.25), color);
    match btn {
        NodeButton::Edit => {
            let a = Pos2::new(c.x - s * 0.55, c.y + s * 0.55);
            let b = Pos2::new(c.x + s * 0.25, c.y - s * 0.25);
            painter.line_segment([a, b], stroke);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    a,
                    Pos2::new(a.x + s * 0.22, a.y - s * 0.08),
                    Pos2::new(a.x + s * 0.08, a.y - s * 0.22),
                ],
                color,
                Stroke::NONE,
            ));
            let e0 = Pos2::new(c.x + s * 0.15, c.y - s * 0.55);
            let e1 = Pos2::new(c.x + s * 0.55, c.y - s * 0.15);
            painter.line_segment([e0, e1], stroke);
            painter.line_segment([Pos2::new(c.x + s * 0.05, c.y - s * 0.35), e0], stroke);
            painter.line_segment([Pos2::new(c.x + s * 0.35, c.y - s * 0.05), e1], stroke);
        }
        NodeButton::Clone => {
            let back = Rect::from_min_max(
                Pos2::new(c.x - s * 0.15, c.y - s * 0.55),
                Pos2::new(c.x + s * 0.55, c.y + s * 0.15),
            );
            let front = Rect::from_min_max(
                Pos2::new(c.x - s * 0.55, c.y - s * 0.15),
                Pos2::new(c.x + s * 0.15, c.y + s * 0.55),
            );
            painter.rect_stroke(back, 2.0, stroke, egui::StrokeKind::Middle);
            painter.rect_filled(front, 2.0, crate::theme::SCENE_NODE_BG);
            painter.rect_stroke(front, 2.0, stroke, egui::StrokeKind::Middle);
        }
        NodeButton::CloneTo => {
            let back = Rect::from_min_max(
                Pos2::new(c.x - s * 0.55, c.y - s * 0.45),
                Pos2::new(c.x + s * 0.05, c.y + s * 0.15),
            );
            let front = Rect::from_min_max(
                Pos2::new(c.x - s * 0.35, c.y - s * 0.2),
                Pos2::new(c.x + s * 0.25, c.y + s * 0.4),
            );
            painter.rect_stroke(back, 1.5, stroke, egui::StrokeKind::Middle);
            painter.rect_filled(front, 1.5, crate::theme::SCENE_NODE_BG);
            painter.rect_stroke(front, 1.5, stroke, egui::StrokeKind::Middle);
            let tip = Pos2::new(c.x + s * 0.65, c.y + s * 0.05);
            painter.line_segment([Pos2::new(c.x + s * 0.2, c.y + s * 0.05), tip], stroke);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    tip,
                    Pos2::new(tip.x - s * 0.28, tip.y - s * 0.22),
                    Pos2::new(tip.x - s * 0.28, tip.y + s * 0.22),
                ],
                color,
                Stroke::NONE,
            ));
        }
        NodeButton::Delete => {
            let o = s * 0.45;
            painter.line_segment(
                [Pos2::new(c.x - o, c.y - o), Pos2::new(c.x + o, c.y + o)],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(c.x + o, c.y - o), Pos2::new(c.x - o, c.y + o)],
                stroke,
            );
        }
        NodeButton::Root => {}
    }
}

fn draw_mesh_grid(painter: &egui::Painter, rect: Rect, pan: Vec2, zoom: f32, dark: bool) {
    let mut step_world = GRID_STEP;
    while step_world * zoom < 20.0 {
        step_world *= 2.0;
        if step_world > GRID_STEP * 64.0 {
            break;
        }
    }
    let step = step_world * zoom;

    let dot = if dark {
        Color32::from_rgba_unmultiplied(255, 255, 255, 12)
    } else {
        Color32::from_rgba_unmultiplied(33, 35, 48, 14)
    };
    let major_line = if dark {
        Color32::from_rgba_unmultiplied(255, 255, 255, 10)
    } else {
        Color32::from_rgba_unmultiplied(33, 35, 48, 10)
    };

    let origin = rect.center() + pan * zoom;
    let start_x = ((rect.left() - origin.x) / step).floor() as i32 - 1;
    let end_x = ((rect.right() - origin.x) / step).ceil() as i32 + 1;
    let start_y = ((rect.top() - origin.y) / step).floor() as i32 - 1;
    let end_y = ((rect.bottom() - origin.y) / step).ceil() as i32 + 1;

    for i in start_x..=end_x {
        let x = origin.x + i as f32 * step;
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(1.0, major_line),
        );
    }
    for j in start_y..=end_y {
        let y = origin.y + j as f32 * step;
        painter.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            Stroke::new(1.0, major_line),
        );
        for i in start_x..=end_x {
            let x = origin.x + i as f32 * step;
            painter.circle_filled(Pos2::new(x, y), 1.0, dot);
        }
    }
}

/// Softer than shell/extreme bg — easier on the eyes for long graph sessions.
fn graph_bg(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x1a, 0x1a, 0x1a)
    } else {
        Color32::from_rgb(0xeb, 0xeb, 0xed)
    }
}

fn layout_nodes(scene: &Scene) -> HashMap<NanoID, Pos2> {
    let mut map = HashMap::new();
    for (i, stage) in scene.stages.iter().enumerate() {
        if let Some(node) = scene.graph.get(&stage.id) {
            map.insert(stage.id.clone(), Pos2::new(node.x, node.y));
            continue;
        }
        let col = (i % 4) as f32;
        let row = (i / 4) as f32;
        map.insert(
            stage.id.clone(),
            Pos2::new(40.0 + col * (NODE_W + 60.0), 40.0 + row * (NODE_H + 80.0)),
        );
    }
    map
}

pub fn ensure_graph_node(scene: &mut Scene, stage_id: &NanoID, index: usize) {
    scene.graph.entry(stage_id.clone()).or_insert_with(|| {
        let col = (index % 4) as f32;
        let row = (index / 4) as f32;
        Node {
            dest: Vec::new(),
            x: 40.0 + col * (NODE_W + 40.0),
            y: 40.0 + row * (NODE_H + 40.0),
        }
    });
}
