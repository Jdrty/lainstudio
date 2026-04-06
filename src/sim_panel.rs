//! avr_sim_panel tabs cpu ports timers sram

use eframe::egui::{
    self, Align, Align2, Button, Color32, Frame, Grid, Key, Label, Layout, Margin,
    RichText, ScrollArea, Sense, Stroke, TextEdit, Ui, Window,
};

use crate::avr::cpu::{
    Cpu, StepResult, SREG_C, SREG_H, SREG_I, SREG_N, SREG_S, SREG_T, SREG_V, SREG_Z,
};
use crate::avr::io_map;
use crate::welcome::{START_GREEN, START_GREEN_DIM};

const AMBER:   Color32 = Color32::from_rgb(255, 185, 55);
const DIM:     Color32 = Color32::from_rgb(65,  65,  65);
const ERR_RED: Color32 = Color32::from_rgb(255, 80,  80);

// public_types
const FLASH_PER_PAGE: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimTab { Cpu, Ports, Timers, Sram, Flash, Break, Stack }

// stack_popup_state
pub struct StackState {
    pub popup_open: bool,
}
impl Default for StackState {
    fn default() -> Self { Self { popup_open: false } }
}

// ── IPS speed-limit state ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpsUnit { Ips, Kips, Mips }

impl IpsUnit {
    pub fn label(self) -> &'static str {
        match self { Self::Ips => "IPS", Self::Kips => "kIPS", Self::Mips => "MIPS" }
    }
    pub fn multiplier(self) -> f64 {
        match self { Self::Ips => 1.0, Self::Kips => 1_000.0, Self::Mips => 1_000_000.0 }
    }
}

pub struct SpeedLimitState {
    pub enabled:    bool,
    pub value_text: String,   // raw input text
    pub unit:       IpsUnit,
}

impl Default for SpeedLimitState {
    fn default() -> Self {
        Self { enabled: false, value_text: "1".to_string(), unit: IpsUnit::Mips }
    }
}

impl SpeedLimitState {
    /// Resolved limit in IPS, or None if disabled / invalid.
    pub fn limit_ips(&self) -> Option<f64> {
        if !self.enabled { return None; }
        self.value_text.trim().parse::<f64>().ok()
            .filter(|&v| v > 0.0)
            .map(|v| v * self.unit.multiplier())
    }
}

// ── Breakpoint state ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BpAction { Pause, PrintTerm, PrintAndPause }

impl BpAction {
    fn label(self) -> &'static str {
        match self {
            Self::Pause        => "Pause",
            Self::PrintTerm    => "Print → terminal",
            Self::PrintAndPause => "Print + Pause",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Breakpoint {
    pub addr:    u16,
    pub action:  BpAction,
    pub message: String,
    pub enabled: bool,
}

pub struct BreakpointState {
    pub breakpoints:  Vec<Breakpoint>,
    pub new_addr_text: String,
    pub new_action:   BpAction,
    pub new_message:  String,
}

impl Default for BreakpointState {
    fn default() -> Self {
        Self {
            breakpoints:   Vec::new(),
            new_addr_text: String::new(),
            new_action:    BpAction::Pause,
            new_message:   String::new(),
        }
    }
}

impl BreakpointState {
    /// Flat list of enabled breakpoint addresses (used by CPU hot loop).
    pub fn active_addrs(&self) -> Vec<u16> {
        self.breakpoints.iter()
            .filter(|b| b.enabled)
            .map(|b| b.addr)
            .collect()
    }
}

pub struct FlashState {
    pub page:      usize,
    pub jump_text: String,
    pub jumping:   bool,
}

impl Default for FlashState {
    fn default() -> Self {
        Self { page: 0, jump_text: String::new(), jumping: false }
    }
}

/// Max external SRAM addressable by ATmega128A: 0x1100–0xFFFF = 61184 bytes.
pub const XMEM_MAX: u32 = 0xEF00;

pub struct XmemState {
    pub size_text: String,
}

impl Default for XmemState {
    fn default() -> Self { Self { size_text: "1024".to_string() } }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimAction {
    None,
    Assemble,
    Step,
    Run10,
    Run100,
    Reset,
    AutoToggle,
    SetIoBit { addr: u16, mask: u8 },
    /// size == 0 disables XMEM
    SetXmem(u32),
}

// entry_point

pub fn show_sim_panel(
    ui:            &mut Ui,
    cpu:           &Cpu,
    last_result:   Option<StepResult>,
    active_tab:    &mut SimTab,
    auto_running:  bool,
    ips:           f64,
    flash_state:   &mut FlashState,
    speed_limit:   &mut SpeedLimitState,
    bp_state:      &mut BreakpointState,
    stack_state:   &mut StackState,
    xmem_state:    &mut XmemState,
) -> SimAction {
    let mut action = SimAction::None;

    Frame::NONE
        .fill(Color32::from_rgb(3, 7, 3))
        .stroke(Stroke::new(1.0, START_GREEN_DIM))
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());

            // sticky_header
            ui.label(
                RichText::new(format!("[ AVR SIM  {} ]", cpu.model.label()))
                    .monospace().size(13.0).color(START_GREEN),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("PC  {:04X}", cpu.pc))
                        .monospace().size(12.5).color(AMBER),
                );
                ui.add_space(12.0);
                ui.label(
                    RichText::new(format!("SP  {:04X}", cpu.sp))
                        .monospace().size(12.5).color(START_GREEN_DIM),
                );
                ui.add_space(12.0);
                ui.label(
                    RichText::new(format!("CYC {}", cpu.cycles))
                        .monospace().size(12.5).color(START_GREEN_DIM),
                );
            });
            ui.add_space(6.0);

            // tab_bar
            ui.horizontal(|ui| {
                for (tab, label) in [
                    (SimTab::Cpu,    "CPU"),
                    (SimTab::Ports,  "PORTS"),
                    (SimTab::Timers, "TIMERS"),
                    (SimTab::Sram,   "SRAM"),
                    (SimTab::Flash,  "FLASH"),
                    (SimTab::Break,  "BREAK"),
                    (SimTab::Stack,  "STACK"),
                ] {
                    let selected   = *active_tab == tab;
                    let color      = if selected { START_GREEN } else { DIM };
                    let fill       = if selected { Color32::from_rgb(8, 24, 8) }
                                     else        { Color32::from_rgb(3, 7, 3) };
                    let stroke_col = if selected { START_GREEN } else { DIM };
                    let resp = ui.add(
                        Button::new(
                            RichText::new(label).monospace().size(11.5).color(color),
                        )
                        .fill(fill)
                        .stroke(Stroke::new(1.0, stroke_col)),
                    );
                    if resp.clicked() { *active_tab = tab; }
                }
            });
            ui.separator();
            ui.add_space(4.0);

            // scrollable_tab_content
            let avail_h = ui.available_height() - 142.0; // room_for_controls
            let tab_action = ScrollArea::vertical()
                .id_salt("sim_scroll")
                .auto_shrink([false, false])
                .max_height(avail_h.max(120.0))
                .show(ui, |ui| -> SimAction {
                    match *active_tab {
                        SimTab::Cpu    => { show_cpu_tab(ui, cpu, last_result); SimAction::None }
                        SimTab::Ports  => { show_ports_tab(ui, cpu);            SimAction::None }
                        SimTab::Timers => show_timers_tab(ui, cpu),
                        SimTab::Sram   => show_sram_tab(ui, cpu, xmem_state),
                        SimTab::Flash  => { show_flash_tab(ui, cpu, flash_state); SimAction::None }
                        SimTab::Break  => { show_break_tab(ui, bp_state);       SimAction::None }
                        SimTab::Stack  => show_stack_tab(ui, cpu, stack_state),
                    }
                }).inner;
            if action == SimAction::None { action = tab_action; }

            // sticky_controls
            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);

            if assemble_btn(ui, "ASSEMBLE  (from editor)").clicked() {
                action = SimAction::Assemble;
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if retro_btn(ui, "STEP").clicked()        { action = SimAction::Step; }
                if retro_btn(ui, "RUN\u{00D7}10").clicked()  { action = SimAction::Run10; }
                if retro_btn(ui, "RUN\u{00D7}100").clicked() { action = SimAction::Run100; }
                if retro_btn(ui, "RESET").clicked()       { action = SimAction::Reset; }
            });
            ui.add_space(4.0);
            // AUTO + IPS display
            ui.horizontal(|ui| {
                if auto_running {
                    if ui.add(
                        Button::new(
                            RichText::new("\u{25A0} STOP").monospace().size(12.5).color(AMBER),
                        )
                        .fill(Color32::from_rgb(30, 12, 0))
                        .stroke(Stroke::new(1.5, AMBER)),
                    ).clicked() {
                        action = SimAction::AutoToggle;
                    }
                } else if ui.add(
                    Button::new(
                        RichText::new("\u{25B6} AUTO").monospace().size(12.5).color(START_GREEN),
                    )
                    .fill(Color32::from_rgb(6, 20, 6))
                    .stroke(Stroke::new(1.5, START_GREEN)),
                ).clicked() {
                    action = SimAction::AutoToggle;
                }
                ui.add_space(8.0);
                ui.label(
                    RichText::new(fmt_ips(ips, auto_running))
                        .monospace()
                        .size(12.0)
                        .color(if auto_running { AMBER } else { DIM }),
                );
            });
            // speed-limit row
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.checkbox(
                    &mut speed_limit.enabled,
                    RichText::new("Limit:").monospace().size(11.0).color(START_GREEN_DIM),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut speed_limit.value_text)
                        .desired_width(44.0)
                        .font(egui::TextStyle::Monospace),
                );
                egui::ComboBox::from_id_salt("ips_unit")
                    .width(58.0)
                    .selected_text(
                        RichText::new(speed_limit.unit.label())
                            .monospace().size(11.0).color(START_GREEN),
                    )
                    .show_ui(ui, |ui| {
                        ui.style_mut().visuals.override_text_color = Some(START_GREEN);
                        for u in [IpsUnit::Ips, IpsUnit::Kips, IpsUnit::Mips] {
                            ui.selectable_value(
                                &mut speed_limit.unit, u,
                                RichText::new(u.label()).monospace().size(11.0),
                            );
                        }
                    });
                if let Some(lim) = speed_limit.limit_ips() {
                    ui.label(
                        RichText::new(format!("= {}", fmt_ips_plain(lim)))
                            .monospace().size(10.5).color(DIM),
                    );
                }
            });
        });

    action
}

// cpu_tab

fn show_cpu_tab(ui: &mut Ui, cpu: &Cpu, last_result: Option<StepResult>) {
    section_label(ui, "REGISTERS");
    ui.add_space(4.0);
    Grid::new("sim_regs")
        .num_columns(4)
        .spacing([10.0, 2.0])
        .show(ui, |ui| {
            for row in 0..8usize {
                for col in 0..4usize {
                    let idx = col * 8 + row;
                    let val = cpu.regs[idx];
                    let color = if val != 0 { START_GREEN } else { DIM };
                    ui.label(
                        RichText::new(format!("R{idx:02}:{val:02X}"))
                            .monospace().size(12.0).color(color),
                    );
                }
                ui.end_row();
            }
        });

    ui.add_space(6.0);
    ui.separator();
    ui.add_space(4.0);

    section_label(ui, "SREG");
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        for &(name, bit) in &[
            ("I", SREG_I), ("T", SREG_T), ("H", SREG_H), ("S", SREG_S),
            ("V", SREG_V), ("N", SREG_N), ("Z", SREG_Z), ("C", SREG_C),
        ] {
            let set = (cpu.sreg >> bit) & 1 != 0;
            let color = if set { AMBER } else { DIM };
            ui.label(
                RichText::new(format!("{name}:{}", (cpu.sreg >> bit) & 1))
                    .monospace().size(12.5).color(color),
            );
        }
    });

    ui.add_space(6.0);
    ui.separator();
    ui.add_space(4.0);

    section_label(ui, "FLASH DISASM");
    ui.add_space(4.0);
    let pc    = cpu.pc;
    let start = pc.saturating_sub(3);
    for addr in start..start + 8 {
        if addr as usize >= cpu.flash_words() { break; }
        let is_current = addr == pc;
        let arrow  = if is_current { "\u{2192}" } else { " " };
        let op     = cpu.flash[addr as usize];
        let disasm = cpu.disasm_at(addr);
        let cyc    = Cpu::instr_cycles_str(op);
        let color  = if is_current { AMBER } else { START_GREEN_DIM };
        let ivt_ann = cpu.ivt_name(addr as u32)
            .map(|n| format!("  ; <{n}>"))
            .unwrap_or_default();
        ui.label(
            RichText::new(format!("{arrow} {:04X}  {:<18} [{:>5}]{ivt_ann}", addr, disasm, format!("{cyc}cy")))
                .monospace().size(12.0).color(color),
        );
    }

    if let Some(res) = last_result {
        ui.add_space(4.0);
        match res {
            StepResult::UnknownOpcode(op) => {
                ui.label(
                    RichText::new(format!("! UNKNOWN OPCODE 0x{op:04X}"))
                        .monospace().size(11.5).color(ERR_RED),
                );
            }
            StepResult::Halted => {
                ui.label(
                    RichText::new("! HALTED (PC out of Flash)")
                        .monospace().size(11.5).color(AMBER),
                );
            }
            StepResult::Ok => {}
        }
    }
}

// ports_tab

fn show_ports_tab(ui: &mut Ui, cpu: &Cpu) {
    section_label(ui, "GPIO PORTS  (DDR=0 INPUT, DDR=1 OUTPUT)");
    ui.add_space(6.0);

    let xmem_active = cpu.has_xmem() && !cpu.xmem.is_empty();
    let (xmem_portc_n, xmem_portc_mask) = if xmem_active {
        let sz = cpu.xmem.len() as u32;
        let n = xmem_portc_pins(sz);
        let m = if n >= 8 { 0xFF } else { (1u8 << n).wrapping_sub(1) };
        (n, m)
    } else {
        (0u8, 0u8)
    };

    ui.label(
        RichText::new("PORT  DDR   OUT   PIN   7 6 5 4 3 2 1 0")
            .monospace().size(11.5).color(START_GREEN_DIM),
    );
    ui.add_space(2.0);
    ui.separator();

    for &(name, port_addr, ddr_addr, pin_addr) in cpu.gpio_ports() {
        let port = cpu.read_mem(port_addr);
        let ddr  = cpu.read_mem(ddr_addr);
        let pin  = cpu.read_mem(pin_addr);

        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{name}     {ddr:02X}    {port:02X}    {pin:02X}    "))
                    .monospace().size(12.0).color(START_GREEN),
            );
            for bit in (0..8u8).rev() {
                let xmem_pin = xmem_active
                    && match name {
                        "A" => true,
                        "C" => xmem_portc_n > 0 && (xmem_portc_mask & (1u8 << bit)) != 0,
                        "G" => bit < 3,
                        _ => false,
                    };
                let is_out = (ddr >> bit) & 1 != 0;
                let high   = if is_out { (port >> bit) & 1 != 0 }
                             else      { (pin  >> bit) & 1 != 0 };
                let (ch, col) = if is_out {
                    if high { ('\u{2588}', AMBER) } else { ('\u{2591}', START_GREEN_DIM) }
                } else {
                    ('\u{00B7}', DIM)
                };

                ui.scope(|ui| {
                    ui.set_width(10.0);
                    const XMEM_OUT_LOW: char = '\u{2504}'; // diff for red idk
                    let dot_r = 2.05;
                    let label = if xmem_pin {
                        if is_out {
                            if high {
                                Label::new(
                                    RichText::new('\u{2588}'.to_string())
                                        .monospace()
                                        .size(13.0)
                                        .color(ERR_RED),
                                )
                            } else {
                                Label::new(
                                    RichText::new(XMEM_OUT_LOW.to_string())
                                        .monospace()
                                        .size(13.0)
                                        .color(ERR_RED),
                                )
                            }
                        } else {
                            Label::new(
                                RichText::new(ch.to_string())
                                    .monospace()
                                    .size(13.0)
                                    .color(Color32::TRANSPARENT),
                            )
                        }
                    } else {
                        Label::new(RichText::new(ch.to_string()).monospace().size(13.0).color(col))
                    };
                    let resp = ui.add(label.sense(Sense::hover()));
                    if xmem_pin && !is_out {
                        ui.painter()
                            .circle_filled(resp.rect.center(), dot_r, ERR_RED);
                    }
                });
                if bit > 0 {
                    ui.add_space(-4.0);
                }
            }
        });
    }

    ui.add_space(8.0);
    ui.separator();
    ui.add_space(4.0);
    ui.label(
        RichText::new("  \u{2588} OUT HIGH    \u{2591} OUT LOW    \u{00B7} INPUT")
            .monospace().size(11.0).color(START_GREEN_DIM),
    );
    if xmem_active {
        ui.add_space(2.0);
        ui.label(
            RichText::new("")
                .monospace().size(11.0).color(ERR_RED),
        );
    }
}

// timers_tab
fn show_timers_tab(ui: &mut Ui, cpu: &Cpu) -> SimAction {
    let mut action = SimAction::None;
    // data_addr_to_io_idx
    let io = &cpu.io;
    let ix = |a: u16| -> u8 { io[(a as usize) - 0x0020] };

    let tifr  = ix(io_map::TIFR);
    let timsk = ix(io_map::TIMSK);

    // timer0_ui
    timer_section(ui, "TIMER 0", "(8-bit)");

    let tccr0 = ix(io_map::TCCR0);
    let tcnt0 = ix(io_map::TCNT0);
    let ocr0  = ix(io_map::OCR0);
    let cs0   = tccr0 & 0x07;
    let ctc0  = (tccr0 & 0x08) != 0;

    Grid::new("t0_grid").num_columns(3).spacing([8.0, 2.0]).show(ui, |ui| {
        kv3(ui, "TCCR0", &format!("{tccr0:02X}"),
            &format!("{}  {}", t01_cs_str(cs0), if ctc0 { "CTC" } else { "Normal" }));
        kv3(ui, "TCNT0", &format!("{tcnt0:02X}"), &format!("[{}]", tcnt0));
        kv3(ui, "OCR0",  &format!("{ocr0:02X}"),  &format!("[{}]", ocr0));
    });
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        flag_lbl(ui, "TOV",  tifr & 0x01 != 0);
        flag_lbl(ui, "OCF",  tifr & 0x02 != 0);
        ui.label(RichText::new(" | ").monospace().size(11.0).color(DIM));
        flag_lbl(ui, "TOIE", timsk & 0x01 != 0);
        flag_lbl(ui, "OCIE", timsk & 0x02 != 0);
    });

    ui.add_space(6.0);
    ui.separator();
    ui.add_space(4.0);

    // timer1_ui
    timer_section(ui, "TIMER 1", "(16-bit)");

    let tccr1a = ix(io_map::TCCR1A);
    let tccr1b = ix(io_map::TCCR1B);
    let tcnt1  = (ix(io_map::TCNT1H) as u16) << 8 | ix(io_map::TCNT1L) as u16;
    let ocr1a  = (ix(io_map::OCR1AH) as u16) << 8 | ix(io_map::OCR1AL) as u16;
    let ocr1b  = (ix(io_map::OCR1BH) as u16) << 8 | ix(io_map::OCR1BL) as u16;
    let cs1    = tccr1b & 0x07;
    let ctc1   = (tccr1b & 0x08) != 0;

    Grid::new("t1_grid").num_columns(3).spacing([8.0, 2.0]).show(ui, |ui| {
        kv3(ui, "TCCR1A", &format!("{tccr1a:02X}"), "");
        kv3(ui, "TCCR1B", &format!("{tccr1b:02X}"),
            &format!("{}  {}", t01_cs_str(cs1), if ctc1 { "CTC" } else { "Normal" }));
        kv3(ui, "TCNT1",  &format!("{tcnt1:04X}"), &format!("[{}]", tcnt1));
        kv3(ui, "OCR1A",  &format!("{ocr1a:04X}"), &format!("[{}]", ocr1a));
        kv3(ui, "OCR1B",  &format!("{ocr1b:04X}"), &format!("[{}]", ocr1b));
    });
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        flag_lbl(ui, "TOV1",   tifr & 0x04 != 0);
        flag_lbl(ui, "OCF1A",  tifr & 0x10 != 0);
        flag_lbl(ui, "OCF1B",  tifr & 0x08 != 0);
        ui.label(RichText::new(" | ").monospace().size(11.0).color(DIM));
        flag_lbl(ui, "TOIE1",  timsk & 0x04 != 0);
        flag_lbl(ui, "OCIE1A", timsk & 0x10 != 0);
        flag_lbl(ui, "OCIE1B", timsk & 0x08 != 0);
    });

    ui.add_space(6.0);
    ui.separator();
    ui.add_space(4.0);

    // timer2_ui
    timer_section(ui, "TIMER 2", "(8-bit async)");

    let tccr2 = ix(io_map::TCCR2);
    let tcnt2 = ix(io_map::TCNT2);
    let ocr2  = ix(io_map::OCR2);
    let cs2   = tccr2 & 0x07;
    let ctc2  = (tccr2 & 0x08) != 0;

    Grid::new("t2_grid").num_columns(3).spacing([8.0, 2.0]).show(ui, |ui| {
        kv3(ui, "TCCR2", &format!("{tccr2:02X}"),
            &format!("{}  {}", t2_cs_str(cs2), if ctc2 { "CTC" } else { "Normal" }));
        kv3(ui, "TCNT2", &format!("{tcnt2:02X}"), &format!("[{}]", tcnt2));
        kv3(ui, "OCR2",  &format!("{ocr2:02X}"),  &format!("[{}]", ocr2));
    });
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        flag_lbl(ui, "TOV2",  tifr & 0x40 != 0);
        flag_lbl(ui, "OCF2",  tifr & 0x80 != 0);
        ui.label(RichText::new(" | ").monospace().size(11.0).color(DIM));
        flag_lbl(ui, "TOIE2", timsk & 0x40 != 0);
        flag_lbl(ui, "OCIE2", timsk & 0x80 != 0);
    });

    ui.add_space(6.0);
    ui.separator();
    ui.add_space(4.0);

    if cpu.has_timer3() {
        // timer3_ui
        timer_section(ui, "TIMER 3", "(16-bit)");

        let etifr  = ix(io_map::ETIFR);
        let etimsk = ix(io_map::ETIMSK);

        let tccr3a = ix(io_map::TCCR3A);
        let tccr3b = ix(io_map::TCCR3B);
        let tccr3c = ix(io_map::TCCR3C);
        let tcnt3  = (ix(io_map::TCNT3H) as u16) << 8 | ix(io_map::TCNT3L) as u16;
        let ocr3a  = (ix(io_map::OCR3AH) as u16) << 8 | ix(io_map::OCR3AL) as u16;
        let ocr3b  = (ix(io_map::OCR3BH) as u16) << 8 | ix(io_map::OCR3BL) as u16;
        let ocr3c  = (ix(io_map::OCR3CH) as u16) << 8 | ix(io_map::OCR3CL) as u16;
        let cs3    = tccr3b & 0x07;
        let ctc3   = (tccr3b & 0x08) != 0;

        Grid::new("t3_grid").num_columns(3).spacing([8.0, 2.0]).show(ui, |ui| {
            kv3(ui, "TCCR3A", &format!("{tccr3a:02X}"), "");
            kv3(ui, "TCCR3B", &format!("{tccr3b:02X}"),
                &format!("{}  {}", t01_cs_str(cs3), if ctc3 { "CTC" } else { "Normal" }));
            kv3(ui, "TCCR3C", &format!("{tccr3c:02X}"), "");
            kv3(ui, "TCNT3",  &format!("{tcnt3:04X}"), &format!("[{}]", tcnt3));
            kv3(ui, "OCR3A",  &format!("{ocr3a:04X}"), &format!("[{}]", ocr3a));
            kv3(ui, "OCR3B",  &format!("{ocr3b:04X}"), &format!("[{}]", ocr3b));
            kv3(ui, "OCR3C",  &format!("{ocr3c:04X}"), &format!("[{}]", ocr3c));
        });
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            flag_lbl(ui, "TOV3",   etifr & 0x10 != 0);
            flag_lbl(ui, "OCF3A",  etifr & 0x08 != 0);
            flag_lbl(ui, "OCF3B",  etifr & 0x04 != 0);
            flag_lbl(ui, "OCF3C",  etifr & 0x02 != 0);
            ui.label(RichText::new(" | ").monospace().size(11.0).color(DIM));
            flag_lbl(ui, "TOIE3",  etimsk & 0x10 != 0);
            flag_lbl(ui, "OCIE3A", etimsk & 0x08 != 0);
            flag_lbl(ui, "OCIE3B", etimsk & 0x04 != 0);
            flag_lbl(ui, "OCIE3C", etimsk & 0x02 != 0);
        });

        ui.add_space(6.0);
        ui.separator();
        ui.add_space(4.0);
    }

    // timsk_tifr_raw
    section_label(ui, "REGISTERS (raw)");
    ui.add_space(2.0);
    Grid::new("tmr_raw").num_columns(3).spacing([8.0, 2.0]).show(ui, |ui| {
        kv3(ui, "TIMSK",  &format!("{timsk:02X}"),  &format!("{timsk:08b}b"));
        kv3(ui, "TIFR",   &format!("{tifr:02X}"),   &format!("{tifr:08b}b"));
        if cpu.has_timer3() {
            let etimsk = ix(io_map::ETIMSK);
            let etifr  = ix(io_map::ETIFR);
            kv3(ui, "ETIMSK", &format!("{etimsk:02X}"), &format!("{etimsk:08b}b"));
            kv3(ui, "ETIFR",  &format!("{etifr:02X}"),  &format!("{etifr:08b}b"));
        }
    });

    // manual_interrupt_triggers
    ui.add_space(8.0);
    ui.separator();
    ui.add_space(4.0);
    section_label(ui, "MANUAL IRQ TRIGGERS");
    ui.add_space(4.0);
    ui.label(
        RichText::new("Force-set interrupt flags to test ISRs (SREG I must be set).")
            .monospace().size(10.5).color(DIM),
    );
    ui.add_space(6.0);

    let mut trig_btn = |ui: &mut Ui, label: &str, addr: u16, mask: u8| {
        if ui.add(
            Button::new(RichText::new(label).monospace().size(11.0).color(Color32::BLACK))
                .fill(START_GREEN_DIM)
                .stroke(Stroke::new(1.0, START_GREEN)),
        ).clicked() {
            action = SimAction::SetIoBit { addr, mask };
        }
    };

    timer_section(ui, "TIMER 0 triggers", "");
    ui.horizontal(|ui| {
        trig_btn(ui, "TOV0", io_map::TIFR, 0x01);
        ui.add_space(4.0);
        trig_btn(ui, "OCF0", io_map::TIFR, 0x02);
    });
    ui.add_space(4.0);

    timer_section(ui, "TIMER 1 triggers", "");
    ui.horizontal(|ui| {
        trig_btn(ui, "TOV1",  io_map::TIFR, 0x04);
        ui.add_space(4.0);
        trig_btn(ui, "OCF1A", io_map::TIFR, 0x10);
        ui.add_space(4.0);
        trig_btn(ui, "OCF1B", io_map::TIFR, 0x08);
        ui.add_space(4.0);
        trig_btn(ui, "ICF1",  io_map::TIFR, 0x20);
    });
    ui.add_space(4.0);

    timer_section(ui, "TIMER 2 triggers", "");
    ui.horizontal(|ui| {
        trig_btn(ui, "TOV2", io_map::TIFR, 0x40);
        ui.add_space(4.0);
        trig_btn(ui, "OCF2", io_map::TIFR, 0x80);
    });
    if cpu.has_timer3() {
        ui.add_space(4.0);
        timer_section(ui, "TIMER 3 triggers", "");
        ui.horizontal(|ui| {
            trig_btn(ui, "TOV3",  io_map::ETIFR, 0x10);
            ui.add_space(4.0);
            trig_btn(ui, "OCF3A", io_map::ETIFR, 0x08);
            ui.add_space(4.0);
            trig_btn(ui, "OCF3B", io_map::ETIFR, 0x04);
            ui.add_space(4.0);
            trig_btn(ui, "OCF3C", io_map::ETIFR, 0x02);
        });
    }

    action
}

/// How many Port C pins are required to address `size` bytes of external memory.
fn xmem_portc_pins(size: u32) -> u8 {
    if size <= 256 { return 0; }
    // bits_needed = ceil(log2(size)) = 32 - leading_zeros(size - 1)
    let bits = 32u32.saturating_sub((size - 1).leading_zeros());
    bits.saturating_sub(8).min(8) as u8
}

// sram_tab
fn show_sram_tab(ui: &mut Ui, cpu: &Cpu, xmem: &mut XmemState) -> SimAction {
    let mut action = SimAction::None;
    let sp = cpu.sp;
    let ram_start = cpu.ram_start();
    let ram_end = cpu.ram_end();
    let xmem_base = cpu.xmem_base() as u32;

    // ── XMEM config ──────────────────────────────────────────────────────────
    let xmem_supported = cpu.has_xmem();
    let xmem_active = xmem_supported && !cpu.xmem.is_empty();
    let xmem_size   = cpu.xmem.len() as u32;

    if xmem_supported {
        ui.separator();
        ui.add_space(2.0);
        section_label(ui, "EXTERNAL SRAM (XMEM)");
        ui.add_space(4.0);

        ui.label(
        RichText::new(
            "Maps data addresses 0x1100–0x(end) to an external SRAM chip via a \
             multiplexed bus. Hardware pins are assigned automatically; no DDR \
             configuration is required or possible for these pins.",
        )
        .monospace().size(10.0).color(DIM),
    );
        ui.add_space(6.0);

    // size_input_row
        let parsed_size: Option<u32> = xmem.size_text.trim().parse::<u32>().ok()
        .filter(|&v| v > 0 && v <= XMEM_MAX);
        let input_ok = parsed_size.is_some();

        ui.horizontal(|ui| {
        ui.label(RichText::new("Size:").monospace().size(11.0).color(START_GREEN_DIM));
        let resp = ui.add(
            TextEdit::singleline(&mut xmem.size_text)
                .desired_width(72.0)
                .font(egui::TextStyle::Monospace),
        );
        ui.label(RichText::new("bytes").monospace().size(11.0).color(DIM));
        ui.add_space(4.0);
        ui.label(
            RichText::new(format!("(max {})", XMEM_MAX))
                .monospace().size(10.0).color(DIM),
        );
        let _ = resp;
        });

        if !input_ok && !xmem.size_text.trim().is_empty() {
        ui.label(
            RichText::new(format!("✗ must be 1–{XMEM_MAX}"))
                .monospace().size(10.5).color(ERR_RED),
        );
        }

        ui.add_space(4.0);
        ui.horizontal(|ui| {
        // ENABLE button
        let can_enable = input_ok;
        if ui.add_enabled(
            can_enable,
            Button::new(RichText::new("ENABLE XMEM").monospace().size(11.0).color(Color32::BLACK))
                .fill(if xmem_active { AMBER } else { START_GREEN_DIM })
                .stroke(Stroke::new(1.0, START_GREEN)),
        ).clicked() {
            if let Some(sz) = parsed_size { action = SimAction::SetXmem(sz); }
        }
        ui.add_space(6.0);
        if ui.add(
            Button::new(RichText::new("DISABLE").monospace().size(11.0).color(Color32::BLACK))
                .fill(DIM)
                .stroke(Stroke::new(1.0, DIM)),
        ).clicked() {
            action = SimAction::SetXmem(0);
        }
        if xmem_active {
            ui.add_space(8.0);
            ui.label(
                RichText::new(format!("ACTIVE  {xmem_size} B  (0x{xmem_base:04X}–0x{:04X})", xmem_base + xmem_size - 1))
                    .monospace().size(11.0).color(AMBER),
            );
        }
        });

    // pin_assignment
        if xmem_active {
        let portc_n = xmem_portc_pins(xmem_size);
        let portc_mask: u8 = if portc_n >= 8 { 0xFF } else { (1u8 << portc_n).wrapping_sub(1) };

        ui.add_space(6.0);
        section_label(ui, "DEDICATED PINS");
        ui.add_space(2.0);
        ui.label(
            RichText::new("PG0=/RD  PG1=/WR  PG2=ALE")
                .monospace().size(11.0).color(START_GREEN_DIM),
        );
        ui.label(
            RichText::new("PA0–PA7 = XAD0–XAD7   (data + lower address, always)")
                .monospace().size(11.0).color(START_GREEN_DIM),
        );
        if portc_n == 0 {
            ui.label(
                RichText::new("Port C: free  (size ≤ 256 B, upper address not needed)")
                    .monospace().size(11.0).color(DIM),
            );
        } else {
            let pc_hi = portc_n - 1;
            ui.label(
                RichText::new(format!("PC0–PC{pc_hi} = XA8–XA{}   (upper address)", 7 + portc_n))
                    .monospace().size(11.0).color(START_GREEN_DIM),
            );
            if portc_n < 8 {
                let free_lo = portc_n;
                ui.label(
                    RichText::new(format!("PC{free_lo}–PC7: free  ({} pins)", 8 - portc_n))
                        .monospace().size(11.0).color(DIM),
                );
            }
        }

        // conflict_detection
        let ddra  = cpu.read_mem(io_map::DDRA);
        let ddrc  = cpu.read_mem(io_map::DDRC);
        let ddrg  = cpu.read_mem(io_map::DDRG);

        let conflict_a = ddra != 0;
        let conflict_c = portc_n > 0 && (ddrc & portc_mask) != 0;
        let conflict_g = (ddrg & 0x07) != 0;

        if conflict_a || conflict_c || conflict_g {
            ui.add_space(4.0);
            section_label(ui, "PIN CONFLICTS");
            ui.add_space(2.0);
            if conflict_a {
                ui.label(
                    RichText::new(format!("⚠ DDRA=0x{ddra:02X}: Port A pins set as GPIO output — XMEM takes priority"))
                        .monospace().size(10.5).color(ERR_RED),
                );
            }
            if conflict_c {
                ui.label(
                    RichText::new(format!("⚠ DDRC=0x{ddrc:02X}: Port C pins PC0–PC{} conflict with XA8–XA{}", portc_n-1, 7+portc_n))
                        .monospace().size(10.5).color(ERR_RED),
                );
            }
            if conflict_g {
                ui.label(
                    RichText::new(format!("⚠ DDRG=0x{ddrg:02X}: PG0–PG2 (/RD,/WR,ALE) conflict with GPIO output"))
                        .monospace().size(10.5).color(ERR_RED),
                );
            }
        }
        }
    }

    ui.add_space(6.0);
    ui.separator();
    ui.add_space(4.0);

    // sp_status
    section_label(ui, &format!("SRAM  0x{ram_start:04X} – 0x{ram_end:04X}  ({} bytes)", cpu.sram.len()));
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        ui.label(RichText::new("SP →").monospace().size(12.0).color(AMBER));
        ui.label(
            RichText::new(format!("0x{sp:04X}"))
                .monospace().size(12.0).color(START_GREEN),
        );
        let sp_in_sram = sp >= ram_start && sp <= ram_end;
        if sp_in_sram {
            let depth = ram_end.wrapping_sub(sp);
            ui.add_space(8.0);
            ui.label(
                RichText::new(format!("(stack depth: {depth} B)"))
                    .monospace().size(11.0).color(START_GREEN_DIM),
            );
        } else if sp == 0x0000 {
            ui.add_space(8.0);
            ui.label(
                RichText::new("(uninitialized)").monospace().size(11.0).color(DIM),
            );
        }
    });
    ui.add_space(4.0);

    // sp_row_index
    let sp_row: Option<usize> = if sp >= ram_start && sp <= ram_end {
        let sram_off = (sp - ram_start) as usize;
        Some(sram_off / 8)
    } else {
        None
    };

    let sram  = &cpu.sram;
    let rows  = sram.len() / 8; // 512x8

    Grid::new("sram_grid")
        .num_columns(10)  // addr 8bytes mark
        .spacing([5.0, 1.5])
        .show(ui, |ui| {
            // header
            ui.label(RichText::new("ADDR").monospace().size(11.0).color(START_GREEN_DIM));
            for col in 0..8usize {
                ui.label(
                    RichText::new(format!("+{col:X}"))
                        .monospace().size(11.0).color(START_GREEN_DIM),
                );
            }
            ui.label(RichText::new("").monospace().size(11.0).color(DIM)); // mark_hdr
            ui.end_row();

            // data_rows
            let mut skipping = false;

            for row in 0..rows {
                let base       = row * 8;
                let addr       = ram_start as u32 + base as u32;
                let slice      = &sram[base..base + 8];
                let all0       = slice.iter().all(|&b| b == 0);
                let is_sp_row  = sp_row == Some(row);

                // show_sp_row row0 nonzero_rows
                if all0 && row > 0 && !is_sp_row {
                    if !skipping {
                        skipping = true;
                        ui.label(RichText::new("  ···").monospace().size(10.5).color(DIM));
                        for _ in 0..8 {
                            ui.label(RichText::new("··").monospace().size(10.5).color(DIM));
                        }
                        ui.label(RichText::new("").monospace().size(10.5).color(DIM));
                        ui.end_row();
                    }
                    continue;
                }
                skipping = false;

                // addr_col
                let addr_color = if is_sp_row { AMBER } else { START_GREEN_DIM };
                ui.label(
                    RichText::new(format!("{addr:04X}"))
                        .monospace().size(11.0).color(addr_color),
                );

                // byte_cols
                for (col_idx, &b) in slice.iter().enumerate() {
                    let byte_addr = addr + col_idx as u32;
                    let is_sp_byte = byte_addr == sp as u32;
                    let color = if is_sp_byte { AMBER }
                                else if b != 0 { START_GREEN }
                                else { DIM };
                    ui.label(
                        RichText::new(format!("{b:02X}"))
                            .monospace().size(11.0).color(color),
                    );
                }

                // sp_marker_col
                if is_sp_row {
                    ui.label(
                        RichText::new(format!("\u{2190} SP {:04X}", sp))
                            .monospace().size(10.5).color(AMBER),
                    );
                } else {
                    ui.label(RichText::new("").monospace().size(11.0).color(DIM));
                }
                ui.end_row();
            }
        });

    // xmem_contents
    if xmem_active && xmem_size > 0 {
        ui.add_space(6.0);
        ui.separator();
        ui.add_space(4.0);
        section_label(ui, &format!("XMEM  0x{xmem_base:04X} – 0x{:04X}  ({xmem_size} bytes)", xmem_base + xmem_size - 1));
        ui.add_space(4.0);

        ui.label(
            RichText::new("  ADDR    +0   +1   +2   +3   +4   +5   +6   +7")
                .monospace().size(10.5).color(START_GREEN_DIM),
        );
        ui.add_space(2.0);

        let mut skipping = false;
        let rows = ((xmem_size as usize) + 7) / 8;
        for row in 0..rows {
            let base = row * 8;
            let slice_end = (base + 8).min(xmem_size as usize);
            let slice = &cpu.xmem[base..slice_end];
            let all0 = slice.iter().all(|&b| b == 0);
            if all0 && row > 0 {
                if !skipping {
                    skipping = true;
                    ui.label(RichText::new("  ···").monospace().size(10.5).color(DIM));
                }
                continue;
            }
            skipping = false;
            let addr = xmem_base + base as u32;
            let mut line = format!("  0x{addr:04X}  ");
            for b in slice { line.push_str(&format!(" {b:02X}  ")); }
            ui.label(RichText::new(line).monospace().size(10.5).color(START_GREEN));
        }
    }

    // eeprom_section
    {
        ui.add_space(6.0);
        ui.separator();
        ui.add_space(4.0);
        section_label(ui, &format!("EEPROM  0x000 – 0x{:03X}  ({} bytes, non-volatile)", cpu.eeprom.len().saturating_sub(1), cpu.eeprom.len()));
        ui.add_space(2.0);
        ui.label(
            RichText::new("  Persists across reset. Unprogrammed bytes = 0xFF.")
                .size(11.0).color(START_GREEN_DIM),
        );
        ui.add_space(4.0);

        let ep = &cpu.eeprom;
        let rows = (ep.len() + 7) / 8;
        let mut skipping = false;

        ui.label(
            RichText::new("  ADDR    +0   +1   +2   +3   +4   +5   +6   +7")
                .monospace().size(10.5).color(START_GREEN_DIM),
        );
        ui.add_space(2.0);

        for row in 0..rows {
            let base = row * 8;
            let end  = (base + 8).min(ep.len());
            let slice = &ep[base..end];
            let all_ff = slice.iter().all(|&b| b == 0xFF);
            if all_ff {
                if !skipping {
                    skipping = true;
                    ui.label(RichText::new("  ···").monospace().size(10.5).color(DIM));
                }
                continue;
            }
            skipping = false;
            let mut line = format!("  0x{base:03X}   ");
            for b in slice { line.push_str(&format!(" {b:02X}  ")); }
            ui.label(RichText::new(line).monospace().size(10.5).color(AMBER));
        }
    }

    action
}

// stack_tab

fn show_stack_tab(ui: &mut Ui, cpu: &Cpu, s: &mut StackState) -> SimAction {
    let sp  = cpu.sp;
    let sph = (sp >> 8) as u8;
    let spl = (sp & 0xFF) as u8;
    let ram_start = cpu.ram_start();
    let ramend = cpu.ram_end();

    section_label(ui, "STACK POINTER");
    ui.add_space(4.0);
    Grid::new("sp_grid").num_columns(3).spacing([16.0, 2.0]).show(ui, |ui| {
        ui.label(RichText::new(format!("SPH  0x{sph:02X}")).monospace().size(13.0).color(AMBER));
        ui.label(RichText::new(format!("SPL  0x{spl:02X}")).monospace().size(13.0).color(AMBER));
        ui.label(
            RichText::new(format!("SP = 0x{sp:04X}")).monospace().size(13.0).color(START_GREEN),
        );
        ui.end_row();
    });

    ui.add_space(4.0);
    let stack_top = sp.wrapping_add(1);
    let depth = if sp < ramend { ramend - sp } else { 0 };

    if sp == 0 {
        ui.label(RichText::new("SP not initialized (0x0000)").monospace().size(11.0).color(DIM));
    } else if stack_top > ramend {
        ui.label(RichText::new("Stack empty (SP = RAMEND)").monospace().size(11.0).color(DIM));
    } else {
        ui.label(
            RichText::new(format!("Stack depth: {depth} bytes  (0x{stack_top:04X} – 0x{ramend:04X})"))
                .monospace().size(11.5).color(START_GREEN_DIM),
        );
    }

    ui.add_space(6.0);
    if ui.add(
        Button::new(RichText::new("STACK FRAMES").monospace().size(11.5).color(Color32::BLACK))
            .fill(START_GREEN_DIM)
            .stroke(Stroke::new(1.0, START_GREEN)),
    ).clicked() {
        s.popup_open = true;
    }

    // stack_bytes_grid
    ui.add_space(6.0);
    ui.separator();
    ui.add_space(4.0);
    section_label(ui, "STACK CONTENTS  (SP+1 → RAMEND)");
    ui.add_space(4.0);

    if depth == 0 || sp == 0 {
        ui.label(RichText::new("(empty)").monospace().size(11.0).color(DIM));
    } else {
        // header
        ui.label(
            RichText::new("  ADDR    +0   +1   +2   +3   +4   +5   +6   +7")
                .monospace().size(10.5).color(START_GREEN_DIM),
        );
        ui.add_space(2.0);

        let row_width = 8usize;
        let start_addr = stack_top as usize;
        let end_addr   = ramend as usize + 1;
        let first_row  = start_addr / row_width;
        let last_row   = (end_addr - 1) / row_width;

        for row in first_row..=last_row {
            let row_base = row * row_width;
            let sp_row = sp >= row_base as u16 && (sp as usize) < row_base + row_width
                         && sp >= ram_start;
            let color_row = if sp_row { AMBER } else { START_GREEN };

            let mut line = format!("  0x{row_base:04X}  ");
            let mut has_content = false;
            for col in 0..row_width {
                let addr = row_base + col;
                if addr < ram_start as usize || addr > ramend as usize {
                    line.push_str("     ");
                    continue;
                }
                if addr < start_addr {
                    line.push_str("  .. ");
                    continue;
                }
                    let b = cpu.sram[addr - ram_start as usize];
                    let is_sp = addr as u16 == sp;
                if is_sp {
                    line.push_str(&format!("[{b:02X}] "));
                } else {
                    line.push_str(&format!(" {b:02X}  "));
                }
                has_content = true;
            }
            if has_content {
                ui.label(RichText::new(line).monospace().size(10.5).color(color_row));
            }
        }
    }

    // stack_frames_popup
    if s.popup_open {
        let ctx = ui.ctx().clone();
        Window::new("__stack_frames__")
            .title_bar(false)
            .frame(
                Frame::NONE
                    .fill(Color32::from_rgb(3, 8, 3))
                    .stroke(Stroke::new(1.5, START_GREEN_DIM))
                    .inner_margin(Margin::same(14)),
            )
            .fixed_size([480.0, 420.0])
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(&ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("[ STACK FRAME ANALYSIS ]")
                            .monospace().size(13.0).color(START_GREEN),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button(RichText::new("✕").monospace().size(13.0).color(AMBER))
                            .clicked()
                        {
                            s.popup_open = false;
                        }
                    });
                });
                ui.separator();
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Heuristic: 2-byte pairs that form a valid flash word address are marked as potential return addresses.")
                        .monospace().size(10.0).color(DIM),
                );
                ui.add_space(6.0);

                if depth == 0 || sp == 0 {
                    ui.label(RichText::new("Stack is empty.").monospace().size(11.0).color(DIM));
                    return;
                }

                ui.label(
                    RichText::new(format!("{:<6} {:<6} {:<22} {}", "ADDR", "BYTES", "INTERPRETATION", "DISASM"))
                        .monospace().size(11.0).color(START_GREEN_DIM),
                );
                ui.separator();

                ScrollArea::vertical().id_salt("sf_scroll").max_height(300.0).show(ui, |ui| {
                    let mut addr = stack_top as usize;
                    while addr <= ramend as usize {
                        if addr + 1 <= ramend as usize {
                            // read 2-byte pair: AVR pushes PCH first (higher addr), PCL second (lower addr)
                            // top of stack = SP+1 = PCL, SP+2 = PCH
                            let lo = cpu.sram[addr     - ram_start as usize];
                            let hi = cpu.sram[addr + 1 - ram_start as usize];
                            let word_addr = (hi as u32) << 8 | lo as u32;

                            if word_addr > 0
                                && (word_addr as usize) < cpu.flash_words()
                                && cpu.flash[word_addr as usize] != 0
                            {
                                // looks like a return address
                                let disasm = cpu.disasm_at(word_addr);
                                let cyc    = Cpu::instr_cycles_str(cpu.flash[word_addr as usize]);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!(
                                            "0x{addr:04X}  {lo:02X} {hi:02X}  → RET 0x{word_addr:04X}"
                                        ))
                                        .monospace().size(10.5).color(AMBER),
                                    );
                                    ui.label(
                                        RichText::new(format!("  {disasm} [{cyc}cy]"))
                                            .monospace().size(10.5).color(START_GREEN_DIM),
                                    );
                                });
                                addr += 2;
                                continue;
                            }
                        }
                        // single byte (pushed variable or data)
                let b = cpu.sram[addr - ram_start as usize];
                let note = if b == 0 { " (zero)" } else { "" };
                        ui.label(
                            RichText::new(format!(
                                "0x{addr:04X}  {b:02X}      PUSH'd byte  {b:3}{note}"
                            ))
                            .monospace().size(10.5).color(START_GREEN),
                        );
                        addr += 1;
                    }
                });
            });
    }

    SimAction::None
}

// break tab
fn show_break_tab(ui: &mut Ui, bp: &mut BreakpointState) {
    section_label(ui, "BREAKPOINTS");
    ui.add_space(4.0);

    // new breakpoint
    Frame::NONE
        .stroke(Stroke::new(1.0, DIM))
        .inner_margin(Margin::same(6))
        .show(ui, |ui| {
            ui.label(RichText::new("NEW BREAKPOINT").monospace().size(11.0).color(START_GREEN_DIM));
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label(RichText::new("Addr (hex):").monospace().size(11.0).color(DIM));
                ui.add(
                    egui::TextEdit::singleline(&mut bp.new_addr_text)
                        .desired_width(56.0)
                        .font(egui::TextStyle::Monospace),
                );
            });
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Action:").monospace().size(11.0).color(DIM));
                egui::ComboBox::from_id_salt("bp_action")
                    .selected_text(
                        RichText::new(bp.new_action.label()).monospace().size(11.0).color(START_GREEN),
                    )
                    .show_ui(ui, |ui| {
                        ui.style_mut().visuals.override_text_color = Some(START_GREEN);
                        for a in [BpAction::Pause, BpAction::PrintTerm, BpAction::PrintAndPause] {
                            ui.selectable_value(
                                &mut bp.new_action, a,
                                RichText::new(a.label()).monospace().size(11.0),
                            );
                        }
                    });
            });
            if bp.new_action != BpAction::Pause {
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Message:").monospace().size(11.0).color(DIM));
                    ui.add(
                        egui::TextEdit::singleline(&mut bp.new_message)
                            .desired_width(150.0)
                            .font(egui::TextStyle::Monospace),
                    );
                });
            }
            ui.add_space(4.0);
            if ui.add(
                Button::new(RichText::new("ADD").monospace().size(11.5).color(START_GREEN))
                    .fill(Color32::from_rgb(6, 20, 6))
                    .stroke(Stroke::new(1.0, START_GREEN)),
            ).clicked() {
                let addr_str = bp.new_addr_text.trim().trim_start_matches("0x");
                if let Ok(addr) = u16::from_str_radix(addr_str, 16) {
                    let msg = if bp.new_action != BpAction::Pause && !bp.new_message.is_empty() {
                        bp.new_message.clone()
                    } else {
                        format!("BREAKPOINT hit @ 0x{addr:04X}")
                    };
                    bp.breakpoints.push(Breakpoint {
                        addr,
                        action: bp.new_action,
                        message: msg,
                        enabled: true,
                    });
                    bp.new_addr_text.clear();
                }
            }
        });

    ui.add_space(6.0);

    // bp list
    if bp.breakpoints.is_empty() {
        ui.label(RichText::new("  (none)").monospace().size(11.0).color(DIM));
        return;
    }

    let mut to_remove: Option<usize> = None;
    for (i, b) in bp.breakpoints.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.checkbox(&mut b.enabled, "");
            let addr_col = if b.enabled { AMBER } else { DIM };
            ui.label(
                RichText::new(format!("0x{:04X}", b.addr))
                    .monospace().size(11.5).color(addr_col),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new(b.action.label())
                    .monospace().size(10.5).color(START_GREEN_DIM),
            );
            if !b.message.is_empty() {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!("\"{}\"", b.message))
                        .monospace().size(10.5).color(DIM),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(
                    RichText::new("✕").monospace().size(11.0).color(ERR_RED)
                ).clicked() {
                    to_remove = Some(i);
                }
            });
        });
    }
    if let Some(i) = to_remove { bp.breakpoints.remove(i); }

    ui.add_space(6.0);
    if !bp.breakpoints.is_empty() {
        if ui.add(
            Button::new(RichText::new("CLEAR ALL").monospace().size(10.5).color(DIM))
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::new(1.0, DIM)),
        ).clicked() {
            bp.breakpoints.clear();
        }
    }
}

// flash
fn show_flash_tab(ui: &mut Ui, cpu: &Cpu, s: &mut FlashState) {
    // header
    let flash_words = cpu.flash_words();
    let flash_total_pages = flash_words.div_ceil(FLASH_PER_PAGE);
    section_label(ui, &format!(
        "FLASH  0x0000–0x{:04X}  ({} words)  page {}/{}",
        flash_words.saturating_sub(1), flash_words, s.page + 1, flash_total_pages,
    ));
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        // → PC: jump to the page that contains the current PC
        let pc_page = (cpu.pc as usize / FLASH_PER_PAGE).min(flash_total_pages.saturating_sub(1));
        if retro_btn(ui, "\u{2192}PC").clicked() {
            s.page    = pc_page;
            s.jumping = false;
        }
        ui.add_space(6.0);

        // fixed quick-tabs for pages 1–5
        for p in 0..5usize {
            if flash_page_btn(ui, &format!("{}", p + 1), s.page == p).clicked() {
                s.page    = p;
                s.jumping = false;
            }
        }
        ui.add_space(4.0);

        // if current page is outside 1–5 and not the last, show its number in diff color
        if s.page >= 5 && s.page < flash_total_pages.saturating_sub(1) {
            ui.label(
                RichText::new(format!("[{}]", s.page + 1))
                    .monospace().size(11.0).color(AMBER),
            );
            ui.add_space(2.0);
        }

        // "···" jump button / inline text input
        if s.jumping {
            let resp = ui.add(
                TextEdit::singleline(&mut s.jump_text)
                    .desired_width(46.0)
                    .font(egui::TextStyle::Monospace),
            );
            resp.request_focus();
            let enter = ui.input(|i| i.key_pressed(Key::Enter));
            if enter || resp.lost_focus() {
                if let Ok(p) = s.jump_text.trim().parse::<usize>() {
                    s.page = p.saturating_sub(1).min(flash_total_pages.saturating_sub(1));
                }
                s.jumping = false;
            }
        } else if retro_btn(ui, "···").clicked() {
            s.jumping   = true;
            s.jump_text = format!("{}", s.page + 1);
        }

        ui.add_space(4.0);

        // last page always visible
        let last = flash_total_pages.saturating_sub(1);
        if flash_page_btn(ui, &format!("{}", flash_total_pages), s.page == last).clicked() {
            s.page    = last;
            s.jumping = false;
        }
    });

    ui.add_space(4.0);
    ui.separator();
    ui.add_space(2.0);

    // col header
    ui.label(
        RichText::new("   ADDR  WORDS         DISASM")
            .monospace().size(11.0).color(START_GREEN_DIM),
    );
    ui.separator();
    ui.add_space(2.0);

    // instruction rows
    let page_start = (s.page * FLASH_PER_PAGE) as u32;
    let page_end   = (page_start + FLASH_PER_PAGE as u32).min(flash_words as u32);

    let mut addr = page_start;
    let mut zero_run_start: Option<u32> = None;

    while addr < page_end {
        let op = if (addr as usize) < flash_words {
            unsafe { *cpu.flash.get_unchecked(addr as usize) }
        } else {
            0
        };
        let nwords  = Cpu::instr_words(op);
        let op2     = if nwords == 2 && (addr as usize + 1) < flash_words {
            unsafe { *cpu.flash.get_unchecked(addr as usize + 1) }
        } else {
            0
        };
        let is_pc    = addr == cpu.pc as u32;
        let all_zero = op == 0 && (nwords == 1 || op2 == 0);

        // accumulate zero runs (never skip the PC row)
        if all_zero && !is_pc {
            if zero_run_start.is_none() { zero_run_start = Some(addr); }
            addr += nwords as u32;
            continue;
        }

        // skip marker when the zero run ends
        if let Some(start) = zero_run_start.take() {
            let count = addr - start;
            ui.label(
                RichText::new(format!("   ···  ({count} empty words)"))
                    .monospace().size(10.5).color(DIM),
            );
        }

        // row
        let arrow     = if is_pc { "\u{2192}" } else { " " };
        let words_str = if nwords == 2 {
            format!("{op:04X} {op2:04X}")
        } else {
            format!("{op:04X}     ")
        };
        let disasm = cpu.disasm_at(addr);
        let ivt    = cpu.ivt_name(addr)
            .map(|n| format!("  ; <{n}>"))
            .unwrap_or_default();
        let (color, size) = if is_pc { (AMBER, 12.5_f32) } else { (START_GREEN, 12.0_f32) };

        ui.label(
            RichText::new(format!("{arrow}  {addr:04X}  {words_str}  {disasm}{ivt}"))
                .monospace().size(size).color(color),
        );

        addr += nwords as u32;
    }

    // trailing zero-run marker
    if let Some(start) = zero_run_start.take() {
        let count = page_end - start;
        if count > 0 {
            ui.label(
                RichText::new(format!("   ···  ({count} empty words)"))
                    .monospace().size(10.5).color(DIM),
            );
        }
    }
}

// format helper
fn flash_page_btn(ui: &mut Ui, label: &str, selected: bool) -> egui::Response {
    let color  = if selected { AMBER }                          else { START_GREEN_DIM };
    let fill   = if selected { Color32::from_rgb(30, 20, 0) }  else { Color32::from_rgb(6, 16, 6) };
    let stroke  = if selected { AMBER }                         else { DIM };
    ui.add(
        Button::new(RichText::new(label).monospace().size(11.5).color(color))
            .fill(fill)
            .stroke(Stroke::new(1.0, stroke)),
    )
}

fn section_label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).monospace().size(11.0).color(START_GREEN_DIM));
}

fn timer_section(ui: &mut Ui, name: &str, detail: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(name).monospace().size(12.0).color(START_GREEN));
        ui.add_space(4.0);
        ui.label(RichText::new(detail).monospace().size(11.0).color(START_GREEN_DIM));
    });
    ui.add_space(2.0);
}

/// kv_row grid helper
fn kv3(ui: &mut Ui, key: &str, val: &str, ann: &str) {
    ui.label(RichText::new(key).monospace().size(11.0).color(START_GREEN_DIM));
    let vcolor = if val.trim_start_matches('0').is_empty() || val == "0000" || val == "00" {
        DIM
    } else {
        AMBER
    };
    ui.label(RichText::new(val).monospace().size(11.0).color(vcolor));
    ui.label(RichText::new(ann).monospace().size(11.0).color(DIM));
    ui.end_row();
}

fn flag_lbl(ui: &mut Ui, name: &str, set: bool) {
    let color = if set { AMBER } else { DIM };
    ui.label(
        RichText::new(format!("{name}:{}", u8::from(set)))
            .monospace().size(11.0).color(color),
    );
}

fn t01_cs_str(cs: u8) -> &'static str {
    match cs {
        0 => "stopped", 1 => "CLK/1",  2 => "CLK/8",
        3 => "CLK/64",  4 => "CLK/256", 5 => "CLK/1024",
        6 => "ext↓",    7 => "ext↑",    _ => "?",
    }
}

fn t2_cs_str(cs: u8) -> &'static str {
    match cs {
        0 => "stopped",  1 => "CLK/1",   2 => "CLK/8",
        3 => "CLK/32",   4 => "CLK/64",  5 => "CLK/128",
        6 => "CLK/256",  7 => "CLK/1024", _ => "?",
    }
}

fn retro_btn(ui: &mut Ui, label: &str) -> egui::Response {
    ui.add(
        Button::new(RichText::new(label).monospace().size(12.0).color(START_GREEN))
            .fill(Color32::from_rgb(6, 16, 6))
            .stroke(Stroke::new(1.0, START_GREEN_DIM)),
    )
}

fn assemble_btn(ui: &mut Ui, label: &str) -> egui::Response {
    ui.add(
        Button::new(RichText::new(label).monospace().size(12.5).color(START_GREEN))
            .fill(Color32::from_rgb(8, 24, 8))
            .stroke(Stroke::new(1.0, START_GREEN)),
    )
}

fn fmt_ips(ips: f64, running: bool) -> String {
    if !running && ips == 0.0 { return "---".into(); }
    fmt_ips_plain(ips)
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
