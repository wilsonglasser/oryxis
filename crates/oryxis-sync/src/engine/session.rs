//! Sync session helpers extracted from `engine/mod.rs`.
//!
//! Owns the QUIC accept handler (`handle_incoming`), the
//! transport-agnostic session driver (`handle_sync_session`,
//! `run_sync_session_as_client`), the per-peer fan-out
//! (`sync_all_peers`, `sync_with_peer`, `sync_with_peer_via_relay`),
//! and the TLS channel-binding helper (`derive_session_exporter`).
//!
//! These all touch the vault through the same `Arc<Mutex<VaultStore>>`
//! handle that lives on `SyncEngine`, but none of them need direct
//! engine state: callers pass the bits they need (vault, identity,
//! config, event sink) as arguments.

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::sync::mpsc;
use uuid::Uuid;

use oryxis_vault::VaultStore;

use crate::config::SyncConfig;
use crate::crypto::{self, DeviceIdentity};
use crate::error::SyncError;
use crate::protocol::{self, SyncMessage, PROTOCOL_VERSION};
use crate::transport;

use super::manifest::{apply_records, build_manifest, collect_records, peer_shared_secret};
use super::{pairing, HostingPairing, SyncEvent};

/// Handle an incoming QUIC connection.
pub(super) async fn handle_incoming(
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

    // Receive Hello (or a PairingRequest). Pre-auth cap: the peer's
    // identity is verified only AFTER this read returns, so cap the
    // declared frame length tight to keep a hostile dialer from
    // forcing a 16 MiB allocation per connection.
    let msg = transport::recv_message_capped(
        &mut recv,
        transport::MAX_PREAUTH_MESSAGE_BYTES,
    )
    .await?;
    let (peer_id, peer_auth_sig) = match msg {
        SyncMessage::Hello {
            device_id,
            protocol_version,
            auth_signature,
        } => {
            if protocol_version != PROTOCOL_VERSION {
                // Surface to the UI so the user knows which side
                // needs to update. Without this the connection
                // just drops with no actionable signal.
                let _ = event_tx.send(SyncEvent::VersionMismatch {
                    peer_id: device_id,
                    peer_version: protocol_version,
                    local_version: PROTOCOL_VERSION,
                });
                return Err(SyncError::Protocol(format!(
                    "Version mismatch: peer v{protocol_version}, local v{PROTOCOL_VERSION}"
                )));
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
            let mut transport = transport::SessionTransport::Quic { send, recv };
            return pairing::handle_pairing_request(
                &mut transport,
                &vault,
                &identity,
                &hosting_pairing,
                &event_tx,
                device_id,
                device_name,
                public_key,
                pairing_code,
                Some((peer_addr, listen_port)),
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

    let mut transport = transport::SessionTransport::Quic { send, recv };
    match handle_sync_session(&mut transport, &vault, &peer_id, None).await {
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
pub(super) async fn handle_sync_session(
    transport: &mut transport::SessionTransport,
    vault: &Arc<std::sync::Mutex<VaultStore>>,
    peer_id: &Uuid,
    // `pending` is the relay listener's way of replaying the first
    // demuxed frame: the listener peeks the type to know whether to
    // route to pairing or sync, and any other frame goes straight
    // back here as the first iteration's input.
    mut pending: Option<SyncMessage>,
) -> Result<(usize, usize), SyncError> {
    let mut pushed = 0;
    let mut pulled = 0;

    // Per-peer E2E key. Fetched once at session start. Pre-v4 peers
    // (no `shared_secret` row) talk in plaintext; v4 peers always
    // carry one because pairing seeds it.
    let shared_secret = peer_shared_secret(vault, peer_id)?;
    let shared_secret = shared_secret.as_ref();

    loop {
        let msg = match pending.take() {
            Some(m) => m,
            None => transport.recv().await?,
        };
        match msg {
            SyncMessage::ManifestRequest => {
                let manifest = build_manifest(vault)?;
                transport.send(&SyncMessage::Manifest { entries: manifest }).await?;
            }
            SyncMessage::DeltaRequest { needed } => {
                // Peer wants these records from us
                let records = collect_records(vault, &needed, shared_secret)?;
                pushed += records.len();
                transport.send(&SyncMessage::DeltaResponse { records }).await?;
            }
            SyncMessage::DeltaPush { records } => {
                // Peer is pushing records to us
                let count = records.len();
                apply_records(vault, &records, shared_secret)?;
                pulled += count;
                let accepted: Vec<Uuid> = records.iter().map(|r| r.entity_id).collect();
                transport.send(&SyncMessage::DeltaAck { accepted }).await?;
            }
            SyncMessage::Ping => {
                transport.send(&SyncMessage::Pong).await?;
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


/// Sync with all active paired peers. Peers run concurrently: an
/// offline peer burns its own 5s QUIC timeout (plus relay fallback)
/// without stalling the peers behind it, where the previous serial
/// loop made N offline peers cost N x the per-peer budget per tick.
pub(super) async fn sync_all_peers(
    vault: &Arc<std::sync::Mutex<VaultStore>>,
    identity: &DeviceIdentity,
    config: &SyncConfig,
    event_tx: &mpsc::UnboundedSender<SyncEvent>,
) -> Result<(), SyncError> {
    let peers = {
        let v = vault.lock().map_err(|_| SyncError::Vault("Lock".into()))?;
        v.list_sync_peers()?
    };

    // Each task owns its clones (the vault is an Arc, the rest are
    // cheap Clone types) and reports through events, so a failing
    // peer is isolated from the others by construction: nothing here
    // short-circuits the set.
    let mut tasks = tokio::task::JoinSet::new();
    for peer in peers.into_iter().filter(|p| p.is_active) {
        let vault = vault.clone();
        let identity = identity.clone();
        let config = config.clone();
        let event_tx = event_tx.clone();
        tasks.spawn(async move {
            sync_one_peer(&vault, &identity, &config, &event_tx, &peer).await;
        });
    }
    // Drain every task. A panicked task is logged and skipped; it
    // must not cancel the remaining peers.
    while let Some(joined) = tasks.join_next().await {
        if let Err(e) = joined {
            tracing::warn!("sync: peer sync task panicked: {e}");
        }
    }

    Ok(())
}

/// Budget for the tier-2 relay fallback. The relay long-poll retries
/// forever on empty responses, so without a cap an unresponsive peer
/// would park this task (and the whole sync tick awaiting it) for
/// good. A full session is a handful of HTTP round-trips; 60s is
/// generous even on slow links.
const RELAY_SYNC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// One peer's full sync attempt: QUIC tier with a 5s cap, relay
/// fallback tier, vault stamp + UI events on the way out. Never
/// returns an error; every failure path lands in `SyncFailed`.
async fn sync_one_peer(
    vault: &Arc<std::sync::Mutex<VaultStore>>,
    identity: &DeviceIdentity,
    config: &SyncConfig,
    event_tx: &mpsc::UnboundedSender<SyncEvent>,
    peer: &oryxis_vault::SyncPeerRow,
) {
    let _ = event_tx.send(SyncEvent::SyncStarted { peer_id: peer.peer_id });

    // Tier 1: direct QUIC to the last known endpoint. Skipped
    // for peers paired via relay (no endpoint recorded) or peers
    // whose endpoint is the `0.0.0.0` sentinel.
    let mut last_err: Option<SyncError> = None;
    let quic_result = match (&peer.last_known_ip, peer.last_known_port) {
        (Some(ip), Some(port)) if !ip.is_empty() && ip != "0.0.0.0" => {
            match format!("{ip}:{port}").parse::<SocketAddr>() {
                Ok(addr) => Some(
                    tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        sync_with_peer(vault, identity, &peer.peer_id, addr, event_tx),
                    )
                    .await,
                ),
                Err(e) => {
                    last_err = Some(SyncError::Transport(format!(
                        "Parse addr: {e}"
                    )));
                    None
                }
            }
        }
        _ => None,
    };

    let mut sync_outcome: Option<(usize, usize)> = None;
    match quic_result {
        Some(Ok(Ok(r))) => sync_outcome = Some(r),
        Some(Ok(Err(e))) => {
            tracing::debug!(
                "sync: QUIC to {} failed, trying relay: {e}",
                peer.peer_id
            );
            last_err = Some(e);
        }
        Some(Err(_)) => {
            tracing::debug!(
                "sync: QUIC to {} timed out after 5s, trying relay",
                peer.peer_id
            );
            last_err = Some(SyncError::Timeout);
        }
        None => {}
    }

    // Tier 2: relay fallback. Engaged when QUIC failed (or never
    // ran because the peer has no direct endpoint) AND a relay
    // URL is configured.
    if sync_outcome.is_none() {
        if let Some(relay_url) = &config.signaling_url {
            let token = config.signaling_token.clone().unwrap_or_default();
            match tokio::time::timeout(
                RELAY_SYNC_TIMEOUT,
                sync_with_peer_via_relay(
                    vault,
                    identity,
                    relay_url,
                    &token,
                    &peer.peer_id,
                    event_tx,
                ),
            )
            .await
            {
                Ok(Ok(r)) => sync_outcome = Some(r),
                Ok(Err(e)) => last_err = Some(e),
                Err(_) => last_err = Some(SyncError::Timeout),
            }
        }
    }

    match sync_outcome {
        Some((pushed, pulled)) => {
            if let Ok(v) = vault.lock() {
                let _ = v.update_sync_peer_last_synced(&peer.peer_id);
            }
            let _ = event_tx.send(SyncEvent::SyncCompleted {
                peer_id: peer.peer_id,
                pushed,
                pulled,
            });
        }
        None => {
            let error = last_err
                .map(|e| e.to_string())
                .unwrap_or_else(|| "No reachable transport".into());
            let _ = event_tx.send(SyncEvent::SyncFailed {
                peer_id: peer.peer_id,
                error,
            });
        }
    }
}

/// Client side of a relay-based sync session: bind a `RelayClient`
/// long-poll inbox to the peer, run a three-step Ed25519 handshake
/// (`RelayHello` / `RelayHelloAck` / `RelayAuth`) over the relay so the
/// peer cannot be impersonated by anyone who happens to know its
/// `device_id`, then run the same `run_sync_session_as_client` flow
/// that QUIC uses. Per-record AEAD still protects payload contents
/// against a relay-server eavesdropper; the handshake closes the
/// integrity gap (empty-payload tombstones used to bypass AEAD).
async fn sync_with_peer_via_relay(
    vault: &Arc<std::sync::Mutex<VaultStore>>,
    identity: &DeviceIdentity,
    relay_url: &str,
    relay_token: &str,
    peer_id: &Uuid,
    event_tx: &mpsc::UnboundedSender<SyncEvent>,
) -> Result<(usize, usize), SyncError> {
    let client = crate::relay::RelayClient::new(
        relay_url,
        relay_token,
        identity.device_id,
    );
    let mut transport = transport::SessionTransport::RelayClient {
        client,
        peer_id: *peer_id,
        my_id: identity.device_id,
    };

    let client_nonce = crypto::random_relay_nonce();
    transport
        .send(&SyncMessage::RelayHello {
            device_id: identity.device_id,
            protocol_version: PROTOCOL_VERSION,
            client_nonce,
        })
        .await?;

    let ack = transport.recv().await?;
    let SyncMessage::RelayHelloAck {
        device_id: ack_device_id,
        protocol_version,
        server_nonce,
        server_signature,
    } = ack
    else {
        return Err(SyncError::Protocol("Expected RelayHelloAck".into()));
    };
    if ack_device_id != *peer_id {
        return Err(SyncError::Protocol(format!(
            "RelayHelloAck device_id mismatch: expected {peer_id}, got {ack_device_id}"
        )));
    }
    if protocol_version != PROTOCOL_VERSION {
        let _ = event_tx.send(SyncEvent::VersionMismatch {
            peer_id: *peer_id,
            peer_version: protocol_version,
            local_version: PROTOCOL_VERSION,
        });
        return Err(SyncError::Protocol(format!(
            "RelayHelloAck version mismatch: peer v{protocol_version}, local v{PROTOCOL_VERSION}"
        )));
    }
    let peer_pubkey = {
        let v = vault
            .lock()
            .map_err(|_| SyncError::Vault("Lock failed".into()))?;
        v.list_sync_peers()?
            .into_iter()
            .find(|p| p.peer_id == *peer_id && p.is_active)
            .map(|p| p.public_key)
            .ok_or_else(|| SyncError::PeerNotFound(peer_id.to_string()))?
    };
    let transcript = crypto::relay_handshake_transcript(
        &identity.device_id,
        peer_id,
        &client_nonce,
        &server_nonce,
    );
    crypto::verify_relay_handshake(&peer_pubkey, &transcript, &server_signature)
        .map_err(|e| {
            SyncError::PairingFailed(format!(
                "RelayHelloAck signature from {peer_id} did not verify: {e}"
            ))
        })?;
    let client_sig =
        crypto::sign_relay_handshake(&identity.signing_key, &transcript);
    transport
        .send(&SyncMessage::RelayAuth {
            client_signature: client_sig.to_vec(),
        })
        .await?;

    run_sync_session_as_client(&mut transport, vault, peer_id).await
}

/// Sync with a specific peer (client side, initiates connection).
async fn sync_with_peer(
    vault: &Arc<std::sync::Mutex<VaultStore>>,
    identity: &DeviceIdentity,
    peer_id: &Uuid,
    addr: SocketAddr,
    event_tx: &mpsc::UnboundedSender<SyncEvent>,
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
    // Pre-auth cap: the verification step below is the first time
    // we know whether this is really our paired peer talking, so
    // gate the raw allocation accordingly.
    let msg = transport::recv_message_capped(
        &mut recv,
        transport::MAX_PREAUTH_MESSAGE_BYTES,
    )
    .await?;
    let peer_auth_sig = match msg {
        SyncMessage::HelloAck {
            device_id,
            protocol_version,
            auth_signature,
        } => {
            if device_id != *peer_id {
                return Err(SyncError::Protocol("Peer ID mismatch".into()));
            }
            // Server-side check (handle_incoming) covers Hello.version,
            // but the server may legitimately speak a newer protocol
            // than what its HelloAck reports if backward-compat was
            // added. Today we require an exact match either way, so a
            // mismatch here also surfaces a UI event before failing.
            if protocol_version != PROTOCOL_VERSION {
                let _ = event_tx.send(SyncEvent::VersionMismatch {
                    peer_id: *peer_id,
                    peer_version: protocol_version,
                    local_version: PROTOCOL_VERSION,
                });
                return Err(SyncError::Protocol(format!(
                    "HelloAck version mismatch: peer v{protocol_version}, local v{PROTOCOL_VERSION}"
                )));
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

    // Hand off to the transport-agnostic client-side flow so the same
    // code path runs for both QUIC and relay (the relay flow skips
    // the Hello+exporter dance entirely since it has no channel to
    // bind to).
    let mut transport = transport::SessionTransport::Quic { send, recv };
    run_sync_session_as_client(&mut transport, vault, peer_id).await
}

/// Client side of a sync session: ManifestRequest, diff via LWW,
/// pull/push deltas, Bye. Same flow regardless of underlying transport.
async fn run_sync_session_as_client(
    transport: &mut transport::SessionTransport,
    vault: &Arc<std::sync::Mutex<VaultStore>>,
    peer_id: &Uuid,
) -> Result<(usize, usize), SyncError> {
    // Per-peer E2E key, same as on the server side.
    let shared_secret = peer_shared_secret(vault, peer_id)?;
    let shared_secret = shared_secret.as_ref();

    // Request manifest
    transport.send(&SyncMessage::ManifestRequest).await?;
    let remote_manifest = match transport.recv().await? {
        SyncMessage::Manifest { entries } => entries,
        _ => return Err(SyncError::Protocol("Expected Manifest".into())),
    };

    // Build local manifest
    let local_manifest = build_manifest(vault)?;

    // Compare manifests using LWW. Index both sides by (type, id)
    // first so the diff is O(local + remote) instead of the previous
    // O(local x remote) nested scan.
    let local_by_key: std::collections::HashMap<_, _> = local_manifest
        .iter()
        .map(|l| ((l.entity_type, l.entity_id), l))
        .collect();
    let remote_keys: std::collections::HashSet<_> = remote_manifest
        .iter()
        .map(|r| (r.entity_type, r.entity_id))
        .collect();

    let mut needed_from_remote = Vec::new();
    let mut to_push_to_remote = Vec::new();

    for remote_entry in &remote_manifest {
        if let Some(local_entry) = local_by_key
            .get(&(remote_entry.entity_type, remote_entry.entity_id))
            .copied()
        {
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
        if !remote_keys.contains(&(local_entry.entity_type, local_entry.entity_id)) {
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
        transport.send(&SyncMessage::DeltaRequest {
            needed: needed_from_remote,
        }).await?;
        match transport.recv().await? {
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
        transport.send(&SyncMessage::DeltaPush { records }).await?;
        match transport.recv().await? {
            SyncMessage::DeltaAck { .. } => {}
            _ => return Err(SyncError::Protocol("Expected DeltaAck".into())),
        }
    }

    // Done
    transport.send(&SyncMessage::Bye).await?;

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
