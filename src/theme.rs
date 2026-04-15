//! ui palette
//  ill be honest, this is inspired by kuronami base variant in valorant..
//  if it looked terrible, one day ill be good at ui design
use eframe::egui::Color32;

// main stuff
/// primary accent: bright white with a little blue
pub const ACCENT: Color32 = Color32::from_rgb(248, 250, 255);
/// secondary labels, dim strokes
pub const ACCENT_DIM: Color32 = Color32::from_rgb(125, 135, 158);

/// toolbar / menu chrome (alias of accent palette)
pub const START_GREEN: Color32 = ACCENT;
pub const START_GREEN_DIM: Color32 = ACCENT_DIM;

/// strong emphasis: ice blue
pub const FOCUS: Color32 = Color32::from_rgb(175, 205, 255);

/// syntax: immediate / numeric literals
pub const LITERAL_NUM: Color32 = Color32::from_rgb(195, 210, 245);

pub const LABEL_CYAN: Color32 = Color32::from_rgb(120, 210, 255);

pub const ERR_RED: Color32 = Color32::from_rgb(255, 100, 100);
pub const DIM_GRAY: Color32 = Color32::from_rgb(65, 65, 72);

/// docs section headers
pub const SECTION: Color32 = Color32::from_rgb(150, 185, 220);

// application panels (depth / layering)
/// deepest panel fill (main editor surround, side panels)
pub const PANEL_DEEP: Color32 = Color32::from_rgb(4, 6, 12);
/// mid toolbar strip, menus.
pub const PANEL_MID: Color32 = Color32::from_rgb(7, 10, 18);
/// slightly lifted surface (file tabs bar, modal chrome).
pub const PANEL_LIFT: Color32 = Color32::from_rgb(12, 16, 26);

/// Strong button / title-bar slab (dialogs, primary actions).
pub const BUTTON_FILL_STRONG: Color32 = Color32::from_rgb(14, 18, 30);

pub const DISABLED_PANEL: Color32 = Color32::from_rgb(22, 24, 30);

// editor search
/// empty buffer watermark
pub const EDITOR_PLACEHOLDER: Color32 = Color32::from_rgb(72, 78, 90);

pub const SEARCH_BG: Color32 = Color32::from_rgb(4, 6, 14);
pub const MATCH_DIM: Color32 = Color32::from_rgb(90, 110, 150);
pub const MATCH_CUR: Color32 = Color32::from_rgb(210, 225, 255);

// simulator panel (tabs, borders)
pub const SIM_SURFACE: Color32 = Color32::from_rgb(9, 12, 22);
pub const SIM_SURFACE_LIFT: Color32 = Color32::from_rgb(15, 19, 32);
pub const SIM_TAB_ACTIVE: Color32 = Color32::from_rgb(13, 17, 30);

pub const SIM_BORDER: Color32 = Color32::from_rgb(68, 78, 98);
pub const SIM_BORDER_BRIGHT: Color32 = Color32::from_rgb(155, 168, 198);

pub const SIM_STOP_FILL: Color32 = Color32::from_rgb(22, 15, 22);
pub const SIM_STOP_BORDER: Color32 = Color32::from_rgb(118, 95, 112);
