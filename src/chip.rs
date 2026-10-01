use anyhow::{Result, bail};
use crate::target::{Arch, ArchConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChipSpec { pub name: &'static str, pub board: &'static str, pub arch: Arch }

const CHIPS: &[ChipSpec] = &[
    ChipSpec { name: "esp32c3", board: "board-esp32-c3-devkitm-1", arch: Arch::RiscV32 },
    ChipSpec { name: "esp32c5", board: "board-esp32-c5-devkitc-1", arch: Arch::RiscV32 },
    ChipSpec { name: "esp32c6", board: "board-esp32-c6-devkitc-1", arch: Arch::RiscV32 },
    ChipSpec { name: "esp32c61", board: "board-esp32-c61-devkitc-1", arch: Arch::RiscV32 },
    ChipSpec { name: "esp32h2", board: "board-esp32-h2-devkitm-1", arch: Arch::RiscV32 },
    ChipSpec { name: "esp32p4", board: "board-esp32-p4-preview", arch: Arch::RiscV32 },
    ChipSpec { name: "esp32s31", board: "board-esp32-s31-function-coreboard-1", arch: Arch::RiscV32 },
    ChipSpec { name: "esp32", board: "board-esp32-devkit-v1", arch: Arch::Xtensa },
    ChipSpec { name: "esp32s2", board: "board-esp32-s2-devkitm-1", arch: Arch::Xtensa },
    ChipSpec { name: "esp32s3", board: "board-esp32-s3-devkitc-1", arch: Arch::Xtensa },
];

pub fn lookup(chip: &str) -> Result<ChipSpec> {
    let chip = chip.to_ascii_lowercase();
    CHIPS.iter().find(|c| c.name == chip).copied().ok_or_else(|| {
        let supported = CHIPS.iter().map(|c| c.name).collect::<Vec<_>>().join(", ");
        anyhow::anyhow!("unsupported chip '{chip}', supported: {supported}")
    })
}

pub fn arch_config(spec: ChipSpec) -> ArchConfig {
    match spec.arch { Arch::RiscV32 => ArchConfig::riscv32(), Arch::Xtensa => ArchConfig::xtensa() }
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
