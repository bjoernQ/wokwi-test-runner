use std::path::Path;
use std::process::{Child, Command, Stdio};

use anyhow::{Context, Result};

pub struct WokwiSession {
    child: Child,
}

impl WokwiSession {
    pub fn start(project_dir: &Path, elf: &Path, gdb_port: u16, wokwi_cli: &str) -> Result<Self> {
        let elf = elf.canonicalize().with_context(|| format!("ELF not found: {}", elf.display()))?;
        let mut cmd = Command::new(wokwi_cli);
        cmd.arg(project_dir).arg("--elf").arg(&elf).arg("--gdb-server-port").arg(gdb_port.to_string()).arg("--timeout").arg("0").arg("--quiet");
        // Discard simulator logs. An unread pipe fills up and stalls wokwi-cli; null never blocks.
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
        let child = cmd.spawn().with_context(|| format!("failed to spawn {wokwi_cli}"))?;
        Ok(Self { child })
    }

    pub fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for WokwiSession {
    fn drop(&mut self) { self.stop(); }
}

