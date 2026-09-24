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
    /// Bytes to add to PC after handling a semihosting trap.
    pub trap_skip: u32,
    /// Number of 32-bit registers in the `g`/`G` packet.
    pub reg_count: u32,
}

impl RegisterLayout {
    /// ESP RISC-V (ESP32-C3/C6/H2): a0/a1 + pc at index 32 (33 regs, 264 hex chars).
    pub const RISCV32: Self = Self {
        arg0_reg: 10, // a0
        arg1_reg: 11, // a1
        return_reg: 10,
        pc_reg: 32,
        trap_skip: 4, // skip `ebreak` (4 bytes)
        reg_count: 33,
    };

    /// ESP Xtensa (ESP32/S2/S3): a2/a3 + pc at index 0 (105 regs).
    pub const XTENSA: Self = Self {
        arg0_reg: 3, // a2
        arg1_reg: 4, // a3
        return_reg: 3,
        pc_reg: 0,
        trap_skip: 3, // skip `break` (3 bytes)
        reg_count: 105,
    };

    pub fn reg_width_hex(&self) -> usize {
        8
    }

    pub fn g_packet_len(&self) -> usize {
        self.reg_count as usize * self.reg_width_hex()
    }
}
