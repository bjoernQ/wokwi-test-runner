# wokwi-test-runner

Run [embedded-test](https://github.com/probe-rs/embedded-test) binaries in the Wokwi simulator via GDB Remote Serial Protocol. No `gdb` executable is required.

## Prerequisites

- [wokwi-cli](https://github.com/wokwi/wokwi-cli) on `PATH`
- `WOKWI_CLI_TOKEN` environment variable (from https://wokwi.com/dashboard/ci)
- An ELF built with embedded-test (`.embedded_test` section, protocol v1)

## Usage

```powershell
$env:WOKWI_CLI_TOKEN = "wok_..."
wokwi-test-runner esp32c6 path\to\test.elf
```

With an existing Wokwi project (`wokwi.toml` and `diagram.json`):

```powershell
wokwi-test-runner --project-dir path\to\project esp32c6 path\to\test.elf
```

The chip name is still required with `--project-dir`. It selects the GDB register layout. The diagram in that directory selects the board.

List tests from ELF metadata (no Wokwi token needed):

```powershell
wokwi-test-runner --list esp32c6 path\to\test.elf
```

## Chips

The chip argument selects two things: the dev board placed in a generated `diagram.json`, and the GDB register layout used for semihosting.

| Chip | Architecture | Wokwi board |
| --- | --- | --- |
| `esp32c3` | RISC-V | `board-esp32-c3-devkitm-1` |
| `esp32c5` | RISC-V | `board-esp32-c5-devkitc-1` |
| `esp32c6` | RISC-V | `board-esp32-c6-devkitc-1` |
| `esp32c61` | RISC-V | `board-esp32-c61-devkitc-1` |
| `esp32h2` | RISC-V | `board-esp32-h2-devkitm-1` |
| `esp32p4` | RISC-V | `board-esp32-p4-preview` |
| `esp32s31` | RISC-V | `board-esp32-s31-function-coreboard-1` |
| `esp32` | Xtensa | `board-esp32-devkit-v1` |
| `esp32s2` | Xtensa | `board-esp32-s2-devkitm-1` |
| `esp32s3` | Xtensa | `board-esp32-s3-devkitc-1` |

Wokwi also simulates AVR, STM32, and RP2040. Those are not accepted here. Semihosting needs a per-architecture GDB register map and calling convention, and this runner only implements the ESP RISC-V and ESP Xtensa layouts from [esp-serial-dbg](https://github.com/esp-rs/esp-serial-dbg/tree/main/espdbg/src/gdb).

`esp32c3`, `esp32c6`, `esp32p4`, `esp32`, `esp32s2` and `esp32s3` have been run end to end. Each Xtensa chip has its own GDB register layout. The remaining RISC-V chips reuse the same RISC-V layout, but have not been run here. ESP32-C5 and ESP32-S31 are alpha in Wokwi, and ESP32-P4 is beta.

## Options

- `--gdb-port` (default 3333)
- `--wokwi-cli` (default `wokwi-cli`)
- `--project-dir` use an existing `wokwi.toml` and `diagram.json`
- `--list` list tests from the ELF
- `--timeout-secs` wait for the target when a test sets no timeout (default 60). A suite `default_timeout` or `#[test(timeout = …)]` is used instead. It starts after `SYS_GET_CMDLINE` and counts time the target is running, not time spent answering semihosting.
- `--timing` print per-packet and per-stage timings on stderr
- `--part ID=TYPE[,KEY=VALUE...]` add a part to the generated diagram (repeatable)
- `--connect PIN=PIN[=PIN...]` wire pins together in the generated diagram (repeatable)
- `--show-diagram` print the generated `diagram.json` on stderr

Without `--project-dir`, the runner writes a temporary project. `firmware` and `elf` in `wokwi.toml` both point at the given ELF.

## Extra hardware

`--part` and `--connect` extend the generated `diagram.json`. They cannot be combined with `--project-dir`.

The board's part ID is `esp`. A pin without an `ID:` prefix is a board pin, so `2=3` shorts GPIO2 and GPIO3. A chain such as `2=3=4` adds one wire per neighbouring pair. Part types, pin names and attributes are the ones in the [Wokwi part reference](https://docs.wokwi.com/getting-started/supported-hardware), for example `board-bmp180` or `wokwi-dht22`. Attribute values are passed through as strings.

```powershell
wokwi-test-runner esp32c6 path\to\test.elf `
  --connect 2=3 --connect 22=23 `
  --part bmp=board-bmp180 `
  --connect bmp:VCC=3V3 --connect bmp:GND=GND.1 --connect bmp:SDA=6 --connect bmp:SCL=7
```

## How it works

1. Start `wokwi-cli` with `--gdb-server-port` and `--timeout 0`
2. Connect to GDB RSP (client in `src/gdb/`)
3. Read the test list from the ELF, the same metadata `probe-rs run` uses
4. For each test, start a fresh simulation. Wokwi does not implement `monitor reset halt`
5. When the target halts on the semihosting trap, answer `SYS_GET_CMDLINE` with `run_addr <test>`, serve console I/O, and wait for `SYS_EXIT`

RISC-V semihosting uses `a0`/`a1` and the `slli` / `ebreak` / `srai` sequence. Xtensa semihosting uses `a2`/`a3`. After handling a call, the runner advances the program counter past the trap instruction.

## Cargo runner

```toml
[target.riscv32imac-unknown-none-elf]
runner = "wokwi-test-runner esp32c6"
```

## Known limitations

- Each test boots a new simulation, so the ROM log is paid once per test. That log is discarded. Test output comes from semihosting.
- A test that never calls `SYS_EXIT` fails when `--timeout-secs` elapses.
- Peripherals Wokwi does not model (for example a second TWAI node) fail inside the firmware. The runner reports that panic or timeout.
