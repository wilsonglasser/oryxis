use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::crypto::{
    DeviceIdentity, register_sign_payload, sign_register_payload, unregister_sign_payload,
};
use crate::error::SyncError;

/// Wire body sent to `POST /register`. The signature gates re-registers
/// (TOFU: the server stores `public_key` on first register and refuses
/// any later register that signs with a different key for the same
/// `device_id`). `signed_at` is unix epoch seconds and must land inside
/// the server's clock-skew window to be accepted, which kills replay.
#[derive(Debug, Serialize)]
struct RegisterRequest {
    device_id: String,
    public_key_fp: String,
    /// Raw 32-byte Ed25519 verifying key, hex-encoded (64 chars).
    public_key: String,
    ip: String,
    port: u16,
    /// Unix epoch seconds at which the client signed this body.
    signed_at: i64,
    /// 64-byte Ed25519 signature over the canonical bytes returned by
    /// `crypto::register_sign_payload`, hex-encoded (128 chars).
    signature: String,
}

#[derive(Debug, Deserialize)]
pub struct LookupResponse {
    pub device_id: String,
    pub ip: String,
    pub port: u16,
    #[allow(dead_code)]
    pub public_key_fp: String,
}

/// Client for the Cloudflare Workers signaling server.
pub struct SignalingClient {
    base_url: String,
    token: String,
    http: reqwest::Client,
}

impl SignalingClient {
    pub fn new(base_url: &str, token: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').into(),
            token: token.into(),
            http: reqwest::Client::new(),
        }
    }

    /// Register this device's current IP:port on the signaling server.
    /// Signs the body with the device's Ed25519 key so the server can
    /// pin the registration to this device (TOFU on the public key)
    /// and reject blackhole/poisoning attempts from any other bearer
    /// token holder.
    pub async fn register(
        &self,
        identity: &DeviceIdentity,
        ip: &str,
        port: u16,
    ) -> Result<(), SyncError> {
        let signed_at = chrono::Utc::now().timestamp();
        let payload = register_sign_payload(&identity.device_id, ip, port, signed_at);
        let signature = sign_register_payload(&identity.signing_key, &payload);
        let pubkey = identity.public_key().to_bytes();

        let req = RegisterRequest {
            device_id: identity.device_id.to_string(),
            public_key_fp: identity.public_key_fingerprint(),
            public_key: hex::encode(pubkey),
            ip: ip.into(),
            port,
            signed_at,
            signature: hex::encode(signature),
        };

        let resp = self
            .http
            .post(format!("{}/register", self.base_url))
            .bearer_auth(&self.token)
            .json(&req)
            .send()
            .await
            .map_err(|e| SyncError::Discovery(format!("Signaling register: {}", e)))?;

        if !resp.status().is_success() {
            return Err(SyncError::Discovery(format!(
                "Signaling register failed: {}",
                resp.status()
            )));
        }

        tracing::debug!("Signaling: registered {}:{}", ip, port);
        Ok(())
    }

    /// Remove our registration from the signaling server. Signed with
    /// the same Ed25519 key the server pinned at register time; an
    /// attacker holding only the bearer token cannot delete our entry.
    /// Headers carry the auth fields so the URL stays bookmark-able
    /// and the body stays empty.
    ///
    /// Called from the signaling task's shutdown arm so the entry
    /// doesn't linger for the full TTL after the user disables sync
    /// or quits. Failure is logged at debug level only; the worst
    /// case is a stale entry that catches up at the next 5 min TTL.
    pub async fn unregister(&self, identity: &DeviceIdentity) -> Result<(), SyncError> {
        let signed_at = chrono::Utc::now().timestamp();
        let payload = unregister_sign_payload(&identity.device_id, signed_at);
        let signature = sign_register_payload(&identity.signing_key, &payload);
        let pubkey = identity.public_key().to_bytes();

        let resp = self
            .http
            .delete(format!("{}/register/{}", self.base_url, identity.device_id))
            .bearer_auth(&self.token)
            .header("X-Pubkey", hex::encode(pubkey))
            .header("X-Signed-At", signed_at.to_string())
            .header("X-Signature", hex::encode(signature))
            .send()
            .await
            .map_err(|e| SyncError::Discovery(format!("Signaling unregister: {}", e)))?;

        if !resp.status().is_success() {
            return Err(SyncError::Discovery(format!(
                "Signaling unregister failed: {}",
                resp.status()
            )));
        }

        Ok(())
    }

    /// Look up a peer's current IP:port on the signaling server.
    pub async fn lookup(&self, peer_id: &Uuid) -> Result<LookupResponse, SyncError> {
        let resp = self
            .http
            .get(format!("{}/lookup/{}", self.base_url, peer_id))
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|e| SyncError::Discovery(format!("Signaling lookup: {}", e)))?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(SyncError::PeerNotFound(peer_id.to_string()));
        }

        if !resp.status().is_success() {
            return Err(SyncError::Discovery(format!(
                "Signaling lookup failed: {}",
                resp.status()
            )));
        }

        resp.json::<LookupResponse>()
            .await
            .map_err(|e| SyncError::Discovery(format!("Signaling parse: {}", e)))
    }
}
