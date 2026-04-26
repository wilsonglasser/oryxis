use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize, SlavePty};
use std::io::{Read, Write};

use tokio::sync::mpsc;

/// Handle to a running PTY child process.
pub struct PtyHandle {
    writer: Box<dyn Write + Send>,
    _master: Box<dyn MasterPty + Send>,
    // Keep the slave alive for the lifetime of the session.
    // On Windows (ConPTY), dropping the slave calls ClosePseudoConsole(),
    // which terminates the child process.
    _slave: Box<dyn SlavePty + Send>,
}

impl PtyHandle {
    /// Spawn the OS default shell. Equivalent to
    /// `spawn_command(cols, rows, None, &[])`.
    pub fn spawn(
        cols: u16,
        rows: u16,
    ) -> crate::widget::TerminalResult<(Self, mpsc::UnboundedReceiver<Vec<u8>>)>
    {
        Self::spawn_command(cols, rows, None, &[])
    }

    /// Spawn an explicit program in a PTY (e.g. PowerShell or
    /// `wsl.exe -d Ubuntu`). Passing `None` for `program` falls back
    /// to the OS default. Always sets `TERM=xterm-256color` and
    /// `COLORTERM=truecolor` so apps detect 256-color / truecolor.
    pub fn spawn_command(
        cols: u16,
        rows: u16,
        program: Option<&str>,
        args: &[String],
    ) -> crate::widget::TerminalResult<(Self, mpsc::UnboundedReceiver<Vec<u8>>)>
    {
        let pty_system = native_pty_system();

        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = match program {
            Some(p) => CommandBuilder::new(p),
            None => CommandBuilder::new_default_prog(),
        };
        for arg in args {
            cmd.arg(arg);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");

        let _child = pair.slave.spawn_command(cmd)?;

        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

        let (tx, rx) = mpsc::unbounded_channel();

        // Spawn a thread to read PTY output (blocking IO)
        std::thread::Builder::new()
            .name("pty-reader".into())
            .spawn(move || {
                let mut buf = [0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            if tx.send(buf[..n].to_vec()).is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            tracing::debug!("PTY read error: {}", e);
                            break;
                        }
                    }
                }
                tracing::debug!("PTY reader thread exiting");
            })?;

        Ok((
            Self {
                writer,
                _master: pair.master,
                _slave: pair.slave,
            },
            rx,
        ))
    }

    /// Write bytes to the PTY (keyboard input).
    pub fn write(&mut self, data: &[u8]) -> std::io::Result<()> {
        self.writer.write_all(data)?;
        self.writer.flush()
    }

    /// Resize the PTY.
    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), Box<dyn std::error::Error>> {
        self._master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }
}
