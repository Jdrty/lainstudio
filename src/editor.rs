//! Multiline text editor with a line-number gutter.

use eframe::egui::{
    self, Align, Color32, Id, Layout, Margin, RichText, ScrollArea, TextEdit, TextStyle, Ui,
};

use crate::welcome::START_GREEN;

pub struct TextEditor {
    source: String,
    saved_source: String,
    id: Id,
    needs_focus: bool,
}

impl TextEditor {
    pub fn new(id: Id) -> Self {
        Self {
            source: String::new(),
            saved_source: String::new(),
            id,
            needs_focus: false,
        }
    }

    pub fn reset_for_session(&mut self) {
        self.focus_next_frame();
    }

    pub fn set_source(&mut self, source: String) {
        self.saved_source = source.clone();
        self.source = source;
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn is_dirty(&self) -> bool {
        self.source != self.saved_source
    }

    pub fn mark_saved(&mut self) {
        self.saved_source = self.source.clone();
    }

    pub fn focus_next_frame(&mut self) {
        self.needs_focus = true;
    }

    pub fn request_initial_focus(&mut self, ctx: &egui::Context) {
        if self.needs_focus {
            ctx.memory_mut(|mem| mem.request_focus(self.id));
            self.needs_focus = false;
        }
    }

    pub fn show(&mut self, ui: &mut Ui) {
        let font_id = TextStyle::Monospace.resolve(ui.style());
        let row_h = ui.fonts(|f| f.row_height(&font_id));
        let n = line_count(&self.source);
        let digit_cols = (n.max(1).ilog10() + 1).max(3) as usize;
        let gutter_w = ui.fonts(|f| f.glyph_width(&font_id, '0') * digit_cols as f32 + 10.0);

        ScrollArea::vertical()
            .id_salt("editor_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal_top(|ui| {
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
                                            .color(START_GREEN),
                                    );
                                },
                            );
                        }
                    });

                    ui.add(
                        TextEdit::multiline(&mut self.source)
                            .id(self.id)
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
    }
}

fn line_count(text: &str) -> usize {
    if text.is_empty() {
        1
    } else {
        text.split('\n').count()
    }
}
