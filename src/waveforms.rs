//! waveform traces (GPR or GPIO pin) vs simulated time from cycle counter

use std::collections::HashMap;

use eframe::egui::{
    self, epaint, pos2, vec2, Align, Align2, Button, Color32, CornerRadius, CursorIcon, Frame,
    Id, Layout, Margin, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, Vec2, Visuals,
    Window,
};
use eframe::egui::style::HandleShape;

use crate::avr::cpu::Cpu;
use crate::avr::McuModel;
use crate::sim_panel::{IpsUnit, SpeedLimitState};
use crate::theme;
use crate::theme::START_GREEN;

const ADD_CIRCLE_FILL: Color32 = Color32::from_rgb(22, 26, 36);
const ADD_CIRCLE_RIM: Color32 = Color32::from_rgb(55, 62, 78);
const ADD_CIRCLE_INNER: Color32 = Color32::from_rgb(16, 19, 28);

/// Actions from the waveforms panel (handled in [`crate::gui`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaveformAction {
    None,
    StartAuto,
    PauseAuto,
}

/// Zoom / pan / rubber-band state for one trace graph.
#[derive(Clone)]
pub struct TraceViewport {
    pub zoomed:       bool,
    pub cycle_start:  u64,
    pub cycle_span:   u64,
    pub y_lo:         f64,
    pub y_hi:         f64,
    /// Drag rectangle in screen space (two corners), while selecting.
    pub drag_sel:     Option<(Pos2, Pos2)>,
}

impl Default for TraceViewport {
    fn default() -> Self {
        Self {
            zoomed:      false,
            cycle_start: 0,
            cycle_span:  0,
            y_lo:        0.0,
            y_hi:        1.0,
            drag_sel:    None,
        }
    }
}

impl TraceViewport {
    pub fn reset_view(&mut self) {
        self.zoomed = false;
        self.cycle_span = 0;
        self.drag_sel = None;
    }
}

/// Nominal MCU clock for mapping cycle count → simulated time (no fuse-bit model in the core).
pub fn nominal_f_cpu_hz(model: McuModel) -> f64 {
    match model {
        McuModel::Atmega328P => 16_000_000.0,
        McuModel::Atmega128A => 16_000_000.0,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaveformKind {
    Register(u8),
    PortPin { port: char, bit: u8 },
}

impl WaveformKind {
    fn label(self) -> String {
        match self {
            Self::Register(r) => format!("R{r}"),
            Self::PortPin { port, bit } => format!("P{port}{bit}"),
        }
    }
}

#[derive(Clone)]
pub struct WaveformTrace {
    pub id:        u64,
    pub kind:      WaveformKind,
    /// (cycle count, value): pin 0/1, register 0–255 as f64
    pub samples:   Vec<(u64, f64)>,
    last:          Option<f64>,
    /// Running maximum for register autoscale (min Y fixed at 0).
    pub reg_peak:  u8,
}

pub struct WaveformState {
    pub traces:            Vec<WaveformTrace>,
    pub viewports:       HashMap<u64, TraceViewport>,
    pub add_dialog_open:   bool,
    pub add_is_register:   bool,
    pub add_reg:           u8,
    pub add_port:          char,
    pub add_bit:           u8,
    pub add_error:         Option<String>,
    pub fullscreen_id:     Option<u64>,
    next_id:               u64,
    dialog_visuals_backup: Option<Visuals>,
}

impl Default for WaveformState {
    fn default() -> Self {
        Self {
            traces:                Vec::new(),
            viewports:             HashMap::new(),
            add_dialog_open:       false,
            add_is_register:       true,
            add_reg:               0,
            add_port:              'B',
            add_bit:               0,
            add_error:             None,
            fullscreen_id:         None,
            next_id:               1,
            dialog_visuals_backup: None,
        }
    }
}

impl WaveformState {
    pub fn on_reset(&mut self) {
        for t in &mut self.traces {
            t.samples.clear();
            t.last = None;
            t.reg_peak = 0;
        }
        self.viewports.clear();
    }

    /// Sample all traces after an instruction boundary (`cpu.cycles` is current).
    pub fn sample_cpu(&mut self, cpu: &Cpu) {
        if self.traces.is_empty() {
            return;
        }
        const MAX_SAMPLES: usize = 60_000;
        for t in &mut self.traces {
            let v = trace_value(cpu, t.kind);
            if let WaveformKind::Register(_) = t.kind {
                t.reg_peak = t.reg_peak.max(v as u8);
            }
            let changed = t.last.map(|l| (l - v).abs() > f64::EPSILON).unwrap_or(true);
            if changed {
                t.samples.push((cpu.cycles, v));
                if t.samples.len() > MAX_SAMPLES {
                    let drop = t.samples.len() - MAX_SAMPLES;
                    t.samples.drain(0..drop);
                }
                t.last = Some(v);
            }
        }
    }
}

pub fn on_waveforms_panel_hidden(state: &mut WaveformState, ctx: &egui::Context) {
    state.add_dialog_open = false;
    state.fullscreen_id = None;
    if let Some(v) = state.dialog_visuals_backup.take() {
        ctx.style_mut(|s| s.visuals = v);
    }
}

pub fn trace_value(cpu: &Cpu, kind: WaveformKind) -> f64 {
    match kind {
        WaveformKind::Register(r) => cpu.regs[r as usize] as f64,
        WaveformKind::PortPin { port, bit } => {
            let pin_addr = pin_addr_for_cpu(cpu, port);
            if bit > 7 {
                return 0.0;
            }
            let Some(addr) = pin_addr else {
                return 0.0;
            };
            let v = cpu.peek_mem(addr);
            f64::from((v >> bit) & 1)
        }
    }
}

fn pin_addr_for_cpu(cpu: &Cpu, port: char) -> Option<u16> {
    for (name, _, _, pin_addr) in cpu.gpio_ports() {
        if name.chars().next() == Some(port) {
            return Some(*pin_addr);
        }
    }
    None
}

fn port_exists_on_model(model: McuModel, port: char) -> bool {
    match model {
        McuModel::Atmega328P => matches!(port, 'B' | 'C' | 'D'),
        McuModel::Atmega128A => matches!(port, 'A' | 'B' | 'C' | 'D' | 'E' | 'F' | 'G'),
    }
}

fn default_ports(model: McuModel) -> &'static [char] {
    match model {
        McuModel::Atmega328P => &['B', 'C', 'D'],
        McuModel::Atmega128A => &['A', 'B', 'C', 'D', 'E', 'F', 'G'],
    }
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

fn circle_add_button(ui: &mut Ui, open: &mut bool) -> egui::Response {
    let size = 30.0_f32;
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    if response.clicked() {
        *open = true;
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
        .on_hover_text("Add waveform trace")
}

fn add_waveform_chip(ui: &mut Ui, open: &mut bool) {
    Frame::NONE
        .fill(theme::SIM_SURFACE)
        .stroke(Stroke::new(0.75, theme::SIM_BORDER))
        .corner_radius(CornerRadius::same(20))
        .inner_margin(Margin::same(5))
        .show(ui, |ui| {
            circle_add_button(ui, open);
        });
}

fn fmt_ips_plain(ips: f64) -> String {
    if ips >= 1_000_000.0 {
        format!("{:.2} MIPS", ips / 1_000_000.0)
    } else if ips >= 1_000.0 {
        format!("{:.1} kIPS", ips / 1_000.0)
    } else {
        format!("{:.0} IPS", ips)
    }
}

/// Slider value label for the throughput strip (matches [`fmt_ips_plain`]).
fn fmt_slider_ips(n: f64) -> String {
    fmt_ips_plain(n)
}

fn show_throughput_strip(
    ui:           &mut Ui,
    speed_limit:  &mut SpeedLimitState,
    auto_running: bool,
) -> WaveformAction {
    let mut action = WaveformAction::None;
    let mut unlimited = !speed_limit.enabled;
    let mut lim = speed_limit.ips_for_slider();

    ui.horizontal(|ui| {
        let start_en = !auto_running;
        let pause_en = auto_running;
        let start_resp = ui.add_enabled(
            start_en,
            Button::new(
                RichText::new("▶ Start")
                    .monospace()
                    .size(12.5)
                    .color(if start_en { START_GREEN } else { theme::ACCENT_DIM }),
            )
            .fill(theme::SIM_SURFACE_LIFT)
            .stroke(Stroke::new(1.0, theme::SIM_BORDER_BRIGHT))
            .corner_radius(CornerRadius::same(6)),
        );
        if start_resp.clicked() {
            action = WaveformAction::StartAuto;
        }
        start_resp.on_hover_text("Run simulation continuously (AUTO)");

        ui.add_space(8.0);

        let pause_resp = ui.add_enabled(
            pause_en,
            Button::new(
                RichText::new("⏸ Pause")
                    .monospace()
                    .size(12.5)
                    .color(if pause_en { START_GREEN } else { theme::ACCENT_DIM }),
            )
            .fill(theme::SIM_SURFACE_LIFT)
            .stroke(Stroke::new(1.0, theme::SIM_BORDER_BRIGHT))
            .corner_radius(CornerRadius::same(6)),
        );
        if pause_resp.clicked() {
            action = WaveformAction::PauseAuto;
        }
        pause_resp.on_hover_text("Pause AUTO");

        ui.add_space(10.0);
        ui.label(
            RichText::new("AUTO speed")
                .monospace()
                .size(11.5)
                .color(theme::ACCENT),
        );
    });

    ui.add_space(8.0);

    // Card: track + glow edge, slider gets full width (no cramped horizontal row).
    let card_fill = Color32::from_rgb(10, 14, 24);
    let card_rim = Color32::from_rgb(42, 52, 72);
    Frame::NONE
        .fill(card_fill)
        .stroke(Stroke::new(1.0, card_rim))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(12, 11))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("⚡")
                        .size(14.0)
                        .line_height(Some(16.0)),
                );
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("Instruction throughput cap")
                            .monospace()
                            .size(11.0)
                            .color(theme::ACCENT),
                    );
                    ui.label(
                        RichText::new("Limits how fast the simulator runs (wall clock), not MCU f_cpu.")
                            .monospace()
                            .size(9.0)
                            .line_height(Some(12.0))
                            .color(Color32::from_rgba_unmultiplied(130, 140, 165, 220)),
                    );
                });
            });

            ui.add_space(10.0);

            ui.add_enabled_ui(!unlimited, |ui| {
                let w = ui.available_width();
                let slider = egui::Slider::new(&mut lim, 100.0..=20_000_000.0)
                    .logarithmic(true)
                    .show_value(true)
                    .trailing_fill(true)
                    .handle_shape(HandleShape::Rect { aspect_ratio: 0.45 })
                    .custom_formatter(|n, _| fmt_slider_ips(n))
                    .text("");
                let rail = ui.style().spacing.slider_rail_height;
                ui.style_mut().spacing.slider_rail_height = 11.0;
                let changed = ui
                    .add_sized([w, 28.0], slider)
                    .on_hover_text("Drag to cap IPS while AUTO is running")
                    .changed();
                ui.style_mut().spacing.slider_rail_height = rail;
                if changed {
                    speed_limit.set_ips_from_slider(lim, false);
                }
            });

            ui.add_space(10.0);

            ui.horizontal(|ui| {
                let r = ui
                    .checkbox(
                        &mut unlimited,
                        RichText::new("Unlimited (no cap)")
                            .monospace()
                            .size(11.0)
                            .color(theme::ACCENT_DIM),
                    )
                    .on_hover_text("Run AUTO as fast as possible each frame");
                if r.changed() {
                    speed_limit.enabled = !unlimited;
                    if speed_limit.enabled {
                        speed_limit.set_ips_from_slider(lim, false);
                    }
                }
            });
        });

    ui.add_space(10.0);

    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new("Fine tune")
                .monospace()
                .size(10.5)
                .color(theme::ACCENT_DIM),
        );
        ui.add_space(4.0);
        ui.add(
            egui::TextEdit::singleline(&mut speed_limit.value_text)
                .desired_width(48.0)
                .font(egui::TextStyle::Monospace),
        );
        egui::ComboBox::from_id_salt("wf_ips_unit")
            .width(56.0)
            .selected_text(
                RichText::new(speed_limit.unit.label())
                    .monospace()
                    .size(11.0)
                    .color(START_GREEN),
            )
            .show_ui(ui, |ui| {
                ui.style_mut().visuals.override_text_color = Some(START_GREEN);
                for u in [IpsUnit::Ips, IpsUnit::Kips, IpsUnit::Mips] {
                    ui.selectable_value(
                        &mut speed_limit.unit,
                        u,
                        RichText::new(u.label()).monospace().size(11.0),
                    );
                }
            });
        if speed_limit.enabled {
            if let Some(lim) = speed_limit.limit_ips() {
                ui.label(
                    RichText::new(format!("→ {}", fmt_ips_plain(lim)))
                        .monospace()
                        .size(10.5)
                        .color(theme::ACCENT_DIM),
                );
            }
        }
    });
    action
}

/// Simulated seconds of history visible horizontally (default window in cycle space).
const DEFAULT_WINDOW_SIM_SECS: f64 = 0.001;

/// Left gutter for Y tick labels (right-aligned toward plot).
const AXIS_LEFT: f32 = 56.0;
/// Bottom margin: cycle ticks row + axis title row (no overlap with Y ticks).
const AXIS_BOTTOM: f32 = 44.0;
const AXIS_TOP: f32 = 6.0;

const AXIS_TICK: Color32 = Color32::from_rgb(210, 220, 240);
const AXIS_TITLE: Color32 = Color32::from_rgb(175, 190, 215);

fn y_range_auto(trace: &WaveformTrace) -> (f64, f64) {
    match trace.kind {
        WaveformKind::Register(_) => (0.0, trace.reg_peak.max(1) as f64),
        WaveformKind::PortPin { .. } => (0.0, 1.0),
    }
}

fn default_cycle_span(f_cpu: f64) -> u64 {
    let s = (DEFAULT_WINDOW_SIM_SECS * f_cpu) as u64;
    s.max(8)
}

fn view_cycle_value_range(
    trace: &WaveformTrace,
    cpu: &Cpu,
    vp: &TraceViewport,
    f_cpu: f64,
) -> (u64, u64, f64, f64) {
    let def_span = default_cycle_span(f_cpu);
    let max_c = cpu.cycles;
    if !vp.zoomed {
        let c1 = max_c;
        let c0 = c1.saturating_sub(def_span);
        let (y_lo, y_hi) = y_range_auto(trace);
        return (c0, c1, y_lo, y_hi);
    }
    let span = vp.cycle_span.max(1);
    let c0 = vp.cycle_start;
    let c1 = c0.saturating_add(span);
    (c0, c1, vp.y_lo, vp.y_hi)
}

fn plot_to_cycle(px: f32, plot: Rect, c0: u64, c1: u64) -> f64 {
    let u = ((px - plot.left()) / plot.width().max(1.0)).clamp(0.0, 1.0) as f64;
    c0 as f64 + u * (c1.saturating_sub(c0) as f64).max(1.0)
}

fn plot_to_value(py: f32, plot: Rect, y_lo: f64, y_hi: f64) -> f64 {
    let u = ((plot.bottom() - py) / plot.height().max(1.0)).clamp(0.0, 1.0) as f64;
    y_lo + u * (y_hi - y_lo)
}

fn fmt_y_tick(v: f64, kind: WaveformKind) -> String {
    match kind {
        WaveformKind::PortPin { .. } => format!("{}", v.round() as i32),
        WaveformKind::Register(_) => {
            if (v - v.round()).abs() < 1e-4 {
                format!("{}", v.round() as i64)
            } else {
                format!("{:.2}", v)
            }
        }
    }
}

fn draw_waveform_graph(
    ui: &mut Ui,
    trace: &WaveformTrace,
    cpu: &Cpu,
    viewport: &mut TraceViewport,
    f_cpu: f64,
    graph_h: f32,
    widget_id: Id,
    // 1.0 = full available width (aligns painted frame with panel/window edges for resize hit).
    width_scale: f32,
) {
    // Never request more width than the window gives — egui Resize keeps
    // desired_size = max(user_size, last_content_size); a .max(120) here
    // prevented shrinking below ~128px and could fight user resizes.
    let avail_w = ui.available_width();
    let w = if avail_w.is_finite() {
        if (width_scale - 1.0).abs() < 1e-4 {
            avail_w
        } else {
            (avail_w * width_scale).clamp(1.0, avail_w)
        }
    } else {
        120.0
    };
    let outer_h = graph_h + AXIS_BOTTOM + AXIS_TOP + 10.0;
    let (outer_rect, _) = ui.allocate_exact_size(Vec2::new(w, outer_h), Sense::hover());

    let full = outer_rect;
    let plot_rect = Rect::from_min_max(
        pos2(full.left() + AXIS_LEFT, full.top() + AXIS_TOP),
        pos2(full.right() - 2.0, full.bottom() - AXIS_BOTTOM),
    );

    // Interaction first (uses previous frame’s view for mapping).
    let (c_prev0, c_prev1, y_prev_lo, y_prev_hi) =
        view_cycle_value_range(trace, cpu, viewport, f_cpu);
    let response = ui
        .interact(plot_rect, widget_id.with("plot"), Sense::click_and_drag())
        .on_hover_cursor(CursorIcon::Crosshair);

    if response.drag_started() {
        if let Some(pos) = response.interact_pointer_pos() {
            viewport.drag_sel = Some((pos, pos));
        }
    }
    if response.dragged() {
        if let (Some((a, _)), Some(pos)) = (viewport.drag_sel.as_mut(), response.interact_pointer_pos()) {
            viewport.drag_sel = Some((*a, pos));
        }
    }
    if response.drag_stopped() {
        if let Some((a, b)) = viewport.drag_sel.take() {
            let sel = Rect::from_two_pos(a, b).intersect(plot_rect);
            if sel.width() > 4.0 && sel.height() > 4.0 {
                let nc0 = plot_to_cycle(sel.left(), plot_rect, c_prev0, c_prev1) as u64;
                let nc1 = plot_to_cycle(sel.right(), plot_rect, c_prev0, c_prev1) as u64;
                let nv_lo = plot_to_value(sel.bottom(), plot_rect, y_prev_lo, y_prev_hi);
                let nv_hi = plot_to_value(sel.top(), plot_rect, y_prev_lo, y_prev_hi);
                let (lo, hi) = (nc0.min(nc1), nc0.max(nc1));
                let hi = if lo == hi { lo.saturating_add(8) } else { hi };
                let vy_lo = nv_lo.min(nv_hi);
                let mut vy_hi = nv_lo.max(nv_hi);
                if (vy_hi - vy_lo).abs() < 1e-6 {
                    vy_hi = vy_lo + 1.0;
                }
                viewport.zoomed = true;
                viewport.cycle_start = lo;
                viewport.cycle_span = (hi - lo).max(8);
                viewport.y_lo = vy_lo;
                viewport.y_hi = vy_hi;
            }
        }
    }

    let (c0, c1, y_lo, y_hi) = view_cycle_value_range(trace, cpu, viewport, f_cpu);
    let c_span = (c1.saturating_sub(c0)).max(1);

    let p = ui.painter_at(full);
    p.rect_filled(full, CornerRadius::same(4), theme::SIM_SURFACE);
    p.rect_stroke(
        full,
        CornerRadius::same(4),
        Stroke::new(0.75, theme::SIM_BORDER),
        StrokeKind::Inside,
    );

    let font_title = egui::FontId::monospace(11.0);
    let font_axis = egui::FontId::monospace(11.0);
    let y_kind = trace.kind;

    // Y ticks: own column left of plot (right-aligned) — never share corner with cycle ticks.
    let y_label_x = plot_rect.left() - 8.0;
    p.text(
        pos2(y_label_x, plot_rect.top()),
        Align2::RIGHT_BOTTOM,
        fmt_y_tick(y_hi, y_kind),
        font_axis.clone(),
        AXIS_TICK,
    );
    p.text(
        pos2(y_label_x, plot_rect.bottom()),
        Align2::RIGHT_TOP,
        fmt_y_tick(y_lo, y_kind),
        font_axis.clone(),
        AXIS_TICK,
    );
    p.text(
        pos2(full.left() + 4.0, full.top() + 2.0),
        Align2::LEFT_TOP,
        "Value",
        font_title.clone(),
        AXIS_TITLE,
    );

    // X ticks: bottom margin only (below plot), separated from Y corner.
    let cx_row_y = plot_rect.bottom() + 8.0;
    p.text(
        pos2(plot_rect.left(), cx_row_y),
        Align2::LEFT_TOP,
        format!("{}", c0),
        font_axis.clone(),
        AXIS_TICK,
    );
    p.text(
        pos2(plot_rect.right(), cx_row_y),
        Align2::RIGHT_TOP,
        format!("{}", c1),
        font_axis.clone(),
        AXIS_TICK,
    );
    // Bottom-anchor so descenders stay inside `full` (CENTER_TOP at plot_rect.bottom()+offset clipped).
    p.text(
        pos2(plot_rect.center().x, full.bottom() - 1.0),
        Align2::CENTER_BOTTOM,
        "Cycles (cumulative)",
        font_title,
        AXIS_TITLE,
    );

    p.rect_filled(plot_rect, CornerRadius::same(2), Color32::from_rgb(8, 10, 16));
    p.rect_stroke(
        plot_rect,
        CornerRadius::same(2),
        Stroke::new(0.5, Color32::from_gray(45)),
        StrokeKind::Inside,
    );

    let y_at = |v: f64| -> f32 {
        let yn = if (y_hi - y_lo).abs() < 1e-9 {
            0.5
        } else {
            ((v - y_lo) / (y_hi - y_lo)).clamp(0.0, 1.0)
        };
        plot_rect.bottom() - yn as f32 * plot_rect.height()
    };

    let y0 = y_at(y_lo);
    p.line_segment(
        [pos2(plot_rect.left(), y0), pos2(plot_rect.right(), y0)],
        Stroke::new(0.45, Color32::from_gray(42)),
    );

    let trace_lbl = trace.kind.label();
    p.text(
        plot_rect.right_top() + vec2(-4.0, -2.0),
        Align2::RIGHT_BOTTOM,
        trace_lbl,
        egui::FontId::monospace(10.5),
        theme::ACCENT_DIM,
    );

    let x_at = |cyc: u64| -> f32 {
        let u = ((cyc.saturating_sub(c0)) as f64 / c_span as f64).clamp(0.0, 1.0) as f32;
        plot_rect.left() + u * plot_rect.width()
    };

    if trace.samples.is_empty() {
        p.text(
            plot_rect.center(),
            Align2::CENTER_CENTER,
            "no samples yet",
            egui::FontId::monospace(11.0),
            theme::ACCENT_DIM,
        );
    } else {
        let color = theme::ACCENT;
        match trace.kind {
            WaveformKind::PortPin { .. } => {
                let thick = 1.8;
                let samples = &trace.samples;
                for i in 0..samples.len() {
                    let (cyc_i, v_i) = samples[i];
                    let c_next = if i + 1 < samples.len() {
                        samples[i + 1].0
                    } else {
                        cpu.cycles.max(cyc_i)
                    };
                    let c_end = c_next.min(c1).max(cyc_i);
                    if c_end < c0 && cyc_i < c0 {
                        continue;
                    }
                    let x0 = x_at(cyc_i.max(c0));
                    let x1 = x_at(c_end.min(c1));
                    let y = y_at(v_i);
                    if x1 > plot_rect.left() && x0 < plot_rect.right() {
                        p.line_segment([pos2(x0, y), pos2(x1, y)], Stroke::new(thick, color));
                    }
                    if i + 1 < samples.len() {
                        let v_next = samples[i + 1].1;
                        if (v_i - v_next).abs() > 0.5 {
                            let y2 = y_at(v_next);
                            let xb = x_at(c_next.min(c1).max(c0));
                            p.line_segment([pos2(xb, y), pos2(xb, y2)], Stroke::new(thick, color));
                        }
                    }
                }
            }
            WaveformKind::Register(_) => {
                let samples = &trace.samples;
                for w in samples.windows(2) {
                    let &(cy0, v0) = &w[0];
                    let &(cy1, v1) = &w[1];
                    if cy1 < c0 {
                        continue;
                    }
                    if cy0 > c1 {
                        break;
                    }
                    let xa = x_at(cy0.max(c0));
                    let xb = x_at(cy1.min(c1));
                    let y0l = y_at(v0);
                    let y1l = y_at(v1);
                    p.line_segment([pos2(xa, y0l), pos2(xb, y0l)], Stroke::new(1.5, color));
                    p.line_segment([pos2(xb, y0l), pos2(xb, y1l)], Stroke::new(1.5, color));
                }
                if let Some(&(cy_last, v_last)) = samples.last() {
                    if cy_last <= c1 {
                        let xa = x_at(cy_last.max(c0));
                        let xb = x_at(c1);
                        let y = y_at(v_last);
                        p.line_segment([pos2(xa, y), pos2(xb, y)], Stroke::new(1.5, color));
                    }
                }
            }
        }
    }

    if let Some((a, b)) = viewport.drag_sel {
        let sel = Rect::from_two_pos(a, b);
        let fill = Color32::from_rgba_unmultiplied(theme::FOCUS.r(), theme::FOCUS.g(), theme::FOCUS.b(), 40);
        p.rect_filled(sel, CornerRadius::ZERO, fill);
        p.rect_stroke(
            sel,
            CornerRadius::ZERO,
            Stroke::new(1.0, theme::FOCUS),
            StrokeKind::Inside,
        );
    }

    let data_max = trace
        .samples
        .last()
        .map(|(c, _)| *c)
        .unwrap_or(cpu.cycles)
        .max(cpu.cycles);
    let vis_span = c1.saturating_sub(c0).max(1);
    let max_start = data_max.saturating_sub(vis_span);
    let show_pan = viewport.zoomed && max_start > 0;

    if viewport.zoomed {
        ui.add_space(4.0);
        let mut pan = if max_start == 0 {
            0.0_f32
        } else {
            (c0.min(max_start) as f64 / max_start as f64).clamp(0.0, 1.0) as f32
        };
        ui.horizontal(|ui| {
            if show_pan {
                ui.label(
                    RichText::new("Scroll cycles")
                        .monospace()
                        .size(10.0)
                        .color(theme::ACCENT_DIM),
                );
                let wpan = ui.available_width().min(200.0);
                if ui
                    .add_sized(
                        [wpan, 18.0],
                        egui::Slider::new(&mut pan, 0.0..=1.0)
                            .show_value(false)
                            .trailing_fill(true),
                    )
                    .changed()
                {
                    viewport.cycle_start = ((pan as f64) * max_start as f64) as u64;
                }
                ui.add_space(10.0);
            }
            if ui
                .add(
                    Button::new(
                        RichText::new("Relock")
                            .monospace()
                            .size(11.0)
                            .color(START_GREEN),
                    )
                    .fill(theme::SIM_SURFACE_LIFT)
                    .stroke(Stroke::new(1.0, theme::SIM_BORDER_BRIGHT))
                    .corner_radius(CornerRadius::same(5)),
                )
                .on_hover_text("Follow the live waveform again (clear zoom / pan)")
                .clicked()
            {
                viewport.reset_view();
            }
        });
    }
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
        blur:   18,
        spread: 0,
        color:  Color32::from_rgba_unmultiplied(0, 0, 0, 110),
    };
    v.selection.bg_fill = theme::SIM_TAB_ACTIVE;
    v.selection.stroke = Stroke::new(1.0, theme::ACCENT);
    let set_w = |w: &mut egui::style::WidgetVisuals, fill: Color32, fg: Color32, strk: Stroke| {
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
        Color32::from_rgba_unmultiplied(theme::ACCENT.r(), theme::ACCENT.g(), theme::ACCENT.b(), 28)
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

pub fn show_waveforms_panel(
    ctx:          &egui::Context,
    ui:           &mut Ui,
    state:        &mut WaveformState,
    cpu:          &Cpu,
    model:        McuModel,
    speed_limit:  &mut SpeedLimitState,
    auto_running: &mut bool,
) -> WaveformAction {
    let mut wf_action = WaveformAction::None;
    let f_cpu = nominal_f_cpu_hz(model);

    Frame::NONE
        .fill(theme::PANEL_DEEP)
        .stroke(Stroke::new(0.75, theme::SIM_BORDER))
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.style_mut().interaction.selectable_labels = false;

            wf_action = show_throughput_strip(ui, speed_limit, *auto_running);

            ui.add_space(8.0);
            ui.label(
                RichText::new("Waveforms")
                    .monospace()
                    .size(15.0)
                    .color(theme::ACCENT),
            );
            ui.add_space(2.0);
            ui.label(
                RichText::new(format!(
                    "Time axis = cycle count / {:.1} MHz (nominal).",
                    f_cpu / 1_000_000.0
                ))
                .monospace()
                .size(10.5)
                .line_height(Some(14.0))
                .color(theme::ACCENT_DIM),
            );

            ui.add_space(6.0);

            // Reserve space for the pinned footer (separator + “Traces” strip + add chip).
            const WF_ADD_FOOTER_H: f32 = 92.0;
            let avail_h = ui.available_height();
            let scroll_h = if avail_h.is_finite() {
                (avail_h - WF_ADD_FOOTER_H).max(96.0)
            } else {
                220.0
            };

            let fullscreen_id = state.fullscreen_id;
            let traces = &mut state.traces;
            let viewports = &mut state.viewports;
            let fullscreen_ptr = &mut state.fullscreen_id;
            egui::ScrollArea::vertical()
                .id_salt("wf_list")
                .auto_shrink([false, false])
                .max_height(scroll_h)
                .show(ui, |ui| {
                    let mut remove: Option<u64> = None;
                    for t in traces.iter_mut() {
                        if fullscreen_id == Some(t.id) {
                            continue;
                        }
                        let tid = t.id;
                        let vp = viewports.entry(tid).or_default();
                        let graph_id = Id::new(("wf_graph", tid));
                        Frame::NONE
                            .fill(theme::SIM_SURFACE)
                            .stroke(Stroke::new(0.75, theme::SIM_BORDER))
                            .inner_margin(Margin::symmetric(10, 8))
                            .corner_radius(CornerRadius::same(8))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(t.kind.label())
                                            .monospace()
                                            .size(12.0)
                                            .color(theme::ACCENT),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if ui
                                            .small_button(
                                                RichText::new("⛶ Full")
                                                    .monospace()
                                                    .size(10.5)
                                                    .color(theme::ACCENT_DIM),
                                            )
                                            .on_hover_text("Expand over the editor")
                                            .clicked()
                                        {
                                            *fullscreen_ptr = Some(t.id);
                                        }
                                        if ui
                                            .small_button(
                                                RichText::new("Remove")
                                                    .monospace()
                                                    .size(10.5)
                                                    .color(Color32::from_rgb(200, 120, 120)),
                                            )
                                            .clicked()
                                        {
                                            remove = Some(t.id);
                                        }
                                    });
                                });
                                ui.add_space(6.0);
                                draw_waveform_graph(ui, t, cpu, vp, f_cpu, 112.0, graph_id, 1.0);
                            });
                        ui.add_space(10.0);
                    }
                    if let Some(id) = remove {
                        traces.retain(|x| x.id != id);
                        viewports.remove(&id);
                        if *fullscreen_ptr == Some(id) {
                            *fullscreen_ptr = None;
                        }
                    }

                    if traces.is_empty() {
                        ui.add_space(16.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                RichText::new("No traces yet")
                                    .monospace()
                                    .size(12.0)
                                    .color(theme::ACCENT_DIM),
                            );
                        });
                    }
                });

            ui.add_space(6.0);
            ui.separator();
            ui.add_space(4.0);
            Frame::NONE
                .fill(theme::SIM_SURFACE)
                .stroke(Stroke::new(0.75, theme::SIM_BORDER))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Traces")
                                .monospace()
                                .size(10.5)
                                .color(theme::ACCENT_DIM),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            add_waveform_chip(ui, &mut state.add_dialog_open);
                        });
                    });
                });
        });

    let out_action = wf_action;

    if state.add_dialog_open {
        if state.dialog_visuals_backup.is_none() {
            state.dialog_visuals_backup = Some(ui.ctx().style().visuals.clone());
        }
        Window::new("wf_add_dlg")
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
            .show(ctx, |ui| {
                apply_add_dialog_visuals(ui);
                ui.set_min_width(300.0);
                ui.label(
                    RichText::new("Add waveform")
                        .monospace()
                        .size(16.0)
                        .color(theme::ACCENT),
                );
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    ui.selectable_value(&mut state.add_is_register, true, "Register");
                    ui.selectable_value(&mut state.add_is_register, false, "Port pin");
                });
                ui.add_space(10.0);

                if state.add_is_register {
                    ui.label(
                        RichText::new("Register")
                            .monospace()
                            .size(10.5)
                            .color(theme::ACCENT_DIM),
                    );
                    egui::ComboBox::from_id_salt("wf_reg")
                        .selected_text(
                            RichText::new(format!("R{}", state.add_reg))
                                .monospace()
                                .size(12.0)
                                .color(theme::ACCENT),
                        )
                        .width(ui.available_width())
                        .show_ui(ui, |ui| {
                            for r in 0u8..32u8 {
                                ui.selectable_value(&mut state.add_reg, r, format!("R{r}"));
                            }
                        });
                } else {
                    let ports = default_ports(model);
                    if !ports.contains(&state.add_port) {
                        state.add_port = *ports.first().unwrap_or(&'B');
                    }
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
                        egui::ComboBox::from_id_salt("wf_port")
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
                        egui::ComboBox::from_id_salt("wf_bit")
                            .selected_text(
                                RichText::new(format!("{}", state.add_bit))
                                    .monospace()
                                    .size(12.0)
                                    .color(theme::ACCENT),
                            )
                            .width(ui.available_width())
                            .show_ui(ui, |ui| {
                                for b in 0..=7u8 {
                                    ui.selectable_value(&mut state.add_bit, b, format!("{b}"));
                                }
                            });
                    });
                }

                if let Some(ref err) = state.add_error {
                    ui.add_space(8.0);
                    ui.label(RichText::new(err).monospace().size(10.5).color(theme::ERR_RED));
                }

                ui.add_space(18.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if angelic_dialog_button(ui, "Add", true).clicked() {
                            state.add_error = None;
                            let kind = if state.add_is_register {
                                WaveformKind::Register(state.add_reg)
                            } else {
                                if state.add_bit > 7 {
                                    state.add_error = Some("Pin must be 0–7.".to_string());
                                    return;
                                }
                                if !port_exists_on_model(model, state.add_port) {
                                    state.add_error = Some("Invalid port.".to_string());
                                    return;
                                }
                                WaveformKind::PortPin {
                                    port: state.add_port,
                                    bit:  state.add_bit,
                                }
                            };
                            let dup = state.traces.iter().any(|t| t.kind == kind);
                            if dup {
                                state.add_error = Some("That trace already exists.".to_string());
                            } else {
                                let v = trace_value(cpu, kind);
                                let mut reg_peak = 0u8;
                                if let WaveformKind::Register(_) = kind {
                                    reg_peak = v as u8;
                                }
                                state.traces.push(WaveformTrace {
                                    id:       state.next_id,
                                    kind,
                                    samples:  vec![(cpu.cycles, v)],
                                    last:     Some(v),
                                    reg_peak,
                                });
                                state.next_id += 1;
                                state.add_dialog_open = false;
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

    // Expanded graph window (above central editor). Movable/resizable — avoid `.anchor` / `fixed_*`
    // every frame, which re-locks position and size.
    if let Some(fs_id) = state.fullscreen_id {
        let trace_opt = state.traces.iter().find(|t| t.id == fs_id).cloned();
        if let Some(t) = trace_opt {
            let scr = ctx.screen_rect();
            let margin = Vec2::splat(24.0);
            // Moderate default — almost-full defaults made first-open huge; content must also
            // stay within the resize rect or egui grows the window back (desired = max(user, content)).
            let default_size = Vec2::new(
                (scr.width() * 0.52).clamp(360.0, 720.0),
                (scr.height() * 0.45).clamp(280.0, (scr.height() - margin.y * 2.0).max(280.0)),
            );
            let default_pos = scr.center() - default_size * 0.5;
            Window::new("wf_fullscreen")
                .title_bar(false)
                .collapsible(false)
                .resizable(true)
                .resize(|r| r.with_stroke(false))
                .default_pos(default_pos)
                .default_size(default_size)
                .min_size(Vec2::new(280.0, 200.0))
                .max_size(scr.size())
                .frame(
                    Frame::NONE
                        .fill(Color32::from_black_alpha(235))
                        .inner_margin(Margin::symmetric(4, 8)),
                )
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    ui.vertical(|ui| {
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(t.kind.label())
                                    .monospace()
                                    .size(16.0)
                                    .color(theme::ACCENT),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui
                                    .add(
                                        Button::new(
                                            RichText::new("⭳ Minimize")
                                                .monospace()
                                                .size(13.0)
                                                .color(START_GREEN),
                                        )
                                        .fill(theme::SIM_SURFACE_LIFT)
                                        .stroke(Stroke::new(1.0, theme::SIM_BORDER_BRIGHT))
                                        .corner_radius(CornerRadius::same(6)),
                                    )
                                    .on_hover_text("Back to panel")
                                    .clicked()
                                {
                                    state.fullscreen_id = None;
                                }
                                ui.add_space(8.0);
                                let can_reset = state
                                    .viewports
                                    .get(&fs_id)
                                    .map(|v| v.zoomed)
                                    .unwrap_or(false);
                                if ui
                                    .add_enabled(
                                        can_reset,
                                        Button::new(
                                            RichText::new("Reset graph")
                                                .monospace()
                                                .size(12.5)
                                                .color(if can_reset {
                                                    START_GREEN
                                                } else {
                                                    theme::ACCENT_DIM
                                                }),
                                        )
                                        .fill(theme::SIM_SURFACE_LIFT)
                                        .stroke(Stroke::new(1.0, theme::SIM_BORDER_BRIGHT))
                                        .corner_radius(CornerRadius::same(6)),
                                    )
                                    .on_hover_text("Clear zoom / pan to default view")
                                    .clicked()
                                {
                                    if let Some(vp) = state.viewports.get_mut(&fs_id) {
                                        vp.reset_view();
                                    }
                                }
                            });
                        });
                        ui.add_space(12.0);
                        // `draw_waveform_graph` allocates graph_h + axes overhead — must not exceed
                        // available height or last_content_size forces the window back open (egui Resize).
                        let wf_fs_overhead = AXIS_BOTTOM + AXIS_TOP + 10.0;
                        let avail_h_raw = ui.available_height();
                        let cap_h = ui.max_rect().height();
                        let avail_h = if avail_h_raw.is_finite() && avail_h_raw > 0.0 {
                            avail_h_raw.min(if cap_h.is_finite() { cap_h } else { avail_h_raw })
                        } else {
                            240.0
                        };
                        let graph_h = (avail_h - wf_fs_overhead - 4.0).max(80.0);
                        let vp = state.viewports.entry(fs_id).or_default();
                        draw_waveform_graph(
                            ui,
                            &t,
                            cpu,
                            vp,
                            f_cpu,
                            graph_h,
                            Id::new(("wf_fs_graph", fs_id)),
                            1.0,
                        );
                    });
                });
        } else {
            state.fullscreen_id = None;
        }
    }
    out_action
}
