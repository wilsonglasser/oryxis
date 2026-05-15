use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use uuid::Uuid;

use oryxis_vault::VaultStore;

use crate::config::SyncConfig;
use crate::crypto::{self, DeviceIdentity};
use crate::discovery;
use crate::error::SyncError;
use crate::protocol::{self, SyncMessage, ManifestEntry, EntityType, PROTOCOL_VERSION};
use crate::transport;

/// Events emitted by the sync engine for the UI.
#[derive(Debug, Clone)]
pub enum SyncEvent {
    PeerDiscovered {
        device_id: Uuid,
        device_name: String,
        /// The peer's listen address. For mDNS-discovered peers this
        /// is the actual `ip:port` advertised on the LAN, so the UI
        /// can pre-fill the pairing target with one click.
        addr: SocketAddr,
        via: discovery::DiscoveryMethod,
    },
    PairingCodeGenerated {
        code: String,
    },
    PairingCompleted {
        device_id: Uuid,
        device_name: String,
    },
    PairingFailed {
        reason: String,
    },
    SyncStarted {
        peer_id: Uuid,
    },
    SyncCompleted {
        peer_id: Uuid,
        pushed: usize,
        pulled: usize,
    },
    SyncFailed {
        peer_id: Uuid,
        error: String,
    },
    PeerOnline {
        device_id: Uuid,
    },
    PeerOffline {
        device_id: Uuid,
    },
}

/// State for a device that is currently hosting a pairing code and
/// waiting for a peer to join. Lives behind a `Mutex` shared between
/// the engine, its `SyncHandle`s, and the QUIC accept loop.
struct HostingPairing {
    code: String,
    expires_at: Instant,
}

/// URL scheme + path prefix for shareable pairing links.
const PAIRING_LINK_PREFIX: &str = "oryxis://pair/";

/// Tombstones older than this drop on engine boot. Should outlive any
/// realistic offline gap between a paired peer's syncs; a peer that
/// reconnects after the window will silently miss the deletion (and
/// the table won't grow forever in exchange).
const TOMBSTONE_TTL_DAYS: u32 = 30;

/// Build a shareable pairing link from a device id + code. Inverse of
/// [`parse_pairing_link`].
pub fn format_pairing_link(device_id: &Uuid, code: &str) -> String {
    format!("{}{}/{}", PAIRING_LINK_PREFIX, device_id, code)
}

/// Parse an `oryxis://pair/<device_id>/<code>` link. Returns `None` if
/// the prefix is wrong, the UUID is invalid, or the code is not a
/// 6-digit number. Whitespace around the link is trimmed; trailing
/// slashes / query strings are rejected to keep the format strict.
pub fn parse_pairing_link(link: &str) -> Option<(Uuid, String)> {
    let trimmed = link.trim();
    let rest = trimmed.strip_prefix(PAIRING_LINK_PREFIX)?;
    let (id_str, code) = rest.split_once('/')?;
    let device_id = Uuid::parse_str(id_str).ok()?;
    if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((device_id, code.to_string()))
}

/// P2P sync engine.
pub struct SyncEngine {
    config: SyncConfig,
    identity: DeviceIdentity,
    /// `pub(crate)` so the in-crate integration tests can inspect the
    /// vault after a pairing / sync round-trip.
    pub(crate) vault: Arc<std::sync::Mutex<VaultStore>>,
    event_tx: mpsc::UnboundedSender<SyncEvent>,
    event_rx: Option<mpsc::UnboundedReceiver<SyncEvent>>,
    shutdown_tx: Option<tokio::sync::broadcast::Sender<()>>,
    /// Set while this device is hosting a pairing code (single-shot:
    /// cleared once a peer successfully pairs, or on expiry / cancel).
    hosting_pairing: Arc<std::sync::Mutex<Option<HostingPairing>>>,
    /// The QUIC port the server actually bound, known only after
    /// `start()`. Advertised to joiners in `PairingRequest` so the
    /// host can sync back to us.
    bound_port: u16,
}

impl SyncEngine {
    /// Create a new sync engine. Call `start()` to begin background tasks.
    pub fn new(
        config: SyncConfig,
        identity: DeviceIdentity,
        vault: Arc<std::sync::Mutex<VaultStore>>,
    ) -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        Self {
            config,
            identity,
            vault,
            event_tx,
            event_rx: Some(event_rx),
            shutdown_tx: None,
            hosting_pairing: Arc::new(std::sync::Mutex::new(None)),
            bound_port: 0,
        }
    }

    /// Take the event receiver (can only be called once).
    pub fn take_events(&mut self) -> Option<mpsc::UnboundedReceiver<SyncEvent>> {
        self.event_rx.take()
    }

    /// Start background sync tasks (QUIC listener, mDNS, signaling
    /// heartbeat, auto-sync timer). Not `async` (the body only spawns
    /// detached tasks, it never awaits) but it must be called from
    /// within a Tokio runtime context because of those `tokio::spawn`s.
    pub fn start(&mut self) -> Result<(), SyncError> {
        if !self.config.enabled {
            return Ok(());
        }

        // rustls 0.23 requires an explicit CryptoProvider when both
        // `ring` and `aws-lc-rs` could be linked transitively. We pin
        // `ring` here once, before any TLS handshake. `install_default`
        // errors if already installed, fine, treat as idempotent.
        let _ = rustls::crypto::ring::default_provider().install_default();

        let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);
        self.shutdown_tx = Some(shutdown_tx.clone());

        // Start QUIC server
        let endpoint = transport::create_server_endpoint(
            &self.identity.device_id,
            self.config.listen_port,
        )?;

        let listen_port = endpoint
            .local_addr()
            .map(|a| a.port())
            .unwrap_or(0);
        // Remember the actually-bound port so `SyncHandle` can advertise
        // it to pairing joiners (the configured port may have been 0).
        self.bound_port = listen_port;

        // Start mDNS registration and browsing
        let (discovery_tx, mut discovery_rx) = mpsc::unbounded_channel();
        let _mdns_register = discovery::mdns::register(&self.identity.device_id, listen_port).ok();
        let _mdns_browse = discovery::mdns::browse(&self.identity.device_id, discovery_tx).ok();

        // Forward discovery events
        let event_tx = self.event_tx.clone();
        let mut shutdown_rx = shutdown_tx.subscribe();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(peer) = discovery_rx.recv() => {
                        let _ = event_tx.send(SyncEvent::PeerDiscovered {
                            device_id: peer.device_id,
                            device_name: String::new(),
                            addr: peer.addr,
                            via: peer.method,
                        });
                    }
                    _ = shutdown_rx.recv() => break,
                }
            }
        });

        // QUIC server accept loop
        let vault = self.vault.clone();
        let identity = self.identity.clone();
        let event_tx = self.event_tx.clone();
        let hosting_pairing = self.hosting_pairing.clone();
        let mut shutdown_rx = shutdown_tx.subscribe();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    incoming = endpoint.accept() => {
                        if let Some(incoming) = incoming {
                            let vault = vault.clone();
                            let identity = identity.clone();
                            let event_tx = event_tx.clone();
                            let hosting_pairing = hosting_pairing.clone();
                            tokio::spawn(async move {
                                if let Err(e) = handle_incoming(incoming, vault, identity, event_tx, hosting_pairing).await {
                                    tracing::warn!("Incoming connection failed: {}", e);
                                }
                            });
                        } else {
                            break;
                        }
                    }
                    _ = shutdown_rx.recv() => break,
                }
            }
        });

        // Signaling heartbeat. When a signaling URL is configured, ask
        // a STUN server for our public address once a minute and POST
        // it to the signaling server so peers behind a different NAT
        // can `lookup(device_id)` and reach us. No-op LAN-only when
        // `signaling_url` is `None`.
        if let Some(signaling_url) = self.config.signaling_url.clone() {
            let token = self.config.signaling_token.clone().unwrap_or_default();
            let identity = self.identity.clone();
            let bound_port = listen_port;
            let mut shutdown_rx = shutdown_tx.subscribe();
            tokio::spawn(async move {
                let client = discovery::signaling::SignalingClient::new(&signaling_url, &token);
                let mut ticker =
                    tokio::time::interval(std::time::Duration::from_secs(60));
                let mut last_public_ip: Option<String> = None;
                let fp = identity.public_key_fingerprint();
                loop {
                    tokio::select! {
                        _ = ticker.tick() => {
                            // Fresh ephemeral UDP socket per probe; the STUN
                            // server's reply tells us the NAT mapping it sees.
                            let socket = match tokio::net::UdpSocket::bind("0.0.0.0:0").await {
                                Ok(s) => s,
                                Err(e) => {
                                    tracing::warn!("signaling: STUN bind failed: {e}");
                                    continue;
                                }
                            };
                            let pub_addr = match discovery::stun::get_public_addr(&socket).await {
                                Ok(a) => a,
                                Err(e) => {
                                    tracing::warn!("signaling: STUN failed: {e}");
                                    continue;
                                }
                            };
                            let ip = pub_addr.ip().to_string();
                            // Re-register only when the public IP changes
                            // (or on the first tick). Cuts traffic on stable
                            // networks; keeps the lookup fresh after Wi-Fi
                            // hops, cell handoffs, etc.
                            if last_public_ip.as_deref() == Some(ip.as_str()) {
                                continue;
                            }
                            match client.register(&identity.device_id, &fp, &ip, bound_port).await {
                                Ok(()) => {
                                    tracing::info!(
                                        "signaling: registered {} -> {}:{}",
                                        identity.device_id, ip, bound_port
                                    );
                                    last_public_ip = Some(ip);
                                }
                                Err(e) => {
                                    tracing::warn!("signaling register failed: {e}");
                                }
                            }
                        }
                        _ = shutdown_rx.recv() => break,
                    }
                }
            });
        }

        // Auto-sync timer (if mode == Auto)
        if self.config.mode == crate::config::SyncMode::Auto {
            let interval = self.config.auto_interval_secs;
            let vault = self.vault.clone();
            let identity = self.identity.clone();
            let config = self.config.clone();
            let event_tx = self.event_tx.clone();
            let mut shutdown_rx = shutdown_tx.subscribe();
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(std::time::Duration::from_secs(interval));
                ticker.tick().await; // skip first immediate tick
                loop {
                    tokio::select! {
                        _ = ticker.tick() => {
                            let _ = sync_all_peers(&vault, &identity, &config, &event_tx).await;
                        }
                        _ = shutdown_rx.recv() => break,
                    }
                }
            });
        }

        // Garbage-collect tombstones older than 30 days at boot. The
        // tombstones in `sync_metadata` only need to outlive the gap
        // between a peer's syncs; anything older is dead weight. Cheap
        // single-statement DELETE, runs once per session.
        if let Ok(v) = self.vault.lock() {
            match v.vacuum_tombstones(TOMBSTONE_TTL_DAYS) {
                Ok(0) => {}
                Ok(n) => tracing::info!("sync: vacuumed {n} stale tombstones"),
                Err(e) => tracing::warn!("sync: tombstone vacuum failed: {e}"),
            }
        }

        tracing::info!("Sync engine started (port {})", listen_port);
        Ok(())
    }

    /// Stop all background tasks.
    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }

    /// Trigger a manual sync with all paired peers.
    pub async fn sync_now(&self) -> Result<(), SyncError> {
        sync_all_peers(&self.vault, &self.identity, &self.config, &self.event_tx).await
    }

    /// Get a cheap, cloneable handle for triggering syncs and pairing
    /// without owning the engine. The engine stays put in app state
    /// while the handle can be moved into a background `Task`. Call
    /// after `start()` so `bound_port` is the real listener.
    pub fn handle(&self) -> SyncHandle {
        SyncHandle {
            config: self.config.clone(),
            identity: self.identity.clone(),
            vault: self.vault.clone(),
            event_tx: self.event_tx.clone(),
            hosting_pairing: self.hosting_pairing.clone(),
            listen_port: self.bound_port,
        }
    }

    /// Get the device identity.
    pub fn identity(&self) -> &DeviceIdentity {
        &self.identity
    }

    /// The QUIC port the server bound. Zero until `start()` has run.
    pub fn listen_port(&self) -> u16 {
        self.bound_port
    }

    /// Get the config.
    pub fn config(&self) -> &SyncConfig {
        &self.config
    }
}

/// A cheap, cloneable handle for triggering syncs without owning the
/// `SyncEngine`. Every field is `Clone` (the vault is an `Arc`,
/// identity / config are owned `Clone` types, the event sender is an
/// mpsc clone), so a handle can be moved into a background `Task` while
/// the engine itself stays put in app state.
#[derive(Clone)]
pub struct SyncHandle {
    config: SyncConfig,
    identity: DeviceIdentity,
    vault: Arc<std::sync::Mutex<VaultStore>>,
    event_tx: mpsc::UnboundedSender<SyncEvent>,
    hosting_pairing: Arc<std::sync::Mutex<Option<HostingPairing>>>,
    listen_port: u16,
}

/// How long a hosted pairing code stays valid.
const PAIRING_CODE_TTL: Duration = Duration::from_secs(300);

impl SyncHandle {
    /// Sync with every active paired peer. Safe to call from a
    /// background task: it locks the vault only in short bursts, never
    /// across an `.await`.
    pub async fn sync_now(&self) -> Result<(), SyncError> {
        sync_all_peers(&self.vault, &self.identity, &self.config, &self.event_tx).await
    }

    /// Begin hosting a pairing code. Returns the 6-digit code to show
    /// the user; the matching `SyncEvent::PairingCodeGenerated` is also
    /// emitted so any subscriber sees it. The code is single-shot and
    /// expires after `PAIRING_CODE_TTL`.
    pub fn start_hosting_pairing(&self) -> String {
        let code = crypto::generate_pairing_code();
        if let Ok(mut state) = self.hosting_pairing.lock() {
            *state = Some(HostingPairing {
                code: code.clone(),
                expires_at: Instant::now() + PAIRING_CODE_TTL,
            });
        }
        let _ = self
            .event_tx
            .send(SyncEvent::PairingCodeGenerated { code: code.clone() });
        code
    }

    /// Stop hosting a pairing code (user cancelled, or the modal closed).
    pub fn cancel_hosting_pairing(&self) {
        if let Ok(mut state) = self.hosting_pairing.lock() {
            *state = None;
        }
    }

    /// Build the shareable pairing link for the current device + code.
    /// Format: `oryxis://pair/<device-uuid>/<6-digit-code>`. The joiner
    /// pastes this, the link is parsed back into `(device_id, code)`,
    /// and `join_pairing_remote` looks the device up on the signaling
    /// server before running the handshake.
    pub fn pairing_link(&self, code: &str) -> String {
        format_pairing_link(&self.identity.device_id, code)
    }

    /// Join a peer using a pairing link (`oryxis://pair/<device_id>/<code>`).
    /// Requires the signaling URL to be configured, since the device
    /// id has to be resolved to a public `ip:port` before the QUIC
    /// handshake can run. Emits `PairingCompleted` / `PairingFailed`.
    pub async fn join_pairing_remote(&self, link: &str) -> Result<(), SyncError> {
        let (device_id, code) = match parse_pairing_link(link) {
            Some(pair) => pair,
            None => {
                let reason = "Pairing link is not a valid oryxis://pair/... URL".to_string();
                let _ = self
                    .event_tx
                    .send(SyncEvent::PairingFailed { reason: reason.clone() });
                return Err(SyncError::PairingFailed(reason));
            }
        };
        let Some(signaling_url) = self.config.signaling_url.clone() else {
            let reason =
                "Signaling URL is not configured; set one in Settings > Sync > Advanced"
                    .to_string();
            let _ = self
                .event_tx
                .send(SyncEvent::PairingFailed { reason: reason.clone() });
            return Err(SyncError::PairingFailed(reason));
        };
        let token = self.config.signaling_token.clone().unwrap_or_default();
        let client = discovery::signaling::SignalingClient::new(&signaling_url, &token);
        let lookup = match client.lookup(&device_id).await {
            Ok(l) => l,
            Err(e) => {
                let reason = format!("Signaling lookup failed: {e}");
                let _ = self
                    .event_tx
                    .send(SyncEvent::PairingFailed { reason: reason.clone() });
                return Err(SyncError::PairingFailed(reason));
            }
        };
        let addr: SocketAddr = match format!("{}:{}", lookup.ip, lookup.port).parse() {
            Ok(a) => a,
            Err(e) => {
                let reason = format!("Signaling returned invalid address: {e}");
                let _ = self
                    .event_tx
                    .send(SyncEvent::PairingFailed { reason: reason.clone() });
                return Err(SyncError::PairingFailed(reason));
            }
        };
        self.join_pairing(addr, code).await
    }

    /// Join a peer that is hosting a pairing code: connect to `addr`,
    /// run the challenge/response handshake, and on success persist the
    /// host as a peer. Emits `PairingCompleted` / `PairingFailed`.
    pub async fn join_pairing(&self, addr: SocketAddr, code: String) -> Result<(), SyncError> {
        match self.join_pairing_inner(addr, &code).await {
            Ok((device_id, device_name)) => {
                let _ = self.event_tx.send(SyncEvent::PairingCompleted {
                    device_id,
                    device_name,
                });
                Ok(())
            }
            Err(e) => {
                let _ = self.event_tx.send(SyncEvent::PairingFailed {
                    reason: e.to_string(),
                });
                Err(e)
            }
        }
    }

    async fn join_pairing_inner(
        &self,
        addr: SocketAddr,
        code: &str,
    ) -> Result<(Uuid, String), SyncError> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = transport::create_client_endpoint()?;
        let connection = client
            .connect(addr, "oryxis-sync")
            .map_err(|e| SyncError::Transport(format!("Connect: {}", e)))?
            .await
            .map_err(|e| SyncError::Transport(format!("Handshake: {}", e)))?;
        let (mut send, mut recv) = connection
            .open_bi()
            .await
            .map_err(|e| SyncError::Transport(format!("Open stream: {}", e)))?;

        // Fresh X25519 keypair for the pairing-time DH. We hold the
        // secret across the recv-PairingAccepted await, then consume
        // it in `x25519_dh` so the ephemeral private key is forgotten.
        let (joiner_x25519_secret, joiner_x25519_pub) = crypto::x25519_keypair();

        transport::send_message(
            &mut send,
            &SyncMessage::PairingRequest {
                device_id: self.identity.device_id,
                device_name: self.identity.device_name.clone(),
                public_key: self.identity.public_key_bytes(),
                pairing_code: code.to_string(),
                listen_port: self.listen_port,
                x25519_pub: joiner_x25519_pub.to_vec(),
            },
        )
        .await?;

        // Host -> PairingChallenge (or PairingRejected).
        let challenge = match transport::recv_message(&mut recv).await? {
            SyncMessage::PairingChallenge { challenge } => challenge,
            SyncMessage::PairingRejected { reason } => {
                return Err(SyncError::PairingFailed(reason));
            }
            _ => return Err(SyncError::Protocol("Expected PairingChallenge".into())),
        };
        let challenge: [u8; 32] = challenge
            .as_slice()
            .try_into()
            .map_err(|_| SyncError::Protocol("Challenge must be 32 bytes".into()))?;

        // Prove possession of the private key paired with the public
        // key we just sent.
        let signed = crypto::sign_ed25519_32(&self.identity.signing_key, &challenge);
        transport::send_message(
            &mut send,
            &SyncMessage::PairingResponse {
                signed_challenge: signed.to_vec(),
            },
        )
        .await?;

        // Host -> PairingAccepted (or PairingRejected).
        let (device_id, device_name, public_key, host_x25519_pub) =
            match transport::recv_message(&mut recv).await? {
                SyncMessage::PairingAccepted {
                    device_id,
                    device_name,
                    public_key,
                    x25519_pub,
                } => (device_id, device_name, public_key, x25519_pub),
                SyncMessage::PairingRejected { reason } => {
                    return Err(SyncError::PairingFailed(reason));
                }
                _ => return Err(SyncError::Protocol("Expected PairingAccepted".into())),
            };
        let host_x25519_pub: [u8; 32] = host_x25519_pub
            .as_slice()
            .try_into()
            .map_err(|_| SyncError::Protocol("Host x25519_pub must be 32 bytes".into()))?;

        // Both sides DH to the same 32-byte secret; store it on the
        // peer row so every later `SyncRecord.payload` between us and
        // this host is sealed with ChaCha20-Poly1305.
        let shared_secret = crypto::x25519_dh(joiner_x25519_secret, &host_x25519_pub);

        // Persist the host as a peer. `addr` is the host's listen
        // address (it is what we dialed), so future syncs can reach it.
        let now = chrono::Utc::now();
        {
            let v = self
                .vault
                .lock()
                .map_err(|_| SyncError::Vault("Lock".into()))?;
            v.save_sync_peer(
                &device_id,
                &device_name,
                &public_key,
                Some(&shared_secret),
                &now,
            )?;
            v.update_sync_peer_endpoint(&device_id, &addr.ip().to_string(), addr.port())?;
        }

        // Send `Bye` as a delivery barrier: it tells the host we have
        // read `PairingAccepted`, so the host can drop the connection
        // without losing the still-buffered final frame. Mirrors the
        // `Bye` the sync session already uses for the same reason.
        let _ = transport::send_message(&mut send, &SyncMessage::Bye).await;
        Ok((device_id, device_name))
    }
}

/// Handle an incoming QUIC connection.
async fn handle_incoming(
    incoming: quinn::Incoming,
    vault: Arc<std::sync::Mutex<VaultStore>>,
    identity: DeviceIdentity,
    event_tx: mpsc::UnboundedSender<SyncEvent>,
    hosting_pairing: Arc<std::sync::Mutex<Option<HostingPairing>>>,
) -> Result<(), SyncError> {
    let connection = incoming
        .await
        .map_err(|e| SyncError::Transport(format!("Accept: {}", e)))?;

    // Channel-binding exporter (RFC 5705) from the QUIC TLS session.
    // Signed by the peer's long-term Ed25519 identity inside Hello, so a
    // MITM cannot relay a signature: its TLS sessions on either side
    // derive different exporters.
    let exporter = derive_session_exporter(&connection)?;

    let (mut send, mut recv) = connection
        .accept_bi()
        .await
        .map_err(|e| SyncError::Transport(format!("Accept stream: {}", e)))?;

    // Receive Hello (or a PairingRequest)
    let msg = transport::recv_message(&mut recv).await?;
    let (peer_id, peer_auth_sig) = match msg {
        SyncMessage::Hello {
            device_id,
            protocol_version,
            auth_signature,
        } => {
            if protocol_version != PROTOCOL_VERSION {
                return Err(SyncError::Protocol("Version mismatch".into()));
            }
            (device_id, auth_signature)
        }
        SyncMessage::PairingRequest {
            device_id,
            device_name,
            public_key,
            pairing_code,
            listen_port,
            x25519_pub,
        } => {
            // Pairing connection, not a sync session. Run the
            // challenge/response handshake and return, never touching
            // the Hello auth path below.
            let peer_addr = connection.remote_address();
            return handle_pairing_request(
                &mut send,
                &mut recv,
                &vault,
                &identity,
                &hosting_pairing,
                &event_tx,
                device_id,
                device_name,
                public_key,
                pairing_code,
                peer_addr,
                listen_port,
                x25519_pub,
            )
            .await;
        }
        _ => return Err(SyncError::Protocol("Expected Hello".into())),
    };

    // Look up the peer's stored Ed25519 pubkey and verify the
    // channel-bound signature BEFORE doing anything else with the peer.
    // Unknown / inactive peers fall through to the "Bye" path below
    // without a verify attempt, so we never leak verify timing.
    let peer_pubkey = {
        let vault_guard = vault.lock().map_err(|_| SyncError::Vault("Lock failed".into()))?;
        let peers = vault_guard.list_sync_peers()?;
        peers
            .into_iter()
            .find(|p| p.peer_id == peer_id && p.is_active)
            .map(|p| p.public_key)
    };

    let Some(peer_pubkey) = peer_pubkey else {
        tracing::warn!("Unknown peer {} tried to connect", peer_id);
        transport::send_message(&mut send, &SyncMessage::Bye).await?;
        return Ok(());
    };

    if let Err(e) = crypto::verify_session_handshake(&peer_pubkey, &exporter, &peer_auth_sig) {
        tracing::warn!("Peer {} failed handshake auth: {}", peer_id, e);
        transport::send_message(&mut send, &SyncMessage::Bye).await?;
        return Err(SyncError::PairingFailed(format!(
            "Peer {} signature did not verify",
            peer_id
        )));
    }

    // Send HelloAck with our own signature so the client can also
    // authenticate us against the pubkey it stored for our device_id.
    let our_signature = crypto::sign_session_handshake(&identity.signing_key, &exporter);
    transport::send_message(&mut send, &SyncMessage::HelloAck {
        device_id: identity.device_id,
        protocol_version: PROTOCOL_VERSION,
        auth_signature: our_signature.to_vec(),
    }).await?;

    // Handle sync messages
    let _ = event_tx.send(SyncEvent::SyncStarted { peer_id });

    match handle_sync_session(&mut send, &mut recv, &vault, &peer_id).await {
        Ok((pushed, pulled)) => {
            let _ = event_tx.send(SyncEvent::SyncCompleted { peer_id, pushed, pulled });
        }
        Err(e) => {
            let _ = event_tx.send(SyncEvent::SyncFailed {
                peer_id,
                error: e.to_string(),
            });
        }
    }

    Ok(())
}

/// Server side of the pairing handshake. The joiner has already sent
/// `PairingRequest`; we check it against the code we are hosting,
/// challenge the joiner to prove it holds the private key for the
/// public key it sent, and on success persist it as a peer and reply
/// with our own identity. Single-shot: a successful pair clears the
/// hosting code so the same code can't pair a second device.
#[allow(clippy::too_many_arguments)]
async fn handle_pairing_request(
    send: &mut quinn::SendStream,
    recv: &mut quinn::RecvStream,
    vault: &Arc<std::sync::Mutex<VaultStore>>,
    identity: &DeviceIdentity,
    hosting_pairing: &Arc<std::sync::Mutex<Option<HostingPairing>>>,
    event_tx: &mpsc::UnboundedSender<SyncEvent>,
    device_id: Uuid,
    device_name: String,
    public_key: Vec<u8>,
    pairing_code: String,
    peer_addr: SocketAddr,
    listen_port: u16,
    joiner_x25519_pub: Vec<u8>,
) -> Result<(), SyncError> {
    // Is there a live hosting code? Drop expired ones here so a stale
    // code never pairs.
    let expected = {
        let state = hosting_pairing
            .lock()
            .map_err(|_| SyncError::Vault("Lock".into()))?;
        state
            .as_ref()
            .filter(|s| s.expires_at > Instant::now())
            .map(|s| s.code.clone())
    };
    let Some(expected) = expected else {
        return reject_pairing(send, recv, "Not hosting pairing (or code expired)").await;
    };
    if !crypto::constant_time_eq(expected.as_bytes(), pairing_code.as_bytes()) {
        return reject_pairing(send, recv, "Wrong pairing code").await;
    }

    // Code matches. Challenge the joiner with a fresh nonce so an
    // intercepted `PairingRequest` can't be replayed.
    let challenge = crypto::random_challenge();
    transport::send_message(
        send,
        &SyncMessage::PairingChallenge {
            challenge: challenge.to_vec(),
        },
    )
    .await?;

    let signed = match transport::recv_message(recv).await? {
        SyncMessage::PairingResponse { signed_challenge } => signed_challenge,
        _ => return Err(SyncError::Protocol("Expected PairingResponse".into())),
    };
    if crypto::verify_ed25519_32(&public_key, &challenge, &signed).is_err() {
        return reject_pairing(send, recv, "Bad challenge response").await;
    }

    // Joiner's X25519 pubkey must be 32 bytes; reject otherwise. Then
    // generate our own ephemeral keypair and DH to the shared secret
    // we'll seal payloads with from now on.
    let joiner_x25519_pub: [u8; 32] = joiner_x25519_pub
        .as_slice()
        .try_into()
        .map_err(|_| SyncError::Protocol("Joiner x25519_pub must be 32 bytes".into()))?;
    let (host_x25519_secret, host_x25519_pub) = crypto::x25519_keypair();
    let shared_secret = crypto::x25519_dh(host_x25519_secret, &joiner_x25519_pub);

    // Verified. Persist the joiner as a peer (IP from the connection,
    // listen port from the request, shared secret from the DH above)
    // and clear the single-shot code.
    let now = chrono::Utc::now();
    {
        let v = vault.lock().map_err(|_| SyncError::Vault("Lock".into()))?;
        v.save_sync_peer(
            &device_id,
            &device_name,
            &public_key,
            Some(&shared_secret),
            &now,
        )?;
        v.update_sync_peer_endpoint(&device_id, &peer_addr.ip().to_string(), listen_port)?;
    }
    if let Ok(mut state) = hosting_pairing.lock() {
        *state = None;
    }

    transport::send_message(
        send,
        &SyncMessage::PairingAccepted {
            device_id: identity.device_id,
            device_name: identity.device_name.clone(),
            public_key: identity.public_key_bytes(),
            x25519_pub: host_x25519_pub.to_vec(),
        },
    )
    .await?;
    // Delivery barrier: wait for the joiner's `Bye` so we don't drop
    // the connection (and the still-buffered `PairingAccepted` frame)
    // before the joiner has read it.
    let _ = transport::recv_message(recv).await;

    let _ = event_tx.send(SyncEvent::PairingCompleted {
        device_id,
        device_name,
    });
    Ok(())
}

/// Send a `PairingRejected` and hold the connection open until the
/// joiner has read it: without the barrier `recv`, returning here
/// would drop the connection before the still-buffered rejection
/// frame is delivered, and the joiner would see a bare "connection
/// lost" instead of the reason.
async fn reject_pairing(
    send: &mut quinn::SendStream,
    recv: &mut quinn::RecvStream,
    reason: &str,
) -> Result<(), SyncError> {
    transport::send_message(
        send,
        &SyncMessage::PairingRejected {
            reason: reason.to_string(),
        },
    )
    .await?;
    let _ = transport::recv_message(recv).await;
    Ok(())
}

/// Handle the sync protocol after handshake.
async fn handle_sync_session(
    send: &mut quinn::SendStream,
    recv: &mut quinn::RecvStream,
    vault: &Arc<std::sync::Mutex<VaultStore>>,
    peer_id: &Uuid,
) -> Result<(usize, usize), SyncError> {
    let mut pushed = 0;
    let mut pulled = 0;

    // Per-peer E2E key. Fetched once at session start. Pre-v4 peers
    // (no `shared_secret` row) talk in plaintext; v4 peers always
    // carry one because pairing seeds it.
    let shared_secret = peer_shared_secret(vault, peer_id)?;
    let shared_secret = shared_secret.as_ref();

    loop {
        let msg = transport::recv_message(recv).await?;
        match msg {
            SyncMessage::ManifestRequest => {
                let manifest = build_manifest(vault)?;
                transport::send_message(send, &SyncMessage::Manifest { entries: manifest }).await?;
            }
            SyncMessage::DeltaRequest { needed } => {
                // Peer wants these records from us
                let records = collect_records(vault, &needed, shared_secret)?;
                pushed += records.len();
                transport::send_message(send, &SyncMessage::DeltaResponse { records }).await?;
            }
            SyncMessage::DeltaPush { records } => {
                // Peer is pushing records to us
                let count = records.len();
                apply_records(vault, &records, shared_secret)?;
                pulled += count;
                let accepted: Vec<Uuid> = records.iter().map(|r| r.entity_id).collect();
                transport::send_message(send, &SyncMessage::DeltaAck { accepted }).await?;
            }
            SyncMessage::Ping => {
                transport::send_message(send, &SyncMessage::Pong).await?;
            }
            SyncMessage::Bye => break,
            _ => {
                tracing::warn!("Unexpected message in sync session");
                break;
            }
        }
    }

    Ok((pushed, pulled))
}

/// Fetch the persisted X25519 shared secret for a paired peer and
/// coerce it to a fixed 32-byte array. Returns `None` if the peer
/// doesn't have one (legacy rows, or a future ABI we don't recognise).
fn peer_shared_secret(
    vault: &Arc<std::sync::Mutex<VaultStore>>,
    peer_id: &Uuid,
) -> Result<Option<[u8; 32]>, SyncError> {
    let v = vault.lock().map_err(|_| SyncError::Vault("Lock".into()))?;
    let bytes = v.get_sync_peer_shared_secret(peer_id)?;
    Ok(bytes.and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok()))
}

/// Build a manifest of all syncable entities in the vault, plus a
/// deletion entry (`is_deleted = true`) for every tombstone recorded
/// in `sync_metadata`. The tombstones are what let a delete propagate:
/// without them a peer that still holds the entity would push its
/// stale copy back and the delete would silently undo itself.
pub(crate) fn build_manifest(
    vault: &Arc<std::sync::Mutex<VaultStore>>,
) -> Result<Vec<ManifestEntry>, SyncError> {
    let v = vault.lock().map_err(|_| SyncError::Vault("Lock".into()))?;
    let mut entries = Vec::new();

    for c in v.list_connections()? {
        entries.push(ManifestEntry {
            entity_type: EntityType::Connection,
            entity_id: c.id,
            updated_at: c.updated_at,
            is_deleted: false,
        });
    }
    for k in v.list_keys()? {
        entries.push(ManifestEntry {
            entity_type: EntityType::SshKey,
            entity_id: k.id,
            updated_at: k.updated_at,
            is_deleted: false,
        });
    }
    for i in v.list_identities()? {
        entries.push(ManifestEntry {
            entity_type: EntityType::Identity,
            entity_id: i.id,
            updated_at: i.updated_at,
            is_deleted: false,
        });
    }
    for pi in v.list_proxy_identities()? {
        entries.push(ManifestEntry {
            entity_type: EntityType::ProxyIdentity,
            entity_id: pi.id,
            updated_at: pi.updated_at,
            is_deleted: false,
        });
    }
    for g in v.list_groups()? {
        entries.push(ManifestEntry {
            entity_type: EntityType::Group,
            entity_id: g.id,
            updated_at: g.updated_at,
            is_deleted: false,
        });
    }
    for s in v.list_snippets()? {
        entries.push(ManifestEntry {
            entity_type: EntityType::Snippet,
            entity_id: s.id,
            updated_at: s.updated_at,
            is_deleted: false,
        });
    }
    for kh in v.list_known_hosts()? {
        entries.push(ManifestEntry {
            entity_type: EntityType::KnownHost,
            entity_id: kh.id,
            updated_at: kh.updated_at,
            is_deleted: false,
        });
    }
    for cp in v.list_cloud_profiles()? {
        entries.push(ManifestEntry {
            entity_type: EntityType::CloudProfile,
            entity_id: cp.id,
            updated_at: cp.updated_at,
            is_deleted: false,
        });
    }

    // Tombstones. A live entity always wins over a stale tombstone for
    // the same id (the entity was re-created from a newer peer copy
    // after the delete), so we only surface tombstones whose id isn't
    // already present as a live entry above.
    let live: std::collections::HashSet<(EntityType, Uuid)> =
        entries.iter().map(|e| (e.entity_type, e.entity_id)).collect();
    for tomb in v.list_tombstones()? {
        let Some(entity_type) = EntityType::from_wire_str(&tomb.entity_type) else {
            // Tombstone for an entity type this build doesn't know.
            // Skip it rather than fail the whole manifest.
            continue;
        };
        if live.contains(&(entity_type, tomb.entity_id)) {
            continue;
        }
        entries.push(ManifestEntry {
            entity_type,
            entity_id: tomb.entity_id,
            updated_at: tomb.deleted_at,
            is_deleted: true,
        });
    }

    Ok(entries)
}

/// Collect serialized records requested by the peer. A requested ref
/// that matches a tombstone is returned as a deletion marker (empty
/// payload, `is_deleted = true`) instead of an entity payload.
///
/// `shared_secret` is the X25519-derived key from pairing time. When
/// `Some`, every non-tombstone payload is sealed with
/// ChaCha20-Poly1305 before going on the wire. Tombstone records skip
/// encryption (their payload is empty by construction).
pub(crate) fn collect_records(
    vault: &Arc<std::sync::Mutex<VaultStore>>,
    needed: &[protocol::DeltaRef],
    shared_secret: Option<&[u8; 32]>,
) -> Result<Vec<protocol::SyncRecord>, SyncError> {
    let v = vault.lock().map_err(|_| SyncError::Vault("Lock".into()))?;
    // Tombstones recorded in `sync_metadata`. Loaded once up front so a
    // large `needed` list doesn't re-query per ref.
    let tombstones = v.list_tombstones()?;
    // Off by default. When on, password fields are included in the
    // wrapper payloads, older peers ignore them automatically. The
    // setting lives in the SQLite `settings` table so it flips per
    // device without touching the model.
    let sync_passwords = v
        .get_setting("sync_passwords")
        .ok()
        .flatten()
        .as_deref()
        == Some("true");
    let mut records = Vec::new();

    for delta in needed {
        // A requested ref that matches a tombstone is a deletion: emit
        // a marker record with an empty payload carrying the deletion
        // timestamp, so the receiver's LWW resolves it like any other
        // record and `apply_records` runs the local delete.
        if let Some(tomb) = tombstones.iter().find(|t| {
            t.entity_id == delta.entity_id
                && EntityType::from_wire_str(&t.entity_type) == Some(delta.entity_type)
        }) {
            records.push(protocol::SyncRecord {
                entity_type: delta.entity_type,
                entity_id: delta.entity_id,
                updated_at: tomb.deleted_at,
                is_deleted: true,
                payload: Vec::new(),
            });
            continue;
        }

        // For now, payload is unencrypted JSON (E2E encryption uses shared secret, added in pairing flow)
        let payload = match delta.entity_type {
            EntityType::Connection => {
                let conns = v.list_connections()?;
                conns.iter().find(|c| c.id == delta.entity_id).map(|c| {
                    let password = if sync_passwords {
                        v.get_connection_password(&c.id).ok().flatten()
                    } else {
                        None
                    };
                    let proxy_password = if sync_passwords {
                        v.get_proxy_password(&c.id).ok().flatten()
                    } else {
                        None
                    };
                    let wrapper = protocol::SyncConnection {
                        connection: c.clone(),
                        password,
                        proxy_password,
                    };
                    serde_json::to_vec(&wrapper).unwrap_or_default()
                })
            }
            EntityType::SshKey => {
                let keys = v.list_keys()?;
                keys.iter()
                    .find(|k| k.id == delta.entity_id)
                    .map(|k| serde_json::to_vec(k).unwrap_or_default())
            }
            EntityType::Identity => {
                let idents = v.list_identities()?;
                idents.iter().find(|i| i.id == delta.entity_id).map(|i| {
                    let password = if sync_passwords {
                        v.get_identity_password(&i.id).ok().flatten()
                    } else {
                        None
                    };
                    let wrapper = protocol::SyncIdentity {
                        identity: i.clone(),
                        password,
                    };
                    serde_json::to_vec(&wrapper).unwrap_or_default()
                })
            }
            EntityType::ProxyIdentity => {
                let items = v.list_proxy_identities()?;
                items.iter().find(|pi| pi.id == delta.entity_id).map(|pi| {
                    let password = if sync_passwords {
                        v.get_proxy_identity_password(&pi.id).ok().flatten()
                    } else {
                        None
                    };
                    let wrapper = protocol::SyncProxyIdentity {
                        proxy_identity: pi.clone(),
                        password,
                    };
                    serde_json::to_vec(&wrapper).unwrap_or_default()
                })
            }
            EntityType::Group => {
                let groups = v.list_groups()?;
                groups.iter()
                    .find(|g| g.id == delta.entity_id)
                    .map(|g| serde_json::to_vec(g).unwrap_or_default())
            }
            EntityType::Snippet => {
                let snippets = v.list_snippets()?;
                snippets.iter()
                    .find(|s| s.id == delta.entity_id)
                    .map(|s| serde_json::to_vec(s).unwrap_or_default())
            }
            EntityType::KnownHost => {
                let hosts = v.list_known_hosts()?;
                hosts.iter()
                    .find(|kh| kh.id == delta.entity_id)
                    .map(|kh| serde_json::to_vec(kh).unwrap_or_default())
            }
            EntityType::CloudProfile => {
                let items = v.list_cloud_profiles()?;
                items.iter().find(|cp| cp.id == delta.entity_id).map(|cp| {
                    let secret = if sync_passwords {
                        v.get_cloud_profile_secret(&cp.id).ok().flatten()
                    } else {
                        None
                    };
                    let wrapper = protocol::SyncCloudProfile {
                        profile: cp.clone(),
                        secret,
                    };
                    serde_json::to_vec(&wrapper).unwrap_or_default()
                })
            }
        };

        if let Some(data) = payload {
            // Seal the payload with the per-peer shared secret. A
            // missing secret means we're talking to a legacy peer that
            // never did the X25519 exchange; ship the plaintext so
            // they can still parse it.
            let wire_payload = match shared_secret {
                Some(secret) => crypto::encrypt_payload(&data, secret)?,
                None => data,
            };
            records.push(protocol::SyncRecord {
                entity_type: delta.entity_type,
                entity_id: delta.entity_id,
                updated_at: chrono::Utc::now(),
                is_deleted: false,
                payload: wire_payload,
            });
        }
    }

    Ok(records)
}

/// Apply received records to the local vault. A record with
/// `is_deleted = true` runs the matching `delete_*`, which also records
/// a fresh local tombstone, so the deletion keeps propagating onward to
/// this device's other peers.
///
/// `shared_secret` is the X25519-derived key from pairing time. When
/// `Some`, every non-tombstone payload is unsealed with
/// ChaCha20-Poly1305 before deserialization. A decrypt failure means
/// the record was forged or tampered with; we skip it and warn.
pub(crate) fn apply_records(
    vault: &Arc<std::sync::Mutex<VaultStore>>,
    records: &[protocol::SyncRecord],
    shared_secret: Option<&[u8; 32]>,
) -> Result<(), SyncError> {
    let v = vault.lock().map_err(|_| SyncError::Vault("Lock".into()))?;

    for record in records {
        if record.is_deleted {
            // Handle deletion
            match record.entity_type {
                EntityType::Connection => { let _ = v.delete_connection(&record.entity_id); }
                EntityType::SshKey => { let _ = v.delete_key(&record.entity_id); }
                EntityType::Identity => { let _ = v.delete_identity(&record.entity_id); }
                EntityType::ProxyIdentity => { let _ = v.delete_proxy_identity(&record.entity_id); }
                EntityType::Group => { let _ = v.delete_group(&record.entity_id); }
                EntityType::Snippet => { let _ = v.delete_snippet(&record.entity_id); }
                EntityType::KnownHost => { let _ = v.delete_known_host(&record.entity_id); }
                EntityType::CloudProfile => { let _ = v.delete_cloud_profile(&record.entity_id); }
            }
            continue;
        }

        // Unseal the payload with the per-peer secret. A decrypt
        // failure (tampering, key mismatch, legacy peer that didn't
        // encrypt) returns owned bytes either way; the deserializer
        // catches the garbage path with a parse error and we skip.
        let payload: std::borrow::Cow<'_, [u8]> = match shared_secret {
            Some(secret) => match crypto::decrypt_payload(&record.payload, secret) {
                Ok(plain) => std::borrow::Cow::Owned(plain),
                Err(e) => {
                    tracing::warn!(
                        "sync: failed to decrypt {} {}: {e}",
                        record.entity_type,
                        record.entity_id
                    );
                    continue;
                }
            },
            None => std::borrow::Cow::Borrowed(&record.payload),
        };

        match record.entity_type {
            EntityType::Connection => {
                // `SyncConnection` flattens the inner `Connection`, so a
                // payload from a pre-wrapper peer (bare `Connection` JSON)
                // still deserializes, the optional password fields just
                // resolve to `None` via `#[serde(default)]`.
                if let Ok(sc) = serde_json::from_slice::<protocol::SyncConnection>(&payload) {
                    let id = sc.connection.id;
                    let _ = v.save_connection(&sc.connection, sc.password.as_deref());
                    if let Some(pp) = &sc.proxy_password {
                        let _ = v.set_proxy_password(&id, Some(pp));
                    }
                }
            }
            EntityType::SshKey => {
                if let Ok(key) = serde_json::from_slice::<oryxis_core::models::SshKey>(&payload) {
                    let _ = v.save_key(&key, None);
                }
            }
            EntityType::Identity => {
                if let Ok(si) = serde_json::from_slice::<protocol::SyncIdentity>(&payload) {
                    let _ = v.save_identity(&si.identity, si.password.as_deref());
                }
            }
            EntityType::ProxyIdentity => {
                if let Ok(spi) = serde_json::from_slice::<protocol::SyncProxyIdentity>(&payload) {
                    let _ = v.save_proxy_identity(&spi.proxy_identity, spi.password.as_deref());
                }
            }
            EntityType::Group => {
                if let Ok(group) = serde_json::from_slice::<oryxis_core::models::Group>(&payload) {
                    let _ = v.save_group(&group);
                }
            }
            EntityType::Snippet => {
                if let Ok(snippet) = serde_json::from_slice::<oryxis_core::models::Snippet>(&payload) {
                    let _ = v.save_snippet(&snippet);
                }
            }
            EntityType::KnownHost => {
                if let Ok(kh) = serde_json::from_slice::<oryxis_core::models::KnownHost>(&payload) {
                    let _ = v.save_known_host(&kh);
                }
            }
            EntityType::CloudProfile => {
                if let Ok(scp) = serde_json::from_slice::<protocol::SyncCloudProfile>(&payload) {
                    let _ = v.save_cloud_profile(&scp.profile, scp.secret.as_deref());
                }
            }
        }
    }

    Ok(())
}

/// Sync with all active paired peers.
async fn sync_all_peers(
    vault: &Arc<std::sync::Mutex<VaultStore>>,
    identity: &DeviceIdentity,
    _config: &SyncConfig,
    event_tx: &mpsc::UnboundedSender<SyncEvent>,
) -> Result<(), SyncError> {
    let peers = {
        let v = vault.lock().map_err(|_| SyncError::Vault("Lock".into()))?;
        v.list_sync_peers()?
    };

    for peer in peers.iter().filter(|p| p.is_active) {
        if let (Some(ip), Some(port)) = (&peer.last_known_ip, peer.last_known_port) {
            let addr: SocketAddr = format!("{}:{}", ip, port)
                .parse()
                .map_err(|e| SyncError::Transport(format!("Parse addr: {}", e)))?;

            let _ = event_tx.send(SyncEvent::SyncStarted { peer_id: peer.peer_id });

            match sync_with_peer(vault, identity, &peer.peer_id, addr).await {
                Ok((pushed, pulled)) => {
                    let v = vault.lock().map_err(|_| SyncError::Vault("Lock".into()))?;
                    let _ = v.update_sync_peer_last_synced(&peer.peer_id);
                    drop(v);
                    let _ = event_tx.send(SyncEvent::SyncCompleted {
                        peer_id: peer.peer_id,
                        pushed,
                        pulled,
                    });
                }
                Err(e) => {
                    let _ = event_tx.send(SyncEvent::SyncFailed {
                        peer_id: peer.peer_id,
                        error: e.to_string(),
                    });
                }
            }
        }
    }

    Ok(())
}

/// Sync with a specific peer (client side, initiates connection).
async fn sync_with_peer(
    vault: &Arc<std::sync::Mutex<VaultStore>>,
    identity: &DeviceIdentity,
    peer_id: &Uuid,
    addr: SocketAddr,
) -> Result<(usize, usize), SyncError> {
    let client = transport::create_client_endpoint()?;

    let connection = client
        .connect(addr, "oryxis-sync")
        .map_err(|e| SyncError::Transport(format!("Connect: {}", e)))?
        .await
        .map_err(|e| SyncError::Transport(format!("Handshake: {}", e)))?;

    // Channel-binding exporter from the TLS session. Both sides will
    // derive the same value if (and only if) they share the same TLS
    // session, which is what we sign with the long-term Ed25519 key.
    let exporter = derive_session_exporter(&connection)?;

    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .map_err(|e| SyncError::Transport(format!("Open stream: {}", e)))?;

    // Send Hello with our channel-bound signature
    let our_signature = crypto::sign_session_handshake(&identity.signing_key, &exporter);
    transport::send_message(&mut send, &SyncMessage::Hello {
        device_id: identity.device_id,
        protocol_version: PROTOCOL_VERSION,
        auth_signature: our_signature.to_vec(),
    }).await?;

    // Receive HelloAck and verify the server's signature against the
    // pubkey we stored at pairing time. If the server is a MITM (its
    // TLS session with us has a different exporter than the real
    // peer's), the relayed signature will fail verification here.
    let msg = transport::recv_message(&mut recv).await?;
    let peer_auth_sig = match msg {
        SyncMessage::HelloAck {
            device_id,
            auth_signature,
            ..
        } => {
            if device_id != *peer_id {
                return Err(SyncError::Protocol("Peer ID mismatch".into()));
            }
            auth_signature
        }
        _ => return Err(SyncError::Protocol("Expected HelloAck".into())),
    };

    let peer_pubkey = {
        let vault_guard = vault.lock().map_err(|_| SyncError::Vault("Lock failed".into()))?;
        let peers = vault_guard.list_sync_peers()?;
        peers
            .into_iter()
            .find(|p| p.peer_id == *peer_id && p.is_active)
            .map(|p| p.public_key)
            .ok_or_else(|| SyncError::PeerNotFound(peer_id.to_string()))?
    };

    crypto::verify_session_handshake(&peer_pubkey, &exporter, &peer_auth_sig).map_err(|e| {
        SyncError::PairingFailed(format!(
            "Peer {} HelloAck signature did not verify: {}",
            peer_id, e
        ))
    })?;

    // Pull the X25519 shared secret once: every payload exchanged in
    // this session is sealed with it. v4 peers always carry one (set
    // at pairing); a missing secret means we're talking to a legacy
    // peer that the version check should already have rejected.
    let shared_secret = peer_shared_secret(vault, peer_id)?;
    let shared_secret = shared_secret.as_ref();

    // Request manifest
    transport::send_message(&mut send, &SyncMessage::ManifestRequest).await?;
    let remote_manifest = match transport::recv_message(&mut recv).await? {
        SyncMessage::Manifest { entries } => entries,
        _ => return Err(SyncError::Protocol("Expected Manifest".into())),
    };

    // Build local manifest
    let local_manifest = build_manifest(vault)?;

    // Compare manifests using LWW
    let mut needed_from_remote = Vec::new();
    let mut to_push_to_remote = Vec::new();

    for remote_entry in &remote_manifest {
        if let Some(local_entry) = local_manifest.iter().find(|l| {
            l.entity_type == remote_entry.entity_type && l.entity_id == remote_entry.entity_id
        }) {
            match crate::conflict::resolve(local_entry, remote_entry) {
                crate::conflict::SyncAction::AcceptRemote => {
                    needed_from_remote.push(protocol::DeltaRef {
                        entity_type: remote_entry.entity_type,
                        entity_id: remote_entry.entity_id,
                    });
                }
                crate::conflict::SyncAction::PushLocal => {
                    to_push_to_remote.push(protocol::DeltaRef {
                        entity_type: local_entry.entity_type,
                        entity_id: local_entry.entity_id,
                    });
                }
                crate::conflict::SyncAction::Skip => {}
            }
        } else {
            // Not in local, pull from remote
            needed_from_remote.push(protocol::DeltaRef {
                entity_type: remote_entry.entity_type,
                entity_id: remote_entry.entity_id,
            });
        }
    }

    // Records only in local, push to remote
    for local_entry in &local_manifest {
        if !remote_manifest.iter().any(|r| {
            r.entity_type == local_entry.entity_type && r.entity_id == local_entry.entity_id
        }) {
            to_push_to_remote.push(protocol::DeltaRef {
                entity_type: local_entry.entity_type,
                entity_id: local_entry.entity_id,
            });
        }
    }

    let mut pulled = 0;
    let mut pushed = 0;

    // Pull from remote
    if !needed_from_remote.is_empty() {
        transport::send_message(&mut send, &SyncMessage::DeltaRequest {
            needed: needed_from_remote,
        }).await?;
        match transport::recv_message(&mut recv).await? {
            SyncMessage::DeltaResponse { records } => {
                pulled = records.len();
                apply_records(vault, &records, shared_secret)?;
            }
            _ => return Err(SyncError::Protocol("Expected DeltaResponse".into())),
        }
    }

    // Push to remote
    if !to_push_to_remote.is_empty() {
        let records = collect_records(vault, &to_push_to_remote, shared_secret)?;
        pushed = records.len();
        transport::send_message(&mut send, &SyncMessage::DeltaPush { records }).await?;
        match transport::recv_message(&mut recv).await? {
            SyncMessage::DeltaAck { .. } => {}
            _ => return Err(SyncError::Protocol("Expected DeltaAck".into())),
        }
    }

    // Done
    transport::send_message(&mut send, &SyncMessage::Bye).await?;

    Ok((pushed, pulled))
}

/// Extract the RFC 5705 keying-material exporter from a QUIC TLS session.
/// Both peers of a non-MITM'd handshake derive the same bytes here, so
/// signing it with each side's Ed25519 identity gives a channel-bound
/// proof of identity that resists relay attacks. A MITM holding two
/// separate TLS sessions sees two distinct exporters and cannot forge.
fn derive_session_exporter(
    connection: &quinn::Connection,
) -> Result<[u8; crypto::SESSION_EXPORTER_LEN], SyncError> {
    let mut buf = [0u8; crypto::SESSION_EXPORTER_LEN];
    connection
        .export_keying_material(&mut buf, crypto::SESSION_EXPORTER_LABEL, &[])
        .map_err(|e| SyncError::Crypto(format!("Exporter unavailable: {:?}", e)))?;
    Ok(buf)
}
