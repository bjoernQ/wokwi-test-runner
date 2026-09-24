use anyhow::Result;
use crate::semihosting::{OpenMode, SemihostingCommand};
use crate::target::Target;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HandleKind {
    Stdout,
    Stderr,
}

pub struct SemihostingFiles {
    handles: Vec<Option<HandleKind>>,
}

impl SemihostingFiles {
    pub fn new() -> Self {
        Self { handles: Vec::new() }
    }

    pub fn handle(&mut self, target: &mut Target, cmd: SemihostingCommand) -> Result<()> {
        match cmd {
            SemihostingCommand::Open { path, mode } => self.handle_open(target, path, mode),
            SemihostingCommand::Close { handle } => self.handle_close(target, handle),
            SemihostingCommand::Write { handle, data } => self.handle_write(target, handle, &data),
            SemihostingCommand::WriteConsole { text } => {
                eprint!("{text}");
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn handle_open(&mut self, target: &mut Target, path: String, mode: OpenMode) -> Result<()> {
        let kind = if path == ":tt" {
            match mode {
                OpenMode::Write | OpenMode::WriteBinary => HandleKind::Stdout,
                OpenMode::Append | OpenMode::AppendBinary => HandleKind::Stderr,
                _ => {
                    target.write_return(-1)?;
                    return Ok(());
                }
            }
        } else {
            target.write_return(-1)?;
            return Ok(());
        };
        self.handles.push(Some(kind));
        target.write_return(self.handles.len() as i32)?;
        Ok(())
    }

    fn handle_close(&mut self, target: &mut Target, handle: u32) -> Result<()> {
        if self.take_handle(handle).is_some() {
            self.trim_handles();
            target.write_return(0)?;
        }
        Ok(())
    }

    fn handle_write(&mut self, target: &mut Target, handle: u32, data: &[u8]) -> Result<()> {
        let Some(kind) = self.get_handle(handle) else {
            return Ok(());
        };
        let text = String::from_utf8_lossy(data);
        match kind {
            HandleKind::Stdout => eprint!("{text}"),
            HandleKind::Stderr => eprint!("{text}"),
        }
        target.write_return(0)?;
        Ok(())
    }

    fn get_handle(&self, handle: u32) -> Option<HandleKind> {
        self.handles.get(handle as usize - 1).and_then(|h| *h)
    }

    fn take_handle(&mut self, handle: u32) -> Option<HandleKind> {
        self.handles.get_mut(handle as usize - 1).and_then(|h| h.take())
    }

    fn trim_handles(&mut self) {
        while self.handles.last().is_some_and(|h| h.is_none()) {
            self.handles.pop();
        }
    }
}
