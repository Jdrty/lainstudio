//! Native window using egui (immediate mode) via eframe.

use eframe::egui::{
    self, Align, Color32, FontData, FontDefinitions, FontFamily, FontId, Frame, Id, Layout, Margin,
    RichText, ScrollArea, Stroke, TextEdit, TextStyle, Visuals,
};
use std::sync::Arc;

fn line_count(text: &str) -> usize {
    if text.is_empty() {
        1
    } else {
        text.split('\n').count()
    }
}

fn setup(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "iosevka_term".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../include/IosevkaTerm-Regular.ttf"
        ))),
    );
    if let Some(stack) = fonts.families.get_mut(&FontFamily::Monospace) {
        stack.insert(0, "iosevka_term".to_owned());
    }
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
    black_widget(&mut visuals.widgets.inactive);
    black_widget(&mut visuals.widgets.hovered);
    black_widget(&mut visuals.widgets.active);
    black_widget(&mut visuals.widgets.open);

    visuals.text_cursor.stroke = Stroke::new(2.0, Color32::WHITE);
    visuals.selection.bg_fill = Color32::from_rgb(55, 55, 55);
    visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    ctx.set_visuals(visuals);

    ctx.style_mut(|style| {
        style
            .text_styles
            .insert(TextStyle::Monospace, FontId::new(14.0, FontFamily::Monospace));
    });
}

fn main() -> eframe::Result {
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "Lain Studio",
        native_options,
        Box::new(|cc| Ok(Box::new(LainApp::new(cc)))),
    )
}

struct LainApp {
    source: String,
    editor_id: Id,
    focus_set: bool,
}

impl LainApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup(&cc.egui_ctx);
        Self {
            source: String::new(),
            editor_id: Id::new("main_editor"),
            focus_set: false,
        }
    }
}

impl eframe::App for LainApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.focus_set {
            ctx.memory_mut(|mem| mem.request_focus(self.editor_id));
            self.focus_set = true;
        }

        egui::CentralPanel::default()
            .frame(
                Frame::NONE
                    .fill(Color32::BLACK)
                    .inner_margin(Margin::same(6)),
            )
            .show(ctx, |ui| {
                ui.set_min_size(ui.available_size());

                let font_id = TextStyle::Monospace.resolve(ui.style());
                let row_h = ui.fonts(|f| f.row_height(&font_id));
                let n = line_count(&self.source);
                let digit_cols = (n.max(1).ilog10() + 1).max(3) as usize;
                let gutter_w = ui.fonts(|f| {
                    f.glyph_width(&font_id, '0') * digit_cols as f32 + 10.0
                });

                ScrollArea::vertical()
                    .id_salt("editor_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal_top(|ui| {
                            // `vertical` stacks line numbers; `horizontal_top` alone would place each row to the right of the last.
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                                ui.set_width(gutter_w);
                                for line_nr in 1..=n {
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(gutter_w, row_h),
                                        Layout::right_to_left(Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new(format!("{line_nr}"))
                                                    .font(font_id.clone())
                                                    .color(Color32::WHITE),
                                            );
                                        },
                                    );
                                }
                            });

                            ui.add(
                                TextEdit::multiline(&mut self.source)
                                    .id(self.editor_id)
                                    .frame(false)
                                    .code_editor()
                                    .margin(Margin::ZERO)
                                    .text_color(Color32::WHITE)
                                    .background_color(Color32::BLACK)
                                    .desired_width(ui.available_width())
                                    .desired_rows(1),
                            );
                        });
                    });
            });
    }
}
