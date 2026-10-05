use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

/// wokwi-cli reads the coverage from the server during shutdown, which can take a while for large reports.
const GRACEFUL_STOP_TIMEOUT: Duration = Duration::from_secs(30);

pub struct WokwiSession {
    child: Child,
    graceful: bool,
}

impl WokwiSession {
    pub fn start(project_dir: &Path, elf: &Path, gdb_port: u16, wokwi_cli: &str, coverage_file: Option<&Path>) -> Result<Self> {
        let elf = elf.canonicalize().with_context(|| format!("ELF not found: {}", elf.display()))?;
        let mut cmd = Command::new(wokwi_cli);
        cmd.arg(project_dir).arg("--elf").arg(&elf).arg("--gdb-server-port").arg(gdb_port.to_string()).arg("--timeout").arg("0").arg("--quiet");
        // wokwi-cli only writes the coverage file on a clean shutdown, so such sessions are stopped with a signal instead of a kill.
        let graceful = coverage_file.is_some();
        if let Some(path) = coverage_file {
            cmd.arg("--coverage-file").arg(path);
            platform::graceful(&mut cmd);
        }
        // Discard simulator logs. An unread pipe fills up and stalls wokwi-cli; null never blocks.
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
        let child = cmd.spawn().with_context(|| format!("failed to spawn {wokwi_cli}"))?;
        if graceful {
            platform::track(&child);
        }
        Ok(Self { child, graceful })
    }

    pub fn stop(&mut self) {
        if let Ok(Some(_)) = self.child.try_wait() {
            return;
        }
        if self.graceful && platform::request_stop(&self.child) {
            let deadline = Instant::now() + GRACEFUL_STOP_TIMEOUT;
            while Instant::now() < deadline {
                match self.child.try_wait() {
                    Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                    _ => break,
                }
            }
        }
        if self.graceful {
            platform::untrack();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for WokwiSession {
    fn drop(&mut self) { self.stop(); }
}

#[cfg(windows)]
mod platform {
    use std::os::windows::process::CommandExt;
    use std::process::{Child, Command};
    use std::sync::Once;
    use std::sync::atomic::{AtomicU32, Ordering};

    const CREATE_NEW_PROCESS_GROUP: u32 = 0x200;
    const CTRL_C_EVENT: u32 = 0;
    const CTRL_BREAK_EVENT: u32 = 1;

    /// Process group of the running wokwi-cli, 0 if none.
    static GROUP: AtomicU32 = AtomicU32::new(0);
    static HANDLER: Once = Once::new();

    #[link(name = "kernel32")]
    extern "system" {
        fn GenerateConsoleCtrlEvent(event: u32, group: u32) -> i32;
        fn SetConsoleCtrlHandler(handler: Option<unsafe extern "system" fn(u32) -> i32>, add: i32) -> i32;
    }

    /// A new process group ignores Ctrl+C, so forward it as Ctrl+Break; otherwise wokwi-cli outlives the runner.
    unsafe extern "system" fn forward_ctrl_c(event: u32) -> i32 {
        let group = GROUP.load(Ordering::SeqCst);
        if event == CTRL_C_EVENT && group != 0 {
            GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, group);
        }
        0
    }

    /// Ctrl+Break can only be sent to a whole process group, so give wokwi-cli its own.
    pub fn graceful(cmd: &mut Command) {
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
        HANDLER.call_once(|| unsafe {
            SetConsoleCtrlHandler(Some(forward_ctrl_c), 1);
        });
    }

    pub fn track(child: &Child) { GROUP.store(child.id(), Ordering::SeqCst); }

    pub fn untrack() { GROUP.store(0, Ordering::SeqCst); }

    /// wokwi-cli handles Ctrl+Break as SIGBREAK.
    pub fn request_stop(child: &Child) -> bool {
        unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, child.id()) != 0 }
    }
}

#[cfg(unix)]
mod platform {
    use std::process::{Child, Command};

    const SIGTERM: i32 = 15;

    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }

    pub fn graceful(_cmd: &mut Command) {}

    pub fn track(_child: &Child) {}

    pub fn untrack() {}

    pub fn request_stop(child: &Child) -> bool {
        unsafe { kill(child.id() as i32, SIGTERM) == 0 }
    }
}
