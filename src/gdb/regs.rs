//! GDB register layouts for ESP chips.
//!
//! GDB indices from [esp-serial-dbg](https://github.com/esp-rs/esp-serial-dbg/tree/main/espdbg/src/gdb).
//! Semihosting calling convention from the RISC-V / Xtensa semihosting specs:
//! - RISC-V: op in a0, arg block pointer in a1, return in a0; advance PC past `ebreak`.
//! - Xtensa: op in a2, arg in a3, return in a2; advance PC past `break`.

#[derive(Debug, Clone, Copy)]
pub struct RegisterLayout {
    /// GDB index of the semihosting operation register.
    pub arg0_reg: u32,
    /// GDB index of the semihosting parameter register.
    pub arg1_reg: u32,
    /// GDB index where the host writes the syscall return value.
    pub return_reg: u32,
    /// Program counter register index in the `g` packet.
    pub pc_reg: u32,
    /// Second register that must hold the new PC, because the simulator resumes from it
    /// and ignores writes to `pc_reg`.
    pub resume_pc_reg: Option<u32>,
    /// Bytes to add to PC after handling a semihosting trap.
    pub trap_skip: u32,
    /// Number of 32-bit registers in the `g`/`G` packet.
    pub reg_count: u32,
    /// Xtensa windowed register file. When set, `arg0_reg`/`arg1_reg`/`return_reg` index
    /// the window at WINDOWBASE 0 and must be rotated by the current WINDOWBASE.
    pub window: Option<RegisterWindow>,
}

/// The `g` packet holds the physical `ar0..ar63`; logical `aN` is `ar[(WINDOWBASE * 4 + N) % 64]`.
#[derive(Debug, Clone, Copy)]
pub struct RegisterWindow {
    /// GDB index of `ar0`.
    pub ar0_reg: u32,
    /// Number of physical address registers.
    pub ar_count: u32,
    /// GDB index of WINDOWBASE.
    pub windowbase_reg: u32,
}

impl RegisterWindow {
    /// GDB index of the physical register backing `reg` (an index at WINDOWBASE 0).
    pub fn physical(&self, reg: u32, windowbase: u32) -> u32 {
        let logical = reg - self.ar0_reg;
        self.ar0_reg + (windowbase * 4 + logical) % self.ar_count
    }
}

impl RegisterLayout {
    /// ESP RISC-V (ESP32-C3/C6/H2): a0/a1 + pc at index 32 (33 regs, 264 hex chars).
    pub const RISCV32: Self = Self {
        arg0_reg: 10, // a0
        arg1_reg: 11, // a1
        return_reg: 10,
        pc_reg: 32,
        resume_pc_reg: None,
        trap_skip: 4, // skip `ebreak` (4 bytes)
        reg_count: 33,
        window: None,
    };

    /// ESP32: pc at index 0, ar0..ar63 at 1..=64, WINDOWBASE at 69 (105 regs).
    pub const XTENSA: Self = Self {
        arg0_reg: 3, // a2
        arg1_reg: 4, // a3
        return_reg: 3,
        pc_reg: 0,
        resume_pc_reg: None,
        trap_skip: 3, // skip `break` (3 bytes)
        reg_count: 105,
        window: Some(RegisterWindow { ar0_reg: 1, ar_count: 64, windowbase_reg: 69 }),
    };

    /// ESP32-S3: same order as ESP32 through WINDOWBASE at 69, but 102 regs. Wokwi resumes
    /// from index 83 and ignores a PC written at index 0.
    pub const XTENSA_S3: Self = Self {
        reg_count: 102,
        resume_pc_reg: Some(83),
        ..Self::XTENSA
    };

    /// ESP32-S2: no loop, MAC16 or FPU registers, so WINDOWBASE follows SAR at 66 (72 regs).
    pub const XTENSA_S2: Self = Self {
        reg_count: 72,
        window: Some(RegisterWindow { ar0_reg: 1, ar_count: 64, windowbase_reg: 66 }),
        ..Self::XTENSA
    };

    pub fn reg_width_hex(&self) -> usize {
        8
    }

    pub fn g_packet_len(&self) -> usize {
        self.reg_count as usize * self.reg_width_hex()
    }
}
