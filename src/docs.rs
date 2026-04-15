//! flash_locations_overlay
use eframe::egui::{
    self, Align, Button, Color32, Frame, Grid, Layout, Margin, RichText, ScrollArea, Stroke, Ui,
};
use crate::avr::cpu::Cpu;
use crate::avr::McuModel;
use crate::theme;
use crate::theme::{START_GREEN, START_GREEN_DIM};

const FOCUS: Color32 = theme::FOCUS;
const DIM: Color32 = theme::DIM_GRAY;
const SEC_COL: Color32 = theme::SECTION;

pub fn show_flash_locations_window(
    ctx: &egui::Context,
    open: &mut bool,
    assembled_board: Option<McuModel>,
    cpu: &Cpu,
) {
    if !*open {
        return;
    }
    let screen = ctx.screen_rect();
    let w = screen.width() * 0.88;
    let h = screen.height() * 0.88;

    egui::Window::new("flash_locations_win")
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .fixed_size([w, h])
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .frame(
            Frame::NONE
                .fill(theme::PANEL_DEEP)
                .stroke(Stroke::new(1.5, START_GREEN))
                .inner_margin(Margin::same(16)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                Frame::NONE
                    .fill(theme::BUTTON_FILL_STRONG)
                    .inner_margin(Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("[ FLASH LOCATIONS ]")
                                .monospace()
                                .size(16.0)
                                .color(START_GREEN),
                        );
                    });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(
                            Button::new(RichText::new("  X  ").monospace().size(13.0).color(START_GREEN))
                                .fill(theme::BUTTON_FILL_STRONG)
                                .stroke(Stroke::new(1.0, START_GREEN)),
                        )
                        .clicked()
                    {
                        *open = false;
                    }
                });
            });

            let board_known = assembled_board.is_some();
            ui.add_space(8.0);
            if let Some(board) = assembled_board {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("Target: {}", board.label()))
                            .monospace()
                            .size(14.0)
                            .color(START_GREEN_DIM),
                    );
                });
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!(
                        "Memory map ({}):  data SRAM 0x{:04X}–0x{:04X}  ({} B)  ·  program flash {} words  ·  EEPROM {} B",
                        board.label(),
                        cpu.ram_start(),
                        cpu.ram_end(),
                        cpu.sram.len(),
                        cpu.flash_words(),
                        cpu.eeprom.len(),
                    ))
                    .monospace()
                    .size(11.0)
                    .color(DIM),
                );
            } else {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Target: —  (assemble with a `.board` line to fix IVT addresses)")
                            .monospace()
                            .size(13.0)
                            .color(theme::ACCENT_DIM),
                    );
                });
            }
            ui.separator();
            ui.add_space(4.0);

            ScrollArea::vertical()
                .id_salt("flash_loc_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    show_ivt_section(ui, cpu, board_known);
                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(6.0);
                    show_code_regions_section(ui, cpu, board_known);
                });
        });
}

fn vector_content(cpu: &Cpu, addr: u32) -> (String, bool) {
    let flash_words = cpu.flash_words();
    let op = if (addr as usize) < flash_words {
        cpu.flash[addr as usize]
    } else {
        0
    };
    if op == 0x0000 {
        return ("EMPTY".to_string(), false);
    }
    if (op & 0xFE0E) == 0x940C {
        let next = if (addr as usize + 1) < flash_words {
            cpu.flash[addr as usize + 1]
        } else {
            0
        };
        let target = (((op & 0x01F0) as u32) << 17)
            | (((op & 0x0001) as u32) << 16)
            | (next as u32);
        return (format!("JMP  → 0x{target:04X}"), true);
    }
    if (op & 0xF000) == 0xC000 {
        let raw = op & 0x0FFF;
        let k = (((raw << 4) as i16) >> 4) as i32;
        let target = (addr as i32 + 1 + k) as u32;
        let sign = if k >= 0 { "+" } else { "" };
        return (format!("RJMP → 0x{target:04X}  ({sign}{k})"), true);
    }
    (format!("{}  [non-jump]", cpu.disasm_at(addr)), true)
}

fn show_ivt_section(ui: &mut Ui, cpu: &Cpu, board_known: bool) {
    if !board_known {
        ui.label(
            RichText::new("INTERRUPT VECTOR TABLE  (0x0000 – 0x????, byte addresses)")
                .monospace()
                .size(12.5)
                .color(SEC_COL),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new("  ADDR   VECTOR           CONTENT")
                .monospace()
                .size(11.0)
                .color(DIM),
        );
        ui.separator();
        ui.add_space(2.0);
        Grid::new("ivt_grid_unknown")
            .num_columns(3)
            .spacing([10.0, 2.0])
            .show(ui, |ui| {
                for _ in 0..32 {
                    ui.label(
                        RichText::new("  ???")
                            .monospace()
                            .size(11.5)
                            .color(START_GREEN_DIM),
                    );
                    ui.label(
                        RichText::new("???")
                            .monospace()
                            .size(11.5)
                            .color(DIM),
                    );
                    ui.label(
                        RichText::new("???")
                            .monospace()
                            .size(11.5)
                            .color(DIM),
                    );
                    ui.end_row();
                }
            });
        ui.add_space(4.0);
        ui.label(
            RichText::new("  Code area begins at 0x???? (byte address), first word after IVT")
                .monospace()
                .size(11.0)
                .color(DIM),
        );
        return;
    }

    let ivt_end_word = cpu.ivt_end_word();
    let ivt_end_byte = ivt_end_word * 2;
    ui.label(
        RichText::new(format!(
            "INTERRUPT VECTOR TABLE  (0x0000 – 0x{ivt_end_byte:04X}, byte addresses)"
        ))
        .monospace()
        .size(12.5)
        .color(SEC_COL),
    );
    ui.add_space(4.0);

    ui.label(
        RichText::new("  ADDR   VECTOR           CONTENT")
            .monospace()
            .size(11.0)
            .color(DIM),
    );
    ui.separator();
    ui.add_space(2.0);

    Grid::new("ivt_grid")
        .num_columns(3)
        .spacing([10.0, 2.0])
        .show(ui, |ui| {
            for addr in 0..=ivt_end_word {
                let Some(name) = cpu.ivt_name(addr) else { continue; };
                let (content, occupied) = vector_content(cpu, addr);
                let name_col = if occupied { FOCUS } else { DIM };
                let cont_col = if occupied { START_GREEN } else { DIM };
                let addr_byte = addr * 2;

                ui.label(
                    RichText::new(format!("  0x{addr_byte:04X}"))
                        .monospace()
                        .size(11.5)
                        .color(START_GREEN_DIM),
                );
                ui.label(
                    RichText::new(format!("{name:<16}"))
                        .monospace()
                        .size(11.5)
                        .color(name_col),
                );
                ui.label(
                    RichText::new(content).monospace().size(11.5).color(cont_col),
                );
                ui.end_row();
            }
        });

    ui.add_space(4.0);
    ui.label(
        RichText::new(format!(
            "  Code area begins at 0x{:04X} (byte address), first word after IVT",
            (ivt_end_word + 1) * 2
        ))
        .monospace()
        .size(11.0)
        .color(DIM),
    );
}

fn show_code_regions_section(ui: &mut Ui, cpu: &Cpu, board_known: bool) {
    if !board_known {
        ui.label(
            RichText::new("CODE REGIONS  (non-empty flash beyond IVT end 0x???? bytes)")
                .monospace()
                .size(12.5)
                .color(SEC_COL),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new("  START    END      WORDS    DISASM (first instr)")
                .monospace()
                .size(11.0)
                .color(DIM),
        );
        ui.separator();
        ui.add_space(2.0);
        Grid::new("code_regions_unknown")
            .num_columns(4)
            .spacing([10.0, 2.0])
            .show(ui, |ui| {
                ui.label(
                    RichText::new("  ???")
                        .monospace()
                        .size(11.5)
                        .color(FOCUS),
                );
                ui.label(
                    RichText::new("???")
                        .monospace()
                        .size(11.5)
                        .color(START_GREEN_DIM),
                );
                ui.label(
                    RichText::new("???")
                        .monospace()
                        .size(11.5)
                        .color(START_GREEN),
                );
                ui.label(
                    RichText::new("???")
                        .monospace()
                        .size(11.5)
                        .color(DIM),
                );
                ui.end_row();
            });
        return;
    }

    let ivt_end_word = cpu.ivt_end_word();
    let ivt_end_byte = ivt_end_word * 2;
    ui.label(
        RichText::new(format!(
            "CODE REGIONS  (non-empty flash words beyond IVT end 0x{ivt_end_byte:04X} bytes)"
        ))
        .monospace()
        .size(12.5)
        .color(SEC_COL),
    );
    ui.add_space(4.0);

    let start_scan = (ivt_end_word as usize).saturating_add(1);
    let mut regions: Vec<(u32, u32, u32)> = Vec::new();
    let mut in_region = false;
    let mut reg_start = 0u32;
    let mut reg_words = 0u32;

    let mut a = start_scan;
    let flash_words = cpu.flash_words();
    while a < flash_words {
        let op = cpu.flash[a];
        if op != 0 {
            if !in_region {
                reg_start = a as u32;
                reg_words = 0;
                in_region = true;
            }
            let nw = crate::avr::cpu::Cpu::instr_words(op);
            reg_words += nw as u32;
            a += nw;
        } else {
            if in_region {
                regions.push((reg_start, (reg_start + reg_words), reg_words));
                in_region = false;
            }
            a += 1;
        }
    }
    if in_region {
        regions.push((reg_start, reg_start + reg_words, reg_words));
    }

    if regions.is_empty() {
        ui.label(
            RichText::new("  (flash is empty — assemble a program first)")
                .monospace()
                .size(11.5)
                .color(DIM),
        );
        return;
    }

    let total_words: u32 = regions.iter().map(|r| r.2).sum();
    ui.label(
        RichText::new(format!(
            "  {} region{},  {} words total  ({} bytes)",
            regions.len(),
            if regions.len() == 1 { "" } else { "s" },
            total_words,
            total_words * 2,
        ))
        .monospace()
        .size(11.5)
        .color(START_GREEN_DIM),
    );
    ui.add_space(4.0);

    ui.label(
        RichText::new("  START    END      WORDS    DISASM (first instr)")
            .monospace()
            .size(11.0)
            .color(DIM),
    );
    ui.separator();
    ui.add_space(2.0);

    Grid::new("code_regions_grid")
        .num_columns(4)
        .spacing([10.0, 2.0])
        .show(ui, |ui| {
            for (start, end, words) in &regions {
                let first_disasm = cpu.disasm_at(*start);
                ui.label(
                    RichText::new(format!("  0x{start:04X}"))
                        .monospace()
                        .size(11.5)
                        .color(FOCUS),
                );
                ui.label(
                    RichText::new(format!("0x{:04X}", end - 1))
                        .monospace()
                        .size(11.5)
                        .color(START_GREEN_DIM),
                );
                ui.label(
                    RichText::new(format!("{words:>6} words"))
                        .monospace()
                        .size(11.5)
                        .color(START_GREEN),
                );
                ui.label(
                    RichText::new(first_disasm)
                        .monospace()
                        .size(11.5)
                        .color(DIM),
                );
                ui.end_row();
            }
        });
}
