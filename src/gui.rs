//! application shell

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{
    self, Align2, Color32, ComboBox, CornerRadius, FontData, FontDefinitions, FontFamily, FontId,
    Frame, Id, Margin, RichText, Stroke, TextStyle, TopBottomPanel, Visuals, Window,
};

use crate::avr::assembler::assemble;
use crate::avr::cpu::StepResult;
use crate::avr::Cpu;
use crate::editor::TextEditor;
use crate::docs::{show_flash_locations_window, show_isa_window};
use crate::cycle_helper::{show_cycle_helper, CycleHelperState};
use crate::sim_panel::{
    show_sim_panel, BreakpointState, BpAction, FlashState, SimAction, SimTab,
    SpeedLimitState, StackState, XmemState,
};
use crate::toolbar::{show_toolbar, ToolbarAction};
use crate::word_helper::{show_word_helper, WordHelperState};
use crate::welcome::{
    show_create_project, show_welcome, CreateProjectAction, WelcomeAction, START_GREEN_DIM,
};

pub fn setup_style(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "iosevka_term".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../include/IosevkaTerm-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "orbitron_title".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../include/Orbitron-Variable.ttf"
        ))),
    );
    if let Some(stack) = fonts.families.get_mut(&FontFamily::Monospace) {
        stack.insert(0, "iosevka_term".to_owned());
    }
    fonts.families.insert(
        FontFamily::Name("lain_title".into()),
        vec!["orbitron_title".to_owned()],
    );
    ctx.set_fonts(fonts);

    let mut visuals = Visuals::dark();
    visuals.override_text_color = Some(Color32::WHITE);
    visuals.extreme_bg_color = Color32::BLACK;
    visuals.faint_bg_color = Color32::BLACK;
    visuals.panel_fill = Color32::BLACK;
    visuals.window_fill = Color32::BLACK;
    visuals.code_bg_color = Color32::BLACK;

    let black_widget = |w: &mut egui::style::WidgetVisuals| {
        w.bg_fill = Color32::BLACK;
        w.bg_stroke = Stroke::NONE;
    };
    black_widget(&mut visuals.widgets.noninteractive);

    visuals.widgets.inactive.bg_fill = Color32::TRANSPARENT;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_black_alpha(0));
    visuals.widgets.inactive.corner_radius = CornerRadius::ZERO;

    visuals.widgets.hovered.bg_fill = Color32::from_rgba_premultiplied(160, 240, 160, 20);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(100, 180, 100));
    visuals.widgets.hovered.corner_radius = CornerRadius::ZERO;

    visuals.widgets.active.bg_fill = Color32::from_rgba_premultiplied(160, 240, 160, 44);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, Color32::from_rgb(160, 240, 160));
    visuals.widgets.active.corner_radius = CornerRadius::ZERO;

    visuals.widgets.open.bg_fill = Color32::from_rgba_premultiplied(160, 240, 160, 32);
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, Color32::from_rgb(160, 240, 160));
    visuals.widgets.open.corner_radius = CornerRadius::ZERO;

    visuals.text_cursor.stroke = Stroke::new(2.0, Color32::WHITE);
    visuals.selection.bg_fill = Color32::from_rgb(55, 55, 55);
    visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    ctx.set_visuals(visuals);

    ctx.style_mut(|style| {
        style
            .text_styles
            .insert(TextStyle::Monospace, FontId::new(14.0, FontFamily::Monospace));
    });

    // egui_embedded_png_bytes
    egui_extras::install_image_loaders(ctx);
}

pub struct Workspace {
    pub root: PathBuf,
    pub active_file: Option<PathBuf>,
}

enum AppPhase {
    Welcome,
    CreateProject {
        parent_dir: Option<PathBuf>,
        name: String,
        err: Option<String>,
    },
    Editor {
        workspace: Workspace,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileExtension {
    Lain,
    H,
    Md,
    Txt,
}

impl FileExtension {
    fn as_str(self) -> &'static str {
        match self {
            Self::Lain => "lain",
            Self::H => "h",
            Self::Md => "md",
            Self::Txt => "txt",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Lain => ".lain",
            Self::H => ".h",
            Self::Md => ".md",
            Self::Txt => ".txt",
        }
    }
}

enum ModalState {
    None,
    NewDir {
        name: String,
        err: Option<String>,
    },
    NewFile {
        name: String,
        extension: FileExtension,
        err: Option<String>,
    },
    ConfirmOpenFolder {
        err: Option<String>,
    },
}

struct StatusMessage {
    text: String,
    is_error: bool,
}

pub struct LainApp {
    phase: AppPhase,
    editor: TextEditor,
    modal: ModalState,
    status: Option<StatusMessage>,
    sim: Cpu,
    show_sim: bool,
    sim_tab: SimTab,
    sim_last_result: Option<StepResult>,
    show_isa: bool,
    flash_state: FlashState,
    show_flash_locations: bool,
    show_word_helper: bool,
    word_helper_state: WordHelperState,
    show_cycle_helper: bool,
    cycle_helper_state: CycleHelperState,
    stack_state: StackState,
    xmem_state:  XmemState,
    speed_limit: SpeedLimitState,
    breakpoints: BreakpointState,
    // auto_run_state
    auto_running: bool,
    ips: f64,
    ips_sample_start: std::time::Instant,
    ips_sample_steps: u64,
    // token-bucket for the IPS speed limiter (wall-clock based, not frame-dt)
    limit_clock:      std::time::Instant,
    limit_steps_done: u64,
}

impl LainApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_style(&cc.egui_ctx);
        Self {
            phase: AppPhase::Welcome,
            editor: TextEditor::new(Id::new("main_editor")),
            modal: ModalState::None,
            status: None,
            sim: Cpu::new(),
            show_sim: false,
            sim_tab: SimTab::Cpu,
            sim_last_result: None,
            show_isa: false,
            flash_state: FlashState::default(),
            show_flash_locations: false,
            show_word_helper: false,
            word_helper_state: WordHelperState::default(),
            show_cycle_helper: false,
            cycle_helper_state: CycleHelperState::default(),
            stack_state: StackState::default(),
            xmem_state:  XmemState::default(),
            speed_limit: SpeedLimitState::default(),
            breakpoints: BreakpointState::default(),
            auto_running: false,
            ips: 0.0,
            ips_sample_start: std::time::Instant::now(),
            ips_sample_steps: 0,
            limit_clock:      std::time::Instant::now(),
            limit_steps_done: 0,
        }
    }

    /// Return (filename, content) pairs for all .lain files in the workspace root.
    fn collect_lain_files(&self) -> Vec<(String, String)> {
        let Some(ws) = self.current_workspace() else { return vec![]; };
        let Ok(entries) = std::fs::read_dir(&ws.root) else { return vec![]; };
        let mut out = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("lain") {
                let name = path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("?")
                    .to_string();
                if let Ok(content) = std::fs::read_to_string(&path) {
                    out.push((name, content));
                }
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    fn current_workspace(&self) -> Option<&Workspace> {
        match &self.phase {
            AppPhase::Editor { workspace } => Some(workspace),
            _ => None,
        }
    }

    fn current_workspace_mut(&mut self) -> Option<&mut Workspace> {
        match &mut self.phase {
            AppPhase::Editor { workspace } => Some(workspace),
            _ => None,
        }
    }

    fn set_status(&mut self, text: impl Into<String>) {
        self.status = Some(StatusMessage {
            text: text.into(),
            is_error: false,
        });
    }

    fn set_error(&mut self, text: impl Into<String>) {
        self.status = Some(StatusMessage {
            text: text.into(),
            is_error: true,
        });
    }

    fn enter_editor(&mut self, workspace: Workspace) {
        let load_result = workspace
            .active_file
            .as_ref()
            .map(|path| read_text_file(path))
            .transpose();

        self.editor.reset_for_session();
        match load_result {
            Ok(Some(contents)) => self.editor.set_source(contents),
            Ok(None) => self.editor.set_source(String::new()),
            Err(err) => {
                self.editor
                    .set_source(format!("// Could not read file: {err}\n"));
                self.set_error(err);
            }
        }
        self.phase = AppPhase::Editor { workspace };
        self.modal = ModalState::None;
    }

    fn open_workspace(&mut self, root: PathBuf) {
        let active_file = find_first_supported_file(&root);
        self.enter_editor(Workspace { root, active_file });
        self.set_status("Opened folder.");
    }

    fn save_current_file(&mut self) -> Result<String, String> {
        let path = self
            .current_workspace()
            .and_then(|workspace| workspace.active_file.clone())
            .ok_or_else(|| "No active file selected. Use File > New file first.".to_string())?;

        fs::write(&path, self.editor.source()).map_err(|err| err.to_string())?;
        self.editor.mark_saved();
        self.editor.focus_next_frame();
        Ok(format!("Saved {}", path.display()))
    }

    fn save_all_files(&mut self) -> Result<String, String> {
        // single_buffer_save_all_flush
        self.save_current_file()?;
        Ok("Saved all tracked files.".to_string())
    }

    fn request_open_folder(&mut self) {
        if self.editor.is_dirty() {
            self.modal = ModalState::ConfirmOpenFolder { err: None };
        } else {
            self.perform_open_folder_picker();
        }
    }

    fn perform_open_folder_picker(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Open folder")
            .pick_folder()
        {
            self.open_workspace(path);
        }
    }

    fn create_new_dir(&mut self, name: &str) -> Result<String, String> {
        let name = validate_leaf_name(name)?;
        let root = self
            .current_workspace()
            .map(|workspace| workspace.root.clone())
            .ok_or_else(|| "No workspace is open.".to_string())?;

        let dir_path = root.join(name);
        if dir_path.exists() {
            return Err(format!("Already exists: {}", dir_path.display()));
        }

        fs::create_dir_all(&dir_path).map_err(|err| err.to_string())?;
        self.editor.focus_next_frame();
        Ok(format!("Created {}", dir_path.display()))
    }

    fn create_new_file(
        &mut self,
        name: &str,
        extension: FileExtension,
    ) -> Result<String, String> {
        let name = validate_leaf_name(name)?;

        if self.editor.is_dirty() && self.current_workspace().and_then(|w| w.active_file.as_ref()).is_some() {
            self.save_current_file()?;
        }

        let root = self
            .current_workspace()
            .map(|workspace| workspace.root.clone())
            .ok_or_else(|| "No workspace is open.".to_string())?;

        let path = root.join(format!("{name}.{}", extension.as_str()));
        if path.exists() {
            return Err(format!("Already exists: {}", path.display()));
        }

        fs::write(&path, "").map_err(|err| err.to_string())?;
        if let Some(workspace) = self.current_workspace_mut() {
            workspace.active_file = Some(path.clone());
        }
        self.editor.set_source(String::new());
        self.editor.focus_next_frame();
        Ok(format!("Created {}", path.display()))
    }

    fn add_file_to_project(&mut self) -> Result<String, String> {
        let root = self
            .current_workspace()
            .map(|workspace| workspace.root.clone())
            .ok_or_else(|| "No workspace is open.".to_string())?;

        let Some(source_path) = rfd::FileDialog::new()
            .set_title("Add file to project")
            .pick_file()
        else {
            return Ok("Add file cancelled.".to_string());
        };

        let file_name = source_path
            .file_name()
            .ok_or_else(|| "Selected file has no name.".to_string())?;
        let dest_path = root.join(file_name);
        if dest_path.exists() {
            return Err(format!("Already exists: {}", dest_path.display()));
        }

        fs::copy(&source_path, &dest_path).map_err(|err| err.to_string())?;

        if is_supported_text_file(&dest_path) {
            let contents = read_text_file(&dest_path)?;
            if let Some(workspace) = self.current_workspace_mut() {
                workspace.active_file = Some(dest_path.clone());
            }
            self.editor.set_source(contents);
            self.editor.focus_next_frame();
        }

        Ok(format!("Added {}", dest_path.display()))
    }

    fn handle_toolbar_action(&mut self, action: ToolbarAction) {
        match action {
            ToolbarAction::None => {}
            ToolbarAction::Save => match self.save_current_file() {
                Ok(msg) => self.set_status(msg),
                Err(err) => self.set_error(err),
            },
            ToolbarAction::SaveAll => match self.save_all_files() {
                Ok(msg) => self.set_status(msg),
                Err(err) => self.set_error(err),
            },
            ToolbarAction::NewFile => {
                self.modal = ModalState::NewFile {
                    name: String::new(),
                    extension: FileExtension::Lain,
                    err: None,
                };
            }
            ToolbarAction::NewDir => {
                self.modal = ModalState::NewDir {
                    name: String::new(),
                    err: None,
                };
            }
            ToolbarAction::OpenFolder => {
                self.request_open_folder();
            }
            ToolbarAction::AddFileToProject => match self.add_file_to_project() {
                Ok(msg) => self.set_status(msg),
                Err(err) => self.set_error(err),
            },
            ToolbarAction::SimTogglePanel => {
                self.show_sim = !self.show_sim;
                if self.show_sim {
                    self.show_word_helper  = false;
                    self.show_cycle_helper = false;
                }
            }
            ToolbarAction::DocsInstructionSet => {
                self.show_isa = true;
            }
            ToolbarAction::DocsFlashLocations => {
                self.show_flash_locations = true;
            }
            ToolbarAction::HelpersWordHelper => {
                self.show_word_helper = !self.show_word_helper;
                if self.show_word_helper {
                    self.show_sim          = false;
                    self.show_cycle_helper = false;
                }
            }
            ToolbarAction::HelpersCycleHelper => {
                self.show_cycle_helper = !self.show_cycle_helper;
                if self.show_cycle_helper {
                    self.show_sim         = false;
                    self.show_word_helper = false;
                }
            }
        }
    }

    fn show_modal(&mut self, ctx: &egui::Context) {
        enum ModalAction {
            None,
            Close,
            CreateDir(String),
            CreateFile(String, FileExtension),
            SaveThenOpenFolder,
            DiscardThenOpenFolder,
        }

        let mut action = ModalAction::None;

        match &mut self.modal {
            ModalState::None => {}
            ModalState::NewDir { name, err } => {
                Window::new("New dir")
                    .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                    .collapsible(false)
                    .resizable(false)
                    .frame(
                        Frame::NONE
                            .fill(Color32::from_rgb(3, 8, 3))
                            .stroke(Stroke::new(1.5, START_GREEN_DIM))
                            .corner_radius(CornerRadius::ZERO)
                            .inner_margin(Margin::same(14)),
                    )
                    .show(ctx, |ui| {
                        ui.label("Create a new directory under the current project.");
                        ui.add_space(8.0);
                        ui.label("Directory name");
                        ui.text_edit_singleline(name);

                        if let Some(message) = err.as_ref() {
                            ui.add_space(8.0);
                            ui.colored_label(Color32::from_rgb(255, 140, 140), message);
                        }

                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            if ui.button("Cancel").clicked() {
                                action = ModalAction::Close;
                            }
                            if ui.button("Create").clicked() {
                                action = ModalAction::CreateDir(name.clone());
                            }
                        });
                    });
            }
            ModalState::NewFile {
                name,
                extension,
                err,
            } => {
                Window::new("New file")
                    .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                    .collapsible(false)
                    .resizable(false)
                    .frame(
                        Frame::NONE
                            .fill(Color32::from_rgb(3, 8, 3))
                            .stroke(Stroke::new(1.5, START_GREEN_DIM))
                            .corner_radius(CornerRadius::ZERO)
                            .inner_margin(Margin::same(14)),
                    )
                    .show(ctx, |ui| {
                        ui.label("Create a new file under the current project.");
                        ui.add_space(8.0);
                        ui.label("File name");
                        ui.text_edit_singleline(name);
                        ui.add_space(8.0);

                        ComboBox::from_id_salt("new_file_extension")
                            .selected_text(extension.label())
                            .show_ui(ui, |ui| {
                                for candidate in [
                                    FileExtension::Lain,
                                    FileExtension::H,
                                    FileExtension::Md,
                                    FileExtension::Txt,
                                ] {
                                    ui.selectable_value(extension, candidate, candidate.label());
                                }
                            });

                        if let Some(message) = err.as_ref() {
                            ui.add_space(8.0);
                            ui.colored_label(Color32::from_rgb(255, 140, 140), message);
                        }

                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            if ui.button("Cancel").clicked() {
                                action = ModalAction::Close;
                            }
                            if ui.button("Create").clicked() {
                                action = ModalAction::CreateFile(name.clone(), *extension);
                            }
                        });
                    });
            }
            ModalState::ConfirmOpenFolder { err } => {
                Window::new("Unsaved changes")
                    .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                    .collapsible(false)
                    .resizable(false)
                    .frame(
                        Frame::NONE
                            .fill(Color32::from_rgb(3, 8, 3))
                            .stroke(Stroke::new(1.5, START_GREEN_DIM))
                            .corner_radius(CornerRadius::ZERO)
                            .inner_margin(Margin::same(14)),
                    )
                    .show(ctx, |ui| {
                        ui.label("Save the current file before opening another folder?");
                        if let Some(message) = err.as_ref() {
                            ui.add_space(8.0);
                            ui.colored_label(Color32::from_rgb(255, 140, 140), message);
                        }
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            if ui.button("Save").clicked() {
                                action = ModalAction::SaveThenOpenFolder;
                            }
                            if ui.button("Don't Save").clicked() {
                                action = ModalAction::DiscardThenOpenFolder;
                            }
                            if ui.button("Cancel").clicked() {
                                action = ModalAction::Close;
                            }
                        });
                    });
            }
        }

        match action {
            ModalAction::None => {}
            ModalAction::Close => {
                self.modal = ModalState::None;
                self.editor.focus_next_frame();
            }
            ModalAction::CreateDir(new_name) => match self.create_new_dir(&new_name) {
                Ok(msg) => {
                    self.modal = ModalState::None;
                    self.set_status(msg);
                }
                Err(message) => {
                    if let ModalState::NewDir { err, .. } = &mut self.modal {
                        *err = Some(message);
                    }
                }
            },
            ModalAction::CreateFile(file_name, extension) => match self.create_new_file(&file_name, extension) {
                Ok(msg) => {
                    self.modal = ModalState::None;
                    self.set_status(msg);
                }
                Err(message) => {
                    if let ModalState::NewFile { err, .. } = &mut self.modal {
                        *err = Some(message);
                    }
                }
            },
            ModalAction::SaveThenOpenFolder => match self.save_current_file() {
                Ok(msg) => {
                    self.set_status(msg);
                    self.modal = ModalState::None;
                    self.perform_open_folder_picker();
                }
                Err(message) => {
                    if let ModalState::ConfirmOpenFolder { err } = &mut self.modal {
                        *err = Some(message);
                    }
                }
            },
            ModalAction::DiscardThenOpenFolder => {
                self.modal = ModalState::None;
                self.perform_open_folder_picker();
            }
        }
    }
}

impl eframe::App for LainApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut toolbar_action = ToolbarAction::None;

        if let AppPhase::Editor { workspace } = &self.phase {
            TopBottomPanel::top("retro_toolbar")
                .exact_height(44.0)
                .show(ctx, |ui| {
                    toolbar_action = show_toolbar(
                        ui,
                        workspace.active_file.as_deref(),
                        &workspace.root,
                        self.editor.is_dirty(),
                        self.show_sim,
                        self.show_word_helper || self.show_cycle_helper,
                    );
                });
        }

        // rhs_panel_editor_only: sim or helpers, mutually exclusive
        let mut sim_action = SimAction::None;
        let rhs_open = (self.show_sim || self.show_word_helper || self.show_cycle_helper)
            && matches!(self.phase, AppPhase::Editor { .. });

        if rhs_open {
            egui::SidePanel::right("rhs_panel")
                .exact_width(340.0)
                .resizable(false)
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    if self.show_sim {
                        sim_action = show_sim_panel(
                            ui,
                            &self.sim,
                            self.sim_last_result,
                            &mut self.sim_tab,
                            self.auto_running,
                            self.ips,
                            &mut self.flash_state,
                            &mut self.speed_limit,
                            &mut self.breakpoints,
                            &mut self.stack_state,
                            &mut self.xmem_state,
                        );
                    } else if self.show_word_helper {
                        let files = self.collect_lain_files();
                        show_word_helper(ui, &mut self.word_helper_state, &files);
                    } else if self.show_cycle_helper {
                        let files = self.collect_lain_files();
                        show_cycle_helper(ui, &mut self.cycle_helper_state, &files);
                    }
                });
        }

        egui::CentralPanel::default()
            .frame(
                Frame::NONE
                    .fill(Color32::BLACK)
                    .inner_margin(Margin::same(6)),
            )
            .show(ctx, |ui| {
                ui.set_min_size(ui.available_size());

                match &mut self.phase {
                    AppPhase::Welcome => match show_welcome(ui) {
                        WelcomeAction::OpenFolder => {
                            self.perform_open_folder_picker();
                        }
                        WelcomeAction::CreateNew => {
                            self.phase = AppPhase::CreateProject {
                                parent_dir: None,
                                name: String::new(),
                                err: None,
                            };
                        }
                        WelcomeAction::None => {}
                    },
                    AppPhase::CreateProject {
                        parent_dir,
                        name,
                        err,
                    } => match show_create_project(ui, parent_dir, name, err) {
                        CreateProjectAction::PickParentFolder => {
                            if let Some(path) = rfd::FileDialog::new()
                                .set_title("Choose parent folder")
                                .pick_folder()
                            {
                                *parent_dir = Some(path);
                                *err = None;
                            }
                        }
                        CreateProjectAction::Back => {
                            self.phase = AppPhase::Welcome;
                        }
                        CreateProjectAction::Submit => {
                            *err = None;
                            if let Some(parent) = parent_dir.clone() {
                                match try_create_lain_project(&parent, name) {
                                    Ok((root, lain_path)) => {
                                        self.enter_editor(Workspace {
                                            root,
                                            active_file: Some(lain_path),
                                        });
                                        self.set_status("Created project.");
                                    }
                                    Err(message) => *err = Some(message),
                                }
                            } else {
                                *err = Some("Choose a parent folder first.".to_string());
                            }
                        }
                        CreateProjectAction::None => {}
                    },
                    AppPhase::Editor { .. } => {
                        if matches!(self.modal, ModalState::None) {
                            self.editor.request_initial_focus(ctx);
                        }
                        ui.vertical(|ui| {
                            if let Some(status) = &self.status {
                                ui.label(
                                    RichText::new(&status.text)
                                        .monospace()
                                        .size(13.0)
                                        .color(if status.is_error {
                                            Color32::from_rgb(255, 140, 140)
                                        } else {
                                            START_GREEN_DIM
                                        }),
                                );
                                ui.add_space(6.0);
                            }
                            self.editor.show(ui);
                        });
                    }
                }
            });

        if toolbar_action != ToolbarAction::None {
            self.handle_toolbar_action(toolbar_action);
        }

        match sim_action {
            SimAction::None => {}
            SimAction::Assemble => {
                let source = self.editor.source().to_owned();
                match assemble(&source) {
                    Ok(words) => {
                        let n = words.len();
                        self.sim.reset();
                        self.sim.load_flash(&words);
                        self.sim_last_result = None;
                        self.set_status(format!(
                            "Assembled {n} word{} → Flash.",
                            if n == 1 { "" } else { "s" }
                        ));
                    }
                    Err(errors) => {
                        let msg = errors
                            .iter()
                            .map(|e| e.to_string())
                            .collect::<Vec<_>>()
                            .join("   ");
                        self.set_error(msg);
                    }
                }
            }
            SimAction::Step => {
                self.sim_last_result = Some(self.sim.step());
            }
            SimAction::Run10 => {
                let (_, r) = self.sim.step_n(10);
                self.sim_last_result = Some(r);
            }
            SimAction::Run100 => {
                let (_, r) = self.sim.step_n(100);
                self.sim_last_result = Some(r);
            }
            SimAction::Reset => {
                self.sim.reset();
                self.sim_last_result = None;
                self.auto_running = false;
                self.ips = 0.0;
                self.ips_sample_steps = 0;
                self.ips_sample_start = std::time::Instant::now();
            }
            SimAction::AutoToggle => {
                self.auto_running = !self.auto_running;
                if self.auto_running {
                    // reset_ips_window_on_run
                    self.ips_sample_start = std::time::Instant::now();
                    self.ips_sample_steps = 0;
                    // reset_token_bucket
                    self.limit_clock      = std::time::Instant::now();
                    self.limit_steps_done = 0;
                }
            }
            SimAction::SetIoBit { addr, mask } => {
                self.sim.set_io_bit(addr, mask);
            }
            SimAction::SetXmem(size) => {
                self.sim.configure_xmem(size);
            }
        }

        // auto_run loop with optional IPS cap and breakpoint support
        if self.auto_running {
            let bp_addrs  = self.breakpoints.active_addrs();
            let limit_ips = self.speed_limit.limit_ips();

            let (steps, result, bp_hit) = if let Some(limit) = limit_ips {
                // token-bucket: compare wall-clock elapsed against steps already done
                // this is independent of frame-rate so the limit is always accurate
                let elapsed = self.limit_clock.elapsed().as_secs_f64();
                let allowed = (limit * elapsed) as u64;
                let to_run  = allowed.saturating_sub(self.limit_steps_done);

                if to_run > 0 {
                    // cap each burst to ~20 ms worth to keep the UI responsive
                    let burst_cap = ((limit * 0.020) as u64).max(1);
                    let batch = to_run.min(burst_cap);
                    let r = self.sim.run_n_break(batch, &bp_addrs);
                    self.limit_steps_done += r.0;
                    r
                } else {
                    // ahead of budget — skip this frame, wake up soon
                    ctx.request_repaint_after(std::time::Duration::from_micros(500));
                    (0, StepResult::Ok, None)
                }
            } else {
                // unlimited: run for 12 ms
                self.sim.run_timed_break(12, &bp_addrs)
            };

            // reset the bucket every 10 s to prevent u64 overflow / drift
            if self.limit_clock.elapsed().as_secs_f64() > 10.0 {
                self.limit_clock      = std::time::Instant::now();
                self.limit_steps_done = 0;
            }

            if let Some(bp_addr) = bp_hit {
                if let Some(bp) = self.breakpoints.breakpoints.iter().find(|b| b.addr == bp_addr) {
                    let should_pause = matches!(bp.action, BpAction::Pause | BpAction::PrintAndPause);
                    if should_pause {
                        self.auto_running = false;
                        self.set_status(format!("Breakpoint hit @ 0x{bp_addr:04X}"));
                    }
                }
            }

            if result != StepResult::Ok {
                self.sim_last_result = Some(result);
                self.auto_running = false;
            }

            // ips_accum_250ms_refresh
            self.ips_sample_steps += steps;
            let elapsed_secs = self.ips_sample_start.elapsed().as_secs_f64();
            if elapsed_secs >= 0.25 {
                self.ips = self.ips_sample_steps as f64 / elapsed_secs;
                self.ips_sample_steps = 0;
                self.ips_sample_start = std::time::Instant::now();
            }

            ctx.request_repaint(); // auto_run_repaint
        }

        // doc_overlays_z_last
        show_isa_window(ctx, &mut self.show_isa);
        show_flash_locations_window(ctx, &mut self.show_flash_locations, &self.sim);

        self.show_modal(ctx);
    }
}

fn validate_leaf_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Enter a name.".to_string());
    }
    if name.contains('/') || name.contains('\\') {
        return Err("Name cannot contain path separators.".to_string());
    }
    if name == "." || name == ".." {
        return Err("Invalid name.".to_string());
    }
    #[cfg(windows)]
    if name
        .chars()
        .any(|c| ['<', '>', ':', '"', '|', '?', '*'].contains(&c))
    {
        return Err("Name contains invalid characters.".to_string());
    }
    Ok(name.to_string())
}

fn is_supported_text_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("lain" | "h" | "md" | "txt")
    )
}

fn read_text_file(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|err| format!("{}: {}", path.display(), err))
}

fn find_first_supported_file(root: &Path) -> Option<PathBuf> {
    let mut pending = vec![root.to_path_buf()];

    while let Some(dir) = pending.pop() {
        let entries = fs::read_dir(&dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if is_supported_text_file(&path) {
                return Some(path);
            }
        }
    }

    None
}

/// mkdir_project parent_name_lainfile
fn try_create_lain_project(parent: &Path, name: &str) -> Result<(PathBuf, PathBuf), String> {
    let name = validate_leaf_name(name)?;
    let root = parent.join(&name);
    if root.exists() {
        return Err(format!("Already exists: {}", root.display()));
    }
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let lain_path = root.join(format!("{name}.lain"));
    fs::write(&lain_path, "").map_err(|e| e.to_string())?;
    Ok((root, lain_path))
}
