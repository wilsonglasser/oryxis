use std::net::SocketAddr;
use std::sync::Arc;

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

/// P2P sync engine.
pub struct SyncEngine {
    config: SyncConfig,
    identity: DeviceIdentity,
    vault: Arc<std::sync::Mutex<VaultStore>>,
    event_tx: mpsc::UnboundedSender<SyncEvent>,
    event_rx: Option<mpsc::UnboundedReceiver<SyncEvent>>,
    shutdown_tx: Option<tokio::sync::broadcast::Sender<()>>,
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
        }
    }

    /// Take the event receiver (can only be called once).
    pub fn take_events(&mut self) -> Option<mpsc::UnboundedReceiver<SyncEvent>> {
        self.event_rx.take()
    }

    /// Start background sync tasks (QUIC listener, mDNS, signaling heartbeat, auto-sync timer).
    pub async fn start(&mut self) -> Result<(), SyncError> {
        if !self.config.enabled {
            return Ok(());
        }

        // rustls 0.23 requires an explicit CryptoProvider when both
        // `ring` and `aws-lc-rs` could be linked transitively. We pin
        // `ring` here once, before any TLS handshake. `install_default`
        // errors if already installed — fine, treat as idempotent.
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
        let mut shutdown_rx = shutdown_tx.subscribe();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    incoming = endpoint.accept() => {
                        if let Some(incoming) = incoming {
                            let vault = vault.clone();
                            let identity = identity.clone();
                            let event_tx = event_tx.clone();
                            tokio::spawn(async move {
                                if let Err(e) = handle_incoming(incoming, vault, identity, event_tx).await {
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

    /// Start pairing — returns a 6-digit code to show the user.
    pub fn start_pairing(&self) -> String {
        let code = crypto::generate_pairing_code();
        let _ = self.event_tx.send(SyncEvent::PairingCodeGenerated {
            code: code.clone(),
        });
        code
    }

    /// Get the device identity.
    pub fn identity(&self) -> &DeviceIdentity {
        &self.identity
    }

    /// Get the config.
    pub fn config(&self) -> &SyncConfig {
        &self.config
    }
}

/// Handle an incoming QUIC connection.
async fn handle_incoming(
    incoming: quinn::Incoming,
    vault: Arc<std::sync::Mutex<VaultStore>>,
    identity: DeviceIdentity,
    event_tx: mpsc::UnboundedSender<SyncEvent>,
) -> Result<(), SyncError> {
    let connection = incoming
        .await
        .map_err(|e| SyncError::Transport(format!("Accept: {}", e)))?;

    let (mut send, mut recv) = connection
        .accept_bi()
        .await
        .map_err(|e| SyncError::Transport(format!("Accept stream: {}", e)))?;

    // Receive Hello
    let msg = transport::recv_message(&mut recv).await?;
    let peer_id = match msg {
        SyncMessage::Hello { device_id, protocol_version } => {
            if protocol_version != PROTOCOL_VERSION {
                return Err(SyncError::Protocol("Version mismatch".into()));
            }
            device_id
        }
        SyncMessage::PairingRequest { .. } => {
            // Handle pairing on server side
            // For now, reject (pairing needs the code from UI)
            transport::send_message(&mut send, &SyncMessage::PairingRejected {
                reason: "Use the pairing code flow".into(),
            }).await?;
            return Ok(());
        }
        _ => return Err(SyncError::Protocol("Expected Hello".into())),
    };

    // Send HelloAck
    transport::send_message(&mut send, &SyncMessage::HelloAck {
        device_id: identity.device_id,
        protocol_version: PROTOCOL_VERSION,
    }).await?;

    // Check if peer is known
    let is_known = {
        let vault_guard = vault.lock().map_err(|_| SyncError::Vault("Lock failed".into()))?;
        let peers = vault_guard.list_sync_peers()?;
        peers.iter().any(|p| p.peer_id == peer_id && p.is_active)
    };

    if !is_known {
        tracing::warn!("Unknown peer {} tried to connect", peer_id);
        transport::send_message(&mut send, &SyncMessage::Bye).await?;
        return Ok(());
    }

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

/// Handle the sync protocol after handshake.
async fn handle_sync_session(
    send: &mut quinn::SendStream,
    recv: &mut quinn::RecvStream,
    vault: &Arc<std::sync::Mutex<VaultStore>>,
    _peer_id: &Uuid,
) -> Result<(usize, usize), SyncError> {
    let mut pushed = 0;
    let mut pulled = 0;

    loop {
        let msg = transport::recv_message(recv).await?;
        match msg {
            SyncMessage::ManifestRequest => {
                let manifest = build_manifest(vault)?;
                transport::send_message(send, &SyncMessage::Manifest { entries: manifest }).await?;
            }
            SyncMessage::DeltaRequest { needed } => {
                // Peer wants these records from us
                let records = collect_records(vault, &needed)?;
                pushed += records.len();
                transport::send_message(send, &SyncMessage::DeltaResponse { records }).await?;
            }
            SyncMessage::DeltaPush { records } => {
                // Peer is pushing records to us
                let count = records.len();
                apply_records(vault, &records)?;
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

/// Build a manifest of all syncable entities in the vault.
fn build_manifest(
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

    Ok(entries)
}

/// Collect serialized records requested by the peer.
fn collect_records(
    vault: &Arc<std::sync::Mutex<VaultStore>>,
    needed: &[protocol::DeltaRef],
) -> Result<Vec<protocol::SyncRecord>, SyncError> {
    let v = vault.lock().map_err(|_| SyncError::Vault("Lock".into()))?;
    // Off by default. When on, password fields are included in the
    // wrapper payloads — older peers ignore them automatically. The
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
        };

        if let Some(data) = payload {
            records.push(protocol::SyncRecord {
                entity_type: delta.entity_type,
                entity_id: delta.entity_id,
                updated_at: chrono::Utc::now(),
                is_deleted: false,
                payload: data,
            });
        }
    }

    Ok(records)
}

/// Apply received records to the local vault.
fn apply_records(
    vault: &Arc<std::sync::Mutex<VaultStore>>,
    records: &[protocol::SyncRecord],
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
            }
            continue;
        }

        match record.entity_type {
            EntityType::Connection => {
                // `SyncConnection` flattens the inner `Connection`, so a
                // payload from a pre-wrapper peer (bare `Connection` JSON)
                // still deserializes — the optional password fields just
                // resolve to `None` via `#[serde(default)]`.
                if let Ok(sc) = serde_json::from_slice::<protocol::SyncConnection>(&record.payload) {
                    let id = sc.connection.id;
                    let _ = v.save_connection(&sc.connection, sc.password.as_deref());
                    if let Some(pp) = &sc.proxy_password {
                        let _ = v.set_proxy_password(&id, Some(pp));
                    }
                }
            }
            EntityType::SshKey => {
                if let Ok(key) = serde_json::from_slice::<oryxis_core::models::SshKey>(&record.payload) {
                    let _ = v.save_key(&key, None);
                }
            }
            EntityType::Identity => {
                if let Ok(si) = serde_json::from_slice::<protocol::SyncIdentity>(&record.payload) {
                    let _ = v.save_identity(&si.identity, si.password.as_deref());
                }
            }
            EntityType::ProxyIdentity => {
                if let Ok(spi) = serde_json::from_slice::<protocol::SyncProxyIdentity>(&record.payload) {
                    let _ = v.save_proxy_identity(&spi.proxy_identity, spi.password.as_deref());
                }
            }
            EntityType::Group => {
                if let Ok(group) = serde_json::from_slice::<oryxis_core::models::Group>(&record.payload) {
                    let _ = v.save_group(&group);
                }
            }
            EntityType::Snippet => {
                if let Ok(snippet) = serde_json::from_slice::<oryxis_core::models::Snippet>(&record.payload) {
                    let _ = v.save_snippet(&snippet);
                }
            }
            EntityType::KnownHost => {
                if let Ok(kh) = serde_json::from_slice::<oryxis_core::models::KnownHost>(&record.payload) {
                    let _ = v.save_known_host(&kh);
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

/// Sync with a specific peer (client side — initiates connection).
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

    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .map_err(|e| SyncError::Transport(format!("Open stream: {}", e)))?;

    // Send Hello
    transport::send_message(&mut send, &SyncMessage::Hello {
        device_id: identity.device_id,
        protocol_version: PROTOCOL_VERSION,
    }).await?;

    // Receive HelloAck
    let msg = transport::recv_message(&mut recv).await?;
    match msg {
        SyncMessage::HelloAck { device_id, .. } => {
            if device_id != *peer_id {
                return Err(SyncError::Protocol("Peer ID mismatch".into()));
            }
        }
        _ => return Err(SyncError::Protocol("Expected HelloAck".into())),
    }

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
            // Not in local — pull from remote
            needed_from_remote.push(protocol::DeltaRef {
                entity_type: remote_entry.entity_type,
                entity_id: remote_entry.entity_id,
            });
        }
    }

    // Records only in local — push to remote
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
                apply_records(vault, &records)?;
            }
            _ => return Err(SyncError::Protocol("Expected DeltaResponse".into())),
        }
    }

    // Push to remote
    if !to_push_to_remote.is_empty() {
        let records = collect_records(vault, &to_push_to_remote)?;
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
