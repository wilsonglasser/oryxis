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
    /// The child process. Killed on `Drop` so closing a pane / tab tears
    /// down the shell. Without this, the reader thread holds a cloned
    /// master fd that keeps the slave open, so on Unix the child never
    /// gets SIGHUP and a long-running app (htop, a `tail -f`) survives the
    /// close, with the reader thread spinning forever on its output.
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

impl Drop for PtyHandle {
    fn drop(&mut self) {
        // Best effort: SIGKILL the child, then reap it so it doesn't
        // linger as a zombie. After the kill the child exits promptly, so
        // the `wait` doesn't meaningfully block. Killing also lets the
        // reader thread see EOF and exit, ending the PTY output stream.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
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
    /// cursor-position, etc.), without that, ConPTY blocks on
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

        let child = pair.slave.spawn_command(cmd)?;

        let mut reader = pair.master.try_clone_reader()?;
        let mut writer = pair.master.take_writer()?;

        let (tx, rx) = mpsc::unbounded_channel();
        let (write_tx, mut write_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        event_proxy.set_pty_write_tx(write_tx.clone());

        // Dedicated writer thread, drains the central write channel
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

        // Spawn a thread to read PTY output (blocking IO). Raw chunks go
        // to a coalescer thread (below) instead of straight to the UI, so
        // a heavy output burst becomes a few large messages rather than
        // one update+view+draw cycle per 8KB read.
        let program_log = program.unwrap_or("<default>").to_string();
        let (raw_tx, raw_rx) = std::sync::mpsc::channel::<Vec<u8>>();
        std::thread::Builder::new()
            .name("pty-reader".into())
            .spawn(move || {
                tracing::debug!("PTY reader thread started for {}", program_log);
                let mut buf = [0u8; 8192];
                let mut total_bytes: u64 = 0;
                let mut chunk_count: u64 = 0;
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => {
                            tracing::warn!(
                                "PTY EOF for {} after {} bytes ({} chunks), child likely exited",
                                program_log, total_bytes, chunk_count,
                            );
                            break;
                        }
                        Ok(n) => {
                            chunk_count += 1;
                            total_bytes += n as u64;
                            if raw_tx.send(buf[..n].to_vec()).is_err() {
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

        // Coalescer thread: batches raw chunks into one message per burst.
        // The first chunk is taken with a blocking recv (zero added latency
        // for interactive echo); after that, anything already queued is
        // drained with try_recv. A short grace wait is only used once the
        // batch already looks like bulk output (>= one typical read), so a
        // lone keystroke echo is never delayed by it. Exits when the reader
        // thread drops its sender, which drops `tx` and ends the UI stream.
        let coalesce_log = program.unwrap_or("<default>").to_string();
        std::thread::Builder::new()
            .name("pty-coalesce".into())
            .spawn(move || {
                use std::sync::mpsc::TryRecvError;
                // Cap one forwarded message at ~64KB so a giant paste of
                // output still yields steady redraws instead of one stall.
                const COALESCE_MAX: usize = 64 * 1024;
                // Batches at or above this are treated as a burst in
                // flight, worth a short wait for the next read to land.
                const BURST_THRESHOLD: usize = 2048;
                const GRACE: std::time::Duration = std::time::Duration::from_millis(2);
                while let Ok(first) = raw_rx.recv() {
                    let mut batch = first;
                    while batch.len() < COALESCE_MAX {
                        match raw_rx.try_recv() {
                            Ok(more) => batch.extend_from_slice(&more),
                            Err(TryRecvError::Empty) => {
                                if batch.len() >= BURST_THRESHOLD {
                                    match raw_rx.recv_timeout(GRACE) {
                                        Ok(more) => batch.extend_from_slice(&more),
                                        Err(_) => break,
                                    }
                                } else {
                                    break;
                                }
                            }
                            Err(TryRecvError::Disconnected) => break,
                        }
                    }
                    if tx.send(batch).is_err() {
                        break;
                    }
                }
                tracing::debug!("PTY coalescer thread exiting for {}", coalesce_log);
            })?;

        Ok((
            Self {
                write_tx,
                _master: pair.master,
                _slave: pair.slave,
                child,
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
