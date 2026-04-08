//! avr_core atmega128a

pub mod assembler;
pub mod cpu;
pub mod intel_hex;
pub mod io_map;
pub use cpu::Cpu;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum McuModel {
    Atmega128A,
    Atmega328P,
}

impl McuModel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Atmega128A => "ATmega128A",
            Self::Atmega328P => "ATmega328P",
        }
    }

    pub fn flash_word_count(self) -> usize {
        match self {
            Self::Atmega128A => crate::avr::cpu::FLASH_WORDS_128A,
            Self::Atmega328P => crate::avr::cpu::FLASH_WORDS_328P,
        }
    }

    pub fn bootloader_reserved_words(self) -> usize {
        match self {
            Self::Atmega328P => 256, // 512 B — Optiboot on Uno / Nano (starts at byte 0x7E00)
            Self::Atmega128A => 512, // 1024 B — typical for larger AVRs with Arduino-style boot
        }
    }

    /// Application flash word count for serial uploads (full flash minus bootloader tail).
    pub fn application_flash_words(self) -> usize {
        self.flash_word_count()
            .saturating_sub(self.bootloader_reserved_words())
    }

    /// `avrdude -p` part id (short form).
    pub fn avrdude_part(self) -> &'static str {
        match self {
            Self::Atmega128A => "m128",
            Self::Atmega328P => "m328p",
        }
    }
}
