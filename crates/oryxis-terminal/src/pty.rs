use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};

use tokio::sync::mpsc;

/// Handle to a running PTY child process.
pub struct PtyHandle {
    writer: Box<dyn Write + Send>,
    _master: Box<dyn MasterPty + Send>,
}

impl PtyHandle {
    /// Spawn a new shell in a PTY. Returns the handle and a receiver for output bytes.
    pub fn spawn(
        cols: u16,
        rows: u16,
    ) -> crate::widget::TerminalResult<(Self, mpsc::UnboundedReceiver<Vec<u8>>)>
    {
        let pty_system = native_pty_system();

        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = CommandBuilder::new_default_prog();
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");

        let _child = pair.slave.spawn_command(cmd)?;
        // Drop slave so we only interact via master
        drop(pair.slave);

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
