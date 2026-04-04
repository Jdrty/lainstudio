//! atmega128a cpu, full isa, throughput_tuned
//!
//! decode_execute: match op>>12 → llvm jump table, o1 per opcode
//! step_n: no step() wrapper, unchecked flash, skip redundant wrapping_add
//! run_timed: 100k_step batches, one Instant::now per batch
//! .

use super::io_map;

pub const FLASH_WORDS: usize = 65_536;
const SRAM_BYTES:  usize = 4_096;
const IO_SIZE:     usize = 0x00E0; // data_mem 0x0020–0x00FF

// sreg_bit_indices
pub const SREG_C: u8 = 0;
pub const SREG_Z: u8 = 1;
pub const SREG_N: u8 = 2;
pub const SREG_V: u8 = 3;
pub const SREG_S: u8 = 4;
pub const SREG_H: u8 = 5;
pub const SREG_T: u8 = 6;
pub const SREG_I: u8 = 7;

// public_types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepResult { Ok, Halted, UnknownOpcode(u16) }

// cpu
#[derive(Clone)]
pub struct Cpu {
    pub flash:  Vec<u16>,
    pub regs:   [u8; 32],
    pub pc:     u32,
    pub sreg:   u8,
    pub sp:     u16,
    pub io:     [u8; IO_SIZE],
    pub sram:   Vec<u8>,
    pub cycles: u64,
    /// cycles_snapshot_when_timers_last_ticked
    timer_last_cycles: u64,
}

impl Default for Cpu { fn default() -> Self { Self::new() } }

impl Cpu {
    pub fn new() -> Self {
        Self {
            flash:  vec![0u16; FLASH_WORDS],
            regs:   [0u8; 32],
            pc:     0,
            sreg:   0,
            sp:     0x10FF,
            io:     [0u8; IO_SIZE],
            sram:   vec![0u8; SRAM_BYTES],
            cycles: 0,
            timer_last_cycles: 0,
        }
    }

    pub fn reset(&mut self) {
        self.regs              = [0u8; 32];
        self.pc                = 0;
        self.sreg              = 0;
        self.sp                = 0x10FF;
        self.io                = [0u8; IO_SIZE];
        self.sram              = vec![0u8; SRAM_BYTES];
        self.cycles            = 0;
        self.timer_last_cycles = 0;
    }

    pub fn load_flash(&mut self, words: &[u16]) {
        let n = words.len().min(FLASH_WORDS);
        self.flash[..n].copy_from_slice(&words[..n]);
        self.flash[n..].fill(0);
    }

    // memory_access

    pub fn read_mem(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x001F => self.regs[addr as usize],
            0x0020..=0x00FF => match addr {
                io_map::SPL  => self.sp as u8,
                io_map::SPH  => (self.sp >> 8) as u8,
                io_map::SREG => self.sreg,
                _            => self.io[(addr - 0x0020) as usize],
            },
            0x0100..=0x10FF => self.sram[(addr - 0x0100) as usize],
            _               => 0,
        }
    }

    pub fn write_mem(&mut self, addr: u16, val: u8) {
        match addr {
            0x0000..=0x001F => self.regs[addr as usize] = val,
            0x0020..=0x00FF => match addr {
                io_map::SPL  => self.sp = (self.sp & 0xFF00) | val as u16,
                io_map::SPH  => self.sp = (self.sp & 0x00FF) | ((val as u16) << 8),
                io_map::SREG => self.sreg = val,
                // tifr_write_1_clears_flag
                io_map::TIFR => self.io[(io_map::TIFR - 0x0020) as usize] &= !val,
                _            => self.io[(addr - 0x0020) as usize] = val,
            },
            0x0100..=0x10FF => self.sram[(addr - 0x0100) as usize] = val,
            _               => {},
        }
    }

    // reg_pair_helpers

    #[inline(always)] fn get_x(&self) -> u16 { u16::from_le_bytes([self.regs[26], self.regs[27]]) }
    #[inline(always)] fn get_y(&self) -> u16 { u16::from_le_bytes([self.regs[28], self.regs[29]]) }
    #[inline(always)] fn get_z(&self) -> u16 { u16::from_le_bytes([self.regs[30], self.regs[31]]) }

    #[inline(always)] fn set_x(&mut self, v: u16) { let b = v.to_le_bytes(); self.regs[26]=b[0]; self.regs[27]=b[1]; }
    #[inline(always)] fn set_y(&mut self, v: u16) { let b = v.to_le_bytes(); self.regs[28]=b[0]; self.regs[29]=b[1]; }
    #[inline(always)] fn set_z(&mut self, v: u16) { let b = v.to_le_bytes(); self.regs[30]=b[0]; self.regs[31]=b[1]; }

    // stack

    #[inline(always)] fn push(&mut self, v: u8) { self.write_mem(self.sp, v); self.sp = self.sp.wrapping_sub(1); }
    #[inline(always)] fn pop(&mut self) -> u8   { self.sp = self.sp.wrapping_add(1); self.read_mem(self.sp) }

    fn push_pc(&mut self, addr: u32) {
        self.push(((addr >> 8) & 0xFF) as u8);
        self.push((addr & 0xFF) as u8);
    }
    fn pop_pc(&mut self) -> u32 {
        let lo = self.pop() as u32;
        let hi = self.pop() as u32;
        (hi << 8) | lo
    }

    // skip_helper

    #[inline(always)]
    fn is_2word(op: u16) -> bool {
        (op & 0xFE0F == 0x9000)
        || (op & 0xFE0F == 0x9200)
        || (op & 0xFE0E == 0x940C)
        || (op & 0xFE0E == 0x940E)
    }

    // sreg_helpers

    #[inline(always)]
    fn set_sreg_bit(&mut self, bit: u8, val: bool) {
        if val { self.sreg |= 1 << bit; } else { self.sreg &= !(1 << bit); }
    }

    /// sreg_after_sub_family keep_z: sbc/cpc never set Z in one step
    #[inline(always)]
    fn sub_flags(&mut self, rd: u8, rr: u8, res: u8, keep_z: bool) {
        let n = (res & 0x80) != 0;
        let v = ((rd ^ rr) & (rd ^ res) & 0x80) != 0;
        self.set_sreg_bit(SREG_C, (rd as u16) < (rr as u16));
        if !keep_z { self.set_sreg_bit(SREG_Z, res == 0); }
        else if res != 0 { self.set_sreg_bit(SREG_Z, false); }
        self.set_sreg_bit(SREG_N, n);
        self.set_sreg_bit(SREG_V, v);
        self.set_sreg_bit(SREG_S, n ^ v);
        self.set_sreg_bit(SREG_H, (rd & 0x0F) < (rr & 0x0F));
    }

    /// sreg_after_logic v_clear nzs_from_res
    #[inline(always)]
    fn logic_flags(&mut self, res: u8) {
        let n = (res & 0x80) != 0;
        self.set_sreg_bit(SREG_Z, res == 0);
        self.set_sreg_bit(SREG_N, n);
        self.set_sreg_bit(SREG_V, false);
        self.set_sreg_bit(SREG_S, n);
    }

    // word_from_flash
    /// fetch_advance_pc safety: pc < flash_words
    #[inline(always)]
    unsafe fn fetch_unchecked(&mut self) -> u16 {
        let w = *self.flash.get_unchecked(self.pc as usize);
        self.pc += 1;
        w
    }

    // public_step
    pub fn step(&mut self) -> StepResult {
        if self.pc as usize >= FLASH_WORDS { return StepResult::Halted; }
        // safety: flash_bounds_ok
        let op = unsafe { *self.flash.get_unchecked(self.pc as usize) };
        self.pc += 1;
        let r = self.decode_execute(op);
        self.tick_timers();
        r
    }

    /// step_n times → (steps_run, last_result)
    pub fn step_n(&mut self, n: u32) -> (u32, StepResult) {
        let mut ran = 0u32;
        while ran < n {
            if self.pc as usize >= FLASH_WORDS {
                return (ran, StepResult::Halted);
            }
            // safety: flash_bounds_ok
            let op = unsafe { *self.flash.get_unchecked(self.pc as usize) };
            self.pc += 1;
            ran += 1;
            let r = self.decode_execute(op);
            if r != StepResult::Ok { return (ran, r); }
        }
        (ran, StepResult::Ok)
    }

    /// run_timed ~millis ms → (total_steps, last_result)
    pub fn run_timed(&mut self, millis: u64) -> (u64, StepResult) {
        let budget = std::time::Duration::from_millis(millis);
        let t0     = std::time::Instant::now();
        let mut total = 0u64;
        loop {
            let (n, r) = self.step_n(100_000);
            self.tick_timers(); // timers_each_100k_batch
            total += n as u64;
            if r != StepResult::Ok { return (total, r); }
            if t0.elapsed() >= budget { return (total, StepResult::Ok); }
        }
    }


    // interrupt_vector_table
    /// ivt_name flash_word_addr or none
    pub fn ivt_name(addr: u32) -> Option<&'static str> {
        match addr {
            0x0000 => Some("RESET"),
            0x0002 => Some("INT0"),
            0x0004 => Some("INT1"),
            0x0006 => Some("INT2"),
            0x0008 => Some("INT3"),
            0x000A => Some("INT4"),
            0x000C => Some("INT5"),
            0x000E => Some("INT6"),
            0x0010 => Some("INT7"),
            0x0012 => Some("TIMER2_COMP"),
            0x0014 => Some("TIMER2_OVF"),
            0x0016 => Some("TIMER1_CAPT"),
            0x0018 => Some("TIMER1_COMPA"),
            0x001A => Some("TIMER1_COMPB"),
            0x001C => Some("TIMER1_OVF"),
            0x001E => Some("TIMER0_COMP"),
            0x0020 => Some("TIMER0_OVF"),
            0x0022 => Some("SPI_STC"),
            0x0024 => Some("USART0_RX"),
            0x0026 => Some("USART0_UDRE"),
            0x0028 => Some("USART0_TX"),
            0x002A => Some("ADC"),
            0x002C => Some("EE_RDY"),
            0x002E => Some("ANA_COMP"),
            0x0030 => Some("TIMER1_COMPC"),
            0x0032 => Some("TIMER3_CAPT"),
            0x0034 => Some("TIMER3_COMPA"),
            0x0036 => Some("TIMER3_COMPB"),
            0x0038 => Some("TIMER3_COMPC"),
            0x003A => Some("TIMER3_OVF"),
            0x003C => Some("USART1_RX"),
            0x003E => Some("USART1_UDRE"),
            0x0040 => Some("USART1_TX"),
            0x0042 => Some("TWI"),
            0x0044 => Some("SPM_RDY"),
            _ => None,
        }
    }


    // hardware_timers
    /// tick_timers: catch_up_cycles since last call (each step + run_timed batches)
    pub fn tick_timers(&mut self) {
        let elapsed = self.cycles.wrapping_sub(self.timer_last_cycles);
        if elapsed == 0 { return; }
        self.timer_last_cycles = self.cycles;
        Self::tick_t0(&mut self.io, elapsed);
        Self::tick_t1(&mut self.io, elapsed);
        Self::tick_t2(&mut self.io, elapsed);
    }

    // data_addr_to_io_index
    #[inline(always)]
    fn ii(addr: u16) -> usize { addr as usize - 0x0020 }

    // timer0_8bit
    // tifr tov0=0 ocf0=1
    // tccr0 cs0 prescale wgm01 ctc
    fn tick_t0(io: &mut [u8; IO_SIZE], elapsed: u64) {
        let tccr0 = io[Self::ii(io_map::TCCR0)];
        let div: u64 = match tccr0 & 0x07 {
            1 => 1, 2 => 8, 3 => 64, 4 => 256, 5 => 1024,
            _ => return, // stop_or_ext_clk
        };
        let ticks = elapsed / div;
        if ticks == 0 { return; }

        let ctc  = (tccr0 & 0x08) != 0; // wgm01 ctc
        let tcnt = io[Self::ii(io_map::TCNT0)] as u64;
        let ocr  = io[Self::ii(io_map::OCR0)]  as u64;

        if ctc {
            // ctc count_to_ocr_match_ocf0
            let period   = (ocr + 1).max(1);
            let new_raw  = tcnt + ticks;
            io[Self::ii(io_map::TCNT0)] = (new_raw % period) as u8;
            if new_raw >= period {
                io[Self::ii(io_map::TIFR)] |= 0x02; // ocf0
            }
        } else {
            // normal_mode wrap_tov0
            let new_raw = tcnt + ticks;
            io[Self::ii(io_map::TCNT0)] = (new_raw % 256) as u8;
            if new_raw >= 256 {
                io[Self::ii(io_map::TIFR)] |= 0x01; // tov0
            }
            // ocf0 tcnt_crossed_ocr
            if (tcnt <= ocr && new_raw > ocr) || new_raw >= 256 {
                io[Self::ii(io_map::TIFR)] |= 0x02; // ocf0
            }
        }
    }

    // timer1_16bit best timer ^_^
    // tifr tov1=2 ocf1b=3 ocf1a=4
    // tccr1b cs1 wgm12 ctc_ocr1a
    fn tick_t1(io: &mut [u8; IO_SIZE], elapsed: u64) {
        let tccr1b = io[Self::ii(io_map::TCCR1B)];
        let div: u64 = match tccr1b & 0x07 {
            1 => 1, 2 => 8, 3 => 64, 4 => 256, 5 => 1024,
            _ => return,
        };
        let ticks = elapsed / div;
        if ticks == 0 { return; }

        let ctc = (tccr1b & 0x08) != 0; // wgm12 ctc_top_ocr1a

        let tcnt = {
            let lo = io[Self::ii(io_map::TCNT1L)] as u64;
            let hi = io[Self::ii(io_map::TCNT1H)] as u64;
            (hi << 8) | lo
        };
        let ocr1a = {
            let lo = io[Self::ii(io_map::OCR1AL)] as u64;
            let hi = io[Self::ii(io_map::OCR1AH)] as u64;
            (hi << 8) | lo
        };
        let ocr1b = {
            let lo = io[Self::ii(io_map::OCR1BL)] as u64;
            let hi = io[Self::ii(io_map::OCR1BH)] as u64;
            (hi << 8) | lo
        };

        if ctc {
            let period  = (ocr1a + 1).max(1);
            let new_raw = tcnt + ticks;
            let new16   = (new_raw % period) as u16;
            io[Self::ii(io_map::TCNT1L)] = new16 as u8;
            io[Self::ii(io_map::TCNT1H)] = (new16 >> 8) as u8;
            if new_raw >= period {
                io[Self::ii(io_map::TIFR)] |= 0x10; // ocf1a
            }
        } else {
            let new_raw = tcnt + ticks;
            let new16   = (new_raw % 65536) as u16;
            io[Self::ii(io_map::TCNT1L)] = new16 as u8;
            io[Self::ii(io_map::TCNT1H)] = (new16 >> 8) as u8;
            if new_raw >= 65536 {
                io[Self::ii(io_map::TIFR)] |= 0x04; // tov1
            }
            if (tcnt <= ocr1a && new_raw > ocr1a) || new_raw >= 65536 {
                io[Self::ii(io_map::TIFR)] |= 0x10; // ocf1a
            }
            if (tcnt <= ocr1b && new_raw > ocr1b) || new_raw >= 65536 {
                io[Self::ii(io_map::TIFR)] |= 0x08; // ocf1b
            }
        }
    }

    // timer2_8bit worst timer ￢_￢
    // tifr tov2=6 ocf2=7
    // prescale 1 8 32 64 128 256 1024
    fn tick_t2(io: &mut [u8; IO_SIZE], elapsed: u64) {
        let tccr2 = io[Self::ii(io_map::TCCR2)];
        let div: u64 = match tccr2 & 0x07 {
            1 => 1, 2 => 8, 3 => 32, 4 => 64, 5 => 128, 6 => 256, 7 => 1024,
            _ => return,
        };
        let ticks = elapsed / div;
        if ticks == 0 { return; }

        let ctc  = (tccr2 & 0x08) != 0; // wgm21 ctc
        let tcnt = io[Self::ii(io_map::TCNT2)] as u64;
        let ocr  = io[Self::ii(io_map::OCR2)]  as u64;

        if ctc {
            let period  = (ocr + 1).max(1);
            let new_raw = tcnt + ticks;
            io[Self::ii(io_map::TCNT2)] = (new_raw % period) as u8;
            if new_raw >= period {
                io[Self::ii(io_map::TIFR)] |= 0x80; // ocf2
            }
        } else {
            let new_raw = tcnt + ticks;
            io[Self::ii(io_map::TCNT2)] = (new_raw % 256) as u8;
            if new_raw >= 256 {
                io[Self::ii(io_map::TIFR)] |= 0x40; // tov2
            }
            if (tcnt <= ocr && new_raw > ocr) || new_raw >= 256 {
                io[Self::ii(io_map::TIFR)] |= 0x80; // ocf2
            }
        }
    }


    // decode_execute op_hi_nibble o1_dispatch

    #[inline(always)]
    fn decode_execute(&mut self, op: u16) -> StepResult {

        // decode_field_macros
        macro_rules! d    { () => { ((op >> 4) & 0x1F) as usize } }
        macro_rules! r    { () => { (((op >> 5) & 0x10) | (op & 0x0F)) as usize } }
        macro_rules! imm8 { () => { (((op >> 4) & 0xF0) | (op & 0x0F)) as u8 } }

        match op >> 12 {

        // 0x0 nop_movw_mul_cpc_sbc_add
        0x0 => {
            match (op >> 10) & 0x3 {
                0 => { // 0x0000_03ff nop_movw_mul_family
                    match (op >> 8) & 0x3 {
                        0 => { self.cycles += 1; } // nop_or_rsrv
                        1 => { // movw
                            let d = (((op >> 4) & 0x0F) * 2) as usize;
                            let r = ((op & 0x0F) * 2) as usize;
                            self.regs[d]   = self.regs[r];
                            self.regs[d+1] = self.regs[r+1];
                            self.cycles += 1;
                        }
                        2 => { // muls
                            let d = (((op >> 4) & 0x0F) + 16) as usize;
                            let r = ((op & 0x0F) + 16) as usize;
                            let res = (self.regs[d] as i8 as i16).wrapping_mul(self.regs[r] as i8 as i16) as u16;
                            self.regs[0] = res as u8; self.regs[1] = (res >> 8) as u8;
                            self.set_sreg_bit(SREG_Z, res == 0);
                            self.set_sreg_bit(SREG_C, (res & 0x8000) != 0);
                            self.cycles += 2;
                        }
                        _ => { // mulsu_fmul_family
                            let d = (((op >> 4) & 0x07) + 16) as usize;
                            let r = ((op & 0x07) + 16) as usize;
                            match (op >> 3) & 0x11 { // bits_7_3
                                0x00 => { // mulsu
                                    let res = (self.regs[d] as i8 as i16).wrapping_mul(self.regs[r] as u16 as i16) as u16;
                                    self.regs[0]=res as u8; self.regs[1]=(res>>8) as u8;
                                    self.set_sreg_bit(SREG_Z, res==0); self.set_sreg_bit(SREG_C,(res&0x8000)!=0);
                                }
                                0x01 => { // fmul
                                    let un = (self.regs[d] as u16)*(self.regs[r] as u16);
                                    let res = un<<1;
                                    self.regs[0]=res as u8; self.regs[1]=(res>>8) as u8;
                                    self.set_sreg_bit(SREG_Z,res==0); self.set_sreg_bit(SREG_C,(un&0x8000)!=0);
                                }
                                0x10 => { // fmuls
                                    let un = (self.regs[d] as i8 as i16).wrapping_mul(self.regs[r] as i8 as i16) as u16;
                                    let res = un<<1;
                                    self.regs[0]=res as u8; self.regs[1]=(res>>8) as u8;
                                    self.set_sreg_bit(SREG_Z,res==0); self.set_sreg_bit(SREG_C,(un&0x8000)!=0);
                                }
                                _ => { // fmulsu
                                    let un = (self.regs[d] as i8 as i32 * self.regs[r] as i32) as u16;
                                    let res = un<<1;
                                    self.regs[0]=res as u8; self.regs[1]=(res>>8) as u8;
                                    self.set_sreg_bit(SREG_Z,res==0); self.set_sreg_bit(SREG_C,(un&0x8000)!=0);
                                }
                            }
                            self.cycles += 2;
                        }
                    }
                }
                1 => { // cpc
                    let (d,r) = (d!(), r!());
                    let c = self.sreg & 1;
                    let rd = self.regs[d]; let rr = self.regs[r].wrapping_add(c);
                    let res = rd.wrapping_sub(rr);
                    self.sub_flags(rd, rr, res, true);
                    self.cycles += 1;
                }
                2 => { // sbc
                    let (d,r) = (d!(), r!());
                    let c = self.sreg & 1;
                    let rd = self.regs[d]; let rr = self.regs[r];
                    let res = rd.wrapping_sub(rr).wrapping_sub(c);
                    self.regs[d] = res;
                    self.sub_flags(rd, rr.wrapping_add(c), res, true);
                    self.cycles += 1;
                }
                _ => { // add
                    let (d,r) = (d!(), r!());
                    let rd = self.regs[d]; let rr = self.regs[r];
                    let wide = rd as u16 + rr as u16;
                    let res = wide as u8;
                    self.regs[d] = res;
                    let n = (res & 0x80) != 0;
                    let v = (!(rd ^ rr) & (rd ^ res) & 0x80) != 0;
                    self.set_sreg_bit(SREG_C, wide > 0xFF);
                    self.set_sreg_bit(SREG_Z, res == 0);
                    self.set_sreg_bit(SREG_N, n);
                    self.set_sreg_bit(SREG_V, v);
                    self.set_sreg_bit(SREG_S, n ^ v);
                    self.set_sreg_bit(SREG_H, (rd & 0x0F) + (rr & 0x0F) > 0x0F);
                    self.cycles += 1;
                }
            }
        }

        // 0x1 cpse_cp_sub_adc
        0x1 => {
            match (op >> 10) & 0x3 {
                0 => { // cpse
                    let (d,r) = (d!(), r!());
                    if self.regs[d] == self.regs[r] {
                        // safety: skip_target_bounds_in_step
                        let next = if (self.pc as usize) < FLASH_WORDS {
                            unsafe { *self.flash.get_unchecked(self.pc as usize) }
                        } else { 0 };
                        let skip = if Self::is_2word(next) { 2 } else { 1 };
                        self.pc += skip;
                        self.cycles += 1 + skip as u64;
                    } else { self.cycles += 1; }
                }
                1 => { // cp
                    let (d,r) = (d!(), r!());
                    let rd=self.regs[d]; let rr=self.regs[r];
                    self.sub_flags(rd, rr, rd.wrapping_sub(rr), false);
                    self.cycles += 1;
                }
                2 => { // sub
                    let (d,r) = (d!(), r!());
                    let rd=self.regs[d]; let rr=self.regs[r];
                    let res = rd.wrapping_sub(rr);
                    self.regs[d] = res;
                    self.sub_flags(rd, rr, res, false);
                    self.cycles += 1;
                }
                _ => { // adc
                    let (d,r) = (d!(), r!());
                    let rd=self.regs[d]; let rr=self.regs[r];
                    let c = (self.sreg & 1) as u16;
                    let wide = rd as u16 + rr as u16 + c;
                    let res = wide as u8;
                    self.regs[d] = res;
                    let n = (res & 0x80) != 0;
                    let v = (!(rd ^ rr) & (rd ^ res) & 0x80) != 0;
                    self.set_sreg_bit(SREG_C, wide > 0xFF);
                    self.set_sreg_bit(SREG_Z, res == 0);
                    self.set_sreg_bit(SREG_N, n);
                    self.set_sreg_bit(SREG_V, v);
                    self.set_sreg_bit(SREG_S, n ^ v);
                    self.set_sreg_bit(SREG_H, (rd & 0x0F) + (rr & 0x0F) + c as u8 > 0x0F);
                    self.cycles += 1;
                }
            }
        }

        // 0x2 and_eor_or_mov
        0x2 => {
            let (d,r) = (d!(), r!());
            match (op >> 10) & 0x3 {
                0 => { let res=self.regs[d]&self.regs[r]; self.regs[d]=res; self.logic_flags(res); } // and
                1 => { let res=self.regs[d]^self.regs[r]; self.regs[d]=res; self.logic_flags(res); } // eor
                2 => { let res=self.regs[d]|self.regs[r]; self.regs[d]=res; self.logic_flags(res); } // or
                _ => { self.regs[d] = self.regs[r]; }                                                // mov
            }
            self.cycles += 1;
        }

        // 0x3 cpi
        0x3 => {
            let d = (((op >> 4) & 0x0F) + 16) as usize;
            let k = imm8!();
            let rd = self.regs[d];
            self.sub_flags(rd, k, rd.wrapping_sub(k), false);
            self.cycles += 1;
        }

        // 0x4 sbci
        0x4 => {
            let d = (((op >> 4) & 0x0F) + 16) as usize;
            let k = imm8!();
            let c = self.sreg & 1;
            let rd = self.regs[d];
            let res = rd.wrapping_sub(k).wrapping_sub(c);
            self.regs[d] = res;
            self.sub_flags(rd, k.wrapping_add(c), res, true);
            self.cycles += 1;
        }

        // 0x5 subi
        0x5 => {
            let d = (((op >> 4) & 0x0F) + 16) as usize;
            let k = imm8!();
            let rd = self.regs[d];
            let res = rd.wrapping_sub(k);
            self.regs[d] = res;
            self.sub_flags(rd, k, res, false);
            self.cycles += 1;
        }

        // 0x6 ori
        0x6 => {
            let d = (((op >> 4) & 0x0F) + 16) as usize;
            let k = imm8!();
            let res = self.regs[d] | k;
            self.regs[d] = res;
            self.logic_flags(res);
            self.cycles += 1;
        }

        // 0x7 andi
        0x7 => {
            let d = (((op >> 4) & 0x0F) + 16) as usize;
            let k = imm8!();
            let res = self.regs[d] & k;
            self.regs[d] = res;
            self.logic_flags(res);
            self.cycles += 1;
        }

        // 0x8_0xa ldd_std_disp q_hi_from_nibble
        0x8 | 0xA => {
            let d_r = ((op >> 4) & 0x1F) as usize;
            let q   = ((op >> 8) & 0x20) | ((op >> 7) & 0x18) | (op & 0x07);
            let base = if op & 0x0008 != 0 { self.get_y() } else { self.get_z() };
            let addr = base.wrapping_add(q);
            if op & 0x0200 == 0 { // load bit9_0
                self.regs[d_r] = self.read_mem(addr);
            } else {              // store bit9_1
                let val = self.regs[d_r];
                self.write_mem(addr, val);
            }
            self.cycles += 2;
        }

        // 0x9 exec_g9_mixed
        0x9 => {
            let r = self.exec_g9(op);
            if r != StepResult::Ok { return r; }
        }

        // 0xb in_out
        0xB => {
            let r_d = ((op >> 4) & 0x1F) as usize;
            let a   = ((op >> 5) & 0x30) | (op & 0x0F);
            let mem = io_map::io_to_mem(a as u8);
            if op & 0x0800 == 0 { // in
                self.regs[r_d] = self.read_mem(mem);
            } else {              // out
                let val = self.regs[r_d];
                self.write_mem(mem, val);
            }
            self.cycles += 1;
        }

        // 0xc rjmp_hot_path
        0xC => {
            // sext12 k via i16 shift_trick
            let k = (((op << 4) as i16) >> 4) as i32;
            self.pc = self.pc.wrapping_add_signed(k);
            self.cycles += 2;
        }

        // 0xd rcall
        0xD => {
            let k = (((op << 4) as i16) >> 4) as i32;
            let ret = self.pc;
            self.push_pc(ret);
            self.pc = self.pc.wrapping_add_signed(k);
            self.cycles += 3;
        }

        // 0xe ldi
        0xE => {
            let d = (((op >> 4) & 0x0F) + 16) as usize;
            self.regs[d] = imm8!();
            self.cycles += 1;
        }

        // 0xf brbs_brbc_bld_bst_sbrc_sbrs
        _ => {
            match (op >> 10) & 0x3 {
                0 => { // brbs
                    let s = op & 7;
                    let k = (((op >> 3) << 9) as i16 >> 9) as i32;
                    if (self.sreg >> s) & 1 != 0 {
                        self.pc = self.pc.wrapping_add_signed(k);
                        self.cycles += 2;
                    } else { self.cycles += 1; }
                }
                1 => { // brbc
                    let s = op & 7;
                    let k = (((op >> 3) << 9) as i16 >> 9) as i32;
                    if (self.sreg >> s) & 1 == 0 {
                        self.pc = self.pc.wrapping_add_signed(k);
                        self.cycles += 2;
                    } else { self.cycles += 1; }
                }
                2 => { // bld_bst
                    let r_d = ((op >> 4) & 0x1F) as usize;
                    let b   = op & 7;
                    if op & 0x0200 == 0 { // bld
                        let t = (self.sreg >> SREG_T) & 1;
                        self.regs[r_d] = (self.regs[r_d] & !(1 << b)) | (t << b) as u8;
                    } else {               // bst
                        let t = (self.regs[r_d] >> b) & 1 != 0;
                        self.set_sreg_bit(SREG_T, t);
                    }
                    self.cycles += 1;
                }
                _ => { // sbrc_sbrs
                    let r = ((op >> 4) & 0x1F) as usize;
                    let b = op & 7;
                    let bit_val = (self.regs[r] >> b) & 1;
                    let want    = if op & 0x0200 == 0 { 0 } else { 1 };
                    if bit_val == want {
                        let next = if (self.pc as usize) < FLASH_WORDS {
                            unsafe { *self.flash.get_unchecked(self.pc as usize) }
                        } else { 0 };
                        let skip = if Self::is_2word(next) { 2 } else { 1 };
                        self.pc += skip;
                        self.cycles += 1 + skip as u64;
                    } else { self.cycles += 1; }
                }
            }
        }

        } // end op_hi_match

        StepResult::Ok
    }

    // exec_g9 second_nibble dispatch o1

    fn exec_g9(&mut self, op: u16) -> StepResult {
        let d = ((op >> 4) & 0x1F) as usize; // ld_st_rd_field
        match (op >> 8) & 0xF {

        // 0x90_91 ld_lds_family
        0x0 | 0x1 => {
            match op & 0x0F {
                0x0 => { // lds_k_2w
                    if self.pc as usize >= FLASH_WORDS { return StepResult::Halted; }
                    let k = unsafe { self.fetch_unchecked() } as u16;
                    self.regs[d] = self.read_mem(k);
                    self.cycles += 2;
                }
                0x1 => { // ld_z_plus
                    let z = self.get_z(); self.regs[d] = self.read_mem(z); self.set_z(z.wrapping_add(1)); self.cycles += 2;
                }
                0x2 => { // ld_minus_z
                    let z = self.get_z().wrapping_sub(1); self.set_z(z); self.regs[d] = self.read_mem(z); self.cycles += 2;
                }
                0x4 => { // lpm_z
                    let z = self.get_z();
                    self.regs[d] = (self.flash[(z>>1) as usize] >> ((z&1)*8)) as u8;
                    self.cycles += 3;
                }
                0x5 => { // lpm_z_plus
                    let z = self.get_z();
                    self.regs[d] = (self.flash[(z>>1) as usize] >> ((z&1)*8)) as u8;
                    self.set_z(z.wrapping_add(1));
                    self.cycles += 3;
                }
                0x6 => { // elpm_z
                    let z = self.get_z();
                    self.regs[d] = (self.flash[(z>>1) as usize] >> ((z&1)*8)) as u8;
                    self.cycles += 3;
                }
                0x7 => { // elpm_z_plus
                    let z = self.get_z();
                    self.regs[d] = (self.flash[(z>>1) as usize] >> ((z&1)*8)) as u8;
                    self.set_z(z.wrapping_add(1));
                    self.cycles += 3;
                }
                0x9 => { // ld_y_plus
                    let y = self.get_y(); self.regs[d] = self.read_mem(y); self.set_y(y.wrapping_add(1)); self.cycles += 2;
                }
                0xA => { // ld_minus_y
                    let y = self.get_y().wrapping_sub(1); self.set_y(y); self.regs[d] = self.read_mem(y); self.cycles += 2;
                }
                0xC => { // ld_x
                    self.regs[d] = self.read_mem(self.get_x()); self.cycles += 2;
                }
                0xD => { // ld_x_plus
                    let x = self.get_x(); self.regs[d] = self.read_mem(x); self.set_x(x.wrapping_add(1)); self.cycles += 2;
                }
                0xE => { // ld_minus_x
                    let x = self.get_x().wrapping_sub(1); self.set_x(x); self.regs[d] = self.read_mem(x); self.cycles += 2;
                }
                0xF => { // pop
                    self.regs[d] = self.pop(); self.cycles += 2;
                }
                _ => return StepResult::UnknownOpcode(op),
            }
        }

        // 0x92_93 st_sts_family
        0x2 | 0x3 => {
            let r = d; // same_rd_field_as_ld
            match op & 0x0F {
                0x0 => { // sts_k_2w
                    if self.pc as usize >= FLASH_WORDS { return StepResult::Halted; }
                    let k = unsafe { self.fetch_unchecked() } as u16;
                    let val = self.regs[r]; self.write_mem(k, val); self.cycles += 2;
                }
                0x1 => { let z=self.get_z(); let v=self.regs[r]; self.write_mem(z,v); self.set_z(z.wrapping_add(1)); self.cycles+=2; } // st_z_plus
                0x2 => { let z=self.get_z().wrapping_sub(1); self.set_z(z); let v=self.regs[r]; self.write_mem(z,v); self.cycles+=2; } // st_minus_z
                0x4..=0x7 => { self.cycles += 2; } // avrxm_xch_stub
                0x9 => { let y=self.get_y(); let v=self.regs[r]; self.write_mem(y,v); self.set_y(y.wrapping_add(1)); self.cycles+=2; } // st_y_plus
                0xA => { let y=self.get_y().wrapping_sub(1); self.set_y(y); let v=self.regs[r]; self.write_mem(y,v); self.cycles+=2; } // st_minus_y
                0xC => { let x=self.get_x(); let v=self.regs[r]; self.write_mem(x,v); self.cycles+=2; } // st_x
                0xD => { let x=self.get_x(); let v=self.regs[r]; self.write_mem(x,v); self.set_x(x.wrapping_add(1)); self.cycles+=2; } // st_x_plus
                0xE => { let x=self.get_x().wrapping_sub(1); self.set_x(x); let v=self.regs[r]; self.write_mem(x,v); self.cycles+=2; } // st_minus_x
                0xF => { let v=self.regs[r]; self.push(v); self.cycles+=2; } // push
                _ => return StepResult::UnknownOpcode(op),
            }
        }

        // 0x94_95 alu_specials
        0x4 | 0x5 => {
            match op {
                // opcode_exact_specials
                0x9409 => { self.pc = self.get_z() as u32; self.cycles += 2; }                                            // ijmp
                0x9419 => { self.pc = self.get_z() as u32; self.cycles += 2; }                                            // eijmp
                0x9509 => { let r=self.pc; self.push_pc(r); self.pc=self.get_z() as u32; self.cycles+=3; }                // icall
                0x9519 => { let r=self.pc; self.push_pc(r); self.pc=self.get_z() as u32; self.cycles+=3; }                // eicall
                0x9508 => { self.pc = self.pop_pc(); self.cycles += 4; }                                                   // ret
                0x9518 => { self.pc = self.pop_pc(); self.set_sreg_bit(SREG_I, true); self.cycles += 4; }                 // reti
                0x95C8 => { let z=self.get_z(); self.regs[0]=(self.flash[(z>>1) as usize]>>((z&1)*8)) as u8; self.cycles+=3; } // lpm_r0
                0x95D8 => { let z=self.get_z(); self.regs[0]=(self.flash[(z>>1) as usize]>>((z&1)*8)) as u8; self.cycles+=3; } // elpm_r0
                0x95E8 | 0x95F8 | 0x9588 | 0x9598 | 0x95A8 => { self.cycles += 1; }                                      // spm_sleep_break_wdr
                _ => {
                    // bset_bclr
                    if op & 0xFF8F == 0x9408 {
                        self.sreg |= 1 << ((op >> 4) & 7);
                        self.cycles += 1;
                    } else if op & 0xFF8F == 0x9488 {
                        self.sreg &= !(1 << ((op >> 4) & 7));
                        self.cycles += 1;
                    // jmp_call_2w
                    } else if op & 0xFE0E == 0x940C {
                        let k = if (self.pc as usize) < FLASH_WORDS {
                            (unsafe { self.fetch_unchecked() }) as u32
                        } else { return StepResult::Halted; };
                        if op & 0x0001 != 0 { // call
                            let ret = self.pc;
                            self.push_pc(ret);
                            self.cycles += 4;
                        } else {              // jmp
                            self.cycles += 3;
                        }
                        self.pc = k;
                    // single_reg_alu
                    } else {
                        match op & 0x0F {
                            0x0 => { // com
                                let res = !self.regs[d];
                                self.regs[d] = res;
                                let n = (res & 0x80) != 0;
                                self.set_sreg_bit(SREG_C, true); self.set_sreg_bit(SREG_Z, res==0);
                                self.set_sreg_bit(SREG_N, n); self.set_sreg_bit(SREG_V, false); self.set_sreg_bit(SREG_S, n);
                            }
                            0x1 => { // neg
                                let rd=self.regs[d]; let res=0u8.wrapping_sub(rd); self.regs[d]=res;
                                let n=(res&0x80)!=0; let v=res==0x80;
                                self.set_sreg_bit(SREG_C,res!=0); self.set_sreg_bit(SREG_Z,res==0);
                                self.set_sreg_bit(SREG_N,n); self.set_sreg_bit(SREG_V,v); self.set_sreg_bit(SREG_S,n^v);
                                self.set_sreg_bit(SREG_H,(res&0x08!=0)|(rd&0x08!=0));
                            }
                            0x2 => { let b=self.regs[d]; self.regs[d]=(b<<4)|(b>>4); } // swap
                            0x3 => { // inc
                                let old=self.regs[d]; let res=old.wrapping_add(1); self.regs[d]=res;
                                let n=(res&0x80)!=0; let v=old==0x7F;
                                self.set_sreg_bit(SREG_Z,res==0); self.set_sreg_bit(SREG_N,n);
                                self.set_sreg_bit(SREG_V,v); self.set_sreg_bit(SREG_S,n^v);
                            }
                            0x5 => { // asr
                                let rd=self.regs[d]; let c=rd&1; let res=((rd as i8)>>1) as u8; self.regs[d]=res;
                                let n=(res&0x80)!=0; let v=n^(c!=0);
                                self.set_sreg_bit(SREG_C,c!=0); self.set_sreg_bit(SREG_Z,res==0);
                                self.set_sreg_bit(SREG_N,n); self.set_sreg_bit(SREG_V,v); self.set_sreg_bit(SREG_S,n^v);
                            }
                            0x6 => { // lsr
                                let rd=self.regs[d]; let c=rd&1; let res=rd>>1; self.regs[d]=res;
                                let v=c!=0;
                                self.set_sreg_bit(SREG_C,c!=0); self.set_sreg_bit(SREG_Z,res==0);
                                self.set_sreg_bit(SREG_N,false); self.set_sreg_bit(SREG_V,v); self.set_sreg_bit(SREG_S,v);
                            }
                            0x7 => { // ror
                                let rd=self.regs[d]; let c_in=(self.sreg&1) as u8; let c_out=rd&1;
                                let res=(c_in<<7)|(rd>>1); self.regs[d]=res;
                                let n=(res&0x80)!=0; let v=n^(c_out!=0);
                                self.set_sreg_bit(SREG_C,c_out!=0); self.set_sreg_bit(SREG_Z,res==0);
                                self.set_sreg_bit(SREG_N,n); self.set_sreg_bit(SREG_V,v); self.set_sreg_bit(SREG_S,n^v);
                            }
                            0xA => { // dec
                                let old=self.regs[d]; let res=old.wrapping_sub(1); self.regs[d]=res;
                                let n=(res&0x80)!=0; let v=old==0x80;
                                self.set_sreg_bit(SREG_Z,res==0); self.set_sreg_bit(SREG_N,n);
                                self.set_sreg_bit(SREG_V,v); self.set_sreg_bit(SREG_S,n^v);
                            }
                            _ => return StepResult::UnknownOpcode(op),
                        }
                        self.cycles += 1;
                    }
                }
            }
        }

        // 0x96 adiw
        0x6 => {
            let dd   = ((op >> 4) & 0x03) as usize;
            let base = 24 + dd * 2;
            let k    = ((op >> 2) & 0x30) | (op & 0x0F);
            let reg16 = u16::from_le_bytes([self.regs[base], self.regs[base+1]]);
            let res32 = reg16 as u32 + k as u32;
            let res16 = res32 as u16;
            self.regs[base]   = res16 as u8;
            self.regs[base+1] = (res16 >> 8) as u8;
            let n = (res16 & 0x8000) != 0;
            let v = (reg16 & 0x8000 == 0) && (res16 & 0x8000 != 0);
            self.set_sreg_bit(SREG_C, res32 > 0xFFFF);
            self.set_sreg_bit(SREG_Z, res16 == 0);
            self.set_sreg_bit(SREG_N, n);
            self.set_sreg_bit(SREG_V, v);
            self.set_sreg_bit(SREG_S, n ^ v);
            self.cycles += 2;
        }

        // 0x97 sbiw
        0x7 => {
            let dd   = ((op >> 4) & 0x03) as usize;
            let base = 24 + dd * 2;
            let k    = ((op >> 2) & 0x30) | (op & 0x0F);
            let reg16 = u16::from_le_bytes([self.regs[base], self.regs[base+1]]);
            let res16 = reg16.wrapping_sub(k as u16);
            self.regs[base]   = res16 as u8;
            self.regs[base+1] = (res16 >> 8) as u8;
            let n = (res16 & 0x8000) != 0;
            let v = (reg16 & 0x8000 != 0) && (res16 & 0x8000 == 0);
            self.set_sreg_bit(SREG_C, reg16 < k as u16);
            self.set_sreg_bit(SREG_Z, res16 == 0);
            self.set_sreg_bit(SREG_N, n);
            self.set_sreg_bit(SREG_V, v);
            self.set_sreg_bit(SREG_S, n ^ v);
            self.cycles += 2;
        }

        // 0x98 cbi
        0x8 => {
            let a=(op>>3)&0x1F; let b=op&7;
            let mem=io_map::io_to_mem(a as u8);
            let v=self.read_mem(mem)&!(1<<b); self.write_mem(mem,v); self.cycles+=2;
        }

        // 0x99 sbic
        0x9 => {
            let a=(op>>3)&0x1F; let b=op&7;
            let mem=io_map::io_to_mem(a as u8);
            if (self.read_mem(mem)>>b)&1==0 {
                let next = if (self.pc as usize)<FLASH_WORDS { unsafe{*self.flash.get_unchecked(self.pc as usize)} } else {0};
                let skip=if Self::is_2word(next){2}else{1};
                self.pc+=skip; self.cycles+=1+skip as u64;
            } else { self.cycles+=1; }
        }

        // 0x9a sbi
        0xA => {
            let a=(op>>3)&0x1F; let b=op&7;
            let mem=io_map::io_to_mem(a as u8);
            let v=self.read_mem(mem)|(1<<b); self.write_mem(mem,v); self.cycles+=2;
        }

        // 0x9b sbis
        0xB => {
            let a=(op>>3)&0x1F; let b=op&7;
            let mem=io_map::io_to_mem(a as u8);
            if (self.read_mem(mem)>>b)&1==1 {
                let next = if (self.pc as usize)<FLASH_WORDS { unsafe{*self.flash.get_unchecked(self.pc as usize)} } else {0};
                let skip=if Self::is_2word(next){2}else{1};
                self.pc+=skip; self.cycles+=1+skip as u64;
            } else { self.cycles+=1; }
        }

        // 0x9c_9f mul
        _ => {
            let d2 = ((op >> 4) & 0x1F) as usize;
            let r2 = (((op >> 5) & 0x10) | (op & 0x0F)) as usize;
            let res = (self.regs[d2] as u16) * (self.regs[r2] as u16);
            self.regs[0] = res as u8;
            self.regs[1] = (res >> 8) as u8;
            self.set_sreg_bit(SREG_Z, res == 0);
            self.set_sreg_bit(SREG_C, (res & 0x8000) != 0);
            self.cycles += 2;
        }

        } // end second_nibble_match

        StepResult::Ok
    }

    // disasm_at

    pub fn disasm_at(&self, addr: u32) -> String {
        if addr as usize >= FLASH_WORDS { return "---".into(); }
        let op   = self.flash[addr as usize];
        let next = if addr as usize + 1 < FLASH_WORDS { self.flash[addr as usize + 1] } else { 0 };

        macro_rules! dr   { () => { (((op>>4)&0x1F), ((op>>5)&0x10)|(op&0x0F)) } }
        macro_rules! immd { () => { (((op>>4)&0x0F)+16, ((op>>4)&0xF0)|(op&0x0F)) } }

        match op {
            0x0000 => return "NOP".into(),
            0x9409 => return "IJMP".into(),
            0x9419 => return "EIJMP".into(),
            0x9509 => return "ICALL".into(),
            0x9519 => return "EICALL".into(),
            0x9508 => return "RET".into(),
            0x9518 => return "RETI".into(),
            0x95C8 => return "LPM".into(),
            0x95D8 => return "ELPM".into(),
            0x95E8 => return "SPM".into(),
            0x95F8 => return "SPM Z+".into(),
            0x9588 => return "SLEEP".into(),
            0x9598 => return "BREAK".into(),
            0x95A8 => return "WDR".into(),
            _ => {}
        }

        if op & 0xFF8F == 0x9408 { let s=(op>>4)&7; let n=["SEC","SEZ","SEN","SEV","SES","SEH","SET","SEI"]; return n[s as usize].into(); }
        if op & 0xFF8F == 0x9488 { let s=(op>>4)&7; let n=["CLC","CLZ","CLN","CLV","CLS","CLH","CLT","CLI"]; return n[s as usize].into(); }

        match op >> 12 {
            0x0 => {
                if op & 0xFF00 == 0x0100 { let d=(op>>4)&0xF; let r=op&0xF; return format!("MOVW R{},{}", d*2, r*2); }
                if op & 0xFF00 == 0x0200 { let (d,r)=dr!(); return format!("MULS R{d},R{r}"); }
                let (d,r) = dr!();
                match (op>>10)&3 {
                    0 => return format!("NOP"),
                    1 => return format!("CPC R{d},R{r}"),
                    2 => return format!("SBC R{d},R{r}"),
                    _ => return if d==r { format!("LSL R{d}") } else { format!("ADD R{d},R{r}") },
                }
            }
            0x1 => {
                let (d,r) = dr!();
                match (op>>10)&3 {
                    0 => return format!("CPSE R{d},R{r}"),
                    1 => return format!("CP R{d},R{r}"),
                    2 => return format!("SUB R{d},R{r}"),
                    _ => return if d==r { format!("ROL R{d}") } else { format!("ADC R{d},R{r}") },
                }
            }
            0x2 => {
                let (d,r) = dr!();
                match (op>>10)&3 {
                    0 => return if d==r { format!("TST R{d}") } else { format!("AND R{d},R{r}") },
                    1 => return if d==r { format!("CLR R{d}") } else { format!("EOR R{d},R{r}") },
                    2 => return format!("OR R{d},R{r}"),
                    _ => return format!("MOV R{d},R{r}"),
                }
            }
            0x3 => { let (d,k)=immd!(); return format!("CPI R{d},0x{k:02X}"); }
            0x4 => { let (d,k)=immd!(); return format!("SBCI R{d},0x{k:02X}"); }
            0x5 => { let (d,k)=immd!(); return format!("SUBI R{d},0x{k:02X}"); }
            0x6 => { let (d,k)=immd!(); return format!("ORI R{d},0x{k:02X}"); }
            0x7 => { let (d,k)=immd!(); return format!("ANDI R{d},0x{k:02X}"); }
            0x8|0xA => {
                let d=(op>>4)&0x1F; let q=((op>>8)&0x20)|((op>>7)&0x18)|(op&7);
                let base=if op&8!=0{"Y"}else{"Z"};
                if op&0x0200==0 { return format!("LDD R{d},{base}+{q}"); }
                else            { return format!("STD {base}+{q},R{d}"); }
            }
            0x9 => {
                let d=(op>>4)&0x1F;
                if op & 0xFE0F == 0x9000 { return format!("LDS R{d},0x{next:04X}"); }
                if op & 0xFE0F == 0x9001 { return format!("LD R{d},Z+"); }
                if op & 0xFE0F == 0x9002 { return format!("LD R{d},-Z"); }
                if op & 0xFE0F == 0x9004 { return format!("LPM R{d},Z"); }
                if op & 0xFE0F == 0x9005 { return format!("LPM R{d},Z+"); }
                if op & 0xFE0F == 0x9009 { return format!("LD R{d},Y+"); }
                if op & 0xFE0F == 0x900A { return format!("LD R{d},-Y"); }
                if op & 0xFE0F == 0x900C { return format!("LD R{d},X"); }
                if op & 0xFE0F == 0x900D { return format!("LD R{d},X+"); }
                if op & 0xFE0F == 0x900E { return format!("LD R{d},-X"); }
                if op & 0xFE0F == 0x900F { return format!("POP R{d}"); }
                if op & 0xFE0F == 0x9200 { return format!("STS 0x{next:04X},R{d}"); }
                if op & 0xFE0F == 0x9201 { return format!("ST Z+,R{d}"); }
                if op & 0xFE0F == 0x9202 { return format!("ST -Z,R{d}"); }
                if op & 0xFE0F == 0x9209 { return format!("ST Y+,R{d}"); }
                if op & 0xFE0F == 0x920A { return format!("ST -Y,R{d}"); }
                if op & 0xFE0F == 0x920C { return format!("ST X,R{d}"); }
                if op & 0xFE0F == 0x920D { return format!("ST X+,R{d}"); }
                if op & 0xFE0F == 0x920E { return format!("ST -X,R{d}"); }
                if op & 0xFE0F == 0x920F { return format!("PUSH R{d}"); }
                if op & 0xFE0F == 0x9400 { return format!("COM R{d}"); }
                if op & 0xFE0F == 0x9401 { return format!("NEG R{d}"); }
                if op & 0xFE0F == 0x9402 { return format!("SWAP R{d}"); }
                if op & 0xFE0F == 0x9403 { return format!("INC R{d}"); }
                if op & 0xFE0F == 0x9405 { return format!("ASR R{d}"); }
                if op & 0xFE0F == 0x9406 { return format!("LSR R{d}"); }
                if op & 0xFE0F == 0x9407 { return format!("ROR R{d}"); }
                if op & 0xFE0F == 0x940A { return format!("DEC R{d}"); }
                if op & 0xFE0E == 0x940C { return format!("JMP 0x{next:04X}"); }
                if op & 0xFE0E == 0x940E { return format!("CALL 0x{next:04X}"); }
                if op & 0xFF00 == 0x9600 { let dd=(op>>4)&3; let b=24+dd*2; let k=((op>>2)&0x30)|(op&0xF); return format!("ADIW R{b},{k}"); }
                if op & 0xFF00 == 0x9700 { let dd=(op>>4)&3; let b=24+dd*2; let k=((op>>2)&0x30)|(op&0xF); return format!("SBIW R{b},{k}"); }
                if op & 0xFF00 == 0x9800 { return format!("CBI 0x{:02X},{}",(op>>3)&0x1F,op&7); }
                if op & 0xFF00 == 0x9900 { return format!("SBIC 0x{:02X},{}",(op>>3)&0x1F,op&7); }
                if op & 0xFF00 == 0x9A00 { return format!("SBI 0x{:02X},{}",(op>>3)&0x1F,op&7); }
                if op & 0xFF00 == 0x9B00 { return format!("SBIS 0x{:02X},{}",(op>>3)&0x1F,op&7); }
                if op & 0xFC00 == 0x9C00 { let (d,r)=dr!(); return format!("MUL R{d},R{r}"); }
            }
            0xB => {
                let r_d=(op>>4)&0x1F; let a=((op>>5)&0x30)|(op&0xF);
                if op&0x0800==0 { return format!("IN R{r_d},0x{a:02X}"); }
                else            { return format!("OUT 0x{a:02X},R{r_d}"); }
            }
            0xC => {
                let k=(((op<<4) as i16)>>4) as i32;
                let t=(addr as i32+1+k) as u32;
                return format!("RJMP 0x{t:04X}");
            }
            0xD => {
                let k=(((op<<4) as i16)>>4) as i32;
                let t=(addr as i32+1+k) as u32;
                return format!("RCALL 0x{t:04X}");
            }
            0xE => { let (d,k)=immd!(); return format!("LDI R{d},0x{k:02X}"); }
            _ => { // op_hi_f
                let s=op&7;
                let k=(((op>>3)<<9) as i16>>9) as i32;
                let t=(addr as i32+1+k) as u32;
                match (op>>10)&3 {
                    0 => { let n=["BRCS","BREQ","BRMI","BRVS","BRLT","BRHS","BRTS","BRIE"]; return format!("{} 0x{t:04X}",n[s as usize]); }
                    1 => { let n=["BRCC","BRNE","BRPL","BRVC","BRGE","BRHC","BRTC","BRID"]; return format!("{} 0x{t:04X}",n[s as usize]); }
                    2 => { let r_d=(op>>4)&0x1F; if op&0x0200==0 { return format!("BLD R{r_d},{s}"); } else { return format!("BST R{r_d},{s}"); } }
                    _ => { let r=(op>>4)&0x1F; if op&0x0200==0 { return format!("SBRC R{r},{s}"); } else { return format!("SBRS R{r},{s}"); } }
                }
            }
        }
        format!("??? 0x{op:04X}")
    }
}

// tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nop() {
        let mut cpu = Cpu::new();
        assert_eq!(cpu.step(), StepResult::Ok);
        assert_eq!(cpu.pc, 1);
        assert_eq!(cpu.cycles, 1);
    }

    #[test]
    fn test_ldi() {
        let mut cpu = Cpu::new();
        cpu.load_flash(&[0xE00A]);
        cpu.step();
        assert_eq!(cpu.regs[16], 0x0A);
    }

    #[test]
    fn test_add() {
        let mut cpu = Cpu::new();
        cpu.load_flash(&[0xE00A, 0xE015, 0x0F01]);
        cpu.step_n(3);
        assert_eq!(cpu.regs[16], 0x0F);
    }

    #[test]
    fn test_add_zero_result() {
        let mut cpu = Cpu::new();
        cpu.load_flash(&[0xE000, 0xE010, 0x0F01]);
        cpu.step_n(3);
        assert_eq!(cpu.regs[16], 0x00);
        assert_ne!(cpu.sreg & (1 << SREG_Z), 0);
    }

    #[test]
    fn test_out_in() {
        let ldi_op: u16 = 0xEA0B;
        let out_op: u16 = 0xB800 | ((0x1Bu16 & 0x30) << 5) | ((16u16 & 0x10) << 4) | ((16u16 & 0x0F) << 4) | (0x1Bu16 & 0x0F);
        let in_op:  u16 = 0xB000 | ((0x1Bu16 & 0x30) << 5) | ((17u16 & 0x10) << 4) | ((17u16 & 0x0F) << 4) | (0x1Bu16 & 0x0F);
        let mut cpu = Cpu::new();
        cpu.load_flash(&[ldi_op, out_op, in_op]);
        cpu.step_n(3);
        assert_eq!(cpu.io[0x1B], 0xAB);
        assert_eq!(cpu.regs[17], 0xAB);
    }

    #[test]
    fn test_lds_sts() {
        let mut cpu = Cpu::new();
        cpu.load_flash(&[0xE402, 0x9300, 0x0100, 0x9110, 0x0100]);
        cpu.step();
        assert_eq!(cpu.regs[16], 0x42);
        cpu.step();
        assert_eq!(cpu.sram[0], 0x42);
        cpu.step();
        assert_eq!(cpu.regs[17], 0x42);
    }

    #[test]
    fn test_rjmp() {
        let rjmp: u16 = 0xC000 | 0xFFF;
        let mut cpu = Cpu::new();
        cpu.load_flash(&[rjmp]);
        cpu.step();
        assert_eq!(cpu.pc, 0);
    }

    #[test]
    fn test_adc_ror() {
        let mut cpu = Cpu::new();
        cpu.load_flash(&[0xEF0F, 0xE011, 0x1F01]);
        cpu.step_n(3);
        assert_eq!(cpu.regs[16], 0x00);
        assert_ne!(cpu.sreg & (1 << SREG_C), 0);
        assert_ne!(cpu.sreg & (1 << SREG_Z), 0);
    }

    #[test]
    fn test_push_pop() {
        let mut cpu = Cpu::new();
        cpu.load_flash(&[0xE505, 0x930F, 0x911F]);
        cpu.step_n(3);
        assert_eq!(cpu.regs[17], 0x55);
    }

    #[test]
    fn test_logic() {
        let mut cpu = Cpu::new();
        cpu.load_flash(&[0xEA0A, 0xE515, 0x2B01]);
        cpu.step_n(3);
        assert_eq!(cpu.regs[16], 0xFF);
        assert_eq!(cpu.sreg & (1 << SREG_V), 0);
    }

    #[test]
    fn test_rcall_ret() {
        let mut cpu = Cpu::new();
        cpu.load_flash(&[0xD001, 0x0000, 0xE402, 0x9508]);
        cpu.step();
        assert_eq!(cpu.pc, 2);
        cpu.step();
        assert_eq!(cpu.regs[16], 0x42);
        cpu.step();
        assert_eq!(cpu.pc, 1);
    }
}
