use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use crate::timing;
use crate::elf::{TestCase, TestOutcome};
use crate::file_io::SemihostingFiles;
use crate::gdb::GdbClient;
use crate::gdb::rsp::StopReply;
use crate::semihosting::{
    ElfCache, SemihostingCommand, decode_semihosting, handle_errno, handle_time,
    write_command_line,
};
use crate::target::{ArchConfig, Target};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome { Pass, Panic }

pub fn run_test(
    gdb: &mut GdbClient,
    arch: ArchConfig,
    test: &TestCase,
    default_timeout: Duration,
    reset_before: bool,
    cache: &mut ElfCache,
) -> Result<RunOutcome> {
    // `timeout` on the test is the suite default, or a per-test override. `--timeout-secs` applies only when neither is set.
    let timeout = test.timeout.map(|secs| Duration::from_secs(secs as u64)).unwrap_or(default_timeout);
    // Boot and the first SYS_GET_CMDLINE are not part of the test's own timeout.
    let boot_timeout = default_timeout.max(Duration::from_secs(60));
    if reset_before {
        gdb.reset_halt()?;
    }
    let mut running_left = boot_timeout;
    let mut cmdline_sent = false;
    let mut files = SemihostingFiles::new();
    let mut stop = continue_until(gdb, &mut running_left, timeout)?;

    loop {
        match stop {
            StopReply::FileIo(pkt) => {
                eprintln!("debug: F packet: {pkt}");
                stop = gdb.file_io_reply("F-1")?;
                continue;
            }
            StopReply::Halted => {
                let decode_started = Instant::now();
                let cmd = {
                    let mut target = Target::new(gdb, arch);
                    match decode_semihosting(&mut target, cache) {
                        Ok(c) => c,
                        Err(e) => {
                            timing!("timing: not semihosting after {}ms: {e}", decode_started.elapsed().as_millis());
                            stop = continue_until(gdb, &mut running_left, timeout)?;
                            continue;
                        }
                    }
                };
                timing!("timing: decoded {} in {}ms", cmd.label(), decode_started.elapsed().as_millis());
                match cmd {
                    SemihostingCommand::GetCommandLine { block_address } if !cmdline_sent => {
                        let cmdline = if let Some(addr) = test.address {
                            format!("run_addr {addr}")
                        } else {
                            format!("run {}", test.name)
                        };
                        {
                            let mut target = Target::new(gdb, arch);
                            write_command_line(&mut target, cache, block_address, &cmdline)?;
                            target.finish_semihosting()?;
                        }
                        cmdline_sent = true;
                        // Host time spent answering semihosting is not part of the test timeout.
                        running_left = timeout;
                    }
                    SemihostingCommand::ExitSuccess if cmdline_sent => return Ok(RunOutcome::Pass),
                    SemihostingCommand::ExitError { .. } if cmdline_sent => return Ok(RunOutcome::Panic),
                    SemihostingCommand::Open { .. } | SemihostingCommand::Close { .. }
                    | SemihostingCommand::Write { .. } | SemihostingCommand::WriteConsole { .. } => {
                        let mut target = Target::new(gdb, arch);
                        files.handle(&mut target, cmd.clone())?;
                        target.finish_semihosting()?;
                    }
                    SemihostingCommand::Time => {
                        let mut target = Target::new(gdb, arch);
                        handle_time(&mut target)?;
                        target.finish_semihosting()?;
                    }
                    SemihostingCommand::Errno => {
                        let mut target = Target::new(gdb, arch);
                        handle_errno(&mut target)?;
                        target.finish_semihosting()?;
                    }
                    SemihostingCommand::Unknown { operation, parameter } => {
                        eprintln!("warning: unhandled semihosting op={operation:#x} param={parameter:#x}");
                        let mut target = Target::new(gdb, arch);
                        target.write_return(-1)?;
                        target.finish_semihosting()?;
                    }
                    other => bail!("unexpected semihosting {:?} (cmdline_sent={cmdline_sent})", other),
                }
            }
        }
        stop = continue_until(gdb, &mut running_left, timeout)?;
    }
}

fn continue_until(gdb: &mut GdbClient, running_left: &mut Duration, timeout: Duration) -> Result<StopReply> {
    if running_left.is_zero() {
        bail!("test timed out after {:?}", timeout);
    }
    gdb.set_io_timeout(*running_left)?;
    let started = Instant::now();
    match gdb.continue_run() {
        Ok(stop) => {
            let elapsed = started.elapsed();
            timing!("timing: continue {}ms until halt", elapsed.as_millis());
            *running_left = running_left.saturating_sub(elapsed);
            Ok(stop)
        }
        Err(err) if is_timeout(&err) => bail!("test timed out after {:?}", timeout),
        Err(err) => Err(err),
    }
}

fn is_timeout(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause.downcast_ref::<std::io::Error>().is_some_and(|io| io.kind() == std::io::ErrorKind::TimedOut)
    })
}

pub fn check_outcome(test: &TestCase, outcome: RunOutcome) -> Result<()> {
    match (test.expected_outcome, outcome) {
        (TestOutcome::Pass, RunOutcome::Pass) => Ok(()),
        (TestOutcome::Panic, RunOutcome::Panic) => Ok(()),
        (TestOutcome::Pass, RunOutcome::Panic) => bail!("test should pass but panicked"),
        (TestOutcome::Panic, RunOutcome::Pass) => bail!("test should panic but passed"),
    }
}
