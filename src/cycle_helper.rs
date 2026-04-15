//! cycle_helper rhs_panel cycles_between_two_lines

use eframe::egui::{
    self, Button, Color32, ComboBox, Frame, Margin, RichText, Stroke, TextEdit, Ui,
};
use crate::avr::assembler::assemble_full;
use crate::avr::cpu::Cpu;
use crate::theme;
use crate::theme::{START_GREEN, START_GREEN_DIM};

const FOCUS: Color32 = theme::FOCUS;
const DIM: Color32 = theme::DIM_GRAY;
const ERR_RED: Color32 = theme::ERR_RED;

// state
pub struct CycleHelperState {
    pub file_idx:    usize,
    pub line_a_text: String,
    pub line_b_text: String,
}

impl Default for CycleHelperState {
    fn default() -> Self {
        Self {
            file_idx:    0,
            line_a_text: String::new(),
            line_b_text: String::new(),
        }
    }
}

// helpers
/// sum (min, max) cycles for all instructions in flash[addr_a..addr_b]
/// handles 2-word instructions by advancing by 2
fn cycles_in_range(flash: &[u16], addr_a: u32, addr_b: u32) -> (u64, u64) {
    let (start, end) = if addr_a <= addr_b {
        (addr_a as usize, addr_b as usize)
    } else {
        (addr_b as usize, addr_a as usize)
    };
    let mut min_total: u64 = 0;
    let mut max_total: u64 = 0;
    let mut i = start;
    while i < end && i < flash.len() {
        let op = flash[i];
        let (mn, mx) = Cpu::instr_cycles(op);
        min_total += mn as u64;
        max_total += mx as u64;
        i += Cpu::instr_words(op);
    }
    (min_total, max_total)
}

/// count total instructions in range (for display)
fn instr_count(flash: &[u16], addr_a: u32, addr_b: u32) -> usize {
    let (start, end) = if addr_a <= addr_b {
        (addr_a as usize, addr_b as usize)
    } else {
        (addr_b as usize, addr_a as usize)
    };
    let mut count = 0usize;
    let mut i = start;
    while i < end && i < flash.len() {
        count += 1;
        i += Cpu::instr_words(flash[i]);
    }
    count
}

// rhs panel
/// `files`: list of (display_name, source_content) pairs from the workspace.
pub fn show_cycle_helper(
    ui:    &mut Ui,
    state: &mut CycleHelperState,
    files: &[(String, String)],
) {
    Frame::NONE
        .fill(theme::PANEL_DEEP)
        .stroke(Stroke::new(1.0, START_GREEN_DIM))
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());

            ui.label(
                RichText::new("[ CYCLE HELPER ]")
                    .monospace().size(13.0).color(START_GREEN),
            );
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(6.0);

            if files.is_empty() {
                ui.label(
                    RichText::new("No valid files found in workspace.")
                        .monospace().size(11.5).color(DIM),
                );
                return;
            }

            // file selector
            ui.label(RichText::new("FILE").monospace().size(11.5).color(START_GREEN_DIM));
            ui.add_space(2.0);
            let sel_name = files.get(state.file_idx)
                .map(|(n, _)| n.as_str())
                .unwrap_or("—");
            ComboBox::from_id_salt("ch_file")
                .width(180.0)
                .selected_text(RichText::new(sel_name).monospace().size(11.0).color(START_GREEN))
                .show_ui(ui, |ui| {
                    ui.style_mut().visuals.override_text_color = Some(START_GREEN);
                    for (i, (name, _)) in files.iter().enumerate() {
                        ui.selectable_value(
                            &mut state.file_idx,
                            i,
                            RichText::new(name).monospace().size(11.0),
                        );
                    }
                });

            ui.add_space(8.0);

            // assemble & resolve addresses
            let source = files.get(state.file_idx).map(|(_, c)| c.as_str()).unwrap_or("");
            let assembled: Option<(Vec<u16>, Vec<(usize, u32)>)> =
                assemble_full(source).ok();

            let resolve = |line_text: &str| -> Result<u32, String> {
                let ln = line_text.trim().parse::<usize>()
                    .map_err(|_| "invalid line number".to_string())?;
                match &assembled {
                    None    => Err("assembly failed".to_string()),
                    Some((_, map)) => {
                        map.iter()
                            .find(|&&(l, _)| l == ln)
                            .or_else(|| map.iter().find(|&&(l, _)| l >= ln))
                            .map(|&(_, a)| a)
                            .ok_or_else(|| format!("line {ln} is past last instruction"))
                    }
                }
            };

            // line A
            ui.label(RichText::new("LINE A").monospace().size(11.5).color(START_GREEN_DIM));
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("line:").monospace().size(11.0).color(DIM));
                ui.add(
                    TextEdit::singleline(&mut state.line_a_text)
                        .desired_width(60.0)
                        .font(egui::TextStyle::Monospace),
                );
            });
            let addr_a_res = if state.line_a_text.trim().is_empty() {
                Err("".to_string())
            } else {
                resolve(&state.line_a_text)
            };
            match &addr_a_res {
                Ok(a)  => { ui.label(RichText::new(format!("  → word 0x{a:04X}")).monospace().size(11.0).color(FOCUS)); }
                Err(e) if !e.is_empty() => { ui.label(RichText::new(format!("  ✗ {e}")).monospace().size(10.5).color(ERR_RED)); }
                _ => { ui.label(RichText::new("").monospace().size(10.5)); }
            }

            ui.add_space(4.0);

            // line B
            ui.label(RichText::new("LINE B").monospace().size(11.5).color(START_GREEN_DIM));
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("line:").monospace().size(11.0).color(DIM));
                ui.add(
                    TextEdit::singleline(&mut state.line_b_text)
                        .desired_width(60.0)
                        .font(egui::TextStyle::Monospace),
                );
            });
            let addr_b_res = if state.line_b_text.trim().is_empty() {
                Err("".to_string())
            } else {
                resolve(&state.line_b_text)
            };
            match &addr_b_res {
                Ok(b)  => { ui.label(RichText::new(format!("  → word 0x{b:04X}")).monospace().size(11.0).color(FOCUS)); }
                Err(e) if !e.is_empty() => { ui.label(RichText::new(format!("  ✗ {e}")).monospace().size(10.5).color(ERR_RED)); }
                _ => { ui.label(RichText::new("").monospace().size(10.5)); }
            }

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(6.0);

            // results
            match (addr_a_res, addr_b_res) {
                (Ok(a), Ok(b)) => {
                    if let Some((flash, _)) = &assembled {
                        show_cycle_results(ui, flash, a, b);
                    }
                }
                _ => {
                    ui.label(
                        RichText::new("Enter valid line numbers for both slots.")
                            .monospace().size(11.0).color(DIM),
                    );
                }
            }
        });
}

fn show_cycle_results(ui: &mut Ui, flash: &[u16], a: u32, b: u32) {
    let (start, end) = if a <= b { (a, b) } else { (b, a) };
    let word_span  = end - start;
    let (mn, mx)   = cycles_in_range(flash, a, b);
    let n_instrs   = instr_count(flash, a, b);

    // direction indicator
    let dir_label = if a <= b { "A → B" } else { "B → A" };
    ui.label(
        RichText::new(format!("{dir_label}  ({n_instrs} instructions, {word_span} words)"))
            .monospace().size(11.0).color(START_GREEN_DIM),
    );
    ui.add_space(6.0);

    if mn == mx {
        // deterministic (no branches)
        ui.add(
            Button::new(
                RichText::new(format!("{mn} cycles"))
                    .monospace().size(14.0).color(Color32::BLACK),
            )
            .fill(START_GREEN)
            .stroke(Stroke::new(1.0, START_GREEN)),
        );
    } else {
        // variable (branches / skips)
        ui.horizontal(|ui| {
            ui.add(
                Button::new(
                    RichText::new(format!("{mn} min"))
                        .monospace().size(13.0).color(Color32::BLACK),
                )
                .fill(START_GREEN)
                .stroke(Stroke::new(1.0, START_GREEN)),
            );
            ui.add_space(6.0);
            ui.add(
                Button::new(
                    RichText::new(format!("{mx} max"))
                        .monospace().size(13.0).color(Color32::BLACK),
                )
                .fill(FOCUS)
                .stroke(Stroke::new(1.0, FOCUS)),
            );
        });
        ui.add_space(2.0);
        ui.label(
            RichText::new("(range due to branches/skips in the region)")
                .monospace().size(10.0).color(DIM),
        );
    }

    ui.add_space(6.0);
    ui.separator();
    ui.add_space(4.0);
    ui.label(
        RichText::new("Note: counts instructions from A's word addr up to (not including) B's word addr.")
            .monospace().size(10.0).color(DIM),
    );
}
