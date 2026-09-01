use super::*;

// ---------------------------------------------------------------------------
// SSH Handle (opaque wrapper for step-by-step connection)
// ---------------------------------------------------------------------------

/// Opaque handle to an SSH connection after transport is established.
/// Used between `establish_transport` and `do_authenticate` / `open_session`.
pub struct SshHandle(pub(crate) client::Handle<ClientHandler>);

pub(crate) type SharedHandle = Arc<tokio::sync::Mutex<client::Handle<ClientHandler>>>;

/// The routing table proper: (bind address, bind port) as requested via
/// `tcpip_forward` -> the drain of the `-R` rule that owns that
/// server-side listener.
pub(crate) type RemoteRoutes = std::collections::HashMap<
    (String, u16),
    tokio::sync::mpsc::UnboundedSender<russh::Channel<russh::client::Msg>>,
>;

/// Shared routing table for inbound `forwarded-tcpip` channels on a
/// forward connection. A std `Mutex` on purpose: every access is a tiny
/// lookup/edit with no await while held, and `ForwardSession`'s (sync)
/// `Drop` must be able to remove its route.
pub(crate) type RemoteRouteMap = Arc<std::sync::Mutex<RemoteRoutes>>;

/// Lock a `RemoteRouteMap`, riding through poison (the map holds plain
/// data; a panicked holder can't leave it inconsistent).
pub(crate) fn lock_routes(routes: &RemoteRouteMap) -> std::sync::MutexGuard<'_, RemoteRoutes> {
    match routes.lock() {
        Ok(g) => g,
        Err(poison) => poison.into_inner(),
    }
}

/// Pick the route for an inbound `forwarded-tcpip` channel. Exact
/// (address, port) match first; when the server echoes a normalized
/// bind address that doesn't literally match the request (some sshds
/// answer `localhost` binds with an IP, or `""` with `0.0.0.0`), fall
/// back to the port alone as long as it identifies exactly one route,
/// so the fallback can never cross-wire two rules.
pub(crate) fn route_lookup<V: Clone>(
    routes: &std::collections::HashMap<(String, u16), V>,
    addr: &str,
    port: u16,
) -> Option<V> {
    if let Some(v) = routes.get(&(addr.to_string(), port)) {
        return Some(v.clone());
    }
    let mut by_port = routes.iter().filter(|((_, p), _)| *p == port);
    match (by_port.next(), by_port.next()) {
        (Some((_, v)), None) => Some(v.clone()),
        _ => None,
    }
}

/// Bind local TCP listeners for port forwards, validating all ports upfront.
/// Returns the bound listeners (actual forwarding starts after PTY session opens).
pub(crate) async fn bind_port_forward_listeners(
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
pub(crate) fn spawn_port_forward_tasks(
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
                // Interactive clients (RDP, VNC, DB tools) connect here;
                // Nagle only delays their small writes into the tunnel.
                let _ = stream.set_nodelay(true);

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
    pub(crate) handle: SharedHandle,
    // `Arc` so an internal watcher (the RDP/VNC auto-close task) can hold a
    // second handle able to fire cancellation, alongside the owner's
    // `cancel()`. A `watch::Sender` is single-producer and not `Clone`, so
    // the shared reference is the Arc.
    pub(crate) cancel_tx: Arc<tokio::sync::watch::Sender<bool>>,
    pub(crate) _tasks: Vec<tokio::task::JoinHandle<()>>,
    /// For `-R` only: the server-side bind that must be released with
    /// `cancel_tcpip_forward` on stop. `None` for `-L` / `-D`.
    pub(crate) remote_bind: Option<(String, u16)>,
    /// For `-R` on a shared connection: this rule's entry in the
    /// connection's routing table, removed on cancel/drop so a later
    /// rule can reclaim the bind. `None` for `-L` / `-D`.
    pub(crate) remote_route: Option<(RemoteRouteMap, (String, u16))>,
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
        self.remove_remote_route();
        if let Some((host, port)) = &self.remote_bind {
            let handle = self.handle.lock().await;
            let _ = handle.cancel_tcpip_forward(host.clone(), *port as u32).await;
        }
    }

    /// Unregister this rule's `-R` route from the shared connection's
    /// routing table (no-op for `-L` / `-D`). Runs before the server-side
    /// release so a channel racing the teardown is rejected, not routed
    /// into a drain that is going away.
    fn remove_remote_route(&self) {
        if let Some((routes, key)) = &self.remote_route {
            lock_routes(routes).remove(key);
        }
    }

    /// Whether this forward has been cancelled, by an explicit `cancel()`
    /// or by its own auto-close watcher.
    ///
    /// Distinct from [`Self::is_alive`], which asks about the CONNECTION:
    /// a forward that rides a session's own connection (a terminal
    /// callback tunnel) self-closes while that connection stays up, so
    /// "alive" is still true for a tunnel whose listener is gone.
    pub fn is_cancelled(&self) -> bool {
        *self.cancel_tx.borrow()
    }

    /// A receiver that flips to `true` when the forward is cancelled, whether
    /// by an explicit `cancel()` / drop or by an internal auto-close watcher.
    /// The RDP/VNC launcher awaits this to learn the tunnel closed on its own
    /// (client window shut) and drop its bookkeeping entry.
    pub fn subscribe_cancel(&self) -> tokio::sync::watch::Receiver<bool> {
        self.cancel_tx.subscribe()
    }
}

impl Drop for ForwardSession {
    fn drop(&mut self) {
        // Best-effort: fire cancellation so the accept loop and bridges stop
        // even if `cancel()` was never awaited. The `-R` server-side release
        // needs an await, so callers that care should `cancel().await` first.
        let _ = self.cancel_tx.send(true);
        self.remove_remote_route();
    }
}

// ---------------------------------------------------------------------------
// Shared forward connection (issue #126)
// ---------------------------------------------------------------------------

/// A PTY-less SSH connection dedicated to the port forwards of one host.
/// Any mix of rules attaches onto it: each `-L` / `-D` listener opens its
/// `direct-tcpip` channels on this handle, and each `-R` rule registers a
/// route in `remote_routes` so inbound `forwarded-tcpip` channels reach
/// the right drain. This is what lets N rules to the same host cost one
/// SSH connection instead of N (issue #126), the same multiplexing OpenSSH
/// does for `ssh -L .. -L .. -R ..`.
///
/// Cheap to clone (both fields are `Arc`s). The SSH connection closes when
/// the last clone and the last attached `ForwardSession` are dropped.
#[derive(Clone)]
pub struct ForwardConn {
    pub(crate) handle: SharedHandle,
    pub(crate) remote_routes: RemoteRouteMap,
}

impl ForwardConn {
    /// Whether the underlying SSH connection is still up. Same `try_lock`
    /// reasoning as `ForwardSession::is_alive`: a busy lock means the
    /// connection is being used, i.e. alive.
    pub fn is_alive(&self) -> bool {
        match self.handle.try_lock() {
            Ok(h) => !h.is_closed(),
            Err(_) => true,
        }
    }

    /// Attach one rule to this connection: bind its local listener
    /// (`-L` / `-D`) or request the server-side bind (`-R`), and spawn the
    /// bridging tasks. The returned `ForwardSession` rides this shared
    /// connection; cancelling it tears down only this rule's listener and
    /// tasks, never the connection or its sibling forwards.
    pub async fn attach(&self, rule: &PortForwardRule) -> Result<ForwardSession, SshError> {
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        let cancel_tx = Arc::new(cancel_tx);

        match rule.kind {
            ForwardKind::Local => {
                let listener =
                    bind_forward_listener(&rule.listen_host, rule.listen_port).await?;
                let task = spawn_local_forward_task(
                    listener,
                    Arc::clone(&self.handle),
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
                    handle: Arc::clone(&self.handle),
                    cancel_tx,
                    _tasks: vec![task],
                    remote_bind: None,
                    remote_route: None,
                })
            }
            ForwardKind::Remote => {
                let key = (rule.listen_host.clone(), rule.listen_port);
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
                {
                    // Register the route BEFORE asking the server, so a
                    // channel arriving the instant the bind lands already
                    // has somewhere to go.
                    let mut routes = lock_routes(&self.remote_routes);
                    if routes.contains_key(&key) {
                        return Err(SshError::Channel(format!(
                            "remote forward {}:{} is already active on this connection",
                            key.0, key.1
                        )));
                    }
                    routes.insert(key.clone(), tx);
                }
                // Ask the server to listen on `listen_host:listen_port` and
                // tunnel inbound connections back to us. A denied request
                // (e.g. `AllowTcpForwarding no`) fails the toggle.
                let requested = {
                    let h = self.handle.lock().await;
                    h.tcpip_forward(rule.listen_host.clone(), rule.listen_port as u32)
                        .await
                };
                if let Err(e) = requested {
                    lock_routes(&self.remote_routes).remove(&key);
                    return Err(SshError::Channel(format!(
                        "remote forward request denied: {e}"
                    )));
                }
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
                    handle: Arc::clone(&self.handle),
                    cancel_tx,
                    _tasks: vec![task],
                    remote_bind: Some(key.clone()),
                    remote_route: Some((Arc::clone(&self.remote_routes), key)),
                })
            }
            ForwardKind::Dynamic => {
                let listener =
                    bind_forward_listener(&rule.listen_host, rule.listen_port).await?;
                let task = spawn_dynamic_forward_task(
                    listener,
                    Arc::clone(&self.handle),
                    rule.listen_port,
                    cancel_rx,
                );
                tracing::info!(
                    "forward(-D) SOCKS5 {}:{} up",
                    rule.listen_host, rule.listen_port
                );
                Ok(ForwardSession {
                    handle: Arc::clone(&self.handle),
                    cancel_tx,
                    _tasks: vec![task],
                    remote_bind: None,
                    remote_route: None,
                })
            }
        }
    }
}

impl std::fmt::Debug for ForwardConn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ForwardConn")
            .field("alive", &self.is_alive())
            .finish()
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
pub(crate) async fn bind_forward_listener(
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
pub(crate) fn spawn_local_forward_task(
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
            // Interactive clients (RDP, VNC, DB tools) connect here;
            // Nagle only delays their small writes into the tunnel.
            let _ = stream.set_nodelay(true);

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

/// Grace after the last connection drops before an auto-close `-L` tunnel
/// tears itself down. Long enough to ride out an RDP/VNC renegotiation blip,
/// short enough that a closed desktop window doesn't leave the tunnel idle.
pub(crate) const RD_TUNNEL_IDLE_GRACE: std::time::Duration = std::time::Duration::from_secs(10);

/// When an ephemeral `-L` tunnel tears itself down. A saved rule has no
/// such policy: it lives until the user toggles it off. A tunnel opened
/// FOR something (a desktop client, an OAuth callback) has to end on its
/// own, because nothing else in the app knows when that something is
/// finished with it.
#[derive(Debug, Clone, Copy)]
pub struct AutoClose {
    /// Idle time after the last connection closes before teardown. A new
    /// connection arriving inside the grace aborts it.
    pub idle_grace: std::time::Duration,
    /// Cap on the wait for the FIRST connection. `None` waits
    /// indefinitely (the RDP/VNC launcher: the user may take their time
    /// getting to the client window, and the tunnel is visible in the
    /// UI meanwhile). `Some` suits a tunnel opened for one expected
    /// callback, which is either used within a couple of minutes or
    /// never.
    pub unused_timeout: Option<std::time::Duration>,
}

impl AutoClose {
    /// Close `idle_grace` after the tunnel goes idle, and never on the
    /// wait for a first connection.
    pub fn on_idle(idle_grace: std::time::Duration) -> Self {
        Self { idle_grace, unused_timeout: None }
    }
}

/// A `-L` accept loop like `spawn_local_forward_task`, but it counts live
/// connections and fires `cancel_tx` once the tunnel has served at least one
/// connection and then sat idle for `policy.idle_grace` (or, when
/// `policy.unused_timeout` is set, once it has waited that long without
/// serving one at all). The RDP/VNC launcher uses this so its ephemeral
/// tunnel self-destructs when the desktop client disconnects, with no
/// dependency on the client's process lifetime (works the same for blocking
/// viewers and handoff launchers); the terminal's callback tunnel uses it so
/// an abandoned login doesn't leave a local port bound.
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_autoclose_local_forward_task(
    listener: tokio::net::TcpListener,
    handle: SharedHandle,
    target_host: String,
    target_port: u16,
    listen_port: u16,
    cancel: tokio::sync::watch::Receiver<bool>,
    cancel_tx: Arc<tokio::sync::watch::Sender<bool>>,
    policy: AutoClose,
) -> tokio::task::JoinHandle<()> {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    let active = Arc::new(AtomicUsize::new(0));
    let ever_used = Arc::new(AtomicBool::new(false));
    // Bumped on every connection open/close so the idle watcher re-evaluates
    // promptly instead of polling.
    let (wake_tx, wake_rx) = tokio::sync::watch::channel(0u64);
    let wake_tx = Arc::new(wake_tx);

    // Idle watcher: once the tunnel has been used and drops to zero live
    // connections, cancel after `idle_grace` of continued silence. A new
    // connection arriving inside the grace aborts the teardown.
    {
        let active = Arc::clone(&active);
        let ever_used = Arc::clone(&ever_used);
        let cancel_tx = Arc::clone(&cancel_tx);
        let mut wake_rx = wake_rx;
        let mut cancel_watch = cancel.clone();
        tokio::spawn(async move {
            loop {
                if *cancel_watch.borrow() {
                    return; // already cancelled by the owner (Stop / drop)
                }
                let idle =
                    ever_used.load(Ordering::SeqCst) && active.load(Ordering::SeqCst) == 0;
                if idle {
                    tokio::select! {
                        _ = cancel_watch.changed() => return,
                        _ = wake_rx.changed() => continue,
                        _ = tokio::time::sleep(policy.idle_grace) => {
                            if active.load(Ordering::SeqCst) == 0 {
                                tracing::info!(
                                    "forward(-L ephemeral) {} idle {:?}, auto-closing",
                                    listen_port, policy.idle_grace
                                );
                                let _ = cancel_tx.send(true);
                                return;
                            }
                        }
                    }
                } else if let Some(unused) = policy.unused_timeout
                    && !ever_used.load(Ordering::SeqCst)
                {
                    // Nothing has ever connected. Wait out the cap, then
                    // give the port back: the browser tab this tunnel was
                    // opened for was closed, or the login was abandoned.
                    tokio::select! {
                        _ = cancel_watch.changed() => return,
                        _ = wake_rx.changed() => continue,
                        _ = tokio::time::sleep(unused) => {
                            if !ever_used.load(Ordering::SeqCst) {
                                tracing::info!(
                                    "forward(-L ephemeral) {} unused after {:?}, auto-closing",
                                    listen_port, unused
                                );
                                let _ = cancel_tx.send(true);
                                return;
                            }
                        }
                    }
                } else {
                    tokio::select! {
                        _ = cancel_watch.changed() => return,
                        _ = wake_rx.changed() => {}
                    }
                }
            }
        });
    }

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
            // Interactive clients (RDP, VNC, DB tools) connect here;
            // Nagle only delays their small writes into the tunnel.
            let _ = stream.set_nodelay(true);

            active.fetch_add(1, Ordering::SeqCst);
            ever_used.store(true, Ordering::SeqCst);
            let _ = wake_tx.send(0);

            let shared = Arc::clone(&handle);
            let target_host = target_host.clone();
            let child_cancel = cancel.clone();
            let active = Arc::clone(&active);
            let wake_tx = Arc::clone(&wake_tx);
            tokio::spawn(async move {
                bridge_direct_tcpip(
                    shared, stream, target_host, target_port, listen_port, child_cancel,
                )
                .await;
                active.fetch_sub(1, Ordering::SeqCst);
                let _ = wake_tx.send(0);
            });
        }
        tracing::debug!("forward accept loop on {} stopped", listen_port);
    })
}

/// Open a `direct-tcpip` channel to `target_host:target_port` and pump bytes
/// between it and `stream`, stopping on EOF, error, or cancellation.
pub(crate) async fn bridge_direct_tcpip(
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
pub(crate) async fn bridge_channel_to_target(
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
    // Interactive protocols (RDP, VNC) ride these bridges; Nagle only
    // adds latency on top of the SSH channel's own framing.
    let _ = stream.set_nodelay(true);

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
pub(crate) fn spawn_dynamic_forward_task(
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
            // Same rationale as the -L accepts: the dynamic forward
            // carries interactive client traffic.
            let _ = stream.set_nodelay(true);
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
pub(crate) async fn socks5_reply(
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
pub(crate) async fn socks5_negotiate(
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
pub(crate) async fn bridge_socks5(
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

#[cfg(test)]
mod route_tests {
    use super::route_lookup;
    use std::collections::HashMap;

    fn map(entries: &[(&str, u16, u32)]) -> HashMap<(String, u16), u32> {
        entries
            .iter()
            .map(|(h, p, v)| ((h.to_string(), *p), *v))
            .collect()
    }

    #[test]
    fn exact_address_and_port_wins() {
        // Two rules on the same port, different bind addresses: the
        // exact match must pick the right one, never the port fallback.
        let m = map(&[("127.0.0.1", 8080, 1), ("192.168.0.5", 8080, 2)]);
        assert_eq!(route_lookup(&m, "127.0.0.1", 8080), Some(1));
        assert_eq!(route_lookup(&m, "192.168.0.5", 8080), Some(2));
    }

    #[test]
    fn normalized_address_falls_back_to_unique_port() {
        // Some sshds echo a normalized bind address (`localhost` answered
        // as an IP, `""` as `0.0.0.0`); a port held by exactly one rule
        // still routes.
        let m = map(&[("localhost", 9000, 1), ("127.0.0.1", 9100, 2)]);
        assert_eq!(route_lookup(&m, "::1", 9000), Some(1));
        assert_eq!(route_lookup(&m, "0.0.0.0", 9100), Some(2));
    }

    #[test]
    fn ambiguous_port_never_cross_wires() {
        // The fallback must refuse when two rules share the port: routing
        // a channel to the wrong rule's target would be worse than
        // rejecting the open.
        let m = map(&[("127.0.0.1", 8080, 1), ("192.168.0.5", 8080, 2)]);
        assert_eq!(route_lookup(&m, "10.0.0.1", 8080), None);
    }

    #[test]
    fn unknown_port_is_rejected() {
        let m = map(&[("127.0.0.1", 8080, 1)]);
        assert_eq!(route_lookup(&m, "127.0.0.1", 9999), None);
        assert_eq!(route_lookup::<u32>(&HashMap::new(), "127.0.0.1", 8080), None);
    }
}
