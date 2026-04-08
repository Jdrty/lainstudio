//! intel HEX (IHEX) for AVR flash — avrdude `-U flash:w:file.hex:i` compatible
/// `application_flash_words` should be the span you intend to program (omit the
/// bootloader region at the end of flash when using a serial bootloader).
/// build Intel HEX from flash words
pub fn flash_words_to_intel_hex(words: &[u16], application_flash_words: usize) -> String {
    let mut out = String::new();
    let mut buf: Vec<u8> = Vec::with_capacity(application_flash_words * 2);
    for i in 0..application_flash_words {
        let w = words.get(i).copied().unwrap_or(0xFFFF);
        buf.push(w as u8);
        buf.push((w >> 8) as u8);
    }

    let mut upper: u32 = 0xFFFF;
    let mut i = 0usize;
    while i < buf.len() {
        let abs = i as u32;
        let u = abs >> 16;
        if u != upper {
            upper = u;
            let b = (u >> 8) as u8;
            let a = u as u8;
            out.push_str(&format_hex_record(2, 0x0000, 4, &[b, a]));
            out.push('\n');
        }
        let addr16 = (abs & 0xFFFF) as u16;
        let room = (0x10000u32 - (abs & 0xFFFF)) as usize;
        let chunk = (buf.len() - i).min(16).min(room);
        let slice = &buf[i..i + chunk];
        out.push_str(&format_hex_record(chunk as u8, addr16, 0, slice));
        out.push('\n');
        i += chunk;
    }

    out.push_str(":00000001FF\n");
    out
}

fn format_hex_record(len: u8, addr: u16, typ: u8, data: &[u8]) -> String {
    let mut sum: u32 = len as u32
        + ((addr >> 8) as u32)
        + ((addr & 0xFF) as u32)
        + typ as u32;
    for &b in data {
        sum += b as u32;
    }
    let cc = (0u32.wrapping_sub(sum) & 0xFF) as u8;
    let mut s = format!(":{len:02X}{addr:04X}{typ:02X}");
    for &b in data {
        s.push_str(&format!("{b:02X}"));
    }
    s.push_str(&format!("{cc:02X}"));
    s
}

/// basic structural + checksum
pub fn validate_intel_hex(text: &str) -> Result<(), String> {
    let mut saw_eof = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if !line.starts_with(':') {
            return Err(format!("Invalid line (no ':'): {}", line.chars().take(40).collect::<String>()));
        }
        let hex_part = &line[1..];
        if hex_part.len() < 10 || hex_part.len() % 2 != 0 {
            return Err("Invalid record length".to_string());
        }
        let bytes: Result<Vec<u8>, _> = (0..hex_part.len())
            .step_by(2)
            .map(|j| u8::from_str_radix(&hex_part[j..j + 2], 16))
            .collect();
        let bytes = bytes.map_err(|_| "Invalid hex digit".to_string())?;
        let sum: u32 = bytes.iter().map(|&b| b as u32).sum();
        if sum & 0xFF != 0 {
            return Err("Checksum mismatch".to_string());
        }
        let typ = bytes[3];
        if typ == 1 {
            saw_eof = true;
        }
    }
    if !saw_eof {
        return Err("Missing end-of-file record (:00000001FF)".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eof_only_validates() {
        validate_intel_hex(":00000001FF\n").unwrap();
    }

    #[test]
    fn short_flash_roundtrip() {
        let mut w = vec![0u16; 4];
        w[0] = 0x940c;
        w[1] = 0x0000;
        let hex = flash_words_to_intel_hex(&w, 4);
        validate_intel_hex(&hex).unwrap();
        assert!(hex.contains(":00000001FF"));
    }
}
