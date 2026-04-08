#![allow(dead_code)]
//! I/O maps: ATmega128A (`IO_NAMES`, `PORTS`) and ATmega328P (`IO_NAMES_328P`, `PORTS_328P`).
//! std_io 0x20_5f io_off data_minus_20 sbi_cbi_to_data_3f ext_io 0x60_ff ld_st

use super::McuModel;

// std_io_low sbi_cbi_ok
// io00_rsrv
pub const PINE:   u16 = 0x0021; // I/O 0x01 Port E Input Pins
pub const DDRE:   u16 = 0x0022; // I/O 0x02 Port E Data Direction
pub const PORTE:  u16 = 0x0023; // I/O 0x03 Port E Data Register

pub const ADCL:   u16 = 0x0024; // I/O 0x04 ADC Data Register Low
pub const ADCH:   u16 = 0x0025; // I/O 0x05 ADC Data Register High
pub const ADCSRA: u16 = 0x0026; // I/O 0x06 ADC Control & Status A
pub const ADMUX:  u16 = 0x0027; // I/O 0x07 ADC Multiplexer Selection

pub const ACSR:   u16 = 0x0028; // I/O 0x08 Analog Comparator Control

pub const UBRR0L: u16 = 0x0029; // I/O 0x09 USART0 Baud Rate Low
pub const UCSR0B: u16 = 0x002A; // I/O 0x0A USART0 Control/Status B
pub const UCSR0A: u16 = 0x002B; // I/O 0x0B USART0 Control/Status A
pub const UDR0:   u16 = 0x002C; // I/O 0x0C USART0 Data Register

pub const SPCR:   u16 = 0x002D; // I/O 0x0D SPI Control Register
pub const SPSR:   u16 = 0x002E; // I/O 0x0E SPI Status Register
pub const SPDR:   u16 = 0x002F; // I/O 0x0F SPI Data Register

pub const PIND:   u16 = 0x0030; // I/O 0x10 Port D Input Pins
pub const DDRD:   u16 = 0x0031; // I/O 0x11 Port D Data Direction
pub const PORTD:  u16 = 0x0032; // I/O 0x12 Port D Data Register

pub const PINC:   u16 = 0x0033; // I/O 0x13 Port C Input Pins
pub const DDRC:   u16 = 0x0034; // I/O 0x14 Port C Data Direction
pub const PORTC:  u16 = 0x0035; // I/O 0x15 Port C Data Register

pub const PINB:   u16 = 0x0036; // I/O 0x16 Port B Input Pins
pub const DDRB:   u16 = 0x0037; // I/O 0x17 Port B Data Direction
pub const PORTB:  u16 = 0x0038; // I/O 0x18 Port B Data Register

pub const PINA:   u16 = 0x0039; // I/O 0x19 Port A Input Pins
pub const DDRA:   u16 = 0x003A; // I/O 0x1A Port A Data Direction
pub const PORTA:  u16 = 0x003B; // I/O 0x1B Port A Data Register

pub const RAMPZ:  u16 = 0x003C; // I/O 0x1C Extended Z-Pointer Register

// io1d_1f_rsrv
// std_io_high in_out_no_sbi_above_1f
pub const WDTCR:  u16 = 0x0041; // I/O 0x21 Watchdog Timer Control

pub const OCR2:   u16 = 0x0043; // I/O 0x23 Timer2 Output Compare
pub const TCNT2:  u16 = 0x0044; // I/O 0x24 Timer/Counter 2
pub const TCCR2:  u16 = 0x0045; // I/O 0x25 Timer2 Control

pub const ICR1L:  u16 = 0x0046; // I/O 0x26 Timer1 Input Capture Low
pub const ICR1H:  u16 = 0x0047; // I/O 0x27 Timer1 Input Capture High
pub const OCR1BL: u16 = 0x0048; // I/O 0x28 Timer1 Compare B Low
pub const OCR1BH: u16 = 0x0049; // I/O 0x29 Timer1 Compare B High
pub const OCR1AL: u16 = 0x004A; // I/O 0x2A Timer1 Compare A Low
pub const OCR1AH: u16 = 0x004B; // I/O 0x2B Timer1 Compare A High
pub const TCNT1L: u16 = 0x004C; // I/O 0x2C Timer/Counter 1 Low
pub const TCNT1H: u16 = 0x004D; // I/O 0x2D Timer/Counter 1 High
pub const TCCR1B: u16 = 0x004E; // I/O 0x2E Timer1 Control B
pub const TCCR1A: u16 = 0x004F; // I/O 0x2F Timer1 Control A

pub const ASSR:   u16 = 0x0050; // I/O 0x30 Async Status Register
pub const OCR0:   u16 = 0x0051; // I/O 0x31 Timer0 Output Compare
pub const TCNT0:  u16 = 0x0052; // I/O 0x32 Timer/Counter 0
pub const TCCR0:  u16 = 0x0053; // I/O 0x33 Timer0 Control

pub const MCUCSR: u16 = 0x0054; // I/O 0x34 MCU Control/Status
pub const MCUCR:  u16 = 0x0055; // I/O 0x35 MCU Control Register

pub const TIFR:   u16 = 0x0056; // I/O 0x36 Timer Interrupt Flag
pub const TIMSK:  u16 = 0x0057; // I/O 0x37 Timer Interrupt Mask

pub const EIFR:   u16 = 0x0058; // I/O 0x38 External Interrupt Flags
pub const EIMSK:  u16 = 0x0059; // I/O 0x39 External Interrupt Mask
pub const EICRB:  u16 = 0x005A; // I/O 0x3A Ext. Interrupt Control B
pub const RAMPZ2: u16 = 0x005B; // I/O 0x3B (reserved/RAMPX)
pub const XDIV:   u16 = 0x005C; // I/O 0x3C XTAL Divide Control

pub const SPL:    u16 = 0x005D; // I/O 0x3D Stack Pointer Low
pub const SPH:    u16 = 0x005E; // I/O 0x3E Stack Pointer High
pub const SREG:   u16 = 0x005F; // I/O 0x3F Status Register

// ext_io ld_st_only
pub const PINF:   u16 = 0x0060; // Port F Input Pins
pub const DDRF:   u16 = 0x0061; // Port F Data Direction
pub const PORTF:  u16 = 0x0062; // Port F Data Register

pub const PING:   u16 = 0x0063; // Port G Input Pins
pub const DDRG:   u16 = 0x0064; // Port G Data Direction
pub const PORTG:  u16 = 0x0065; // Port G Data Register

pub const UCSR1C: u16 = 0x009D; // USART1 Control/Status C
pub const UBRR1H: u16 = 0x0098; // USART1 Baud Rate High
pub const UBRR1L: u16 = 0x0099; // USART1 Baud Rate Low
pub const UCSR1B: u16 = 0x009A; // USART1 Control/Status B
pub const UCSR1A: u16 = 0x009B; // USART1 Control/Status A
pub const UDR1:   u16 = 0x009C; // USART1 Data Register

pub const UBRR0H: u16 = 0x0090; // USART0 Baud Rate High

pub const TCCR3A: u16 = 0x008B; // Timer3 Control A
pub const TCCR3B: u16 = 0x008A; // Timer3 Control B
pub const TCCR3C: u16 = 0x008C; // Timer3 Control C
pub const TCNT3L: u16 = 0x0088; // Timer/Counter 3 Low
pub const TCNT3H: u16 = 0x0089; // Timer/Counter 3 High
pub const OCR3AL: u16 = 0x0086; // Timer3 Compare A Low
pub const OCR3AH: u16 = 0x0087; // Timer3 Compare A High
pub const OCR3BL: u16 = 0x0084; // Timer3 Compare B Low
pub const OCR3BH: u16 = 0x0085; // Timer3 Compare B High
pub const OCR3CL: u16 = 0x0082; // Timer3 Compare C Low
pub const OCR3CH: u16 = 0x0083; // Timer3 Compare C High
pub const ICR3L:  u16 = 0x0080; // Timer3 Input Capture Low
pub const ICR3H:  u16 = 0x0081; // Timer3 Input Capture High

pub const ETIMSK: u16 = 0x007D; // Extended Timer Interrupt Mask
pub const ETIFR:  u16 = 0x007C; // Extended Timer Interrupt Flags

pub const EICRA:  u16 = 0x006A; // Ext. Interrupt Control A

pub const EEARH:  u16 = 0x003C; // I/O 0x1C EEPROM Address High
pub const EEARL:  u16 = 0x003D; // I/O 0x1D EEPROM Address Low
pub const EEDR:   u16 = 0x003E; // I/O 0x1E EEPROM Data Register
pub const EECR:   u16 = 0x003F; // I/O 0x1F EEPROM Control Register

// helpers
/// io_to_mem in_out 0_3f
#[inline]
pub const fn io_to_mem(a: u8) -> u16 {
    0x0020 + a as u16
}

/// ATmega128A: `IN`/`OUT`/`SBI` I/O addresses (0…0x3F).
pub const IO_NAMES: &[(&str, u8)] = &[
    // status_stack
    ("SREG",   0x3F), ("SPH",    0x3E), ("SPL",    0x3D),

    // port_a
    ("PORTA",  0x1B), ("DDRA",   0x1A), ("PINA",   0x19),
    // port_b
    ("PORTB",  0x18), ("DDRB",   0x17), ("PINB",   0x16),
    // port_c
    ("PORTC",  0x15), ("DDRC",   0x14), ("PINC",   0x13),
    // port_d
    ("PORTD",  0x12), ("DDRD",   0x11), ("PIND",   0x10),
    // port_e
    ("PORTE",  0x03), ("DDRE",   0x02), ("PINE",   0x01),

    // spi
    ("SPDR",   0x0F), ("SPSR",   0x0E), ("SPCR",   0x0D),

    // usart0
    ("UDR0",   0x0C), ("UCSR0A", 0x0B), ("UCSR0B", 0x0A), ("UBRR0L", 0x09),

    // ac_adc
    ("ACSR",   0x08),
    ("ADMUX",  0x07), ("ADCSRA", 0x06), ("ADCH",   0x05), ("ADCL",   0x04),

    // eeprom (standard IO, all accessible via IN/OUT/SBI/SBIC)
    ("EECR",   0x1F),
    ("EEDR",   0x1E),
    ("EEARL",  0x1D),
    ("EEARH",  0x1C),

    // rampz at correct ATmega128A address (I/O 0x3B)
    ("RAMPZ",  0x3B),

    // wdt
    ("WDTCR",  0x21),

    // t0
    ("TCCR0",  0x33), ("TCNT0",  0x32), ("OCR0",   0x31),
    // assr
    ("ASSR",   0x30),
    // t2
    ("TCCR2",  0x25), ("TCNT2",  0x24), ("OCR2",   0x23),
    // t1
    ("TCCR1A", 0x2F), ("TCCR1B", 0x2E),
    ("TCNT1H", 0x2D), ("TCNT1L", 0x2C),
    ("OCR1AH", 0x2B), ("OCR1AL", 0x2A),
    ("OCR1BH", 0x29), ("OCR1BL", 0x28),
    ("ICR1H",  0x27), ("ICR1L",  0x26),

    // mcu
    ("MCUCR",  0x35), ("MCUCSR", 0x34),

    // irq_flags_masks
    ("TIMSK",  0x37), ("TIFR",   0x36),
    ("EIMSK",  0x39), ("EIFR",   0x38),
    ("EICRB",  0x3A),

    // xdiv
    ("XDIV",   0x3C),
];

/// ATmega328P `_SFR_IO8` addresses from avr-libc `iom328p.h` (same encoding as `IN`/`OUT`).
pub const IO_NAMES_328P: &[(&str, u8)] = &[
    ("ACSR", 0x30),
    ("DDRB", 0x04),
    ("DDRC", 0x07),
    ("DDRD", 0x0A),
    ("EEARH", 0x22),
    ("EEARL", 0x21),
    ("EECR", 0x1F),
    ("EEDR", 0x20),
    ("EIFR", 0x1C),
    ("EIMSK", 0x1D),
    ("GPIOR0", 0x1E),
    ("GPIOR1", 0x2A),
    ("GPIOR2", 0x2B),
    ("GTCCR", 0x23),
    ("MCUCR", 0x35),
    ("MCUCSR", 0x34),
    ("MCUSR", 0x34),
    ("OCR0A", 0x27),
    ("OCR0B", 0x28),
    ("PCIFR", 0x1B),
    ("PINB", 0x03),
    ("PINC", 0x06),
    ("PIND", 0x09),
    ("PORTB", 0x05),
    ("PORTC", 0x08),
    ("PORTD", 0x0B),
    ("SPCR", 0x2C),
    ("SPDR", 0x2E),
    ("SPSR", 0x2D),
    ("SMCR", 0x33),
    ("SPMCSR", 0x37),
    ("SPL", 0x3D),
    ("SPH", 0x3E),
    ("SREG", 0x3F),
    ("TCCR0A", 0x24),
    ("TCCR0B", 0x25),
    ("TCNT0", 0x26),
    ("TIFR0", 0x15),
    ("TIFR1", 0x16),
    ("TIFR2", 0x17),
];

#[inline]
pub fn io_names(model: McuModel) -> &'static [(&'static str, u8)] {
    match model {
        McuModel::Atmega128A => IO_NAMES,
        McuModel::Atmega328P => IO_NAMES_328P,
    }
}

/// ATmega328P: data-space addresses (`0x20` + I/O offset) for B/C/D only.
pub const PORTS_328P: [(&str, u16, u16, u16); 3] = [
    ("B", io_to_mem(0x05), io_to_mem(0x04), io_to_mem(0x03)),
    ("C", io_to_mem(0x08), io_to_mem(0x07), io_to_mem(0x06)),
    ("D", io_to_mem(0x0B), io_to_mem(0x0A), io_to_mem(0x09)),
];

/// ports tuple label port_ddr_pin_mem
pub const PORTS: [(&str, u16, u16, u16); 7] = [
    ("A", PORTA, DDRA, PINA),
    ("B", PORTB, DDRB, PINB),
    ("C", PORTC, DDRC, PINC),
    ("D", PORTD, DDRD, PIND),
    ("E", PORTE, DDRE, PINE),
    ("F", PORTF, DDRF, PINF),
    ("G", PORTG, DDRG, PING),
];
