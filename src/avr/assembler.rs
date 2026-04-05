//! two_pass_avr_asm full_isa
//! pass0 collect equ set def
//! pass1 labels word_addrs count_words
//! pass2 encode branch_fixup
//! extras: equ forms bare_assign $hex low_high io_names equ_immediates

use std::collections::HashMap;
use super::io_map;

// public_api

#[derive(Debug, Clone)]
pub struct AsmError {
    pub line: usize,
    pub msg:  String,
}

impl std::fmt::Display for AsmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.msg)
    }
}

/// Assemble source and also return a source-map of (1-indexed line → word address).
/// Only instruction lines appear in the map; labels, directives, and blanks are omitted.
/// The map is sorted by line number ascending.
pub fn assemble_full(
    source: &str,
) -> Result<(Vec<u16>, Vec<(usize, u32)>), Vec<AsmError>> {
    assemble_inner(source, true)
}

pub fn assemble(source: &str) -> Result<Vec<u16>, Vec<AsmError>> {
    assemble_inner(source, false).map(|(w, _)| w)
}

fn assemble_inner(
    source: &str,
    build_map: bool,
) -> Result<(Vec<u16>, Vec<(usize, u32)>), Vec<AsmError>> {
    // preprocess: normalize local numeric labels (1: / 1b / 1f)
    let preprocessed = normalize_local_labels(source);
    let source = preprocessed.as_str();

    // pass_0 builtins_then_user_equ sym resolves in_order no_forward_equ
    let mut equates = builtin_equates();
    for raw in source.lines() {
        let line = strip_comment(raw).trim();
        if let Some((name, val_raw)) = parse_equate_name(line) {
            if let Ok(val) = sym(val_raw, &equates) {
                equates.insert(name, val);
            }
        }
    }

    // pass_1 labels_org_addr
    let mut labels: HashMap<String, u32> = HashMap::new();
    let mut addr: u32 = 0;
    for raw in source.lines() {
        let line = strip_comment(raw).trim();
        if line.is_empty() { continue; }
        if parse_equate_name(line).is_some() { continue; }
        if let Some(new_addr) = parse_org(line, &equates) {
            addr = new_addr;
            continue;
        }
        let is_data = is_data_directive(line);
        if line.starts_with('.') && !is_data { continue; } // skip non-data directives
        let (maybe_label, instr_part) = split_label_instr(line);
        if let Some(lbl) = maybe_label {
            labels.insert(lbl.to_lowercase(), addr);
        }
        if !instr_part.is_empty() {
            addr += instruction_words(instr_part);
        }
    }

    // pass_2 encode_org_nop_pad
    let mut words:      Vec<u16>          = Vec::new();
    let mut source_map: Vec<(usize, u32)> = Vec::new();
    let mut errors:     Vec<AsmError>     = Vec::new();
    addr = 0;
    for (idx, raw) in source.lines().enumerate() {
        let line_nr = idx + 1;
        let line    = strip_comment(raw).trim();
        if line.is_empty() { continue; }
        if parse_equate_name(line).is_some() { continue; }
        if let Some(new_addr) = parse_org(line, &equates) {
            if new_addr < addr {
                errors.push(AsmError {
                    line: line_nr,
                    msg: format!(".org 0x{new_addr:04X} is behind current address 0x{addr:04X}"),
                });
            } else {
                // org_gap_nop_fill
                while (words.len() as u32) < new_addr {
                    words.push(0x0000);
                }
                addr = new_addr;
            }
            continue;
        }
        let is_data = is_data_directive(line);
        if line.starts_with('.') && !is_data { continue; } // skip non-data directives
        let (maybe_label, instr_part) = split_label_instr(line);
        // pure label line – nothing to encode
        if maybe_label.is_some() && instr_part.is_empty() { continue; }
        let instr_line = if maybe_label.is_some() { instr_part } else { line };
        if build_map { source_map.push((line_nr, addr)); }
        match encode(instr_line, addr, &labels, &equates) {
            Ok(encoded) => { addr += encoded.len() as u32; words.extend(encoded); }
            Err(msg)    => errors.push(AsmError { line: line_nr, msg }),
        }
    }
    if errors.is_empty() { Ok((words, source_map)) } else { Err(errors) }
}

// directive_handling

/// parse_org target_word_addr or none
fn parse_org(line: &str, equates: &HashMap<String, u32>) -> Option<u32> {
    let rest = line.trim()
        .strip_prefix(".org")
        .or_else(|| line.trim().strip_prefix(".ORG"))?;
    sym(rest.trim(), equates).ok()
}

/// builtin_equates like avr_io_h io_names→io_addr_0_3f in_out_sbi_cbi_ok bit_eq→0_7
fn builtin_equates() -> HashMap<String, u32> {
    let mut m: HashMap<String, u32> = HashMap::new();
    // add closure lowercases keys
    let mut add = |k: &str, v: u32| { m.insert(k.to_lowercase(), v); };

    // io_addrs_io_names (standard IO, 6-bit address for IN/OUT)
    for &(name, io_addr) in io_map::IO_NAMES {
        add(name, io_addr as u32);
    }

    // ext_io_data_addrs lds_sts_only (data addresses >= 0x60, not in IO_NAMES)
    add("PINF",   io_map::PINF   as u32);
    add("DDRF",   io_map::DDRF   as u32);
    add("PORTF",  io_map::PORTF  as u32);
    add("PING",   io_map::PING   as u32);
    add("DDRG",   io_map::DDRG   as u32);
    add("PORTG",  io_map::PORTG  as u32);
    add("EICRA",  io_map::EICRA  as u32);
    add("ETIFR",  io_map::ETIFR  as u32);
    add("ETIMSK", io_map::ETIMSK as u32);
    add("ICR3L",  io_map::ICR3L  as u32);
    add("ICR3H",  io_map::ICR3H  as u32);
    add("OCR3CL", io_map::OCR3CL as u32);
    add("OCR3CH", io_map::OCR3CH as u32);
    add("OCR3BL", io_map::OCR3BL as u32);
    add("OCR3BH", io_map::OCR3BH as u32);
    add("OCR3AL", io_map::OCR3AL as u32);
    add("OCR3AH", io_map::OCR3AH as u32);
    add("TCNT3L", io_map::TCNT3L as u32);
    add("TCNT3H", io_map::TCNT3H as u32);
    add("TCCR3B", io_map::TCCR3B as u32);
    add("TCCR3A", io_map::TCCR3A as u32);
    add("TCCR3C", io_map::TCCR3C as u32);
    add("UBRR0H", io_map::UBRR0H as u32);
    add("UCSR1A", io_map::UCSR1A as u32);
    add("UCSR1B", io_map::UCSR1B as u32);
    add("UCSR1C", io_map::UCSR1C as u32);
    add("UBRR1H", io_map::UBRR1H as u32);
    add("UBRR1L", io_map::UBRR1L as u32);
    add("UDR1",   io_map::UDR1   as u32);

    // memory_map_constants
    add("RAMSTART",   0x0100);
    add("RAMEND",     0x10FF);
    add("FLASHEND",   0xFFFF);
    add("E2END",      0x0FFF);
    add("E2PAGESIZE", 8);
    add("PAGESIZE",   128);

    // ivt_word_addrs
    add("RESET_vect",        0x0000);
    add("INT0_vect",         0x0002);
    add("INT1_vect",         0x0004);
    add("INT2_vect",         0x0006);
    add("INT3_vect",         0x0008);
    add("INT4_vect",         0x000A);
    add("INT5_vect",         0x000C);
    add("INT6_vect",         0x000E);
    add("INT7_vect",         0x0010);
    add("TIMER2_COMP_vect",  0x0012);
    add("TIMER2_OVF_vect",   0x0014);
    add("TIMER1_CAPT_vect",  0x0016);
    add("TIMER1_COMPA_vect", 0x0018);
    add("TIMER1_COMPB_vect", 0x001A);
    add("TIMER1_OVF_vect",   0x001C);
    add("TIMER0_COMP_vect",  0x001E);
    add("TIMER0_OVF_vect",   0x0020);
    add("SPI_STC_vect",      0x0022);
    add("USART0_RX_vect",    0x0024);
    add("USART0_UDRE_vect",  0x0026);
    add("USART0_TX_vect",    0x0028);
    add("ADC_vect",          0x002A);
    add("EE_RDY_vect",       0x002C);
    add("ANA_COMP_vect",     0x002E);
    add("TIMER1_COMPC_vect", 0x0030);
    add("TIMER3_CAPT_vect",  0x0032);
    add("TIMER3_COMPA_vect", 0x0034);
    add("TIMER3_COMPB_vect", 0x0036);
    add("TIMER3_COMPC_vect", 0x0038);
    add("TIMER3_OVF_vect",   0x003A);
    add("USART1_RX_vect",    0x003C);
    add("USART1_UDRE_vect",  0x003E);
    add("USART1_TX_vect",    0x0040);
    add("TWI_vect",          0x0042);
    add("SPM_RDY_vect",      0x0044);

    // sreg_bit_indices
    add("SREG_C", 0); add("SREG_Z", 1); add("SREG_N", 2); add("SREG_V", 3);
    add("SREG_S", 4); add("SREG_H", 5); add("SREG_T", 6); add("SREG_I", 7);

    // port_bit_aliases_px_n
    for port in ['A','B','C','D','E','F','G'] {
        for bit in 0u32..8 {
            add(&format!("P{port}{bit}"),    bit);
            add(&format!("PORT{port}{bit}"), bit);
            add(&format!("DDR{port}{bit}"),  bit);
            add(&format!("PIN{port}{bit}"),  bit);
        }
    }

    // timer0_bits
    add("CS00",  0); add("CS01",  1); add("CS02",  2);
    add("WGM01", 3); add("COM00", 4); add("COM01", 5); add("WGM00", 6); add("FOC0", 7);

    // timer1_bits
    // tccr1a
    add("WGM10",  0); add("WGM11",  1);
    add("COM1C0", 2); add("COM1C1", 3);
    add("COM1B0", 4); add("COM1B1", 5);
    add("COM1A0", 6); add("COM1A1", 7);
    // tccr1b
    add("CS10",  0); add("CS11",  1); add("CS12",  2);
    add("WGM12", 3); add("WGM13", 4);
    add("ICES1", 6); add("ICNC1", 7);
    // tccr1c
    add("FOC1C", 5); add("FOC1B", 6); add("FOC1A", 7);

    // timer2_bits
    add("CS20",  0); add("CS21",  1); add("CS22",  2);
    add("WGM21", 3); add("COM20", 4); add("COM21", 5); add("WGM20", 6); add("FOC2", 7);

    // timer3_bits_like_t1
    add("WGM30",  0); add("WGM31",  1);
    add("COM3C0", 2); add("COM3C1", 3);
    add("COM3B0", 4); add("COM3B1", 5);
    add("COM3A0", 6); add("COM3A1", 7);
    add("CS30",   0); add("CS31",   1); add("CS32",   2);
    add("WGM32",  3); add("WGM33",  4);
    add("ICES3",  6); add("ICNC3",  7);

    // timsk
    add("TOIE0", 0); add("OCIE0",  1);
    add("TOIE1", 2); add("OCIE1B", 3); add("OCIE1A", 4); add("TICIE1", 5);
    add("TOIE2", 6); add("OCIE2",  7);

    // tifr
    add("TOV0", 0); add("OCF0",  1);
    add("TOV1", 2); add("OCF1B", 3); add("OCF1A", 4); add("ICF1", 5);
    add("TOV2", 6); add("OCF2",  7);

    // etimsk
    add("TOIE3",  0); add("OCIE3B", 1); add("OCIE3A", 2); add("TICIE3", 3);
    add("OCIE3C", 4); add("OCIE1C", 5);

    // etifr
    add("TOV3",  0); add("OCF3B", 1); add("OCF3A", 2); add("ICF3", 3);
    add("OCF3C", 4); add("OCF1C", 5);

    // spi_bits
    add("SPR0", 0); add("SPR1", 1); add("CPHA", 2); add("CPOL", 3);
    add("MSTR", 4); add("DORD", 5); add("SPE",  6); add("SPIE", 7);
    add("SPI2X", 0); add("WCOL", 6); add("SPIF", 7);

    // usart0
    // ucsr0a
    add("MPCM0", 0); add("U2X0",  1); add("UPE0", 2); add("DOR0",  3);
    add("FE0",   4); add("UDRE0", 5); add("TXC0", 6); add("RXC0",  7);
    // ucsr0b
    add("TXB80",  0); add("RXB80",  1); add("UCSZ02", 2); add("TXEN0",  3);
    add("RXEN0",  4); add("UDRIE0", 5); add("TXCIE0", 6); add("RXCIE0", 7);
    // ucsr0c
    add("UCPOL0", 0); add("UCSZ00", 1); add("UCSZ01", 2); add("USBS0", 3);
    add("UPM00",  4); add("UPM01",  5); add("UMSEL0", 6);

    // usart1_same_layout
    add("MPCM1", 0); add("U2X1",  1); add("UPE1", 2); add("DOR1",  3);
    add("FE1",   4); add("UDRE1", 5); add("TXC1", 6); add("RXC1",  7);
    add("TXB81",  0); add("RXB81",  1); add("UCSZ12", 2); add("TXEN1",  3);
    add("RXEN1",  4); add("UDRIE1", 5); add("TXCIE1", 6); add("RXCIE1", 7);
    add("UCPOL1", 0); add("UCSZ10", 1); add("UCSZ11", 2); add("USBS1", 3);
    add("UPM10",  4); add("UPM11",  5); add("UMSEL1", 6);

    // adc
    // adcsra
    add("ADPS0", 0); add("ADPS1", 1); add("ADPS2", 2);
    add("ADIE",  3); add("ADIF",  4); add("ADATE", 5); add("ADSC", 6); add("ADEN", 7);
    // admux
    add("MUX0",  0); add("MUX1",  1); add("MUX2", 2); add("MUX3", 3); add("MUX4", 4);
    add("ADLAR", 5); add("REFS0", 6); add("REFS1", 7);

    // acsr_bits
    add("ACIS0", 0); add("ACIS1", 1); add("ACIC", 2); add("ACIE", 3);
    add("ACI",   4); add("ACO",   5); add("ACBG", 6); add("ACD",  7);

    // eimsk_bits
    add("INT0", 0); add("INT1", 1); add("INT2", 2); add("INT3", 3);
    add("INT4", 4); add("INT5", 5); add("INT6", 6); add("INT7", 7);

    // eeprom_control_bits (EECR register)
    add("EERE",  0); // read enable
    add("EEWE",  1); // write enable
    add("EEMWE", 2); // master write enable
    add("EERIE", 3); // ready interrupt enable
    add("EEPM0", 4);
    add("EEPM1", 5);

    // wdt_bits
    add("WDP0", 0); add("WDP1", 1); add("WDP2", 2); add("WDE", 3); add("WDCE", 4);

    // mcucr_bits
    add("IVCE",  0); add("IVSEL", 1); add("PUD",   4);
    add("SRW10", 5); add("SRE",   6);

    m
}

/// parse_equate_line name_raw_val no_eval sym resolves
fn parse_equate_name(line: &str) -> Option<(String, &str)> {
    let line = line.trim();
    let rest = line.strip_prefix(".equ")
        .or_else(|| line.strip_prefix(".EQU"))
        .or_else(|| line.strip_prefix(".set"))
        .or_else(|| line.strip_prefix(".SET"))
        .or_else(|| line.strip_prefix(".def"))
        .or_else(|| line.strip_prefix(".DEF"));
    let (name_raw, val_raw) = if let Some(rest) = rest {
        let rest = rest.trim();
        if let Some(i) = rest.find('=') {
            (&rest[..i], &rest[i + 1..])
        } else if let Some(i) = rest.find(',') {
            (&rest[..i], &rest[i + 1..])
        } else {
            return None;
        }
    } else {
        if let Some(i) = line.find('=') {
            let name_part = line[..i].trim();
            if name_part.is_empty() || name_part.contains(' ') { return None; }
            (name_part, &line[i + 1..])
        } else {
            return None;
        }
    };
    let name = name_raw.trim().to_lowercase();
    if name.is_empty() { return None; }
    Some((name, val_raw.trim()))
}

/// try_parse_equ_literal same_forms_as_parse_equate_name
// helpers

fn strip_comment(line: &str) -> &str {
    // strip ; (asm comment) and # (C preprocessor / GAS comment)
    let line = line.find(';').map(|i| &line[..i]).unwrap_or(line);
    line.find('#').map(|i| &line[..i]).unwrap_or(line)
}

/// Split a line into an optional leading label and the remaining instruction text.
/// Handles both `label:` (standalone) and `label: instruction` (same-line, GAS style).
fn split_label_instr(line: &str) -> (Option<&str>, &str) {
    if let Some(colon) = line.find(':') {
        let before = &line[..colon];
        // Label must be a non-empty identifier token (no whitespace inside)
        if !before.is_empty() && !before.contains(|c: char| c.is_whitespace()) {
            let rest = line[colon + 1..].trim();
            return (Some(before.trim()), rest);
        }
    }
    (None, line)
}


/// Rewrite local numeric GAS labels (`1:`, `2:`, …) to unique internal names,
/// and replace all `Nb` / `Nf` references accordingly.
fn normalize_local_labels(source: &str) -> String {
    let lines: Vec<&str> = source.lines().collect();

    // Pass 1: number each `N:` occurrence → unique name __L{digit}_{idx}
    // local_defs[digit] = Vec<(line_index, unique_name)>
    let mut local_defs: std::collections::HashMap<u32, Vec<(usize, String)>> = Default::default();
    let mut counters:   std::collections::HashMap<u32, usize>                 = Default::default();

    for (ln, raw) in lines.iter().enumerate() {
        let stripped = strip_comment(raw).trim();
        // Accept standalone `N:` or inline `N: instruction`
        if let (Some(lbl), _) = split_label_instr(stripped) {
            if !lbl.is_empty() && lbl.chars().all(|c| c.is_ascii_digit()) {
                if let Ok(digit) = lbl.parse::<u32>() {
                    let cnt = counters.entry(digit).or_insert(0);
                    let name = format!("__L{digit}_{cnt}");
                    *cnt += 1;
                    local_defs.entry(digit).or_default().push((ln, name));
                }
            }
        }
    }

    if local_defs.is_empty() {
        return source.to_string();
    }

    // Build reverse map: line_idx → unique_name (for renaming label definitions)
    let mut def_at_line: std::collections::HashMap<usize, &str> = Default::default();
    for def_list in local_defs.values() {
        for (ln, name) in def_list {
            def_at_line.insert(*ln, name.as_str());
        }
    }

    // Pass 2: rewrite lines — rename label defs AND Nb/Nf refs
    let mut out = String::with_capacity(source.len() + 64);
    for (ln, raw) in lines.iter().enumerate() {
        // If this line has a local numeric label definition, rename it first.
        let owned;
        let working = if let Some(&uname) = def_at_line.get(&ln) {
            let stripped = strip_comment(raw).trim();
            if let (Some(lbl), _) = split_label_instr(stripped) {
                if lbl.chars().all(|c| c.is_ascii_digit()) {
                    // Replace the digit label with its unique name in the raw line.
                    // Find the first non-whitespace run that equals `lbl` followed by `:`.
                    let leading_ws = raw.len() - raw.trim_start().len();
                    let expected = format!("{}:", lbl);
                    if raw.trim_start().starts_with(&expected) {
                        owned = format!("{}{}{}",
                            &raw[..leading_ws],
                            &format!("{}:", uname),
                            &raw[leading_ws + expected.len()..]);
                        owned.as_str()
                    } else { raw }
                } else { raw }
            } else { raw }
        } else { raw };

        let new_line = rewrite_local_refs(working, ln, &local_defs);
        out.push_str(&new_line);
        out.push('\n');
    }
    // Remove the trailing newline we added (preserve original ending)
    if !source.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    out
}

fn rewrite_local_refs(
    line: &str,
    current_ln: usize,
    defs: &std::collections::HashMap<u32, Vec<(usize, String)>>,
) -> String {
    let b = line.as_bytes();
    let mut out = String::with_capacity(line.len() + 16);
    let mut i = 0;

    while i < b.len() {
        // Detect a potential local-label reference: digit(s) followed by 'b' or 'f',
        // not preceded by an alphanumeric/underscore char (word boundary).
        let prev_word = i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_');
        if !prev_word && b[i].is_ascii_digit() {
            let start = i;
            while i < b.len() && b[i].is_ascii_digit() { i += 1; }
            if i < b.len() && (b[i] == b'b' || b[i] == b'f') {
                let dir = b[i];
                let next_word = (i + 1) < b.len()
                    && (b[i + 1].is_ascii_alphanumeric() || b[i + 1] == b'_');
                if !next_word {
                    let digit_str = &line[start..i];
                    if let Ok(digit) = digit_str.parse::<u32>() {
                        if let Some(def_list) = defs.get(&digit) {
                            let resolved = if dir == b'b' {
                                // most recent definition at or before this line
                                def_list.iter()
                                    .filter(|&&(dl, _)| dl <= current_ln)
                                    .last()
                                    .map(|(_, n)| n.as_str())
                            } else {
                                // next definition after this line
                                def_list.iter()
                                    .find(|&&(dl, _)| dl > current_ln)
                                    .map(|(_, n)| n.as_str())
                            };
                            if let Some(name) = resolved {
                                out.push_str(name);
                                i += 1; // consume 'b' or 'f'
                                continue;
                            }
                        }
                    }
                    // no match – emit as-is
                    out.push_str(&line[start..i]);
                    continue; // don't advance i, loop will push b[i]
                }
            }
            // not a local ref – emit the digits we consumed
            out.push_str(&line[start..i]);
            continue;
        }

        out.push(b[i] as char);
        i += 1;
    }
    out
}

fn is_data_directive(line: &str) -> bool {
    let m = line.split_whitespace().next().unwrap_or("").to_uppercase();
    matches!(m.as_str(), ".BYTE" | ".WORD" | ".SHORT")
}

fn instruction_words(line: &str) -> u32 {
    let (m, rest) = line.split_once(|c: char| c.is_whitespace()).unwrap_or((line, ""));
    match m.to_uppercase().as_str() {
        "LDS" | "STS" | "JMP" | "CALL" => 2,
        ".BYTE" => {
            // each pair of bytes → 1 word; at least 1 word
            let count = rest.split(',').filter(|s| !s.trim().is_empty()).count();
            ((count as u32 + 1) / 2).max(1)
        }
        ".WORD" | ".SHORT" => {
            rest.split(',').filter(|s| !s.trim().is_empty()).count() as u32
        }
        _ => 1,
    }
}

// encoder

fn encode(
    line:    &str,
    cur:     u32,
    labels:  &HashMap<String, u32>,
    equates: &HashMap<String, u32>,
) -> Result<Vec<u16>, String> {
    let (mnem_raw, rest) = line.split_once(|c: char| c.is_whitespace()).unwrap_or((line, ""));
    let rest = rest.trim();
    let m = mnem_raw.to_uppercase();

    // data_directives
    if m == ".BYTE" {
        let bytes: Vec<u8> = rest.split(',')
            .filter(|s| !s.trim().is_empty())
            .map(|s| sym(s.trim(), equates).map(|v| v as u8))
            .collect::<Result<_, _>>()?;
        if bytes.is_empty() { return Ok(vec![0x0000]); }
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            let lo = bytes[i] as u16;
            let hi = if i + 1 < bytes.len() { bytes[i + 1] as u16 } else { 0x00 };
            out.push(lo | (hi << 8));
            i += 2;
        }
        return Ok(out);
    }
    if m == ".WORD" || m == ".SHORT" {
        let words: Vec<u16> = rest.split(',')
            .filter(|s| !s.trim().is_empty())
            .map(|s| sym(s.trim(), equates).map(|v| v as u16))
            .collect::<Result<_, _>>()?;
        return if words.is_empty() { Ok(vec![0x0000]) } else { Ok(words) };
    }

    // zero_op_insts
    const ZERO_OPS: &[(&str, u16)] = &[
        ("NOP",    0x0000), ("RET",    0x9508), ("RETI",   0x9518),
        ("IJMP",   0x9409), ("EIJMP",  0x9419),
        ("ICALL",  0x9509), ("EICALL", 0x9519),
        ("SPM",    0x95E8), ("BREAK",  0x9598),
        ("SLEEP",  0x9588), ("WDR",    0x95A8),
        // sreg_set_clear_aliases
        ("SEC",    0x9408), ("CLC",    0x9488),
        ("SEZ",    0x9418), ("CLZ",    0x9498),
        ("SEN",    0x9428), ("CLN",    0x94A8),
        ("SEV",    0x9438), ("CLV",    0x94B8),
        ("SES",    0x9448), ("CLS",    0x94C8),
        ("SEH",    0x9458), ("CLH",    0x94D8),
        ("SET",    0x9468), ("CLT",    0x94E8),
        ("SEI",    0x9478), ("CLI",    0x94F8),
    ];
    if let Some(&(_, op)) = ZERO_OPS.iter().find(|&&(n, _)| n == m.as_str()) {
        no_ops(rest)?;
        return Ok(vec![op]);
    }

    // single_reg_alu_pat
    const SING_REG: &[(&str, u16)] = &[
        ("COM",  0x9400), ("NEG",  0x9401), ("SWAP", 0x9402),
        ("INC",  0x9403), ("ASR",  0x9405), ("LSR",  0x9406),
        ("ROR",  0x9407), ("DEC",  0x940A), ("PUSH", 0x920F),
        ("POP",  0x900F),
    ];
    if let Some(&(_, base)) = SING_REG.iter().find(|&&(n, _)| n == m.as_str()) {
        let d = reg(rest)? as u16;
        return Ok(vec![base | ((d & 0x10) << 4) | ((d & 0x0F) << 4)]);
    }

    // reg_alias_expand
    match m.as_str() {
        "TST" => { let d = reg(rest)? as u16; return Ok(vec![rr_op(0x2000, d, d)]); }
        "CLR" => { let d = reg(rest)? as u16; return Ok(vec![rr_op(0x2400, d, d)]); }
        "LSL" => { let d = reg(rest)? as u16; return Ok(vec![rr_op(0x0C00, d, d)]); }
        "ROL" => { let d = reg(rest)? as u16; return Ok(vec![rr_op(0x1C00, d, d)]); }
        "SER" => {
            let d = reg(rest)?;
            guard(d >= 16, &format!("SER: Rd must be R16–R31 (got R{d})"))?;
            let off = (d - 16) as u16;
            return Ok(vec![0xE000 | (0xF0 << 4) | (off << 4) | 0x0F]);
        }
        _ => {}
    }

    // rr_alu
    const TWO_REG: &[(&str, u16)] = &[
        ("CPC",  0x0400), ("SBC",  0x0800), ("ADD",  0x0C00),
        ("CPSE", 0x1000), ("CP",   0x1400), ("SUB",  0x1800),
        ("ADC",  0x1C00), ("AND",  0x2000), ("EOR",  0x2400),
        ("OR",   0x2800), ("MOV",  0x2C00), ("MUL",  0x9C00),
    ];
    if let Some(&(_, base)) = TWO_REG.iter().find(|&&(n, _)| n == m.as_str()) {
        let (ds, rs) = two_ops(rest)?;
        return Ok(vec![rr_op(base, reg(ds)? as u16, reg(rs)? as u16)]);
    }

    // mul_family
    match m.as_str() {
        "MULS" => {
            let (ds, rs) = two_ops(rest)?;
            let d = reg(ds)?; let r = reg(rs)?;
            guard(d >= 16 && d <= 31 && r >= 16 && r <= 31, "MULS: needs R16–R31")?;
            return Ok(vec![0x0200 | (((d - 16) as u16) << 4) | ((r - 16) as u16)]);
        }
        "MULSU" | "FMUL" | "FMULS" | "FMULSU" => {
            let (ds, rs) = two_ops(rest)?;
            let d = reg(ds)?; let r = reg(rs)?;
            guard(d >= 16 && d <= 23 && r >= 16 && r <= 23,
                &format!("{m}: needs R16–R23"))?;
            let base: u16 = match m.as_str() {
                "MULSU"  => 0x0300, "FMUL"  => 0x0308,
                "FMULS"  => 0x0380, _        => 0x0388,
            };
            return Ok(vec![base | (((d - 16) as u16) << 4) | ((r - 16) as u16)]);
        }
        _ => {}
    }

    // movw
    if m == "MOVW" {
        let (ds, rs) = two_ops(rest)?;
        let d = reg(ds)?; let r = reg(rs)?;
        guard(d % 2 == 0 && r % 2 == 0, "MOVW: registers must be even")?;
        return Ok(vec![0x0100 | (((d / 2) as u16) << 4) | ((r / 2) as u16)]);
    }

    // imm_r16_r31
    const IMM_OPS: &[(&str, u16)] = &[
        ("LDI",  0xE000), ("CPI",  0x3000),
        ("SUBI", 0x5000), ("SBCI", 0x4000),
        ("ORI",  0x6000), ("ANDI", 0x7000),
        ("SBR",  0x6000), // alias for ORI
        ("CBR",  0x7000), // alias for ANDI (complement applied below)
    ];
    for &(name, base) in IMM_OPS {
        if m != name { continue; }
        let (ds, ks) = two_ops(rest)?;
        let d = reg(ds)?;
        guard(d >= 16 && d <= 31, &format!("{m}: Rd must be R16–R31 (got R{d})"))?;
        let mut k = imm_u8_sym(ks, equates)?;
        if m == "CBR" { k = !k; }
        let doff = (d - 16) as u16;
        let ku   = k as u16;
        return Ok(vec![base | ((ku & 0xF0) << 4) | (doff << 4) | (ku & 0x0F)]);
    }

    // adiw_sbiw
    if m == "ADIW" || m == "SBIW" {
        let (ds, ks) = two_ops(rest)?;
        let d = reg(ds)?;
        let valid = [24u32, 26, 28, 30];
        guard(valid.contains(&d), &format!("{m}: Rd must be R24/R26/R28/R30"))?;
        let k = sym(ks.trim(), equates)?;
        guard(k <= 63, &format!("{m}: K={k} exceeds 63"))?;
        let dd  = ((d - 24) / 2) as u16;
        let ku  = k as u16;
        let base: u16 = if m == "ADIW" { 0x9600 } else { 0x9700 };
        return Ok(vec![base | ((ku & 0x30) << 2) | (dd << 4) | (ku & 0x0F)]);
    }

    // lds_sts_2w
    if m == "LDS" {
        let (ds, ks) = two_ops(rest)?;
        let d = reg(ds)? as u16;
        let k = sym(ks.trim(), equates)? as u16;
        return Ok(vec![0x9000 | ((d & 0x10) << 4) | ((d & 0x0F) << 4), k]);
    }
    if m == "STS" {
        let (ks, rs) = two_ops(rest)?;
        let r = reg(rs)? as u16;
        let k = sym(ks.trim(), equates)? as u16;
        return Ok(vec![0x9200 | ((r & 0x10) << 4) | ((r & 0x0F) << 4), k]);
    }

    // ld_st_ptr
    if m == "LD" || m == "LDD" {
        let (ds, ptrs) = two_ops(rest)?;
        let d = reg(ds)? as u16;
        let ptr = parse_ptr(ptrs)?;
        return Ok(vec![encode_ld(d, ptr)?]);
    }
    if m == "ST" || m == "STD" {
        let (ptrs, rs) = two_ops(rest)?;
        let r = reg(rs)? as u16;
        let ptr = parse_ptr(ptrs)?;
        return Ok(vec![encode_st(r, ptr)?]);
    }

    // lpm_elpm_variants
    if m == "LPM" {
        if rest.is_empty() { return Ok(vec![0x95C8]); }
        let (ds, ptrs) = two_ops(rest)?;
        let d = reg(ds)? as u16;
        let base: u16 = match ptrs.trim().to_uppercase().as_str() {
            "Z"  => 0x9004,
            "Z+" => 0x9005,
            _    => return Err(format!("LPM: invalid pointer '{ptrs}'")),
        };
        return Ok(vec![base | ((d & 0x10) << 4) | ((d & 0x0F) << 4)]);
    }
    if m == "ELPM" {
        if rest.is_empty() { return Ok(vec![0x95D8]); }
        let (ds, ptrs) = two_ops(rest)?;
        let d = reg(ds)? as u16;
        let base: u16 = match ptrs.trim().to_uppercase().as_str() {
            "Z"  => 0x9006,
            "Z+" => 0x9007,
            _    => return Err(format!("ELPM: invalid pointer '{ptrs}'")),
        };
        return Ok(vec![base | ((d & 0x10) << 4) | ((d & 0x0F) << 4)]);
    }
    if m == "SPM" {
        if rest.trim().to_uppercase() == "Z+" { return Ok(vec![0x95F8]); }
        if rest.trim().is_empty() { return Ok(vec![0x95E8]); }
        return Err("SPM takes no operands or 'Z+'".into());
    }

    // in_out
    if m == "IN" {
        let (ds, as_) = two_ops(rest)?;
        let d = reg(ds)? as u16;
        let a = io_addr_sym(as_, equates)? as u16;
        guard(a <= 0x3F, &format!("IN: I/O addr 0x{a:02X} > 0x3F (use LDS instead)"))?;
        return Ok(vec![0xB000 | ((a & 0x30) << 5) | ((d & 0x10) << 4) | ((d & 0x0F) << 4) | (a & 0x0F)]);
    }
    if m == "OUT" {
        let (as_, rs) = two_ops(rest)?;
        let r = reg(rs)? as u16;
        let a = io_addr_sym(as_, equates)? as u16;
        guard(a <= 0x3F, &format!("OUT: I/O addr 0x{a:02X} > 0x3F (use STS instead)"))?;
        return Ok(vec![0xB800 | ((a & 0x30) << 5) | ((r & 0x10) << 4) | ((r & 0x0F) << 4) | (a & 0x0F)]);
    }

    // sbi_cbi_sbis_sbic
    // io_addr_low_32_only
    const IO_BIT_OPS: &[(&str, u16)] = &[
        ("CBI",  0x9800), ("SBIC", 0x9900),
        ("SBI",  0x9A00), ("SBIS", 0x9B00),
    ];
    if let Some(&(_, base)) = IO_BIT_OPS.iter().find(|&&(n, _)| n == m.as_str()) {
        let (as_, bs) = two_ops(rest)?;
        let a = io_addr_sym(as_, equates)?;
        guard(a <= 31, &format!("{m}: I/O address 0x{a:02X} is out of range \
            (SBI/CBI/SBIC/SBIS only reach I/O 0x00–0x1F; use SBRS/SBRC + IN for higher registers)"))?;
        let b = sym(bs.trim(), equates)?;
        guard(b <= 7, &format!("{m}: bit {b} > 7"))?;
        return Ok(vec![base | ((a as u16) << 3) | (b as u16)]);
    }

    // bset_bclr
    if m == "BSET" || m == "BCLR" {
        let s = sym(rest.trim(), equates)?;
        guard(s <= 7, &format!("{m}: bit {s} > 7"))?;
        let base: u16 = if m == "BSET" { 0x9408 } else { 0x9488 };
        return Ok(vec![base | ((s as u16) << 4)]);
    }

    // bst_bld
    if m == "BST" {
        let (rs, bs) = two_ops(rest)?;
        let r = reg(rs)? as u16;
        let b = sym(bs.trim(), equates)?;
        guard(b <= 7, "BST: bit > 7")?;
        return Ok(vec![0xFA00 | ((r & 0x10) << 4) | ((r & 0x0F) << 4) | (b as u16)]);
    }
    if m == "BLD" {
        let (ds, bs) = two_ops(rest)?;
        let d = reg(ds)? as u16;
        let b = sym(bs.trim(), equates)?;
        guard(b <= 7, "BLD: bit > 7")?;
        return Ok(vec![0xF800 | ((d & 0x10) << 4) | ((d & 0x0F) << 4) | (b as u16)]);
    }

    // sbrc_sbrs
    if m == "SBRC" || m == "SBRS" {
        let (rs, bs) = two_ops(rest)?;
        let r = reg(rs)? as u16;
        let b = sym(bs.trim(), equates)?;
        guard(b <= 7, &format!("{m}: bit > 7"))?;
        let base: u16 = if m == "SBRC" { 0xFC00 } else { 0xFE00 };
        return Ok(vec![base | ((r & 0x10) << 4) | ((r & 0x0F) << 4) | (b as u16)]);
    }

    // jumps_calls
    if m == "RJMP" {
        let k = branch_offset(rest, cur, labels, equates)?;
        guard(k >= -2048 && k <= 2047, &format!("RJMP offset {k} out of ±2048"))?;
        return Ok(vec![0xC000 | ((k as u16) & 0x0FFF)]);
    }
    if m == "RCALL" {
        let k = branch_offset(rest, cur, labels, equates)?;
        guard(k >= -2048 && k <= 2047, &format!("RCALL offset {k} out of ±2048"))?;
        return Ok(vec![0xD000 | ((k as u16) & 0x0FFF)]);
    }
    if m == "JMP" {
        let k = resolve_addr(rest, labels, equates)?;
        return Ok(vec![0x940C, k as u16]);
    }
    if m == "CALL" {
        let k = resolve_addr(rest, labels, equates)?;
        return Ok(vec![0x940E, k as u16]);
    }

    // conditional_branches
    let branch_table: &[(&str, u16, u8)] = &[
        ("BRBS", 0xF000, 255), ("BRBC", 0xF400, 255),
        ("BREQ", 0xF000,   1), ("BRNE", 0xF400,   1),
        ("BRCS", 0xF000,   0), ("BRCC", 0xF400,   0),
        ("BRLO", 0xF000,   0), ("BRSH", 0xF400,   0),
        ("BRMI", 0xF000,   2), ("BRPL", 0xF400,   2),
        ("BRVS", 0xF000,   3), ("BRVC", 0xF400,   3),
        ("BRLT", 0xF000,   4), ("BRGE", 0xF400,   4),
        ("BRHS", 0xF000,   5), ("BRHC", 0xF400,   5),
        ("BRTS", 0xF000,   6), ("BRTC", 0xF400,   6),
        ("BRIE", 0xF000,   7), ("BRID", 0xF400,   7),
    ];
    if let Some(&(_, base, fixed_s)) = branch_table.iter().find(|&&(n, _, _)| n == m.as_str()) {
        let (s_str, k_target) = if fixed_s == 255 {
            two_ops(rest)?
        } else {
            ("0", rest)
        };
        let s_val: u16 = if fixed_s == 255 {
            let sv = sym(s_str.trim(), equates)?;
            guard(sv <= 7, &format!("{m}: SREG bit {sv} > 7"))?;
            sv as u16
        } else {
            fixed_s as u16
        };
        let k = branch_offset(k_target, cur, labels, equates)?;
        guard(k >= -64 && k <= 63, &format!("{m}: offset {k} out of ±64 word range"))?;
        return Ok(vec![base | (((k as u16) & 0x7F) << 3) | s_val]);
    }

    // des_k
    if m == "DES" {
        let k = sym(rest.trim(), equates)?;
        guard(k <= 15, "DES: K must be 0–15")?;
        return Ok(vec![0x940B | ((k as u16) << 4)]);
    }

    // avrxm_mem_ops_stub
    const AVXM: &[(&str, u16)] = &[
        ("XCH", 0x9204), ("LAS", 0x9205), ("LAC", 0x9206), ("LAT", 0x9207),
    ];
    if let Some(&(_, base)) = AVXM.iter().find(|&&(n, _)| n == m.as_str()) {
        let (_, rs) = two_ops(rest)?;
        let r = reg(rs)? as u16;
        return Ok(vec![base | ((r & 0x10) << 4) | ((r & 0x0F) << 4)]);
    }

    Err(format!("Unknown mnemonic '{}'", mnem_raw))
}

// pointer_addressing

#[derive(Clone, Copy)]
enum Ptr {
    X, Xp, Xm,
    Y, Yp, Ym, Yq(u8),
    Z, Zp, Zm, Zq(u8),
}

fn parse_ptr(s: &str) -> Result<Ptr, String> {
    let u = s.trim().to_uppercase();
    Ok(match u.as_str() {
        "X"  => Ptr::X,  "X+" => Ptr::Xp, "-X" => Ptr::Xm,
        "Y"  => Ptr::Y,  "Y+" => Ptr::Yp, "-Y" => Ptr::Ym,
        "Z"  => Ptr::Z,  "Z+" => Ptr::Zp, "-Z" => Ptr::Zm,
        _ => {
            if let Some(qs) = u.strip_prefix("Y+") {
                let q = parse_imm(qs)?;
                guard(q <= 63, &format!("Displacement {q} > 63"))?;
                return Ok(Ptr::Yq(q as u8));
            }
            if let Some(qs) = u.strip_prefix("Z+") {
                let q = parse_imm(qs)?;
                guard(q <= 63, &format!("Displacement {q} > 63"))?;
                return Ok(Ptr::Zq(q as u8));
            }
            if u.starts_with("X+") {
                return Err(
                    "X register does not support displacement addressing (AVR ISA); \
                     use Y+q or Z+q, or manually adjust X before loading".to_string()
                );
            }
            return Err(format!("Unknown pointer operand '{s}'"));
        }
    })
}

fn encode_ld(d: u16, ptr: Ptr) -> Result<u16, String> {
    let d_bits = |base: u16| base | ((d & 0x10) << 4) | ((d & 0x0F) << 4);
    Ok(match ptr {
        Ptr::X     => d_bits(0x900C), Ptr::Xp  => d_bits(0x900D), Ptr::Xm  => d_bits(0x900E),
        Ptr::Y     => ldd_y(d, 0),    Ptr::Yp  => d_bits(0x9009), Ptr::Ym  => d_bits(0x900A),
        Ptr::Z     => ldd_z(d, 0),    Ptr::Zp  => d_bits(0x9001), Ptr::Zm  => d_bits(0x9002),
        Ptr::Yq(q) => ldd_y(d, q as u16),
        Ptr::Zq(q) => ldd_z(d, q as u16),
    })
}

fn encode_st(r: u16, ptr: Ptr) -> Result<u16, String> {
    let r_bits = |base: u16| base | ((r & 0x10) << 4) | ((r & 0x0F) << 4);
    Ok(match ptr {
        Ptr::X     => r_bits(0x920C), Ptr::Xp  => r_bits(0x920D), Ptr::Xm  => r_bits(0x920E),
        Ptr::Y     => std_y(r, 0),    Ptr::Yp  => r_bits(0x9209), Ptr::Ym  => r_bits(0x920A),
        Ptr::Z     => std_z(r, 0),    Ptr::Zp  => r_bits(0x9201), Ptr::Zm  => r_bits(0x9202),
        Ptr::Yq(q) => std_y(r, q as u16),
        Ptr::Zq(q) => std_z(r, q as u16),
    })
}

/// ldd_z bit_pattern_10q0
fn ldd_z(d: u16, q: u16) -> u16 {
    0x8000
    | ((q & 0x20) << 8) | ((q & 0x18) << 7)
    | ((d & 0x10) << 4) | ((d & 0x0F) << 4)
    | (q & 0x07)
}
fn ldd_y(d: u16, q: u16) -> u16 { ldd_z(d, q) | 0x0008 }

/// std_z bit_pattern_store
fn std_z(r: u16, q: u16) -> u16 {
    0x8200
    | ((q & 0x20) << 8) | ((q & 0x18) << 7)
    | ((r & 0x10) << 4) | ((r & 0x0F) << 4)
    | (q & 0x07)
}
fn std_y(r: u16, q: u16) -> u16 { std_z(r, q) | 0x0008 }

// operand_parsers

fn no_ops(rest: &str) -> Result<(), String> {
    if !rest.trim().is_empty() { Err(format!("Unexpected operand: '{rest}'")) } else { Ok(()) }
}

fn two_ops(operands: &str) -> Result<(&str, &str), String> {
    match operands.find(',') {
        None    => Err("Expected two operands separated by ','".into()),
        Some(i) => Ok((operands[..i].trim(), operands[i + 1..].trim())),
    }
}

fn guard(cond: bool, msg: &str) -> Result<(), String> {
    if cond { Ok(()) } else { Err(msg.to_string()) }
}

fn reg(s: &str) -> Result<u32, String> {
    let u = s.trim().to_uppercase();
    // named aliases (avr-libc / GAS convention)
    match u.as_str() {
        "XL" => return Ok(26), "XH" => return Ok(27),
        "YL" => return Ok(28), "YH" => return Ok(29),
        "ZL" => return Ok(30), "ZH" => return Ok(31),
        _ => {}
    }
    let digits = u.strip_prefix('R').ok_or_else(|| format!("Expected register, got '{s}'"))?;
    let n: u32 = digits.parse().map_err(|_| format!("Invalid register '{s}'"))?;
    guard(n <= 31, &format!("R{n} out of range"))?;
    Ok(n)
}

fn imm_u8_sym(s: &str, equates: &HashMap<String, u32>) -> Result<u8, String> {
    let v = sym(s.trim(), equates)?;
    guard(v <= 255, &format!("Immediate {v} > 255"))?;
    Ok(v as u8)
}

// -- expression evaluator: handles (1 << N) | ... style operands --

#[derive(Clone, Debug)]
enum ExprTok {
    Num(u32),
    Id(String),
    Shl,
    Shr,
    LParen,
    RParen,
    Op(char),
}

fn tokenize_expr(s: &str) -> Result<Vec<ExprTok>, String> {
    let b = s.as_bytes();
    let mut i = 0;
    let mut out: Vec<ExprTok> = Vec::new();
    while i < b.len() {
        match b[i] {
            c if (c as char).is_ascii_whitespace() => { i += 1; }
            b'(' => { out.push(ExprTok::LParen); i += 1; }
            b')' => { out.push(ExprTok::RParen); i += 1; }
            b'+' | b'-' | b'*' | b'/' | b'|' | b'&' | b'^' | b'~'
                => { out.push(ExprTok::Op(b[i] as char)); i += 1; }
            b'<' if i + 1 < b.len() && b[i + 1] == b'<' => { out.push(ExprTok::Shl); i += 2; }
            b'>' if i + 1 < b.len() && b[i + 1] == b'>' => { out.push(ExprTok::Shr); i += 2; }
            b'$' => {
                i += 1;
                let start = i;
                while i < b.len() && b[i].is_ascii_hexdigit() { i += 1; }
                if i == start { return Err("empty $hex literal".to_string()); }
                out.push(ExprTok::Num(
                    u32::from_str_radix(&s[start..i], 16)
                        .map_err(|_| "invalid $hex".to_string())?,
                ));
            }
            c if (c as char).is_ascii_digit() => {
                if b[i] == b'0' && i + 1 < b.len() && b[i + 1] == b'x' {
                    i += 2;
                    let start = i;
                    while i < b.len() && b[i].is_ascii_hexdigit() { i += 1; }
                    out.push(ExprTok::Num(
                        u32::from_str_radix(&s[start..i], 16)
                            .map_err(|_| "invalid 0x literal".to_string())?,
                    ));
                } else if b[i] == b'0' && i + 1 < b.len() && b[i + 1] == b'b' {
                    i += 2;
                    let start = i;
                    while i < b.len() && (b[i] == b'0' || b[i] == b'1') { i += 1; }
                    out.push(ExprTok::Num(
                        u32::from_str_radix(&s[start..i], 2)
                            .map_err(|_| "invalid 0b literal".to_string())?,
                    ));
                } else {
                    let start = i;
                    while i < b.len() && b[i].is_ascii_digit() { i += 1; }
                    out.push(ExprTok::Num(
                        s[start..i].parse::<u32>()
                            .map_err(|_| "invalid decimal".to_string())?,
                    ));
                }
            }
            c if (c as char).is_ascii_alphabetic() || c == b'_' => {
                let start = i;
                while i < b.len()
                    && (b[i].is_ascii_alphanumeric() || b[i] == b'_') { i += 1; }
                out.push(ExprTok::Id(s[start..i].to_string()));
            }
            c => return Err(format!("unexpected character '{}' in expression", c as char)),
        }
    }
    Ok(out)
}

fn expr_or(t: &[ExprTok], p: &mut usize, eq: &HashMap<String, u32>) -> Result<u32, String> {
    let mut v = expr_xor(t, p, eq)?;
    while matches!(t.get(*p), Some(ExprTok::Op('|'))) { *p += 1; v |= expr_xor(t, p, eq)?; }
    Ok(v)
}
fn expr_xor(t: &[ExprTok], p: &mut usize, eq: &HashMap<String, u32>) -> Result<u32, String> {
    let mut v = expr_and(t, p, eq)?;
    while matches!(t.get(*p), Some(ExprTok::Op('^'))) { *p += 1; v ^= expr_and(t, p, eq)?; }
    Ok(v)
}
fn expr_and(t: &[ExprTok], p: &mut usize, eq: &HashMap<String, u32>) -> Result<u32, String> {
    let mut v = expr_shift(t, p, eq)?;
    while matches!(t.get(*p), Some(ExprTok::Op('&'))) { *p += 1; v &= expr_shift(t, p, eq)?; }
    Ok(v)
}
fn expr_shift(t: &[ExprTok], p: &mut usize, eq: &HashMap<String, u32>) -> Result<u32, String> {
    let mut v = expr_add(t, p, eq)?;
    loop {
        match t.get(*p) {
            Some(ExprTok::Shl) => { *p += 1; v = v.wrapping_shl(expr_add(t, p, eq)?); }
            Some(ExprTok::Shr) => { *p += 1; v = v.wrapping_shr(expr_add(t, p, eq)?); }
            _ => break,
        }
    }
    Ok(v)
}
fn expr_add(t: &[ExprTok], p: &mut usize, eq: &HashMap<String, u32>) -> Result<u32, String> {
    let mut v = expr_mul(t, p, eq)?;
    loop {
        match t.get(*p) {
            Some(ExprTok::Op('+')) => { *p += 1; v = v.wrapping_add(expr_mul(t, p, eq)?); }
            Some(ExprTok::Op('-')) => { *p += 1; v = v.wrapping_sub(expr_mul(t, p, eq)?); }
            _ => break,
        }
    }
    Ok(v)
}
fn expr_mul(t: &[ExprTok], p: &mut usize, eq: &HashMap<String, u32>) -> Result<u32, String> {
    let mut v = expr_unary(t, p, eq)?;
    loop {
        match t.get(*p) {
            Some(ExprTok::Op('*')) => { *p += 1; v = v.wrapping_mul(expr_unary(t, p, eq)?); }
            Some(ExprTok::Op('/')) => {
                *p += 1;
                let d = expr_unary(t, p, eq)?;
                if d == 0 { return Err("division by zero".to_string()); }
                v /= d;
            }
            _ => break,
        }
    }
    Ok(v)
}
fn expr_unary(t: &[ExprTok], p: &mut usize, eq: &HashMap<String, u32>) -> Result<u32, String> {
    match t.get(*p) {
        Some(ExprTok::Op('-')) => { *p += 1; expr_unary(t, p, eq).map(|v| 0u32.wrapping_sub(v)) }
        Some(ExprTok::Op('~')) => { *p += 1; expr_unary(t, p, eq).map(|v| !v) }
        _ => expr_atom(t, p, eq),
    }
}
fn expr_atom(t: &[ExprTok], p: &mut usize, eq: &HashMap<String, u32>) -> Result<u32, String> {
    match t.get(*p).cloned() {
        Some(ExprTok::Num(n)) => { *p += 1; Ok(n) }
        Some(ExprTok::LParen) => {
            *p += 1;
            let v = expr_or(t, p, eq)?;
            if matches!(t.get(*p), Some(ExprTok::RParen)) { *p += 1; Ok(v) }
            else { Err("missing ')' in expression".to_string()) }
        }
        Some(ExprTok::Id(name)) => {
            *p += 1;
            // function call: id '(' expr ')'
            if matches!(t.get(*p), Some(ExprTok::LParen)) {
                *p += 1;
                let inner = expr_or(t, p, eq)?;
                if matches!(t.get(*p), Some(ExprTok::RParen)) { *p += 1; }
                else { return Err(format!("missing ')' after {}()", name)); }
                return match name.to_lowercase().as_str() {
                    "lo8"     | "low"  => Ok(inner & 0xFF),
                    "hi8"     | "high" => Ok((inner >> 8)  & 0xFF),
                    "hh8"              => Ok((inner >> 16) & 0xFF),
                    "hlo8"             => Ok((inner >> 8)  & 0xFF),
                    "hhi8"             => Ok((inner >> 24) & 0xFF),
                    "pm_lo8"           => Ok((inner >> 1)  & 0xFF),
                    "pm_hi8"           => Ok((inner >> 9)  & 0xFF),
                    "pm_hh8"           => Ok((inner >> 17) & 0xFF),
                    "pm_hlo8"          => Ok((inner >> 9)  & 0xFF),
                    "gs"               => Ok(inner >> 1),
                    _ => Err(format!("unknown function '{}'", name)),
                };
            }
            // plain identifier – look up in equates
            let key = name.to_lowercase();
            eq.get(key.as_str())
                .copied()
                .ok_or_else(|| format!("undefined symbol '{}'", name))
        }
        other => Err(format!("unexpected token {:?} in expression", other)),
    }
}

/// Evaluate a symbol/expression string: literals, identifiers, operators, function calls.
fn sym(s: &str, equates: &HashMap<String, u32>) -> Result<u32, String> {
    let s = s.trim();
    let toks = tokenize_expr(s)
        .map_err(|e| format!("Invalid integer '{s}': {e}"))?;
    let mut pos = 0;
    let v = expr_or(&toks, &mut pos, equates)
        .map_err(|_| format!("Invalid integer '{s}'"))?;
    if pos < toks.len() {
        return Err(format!("Invalid integer '{s}'"));
    }
    Ok(v)
}

/// io_addr_sym equ_io_names_num
fn io_addr_sym(s: &str, equates: &HashMap<String, u32>) -> Result<u32, String> {
    // equ_first
    let key = s.trim().to_lowercase();
    if let Some(&v) = equates.get(&key) {
        return Ok(v);
    }
    // io_names
    let upper = s.trim().to_uppercase();
    for &(name, addr) in io_map::IO_NAMES {
        if upper == name { return Ok(addr as u32); }
    }
    // bare_num
    parse_imm(s.trim()).map_err(|_| format!("Unknown I/O register or bad address: '{s}'"))
}

/// parse_imm hex_dollar_bin_dec no_sym_use_sym_instead
pub fn parse_imm(s: &str) -> Result<u32, String> {
    let s = s.trim();
    // hex_0x_or_dollar
    if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return u32::from_str_radix(h, 16).map_err(|_| format!("Invalid hex '{s}'"));
    }
    if let Some(h) = s.strip_prefix('$') {
        return u32::from_str_radix(h, 16).map_err(|_| format!("Invalid hex '{s}'"));
    }
    // bin_0b
    if let Some(b) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
        return u32::from_str_radix(b, 2).map_err(|_| format!("Invalid binary '{s}'"));
    }
    // dec_u32
    s.parse::<u32>().map_err(|_| format!("Invalid integer '{s}'"))
}

fn branch_offset(
    s:       &str,
    cur:     u32,
    labels:  &HashMap<String, u32>,
    equates: &HashMap<String, u32>,
) -> Result<i32, String> {
    let key = s.trim().to_lowercase();
    if let Some(&target) = labels.get(&key) {
        return Ok(target as i32 - (cur as i32 + 1));
    }
    // branch_sym_as_offset_ok
    sym(s.trim(), equates).map(|v| v as i32)
}

fn resolve_addr(
    s:       &str,
    labels:  &HashMap<String, u32>,
    equates: &HashMap<String, u32>,
) -> Result<u32, String> {
    let key = s.trim().to_lowercase();
    if let Some(&target) = labels.get(&key) {
        return Ok(target);
    }
    sym(s.trim(), equates)
}

fn rr_op(base: u16, d: u16, r: u16) -> u16 {
    base | ((r & 0x10) << 5) | ((d & 0x10) << 4) | ((d & 0x0F) << 4) | (r & 0x0F)
}

// tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asm_nop()  { assert_eq!(assemble("NOP").unwrap(), &[0x0000]); }
    #[test]
    fn asm_ldi()  { assert_eq!(assemble("LDI R16, 0x0A").unwrap(), &[0xE00A]); }
    #[test]
    fn asm_add()  { assert_eq!(assemble("ADD R16, R17").unwrap(), &[0x0F01]); }

    #[test]
    fn asm_full_program() {
        let words = assemble("LDI R16, 0x0A\nLDI R17, 0x05\nADD R16, R17\nNOP").unwrap();
        assert_eq!(words, &[0xE00A, 0xE015, 0x0F01, 0x0000]);
    }

    #[test]
    fn asm_out_porta() {
        let words = assemble("OUT PORTA, R16").unwrap();
        let op = words[0];
        assert_eq!(op & 0xF800, 0xB800);
        assert_eq!((op >> 4) & 0x1F, 16);
        assert_eq!(((op >> 5) & 0x30) | (op & 0x0F), 0x1B);
    }

    #[test]
    fn asm_sbi_portb() {
        // regression sbi_portb5→io18_b5
        let words = assemble("SBI PORTB, 5").unwrap();
        assert_eq!(words.len(), 1);
        let op = words[0];
        assert_eq!(op & 0xFF00, 0x9A00);  // sbi_opc
        assert_eq!((op >> 3) & 0x1F, 0x18); // io_18_portb
        assert_eq!(op & 0x07, 5);           // bit5
    }

    #[test]
    fn asm_cbi_ddrb() {
        let words = assemble("CBI DDRB, 3").unwrap();
        let op = words[0];
        assert_eq!(op & 0xFF00, 0x9800);
        assert_eq!((op >> 3) & 0x1F, 0x17); // ddrb_io17
        assert_eq!(op & 0x07, 3);
    }

    #[test]
    fn asm_equ_directive() {
        let src = ".equ LED_PIN = 5\nSBI PORTB, LED_PIN";
        let words = assemble(src).unwrap();
        let op = words[0];
        assert_eq!(op & 0x07, 5);
    }

    #[test]
    fn asm_equ_ldi() {
        let src = ".equ MY_VAL = 42\nLDI R16, MY_VAL";
        let words = assemble(src).unwrap();
        assert_eq!(words.len(), 1);
        // ldi_enc r16_k42 → e20a
        assert_eq!(words[0], 0xE20A);
    }

    #[test]
    fn asm_dollar_hex() {
        // dollar_hex_ok
        let words = assemble("LDI R16, $FF").unwrap();
        assert_eq!(words[0], 0xEF0F); // ldi_r16_ff
    }

    #[test]
    fn asm_low_high() {
        let src = ".equ ADDR = 0x1234\nLDI R16, low(ADDR)\nLDI R17, high(ADDR)";
        // low_high_equ → e304 e112
        let words = assemble(src).unwrap();
        assert_eq!(words.len(), 2);
        assert_eq!(words[0], 0xE304); // ldi_34
        assert_eq!(words[1], 0xE112); // ldi_12
    }

    #[test]
    fn asm_avr_as_operators() {
        // lo8 / hi8 / hh8 / hlo8 / hhi8
        let src = ".equ V = 0xDEADBEEF\n\
                   LDI R16, lo8(V)\n\
                   LDI R17, hi8(V)\n\
                   LDI R18, hh8(V)\n\
                   LDI R19, hhi8(V)\n\
                   LDI R20, hlo8(V)";
        let words = assemble(src).unwrap();
        assert_eq!(words.len(), 5);
        let imm = |w: u16| -> u8 { (((w >> 4) & 0xF0) | (w & 0x0F)) as u8 };
        assert_eq!(imm(words[0]), 0xEF); // lo8  of 0xDEADBEEF
        assert_eq!(imm(words[1]), 0xBE); // hi8  of 0xDEADBEEF
        assert_eq!(imm(words[2]), 0xAD); // hh8  of 0xDEADBEEF
        assert_eq!(imm(words[3]), 0xDE); // hhi8 of 0xDEADBEEF
        assert_eq!(imm(words[4]), 0xBE); // hlo8 == hi8

        // pm_lo8 / pm_hi8: divide by 2 first (byte-addr to word-addr)
        let src2 = ".equ FUNC = 0x0200\n\
                    LDI R16, pm_lo8(FUNC)\n\
                    LDI R17, pm_hi8(FUNC)";
        let words2 = assemble(src2).unwrap();
        assert_eq!(words2.len(), 2);
        assert_eq!(imm(words2[0]), 0x00); // (0x200 >> 1) & 0xFF = 0x00
        assert_eq!(imm(words2[1]), 0x01); // (0x200 >> 9) & 0xFF = 0x01

        // gs: word address (val >> 1)
        let src3 = ".equ FUNC = 0x0100\nLDI R16, lo8(gs(FUNC))";
        let words3 = assemble(src3).unwrap();
        assert_eq!(imm(words3[0]), 0x80); // gs(0x100) = 0x80, lo8 = 0x80
    }

    #[test]
    fn asm_rjmp_label() {
        let words = assemble("loop:\n  RJMP loop").unwrap();
        assert_eq!(words[0] & 0xF000, 0xC000);
        let k12 = words[0] & 0x0FFF;
        let k = if k12 & 0x0800 != 0 { k12 as i16 - 4096 } else { k12 as i16 };
        assert_eq!(k, -1);
    }

    #[test]
    fn asm_breq_brne() {
        let words = assemble("dec_loop:\n  DEC R16\n  BRNE dec_loop").unwrap();
        assert_eq!(words.len(), 2);
        let brne = words[1];
        assert_eq!(brne & 0xFC07, 0xF401);
        let k7  = ((brne >> 3) & 0x7F) as i16;
        let k   = if k7 & 0x40 != 0 { k7 - 128 } else { k7 };
        assert_eq!(k, -2);
    }

    #[test]
    fn asm_ld_st_indirect() {
        let words = assemble("ST X, R16\n LD R17, X").unwrap();
        assert_eq!(words.len(), 2);
        assert_eq!(words[0] & 0xFE0F, 0x920C);
        assert_eq!(words[1] & 0xFE0F, 0x900C);
    }

    #[test]
    fn asm_ldd_stdy() {
        let w = assemble("LDD R16, Y+5").unwrap();
        let op = w[0];
        let q = ((op >> 8) & 0x20) | ((op >> 7) & 0x18) | (op & 0x07);
        let d = (op >> 4) & 0x1F;
        assert_eq!(d, 16);
        assert_eq!(q, 5);
    }

    #[test]
    fn asm_aliases() {
        assert!(assemble("TST R16").is_ok());
        assert!(assemble("CLR R0").is_ok());
        assert!(assemble("LSL R16").is_ok());
        assert!(assemble("ROL R20").is_ok());
        assert!(assemble("SER R16").is_ok());
    }

    #[test]
    fn asm_push_pop_rcall() {
        assert!(assemble("PUSH R16").is_ok());
        assert!(assemble("POP R16").is_ok());
        assert!(assemble("RCALL 5").is_ok());
        assert!(assemble("RET").is_ok());
    }

    #[test]
    fn asm_error_unknown() {
        assert!(assemble("MOV R0, R1").is_ok());
        let errs = assemble("FOOBAR R0").unwrap_err();
        assert_eq!(errs.len(), 1);
        assert!(errs[0].msg.contains("FOOBAR"));
    }

    #[test]
    fn asm_error_ldi_low_reg() {
        let errs = assemble("LDI R0, 5").unwrap_err();
        assert_eq!(errs.len(), 1);
        assert!(errs[0].msg.contains("R16"));
    }
}
