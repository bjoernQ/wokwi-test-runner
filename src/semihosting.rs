use std::collections::{HashMap, HashSet};

use anyhow::{Result, bail};
use crate::gdb::rsp::is_semihosting_op;
use crate::target::{Arch, Target};

const RISCV_EBREAK: u32 = 0x0010_0073;
const RISCV_SEMI_PRE: u32 = 0x01f0_1013;  // slli zero, zero, 0x1f
const RISCV_SEMI_POST: u32 = 0x4070_5013; // srai zero, zero, 0x7

const SYS_OPEN: u32 = 0x01;
const SYS_CLOSE: u32 = 0x02;
const SYS_WRITEC: u32 = 0x03;
const SYS_WRITE0: u32 = 0x04;
const SYS_WRITE: u32 = 0x05;
const SYS_GET_CMDLINE: u32 = 0x15;
const SYS_EXIT: u32 = 0x18;
const SYS_EXIT_EXTENDED: u32 = 0x20;
const SYS_EXIT_ADP_STOPPED_APPLICATIONEXIT: u32 = 0x20026;
const SYS_TIME: u32 = 0x11;
const SYS_ERRNO: u32 = 0x13;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenMode {
    Read,
    ReadBinary,
    ReadUpdate,
    ReadUpdateBinary,
    Write,
    WriteBinary,
    WriteUpdate,
    WriteUpdateBinary,
    Append,
    AppendBinary,
    AppendUpdate,
    AppendUpdateBinary,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemihostingCommand {
    ExitSuccess,
    ExitError { exit_status: Option<u32> },
    GetCommandLine { block_address: u32 },
    Open { path: String, mode: OpenMode },
    Close { handle: u32 },
    Write { handle: u32, data: Vec<u8> },
    WriteConsole { text: String },
    Time,
    Errno,
    Unknown { operation: u32, parameter: u32 },
}

impl SemihostingCommand {
    pub fn label(&self) -> &'static str {
        match self {
            SemihostingCommand::ExitSuccess => "SYS_EXIT ok",
            SemihostingCommand::ExitError { .. } => "SYS_EXIT error",
            SemihostingCommand::GetCommandLine { .. } => "SYS_GET_CMDLINE",
            SemihostingCommand::Open { .. } => "SYS_OPEN",
            SemihostingCommand::Close { .. } => "SYS_CLOSE",
            SemihostingCommand::Write { .. } => "SYS_WRITE",
            SemihostingCommand::WriteConsole { .. } => "SYS_WRITEC/0",
            SemihostingCommand::Time => "SYS_TIME",
            SemihostingCommand::Errno => "SYS_ERRNO",
            SemihostingCommand::Unknown { .. } => "SYS_UNKNOWN",
        }
    }
}

/// Stable for one ELF: trap PCs and the SYS_GET_CMDLINE buffer, which the linker places at fixed addresses.
#[derive(Default)]
pub struct ElfCache {
    traps: HashSet<u32>,
    cmdline: HashMap<u32, (u32, u32)>,
}

pub fn decode_semihosting(target: &mut Target, cache: &mut ElfCache) -> Result<SemihostingCommand> {
    let (operation, parameter) = find_semihosting_args(target, cache)?;
    decode_args(target, operation, parameter)
}

fn find_semihosting_args(target: &mut Target, cache: &mut ElfCache) -> Result<(u32, u32)> {
    let arch = target.arch();
    let operation = target.read_reg_raw(arch.regs.arg0_reg)?;
    let parameter = target.read_reg_raw(arch.regs.arg1_reg)?;
    if !is_semihosting_op(operation) {
        let pc = target.read_pc().unwrap_or(0);
        bail!(
            "halt at {pc:#x} is not semihosting (op={operation:#x}, param={parameter:#x})"
        );
    }
    snap_pc_to_trap(target, cache)?;
    Ok((operation, parameter))
}

/// If PC is on the hint or the trap, move it onto the trap so `PC += skip` lands past it.
fn snap_pc_to_trap(target: &mut Target, cache: &mut ElfCache) -> Result<()> {
    if target.arch().arch != Arch::RiscV32 {
        return Ok(());
    }
    let pc = target.read_pc()?;
    if cache.traps.contains(&pc) {
        return Ok(());
    }
    if cache.traps.contains(&pc.wrapping_sub(4)) {
        target.write_pc(pc.wrapping_sub(4))?;
        return Ok(());
    }
    for delta in [0i32, -4, 4, -2, 2] {
        let addr = pc.wrapping_add(delta as u32);
        if target.read_u32(addr as u64).ok() == Some(RISCV_EBREAK) {
            let pre = target.read_u32(addr.wrapping_sub(4) as u64).unwrap_or(0);
            let post = target.read_u32(addr.wrapping_add(4) as u64).unwrap_or(0);
            if pre == RISCV_SEMI_PRE && post == RISCV_SEMI_POST {
                cache.traps.insert(addr);
                if addr != pc {
                    target.write_pc(addr)?;
                }
            }
            return Ok(());
        }
    }
    Ok(())
}

fn decode_args(target: &mut Target, operation: u32, parameter: u32) -> Result<SemihostingCommand> {
    Ok(match operation {
        SYS_EXIT if parameter == SYS_EXIT_ADP_STOPPED_APPLICATIONEXIT => SemihostingCommand::ExitSuccess,
        SYS_EXIT => SemihostingCommand::ExitError { exit_status: Some(parameter) },
        SYS_EXIT_EXTENDED => {
            let bytes = target.read_mem(parameter as u64, 8)?;
            let reason = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
            let subcode = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
            match (reason, subcode) {
                (SYS_EXIT_ADP_STOPPED_APPLICATIONEXIT, 0) => SemihostingCommand::ExitSuccess,
                (SYS_EXIT_ADP_STOPPED_APPLICATIONEXIT, exit_status) => {
                    SemihostingCommand::ExitError { exit_status: Some(exit_status) }
                }
                _ => SemihostingCommand::ExitError { exit_status: Some(subcode) },
            }
        }
        SYS_GET_CMDLINE => SemihostingCommand::GetCommandLine { block_address: parameter },
        SYS_OPEN => {
            let params = read_params(target, parameter, 3)?;
            let path = read_cstring(target, params[0], Some(params[2]))?;
            SemihostingCommand::Open { path, mode: decode_open_mode(params[1]) }
        }
        SYS_CLOSE => {
            let params = read_params(target, parameter, 1)?;
            SemihostingCommand::Close { handle: params[0] }
        }
        SYS_WRITEC => {
            let text = read_cstring(target, parameter, Some(1))?;
            SemihostingCommand::WriteConsole { text }
        }
        SYS_WRITE0 => {
            let text = read_cstring(target, parameter, None)?;
            SemihostingCommand::WriteConsole { text }
        }
        SYS_WRITE => {
            let params = read_params(target, parameter, 3)?;
            let data = target.read_mem(params[1] as u64, params[2] as usize)?;
            SemihostingCommand::Write { handle: params[0], data }
        }
        SYS_TIME if parameter == 0 => SemihostingCommand::Time,
        SYS_ERRNO if parameter == 0 => SemihostingCommand::Errno,
        _ => SemihostingCommand::Unknown { operation, parameter },
    })
}

fn read_params(target: &mut Target, pointer: u32, count: usize) -> Result<Vec<u32>> {
    let bytes = target.read_mem(pointer as u64, count * 4)?;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let start = i * 4;
        out.push(u32::from_le_bytes(bytes[start..start + 4].try_into().unwrap()));
    }
    Ok(out)
}

fn read_cstring(target: &mut Target, address: u32, max_len: Option<u32>) -> Result<String> {
    if let Some(len) = max_len {
        let bytes = target.read_mem(address as u64, len as usize)?;
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        return Ok(String::from_utf8_lossy(&bytes[..end]).into_owned());
    }
    let mut bytes = Vec::new();
    let mut addr = address as u64;
    loop {
        let chunk = target.read_mem(addr, 128)?;
        if let Some(end) = chunk.iter().position(|&b| b == 0) {
            bytes.extend_from_slice(&chunk[..end]);
            break;
        }
        bytes.extend_from_slice(&chunk);
        addr += chunk.len() as u64;
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn decode_open_mode(mode: u32) -> OpenMode {
    match mode {
        0 => OpenMode::Read,
        1 => OpenMode::ReadBinary,
        2 => OpenMode::ReadUpdate,
        3 => OpenMode::ReadUpdateBinary,
        4 => OpenMode::Write,
        5 => OpenMode::WriteBinary,
        6 => OpenMode::WriteUpdate,
        7 => OpenMode::WriteUpdateBinary,
        8 => OpenMode::Append,
        9 => OpenMode::AppendBinary,
        10 => OpenMode::AppendUpdate,
        11 => OpenMode::AppendUpdateBinary,
        _ => OpenMode::Unknown,
    }
}

pub fn write_command_line(target: &mut Target, cache: &mut ElfCache, block_address: u32, cmdline: &str) -> Result<()> {
    let mut buf = cmdline.as_bytes().to_vec();
    buf.push(0);
    let (ptr, len) = if let Some(cached) = cache.cmdline.get(&block_address) {
        *cached
    } else {
        let ptr = target.read_u32(block_address as u64)?;
        let len = target.read_u32(block_address as u64 + 4)?;
        cache.cmdline.insert(block_address, (ptr, len));
        (ptr, len)
    };
    if buf.len() > len as usize {
        bail!("cmdline buffer too small: need {}, have {}", buf.len(), len);
    }
    target.write_mem(ptr as u64, &buf)?;
    // SYS_GET_CMDLINE: second word becomes the string length, excluding the trailing NUL.
    let reported = (buf.len() - 1) as u32;
    let mut block = [0u8; 8];
    block[0..4].copy_from_slice(&ptr.to_le_bytes());
    block[4..8].copy_from_slice(&reported.to_le_bytes());
    target.write_mem(block_address as u64, &block)?;
    target.write_return(0)?;
    Ok(())
}

pub fn handle_time(target: &mut Target) -> Result<()> {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs() as u32;
    target.write_return(secs as i32)?;
    Ok(())
}

pub fn handle_errno(target: &mut Target) -> Result<()> {
    target.write_return(0)?;
    Ok(())
}

/// Advance PC past the semihosting trap instruction (per RISC-V / Xtensa semihosting specs).
pub fn finish_semihosting(target: &mut Target, _cmd: &SemihostingCommand) -> Result<()> {
    let skip = target.arch().regs.trap_skip;
    target.advance_pc(skip)?;
    Ok(())
}
