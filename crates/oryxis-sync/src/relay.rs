//! HTTP relay transport. Counterpart to the `signaling-worker`'s
//! `/relay/:recipient_id/inbox` endpoints (and the standalone
//! `oryxis-relay` binary, which serves the same API). Used as
//! transport fallback when QUIC direct can't punch through NAT.
//!
//! Wire format is identical to the QUIC streams: bincode-encoded
//! `SyncMessage` frames, sealed at the application layer by the
//! pairing-derived ChaCha20-Poly1305 key (see `engine::collect_records`
//! / `apply_records`). The relay sees opaque bytes only.
//!
//! Session model: one in-flight call to `recv` per `RelayClient`.
//! Engine sessions are sequential (one peer at a time), so demux
//! across concurrent peers is unnecessary in v1; the caller verifies
//! the returned `sender_id` matches the expected peer.

use uuid::Uuid;

use crate::error::SyncError;
use crate::protocol::SyncMessage;

/// Default long-poll window used by `recv` between empty responses.
/// Picked to match the worker's `MAX_WAIT_MS`; bigger doesn't help.
const RECV_WAIT_MS: u32 = 30_000;

/// Soft cap on a single relayed frame, matches the worker's
/// `MAX_FRAME_BYTES` (256 KiB). Engine-side payloads stay well below.
const MAX_FRAME_BYTES: usize = 256 * 1024;

/// HTTP relay client. Cheap to clone (the inner `reqwest::Client` is
/// `Arc`-wrapped).
#[derive(Clone)]
pub struct RelayClient {
    base_url: String,
    token: String,
    sender_id: Uuid,
    http: reqwest::Client,
}

impl RelayClient {
    /// Build a new client pointed at a deployed relay (Cloudflare
    /// Worker or `oryxis-relay` binary). `sender_id` is this device's
    /// UUID, sent in `X-Sender-Id` so the peer can demux multi-source
    /// inboxes and reply to the right address.
    pub fn new(base_url: &str, token: &str, sender_id: Uuid) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            token: token.to_string(),
            sender_id,
            http: reqwest::Client::new(),
        }
    }

    /// Enqueue `msg` for `recipient`. Returns once the relay has
    /// accepted the frame (HTTP 204). The recipient's `recv` long-poll
    /// picks it up on the next iteration.
    pub async fn send(
        &self,
        recipient: Uuid,
        msg: &SyncMessage,
    ) -> Result<(), SyncError> {
        let frame = crate::protocol::encode_message(msg)
            .map_err(|e| SyncError::Protocol(format!("relay encode: {e}")))?;
        // `encode_message` prepends a 4-byte length header for QUIC
        // streams; the relay carries one frame per HTTP request, so we
        // strip it before posting.
        let frame_body = if frame.len() >= 4 {
            frame[4..].to_vec()
        } else {
            return Err(SyncError::Protocol("relay encode: short frame".into()));
        };
        if frame_body.len() > MAX_FRAME_BYTES {
            return Err(SyncError::Protocol(format!(
                "relay frame too large: {} bytes (max {MAX_FRAME_BYTES})",
                frame_body.len()
            )));
        }
        let url = format!("{}/relay/{recipient}/inbox", self.base_url);
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.token)
            .header("X-Sender-Id", self.sender_id.to_string())
            .header("Content-Type", "application/octet-stream")
            .body(frame_body)
            .send()
            .await
            .map_err(|e| SyncError::Transport(format!("relay POST: {e}")))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(SyncError::Transport(format!(
                "relay POST {url} -> {status}: {body}"
            )));
        }
        Ok(())
    }

    /// Block via long-poll until a frame addressed to `my_id` arrives.
    /// Returns the originating sender + decoded `SyncMessage`. Loops
    /// internally on HTTP 204 (no message landed within the wait
    /// window), so the caller treats this like a `quinn` stream read:
    /// it either returns a message or an error.
    pub async fn recv(&self, my_id: Uuid) -> Result<(Uuid, SyncMessage), SyncError> {
        loop {
            if let Some(pair) = self.poll_once(my_id, RECV_WAIT_MS).await? {
                return Ok(pair);
            }
        }
    }

    /// One iteration of the long-poll. `None` means "no message within
    /// `wait_ms`, try again". `Err` propagates transport / decode
    /// failures so the caller can give up if needed.
    async fn poll_once(
        &self,
        my_id: Uuid,
        wait_ms: u32,
    ) -> Result<Option<(Uuid, SyncMessage)>, SyncError> {
        let url = format!(
            "{}/relay/{my_id}/inbox?wait_ms={wait_ms}",
            self.base_url
        );
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|e| SyncError::Transport(format!("relay GET: {e}")))?;
        let status = resp.status();
        if status.as_u16() == 204 {
            return Ok(None);
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            // 404 / 410 / 501 mean the relay can't service this request
            // even if we retry: wrong path, deprecated worker, recipient
            // slot deleted server-side. Surface as a distinct error so
            // the engine stops polling instead of looping every 2 s.
            // Anything else (5xx, 429, network blips) stays Transport
            // and the engine keeps retrying.
            let permanent = matches!(status.as_u16(), 404 | 410 | 501);
            let msg = format!("relay GET {url} -> {status}: {body}");
            return Err(if permanent {
                SyncError::RelayUnavailable(msg)
            } else {
                SyncError::Transport(msg)
            });
        }
        let sender_id: Uuid = resp
            .headers()
            .get("X-Sender-Id")
            .and_then(|h| h.to_str().ok())
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or_else(|| SyncError::Protocol("relay GET: missing sender id".into()))?;
        let body = resp
            .bytes()
            .await
            .map_err(|e| SyncError::Transport(format!("relay GET body: {e}")))?;
        let msg = crate::protocol::decode_message(&body)
            .map_err(|e| SyncError::Protocol(format!("relay decode: {e}")))?;
        Ok(Some((sender_id, msg)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `send` rejects oversized frames before going to the network.
    /// We can't easily test the success path here without a live relay
    /// server; the round-trip is covered at the integration level when
    /// Phase D3/D4 land.
    #[tokio::test]
    async fn relay_rejects_oversized_frame() {
        let client = RelayClient::new(
            "https://example.invalid",
            "tok",
            Uuid::new_v4(),
        );
        // Build a `Snippet` payload large enough to push the encoded
        // frame past MAX_FRAME_BYTES. The Snippet model has free-form
        // `command` so we stuff it.
        let snippet = oryxis_core::models::snippet::Snippet::new(
            "big",
            "x".repeat(MAX_FRAME_BYTES + 1024),
        );
        let payload = serde_json::to_vec(&snippet).unwrap();
        let msg = SyncMessage::DeltaPush {
            records: vec![crate::protocol::SyncRecord {
                entity_type: crate::protocol::EntityType::Snippet,
                entity_id: snippet.id,
                updated_at: chrono::Utc::now(),
                is_deleted: false,
                payload,
            }],
        };
        let err = client.send(Uuid::new_v4(), &msg).await.unwrap_err();
        assert!(
            matches!(err, SyncError::Protocol(_)),
            "expected Protocol(too large), got {err:?}"
        );
    }
}
