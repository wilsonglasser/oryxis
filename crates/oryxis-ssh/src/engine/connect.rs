use super::*;

impl SshEngine {
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
        key_material: Option<KeyMaterial<'_>>,
        cols: u32,
        rows: u32,
    ) -> Result<(SshSession, mpsc::UnboundedReceiver<Vec<u8>>), SshError> {
        self.connect_with_resolver(connection, password, key_material, cols, rows, None)
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
    pub(crate) async fn dial(
        &self,
        connection: &Connection,
        resolver: Option<&ConnectionResolver>,
    ) -> Result<client::Handle<ClientHandler>, SshError> {
        let target_host = &connection.hostname;
        let target_port = connection.port;
        // Brackets bare IPv6 literals; hostnames/IPv4 pass through.
        let addr = oryxis_core::net::host_port(target_host, target_port);
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
                self.connect_via_proxy(
                    proxy,
                    &ProxyTokens::for_dial(connection),
                    self.address_family,
                )
                .await
            } else {
                let config = self.make_config();
                let handler = self.make_handler(target_host, target_port);
                // Dial ourselves (instead of `client::connect`) so the
                // socket honors the address-family preference and gets
                // TCP_NODELAY before the SSH handshake starts.
                let stream = self.dial_tcp(&addr, self.address_family).await?;
                client::connect_stream(config, stream, handler)
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
        key_material: Option<KeyMaterial<'_>>,
        cols: u32,
        rows: u32,
        resolver: Option<&ConnectionResolver>,
    ) -> Result<(SshSession, mpsc::UnboundedReceiver<Vec<u8>>), SshError> {
        let handle = self.dial(connection, resolver).await?;

        self.authenticate_and_open(handle, connection, password, key_material, cols, rows)
            .await
    }

    /// Dial + authenticate a probe-only [`MonitorConn`] for the
    /// multi-host monitor dashboard (issue #95): the full transport
    /// path (proxy, jump chain, host-key verification, TOTP autofill)
    /// with no PTY and no shell, so the host only ever sees the login
    /// and the per-poll exec channels. Headless by design: a host
    /// whose auth needs an interactive answer the configured
    /// credentials can't give fails here instead of prompting.
    pub async fn connect_monitor(
        &self,
        connection: &Connection,
        password: Option<&str>,
        key_material: Option<KeyMaterial<'_>>,
        resolver: Option<&ConnectionResolver>,
    ) -> Result<super::MonitorConn, SshError> {
        let mut handle = self.dial(connection, resolver).await?;
        self.authenticate_handle_bounded(&mut handle, connection, password, key_material)
            .await?;
        Ok(super::MonitorConn::new(handle))
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
    /// timeout (default 120s, matching sshd's LoginGraceTime) so a
    /// misbehaving server wedging mid-handshake can't hang the connect
    /// flow forever, without cutting legitimate slow auth short.
    pub async fn do_authenticate(
        &self,
        handle: &mut SshHandle,
        connection: &Connection,
        password: Option<&str>,
        key_material: Option<KeyMaterial<'_>>,
    ) -> Result<(), SshError> {
        self.authenticate_handle_bounded(&mut handle.0, connection, password, key_material)
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
    pub(crate) async fn authenticate_handle_bounded(
        &self,
        handle: &mut client::Handle<ClientHandler>,
        connection: &Connection,
        password: Option<&str>,
        key_material: Option<KeyMaterial<'_>>,
    ) -> Result<(), SshError> {
        // Interactive and PasswordPrompt both park on human input, which
        // routinely exceeds any network bound. Their network round-trips
        // are capped individually inside the auth path instead, so the
        // blanket `auth_timeout` is skipped here for both. Auto joins them
        // when the quick-connect interactive fallback can prompt: its tail
        // may park on the same modal.
        //
        // The RFC 4252 partial-success continuation can pop the 2FA modal
        // under any method, but it stays under the blanket: the default
        // 120s mirrors sshd's LoginGraceTime, i.e. the server would drop
        // a slower typist anyway, and the TOTP autofill answers the
        // common case without any human wait at all.
        let may_prompt = self.auto_interactive_fallback && self.kbi_ask_tx.is_some();
        if matches!(
            connection.auth_method,
            AuthMethod::Interactive | AuthMethod::PasswordPrompt
        ) || (connection.auth_method == AuthMethod::Auto && may_prompt)
        {
            return self
                .authenticate_handle(handle, connection, password, key_material)
                .await;
        }
        let auth_timeout = self.auth_timeout;
        tokio::time::timeout(
            auth_timeout,
            self.authenticate_handle(handle, connection, password, key_material),
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
            self.open_pty_session(super::SshTransport::new(handle.0), cols, rows, listeners),
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

    /// Open a `-L` forward on an OS-assigned ephemeral local port and
    /// report the port back, so a caller (the RDP/VNC launcher) can
    /// point a client at `127.0.0.1:<port>` with no bind race: the
    /// listener owns the port before we return it. The returned
    /// `ForwardSession` keeps the tunnel up until dropped / cancelled;
    /// its lifetime is deliberately independent of any client process.
    pub async fn connect_local_forward_ephemeral(
        self,
        connection: &Connection,
        password: Option<&str>,
        key_material: Option<KeyMaterial<'_>>,
        target_host: &str,
        target_port: u16,
        resolver: Option<&ConnectionResolver>,
    ) -> Result<(ForwardSession, u16), SshError> {
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        let cancel_tx = Arc::new(cancel_tx);
        let mut handle = self.establish_transport(connection, resolver).await?;
        self.do_authenticate(&mut handle, connection, password, key_material)
            .await?;
        let shared = Arc::new(tokio::sync::Mutex::new(handle.0));

        // Port 0 -> the OS picks a free port; read it back from the bound
        // listener before spawning, so what we return is what's bound.
        let listener = bind_forward_listener("127.0.0.1", 0).await?;
        let local_port = listener
            .local_addr()
            .map_err(|e| SshError::Channel(format!("forward local_addr: {e}")))?
            .port();
        // Auto-close: unlike a saved `-L` rule, this tunnel exists only to
        // carry one desktop session. Once it has served a connection and then
        // sits idle (client window closed), tear it down so the SSH handle
        // doesn't linger. Independent of any client process, so it works
        // uniformly for blocking viewers (xfreerdp) and handoff launchers
        // (`open rdp://`, remmina) alike.
        let task = spawn_autoclose_local_forward_task(
            listener,
            Arc::clone(&shared),
            target_host.to_string(),
            target_port,
            local_port,
            cancel_rx,
            Arc::clone(&cancel_tx),
            AutoClose::on_idle(RD_TUNNEL_IDLE_GRACE),
        );
        tracing::info!(
            "forward(-L ephemeral) 127.0.0.1:{} -> {}:{} up",
            local_port, target_host, target_port
        );
        Ok((
            ForwardSession {
                handle: shared,
                cancel_tx,
                _tasks: vec![task],
                remote_bind: None,
                remote_route: None,
            },
            local_port,
        ))
    }

    /// Establish a dedicated, PTY-less SSH connection for the port
    /// forwards of one host, with no rule attached yet. Runs the same
    /// transport + auth ladder as a terminal connect; rules then attach
    /// via `ForwardConn::attach`, each as channels multiplexed on this
    /// single connection (issue #126), the way OpenSSH stacks
    /// `-L`/`-R`/`-D` flags on one `ssh` invocation.
    ///
    /// Consumes `self` because the `-R` routing table must be installed
    /// on the handler *before* the transport (and thus the handler) is
    /// created — and any rule kind may attach later, so it is always
    /// installed.
    pub async fn connect_forward_conn(
        mut self,
        connection: &Connection,
        password: Option<&str>,
        key_material: Option<KeyMaterial<'_>>,
        resolver: Option<&ConnectionResolver>,
    ) -> Result<ForwardConn, SshError> {
        let routes: RemoteRouteMap = Arc::default();
        self.remote_routes = Some(Arc::clone(&routes));
        let mut handle = self.establish_transport(connection, resolver).await?;
        self.do_authenticate(&mut handle, connection, password, key_material)
            .await?;
        Ok(ForwardConn {
            handle: Arc::new(tokio::sync::Mutex::new(handle.0)),
            remote_routes: routes,
        })
    }

    /// One-shot convenience: open a forward connection and attach a single
    /// rule. The returned `ForwardSession` holds the connection open until
    /// cancelled. Callers that may run several rules against the same host
    /// should hold the `ForwardConn` from `connect_forward_conn` and
    /// attach each rule instead of calling this repeatedly.
    pub async fn connect_forward(
        self,
        connection: &Connection,
        password: Option<&str>,
        key_material: Option<KeyMaterial<'_>>,
        rule: &PortForwardRule,
        resolver: Option<&ConnectionResolver>,
    ) -> Result<ForwardSession, SshError> {
        let conn = self
            .connect_forward_conn(connection, password, key_material, resolver)
            .await?;
        conn.attach(rule).await
    }

    // -----------------------------------------------------------------------
    // Transport resolvers
    // -----------------------------------------------------------------------

    /// Connect via SOCKS or HTTP proxy. `family` governs the socket to
    /// the PROXY (the only dial this machine makes on this path); it is
    /// the target connection's preference, or the bastion's when the
    /// proxied hop is a jump chain's first host.
    ///
    /// `dial` names the target. The SOCKS and HTTP branches read only
    /// its host and port; the whole of it exists because a
    /// `ProxyType::Command` line can also name the login (`%r`) and the
    /// connection's own name (`%n`), and those have to come from the
    /// connection being dialed rather than be re-derived here.
    pub(crate) async fn connect_via_proxy(
        &self,
        proxy: &ProxyConfig,
        dial: &ProxyTokens<'_>,
        family: AddressFamily,
    ) -> Result<client::Handle<ClientHandler>, SshError> {
        let (target_host, target_port) = (dial.host, dial.port);
        let proxy_addr = oryxis_core::net::host_port(&proxy.host, proxy.port);
        tracing::info!("Connecting via {:?} proxy at {}", proxy.proxy_type, proxy_addr);

        match &proxy.proxy_type {
            ProxyType::Socks5 => {
                // Dial the proxy ourselves (family + TCP_NODELAY), then
                // run the SOCKS handshake over the prepared socket.
                let socket = self
                    .dial_tcp(&proxy_addr, family)
                    .await
                    .map_err(|e| SshError::Proxy(format!("SOCKS5 proxy connect: {}", e)))?;
                let stream = if let Some(user) = &proxy.username {
                    // SOCKS5 username/password auth (RFC 1929). Password
                    // is hydrated from the vault before this call; if
                    // the user configured no password, send an empty
                    // one, the proxy may still accept it.
                    tokio_socks::tcp::Socks5Stream::connect_with_password_and_socket(
                        socket,
                        (target_host, target_port),
                        user.as_str(),
                        proxy.password.as_deref().unwrap_or(""),
                    )
                    .await
                    .map_err(|e| SshError::Proxy(format!("SOCKS5 auth: {}", e)))?
                } else {
                    tokio_socks::tcp::Socks5Stream::connect_with_socket(
                        socket,
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
                let socket = self
                    .dial_tcp(&proxy_addr, family)
                    .await
                    .map_err(|e| SshError::Proxy(format!("SOCKS4 proxy connect: {}", e)))?;
                let stream = if let Some(user) = &proxy.username {
                    tokio_socks::tcp::Socks4Stream::connect_with_userid_and_socket(
                        socket,
                        (target_host, target_port),
                        user.as_str(),
                    )
                    .await
                    .map_err(|e| SshError::Proxy(format!("SOCKS4: {}", e)))?
                } else {
                    tokio_socks::tcp::Socks4Stream::connect_with_socket(
                        socket,
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
                        family,
                    )
                    .await?;

                let config = self.make_config();
                client::connect_stream(config, stream, self.make_handler(target_host, target_port))
                    .await
                    .map_err(|e| SshError::Proxy(format!("SSH over HTTP CONNECT: {}", e)))
            }
            ProxyType::Command(cmd) => {
                let (stream, stderr) = self.proxy_command(cmd, dial).await?;

                let config = self.make_config();
                match client::connect_stream(
                    config,
                    stream,
                    self.make_handler(target_host, target_port),
                )
                .await
                {
                    Ok(handle) => Ok(handle),
                    // russh only ever saw the transport end. The reason
                    // it ended is on the proxy's stderr, so the error
                    // carries it rather than leaving an unexplained
                    // "Disconnected" on screen and the account of it in
                    // a log file nobody was told to open.
                    Err(e) => Err(ProxyCommandError::Transport {
                        transport: e.to_string(),
                        stderr: stderr.settled_tail().await,
                    }
                    .into()),
                }
            }
        }
    }

    /// HTTP CONNECT tunnel, establish a TCP tunnel through an HTTP proxy.
    /// Supports Basic auth (RFC 7617) when `username` is provided.
    pub(crate) async fn http_connect_tunnel(
        &self,
        proxy_addr: &str,
        target_host: &str,
        target_port: u16,
        username: Option<&str>,
        password: Option<&str>,
        family: AddressFamily,
    ) -> Result<TcpStream, SshError> {
        let mut stream = self
            .dial_tcp(proxy_addr, family)
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
    /// Spawn a `ProxyCommand` and hand the SSH transport its pipes.
    ///
    /// This is the one place in the product where stored connection data
    /// becomes a LOCAL process, and it runs before the handshake, so
    /// neither host-key verification nor a failed auth can undo it. The
    /// data reaching it is not necessarily the local user's: a sync peer
    /// writes connections, proxy identities and group defaults verbatim,
    /// and a group default lands on every host in the group that has no
    /// proxy of its own. So the spawn asks first, and an engine with
    /// nobody to ask refuses.
    ///
    /// The line is asked about, and fingerprinted, exactly as stored:
    /// `%h` / `%n` / `%p` / `%r` are resolved by
    /// `proxy_spawn::expand_proxy_tokens` only once consent is in
    /// hand, so one approval covers every host that shares the proxy and
    /// the values filling those slots are the ones checked there.
    ///
    /// Hands back the transport and the proxy's stderr sink, which is
    /// the only account of why the transport dies when it does.
    pub(crate) async fn proxy_command(
        &self,
        cmd: &str,
        dial: &ProxyTokens<'_>,
    ) -> Result<
        (
            impl AsyncRead + AsyncWrite + Unpin + Send + 'static,
            super::proxy_spawn::ProxyStderr,
        ),
        SshError,
    > {
        let (target_host, target_port) = (dial.host, dial.port);
        // The line itself never reaches the log: it is user-authored and
        // can embed credentials, which is why the connect progress card
        // announces only that a command proxy is in play.
        tracing::info!("ProxyCommand for {}:{}", target_host, target_port);

        let Some(ref tx) = self.proxy_cmd_ask_tx else {
            tracing::warn!(
                "refusing command proxy for {}:{}: no approval channel on this engine",
                target_host,
                target_port
            );
            return Err(SshError::ProxyCommandNotApproved);
        };
        let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
        let query = ProxyCommandQuery {
            command: cmd.to_string(),
            target_host: target_host.to_string(),
            target_port,
        };
        if tx.send((query, resp_tx)).await.is_err() {
            // The UI went away mid-dial. A dropped asker is not consent.
            return Err(SshError::ProxyCommandNotApproved);
        }
        if !resp_rx.await.unwrap_or(false) {
            return Err(SshError::ProxyCommandNotApproved);
        }

        let line = super::proxy_spawn::expand_proxy_tokens(cmd, dial)?;

        let mut child = super::proxy_spawn::spawn_proxy_process(&line)
            .map_err(|e| ProxyCommandError::Spawn(e.to_string()))?;

        // The proxy's own complaints are the only account of why a dial
        // through it failed; without them a bad profile or an expired
        // token reads as an unexplained EOF in the version exchange.
        let stderr = match child.stderr.take() {
            Some(stderr) => super::proxy_spawn::watch_proxy_stderr(
                stderr,
                target_host.to_string(),
                target_port,
            ),
            None => Default::default(),
        };

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| SshError::Proxy("ProxyCommand: no stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| SshError::Proxy("ProxyCommand: no stdout".into()))?;

        Ok((tokio::io::join(stdout, stdin), stderr))
    }

    /// Connect via jump hosts (SSH tunneling through bastion hosts).
    pub(crate) async fn connect_via_jump_hosts(
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

        let first_addr = oryxis_core::net::host_port(&first_jump.hostname, first_jump.port);
        let mut current_handle = if let Some(first_proxy) = resolver.proxies.get(&first_jump_id) {
            tracing::info!(
                "First jump host {} sits behind {:?} proxy",
                first_addr,
                first_proxy.proxy_type
            );
            self.connect_via_proxy(
                first_proxy,
                &ProxyTokens::for_dial(first_jump),
                first_jump.address_family,
            )
            .await
            .map_err(|e| SshError::JumpHost(format!("Jump host {} via proxy: {}", first_addr, e)))?
        } else {
            let config = self.make_config();
            let handler = self.make_handler(&first_jump.hostname, first_jump.port);
            // The socket goes to the BASTION, so its address-family
            // preference (not the target's) governs this dial.
            let stream = self.dial_tcp(&first_addr, first_jump.address_family).await
                .map_err(|e| SshError::JumpHost(format!("Jump host {}: {}", first_addr, e)))?;
            client::connect_stream(config, stream, handler)
                .await
                .map_err(|e| SshError::JumpHost(format!("Jump host {}: {}", first_addr, e)))?
        };

        // Authenticate on first jump host (its own key + optional cert).
        let first_pw = resolver.passwords.get(&first_jump_id);
        let first_cert = resolver.certificates.get(&first_jump_id).map(String::as_str);
        let first_km = resolver
            .private_keys
            .get(&first_jump_id)
            .map(|pem| KeyMaterial::new(pem, first_cert));
        self.authenticate_handle(
            &mut current_handle,
            first_jump,
            first_pw.map(String::as_str),
            first_km,
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
            let jump_cert = resolver.certificates.get(&jump_id).map(String::as_str);
            let jump_km = resolver
                .private_keys
                .get(&jump_id)
                .map(|pem| KeyMaterial::new(pem, jump_cert));
            self.authenticate_handle(
                &mut current_handle,
                jump,
                jump_pw.map(String::as_str),
                jump_km,
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

}
