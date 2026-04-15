//! word_helper rhs_panel distance_rjmp_vs_jmp

use eframe::egui::{
    self, Button, Color32, ComboBox, Frame, Margin, RichText, Stroke, TextEdit, Ui,
};
use crate::avr::assembler::assemble_full;
use crate::theme;
use crate::theme::{START_GREEN, START_GREEN_DIM};

const FOCUS: Color32 = theme::FOCUS;
const DIM: Color32 = theme::DIM_GRAY;
const ERR_RED: Color32 = theme::ERR_RED;

// RJMP is ±2047 from (PC+1), 1 word.  JMP is 2 words but unlimited range.
const RJMP_MAX: i64 = 2047;
const RJMP_MIN: i64 = -2048;

// slot_state

pub struct WordHelperSlot {
    pub file_idx:  usize,    // index into the `files` slice passed to show_word_helper
    pub line_text: String,   // raw text the user typed
}

impl Default for WordHelperSlot {
    fn default() -> Self {
        Self { file_idx: 0, line_text: String::new() }
    }
}

pub struct WordHelperState {
    pub slot_a: WordHelperSlot,
    pub slot_b: WordHelperSlot,
}

impl Default for WordHelperState {
    fn default() -> Self {
        Self { slot_a: WordHelperSlot::default(), slot_b: WordHelperSlot::default() }
    }
}

// lookup_word_addr: returns word address for 1-indexed line in source, or Err
fn addr_for_line(source: &str, line: usize) -> Result<u32, String> {
    let (_, map) = assemble_full(source)
        .map_err(|errs| errs.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; "))?;
    if map.is_empty() {
        return Err("no instructions assembled".to_string());
    }
    // Exact match first, then first instruction at or after the line
    map.iter()
        .find(|&&(ln, _)| ln == line)
        .or_else(|| map.iter().find(|&&(ln, _)| ln >= line))
        .map(|&(_, addr)| addr)
        .ok_or_else(|| format!("line {line} is after the last instruction"))
}

// rhs_panel

/// `files`: list of (display_name, source_content) pairs from the workspace.
pub fn show_word_helper(
    ui:    &mut Ui,
    state: &mut WordHelperState,
    files: &[(String, String)],
) {
    Frame::NONE
        .fill(theme::PANEL_DEEP)
        .stroke(Stroke::new(1.0, START_GREEN_DIM))
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());

            ui.label(
                RichText::new("[ WORD HELPER ]")
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

            // slot_a
            let addr_a = render_slot(ui, "SLOT A", &mut state.slot_a, files, "wh_a");
            ui.add_space(6.0);
            // slot_b
            let addr_b = render_slot(ui, "SLOT B", &mut state.slot_b, files, "wh_b");

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(6.0);

            // results
            match (addr_a, addr_b) {
                (Ok(a), Ok(b)) => show_results(ui, a, b),
                _ => {
                    ui.label(
                        RichText::new("Enter valid file + line for both slots.")
                            .monospace().size(11.0).color(DIM),
                    );
                }
            }
        });
}

fn render_slot(
    ui:    &mut Ui,
    label: &str,
    slot:  &mut WordHelperSlot,
    files: &[(String, String)],
    id:    &str,
) -> Result<u32, String> {
    ui.label(RichText::new(label).monospace().size(11.5).color(START_GREEN_DIM));
    ui.add_space(2.0);

    ui.horizontal(|ui| {
        // file dropdown
        let sel_name = files.get(slot.file_idx)
            .map(|(n, _)| n.as_str())
            .unwrap_or("—");

        ComboBox::from_id_salt(format!("{id}_file"))
            .width(140.0)
            .selected_text(RichText::new(sel_name).monospace().size(11.0).color(START_GREEN))
            .show_ui(ui, |ui| {
                ui.style_mut().visuals.override_text_color = Some(START_GREEN);
                for (i, (name, _)) in files.iter().enumerate() {
                    ui.selectable_value(
                        &mut slot.file_idx,
                        i,
                        RichText::new(name).monospace().size(11.0),
                    );
                }
            });

        // line number input
        ui.label(RichText::new("line:").monospace().size(11.0).color(DIM));
        ui.add(
            TextEdit::singleline(&mut slot.line_text)
                .desired_width(48.0)
                .font(egui::TextStyle::Monospace),
        );
    }).inner;

    // compute address
    let line_nr = slot.line_text.trim().parse::<usize>().ok();
    match line_nr {
        None => {
            ui.label(RichText::new("").monospace().size(10.5).color(DIM));
            Err("no line".to_string())
        }
        Some(ln) => {
            let content = files.get(slot.file_idx).map(|(_, c)| c.as_str()).unwrap_or("");
            match addr_for_line(content, ln) {
                Ok(addr) => {
                    ui.label(
                        RichText::new(format!("  → word address 0x{addr:04X}"))
                            .monospace().size(11.0).color(FOCUS),
                    );
                    Ok(addr)
                }
                Err(e) => {
                    ui.label(
                        RichText::new(format!("  ✗ {e}"))
                            .monospace().size(10.5).color(ERR_RED),
                    );
                    Err(e)
                }
            }
        }
    }
}

fn show_results(ui: &mut Ui, a: u32, b: u32) {
    let diff = (b as i64) - (a as i64);
    let abs_diff = diff.unsigned_abs();

    // distance
    let sign = if diff >= 0 { "+" } else { "" };
    ui.label(
        RichText::new(format!("DISTANCE   {sign}{diff}  words  (0x{abs_diff:04X})"))
            .monospace().size(12.5).color(START_GREEN),
    );
    ui.add_space(6.0);
    ui.separator();
    ui.add_space(4.0);

    // A → B: rjmp_offset = B - (A+1)
    let off_ab = (b as i64) - (a as i64 + 1);
    let rjmp_ab = off_ab >= RJMP_MIN && off_ab <= RJMP_MAX;

    ui.horizontal(|ui| {
        ui.label(RichText::new("A → B:").monospace().size(11.5).color(DIM));
        if rjmp_ab {
            ui.add(
                Button::new(RichText::new("RJMP ✓").monospace().size(12.0).color(Color32::BLACK))
                    .fill(START_GREEN)
                    .stroke(Stroke::new(1.0, START_GREEN)),
            );
            ui.label(
                RichText::new(format!("  1 word  (offset {off_ab:+})"))
                    .monospace().size(11.0).color(START_GREEN_DIM),
            );
        } else {
            ui.add(
                Button::new(RichText::new("JMP").monospace().size(12.0).color(Color32::BLACK))
                    .fill(FOCUS)
                    .stroke(Stroke::new(1.0, FOCUS)),
            );
            ui.label(
                RichText::new(format!("  2 words  (offset {off_ab:+}, exceeds ±2047)"))
                    .monospace().size(11.0).color(FOCUS),
            );
        }
    });
    ui.add_space(4.0);

    // B → A: rjmp_offset = A - (B+1)
    let off_ba = (a as i64) - (b as i64 + 1);
    let rjmp_ba = off_ba >= RJMP_MIN && off_ba <= RJMP_MAX;

    ui.horizontal(|ui| {
        ui.label(RichText::new("B → A:").monospace().size(11.5).color(DIM));
        if rjmp_ba {
            ui.add(
                Button::new(RichText::new("RJMP ✓").monospace().size(12.0).color(Color32::BLACK))
                    .fill(START_GREEN)
                    .stroke(Stroke::new(1.0, START_GREEN)),
            );
            ui.label(
                RichText::new(format!("  1 word  (offset {off_ba:+})"))
                    .monospace().size(11.0).color(START_GREEN_DIM),
            );
        } else {
            ui.add(
                Button::new(RichText::new("JMP").monospace().size(12.0).color(Color32::BLACK))
                    .fill(FOCUS)
                    .stroke(Stroke::new(1.0, FOCUS)),
            );
            ui.label(
                RichText::new(format!("  2 words  (offset {off_ba:+}, exceeds ±2047)"))
                    .monospace().size(11.0).color(FOCUS),
            );
        }
    });
    ui.add_space(6.0);

    // visual_scale_bar: RJMP range indicator
    ui.add_space(4.0);
    ui.separator();
    ui.add_space(4.0);
    ui.label(
        RichText::new("RJMP range: ±2047 words (1 word instr)   |   JMP: any (2 word instr)")
            .monospace().size(10.5).color(DIM),
    );
}
