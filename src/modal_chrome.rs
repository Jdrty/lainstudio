//! styling for file dialogs and confirmation modals (matches simulator / peripheral panels)
//! (mostly, not fully implimented)

use eframe::egui::{
    self, Button, Color32, CornerRadius, FontId, Frame, Margin, RichText, Stroke, TextEdit, Ui,
};

use crate::theme;
use crate::theme::{START_GREEN, START_GREEN_DIM};

pub fn modal_window_frame() -> Frame {
    Frame::NONE
        .fill(theme::SIM_SURFACE)
        .stroke(Stroke::new(0.75, theme::SIM_BORDER))
        .inner_margin(Margin::same(14))
        .corner_radius(CornerRadius::same(5))
}

pub fn modal_title(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .monospace()
            .size(13.0)
            .color(START_GREEN),
    );
}

pub fn modal_body(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .monospace()
            .size(12.0)
            .color(theme::ACCENT_DIM),
    );
}

pub fn modal_caption(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .monospace()
            .size(11.0)
            .color(theme::ACCENT_DIM),
    );
}

pub fn modal_error(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .monospace()
            .size(11.5)
            .color(theme::ERR_RED),
    );
}

pub fn modal_single_line_edit(ui: &mut Ui, text: &mut String) {
    modal_single_line_edit_with_id(ui, text, None, f32::INFINITY);
}

/// Same as [`modal_single_line_edit`] but with a stable widget id and width (dialogs use `INFINITY`).
pub fn modal_single_line_edit_with_id(
    ui: &mut Ui,
    text: &mut String,
    id: Option<egui::Id>,
    desired_width: f32,
) -> egui::Response {
    let inner = Frame::NONE
        .fill(theme::SIM_SURFACE_LIFT)
        .stroke(Stroke::new(0.75, theme::SIM_BORDER))
        .inner_margin(Margin::symmetric(8, 6))
        .corner_radius(CornerRadius::same(4))
        .show(ui, |ui| {
            let mut te = TextEdit::singleline(text)
                .font(FontId::monospace(12.0))
                .desired_width(desired_width);
            if let Some(i) = id {
                te = te.id(i);
            }
            ui.add(te)
        });
    inner.inner
}

/// Compact chrome for the editor find bar (sim / peripheral style).
pub fn search_bar_frame() -> Frame {
    Frame::NONE
        .fill(theme::SEARCH_BG)
        .stroke(Stroke::new(0.75, theme::SIM_BORDER))
        .inner_margin(Margin::symmetric(10, 8))
        .corner_radius(CornerRadius::same(5))
}

pub fn modal_btn_primary(ui: &mut Ui, label: &str) -> egui::Response {
    ui.add(
        Button::new(
            RichText::new(label)
                .monospace()
                .size(12.0)
                .color(Color32::BLACK),
        )
        .fill(START_GREEN_DIM)
        .stroke(Stroke::new(1.0, START_GREEN))
        .corner_radius(CornerRadius::same(5)),
    )
}

pub fn modal_btn_secondary(ui: &mut Ui, label: &str) -> egui::Response {
    ui.add(
        Button::new(
            RichText::new(label)
                .monospace()
                .size(12.0)
                .color(START_GREEN_DIM),
        )
        .fill(theme::SIM_SURFACE_LIFT)
        .stroke(Stroke::new(0.75, theme::SIM_BORDER))
        .corner_radius(CornerRadius::same(5)),
    )
}

pub fn modal_btn_danger(ui: &mut Ui, label: &str) -> egui::Response {
    ui.add(
        Button::new(
            RichText::new(label)
                .monospace()
                .size(12.0)
                .color(theme::ACCENT_DIM),
        )
        .fill(theme::SIM_TAB_ACTIVE)
        .stroke(Stroke::new(0.75, theme::SIM_BORDER_BRIGHT))
        .corner_radius(CornerRadius::same(5)),
    )
}
