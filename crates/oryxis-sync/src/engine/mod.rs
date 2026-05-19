use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use uuid::Uuid;

/// Hard cap on concurrent per-sender sessions tracked by the relay
/// inbox demux. A bearer-token holder cycling fresh `X-Sender-Id`
/// UUIDs would otherwise force the receiver to allocate an mpsc and
/// spawn a task per id without bound. 64 covers any realistic
/// fleet size and triggers FIFO eviction beyond that.
const MAX_RELAY_SESSIONS: usize = 64;

/// FIFO-evicting map from a peer's sender id to the mpsc channel
/// feeding its in-flight session. When the map reaches
/// [`MAX_RELAY_SESSIONS`], the oldest entry is dropped (its sender
/// closes, the spawned session task winds down on next `recv`)
/// before the new entry is inserted.
struct BoundedSessionMap {
    map: HashMap<Uuid, tokio::sync::mpsc::UnboundedSender<SyncMessage>>,
    fifo: VecDeque<Uuid>,
}

impl BoundedSessionMap {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            fifo: VecDeque::new(),
        }
    }

    fn get(&self, k: &Uuid) -> Option<&tokio::sync::mpsc::UnboundedSender<SyncMessage>> {
        self.map.get(k)
    }

    fn remove(&mut self, k: &Uuid) {
        if self.map.remove(k).is_some() {
            self.fifo.retain(|x| x != k);
        }
    }

    fn insert(&mut self, k: Uuid, tx: tokio::sync::mpsc::UnboundedSender<SyncMessage>) {
        if self.map.contains_key(&k) {
            // Re-insert wins; refresh FIFO position so the renewed
            // session isn't evicted before older idle peers.
            self.fifo.retain(|x| x != &k);
        } else if self.map.len() >= MAX_RELAY_SESSIONS {
            if let Some(evict) = self.fifo.pop_front() {
                self.map.remove(&evict);
                tracing::warn!(
                    "relay session map at cap ({}), evicting {evict}",
                    MAX_RELAY_SESSIONS
                );
            }
        }
        self.map.insert(k, tx);
        self.fifo.push_back(k);
    }
}

use oryxis_vault::VaultStore;

use crate::config::SyncConfig;
use crate::crypto::{self, DeviceIdentity};
use crate::discovery;
use crate::error::SyncError;
use crate::protocol::SyncMessage;
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
    /// The signaling-server heartbeat just successfully POSTed our
    /// public address. Emitted on each *new* public IP (re-registers
    /// for the same IP are silent). Tells the user "remote pairing
    /// will find me at this address now".
    SignalingRegistered {
        ip: String,
        port: u16,
    },
    /// Either the STUN probe or the signaling POST failed; the
    /// heartbeat will retry on its next tick. Surfaced so the user
    /// knows cross-network pairing isn't currently reachable.
    SignalingFailed {
        reason: String,
    },
    /// A peer attempted to handshake with a protocol version this
    /// build doesn't speak. Surfaced so the UI can prompt the user
    /// to update (or update the peer). Fired from both sides:
    /// server when an incoming Hello mismatches, and client when an
    /// outgoing HelloAck does.
    VersionMismatch {
        peer_id: Uuid,
        peer_version: u32,
        local_version: u32,
    },
    /// A paired peer hasn't synced in long enough that the tombstone
    /// GC window (30 days, see [`TOMBSTONE_TTL_DAYS`]) is closing in
    /// on it. Fired at boot for every peer with
    /// `last_synced_at < now - 25 days` so the UI can nudge the user
    /// to bring the peer online before deletes silently resurrect.
    PeerStaleWarning {
        peer_id: Uuid,
        days_since_sync: i64,
    },
}

/// State for a device that is currently hosting a pairing code and
/// waiting for a peer to join. Lives behind a `Mutex` shared between
/// the engine, its `SyncHandle`s, and the QUIC accept loop. Visible
/// to the submodules so `pairing.rs` can validate against it.
pub(crate) struct HostingPairing {
    pub(crate) code: String,
    pub(crate) expires_at: Instant,
    /// Per-source failed-attempt counter. Key is a short label that
    /// identifies the joiner network identity:
    ///   `quic:<ip>` for direct LAN / cross-NAT QUIC dial
    ///   `relay:<device_id>` for relay-routed sessions
    /// Once a given source hits [`MAX_PAIRING_ATTEMPTS`] only that
    /// source is rejected (with `Rate limited` instead of `Wrong
    /// pairing code`). The hosted code stays alive for everyone
    /// else, so an attacker grinding the 10^6 code space from one
    /// IP cannot deny service to the legitimate user paired from
    /// a different network. The total map size is also capped via
    /// [`MAX_PAIRING_SOURCES`] to keep memory bounded under churn.
    pub(super) attempts_by_source: HashMap<String, u32>,
}

/// Max failed attempts a single joiner source can make against the
/// hosted code before that source is rejected. Conservative: the
/// legitimate joiner types the code once, so 3 tries is enough room
/// for a typo and well below the brute-force regime.
pub(super) const MAX_PAIRING_ATTEMPTS: u32 = 3;

/// Soft cap on distinct sources tracked at once. An attacker cycling
/// fresh sender IDs would otherwise grow `attempts_by_source` without
/// bound; at the cap we clear the oldest entries (here: all of them,
/// since the typing legit user is the only entry that matters and we
/// don't expect to be at the cap under normal use).
pub(super) const MAX_PAIRING_SOURCES: usize = 1024;

/// Tombstones older than this drop on engine boot. Should outlive any
/// realistic offline gap between a paired peer's syncs; a peer that
/// reconnects after the window will silently miss the deletion (and
/// the table won't grow forever in exchange).
const TOMBSTONE_TTL_DAYS: u32 = 30;

mod manifest;
mod pairing;
mod relay_glue;
mod session;

pub use pairing::{format_pairing_link, parse_pairing_link};

// Re-exported so `crate::engine::build_manifest` etc. still resolve
// for the in-crate integration tests after the manifest split.
#[allow(unused_imports)]
pub(crate) use manifest::{apply_records, build_manifest, collect_records};
use relay_glue::run_relay_inbound_session;
use session::{handle_incoming, sync_all_peers};

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
    /// `pub(crate)` so the per-source attempt-cap regression test in
    /// `tests.rs` can inspect that the hosted code survives a noisy
    /// joiner.
    pub(crate) hosting_pairing: Arc<std::sync::Mutex<Option<HostingPairing>>>,
    /// The QUIC port the server actually bound, known only after
    /// `start()`. Advertised to joiners in `PairingRequest` so the
    /// host can sync back to us.
    bound_port: u16,
    /// mDNS registration and browser handles. The `ServiceDaemon`
    /// drops would deregister the device from the LAN, so we have
    /// to keep them owned by the engine for as long as sync is up.
    /// Previously these were stack locals in `start()` and dropped
    /// the moment the function returned, silently disabling LAN
    /// discovery. Cleared in `stop()` to release the daemons.
    mdns_register: Option<mdns_sd::ServiceDaemon>,
    mdns_browse: Option<mdns_sd::ServiceDaemon>,
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
            mdns_register: None,
            mdns_browse: None,
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

        // Start mDNS registration and browsing. The `ServiceDaemon`s
        // have to stay alive for as long as sync is running, so we
        // hand them to the engine. Letting them drop here (the prior
        // behaviour) silently took LAN discovery offline the moment
        // `start()` returned.
        let (discovery_tx, mut discovery_rx) = mpsc::unbounded_channel();
        self.mdns_register =
            discovery::mdns::register(&self.identity.device_id, listen_port).ok();
        self.mdns_browse =
            discovery::mdns::browse(&self.identity.device_id, discovery_tx).ok();

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
            let heartbeat_tx = self.event_tx.clone();
            tokio::spawn(async move {
                let client = discovery::signaling::SignalingClient::new(&signaling_url, &token);
                let mut ticker =
                    tokio::time::interval(std::time::Duration::from_secs(60));
                let mut last_public_ip: Option<String> = None;
                // The Cloudflare Worker keeps registrations alive for
                // 5min; refresh every 3min to keep a safety margin
                // against the TTL even when our public IP is stable.
                let refresh_interval = std::time::Duration::from_secs(180);
                let mut last_register_at: Option<std::time::Instant> = None;
                loop {
                    tokio::select! {
                        _ = ticker.tick() => {
                            // Fresh ephemeral UDP socket per probe; the STUN
                            // server's reply tells us the NAT mapping it sees.
                            let socket = match tokio::net::UdpSocket::bind("0.0.0.0:0").await {
                                Ok(s) => s,
                                Err(e) => {
                                    let reason = format!("STUN bind failed: {e}");
                                    tracing::warn!("signaling: {reason}");
                                    let _ = heartbeat_tx.send(SyncEvent::SignalingFailed { reason });
                                    continue;
                                }
                            };
                            let pub_addr = match discovery::stun::get_public_addr(&socket).await {
                                Ok(a) => a,
                                Err(e) => {
                                    let reason = format!("STUN failed: {e}");
                                    tracing::warn!("signaling: {reason}");
                                    let _ = heartbeat_tx.send(SyncEvent::SignalingFailed { reason });
                                    continue;
                                }
                            };
                            let ip = pub_addr.ip().to_string();
                            // Re-register when the public IP changes
                            // (Wi-Fi roam, cell handoff) OR when the
                            // last register is older than the refresh
                            // interval. The Worker's KV entry has a
                            // 5-min TTL, so refreshing every 3 min
                            // keeps the device discoverable.
                            let ip_changed = last_public_ip.as_deref() != Some(ip.as_str());
                            let needs_refresh = last_register_at
                                .is_none_or(|t| t.elapsed() >= refresh_interval);
                            if !ip_changed && !needs_refresh {
                                continue;
                            }
                            match client.register(&identity, &ip, bound_port).await {
                                Ok(()) => {
                                    tracing::info!(
                                        "signaling: registered {} -> {}:{}",
                                        identity.device_id, ip, bound_port
                                    );
                                    last_public_ip = Some(ip.clone());
                                    last_register_at = Some(std::time::Instant::now());
                                    let _ = heartbeat_tx.send(SyncEvent::SignalingRegistered {
                                        ip,
                                        port: bound_port,
                                    });
                                }
                                Err(e) => {
                                    let reason = format!("register failed: {e}");
                                    tracing::warn!("signaling: {reason}");
                                    let _ = heartbeat_tx.send(SyncEvent::SignalingFailed { reason });
                                }
                            }
                        }
                        _ = shutdown_rx.recv() => {
                            // Best-effort unregister so the entry
                            // doesn't linger for the full 5 min TTL
                            // after the user turns sync off or quits.
                            // Only call if we have something to undo:
                            // a never-registered task has nothing to
                            // delete and we don't want a 404 in logs.
                            // Nested `if let` (not chained `&&`) since
                            // this crate is on edition 2021, where
                            // let-chains aren't stable.
                            if last_register_at.is_some() {
                                if let Err(e) = client.unregister(&identity).await {
                                    tracing::debug!("signaling: unregister on shutdown: {e}");
                                }
                            }
                            break;
                        }
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
            // Stale-peer warning: nudge the user when a paired peer
            // is approaching the tombstone GC cliff so they can bring
            // it online before deletes silently resurrect. Threshold
            // is 5 days inside the TTL so the user has time to act.
            let warn_after_days = TOMBSTONE_TTL_DAYS.saturating_sub(5) as i64;
            let now = chrono::Utc::now();
            if let Ok(peers) = v.list_sync_peers() {
                for peer in peers.iter().filter(|p| p.is_active) {
                    let Some(last) = peer.last_synced_at else { continue };
                    let days = (now - last).num_days();
                    if days >= warn_after_days {
                        let _ = self.event_tx.send(SyncEvent::PeerStaleWarning {
                            peer_id: peer.peer_id,
                            days_since_sync: days,
                        });
                    }
                }
            }
        }

        // Relay inbox listener. Long-polls the configured relay for
        // any frame addressed to us, demuxes by sender, and spawns a
        // handler task per session. Mirrors the QUIC accept loop but
        // over HTTP. When `signaling_url` isn't set there's no relay
        // to listen on, so this task is skipped entirely.
        if let Some(relay_url) = self.config.signaling_url.clone() {
            let token = self.config.signaling_token.clone().unwrap_or_default();
            let my_id = self.identity.device_id;
            let vault = self.vault.clone();
            let identity = self.identity.clone();
            let event_tx = self.event_tx.clone();
            let hosting_pairing = self.hosting_pairing.clone();
            let mut shutdown_rx = shutdown_tx.subscribe();
            tokio::spawn(async move {
                let client = crate::relay::RelayClient::new(&relay_url, &token, my_id);
                // device_id -> mpsc to its in-flight session. New
                // senders trigger a fresh server-side session.
                let sessions: Arc<std::sync::Mutex<BoundedSessionMap>> =
                    Arc::new(std::sync::Mutex::new(BoundedSessionMap::new()));
                loop {
                    tokio::select! {
                        biased;
                        _ = shutdown_rx.recv() => break,
                        recvd = client.recv(my_id) => {
                            let (from, msg) = match recvd {
                                Ok(pair) => pair,
                                Err(crate::SyncError::RelayUnavailable(detail)) => {
                                    // Permanent server-side condition
                                    // (404/410/501). Retrying just burns
                                    // network + battery; log loud once,
                                    // surface a SignalingFailed event
                                    // so the user sees why relay sync
                                    // went quiet, then exit the poll
                                    // task. Local mDNS + STUN paths
                                    // keep running because this loop
                                    // is the relay-specific path only.
                                    tracing::warn!(
                                        "relay inbox unavailable, giving up: {detail}"
                                    );
                                    let _ = event_tx.send(SyncEvent::SignalingFailed {
                                        reason: format!(
                                            "Relay inbox unavailable: {detail}"
                                        ),
                                    });
                                    break;
                                }
                                Err(e) => {
                                    tracing::debug!("relay inbox: {e}");
                                    // Backoff a touch before retrying;
                                    // long-poll errors usually mean
                                    // transient network glitches.
                                    tokio::time::sleep(
                                        std::time::Duration::from_secs(2),
                                    ).await;
                                    continue;
                                }
                            };
                            // Forward to an existing session if any.
                            // Mutex poison: recover the inner value
                            // (the map is just a routing table; a
                            // panicked holder doesn't corrupt the
                            // routing semantics) so a single bad
                            // session can't kill the whole relay
                            // demux. Same pattern relay/main.rs:117
                            // uses for its rate-limiter bucket.
                            {
                                let mut map = match sessions.lock() {
                                    Ok(g) => g,
                                    Err(p) => p.into_inner(),
                                };
                                if let Some(tx) = map.get(&from) {
                                    if tx.send(msg.clone()).is_ok() {
                                        continue;
                                    }
                                    map.remove(&from);
                                }
                            }
                            // No active session: spawn one. PairingRequest
                            // and ManifestRequest are the only valid
                            // session openers; other messages are dropped.
                            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                            match sessions.lock() {
                                Ok(mut g) => g.insert(from, tx.clone()),
                                Err(p) => p.into_inner().insert(from, tx.clone()),
                            }
                            let _ = tx.send(msg);
                            let session_client = client.clone();
                            let vault = vault.clone();
                            let identity = identity.clone();
                            let event_tx = event_tx.clone();
                            let hosting_pairing = hosting_pairing.clone();
                            let sessions = sessions.clone();
                            tokio::spawn(async move {
                                run_relay_inbound_session(
                                    session_client,
                                    from,
                                    rx,
                                    vault,
                                    identity,
                                    hosting_pairing,
                                    event_tx,
                                )
                                .await;
                                match sessions.lock() {
                                    Ok(mut g) => g.remove(&from),
                                    Err(p) => p.into_inner().remove(&from),
                                };
                            });
                        }
                    }
                }
            });
        }

        tracing::info!("Sync engine started (port {})", listen_port);
        Ok(())
    }

    /// Stop all background tasks.
    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        // Drop the mDNS daemons so we deregister from the LAN as
        // soon as the user toggles sync off (rather than lingering
        // in service browsers until the process exits).
        self.mdns_register = None;
        self.mdns_browse = None;
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
                attempts_by_source: HashMap::new(),
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

        // Tier 1: direct QUIC. Bounded at 8s so we don't make the user
        // wait the full 30s pairing budget on every blocked-NAT case.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let direct = tokio::time::timeout(
            std::time::Duration::from_secs(8),
            self.join_pairing_inner(addr, &code),
        )
        .await;
        match direct {
            Ok(Ok((device_id, device_name))) => {
                let _ = self.event_tx.send(SyncEvent::PairingCompleted {
                    device_id,
                    device_name,
                });
                return Ok(());
            }
            Ok(Err(e)) => {
                tracing::info!(
                    "direct pairing to {addr} failed ({e}); attempting relay fallback"
                );
            }
            Err(_) => {
                tracing::info!(
                    "direct pairing to {addr} timed out after 8s; attempting relay fallback"
                );
            }
        }

        // Tier 2: relay. The link already carries the host's device id
        // (decoded above) so we don't need any further signaling.
        let inner = self.join_pairing_via_relay(device_id, &code);
        let result =
            tokio::time::timeout(std::time::Duration::from_secs(30), inner).await;
        match result {
            Ok(Ok((dev_id, device_name))) => {
                let _ = self.event_tx.send(SyncEvent::PairingCompleted {
                    device_id: dev_id,
                    device_name,
                });
                Ok(())
            }
            Ok(Err(e)) => {
                let reason = format!("Relay pairing failed: {e}");
                let _ = self
                    .event_tx
                    .send(SyncEvent::PairingFailed { reason: reason.clone() });
                Err(SyncError::PairingFailed(reason))
            }
            Err(_) => {
                let reason =
                    "Relay pairing timed out after 30s. Host may be offline.".to_string();
                let _ = self
                    .event_tx
                    .send(SyncEvent::PairingFailed { reason: reason.clone() });
                Err(SyncError::PairingFailed(reason))
            }
        }
    }

    /// Join a peer that is hosting a pairing code: connect to `addr`,
    /// run the challenge/response handshake, and on success persist the
    /// host as a peer. Emits `PairingCompleted` / `PairingFailed`.
    pub async fn join_pairing(&self, addr: SocketAddr, code: String) -> Result<(), SyncError> {
        // 30s cap on the whole pairing handshake (QUIC connect +
        // challenge round + accepted). Long enough for slow networks,
        // short enough that the UI doesn't hang forever when the
        // host's QUIC port is firewalled / NAT-blocked.
        let inner = self.join_pairing_inner(addr, &code);
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            inner,
        )
        .await;
        match result {
            Ok(Ok((device_id, device_name))) => {
                let _ = self.event_tx.send(SyncEvent::PairingCompleted {
                    device_id,
                    device_name,
                });
                Ok(())
            }
            Ok(Err(e)) => {
                let _ = self.event_tx.send(SyncEvent::PairingFailed {
                    reason: e.to_string(),
                });
                Err(e)
            }
            Err(_) => {
                let reason = format!(
                    "Handshake timed out after 30s; {addr} is unreachable (NAT / firewall?)"
                );
                let _ = self
                    .event_tx
                    .send(SyncEvent::PairingFailed { reason: reason.clone() });
                Err(SyncError::PairingFailed(reason))
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
        let (send, recv) = connection
            .open_bi()
            .await
            .map_err(|e| SyncError::Transport(format!("Open stream: {}", e)))?;

        let mut transport = transport::SessionTransport::Quic { send, recv };
        pairing::run_pairing_as_joiner(
            &mut transport,
            &self.identity,
            &self.vault,
            self.listen_port,
            code,
            Some((addr.ip(), addr.port())),
        )
        .await
    }

    /// Run a pairing handshake over the relay instead of QUIC. Used
    /// when the joiner can't reach the host's QUIC port (typical for
    /// NAT-blocked / WSL / carrier-grade setups). `host_device_id`
    /// comes from the `oryxis://pair/<host>/<code>` link.
    async fn join_pairing_via_relay(
        &self,
        host_device_id: Uuid,
        code: &str,
    ) -> Result<(Uuid, String), SyncError> {
        let signaling_url = self
            .config
            .signaling_url
            .clone()
            .ok_or_else(|| SyncError::PairingFailed(
                "Relay not configured (Settings > Sync > Advanced > Signaling Server)".into(),
            ))?;
        let token = self.config.signaling_token.clone().unwrap_or_default();
        let client = crate::relay::RelayClient::new(
            &signaling_url,
            &token,
            self.identity.device_id,
        );
        let mut transport = transport::SessionTransport::RelayClient {
            client,
            peer_id: host_device_id,
            my_id: self.identity.device_id,
        };
        pairing::run_pairing_as_joiner(
            &mut transport,
            &self.identity,
            &self.vault,
            self.listen_port,
            code,
            None,
        )
        .await
    }
}

#[cfg(test)]
mod bounded_session_tests {
    use super::*;

    #[test]
    fn evicts_oldest_at_cap() {
        let mut map = BoundedSessionMap::new();
        let mut ids = Vec::new();
        // Fill to cap + 1: the very first insertion must be evicted.
        for _ in 0..(MAX_RELAY_SESSIONS + 1) {
            let id = Uuid::new_v4();
            ids.push(id);
            let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
            map.insert(id, tx);
        }
        assert_eq!(map.map.len(), MAX_RELAY_SESSIONS);
        assert!(
            map.get(&ids[0]).is_none(),
            "oldest entry should have been evicted"
        );
        for id in &ids[1..] {
            assert!(map.get(id).is_some(), "{id} should still be present");
        }
    }

    #[test]
    fn re_insert_refreshes_fifo_position() {
        let mut map = BoundedSessionMap::new();
        let mut ids = Vec::new();
        for _ in 0..MAX_RELAY_SESSIONS {
            let id = Uuid::new_v4();
            ids.push(id);
            let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
            map.insert(id, tx);
        }
        // Touch the oldest: it should now be the freshest, so the
        // NEXT new insertion evicts the *second* original, not the
        // refreshed one.
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        map.insert(ids[0], tx);
        let new_id = Uuid::new_v4();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        map.insert(new_id, tx);
        assert!(map.get(&ids[0]).is_some(), "refreshed entry must survive");
        assert!(
            map.get(&ids[1]).is_none(),
            "second-oldest must be the eviction target"
        );
    }
}


