//! Pairing-flow helpers extracted from `engine/mod.rs` to keep the
//! file under a sane size.
//!
//! The pairing flow is fundamentally orthogonal to the regular sync
//! session: it runs once per device pair, gates on a 6-digit
//! single-shot code (rather than a paired Ed25519 key), and bootstraps
//! the per-peer X25519 shared secret that every subsequent
//! `SyncRecord.payload` is sealed with. The functions here are the
//! transport-agnostic primitives (`run_pairing_as_joiner`,
//! `handle_pairing_request`, `reject_pairing`); the SyncHandle and
//! relay glue in sibling modules drive them.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::mpsc;
use uuid::Uuid;

use oryxis_vault::VaultStore;

use crate::crypto::{self, DeviceIdentity};
use crate::error::SyncError;
use crate::protocol::SyncMessage;
use crate::transport;

use super::{HostingPairing, MAX_PAIRING_ATTEMPTS, MAX_PAIRING_SOURCES, SyncEvent};

/// URL scheme + path prefix for shareable pairing links.
pub(super) const PAIRING_LINK_PREFIX: &str = "oryxis://pair/";

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

/// Joiner side of the pairing flow. Runs over either a QUIC bi-stream
/// or a relay session; the transport abstraction in
/// [`crate::transport::SessionTransport`] hides the difference.
pub(super) async fn run_pairing_as_joiner(
    transport: &mut transport::SessionTransport,
    identity: &DeviceIdentity,
    vault: &Arc<std::sync::Mutex<VaultStore>>,
    listen_port: u16,
    code: &str,
    peer_endpoint: Option<(std::net::IpAddr, u16)>,
) -> Result<(Uuid, String), SyncError> {
    // Fresh X25519 keypair for the pairing-time DH. We hold the secret
    // across the recv-PairingAccepted await, then consume it in
    // `x25519_dh` so the ephemeral private key is forgotten.
    let (joiner_x25519_secret, joiner_x25519_pub) = crypto::x25519_keypair();

    transport.send(&SyncMessage::PairingRequest {
        device_id: identity.device_id,
        device_name: identity.device_name.clone(),
        public_key: identity.public_key_bytes(),
        pairing_code: code.to_string(),
        listen_port,
        x25519_pub: joiner_x25519_pub.to_vec(),
    }).await?;

    // Host -> PairingChallenge (or PairingRejected).
    let challenge = match transport.recv().await? {
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
    let signed = crypto::sign_ed25519_32(&identity.signing_key, &challenge);
    transport.send(&SyncMessage::PairingResponse {
        signed_challenge: signed.to_vec(),
    }).await?;

    // Host -> PairingAccepted (or PairingRejected).
    let (device_id, device_name, public_key, host_x25519_pub) =
        match transport.recv().await? {
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

    // Both sides DH to the same 32-byte secret; store it on the peer
    // row so every later `SyncRecord.payload` between us and this host
    // is sealed with ChaCha20-Poly1305.
    let shared_secret = crypto::x25519_dh(joiner_x25519_secret, &host_x25519_pub);

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
        if let Some((ip, port)) = peer_endpoint {
            v.update_sync_peer_endpoint(&device_id, &ip.to_string(), port)?;
        }
    }

    // Delivery barrier: tells the host we read PairingAccepted so the
    // host can drop its connection without losing the buffered final
    // frame. Same trick the sync session uses.
    let _ = transport.send(&SyncMessage::Bye).await;
    Ok((device_id, device_name))
}

/// Host side of the pairing flow. Validates the joiner's code +
/// signature, persists the new peer row, and seeds the shared secret.
#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_pairing_request(
    transport: &mut transport::SessionTransport,
    vault: &Arc<std::sync::Mutex<VaultStore>>,
    identity: &DeviceIdentity,
    hosting_pairing: &Arc<std::sync::Mutex<Option<HostingPairing>>>,
    event_tx: &mpsc::UnboundedSender<SyncEvent>,
    device_id: Uuid,
    device_name: String,
    public_key: Vec<u8>,
    pairing_code: String,
    // QUIC paths know the joiner's source address from the connection;
    // relay paths don't (the joiner is behind NAT we can't reach), so
    // pass `None` and we record `0.0.0.0` to flag "relay-only peer".
    peer_endpoint: Option<(SocketAddr, u16)>,
    joiner_x25519_pub: Vec<u8>,
) -> Result<(), SyncError> {
    // Per-source attempt key. QUIC: IP; relay: joiner device_id.
    let source = match peer_endpoint {
        Some((addr, _)) => source_key_for_quic(&addr),
        None => source_key_for_relay(&device_id),
    };
    // Refuse outright if this source is already over the cap from
    // earlier attempts on the same hosted code. Read the count into
    // a local + drop the lock guard BEFORE awaiting so the future
    // stays `Send` (MutexGuard is not Send across an await).
    let over_cap = {
        let state = hosting_pairing
            .lock()
            .map_err(|_| SyncError::Vault("Lock".into()))?;
        state
            .as_ref()
            .map(|s| s.attempts_by_source.get(&source).copied().unwrap_or(0))
            .unwrap_or(0)
            >= MAX_PAIRING_ATTEMPTS
    };
    if over_cap {
        return reject_pairing(transport, "Rate limited").await;
    }

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
        return reject_pairing(transport, "Not hosting pairing (or code expired)").await;
    };
    if !crypto::constant_time_eq(expected.as_bytes(), pairing_code.as_bytes()) {
        // Wrong code: count it against this source only. The hosted
        // code stays alive for legitimate peers paired from a
        // different network even if an attacker is grinding the
        // 6-digit space from one IP.
        let over = record_pairing_failure(hosting_pairing, &source);
        let reason = if over { "Rate limited" } else { "Wrong pairing code" };
        return reject_pairing(transport, reason).await;
    }

    // Code matches. Challenge the joiner with a fresh nonce so an
    // intercepted `PairingRequest` can't be replayed.
    let challenge = crypto::random_challenge();
    transport.send(&SyncMessage::PairingChallenge {
        challenge: challenge.to_vec(),
    }).await?;

    let signed = match transport.recv().await? {
        SyncMessage::PairingResponse { signed_challenge } => signed_challenge,
        _ => return Err(SyncError::Protocol("Expected PairingResponse".into())),
    };
    if crypto::verify_ed25519_32(&public_key, &challenge, &signed).is_err() {
        // Bad challenge response: also counted against this source.
        // An attacker who happens to know the code is still gated
        // on holding the matching private key, but a brute-force of
        // the signed challenge would be much easier to reason about
        // under the same attempt cap.
        let over = record_pairing_failure(hosting_pairing, &source);
        let reason = if over { "Rate limited" } else { "Bad challenge response" };
        return reject_pairing(transport, reason).await;
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

    // Verified. Persist the joiner as a peer and clear the single-shot
    // code. Endpoint recording differs by transport: QUIC has the
    // joiner's source IP + advertised listen port; relay has neither
    // (we'll sync via relay forever for this peer).
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
        if let Some((addr, listen_port)) = peer_endpoint {
            v.update_sync_peer_endpoint(&device_id, &addr.ip().to_string(), listen_port)?;
        }
    }
    if let Ok(mut state) = hosting_pairing.lock() {
        *state = None;
    }

    transport.send(&SyncMessage::PairingAccepted {
        device_id: identity.device_id,
        device_name: identity.device_name.clone(),
        public_key: identity.public_key_bytes(),
        x25519_pub: host_x25519_pub.to_vec(),
    }).await?;
    // Delivery barrier: wait for the joiner's `Bye` so we don't drop
    // the connection (and the still-buffered `PairingAccepted` frame)
    // before the joiner has read it. On relay the barrier just times
    // out and we move on (next-frame poll is fine).
    let _ = transport.recv().await;

    let _ = event_tx.send(SyncEvent::PairingCompleted {
        device_id,
        device_name,
    });
    Ok(())
}

/// Build a stable source key for the per-source attempt counter.
/// QUIC paths surface the joiner's IP (we trust the QUIC source
/// address because the handshake completed against it); relay paths
/// use the joiner's device_id (the `X-Sender-Id` header on the
/// inbox POST). Keeping the two namespaces distinct prevents an
/// attacker on relay from poisoning the QUIC bucket for a
/// legitimate peer that happens to share device_id with someone
/// else's IP, or vice-versa.
pub(super) fn source_key_for_quic(addr: &SocketAddr) -> String {
    format!("quic:{}", addr.ip())
}
pub(super) fn source_key_for_relay(joiner_device_id: &Uuid) -> String {
    format!("relay:{joiner_device_id}")
}

/// Increment the attempt counter for `source` on the live hosting
/// state. Returns `true` when the source is now over the cap (caller
/// should reject all further attempts from it with rate-limit text,
/// not "wrong code"). Silently no-ops when the state is already
/// cleared (expired between the lookup and now); the lock is
/// short-lived so concurrent attempts under-count by at most one
/// before the cap kicks in.
///
/// CONTRAST with the old "cap on the host" model: there a single
/// noisy attacker invalidated the whole code, hurting the legit
/// user. Per-source caps only deny service to the attacker.
fn record_pairing_failure(
    hosting_pairing: &Arc<std::sync::Mutex<Option<HostingPairing>>>,
    source: &str,
) -> bool {
    let Ok(mut state) = hosting_pairing.lock() else { return false };
    let Some(s) = state.as_mut() else { return false };
    // Cap on total tracked sources to keep the map bounded under a
    // sender_id flood. At the cap, drop everyone (the legit user is
    // safe because they re-enter the code from scratch; the loss is
    // a tiny bit of attacker state).
    if s.attempts_by_source.len() >= MAX_PAIRING_SOURCES {
        s.attempts_by_source.clear();
        tracing::warn!(
            "pairing: cleared attempt map (hit {MAX_PAIRING_SOURCES} distinct sources)"
        );
    }
    let counter = s.attempts_by_source.entry(source.to_string()).or_insert(0);
    *counter = counter.saturating_add(1);
    let over = *counter >= MAX_PAIRING_ATTEMPTS;
    if over {
        tracing::warn!(
            attempts = counter,
            source = %source,
            "pairing: source over the {MAX_PAIRING_ATTEMPTS}-attempt cap"
        );
    }
    over
}

/// Send a `PairingRejected` and hold the connection open until the
/// joiner has read it: without the barrier `recv`, returning here
/// would drop the connection before the still-buffered rejection
/// frame is delivered, and the joiner would see a bare "connection
/// lost" instead of the reason.
pub(super) async fn reject_pairing(
    transport: &mut transport::SessionTransport,
    reason: &str,
) -> Result<(), SyncError> {
    transport.send(&SyncMessage::PairingRejected {
        reason: reason.to_string(),
    }).await?;
    let _ = transport.recv().await;
    Ok(())
}
