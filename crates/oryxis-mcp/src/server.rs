use std::sync::{Arc, Mutex, MutexGuard};

use serde_json::{json, Value};
use tokio::sync::watch;

use oryxis_vault::VaultStore;

use crate::handlers;
use crate::pool::Pool;
use crate::protocol::JsonRpcResponse;
use crate::tools;

/// Protocol revisions this server implements. Tools-only over stdio is
/// wire-identical across all three; listing them lets version
/// negotiation echo whatever the client asked for instead of forcing a
/// downgrade to the oldest revision.
const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18"];

/// Version negotiation per the MCP spec: echo the client's requested
/// version when we support it, otherwise answer with the latest one we
/// do (the client then decides whether to proceed or disconnect).
fn negotiate_protocol_version(params: Option<&Value>) -> &'static str {
    let requested = params
        .and_then(|p| p.get("protocolVersion"))
        .and_then(|v| v.as_str());
    SUPPORTED_PROTOCOL_VERSIONS
        .iter()
        .find(|v| Some(**v) == requested)
        .copied()
        .unwrap_or(SUPPORTED_PROTOCOL_VERSIONS[SUPPORTED_PROTOCOL_VERSIONS.len() - 1])
}

/// What every request shares: the unlocked vault and the connection
/// pool. Requests run concurrently (one task each, see `stdio`), so
/// the vault sits behind a mutex; its reads are short and synchronous
/// and no handler holds the guard across an await, which is what keeps
/// a slow dial from blocking a `ping`.
pub struct Server {
    vault: Mutex<VaultStore>,
    pub pool: Pool,
    /// Host keys the strict dials refused, for `accept_host_key`.
    pub refusals: Arc<crate::hostkey::RefusalLog>,
}

impl Server {
    pub fn new(vault: VaultStore) -> Arc<Self> {
        Arc::new(Self {
            vault: Mutex::new(vault),
            pool: Pool::new(),
            refusals: Arc::new(crate::hostkey::RefusalLog::default()),
        })
    }

    /// The vault, for one synchronous read. A poisoned lock is
    /// recovered rather than propagated: a panic in one request must
    /// not turn every later one into an error.
    pub fn vault(&self) -> MutexGuard<'_, VaultStore> {
        match self.vault.lock() {
            Ok(g) => g,
            Err(poison) => poison.into_inner(),
        }
    }

    /// Close pooled connections before the process ends.
    pub async fn shutdown(&self) {
        self.pool.shutdown().await;
    }

    pub async fn handle_request(
        &self,
        method: &str,
        id: Value,
        params: Option<&Value>,
        cancel: watch::Receiver<bool>,
    ) -> JsonRpcResponse {
        match method {
            "initialize" => JsonRpcResponse::success(
                id,
                json!({
                    "protocolVersion": negotiate_protocol_version(params),
                    "capabilities": {
                        "tools": {}
                    },
                    "serverInfo": {
                        "name": "oryxis-mcp",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }),
            ),

            // Mandatory in every MCP revision: reply with an empty result so
            // health checks don't read a -32601 as a dead server.
            "ping" => JsonRpcResponse::success(id, json!({})),

            "tools/list" => {
                let tools = tools::tool_definitions();
                JsonRpcResponse::success(id, json!({ "tools": tools }))
            }

            "tools/call" => {
                let tool_name = params
                    .and_then(|p| p.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let arguments = params.and_then(|p| p.get("arguments"));

                // Check if MCP is enabled. Read per call, so the toggle in
                // Settings takes effect on the next request without a
                // restart of the client.
                let enabled = self
                    .vault()
                    .get_setting("mcp_server_enabled")
                    .ok()
                    .flatten()
                    .map(|v| v == "true")
                    .unwrap_or(false);
                if !enabled {
                    return JsonRpcResponse::success(
                        id,
                        json!({
                            "content": [{
                                "type": "text",
                                "text": "MCP server is disabled. Enable it in Oryxis Settings > Security."
                            }],
                            "isError": true
                        }),
                    );
                }

                let result = match tool_name {
                    "list_hosts" => handlers::handle_list_hosts(&self.vault(), arguments),
                    "get_host" => handlers::handle_get_host(&self.vault(), arguments),
                    "list_groups" => {
                        Ok(handlers::handle_list_groups(&self.vault()).unwrap_or(json!([])))
                    }
                    "list_keys" => Ok(handlers::handle_list_keys(&self.vault()).unwrap_or(json!([]))),
                    "ssh_execute" => handlers::handle_ssh_execute(self, arguments, cancel).await,
                    "accept_host_key" => crate::hostkey::handle_accept_host_key(self, arguments),
                    "create_host" => crate::writes::handle_create_host(self, arguments),
                    "update_host" => crate::writes::handle_update_host(self, arguments),
                    _ => Err(format!("Unknown tool: {}", tool_name)),
                };

                match result {
                    Ok(data) => {
                        let text = serde_json::to_string_pretty(&data).unwrap_or_default();
                        JsonRpcResponse::success(
                            id,
                            json!({
                                "content": [{
                                    "type": "text",
                                    "text": text
                                }]
                            }),
                        )
                    }
                    Err(e) => JsonRpcResponse::success(
                        id,
                        json!({
                            "content": [{
                                "type": "text",
                                "text": e
                            }],
                            "isError": true
                        }),
                    ),
                }
            }

            _ => JsonRpcResponse::error(id, -32601, format!("Method not found: {}", method)),
        }
    }
}
