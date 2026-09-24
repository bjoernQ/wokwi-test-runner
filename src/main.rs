use std::io::Write;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Parser;

mod chip;
mod elf;
mod file_io;
mod gdb;
mod runner;
mod semihosting;
mod target;
mod trace;
mod wokwi;

use chip::{arch_config, ensure_wokwi_token, lookup};
use elf::parse_elf;
use gdb::{GdbClient, wait_for_port};
use runner::{check_outcome, run_test};
use semihosting::ElfCache;
use wokwi::WokwiSession;

#[derive(Parser)]
#[command(name = "wokwi-test-runner", about = "Run embedded-test binaries in Wokwi via GDB")]
struct Args {
    chip: String,
    elf: PathBuf,
    #[arg(long, default_value = "3333")]
    gdb_port: u16,
    #[arg(long, default_value = "wokwi-cli")]
    wokwi_cli: String,
    #[arg(long)]
    project_dir: Option<PathBuf>,
    #[arg(long)]
    list: bool,
    #[arg(long, default_value = "60")]
    timeout_secs: u64,
    /// Print per-packet and per-stage timings to stderr.
    #[arg(long)]
    timing: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    trace::set_enabled(args.timing);
    let chip = lookup(&args.chip)?;
    let arch = arch_config(chip);
    let embedded = parse_elf(&args.elf)?.context("ELF does not contain embedded-test metadata")?;
    if embedded.version != 1 {
        bail!("embedded-test protocol v0 not supported; rebuild with embedded-test >= 0.7");
    }
    if args.list {
        for test in &embedded.tests {
            let status = if test.ignored { "ignored" } else { "test" };
            println!("{}: {}", status, test.name);
        }
        return Ok(());
    }
    if embedded.entry == 0 {
        bail!("ELF entry point is 0, so the simulator never reaches the test harness. Rebuild with the chip linker script (a working image enters at a 0x42xxxxxx address).");
    }
    ensure_wokwi_token()?;
    let project_dir = prepare_project_dir(&args, chip.board)?;
    let addr: SocketAddr = format!("127.0.0.1:{}", args.gdb_port).parse()?;
    let default_timeout = Duration::from_secs(args.timeout_secs);
    let runnable = embedded.tests.iter().filter(|t| !t.ignored).count();
    eprintln!("\nrunning {runnable} tests\n");
    let mut failed = 0usize;
    let mut elf_cache = ElfCache::default();
    for test in &embedded.tests {
        if test.ignored {
            println!("test {} ... ignored", test.name);
            continue;
        }
        print!("test {} ... ", test.name);
        let _ = std::io::stdout().flush();
        let test_started = std::time::Instant::now();
        // Wokwi has no working monitor-reset, so each test gets a fresh simulation.
        let mut session = WokwiSession::start(&project_dir, &args.elf, args.gdb_port, &args.wokwi_cli)?;
        timing!("timing: spawn {}ms", test_started.elapsed().as_millis());
        let result = (|| {
            let started = std::time::Instant::now();
            wait_for_port(addr, Duration::from_secs(60))?;
            timing!("timing: gdb port {}ms", started.elapsed().as_millis());
            let started = std::time::Instant::now();
            let mut gdb = GdbClient::connect(addr, Duration::from_secs(120), arch.regs)?;
            gdb.handshake()?;
            timing!("timing: handshake {}ms", started.elapsed().as_millis());
            let started = std::time::Instant::now();
            let outcome = run_test(&mut gdb, arch, test, default_timeout, false, &mut elf_cache);
            timing!("timing: run_test {}ms", started.elapsed().as_millis());
            outcome
        })();
        let started = std::time::Instant::now();
        session.stop();
        timing!("timing: stop {}ms  total {}ms", started.elapsed().as_millis(), test_started.elapsed().as_millis());
        match result {
            Ok(outcome) => match check_outcome(test, outcome) {
                Ok(()) => println!("ok"),
                Err(e) => { println!("FAILED"); eprintln!("  {e}"); failed += 1; }
            },
            Err(e) => { println!("FAILED"); eprintln!("  {e}"); failed += 1; }
        }
    }
    eprintln!("\ntest result: {}. {} passed; {} failed; 0 ignored\n", if failed == 0 { "ok" } else { "FAILED" }, runnable - failed, failed);
    if failed > 0 { std::process::exit(1); }
    Ok(())
}

fn prepare_project_dir(args: &Args, board: &str) -> Result<PathBuf> {
    if let Some(dir) = &args.project_dir {
        return Ok(dir.clone());
    }
    let dir = tempfile::tempdir()?;
    let elf = args.elf.canonicalize().with_context(|| format!("ELF not found: {}", args.elf.display()))?;
    std::fs::write(dir.path().join("wokwi.toml"), chip::wokwi_toml(args.gdb_port, &elf))?;
    std::fs::write(dir.path().join("diagram.json"), chip::diagram_json(board))?;
    Ok(dir.keep())
}

