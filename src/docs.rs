//! isa_ref_overlay show_isa_window_each_frame ~88pct_viewport

use eframe::egui::{
    self, Align, Button, Color32, Frame, Grid, Layout, Margin, RichText, ScrollArea, Stroke, Ui,
};
use crate::avr::cpu::Cpu;
use crate::welcome::{START_GREEN, START_GREEN_DIM};

const AMBER:   Color32 = Color32::from_rgb(255, 185, 55);
const DIM:     Color32 = Color32::from_rgb(65,  65,  65);
const SEC_COL: Color32 = Color32::from_rgb(100, 220, 100);

// flash_locations_window

/// Overlay showing the IVT layout and code regions currently loaded in flash.
pub fn show_flash_locations_window(ctx: &egui::Context, open: &mut bool, cpu: &Cpu) {
    if !*open { return; }
    let screen = ctx.screen_rect();
    let w = screen.width()  * 0.88;
    let h = screen.height() * 0.88;

    egui::Window::new("flash_locations_win")
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .fixed_size([w, h])
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .frame(
            Frame::NONE
                .fill(Color32::from_rgb(3, 7, 3))
                .stroke(Stroke::new(1.5, START_GREEN))
                .inner_margin(Margin::same(16)),
        )
        .show(ctx, |ui| {
            // title_bar
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("[ FLASH LOCATIONS — {} ]", cpu.model.label()))
                        .monospace().size(16.0).color(START_GREEN),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(Button::new(
                        RichText::new("  X  ").monospace().size(13.0).color(START_GREEN),
                    ).stroke(Stroke::new(1.0, START_GREEN))).clicked() {
                        *open = false;
                    }
                });
            });
            ui.separator();
            ui.add_space(4.0);

            ScrollArea::vertical()
                .id_salt("flash_loc_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    show_ivt_section(ui, cpu);
                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(6.0);
                    show_code_regions_section(ui, cpu);
                });
        });
}

fn vector_content(cpu: &Cpu, addr: u32) -> (String, bool) {
    let flash_words = cpu.flash_words();
    let op = if (addr as usize) < flash_words { cpu.flash[addr as usize] } else { 0 };
    if op == 0x0000 {
        return ("EMPTY".to_string(), false);
    }
    // JMP: 1001 010k kkkk 110k  (any k)
    if (op & 0xFE0E) == 0x940C {
        let next = if (addr as usize + 1) < flash_words { cpu.flash[addr as usize + 1] } else { 0 };
        let target = (((op & 0x01F0) as u32) << 17)
                   | (((op & 0x0001) as u32) << 16)
                   | (next as u32);
        return (format!("JMP  → 0x{target:04X}"), true);
    }
    // RJMP: 1100 kkkk kkkk kkkk
    if (op & 0xF000) == 0xC000 {
        let raw = op & 0x0FFF;
        let k   = (((raw << 4) as i16) >> 4) as i32; // sign-extend 12-bit
        let target = (addr as i32 + 1 + k) as u32;
        let sign = if k >= 0 { "+" } else { "" };
        return (format!("RJMP → 0x{target:04X}  ({sign}{k})"), true);
    }
    // Other instruction present — show disasm
    (format!("{}  [non-jump]", cpu.disasm_at(addr)), true)
}

fn show_ivt_section(ui: &mut Ui, cpu: &Cpu) {
    let ivt_end_word = cpu.ivt_end_word();
    let ivt_end_byte = ivt_end_word * 2;
    ui.label(
        RichText::new(format!("INTERRUPT VECTOR TABLE  (0x0000 – 0x{ivt_end_byte:04X}, byte addresses)"))
            .monospace().size(12.5).color(SEC_COL),
    );
    ui.add_space(4.0);

    // column_header
    ui.label(
        RichText::new("  ADDR   VECTOR           CONTENT")
            .monospace().size(11.0).color(DIM),
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
                let name_col = if occupied { AMBER } else { DIM };
                let cont_col = if occupied { START_GREEN } else { DIM };
                let addr_byte = addr * 2;

                ui.label(
                    RichText::new(format!("  0x{addr_byte:04X}"))
                        .monospace().size(11.5).color(START_GREEN_DIM),
                );
                ui.label(
                    RichText::new(format!("{name:<16}"))
                        .monospace().size(11.5).color(name_col),
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
            .monospace().size(11.0).color(DIM),
    );
}

fn show_code_regions_section(ui: &mut Ui, cpu: &Cpu) {
    let ivt_end_word = cpu.ivt_end_word();
    let ivt_end_byte = ivt_end_word * 2;
    ui.label(
        RichText::new(format!(
            "CODE REGIONS  (non-empty flash words beyond IVT end 0x{ivt_end_byte:04X} bytes)"
        ))
            .monospace().size(12.5).color(SEC_COL),
    );
    ui.add_space(4.0);

    // Walk flash starting at first word past the model-specific IVT.
    let start_scan = (ivt_end_word as usize).saturating_add(1);
    let mut regions: Vec<(u32, u32, u32)> = Vec::new(); // (start, end_excl, words)
    let mut in_region = false;
    let mut reg_start = 0u32;
    let mut reg_words = 0u32;

    let mut a = start_scan;
    let flash_words = cpu.flash_words();
    while a < flash_words {
        let op = cpu.flash[a];
        if op != 0 {
            if !in_region { reg_start = a as u32; reg_words = 0; in_region = true; }
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
                .monospace().size(11.5).color(DIM),
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
        )).monospace().size(11.5).color(START_GREEN_DIM),
    );
    ui.add_space(4.0);

    ui.label(
        RichText::new("  START    END      WORDS    DISASM (first instr)")
            .monospace().size(11.0).color(DIM),
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
                        .monospace().size(11.5).color(AMBER),
                );
                ui.label(
                    RichText::new(format!("0x{:04X}", end - 1))
                        .monospace().size(11.5).color(START_GREEN_DIM),
                );
                ui.label(
                    RichText::new(format!("{words:>6} words"))
                        .monospace().size(11.5).color(START_GREEN),
                );
                ui.label(
                    RichText::new(first_disasm)
                        .monospace().size(11.5).color(DIM),
                );
                ui.end_row();
            }
        });
}

// data_types

/// isa_row tuple mnem ops desc flags clk
type Row = (&'static str, &'static str, &'static str, &'static str, &'static str);

struct Section {
    title: &'static str,
    rows:  &'static [Row],
}

// instruction_data
// src DS40002198B tbl5 avre_clocks mega128a

const ARITHMETIC: &[Row] = &[
    ("ADD",    "Rd, Rr",    "Add without Carry",                        "Z,C,N,V,S,H", "1"),
    ("ADC",    "Rd, Rr",    "Add with Carry",                           "Z,C,N,V,S,H", "1"),
    ("ADIW",   "Rd, K",     "Add Immediate to Word",                    "Z,C,N,V,S",   "2"),
    ("SUB",    "Rd, Rr",    "Subtract without Carry",                   "Z,C,N,V,S,H", "1"),
    ("SUBI",   "Rd, K",     "Subtract Immediate",                       "Z,C,N,V,S,H", "1"),
    ("SBC",    "Rd, Rr",    "Subtract with Carry",                      "Z,C,N,V,S,H", "1"),
    ("SBCI",   "Rd, K",     "Subtract Immediate with Carry",            "Z,C,N,V,S,H", "1"),
    ("SBIW",   "Rd, K",     "Subtract Immediate from Word",             "Z,C,N,V,S",   "2"),
    ("AND",    "Rd, Rr",    "Logical AND",                              "Z,N,V,S",     "1"),
    ("ANDI",   "Rd, K",     "Logical AND with Immediate",               "Z,N,V,S",     "1"),
    ("OR",     "Rd, Rr",    "Logical OR",                               "Z,N,V,S",     "1"),
    ("ORI",    "Rd, K",     "Logical OR with Immediate",                "Z,N,V,S",     "1"),
    ("EOR",    "Rd, Rr",    "Exclusive OR",                             "Z,N,V,S",     "1"),
    ("COM",    "Rd",        "One's Complement  (Rd ← 0xFF − Rd)",       "Z,C,N,V,S",   "1"),
    ("NEG",    "Rd",        "Two's Complement  (Rd ← 0x00 − Rd)",       "Z,C,N,V,S,H", "1"),
    ("SBR",    "Rd, K",     "Set Bit(s) in Register  (Rd ← Rd | K)",   "Z,N,V,S",     "1"),
    ("CBR",    "Rd, K",     "Clear Bit(s) in Register",                 "Z,N,V,S",     "1"),
    ("INC",    "Rd",        "Increment",                                "Z,N,V,S",     "1"),
    ("DEC",    "Rd",        "Decrement",                                "Z,N,V,S",     "1"),
    ("TST",    "Rd",        "Test for Zero or Minus  (Rd & Rd)",        "Z,N,V,S",     "1"),
    ("CLR",    "Rd",        "Clear Register  (Rd ← Rd ⊕ Rd)",           "Z,N,V,S",     "1"),
    ("SER",    "Rd",        "Set Register  (Rd ← 0xFF)",                "—",           "1"),
    ("MUL",    "Rd, Rr",    "Multiply Unsigned  (R1:R0 ← Rd × Rr)",    "Z,C",         "2"),
    ("MULS",   "Rd, Rr",    "Multiply Signed  (R1:R0 ← Rd × Rr)",      "Z,C",         "2"),
    ("MULSU",  "Rd, Rr",    "Multiply Signed with Unsigned",            "Z,C",         "2"),
    ("FMUL",   "Rd, Rr",    "Fractional Multiply Unsigned",             "Z,C",         "2"),
    ("FMULS",  "Rd, Rr",    "Fractional Multiply Signed",               "Z,C",         "2"),
    ("FMULSU", "Rd, Rr",    "Fractional Multiply Signed with Unsigned", "Z,C",         "2"),
    ("DES",    "K",         "Data Encryption Standard round",           "—",           "1/2"),
];

const CONTROL: &[Row] = &[
    ("RJMP",   "k",         "Relative Jump  (PC ← PC + k + 1)",        "—",           "2"),
    ("IJMP",   "",          "Indirect Jump to Z  (PC ← Z)",             "—",           "2"),
    ("EIJMP",  "",          "Extended Indirect Jump  (PC ← EIND:Z)",    "—",           "2"),
    ("JMP",    "k",         "Absolute Jump  (PC ← k)",                  "—",           "3"),
    ("RCALL",  "k",         "Relative Call Subroutine",                 "—",           "3/4"),
    ("ICALL",  "",          "Indirect Call to Z",                       "—",           "3/4"),
    ("EICALL", "",          "Extended Indirect Call to Z",              "—",           "4"),
    ("CALL",   "k",         "Call Subroutine  (PC ← k)",                "—",           "4/5"),
    ("RET",    "",          "Subroutine Return  (PC ← STACK)",          "—",           "4/5"),
    ("RETI",   "",          "Interrupt Return  (PC ← STACK)",           "I",           "4/5"),
    ("CPSE",   "Rd, Rr",    "Compare, Skip if Equal",                   "—",           "1/2/3"),
    ("CP",     "Rd, Rr",    "Compare  (Rd − Rr, discard result)",       "Z,C,N,V,S,H", "1"),
    ("CPC",    "Rd, Rr",    "Compare with Carry",                       "Z,C,N,V,S,H", "1"),
    ("CPI",    "Rd, K",     "Compare with Immediate",                   "Z,C,N,V,S,H", "1"),
    ("SBRC",   "Rr, b",     "Skip if Bit in Register Cleared",          "—",           "1/2/3"),
    ("SBRS",   "Rr, b",     "Skip if Bit in Register Set",              "—",           "1/2/3"),
    ("SBIC",   "A, b",      "Skip if Bit in I/O Register Cleared",      "—",           "1/2/3"),
    ("SBIS",   "A, b",      "Skip if Bit in I/O Register Set",          "—",           "1/2/3"),
    ("BRBS",   "s, k",      "Branch if Status Flag Set",                "—",           "1/2"),
    ("BRBC",   "s, k",      "Branch if Status Flag Cleared",            "—",           "1/2"),
    ("BREQ",   "k",         "Branch if Equal  (Z == 1)",                "—",           "1/2"),
    ("BRNE",   "k",         "Branch if Not Equal  (Z == 0)",            "—",           "1/2"),
    ("BRCS",   "k",         "Branch if Carry Set  (C == 1)",            "—",           "1/2"),
    ("BRCC",   "k",         "Branch if Carry Cleared  (C == 0)",        "—",           "1/2"),
    ("BRSH",   "k",         "Branch if Same or Higher  (C == 0)",       "—",           "1/2"),
    ("BRLO",   "k",         "Branch if Lower  (C == 1)",                "—",           "1/2"),
    ("BRMI",   "k",         "Branch if Minus  (N == 1)",                "—",           "1/2"),
    ("BRPL",   "k",         "Branch if Plus  (N == 0)",                 "—",           "1/2"),
    ("BRGE",   "k",         "Branch if Greater or Equal, Signed (S==0)","—",           "1/2"),
    ("BRLT",   "k",         "Branch if Less Than, Signed  (S == 1)",    "—",           "1/2"),
    ("BRHS",   "k",         "Branch if Half Carry Flag Set  (H == 1)",  "—",           "1/2"),
    ("BRHC",   "k",         "Branch if Half Carry Flag Cleared (H==0)", "—",           "1/2"),
    ("BRTS",   "k",         "Branch if T Bit Set  (T == 1)",            "—",           "1/2"),
    ("BRTC",   "k",         "Branch if T Bit Cleared  (T == 0)",        "—",           "1/2"),
    ("BRVS",   "k",         "Branch if Overflow Flag Set  (V == 1)",    "—",           "1/2"),
    ("BRVC",   "k",         "Branch if Overflow Flag Cleared  (V == 0)","—",           "1/2"),
    ("BRIE",   "k",         "Branch if Interrupt Enabled  (I == 1)",    "—",           "1/2"),
    ("BRID",   "k",         "Branch if Interrupt Disabled  (I == 0)",   "—",           "1/2"),
];

const DATA: &[Row] = &[
    ("MOV",    "Rd, Rr",    "Copy Register",                            "—",           "1"),
    ("MOVW",   "Rd, Rr",    "Copy Register Pair  (R[d+1]:Rd ← R[r+1]:Rr)", "—",      "1"),
    ("LDI",    "Rd, K",     "Load Immediate  (Rd ← K, R16–R31 only)",  "—",           "1"),
    ("LDS",    "Rd, k",     "Load Direct from Data Space",              "—",           "2"),
    ("LD",     "Rd, X",     "Load Indirect via X",                      "—",           "2"),
    ("LD",     "Rd, X+",    "Load Indirect via X, Post-Increment X",    "—",           "2"),
    ("LD",     "Rd, -X",    "Load Indirect via X, Pre-Decrement X",     "—",           "2"),
    ("LD",     "Rd, Y",     "Load Indirect via Y",                      "—",           "2"),
    ("LD",     "Rd, Y+",    "Load Indirect via Y, Post-Increment Y",    "—",           "2"),
    ("LD",     "Rd, -Y",    "Load Indirect via Y, Pre-Decrement Y",     "—",           "2"),
    ("LDD",    "Rd, Y+q",   "Load Indirect via Y with Displacement",    "—",           "2"),
    ("LD",     "Rd, Z",     "Load Indirect via Z",                      "—",           "2"),
    ("LD",     "Rd, Z+",    "Load Indirect via Z, Post-Increment Z",    "—",           "2"),
    ("LD",     "Rd, -Z",    "Load Indirect via Z, Pre-Decrement Z",     "—",           "2"),
    ("LDD",    "Rd, Z+q",   "Load Indirect via Z with Displacement",    "—",           "2"),
    ("STS",    "k, Rr",     "Store Direct to Data Space",               "—",           "2"),
    ("ST",     "X, Rr",     "Store Indirect via X",                     "—",           "2"),
    ("ST",     "X+, Rr",    "Store Indirect via X, Post-Increment X",   "—",           "2"),
    ("ST",     "-X, Rr",    "Store Indirect via X, Pre-Decrement X",    "—",           "2"),
    ("ST",     "Y, Rr",     "Store Indirect via Y",                     "—",           "2"),
    ("ST",     "Y+, Rr",    "Store Indirect via Y, Post-Increment Y",   "—",           "2"),
    ("ST",     "-Y, Rr",    "Store Indirect via Y, Pre-Decrement Y",    "—",           "2"),
    ("STD",    "Y+q, Rr",   "Store Indirect via Y with Displacement",   "—",           "2"),
    ("ST",     "Z, Rr",     "Store Indirect via Z",                     "—",           "2"),
    ("ST",     "Z+, Rr",    "Store Indirect via Z, Post-Increment Z",   "—",           "2"),
    ("ST",     "-Z, Rr",    "Store Indirect via Z, Pre-Decrement Z",    "—",           "2"),
    ("STD",    "Z+q, Rr",   "Store Indirect via Z with Displacement",   "—",           "2"),
    ("LPM",    "",          "Load Program Memory → R0  (R0 ← PS(Z))",  "—",           "3"),
    ("LPM",    "Rd, Z",     "Load Program Memory  (Rd ← PS(Z))",        "—",           "3"),
    ("LPM",    "Rd, Z+",    "Load Program Memory, Post-Increment Z",    "—",           "3"),
    ("ELPM",   "",          "Ext. Load Program Memory → R0",            "—",           "3"),
    ("ELPM",   "Rd, Z",     "Ext. Load Program Memory  (Rd ← PS(RAMPZ:Z))", "—",     "3"),
    ("ELPM",   "Rd, Z+",    "Ext. Load Program Memory, Post-Increment", "—",           "3"),
    ("SPM",    "",          "Store Program Memory  (PS(RAMPZ:Z) ← R1:R0)", "—",      "—"),
    ("SPM",    "Z+",        "Store Program Memory, Post-Increment by 2","—",           "—"),
    ("IN",     "Rd, A",     "Read from I/O Register  (Rd ← I/O(A))",   "—",           "1"),
    ("OUT",    "A, Rr",     "Write to I/O Register  (I/O(A) ← Rr)",    "—",           "1"),
    ("PUSH",   "Rr",        "Push Register onto Stack",                 "—",           "2"),
    ("POP",    "Rd",        "Pop Register from Stack",                  "—",           "2"),
    ("XCH",    "Z, Rd",     "Exchange  (DS(Z) ↔ Rd)",                   "—",           "2"),
    ("LAS",    "Z, Rd",     "Load and Set  (DS(Z) ← Rd | DS(Z))",      "—",           "2"),
    ("LAC",    "Z, Rd",     "Load and Clear",                           "—",           "2"),
    ("LAT",    "Z, Rd",     "Load and Toggle  (DS(Z) ← Rd ⊕ DS(Z))",   "—",           "2"),
];

const BITS: &[Row] = &[
    ("LSL",    "Rd",        "Logical Shift Left  (C ← Rd(7), Rd ← Rd<<1)",  "Z,C,N,V,H", "1"),
    ("LSR",    "Rd",        "Logical Shift Right  (C ← Rd(0), Rd ← Rd>>1)", "Z,C,N,V",   "1"),
    ("ROL",    "Rd",        "Rotate Left Through Carry",                     "Z,C,N,V,H", "1"),
    ("ROR",    "Rd",        "Rotate Right Through Carry",                    "Z,C,N,V",   "1"),
    ("ASR",    "Rd",        "Arithmetic Shift Right  (sign bit preserved)",  "Z,C,N,V",   "1"),
    ("SWAP",   "Rd",        "Swap Nibbles  (Rd(3:0) ↔ Rd(7:4))",            "—",          "1"),
    ("SBI",    "A, b",      "Set Bit in I/O Register  (I/O(A,b) ← 1)",      "—",          "2"),
    ("CBI",    "A, b",      "Clear Bit in I/O Register  (I/O(A,b) ← 0)",    "—",          "2"),
    ("BST",    "Rr, b",     "Bit Store from Register to T  (T ← Rr(b))",    "T",          "1"),
    ("BLD",    "Rd, b",     "Bit Load from T to Register  (Rd(b) ← T)",     "—",          "1"),
    ("BSET",   "s",         "Flag Set  (SREG(s) ← 1)",                      "SREG(s)",    "1"),
    ("BCLR",   "s",         "Flag Clear  (SREG(s) ← 0)",                    "SREG(s)",    "1"),
    ("SEC",    "",          "Set Carry Flag  (C ← 1)",                       "C",          "1"),
    ("CLC",    "",          "Clear Carry Flag  (C ← 0)",                     "C",          "1"),
    ("SEN",    "",          "Set Negative Flag  (N ← 1)",                    "N",          "1"),
    ("CLN",    "",          "Clear Negative Flag  (N ← 0)",                  "N",          "1"),
    ("SEZ",    "",          "Set Zero Flag  (Z ← 1)",                        "Z",          "1"),
    ("CLZ",    "",          "Clear Zero Flag  (Z ← 0)",                      "Z",          "1"),
    ("SEI",    "",          "Global Interrupt Enable  (I ← 1)",              "I",          "1"),
    ("CLI",    "",          "Global Interrupt Disable  (I ← 0)",             "I",          "1"),
    ("SES",    "",          "Set Sign Bit  (S ← 1)",                         "S",          "1"),
    ("CLS",    "",          "Clear Sign Bit  (S ← 0)",                       "S",          "1"),
    ("SEV",    "",          "Set Overflow Flag  (V ← 1)",                    "V",          "1"),
    ("CLV",    "",          "Clear Overflow Flag  (V ← 0)",                  "V",          "1"),
    ("SET",    "",          "Set T Flag in SREG  (T ← 1)",                   "T",          "1"),
    ("CLT",    "",          "Clear T Flag in SREG  (T ← 0)",                 "T",          "1"),
    ("SEH",    "",          "Set Half Carry Flag  (H ← 1)",                  "H",          "1"),
    ("CLH",    "",          "Clear Half Carry Flag  (H ← 0)",                "H",          "1"),
];

const MCU: &[Row] = &[
    ("BREAK",  "",          "Break — see debug interface description",   "—",           "1"),
    ("NOP",    "",          "No Operation",                              "—",           "1"),
    ("SLEEP",  "",          "Sleep — see power management description",  "—",           "1"),
    ("WDR",    "",          "Watchdog Reset — see WDT description",      "—",           "1"),
];

const SECTIONS: &[Section] = &[
    Section { title: "ARITHMETIC & LOGIC",     rows: ARITHMETIC },
    Section { title: "CHANGE OF FLOW",         rows: CONTROL    },
    Section { title: "DATA TRANSFER",          rows: DATA       },
    Section { title: "BIT & BIT-TEST",         rows: BITS       },
    Section { title: "MCU CONTROL",            rows: MCU        },
];

// public_entry_point

/// show_isa_window each_frame noop_if_closed
pub fn show_isa_window(ctx: &egui::Context, open: &mut bool) {
    if !*open { return; }

    let screen = ctx.screen_rect();
    let mx  = screen.width()  * 0.06;
    let my  = screen.height() * 0.05;
    let pos = screen.min + egui::vec2(mx, my);
    let sz  = egui::vec2(screen.width() - mx * 2.0, screen.height() - my * 2.0);

    egui::Window::new("__isa_ref__")
        .fixed_pos(pos)
        .fixed_size(sz)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .order(egui::Order::Foreground)
        .frame(
            Frame::NONE
                .fill(Color32::from_rgb(2, 6, 2))
                .stroke(Stroke::new(2.0, START_GREEN))
                .inner_margin(Margin::same(0)),
        )
        .show(ctx, |ui| {
            ui.set_min_size(sz);

            // custom_title_bar
            Frame::NONE
                .fill(Color32::from_rgb(6, 18, 6))
                .stroke(Stroke::new(0.0, Color32::TRANSPARENT))
                .inner_margin(Margin::symmetric(14, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("[ AVR® 8-BIT INSTRUCTION SET REFERENCE  DS40002198B ]")
                                .monospace()
                                .size(14.0)
                                .color(START_GREEN),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.add(
                                Button::new(
                                    RichText::new("  ✕  ")
                                        .monospace()
                                        .size(14.0)
                                        .color(AMBER),
                                )
                                .fill(Color32::from_rgb(22, 5, 5))
                                .stroke(Stroke::new(1.0, AMBER)),
                            ).clicked() {
                                *open = false;
                            }
                        });
                    });
                });

            // title_sep
            ui.add(egui::Separator::default().spacing(0.0));

            // scrollable_table
            ScrollArea::vertical()
                .id_salt("isa_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    Frame::NONE
                        .inner_margin(Margin::symmetric(14, 10))
                        .show(ui, |ui| {
                            // col_legend_top
                            col_legend(ui);
                            ui.add_space(6.0);

                            for section in SECTIONS {
                                show_section(ui, section);
                            }

                            ui.add_space(10.0);
                            ui.label(
                                RichText::new(
                                    "Notes: Clocks column shows AVRe timing (applies to ATmega128A). \
                                     \"—\" for SPM varies with NVM programming time.",
                                )
                                .monospace()
                                .size(10.5)
                                .color(DIM),
                            );
                        });
                });
        });
}

// rendering_helpers

fn col_legend(ui: &mut Ui) {
    // use the same Grid settings as show_section so columns align with the data
    Grid::new("isa_col_legend")
        .num_columns(5)
        .min_col_width(20.0)
        .spacing([10.0, 1.5])
        .show(ui, |ui| {
            ui.label(RichText::new(format!("{:<7}", "MNEM")).monospace().size(12.5).color(DIM));
            ui.label(RichText::new(format!("{:<12}", "OPERANDS")).monospace().size(11.5).color(DIM));
            ui.label(RichText::new("DESCRIPTION").monospace().size(11.5).color(DIM));
            ui.label(RichText::new(format!("{:<12}", "FLAGS")).monospace().size(11.5).color(DIM));
            ui.label(RichText::new("CLK").monospace().size(11.5).color(DIM));
            ui.end_row();
        });
}

fn show_section(ui: &mut Ui, section: &Section) {
    ui.add_space(10.0);

    // section_hdr
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!(":: {} ", section.title))
                .monospace()
                .size(12.5)
                .color(SEC_COL),
        );
        ui.add(egui::Separator::default().horizontal().spacing(4.0));
    });
    ui.add_space(4.0);

    Grid::new(section.title)
        .num_columns(5)
        .min_col_width(20.0)
        .spacing([10.0, 1.5])
        .show(ui, |ui| {
            for &(mnem, ops, desc, flags, clk) in section.rows {
                // mnem_amber_larger
                ui.label(
                    RichText::new(format!("{mnem:<7}"))
                        .monospace()
                        .size(12.5)
                        .color(AMBER),
                );
                // ops_col
                ui.label(
                    RichText::new(format!("{ops:<12}"))
                        .monospace()
                        .size(11.5)
                        .color(START_GREEN),
                );
                // desc_dim
                ui.label(
                    RichText::new(desc)
                        .monospace()
                        .size(11.5)
                        .color(START_GREEN_DIM),
                );
                // flags_green
                ui.label(
                    RichText::new(format!("{flags:<12}"))
                        .monospace()
                        .size(11.5)
                        .color(START_GREEN),
                );
                // clk_col
                ui.label(
                    RichText::new(clk)
                        .monospace()
                        .size(11.5)
                        .color(DIM),
                );
                ui.end_row();
            }
        });
}
