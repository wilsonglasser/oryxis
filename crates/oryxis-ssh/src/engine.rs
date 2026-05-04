use std::sync::Arc;

use russh::keys::{PublicKey, HashAlg, PrivateKeyWithHashAlg};
use russh::{client, ChannelMsg};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::process::Command as TokioCommand;
use tokio::sync::mpsc;

use oryxis_core::models::connection::{AuthMethod, Connection, PortForward, ProxyConfig, ProxyType};
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

// ---------------------------------------------------------------------------
// Client handler
// ---------------------------------------------------------------------------

/// Result of checking a host key against known hosts.
#[derive(Debug, Clone)]
pub enum HostKeyStatus {
    /// Host is known and fingerprint matches — accept silently.
    Known,
    /// Host is known but fingerprint CHANGED — potential MITM.
    Changed { old_fingerprint: String },
    /// Host is not known — need to ask the user.
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

pub(crate) struct ClientHandler {
    hostname: String,
    port: u16,
    host_key_check: Option<HostKeyCheckCallback>,
    host_key_ask_tx: Option<HostKeyAskSender>,
    /// Mirrors `SshEngine::agent_forwarding`. The handler uses it as a
    /// gate on `server_channel_open_agent_forward` — without an opt-in,
    /// inbound forward channels are rejected even if the server tries
    /// to open one.
    agent_forwarding: bool,
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
    let pipe_path = r"\\.\pipe\openssh-ssh-agent";
    let mut agent = tokio::net::windows::named_pipe::ClientOptions::new().open(pipe_path)?;
    let mut stream = channel.into_stream();
    let _ = tokio::io::copy_bidirectional(&mut agent, &mut stream).await?;
    Ok(())
}

impl client::Handler for ClientHandler {
    type Error = SshError;

    async fn check_server_key(&mut self, key: &PublicKey) -> Result<bool, Self::Error> {
        let key_type = key.algorithm().to_string();
        let fingerprint = key.fingerprint(russh::keys::ssh_key::HashAlg::Sha256).to_string();

        tracing::info!(
            "Server key for {}:{} — {} {}",
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
                } else {
                    // No UI channel — reject changed, accept unknown (legacy fallback)
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
            // for. Drop it on the floor — `Channel` is closed when it
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
    /// Shared SSH handle — kept alive for port forward tasks to open channels.
    _handle: Arc<tokio::sync::Mutex<client::Handle<ClientHandler>>>,
    writer_tx: mpsc::UnboundedSender<Vec<u8>>,
    /// Forwarded to the SSH channel as `window-change` requests so the
    /// remote shell sees SIGWINCH and re-renders for the new viewport.
    /// Without this, apps like `top` keep rendering for the original
    /// columns and our local alacritty wraps the overflow into extra
    /// rows ("double line" effect).
    resize_tx: mpsc::UnboundedSender<(u16, u16)>,
    _reader_task: tokio::task::JoinHandle<()>,
    _writer_task: tokio::task::JoinHandle<()>,
    _port_forward_tasks: Vec<tokio::task::JoinHandle<()>>,
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

    /// Open a fresh SFTP subsystem channel on this session — the SSH
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

// ---------------------------------------------------------------------------
// SSH Engine
// ---------------------------------------------------------------------------

/// Resolves connections for jump hosts.
pub struct ConnectionResolver {
    pub connections: Vec<Connection>,
    pub passwords: std::collections::HashMap<uuid::Uuid, String>,
    pub private_keys: std::collections::HashMap<uuid::Uuid, String>,
}

pub struct SshEngine {
    host_key_check: Option<HostKeyCheckCallback>,
    host_key_ask_tx: Option<HostKeyAskSender>,
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
            keepalive_interval: None,
            connect_timeout: std::time::Duration::from_secs(15),
            auth_timeout: std::time::Duration::from_secs(30),
            session_timeout: std::time::Duration::from_secs(10),
            agent_forwarding: false,
        }
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

    /// Configure the client-side keepalive interval (zero / `None` disables).
    pub fn with_keepalive(mut self, interval: Option<std::time::Duration>) -> Self {
        self.keepalive_interval = interval.filter(|d| !d.is_zero());
        self
    }

    fn make_config(&self) -> Arc<client::Config> {
        Arc::new(client::Config {
            keepalive_interval: self.keepalive_interval,
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
    pub async fn connect_with_resolver(
        &self,
        connection: &Connection,
        password: Option<&str>,
        private_key_pem: Option<&str>,
        cols: u32,
        rows: u32,
        resolver: Option<&ConnectionResolver>,
    ) -> Result<(SshSession, mpsc::UnboundedReceiver<Vec<u8>>), SshError> {
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
                    .map_err(|e| SshError::ConnectionFailed(format!("{}: {}", addr, e)))
            }
        };
        let handle = tokio::time::timeout(connect_timeout, connect_fut)
            .await
            .map_err(|_| {
                SshError::ConnectionFailed(format!(
                    "{}: timed out after {}s",
                    addr,
                    connect_timeout.as_secs()
                ))
            })??;

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
        let connect_timeout = self.connect_timeout;

        let target_host = &connection.hostname;
        let target_port = connection.port;
        let addr = format!("{}:{}", target_host, target_port);

        tracing::info!("SSH connecting to {} (timeout: {}s)", addr, connect_timeout.as_secs());

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
                    .map_err(|e| SshError::ConnectionFailed(format!("{}: {}", addr, e)))
            }
        };

        let handle = tokio::time::timeout(connect_timeout, connect_fut)
            .await
            .map_err(|_| SshError::ConnectionFailed(format!(
                "{}: timed out after {}s", addr, connect_timeout.as_secs()
            )))??;
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
        let auth_timeout = self.auth_timeout;
        tokio::time::timeout(
            auth_timeout,
            self.authenticate_handle(&mut handle.0, connection, password, private_key_pem),
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
                    // one — the proxy may still accept it.
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

    /// HTTP CONNECT tunnel — establish a TCP tunnel through an HTTP proxy.
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
        // or chunked proxy may split it — loop until we have headers or hit
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

    /// ProxyCommand — spawn a process and use its stdin/stdout as transport.
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

        // Connect to the first jump host directly
        let first_jump_id = connection.jump_chain[0];
        let first_jump = resolver
            .connections
            .iter()
            .find(|c| c.id == first_jump_id)
            .ok_or_else(|| SshError::JumpHost("First jump host not found".into()))?;

        let first_addr = format!("{}:{}", first_jump.hostname, first_jump.port);
        let config = self.make_config();
        let handler = self.make_handler(&first_jump.hostname, first_jump.port);
        let mut current_handle = client::connect(config, &first_addr, handler)
            .await
            .map_err(|e| SshError::JumpHost(format!("Jump host {}: {}", first_addr, e)))?;

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

                    // 4. Try keyboard-interactive with password
                    tried.push("keyboard-interactive");
                    tracing::info!("Auto: trying keyboard-interactive auth for {}", username);
                    if self.try_keyboard_interactive(handle, username, pw).await? {
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

                // Key was rejected — try password as fallback if available
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
                let pw = password.unwrap_or("");
                tracing::info!("Trying keyboard-interactive auth for {}", username);
                if self.try_keyboard_interactive(handle, username, pw).await? {
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

    /// Try keyboard-interactive auth with a password response.
    async fn try_keyboard_interactive(
        &self,
        handle: &mut client::Handle<ClientHandler>,
        username: &str,
        pw: &str,
    ) -> Result<bool, SshError> {
        let resp = handle
            .authenticate_keyboard_interactive_start(username, None::<String>)
            .await?;
        match resp {
            client::KeyboardInteractiveAuthResponse::Success => Ok(true),
            client::KeyboardInteractiveAuthResponse::Failure { .. } => Ok(false),
            client::KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                let responses: Vec<String> = prompts.iter().map(|_| pw.to_string()).collect();
                let resp2 = handle
                    .authenticate_keyboard_interactive_respond(responses)
                    .await?;
                Ok(matches!(resp2, client::KeyboardInteractiveAuthResponse::Success))
            }
        }
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
        let pipe_path = r"\\.\pipe\openssh-ssh-agent";
        match russh::keys::agent::client::AgentClient::connect_named_pipe(pipe_path).await {
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
                "Windows OpenSSH Agent not available ({}): {}",
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
        // — single-call connects via `connect_with_resolver` were
        // bypassing them, leaving auth/session free to hang on the OS
        // default ceilings.
        let auth_timeout = self.auth_timeout;
        let session_timeout = self.session_timeout;
        tokio::time::timeout(
            auth_timeout,
            self.authenticate_handle(&mut handle, connection, password, private_key_pem),
        )
        .await
        .map_err(|_| {
            SshError::ConnectionFailed(format!(
                "auth timed out after {}s",
                auth_timeout.as_secs()
            ))
        })??;
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
            // Read until channel close (`None`), not just Eof — some
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
        channel
            .request_pty(false, "xterm-256color", cols, rows, 0, 0, &[])
            .await
            .map_err(|e| SshError::Channel(format!("PTY request failed: {}", e)))?;

        // Optional ssh-agent forwarding. Must fire BEFORE `request_shell`
        // — sshd reads the channel requests in order and only sets
        // `SSH_AUTH_SOCK` on the launched process if forwarding was
        // already requested when the shell starts. Issued without
        // `want_reply`; failures (server has `AllowAgentForwarding no`)
        // are not fatal — the user still gets a normal shell, they
        // just can't hop further with their local keys.
        if self.agent_forwarding
            && let Err(e) = channel.agent_forward(false).await
        {
            tracing::warn!("agent_forward request failed (non-fatal): {}", e);
        }

        // Request shell
        channel.request_shell(false).await
            .map_err(|e| SshError::Channel(format!("Shell request failed: {}", e)))?;

        // I/O bridging
        let (output_tx, output_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (writer_tx, mut writer_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (resize_tx, mut resize_rx) = mpsc::unbounded_channel::<(u16, u16)>();

        let mut channel_writer = channel.make_writer();

        // Reader task — multiplexes incoming PTY data with outgoing
        // window-change requests so we only own `channel` in one place.
        let reader_task = tokio::spawn(async move {
            let mut channel = channel;
            loop {
                tokio::select! {
                    msg = channel.wait() => {
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
                        if let Some(b) = bytes
                            && output_tx.send(b).is_err()
                        {
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
                _reader_task: reader_task,
                _writer_task: writer_task,
                _port_forward_tasks: pf_tasks,
                // Default — overridden by the engine right after this
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
/// 7617). `password` may be `None` or empty — the colon separator is
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
    // removed — it had hardcoded credentials and a path that only
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
        // RFC 7617 — "Aladdin:open sesame" → "QWxhZGRpbjpvcGVuIHNlc2FtZQ=="
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
