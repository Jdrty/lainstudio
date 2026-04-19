//! virtual USART

use eframe::egui::{
    self, Align, Button, Color32, CornerRadius, Frame, Layout, Margin, RichText, ScrollArea, Stroke,
    TextEdit, Ui,
};

use crate::avr::cpu::Cpu;
use crate::avr::McuModel;
use crate::sim_panel::{show_sim_machine_status_row, show_sim_sticky_controls, SimAction, SpeedLimitState};
use crate::theme;

const USART0_TERM_W: f32 = 280.0;
const USART0_TERM_H: f32 = 180.0;
/// USART1 (128A): slightly narrower when stacked.
const USART1_TERM_W: f32 = 260.0;
const USART1_TERM_H: f32 = 120.0;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum UartSendLineEnding {
    #[default]
    None,
    /// `\n`
    Newline,
    /// `\r`
    CarriageReturn,
    /// `\r\n`
    BothNlAndCr,
}

impl UartSendLineEnding {
    fn label(self) -> &'static str {
        match self {
            UartSendLineEnding::None => "No line ending",
            UartSendLineEnding::Newline => "Newline",
            UartSendLineEnding::CarriageReturn => "Carriage return",
            UartSendLineEnding::BothNlAndCr => "Both NL & CR",
        }
    }

    fn push_after_payload(self, cpu: &mut Cpu, port: u8) {
        match self {
            UartSendLineEnding::None => {}
            UartSendLineEnding::Newline => cpu.usart_rx_host_push(port, b'\n'),
            UartSendLineEnding::CarriageReturn => cpu.usart_rx_host_push(port, b'\r'),
            UartSendLineEnding::BothNlAndCr => {
                cpu.usart_rx_host_push(port, b'\r');
                cpu.usart_rx_host_push(port, b'\n');
            }
        }
    }
}

pub struct UartPanelState {
    pub line0: String,
    pub line1: String,
    pub rx0:   String,
    pub rx1:   String,
    /// Applied to Send on USART0 and USART1 (same as typical serial monitor).
    pub send_line_ending: UartSendLineEnding,
}

impl Default for UartPanelState {
    fn default() -> Self {
        Self {
            line0: String::new(),
            line1: String::new(),
            rx0:   String::new(),
            rx1:   String::new(),
            send_line_ending: UartSendLineEnding::Newline,
        }
    }
}

pub fn append_uart_tx_to_scrollback(cpu: &mut Cpu, model: McuModel, state: &mut UartPanelState) -> usize {
    let mut n = 0usize;
    let mut drain0 = Vec::new();
    cpu.usart_drain_tx_to_host(0, &mut drain0);
    n += drain0.len();
    append_bytes_as_terminal(&mut state.rx0, &drain0);
    if model == McuModel::Atmega128A {
        let mut drain1 = Vec::new();
        cpu.usart_drain_tx_to_host(1, &mut drain1);
        n += drain1.len();
        append_bytes_as_terminal(&mut state.rx1, &drain1);
    }
    n
}

fn append_bytes_as_terminal(s: &mut String, bytes: &[u8]) {
    for &b in bytes {
        if b == b'\r' {
            continue;
        }
        if b == b'\n' {
            s.push('\n');
            continue;
        }
        if (0x20..0x7F).contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("\\x{b:02X}"));
        }
    }
    const MAX: usize = 96_000;
    if s.len() > MAX {
        let over = s.len() - MAX;
        let start = s
            .char_indices()
            .map(|(i, _)| i)
            .find(|&i| i >= over)
            .unwrap_or(0);
        s.drain(..start);
    }
}

pub fn show_uart_panel(
    ui:              &mut Ui,
    cpu:             &mut Cpu,
    model:           McuModel,
    state:           &mut UartPanelState,
    assembled_board: Option<McuModel>,
    auto_running:    bool,
    ips:             f64,
    speed_limit:     &mut SpeedLimitState,
) -> SimAction {
    let mut action = SimAction::None;

    Frame::NONE
        .fill(theme::PANEL_DEEP)
        .stroke(Stroke::new(0.75, theme::SIM_BORDER))
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());

            let title = match assembled_board {
                Some(m) => format!("[ UART  {} ]", m.label()),
                None => "[ UART ]".to_string(),
            };
            ui.label(
                RichText::new(title)
                    .monospace()
                    .size(13.0)
                    .color(theme::START_GREEN),
            );
            ui.add_space(4.0);
            show_sim_machine_status_row(ui, cpu);
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Line ending:")
                        .monospace()
                        .size(10.5)
                        .color(theme::ACCENT_DIM),
                );
                line_ending_combo(ui, state);
            });
            ui.add_space(4.0);

            ScrollArea::vertical()
                .id_salt("uart_panel_body")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("USART0")
                            .monospace()
                            .size(11.0)
                            .color(theme::ACCENT_DIM),
                    );
                    ui.add_space(2.0);
                    terminal_scroll(
                        ui,
                        "uart_term0",
                        &state.rx0,
                        USART0_TERM_W,
                        USART0_TERM_H,
                    );

                    ui.add_space(8.0);
                    uart_send_row(
                        ui,
                        cpu,
                        "Host → USART0 RX",
                        &mut state.line0,
                        0,
                        state.send_line_ending,
                    );

                    if model == McuModel::Atmega128A {
                        ui.add_space(12.0);
                        ui.label(
                            RichText::new("USART1")
                                .monospace()
                                .size(11.0)
                                .color(theme::ACCENT_DIM),
                        );
                        ui.add_space(2.0);
                        terminal_scroll(
                            ui,
                            "uart_term1",
                            &state.rx1,
                            USART1_TERM_W,
                            USART1_TERM_H,
                        );
                        ui.add_space(8.0);
                        uart_send_row(
                            ui,
                            cpu,
                            "Host → USART1 RX",
                            &mut state.line1,
                            1,
                            state.send_line_ending,
                        );
                    }
                });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .small_button(
                            RichText::new("Clear serial logs")
                                .monospace()
                                .size(10.0)
                                .color(theme::ACCENT_DIM),
                        )
                        .clicked()
                    {
                        state.rx0.clear();
                        state.rx1.clear();
                    }
                });
            });

            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);

            let sticky = show_sim_sticky_controls(
                ui,
                auto_running,
                ips,
                speed_limit,
                "ASSEMBLE  (from editor)",
                "uart_ips_unit",
            );
            if sticky != SimAction::None {
                action = sticky;
            }
        });

    action
}

fn line_ending_combo(ui: &mut Ui, state: &mut UartPanelState) {
    Frame::NONE
        .fill(theme::SIM_SURFACE_LIFT)
        .stroke(Stroke::new(1.0, theme::SIM_BORDER_BRIGHT))
        .inner_margin(Margin::symmetric(6, 3))
        .corner_radius(CornerRadius::same(4))
        .show(ui, |ui| {
            egui::ComboBox::from_id_salt("uart_send_line_ending")
                .width(160.0)
                .selected_text(
                    RichText::new(state.send_line_ending.label())
                        .monospace()
                        .size(10.5)
                        .color(theme::START_GREEN),
                )
                .show_ui(ui, |ui| {
                    ui.style_mut().visuals.override_text_color = Some(theme::START_GREEN);
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    for v in [
                        UartSendLineEnding::None,
                        UartSendLineEnding::Newline,
                        UartSendLineEnding::CarriageReturn,
                        UartSendLineEnding::BothNlAndCr,
                    ] {
                        ui.selectable_value(
                            &mut state.send_line_ending,
                            v,
                            RichText::new(v.label()).monospace().size(10.5),
                        );
                    }
                });
        });
}

fn terminal_scroll(
    ui: &mut Ui,
    id: &'static str,
    rx: &str,
    width: f32,
    max_height: f32,
) {
    ui.scope(|ui| {
        ui.set_min_width(width);
        ui.set_max_width(width);
        Frame::NONE
            .fill(Color32::from_rgb(4, 6, 10))
            .stroke(Stroke::new(0.75, theme::SIM_BORDER))
            .inner_margin(Margin::symmetric(8, 8))
            .corner_radius(CornerRadius::same(4))
            .show(ui, |ui| {
                ScrollArea::vertical()
                    .id_salt(id)
                    .max_height(max_height)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        let text = if rx.is_empty() { "(no output yet)" } else { rx };
                        ui.label(
                            RichText::new(text)
                                .monospace()
                                .size(12.0)
                                .line_height(Some(15.0))
                                .color(theme::START_GREEN_DIM),
                        );
                    });
            });
    });
}

fn uart_send_row(
    ui: &mut Ui,
    cpu: &mut Cpu,
    label: &str,
    line: &mut String,
    port: u8,
    line_ending: UartSendLineEnding,
) {
    ui.label(
        RichText::new(label)
            .monospace()
            .size(10.5)
            .color(theme::ACCENT_DIM),
    );
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let te_width = (ui.available_width() - 78.0).max(40.0);
        Frame::NONE
            .fill(theme::SIM_SURFACE_LIFT)
            .stroke(Stroke::new(1.0, theme::SIM_BORDER_BRIGHT))
            .inner_margin(Margin::symmetric(6, 4))
            .corner_radius(CornerRadius::same(4))
            .show(ui, |ui| {
                ui.set_min_width(te_width);
                ui.set_max_width(te_width);
                ui.add(
                    TextEdit::singleline(line)
                        .desired_width(te_width)
                        .font(egui::TextStyle::Monospace)
                        .frame(false),
                );
            });
        if ui
            .add(
                Button::new(RichText::new("Send").monospace().size(11.0).color(theme::ACCENT))
                    .fill(theme::SIM_SURFACE_LIFT)
                    .stroke(Stroke::new(1.0, theme::SIM_BORDER_BRIGHT))
                    .corner_radius(CornerRadius::same(5)),
            )
            .clicked()
        {
            for b in line.bytes() {
                cpu.usart_rx_host_push(port, b);
            }
            line_ending.push_after_payload(cpu, port);
            line.clear();
        }
    });
}
