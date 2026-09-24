pub mod regs;
pub mod rsp;

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use regs::RegisterLayout;
use rsp::{StopReply, parse_stop_reply};

pub struct GdbClient {
    stream: TcpStream,
    layout: RegisterLayout,
    cached_g: Option<String>,
}

impl GdbClient {
    pub fn connect(addr: SocketAddr, timeout: Duration, layout: RegisterLayout) -> Result<Self> {
        let stream = TcpStream::connect_timeout(&addr, timeout)
            .with_context(|| format!("failed to connect to GDB server at {addr}"))?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        Ok(Self {
            stream,
            layout,
            cached_g: None,
        })
    }

    pub fn handshake(&mut self) -> Result<()> {
        let _ = self.cmd("qSupported:multiprocess+;swbreak+;hwbreak+")?;
        let _ = self.cmd("?")?;
        // Wokwi reports "multi-threaded target" without thread-id; select hart 0.
        let _ = self.cmd("Hg0");
        Ok(())
    }

    pub fn set_io_timeout(&self, timeout: Duration) -> Result<()> {
        self.stream.set_read_timeout(Some(timeout.max(Duration::from_millis(200))))?;
        Ok(())
    }

    pub fn invalidate_regs(&mut self) {
        self.cached_g = None;
    }

    pub fn monitor(&mut self, command: &str) -> Result<String> {
        let hex_cmd = command.bytes().map(|b| format!("{:02x}", b)).collect::<String>();
        self.cmd(&format!("qRcmd,{hex_cmd}"))
    }

    pub fn reset_halt(&mut self) -> Result<()> {
        let _ = self.monitor("reset halt");
        Ok(())
    }

    pub fn continue_run(&mut self) -> Result<StopReply> {
        self.invalidate_regs();
        let _ = self.cmd("Hc0");
        let reply = if self.supports_vcont()? {
            self.cmd("vCont;c:0")?
        } else {
            self.cmd("c")?
        };
        self.invalidate_regs();
        parse_stop_reply(&reply)
    }

    pub fn read_pc(&mut self) -> Result<u32> {
        self.read_reg(self.layout.pc_reg)
    }

    pub fn file_io_reply(&mut self, response: &str) -> Result<StopReply> {
        let reply = self.cmd(response)?;
        parse_stop_reply(&reply)
    }

    pub fn read_mem(&mut self, addr: u64, len: usize) -> Result<Vec<u8>> {
        let reply = self.cmd(&format!("m{:x},{:x}", addr, len))?;
        if reply.starts_with('E') {
            bail!("GDB memory read failed at {addr:#x}: {reply}");
        }
        decode_hex_bytes(&reply)
    }

    pub fn write_mem(&mut self, addr: u64, data: &[u8]) -> Result<()> {
        let hex = encode_hex_bytes(data);
        let reply = self.cmd(&format!("M{:x},{:x}:{}", addr, data.len(), hex))?;
        if reply != "OK" {
            bail!("GDB memory write failed at {addr:#x}: {reply}");
        }
        Ok(())
    }

    pub fn read_reg(&mut self, reg: u32) -> Result<u32> {
        let _ = self.cmd("Hg0");
        // Wokwi's `p` replies are not reliable; the `g` packet matches the ESP register layout.
        self.read_reg_g(reg)
    }

    pub fn write_reg(&mut self, reg: u32, value: u32) -> Result<()> {
        let _ = self.cmd("Hg0");
        self.invalidate_regs();
        if self.write_reg_p(reg, value).is_ok() {
            return Ok(());
        }
        self.write_reg_g(reg, value)
    }

    fn read_reg_g(&mut self, reg: u32) -> Result<u32> {
        if reg >= self.layout.reg_count {
            bail!("register {reg} out of range (max {})", self.layout.reg_count - 1);
        }
        if self.cached_g.is_none() {
            let reply = self.cmd("g")?;
            let expected = self.layout.g_packet_len();
            if reply.len() < expected {
                bail!("g packet too short: got {} expected {}", reply.len(), expected);
            }
            self.cached_g = Some(reply);
        }
        let reply = self.cached_g.as_ref().unwrap();
        let width = self.layout.reg_width_hex();
        let start = (reg as usize) * width;
        parse_reg_value(&reply[start..start + width])
    }

    fn write_reg_p(&mut self, reg: u32, value: u32) -> Result<()> {
        let reply = self.cmd(&format!("P{reg}={}", encode_reg_value(value)))?;
        if reply != "OK" {
            bail!("P{reg} failed: {reply}");
        }
        Ok(())
    }

    fn write_reg_g(&mut self, reg: u32, value: u32) -> Result<()> {
        if reg >= self.layout.reg_count {
            bail!("register {reg} out of range (max {})", self.layout.reg_count - 1);
        }
        if self.cached_g.is_none() {
            let reply = self.cmd("g")?;
            self.cached_g = Some(reply);
        }
        let width = self.layout.reg_width_hex();
        let mut regs = self.cached_g.clone().unwrap();
        let start = (reg as usize) * width;
        let hex = encode_reg_value(value);
        regs.replace_range(start..start + width, &hex);
        let reply = self.cmd(&format!("G{regs}"))?;
        if reply != "OK" {
            bail!("G write failed: {reply}");
        }
        self.cached_g = Some(regs);
        Ok(())
    }

    fn supports_vcont(&mut self) -> Result<bool> {
        let reply = self.cmd("vCont?")?;
        Ok(reply.contains('c'))
    }

    fn cmd(&mut self, payload: &str) -> Result<String> {
        self.send_packet(payload)?;
        self.read_packet()
    }

    fn send_packet(&mut self, payload: &str) -> Result<()> {
        let checksum = payload.bytes().fold(0u8, |acc, b| acc.wrapping_add(b));
        let frame = format!("${payload}#{:02x}", checksum);
        self.stream.write_all(frame.as_bytes())?;
        self.stream.flush()?;
        self.read_ack()?;
        Ok(())
    }

    fn read_packet(&mut self) -> Result<String> {
        self.skip_until(b'$')?;
        let mut buf = Vec::new();
        loop {
            let b = self.read_byte()?;
            if b == b'#' { break; }
            buf.push(b);
        }
        let mut checksum = [0u8; 2];
        self.stream.read_exact(&mut checksum).context("gdb checksum")?;
        self.send_ack()?;
        Ok(String::from_utf8(buf)?)
    }

    fn read_byte(&mut self) -> Result<u8> {
        let mut byte = [0u8; 1];
        self.stream.read_exact(&mut byte).context("gdb read")?;
        Ok(byte[0])
    }

    fn skip_until(&mut self, target: u8) -> Result<()> {
        loop {
            let b = self.read_byte()?;
            if b == target { return Ok(()); }
        }
    }

    fn read_ack(&mut self) -> Result<()> {
        loop {
            let b = self.read_byte()?;
            if b == b'+' { return Ok(()); }
            if b == b'-' { bail!("GDB NACK"); }
        }
    }

    fn send_ack(&mut self) -> Result<()> {
        self.stream.write_all(b"+")?;
        self.stream.flush()?;
        Ok(())
    }
}

fn encode_hex_bytes(data: &[u8]) -> String {
    data.iter().map(|b| format!("{:02x}", b)).collect()
}

fn decode_hex_bytes(hex: &str) -> Result<Vec<u8>> {
    if hex.len() % 2 != 0 { bail!("invalid hex length"); }
    let mut out = Vec::with_capacity(hex.len() / 2);
    for i in (0..hex.len()).step_by(2) {
        out.push(u8::from_str_radix(&hex[i..i + 2], 16)?);
    }
    Ok(out)
}

fn encode_reg_value(value: u32) -> String {
    // GDB RSP: register values are hex byte pairs, least-significant byte first.
    value.to_le_bytes().iter().map(|b| format!("{:02x}", b)).collect()
}

fn parse_reg_value(hex: &str) -> Result<u32> {
    if hex.is_empty() {
        bail!("empty register value");
    }
    let hex = if hex.len() > 8 { &hex[hex.len() - 8..] } else { hex };
    if hex.len() % 2 != 0 {
        bail!("invalid register hex length: {}", hex.len());
    }
    let bytes = decode_hex_bytes(hex)?;
    let mut arr = [0u8; 4];
    arr[..bytes.len()].copy_from_slice(&bytes);
    Ok(u32::from_le_bytes(arr))
}

pub fn wait_for_port(addr: SocketAddr, timeout: Duration) -> Result<()> {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_ok() {
            std::thread::sleep(Duration::from_millis(500));
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    bail!("timed out waiting for GDB server at {addr}");
}


