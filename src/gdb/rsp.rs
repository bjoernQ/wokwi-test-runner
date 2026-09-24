use anyhow::{Result, bail};

#[derive(Debug, Clone)]
pub enum StopReply {
    Halted,
    FileIo(String),
}

pub fn parse_stop_reply(reply: &str) -> Result<StopReply> {
    if reply.starts_with('F') {
        return Ok(StopReply::FileIo(reply.to_string()));
    }
    if reply.starts_with("T") || reply.starts_with('S') {
        return Ok(StopReply::Halted);
    }
    if reply == "X00" || reply.starts_with('W') {
        return Ok(StopReply::Halted);
    }
    if reply == "E01" {
        bail!("target not running");
    }
    Ok(StopReply::Halted)
}

pub fn is_semihosting_op(op: u32) -> bool {
    matches!(
        op,
        0x01 | 0x02 | 0x03 | 0x04 | 0x05 | 0x06 | 0x07 | 0x08 | 0x09 | 0x0a | 0x0b | 0x0c | 0x0d | 0x0e
            | 0x0f | 0x10 | 0x11 | 0x12 | 0x13 | 0x15 | 0x16 | 0x17 | 0x18 | 0x19 | 0x1a
            | 0x1b | 0x1c | 0x1d | 0x1e | 0x1f | 0x20 | 0x100
    )
}

