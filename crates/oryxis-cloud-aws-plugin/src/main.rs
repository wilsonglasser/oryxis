//! `oryxis-cloud-aws-plugin`, the AWS cloud provider as a standalone
//! subprocess.
//!
//! Speaks line-delimited JSON-RPC 2.0 over stdio (the
//! `oryxis-plugin-protocol` contract), the same framing the
//! `oryxis-mcp` server uses. The main app spawns this binary, runs
//! the `initialize` handshake, and drives the 7 provider/transport
//! operations through it; all the heavy `aws-sdk-*` code lives here
//! instead of in the app binary.
//!
//! stdout carries JSON-RPC frames and nothing else, every log line
//! goes to stderr. The process is long-running: it serves requests
//! until stdin closes (the host drops it on idle / shutdown), then
//! exits.
//!
//! Credentials trust boundary: the host (the main app) is the only
//! writer on this stdin pipe and the only reader of stdout. Cloud
//! credentials (`CloudProfile.secret`) travel in plaintext JSON on
//! that pipe, that's intentional and safe in the same process tree.
//! The plugin must never log `profile.secret` to stderr (which the
//! tracing layer drains to the app); never echo it in error
//! messages or response payloads; and never persist it to disk.

mod dispatch;

use std::io::{self, BufRead, Write};

use oryxis_plugin_protocol::{error_codes, method, JsonRpcRequest, JsonRpcResponse};

#[tokio::main]
async fn main() {
    // rustls 0.23 requires a crypto provider installed before any
    // TLS connection. The AWS SDK's HTTPS client otherwise fails
    // with a generic "dispatch failure". `install_default` errors
    // only if a provider was already set, harmless to ignore.
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Logs to stderr so they never corrupt the JSON-RPC stream on
    // stdout.
    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("oryxis_cloud_aws_plugin=info".parse().unwrap()),
        )
        .init();

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        "oryxis-cloud-aws-plugin started"
    );

    let stdin = io::stdin();
    let stdout = io::stdout();

    // One request per line. Blocking reads are fine: the host
    // serializes calls, so there's never more than one in flight,
    // and the runtime stays free for the AWS SDK's async work
    // between lines.
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

        // A request without an id is a notification, no response.
        if request.is_notification() {
            // Host `shutdown` notification (followed by stdin close): exit
            // proactively so we flush before the EOF.
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

    tracing::info!("stdin closed, oryxis-cloud-aws-plugin exiting");
}

/// Serialize a response and write it as one line to stdout,
/// flushing so the host sees it immediately.
fn write_response(stdout: &io::Stdout, response: JsonRpcResponse) {
    match serde_json::to_string(&response) {
        Ok(json) => {
            let mut out = stdout.lock();
            let _ = writeln!(out, "{json}");
            let _ = out.flush();
        }
        // A response that won't serialize is a bug in our own types,
        // not something the host can recover from. Log and move on.
        Err(e) => tracing::error!(error = %e, "failed to serialize response"),
    }
}
