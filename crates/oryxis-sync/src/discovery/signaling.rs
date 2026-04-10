use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::SyncError;

#[derive(Debug, Serialize)]
struct RegisterRequest {
    device_id: String,
    public_key_fp: String,
    ip: String,
    port: u16,
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
    pub async fn register(
        &self,
        device_id: &Uuid,
        public_key_fp: &str,
        ip: &str,
        port: u16,
    ) -> Result<(), SyncError> {
        let req = RegisterRequest {
            device_id: device_id.to_string(),
            public_key_fp: public_key_fp.into(),
            ip: ip.into(),
            port,
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
