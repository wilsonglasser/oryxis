//! `PluginHost`, owns one cloud-provider subprocess and multiplexes
//! typed JSON-RPC calls onto it.
//!
//! Lifecycle (decision A, long-running with idle teardown):
//!
//! - The subprocess is spawned lazily on the first call and reused
//!   for every subsequent one, discovery runs several times per
//!   session and the AWS SDK's cold start is real, so paying it once
//!   beats paying it per call.
//! - A reaper task tears the subprocess down after
//!   [`PluginHost::DEFAULT_IDLE_TIMEOUT`] with no activity, so an
//!   unused provider doesn't sit on ~25 MB of resident memory.
//! - If the pipe goes unhealthy (process exits, stdout closes, a
//!   call times out) the connection is dropped and the next call
//!   respawns, restart-on-crash without the caller noticing beyond
//!   one failed call.
//!
//! Concurrency model: calls are serialized through the connection
//! mutex. A multiplexing reader still routes responses by id (so the
//! framing is already correct for concurrent calls), but the host
//! holds the lock for the duration of a call. That's deliberate for
//! v1, discovery isn't hot enough to need parallel in-flight calls,
//! and serialization keeps the lifecycle reasoning simple.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, Weak};
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::{oneshot, Mutex};
use tokio::task::JoinHandle;

use oryxis_plugin_protocol::{
    method, rpc_error_to_cloud, InitializeParams, InitializeResult, JsonRpcRequest,
    JsonRpcResponse, Method, SUPPORTED_PROTOCOL_VERSIONS,
};

use super::PluginError;

/// Process-global generation counter. Each spawned connection takes
/// the next value so its reaper can tell "its" connection from a
/// successor that replaced it.
static EPOCH: AtomicU64 = AtomicU64::new(0);

/// Map of in-flight requests, keyed by JSON-RPC id. Shared between a
/// connection and its reader task.
type PendingMap = Arc<StdMutex<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>>;

/// One cloud-provider plugin subprocess and the machinery to call
/// it. Cheap to construct, the subprocess isn't spawned until the
/// first call.
pub struct PluginHost {
    binary: PathBuf,
    provider_id: String,
    idle_timeout: Duration,
    call_timeout: Duration,
    /// The live connection, `None` until first use and again after
    /// idle teardown / crash. Behind a `tokio::sync::Mutex` so calls
    /// serialize cleanly across `.await` points.
    inner: Arc<Mutex<Option<Connection>>>,
}

impl PluginHost {
    /// Idle window before the subprocess is reaped. Matches the plan's
    /// 5-minute figure.
    pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(300);
    /// Per-call ceiling. Generous, discovery against a large account
    /// legitimately takes a while.
    pub const DEFAULT_CALL_TIMEOUT: Duration = Duration::from_secs(120);

    /// Build a host for `provider_id` backed by the binary at
    /// `binary`. Nothing is spawned here.
    pub fn new(binary: impl Into<PathBuf>, provider_id: impl Into<String>) -> Self {
        Self {
            binary: binary.into(),
            provider_id: provider_id.into(),
            idle_timeout: Self::DEFAULT_IDLE_TIMEOUT,
            call_timeout: Self::DEFAULT_CALL_TIMEOUT,
            inner: Arc::new(Mutex::new(None)),
        }
    }

    /// Override the idle / call timeouts. Builder-style, mainly for
    /// tests that can't wait five minutes for a reaper tick.
    pub fn with_timeouts(mut self, idle: Duration, call: Duration) -> Self {
        self.idle_timeout = idle;
        self.call_timeout = call;
        self
    }

    /// Provider id this host drives (`"aws"`, ...).
    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }

    /// Ensure the subprocess is up and return a clone of its
    /// `initialize` result, provider id, plugin version, negotiated
    /// protocol, and the capability list the UI uses to grey out
    /// unsupported transports.
    pub async fn initialize_info(&self) -> Result<InitializeResult, PluginError> {
        let mut guard = self.inner.lock().await;
        self.ensure_connection(&mut guard).await?;
        Ok(guard.as_ref().expect("ensured above").init.clone())
    }

    /// Issue a typed JSON-RPC call, spawning the subprocess if it
    /// isn't running. On an unhealthy pipe the connection is torn
    /// down before the error returns, so the next call gets a fresh
    /// process.
    pub async fn call<M: Method>(&self, params: M::Params) -> Result<M::Result, PluginError> {
        let params_json = serde_json::to_value(&params)
            .map_err(|e| PluginError::Protocol(format!("serialize params: {e}")))?;

        let mut guard = self.inner.lock().await;
        self.ensure_connection(&mut guard).await?;

        // Scope the `&mut Connection` borrow so the error arm below
        // can take the guard.
        let send_result = {
            let conn = guard.as_mut().expect("ensured above");
            let r = conn.request(M::NAME, params_json, self.call_timeout).await;
            if r.is_ok() {
                conn.last_used = Instant::now();
            }
            r
        };

        let resp = match send_result {
            Ok(r) => r,
            Err(e) => {
                // An unhealthy pipe stays unhealthy. Drop the
                // connection so the next call respawns cleanly.
                if matches!(
                    e,
                    PluginError::ProcessGone | PluginError::Timeout(_) | PluginError::Io(_)
                ) {
                    *guard = None;
                }
                return Err(e);
            }
        };

        if let Some(err) = resp.error {
            // The call reached the provider and it returned a
            // `CloudError`, recover the exact variant from `data`.
            return Err(PluginError::Provider(rpc_error_to_cloud(&err)));
        }
        let result = resp.result.unwrap_or(Value::Null);
        serde_json::from_value::<M::Result>(result)
            .map_err(|e| PluginError::Protocol(format!("bad result for {}: {e}", M::NAME)))
    }

    /// Spawn the subprocess if there's no connection, or if the
    /// current one has been idle past the timeout, a stale
    /// connection the reaper simply hasn't gotten to yet.
    async fn ensure_connection(
        &self,
        guard: &mut Option<Connection>,
    ) -> Result<(), PluginError> {
        let stale = matches!(
            guard.as_ref(),
            Some(c) if c.last_used.elapsed() >= self.idle_timeout
        );
        if stale {
            *guard = None;
        }
        if guard.is_none() {
            let conn = Connection::spawn(
                &self.binary,
                &self.provider_id,
                self.call_timeout,
                self.idle_timeout,
                Arc::downgrade(&self.inner),
            )
            .await?;
            *guard = Some(conn);
        }
        Ok(())
    }
}

/// A live plugin subprocess plus the state to talk to it.
struct Connection {
    /// `kill_on_drop(true)` is set, so the subprocess dies with this
    /// struct even before `Drop` gets to `start_kill`.
    child: Child,
    stdin: ChildStdin,
    /// In-flight requests, the reader task fulfils each oneshot when
    /// the matching response line arrives.
    pending: PendingMap,
    /// Next JSON-RPC id to hand out (id 0 was the handshake).
    next_id: u64,
    /// Result of the `initialize` handshake.
    init: InitializeResult,
    /// Generation token, lets the reaper distinguish this connection
    /// from a successor.
    epoch: u64,
    /// Last completed call, drives idle teardown.
    last_used: Instant,
    reader: JoinHandle<()>,
    stderr: JoinHandle<()>,
}

impl Drop for Connection {
    fn drop(&mut self) {
        // A dropped `JoinHandle` does not stop its task, abort the
        // reader and stderr drain explicitly. The child itself dies
        // via `kill_on_drop`, `start_kill` just makes it immediate.
        self.reader.abort();
        self.stderr.abort();
        let _ = self.child.start_kill();
    }
}

impl Connection {
    /// Spawn the subprocess, wire up the reader / stderr tasks, run
    /// the `initialize` handshake, and start the idle reaper.
    async fn spawn(
        binary: &Path,
        provider_id: &str,
        call_timeout: Duration,
        idle_timeout: Duration,
        host: Weak<Mutex<Option<Connection>>>,
    ) -> Result<Connection, PluginError> {
        if !binary.exists() {
            return Err(PluginError::BinaryNotFound(binary.to_path_buf()));
        }
        let mut child = Command::new(binary)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| PluginError::Spawn(e.to_string()))?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| PluginError::Spawn("plugin stdin not captured".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| PluginError::Spawn("plugin stdout not captured".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| PluginError::Spawn("plugin stderr not captured".into()))?;

        let pending: PendingMap = Arc::new(StdMutex::new(HashMap::new()));
        let reader = tokio::spawn(reader_task(stdout, pending.clone(), provider_id.to_string()));
        let stderr_h = tokio::spawn(stderr_task(stderr, provider_id.to_string()));

        // `initialize` handshake, before the struct exists, on id 0.
        let params = serde_json::to_value(InitializeParams {
            supported_versions: SUPPORTED_PROTOCOL_VERSIONS.to_vec(),
        })
        .map_err(|e| PluginError::Protocol(format!("serialize initialize: {e}")))?;
        let resp = send_rpc(
            &mut stdin,
            &pending,
            0,
            method::INITIALIZE,
            params,
            call_timeout,
        )
        .await?;
        if let Some(err) = resp.error {
            return Err(PluginError::Protocol(format!(
                "initialize failed: {} (code {})",
                err.message, err.code
            )));
        }
        let init: InitializeResult =
            serde_json::from_value(resp.result.unwrap_or(Value::Null))
                .map_err(|e| PluginError::Protocol(format!("bad initialize result: {e}")))?;
        if !SUPPORTED_PROTOCOL_VERSIONS.contains(&init.protocol_version) {
            return Err(PluginError::VersionMismatch {
                host: SUPPORTED_PROTOCOL_VERSIONS.to_vec(),
                plugin: vec![init.protocol_version],
            });
        }

        let epoch = EPOCH.fetch_add(1, Ordering::Relaxed);
        tokio::spawn(reaper_task(host, epoch, idle_timeout, provider_id.to_string()));

        tracing::info!(
            target = "oryxis::plugins",
            provider = %provider_id,
            plugin_version = %init.plugin_version,
            protocol = init.protocol_version,
            "plugin subprocess ready"
        );

        Ok(Connection {
            child,
            stdin,
            pending,
            next_id: 1,
            init,
            epoch,
            last_used: Instant::now(),
            reader,
            stderr: stderr_h,
        })
    }

    /// Send one request and await its response.
    async fn request(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<JsonRpcResponse, PluginError> {
        let id = self.next_id;
        self.next_id += 1;
        send_rpc(&mut self.stdin, &self.pending, id, method, params, timeout).await
    }
}

/// Write one request frame and await the matching response. Free
/// function rather than a method so `Connection::spawn` can run the
/// handshake before the `Connection` struct exists.
async fn send_rpc(
    stdin: &mut ChildStdin,
    pending: &StdMutex<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>,
    id: u64,
    method: &str,
    params: Value,
    timeout: Duration,
) -> Result<JsonRpcResponse, PluginError> {
    let (tx, rx) = oneshot::channel();
    pending.lock().unwrap().insert(id, tx);

    let req = JsonRpcRequest::new(id, method, params);
    let line = serde_json::to_string(&req)
        .map_err(|e| PluginError::Protocol(format!("serialize request: {e}")))?;

    // A write failure means the pipe is gone, clear the pending
    // entry so it doesn't leak.
    let write = async {
        stdin.write_all(line.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await
    };
    if let Err(e) = write.await {
        pending.lock().unwrap().remove(&id);
        return Err(PluginError::Io(e));
    }

    match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(resp)) => Ok(resp),
        // Sender dropped: the reader task exited (EOF / crash).
        Ok(Err(_)) => Err(PluginError::ProcessGone),
        Err(_) => {
            pending.lock().unwrap().remove(&id);
            Err(PluginError::Timeout(timeout))
        }
    }
}

/// Read response frames off the subprocess's stdout and route each
/// to the waiting caller by id. Exits on EOF or a read error, and on
/// the way out drops every pending sender so in-flight callers get
/// `ProcessGone` instead of hanging until their timeout.
async fn reader_task(stdout: ChildStdout, pending: PendingMap, provider_id: String) {
    let mut lines = BufReader::new(stdout).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                match serde_json::from_str::<JsonRpcResponse>(line) {
                    Ok(resp) => {
                        let slot = resp
                            .id
                            .as_u64()
                            .and_then(|id| pending.lock().unwrap().remove(&id));
                        match slot {
                            Some(tx) => {
                                let _ = tx.send(resp);
                            }
                            None => tracing::warn!(
                                target = "oryxis::plugins",
                                provider = %provider_id,
                                "plugin response with unknown id: {:?}",
                                resp.id
                            ),
                        }
                    }
                    Err(e) => tracing::warn!(
                        target = "oryxis::plugins",
                        provider = %provider_id,
                        error = %e,
                        "unparseable plugin response line"
                    ),
                }
            }
            Ok(None) => break, // EOF, plugin closed stdout
            Err(e) => {
                tracing::error!(
                    target = "oryxis::plugins",
                    provider = %provider_id,
                    error = %e,
                    "error reading plugin stdout"
                );
                break;
            }
        }
    }
    pending.lock().unwrap().clear();
}

/// Drain the subprocess's stderr into the tracing log. Plugins log
/// to stderr (stdout is reserved for JSON-RPC frames, same rule as
/// `oryxis-mcp`).
async fn stderr_task(stderr: ChildStderr, provider_id: String) {
    let mut lines = BufReader::new(stderr).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim();
        if !line.is_empty() {
            tracing::debug!(
                target = "oryxis::plugins",
                provider = %provider_id,
                "{line}"
            );
        }
    }
}

/// Tear the subprocess down once it's been idle past the timeout.
/// One reaper is spawned per connection and exits as soon as its
/// connection is gone or has been replaced.
async fn reaper_task(
    host: Weak<Mutex<Option<Connection>>>,
    epoch: u64,
    idle_timeout: Duration,
    provider_id: String,
) {
    // Check twice per window so teardown lands within ~half the
    // timeout of the actual deadline.
    let tick = (idle_timeout / 2).max(Duration::from_secs(1));
    loop {
        tokio::time::sleep(tick).await;
        let Some(host) = host.upgrade() else {
            return; // PluginHost dropped, nothing left to reap.
        };
        let mut guard = host.lock().await;
        match guard.as_ref() {
            // Already torn down, or replaced by a newer connection
            // that runs its own reaper, this one is done.
            None => return,
            Some(conn) if conn.epoch != epoch => return,
            Some(conn) if conn.last_used.elapsed() >= idle_timeout => {
                tracing::info!(
                    target = "oryxis::plugins",
                    provider = %provider_id,
                    "plugin idle for {idle_timeout:?}, shutting down subprocess"
                );
                *guard = None; // drops Connection: kills child, aborts tasks
                return;
            }
            // Still warm, keep watching.
            Some(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn missing_binary_surfaces_binary_not_found() {
        // No subprocess can spawn from a path that doesn't exist,
        // the host must report it as `BinaryNotFound` rather than a
        // generic spawn error.
        let host = PluginHost::new(
            "/nonexistent/oryxis-cloud-test-plugin",
            "test",
        );
        let err = host.initialize_info().await.unwrap_err();
        assert!(
            matches!(err, PluginError::BinaryNotFound(_)),
            "expected BinaryNotFound, got {err:?}"
        );
    }

    #[test]
    fn host_construction_does_not_spawn() {
        // `new` is pure, no process, no panic, just field setup.
        let host = PluginHost::new("/bin/true", "test")
            .with_timeouts(Duration::from_secs(10), Duration::from_secs(5));
        assert_eq!(host.provider_id(), "test");
        assert_eq!(host.idle_timeout, Duration::from_secs(10));
        assert_eq!(host.call_timeout, Duration::from_secs(5));
    }
}
