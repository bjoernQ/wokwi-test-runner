use anyhow::Result;
use crate::gdb::GdbClient;
use crate::gdb::regs::RegisterLayout;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch { RiscV32, Xtensa }

#[derive(Debug, Clone, Copy)]
pub struct ArchConfig {
    pub arch: Arch,
    pub regs: RegisterLayout,
}

pub struct Target<'a> {
    gdb: &'a mut GdbClient,
    arch: ArchConfig,
}

impl<'a> Target<'a> {
    pub fn new(gdb: &'a mut GdbClient, arch: ArchConfig) -> Self { Self { gdb, arch } }
    pub fn arch(&self) -> ArchConfig { self.arch }
    /// Read an argument register, following the Xtensa register window.
    pub fn read_arg_reg(&mut self, reg: u32) -> Result<u32> {
        let reg = self.physical_reg(reg)?;
        self.gdb.read_reg(reg)
    }
    fn physical_reg(&mut self, reg: u32) -> Result<u32> {
        match self.arch.regs.window {
            Some(window) => Ok(window.physical(reg, self.gdb.read_reg(window.windowbase_reg)?)),
            None => Ok(reg),
        }
    }
    pub fn read_u32(&mut self, addr: u64) -> Result<u32> {
        let bytes = self.gdb.read_mem(addr, 4)?;
        Ok(u32::from_le_bytes(bytes[..4].try_into().unwrap()))
    }
    pub fn read_mem(&mut self, addr: u64, len: usize) -> Result<Vec<u8>> { self.gdb.read_mem(addr, len) }
    pub fn write_mem(&mut self, addr: u64, data: &[u8]) -> Result<()> { self.gdb.write_mem(addr, data) }
    /// Stage the syscall return value. `finish_semihosting` sends it with the new PC.
    pub fn write_return(&mut self, value: i32) -> Result<()> {
        let reg = self.physical_reg(self.arch.regs.return_reg)?;
        self.gdb.stage_reg(reg, value as u32)
    }
    pub fn read_pc(&mut self) -> Result<u32> { self.gdb.read_pc() }
    pub fn write_pc(&mut self, pc: u32) -> Result<()> {
        if let Some(reg) = self.arch.regs.resume_pc_reg {
            self.gdb.stage_reg(reg, pc)?;
        }
        self.gdb.stage_reg(self.arch.regs.pc_reg, pc)
    }
    /// Stage PC past the trap and send every staged register in one `G` packet.
    pub fn finish_semihosting(&mut self) -> Result<()> {
        let pc = self.read_pc()?;
        self.write_pc(pc + self.arch.regs.trap_skip)?;
        self.gdb.flush_regs()
    }
}
