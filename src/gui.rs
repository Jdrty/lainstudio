//! application shell

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::thread;

use eframe::egui::{
    self, Align2, Color32, ComboBox, CornerRadius, FontData, FontDefinitions, FontFamily, FontId,
    Frame, Id, Key, Margin, Modifiers, RichText, Stroke, TextStyle, TopBottomPanel, ViewportCommand,
    Visuals, Window,
};

use crate::avr::assembler::assemble_for_model;
use crate::avr::cpu::StepResult;
use crate::avr::intel_hex::{self, validate_intel_hex};
use crate::avr::parse_board_from_source;
use crate::avr::McuModel;
use crate::avr::Cpu;
use crate::editor::TextEditor;
use crate::docs::show_flash_locations_window;
use crate::cycle_helper::{show_cycle_helper, CycleHelperState};
use crate::peripherals::{
    apply_peripherals_to_cpu, load_peripherals_from_disk, on_peripherals_panel_hidden,
    show_peripherals_panel, PeripheralState,
};
use crate::sim_panel::{
    show_sim_panel, BreakpointState, BpAction, FlashState, SimAction, SimTab,
    SpeedLimitState, StackState, XmemState,
};
use crate::toolbar::{show_toolbar, ToolbarAction};
use crate::upload_panel::{scan_serial_ports, show_upload_panel, UploadAction};
use crate::word_helper::{show_word_helper, WordHelperState};
use crate::modal_chrome::{
    modal_body, modal_btn_danger, modal_btn_primary, modal_btn_secondary, modal_caption,
    modal_error, modal_single_line_edit, modal_title, modal_window_frame,
};
use crate::theme;
use crate::waveforms::{
    on_waveforms_panel_hidden, show_waveforms_panel, WaveformAction, WaveformState,
};
use crate::theme::{START_GREEN, START_GREEN_DIM};

/// Written by “Assemble and Link”; consumed by avrdude.
const FIRMWARE_HEX: &str = "firmware.hex";

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
        FontFamily::Name("fm_title".into()),
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

    visuals.widgets.hovered.bg_fill =
        Color32::from_rgba_unmultiplied(START_GREEN.r(), START_GREEN.g(), START_GREEN.b(), 20);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, START_GREEN_DIM);
    visuals.widgets.hovered.corner_radius = CornerRadius::ZERO;

    visuals.widgets.active.bg_fill =
        Color32::from_rgba_unmultiplied(START_GREEN.r(), START_GREEN.g(), START_GREEN.b(), 44);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, START_GREEN);
    visuals.widgets.active.corner_radius = CornerRadius::ZERO;

    visuals.widgets.open.bg_fill =
        Color32::from_rgba_unmultiplied(START_GREEN.r(), START_GREEN.g(), START_GREEN.b(), 32);
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, START_GREEN);
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

    // Cmd+/− zoom applies only to the editor (see `TextEditor::apply_editor_zoom_keyboard`).
    ctx.options_mut(|o| o.zoom_with_keyboard = false);

    // egui_embedded_png_bytes
    egui_extras::install_image_loaders(ctx);
}

pub struct Workspace {
    pub root: PathBuf,
    pub active_file: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileExtension {
    Fm,
    Asm,
    Gas, // .S
    H,
    Md,
    Txt,
}

impl FileExtension {
    fn as_str(self) -> &'static str {
        match self {
            Self::Fm => "fm",
            Self::Asm => "asm",
            Self::Gas => "S",
            Self::H => "h",
            Self::Md => "md",
            Self::Txt => "txt",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Fm => ".fm",
            Self::Asm => ".asm",
            Self::Gas => ".S",
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
    /// Window close while buffer is dirty.
    ConfirmQuit {
        err: Option<String>,
    },
    /// macOS: avrdude missing after Upload — offer Homebrew install (may chain silent Homebrew).
    InstallAvrdudeHomebrew,
}

struct StatusMessage {
    text: String,
    is_error: bool,
}

pub struct FullMetalApp {
    workspace: Workspace,
    editor: TextEditor,
    modal: ModalState,
    status: Option<StatusMessage>,
    sim: Cpu,
    show_sim: bool,
    sim_tab: SimTab,
    sim_last_result: Option<StepResult>,
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
    /// `limit_ips().map(f64::to_bits)` — when the dial/units change, reset the bucket so lowering IPS does not “freeze” AUTO.
    last_limit_ips_bits: Option<u64>,
    /// right panel: virtual GPIO / ADC peripherals
    show_peripherals: bool,
    peripheral_state: PeripheralState,
    /// right panel: waveform traces
    show_waveforms: bool,
    waveform_state: WaveformState,
    /// right panel: firmware hex + avrdude (replaces SIM / helpers while open)
    show_upload: bool,
    upload_programmer: String,
    upload_port: String,
    /// when true, `-P` is edited as free text; otherwise chosen from the serial port list
    upload_port_custom: bool,
    upload_status_line: String,
    upload_job_rx: Option<Receiver<String>>,
    /// Set only after a successful assemble (Sim or Assemble and Link) with a valid `.board` in source.
    assembled_board: Option<McuModel>,
}

impl FullMetalApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_style(&cc.egui_ctx);
        let root = scratch_workspace_root();
        let _ = fs::create_dir_all(&root);
        let mut app = Self {
            workspace: Workspace {
                root,
                active_file: None,
            },
            editor: TextEditor::new(Id::new("main_editor")),
            modal: ModalState::None,
            status: None,
            sim: Cpu::new(),
            show_sim: false,
            sim_tab: SimTab::Cpu,
            sim_last_result: None,
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
            last_limit_ips_bits: None,
            show_peripherals: false,
            peripheral_state: PeripheralState::default(),
            show_waveforms: false,
            waveform_state: WaveformState::default(),
            show_upload: false,
            upload_programmer: "arduino".to_string(),
            upload_port: String::new(),
            upload_port_custom: false,
            upload_status_line: String::new(),
            upload_job_rx: None,
            assembled_board: None,
        };
        app.reset_simulator_for_workspace(McuModel::Atmega328P);
        app
    }

    /// (filename, content) for supported assembly sources in the workspace root (non-recursive).
    fn collect_asm_files(&self) -> Vec<(String, String)> {
        let ws = &self.workspace;
        let Ok(entries) = std::fs::read_dir(&ws.root) else { return vec![]; };
        let mut out = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && is_supported_text_file(&path) {
                let name = path
                    .file_name()
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

    fn mcu_model_from_editor(&self) -> McuModel {
        parse_board_from_source(self.editor.source()).unwrap_or(McuModel::Atmega328P)
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

    fn reset_simulator_for_workspace(&mut self, model: McuModel) {
        self.sim = Cpu::new_for_model(model);
        self.speed_limit = SpeedLimitState::default();
        self.last_limit_ips_bits = None;
        self.limit_clock = std::time::Instant::now();
        self.limit_steps_done = 0;
        self.auto_running = false;
        self.ips = 0.0;
        self.ips_sample_start = std::time::Instant::now();
        self.ips_sample_steps = 0;
        self.breakpoints = BreakpointState::default();
        self.flash_state = FlashState::default();
        self.stack_state = StackState::default();
        self.xmem_state = XmemState::default();
        self.sim_last_result = None;
        self.sim_tab = SimTab::Cpu;
        self.waveform_state = WaveformState::default();
        self.show_waveforms = false;
    }

    fn enter_editor(&mut self, workspace: Workspace) {
        self.assembled_board = None;
        let root = workspace.root.clone();
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
        let model = parse_board_from_source(self.editor.source()).unwrap_or(McuModel::Atmega328P);
        self.reset_simulator_for_workspace(model);
        self.peripheral_state = load_peripherals_from_disk(&root);
        self.workspace = workspace;
        self.modal = ModalState::None;
    }

    fn open_workspace(&mut self, root: PathBuf) {
        let active_file = find_first_supported_file(&root);
        self.enter_editor(Workspace { root, active_file });
        self.set_status("Opened folder.");
    }

    fn switch_active_file(&mut self, path: PathBuf) -> Result<String, String> {
        self.assembled_board = None;
        if self.editor.is_dirty() && self.workspace.active_file.is_some() {
            self.save_current_file()?;
        }
        let contents = read_text_file(&path)?;
        self.workspace.active_file = Some(path.clone());
        self.editor.set_source(contents);
        let model = parse_board_from_source(self.editor.source()).unwrap_or(McuModel::Atmega328P);
        self.reset_simulator_for_workspace(model);
        self.editor.focus_next_frame();
        Ok(format!("Opened {}", path.display()))
    }

    fn source_for_assembly(&self) -> Result<String, String> {
        let workspace = &self.workspace;
        let Some(active) = workspace.active_file.as_ref() else {
            return Ok(self.editor.source().to_string());
        };
        expand_source_with_includes(workspace, active, self.editor.source())
    }

    fn assemble_and_write_firmware_hex(&mut self) -> Result<(), String> {
        if self.editor.is_dirty() {
            self.save_current_file()?;
        }
        let workspace = &self.workspace;
        let model = self.mcu_model_from_editor();
        let source = self.source_for_assembly()?;
        let words = assemble_for_model(&source).map_err(|errs| {
            errs.iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("   ")
        })?;
        let flash_words = model.flash_word_count();
        let app_words = model.application_flash_words();
        let hex_text = intel_hex::flash_words_to_intel_hex(&words, app_words);
        validate_intel_hex(&hex_text)?;
        let path = workspace.root.join(FIRMWARE_HEX);
        fs::write(&path, &hex_text).map_err(|e| e.to_string())?;
        self.upload_status_line = format!(
            "OK: wrote {} — Intel HEX validated, {} app words of {} flash ({} bytes), bootloader tail omitted.",
            path.display(),
            app_words,
            flash_words,
            app_words * 2
        );
        self.assembled_board = Some(model);
        Ok(())
    }

    fn poll_upload_job(&mut self) {
        let Some(rx) = self.upload_job_rx.take() else {
            return;
        };
        let mut batch = Vec::new();
        loop {
            match rx.try_recv() {
                Ok(msg) => batch.push(msg),
                Err(TryRecvError::Empty) => {
                    if !batch.is_empty() {
                        for msg in batch {
                            self.handle_upload_job_message(msg);
                        }
                    }
                    self.upload_job_rx = Some(rx);
                    return;
                }
                Err(TryRecvError::Disconnected) => {
                    for msg in batch {
                        self.handle_upload_job_message(msg);
                    }
                    return;
                }
            }
        }
    }

    fn handle_upload_job_message(&mut self, msg: String) {
        if msg.starts_with("AVRDUDE_MISSING\n") {
            self.upload_status_line = msg
                .strip_prefix("AVRDUDE_MISSING\n")
                .unwrap_or("")
                .trim()
                .to_string();
            #[cfg(target_os = "macos")]
            {
                self.modal = ModalState::InstallAvrdudeHomebrew;
            }
            return;
        }
        if msg.starts_with("INSTALL_OK\n") {
            self.upload_status_line = msg
                .strip_prefix("INSTALL_OK\n")
                .unwrap_or("")
                .trim()
                .to_string();
            self.rebuild_firmware_hex_then_upload();
            return;
        }
        if msg.starts_with("INSTALL_FAIL\n") {
            self.upload_status_line = msg
                .strip_prefix("INSTALL_FAIL\n")
                .unwrap_or("")
                .trim()
                .to_string();
            return;
        }
        self.upload_status_line = msg;
    }

    /// re-run assemble so `firmware.hex` matches the editor and omits the bootloader tail, then upload
    fn rebuild_firmware_hex_then_upload(&mut self) {
        match self.assemble_and_write_firmware_hex() {
            Ok(()) => self.spawn_avrdude_upload(),
            Err(e) => {
                self.upload_status_line =
                    format!("Error: assemble before upload failed — {e}");
            }
        }
    }

    fn spawn_avrdude_upload(&mut self) {
        let ws = &self.workspace;
        let hex_path = ws.root.join(FIRMWARE_HEX);
        if !hex_path.is_file() {
            self.upload_status_line = format!("Error: {} not found. Run Assemble and Link first.", hex_path.display());
            return;
        }
        if self.upload_job_rx.is_some() {
            self.upload_status_line = "Busy: wait for the current command to finish.".to_string();
            return;
        }

        let part = self.mcu_model_from_editor().avrdude_part().to_string();
        let prog = self.upload_programmer.trim().to_string();
        let port = self.upload_port.trim().to_string();
        if prog.is_empty() {
            self.upload_status_line = "Error: programmer (-c) is empty.".to_string();
            return;
        }

        let (tx, rx) = mpsc::channel::<String>();
        self.upload_job_rx = Some(rx);
        self.upload_status_line = "Looking for avrdude in PATH…".to_string();

        thread::spawn(move || {
            let found = Command::new("sh")
                .args(["-c", "command -v avrdude"])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);

            if !found {
                #[cfg(target_os = "macos")]
                {
                    let _ = tx.send(
                        "AVRDUDE_MISSING\nError: avrdude not found in PATH. Use “Install” in the dialog to install via Homebrew, or add avrdude to PATH."
                            .to_string(),
                    );
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = tx.send(
                        "Error: avrdude not found in PATH. Install avrdude for your system and ensure it is on PATH."
                            .to_string(),
                    );
                }
                return;
            }

            let mut cmd = Command::new("avrdude");
            cmd.arg("-v")
                .arg("-p")
                .arg(&part)
                .arg("-c")
                .arg(&prog);
            if !port.is_empty() {
                cmd.arg("-P").arg(&port);
            }
            cmd.arg("-U")
                .arg(format!("flash:w:{}:i", hex_path.display()));

            match cmd.output() {
                Ok(o) => {
                    let mut s = String::from("Found avrdude in PATH.\n\n");
                    if !o.stdout.is_empty() {
                        s.push_str(&String::from_utf8_lossy(&o.stdout));
                    }
                    if !o.stderr.is_empty() {
                        s.push_str(&String::from_utf8_lossy(&o.stderr));
                    }
                    if o.status.success() {
                        s.insert_str(0, "Upload OK.\n");
                    } else {
                        s.insert_str(0, "Error: avrdude exited with failure.\n");
                    }
                    let _ = tx.send(s);
                }
                Err(e) => {
                    let _ = tx.send(format!("Error: could not run avrdude: {e}"));
                }
            }
        });
    }

    /// brew and such
    #[cfg(target_os = "macos")]
    fn spawn_avrdude_homebrew_install_chain(&mut self) {
        if self.upload_job_rx.is_some() {
            self.upload_status_line =
                "Busy: wait for the current command to finish.".to_string();
            return;
        }
        let (tx, rx) = mpsc::channel::<String>();
        self.upload_job_rx = Some(rx);
        self.upload_status_line = "Installing avrdude via Homebrew…".to_string();

        thread::spawn(move || {
            let brew_install = r#"if command -v brew >/dev/null 2>&1; then brew install avrdude; elif [ -x /opt/homebrew/bin/brew ]; then /opt/homebrew/bin/brew install avrdude; elif [ -x /usr/local/bin/brew ]; then /usr/local/bin/brew install avrdude; else exit 127; fi"#;
            let shell_has_brew = || {
                Command::new("/bin/bash")
                    .arg("-c")
                    .arg(
                        "command -v brew >/dev/null 2>&1 || [ -x /opt/homebrew/bin/brew ] || [ -x /usr/local/bin/brew ]",
                    )
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false)
            };

            let mut log = String::new();
            log.push_str("Running brew install avrdude…\n\n");
            match Command::new("/bin/bash")
                .arg("-c")
                .arg(brew_install)
                .output()
            {
                Ok(o) if o.status.success() => {
                    log.push_str(&String::from_utf8_lossy(&o.stdout));
                    log.push_str(&String::from_utf8_lossy(&o.stderr));
                    let _ = tx.send(format!("INSTALL_OK\n{log}"));
                    return;
                }
                Ok(o) => {
                    log.push_str(&String::from_utf8_lossy(&o.stdout));
                    log.push_str(&String::from_utf8_lossy(&o.stderr));
                    if shell_has_brew() {
                        let _ = tx.send(format!(
                            "INSTALL_FAIL\n{log}\n\nbrew install avrdude failed while Homebrew is installed. Fix the error above or install avrdude manually."
                        ));
                        return;
                    }
                }
                Err(e) => {
                    log.push_str(&format!("{e}\n"));
                    if shell_has_brew() {
                        let _ = tx.send(format!("INSTALL_FAIL\n{log}"));
                        return;
                    }
                }
            }

            log.push_str(
                "\nHomebrew not found. Running official installer (NONINTERACTIVE, may take several minutes)…\n\n",
            );
            let hb_script = r#"NONINTERACTIVE=1 /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)""#;
            match Command::new("/bin/bash").arg("-c").arg(hb_script).output() {
                Ok(o) => {
                    log.push_str(&String::from_utf8_lossy(&o.stdout));
                    log.push_str(&String::from_utf8_lossy(&o.stderr));
                    if !o.status.success() {
                        log.push_str("\n\nHomebrew installer exited with an error.\n");
                    }
                }
                Err(e) => {
                    log.push_str(&format!("Could not run Homebrew installer: {e}\n"));
                }
            }

            log.push_str("\nRetrying brew install avrdude…\n\n");
            match Command::new("/bin/bash")
                .arg("-c")
                .arg(brew_install)
                .output()
            {
                Ok(o) if o.status.success() => {
                    log.push_str(&String::from_utf8_lossy(&o.stdout));
                    log.push_str(&String::from_utf8_lossy(&o.stderr));
                    let _ = tx.send(format!("INSTALL_OK\n{log}"));
                }
                Ok(o) => {
                    log.push_str(&String::from_utf8_lossy(&o.stdout));
                    log.push_str(&String::from_utf8_lossy(&o.stderr));
                    let _ = tx.send(format!("INSTALL_FAIL\n{log}"));
                }
                Err(e) => {
                    let _ = tx.send(format!("INSTALL_FAIL\n{log}{e}"));
                }
            }
        });
    }

    fn save_current_file(&mut self) -> Result<String, String> {
        if let Some(path) = self.workspace.active_file.clone() {
            fs::write(&path, self.editor.source()).map_err(|err| err.to_string())?;
            self.editor.mark_saved();
            self.editor.focus_next_frame();
            return Ok(format!("Saved {}", path.display()));
        }
        let path = rfd::FileDialog::new()
            .set_title("Save as")
            .add_filter("Assembly", &["fm", "asm", "s"])
            .save_file()
            .ok_or_else(|| "Save cancelled.".to_string())?;
        fs::write(&path, self.editor.source()).map_err(|e| e.to_string())?;
        self.workspace.root = path
            .parent()
            .unwrap_or_else(|| self.workspace.root.as_path())
            .to_path_buf();
        self.workspace.active_file = Some(path.clone());
        let model = parse_board_from_source(self.editor.source()).unwrap_or(McuModel::Atmega328P);
        self.reset_simulator_for_workspace(model);
        self.peripheral_state = load_peripherals_from_disk(&self.workspace.root);
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
        let root = self.workspace.root.clone();

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

        if self.editor.is_dirty() && self.workspace.active_file.is_some() {
            self.save_current_file()?;
        }

        let root = self.workspace.root.clone();

        let path = root.join(format!("{name}.{}", extension.as_str()));
        if path.exists() {
            return Err(format!("Already exists: {}", path.display()));
        }

        fs::write(&path, "").map_err(|err| err.to_string())?;
        self.workspace.active_file = Some(path.clone());
        self.editor.set_source(String::new());
        self.assembled_board = None;
        self.editor.focus_next_frame();
        Ok(format!("Created {}", path.display()))
    }

    fn add_file_to_project(&mut self) -> Result<String, String> {
        let root = self.workspace.root.clone();

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
            self.assembled_board = None;
            let contents = read_text_file(&dest_path)?;
            self.workspace.active_file = Some(dest_path.clone());
            self.editor.set_source(contents);
            let model = parse_board_from_source(self.editor.source()).unwrap_or(McuModel::Atmega328P);
            self.reset_simulator_for_workspace(model);
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
                    extension: FileExtension::Fm,
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
                    self.show_upload = false;
                    self.show_peripherals = false;
                    self.show_waveforms = false;
                    self.show_word_helper  = false;
                    self.show_cycle_helper = false;
                }
            }
            ToolbarAction::PeripheralsTogglePanel => {
                self.show_peripherals = !self.show_peripherals;
                if self.show_peripherals {
                    self.show_sim = false;
                    self.show_upload = false;
                    self.show_waveforms = false;
                    self.show_word_helper = false;
                    self.show_cycle_helper = false;
                }
            }
            ToolbarAction::WaveformsTogglePanel => {
                self.show_waveforms = !self.show_waveforms;
                if self.show_waveforms {
                    self.show_sim = false;
                    self.show_upload = false;
                    self.show_peripherals = false;
                    self.show_word_helper = false;
                    self.show_cycle_helper = false;
                }
            }
            ToolbarAction::UploadTogglePanel => {
                self.show_upload = !self.show_upload;
                if self.show_upload {
                    self.show_sim = false;
                    self.show_peripherals = false;
                    self.show_waveforms = false;
                    self.show_word_helper = false;
                    self.show_cycle_helper = false;
                }
            }
            ToolbarAction::DocsFlashLocations => {
                self.show_flash_locations = true;
            }
            ToolbarAction::HelpersWordHelper => {
                self.show_word_helper = !self.show_word_helper;
                if self.show_word_helper {
                    self.show_sim          = false;
                    self.show_peripherals = false;
                    self.show_waveforms = false;
                    self.show_upload = false;
                    self.show_cycle_helper = false;
                }
            }
            ToolbarAction::HelpersCycleHelper => {
                self.show_cycle_helper = !self.show_cycle_helper;
                if self.show_cycle_helper {
                    self.show_sim         = false;
                    self.show_peripherals = false;
                    self.show_waveforms = false;
                    self.show_upload = false;
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
            SaveThenQuit,
            DiscardThenQuit,
            RunAvrdudeHomebrewInstall,
        }

        let mut action = ModalAction::None;

        match &mut self.modal {
            ModalState::None => {}
            ModalState::NewDir { name, err } => {
                Window::new("New directory")
                    .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                    .collapsible(false)
                    .resizable(false)
                    .frame(modal_window_frame())
                    .show(ctx, |ui| {
                        modal_title(ui, "New directory");
                        ui.add_space(6.0);
                        modal_body(
                            ui,
                            "Create a new directory under the current project.",
                        );
                        ui.add_space(10.0);
                        modal_caption(ui, "Directory name");
                        ui.add_space(4.0);
                        modal_single_line_edit(ui, name);

                        if let Some(message) = err.as_ref() {
                            ui.add_space(8.0);
                            modal_error(ui, message);
                        }

                        ui.add_space(14.0);
                        ui.horizontal(|ui| {
                            if modal_btn_secondary(ui, "Cancel").clicked() {
                                action = ModalAction::Close;
                            } else if modal_btn_primary(ui, "Create").clicked() {
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
                    .frame(modal_window_frame())
                    .show(ctx, |ui| {
                        modal_title(ui, "New file");
                        ui.add_space(6.0);
                        modal_body(ui, "Create a new file under the current project.");
                        ui.add_space(10.0);
                        modal_caption(ui, "File name");
                        ui.add_space(4.0);
                        modal_single_line_edit(ui, name);
                        ui.add_space(10.0);

                        modal_caption(ui, "Extension");
                        ui.add_space(4.0);
                        ComboBox::from_id_salt("new_file_extension_modal")
                            .selected_text(
                                RichText::new(extension.label())
                                    .monospace()
                                    .size(12.0)
                                    .color(theme::ACCENT_DIM),
                            )
                            .show_ui(ui, |ui| {
                                for candidate in [
                                    FileExtension::Fm,
                                    FileExtension::Asm,
                                    FileExtension::Gas,
                                    FileExtension::H,
                                    FileExtension::Md,
                                    FileExtension::Txt,
                                ] {
                                    let label = RichText::new(candidate.label())
                                        .monospace()
                                        .size(12.0)
                                        .color(theme::ACCENT_DIM);
                                    ui.selectable_value(extension, candidate, label);
                                }
                            });

                        if let Some(message) = err.as_ref() {
                            ui.add_space(8.0);
                            modal_error(ui, message);
                        }

                        ui.add_space(14.0);
                        ui.horizontal(|ui| {
                            if modal_btn_secondary(ui, "Cancel").clicked() {
                                action = ModalAction::Close;
                            } else if modal_btn_primary(ui, "Create").clicked() {
                                action = ModalAction::CreateFile(name.clone(), *extension);
                            }
                        });
                    });
            }
            ModalState::ConfirmOpenFolder { err } => {
                Window::new("Unsaved changes — open folder")
                    .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                    .collapsible(false)
                    .resizable(false)
                    .frame(modal_window_frame())
                    .show(ctx, |ui| {
                        modal_title(ui, "Unsaved changes");
                        ui.add_space(6.0);
                        modal_body(
                            ui,
                            "Save the current file before opening another folder?",
                        );
                        if let Some(message) = err.as_ref() {
                            ui.add_space(8.0);
                            modal_error(ui, message);
                        }
                        ui.add_space(14.0);
                        ui.horizontal(|ui| {
                            if modal_btn_primary(ui, "Save").clicked() {
                                action = ModalAction::SaveThenOpenFolder;
                            } else if modal_btn_danger(ui, "Don't Save").clicked() {
                                action = ModalAction::DiscardThenOpenFolder;
                            } else if modal_btn_secondary(ui, "Cancel").clicked() {
                                action = ModalAction::Close;
                            }
                        });
                    });
            }
            ModalState::ConfirmQuit { err } => {
                Window::new("Unsaved changes — quit")
                    .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                    .collapsible(false)
                    .resizable(false)
                    .frame(modal_window_frame())
                    .show(ctx, |ui| {
                        modal_title(ui, "Unsaved changes");
                        ui.add_space(6.0);
                        modal_body(ui, "Save unsaved changes before closing?");
                        if let Some(message) = err.as_ref() {
                            ui.add_space(8.0);
                            modal_error(ui, message);
                        }
                        ui.add_space(14.0);
                        ui.horizontal(|ui| {
                            if modal_btn_primary(ui, "Save").clicked() {
                                action = ModalAction::SaveThenQuit;
                            } else if modal_btn_danger(ui, "Don't Save").clicked() {
                                action = ModalAction::DiscardThenQuit;
                            } else if modal_btn_secondary(ui, "Cancel").clicked() {
                                action = ModalAction::Close;
                            }
                        });
                    });
            }
            ModalState::InstallAvrdudeHomebrew => {
                Window::new("Install AVRDUDE (Homebrew)")
                    .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                    .collapsible(false)
                    .resizable(false)
                    .frame(modal_window_frame())
                    .show(ctx, |ui| {
                        modal_title(ui, "Install AVRDUDE");
                        ui.add_space(6.0);
                        modal_body(
                            ui,
                            "avrdude was not found. This runs brew install avrdude. \
                             If Homebrew is not installed, the official installer runs next (silent, NONINTERACTIVE; may take several minutes).",
                        );
                        ui.add_space(14.0);
                        ui.horizontal(|ui| {
                            if modal_btn_secondary(ui, "Cancel").clicked() {
                                action = ModalAction::Close;
                            } else if modal_btn_primary(ui, "Install").clicked() {
                                action = ModalAction::RunAvrdudeHomebrewInstall;
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
                self.editor.discard_unsaved_changes();
                self.modal = ModalState::None;
                self.perform_open_folder_picker();
            }
            ModalAction::SaveThenQuit => match self.save_current_file() {
                Ok(msg) => {
                    self.set_status(msg);
                    self.modal = ModalState::None;
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
                Err(message) => {
                    if let ModalState::ConfirmQuit { err } = &mut self.modal {
                        *err = Some(message);
                    }
                }
            },
            ModalAction::DiscardThenQuit => {
                self.editor.discard_unsaved_changes();
                self.modal = ModalState::None;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            ModalAction::RunAvrdudeHomebrewInstall => {
                self.modal = ModalState::None;
                self.editor.focus_next_frame();
                #[cfg(target_os = "macos")]
                self.spawn_avrdude_homebrew_install_chain();
                #[cfg(not(target_os = "macos"))]
                {
                    self.upload_status_line =
                        "Homebrew install is only offered on macOS.".to_string();
                }
            }
        }
    }
}

impl eframe::App for FullMetalApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_upload_job();
        if self.upload_job_rx.is_some() {
            ctx.request_repaint();
        }

        if ctx.input(|i| i.viewport().close_requested()) {
            let block_close =
                self.editor.is_dirty() || matches!(self.modal, ModalState::ConfirmQuit { .. });
            if block_close {
                ctx.send_viewport_cmd(ViewportCommand::CancelClose);
                if self.editor.is_dirty() && !matches!(self.modal, ModalState::ConfirmQuit { .. }) {
                    self.modal = ModalState::ConfirmQuit { err: None };
                }
            }
        }

        let editor_id = self.editor.text_edit_id();
        if ctx.memory(|m| m.has_focus(editor_id)) {
            self.editor.apply_editor_zoom_keyboard(ctx);
        }
        if ctx.memory(|m| m.has_focus(editor_id)) && self.editor.board_inline_accept_ok() {
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Tab))
                || ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowRight))
                || ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowLeft))
            {
                self.editor.apply_board_inline_completion(ctx);
            }
        }

        self.sim.clear_peripheral_inputs();
        apply_peripherals_to_cpu(&self.peripheral_state, &mut self.sim);
        if !self.show_peripherals {
            let root_buf = Some(self.workspace.root.clone());
            on_peripherals_panel_hidden(&mut self.peripheral_state, ctx, root_buf.as_deref());
        }
        if !self.show_waveforms {
            on_waveforms_panel_hidden(&mut self.waveform_state, ctx);
        }

        let mut toolbar_action = ToolbarAction::None;

        {
            let workspace = &self.workspace;
            TopBottomPanel::top("retro_toolbar")
                .exact_height(44.0)
                .show(ctx, |ui| {
                    toolbar_action = show_toolbar(
                        ui,
                        workspace.active_file.as_deref(),
                        &workspace.root,
                        self.editor.is_dirty(),
                        self.show_sim,
                        self.show_peripherals,
                        self.show_waveforms,
                        self.show_upload,
                        self.show_word_helper || self.show_cycle_helper,
                        self.assembled_board,
                    );
                });

            let files = list_workspace_supported_files(&workspace.root);
            let active = workspace.active_file.clone();
            let mut pending_switch: Option<PathBuf> = None;
            if files.len() > 1 {
                TopBottomPanel::top("workspace_files_bar")
                    .exact_height(32.0)
                    .show(ctx, |ui| {
                        Frame::NONE
                            .fill(theme::PANEL_LIFT)
                            .stroke(Stroke::new(1.0, START_GREEN_DIM))
                            .inner_margin(Margin::symmetric(8, 4))
                            .show(ui, |ui| {
                                egui::ScrollArea::horizontal()
                                    .id_salt("workspace_files_scroll")
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            for path in &files {
                                                let is_active = active.as_ref() == Some(path);
                                                let rel = path
                                                    .strip_prefix(&workspace.root)
                                                    .ok()
                                                    .unwrap_or(path.as_path());
                                                let name = rel.display().to_string();
                                                let resp = ui.add(
                                                    egui::Button::new(
                                                        RichText::new(name)
                                                            .monospace()
                                                            .size(12.0)
                                                            .color(if is_active { Color32::BLACK } else { START_GREEN }),
                                                    )
                                                    .fill(if is_active { START_GREEN } else { Color32::TRANSPARENT })
                                                    .stroke(Stroke::new(1.0, START_GREEN_DIM)),
                                                );
                                                if resp.clicked() {
                                                    pending_switch = Some(path.clone());
                                                }
                                            }
                                        });
                                    });
                            });
                    });
            }

            if let Some(path) = pending_switch {
                match self.switch_active_file(path) {
                    Ok(msg) => self.set_status(msg),
                    Err(err) => self.set_error(err),
                }
            }
        }

        // rhs_panel_editor_only: sim, helpers, or upload (mutually exclusive)
        let mut sim_action = SimAction::None;
        let mut upload_action = UploadAction::None;
        let mut wf_action = WaveformAction::None;
        let rhs_open = self.show_sim
            || self.show_peripherals
            || self.show_waveforms
            || self.show_word_helper
            || self.show_cycle_helper
            || self.show_upload;

        if rhs_open {
            egui::SidePanel::right("rhs_panel")
                .exact_width(360.0)
                .resizable(false)
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    let model = self.mcu_model_from_editor();
                    if self.show_upload {
                        let ports = scan_serial_ports();
                        upload_action = show_upload_panel(
                            ui,
                            FIRMWARE_HEX,
                            true,
                            self.assembled_board,
                            &mut self.upload_programmer,
                            &mut self.upload_port,
                            &mut self.upload_port_custom,
                            &ports,
                            &self.upload_status_line,
                        );
                    } else if self.show_sim {
                        let peripheral_pins = self.peripheral_state.pin_occupancy();
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
                            &peripheral_pins,
                            self.assembled_board,
                        );
                    } else if self.show_peripherals {
                        let project_root_buf = self.workspace.root.clone();
                        show_peripherals_panel(
                            ui,
                            &mut self.peripheral_state,
                            model,
                            &self.sim,
                            Some(project_root_buf.as_path()),
                        );
                    } else if self.show_waveforms {
                        wf_action = show_waveforms_panel(
                            ctx,
                            ui,
                            &mut self.waveform_state,
                            &self.sim,
                            model,
                            &mut self.speed_limit,
                            &mut self.auto_running,
                        );
                    } else if self.show_word_helper {
                        let files = self.collect_asm_files();
                        show_word_helper(ui, &mut self.word_helper_state, &files);
                    } else if self.show_cycle_helper {
                        let files = self.collect_asm_files();
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

                if matches!(self.modal, ModalState::None) {
                    self.editor.request_initial_focus(ctx);
                }
                let ghost = self.workspace.active_file.is_none() && self.editor.source().is_empty();
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
                    self.editor.show(ui, ghost);
                });
            });

        if toolbar_action != ToolbarAction::None {
            self.handle_toolbar_action(toolbar_action);
        }

        match upload_action {
            UploadAction::None => {}
            UploadAction::AssembleAndLink => match self.assemble_and_write_firmware_hex() {
                Ok(()) => self.set_status(format!("Wrote {FIRMWARE_HEX} in project folder.")),
                Err(e) => self.set_error(e),
            },
            UploadAction::UploadAvrdude => self.rebuild_firmware_hex_then_upload(),
        }

        match wf_action {
            WaveformAction::None => {}
            WaveformAction::StartAuto => {
                if !self.auto_running {
                    self.auto_running = true;
                    self.ips_sample_start = std::time::Instant::now();
                    self.ips_sample_steps = 0;
                    self.limit_clock = std::time::Instant::now();
                    self.limit_steps_done = 0;
                }
            }
            WaveformAction::PauseAuto => {
                self.auto_running = false;
            }
        }

        match sim_action {
            SimAction::None => {}
            SimAction::Assemble => {
                let source = match self.source_for_assembly() {
                    Ok(src) => src,
                    Err(err) => {
                        self.set_error(err);
                        return;
                    }
                };
                match assemble_for_model(&source) {
                    Ok(words) => {
                        let n = words.len();
                        let model = parse_board_from_source(&source)
                            .expect("assemble succeeded implies valid .board");
                        // Must match `McuModel` from `.board` — SRAM bounds, IVT layout, and flash
                        // limit all depend on the chip (328P vs 128A are not interchangeable).
                        if self.sim.model != model {
                            self.reset_simulator_for_workspace(model);
                        } else {
                            self.sim.reset();
                        }
                        self.sim.load_flash(&words);
                        self.sim_last_result = None;
                        self.waveform_state.on_reset();
                        self.assembled_board = Some(model);
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
                self.waveform_state.sample_cpu(&self.sim);
            }
            SimAction::Run10 => {
                let (_, r) = {
                    let wf = &mut self.waveform_state;
                    let sim = &mut self.sim;
                    sim.step_n_hook(10, |cpu| {
                        wf.sample_cpu(cpu);
                    })
                };
                self.sim_last_result = Some(r);
            }
            SimAction::Run100 => {
                let (_, r) = {
                    let wf = &mut self.waveform_state;
                    let sim = &mut self.sim;
                    sim.step_n_hook(100, |cpu| {
                        wf.sample_cpu(cpu);
                    })
                };
                self.sim_last_result = Some(r);
            }
            SimAction::Reset => {
                self.sim.reset();
                self.waveform_state.on_reset();
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
            let cur_limit_bits = limit_ips.map(f64::to_bits);
            if cur_limit_bits != self.last_limit_ips_bits {
                self.limit_clock = std::time::Instant::now();
                self.limit_steps_done = 0;
            }
            self.last_limit_ips_bits = cur_limit_bits;

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
                    let r = {
                        let wf = &mut self.waveform_state;
                        let sim = &mut self.sim;
                        sim.run_n_break_hook(batch, &bp_addrs, |cpu| {
                            wf.sample_cpu(cpu);
                        })
                    };
                    self.limit_steps_done += r.0;
                    r
                } else {
                    // ahead of budget — skip this frame, wake up soon
                    ctx.request_repaint_after(std::time::Duration::from_micros(500));
                    (0, StepResult::Ok, None)
                }
            } else {
                // unlimited: run for 12 ms
                let wf = &mut self.waveform_state;
                let sim = &mut self.sim;
                sim.run_timed_break_hook(12, &bp_addrs, |cpu| {
                    wf.sample_cpu(cpu);
                })
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

        show_flash_locations_window(ctx, &mut self.show_flash_locations, self.assembled_board, &self.sim);

        self.show_modal(ctx);
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {}
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
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "fm" | "h" | "md" | "txt" | "asm" | "s"
            )
        })
}

fn scratch_workspace_root() -> PathBuf {
    std::env::temp_dir().join("full_metal_studio_scratch")
}

fn read_text_file(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|err| format!("{}: {}", path.display(), err))
}

fn find_first_supported_file(root: &Path) -> Option<PathBuf> {
    list_workspace_supported_files(root).into_iter().next()
}

fn list_workspace_supported_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if is_supported_text_file(&path) {
                out.push(path);
            }
        }
    }
    out.sort_by(|a, b| a.to_string_lossy().cmp(&b.to_string_lossy()));
    out
}

fn parse_include_target(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    let rest = trimmed
        .strip_prefix(".include")
        .or_else(|| trimmed.strip_prefix(".INCLUDE"))
        .or_else(|| trimmed.strip_prefix("#include"))?
        .trim();
    let start = rest.find('"')?;
    let rest = &rest[start + 1..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

fn normalize_include_path(base_dir: &Path, include: &str) -> PathBuf {
    let p = Path::new(include);
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        base_dir.join(p)
    };
    std::fs::canonicalize(&joined).unwrap_or(joined)
}

fn strip_asm_line_comment(line: &str) -> &str {
    let line = line.find(';').map(|i| &line[..i]).unwrap_or(line);
    line.find('#').map(|i| &line[..i]).unwrap_or(line)
}

fn split_leading_include_block(source: &str) -> (Vec<String>, String) {
    let lines: Vec<&str> = source.lines().collect();
    let mut targets = Vec::new();
    let mut i = 0usize;
    while i < lines.len() {
        let raw = lines[i];
        let t = strip_asm_line_comment(raw).trim();
        if t.is_empty() {
            i += 1;
            continue;
        }
        if let Some(target) = parse_include_target(raw) {
            targets.push(target.to_string());
            i += 1;
            continue;
        }
        break;
    }
    let rest = lines[i..].join("\n");
    (targets, rest)
}

fn expand_source_with_includes(
    workspace: &Workspace,
    active_path: &Path,
    active_source: &str,
) -> Result<String, String> {
    let mut stack = Vec::<PathBuf>::new();
    let mut expanded = String::new();
    let active_norm = std::fs::canonicalize(active_path).unwrap_or_else(|_| active_path.to_path_buf());
    expand_source_inner(
        workspace,
        &active_norm,
        active_source,
        Some((&active_norm, active_source)),
        &mut stack,
        &mut expanded,
    )?;
    Ok(expanded)
}

fn expand_source_inner(
    workspace: &Workspace,
    current_path: &Path,
    source: &str,
    active_override: Option<(&Path, &str)>,
    stack: &mut Vec<PathBuf>,
    out: &mut String,
) -> Result<(), String> {
    let current_norm = std::fs::canonicalize(current_path).unwrap_or_else(|_| current_path.to_path_buf());
    if stack.contains(&current_norm) {
        let cycle = stack
            .iter()
            .chain(std::iter::once(&current_norm))
            .map(|p| p.strip_prefix(&workspace.root).unwrap_or(p).display().to_string())
            .collect::<Vec<_>>()
            .join(" -> ");
        return Err(format!("Include cycle detected: {cycle}"));
    }
    stack.push(current_norm.clone());

    let (leading_targets, rest) = split_leading_include_block(source);
    let base_dir = current_norm.parent().unwrap_or(&workspace.root);
    let mut included_once = HashSet::<PathBuf>::new();

    for line in rest.lines() {
        if let Some(target) = parse_include_target(line) {
            let include_path = normalize_include_path(base_dir, target);
            if !include_path.starts_with(&workspace.root) {
                return Err(format!(
                    "Include escapes workspace: {}",
                    include_path.display()
                ));
            }
            if !included_once.insert(include_path.clone()) {
                continue;
            }
            let include_source = if let Some((active_path, src)) = active_override {
                if include_path == active_path {
                    src.to_string()
                } else {
                    read_text_file(&include_path)?
                }
            } else {
                read_text_file(&include_path)?
            };
            expand_source_inner(
                workspace,
                &include_path,
                &include_source,
                active_override,
                stack,
                out,
            )?;
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }

    for target in leading_targets {
        let include_path = normalize_include_path(&base_dir, &target);
        if !include_path.starts_with(&workspace.root) {
            return Err(format!(
                "Include escapes workspace: {}",
                include_path.display()
            ));
        }
        if !included_once.insert(include_path.clone()) {
            continue;
        }
        let include_source = if let Some((active_path, src)) = active_override {
            if include_path == active_path {
                src.to_string()
            } else {
                read_text_file(&include_path)?
            }
        } else {
            read_text_file(&include_path)?
        };
        expand_source_inner(
            workspace,
            &include_path,
            &include_source,
            active_override,
            stack,
            out,
        )?;
    }

    let _ = stack.pop();
    Ok(())
}
