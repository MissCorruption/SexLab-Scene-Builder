use crate::furniture::{furniture_label, FURNITURE_GROUPS};
use crate::graph::{self, GraphAction, GraphView};
use crate::graph_layout::{arrange_scene, graph_coords_all_zeros, graph_coords_stacked};
use crate::io::{self, DialogResult};
use crate::jobs::{ChannelProgress, JobEvent, JobUi};
use crate::layout;
use crate::prefs::{Prefs, ThemePref};
use crate::stage_editor::{show_stage_editor, StageEditorAction, StageEditorState};
use crate::tag_tree::{tag_tree_ui, TagTreeState};
use crate::toasts::{ToastKind, Toasts};
use crate::workspace::Workspace;
use eframe::App;
use egui::{Context, RichText};
use log::{error, info};
use scene_builder_core::project::define::Node as GraphNode;
use scene_builder_core::project::package::{ExportKind, Package};
use scene_builder_core::project::scene::Scene;
use scene_builder_core::project::stage::Stage;
use scene_builder_core::project::NanoID;
use scene_builder_core::Progress;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

const WIKI_URL: &str =
    "https://slp-community.github.io/SexLab-Wiki/slsb/creating-packs-using-slsb/";
const DISCORD_URL: &str = "https://discord.gg/JPSHb4ebqj";
const PATREON_URL: &str = "https://www.patreon.com/ScrabJoseline";
const KOFI_URL: &str = "https://ko-fi.com/scrab";
const KOFI_MISS_URL: &str = "https://ko-fi.com/misscorruption";
const REPO_URL: &str = "https://github.com/SLP-Community/SexLab-Scene-Builder";

enum PendingAction {
    New,
    Open,
    ImportSlal,
    Quit,
}

/// Pre-export confirmations (Pandora clip tip and merge warning).
enum ExportConfirm {
    Tip {
        kind: ExportKind,
        dont_show: bool,
    },
    Merge {
        path: PathBuf,
        kind: ExportKind,
        dont_show: bool,
    },
}

pub struct SceneBuilderApp {
    ws: Workspace,
    prefs: Prefs,
    graph: GraphView,
    stage_editor: Option<StageEditorState>,
    job: JobUi,
    job_rx: Receiver<JobEvent>,
    job_tx: Sender<JobEvent>,
    dialog_rx: Receiver<DialogResult>,
    dialog_tx: Sender<DialogResult>,
    show_close_confirm: bool,
    show_about: bool,
    pending_after_confirm: Option<PendingAction>,
    status: String,
    /// Stage awaiting a target scene in the "Clone to…" modal.
    clone_to: Option<NanoID>,
    clone_to_search: String,
    scene_search: String,
    confirm_clear_canvas: bool,
    confirm_delete_scene: Option<NanoID>,
    export_confirm: Option<ExportConfirm>,
    tag_tree_state: TagTreeState,
    race_keys: Vec<String>,
    toasts: Toasts,
}

impl SceneBuilderApp {
    pub const APP_TITLE: &'static str = "SexLab Scene Builder";

    pub fn new(prefs: Prefs) -> Self {
        let (job_tx, job_rx) = mpsc::channel();
        let (dialog_tx, dialog_rx) = mpsc::channel();
        Self {
            ws: Workspace::new(),
            prefs,
            graph: GraphView::default(),
            stage_editor: None,
            job: JobUi::default(),
            job_rx,
            job_tx,
            dialog_rx,
            dialog_tx,
            show_close_confirm: false,
            show_about: false,
            pending_after_confirm: None,
            status: String::new(),
            clone_to: None,
            clone_to_search: String::new(),
            scene_search: String::new(),
            confirm_clear_canvas: false,
            confirm_delete_scene: None,
            export_confirm: None,
            tag_tree_state: TagTreeState::default(),
            race_keys: scene_builder_core::racekeys::get_race_keys_string(),
            toasts: Toasts::default(),
        }
    }

    fn window_title(&self) -> String {
        let name = self.ws.pack_display_name();
        if self.ws.dirty {
            format!("* {} - {}", name, Self::APP_TITLE)
        } else if self.ws.package.pack_name.is_empty() && !self.ws.has_save_path() {
            Self::APP_TITLE.to_string()
        } else {
            format!("{} - {}", name, Self::APP_TITLE)
        }
    }

    fn mark_dirty(&mut self) {
        self.ws.mark_dirty();
    }

    fn request_if_clean(&mut self, action: PendingAction) {
        if self.ws.dirty {
            self.pending_after_confirm = Some(action);
            self.show_close_confirm = true;
        } else {
            self.run_pending(action);
        }
    }

    fn run_pending(&mut self, action: PendingAction) {
        match action {
            PendingAction::New => {
                self.ws.reset();
                self.graph.selected = None;
                self.stage_editor = None;
                self.status = "New project".into();
            }
            PendingAction::Open => io::spawn_open(self.dialog_tx.clone()),
            PendingAction::ImportSlal => io::spawn_slal(self.dialog_tx.clone()),
            PendingAction::Quit => {}
        }
    }

    fn save_project(&mut self, save_as: bool) {
        if !save_as && self.ws.has_save_path() {
            let path = self.ws.package.pack_path.clone();
            match self.ws.package.write(path) {
                Ok(()) => {
                    self.ws.dirty = false;
                    self.status = "Saved".into();
                }
                Err(e) => {
                    self.status = format!("Save failed: {e}");
                    error!("{e}");
                }
            }
        } else {
            let suggested = if self.ws.package.pack_name.is_empty() {
                "project.slsb.json".into()
            } else {
                format!("{}.slsb.json", self.ws.package.pack_name)
            };
            io::spawn_save_as(self.dialog_tx.clone(), suggested);
        }
    }

    /// Show the Pandora clip tip before export unless the user dismissed it.
    fn request_export(&mut self, kind: ExportKind) {
        if self.prefs.hide_export_clip_tip {
            io::spawn_export(self.dialog_tx.clone(), kind);
        } else {
            self.export_confirm = Some(ExportConfirm::Tip {
                kind,
                dont_show: false,
            });
        }
    }

    /// Warn when soft-merging into a non-empty export folder unless dismissed.
    fn export_dir_chosen(&mut self, path: PathBuf, kind: ExportKind) {
        let (_, write_roots) = self.ws.package.resolve_export_paths(&path, kind);
        let would_merge = write_roots
            .iter()
            .any(|p| scene_builder_core::project::package::dir_nonempty(p));
        if would_merge && !self.prefs.hide_export_merge_warn {
            self.export_confirm = Some(ExportConfirm::Merge {
                path,
                kind,
                dont_show: false,
            });
        } else {
            self.start_export(path, kind);
        }
    }

    fn start_export(&mut self, parent: PathBuf, kind: ExportKind) {
        let pack = self.ws.package.clone();
        let tx = self.job_tx.clone();
        self.job = JobUi {
            active: true,
            title: "Export".into(),
            message: "Starting…".into(),
            fraction: 0.0,
        };
        thread::spawn(move || {
            let progress = ChannelProgress::new(tx.clone());
            progress.set_title("Export");
            progress.set_message("Resolving paths…");
            progress.set_fraction(0.1);
            let (pack_root, _) = pack.resolve_export_paths(&parent, kind);
            progress.set_message(format!("Writing to {}…", pack_root.display()).as_str());
            progress.set_fraction(0.3);
            let result = pack.export_into(&pack_root, kind);
            match result {
                Ok(()) => {
                    progress.set_fraction(1.0);
                    progress.set_message("Done");
                    let _ = tx.send(JobEvent::Finished {
                        ok: true,
                        message: format!("Exported to {}", pack_root.display()),
                    });
                }
                Err(e) => {
                    let _ = tx.send(JobEvent::Finished {
                        ok: false,
                        message: e,
                    });
                }
            }
        });
    }

    fn start_slal_pack_import(&mut self, dir: PathBuf) {
        let tx = self.job_tx.clone();
        self.job = JobUi {
            active: true,
            title: "Import SLAL pack".into(),
            message: "Scanning folder…".into(),
            fraction: 0.1,
        };
        thread::spawn(move || {
            let progress = ChannelProgress::new(tx.clone());
            progress.set_title("Import SLAL pack");
            progress.set_message("Reading pack…");
            match Package::from_slal_pack(dir, Some(&progress)) {
                Ok(pack) => {
                    let n = pack.scenes.len();
                    let _ = tx.send(JobEvent::PackageUpdated {
                        package: pack,
                        message: format!("Imported {n} scene(s) from SLAL pack"),
                        dirty: false,
                    });
                }
                Err(e) => {
                    let _ = tx.send(JobEvent::Finished {
                        ok: false,
                        message: e,
                    });
                }
            }
        });
    }

    fn start_enrich_slanim(&mut self, paths: Vec<PathBuf>) {
        let mut pack = self.ws.package.clone();
        let tx = self.job_tx.clone();
        self.job = JobUi {
            active: true,
            title: "Enrich SLAnim".into(),
            message: "Reading sources…".into(),
            fraction: 0.2,
        };
        thread::spawn(move || {
            let progress = ChannelProgress::new(tx.clone());
            progress.set_title("Enrich SLAnim");
            progress.set_message("Applying…");
            match pack.enrich_from_slanim_paths(&paths) {
                Ok(summary) => {
                    let msg = summary.message();
                    let _ = tx.send(JobEvent::PackageUpdated {
                        package: pack,
                        message: msg,
                        dirty: true,
                    });
                }
                Err(e) => {
                    let _ = tx.send(JobEvent::Finished {
                        ok: false,
                        message: e,
                    });
                }
            }
        });
    }

    fn start_enrich_fnis(&mut self, paths: Vec<PathBuf>) {
        let mut pack = self.ws.package.clone();
        let tx = self.job_tx.clone();
        self.job = JobUi {
            active: true,
            title: "Enrich FNIS".into(),
            message: "Reading AnimLists…".into(),
            fraction: 0.2,
        };
        thread::spawn(move || {
            let progress = ChannelProgress::new(tx.clone());
            progress.set_title("Enrich FNIS");
            progress.set_message("Applying…");
            match pack.enrich_from_fnis_paths(&paths) {
                Ok(summary) => {
                    let msg = summary.message_fnis();
                    let _ = tx.send(JobEvent::PackageUpdated {
                        package: pack,
                        message: msg,
                        dirty: true,
                    });
                }
                Err(e) => {
                    let _ = tx.send(JobEvent::Finished {
                        ok: false,
                        message: e,
                    });
                }
            }
        });
    }

    fn poll_channels(&mut self, ctx: &Context) {
        while let Ok(ev) = self.job_rx.try_recv() {
            match ev {
                JobEvent::Progress {
                    title,
                    message,
                    fraction,
                } => {
                    self.job.active = true;
                    self.job.title = title;
                    self.job.message = message;
                    self.job.fraction = fraction;
                }
                JobEvent::Finished { ok, message } => {
                    self.job.active = false;
                    self.status = message.clone();
                    if !ok {
                        error!("{message}");
                    } else {
                        info!("{message}");
                    }
                }
                JobEvent::PackageUpdated {
                    package,
                    message,
                    dirty,
                } => {
                    self.ws.set_package(package, dirty);
                    self.graph.selected = None;
                    self.job.active = false;
                    self.stage_editor = None;
                    self.status = message;
                    if let Some(id) = self.ws.package.scenes.keys().next().cloned() {
                        self.select_scene(id);
                    }
                }
            }
            ctx.request_repaint();
        }

        while let Ok(ev) = self.dialog_rx.try_recv() {
            match ev {
                DialogResult::Open(path) => match Package::load_from_path(path) {
                    Ok(pack) => {
                        self.ws.package = pack;
                        self.ws.dirty = false;
                        self.stage_editor = None;
                        self.status = format!("Opened {}", self.ws.package.pack_path.display());
                        if let Some(id) = self.ws.package.scenes.keys().next().cloned() {
                            self.select_scene(id);
                        } else {
                            self.ws.selected_scene = None;
                            self.graph.selected = None;
                        }
                    }
                    Err(e) => {
                        self.status = format!("Open failed: {e}");
                        error!("{e}");
                    }
                },
                DialogResult::OpenSlal(path) => self.start_slal_pack_import(path),
                DialogResult::OpenOffset(path) => {
                    match self.ws.package.import_offset_from_path(path) {
                        Ok(()) => {
                            self.ws.dirty = true;
                            self.status = "Imported offsets".into();
                        }
                        Err(e) => {
                            self.status = format!("Offset import failed: {e}");
                            error!("{e}");
                        }
                    }
                }
                DialogResult::SaveAs(path) => match self.ws.package.write(path) {
                    Ok(()) => {
                        self.ws.dirty = false;
                        self.status = "Saved".into();
                    }
                    Err(e) => {
                        self.status = format!("Save failed: {e}");
                        error!("{e}");
                    }
                },
                DialogResult::ExportDir { path, kind } => self.export_dir_chosen(path, kind),
                DialogResult::EnrichSlanim(paths) => self.start_enrich_slanim(paths),
                DialogResult::EnrichFnis(paths) => self.start_enrich_fnis(paths),
                DialogResult::Cancelled => {}
            }
            ctx.request_repaint();
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui, ctx: &Context) {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui
                    .add(egui::Button::new("New").shortcut_text("Ctrl+N"))
                    .clicked()
                {
                    self.request_if_clean(PendingAction::New);
                    ui.close_menu();
                }
                if ui
                    .add(egui::Button::new("Open…").shortcut_text("Ctrl+O"))
                    .clicked()
                {
                    self.request_if_clean(PendingAction::Open);
                    ui.close_menu();
                }
                if ui
                    .add(egui::Button::new("Save").shortcut_text("Ctrl+S"))
                    .clicked()
                {
                    self.save_project(false);
                    ui.close_menu();
                }
                if ui
                    .add(egui::Button::new("Save As…").shortcut_text("Ctrl+Shift+S"))
                    .clicked()
                {
                    self.save_project(true);
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Import SLAL pack…").clicked() {
                    self.request_if_clean(PendingAction::ImportSlal);
                    ui.close_menu();
                }
                if ui.button("Import Offset…").clicked() {
                    io::spawn_offset(self.dialog_tx.clone());
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Export SLSB…").clicked() {
                    self.request_export(ExportKind::Slsb);
                    ui.close_menu();
                }
                if ui.button("Export SLAL…").clicked() {
                    self.request_export(ExportKind::Slal);
                    ui.close_menu();
                }
                if ui
                    .add(egui::Button::new("Export Both…").shortcut_text("Ctrl+B"))
                    .clicked()
                {
                    self.request_export(ExportKind::Both);
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    if self.ws.dirty {
                        self.pending_after_confirm = Some(PendingAction::Quit);
                        self.show_close_confirm = true;
                    } else {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    ui.close_menu();
                }
            });
            ui.menu_button("Tools", |ui| {
                if ui.button("Enrich SLAnim…").clicked() {
                    io::spawn_enrich_slanim(self.dialog_tx.clone());
                    ui.close_menu();
                }
                if ui.button("Enrich FNIS…").clicked() {
                    io::spawn_enrich_fnis(self.dialog_tx.clone());
                    ui.close_menu();
                }
            });
            ui.menu_button("View", |ui| {
                ui.menu_button("Theme", |ui| {
                    for (label, pref) in [
                        ("System", ThemePref::System),
                        ("Light", ThemePref::Light),
                        ("Dark", ThemePref::Dark),
                    ] {
                        if ui
                            .selectable_label(self.prefs.theme == pref, label)
                            .clicked()
                        {
                            self.prefs.theme = pref;
                            pref.apply(ctx);
                            self.prefs.save();
                            ui.close_menu();
                        }
                    }
                });
                #[cfg(windows)]
                {
                    let mut show = self.prefs.show_console;
                    if ui
                        .checkbox(&mut show, "Show console")
                        .on_hover_text("Attach a console window for log output (also: --console)")
                        .changed()
                    {
                        self.prefs.show_console = show;
                        self.prefs.save();
                        if show {
                            let _ = crate::console_win::show();
                            info!("Console enabled");
                        } else {
                            crate::console_win::hide();
                        }
                    }
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("Wiki").clicked() {
                    let _ = open::that(WIKI_URL);
                    ui.close_menu();
                }
                if ui.button("About").clicked() {
                    self.show_about = true;
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Discord").clicked() {
                    let _ = open::that(DISCORD_URL);
                    ui.close_menu();
                }
                if ui.button("Patreon").clicked() {
                    let _ = open::that(PATREON_URL);
                    ui.close_menu();
                }
                if ui.button("Ko-Fi (Scrab)").clicked() {
                    let _ = open::that(KOFI_URL);
                    ui.close_menu();
                }
                if ui.button("Ko-Fi (Miss Corruption)").clicked() {
                    let _ = open::that(KOFI_MISS_URL);
                    ui.close_menu();
                }
            });
        });
    }

    fn left_panel(&mut self, ui: &mut egui::Ui) {
        crate::theme::fill_width(ui);
        let full = ui.available_width();
        let muted = crate::theme::text_muted(ui.visuals().dark_mode);
        for (value, hint) in [
            (&mut self.ws.package.pack_name, "Package Name"),
            (&mut self.ws.package.pack_author, "Author Name"),
            (&mut self.ws.package.pack_version, "Pack Version"),
        ] {
            if ui
                .add(
                    egui::TextEdit::singleline(value)
                        .hint_text(RichText::new(hint).color(muted).italics())
                        .desired_width(full),
                )
                .changed()
            {
                self.ws.dirty = true;
            }
        }

        ui.separator();

        if ui
            .add(egui::Button::new("+  New Scene").frame(false))
            .clicked()
        {
            self.add_blank_scene();
        }

        ui.add(
            egui::TextEdit::singleline(&mut self.scene_search)
                .hint_text("Search scenes")
                .desired_width(full),
        );

        let mut to_delete: Option<NanoID> = None;
        let mut to_select: Option<NanoID> = None;
        let count = self.ws.package.scenes.len();
        let header = if count > 0 {
            format!("Scenes ({count})")
        } else {
            "Scenes".to_string()
        };
        let needle = self.scene_search.trim().to_lowercase();
        let mut rows: Vec<(NanoID, String, bool)> = self
            .ws
            .package
            .scenes
            .iter()
            .map(|(id, scene)| {
                let label = if scene.name.is_empty() {
                    id.0.clone()
                } else {
                    scene.name.clone()
                };
                (id.clone(), label, scene.has_warnings)
            })
            .collect();
        if !needle.is_empty() {
            rows.retain(|(_, label, _)| label.to_lowercase().contains(&needle));
        }
        rows.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .hscroll(false)
            .show(ui, |ui| {
                crate::theme::fill_width(ui);
                egui::CollapsingHeader::new(header)
                    .default_open(true)
                    .show(ui, |ui| {
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            crate::theme::fill_width(ui);
                            for (id, label, has_warnings) in &rows {
                                let selected = self.ws.selected_scene.as_ref() == Some(id);
                                let icon = if *has_warnings {
                                    RichText::new("⚠").color(egui::Color32::RED)
                                } else {
                                    RichText::new("◆").color(egui::Color32::from_rgb(17, 175, 17))
                                };
                                ui.horizontal(|ui| {
                                    crate::theme::fill_width(ui);
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    ui.label(icon);
                                    let resp = truncated_selectable(ui, selected, label)
                                        .on_hover_text(label);
                                    if resp.clicked() {
                                        to_select = Some(id.clone());
                                    }
                                    resp.context_menu(|ui| {
                                        if ui.button("Edit").clicked() {
                                            to_select = Some(id.clone());
                                            ui.close_menu();
                                        }
                                        if ui
                                            .button(
                                                RichText::new("Delete").color(egui::Color32::RED),
                                            )
                                            .clicked()
                                        {
                                            to_delete = Some(id.clone());
                                            ui.close_menu();
                                        }
                                    });
                                });
                            }
                        });
                    });
            });

        if let Some(id) = to_select {
            self.select_scene(id);
        }
        if let Some(id) = to_delete {
            self.confirm_delete_scene = Some(id);
        }
    }

    fn add_blank_scene(&mut self) {
        let mut scene = Scene::default();
        scene.name = format!("Scene {}", self.ws.package.scenes.len() + 1);
        let stage = Stage::new(&scene);
        scene.root = stage.id.clone();
        graph::ensure_graph_node(&mut scene, &stage.id, 0);
        scene.stages.push(stage);
        scene.positions = scene
            .stages
            .first()
            .map(|s| {
                s.positions
                    .iter()
                    .map(|p| p.extract_position_info())
                    .collect()
            })
            .unwrap_or_default();
        let id = scene.id.clone();
        self.ws.package.save_scene(scene);
        self.select_scene(id);
        self.mark_dirty();
    }

    fn select_scene(&mut self, id: NanoID) {
        self.ws.selected_scene = Some(id.clone());
        self.graph.selected = None;
        self.graph.request_fit();
        if let Some(scene) = self.ws.package.get_scene_mut(&id) {
            if graph_coords_stacked(scene) || graph_coords_all_zeros(scene) {
                arrange_scene(scene);
            }
        }
    }

    fn delete_stage_from_scene(&mut self, scene_id: &NanoID, stage_id: &NanoID) {
        let Some(scene) = self.ws.package.get_scene_mut(scene_id) else {
            return;
        };
        self.graph.push_undo(scene);
        scene.stages.retain(|s| &s.id != stage_id);
        scene.graph.remove(stage_id);
        for node in scene.graph.values_mut() {
            node.dest.retain(|d| d != stage_id);
        }
        if scene.root == *stage_id {
            scene.root = scene
                .stages
                .first()
                .map(|s| s.id.clone())
                .unwrap_or_else(NanoID::new_nanoid);
        }
        if self.graph.selected.as_ref() == Some(stage_id) {
            self.graph.selected = None;
        }
        Stage::renumber_auto_names(scene);
        self.mark_dirty();
    }

    fn set_scene_root(&mut self, scene_id: &NanoID, stage_id: &NanoID) {
        let Some(scene) = self.ws.package.get_scene_mut(scene_id) else {
            return;
        };
        if scene.stages.iter().any(|s| &s.id == stage_id) {
            scene.root = stage_id.clone();
            self.mark_dirty();
        }
    }

    fn center_panel(&mut self, ui: &mut egui::Ui) {
        ui.set_clip_rect(ui.clip_rect().intersect(ui.max_rect()));
        let Some(scene_id) = self.ws.selected_scene.clone() else {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() * 0.4);
                    ui.label(RichText::new("No scene loaded :(").weak());
                    ui.add_space(layout::SPACE);
                    if ui.button("New Scene").clicked() {
                        self.add_blank_scene();
                    }
                });
            });
            return;
        };

        let mut open_editor: Option<NanoID> = None;
        let mut add_stage = false;
        let mut store = false;
        let mut rename: Option<String> = None;
        let mut toolbar_action = crate::graph::GraphAction::None;

        {
            let Some(scene) = self.ws.package.get_scene_mut(&scene_id) else {
                ui.label("Scene missing");
                return;
            };

            // Name | graph controls | Add Stage + Store, packed from the right
            // of the *clipped* center column so the action buttons cannot paint
            // over the tags sidebar when the leftover width is under ~616px.
            let full = ui.available_rect_before_wrap().intersect(ui.clip_rect());
            let row_h = ui.spacing().interact_size.y.max(28.0);
            let (left_rect, mid_rect, right_rect) = scene_header_strips(full, row_h);

            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(left_rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                |ui| {
                    ui.set_clip_rect(ui.clip_rect().intersect(left_rect));
                    // Always take the dirty-slot so the name field's id does not
                    // jump when ≠ appears after the first keystroke.
                    let dirty_sz = egui::vec2(22.0, 22.0);
                    let (dirty_rect, dirty_resp) =
                        ui.allocate_exact_size(dirty_sz, egui::Sense::hover());
                    if self.ws.dirty {
                        ui.painter().text(
                            dirty_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "≠",
                            egui::FontId::proportional(22.0),
                            egui::Color32::RED,
                        );
                        dirty_resp.on_hover_text("Unsaved changes");
                    }
                    let mut name = scene.name.clone();
                    let name_edit = egui::TextEdit::singleline(&mut name)
                        .frame(false)
                        .hint_text("Scene Name")
                        .font(egui::TextStyle::Heading)
                        .id_salt(("scene_name", scene_id.0.as_str()))
                        .desired_width((ui.available_width() - 8.0).max(60.0));
                    let output = name_edit.show(ui);
                    if output.response.changed() {
                        rename = Some(name.clone());
                    }
                    if output.response.gained_focus() && output.response.clicked() {
                        if let Some(mut state) =
                            egui::TextEdit::load_state(ui.ctx(), output.response.id)
                        {
                            let range = egui::text::CCursorRange::two(
                                egui::text::CCursor::new(0),
                                egui::text::CCursor::new(name.chars().count()),
                            );
                            state.cursor.set_char_range(Some(range));
                            state.store(ui.ctx(), output.response.id);
                        }
                    }
                },
            );

            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(mid_rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                |ui| {
                    ui.set_clip_rect(ui.clip_rect().intersect(mid_rect));
                    ui.separator();
                    toolbar_action = self.graph.toolbar_ui(ui, scene);
                },
            );

            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(right_rect)
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
                |ui| {
                    ui.set_clip_rect(ui.clip_rect().intersect(right_rect));
                    let accent = crate::theme::accent(ui.visuals().dark_mode);
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Store").color(egui::Color32::WHITE))
                                .fill(accent),
                        )
                        .clicked()
                    {
                        store = true;
                    }
                    if ui.button("Add Stage").clicked() {
                        add_stage = true;
                    }
                    ui.separator();
                },
            );

            // Reserve the row; strip UIs are clipped so they must not expand width.
            let _ = ui.allocate_rect(
                egui::Rect::from_min_size(full.min, egui::vec2(full.width(), row_h)),
                egui::Sense::hover(),
            );

            ui.separator();
        }

        if let Some(name) = rename {
            if let Some(scene) = self.ws.package.get_scene_mut(&scene_id) {
                scene.name = name;
                self.mark_dirty();
            }
        }
        if store {
            self.store_scene(ui.ctx(), &scene_id);
        }

        let action = {
            let Some(scene) = self.ws.package.get_scene_mut(&scene_id) else {
                return;
            };
            egui::Frame::canvas(ui.style())
                .show(ui, |ui| self.graph.ui(ui, scene))
                .inner
        };

        let action = if !matches!(action, crate::graph::GraphAction::None) {
            action
        } else {
            toolbar_action
        };

        match action {
            GraphAction::None => {}
            GraphAction::Select(_) => {}
            GraphAction::OpenEditor(id) => {
                open_editor = Some(id);
            }
            GraphAction::CloneStage(id) => {
                self.clone_stage_in_scene(&scene_id, &id);
            }
            GraphAction::CloneStageTo(id) => {
                self.clone_to = Some(id);
                self.clone_to_search.clear();
            }
            GraphAction::ClearCanvas => {
                self.confirm_clear_canvas = true;
            }
            GraphAction::SetRoot(id) => {
                self.set_scene_root(&scene_id, &id);
            }
            GraphAction::DeleteStage(id) => {
                self.delete_stage_from_scene(&scene_id, &id);
            }
            GraphAction::Arrange => {
                if let Some(scene) = self.ws.package.get_scene_mut(&scene_id) {
                    arrange_scene(scene);
                    self.mark_dirty();
                }
            }
            GraphAction::Dirty => {
                self.mark_dirty();
            }
        }

        if add_stage {
            if let Some(id) = self.add_stage_to_scene(&scene_id) {
                open_editor = Some(id);
            }
        }
        if let Some(stage_id) = open_editor {
            self.open_stage_editor(&scene_id, &stage_id);
        }
    }

    /// Adds a stage (optionally linked from the previous last stage). Returns the new id.
    fn add_stage_to_scene(&mut self, scene_id: &NanoID) -> Option<NanoID> {
        let scene = self.ws.package.get_scene_mut(scene_id)?;
        self.graph.push_undo(scene);
        let stage = Stage::new(scene);
        let id = stage.id.clone();
        let idx = scene.stages.len();
        if scene.stages.is_empty() {
            scene.root = id.clone();
        } else if let Some(prev) = scene.stages.last() {
            let prev_id = prev.id.clone();
            graph::ensure_graph_node(scene, &prev_id, idx.saturating_sub(1));
            if let Some(node) = scene.graph.get_mut(&prev_id) {
                if node.dest.is_empty() {
                    node.dest.push(id.clone());
                }
            }
        }
        graph::ensure_graph_node(scene, &id, idx);
        scene.stages.push(stage);
        Stage::renumber_auto_names(scene);
        self.graph.selected = Some(id.clone());
        self.mark_dirty();
        Some(id)
    }

    /// Validate name/root/reachability, toast problems, then persist has_warnings.
    fn store_scene(&mut self, ctx: &Context, scene_id: &NanoID) {
        let Some(scene) = self.ws.package.get_scene(scene_id) else {
            return;
        };
        let mut has_warnings = false;
        let mut do_save = true;

        if scene.name.trim().is_empty() {
            self.toasts.push(
                ctx,
                ToastKind::Error,
                "Missing Name",
                "Add a short, descriptive name to your scene.",
            );
            do_save = false;
        }

        let root_exists = scene.stages.iter().any(|s| s.id == scene.root);
        if !root_exists {
            self.toasts.push(
                ctx,
                ToastKind::Warning,
                "Missing Start Animation",
                "Choose the stage which the scene is supposed to start at.",
            );
            has_warnings = true;
        } else {
            // BFS from root across graph destinations.
            let mut visited = std::collections::HashSet::new();
            let mut queue = vec![scene.root.clone()];
            visited.insert(scene.root.clone());
            while let Some(id) = queue.pop() {
                if let Some(node) = scene.graph.get(&id) {
                    for dest in &node.dest {
                        if scene.stages.iter().any(|s| &s.id == dest)
                            && visited.insert(dest.clone())
                        {
                            queue.push(dest.clone());
                        }
                    }
                }
            }
            if visited.len() < scene.stages.len() {
                self.toasts.push(
                    ctx,
                    ToastKind::Warning,
                    "Unreachable Stages",
                    "Scene contains stages which cannot be reached from the start animation.",
                );
                has_warnings = true;
            }
        }

        if !do_save {
            return;
        }
        if let Some(scene) = self.ws.package.get_scene_mut(scene_id) {
            scene.has_warnings = has_warnings;
        }
        self.status = "Scene stored".into();
    }

    /// Duplicate a stage inside its own scene, offset from the original.
    fn clone_stage_in_scene(&mut self, scene_id: &NanoID, stage_id: &NanoID) {
        let Some(scene) = self.ws.package.get_scene_mut(scene_id) else {
            return;
        };
        let Some(orig) = scene.get_stage(stage_id) else {
            return;
        };
        let mut copy = orig.clone();
        copy.id = NanoID::new_nanoid();
        if Stage::is_auto_name(&copy.name) {
            copy.name = "Stage 0/0".into();
        }
        let new_id = copy.id.clone();
        let (x, y) = scene
            .graph
            .get(stage_id)
            .map(|n| (n.x + 40.0, n.y + 40.0))
            .unwrap_or((40.0, 40.0));
        self.graph.push_undo(scene);
        scene.graph.insert(
            new_id.clone(),
            GraphNode {
                dest: Vec::new(),
                x,
                y,
            },
        );
        scene.stages.push(copy);
        Stage::renumber_auto_names(scene);
        self.graph.selected = Some(new_id);
        self.mark_dirty();
    }

    /// Copy a stage into another scene (Clone to… modal target).
    fn clone_stage_to_scene(
        &mut self,
        ctx: &Context,
        stage_id: &NanoID,
        from_scene: &NanoID,
        to_scene: &NanoID,
    ) {
        let Some(stage) = self
            .ws
            .package
            .get_scene(from_scene)
            .and_then(|s| s.get_stage(stage_id))
            .cloned()
        else {
            self.toasts.push(
                ctx,
                ToastKind::Error,
                "Clone failed",
                "The source stage no longer exists.",
            );
            return;
        };
        let target_name;
        {
            let Some(target) = self.ws.package.get_scene_mut(to_scene) else {
                return;
            };
            let src_n = stage.positions.len();
            let mut copy = stage;
            copy.id = NanoID::new_nanoid();
            let idx = target.stages.len();
            graph::ensure_graph_node(target, &copy.id, idx);
            let toast_detail;
            if target.stages.is_empty() {
                target.root = copy.id.clone();
                target.positions = copy
                    .positions
                    .iter()
                    .map(|p| p.extract_position_info())
                    .collect();
                toast_detail = format!("Added to \"{}\".", target.name);
            } else {
                let dst_n = target.positions.len();
                if src_n.max(1) != dst_n.max(1) {
                    crate::positions::adopt_scene_position_count(target, src_n);
                    toast_detail = format!(
                        "Added to \"{}\" (scene positions {} → {}).",
                        target.name, dst_n, src_n
                    );
                } else {
                    toast_detail = format!("Added to \"{}\".", target.name);
                }
            }
            target_name = toast_detail;
            if Stage::is_auto_name(&copy.name) {
                copy.name = "Stage 0/0".into();
            }
            target.stages.push(copy);
            Stage::renumber_auto_names(target);
        }
        self.mark_dirty();
        self.toasts
            .push(ctx, ToastKind::Success, "Stage cloned", &target_name);
    }

    fn open_stage_editor(&mut self, scene_id: &NanoID, stage_id: &NanoID) {
        let Some(scene) = self.ws.package.get_scene(scene_id) else {
            return;
        };
        let Some(stage) = scene.get_stage(stage_id) else {
            return;
        };
        self.stage_editor = Some(StageEditorState::new(
            scene_id.clone(),
            stage.clone(),
            scene.positions.clone(),
        ));
    }

    /// Right column: Scene Tags (fills remaining height, scrolls) + Furniture (pinned).
    fn tags_furniture_panel(&mut self, ui: &mut egui::Ui) {
        let Some(scene_id) = self.ws.selected_scene.clone() else {
            return;
        };
        crate::theme::fill_width(ui);
        let avail = ui.available_rect_before_wrap();
        let panel_w = layout::finite_or(avail.width(), 200.0);
        let avail_h = layout::finite_or(avail.height(), 400.0);
        if panel_w <= 0.0 || avail_h <= 0.0 {
            return;
        }

        let furni_id = ui.id().with("furniture_h");
        let tags_min = layout::TAGS_SCROLL_MIN_H;
        let prev_furni = ui
            .ctx()
            .data(|d| d.get_temp::<f32>(furni_id))
            .filter(|h| h.is_finite())
            .unwrap_or(168.0);
        let furni_h = prev_furni.clamp(1.0, (avail_h - tags_min).max(1.0));
        let split_y = (avail.max.y - furni_h).max(avail.min.y + tags_min);
        let tags_rect = egui::Rect::from_min_max(avail.min, egui::pos2(avail.max.x, split_y));
        let furni_rect = egui::Rect::from_min_max(egui::pos2(avail.min.x, split_y), avail.max);

        {
            let mut tags_ui = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt("scene_tags_block")
                    .max_rect(tags_rect)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            tags_ui.set_clip_rect(tags_rect.intersect(ui.clip_rect()));
            tags_ui.set_max_width(panel_w);
            crate::theme::fill_width(&mut tags_ui);

            let mut copy_to_stages = false;
            tags_ui.label(RichText::new("Scene Tags").strong());
            tags_ui.horizontal_wrapped(|ui| {
                ui.set_max_width(panel_w);
                let has_stages = self
                    .ws
                    .package
                    .get_scene(&scene_id)
                    .map(|s| !s.stages.is_empty())
                    .unwrap_or(false);
                if ui
                    .add_enabled(has_stages, egui::Button::new("Copy").small())
                    .on_hover_text("Copy scene tags onto every stage (replaces each stage's tags).")
                    .clicked()
                {
                    copy_to_stages = true;
                }
                crate::theme::info_tip(
                    ui,
                    "Tags which are shared between all stages in the scene.",
                );
            });

            egui::ScrollArea::vertical()
                .id_salt("scene_tags_scroll")
                .auto_shrink([false, false])
                .hscroll(false)
                .show(&mut tags_ui, |ui| {
                    ui.set_max_width(panel_w);

                    let mut tags_changed = false;
                    let mut custom_changed = false;
                    if let Some(scene) = self.ws.package.get_scene_mut(&scene_id) {
                        let result = tag_tree_ui(
                            ui,
                            "scene_tags",
                            &mut self.tag_tree_state,
                            &mut scene.tags,
                            &mut self.prefs.custom_tags,
                        );
                        tags_changed = result.tags_changed;
                        custom_changed = result.custom_changed;
                    }
                    if copy_to_stages {
                        if let Some(scene) = self.ws.package.get_scene_mut(&scene_id) {
                            let copied = scene.tags.clone();
                            for stage in &mut scene.stages {
                                stage.tags = copied.clone();
                            }
                        }
                        self.mark_dirty();
                    }
                    if tags_changed {
                        self.mark_dirty();
                    }
                    if custom_changed {
                        self.prefs.save();
                    }
                });
        }

        {
            let mut furni_ui = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt("furniture_block")
                    .max_rect(furni_rect)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            furni_ui.set_clip_rect(furni_rect.intersect(ui.clip_rect()));
            furni_ui.set_max_width(panel_w);
            crate::theme::fill_width(&mut furni_ui);
            furni_ui.horizontal_wrapped(|ui| {
                crate::theme::fill_width(ui);
                ui.label(RichText::new("Furniture").strong());
                crate::theme::info_tip(ui, "Furniture settings for the scene.");
            });
            self.furniture_section(&mut furni_ui, &scene_id);
            furni_ui.add_space(4.0);
            let used = furni_ui.min_rect().height();
            if used.is_finite() {
                ui.ctx()
                    .data_mut(|d| d.insert_temp(furni_id, used.max(1.0)));
            }
        }

        ui.allocate_rect(avail, egui::Sense::hover());
    }

    fn furniture_section(&mut self, ui: &mut egui::Ui, scene_id: &NanoID) {
        let mut furni_changed = false;
        if let Some(scene) = self.ws.package.get_scene_mut(scene_id) {
            let furniture = &mut scene.furniture;
            let selected_label = {
                let names: Vec<&str> = furniture
                    .furni_types
                    .iter()
                    .map(|t| furniture_label(t))
                    .collect();
                if names.is_empty() {
                    "None".to_string()
                } else {
                    names.join(", ")
                }
            };
            egui::ComboBox::from_id_salt("furniture_select")
                .width(layout::finite_or(ui.available_width(), 120.0))
                .selected_text(selected_label)
                .show_ui(ui, |ui| {
                    let mut none_on = furniture.furni_types.iter().any(|t| t == "None");
                    if ui.checkbox(&mut none_on, "None").changed() {
                        furniture.furni_types = vec!["None".into()];
                        furni_changed = true;
                    }
                    for group in FURNITURE_GROUPS {
                        ui.label(RichText::new(group.label).small());
                        for (label, value) in group.options {
                            let mut on = furniture.furni_types.iter().any(|t| t == value);
                            if ui.checkbox(&mut on, *label).changed() {
                                if on {
                                    furniture.furni_types.retain(|t| t != "None");
                                    furniture.furni_types.push((*value).to_string());
                                    furniture.allow_bed = false;
                                } else {
                                    furniture.furni_types.retain(|t| t != value);
                                    if furniture.furni_types.is_empty() {
                                        furniture.furni_types = vec!["None".into()];
                                    }
                                }
                                furni_changed = true;
                            }
                        }
                    }
                });

            let none_selected = furniture.furni_types.iter().any(|t| t == "None");
            let mut allow_bed = furniture.allow_bed;
            if ui
                .add_enabled(
                    none_selected,
                    egui::Checkbox::new(&mut allow_bed, "Allow Bed"),
                )
                .changed()
            {
                furniture.allow_bed = allow_bed;
                furni_changed = true;
            }
            let mut private = scene.private;
            if ui.checkbox(&mut private, "Private").changed() {
                scene.private = private;
                furni_changed = true;
            }

            ui.add_space(4.0);
            // Avoid ui.columns — it expands the parent when column content
            // exceeds the soft max (was blowing the right panel to ~2k px).
            egui::Grid::new("furniture_offset_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .min_col_width(((ui.available_width() - 8.0) / 2.0).max(40.0))
                .show(ui, |ui| {
                    let offset = &mut scene.furniture.offset;
                    let fields: [(&str, &mut f32, Option<std::ops::RangeInclusive<f32>>); 4] = [
                        ("X", &mut offset.x, None),
                        ("Y", &mut offset.y, None),
                        ("Z", &mut offset.z, None),
                        ("°", &mut offset.r, Some(0.0..=359.9_f32)),
                    ];
                    for (i, (label, value, clamp)) in fields.into_iter().enumerate() {
                        let mut drag = egui::DragValue::new(value).speed(0.1).fixed_decimals(1);
                        if let Some(range) = clamp {
                            drag = drag.range(range);
                        }
                        if crate::theme::labeled_drag(ui, label, drag).changed() {
                            furni_changed = true;
                        }
                        if i % 2 == 1 {
                            ui.end_row();
                        }
                    }
                });
        }
        if furni_changed {
            self.mark_dirty();
        }
    }

    fn modals(&mut self, ctx: &Context) {
        if self.show_close_confirm {
            egui::Window::new("Unsaved changes")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("There are unsaved changes. Continue and discard them?");
                    ui.horizontal(|ui| {
                        if ui.button("Discard").clicked() {
                            self.show_close_confirm = false;
                            self.ws.dirty = false;
                            if let Some(action) = self.pending_after_confirm.take() {
                                match action {
                                    PendingAction::Quit => {
                                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                    }
                                    other => self.run_pending(other),
                                }
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_close_confirm = false;
                            self.pending_after_confirm = None;
                        }
                    });
                });
        }

        if let Some(confirm) = self.export_confirm.take() {
            let fnis_mod = self.ws.package.fnis_mod_name();
            let mut keep = Some(confirm);
            match keep.as_mut().unwrap() {
                ExportConfirm::Tip { kind, dont_show } => {
                    let kind = *kind;
                    let mut decided: Option<bool> = None;
                    egui::Window::new("Animation clips for Pandora")
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .show(ctx, |ui| {
                            ui.set_max_width(460.0);
                            ui.label(format!(
                                "Export writes into a subfolder named {fnis_mod} under the folder you pick.\n\n\
                                 It writes AnimLists, Behavior files, and registry data — not your .hkx animation clips.\n\n\
                                 Copy your animation HKX files into:\n\
                                 meshes/actors/<race>/animations/{fnis_mod}/\n\n\
                                 For humans that is usually:\n\
                                 meshes/actors/character/animations/{fnis_mod}/\n\n\
                                 Pandora only plays clips that live in the folder the Behavior references."
                            ));
                            ui.add_space(6.0);
                            ui.checkbox(dont_show, "Don't show this tip again on export");
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                if ui.button("Continue").clicked() {
                                    decided = Some(true);
                                }
                                if ui.button("Cancel").clicked() {
                                    decided = Some(false);
                                }
                            });
                        });
                    if let Some(proceed) = decided {
                        let dont_show = matches!(
                            keep.as_ref(),
                            Some(ExportConfirm::Tip {
                                dont_show: true,
                                ..
                            })
                        );
                        if dont_show {
                            self.prefs.hide_export_clip_tip = true;
                            self.prefs.save();
                        }
                        keep = None;
                        if proceed {
                            io::spawn_export(self.dialog_tx.clone(), kind);
                        }
                    }
                }
                ExportConfirm::Merge {
                    path,
                    kind,
                    dont_show,
                } => {
                    let kind = *kind;
                    let path = path.clone();
                    let mut decided: Option<bool> = None;
                    egui::Window::new("Export merge")
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .show(ctx, |ui| {
                            ui.set_max_width(460.0);
                            ui.label(format!(
                                "Export writes into a subfolder named {fnis_mod} and soft-merges with anything already there.\n\n\
                                 Matching files are overwritten. Other files (such as .hkx animation clips) are kept.\n\n\
                                 Continue?"
                            ));
                            ui.add_space(6.0);
                            ui.checkbox(dont_show, "Don't warn about export overwrites again");
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                if ui.button("Continue").clicked() {
                                    decided = Some(true);
                                }
                                if ui.button("Cancel").clicked() {
                                    decided = Some(false);
                                }
                            });
                        });
                    if let Some(proceed) = decided {
                        let dont_show = matches!(
                            keep.as_ref(),
                            Some(ExportConfirm::Merge {
                                dont_show: true,
                                ..
                            })
                        );
                        if dont_show {
                            self.prefs.hide_export_merge_warn = true;
                            self.prefs.save();
                        }
                        keep = None;
                        if proceed {
                            self.start_export(path, kind);
                        }
                    }
                }
            }
            self.export_confirm = keep;
        }

        if let Some(scene_id) = self.confirm_delete_scene.clone() {
            let name = self
                .ws
                .package
                .get_scene(&scene_id)
                .map(|s| {
                    if s.name.is_empty() {
                        s.id.0.clone()
                    } else {
                        s.name.clone()
                    }
                })
                .unwrap_or_default();
            egui::Window::new("Delete scene")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(format!("Delete \"{name}\"? This cannot be undone."));
                    ui.horizontal(|ui| {
                        if ui
                            .button(RichText::new("Delete").color(egui::Color32::RED))
                            .clicked()
                        {
                            self.ws.package.discard_scene(&scene_id);
                            if self.ws.selected_scene.as_ref() == Some(&scene_id) {
                                self.ws.selected_scene = None;
                                self.graph.selected = None;
                            }
                            self.mark_dirty();
                            self.confirm_delete_scene = None;
                        }
                        if ui.button("Cancel").clicked() {
                            self.confirm_delete_scene = None;
                        }
                    });
                });
        }

        if self.confirm_clear_canvas {
            egui::Window::new("Clear canvas")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("Remove all stages from this scene? This can be undone.");
                    ui.horizontal(|ui| {
                        if ui.button("Clear").clicked() {
                            self.confirm_clear_canvas = false;
                            if let Some(id) = self.ws.selected_scene.clone() {
                                if let Some(scene) = self.ws.package.get_scene_mut(&id) {
                                    self.graph.push_undo(scene);
                                    scene.stages.clear();
                                    scene.graph.clear();
                                    scene.root = NanoID::new_nanoid();
                                    self.graph.selected = None;
                                    self.mark_dirty();
                                }
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            self.confirm_clear_canvas = false;
                        }
                    });
                });
        }

        if let Some(stage_id) = self.clone_to.clone() {
            let mut close = false;
            let mut target: Option<NanoID> = None;
            let src_n = self
                .ws
                .selected_scene
                .as_ref()
                .and_then(|sid| self.ws.package.get_scene(sid))
                .and_then(|s| s.get_stage(&stage_id))
                .map(|s| s.positions.len())
                .unwrap_or(0);
            egui::Window::new("Clone stage to…")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.clone_to_search)
                            .hint_text("Search scenes"),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!(
                            "This stage has {src_n} position(s). The target scene will use that count."
                        ))
                        .small()
                        .weak(),
                    );
                    ui.add_space(4.0);
                    let needle = self.clone_to_search.to_lowercase();
                    let mut rows: Vec<(NanoID, String, usize)> = self
                        .ws
                        .package
                        .scenes
                        .iter()
                        .filter(|(id, _)| Some(*id) != self.ws.selected_scene.as_ref())
                        .map(|(id, scene)| {
                            let name = if scene.name.is_empty() {
                                id.0.clone()
                            } else {
                                scene.name.clone()
                            };
                            (id.clone(), name, scene.positions.len())
                        })
                        .filter(|(_, name, _)| {
                            needle.is_empty() || name.to_lowercase().contains(&needle)
                        })
                        .collect();
                    rows.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
                    egui::ScrollArea::vertical()
                        .max_height(260.0)
                        .show(ui, |ui| {
                            for (id, name, n_pos) in &rows {
                                let label = format!("{name}  ·  {n_pos} pos");
                                if ui.selectable_label(false, label).clicked() {
                                    target = Some(id.clone());
                                }
                            }
                        });
                    ui.add_space(4.0);
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            if let Some(to_scene) = target {
                if let Some(from_scene) = self.ws.selected_scene.clone() {
                    self.clone_stage_to_scene(ctx, &stage_id, &from_scene, &to_scene);
                }
                close = true;
            }
            if close {
                self.clone_to = None;
            }
        }

        if self.show_about {
            egui::Window::new("About SexLab Scene Builder")
                .collapsible(false)
                .resizable(false)
                .open(&mut self.show_about)
                .show(ctx, |ui| {
                    ui.label(format!(
                        "SexLab Scene Builder {}",
                        env!("CARGO_PKG_VERSION")
                    ));
                    ui.label("Apache-2.0 — Scrab and contributors");
                    if ui.link(REPO_URL).clicked() {
                        let _ = open::that(REPO_URL);
                    }
                    ui.separator();
                    ui.label("Third-party: serde-hkx (MIT OR Apache-2.0) for Behavior.hkx packing");
                });
        }

        if self.job.active {
            egui::Window::new(&self.job.title)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(&self.job.message);
                    let progress = egui::ProgressBar::new(self.job.fraction)
                        .show_percentage()
                        .animate(true);
                    ui.add(progress);
                });
        }
    }

    fn handle_stage_editor(&mut self, ctx: &Context) {
        let Some(mut editor) = self.stage_editor.take() else {
            return;
        };
        let action = show_stage_editor(ctx, &mut editor, &mut self.prefs.custom_tags);
        if editor.custom_tags_changed {
            editor.custom_tags_changed = false;
            self.prefs.save();
        }
        match action {
            StageEditorAction::None => {
                if editor.open {
                    self.stage_editor = Some(editor);
                }
            }
            StageEditorAction::Cancel => {}
            StageEditorAction::Save => {
                let scene_id = editor.scene_id.clone();
                let stage = editor.draft.clone();
                let infos = editor.positions_info.clone();
                if let Some(scene) = self.ws.package.get_scene_mut(&scene_id) {
                    if let Some(existing) = scene.get_stage_mut(&stage.id) {
                        *existing = stage;
                    } else {
                        let idx = scene.stages.len();
                        graph::ensure_graph_node(scene, &stage.id, idx);
                        scene.stages.push(stage);
                    }
                    scene.positions = infos;
                    self.mark_dirty();
                    self.status = "Stage saved".into();
                }
            }
        }
    }
}

impl App for SceneBuilderApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.poll_channels(ctx);

        if ctx.input(|i| i.viewport().close_requested()) {
            if self.stage_editor.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            } else if self.ws.dirty {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.pending_after_confirm = Some(PendingAction::Quit);
                self.show_close_confirm = true;
            }
        }

        ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.window_title()));

        if self.stage_editor.is_none() {
            use egui::{Key, KeyboardShortcut, Modifiers};
            const SAVE_AS: KeyboardShortcut =
                KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::S);
            const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
            const NEW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
            const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
            const EXPORT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::B);
            if ctx.input_mut(|i| i.consume_shortcut(&SAVE_AS)) {
                self.save_project(true);
            } else if ctx.input_mut(|i| i.consume_shortcut(&SAVE)) {
                self.save_project(false);
            }
            if ctx.input_mut(|i| i.consume_shortcut(&NEW)) {
                self.request_if_clean(PendingAction::New);
            }
            if ctx.input_mut(|i| i.consume_shortcut(&OPEN)) {
                self.request_if_clean(PendingAction::Open);
            }
            if ctx.input_mut(|i| i.consume_shortcut(&EXPORT)) {
                self.request_export(ExportKind::Both);
            }
        }

        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            self.menu_bar(ui, ctx);
        });

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(self.ws.document_status());
                });
            });
        });

        let left_w = self.prefs.left_panel_width;
        let dark = self.prefs.theme.is_dark();
        let panel_stroke = egui::Stroke::new(1.0, crate::theme::border(dark));
        egui::SidePanel::left("left")
            .resizable(true)
            .default_width(left_w)
            .width_range(layout::LEFT_PANEL_MIN..=layout::LEFT_PANEL_MAX)
            .frame(
                egui::Frame::side_top_panel(&ctx.style())
                    .fill(crate::theme::panel_bg(dark))
                    .stroke(panel_stroke)
                    .inner_margin(egui::Margin::same(layout::PANEL_MARGIN)),
            )
            .show(ctx, |ui| {
                layout::constrain_panel_contents(ui);
                self.left_panel(ui);
                layout::claim_allocated_width(ui);
                let new_w = ui
                    .max_rect()
                    .width()
                    .clamp(layout::LEFT_PANEL_MIN, layout::LEFT_PANEL_MAX);
                if (new_w - self.prefs.left_panel_width).abs() > 1.0 {
                    self.prefs.left_panel_width = new_w;
                    self.prefs.save();
                }
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::central_panel(&ctx.style())
                    .fill(crate::theme::shell_bg(dark))
                    .inner_margin(egui::Margin::same(8)),
            )
            .show(ctx, |ui| {
                // tags/furniture sit right of the graph in the top half.
                if let Some(scene_id) = self.ws.selected_scene.clone() {
                    let measured = ui
                        .ctx()
                        .data(|d| d.get_temp::<f32>(egui::Id::new("scene_positions_needed_h")));
                    let (min_h, default_h, max_h) = crate::positions::panel_height_range(
                        ui.available_height(),
                        self.prefs.bottom_panel_height,
                        measured.unwrap_or(layout::POSITIONS_PANEL_FALLBACK_H),
                    );
                    egui::TopBottomPanel::bottom("scene_positions_panel")
                        .resizable(true)
                        .default_height(default_h)
                        .height_range(min_h..=max_h)
                        .frame(
                            egui::Frame::side_top_panel(&ctx.style())
                                .fill(crate::theme::panel_bg(dark))
                                .stroke(panel_stroke)
                                .inner_margin(egui::Margin::same(layout::PANEL_MARGIN)),
                        )
                        .show_inside(ui, |ui| {
                            let (changed, inner_h) =
                                if let Some(scene) = self.ws.package.get_scene_mut(&scene_id) {
                                    crate::positions::show(ui, scene, &self.race_keys)
                                } else {
                                    (false, layout::POSITIONS_HEADER_H)
                                };
                            if changed {
                                self.mark_dirty();
                            }
                            let needed = inner_h + layout::POSITIONS_PANEL_CHROME;
                            ui.ctx().data_mut(|d| {
                                d.insert_temp(egui::Id::new("scene_positions_needed_h"), needed);
                            });
                            let new_h = ui.max_rect().height();
                            if new_h >= min_h
                                && (new_h - self.prefs.bottom_panel_height).abs() > 1.0
                            {
                                self.prefs.bottom_panel_height = new_h.max(min_h);
                                self.prefs.save();
                            }
                        });

                    let panel_id = egui::Id::new("tags_furniture_panel");
                    let resize_id = panel_id.with("__resize");
                    let avail = ui.available_rect_before_wrap();
                    let max_w =
                        layout::RIGHT_PANEL_MAX.min(avail.width().max(layout::RIGHT_PANEL_MIN));
                    let dragging = ui
                        .ctx()
                        .read_response(resize_id)
                        .is_some_and(|r| r.dragged());
                    let mut width = self
                        .prefs
                        .right_panel_width
                        .clamp(layout::RIGHT_PANEL_MIN, max_w);
                    if dragging {
                        if let Some(pointer) = ui
                            .ctx()
                            .read_response(resize_id)
                            .and_then(|r| r.interact_pointer_pos())
                        {
                            width = (avail.max.x - pointer.x)
                                .abs()
                                .clamp(layout::RIGHT_PANEL_MIN, max_w);
                        }
                    }

                    let mut right = egui::SidePanel::right(panel_id)
                        .resizable(true)
                        .default_width(width)
                        .frame(
                            egui::Frame::side_top_panel(&ctx.style())
                                .fill(crate::theme::panel_bg(dark))
                                .stroke(panel_stroke)
                                .inner_margin(egui::Margin::same(layout::PANEL_MARGIN)),
                        );
                    right = if dragging {
                        right.width_range(layout::RIGHT_PANEL_MIN..=layout::RIGHT_PANEL_MAX)
                    } else {
                        right.exact_width(width)
                    };
                    right.show_inside(ui, |ui| {
                        layout::constrain_panel_contents(ui);
                        crate::theme::fill_width(ui);
                        let allocated = ui.max_rect();
                        self.tags_furniture_panel(ui);
                        ui.expand_to_include_rect(allocated);
                    });

                    let mut panel_rect = avail;
                    panel_rect.min.x = panel_rect.max.x - width;
                    layout::persist_side_panel_rect(ui.ctx(), panel_id, panel_rect);
                    if (width - self.prefs.right_panel_width).abs() > 1.0 {
                        self.prefs.right_panel_width = width;
                        self.prefs.save();
                    }
                }

                egui::CentralPanel::default()
                    .frame(egui::Frame::new().inner_margin(egui::Margin::same(4)))
                    .show_inside(ui, |ui| {
                        self.center_panel(ui);
                    });
            });

        self.handle_stage_editor(ctx);
        self.modals(ctx);
        self.toasts.ui(ctx);

        if self.prefs.capture_viewport(ctx) {
            self.prefs.save();
        }
    }
}

fn truncated_selectable(ui: &mut egui::Ui, selected: bool, text: &str) -> egui::Response {
    let w = ui.available_width().max(0.0);
    let h = ui.spacing().interact_size.y;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
    let visuals = ui.style().interact_selectable(&resp, selected);
    if selected || resp.hovered() || resp.has_focus() {
        ui.painter()
            .rect_filled(rect, visuals.corner_radius, visuals.weak_bg_fill);
    }
    let pad = ui.spacing().button_padding.x;
    let galley = egui::WidgetText::from(text).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        (rect.width() - pad * 2.0).max(0.0),
        egui::TextStyle::Button,
    );
    let pos = egui::pos2(rect.left() + pad, rect.center().y - galley.size().y * 0.5);
    ui.painter().galley(pos, galley, visuals.text_color());
    resp
}

/// Name | toolbar | Add Stage+Store. Packed from the right of `full` so the
/// action buttons stay inside the center column instead of painting over the
/// tags sidebar when width is tight.
fn scene_header_strips(full: egui::Rect, row_h: f32) -> (egui::Rect, egui::Rect, egui::Rect) {
    const ACTIONS_W: f32 = 196.0;
    const TOOLBAR_W: f32 = 340.0;
    let w = full.width().max(0.0);
    let actions_w = ACTIONS_W.min(w);
    let toolbar_w = TOOLBAR_W.min((w - actions_w).max(0.0));
    let y0 = full.min.y;
    let y1 = y0 + row_h;
    let actions = egui::Rect::from_min_max(
        egui::pos2(full.max.x - actions_w, y0),
        egui::pos2(full.max.x, y1),
    );
    let toolbar = egui::Rect::from_min_max(
        egui::pos2(actions.min.x - toolbar_w, y0),
        egui::pos2(actions.min.x, y1),
    );
    let name = egui::Rect::from_min_max(egui::pos2(full.min.x, y0), egui::pos2(toolbar.min.x, y1));
    (name, toolbar, actions)
}
