use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize, SlavePty};
use std::io::{Read, Write};

use tokio::sync::mpsc;

use crate::backend::EventProxy;

/// Handle to a running PTY child process.
pub struct PtyHandle {
    /// Single channel funnelling every byte that needs to reach the
    /// PTY's stdin. Both user keystrokes (via `PtyHandle::write`) and
    /// the terminal emulator's auto-replies (e.g. cursor-position
    /// responses to ConPTY's `\x1b[6n`) push here, and a dedicated
    /// writer thread drains it serially. Routing through one channel
    /// keeps the two write sources from racing on the underlying
    /// `Write` and lets every public method stay `&self`.
    write_tx: mpsc::UnboundedSender<Vec<u8>>,
    _master: Box<dyn MasterPty + Send>,
    // Keep the slave alive for the lifetime of the session.
    // On Windows (ConPTY), dropping the slave calls ClosePseudoConsole(),
    // which terminates the child process.
    _slave: Box<dyn SlavePty + Send>,
}

impl PtyHandle {
    /// Spawn the OS default shell. Equivalent to
    /// `spawn_command(cols, rows, None, &[], event_proxy)`.
    pub fn spawn(
        cols: u16,
        rows: u16,
        event_proxy: &EventProxy,
    ) -> crate::widget::TerminalResult<(Self, mpsc::UnboundedReceiver<Vec<u8>>)>
    {
        Self::spawn_command(cols, rows, None, &[], event_proxy)
    }

    /// Spawn an explicit program in a PTY (e.g. PowerShell or
    /// `wsl.exe -d Ubuntu`). Passing `None` for `program` falls back
    /// to the OS default. Always sets `TERM=xterm-256color` and
    /// `COLORTERM=truecolor` so apps detect 256-color / truecolor.
    /// `event_proxy` is given the writer-side of the central PTY
    /// write channel so the emulator can answer host queries (DSR
    /// cursor-position, etc.) — without that, ConPTY blocks on
    /// `\x1b[6n` and the terminal stays blank.
    pub fn spawn_command(
        cols: u16,
        rows: u16,
        program: Option<&str>,
        args: &[String],
        event_proxy: &EventProxy,
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
        let mut writer = pair.master.take_writer()?;

        let (tx, rx) = mpsc::unbounded_channel();
        let (write_tx, mut write_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        event_proxy.set_pty_write_tx(write_tx.clone());

        // Dedicated writer thread — drains the central write channel
        // into the PTY so user keystrokes and emulator replies share
        // one cursor without racing on the `Write`. Exits cleanly
        // when every sender (PtyHandle + EventProxy clones) is gone.
        let program_label = program.unwrap_or("<default>").to_string();
        std::thread::Builder::new()
            .name("pty-writer".into())
            .spawn(move || {
                while let Some(chunk) = write_rx.blocking_recv() {
                    if let Err(e) = writer.write_all(&chunk) {
                        tracing::warn!(
                            "PTY writer error for {}: {}",
                            program_label, e,
                        );
                        break;
                    }
                    let _ = writer.flush();
                }
                tracing::debug!("PTY writer thread exiting for {}", program_label);
            })?;

        // Spawn a thread to read PTY output (blocking IO)
        let program_log = program.unwrap_or("<default>").to_string();
        std::thread::Builder::new()
            .name("pty-reader".into())
            .spawn(move || {
                tracing::info!("PTY reader thread started for {}", program_log);
                let mut buf = [0u8; 8192];
                let mut total_bytes: u64 = 0;
                let mut chunk_count: u64 = 0;
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => {
                            tracing::warn!(
                                "PTY EOF for {} after {} bytes ({} chunks) — child likely exited",
                                program_log, total_bytes, chunk_count,
                            );
                            break;
                        }
                        Ok(n) => {
                            chunk_count += 1;
                            total_bytes += n as u64;
                            // Log every chunk while diagnosing the
                            // black-terminal symptom; trims back to
                            // first / occasional once stable.
                            tracing::info!(
                                "PTY chunk #{} for {}: {} bytes (total {})  preview={:?}",
                                chunk_count,
                                program_log,
                                n,
                                total_bytes,
                                String::from_utf8_lossy(
                                    &buf[..n.min(64)],
                                ),
                            );
                            if tx.send(buf[..n].to_vec()).is_err() {
                                tracing::warn!(
                                    "PTY receiver dropped for {} after {} bytes",
                                    program_log, total_bytes,
                                );
                                break;
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                "PTY read error for {} after {} bytes: {}",
                                program_log, total_bytes, e,
                            );
                            break;
                        }
                    }
                }
                tracing::debug!("PTY reader thread exiting for {}", program_log);
            })?;

        Ok((
            Self {
                write_tx,
                _master: pair.master,
                _slave: pair.slave,
            },
            rx,
        ))
    }

    /// Write bytes to the PTY (keyboard input). Routes through the
    /// central write channel; the dedicated writer thread does the
    /// actual `Write` so this never blocks on slow PTYs.
    pub fn write(&self, data: &[u8]) -> std::io::Result<()> {
        self.write_tx
            .send(data.to_vec())
            .map_err(|_| std::io::Error::other("PTY writer thread is gone"))
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
