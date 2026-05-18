//! Standalone signaling + relay server for Oryxis sync.
//!
//! API mirrors the Cloudflare Worker in `signaling-worker/worker.js`:
//!
//! ```text
//! POST   /register             register device IP:port (TTL 300s)
//! GET    /lookup/:id           look up peer's IP:port
//! DELETE /register/:id         unregister device
//! POST   /relay/:id/inbox      enqueue a frame for recipient
//! GET    /relay/:id/inbox      long-poll consume the oldest frame
//! ```
//!
//! Every request requires `Authorization: Bearer <token>`. The token
//! is shared between the relay and every paired client; it gates who
//! can use the relay at all (the QUIC / X25519 / Ed25519 layer above
//! still authenticates individual peers and seals payloads end-to-end,
//! so this token is "can talk to the relay" not "can read traffic").
//!
//! Run:
//!
//! ```text
//! oryxis-relay --port 8080 --token <bearer>
//! ```

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use bytes::Bytes;
use clap::Parser;
use serde::Deserialize;
use std::time::Instant;
use uuid::Uuid;

mod discovery;
mod queue;

use discovery::{DeviceRecord, DeviceTable};
use queue::{InboxRegistry, QueueEntry};

/// Soft cap on a single relayed frame, matches the worker and client.
const MAX_FRAME_BYTES: usize = 256 * 1024;
const MAX_WAIT_MS: u64 = 30_000;

#[derive(Parser, Debug)]
#[command(
    name = "oryxis-relay",
    version,
    about = "Self-hostable signaling + relay server for Oryxis sync"
)]
struct Args {
    /// Port to bind. The relay is meant to sit behind TLS termination
    /// (nginx / Caddy / Cloudflare); bind to 127.0.0.1 + reverse proxy
    /// in production.
    #[arg(short, long, default_value_t = 8080, env = "ORYXIS_RELAY_PORT")]
    port: u16,

    /// Bind address. Default `0.0.0.0` for container-friendliness;
    /// switch to `127.0.0.1` when running behind a reverse proxy on
    /// the same host.
    #[arg(short, long, default_value = "0.0.0.0", env = "ORYXIS_RELAY_BIND")]
    bind: String,

    /// Shared bearer token. Must match the value paired clients sent
    /// in `Settings > Sync > Signaling token`.
    #[arg(short, long, env = "ORYXIS_RELAY_TOKEN")]
    token: String,
}

#[derive(Clone)]
struct AppState {
    devices: DeviceTable,
    inboxes: InboxRegistry,
    token: String,
    /// Token-bucket rate limiter keyed by X-Sender-Id. Without it a
    /// single bearer-token holder could fill any recipient's 256-
    /// slot queue in under a second (~256 frames × 256 KiB =
    /// 65 MiB) before the queue's own depth cap kicked oldest. The
    /// bucket bounds steady-state push rate per sender across all
    /// recipients.
    push_limiter: RateLimiter,
}

/// Token bucket rate limiter, one bucket per `Uuid` key. Cheap mutex
/// around a HashMap; the relay is single-tenant by deployment so a
/// hot lock under load is acceptable.
#[derive(Clone)]
struct RateLimiter {
    inner: Arc<std::sync::Mutex<HashMap<Uuid, BucketState>>>,
    capacity: f64,
    refill_per_sec: f64,
}

struct BucketState {
    tokens: f64,
    last: Instant,
}

impl RateLimiter {
    fn new(capacity: f64, refill_per_sec: f64) -> Self {
        Self {
            inner: Arc::new(std::sync::Mutex::new(HashMap::new())),
            capacity,
            refill_per_sec,
        }
    }

    /// Returns `true` when the request was admitted (a token consumed)
    /// and `false` when the bucket was empty (429 Too Many Requests).
    fn try_consume(&self, key: Uuid) -> bool {
        let mut map = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let now = Instant::now();
        let state = map.entry(key).or_insert(BucketState {
            tokens: self.capacity,
            last: now,
        });
        let elapsed = now.duration_since(state.last).as_secs_f64();
        state.tokens = (state.tokens + elapsed * self.refill_per_sec).min(self.capacity);
        state.last = now;
        if state.tokens >= 1.0 {
            state.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    let args = Args::parse();
    if args.token.len() < 16 {
        anyhow::bail!(
            "--token must be at least 16 characters (use a long random string)"
        );
    }

    let state = AppState {
        devices: DeviceTable::new(),
        inboxes: InboxRegistry::new(),
        token: args.token,
        // 10-token burst, refilling at 1/sec. Legit sync flows
        // average well under 1 push/sec; this leaves room for a
        // pairing handshake spike (3-4 frames back-to-back) and
        // blocks an attacker from filling the 256-slot queue
        // faster than ~4 minutes regardless of how many tokens
        // they hold.
        push_limiter: RateLimiter::new(10.0, 1.0),
    };

    state
        .devices
        .clone()
        .spawn_sweeper(Duration::from_secs(60));
    state
        .inboxes
        .clone()
        .spawn_sweeper(Duration::from_secs(60));

    let app = Router::new()
        .route("/register", post(register_device))
        .route("/lookup/:id", get(lookup_device))
        .route("/register/:id", delete(unregister_device))
        .route(
            "/relay/:id/inbox",
            post(push_inbox).get(poll_inbox),
        )
        .route("/healthz", get(healthz))
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", args.bind, args.port)
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid bind address: {e}"))?;
    tracing::info!("oryxis-relay listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("oryxis_relay=info,axum=info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

// ───── auth ─────

/// Check `Authorization: Bearer <token>` against the configured
/// shared secret. Returns a uniform 401 on any failure so callers
/// can't tell missing-header from bad-token.
fn check_auth(headers: &HeaderMap, expected: &str) -> bool {
    let header = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let token = header.strip_prefix("Bearer ").unwrap_or("");
    constant_time_eq(token.as_bytes(), expected.as_bytes())
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::CONTENT_TYPE, "application/json")],
        r#"{"error":"Unauthorized"}"#,
    )
        .into_response()
}

/// Constant-time byte comparison. Tokens are short (≤ 64B) so a
/// hand-rolled loop is fine; pulling in `subtle` for one helper is
/// overkill.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// ───── handlers ─────

#[derive(Deserialize)]
struct RegisterReq {
    device_id: Uuid,
    #[serde(default)]
    public_key_fp: String,
    ip: String,
    port: u16,
}

async fn register_device(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::Json<RegisterReq>,
) -> Response {
    if !check_auth(&headers, &state.token) {
        return unauthorized();
    }
    let now = chrono::Utc::now().to_rfc3339();
    state
        .devices
        .register(DeviceRecord {
            device_id: body.0.device_id,
            public_key_fp: body.0.public_key_fp,
            ip: body.0.ip,
            port: body.0.port,
            registered_at: now,
        })
        .await;
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        r#"{"ok":true,"ttl":300}"#,
    )
        .into_response()
}

async fn lookup_device(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !check_auth(&headers, &state.token) {
        return unauthorized();
    }
    let Ok(uuid) = Uuid::parse_str(&id) else {
        return (
            StatusCode::BAD_REQUEST,
            [(header::CONTENT_TYPE, "application/json")],
            r#"{"error":"Bad id"}"#,
        )
            .into_response();
    };
    match state.devices.lookup(&uuid).await {
        Some(rec) => {
            let body = serde_json::to_string(&rec).unwrap_or_else(|_| "{}".into());
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/json")],
                body,
            )
                .into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "application/json")],
            r#"{"error":"Not found"}"#,
        )
            .into_response(),
    }
}

async fn unregister_device(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !check_auth(&headers, &state.token) {
        return unauthorized();
    }
    if let Ok(uuid) = Uuid::parse_str(&id) {
        state.devices.unregister(&uuid).await;
    }
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        r#"{"ok":true}"#,
    )
        .into_response()
}

async fn push_inbox(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    if !check_auth(&headers, &state.token) {
        return unauthorized();
    }
    let Ok(recipient) = Uuid::parse_str(&id) else {
        return (StatusCode::BAD_REQUEST, "Bad recipient").into_response();
    };
    let sender_str = headers
        .get("X-Sender-Id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let Ok(sender) = Uuid::parse_str(sender_str) else {
        return (StatusCode::BAD_REQUEST, "Missing X-Sender-Id").into_response();
    };
    if body.is_empty() {
        return (StatusCode::BAD_REQUEST, "Empty body").into_response();
    }
    if body.len() > MAX_FRAME_BYTES {
        return (StatusCode::PAYLOAD_TOO_LARGE, "Too large").into_response();
    }
    if !state.push_limiter.try_consume(sender) {
        return (StatusCode::TOO_MANY_REQUESTS, "Rate limit exceeded").into_response();
    }
    let depth = state
        .inboxes
        .push(recipient, QueueEntry {
            sender_id: sender,
            body,
            inserted_at: Instant::now(),
        })
        .await;
    tracing::debug!("relay: push {sender} -> {recipient} (depth {depth})");
    StatusCode::NO_CONTENT.into_response()
}

#[derive(Deserialize)]
struct PollParams {
    #[serde(default)]
    wait_ms: u64,
}

async fn poll_inbox(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(params): Query<PollParams>,
) -> Response {
    if !check_auth(&headers, &state.token) {
        return unauthorized();
    }
    let Ok(recipient) = Uuid::parse_str(&id) else {
        return (StatusCode::BAD_REQUEST, "Bad recipient").into_response();
    };
    let wait_ms = params.wait_ms.min(MAX_WAIT_MS);
    let wait = Duration::from_millis(wait_ms);
    match state.inboxes.pop_wait(recipient, wait).await {
        Some(entry) => {
            let mut resp = Response::new(axum::body::Body::from(entry.body));
            resp.headers_mut().insert(
                header::CONTENT_TYPE,
                "application/octet-stream".parse().unwrap(),
            );
            resp.headers_mut().insert(
                "X-Sender-Id",
                entry.sender_id.to_string().parse().unwrap(),
            );
            resp
        }
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

async fn healthz() -> Response {
    (StatusCode::OK, "ok").into_response()
}

// ───── integration tests ─────
//
// Spin the relay in-process and round-trip every endpoint via
// `reqwest`. End-to-end (peer A sends, peer B polls, body comes
// back identical) is what we actually want to know about; unit
// tests on the queue/discovery primitives live next to them.

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;

    async fn spawn_server() -> (SocketAddr, String) {
        let token = "test-token-must-be-long-enough".to_string();
        let state = AppState {
            devices: DeviceTable::new(),
            inboxes: InboxRegistry::new(),
            token: token.clone(),
            // Generous cap in tests so the integration suite isn't
            // throttled by the steady-state production limit.
            push_limiter: RateLimiter::new(1000.0, 1000.0),
        };
        let app = Router::new()
            .route("/register", post(register_device))
            .route("/lookup/:id", get(lookup_device))
            .route("/register/:id", delete(unregister_device))
            .route("/relay/:id/inbox", post(push_inbox).get(poll_inbox))
            .route("/healthz", get(healthz))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        // Yield so the server is ready before tests hit it.
        tokio::time::sleep(Duration::from_millis(20)).await;
        (addr, token)
    }

    #[tokio::test]
    async fn rejects_missing_auth() {
        let (addr, _) = spawn_server().await;
        let client = reqwest::Client::new();
        let resp = client
            .get(format!("http://{addr}/lookup/{}", Uuid::new_v4()))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 401);
    }

    #[tokio::test]
    async fn register_lookup_unregister() {
        let (addr, token) = spawn_server().await;
        let client = reqwest::Client::new();
        let device_id = Uuid::new_v4();

        let resp = client
            .post(format!("http://{addr}/register"))
            .bearer_auth(&token)
            .json(&serde_json::json!({
                "device_id": device_id,
                "public_key_fp": "fp",
                "ip": "192.0.2.1",
                "port": 9000,
            }))
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success());

        let resp = client
            .get(format!("http://{addr}/lookup/{device_id}"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["ip"], "192.0.2.1");
        assert_eq!(body["port"], 9000);

        let resp = client
            .delete(format!("http://{addr}/register/{device_id}"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success());

        let resp = client
            .get(format!("http://{addr}/lookup/{device_id}"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 404);
    }

    #[tokio::test]
    async fn relay_round_trip() {
        let (addr, token) = spawn_server().await;
        let client = reqwest::Client::new();
        let recipient = Uuid::new_v4();
        let sender = Uuid::new_v4();
        let body = b"\x01\x02\x03 oryxis test frame";

        // Sender posts a frame.
        let resp = client
            .post(format!("http://{addr}/relay/{recipient}/inbox"))
            .bearer_auth(&token)
            .header("X-Sender-Id", sender.to_string())
            .header("Content-Type", "application/octet-stream")
            .body(body.to_vec())
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 204);

        // Recipient long-polls and gets it back.
        let resp = client
            .get(format!(
                "http://{addr}/relay/{recipient}/inbox?wait_ms=1000"
            ))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        assert_eq!(
            resp.headers().get("X-Sender-Id").unwrap().to_str().unwrap(),
            sender.to_string()
        );
        let got = resp.bytes().await.unwrap();
        assert_eq!(&got[..], body);
    }

    #[tokio::test]
    async fn relay_returns_204_after_timeout() {
        let (addr, token) = spawn_server().await;
        let client = reqwest::Client::new();
        let recipient = Uuid::new_v4();
        let start = Instant::now();
        let resp = client
            .get(format!(
                "http://{addr}/relay/{recipient}/inbox?wait_ms=200"
            ))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 204);
        // Sanity: long-poll must actually have waited, not raced to 204.
        assert!(start.elapsed() >= Duration::from_millis(150));
    }

    #[tokio::test]
    async fn relay_rejects_oversized_body() {
        let (addr, token) = spawn_server().await;
        let client = reqwest::Client::new();
        let recipient = Uuid::new_v4();
        let sender = Uuid::new_v4();
        // Build a body just above the cap.
        let body = vec![0u8; MAX_FRAME_BYTES + 1];
        let resp = client
            .post(format!("http://{addr}/relay/{recipient}/inbox"))
            .bearer_auth(&token)
            .header("X-Sender-Id", sender.to_string())
            .header("Content-Type", "application/octet-stream")
            .body(body)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 413);
    }
}
