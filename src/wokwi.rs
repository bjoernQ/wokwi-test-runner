use std::io::Read;
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
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().with_context(|| format!("failed to spawn {wokwi_cli}"))?;
        // Drain both pipes. Leaving them unread fills the buffer and stalls the simulator.
        drain(child.stdout.take());
        drain(child.stderr.take());
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

fn drain(pipe: Option<impl Read + Send + 'static>) {
    let Some(mut pipe) = pipe else { return };
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while pipe.read(&mut buf).ok().is_some_and(|n| n > 0) {}
    });
}

