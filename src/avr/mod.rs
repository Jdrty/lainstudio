//! avr_core atmega128a

pub mod assembler;
pub mod cpu;
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
}
