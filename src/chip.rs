use anyhow::{Result, bail};
use crate::gdb::regs::RegisterLayout;
use crate::target::{Arch, ArchConfig};

#[derive(Debug, Clone, Copy)]
pub struct ChipSpec { pub name: &'static str, pub board: &'static str, pub arch: Arch, pub regs: RegisterLayout }

const fn riscv(name: &'static str, board: &'static str) -> ChipSpec {
    ChipSpec { name, board, arch: Arch::RiscV32, regs: RegisterLayout::RISCV32 }
}

const fn xtensa(name: &'static str, board: &'static str, regs: RegisterLayout) -> ChipSpec {
    ChipSpec { name, board, arch: Arch::Xtensa, regs }
}

const CHIPS: &[ChipSpec] = &[
    riscv("esp32c3", "board-esp32-c3-devkitm-1"),
    riscv("esp32c5", "board-esp32-c5-devkitc-1"),
    riscv("esp32c6", "board-esp32-c6-devkitc-1"),
    riscv("esp32c61", "board-esp32-c61-devkitc-1"),
    riscv("esp32h2", "board-esp32-h2-devkitm-1"),
    riscv("esp32p4", "board-esp32-p4-preview"),
    riscv("esp32s31", "board-esp32-s31-function-coreboard-1"),
    xtensa("esp32", "board-esp32-devkit-v1", RegisterLayout::XTENSA),
    xtensa("esp32s2", "board-esp32-s2-devkitm-1", RegisterLayout::XTENSA_S2),
    xtensa("esp32s3", "board-esp32-s3-devkitc-1", RegisterLayout::XTENSA_S3),
];

pub fn lookup(chip: &str) -> Result<ChipSpec> {
    let chip = chip.to_ascii_lowercase();
    CHIPS.iter().find(|c| c.name == chip).copied().ok_or_else(|| {
        let supported = CHIPS.iter().map(|c| c.name).collect::<Vec<_>>().join(", ");
        anyhow::anyhow!("unsupported chip '{chip}', supported: {supported}")
    })
}

pub fn arch_config(spec: ChipSpec) -> ArchConfig {
    ArchConfig { arch: spec.arch, regs: spec.regs }
}

pub fn wokwi_toml(gdb_port: u16, elf: &std::path::Path) -> String {
    let elf = elf.to_string_lossy().replace('\\', "/");
    format!("[wokwi]\nversion = 1\nfirmware = \"{elf}\"\nelf = \"{elf}\"\ngdbServerPort = {gdb_port}\n")
}

pub fn ensure_wokwi_token() -> Result<()> {
    match std::env::var("WOKWI_CLI_TOKEN") {
        Ok(token) if token.starts_with("wok_") && token.len() == 44 => Ok(()),
        Ok(_) => bail!("WOKWI_CLI_TOKEN is set but does not look valid"),
        Err(_) => bail!("WOKWI_CLI_TOKEN environment variable is not set"),
    }
}
