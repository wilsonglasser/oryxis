mod handlers;
mod protocol;
mod server;
#[cfg(test)]
mod tests;
mod tools;

use std::io::{self, BufRead, Write};

use oryxis_vault::VaultStore;

#[tokio::main]
async fn main() {
    // Logging to stderr (so it doesn't corrupt JSON-RPC on stdout)
    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("oryxis_mcp=info".parse().unwrap()),
        )
        .init();

    // Open vault
    let mut vault = match VaultStore::open_default() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Failed to open vault: {}", e);
            std::process::exit(1);
        }
    };

    // Unlock vault
    let password = std::env::var("ORYXIS_VAULT_PASSWORD").unwrap_or_default();
    if password.is_empty() {
        // Try without password
        if vault.open_without_password().is_err() {
            eprintln!("Vault is password-protected. Set ORYXIS_VAULT_PASSWORD environment variable.");
            std::process::exit(1);
        }
    } else if let Err(e) = vault.unlock(&password) {
        eprintln!("Failed to unlock vault: {}", e);
        std::process::exit(1);
    }

    tracing::info!("Oryxis MCP server started");

    // JSON-RPC stdio loop
    let stdin = io::stdin();
    let stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        if line.trim().is_empty() {
            continue;
        }

        let request: protocol::JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let err = protocol::JsonRpcResponse::error(
                    serde_json::Value::Null,
                    -32700,
                    format!("Parse error: {}", e),
                );
                let response = serde_json::to_string(&err).unwrap();
                let mut out = stdout.lock();
                let _ = writeln!(out, "{}", response);
                let _ = out.flush();
                continue;
            }
        };

        let id = request.id.clone().unwrap_or(serde_json::Value::Null);
        let response = server::handle_request(
            &request.method,
            id,
            request.params.as_ref(),
            &vault,
        )
        .await;

        let json = serde_json::to_string(&response).unwrap();
        let mut out = stdout.lock();
        let _ = writeln!(out, "{}", json);
        let _ = out.flush();
    }
}
