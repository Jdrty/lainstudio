//! virtual simulator peripherals (buttons, potentiometers) with pin validation.

use std::f32::consts::PI;
use std::fs;
use std::path::{Path, PathBuf};

use eframe::egui::{
    self, epaint, pos2, vec2, Align, Align2, Button, Color32, CornerRadius, CursorIcon, Frame,
    Layout, Margin, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2, Visuals, Window,
};
use eframe::egui::scroll_area::ScrollBarVisibility;
use eframe::egui::style::WidgetVisuals;

use serde::{Deserialize, Serialize};

use crate::avr::cpu::Cpu;
use crate::avr::McuModel;
use crate::theme;
const ERR_RED_SOFT: Color32 = Color32::from_rgb(200, 120, 120);

/// stored under each project folder
const PERIPHERALS_REL_PATH: &str = ".full_metal/peripherals.json";

#[derive(Serialize, Deserialize)]
struct PeripheralFile {
    version: u32,
    next_id: u64,
    items:   Vec<PeripheralEntry>,
}

pub fn peripherals_path_for_project(root: &Path) -> PathBuf {
    root.join(PERIPHERALS_REL_PATH)
}

pub fn load_peripherals_from_disk(root: &Path) -> PeripheralState {
    let path = peripherals_path_for_project(root);
    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(_) => return PeripheralState::default(),
    };
    let parsed: PeripheralFile = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(_) => return PeripheralState::default(),
    };
    if parsed.version != 1 {
        return PeripheralState::default();
    }
    let mut items = parsed.items;
    for e in &mut items {
        e.pressed = false;
    }
    let max_id = items.iter().map(|e| e.id).max().unwrap_or(0);
    let next_id = parsed.next_id.max(max_id.saturating_add(1)).max(1);
    PeripheralState {
        items,
        add_dialog_open: false,
        add_kind: PeripheralKind::Button,
        add_port: 'B',
        add_bit: 0,
        add_error: None,
        next_id,
        dialog_visuals_backup: None,
        needs_save: false,
    }
}

fn save_peripherals_to_disk(root: &Path, state: &PeripheralState) -> Result<(), String> {
    let path = peripherals_path_for_project(root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let file = PeripheralFile {
        version: 1,
        next_id: state.next_id,
        items:   state.items.clone(),
    };
    let json = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}

fn reorder_peripherals(items: &mut Vec<PeripheralEntry>, from: usize, to: usize) {
    if from == to || from >= items.len() || to >= items.len() {
        return;
    }
    let mut i = from;
    if i < to {
        while i < to {
            items.swap(i, i + 1);
            i += 1;
        }
    } else {
        while i > to {
            items.swap(i, i - 1);
            i -= 1;
        }
    }
}

pub(crate) fn persist_peripherals_if_needed(state: &mut PeripheralState, project_root: Option<&Path>) {
    if let Some(root) = project_root {
        if state.needs_save {
            match save_peripherals_to_disk(root, state) {
                Ok(()) => state.needs_save = false,
                Err(e) => eprintln!("peripherals save: {e}"),
            }
        }
    }
}

const ADD_CIRCLE_FILL: Color32 = Color32::from_rgb(22, 26, 36);
const ADD_CIRCLE_RIM: Color32 = Color32::from_rgb(55, 62, 78);
const ADD_CIRCLE_INNER: Color32 = Color32::from_rgb(16, 19, 28);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PeripheralKind {
    Button,
    Potentiometer,
}

impl PeripheralKind {
    fn label(self) -> &'static str {
        match self {
            Self::Button => "Button",
            Self::Potentiometer => "Potentiometer",
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PeripheralEntry {
    pub id:        u64,
    pub kind:      PeripheralKind,
    pub port:      char,
    pub bit:       u8,
    pub pressed:   bool,
    pub pot_volts: f32,
}

#[derive(Clone)]
pub struct PeripheralState {
    pub items:           Vec<PeripheralEntry>,
    pub add_dialog_open: bool,
    pub add_kind:        PeripheralKind,
    pub add_port:        char,
    pub add_bit:         u8,
    pub add_error:       Option<String>,
    next_id:             u64,
    dialog_visuals_backup: Option<Visuals>,
    pub(crate) needs_save: bool,
}

impl Default for PeripheralState {
    fn default() -> Self {
        Self {
            items:           Vec::new(),
            add_dialog_open: false,
            add_kind:        PeripheralKind::Button,
            add_port:        'B',
            add_bit:         0,
            add_error:       None,
            next_id:         1,
            dialog_visuals_backup: None,
            needs_save:      false,
        }
    }
}

impl PeripheralState {
    pub fn pin_occupancy(&self) -> Vec<(char, u8)> {
        self.items.iter().map(|e| (e.port, e.bit)).collect()
    }
}

pub fn on_peripherals_panel_hidden(
    state: &mut PeripheralState,
    ctx: &egui::Context,
    project_root: Option<&Path>,
) {
    state.add_dialog_open = false;
    if let Some(v) = state.dialog_visuals_backup.take() {
        ctx.style_mut(|s| s.visuals = v);
    }
    persist_peripherals_if_needed(state, project_root);
}

fn xmem_portc_pins(size: u32) -> u8 {
    if size <= 256 {
        return 0;
    }
    let bits = 32u32.saturating_sub((size - 1).leading_zeros());
    bits.saturating_sub(8).min(8) as u8
}

fn xmem_claims_pin(cpu: &Cpu, port: char, bit: u8) -> bool {
    let xmem_active = cpu.has_xmem() && !cpu.xmem.is_empty();
    if !xmem_active {
        return false;
    }
    let sz = cpu.xmem.len() as u32;
    let n = xmem_portc_pins(sz);
    let portc_mask: u8 = if n >= 8 { 0xFF } else { (1u8 << n).wrapping_sub(1) };
    match port {
        'A' => true,
        'C' => n > 0 && (portc_mask & (1u8 << bit)) != 0,
        'G' => bit < 3,
        _ => false,
    }
}

pub fn adc_channel_for_pot_pin(model: McuModel, port: char, bit: u8) -> Option<u8> {
    match model {
        McuModel::Atmega328P => {
            if port == 'C' && bit <= 5 {
                Some(bit)
            } else {
                None
            }
        }
        McuModel::Atmega128A => {
            if port == 'F' && bit <= 7 {
                Some(bit)
            } else {
                None
            }
        }
    }
}

fn port_exists_on_model(model: McuModel, port: char) -> bool {
    match model {
        McuModel::Atmega328P => matches!(port, 'B' | 'C' | 'D'),
        McuModel::Atmega128A => matches!(port, 'A' | 'B' | 'C' | 'D' | 'E' | 'F' | 'G'),
    }
}

fn default_ports(kind: PeripheralKind, model: McuModel) -> &'static [char] {
    match (kind, model) {
        (PeripheralKind::Potentiometer, McuModel::Atmega328P) => &['C'],
        (PeripheralKind::Potentiometer, McuModel::Atmega128A) => &['F'],
        (_, McuModel::Atmega328P) => &['B', 'C', 'D'],
        (_, McuModel::Atmega128A) => &['A', 'B', 'C', 'D', 'E', 'F', 'G'],
    }
}

pub fn validate_placement(
    model: McuModel,
    cpu: &Cpu,
    kind: PeripheralKind,
    port: char,
    bit: u8,
    existing: &[(char, u8)],
) -> Result<(), String> {
    if bit > 7 {
        return Err("Pin must be 0–7.".to_string());
    }
    if !port_exists_on_model(model, port) {
        return Err("That port does not exist on this MCU.".to_string());
    }
    match kind {
        PeripheralKind::Potentiometer => {
            if adc_channel_for_pot_pin(model, port, bit).is_none() {
                return Err(
                    "Potentiometer must use an ADC pin: ATmega328P PC0–PC5; ATmega128A PF0–PF7."
                        .to_string(),
                );
            }
        }
        PeripheralKind::Button => {}
    }
    if xmem_claims_pin(cpu, port, bit) {
        return Err("That pin is used by external memory (XMEM).".to_string());
    }
    for &(p, b) in existing {
        if p == port && b == bit {
            return Err("Another peripheral already uses this pin.".to_string());
        }
    }
    Ok(())
}

fn pin_addr_for_cpu(cpu: &Cpu, port: char) -> Option<u16> {
    for (name, _, _, pin_addr) in cpu.gpio_ports() {
        if name.chars().next() == Some(port) {
            return Some(*pin_addr);
        }
    }
    None
}

pub fn apply_peripherals_to_cpu(state: &PeripheralState, cpu: &mut Cpu) {
    for e in &state.items {
        match e.kind {
            PeripheralKind::Button => {
                if let Some(addr) = pin_addr_for_cpu(cpu, e.port) {
                    // Tactile switch to GND + internal pull-up: released → pin high, pressed → low.
                    cpu.add_pin_input_override(addr, e.bit, !e.pressed);
                }
            }
            PeripheralKind::Potentiometer => {
                if let Some(ch) = adc_channel_for_pot_pin(cpu.model, e.port, e.bit) {
                    let mv = (e.pot_volts.clamp(0.0, 5.0) * 1000.0) as u32;
                    cpu.set_adc_channel_mv(ch, mv);
                }
            }
        }
    }
}

fn tint(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

fn paint_glowing_plus(painter: &egui::Painter, center: Pos2, half: f32) {
    let layers: [(f32, u8); 4] = [(4.0, 18), (2.5, 45), (1.2, 110), (0.0, 255)];
    for (spread, alpha) in layers {
        let w = 2.2 + spread * 0.15;
        let c = Color32::from_rgba_unmultiplied(255, 255, 255, alpha);
        let s = Stroke::new(w, c);
        painter.line_segment(
            [pos2(center.x - half, center.y), pos2(center.x + half, center.y)],
            s,
        );
        painter.line_segment(
            [pos2(center.x, center.y - half), pos2(center.x, center.y + half)],
            s,
        );
    }
}

/// Pure paint + hit area — no text widget (nothing to select as text).
fn circle_add_button(ui: &mut Ui, state: &mut PeripheralState) -> egui::Response {
    let size = 30.0_f32;
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    if response.clicked() {
        state.add_error = None;
        state.add_dialog_open = true;
    }
    let p = ui.painter_at(rect);
    let c = rect.center();
    let r = size * 0.46;

    p.circle_filled(c, r + 1.0, Color32::from_rgba_unmultiplied(0, 0, 0, 45));
    p.circle_filled(c, r, ADD_CIRCLE_FILL);
    p.circle_stroke(c, r, Stroke::new(1.0, ADD_CIRCLE_RIM));
    p.circle_stroke(c, r - 2.0, Stroke::new(0.65, ADD_CIRCLE_INNER));
    if response.hovered() {
        p.circle_stroke(
            c,
            r + 1.5,
            Stroke::new(
                1.0,
                Color32::from_rgba_unmultiplied(theme::FOCUS.r(), theme::FOCUS.g(), theme::FOCUS.b(), 90),
            ),
        );
    }
    p.line_segment(
        [
            pos2(c.x - r * 0.52, c.y - r * 0.62),
            pos2(c.x + r * 0.52, c.y - r * 0.62),
        ],
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 42)),
    );
    paint_glowing_plus(&p, c, r * 0.3);

    response
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text("Add peripheral")
}

fn add_peripheral_chip(ui: &mut Ui, state: &mut PeripheralState) {
    Frame::NONE
        .fill(theme::SIM_SURFACE)
        .stroke(Stroke::new(0.75, theme::SIM_BORDER))
        .corner_radius(CornerRadius::same(20))
        .inner_margin(Margin::same(5))
        .show(ui, |ui| {
            circle_add_button(ui, state);
        });
}

fn paint_tactile_button_icon(painter: &egui::Painter, rect: Rect, pressed: bool) {
    let off = if pressed { 1.0 } else { 0.0 };
    let body = rect.shrink2(vec2(4.0, 5.0));
    let face = body.translate(vec2(0.0, off));
    let plunger = face.shrink2(vec2(10.0, 8.0)).translate(vec2(0.0, 1.0 + off * 0.5));

    let shadow = Color32::from_rgb(12, 14, 20);
    let bezel = Color32::from_rgb(28, 32, 42);
    let face_col = Color32::from_rgb(38, 44, 56);
    let plunger_top = Color32::from_rgb(58, 64, 78);
    let plunger_bot = Color32::from_rgb(32, 36, 46);
    let hi = Color32::from_rgba_unmultiplied(255, 255, 255, 40);

    painter.rect_filled(rect, CornerRadius::same(6), shadow);
    painter.rect_filled(body, CornerRadius::same(5), bezel);
    painter.rect_filled(face, CornerRadius::same(4), face_col);
    painter.line_segment(
        [face.left_top() + vec2(4.0, 3.0), face.right_top() + vec2(-4.0, 3.0)],
        Stroke::new(1.0, hi),
    );
    painter.rect_filled(plunger, CornerRadius::same(3), plunger_bot);
    let plunger_inset = plunger.shrink(1.0);
    painter.rect_filled(plunger_inset, CornerRadius::same(2), plunger_top);
    if pressed {
        painter.circle_filled(plunger.center(), 2.8, Color32::from_rgba_unmultiplied(255, 255, 255, 25));
    } else {
        painter.circle_filled(plunger.center() + vec2(-0.8, -0.8), 2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 70));
    }
}

fn paint_panel_pot_icon(painter: &egui::Painter, rect: Rect, t: f32) {
    let t = t.clamp(0.0, 1.0);
    let c = rect.center();
    let r_outer = rect.width().min(rect.height()) * 0.42;
    let r_scale = r_outer * 0.92;
    let r_knob = r_outer * 0.55;

    painter.circle_filled(c, r_outer + 3.0, Color32::from_rgb(18, 20, 26));
    painter.circle_filled(c, r_outer + 1.5, Color32::from_rgb(36, 40, 50));
    painter.circle_stroke(c, r_outer + 1.5, Stroke::new(1.0, Color32::from_rgb(58, 64, 78)));

    let start = -0.85 * PI;
    let end = 0.85 * PI;
    let steps = 40;
    let mut prev = pos2(
        c.x + r_scale * start.cos(),
        c.y - r_scale * start.sin(),
    );
    for i in 1..=steps {
        let u = i as f32 / steps as f32;
        let a = start + (end - start) * u;
        let pt = pos2(c.x + r_scale * a.cos(), c.y - r_scale * a.sin());
        painter.line_segment([prev, pt], Stroke::new(1.8, Color32::from_rgb(52, 58, 72)));
        prev = pt;
    }

    for k in 0..=8 {
        let u = k as f32 / 8.0;
        let a = start + (end - start) * u;
        let r1 = r_scale - 2.0;
        let r2 = r_scale - 7.0;
        painter.line_segment(
            [
                pos2(c.x + r1 * a.cos(), c.y - r1 * a.sin()),
                pos2(c.x + r2 * a.cos(), c.y - r2 * a.sin()),
            ],
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(200, 210, 230, 90)),
        );
    }

    for i in 0..28 {
        let a = (i as f32 / 28.0) * TAU;
        let r0 = r_knob + 4.0;
        let r1 = r_knob + 7.5;
        painter.line_segment(
            [
                pos2(c.x + r0 * a.cos(), c.y - r0 * a.sin()),
                pos2(c.x + r1 * a.cos(), c.y - r1 * a.sin()),
            ],
            Stroke::new(1.0, Color32::from_rgb(70, 76, 90)),
        );
    }

    let ang = start + (end - start) * t;
    painter.circle_filled(c, r_knob, Color32::from_rgb(48, 52, 64));
    painter.circle_stroke(c, r_knob, Stroke::new(1.0, Color32::from_rgb(88, 94, 110)));
    painter.circle_filled(c, r_knob - 2.0, Color32::from_rgb(62, 68, 82));
    painter.line_segment(
        [
            pos2(c.x - r_knob * 0.45, c.y - r_knob * 0.35),
            pos2(c.x - r_knob * 0.1, c.y - r_knob * 0.55),
        ],
        Stroke::new(1.2, Color32::from_rgba_unmultiplied(255, 255, 255, 55)),
    );
    let tip = r_knob * 0.72;
    painter.line_segment(
        [
            c,
            pos2(c.x + tip * ang.cos(), c.y - tip * ang.sin()),
        ],
        Stroke::new(2.2, theme::ACCENT),
    );

    let lug_y = c.y + r_outer * 0.75;
    for dx in [-7.0_f32, 0.0, 7.0] {
        let lug = pos2(c.x + dx, lug_y);
        painter.rect_filled(
            Rect::from_center_size(lug, vec2(3.5, 2.2)),
            CornerRadius::ZERO,
            Color32::from_rgb(180, 165, 120),
        );
    }
}

const TAU: f32 = std::f32::consts::TAU;

fn interactive_pot_knob(ui: &mut Ui, volts: &mut f32) -> egui::Response {
    let icon_side = 86.0_f32;
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(icon_side), Sense::click_and_drag());
    let center = rect.center();
    let start = -0.85 * PI;
    let end = 0.85 * PI;

    if response.dragged() || response.drag_started() {
        if let Some(pos) = response.interact_pointer_pos() {
            let dx = pos.x - center.x;
            let dy = pos.y - center.y;
            let a = (-dy).atan2(dx);
            let a = a.clamp(start, end);
            let t = (a - start) / (end - start);
            *volts = t * 5.0;
        }
    }

    let p = ui.painter_at(rect);
    let t_norm = (*volts / 5.0).clamp(0.0, 1.0);
    paint_panel_pot_icon(&p, rect, t_norm);
    if response.hovered() || response.dragged() {
        p.circle_stroke(
            center,
            icon_side * 0.48 + 3.0,
            Stroke::new(
                1.0,
                Color32::from_rgba_unmultiplied(theme::FOCUS.r(), theme::FOCUS.g(), theme::FOCUS.b(), 100),
            ),
        );
    }

    response
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text("Drag on the knob around the arc to set 0–5 V (or use the slider below)")
}

fn apply_add_dialog_visuals(ui: &mut Ui) {
    let v = ui.visuals_mut();
    let corner = CornerRadius::same(6);
    let stroke_menu = Stroke::new(1.0, theme::SIM_BORDER);
    let stroke_hi = Stroke::new(1.0, theme::SIM_BORDER_BRIGHT);

    v.window_fill = theme::SIM_SURFACE_LIFT;
    v.window_stroke = stroke_menu;
    v.menu_corner_radius = corner;
    v.extreme_bg_color = theme::SIM_SURFACE_LIFT;
    v.popup_shadow = epaint::Shadow {
        offset: [0, 4],
        blur: 18,
        spread: 0,
        color: Color32::from_rgba_unmultiplied(0, 0, 0, 110),
    };

    v.selection.bg_fill = theme::SIM_TAB_ACTIVE;
    // `interact_selectable` uses `selection.stroke` as fg for the selected row.
    v.selection.stroke = Stroke::new(1.0, theme::ACCENT);

    let set_w = |w: &mut WidgetVisuals, fill: Color32, fg: Color32, strk: Stroke| {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = strk;
        w.fg_stroke = Stroke::new(1.0, fg);
        w.corner_radius = corner;
        w.expansion = 0.0;
    };

    set_w(&mut v.widgets.inactive, theme::SIM_SURFACE, theme::ACCENT_DIM, stroke_menu);
    set_w(&mut v.widgets.hovered, theme::SIM_TAB_ACTIVE, theme::ACCENT, stroke_hi);
    set_w(&mut v.widgets.active, theme::SIM_TAB_ACTIVE, theme::ACCENT, stroke_hi);
    set_w(&mut v.widgets.open, theme::SIM_SURFACE_LIFT, theme::ACCENT, stroke_hi);
    set_w(
        &mut v.widgets.noninteractive,
        theme::SIM_SURFACE_LIFT,
        theme::ACCENT_DIM,
        stroke_menu,
    );
}

fn angelic_dialog_button(ui: &mut Ui, text: &str, primary: bool) -> egui::Response {
    let fill = if primary {
        tint(theme::ACCENT, 28)
    } else {
        theme::SIM_SURFACE
    };
    let stroke = if primary {
        theme::SIM_BORDER_BRIGHT
    } else {
        theme::SIM_BORDER
    };
    ui.add(
        Button::new(RichText::new(text).monospace().size(12.5).color(theme::ACCENT))
            .fill(fill)
            .stroke(Stroke::new(1.0, stroke))
            .corner_radius(CornerRadius::same(6))
            .min_size(Vec2::new(88.0, 28.0)),
    )
}

pub fn show_peripherals_panel(
    ui: &mut Ui,
    state: &mut PeripheralState,
    model: McuModel,
    cpu: &Cpu,
    project_root: Option<&Path>,
) {
    Frame::NONE
        .fill(theme::PANEL_DEEP)
        .stroke(Stroke::new(0.75, theme::SIM_BORDER))
        .inner_margin(Margin {
            left:   5,
            right:  10,
            top:    10,
            bottom: 10,
        })
        .show(ui, |ui| {
            let panel_w = ui.available_width();
            if panel_w.is_finite() {
                ui.set_max_width(panel_w);
            }
            ui.style_mut().interaction.selectable_labels = false;

            ui.vertical(|ui| {
                ui.label(
                    RichText::new("Peripherals")
                        .monospace()
                        .size(15.0)
                        .color(theme::ACCENT),
                );
                ui.add_space(3.0);
                ui.label(
                    RichText::new(
                        "Virtual GPIO and ADC for the simulator — tie inputs to a port pin. Drag ≡ to reorder.",
                    )
                    .monospace()
                    .size(10.5)
                    .line_height(Some(14.0))
                    .color(theme::ACCENT_DIM),
                );
            });

            ui.add_space(12.0);

            const ADD_FOOTER_H: f32 = 52.0;
            let avail_h = ui.available_height();
            let scroll_h = if avail_h.is_finite() && avail_h > ADD_FOOTER_H + 24.0 {
                (avail_h - ADD_FOOTER_H - 8.0).clamp(120.0, 4000.0)
            } else if avail_h.is_finite() && avail_h > 8.0 {
                (avail_h - 4.0).clamp(120.0, 4000.0)
            } else {
                220.0
            };
            let scroll_w = ui.available_width();
            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden)
                .id_salt("periph_list")
                .max_width(scroll_w)
                .max_height(scroll_h)
                .show(ui, |ui| {
                    let row_max_w = ui.available_width().min(scroll_w);
                    let row_max_w = if row_max_w.is_finite() {
                        row_max_w
                    } else {
                        320.0
                    };
                    let mut remove: Option<u64> = None;
                    let mut pending_reorder: Option<(u64, usize)> = None;
                    for (idx, e) in state.items.iter_mut().enumerate() {
                        let dropped = ui
                            .allocate_ui_with_layout(
                                vec2(row_max_w, 0.0),
                                Layout::top_down(Align::Min),
                                |ui| {
                                    ui.set_min_width(row_max_w);
                                    ui.set_max_width(row_max_w);
                                    let (_, dropped) = ui.dnd_drop_zone::<u64, _>(
                                        Frame::NONE.inner_margin(Margin::ZERO),
                                        |ui| {
                                Frame::NONE
                                    .fill(theme::SIM_SURFACE)
                                    .stroke(Stroke::new(0.75, theme::SIM_BORDER))
                                    .inner_margin(Margin {
                                        left:   4,
                                        right:  8,
                                        top:    8,
                                        bottom: 8,
                                    })
                                    .corner_radius(CornerRadius::same(8))
                                    .show(ui, |ui| {
                                        ui.set_max_width(row_max_w);
                                        ui.horizontal_top(|ui| {
                                            const DRAG_STRIP_W: f32 = 22.0;
                                            const DRAG_GAP: f32 = 4.0;
                                            let row_avail = ui.available_width();
                                            let left_w =
                                                (row_avail - DRAG_STRIP_W - DRAG_GAP).max(0.0);
                                            ui.allocate_ui_with_layout(
                                                vec2(left_w, 0.0),
                                                Layout::top_down(Align::Min),
                                                |ui| {
                                                    ui.set_max_width(left_w);
                                                    ui.set_min_width(0.0);
                                                    ui.vertical(|ui| {
                                                        ui.horizontal(|ui| {
                                                            ui.label(
                                                                RichText::new(format!(
                                                                    "{} · P{}{}",
                                                                    e.kind.label(),
                                                                    e.port,
                                                                    e.bit
                                                                ))
                                                                .monospace()
                                                                .size(11.5)
                                                                .color(theme::ACCENT_DIM),
                                                            );
                                                            ui.with_layout(
                                                                Layout::right_to_left(
                                                                    egui::Align::Center,
                                                                ),
                                                                |ui| {
                                                                    if ui
                                                                        .small_button(
                                                                            RichText::new(
                                                                                "Remove",
                                                                            )
                                                                            .monospace()
                                                                            .size(10.5)
                                                                            .color(ERR_RED_SOFT),
                                                                        )
                                                                        .clicked()
                                                                    {
                                                                        remove = Some(e.id);
                                                                    }
                                                                },
                                                            );
                                                        });
                                                        ui.add_space(8.0);

                                                        match e.kind {
                                                            PeripheralKind::Button => {
                                                                ui.horizontal(|ui| {
                                                                    let icon_w = 52.0;
                                                                    let (icon_rect, _) =
                                                                        ui.allocate_exact_size(
                                                                            Vec2::new(
                                                                                icon_w,
                                                                                44.0,
                                                                            ),
                                                                            Sense::hover(),
                                                                        );
                                                                    let p =
                                                                        ui.painter_at(icon_rect);
                                                                    paint_tactile_button_icon(
                                                                        &p,
                                                                        icon_rect,
                                                                        e.pressed,
                                                                    );

                                                                    let fill = if e.pressed {
                                                                        tint(theme::ACCENT, 55)
                                                                    } else {
                                                                        tint(theme::ACCENT, 18)
                                                                    };
                                                                    let stroke = if e.pressed {
                                                                        theme::ACCENT
                                                                    } else {
                                                                        theme::SIM_BORDER_BRIGHT
                                                                    };
                                                                    let b = ui.add(
                                                                        Button::new(
                                                                            RichText::new(
                                                                                "Hold to press",
                                                                            )
                                                                            .monospace()
                                                                            .size(12.5)
                                                                            .color(theme::ACCENT),
                                                                        )
                                                                        .fill(fill)
                                                                        .stroke(Stroke::new(
                                                                            1.0,
                                                                            stroke,
                                                                        ))
                                                                        .corner_radius(
                                                                            CornerRadius::same(8),
                                                                        ),
                                                                    );
                                                                    e.pressed =
                                                                        b.is_pointer_button_down_on();
                                                                });
                                                            }
                                                            PeripheralKind::Potentiometer => {
                                                                ui.vertical(|ui| {
                                                                    let knob_resp =
                                                                        interactive_pot_knob(
                                                                            ui,
                                                                            &mut e.pot_volts,
                                                                        );
                                                                    ui.add_space(4.0);
                                                                    ui.label(
                                                                        RichText::new(
                                                                            "Drag on the knob · or slider below",
                                                                        )
                                                                        .monospace()
                                                                        .size(9.5)
                                                                        .color(theme::ACCENT_DIM),
                                                                    );
                                                                    ui.add_space(6.0);
                                                                    ui.label(
                                                                        RichText::new(format!(
                                                                            "{:.2} V",
                                                                            e.pot_volts
                                                                        ))
                                                                        .monospace()
                                                                        .size(18.0)
                                                                        .color(theme::ACCENT),
                                                                    );
                                                                    ui.add_space(2.0);
                                                                    ui.label(
                                                                        RichText::new(
                                                                            "Analog 0–5 V",
                                                                        )
                                                                        .monospace()
                                                                        .size(10.0)
                                                                        .color(theme::ACCENT_DIM),
                                                                    );
                                                                    ui.add_space(8.0);
                                                                    let slider_w = ui
                                                                        .available_width()
                                                                        .max(80.0);
                                                                    let slider_resp = ui.add_sized(
                                                                        [slider_w, 18.0],
                                                                        egui::Slider::new(
                                                                            &mut e.pot_volts,
                                                                            0.0..=5.0,
                                                                        )
                                                                        .show_value(false),
                                                                    );
                                                                    if knob_resp.drag_stopped()
                                                                        || slider_resp.changed()
                                                                    {
                                                                        state.needs_save = true;
                                                                    }
                                                                });
                                                            }
                                                        }
                                                    });
                                                },
                                            );
                                            ui.add_space(DRAG_GAP);
                                            ui.allocate_ui_with_layout(
                                                vec2(DRAG_STRIP_W, 0.0),
                                                Layout::top_down(Align::Min),
                                                |ui| {
                                                    ui.set_min_width(DRAG_STRIP_W);
                                                    ui.set_max_width(DRAG_STRIP_W);
                                                    Frame::NONE
                                                        .fill(theme::PANEL_DEEP)
                                                        .stroke(Stroke::new(
                                                            0.75,
                                                            theme::SIM_BORDER,
                                                        ))
                                                        .inner_margin(Margin::symmetric(4, 6))
                                                        .corner_radius(CornerRadius::same(4))
                                                        .show(ui, |ui| {
                                                            let drag_id = ui
                                                                .id()
                                                                .with("periph_drag")
                                                                .with(e.id);
                                                            ui.dnd_drag_source(
                                                                drag_id,
                                                                e.id,
                                                                |ui| {
                                                                    ui.label(
                                                                        RichText::new("≡")
                                                                            .monospace()
                                                                            .size(16.0)
                                                                            .color(
                                                                                theme::ACCENT_DIM,
                                                                            ),
                                                                    );
                                                                },
                                                            );
                                                        });
                                                },
                                            );
                                        });
                                    });
                                        },
                                    );
                                    dropped
                                },
                            )
                            .inner;
                        if let Some(arc) = dropped {
                            pending_reorder = Some((*arc, idx));
                        }
                        ui.add_space(10.0);
                    }
                    if let Some(id) = remove {
                        state.items.retain(|x| x.id != id);
                        state.needs_save = true;
                    }
                    if let Some((from_id, to_idx)) = pending_reorder {
                        if let Some(from_idx) = state.items.iter().position(|x| x.id == from_id) {
                            if from_idx != to_idx {
                                reorder_peripherals(&mut state.items, from_idx, to_idx);
                                state.needs_save = true;
                            }
                        }
                    }

                    if state.items.is_empty() {
                        ui.add_space(24.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                RichText::new("No peripherals yet")
                                    .monospace()
                                    .size(12.0)
                                    .color(theme::ACCENT_DIM),
                            );
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new("Use the round add control at the bottom right of this panel to add a button or potentiometer.")
                                    .monospace()
                                    .size(10.5)
                                    .color(Color32::from_rgba_unmultiplied(125, 135, 158, 200)),
                            );
                        });
                    }
                });

            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(12.0);
                add_peripheral_chip(ui, state);
            });
        });

    if state.add_dialog_open {
        if state.dialog_visuals_backup.is_none() {
            state.dialog_visuals_backup = Some(ui.ctx().style().visuals.clone());
        }
        Window::new("add_peripheral_dlg")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .frame(
                Frame::NONE
                    .fill(theme::SIM_SURFACE_LIFT)
                    .stroke(Stroke::new(1.0, theme::SIM_BORDER_BRIGHT))
                    .inner_margin(Margin::symmetric(20, 18))
                    .corner_radius(CornerRadius::same(12)),
            )
            .show(ui.ctx(), |ui| {
                apply_add_dialog_visuals(ui);
                ui.set_min_width(300.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("Add peripheral")
                            .monospace()
                            .size(16.0)
                            .color(theme::ACCENT),
                    );
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new("Choose type and pin. Pots use ADC-capable pins only.")
                            .monospace()
                            .size(10.5)
                            .color(theme::ACCENT_DIM),
                    );
                });
                ui.add_space(14.0);

                ui.label(
                    RichText::new("Type")
                        .monospace()
                        .size(10.5)
                        .color(theme::ACCENT_DIM),
                );
                ui.add_space(4.0);
                egui::ComboBox::from_id_salt("pk_kind")
                    .selected_text(
                        RichText::new(state.add_kind.label())
                            .monospace()
                            .size(12.0)
                            .color(theme::ACCENT),
                    )
                    .width(ui.available_width())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut state.add_kind, PeripheralKind::Button, "Button");
                        ui.selectable_value(
                            &mut state.add_kind,
                            PeripheralKind::Potentiometer,
                            "Potentiometer",
                        );
                    });

                let ports = default_ports(state.add_kind, model);
                if !ports.contains(&state.add_port) {
                    state.add_port = *ports.first().unwrap_or(&'B');
                }

                ui.add_space(10.0);
                ui.columns(2, |cols| {
                    cols[0].label(
                        RichText::new("Port")
                            .monospace()
                            .size(10.5)
                            .color(theme::ACCENT_DIM),
                    );
                    cols[1].label(
                        RichText::new("Pin")
                            .monospace()
                            .size(10.5)
                            .color(theme::ACCENT_DIM),
                    );
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("pk_port")
                        .selected_text(
                            RichText::new(state.add_port.to_string())
                                .monospace()
                                .size(12.0)
                                .color(theme::ACCENT),
                        )
                        .width(ui.available_width() * 0.48)
                        .show_ui(ui, |ui| {
                            for &p in ports {
                                ui.selectable_value(&mut state.add_port, p, p.to_string());
                            }
                        });
                    ui.add_space(8.0);
                    egui::ComboBox::from_id_salt("pk_bit")
                        .selected_text(
                            RichText::new(format!("{}", state.add_bit))
                                .monospace()
                                .size(12.0)
                                .color(theme::ACCENT),
                        )
                        .width(ui.available_width())
                        .show_ui(ui, |ui| {
                            let max_bit = match state.add_kind {
                                PeripheralKind::Potentiometer if model == McuModel::Atmega328P => 5u8,
                                _ => 7u8,
                            };
                            for b in 0..=max_bit {
                                ui.selectable_value(&mut state.add_bit, b, format!("{b}"));
                            }
                        });
                });

                if let Some(ref err) = state.add_error {
                    ui.add_space(8.0);
                    ui.label(RichText::new(err).monospace().size(10.5).color(ERR_RED_SOFT));
                }

                ui.add_space(18.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        if angelic_dialog_button(ui, "Add", true).clicked() {
                            let occ: Vec<(char, u8)> = state.pin_occupancy();
                            match validate_placement(
                                model,
                                cpu,
                                state.add_kind,
                                state.add_port,
                                state.add_bit,
                                &occ,
                            ) {
                                Ok(()) => {
                                    state.items.push(PeripheralEntry {
                                        id: state.next_id,
                                        kind: state.add_kind,
                                        port: state.add_port,
                                        bit: state.add_bit,
                                        pressed: false,
                                        pot_volts: 2.5,
                                    });
                                    state.next_id += 1;
                                    state.add_dialog_open = false;
                                    state.add_error = None;
                                    state.needs_save = true;
                                }
                                Err(e) => state.add_error = Some(e),
                            }
                        }
                        ui.add_space(8.0);
                        if angelic_dialog_button(ui, "Cancel", false).clicked() {
                            state.add_dialog_open = false;
                            state.add_error = None;
                        }
                    });
                });
            });
    } else if let Some(v) = state.dialog_visuals_backup.take() {
        ui.ctx().style_mut(|s| s.visuals = v);
    }

    persist_peripherals_if_needed(state, project_root);
}
