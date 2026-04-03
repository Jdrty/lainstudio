//! Welcome screen and "create new project" form.

use std::sync::{Arc, OnceLock};

use eframe::egui::{
    self, Color32, FontFamily, FontId, Image, RichText, Stroke, Ui, Vec2,
};
use eframe::egui::text::{LayoutJob, TextFormat};

/// Matrix-style neon green (reference ~#A0F0A0).
pub const START_GREEN: Color32 = Color32::from_rgb(160, 240, 160);

pub const START_GREEN_DIM: Color32 = Color32::from_rgb(100, 180, 100);

/// Overall scale for welcome + new-project screens (**1** = smallest, **100** = largest).
/// Drives text, spacing, and banner width together.
pub const WELCOME_SIZE_PCT: u8 = 68;

/// Gap between content and the nearest screen edge (all four sides).
const MARGIN: f32 = 44.0;

/// Reference size only used for measuring; the rendered title is scaled to
/// fill the effective content width (after [`WELCOME_SIZE_PCT`]).
const TITLE_REF_PX: f32 = 54.0;

fn size_factor() -> f32 {
    (WELCOME_SIZE_PCT.clamp(1, 100) as f32) / 100.0
}

fn title_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(Arc::from("lain_title")))
}

/// Pixel width of `text` at the given font size and extra letter spacing.
fn measure_title_line(ui: &Ui, text: &str, size: f32, spacing: f32) -> f32 {
    let job = LayoutJob::single_section(
        text.to_owned(),
        TextFormat {
            font_id: title_font(size),
            color: START_GREEN,
            extra_letter_spacing: spacing,
            ..Default::default()
        },
    );
    ui.fonts(|f| f.layout_job(job).size().x)
}

fn banner_pixel_size() -> (u32, u32) {
    static DIMS: OnceLock<(u32, u32)> = OnceLock::new();
    *DIMS.get_or_init(|| {
        let bytes = include_bytes!("../assets/images/lain_banner.png");
        let img = image::load_from_memory(bytes)
            .expect("assets/images/lain_banner.png should be a valid PNG");
        (img.width(), img.height())
    })
}

fn banner_size_for_width(w: f32) -> Vec2 {
    let (pw, ph) = banner_pixel_size();
    Vec2::new(w, w * (ph as f32 / pw as f32))
}

/// Shrinks `size` uniformly if it is taller than `max_h` (keeps aspect ratio).
fn clamp_banner_height(size: Vec2, max_h: f32) -> Vec2 {
    if max_h <= 0.0 || size.y <= max_h {
        return size;
    }
    let s = max_h / size.y;
    Vec2::new(size.x * s, max_h)
}

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

pub enum WelcomeAction {
    None,
    OpenFolder,
    CreateNew,
}

pub fn show_welcome(ui: &mut Ui) -> WelcomeAction {
    let mut action = WelcomeAction::None;

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

    ui.vertical_centered(|ui| {
        ui.add_space(MARGIN);

        ui.label(
            RichText::new("LAIN STUDIO")
                .font(title_font(title_px))
                .color(START_GREEN)
                .extra_letter_spacing(letter_sp),
        );
        ui.add_space(sp(12.0));
        ui.label(
            RichText::new("CHOOSE HOW TO START")
                .font(title_font(subtitle_px))
                .color(START_GREEN_DIM)
                .extra_letter_spacing(sub_letter_sp),
        );
        ui.add_space(sp(36.0));

        if green_button(ui, "Open folder…", button_px).clicked() {
            action = WelcomeAction::OpenFolder;
        }
        ui.add_space(sp(8.0));
        if green_button(ui, "Create a new file…", button_px).clicked() {
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

pub fn show_create_project(
    ui: &mut Ui,
    parent_dir: &Option<std::path::PathBuf>,
    name: &mut String,
    err: &Option<String>,
) -> CreateProjectAction {
    let mut action = CreateProjectAction::None;

    let avail_w = ui.available_width();
    let pct = size_factor();
    let content_w = ((avail_w - 2.0 * MARGIN).max(300.0) * pct).max(200.0);

    let ref_w = measure_title_line(ui, "NEW PROJECT", TITLE_REF_PX, 2.0).max(1.0);
    let scale = content_w / ref_w;

    let heading_px = TITLE_REF_PX * scale;
    let label_px = ((12.0 / TITLE_REF_PX) * heading_px).max(13.0);
    let body_px = ((14.0 / TITLE_REF_PX) * heading_px).max(13.0);
    let button_px = ((15.0 / TITLE_REF_PX) * heading_px).max(14.0);
    let letter_sp = 2.0 * scale;
    let sp = |n: f32| -> f32 { n * scale };

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
