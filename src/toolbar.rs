//! toolbar_basic

use std::path::Path;
use std::sync::Arc;

use eframe::egui::{
    menu, Align, CornerRadius, FontFamily, FontId, Frame, Layout, Margin, RichText,
    Stroke, Ui,
};

use crate::avr::McuModel;
use crate::theme::{self, START_GREEN, START_GREEN_DIM};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolbarAction {
    None,
    Save,
    SaveAll,
    NewFile,
    NewDir,
    OpenFolder,
    AddFileToProject,
    SimTogglePanel,
    PeripheralsTogglePanel,
    WaveformsTogglePanel,
    UploadTogglePanel,
    DocsFlashLocations,
    HelpersWordHelper,
    HelpersCycleHelper,
}

fn title_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(Arc::from("fm_title")))
}

fn apply_dropdown_style(ui: &mut Ui) {
    let style = ui.style_mut();
    style.visuals.override_text_color = Some(START_GREEN);
    style.visuals.window_corner_radius = CornerRadius::ZERO;
    style.visuals.menu_corner_radius = CornerRadius::ZERO;
    style.visuals.window_stroke = Stroke::new(1.0, START_GREEN);
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, START_GREEN);
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, START_GREEN);
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, START_GREEN);
    style.visuals.widgets.active.bg_stroke = Stroke::new(1.0, START_GREEN);
    style.visuals.widgets.open.bg_stroke = Stroke::new(1.0, START_GREEN);
    style.visuals.widgets.noninteractive.corner_radius = CornerRadius::ZERO;
    style.visuals.widgets.inactive.corner_radius = CornerRadius::ZERO;
    style.visuals.widgets.hovered.corner_radius = CornerRadius::ZERO;
    style.visuals.widgets.active.corner_radius = CornerRadius::ZERO;
    style.visuals.widgets.open.corner_radius = CornerRadius::ZERO;
}

pub fn show_toolbar(
    ui:                  &mut Ui,
    active_file:         Option<&Path>,
    workspace_root:      &Path,
    is_dirty:            bool,
    sim_visible:        bool,
    peripherals_visible: bool,
    waveforms_visible:   bool,
    upload_visible:     bool,
    helpers_visible:    bool,
    assembled_board:   Option<McuModel>,
) -> ToolbarAction {
    let mut action = ToolbarAction::None;

    Frame::NONE
        .fill(theme::PANEL_MID)
        .stroke(Stroke::new(1.0, START_GREEN_DIM))
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            menu::bar(ui, |ui| {
                ui.menu_button(
                    RichText::new("FILE")
                        .font(title_font(18.0))
                        .color(START_GREEN),
                    |ui| {
                        apply_dropdown_style(ui);

                        if ui.button("Save").clicked() {
                            action = ToolbarAction::Save;
                            ui.close_menu();
                        }
                        if ui.button("Save all").clicked() {
                            action = ToolbarAction::SaveAll;
                            ui.close_menu();
                        }

                        ui.separator();

                        if ui.button("New file").clicked() {
                            action = ToolbarAction::NewFile;
                            ui.close_menu();
                        }
                        if ui.button("New dir").clicked() {
                            action = ToolbarAction::NewDir;
                            ui.close_menu();
                        }

                        ui.separator();

                        if ui.button("Open folder").clicked() {
                            action = ToolbarAction::OpenFolder;
                            ui.close_menu();
                        }
                        if ui.button("Add file to project").clicked() {
                            action = ToolbarAction::AddFileToProject;
                            ui.close_menu();
                        }
                    },
                );

                let sim_label = if sim_visible { "SIM ▪" } else { "SIM" };
                if ui
                    .add(eframe::egui::Button::new(
                        RichText::new(sim_label)
                            .font(title_font(18.0))
                            .color(START_GREEN),
                    ).frame(false))
                    .clicked()
                {
                    action = ToolbarAction::SimTogglePanel;
                }

                let periph_label = if peripherals_visible { "PERIPH ▪" } else { "PERIPH" };
                if ui
                    .add(eframe::egui::Button::new(
                        RichText::new(periph_label)
                            .font(title_font(18.0))
                            .color(START_GREEN),
                    )
                    .frame(false))
                    .clicked()
                {
                    action = ToolbarAction::PeripheralsTogglePanel;
                }

                let wf_label = if waveforms_visible { "WAVEFORMS ▪" } else { "WAVEFORMS" };
                if ui
                    .add(eframe::egui::Button::new(
                        RichText::new(wf_label)
                            .font(title_font(18.0))
                            .color(START_GREEN),
                    )
                    .frame(false))
                    .clicked()
                {
                    action = ToolbarAction::WaveformsTogglePanel;
                }

                let upload_label = if upload_visible { "UPLOAD ▪" } else { "UPLOAD" };
                if ui
                    .add(eframe::egui::Button::new(
                        RichText::new(upload_label)
                            .font(title_font(18.0))
                            .color(START_GREEN),
                    )
                    .frame(false))
                    .clicked()
                {
                    action = ToolbarAction::UploadTogglePanel;
                }

                ui.menu_button(
                    RichText::new("DOCS")
                        .font(title_font(18.0))
                        .color(START_GREEN),
                    |ui| {
                        apply_dropdown_style(ui);
                        if ui.button("Flash locations").clicked() {
                            action = ToolbarAction::DocsFlashLocations;
                            ui.close_menu();
                        }
                    },
                );

                let helpers_label = if helpers_visible { "HELPERS ▪" } else { "HELPERS" };
                ui.menu_button(
                    RichText::new(helpers_label)
                        .font(title_font(18.0))
                        .color(START_GREEN),
                    |ui| {
                        apply_dropdown_style(ui);
                        if ui.button("Word helper").clicked() {
                            action = ToolbarAction::HelpersWordHelper;
                            ui.close_menu();
                        }
                        if ui.button("Cycle helper").clicked() {
                            action = ToolbarAction::HelpersCycleHelper;
                            ui.close_menu();
                        }
                    },
                );

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let mut label = active_file
                        .and_then(|path| path.file_name())
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_else(|| "(unsaved)".to_string());
                    if is_dirty {
                        label.push_str(" *");
                    }

                    ui.label(
                        RichText::new(label)
                            .monospace()
                            .color(START_GREEN)
                            .size(14.0),
                    );
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(
                            assembled_board
                                .map(|m| m.label().to_string())
                                .unwrap_or_else(|| "—".to_string()),
                        )
                            .monospace()
                            .color(START_GREEN_DIM)
                            .size(11.5),
                    );
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new(workspace_root.display().to_string())
                            .monospace()
                            .color(START_GREEN_DIM)
                            .size(12.0),
                    );
                });
            });
        });

    action
}
