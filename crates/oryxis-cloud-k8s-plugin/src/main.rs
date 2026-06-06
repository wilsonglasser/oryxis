//! `oryxis-cloud-k8s-plugin`, the Kubernetes cloud provider as a standalone
//! subprocess.
//!
//! Speaks line-delimited JSON-RPC 2.0 over stdio (the
//! `oryxis-plugin-protocol` contract), the same framing the AWS plugin and
//! `oryxis-mcp` use. The main app spawns this binary, runs the `initialize`
//! handshake, and drives discovery / resolve / credentials through it.
//!
//! Unlike the AWS plugin this carries no SDK: every operation shells out to
//! `kubectl`. It stays a separate subprocess only for architectural
//! consistency (uniform `PluginProvider` registry + download-on-demand).
//!
//! stdout carries JSON-RPC frames and nothing else; logs go to stderr. The
//! process is long-running: it serves requests until stdin closes.

mod dispatch;

use std::io::{self, BufRead, Write};

use oryxis_plugin_protocol::{error_codes, method, JsonRpcRequest, JsonRpcResponse};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("oryxis_cloud_k8s_plugin=info".parse().unwrap()),
        )
        .init();

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        "oryxis-cloud-k8s-plugin started"
    );

    let stdin = io::stdin();
    let stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                tracing::error!(error = %e, "stdin read error, exiting");
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                write_response(
                    &stdout,
                    JsonRpcResponse::error(
                        serde_json::Value::Null,
                        error_codes::PARSE_ERROR,
                        format!("parse error: {e}"),
                    ),
                );
                continue;
            }
        };

        if request.is_notification() {
            // The host sends a `shutdown` notification (then closes stdin)
            // when tearing the plugin down. Exit proactively so we flush and
            // stop before the EOF that follows.
            if request.method == method::SHUTDOWN {
                tracing::info!("received shutdown notification, exiting");
                break;
            }
            continue;
        }
        let id = request.id.clone().unwrap_or(serde_json::Value::Null);

        let response = dispatch::handle(&request.method, id, request.params).await;
        write_response(&stdout, response);
    }

    tracing::info!("stdin closed, oryxis-cloud-k8s-plugin exiting");
}

/// Serialize a response and write it as one line to stdout, flushing so the
/// host sees it immediately.
fn write_response(stdout: &io::Stdout, response: JsonRpcResponse) {
    match serde_json::to_string(&response) {
        Ok(json) => {
            let mut out = stdout.lock();
            let _ = writeln!(out, "{json}");
            let _ = out.flush();
        }
        Err(e) => tracing::error!(error = %e, "failed to serialize response"),
    }
}
