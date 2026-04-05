use std::sync::Arc;

use async_trait::async_trait;
use russh::keys::ssh_key::PublicKey;
use russh::{client, ChannelMsg};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::process::Command as TokioCommand;
use tokio::sync::mpsc;

use oryxis_core::models::connection::{AuthMethod, Connection, ProxyConfig, ProxyType};
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

#[async_trait]
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
// SSH Session
// ---------------------------------------------------------------------------

/// A live SSH session with a remote PTY channel.
pub struct SshSession {
    _handle: client::Handle<ClientHandler>,
    writer_tx: mpsc::UnboundedSender<Vec<u8>>,
    _reader_task: tokio::task::JoinHandle<()>,
    _writer_task: tokio::task::JoinHandle<()>,
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

        // 1. Resolve transport: jump hosts → proxy → direct TCP
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

        // 2. Authenticate + open session
        self.authenticate_and_open(handle, connection, password, private_key_pem, cols, rows)
            .await
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
        let authenticated = self
            .do_auth(handle, username, &connection.auth_method, password, private_key_pem)
            .await?;

        if !authenticated {
            return Err(SshError::AuthFailed);
        }
        tracing::info!("Authenticated as {} on {}", username, connection.hostname);
        Ok(())
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
            AuthMethod::Password => {
                let pw = password.ok_or(SshError::AuthFailed)?;
                Ok(handle.authenticate_password(username, pw).await?)
            }
            AuthMethod::Key => {
                let pem = private_key_pem
                    .ok_or_else(|| SshError::Key("Private key not provided".into()))?;
                let key_pair = russh_keys::decode_secret_key(pem, None)
                    .map_err(|e| SshError::Key(format!("Failed to decode key: {}", e)))?;
                Ok(handle
                    .authenticate_publickey(username, Arc::new(key_pair))
                    .await?)
            }
            AuthMethod::Agent => {
                self.auth_via_agent(handle, username).await
            }
            AuthMethod::Interactive => {
                let pw = password.unwrap_or("").to_string();
                let resp = handle
                    .authenticate_keyboard_interactive_start(username, None::<String>)
                    .await?;
                match resp {
                    client::KeyboardInteractiveAuthResponse::Success => Ok(true),
                    client::KeyboardInteractiveAuthResponse::Failure => Ok(false),
                    client::KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                        let responses: Vec<String> =
                            prompts.iter().map(|_| pw.clone()).collect();
                        let resp2 = handle
                            .authenticate_keyboard_interactive_respond(responses)
                            .await?;
                        Ok(matches!(
                            resp2,
                            client::KeyboardInteractiveAuthResponse::Success
                        ))
                    }
                }
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
        match russh_keys::agent::client::AgentClient::connect_env().await {
            Ok(mut agent) => {
                let identities = agent
                    .request_identities()
                    .await
                    .map_err(|e| SshError::Key(format!("Agent: {}", e)))?;

                for identity in identities {
                    if let Ok(true) = handle
                        .authenticate_publickey_with(username, identity, &mut agent)
                        .await
                    {
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
        match russh_keys::agent::client::AgentClient::connect_named_pipe(pipe_path).await {
            Ok(mut agent) => {
                let identities = agent
                    .request_identities()
                    .await
                    .map_err(|e| SshError::Key(format!("Agent: {}", e)))?;

                for identity in identities {
                    if let Ok(true) = handle
                        .authenticate_publickey_with(username, identity, &mut agent)
                        .await
                    {
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
        // Authenticate
        self.authenticate_handle(&mut handle, connection, password, private_key_pem)
            .await?;

        // Open session channel
        let channel = handle.channel_open_session().await?;

        // Request PTY
        channel
            .request_pty(false, "xterm-256color", cols, rows, 0, 0, &[])
            .await?;

        // Request shell
        channel.request_shell(false).await?;

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

        Ok((
            SshSession {
                _handle: handle,
                writer_tx,
                _reader_task: reader_task,
                _writer_task: writer_task,
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
}
