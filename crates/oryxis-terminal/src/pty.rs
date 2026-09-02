use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
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
    /// Kills the child on `Drop`, so closing a pane / tab tears down the
    /// shell. Without it the reader thread holds a cloned master fd that
    /// keeps the slave open, so on Unix the child never gets SIGHUP and a
    /// long-running app (htop, a `tail -f`) survives the close with the
    /// reader spinning forever on its output.
    ///
    /// A killer rather than the `Child` itself, because the child now
    /// lives in the waiter thread below, blocked in `wait()`. That is the
    /// split `ChildKiller` exists for.
    killer: Box<dyn portable_pty::ChildKiller + Send + Sync>,
    /// Fires once the child process has been reaped, whether it exited
    /// on its own or was killed. Taken by whoever wants to be told.
    ///
    /// An explicit signal, NOT the output stream ending, because those
    /// are not the same event and only this one means "the shell is
    /// gone". A pty's reader cannot be relied on to notice: on Windows
    /// the pseudoconsole outlives the slave (the master holds an `Arc`
    /// to the same one), so the read side stays open and the reader
    /// stays blocked until the whole handle is dropped, which can be
    /// minutes after the shell died. Anything driven off the byte
    /// stream is therefore reporting teardown, not exit.
    child_exit: Option<tokio::sync::oneshot::Receiver<()>>,
}

impl Drop for PtyHandle {
    fn drop(&mut self) {
        // Best effort: SIGKILL the child. The waiter thread is blocked in
        // `wait()` and reaps it, so there is no zombie and nothing to
        // block on here.
        let _ = self.killer.kill();
    }
}

impl PtyHandle {
    /// Spawn the OS default shell. Equivalent to
    /// `spawn_command(cols, rows, None, &[], None, &[], event_proxy)`.
    pub fn spawn(
        cols: u16,
        rows: u16,
        event_proxy: &EventProxy,
    ) -> crate::widget::TerminalResult<(Self, mpsc::UnboundedReceiver<Vec<u8>>)>
    {
        Self::spawn_command(cols, rows, None, &[], None, &[], event_proxy)
    }

    /// Spawn an explicit program in a PTY (e.g. PowerShell or
    /// `wsl.exe -d Ubuntu`). Passing `None` for `program` falls back
    /// to the OS default. Always sets `TERM=xterm-256color` and
    /// `COLORTERM=truecolor` so apps detect 256-color / truecolor.
    /// `env` adds (or overrides) variables for the child, which is what
    /// a saved local host passes its own `env_vars` through; an empty
    /// slice keeps the inherited environment exactly as it was.
    /// `event_proxy` is given the writer-side of the central PTY
    /// write channel so the emulator can answer host queries (DSR
    /// cursor-position, etc.), without that, ConPTY blocks on
    /// `\x1b[6n` and the terminal stays blank.
    pub fn spawn_command(
        cols: u16,
        rows: u16,
        program: Option<&str>,
        args: &[String],
        cwd: Option<&str>,
        env: &[(String, String)],
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
        // Host-provided variables last, so a host that deliberately
        // sets TERM (a device that only understands vt100) wins over
        // the defaults above rather than being silently overruled.
        for (name, value) in env {
            if !name.trim().is_empty() {
                cmd.env(name, value);
            }
        }
        // Start in the inherited working directory (OSC 7) when it still
        // exists; a stale dir would make the shell fail to launch, so fall
        // back to the default by ignoring a missing path.
        if let Some(dir) = cwd
            && std::path::Path::new(dir).is_dir()
        {
            cmd.cwd(dir);
        }

        let mut child = pair.slave.spawn_command(cmd)?;
        let killer = child.clone_killer();

        // Watch the child, so a shell that exits on its own is noticed.
        //
        // Nothing used to. The handle held the slave for the whole
        // session and a pty's read side cannot reach EOF while a writer
        // is open, so `exit` in a local shell produced no event at all:
        // the pane froze, the reader thread stayed blocked on a shell
        // that was already gone, and the only EOF it ever saw came from
        // `Drop` killing a child that had died minutes earlier. That is
        // why the reader's own log line can only say "child LIKELY
        // exited"; it never actually knew.
        //
        // The answer is this thread and the oneshot it fires, not
        // anything the byte stream does. Closing the slave here does
        // free the reader on Unix, but on Windows it is inert: the
        // pseudoconsole is behind an `Arc` the master holds too, so
        // `ClosePseudoConsole()` does not run and the reader stays
        // blocked regardless. Correctness must not rest on which of
        // those a platform does, so it rests on the signal instead.
        let slave = pair.slave;
        let (exit_tx, exit_rx) = tokio::sync::oneshot::channel();
        let waiter_label = program.unwrap_or("<default>").to_string();
        std::thread::Builder::new()
            .name("pty-waiter".into())
            .spawn(move || {
                let status = child.wait();
                // Let the reader drain what the shell wrote on its way
                // out before anyone is told it is gone, so the last of
                // the session reaches the screen and the recording
                // ahead of the notice that ends it.
                std::thread::sleep(std::time::Duration::from_millis(100));
                tracing::debug!(
                    "PTY child exited for {} ({:?})",
                    waiter_label, status,
                );
                let _ = exit_tx.send(());
                drop(slave);
            })?;

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
                killer,
                child_exit: Some(exit_rx),
            },
            rx,
        ))
    }

    /// Take the child-exit signal, once. `None` on every later call, so
    /// two callers cannot both believe they are the one being told.
    pub fn take_child_exit(&mut self) -> Option<tokio::sync::oneshot::Receiver<()>> {
        self.child_exit.take()
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
