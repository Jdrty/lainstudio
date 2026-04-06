//! welcome_screen

use std::f32::consts::{SQRT_2, TAU};
use std::sync::{Arc, OnceLock};

use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{self, Color32, FontId, Image, RichText, Stroke, StrokeKind, Ui, Vec2};

use crate::avr::McuModel;
use crate::welcome_font;

pub const START_GREEN: Color32 = Color32::from_rgb(0x0b, 0xca, 0x0b);

pub const START_GREEN_DIM: Color32 = START_GREEN;

pub const WELCOME_FILL: Color32 = START_GREEN;
pub const WELCOME_OUTLINE: Color32 = Color32::from_rgb(0x21, 0x3f, 0x00);

const OUTLINE_SOFT_X: f32 = 15.51;
const OUTLINE_SOFT_Y: f32 = 14.39;
const OUTLINE_GLOW: f32 = 0.355;
const OUTLINE_THICK_REL: f32 = 0.0888;

const FILL_SOFT_X: f32 = 2.99;
const FILL_SOFT_Y: f32 = 0.93;
const FILL_GLOW: f32 = 0.178;

/// Maps design softness numbers to point offsets (~proportional to cap height).
const SOFT_TO_PT: f32 = 1.0 / 72.0;

pub const WELCOME_SIZE_PCT: u8 = 68;

const MARGIN: f32 = 44.0;

const TITLE_REF_PX: f32 = 54.0;

fn size_factor() -> f32 {
    (WELCOME_SIZE_PCT.clamp(1, 100) as f32) / 100.0
}

fn layout_welcome_galley(ui: &Ui, text: &str, size: f32, spacing: f32) -> Arc<egui::Galley> {
    let job = LayoutJob::single_section(
        text.to_owned(),
        TextFormat {
            font_id: FontId::new(size, welcome_font::family()),
            color: WELCOME_FILL,
            extra_letter_spacing: spacing,
            ..Default::default()
        },
    );
    ui.fonts(|f| f.layout_job(job))
}

fn measure_title_line(ui: &Ui, text: &str, size: f32, spacing: f32) -> f32 {
    layout_welcome_galley(ui, text, size, spacing).size().x
}

fn oct_offsets(r: f32) -> [(f32, f32); 8] {
    let s = SQRT_2.recip() * r;
    [
        (-r, 0.0),
        (r, 0.0),
        (0.0, -r),
        (0.0, r),
        (-s, -s),
        (s, -s),
        (-s, s),
        (s, s),
    ]
}

fn paint_welcome_text_glow(ui: &Ui, galley: Arc<egui::Galley>, pos: egui::Pos2, font_size: f32) {
    let painter = ui.painter();

    let sx_o = font_size * OUTLINE_SOFT_X * SOFT_TO_PT;
    let sy_o = font_size * OUTLINE_SOFT_Y * SOFT_TO_PT;
    let sx_f = font_size * FILL_SOFT_X * SOFT_TO_PT;
    let sy_f = font_size * FILL_SOFT_Y * SOFT_TO_PT;
    let thick = (font_size * OUTLINE_THICK_REL).max(0.5);

    let outline_glow_alpha = (OUTLINE_GLOW * 255.0) as u8;
    let fill_glow_alpha = (FILL_GLOW * 255.0) as u8;

    const RINGS: i32 = 5;
    const SPOKES: i32 = 10;

    for ring in 1..=RINGS {
        let t = ring as f32 / RINGS as f32;
        let rx = sx_o * t;
        let ry = sy_o * t;
        let a = (outline_glow_alpha as f32 * (1.0 - t * 0.35)) / 255.0;
        for k in 0..SPOKES {
            let ang = TAU * k as f32 / SPOKES as f32;
            let dx = rx * ang.cos();
            let dy = ry * ang.sin();
            painter.galley_with_override_text_color(
                pos + egui::vec2(dx, dy),
                galley.clone(),
                WELCOME_OUTLINE.gamma_multiply(a),
            );
        }
    }

    for &(dx, dy) in &oct_offsets(thick) {
        painter.galley_with_override_text_color(
            pos + egui::vec2(dx, dy),
            galley.clone(),
            WELCOME_OUTLINE,
        );
    }
    let inner = thick * 0.52;
    for &(dx, dy) in &oct_offsets(inner) {
        painter.galley_with_override_text_color(
            pos + egui::vec2(dx, dy),
            galley.clone(),
            WELCOME_OUTLINE.gamma_multiply(0.72),
        );
    }

    for k in 0..8 {
        let ang = TAU * k as f32 / 8.0;
        let dx = sx_f * 0.55 * ang.cos();
        let dy = sy_f * 0.55 * ang.sin();
        painter.galley_with_override_text_color(
            pos + egui::vec2(dx, dy),
            galley.clone(),
            WELCOME_FILL.gamma_multiply(fill_glow_alpha as f32 / 255.0),
        );
    }

    painter.galley_with_override_text_color(pos, galley, WELCOME_FILL);
}

fn paint_welcome_button_frame(
    painter: &egui::Painter,
    rect: egui::Rect,
    ref_px: f32,
    hovered: bool,
) {
    let cr = egui::CornerRadius::ZERO;
    if hovered {
        painter.rect_filled(rect, cr, START_GREEN.gamma_multiply(0.1));
    }

    let stroke_w = (ref_px * 0.055).clamp(1.0, 1.45);
    painter.rect_stroke(
        rect.expand(0.5),
        cr,
        Stroke::new(1.0, START_GREEN.gamma_multiply(0.18)),
        StrokeKind::Outside,
    );
    painter.rect_stroke(
        rect,
        cr,
        Stroke::new(stroke_w, START_GREEN),
        StrokeKind::Inside,
    );
}

fn centered_welcome_line(ui: &mut Ui, text: &str, font_size: f32, letter_sp: f32) {
    let galley = layout_welcome_galley(ui, text, font_size, letter_sp);
    let size = galley.size();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(((ui.available_width() - size.x) * 0.5).max(0.0));
        let (_, rect) = ui.allocate_space(size);
        paint_welcome_text_glow(ui, galley, rect.min, font_size);
    });
}

fn banner_pixel_size() -> (u32, u32) {
    static DIMS: OnceLock<(u32, u32)> = OnceLock::new();
    *DIMS.get_or_init(|| {
        let bytes = include_bytes!("../assets/images/lain_banner.png");
        let img = image::load_from_memory(bytes).expect(
            "assets/images/lain_banner.png should be valid image bytes (PNG or JPEG, etc.)",
        );
        (img.width(), img.height())
    })
}

fn banner_size_for_width(w: f32) -> Vec2 {
    let (pw, ph) = banner_pixel_size();
    Vec2::new(w, w * (ph as f32 / pw as f32))
}

fn clamp_banner_height(size: Vec2, max_h: f32) -> Vec2 {
    if max_h <= 0.0 || size.y <= max_h {
        return size;
    }
    let s = max_h / size.y;
    Vec2::new(size.x * s, max_h)
}

struct WelcomeStartButton {
    pub response: egui::Response,
}

fn welcome_start_button(ui: &mut Ui, label: &str, font_px: f32) -> WelcomeStartButton {
    let galley = layout_welcome_galley(ui, label, font_px, 0.0);
    let text_size = galley.size();
    let pad = egui::vec2(10.0, 6.0);
    let hit_size = text_size + pad * 2.0;

    let mut out = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(((ui.available_width() - hit_size.x) * 0.5).max(0.0));
        let (rect, response) = ui.allocate_exact_size(hit_size, egui::Sense::click());
        paint_welcome_button_frame(&ui.painter(), rect, font_px, response.hovered());
        let text_rect = egui::Rect::from_center_size(rect.center(), text_size);
        paint_welcome_text_glow(ui, galley, text_rect.min, font_px);
        out = Some(WelcomeStartButton { response });
    });
    out.expect("horizontal always runs")
}

fn skip_boot_checkbox(ui: &mut Ui, checked: &mut bool, label_px: f32, letter_sp: f32, stroke_ref_px: f32) {
    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let box_s = (label_px * 1.08).clamp(12.0, 18.0);
        let (rect, response) = ui.allocate_exact_size(egui::vec2(box_s, box_s), egui::Sense::click());
        if response.clicked() {
            *checked = !*checked;
        }

        let painter = ui.painter();
        paint_welcome_button_frame(&painter, rect, stroke_ref_px, response.hovered());
        if *checked {
            let cr = egui::CornerRadius::ZERO;
            let inset = (box_s * 0.22).max(3.2);
            painter.rect_filled(rect.shrink(inset), cr, START_GREEN);
        }

        let galley = layout_welcome_galley(ui, "Skip start animation", label_px, letter_sp);
        let text_size = galley.size();
        let (text_rect, _) = ui.allocate_exact_size(text_size, egui::Sense::hover());
        paint_welcome_text_glow(ui, galley, text_rect.min, label_px);
    });
}

pub enum WelcomeAction {
    None,
    OpenFolder,
    CreateNew,
}

pub fn show_welcome(ui: &mut Ui, skip_start_animation: &mut bool) -> WelcomeAction {
    let mut action = WelcomeAction::None;

    let panel = ui.max_rect();
    let avail_w = ui.available_width();
    let pct = size_factor();
    let content_w = ((avail_w - 2.0 * MARGIN).max(300.0) * pct).max(200.0);

    let ref_w = measure_title_line(ui, "LAIN STUDIO", TITLE_REF_PX, 2.0).max(1.0);
    let scale = content_w / ref_w;

    let title_px = TITLE_REF_PX * scale;
    let subtitle_px = (19.0 / TITLE_REF_PX) * title_px;
    let button_px = ((15.0 / TITLE_REF_PX) * title_px).max(14.0);
    let letter_sp = 2.0 * scale;
    let sub_letter_sp = 3.0 * scale;
    let sp = |n: f32| -> f32 { n * scale };

    let skip_label_px = (subtitle_px * 0.48).max(8.5);
    let skip_letter_sp = sub_letter_sp * 0.48;
    let skip_galley = layout_welcome_galley(ui, "Skip start animation", skip_label_px, skip_letter_sp);
    let box_s = (skip_label_px * 1.08).clamp(12.0, 18.0);
    let row_h = (box_s.max(skip_galley.size().y) + 10.0).max(28.0);
    let toggle_w = (16.0 + box_s + 10.0 + skip_galley.size().x + 20.0).min((panel.width() - 32.0).max(200.0));
    let toggle_rect = egui::Rect::from_min_size(panel.left_top() + egui::vec2(16.0, 12.0), egui::vec2(toggle_w, row_h));
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(toggle_rect), |ui| {
        skip_boot_checkbox(ui, skip_start_animation, skip_label_px, skip_letter_sp, skip_label_px);
    });

    ui.vertical_centered(|ui| {
        ui.add_space(MARGIN);

        centered_welcome_line(ui, "LAIN STUDIO", title_px, letter_sp);
        ui.add_space(sp(12.0));
        centered_welcome_line(ui, "CHOOSE HOW TO START", subtitle_px, sub_letter_sp);
        ui.add_space(sp(36.0));

        let btn_open = welcome_start_button(ui, "Open folder…", button_px);
        if btn_open.response.clicked() {
            action = WelcomeAction::OpenFolder;
        }
        ui.add_space(sp(8.0));
        let btn_create = welcome_start_button(ui, "Create a new file…", button_px);
        if btn_create.response.clicked() {
            action = WelcomeAction::CreateNew;
        }

        ui.add_space(sp(28.0));

        let max_banner_h = (ui.available_height() - MARGIN).max(0.0);
        let banner_natural = banner_size_for_width(content_w);
        let banner_size = clamp_banner_height(banner_natural, max_banner_h);

        ui.add(
            Image::new(egui::include_image!("../assets/images/lain_banner.png"))
                .fit_to_exact_size(banner_size),
        );
        ui.add_space(MARGIN);
    });
    action
}

pub enum CreateProjectAction {
    None,
    PickParentFolder,
    Back,
    Submit,
}

fn title_font(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name(std::sync::Arc::from("lain_title")))
}

pub fn show_create_project(
    ui: &mut Ui,
    parent_dir: &Option<std::path::PathBuf>,
    name: &mut String,
    model: &mut McuModel,
    err: &Option<String>,
) -> CreateProjectAction {
    let mut action = CreateProjectAction::None;

    let avail_w = ui.available_width();
    let pct = size_factor();
    let content_w = ((avail_w - 2.0 * MARGIN).max(300.0) * pct).max(200.0);

    let ref_w = {
        let job = LayoutJob::single_section(
            "NEW PROJECT".into(),
            TextFormat {
                font_id: title_font(TITLE_REF_PX),
                color: START_GREEN,
                extra_letter_spacing: 2.0,
                ..Default::default()
            },
        );
        ui.fonts(|f| f.layout_job(job).size().x)
    }
    .max(1.0);
    let scale = content_w / ref_w;

    let heading_px = TITLE_REF_PX * scale;
    let label_px = ((12.0 / TITLE_REF_PX) * heading_px).max(13.0);
    let body_px = ((14.0 / TITLE_REF_PX) * heading_px).max(13.0);
    let button_px = ((15.0 / TITLE_REF_PX) * heading_px).max(14.0);
    let letter_sp = 2.0 * scale;
    let sp = |n: f32| -> f32 { n * scale };

    fn green_button(ui: &mut Ui, label: &str, font_px: f32) -> egui::Response {
        ui.add(
            egui::Button::new(
                RichText::new(label)
                    .color(START_GREEN)
                    .font(FontId::monospace(font_px)),
            )
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::new(1.0, START_GREEN)),
        )
    }

    fn green_toggle_button(ui: &mut Ui, label: &str, selected: bool, font_px: f32) -> egui::Response {
        ui.add(
            egui::Button::new(
                RichText::new(label)
                    .color(if selected { Color32::BLACK } else { START_GREEN })
                    .font(FontId::monospace(font_px)),
            )
            .fill(if selected {
                START_GREEN
            } else {
                Color32::TRANSPARENT
            })
            .stroke(Stroke::new(1.0, START_GREEN)),
        )
    }

    ui.vertical_centered(|ui| {
        ui.add_space(MARGIN);
        ui.label(
            RichText::new("NEW PROJECT")
                .font(title_font(heading_px))
                .color(START_GREEN)
                .extra_letter_spacing(letter_sp),
        );
        ui.add_space(sp(20.0));

        ui.label(
            RichText::new("PARENT LOCATION")
                .font(title_font(label_px))
                .color(START_GREEN_DIM)
                .extra_letter_spacing(letter_sp),
        );
        let parent_label = parent_dir
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(not chosen)".to_string());
        ui.label(
            RichText::new(parent_label)
                .monospace()
                .color(START_GREEN)
                .size(body_px),
        );
        ui.add_space(sp(6.0));
        if green_button(ui, "Choose location…", button_px).clicked() {
            action = CreateProjectAction::PickParentFolder;
        }

        ui.add_space(sp(16.0));
        ui.label(
            RichText::new("TARGET MCU")
                .font(title_font(label_px))
                .color(START_GREEN_DIM)
                .extra_letter_spacing(letter_sp),
        );
        ui.horizontal(|ui| {
            if green_toggle_button(ui, "ATmega328P", *model == McuModel::Atmega328P, button_px).clicked() {
                *model = McuModel::Atmega328P;
            }
            if green_toggle_button(ui, "ATmega128A", *model == McuModel::Atmega128A, button_px).clicked() {
                *model = McuModel::Atmega128A;
            }
        });

        ui.add_space(sp(12.0));
        ui.label(
            RichText::new("NAME")
                .font(title_font(label_px))
                .color(START_GREEN_DIM)
                .extra_letter_spacing(letter_sp),
        );
        ui.label(
            RichText::new("Creates a folder with this name and a matching .lain file inside.")
                .size(body_px * 0.85)
                .color(START_GREEN_DIM),
        );
        ui.add_space(sp(4.0));
        ui.add(
            egui::TextEdit::singleline(name)
                .desired_width((content_w).min(520.0))
                .font(FontId::monospace(body_px))
                .text_color(START_GREEN)
                .hint_text(RichText::new("my_project").color(START_GREEN_DIM)),
        );

        if let Some(msg) = err {
            ui.add_space(sp(8.0));
            ui.colored_label(Color32::from_rgb(255, 140, 140), msg);
        }

        ui.add_space(sp(24.0));
        ui.horizontal(|ui| {
            if green_button(ui, "Back", button_px).clicked() {
                action = CreateProjectAction::Back;
            }
            ui.add_space(sp(12.0));
            if green_button(ui, "Create", button_px).clicked() {
                action = CreateProjectAction::Submit;
            }
        });
        ui.add_space(MARGIN);
    });
    action
}
