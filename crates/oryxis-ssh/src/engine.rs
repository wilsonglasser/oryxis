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

/// Callback to verify/store host keys (TOFU).
pub type HostKeyCallback = Arc<dyn Fn(&str, u16, &str, &str) -> bool + Send + Sync>;

struct ClientHandler {
    hostname: String,
    port: u16,
    host_key_cb: Option<HostKeyCallback>,
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

        if let Some(ref cb) = self.host_key_cb {
            Ok(cb(&self.hostname, self.port, &key_type, &fingerprint))
        } else {
            Ok(true) // accept all if no callback
        }
    }
}

// ---------------------------------------------------------------------------
// SSH Handle (opaque wrapper for step-by-step connection)
// ---------------------------------------------------------------------------

/// Opaque handle to an SSH connection after transport is established.
/// Used between `establish_transport` and `do_authenticate` / `open_session`.
pub struct SshHandle(client::Handle<ClientHandler>);

type SharedHandle = Arc<tokio::sync::Mutex<client::Handle<ClientHandler>>>;

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
    _reader_task: tokio::task::JoinHandle<()>,
    _writer_task: tokio::task::JoinHandle<()>,
    _port_forward_tasks: Vec<tokio::task::JoinHandle<()>>,
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

    pub fn is_alive(&self) -> bool {
        !self.writer_tx.is_closed()
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
    host_key_cb: Option<HostKeyCallback>,
}

impl Default for SshEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl SshEngine {
    pub fn new() -> Self {
        Self { host_key_cb: None }
    }

    /// Set a callback for host key verification (TOFU).
    /// Callback receives (hostname, port, key_type, fingerprint) and returns true to accept.
    pub fn with_host_key_cb(mut self, cb: HostKeyCallback) -> Self {
        self.host_key_cb = Some(cb);
        self
    }

    fn make_handler(&self, hostname: &str, port: u16) -> ClientHandler {
        ClientHandler {
            hostname: hostname.into(),
            port,
            host_key_cb: self.host_key_cb.clone(),
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

    /// Connect with a resolver for jump host credentials.
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

        tracing::info!("SSH connecting to {}", addr);

        let handle = if !connection.jump_chain.is_empty() {
            self.connect_via_jump_hosts(connection, resolver, &addr).await?
        } else if let Some(proxy) = &connection.proxy {
            self.connect_via_proxy(proxy, target_host, target_port).await?
        } else {
            let config = Arc::new(client::Config::default());
            let handler = self.make_handler(target_host, target_port);
            client::connect(config, &addr, handler)
                .await
                .map_err(|e| SshError::ConnectionFailed(format!("{}: {}", addr, e)))?
        };

        self.authenticate_and_open(handle, connection, password, private_key_pem, cols, rows)
            .await
    }

    /// Step 1: Establish TCP transport (direct, proxy, or jump host).
    /// Returns an opaque handle after successful TCP connection + SSH handshake + host key verification.
    pub async fn establish_transport(
        &self,
        connection: &Connection,
        resolver: Option<&ConnectionResolver>,
    ) -> Result<SshHandle, SshError> {
        let target_host = &connection.hostname;
        let target_port = connection.port;
        let addr = format!("{}:{}", target_host, target_port);

        tracing::info!("SSH connecting to {}", addr);

        let handle = if !connection.jump_chain.is_empty() {
            self.connect_via_jump_hosts(connection, resolver, &addr).await?
        } else if let Some(proxy) = &connection.proxy {
            self.connect_via_proxy(proxy, target_host, target_port).await?
        } else {
            let config = Arc::new(client::Config::default());
            let handler = self.make_handler(target_host, target_port);
            client::connect(config, &addr, handler)
                .await
                .map_err(|e| SshError::ConnectionFailed(format!("{}: {}", addr, e)))?
        };
        Ok(SshHandle(handle))
    }

    /// Step 2: Authenticate on an established handle.
    pub async fn do_authenticate(
        &self,
        handle: &mut SshHandle,
        connection: &Connection,
        password: Option<&str>,
        private_key_pem: Option<&str>,
    ) -> Result<(), SshError> {
        self.authenticate_handle(&mut handle.0, connection, password, private_key_pem).await
    }

    /// Step 3: Open PTY session on an authenticated handle.
    pub async fn open_session(
        &self,
        handle: SshHandle,
        cols: u32,
        rows: u32,
        port_forwards: &[PortForward],
    ) -> Result<(SshSession, mpsc::UnboundedReceiver<Vec<u8>>), SshError> {
        let listeners = bind_port_forward_listeners(port_forwards).await?;
        self.open_pty_session(handle.0, cols, rows, listeners).await
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
                    // SOCKS5 with auth (password from proxy config not stored yet, use empty)
                    tokio_socks::tcp::Socks5Stream::connect_with_password(
                        proxy_addr.as_str(),
                        (target_host, target_port),
                        user.as_str(),
                        "",
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

                let config = Arc::new(client::Config::default());
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

                let config = Arc::new(client::Config::default());
                client::connect_stream(config, stream, self.make_handler(target_host, target_port))
                    .await
                    .map_err(|e| SshError::Proxy(format!("SSH over SOCKS4: {}", e)))
            }
            ProxyType::Http => {
                let stream = self
                    .http_connect_tunnel(&proxy_addr, target_host, target_port)
                    .await?;

                let config = Arc::new(client::Config::default());
                client::connect_stream(config, stream, self.make_handler(target_host, target_port))
                    .await
                    .map_err(|e| SshError::Proxy(format!("SSH over HTTP CONNECT: {}", e)))
            }
            ProxyType::Command(cmd) => {
                let stream = self.proxy_command(cmd).await?;

                let config = Arc::new(client::Config::default());
                client::connect_stream(config, stream, self.make_handler(target_host, target_port))
                    .await
                    .map_err(|e| SshError::Proxy(format!("SSH over ProxyCommand: {}", e)))
            }
        }
    }

    /// HTTP CONNECT tunnel — establish a TCP tunnel through an HTTP proxy.
    async fn http_connect_tunnel(
        &self,
        proxy_addr: &str,
        target_host: &str,
        target_port: u16,
    ) -> Result<TcpStream, SshError> {
        let mut stream = TcpStream::connect(proxy_addr)
            .await
            .map_err(|e| SshError::Proxy(format!("HTTP proxy connect: {}", e)))?;

        let connect_req = format!(
            "CONNECT {}:{} HTTP/1.1\r\nHost: {}:{}\r\n\r\n",
            target_host, target_port, target_host, target_port
        );

        stream
            .write_all(connect_req.as_bytes())
            .await
            .map_err(|e| SshError::Proxy(format!("HTTP CONNECT write: {}", e)))?;

        // Read response
        let mut buf = vec![0u8; 1024];
        let n = tokio::io::AsyncReadExt::read(&mut stream, &mut buf)
            .await
            .map_err(|e| SshError::Proxy(format!("HTTP CONNECT read: {}", e)))?;

        let response = String::from_utf8_lossy(&buf[..n]);
        if !response.contains("200") {
            return Err(SshError::Proxy(format!(
                "HTTP CONNECT failed: {}",
                response.lines().next().unwrap_or("unknown")
            )));
        }

        tracing::info!("HTTP CONNECT tunnel established");
        Ok(stream)
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
        let config = Arc::new(client::Config::default());
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
            let config = Arc::new(client::Config::default());
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
        let config = Arc::new(client::Config::default());
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
        self.authenticate_handle(&mut handle, connection, password, private_key_pem)
            .await?;
        let listeners = bind_port_forward_listeners(&connection.port_forwards).await?;
        self.open_pty_session(handle, cols, rows, listeners).await
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
            loop {
                match channel.wait().await {
                    Some(ChannelMsg::Data { data }) => stdout.extend_from_slice(&data),
                    Some(ChannelMsg::ExtendedData { data, ext }) => {
                        if ext == 1 {
                            stderr.extend_from_slice(&data);
                        }
                    }
                    Some(ChannelMsg::ExitStatus { exit_status }) => {
                        exit_code = Some(exit_status);
                    }
                    Some(ChannelMsg::Eof) | None => break,
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

        // Request shell
        channel.request_shell(false).await
            .map_err(|e| SshError::Channel(format!("Shell request failed: {}", e)))?;

        // I/O bridging
        let (output_tx, output_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (writer_tx, mut writer_rx) = mpsc::unbounded_channel::<Vec<u8>>();

        let mut channel_writer = channel.make_writer();

        // Reader task
        let reader_task = tokio::spawn(async move {
            let mut channel = channel;
            loop {
                match channel.wait().await {
                    Some(ChannelMsg::Data { data }) => {
                        if output_tx.send(data.to_vec()).is_err() {
                            break;
                        }
                    }
                    Some(ChannelMsg::ExtendedData { data, ext }) => {
                        if ext == 1
                            && output_tx.send(data.to_vec()).is_err() {
                                break;
                            }
                    }
                    Some(ChannelMsg::ExitStatus { exit_status }) => {
                        tracing::info!("Remote exited with status {}", exit_status);
                        break;
                    }
                    Some(ChannelMsg::Eof) | None => {
                        tracing::info!("SSH channel closed");
                        break;
                    }
                    _ => {}
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
                _reader_task: reader_task,
                _writer_task: writer_task,
                _port_forward_tasks: pf_tasks,
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
        assert!(engine.host_key_cb.is_none());
    }

    #[test]
    fn engine_with_callback() {
        let cb: HostKeyCallback = Arc::new(|_h, _p, _t, _f| true);
        let engine = SshEngine::new().with_host_key_cb(cb);
        assert!(engine.host_key_cb.is_some());
    }

    /// Integration test: connect to a real SSH server.
    /// Run with: cargo test -p oryxis-ssh -- --ignored real_ssh
    #[tokio::test]
    #[ignore]
    async fn real_ssh_connect_with_key() {
        let key_pem = std::fs::read_to_string("/home/wilson/Chaves/spmundi-nova")
            .expect("Key file not found");

        // Step 1: test key decode
        let private_key = match russh::keys::decode_secret_key(&key_pem, None) {
            Ok(kp) => {
                println!("Key decoded OK: {:?}", kp.algorithm());
                kp
            }
            Err(e) => panic!("Key decode FAILED: {}", e),
        };

        // Step 2: raw TCP + handshake
        let config = Arc::new(client::Config::default());
        let handler = ClientHandler {
            hostname: "167.172.251.123".into(),
            port: 22,
            host_key_cb: Some(Arc::new(|_h, _p, _t, _f| true)),
        };
        let mut handle = match client::connect(config, "167.172.251.123:22", handler).await {
            Ok(h) => {
                println!("TCP + handshake OK");
                h
            }
            Err(e) => panic!("Connect FAILED: {}", e),
        };

        // Step 3: try publickey with rsa-sha2-256
        println!("Key algorithm: {:?}", private_key.algorithm());
        println!("Key public algorithm: {:?}", private_key.public_key().algorithm());
        let key = PrivateKeyWithHashAlg::new(
            Arc::new(private_key),
            Some(HashAlg::Sha256),
        );
        match handle.authenticate_publickey("root", key).await {
            Ok(res) if res.success() => println!("Publickey auth SUCCESS"),
            Ok(_) => println!("Publickey auth REJECTED"),
            Err(e) => println!("Publickey auth ERROR: {}", e),
        }

        // Step 4: try password
        match handle.authenticate_password("root", "Wrg575488$").await {
            Ok(res) if res.success() => println!("Password auth SUCCESS"),
            Ok(_) => println!("Password auth REJECTED"),
            Err(e) => println!("Password auth ERROR: {}", e),
        }
    }
}
