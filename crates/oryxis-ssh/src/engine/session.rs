use super::*;

// ---------------------------------------------------------------------------
// SSH Session
// ---------------------------------------------------------------------------

/// Result of a non-interactive command execution.
pub struct ExecResult {
    pub exit_code: u32,
    pub stdout: String,
    pub stderr: String,
}

/// Open one exec side channel on the shared handle, run `command`, and
/// collect bounded stdout. The body behind both [`SshSession::probe`]
/// and [`MonitorConn::probe`](super::MonitorConn::probe): the handle
/// lock is released as soon as the channel is open, so other tasks
/// (SFTP, forwards) aren't blocked while the command runs.
pub(crate) async fn probe_on(
    handle: &Arc<tokio::sync::Mutex<client::Handle<ClientHandler>>>,
    command: &str,
    timeout: std::time::Duration,
) -> Option<String> {
    let handle = handle.lock().await;
    let mut channel = handle.channel_open_session().await.ok()?;
    channel.exec(true, command).await.ok()?;
    drop(handle); // release so other tasks can use the shared handle

    // Hard cap on collected output: probe payloads are a few KB, and
    // the host side is untrusted, so an unbounded collect would let a
    // hostile (or misconfigured) command stream hundreds of MB into
    // memory within the timeout window. Generous headroom for a busy
    // host's df/socket tables; excess is dropped, not an error.
    const PROBE_STDOUT_CAP: usize = 512 * 1024;
    let mut stdout = Vec::new();
    let collect = async {
        loop {
            match channel.wait().await {
                // Once the cap is hit the guard stops matching and
                // excess data falls through to `_` (drained, dropped).
                Some(russh::ChannelMsg::Data { data })
                    if stdout.len() < PROBE_STDOUT_CAP =>
                {
                    let room = PROBE_STDOUT_CAP - stdout.len();
                    stdout.extend_from_slice(&data[..data.len().min(room)]);
                }
                Some(russh::ChannelMsg::Eof)
                | Some(russh::ChannelMsg::ExitStatus { .. })
                | None => break,
                _ => {}
            }
        }
    };
    tokio::time::timeout(timeout, collect).await.ok()?;
    Some(String::from_utf8_lossy(&stdout).into_owned())
}

/// A live SSH session with a remote PTY channel.
pub struct SshSession {
    /// The CONNECTION this session's channel rides on, shared with
    /// every other session on the same host, the SFTP surface and the
    /// port-forward tasks. Holding an `Arc` is what keeps the link
    /// alive exactly as long as someone is using it: the last owner
    /// dropping it IS the disconnect (see `SshTransport`).
    pub(crate) transport: Arc<super::SshTransport>,
    pub(crate) writer_tx: mpsc::UnboundedSender<Vec<u8>>,
    /// Forwarded to the SSH channel as `window-change` requests so the
    /// remote shell sees SIGWINCH and re-renders for the new viewport.
    /// Without this, apps like `top` keep rendering for the original
    /// columns and our local alacritty wraps the overflow into extra
    /// rows ("double line" effect).
    pub(crate) resize_tx: mpsc::UnboundedSender<(u16, u16)>,
    pub(crate) reader_task: tokio::task::JoinHandle<()>,
    pub(crate) writer_task: tokio::task::JoinHandle<()>,
    // Per-host forward tasks live on the transport now (forwards are
    // per CONNECTION): owned here they died with the dialing tab while
    // reused tabs kept the link, and no session ever rebound them.
    // Link quality lives on the transport now: one prober per
    // CONNECTION rather than one per session, so two tabs to the same
    // host no longer ping the same wire twice.
    /// Latched by `close()` so teardown runs exactly once even when both
    /// an explicit close and the `Drop` backstop fire.
    pub(crate) closed: std::sync::atomic::AtomicBool,
    /// Set by the reader task on its way out, BEFORE it drops the output
    /// sender. See [`SshSession::is_alive`] for why the order is the
    /// whole point.
    pub(crate) reader_done: Arc<std::sync::atomic::AtomicBool>,
    /// Cap on how long `open_sftp` (and the per-sibling open in the
    /// transfer pool) wait before giving up. Set by `SshEngine`'s
    /// builder so the user can tune it from the SFTP settings panel.
    pub(crate) sftp_open_timeout: std::time::Duration,
    /// Set when the pre-PTY terminfo probe found the configured `TERM`
    /// missing on the host (issue #88). See `engine::terminfo`.
    pub(crate) term_fallback: Option<TermFallback>,
}

impl std::fmt::Debug for SshSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SshSession")
            .field("alive", &self.is_alive())
            .finish()
    }
}

impl SshSession {
    pub fn write(&self, data: &[u8]) -> Result<(), SshError> {
        self.writer_tx
            .send(data.to_vec())
            .map_err(|e| SshError::Channel(format!("write failed: {}", e)))
    }

    /// Notify the remote shell that the local viewport changed shape.
    /// Errors are swallowed because resize requests fire often and a
    /// dropped one is cosmetically ugly but never fatal.
    pub fn resize(&self, cols: u16, rows: u16) {
        let _ = self.resize_tx.send((cols, rows));
    }

    /// Hand out a clone of the resize sender so the terminal state can
    /// forward viewport changes directly without round-tripping a message.
    pub fn resize_sender(&self) -> mpsc::UnboundedSender<(u16, u16)> {
        self.resize_tx.clone()
    }

    /// Hand out a clone of the input sender so the terminal emulator can
    /// answer in-band queries (cursor position report, device attributes,
    /// DECRQM, ...) directly on the channel. Remote programs block waiting
    /// for these replies; without the back-channel they hang with the tty
    /// in raw mode, which looks like a full terminal freeze.
    pub fn write_sender(&self) -> mpsc::UnboundedSender<Vec<u8>> {
        self.writer_tx.clone()
    }

    /// Open a fresh SFTP subsystem channel on this session, the SSH
    /// connection multiplexes, so the original PTY channel keeps running.
    /// Wrapped in the engine-configured timeout to keep `open_sftp` from
    /// hanging the UI when a server doesn't speak the sftp subsystem.
    pub async fn open_sftp(&self) -> Result<crate::sftp::SftpClient, SshError> {
        let timeout = self.sftp_open_timeout;
        let handle_for_exec = Arc::clone(self.transport.handle());
        let inner = async {
            let handle = self.transport.handle().lock().await;
            let channel = handle
                .channel_open_session()
                .await
                .map_err(|e| SshError::Channel(format!("sftp channel open: {e}")))?;
            channel
                .request_subsystem(true, "sftp")
                .await
                .map_err(|e| SshError::Channel(format!("sftp subsystem: {e}")))?;
            let session = russh_sftp::client::SftpSession::new(channel.into_stream())
                .await
                .map_err(|e| SshError::Channel(format!("sftp init: {e}")))?;
            Ok::<_, SshError>(session)
        };
        let session = tokio::time::timeout(timeout, inner)
            .await
            .map_err(|_| {
                SshError::Channel(format!(
                    "sftp open timed out after {}s",
                    timeout.as_secs()
                ))
            })??;
        Ok(crate::sftp::SftpClient::new(session, handle_for_exec, timeout))
    }

    /// Open a `-L` forward on THIS session's existing connection, bound
    /// to a caller-chosen local port.
    ///
    /// Another channel on a connection that is already authenticated, so
    /// it costs no second handshake and no second password prompt - the
    /// point of `SshTransport` being shared. What it cannot do is pick
    /// the local port the way `connect_local_forward_ephemeral` does: a
    /// loopback OAuth callback is registered with the authorization
    /// server as an exact `redirect_uri`, so the local end has to be the
    /// very port the remote process is listening on or the browser lands
    /// nowhere. A port already bound here is therefore an error and not
    /// something to work around - the caller's cue to say so, rather
    /// than to send a browser (carrying an authorization code) to
    /// whatever else holds that port.
    ///
    /// Binds LOOPBACK only, never `0.0.0.0`: the far end is a service on
    /// the remote's loopback that is deliberately not exposed, and the
    /// near end is for this machine's browser alone. The family is the
    /// caller's (a callback written at `[::1]` is dialled over IPv6 and
    /// an IPv4 listener would not be found), the refusal is not: a dial
    /// site that passed a routable address would put that service on the
    /// network, so this checks rather than trusts.
    pub async fn open_local_forward(
        &self,
        listen_host: &str,
        listen_port: u16,
        target_host: &str,
        target_port: u16,
        policy: AutoClose,
    ) -> Result<ForwardSession, SshError> {
        if !listen_host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
        {
            return Err(SshError::Channel(format!(
                "refusing to bind {listen_host}: not a loopback address"
            )));
        }
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        let cancel_tx = Arc::new(cancel_tx);
        let listener = bind_forward_listener(listen_host, listen_port).await?;
        let handle = Arc::clone(self.transport.handle());
        let task = spawn_autoclose_local_forward_task(
            listener,
            Arc::clone(&handle),
            target_host.to_string(),
            target_port,
            listen_port,
            cancel_rx,
            Arc::clone(&cancel_tx),
            policy,
        );
        tracing::info!(
            "forward(-L on session) {}:{} -> {}:{} up",
            listen_host, listen_port, target_host, target_port
        );
        Ok(ForwardSession {
            handle,
            cancel_tx,
            _tasks: vec![task],
            remote_bind: None,
            remote_route: None,
        })
    }

    /// Run a short, silent command on a side channel of this live session
    /// and return its stdout. Same shape as `detect_os` (which predates
    /// it), generalized so callers can supply the command: the host
    /// monitor batches its whole `/proc` read into one `sh -c` per tick,
    /// keeping the cost at a single channel round trip.
    ///
    /// Nothing reaches the user's PTY, and the shared handle lock is
    /// released as soon as the channel is open so other tasks (SFTP,
    /// forwards) aren't blocked while the command runs. Returns `None` on
    /// any channel failure or if the command outlives `timeout`.
    pub async fn probe(
        &self,
        command: &str,
        timeout: std::time::Duration,
    ) -> Option<String> {
        probe_on(self.transport.handle(), command, timeout).await
    }

    /// [`Self::probe`] with the full result: exit status, stdout AND
    /// stderr, plus an optional stdin payload written before the command
    /// is read back.
    ///
    /// `probe` throws the exit status away because a monitor tick only
    /// cares about the text it got. A command that ACTS on the host (the
    /// Monitor tab's kill-the-process-on-this-port, issue #96) has to
    /// know whether it worked, and `sudo -S` reads its password from
    /// stdin, so both are surfaced here. The stdin bytes are written and
    /// EOF'd before the collect loop: a secret must never travel in the
    /// command line, where `ps` on the host would show it to every user.
    ///
    /// Same channel discipline as `probe`: the shared handle lock is
    /// released as soon as the channel is open, output is capped (the
    /// host is untrusted), and the loop reads until the channel CLOSES
    /// rather than stopping at `Eof`, because some servers deliver
    /// `ExitStatus` afterwards and an early break would report 255 for a
    /// command that succeeded. Returns `None` on any channel failure or
    /// if the command outlives `timeout`.
    pub async fn exec_capture(
        &self,
        command: &str,
        stdin: Option<Vec<u8>>,
        timeout: std::time::Duration,
    ) -> Option<ExecResult> {
        let handle = self.transport.handle().lock().await;
        let mut channel = handle.channel_open_session().await.ok()?;
        channel.exec(true, command).await.ok()?;
        drop(handle); // release so other tasks can use the shared handle
        if let Some(data) = stdin {
            channel.data(&data[..]).await.ok()?;
            // Without the EOF a `sudo -S` that never reads (NOPASSWD
            // path) leaves the channel half-open until the timeout.
            channel.eof().await.ok()?;
        }

        // Same cap as `probe`: enough for a socket table, small enough
        // that a hostile command can't stream memory away inside the
        // timeout window. Excess is drained and dropped, not an error.
        const EXEC_OUTPUT_CAP: usize = 512 * 1024;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exit_code: Option<u32> = None;
        let collect = async {
            loop {
                match channel.wait().await {
                    Some(russh::ChannelMsg::Data { data }) if stdout.len() < EXEC_OUTPUT_CAP => {
                        let room = EXEC_OUTPUT_CAP - stdout.len();
                        stdout.extend_from_slice(&data[..data.len().min(room)]);
                    }
                    Some(russh::ChannelMsg::ExtendedData { data, ext: 1 })
                        if stderr.len() < EXEC_OUTPUT_CAP =>
                    {
                        let room = EXEC_OUTPUT_CAP - stderr.len();
                        stderr.extend_from_slice(&data[..data.len().min(room)]);
                    }
                    Some(russh::ChannelMsg::ExitStatus { exit_status }) => {
                        exit_code = Some(exit_status);
                    }
                    None => break,
                    _ => {}
                }
            }
        };
        tokio::time::timeout(timeout, collect).await.ok()?;
        Some(ExecResult {
            // A server that closes the channel without an exit status
            // leaves us with 255, the same "unknown failure" convention
            // the SFTP exec path uses.
            exit_code: exit_code.unwrap_or(255),
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        })
    }

    pub fn is_alive(&self) -> bool {
        // Four death signals, any one of which means the session is
        // unusable: an explicit `close()` (latch, the task aborts it
        // triggers land asynchronously), the reader task having exited
        // (EOF / exit-status / transport drop; the writer task alone
        // can't notice, it blocks on its queue forever when nothing
        // writes), the writer channel being gone, and `reader_done`.
        //
        // `reader_done` looks redundant next to `reader_task` and is
        // not, because it is the only one with a guaranteed ORDER. The
        // app reads the end of the output stream as the session's death
        // notice (`SshDisconnected`) and asks this before acting on it,
        // so a notice arriving while this still says "alive" reads as a
        // notice from a session the pane has already replaced, and is
        // discarded. `reader_task.is_finished()` cannot carry that
        // weight: the output sender is dropped as the task's future
        // returns, and the handle is not marked finished until that
        // return completes, so a reader watching from another thread
        // can legally observe the closed channel first. The flag is set
        // by the reader itself, before the drop, in the same task with
        // no await in between, which makes "dead before silent" true by
        // construction rather than by scheduling luck.
        !self.closed.load(std::sync::atomic::Ordering::SeqCst)
            && !self.reader_done.load(std::sync::atomic::Ordering::SeqCst)
            && !self.reader_task.is_finished()
            && !self.writer_tx.is_closed()
    }

    /// Point-in-time link-quality figures for this session's CONNECTION
    /// (RTT probe window). See [`NetQualitySnapshot`]. Sessions sharing
    /// a connection report the same figures, which is correct: they are
    /// measuring one wire.
    pub fn net_quality(&self) -> NetQualitySnapshot {
        self.transport.net_quality()
    }

    /// The connection this session rides, for opening another session
    /// on it (see `SshEngine::open_session_on`) or for parking a `Weak`
    /// in the app's reuse pool.
    pub fn transport(&self) -> &Arc<super::SshTransport> {
        &self.transport
    }

    /// How many owners this connection has, sessions plus whatever else
    /// holds it. `1` means this session is the last one and closing it
    /// takes the connection down with it, which is what the UI needs in
    /// order to say whether a tab shares its link.
    pub fn transport_owners(&self) -> usize {
        Arc::strong_count(&self.transport)
    }

    /// The terminfo fallback applied when the PTY was requested, if the
    /// host turned out to lack the configured `TERM` entry (issue #88).
    /// The UI surfaces it so the user knows why `TERM` differs and can
    /// change the host's Terminal Type for good.
    pub fn term_fallback(&self) -> Option<&TermFallback> {
        self.term_fallback.as_ref()
    }

    /// Tear the session down. Idempotent: only the first call acts.
    ///
    /// Aborts the reader / writer tasks. Aborting the reader task drops
    /// the output channel sender, so the app-side output stream ends
    /// cleanly (recv returns `None`) instead of hanging on a dead
    /// session.
    pub fn close(&self) {
        use std::sync::atomic::Ordering;
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        self.reader_task.abort();
        self.writer_task.abort();
        // The CONNECTION is deliberately not touched here, and neither
        // are the per-host forward listeners, which ride it. Both
        // belong to `SshTransport`, which disconnects (and releases the
        // listeners) when its last owner drops it, and this session may
        // not be the last: a second tab reusing the same link, or an
        // SFTP surface still mounted on it, must survive this one
        // closing, forwards included. Dropping our `Arc` (which
        // happens right after, when the session itself is dropped) is
        // how this session says it is done with the link.
    }

    /// Detect the remote OS by executing a silent probe on a side channel
    /// (no output goes to the user's PTY). Parses `/etc/os-release` for
    /// Linux; falls back to `uname -s` for non-Linux (Darwin, FreeBSD…).
    ///
    /// Returns `Some("ubuntu" | "debian" | "alpine" | "rhel" | "fedora" |
    /// "arch" | "amzn" | "centos" | "rocky" | "alma" | "darwin" | "freebsd"
    /// | "openbsd" | "netbsd")` or `None` on any parse / channel failure.
    pub async fn detect_os(&self) -> Option<String> {
        let cmd = "cat /etc/os-release 2>/dev/null; echo '---OXYXIS-SEP---'; uname -s";
        let handle = self.transport.handle().lock().await;
        let mut channel = handle.channel_open_session().await.ok()?;
        channel.exec(true, cmd).await.ok()?;
        drop(handle); // release so other tasks can use the shared handle

        let mut stdout = Vec::new();
        let collect = async {
            loop {
                match channel.wait().await {
                    Some(russh::ChannelMsg::Data { data }) => stdout.extend_from_slice(&data),
                    Some(russh::ChannelMsg::Eof) | Some(russh::ChannelMsg::ExitStatus { .. }) | None => break,
                    _ => {}
                }
            }
        };
        if tokio::time::timeout(std::time::Duration::from_secs(6), collect).await.is_err() {
            return None;
        }

        let text = String::from_utf8_lossy(&stdout);
        let mut parts = text.split("---OXYXIS-SEP---");
        let os_release = parts.next().unwrap_or("");
        let uname_s = parts.next().unwrap_or("").trim();

        // Try /etc/os-release first: `ID=ubuntu` (may be quoted).
        for line in os_release.lines() {
            if let Some(rest) = line.strip_prefix("ID=") {
                let id = rest.trim().trim_matches('"').trim_matches('\'').to_lowercase();
                if !id.is_empty() { return Some(id); }
            }
        }
        // Fallback: uname -s → darwin / freebsd / openbsd / netbsd / linux.
        let u = uname_s.to_lowercase();
        if !u.is_empty() && u != "linux" { return Some(u); }
        None
    }
}

impl Drop for SshSession {
    fn drop(&mut self) {
        // Backstop: an SshSession dropped without an explicit close()
        // must not leak its tokio tasks, the live SSH connection, or
        // any bound port-forward listeners.
        self.close();
    }
}
