//! Server-side dispatcher for relay-inbound sync sessions. The relay
//! transport has no TLS channel binding, so this module also implements
//! the v5 Ed25519 handshake (`RelayHello` / `RelayHelloAck` /
//! `RelayAuth`) before falling through to the regular sync session
//! handler in `mod.rs::handle_sync_session`.

use std::sync::Arc;

use tokio::sync::mpsc;
use uuid::Uuid;

use oryxis_vault::VaultStore;

use crate::crypto::{self, DeviceIdentity};
use crate::protocol::{SyncMessage, PROTOCOL_VERSION};
use crate::transport;

use super::session::handle_sync_session;
use super::{pairing, HostingPairing, SyncEvent};

/// Server-side dispatcher for a relay-inbound session. The listener
/// in `start()` has already routed the very first frame from this
/// sender into our mpsc; we peek that frame to decide whether the
/// other side is opening a pairing or a sync session, then call the
/// matching handler with a `RelayServer` transport that consumes the
/// rest of the frames from the same mpsc.
pub(super) async fn run_relay_inbound_session(
    client: crate::relay::RelayClient,
    sender_id: Uuid,
    rx: tokio::sync::mpsc::UnboundedReceiver<SyncMessage>,
    vault: Arc<std::sync::Mutex<VaultStore>>,
    identity: DeviceIdentity,
    hosting_pairing: Arc<std::sync::Mutex<Option<HostingPairing>>>,
    event_tx: mpsc::UnboundedSender<SyncEvent>,
) {
    let mut transport = transport::SessionTransport::RelayServer {
        client,
        peer_id: sender_id,
        inbox: rx,
    };
    let first = match transport.recv().await {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("relay session from {sender_id}: {e}");
            return;
        }
    };
    match first {
        SyncMessage::PairingRequest {
            device_id,
            device_name,
            public_key,
            pairing_code,
            x25519_pub,
            ..
        } => {
            let _ = pairing::handle_pairing_request(
                &mut transport,
                &vault,
                &identity,
                &hosting_pairing,
                &event_tx,
                device_id,
                device_name,
                public_key,
                pairing_code,
                None,
                x25519_pub,
            )
            .await;
        }
        SyncMessage::RelayHello {
            device_id,
            protocol_version,
            client_nonce,
        } => {
            // Authenticated relay session. Pre-v5 the sender_id check
            // above was the only gate, so an attacker who knew a
            // paired device's UUID could push forged tombstones
            // (empty payload skips AEAD). v5 makes the client prove
            // it holds the paired Ed25519 private key over a fresh
            // nonce pair before any sync frames are processed.
            if device_id != sender_id {
                tracing::warn!(
                    "relay session: RelayHello device_id {device_id} != relay sender_id {sender_id}"
                );
                return;
            }
            if protocol_version != PROTOCOL_VERSION {
                let _ = event_tx.send(SyncEvent::VersionMismatch {
                    peer_id: sender_id,
                    peer_version: protocol_version,
                    local_version: PROTOCOL_VERSION,
                });
                return;
            }
            let peer_pubkey = {
                let Ok(v) = vault.lock() else { return };
                let Ok(peers) = v.list_sync_peers() else { return };
                peers
                    .into_iter()
                    .find(|p| p.peer_id == sender_id && p.is_active)
                    .map(|p| p.public_key)
            };
            let Some(peer_pubkey) = peer_pubkey else {
                tracing::warn!(
                    "relay session: rejecting unknown sender {sender_id}"
                );
                return;
            };
            let server_nonce = crypto::random_relay_nonce();
            let transcript = crypto::relay_handshake_transcript(
                &sender_id,
                &identity.device_id,
                &client_nonce,
                &server_nonce,
            );
            let server_sig =
                crypto::sign_relay_handshake(&identity.signing_key, &transcript);
            let ack = SyncMessage::RelayHelloAck {
                device_id: identity.device_id,
                protocol_version: PROTOCOL_VERSION,
                server_nonce,
                server_signature: server_sig.to_vec(),
            };
            if let Err(e) = transport.send(&ack).await {
                tracing::warn!(
                    "relay session: send RelayHelloAck to {sender_id} failed: {e}"
                );
                return;
            }
            let auth_msg = match transport.recv().await {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!(
                        "relay session: recv RelayAuth from {sender_id} failed: {e}"
                    );
                    return;
                }
            };
            let SyncMessage::RelayAuth { client_signature } = auth_msg else {
                tracing::warn!(
                    "relay session: expected RelayAuth from {sender_id}, got {auth_msg:?}"
                );
                return;
            };
            if let Err(e) = crypto::verify_relay_handshake(
                &peer_pubkey,
                &transcript,
                &client_signature,
            ) {
                tracing::warn!(
                    "relay session: RelayAuth signature from {sender_id} did not verify: {e}"
                );
                return;
            }
            let _ = event_tx.send(SyncEvent::SyncStarted { peer_id: sender_id });
            match handle_sync_session(&mut transport, &vault, &sender_id, None).await {
                Ok((pushed, pulled)) => {
                    let _ = event_tx.send(SyncEvent::SyncCompleted {
                        peer_id: sender_id,
                        pushed,
                        pulled,
                    });
                }
                Err(e) => {
                    let _ = event_tx.send(SyncEvent::SyncFailed {
                        peer_id: sender_id,
                        error: e.to_string(),
                    });
                }
            }
        }
        other => {
            // Pre-v5 the bare ManifestRequest / DeltaRequest / Ping
            // were accepted here on the basis of the relay sender_id
            // alone. They are now rejected so a stranger that knows
            // a paired device's UUID cannot skip the RelayHello
            // round and slip frames straight into the sync session.
            tracing::warn!(
                "relay session from {sender_id}: unexpected opener {other:?} (RelayHello required)"
            );
        }
    }
}
