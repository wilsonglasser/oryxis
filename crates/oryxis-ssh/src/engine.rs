use std::sync::Arc;

use russh::keys::{PublicKey, HashAlg, PrivateKeyWithHashAlg};
use russh::{client, ChannelMsg};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::process::Command as TokioCommand;
use tokio::sync::mpsc;

use oryxis_core::models::connection::{AuthMethod, Connection, PortForward, ProxyConfig, ProxyType};
use oryxis_core::models::port_forward_rule::{ForwardKind, PortForwardRule};
use thiserror::Error;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum SshError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Authentication failed")]
    AuthFailed,

    #[error("Channel error: {0}")]
    Channel(String),

    #[error("Russh error: {0}")]
    Russh(#[from] russh::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Key error: {0}")]
    Key(String),

    #[error("Proxy error: {0}")]
    Proxy(String),

    #[error("Jump host error: {0}")]
    JumpHost(String),
}

/// Which SSH negotiation category had no common algorithm. Mirrors the
/// per-host override categories so the UI can expand exactly the right
/// one (or all) on a legacy-fallback retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NegCategory {
    Kex,
    HostKey,
    Cipher,
    Mac,
}

/// A "no common algorithm" handshake failure, surfaced structurally so
/// the app can offer "this server only speaks legacy X, connect anyway?"
/// rather than parsing an error string.
#[derive(Debug, Clone)]
pub struct NegotiationFailure {
    pub category: NegCategory,
    /// The algorithms the server offered for the failed category.
    pub server_offers: Vec<String>,
}

impl SshError {
    /// If this is a russh "no common algorithm" failure, return the
    /// failed category and what the server offered. Compression failures
    /// are not user-actionable here, so they map to `None`.
    pub fn negotiation_failure(&self) -> Option<NegotiationFailure> {
        let SshError::Russh(russh::Error::NoCommonAlgo { kind, theirs, .. }) = self else {
            return None;
        };
        let category = match kind {
            russh::AlgorithmKind::Kex => NegCategory::Kex,
            russh::AlgorithmKind::Key => NegCategory::HostKey,
            russh::AlgorithmKind::Cipher => NegCategory::Cipher,
            russh::AlgorithmKind::Mac => NegCategory::Mac,
            russh::AlgorithmKind::Compression => return None,
        };
        Some(NegotiationFailure {
            category,
            server_offers: theirs.clone(),
        })
    }
}

// ---------------------------------------------------------------------------
// Client handler
// ---------------------------------------------------------------------------

/// Result of checking a host key against known hosts.
#[derive(Debug, Clone)]
pub enum HostKeyStatus {
    /// Host is known and fingerprint matches, accept silently.
    Known,
    /// Host is known but fingerprint CHANGED, potential MITM.
    Changed { old_fingerprint: String },
    /// Host is not known, need to ask the user.
    Unknown,
}

/// Query about a host key that the UI must answer.
#[derive(Debug, Clone)]
pub struct HostKeyQuery {
    pub hostname: String,
    pub port: u16,
    pub key_type: String,
    pub fingerprint: String,
    pub status: HostKeyStatus,
}

/// Sync callback that checks known hosts and returns the status.
pub type HostKeyCheckCallback = Arc<dyn Fn(&str, u16, &str, &str) -> HostKeyStatus + Send + Sync>;

/// Channel for asking the UI to verify a host key. The UI sends `true` (accept) or `false` (reject).
pub type HostKeyAskSender = tokio::sync::mpsc::Sender<(HostKeyQuery, tokio::sync::oneshot::Sender<bool>)>;

/// A single keyboard-interactive prompt line. `prompt` is the raw label
/// the server sent (e.g. `"Password:"`, `"Verification code:"`) and must
/// be rendered verbatim, never translated. `echo` says whether the typed
/// answer should be visible (`true`) or masked (`false`).
#[derive(Debug, Clone)]
pub struct KbiPromptField {
    pub prompt: String,
    pub echo: bool,
}

/// A keyboard-interactive challenge round the UI must answer. `name` and
/// `instructions` are server-provided headers (e.g. `"Two-factor
/// authentication"`); both can be empty. One round can carry several
/// prompts (password + OTP, etc.).
#[derive(Debug, Clone)]
pub struct KbiQuery {
    pub name: String,
    pub instructions: String,
    pub prompts: Vec<KbiPromptField>,
}

/// Channel for asking the UI to answer a keyboard-interactive round. The
/// UI sends `Some(answers)` (one per prompt, in order) or `None` to
/// cancel the authentication.
pub type KbiAskSender =
    tokio::sync::mpsc::Sender<(KbiQuery, tokio::sync::oneshot::Sender<Option<Vec<String>>>)>;

pub(crate) struct ClientHandler {
    hostname: String,
    port: u16,
    host_key_check: Option<HostKeyCheckCallback>,
    host_key_ask_tx: Option<HostKeyAskSender>,
    /// Mirrors `SshEngine::agent_forwarding`. The handler uses it as a
    /// gate on `server_channel_open_agent_forward`, without an opt-in,
    /// inbound forward channels are rejected even if the server tries
    /// to open one.
    agent_forwarding: bool,
    /// When there is no UI ask channel (e.g. a port forward auto-started
    /// at boot, before any terminal exists), an unknown host key is
    /// *rejected* rather than blindly TOFU-accepted. Lets a backgrounded
    /// forward fail to off instead of silently trusting a new key.
    strict_host_key: bool,
    /// For remote (`-R`) forwards only: where to hand off inbound
    /// `forwarded-tcpip` channels the server opens. The drain task in
    /// `connect_forward` bridges each one to the rule's local target. When
    /// `None`, the handler drops such channels (we never asked for them).
    forwarded_channel_sink:
        Option<tokio::sync::mpsc::UnboundedSender<russh::Channel<russh::client::Msg>>>,
}

/// Extract the `IdentityAgent` value from an ssh_config-style fragment.
///
/// This is the *fallback* Pageant discovery path (`windows_agent_pipe`
/// enumerates the live pipe first). Pageant (PuTTY 0.81+) and KeePassXC
/// create a per-launch agent pipe whose name changes every run and can
/// publish it as an `IdentityAgent` line in a conf file (default
/// `%USERPROFILE%\.ssh\pageant.conf`, but `--openssh-config` may point
/// it elsewhere, which is why pipe enumeration is preferred). The value
/// may use forward slashes (`//./pipe/pageant.<user>.<guid>`); normalize
/// them to the backslash form the named-pipe client expects. Accepts
/// both the `Keyword Value` and `Keyword=Value` ssh_config spellings.
#[cfg(any(windows, test))]
fn parse_identity_agent(contents: &str) -> Option<String> {
    for raw in contents.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(split_at) = line.find(|c: char| c.is_whitespace() || c == '=') else {
            continue;
        };
        if !line[..split_at].eq_ignore_ascii_case("IdentityAgent") {
            continue;
        }
        let val = line[split_at..]
            .trim_start_matches(|c: char| c.is_whitespace() || c == '=')
            .trim()
            .trim_matches('"');
        if val.is_empty() {
            // Empty IdentityAgent value: keep scanning for a later valid
            // line rather than giving up on the whole file.
            continue;
        }
        return Some(val.replace('/', "\\"));
    }
    None
}

/// Pick the current user's live Pageant agent pipe from a list of
/// named-pipe names (as returned by enumerating `\\.\pipe\`).
///
/// Pageant (PuTTY 0.81+) / KeePassXC publish a per-launch pipe named
/// `pageant.<user>.<guid>` where `<guid>` is randomized every run. We
/// match the current user's entry and return the full `\\.\pipe\<name>`
/// path the named-pipe client expects. Matching is case-insensitive
/// (Win32 pipe names are), but the original name's casing is preserved
/// in the returned path.
///
/// When the user is unknown we fall back to any `pageant.<x>.<guid>`
/// shaped name (single-user machines, the common case), accepting the
/// small risk of another user's pipe over missing the keys entirely.
#[cfg(any(windows, test))]
fn pick_pageant_pipe(names: &[String], user: Option<&str>) -> Option<String> {
    let is_match = |name: &str| -> bool {
        let lower = name.to_ascii_lowercase();
        match user {
            Some(u) if !u.is_empty() => {
                // Trailing dot pins the user segment boundary so
                // `pageant.user.` never matches `pageant.user2.<guid>`.
                let prefix = format!("pageant.{}.", u.to_ascii_lowercase());
                lower.starts_with(&prefix) && lower.len() > prefix.len()
            }
            _ => {
                lower.starts_with("pageant.")
                    && lower.matches('.').count() >= 2
                    && !lower.ends_with('.')
            }
        }
    };
    names
        .iter()
        .find(|n| is_match(n))
        .map(|n| format!(r"\\.\pipe\{n}"))
}

/// Enumerate the Windows named-pipe namespace (`\\.\pipe\`), returning
/// the bare pipe names (without the `\\.\pipe\` prefix). Empty on any
/// failure, callers fall back to other discovery paths.
#[cfg(windows)]
fn list_named_pipes() -> Vec<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{
        FindClose, FindFirstFileW, FindNextFileW, WIN32_FIND_DATAW,
    };

    let pattern: Vec<u16> = std::ffi::OsStr::new(r"\\.\pipe\*")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut data: WIN32_FIND_DATAW = unsafe { std::mem::zeroed() };
    let handle = unsafe { FindFirstFileW(pattern.as_ptr(), &mut data) };
    if handle == INVALID_HANDLE_VALUE {
        return Vec::new();
    }
    let mut out = Vec::new();
    loop {
        let len = data
            .cFileName
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(data.cFileName.len());
        let name = String::from_utf16_lossy(&data.cFileName[..len]);
        if !name.is_empty() && name != "." && name != ".." {
            out.push(name);
        }
        if unsafe { FindNextFileW(handle, &mut data) } == 0 {
            break;
        }
    }
    unsafe {
        FindClose(handle);
    }
    out
}

/// Resolve the Windows ssh-agent named pipe to dial.
///
/// Discovery order:
/// 1. The live Pageant/KeePassXC pipe, found by enumerating the
///    named-pipe namespace (`pick_pageant_pipe`). Authoritative: no
///    config file, no per-launch path to chase (`--openssh-config` can
///    point anywhere), and never stale (a `pageant.conf` can name a
///    dead guid; the live pipe is ground truth). Works even when
///    Pageant was launched without `--openssh-config`, when no conf is
///    written at all.
/// 2. A published `pageant.conf` `IdentityAgent` line at the default
///    `%USERPROFILE%\.ssh\pageant.conf` (see `parse_identity_agent`).
/// 3. The fixed Windows OpenSSH agent pipe.
#[cfg(windows)]
fn windows_agent_pipe() -> String {
    const OPENSSH_PIPE: &str = r"\\.\pipe\openssh-ssh-agent";
    let user = std::env::var("USERNAME").ok();
    if let Some(pipe) = pick_pageant_pipe(&list_named_pipes(), user.as_deref()) {
        tracing::info!("Using live Pageant agent pipe {pipe}");
        return pipe;
    }
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        let conf = std::path::Path::new(&profile)
            .join(".ssh")
            .join("pageant.conf");
        if let Ok(contents) = std::fs::read_to_string(&conf)
            && let Some(pipe) = parse_identity_agent(&contents)
        {
            tracing::info!("Using IdentityAgent pipe from {}", conf.display());
            return pipe;
        }
    }
    OPENSSH_PIPE.to_string()
}

/// Bridge an inbound agent-forward channel to the local ssh-agent so
/// the remote side can use the keys held by our local agent. The remote
/// app speaks ssh-agent protocol over the channel; we just shovel raw
/// bytes between the channel and the local socket / pipe.
#[cfg(unix)]
async fn bridge_agent_channel(
    channel: russh::Channel<russh::client::Msg>,
) -> std::io::Result<()> {
    let path = std::env::var_os("SSH_AUTH_SOCK").ok_or_else(|| {
        std::io::Error::other("agent forwarding requested but SSH_AUTH_SOCK is not set")
    })?;
    let mut agent = tokio::net::UnixStream::connect(&path).await?;
    let mut stream = channel.into_stream();
    let _ = tokio::io::copy_bidirectional(&mut agent, &mut stream).await?;
    Ok(())
}

#[cfg(windows)]
async fn bridge_agent_channel(
    channel: russh::Channel<russh::client::Msg>,
) -> std::io::Result<()> {
    let pipe_path = windows_agent_pipe();
    let mut agent = tokio::net::windows::named_pipe::ClientOptions::new().open(&pipe_path)?;
    let mut stream = channel.into_stream();
    let _ = tokio::io::copy_bidirectional(&mut agent, &mut stream).await?;
    Ok(())
}

impl ClientHandler {
    /// Test-only constructor: a handler that trusts any server key (no
    /// callback, non-strict) so the in-process harness in `sftp_harness`
    /// can build a `Handle<ClientHandler>` over a duplex stream. Fields
    /// are private to this module, so the harness can't build one itself.
    #[cfg(test)]
    pub(crate) fn test_accept_all() -> Self {
        ClientHandler {
            hostname: "harness".into(),
            port: 22,
            host_key_check: None,
            host_key_ask_tx: None,
            agent_forwarding: false,
            strict_host_key: false,
            forwarded_channel_sink: None,
        }
    }
}

impl client::Handler for ClientHandler {
    type Error = SshError;

    async fn check_server_key(&mut self, key: &PublicKey) -> Result<bool, Self::Error> {
        let key_type = key.algorithm().to_string();
        let fingerprint = key.fingerprint(russh::keys::ssh_key::HashAlg::Sha256).to_string();

        tracing::info!(
            "Server key for {}:{}, {} {}",
            self.hostname, self.port, key_type, fingerprint
        );

        let status = if let Some(ref cb) = self.host_key_check {
            cb(&self.hostname, self.port, &key_type, &fingerprint)
        } else {
            HostKeyStatus::Unknown
        };

        match status {
            HostKeyStatus::Known => Ok(true),
            HostKeyStatus::Changed { .. } | HostKeyStatus::Unknown => {
                // Ask the UI
                if let Some(ref tx) = self.host_key_ask_tx {
                    let query = HostKeyQuery {
                        hostname: self.hostname.clone(),
                        port: self.port,
                        key_type,
                        fingerprint,
                        status,
                    };
                    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
                    if tx.send((query, resp_tx)).await.is_err() {
                        return Ok(false);
                    }
                    Ok(resp_rx.await.unwrap_or(false))
                } else if self.strict_host_key {
                    // No UI channel and strict: reject both changed and
                    // unknown so a backgrounded forward never TOFU-trusts.
                    Ok(false)
                } else {
                    // No UI channel, reject changed, accept unknown (legacy fallback)
                    Ok(matches!(status, HostKeyStatus::Unknown))
                }
            }
        }
    }

    async fn server_channel_open_agent_forward(
        &mut self,
        channel: russh::Channel<russh::client::Msg>,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        if !self.agent_forwarding {
            // Server is trying to open a forward channel we never asked
            // for. Drop it on the floor, `Channel` is closed when it
            // goes out of scope.
            tracing::warn!(
                "rejecting unsolicited agent-forward channel from {}:{}",
                self.hostname,
                self.port
            );
            return Ok(());
        }
        tokio::spawn(async move {
            if let Err(e) = bridge_agent_channel(channel).await {
                tracing::warn!("agent-forward bridge ended: {e}");
            }
        });
        Ok(())
    }

    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: russh::Channel<russh::client::Msg>,
        connected_address: &str,
        connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        // Inbound channel for a remote (`-R`) forward. Hand it to the drain
        // task that bridges to the local target; if we have no sink, we
        // never requested a remote forward, so drop it.
        match &self.forwarded_channel_sink {
            Some(sink) => {
                if sink.send(channel).is_err() {
                    tracing::warn!("forwarded-tcpip drain gone, dropping channel");
                }
            }
            None => {
                tracing::warn!(
                    "rejecting unsolicited forwarded-tcpip channel for {}:{}",
                    connected_address,
                    connected_port
                );
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// SSH Handle (opaque wrapper for step-by-step connection)
// ---------------------------------------------------------------------------

/// Opaque handle to an SSH connection after transport is established.
/// Used between `establish_transport` and `do_authenticate` / `open_session`.
pub struct SshHandle(client::Handle<ClientHandler>);

pub(crate) type SharedHandle = Arc<tokio::sync::Mutex<client::Handle<ClientHandler>>>;

/// Bind local TCP listeners for port forwards, validating all ports upfront.
/// Returns the bound listeners (actual forwarding starts after PTY session opens).
async fn bind_port_forward_listeners(
    forwards: &[PortForward],
) -> Result<Vec<(PortForward, tokio::net::TcpListener)>, SshError> {
    use tokio::net::TcpListener;
    let mut listeners = Vec::new();
    for fwd in forwards {
        let listener = TcpListener::bind(("127.0.0.1", fwd.local_port))
            .await
            .map_err(|e| SshError::Channel(format!(
                "Failed to bind local port {}: {}", fwd.local_port, e
            )))?;
        tracing::info!(
            "Port forward: 127.0.0.1:{} -> {}:{}",
            fwd.local_port, fwd.remote_host, fwd.remote_port
        );
        listeners.push((fwd.clone(), listener));
    }
    Ok(listeners)
}

/// Spawn listener tasks that bridge local TCP connections to remote hosts via SSH.
fn spawn_port_forward_tasks(
    listeners: Vec<(PortForward, tokio::net::TcpListener)>,
    handle: &SharedHandle,
) -> Vec<tokio::task::JoinHandle<()>> {
    let mut tasks = Vec::new();
    for (fwd, listener) in listeners {
        let shared = Arc::clone(handle);
        let remote_host = fwd.remote_host;
        let remote_port = fwd.remote_port;
        let local_port = fwd.local_port;

        let task = tokio::spawn(async move {
            loop {
                let (stream, addr) = match listener.accept().await {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::error!("Port forward accept error on {}: {}", local_port, e);
                        break;
                    }
                };
                tracing::debug!("Port forward {} accepted from {}", local_port, addr);

                let shared = Arc::clone(&shared);
                let remote_host = remote_host.clone();
                tokio::spawn(async move {
                    let channel: russh::Channel<russh::client::Msg> = {
                        let handle = shared.lock().await;
                        match handle.channel_open_direct_tcpip(
                            remote_host.clone(),
                            remote_port as u32,
                            "127.0.0.1",
                            local_port as u32,
                        ).await {
                            Ok(ch) => ch,
                            Err(e) => {
                                tracing::error!(
                                    "direct-tcpip to {}:{} failed: {}",
                                    remote_host, remote_port, e
                                );
                                return;
                            }
                        }
                    };

                    let channel_stream = channel.into_stream();
                    let (mut ch_reader, mut ch_writer) = tokio::io::split(channel_stream);
                    let (mut tcp_reader, mut tcp_writer) = tokio::io::split(stream);

                    let c2t = tokio::io::copy(&mut ch_reader, &mut tcp_writer);
                    let t2c = tokio::io::copy(&mut tcp_reader, &mut ch_writer);

                    tokio::select! {
                        r = c2t => { if let Err(e) = r { tracing::debug!("port fwd channel->tcp: {}", e); } }
                        r = t2c => { if let Err(e) = r { tracing::debug!("port fwd tcp->channel: {}", e); } }
                    }
                });
            }
        });
        tasks.push(task);
    }
    tasks
}

// ---------------------------------------------------------------------------
// SSH Session
// ---------------------------------------------------------------------------

/// Result of a non-interactive command execution.
pub struct ExecResult {
    pub exit_code: u32,
    pub stdout: String,
    pub stderr: String,
}

/// A live SSH session with a remote PTY channel.
pub struct SshSession {
    /// Shared SSH handle, kept alive for port forward tasks to open channels.
    _handle: Arc<tokio::sync::Mutex<client::Handle<ClientHandler>>>,
    writer_tx: mpsc::UnboundedSender<Vec<u8>>,
    /// Forwarded to the SSH channel as `window-change` requests so the
    /// remote shell sees SIGWINCH and re-renders for the new viewport.
    /// Without this, apps like `top` keep rendering for the original
    /// columns and our local alacritty wraps the overflow into extra
    /// rows ("double line" effect).
    resize_tx: mpsc::UnboundedSender<(u16, u16)>,
    reader_task: tokio::task::JoinHandle<()>,
    writer_task: tokio::task::JoinHandle<()>,
    port_forward_tasks: Vec<tokio::task::JoinHandle<()>>,
    /// Latched by `close()` so teardown runs exactly once even when both
    /// an explicit close and the `Drop` backstop fire.
    closed: std::sync::atomic::AtomicBool,
    /// Cap on how long `open_sftp` (and the per-sibling open in the
    /// transfer pool) wait before giving up. Set by `SshEngine`'s
    /// builder so the user can tune it from the SFTP settings panel.
    pub(crate) sftp_open_timeout: std::time::Duration,
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

    /// Open a fresh SFTP subsystem channel on this session, the SSH
    /// connection multiplexes, so the original PTY channel keeps running.
    /// Wrapped in the engine-configured timeout to keep `open_sftp` from
    /// hanging the UI when a server doesn't speak the sftp subsystem.
    pub async fn open_sftp(&self) -> Result<crate::sftp::SftpClient, SshError> {
        let timeout = self.sftp_open_timeout;
        let handle_for_exec = self._handle.clone();
        let inner = async {
            let handle = self._handle.lock().await;
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

    pub fn is_alive(&self) -> bool {
        !self.writer_tx.is_closed()
    }

    /// Tear the session down. Idempotent: only the first call acts.
    ///
    /// Aborts the reader / writer / port-forward tasks (releasing any
    /// locally bound `-L` listeners) and disconnects the underlying SSH
    /// connection so the remote side tears its half down too. Aborting
    /// the reader task drops the output channel sender, so the app-side
    /// output stream ends cleanly (recv returns `None`) instead of
    /// hanging on a dead session.
    pub fn close(&self) {
        use std::sync::atomic::Ordering;
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        self.reader_task.abort();
        self.writer_task.abort();
        for task in &self.port_forward_tasks {
            task.abort();
        }
        // Politely disconnect the transport. Needs a runtime to spawn
        // on; when close() runs outside one (e.g. a late Drop during
        // process shutdown) the aborts above already killed the tasks
        // and the TCP socket dies with the last handle clone.
        if let Ok(rt) = tokio::runtime::Handle::try_current() {
            let handle = Arc::clone(&self._handle);
            rt.spawn(async move {
                let h = handle.lock().await;
                let _ = h
                    .disconnect(russh::Disconnect::ByApplication, "session closed", "")
                    .await;
            });
        }
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
        let handle = self._handle.lock().await;
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

// ---------------------------------------------------------------------------
// Port forward session (no PTY)
// ---------------------------------------------------------------------------

/// A live port forward held open by a dedicated SSH connection, with no PTY
/// or shell. Created by `SshEngine::connect_forward` and kept alive by the
/// app's runtime registry until the rule is toggled off.
///
/// Cancellation is explicit, never "drop the JoinHandle" (which would detach
/// the accept loop and leave the listener bound). The `cancel` watch channel
/// is selected on by the accept loop and every in-flight bridge; dropping the
/// `ForwardSession` drops the sender, which also fires cancellation, so
/// removing it from the registry tears the tunnel down cleanly.
pub struct ForwardSession {
    handle: SharedHandle,
    cancel_tx: tokio::sync::watch::Sender<bool>,
    _tasks: Vec<tokio::task::JoinHandle<()>>,
    /// For `-R` only: the server-side bind that must be released with
    /// `cancel_tcpip_forward` on stop. `None` for `-L` / `-D`.
    remote_bind: Option<(String, u16)>,
}

impl ForwardSession {
    /// Whether the underlying SSH connection is still up. Uses `try_lock` so
    /// the liveness poll never blocks behind an in-flight bridge (a busy lock
    /// means the connection is being used, i.e. alive).
    pub fn is_alive(&self) -> bool {
        match self.handle.try_lock() {
            Ok(h) => !h.is_closed(),
            Err(_) => true,
        }
    }

    /// Stop the forward: signal cancellation to all tasks and, for `-R`,
    /// ask the server to release its listener. Idempotent.
    pub async fn cancel(&self) {
        let _ = self.cancel_tx.send(true);
        if let Some((host, port)) = &self.remote_bind {
            let handle = self.handle.lock().await;
            let _ = handle.cancel_tcpip_forward(host.clone(), *port as u32).await;
        }
    }
}

impl Drop for ForwardSession {
    fn drop(&mut self) {
        // Best-effort: fire cancellation so the accept loop and bridges stop
        // even if `cancel()` was never awaited. The `-R` server-side release
        // needs an await, so callers that care should `cancel().await` first.
        let _ = self.cancel_tx.send(true);
    }
}

impl std::fmt::Debug for ForwardSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ForwardSession")
            .field("alive", &self.is_alive())
            .finish()
    }
}

/// Bind a TCP listener for a forward, honouring the rule's `listen_host`
/// (e.g. `0.0.0.0` to expose a `-D`/`-L` listener on the LAN).
async fn bind_forward_listener(
    listen_host: &str,
    listen_port: u16,
) -> Result<tokio::net::TcpListener, SshError> {
    tokio::net::TcpListener::bind((listen_host, listen_port))
        .await
        .map_err(|e| SshError::Channel(format!(
            "Failed to bind {}:{}: {}", listen_host, listen_port, e
        )))
}

/// Spawn a cancel-aware accept loop for a `-L` forward. Each accepted
/// connection opens a `direct-tcpip` channel to `target_host:target_port`
/// and bridges bytes until either side closes or cancellation fires.
fn spawn_local_forward_task(
    listener: tokio::net::TcpListener,
    handle: SharedHandle,
    target_host: String,
    target_port: u16,
    listen_port: u16,
    cancel: tokio::sync::watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut cancel = cancel;
        loop {
            let (stream, addr) = tokio::select! {
                _ = cancel.changed() => break,
                res = listener.accept() => match res {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::error!("forward accept error on {}: {}", listen_port, e);
                        break;
                    }
                },
            };
            tracing::debug!("forward {} accepted from {}", listen_port, addr);

            let shared = Arc::clone(&handle);
            let target_host = target_host.clone();
            let child_cancel = cancel.clone();
            tokio::spawn(async move {
                bridge_direct_tcpip(
                    shared, stream, target_host, target_port, listen_port, child_cancel,
                )
                .await;
            });
        }
        tracing::debug!("forward accept loop on {} stopped", listen_port);
    })
}

/// Open a `direct-tcpip` channel to `target_host:target_port` and pump bytes
/// between it and `stream`, stopping on EOF, error, or cancellation.
async fn bridge_direct_tcpip(
    shared: SharedHandle,
    stream: tokio::net::TcpStream,
    target_host: String,
    target_port: u16,
    src_port: u16,
    mut cancel: tokio::sync::watch::Receiver<bool>,
) {
    let channel: russh::Channel<russh::client::Msg> = {
        let handle = shared.lock().await;
        match handle
            .channel_open_direct_tcpip(
                target_host.clone(),
                target_port as u32,
                "127.0.0.1",
                src_port as u32,
            )
            .await
        {
            Ok(ch) => ch,
            Err(e) => {
                tracing::error!("direct-tcpip to {}:{} failed: {}", target_host, target_port, e);
                return;
            }
        }
    };

    let channel_stream = channel.into_stream();
    let (mut ch_reader, mut ch_writer) = tokio::io::split(channel_stream);
    let (mut tcp_reader, mut tcp_writer) = tokio::io::split(stream);

    let c2t = tokio::io::copy(&mut ch_reader, &mut tcp_writer);
    let t2c = tokio::io::copy(&mut tcp_reader, &mut ch_writer);

    tokio::select! {
        _ = cancel.changed() => {}
        r = c2t => { if let Err(e) = r { tracing::debug!("forward channel->tcp: {}", e); } }
        r = t2c => { if let Err(e) = r { tracing::debug!("forward tcp->channel: {}", e); } }
    }
}

/// Bridge an inbound `forwarded-tcpip` channel (from a `-R` forward) to a
/// local TCP target, pumping bytes until EOF, error, or cancellation. The
/// target here is reached from *this* client, the opposite direction of a
/// `-L` forward.
async fn bridge_channel_to_target(
    channel: russh::Channel<russh::client::Msg>,
    target_host: String,
    target_port: u16,
    mut cancel: tokio::sync::watch::Receiver<bool>,
) {
    let stream = match tokio::net::TcpStream::connect((target_host.as_str(), target_port)).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(
                "remote forward target {}:{} unreachable: {}",
                target_host, target_port, e
            );
            return;
        }
    };

    let channel_stream = channel.into_stream();
    let (mut ch_reader, mut ch_writer) = tokio::io::split(channel_stream);
    let (mut tcp_reader, mut tcp_writer) = tokio::io::split(stream);

    let c2t = tokio::io::copy(&mut ch_reader, &mut tcp_writer);
    let t2c = tokio::io::copy(&mut tcp_reader, &mut ch_writer);

    tokio::select! {
        _ = cancel.changed() => {}
        r = c2t => { if let Err(e) = r { tracing::debug!("remote forward channel->tcp: {}", e); } }
        r = t2c => { if let Err(e) = r { tracing::debug!("remote forward tcp->channel: {}", e); } }
    }
}

/// Spawn a cancel-aware accept loop for a `-D` dynamic forward. The local
/// listener speaks SOCKS5; each accepted connection negotiates a CONNECT
/// target and gets its own `direct-tcpip` channel through the SSH session.
fn spawn_dynamic_forward_task(
    listener: tokio::net::TcpListener,
    handle: SharedHandle,
    listen_port: u16,
    cancel: tokio::sync::watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut cancel = cancel;
        loop {
            let (stream, addr) = tokio::select! {
                _ = cancel.changed() => break,
                res = listener.accept() => match res {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::error!("socks5 accept error on {}: {}", listen_port, e);
                        break;
                    }
                },
            };
            tracing::debug!("socks5 {} accepted from {}", listen_port, addr);
            let shared = Arc::clone(&handle);
            let child_cancel = cancel.clone();
            tokio::spawn(async move {
                bridge_socks5(shared, stream, listen_port, child_cancel).await;
            });
        }
        tracing::debug!("socks5 accept loop on {} stopped", listen_port);
    })
}

/// Write a SOCKS5 reply with the given reply code and a zeroed
/// IPv4 bind address (the client ignores it for CONNECT).
async fn socks5_reply(
    stream: &mut tokio::net::TcpStream,
    rep: u8,
) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;
    stream
        .write_all(&[0x05, rep, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await
}

/// Run the SOCKS5 server handshake (no-auth, CONNECT only) and return the
/// requested destination. Sends the appropriate failure reply itself for
/// the cases it rejects.
async fn socks5_negotiate(
    stream: &mut tokio::net::TcpStream,
) -> std::io::Result<(String, u16)> {
    use std::io::{Error, ErrorKind};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // Greeting: VER, NMETHODS, METHODS[NMETHODS].
    let mut head = [0u8; 2];
    stream.read_exact(&mut head).await?;
    if head[0] != 0x05 {
        return Err(Error::new(ErrorKind::InvalidData, "not a SOCKS5 client"));
    }
    let mut methods = vec![0u8; head[1] as usize];
    stream.read_exact(&mut methods).await?;
    // We only support "no authentication required" (0x00).
    if !methods.contains(&0x00) {
        stream.write_all(&[0x05, 0xFF]).await?;
        return Err(Error::other("no acceptable SOCKS5 method"));
    }
    stream.write_all(&[0x05, 0x00]).await?;

    // Request: VER CMD RSV ATYP DST.ADDR DST.PORT.
    let mut req = [0u8; 4];
    stream.read_exact(&mut req).await?;
    if req[0] != 0x05 {
        return Err(Error::new(ErrorKind::InvalidData, "bad SOCKS5 request"));
    }
    if req[1] != 0x01 {
        // Only CONNECT (0x01); reject BIND / UDP ASSOCIATE.
        socks5_reply(stream, 0x07).await?;
        return Err(Error::other("SOCKS5 command not supported"));
    }
    let host = match req[3] {
        0x01 => {
            let mut a = [0u8; 4];
            stream.read_exact(&mut a).await?;
            std::net::Ipv4Addr::from(a).to_string()
        }
        0x03 => {
            let mut len = [0u8; 1];
            stream.read_exact(&mut len).await?;
            let mut d = vec![0u8; len[0] as usize];
            stream.read_exact(&mut d).await?;
            String::from_utf8_lossy(&d).into_owned()
        }
        0x04 => {
            let mut a = [0u8; 16];
            stream.read_exact(&mut a).await?;
            std::net::Ipv6Addr::from(a).to_string()
        }
        _ => {
            socks5_reply(stream, 0x08).await?;
            return Err(Error::other("SOCKS5 address type not supported"));
        }
    };
    let mut port = [0u8; 2];
    stream.read_exact(&mut port).await?;
    Ok((host, u16::from_be_bytes(port)))
}

/// Handle one SOCKS5 client: negotiate the target, open a `direct-tcpip`
/// channel to it, reply, then relay bytes until EOF / error / cancellation.
async fn bridge_socks5(
    shared: SharedHandle,
    mut stream: tokio::net::TcpStream,
    src_port: u16,
    mut cancel: tokio::sync::watch::Receiver<bool>,
) {
    let (dest_host, dest_port) = match socks5_negotiate(&mut stream).await {
        Ok(d) => d,
        Err(e) => {
            tracing::debug!("socks5 negotiate failed: {}", e);
            return;
        }
    };

    let channel = {
        let handle = shared.lock().await;
        handle
            .channel_open_direct_tcpip(
                dest_host.clone(),
                dest_port as u32,
                "127.0.0.1",
                src_port as u32,
            )
            .await
    };
    let channel = match channel {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("socks5 direct-tcpip to {}:{} failed: {}", dest_host, dest_port, e);
            // 0x05 = connection refused / general failure.
            let _ = socks5_reply(&mut stream, 0x05).await;
            return;
        }
    };
    if socks5_reply(&mut stream, 0x00).await.is_err() {
        return;
    }

    let channel_stream = channel.into_stream();
    let (mut ch_reader, mut ch_writer) = tokio::io::split(channel_stream);
    let (mut tcp_reader, mut tcp_writer) = tokio::io::split(stream);
    let c2t = tokio::io::copy(&mut ch_reader, &mut tcp_writer);
    let t2c = tokio::io::copy(&mut tcp_reader, &mut ch_writer);

    tokio::select! {
        _ = cancel.changed() => {}
        r = c2t => { if let Err(e) = r { tracing::debug!("socks5 channel->tcp: {}", e); } }
        r = t2c => { if let Err(e) = r { tracing::debug!("socks5 tcp->channel: {}", e); } }
    }
}

// ---------------------------------------------------------------------------
// SSH Engine
// ---------------------------------------------------------------------------

/// Resolves connections for jump hosts.
pub struct ConnectionResolver {
    pub connections: Vec<Connection>,
    pub passwords: std::collections::HashMap<uuid::Uuid, String>,
    pub private_keys: std::collections::HashMap<uuid::Uuid, String>,
    /// Effective proxy per jump-host id, hydrated by the caller via
    /// `Vault::resolve_proxy`. Only the first jump's entry is used
    /// subsequent hops travel inside an SSH-tunneled `direct-tcpip`
    /// channel where a proxy doesn't apply.
    pub proxies: std::collections::HashMap<uuid::Uuid, ProxyConfig>,
}

pub struct SshEngine {
    host_key_check: Option<HostKeyCheckCallback>,
    host_key_ask_tx: Option<HostKeyAskSender>,
    /// Optional channel for surfacing keyboard-interactive prompts to the
    /// UI (set only for `AuthMethod::Interactive` on the terminal path).
    /// When absent, interactive auth degrades to filling every prompt with
    /// the stored password, so headless callers (boot port forwards) still
    /// work without a modal.
    kbi_ask_tx: Option<KbiAskSender>,
    /// Optional client-side keepalive: when set, russh sends a no-op
    /// SSH_MSG_GLOBAL_REQUEST every N seconds so NAT / firewall idle
    /// timeouts don't kill the session.
    keepalive_interval: Option<std::time::Duration>,
    /// Phase-by-phase timeouts. Each step of the connect ladder gets
    /// its own bound so a misbehaving server can't hang the UI on any
    /// single stage. Defaults are sane (15s/30s/10s) and the user can
    /// override via the SFTP settings panel.
    connect_timeout: std::time::Duration,
    auth_timeout: std::time::Duration,
    session_timeout: std::time::Duration,
    /// Forward the local ssh-agent socket to the remote shell. Off by
    /// default (matches OpenSSH's default `ForwardAgent no`). Enabled
    /// per-connection from the editor; relayed both to the channel-
    /// level `auth-agent-req@openssh.com` request *and* to
    /// `ClientHandler` so we only accept forward channels we asked for.
    agent_forwarding: bool,
    /// Per-host environment variables sent via `set_env` before the shell
    /// starts. `(name, value)` pairs. Non-fatal: most `sshd` only accept
    /// `LC_*` / `LANG_*` unless `AcceptEnv` is widened.
    env_vars: Vec<(String, String)>,
    /// Per-host character encoding label (e.g. `"Big5"`, `"Shift_JIS"`).
    /// `None` or UTF-8 means no transcoding (the terminal is UTF-8); any
    /// other charset is decoded to UTF-8 on the way in and encoded back on
    /// the way out.
    encoding: Option<String>,
    /// Per-host `TERM` name sent when requesting the PTY. `None` =
    /// `xterm-256color`. See `Connection.terminal_type`.
    terminal_type: Option<String>,
    /// Per-host SSH algorithm overrides (legacy-cipher support). Each
    /// `None` keeps russh's safe `Preferred` default for that category;
    /// `Some(list)` pins exactly those wire names (unknown names dropped).
    /// See `Connection.{ciphers,kex,macs,host_key_algorithms}`.
    algo_ciphers: Option<Vec<String>>,
    algo_kex: Option<Vec<String>>,
    algo_macs: Option<Vec<String>>,
    algo_host_keys: Option<Vec<String>>,
    /// Reject unknown/changed host keys when no UI ask channel is set
    /// (used by boot auto-started port forwards). See
    /// `ClientHandler::strict_host_key`.
    strict_host_key: bool,
    /// Set only for remote (`-R`) forwards. Propagated to the handler so
    /// inbound `forwarded-tcpip` channels reach the drain task. See
    /// `ClientHandler::forwarded_channel_sink`.
    forwarded_channel_sink:
        Option<tokio::sync::mpsc::UnboundedSender<russh::Channel<russh::client::Msg>>>,
}

impl Default for SshEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl SshEngine {
    pub fn new() -> Self {
        Self {
            host_key_check: None,
            host_key_ask_tx: None,
            kbi_ask_tx: None,
            keepalive_interval: None,
            connect_timeout: std::time::Duration::from_secs(15),
            auth_timeout: std::time::Duration::from_secs(30),
            session_timeout: std::time::Duration::from_secs(10),
            agent_forwarding: false,
            env_vars: Vec::new(),
            encoding: None,
            terminal_type: None,
            algo_ciphers: None,
            algo_kex: None,
            algo_macs: None,
            algo_host_keys: None,
            strict_host_key: false,
            forwarded_channel_sink: None,
        }
    }

    /// Pin per-host SSH algorithm overrides. Each `None` keeps russh's
    /// safe default for that category; `Some(list)` forces exactly those
    /// wire names (in order). Used to reach legacy servers that only offer
    /// cbc / sha1 / dh-group1.
    pub fn with_algorithm_overrides(
        mut self,
        ciphers: Option<Vec<String>>,
        kex: Option<Vec<String>>,
        macs: Option<Vec<String>>,
        host_keys: Option<Vec<String>>,
    ) -> Self {
        self.algo_ciphers = ciphers.filter(|v| !v.is_empty());
        self.algo_kex = kex.filter(|v| !v.is_empty());
        self.algo_macs = macs.filter(|v| !v.is_empty());
        self.algo_host_keys = host_keys.filter(|v| !v.is_empty());
        self
    }

    /// Build the russh `Preferred` algorithm set, starting from the safe
    /// default and overriding only the pinned categories. When every
    /// override is `None` the result is byte-identical to the default, so
    /// the secure negotiation is untouched unless the user opts in.
    fn build_preferred(&self) -> russh::Preferred {
        use std::borrow::Cow;
        let mut p = russh::Preferred::DEFAULT;
        if let Some(list) = &self.algo_ciphers {
            let names: Vec<russh::cipher::Name> = list
                .iter()
                .filter_map(|s| russh::cipher::Name::try_from(s.as_str()).ok())
                .collect();
            if !names.is_empty() {
                p.cipher = Cow::Owned(names);
            }
        }
        if let Some(list) = &self.algo_kex {
            let names: Vec<russh::kex::Name> = list
                .iter()
                .filter_map(|s| russh::kex::Name::try_from(s.as_str()).ok())
                .collect();
            if !names.is_empty() {
                p.kex = Cow::Owned(names);
            }
        }
        if let Some(list) = &self.algo_macs {
            let names: Vec<russh::mac::Name> = list
                .iter()
                .filter_map(|s| russh::mac::Name::try_from(s.as_str()).ok())
                .collect();
            if !names.is_empty() {
                p.mac = Cow::Owned(names);
            }
        }
        if let Some(list) = &self.algo_host_keys {
            let algos: Vec<russh::keys::ssh_key::Algorithm> = list
                .iter()
                .filter_map(|s| s.parse().ok())
                .collect();
            if !algos.is_empty() {
                p.key = Cow::Owned(algos);
            }
        }
        p
    }

    /// Reject unknown/changed host keys instead of TOFU-accepting when no
    /// UI ask channel is set. Used for port forwards auto-started at boot,
    /// where there is no terminal to surface a host-key prompt.
    pub fn with_strict_host_key(mut self, enabled: bool) -> Self {
        self.strict_host_key = enabled;
        self
    }

    /// Set per-host environment variables to send (via `set_env`) before
    /// the shell starts on the next session opened on this engine.
    pub fn with_env_vars(mut self, vars: Vec<(String, String)>) -> Self {
        self.env_vars = vars;
        self
    }

    /// Set the per-host character encoding. `None` / `"UTF-8"` leaves the
    /// byte stream untouched; any other label transcodes PTY data to and
    /// from UTF-8 for the terminal.
    pub fn with_encoding(mut self, encoding: Option<String>) -> Self {
        self.encoding = encoding;
        self
    }

    /// Override the `TERM` name requested for the PTY. `None` keeps the
    /// default `xterm-256color`.
    pub fn with_terminal_type(mut self, terminal_type: Option<String>) -> Self {
        self.terminal_type = terminal_type;
        self
    }

    /// Enable ssh-agent forwarding for the next session opened on this
    /// engine. The flag is propagated to the channel-open request and
    /// to the inbound forward-channel handler so we don't proxy
    /// channels we didn't ask for.
    pub fn with_agent_forwarding(mut self, enabled: bool) -> Self {
        self.agent_forwarding = enabled;
        self
    }

    /// Override the TCP/SSH-handshake timeout (default 15s).
    pub fn with_connect_timeout(mut self, t: std::time::Duration) -> Self {
        self.connect_timeout = t;
        self
    }

    /// Override the authentication-phase timeout (default 30s).
    pub fn with_auth_timeout(mut self, t: std::time::Duration) -> Self {
        self.auth_timeout = t;
        self
    }

    /// Override the session/SFTP-channel-open timeout (default 10s).
    /// Applies to PTY session open, SFTP subsystem open, and sibling
    /// channel opens for the parallel transfer pool.
    pub fn with_session_timeout(mut self, t: std::time::Duration) -> Self {
        self.session_timeout = t;
        self
    }

    /// Set a sync callback that checks known hosts and returns the status.
    pub fn with_host_key_check(mut self, cb: HostKeyCheckCallback) -> Self {
        self.host_key_check = Some(cb);
        self
    }

    /// Set a channel for asking the UI to verify unknown/changed host keys.
    pub fn with_host_key_ask(mut self, tx: HostKeyAskSender) -> Self {
        self.host_key_ask_tx = Some(tx);
        self
    }

    /// Set a channel for surfacing keyboard-interactive prompts to the UI.
    /// Only meaningful for `AuthMethod::Interactive`; without it, interactive
    /// auth falls back to answering every prompt with the stored password.
    pub fn with_kbi_ask(mut self, tx: KbiAskSender) -> Self {
        self.kbi_ask_tx = Some(tx);
        self
    }

    /// Configure the client-side keepalive interval (zero / `None` disables).
    pub fn with_keepalive(mut self, interval: Option<std::time::Duration>) -> Self {
        self.keepalive_interval = interval.filter(|d| !d.is_zero());
        self
    }

    fn make_config(&self) -> Arc<client::Config> {
        Arc::new(client::Config {
            keepalive_interval: self.keepalive_interval,
            preferred: self.build_preferred(),
            ..client::Config::default()
        })
    }

    fn make_handler(&self, hostname: &str, port: u16) -> ClientHandler {
        ClientHandler {
            hostname: hostname.into(),
            port,
            host_key_check: self.host_key_check.clone(),
            host_key_ask_tx: self.host_key_ask_tx.clone(),
            agent_forwarding: self.agent_forwarding,
            strict_host_key: self.strict_host_key,
            forwarded_channel_sink: self.forwarded_channel_sink.clone(),
        }
    }

    /// Connect to a remote host with full pipeline support:
    /// - Direct TCP connection
    /// - SOCKS4/5 proxy
    /// - HTTP CONNECT proxy
    /// - ProxyCommand (spawn process as transport)
    /// - Jump hosts (chained SSH connections via direct-tcpip channels)
    pub async fn connect(
        &self,
        connection: &Connection,
        password: Option<&str>,
        private_key_pem: Option<&str>,
        cols: u32,
        rows: u32,
    ) -> Result<(SshSession, mpsc::UnboundedReceiver<Vec<u8>>), SshError> {
        self.connect_with_resolver(connection, password, private_key_pem, cols, rows, None)
            .await
    }

    /// Connect with a resolver for jump host credentials. Wraps the
    /// transport setup in `connect_timeout` so the SFTP picker (which
    /// goes through here) doesn't fall through to the kernel's ~127s
    /// SYN-retransmit ceiling on unreachable hosts.
    /// Establish the raw TCP+SSH transport handle: jump chain first, then
    /// a proxy, else a direct dial, all under the connect timeout so an
    /// unreachable host fails fast instead of hanging on SYN retransmits.
    /// Shared by `connect_with_resolver` and `establish_transport`.
    async fn dial(
        &self,
        connection: &Connection,
        resolver: Option<&ConnectionResolver>,
    ) -> Result<client::Handle<ClientHandler>, SshError> {
        let target_host = &connection.hostname;
        let target_port = connection.port;
        let addr = format!("{}:{}", target_host, target_port);
        let connect_timeout = self.connect_timeout;

        tracing::info!(
            "SSH connecting to {} (timeout: {}s)",
            addr,
            connect_timeout.as_secs()
        );

        let connect_fut = async {
            if !connection.jump_chain.is_empty() {
                self.connect_via_jump_hosts(connection, resolver, &addr).await
            } else if let Some(proxy) = &connection.proxy {
                self.connect_via_proxy(proxy, target_host, target_port).await
            } else {
                let config = self.make_config();
                let handler = self.make_handler(target_host, target_port);
                client::connect(config, &addr, handler)
                    .await
                    .map_err(|e| {
                        // Keep the structured negotiation failure (already an
                        // `SshError::Russh(NoCommonAlgo)` via the handler's
                        // `From`) so the UI can offer the legacy-algorithm
                        // fallback instead of a dead-end error string.
                        if e.negotiation_failure().is_some() {
                            e
                        } else {
                            SshError::ConnectionFailed(format!("{}: {}", addr, e))
                        }
                    })
            }
        };
        tokio::time::timeout(connect_timeout, connect_fut)
            .await
            .map_err(|_| {
                SshError::ConnectionFailed(format!(
                    "{}: timed out after {}s",
                    addr,
                    connect_timeout.as_secs()
                ))
            })?
    }

    pub async fn connect_with_resolver(
        &self,
        connection: &Connection,
        password: Option<&str>,
        private_key_pem: Option<&str>,
        cols: u32,
        rows: u32,
        resolver: Option<&ConnectionResolver>,
    ) -> Result<(SshSession, mpsc::UnboundedReceiver<Vec<u8>>), SshError> {
        let handle = self.dial(connection, resolver).await?;

        self.authenticate_and_open(handle, connection, password, private_key_pem, cols, rows)
            .await
    }

    /// Step 1: Establish TCP transport (direct, proxy, or jump host).
    /// Returns an opaque handle after successful TCP connection + SSH handshake + host key verification.
    ///
    /// Wrapped in a 15-second timeout so unreachable hosts fail fast instead of
    /// hanging on TCP SYN retransmits (Linux default: ~127s for SYN retries).
    pub async fn establish_transport(
        &self,
        connection: &Connection,
        resolver: Option<&ConnectionResolver>,
    ) -> Result<SshHandle, SshError> {
        let handle = self.dial(connection, resolver).await?;
        Ok(SshHandle(handle))
    }

    /// Step 2: Authenticate on an established handle. Configurable
    /// timeout (default 30s) so a misbehaving server wedging mid-
    /// handshake can't hang the connect flow forever.
    pub async fn do_authenticate(
        &self,
        handle: &mut SshHandle,
        connection: &Connection,
        password: Option<&str>,
        private_key_pem: Option<&str>,
    ) -> Result<(), SshError> {
        self.authenticate_handle_bounded(&mut handle.0, connection, password, private_key_pem)
            .await
    }

    /// Run `authenticate_handle` under the auth-stage timeout, EXCEPT for
    /// `AuthMethod::Interactive`. Interactive parks on human input (reading
    /// a prompt, fetching an OTP from a phone), which routinely exceeds any
    /// sane network bound, so the blanket `auth_timeout` would abort the very
    /// 2FA flow it's meant to protect. For Interactive the network
    /// round-trips are bounded individually inside `try_keyboard_interactive`
    /// instead, so a misbehaving server is still capped while a slow human is
    /// not. The user can always cancel the prompt to fail the auth cleanly.
    async fn authenticate_handle_bounded(
        &self,
        handle: &mut client::Handle<ClientHandler>,
        connection: &Connection,
        password: Option<&str>,
        private_key_pem: Option<&str>,
    ) -> Result<(), SshError> {
        if connection.auth_method == AuthMethod::Interactive {
            return self
                .authenticate_handle(handle, connection, password, private_key_pem)
                .await;
        }
        let auth_timeout = self.auth_timeout;
        tokio::time::timeout(
            auth_timeout,
            self.authenticate_handle(handle, connection, password, private_key_pem),
        )
        .await
        .map_err(|_| {
            SshError::ConnectionFailed(format!(
                "auth timed out after {}s",
                auth_timeout.as_secs()
            ))
        })?
    }

    /// Step 3: Open PTY session on an authenticated handle. The session
    /// timeout (default 10s) covers the channel-open + pty-request +
    /// shell-request chain.
    pub async fn open_session(
        &self,
        handle: SshHandle,
        cols: u32,
        rows: u32,
        port_forwards: &[PortForward],
    ) -> Result<(SshSession, mpsc::UnboundedReceiver<Vec<u8>>), SshError> {
        let session_timeout = self.session_timeout;
        let listeners = bind_port_forward_listeners(port_forwards).await?;
        tokio::time::timeout(
            session_timeout,
            self.open_pty_session(handle.0, cols, rows, listeners),
        )
        .await
        .map_err(|_| {
            SshError::ConnectionFailed(format!(
                "session open timed out after {}s",
                session_timeout.as_secs()
            ))
        })?
        .map(|(mut session, rx)| {
            // Propagate the SFTP-open timeout so siblings opened later
            // honour the same configured limit.
            session.sftp_open_timeout = session_timeout;
            (session, rx)
        })
    }

    /// Open a standalone port forward (no PTY). Runs the same transport +
    /// auth ladder as a terminal connect, then binds the forward listener
    /// instead of requesting a shell. The returned `ForwardSession` holds
    /// the connection open until cancelled.
    ///
    /// Consumes `self` because a remote (`-R`) forward must install the
    /// inbound-channel sink on the handler *before* the transport (and thus
    /// the handler) is created.
    pub async fn connect_forward(
        mut self,
        connection: &Connection,
        password: Option<&str>,
        private_key_pem: Option<&str>,
        rule: &PortForwardRule,
        resolver: Option<&ConnectionResolver>,
    ) -> Result<ForwardSession, SshError> {
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

        // Remote forwards need the handler to route inbound `forwarded-tcpip`
        // channels, so wire the sink before `establish_transport` builds it.
        let remote_rx = if rule.kind == ForwardKind::Remote {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            self.forwarded_channel_sink = Some(tx);
            Some(rx)
        } else {
            None
        };

        let mut handle = self.establish_transport(connection, resolver).await?;
        self.do_authenticate(&mut handle, connection, password, private_key_pem)
            .await?;
        let shared = Arc::new(tokio::sync::Mutex::new(handle.0));

        match rule.kind {
            ForwardKind::Local => {
                let listener =
                    bind_forward_listener(&rule.listen_host, rule.listen_port).await?;
                let task = spawn_local_forward_task(
                    listener,
                    Arc::clone(&shared),
                    rule.target_host.clone(),
                    rule.target_port,
                    rule.listen_port,
                    cancel_rx,
                );
                tracing::info!(
                    "forward(-L) {}:{} -> {}:{} up",
                    rule.listen_host, rule.listen_port, rule.target_host, rule.target_port
                );
                Ok(ForwardSession {
                    handle: shared,
                    cancel_tx,
                    _tasks: vec![task],
                    remote_bind: None,
                })
            }
            ForwardKind::Remote => {
                // Ask the server to listen on `listen_host:listen_port` and
                // tunnel inbound connections back to us. A denied request
                // (e.g. `AllowTcpForwarding no`) fails the toggle.
                {
                    let h = shared.lock().await;
                    h.tcpip_forward(rule.listen_host.clone(), rule.listen_port as u32)
                        .await
                        .map_err(|e| {
                            SshError::Channel(format!("remote forward request denied: {e}"))
                        })?;
                }
                let mut rx = remote_rx.expect("remote sink set above for -R");
                let target_host = rule.target_host.clone();
                let target_port = rule.target_port;
                let mut cancel = cancel_rx;
                let task = tokio::spawn(async move {
                    loop {
                        tokio::select! {
                            _ = cancel.changed() => break,
                            ch = rx.recv() => match ch {
                                Some(channel) => {
                                    let th = target_host.clone();
                                    let child_cancel = cancel.clone();
                                    tokio::spawn(async move {
                                        bridge_channel_to_target(
                                            channel, th, target_port, child_cancel,
                                        )
                                        .await;
                                    });
                                }
                                None => break,
                            },
                        }
                    }
                });
                tracing::info!(
                    "forward(-R) server {}:{} -> local {}:{} up",
                    rule.listen_host, rule.listen_port, rule.target_host, rule.target_port
                );
                Ok(ForwardSession {
                    handle: shared,
                    cancel_tx,
                    _tasks: vec![task],
                    remote_bind: Some((rule.listen_host.clone(), rule.listen_port)),
                })
            }
            ForwardKind::Dynamic => {
                let listener =
                    bind_forward_listener(&rule.listen_host, rule.listen_port).await?;
                let task = spawn_dynamic_forward_task(
                    listener,
                    Arc::clone(&shared),
                    rule.listen_port,
                    cancel_rx,
                );
                tracing::info!(
                    "forward(-D) SOCKS5 {}:{} up",
                    rule.listen_host, rule.listen_port
                );
                Ok(ForwardSession {
                    handle: shared,
                    cancel_tx,
                    _tasks: vec![task],
                    remote_bind: None,
                })
            }
        }
    }

    // -----------------------------------------------------------------------
    // Transport resolvers
    // -----------------------------------------------------------------------

    /// Connect via SOCKS or HTTP proxy.
    async fn connect_via_proxy(
        &self,
        proxy: &ProxyConfig,
        target_host: &str,
        target_port: u16,
    ) -> Result<client::Handle<ClientHandler>, SshError> {
        let proxy_addr = format!("{}:{}", proxy.host, proxy.port);
        tracing::info!("Connecting via {:?} proxy at {}", proxy.proxy_type, proxy_addr);

        match &proxy.proxy_type {
            ProxyType::Socks5 => {
                let stream = if let Some(user) = &proxy.username {
                    // SOCKS5 username/password auth (RFC 1929). Password
                    // is hydrated from the vault before this call; if
                    // the user configured no password, send an empty
                    // one, the proxy may still accept it.
                    tokio_socks::tcp::Socks5Stream::connect_with_password(
                        proxy_addr.as_str(),
                        (target_host, target_port),
                        user.as_str(),
                        proxy.password.as_deref().unwrap_or(""),
                    )
                    .await
                    .map_err(|e| SshError::Proxy(format!("SOCKS5 auth: {}", e)))?
                } else {
                    tokio_socks::tcp::Socks5Stream::connect(
                        proxy_addr.as_str(),
                        (target_host, target_port),
                    )
                    .await
                    .map_err(|e| SshError::Proxy(format!("SOCKS5: {}", e)))?
                };

                let config = self.make_config();
                client::connect_stream(config, stream, self.make_handler(target_host, target_port))
                    .await
                    .map_err(|e| SshError::Proxy(format!("SSH over SOCKS5: {}", e)))
            }
            ProxyType::Socks4 => {
                let stream = if let Some(user) = &proxy.username {
                    tokio_socks::tcp::Socks4Stream::connect_with_userid(
                        proxy_addr.as_str(),
                        (target_host, target_port),
                        user.as_str(),
                    )
                    .await
                    .map_err(|e| SshError::Proxy(format!("SOCKS4: {}", e)))?
                } else {
                    tokio_socks::tcp::Socks4Stream::connect(
                        proxy_addr.as_str(),
                        (target_host, target_port),
                    )
                    .await
                    .map_err(|e| SshError::Proxy(format!("SOCKS4: {}", e)))?
                };

                let config = self.make_config();
                client::connect_stream(config, stream, self.make_handler(target_host, target_port))
                    .await
                    .map_err(|e| SshError::Proxy(format!("SSH over SOCKS4: {}", e)))
            }
            ProxyType::Http => {
                let stream = self
                    .http_connect_tunnel(
                        &proxy_addr,
                        target_host,
                        target_port,
                        proxy.username.as_deref(),
                        proxy.password.as_deref(),
                    )
                    .await?;

                let config = self.make_config();
                client::connect_stream(config, stream, self.make_handler(target_host, target_port))
                    .await
                    .map_err(|e| SshError::Proxy(format!("SSH over HTTP CONNECT: {}", e)))
            }
            ProxyType::Command(cmd) => {
                let stream = self.proxy_command(cmd).await?;

                let config = self.make_config();
                client::connect_stream(config, stream, self.make_handler(target_host, target_port))
                    .await
                    .map_err(|e| SshError::Proxy(format!("SSH over ProxyCommand: {}", e)))
            }
        }
    }

    /// HTTP CONNECT tunnel, establish a TCP tunnel through an HTTP proxy.
    /// Supports Basic auth (RFC 7617) when `username` is provided.
    async fn http_connect_tunnel(
        &self,
        proxy_addr: &str,
        target_host: &str,
        target_port: u16,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Result<TcpStream, SshError> {
        let mut stream = TcpStream::connect(proxy_addr)
            .await
            .map_err(|e| SshError::Proxy(format!("HTTP proxy connect: {}", e)))?;

        let connect_req = build_http_connect_request(target_host, target_port, username, password);

        stream
            .write_all(connect_req.as_bytes())
            .await
            .map_err(|e| SshError::Proxy(format!("HTTP CONNECT write: {}", e)))?;

        // Read until end-of-headers ("\r\n\r\n"). A single read() typically
        // delivers the whole CONNECT response on first packet, but a hostile
        // or chunked proxy may split it, loop until we have headers or hit
        // a 16 KiB cap (HTTP requests this small never exceed that).
        let mut buf = Vec::with_capacity(1024);
        let mut chunk = [0u8; 1024];
        loop {
            let n = tokio::io::AsyncReadExt::read(&mut stream, &mut chunk)
                .await
                .map_err(|e| SshError::Proxy(format!("HTTP CONNECT read: {}", e)))?;
            if n == 0 {
                return Err(SshError::Proxy(
                    "HTTP CONNECT: proxy closed before response".into(),
                ));
            }
            buf.extend_from_slice(&chunk[..n]);
            if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() > 16 * 1024 {
                break;
            }
        }

        match parse_http_status(&buf) {
            Some(200) => {
                tracing::info!("HTTP CONNECT tunnel established");
                Ok(stream)
            }
            Some(407) => Err(SshError::Proxy(
                "HTTP CONNECT failed: 407 Proxy Authentication Required".into(),
            )),
            Some(code) => Err(SshError::Proxy(format!(
                "HTTP CONNECT failed: status {}",
                code
            ))),
            None => Err(SshError::Proxy(format!(
                "HTTP CONNECT failed: unparseable response \"{}\"",
                String::from_utf8_lossy(&buf).lines().next().unwrap_or("")
            ))),
        }
    }

    /// ProxyCommand, spawn a process and use its stdin/stdout as transport.
    async fn proxy_command(
        &self,
        cmd: &str,
    ) -> Result<impl AsyncRead + AsyncWrite + Unpin + Send + 'static, SshError> {
        tracing::info!("ProxyCommand: {}", cmd);

        let mut child = TokioCommand::new("sh")
            .arg("-c")
            .arg(cmd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| SshError::Proxy(format!("ProxyCommand spawn: {}", e)))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| SshError::Proxy("ProxyCommand: no stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| SshError::Proxy("ProxyCommand: no stdout".into()))?;

        Ok(tokio::io::join(stdout, stdin))
    }

    /// Connect via jump hosts (SSH tunneling through bastion hosts).
    async fn connect_via_jump_hosts(
        &self,
        connection: &Connection,
        resolver: Option<&ConnectionResolver>,
        final_addr: &str,
    ) -> Result<client::Handle<ClientHandler>, SshError> {
        let resolver = resolver.ok_or_else(|| {
            SshError::JumpHost("Jump hosts require a connection resolver".into())
        })?;

        tracing::info!(
            "Connecting via {} jump host(s)",
            connection.jump_chain.len()
        );

        // Connect to the first jump host. If the jump itself sits
        // behind a proxy, dial via that proxy, only the *first* hop
        // does, since subsequent hops travel inside the SSH tunnel.
        let first_jump_id = connection.jump_chain[0];
        let first_jump = resolver
            .connections
            .iter()
            .find(|c| c.id == first_jump_id)
            .ok_or_else(|| SshError::JumpHost("First jump host not found".into()))?;

        let first_addr = format!("{}:{}", first_jump.hostname, first_jump.port);
        let mut current_handle = if let Some(first_proxy) = resolver.proxies.get(&first_jump_id) {
            tracing::info!(
                "First jump host {} sits behind {:?} proxy",
                first_addr,
                first_proxy.proxy_type
            );
            self.connect_via_proxy(first_proxy, &first_jump.hostname, first_jump.port)
                .await
                .map_err(|e| SshError::JumpHost(format!("Jump host {} via proxy: {}", first_addr, e)))?
        } else {
            let config = self.make_config();
            let handler = self.make_handler(&first_jump.hostname, first_jump.port);
            client::connect(config, &first_addr, handler)
                .await
                .map_err(|e| SshError::JumpHost(format!("Jump host {}: {}", first_addr, e)))?
        };

        // Authenticate on first jump host
        let first_pw = resolver.passwords.get(&first_jump_id);
        let first_key = resolver.private_keys.get(&first_jump_id);
        self.authenticate_handle(
            &mut current_handle,
            first_jump,
            first_pw.map(String::as_str),
            first_key.map(String::as_str),
        )
        .await?;

        // Chain through remaining jump hosts
        for i in 1..connection.jump_chain.len() {
            let jump_id = connection.jump_chain[i];
            let jump = resolver
                .connections
                .iter()
                .find(|c| c.id == jump_id)
                .ok_or_else(|| SshError::JumpHost(format!("Jump host {} not found", jump_id)))?;

            // Open a direct-tcpip channel through current host to next hop
            let channel = current_handle
                .channel_open_direct_tcpip(
                    jump.hostname.clone(),
                    jump.port as u32,
                    "127.0.0.1",
                    0,
                )
                .await
                .map_err(|e| SshError::JumpHost(format!("direct-tcpip to {}: {}", jump.hostname, e)))?;

            let stream = channel.into_stream();
            let config = self.make_config();
            let handler = self.make_handler(&jump.hostname, jump.port);
            current_handle = client::connect_stream(config, stream, handler)
                .await
                .map_err(|e| SshError::JumpHost(format!("SSH handshake via jump: {}", e)))?;

            let jump_pw = resolver.passwords.get(&jump_id);
            let jump_key = resolver.private_keys.get(&jump_id);
            self.authenticate_handle(
                &mut current_handle,
                jump,
                jump_pw.map(String::as_str),
                jump_key.map(String::as_str),
            )
            .await?;
        }

        // Open direct-tcpip channel to final target through the last jump host
        let (target_host, target_port) = parse_addr(final_addr)?;
        let channel = current_handle
            .channel_open_direct_tcpip(target_host.clone(), target_port, "127.0.0.1", 0)
            .await
            .map_err(|e| SshError::JumpHost(format!("direct-tcpip to target {}: {}", final_addr, e)))?;

        let stream = channel.into_stream();
        let config = self.make_config();
        let handler = self.make_handler(&target_host, target_port as u16);
        client::connect_stream(config, stream, handler)
            .await
            .map_err(|e| SshError::JumpHost(format!("SSH handshake to target: {}", e)))
    }

    // -----------------------------------------------------------------------
    // Authentication
    // -----------------------------------------------------------------------

    /// Authenticate on a handle (used for both direct and jump host connections).
    async fn authenticate_handle(
        &self,
        handle: &mut client::Handle<ClientHandler>,
        connection: &Connection,
        password: Option<&str>,
        private_key_pem: Option<&str>,
    ) -> Result<(), SshError> {
        let username = connection.username.as_deref().unwrap_or("root");
        let has_pw = password.is_some();
        let has_key = private_key_pem.is_some();
        tracing::info!(
            "Auth for {}@{} method={:?} has_password={} has_key={}",
            username, connection.hostname, connection.auth_method, has_pw, has_key,
        );

        match self
            .do_auth(handle, username, &connection.auth_method, password, private_key_pem)
            .await
        {
            Ok(true) => {
                tracing::info!("Authenticated as {} on {}", username, connection.hostname);
                Ok(())
            }
            Ok(false) => Err(SshError::Key(format!(
                "Auth rejected for \"{}\" (method: {:?}, password: {}, key: {})",
                username, connection.auth_method, has_pw, has_key,
            ))),
            Err(e) => Err(e),
        }
    }

    async fn do_auth(
        &self,
        handle: &mut client::Handle<ClientHandler>,
        username: &str,
        auth_method: &AuthMethod,
        password: Option<&str>,
        private_key_pem: Option<&str>,
    ) -> Result<bool, SshError> {
        match auth_method {
            AuthMethod::Auto => {
                let mut tried: Vec<&str> = Vec::new();

                // 1. Try publickey if a key is provided
                if let Some(pem) = private_key_pem {
                    tried.push("publickey");
                    tracing::info!("Auto: trying publickey auth for {}", username);
                    match self.try_publickey_auth(handle, username, pem).await {
                        Ok(true) => return Ok(true),
                        Ok(false) => tracing::info!("Auto: publickey rejected"),
                        Err(e) => tracing::info!("Auto: publickey error: {}", e),
                    }
                }

                // 2. Try agent auth
                tried.push("agent");
                tracing::info!("Auto: trying agent auth for {}", username);
                match self.auth_via_agent(handle, username).await {
                    Ok(true) => return Ok(true),
                    Ok(false) => tracing::info!("Auto: agent had no matching keys"),
                    Err(e) => tracing::info!("Auto: agent unavailable: {}", e),
                }

                // 3. Try password if available
                if let Some(pw) = password {
                    tried.push("password");
                    tracing::info!("Auto: trying password auth for {}", username);
                    match handle.authenticate_password(username, pw).await {
                        Ok(res) if res.success() => return Ok(true),
                        Ok(_) => tracing::info!("Auto: password rejected"),
                        Err(e) => tracing::info!("Auto: password error: {}", e),
                    }

                    // 4. Try keyboard-interactive with password. Auto never
                    // surfaces a modal (use_callback = false): it only reaches
                    // here after password already failed, so a prompt at the
                    // tail of Auto would be surprising. The user picks
                    // AuthMethod::Interactive when they want the modal.
                    tried.push("keyboard-interactive");
                    tracing::info!("Auto: trying keyboard-interactive auth for {}", username);
                    if self.try_keyboard_interactive(handle, username, Some(pw), false).await? {
                        return Ok(true);
                    }
                }

                Err(SshError::Key(format!(
                    "All auto auth methods failed for \"{}\". Tried: {}",
                    username,
                    tried.join(", ")
                )))
            }
            AuthMethod::Password => {
                let pw = password.ok_or(SshError::AuthFailed)?;
                tracing::info!("Trying password auth for {}", username);
                let res = handle.authenticate_password(username, pw).await?;
                if !res.success() {
                    return Err(SshError::Key("Password rejected by server".into()));
                }
                Ok(true)
            }
            AuthMethod::Key => {
                let pem = private_key_pem
                    .ok_or_else(|| SshError::Key("No private key selected".into()))?;

                tracing::info!("Trying publickey auth for {}", username);
                if self.try_publickey_auth(handle, username, pem).await? {
                    return Ok(true);
                }

                // Key was rejected, try password as fallback if available
                if let Some(pw) = password {
                    tracing::info!("Key rejected, trying password fallback for {}", username);
                    let res = handle.authenticate_password(username, pw).await?;
                    if res.success() {
                        return Ok(true);
                    }
                    return Err(SshError::Key("Both key and password rejected by server".into()));
                }

                Err(SshError::Key("Public key rejected by server".into()))
            }
            AuthMethod::Agent => {
                tracing::info!("Trying agent auth for {}", username);
                match self.auth_via_agent(handle, username).await {
                    Ok(true) => Ok(true),
                    Ok(false) => {
                        if let Some(pw) = password {
                            tracing::info!("Agent auth failed, trying password for {}", username);
                            let res = handle.authenticate_password(username, pw).await?;
                            if res.success() {
                                return Ok(true);
                            }
                        }
                        Err(SshError::Key("Agent auth failed, no keys matched".into()))
                    }
                    Err(e) => {
                        if let Some(pw) = password {
                            tracing::info!("Agent unavailable ({}), trying password for {}", e, username);
                            let res = handle.authenticate_password(username, pw).await?;
                            if res.success() {
                                return Ok(true);
                            }
                        }
                        Err(e)
                    }
                }
            }
            AuthMethod::Interactive => {
                tracing::info!("Trying keyboard-interactive auth for {}", username);
                if self.try_keyboard_interactive(handle, username, password, true).await? {
                    Ok(true)
                } else {
                    Err(SshError::Key("Keyboard-interactive auth rejected".into()))
                }
            }
        }
    }

    /// Try publickey auth with rsa-sha2-256 for RSA keys.
    async fn try_publickey_auth(
        &self,
        handle: &mut client::Handle<ClientHandler>,
        username: &str,
        pem: &str,
    ) -> Result<bool, SshError> {
        let private_key = russh::keys::decode_secret_key(pem, None)
            .map_err(|e| SshError::Key(format!("Failed to decode key: {}", e)))?;
        let hash = if private_key.algorithm().is_rsa() {
            Some(HashAlg::Sha256)
        } else {
            None
        };
        let key = PrivateKeyWithHashAlg::new(Arc::new(private_key), hash);
        let res = handle.authenticate_publickey(username, key).await?;
        Ok(res.success())
    }

    /// Drive a keyboard-interactive exchange to completion.
    ///
    /// `_start` is called once, then we loop on `_respond` round by round
    /// until the server returns `Success` or `Failure` (a single auth can
    /// span several `InfoRequest` rounds, e.g. password then OTP). The loop
    /// is bounded so a misbehaving server can't pop prompts forever.
    ///
    /// Each round's answers come from one of three sources, in order:
    /// - `use_callback` + a `kbi_ask_tx` channel: surface the prompts to the
    ///   UI and wait for typed answers. The user cancelling (`None`) aborts
    ///   the auth cleanly (`Ok(false)`).
    /// - otherwise `fallback_pw`: answer every prompt with the stored
    ///   password (the Auto path, and the headless degrade path).
    /// - neither available: fail cleanly (`Ok(false)`).
    ///
    /// A round carrying zero prompts is answered with an empty response, so
    /// servers that send an informational-only `InfoRequest` keep advancing.
    async fn try_keyboard_interactive(
        &self,
        handle: &mut client::Handle<ClientHandler>,
        username: &str,
        fallback_pw: Option<&str>,
        use_callback: bool,
    ) -> Result<bool, SshError> {
        // Cap on the number of challenge rounds. Real flows use 1-2; this is
        // just a backstop against a server that loops InfoRequest forever.
        const MAX_ROUNDS: usize = 16;

        // The outer auth-stage timeout is skipped for Interactive (it would
        // abort while the user types an OTP), so bound the individual network
        // round-trips here instead. The human-input wait below stays
        // unbounded but cancellable.
        let net_timeout = self.auth_timeout;
        let net_err = || {
            SshError::ConnectionFailed(format!(
                "keyboard-interactive server response timed out after {}s",
                net_timeout.as_secs()
            ))
        };

        let mut resp = tokio::time::timeout(
            net_timeout,
            handle.authenticate_keyboard_interactive_start(username, None::<String>),
        )
        .await
        .map_err(|_| net_err())??;

        for _ in 0..MAX_ROUNDS {
            let (name, instructions, prompts) = match resp {
                client::KeyboardInteractiveAuthResponse::Success => return Ok(true),
                client::KeyboardInteractiveAuthResponse::Failure { .. } => return Ok(false),
                client::KeyboardInteractiveAuthResponse::InfoRequest {
                    name,
                    instructions,
                    prompts,
                } => (name, instructions, prompts),
            };

            let answers: Vec<String> = if prompts.is_empty() {
                Vec::new()
            } else if use_callback && self.kbi_ask_tx.is_some() {
                let tx = self.kbi_ask_tx.as_ref().unwrap();
                let query = KbiQuery {
                    name,
                    instructions,
                    prompts: prompts
                        .iter()
                        .map(|p| KbiPromptField {
                            prompt: p.prompt.clone(),
                            echo: p.echo,
                        })
                        .collect(),
                };
                let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
                if tx.send((query, resp_tx)).await.is_err() {
                    // UI bridge is gone; treat as cancellation.
                    return Ok(false);
                }
                match resp_rx.await {
                    Ok(Some(answers)) => answers,
                    // User cancelled, or the responder dropped: abort cleanly.
                    Ok(None) | Err(_) => return Ok(false),
                }
            } else if let Some(pw) = fallback_pw {
                prompts.iter().map(|_| pw.to_string()).collect()
            } else {
                return Ok(false);
            };

            resp = tokio::time::timeout(
                net_timeout,
                handle.authenticate_keyboard_interactive_respond(answers),
            )
            .await
            .map_err(|_| net_err())??;
        }

        tracing::warn!("keyboard-interactive exceeded {} rounds, giving up", MAX_ROUNDS);
        Ok(false)
    }

    /// Authenticate and open a PTY session on the handle.
    /// Authenticate via ssh-agent. Uses Unix socket on Linux/macOS, named pipe on Windows.
    #[cfg(unix)]
    async fn auth_via_agent(
        &self,
        handle: &mut client::Handle<ClientHandler>,
        username: &str,
    ) -> Result<bool, SshError> {
        match russh::keys::agent::client::AgentClient::connect_env().await {
            Ok(mut agent) => {
                let identities = agent
                    .request_identities()
                    .await
                    .map_err(|e| SshError::Key(format!("Agent: {}", e)))?;

                for identity in identities {
                    let pubkey = identity.public_key().into_owned();
                    let hash = if pubkey.algorithm().is_rsa() {
                        Some(HashAlg::Sha256)
                    } else {
                        None
                    };
                    if let Ok(res) = handle
                        .authenticate_publickey_with(username, pubkey, hash, &mut agent)
                        .await
                    && res.success() {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Err(e) => Err(SshError::Key(format!("ssh-agent not available: {}", e))),
        }
    }

    /// Authenticate via Windows OpenSSH Agent (named pipe).
    #[cfg(windows)]
    async fn auth_via_agent(
        &self,
        handle: &mut client::Handle<ClientHandler>,
        username: &str,
    ) -> Result<bool, SshError> {
        let pipe_path = windows_agent_pipe();
        match russh::keys::agent::client::AgentClient::connect_named_pipe(&pipe_path).await {
            Ok(mut agent) => {
                let identities = agent
                    .request_identities()
                    .await
                    .map_err(|e| SshError::Key(format!("Agent: {}", e)))?;

                for identity in identities {
                    let pubkey = identity.public_key().into_owned();
                    let hash = if pubkey.algorithm().is_rsa() {
                        Some(HashAlg::Sha256)
                    } else {
                        None
                    };
                    if let Ok(res) = handle
                        .authenticate_publickey_with(username, pubkey, hash, &mut agent)
                        .await
                    && res.success() {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Err(e) => Err(SshError::Key(format!(
                "Windows ssh-agent not available ({}): {}",
                pipe_path, e
            ))),
        }
    }

    async fn authenticate_and_open(
        &self,
        mut handle: client::Handle<ClientHandler>,
        connection: &Connection,
        password: Option<&str>,
        private_key_pem: Option<&str>,
        cols: u32,
        rows: u32,
    ) -> Result<(SshSession, mpsc::UnboundedReceiver<Vec<u8>>), SshError> {
        // Apply the same per-phase timeouts the public 2-step API uses
        //, single-call connects via `connect_with_resolver` were
        // bypassing them, leaving auth/session free to hang on the OS
        // default ceilings. Auth honours the Interactive exemption (human
        // input isn't a network stall) via `authenticate_handle_bounded`.
        let session_timeout = self.session_timeout;
        self.authenticate_handle_bounded(&mut handle, connection, password, private_key_pem)
            .await?;
        let listeners = bind_port_forward_listeners(&connection.port_forwards).await?;
        let (mut session, rx) = tokio::time::timeout(
            session_timeout,
            self.open_pty_session(handle, cols, rows, listeners),
        )
        .await
        .map_err(|_| {
            SshError::ConnectionFailed(format!(
                "session open timed out after {}s",
                session_timeout.as_secs()
            ))
        })??;
        session.sftp_open_timeout = session_timeout;
        Ok((session, rx))
    }

    /// Execute a command without PTY (non-interactive) and return the output.
    pub async fn exec_command(
        &self,
        handle: SshHandle,
        command: &str,
        timeout: std::time::Duration,
    ) -> Result<ExecResult, SshError> {
        let channel = handle.0.channel_open_session().await
            .map_err(|e| SshError::Channel(format!("open session: {}", e)))?;

        channel.exec(true, command).await
            .map_err(|e| SshError::Channel(format!("exec: {}", e)))?;

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exit_code: Option<u32> = None;

        let collect = async {
            let mut channel = channel;
            // Read until channel close (`None`), not just Eof, some
            // servers send `ExitStatus` after `Eof`, so breaking early
            // would leave us defaulting to 255.
            loop {
                match channel.wait().await {
                    Some(ChannelMsg::Data { data }) => stdout.extend_from_slice(&data),
                    Some(ChannelMsg::ExtendedData { data, ext: 1 }) => {
                        stderr.extend_from_slice(&data);
                    }
                    Some(ChannelMsg::ExitStatus { exit_status }) => {
                        exit_code = Some(exit_status);
                    }
                    None => break,
                    _ => {}
                }
            }
        };

        match tokio::time::timeout(timeout, collect).await {
            Ok(()) => {}
            Err(_) => {
                return Err(SshError::Channel("Command timed out".into()));
            }
        }

        Ok(ExecResult {
            exit_code: exit_code.unwrap_or(255),
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        })
    }

    async fn open_pty_session(
        &self,
        handle: client::Handle<ClientHandler>,
        cols: u32,
        rows: u32,
        pf_listeners: Vec<(PortForward, tokio::net::TcpListener)>,
    ) -> Result<(SshSession, mpsc::UnboundedReceiver<Vec<u8>>), SshError> {
        // Open session channel
        let channel = handle.channel_open_session().await
            .map_err(|e| SshError::Channel(format!("Failed to open session channel: {}", e)))?;

        // Request PTY
        let term = self.terminal_type.as_deref().unwrap_or("xterm-256color");
        channel
            .request_pty(false, term, cols, rows, 0, 0, &[])
            .await
            .map_err(|e| SshError::Channel(format!("PTY request failed: {}", e)))?;

        // Optional ssh-agent forwarding. Must fire BEFORE `request_shell`
        //, sshd reads the channel requests in order and only sets
        // `SSH_AUTH_SOCK` on the launched process if forwarding was
        // already requested when the shell starts. Issued without
        // `want_reply`; failures (server has `AllowAgentForwarding no`)
        // are not fatal, the user still gets a normal shell, they
        // just can't hop further with their local keys.
        if self.agent_forwarding
            && let Err(e) = channel.agent_forward(false).await
        {
            tracing::warn!("agent_forward request failed (non-fatal): {}", e);
        }

        // Per-host environment variables. Sent before `request_shell` so
        // the server can apply them to the launched process. Non-fatal:
        // most `sshd` reject anything outside `AcceptEnv` (LC_*/LANG_* by
        // default), and we'd rather give the user a shell than abort.
        for (name, value) in &self.env_vars {
            if let Err(e) = channel.set_env(false, name.clone(), value.clone()).await {
                tracing::warn!("set_env {} failed (non-fatal): {}", name, e);
            }
        }

        // Request shell
        channel.request_shell(false).await
            .map_err(|e| SshError::Channel(format!("Shell request failed: {}", e)))?;

        // I/O bridging
        let (output_tx, output_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (writer_tx, mut writer_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (resize_tx, mut resize_rx) = mpsc::unbounded_channel::<(u16, u16)>();

        // Resolve the per-host charset once. `None` (or UTF-8) means the
        // byte stream is forwarded untouched; any other charset is decoded
        // to UTF-8 inbound and encoded back outbound for the terminal.
        let enc: Option<&'static encoding_rs::Encoding> = self
            .encoding
            .as_deref()
            .and_then(|n| encoding_rs::Encoding::for_label(n.as_bytes()))
            .filter(|e| *e != encoding_rs::UTF_8);

        let mut channel_writer = channel.make_writer();

        // Reader task, multiplexes incoming PTY data with outgoing
        // window-change requests so we only own `channel` in one place.
        let reader_task = tokio::spawn(async move {
            let mut channel = channel;
            // Stateful decoder so a multi-byte char split across two reads
            // still decodes correctly. `None` for UTF-8 (passthrough).
            let mut decoder = enc.map(|e| e.new_decoder());
            // Cap on one forwarded message. Data messages already queued on
            // the channel are folded into a single send so the UI runs one
            // update+view+draw cycle per batch instead of one per SSH packet.
            const COALESCE_MAX: usize = 64 * 1024;
            loop {
                tokio::select! {
                    msg = channel.wait() => {
                        // Set when EOF / exit-status arrives mid-batch: the
                        // accumulated bytes are flushed first, then the loop
                        // exits, so no trailing output is dropped.
                        let mut closed = false;
                        let bytes: Option<Vec<u8>> = match msg {
                            Some(ChannelMsg::Data { data }) => Some(data.to_vec()),
                            Some(ChannelMsg::ExtendedData { data, ext: 1 }) => Some(data.to_vec()),
                            Some(ChannelMsg::ExtendedData { .. }) => continue,
                            Some(ChannelMsg::ExitStatus { exit_status }) => {
                                tracing::info!("Remote exited with status {}", exit_status);
                                break;
                            }
                            Some(ChannelMsg::Eof) | None => {
                                tracing::info!("SSH channel closed");
                                break;
                            }
                            _ => continue,
                        };
                        if let Some(mut b) = bytes {
                            // Coalesce: drain messages that are already
                            // queued (zero timeout never waits for new
                            // data, so interactive echo latency is
                            // unchanged) up to the batch cap.
                            while b.len() < COALESCE_MAX {
                                match tokio::time::timeout(
                                    std::time::Duration::ZERO,
                                    channel.wait(),
                                ).await {
                                    Ok(Some(ChannelMsg::Data { data })) => {
                                        b.extend_from_slice(&data);
                                    }
                                    Ok(Some(ChannelMsg::ExtendedData { data, ext: 1 })) => {
                                        b.extend_from_slice(&data);
                                    }
                                    Ok(Some(ChannelMsg::ExtendedData { .. })) => continue,
                                    Ok(Some(ChannelMsg::ExitStatus { exit_status })) => {
                                        tracing::info!(
                                            "Remote exited with status {}", exit_status,
                                        );
                                        closed = true;
                                        break;
                                    }
                                    Ok(Some(ChannelMsg::Eof)) | Ok(None) => {
                                        tracing::info!("SSH channel closed");
                                        closed = true;
                                        break;
                                    }
                                    Ok(Some(_)) => continue,
                                    // Nothing queued right now: flush.
                                    Err(_) => break,
                                }
                            }
                            let out = match &mut decoder {
                                Some(dec) => {
                                    let mut s = String::with_capacity(b.len() + 16);
                                    let _ = dec.decode_to_string(&b, &mut s, false);
                                    s.into_bytes()
                                }
                                None => b,
                            };
                            if output_tx.send(out).is_err() {
                                break;
                            }
                        }
                        if closed {
                            break;
                        }
                    }
                    Some((cols, rows)) = resize_rx.recv() => {
                        if let Err(e) = channel
                            .window_change(cols as u32, rows as u32, 0, 0)
                            .await
                        {
                            tracing::warn!("SSH window-change failed: {}", e);
                        }
                    }
                }
            }
        });

        // Writer task
        let writer_task = tokio::spawn(async move {
            while let Some(data) = writer_rx.recv().await {
                // Terminal input arrives as UTF-8; encode it to the host
                // charset when one is set. One-shot per write is fine:
                // keystrokes/pastes arrive as whole UTF-8 chars.
                let data = match enc {
                    Some(e) => {
                        let text = String::from_utf8_lossy(&data);
                        let (encoded, _, _) = e.encode(&text);
                        encoded.into_owned()
                    }
                    None => data,
                };
                if let Err(e) = channel_writer.write_all(&data).await {
                    tracing::error!("SSH write error: {}", e);
                    break;
                }
                if let Err(e) = channel_writer.flush().await {
                    tracing::error!("SSH flush error: {}", e);
                    break;
                }
            }
        });

        let shared_handle = Arc::new(tokio::sync::Mutex::new(handle));
        let pf_tasks = spawn_port_forward_tasks(pf_listeners, &shared_handle);

        Ok((
            SshSession {
                _handle: shared_handle,
                writer_tx,
                resize_tx,
                reader_task,
                writer_task,
                port_forward_tasks: pf_tasks,
                closed: std::sync::atomic::AtomicBool::new(false),
                // Default, overridden by the engine right after this
                // returns via `sftp_open_timeout` assignment.
                sftp_open_timeout: std::time::Duration::from_secs(10),
            },
            output_rx,
        ))
    }
}


fn parse_addr(addr: &str) -> Result<(String, u32), SshError> {
    let parts: Vec<&str> = addr.rsplitn(2, ':').collect();
    if parts.len() != 2 {
        return Err(SshError::ConnectionFailed(format!("Invalid address: {}", addr)));
    }
    let port: u32 = parts[0]
        .parse()
        .map_err(|_| SshError::ConnectionFailed(format!("Invalid port in: {}", addr)))?;
    Ok((parts[1].to_string(), port))
}

/// Build the request bytes for an HTTP CONNECT tunnel. When `username`
/// is provided, a `Proxy-Authorization: Basic` header is added (RFC
/// 7617). `password` may be `None` or empty, the colon separator is
/// always present per the spec.
fn build_http_connect_request(
    target_host: &str,
    target_port: u16,
    username: Option<&str>,
    password: Option<&str>,
) -> String {
    use base64::Engine as _;
    let mut req = format!(
        "CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\n",
        host = target_host,
        port = target_port,
    );
    if let Some(user) = username {
        let creds = format!("{}:{}", user, password.unwrap_or(""));
        let encoded = base64::engine::general_purpose::STANDARD.encode(creds);
        req.push_str(&format!("Proxy-Authorization: Basic {}\r\n", encoded));
    }
    req.push_str("\r\n");
    req
}

/// Parse the status code out of an HTTP/1.x response. Returns `None`
/// if the status line can't be read (e.g. the proxy spoke garbage).
fn parse_http_status(buf: &[u8]) -> Option<u16> {
    let line_end = buf.windows(2).position(|w| w == b"\r\n").unwrap_or(buf.len());
    let line = std::str::from_utf8(&buf[..line_end]).ok()?;
    let mut parts = line.split_whitespace();
    let _version = parts.next()?;
    parts.next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_addr_valid() {
        let (host, port) = parse_addr("192.168.1.1:22").unwrap();
        assert_eq!(host, "192.168.1.1");
        assert_eq!(port, 22);
    }

    #[test]
    fn parse_addr_hostname() {
        let (host, port) = parse_addr("server.example.com:2222").unwrap();
        assert_eq!(host, "server.example.com");
        assert_eq!(port, 2222);
    }

    #[test]
    fn parse_addr_no_port_fails() {
        assert!(parse_addr("192.168.1.1").is_err());
    }

    #[test]
    fn parse_addr_bad_port_fails() {
        assert!(parse_addr("host:abc").is_err());
    }

    #[test]
    fn identity_agent_forward_slashes_normalized() {
        let conf = "IdentityAgent //./pipe/pageant.user.0123abcd\n";
        assert_eq!(
            parse_identity_agent(conf).as_deref(),
            Some(r"\\.\pipe\pageant.user.0123abcd")
        );
    }

    #[test]
    fn identity_agent_skips_comments_and_other_keys() {
        let conf = "# pageant\nForwardAgent yes\n  IdentityAgent  \"//./pipe/pageant.abc\"  \n";
        assert_eq!(
            parse_identity_agent(conf).as_deref(),
            Some(r"\\.\pipe\pageant.abc")
        );
    }

    #[test]
    fn identity_agent_equals_spelling() {
        let conf = "IdentityAgent=//./pipe/pageant.eq\n";
        assert_eq!(
            parse_identity_agent(conf).as_deref(),
            Some(r"\\.\pipe\pageant.eq")
        );
    }

    #[test]
    fn identity_agent_absent_returns_none() {
        assert_eq!(parse_identity_agent("ForwardAgent yes\n"), None);
    }

    #[test]
    fn identity_agent_empty_value_keeps_scanning() {
        // A first IdentityAgent line with an empty value must not abort
        // the scan; a later valid line still wins.
        let conf = "IdentityAgent \nIdentityAgent //./pipe/pageant.late\n";
        assert_eq!(
            parse_identity_agent(conf).as_deref(),
            Some(r"\\.\pipe\pageant.late")
        );
    }

    #[test]
    fn pageant_pipe_matches_current_user() {
        let names = vec![
            "openssh-ssh-agent".to_string(),
            "pageant.alice.deadbeef".to_string(),
        ];
        assert_eq!(
            pick_pageant_pipe(&names, Some("alice")).as_deref(),
            Some(r"\\.\pipe\pageant.alice.deadbeef")
        );
    }

    #[test]
    fn pageant_pipe_match_is_case_insensitive() {
        let names = vec!["Pageant.Alice.ABCD".to_string()];
        assert_eq!(
            pick_pageant_pipe(&names, Some("alice")).as_deref(),
            // Original casing preserved in the returned path.
            Some(r"\\.\pipe\Pageant.Alice.ABCD")
        );
    }

    #[test]
    fn pageant_pipe_user_segment_boundary() {
        // `alice` must not match another user whose name starts with it.
        let names = vec!["pageant.alice2.cafe".to_string()];
        assert_eq!(pick_pageant_pipe(&names, Some("alice")), None);
    }

    #[test]
    fn pageant_pipe_ignores_non_pageant_pipes() {
        let names = vec![
            "openssh-ssh-agent".to_string(),
            "discord-ipc-0".to_string(),
        ];
        assert_eq!(pick_pageant_pipe(&names, Some("alice")), None);
    }

    #[test]
    fn pageant_pipe_unknown_user_accepts_any_pageant() {
        let names = vec!["pageant.bob.f00d".to_string()];
        assert_eq!(
            pick_pageant_pipe(&names, None).as_deref(),
            Some(r"\\.\pipe\pageant.bob.f00d")
        );
        // ...but still requires the `<user>.<guid>` shape, not a bare prefix.
        assert_eq!(pick_pageant_pipe(&["pageant.".to_string()], None), None);
        assert_eq!(
            pick_pageant_pipe(&["pageant.solo".to_string()], None),
            None
        );
    }

    #[test]
    fn pageant_pipe_empty_list_is_none() {
        assert_eq!(pick_pageant_pipe(&[], Some("alice")), None);
    }

    #[test]
    fn parse_addr_ipv6() {
        let (host, port) = parse_addr("[::1]:22").unwrap();
        assert_eq!(host, "[::1]");
        assert_eq!(port, 22);
    }

    #[test]
    fn engine_new() {
        let engine = SshEngine::new();
        assert!(engine.host_key_check.is_none());
        assert!(engine.host_key_ask_tx.is_none());
    }

    #[test]
    fn engine_with_callback() {
        let cb: HostKeyCheckCallback = Arc::new(|_h, _p, _t, _f| HostKeyStatus::Known);
        let engine = SshEngine::new().with_host_key_check(cb);
        assert!(engine.host_key_check.is_some());
    }

    // (Personal integration test against a private SSH server was
    // removed, it had hardcoded credentials and a path that only
    // existed on the original author's machine, and didn't compile in
    // CI anyway. End-to-end SSH coverage now lives in the
    // `tests/` directory once the harness is wired.)

    #[test]
    fn http_connect_request_unauthenticated() {
        let req = build_http_connect_request("example.com", 443, None, None);
        assert!(req.starts_with("CONNECT example.com:443 HTTP/1.1\r\n"));
        assert!(req.contains("Host: example.com:443\r\n"));
        assert!(!req.contains("Proxy-Authorization"));
        assert!(req.ends_with("\r\n\r\n"));
    }

    #[test]
    fn http_connect_request_with_basic_auth() {
        // RFC 7617, "Aladdin:open sesame" → "QWxhZGRpbjpvcGVuIHNlc2FtZQ=="
        let req = build_http_connect_request("h", 22, Some("Aladdin"), Some("open sesame"));
        assert!(req.contains("Proxy-Authorization: Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ==\r\n"));
    }

    #[test]
    fn http_connect_request_with_user_no_password() {
        // No password → empty after colon (per RFC 7617).
        let req = build_http_connect_request("h", 22, Some("u"), None);
        // "u:" base64 = "dTo="
        assert!(req.contains("Proxy-Authorization: Basic dTo=\r\n"));
    }

    #[test]
    fn parse_http_status_ok() {
        assert_eq!(parse_http_status(b"HTTP/1.1 200 Connection established\r\n\r\n"), Some(200));
        assert_eq!(parse_http_status(b"HTTP/1.0 407 Proxy Authentication Required\r\n"), Some(407));
        assert_eq!(parse_http_status(b"HTTP/1.1 502 Bad Gateway\r\n"), Some(502));
    }

    #[test]
    fn parse_http_status_garbage() {
        assert_eq!(parse_http_status(b""), None);
        assert_eq!(parse_http_status(b"not http"), None);
        assert_eq!(parse_http_status(b"HTTP/1.1\r\n"), None);
    }
}
