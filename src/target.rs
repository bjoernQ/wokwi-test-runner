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

impl ArchConfig {
    pub fn riscv32() -> Self {
        Self { arch: Arch::RiscV32, regs: RegisterLayout::RISCV32 }
    }
    pub fn xtensa() -> Self {
        Self { arch: Arch::Xtensa, regs: RegisterLayout::XTENSA }
    }
}

pub struct Target<'a> {
    gdb: &'a mut GdbClient,
    arch: ArchConfig,
}

impl<'a> Target<'a> {
    pub fn new(gdb: &'a mut GdbClient, arch: ArchConfig) -> Self { Self { gdb, arch } }
    pub fn arch(&self) -> ArchConfig { self.arch }
    pub fn read_reg_raw(&mut self, reg: u32) -> Result<u32> { self.gdb.read_reg(reg) }
    pub fn read_u32(&mut self, addr: u64) -> Result<u32> {
        let bytes = self.gdb.read_mem(addr, 4)?;
        Ok(u32::from_le_bytes(bytes[..4].try_into().unwrap()))
    }
    pub fn read_mem(&mut self, addr: u64, len: usize) -> Result<Vec<u8>> { self.gdb.read_mem(addr, len) }
    pub fn write_mem(&mut self, addr: u64, data: &[u8]) -> Result<()> { self.gdb.write_mem(addr, data) }
    pub fn write_return(&mut self, value: i32) -> Result<()> {
        self.gdb.write_reg(self.arch.regs.return_reg, value as u32)
    }
    pub fn read_pc(&mut self) -> Result<u32> { self.gdb.read_pc() }
    pub fn write_pc(&mut self, pc: u32) -> Result<()> {
        self.gdb.write_reg(self.arch.regs.pc_reg, pc)
    }
    pub fn advance_pc(&mut self, bytes: u32) -> Result<()> {
        let pc = self.read_pc()?;
        self.write_pc(pc + bytes)
    }
}
