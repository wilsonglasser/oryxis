mod handlers;
mod hostkey;
mod pool;
mod protocol;
mod server;
mod stdio;
#[cfg(test)]
mod tests;
mod tools;

use std::io;
use std::sync::Arc;

use oryxis_vault::VaultStore;

#[tokio::main]
async fn main() {
    // Logging to stderr (so it doesn't corrupt JSON-RPC on stdout). The
    // client that spawned us keeps stderr in its own log, so this is
    // the record a report comes with: every request and its outcome
    // here, every dial and auth step from the engine. `RUST_LOG`
    // widens it (`russh=debug` for the wire).
    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("oryxis_mcp=info".parse().unwrap())
                .add_directive("oryxis_ssh=info".parse().unwrap()),
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

    // Token gate: if the vault stores a non-empty `mcp_server_token`,
    // the caller MUST present a matching `ORYXIS_MCP_TOKEN` env var.
    // An empty stored token keeps the legacy unauthenticated path so
    // existing setups don't break on upgrade.
    let stored_token = vault
        .get_setting("mcp_server_token")
        .ok()
        .flatten()
        .unwrap_or_default();
    if !stored_token.is_empty() {
        let supplied = std::env::var("ORYXIS_MCP_TOKEN").unwrap_or_default();
        if supplied != stored_token {
            eprintln!(
                "MCP token mismatch. Set ORYXIS_MCP_TOKEN to the value shown in Oryxis Settings > Security > MCP. Regenerate the token there if you've lost it."
            );
            std::process::exit(1);
        }
    }

    tracing::info!(version = env!("CARGO_PKG_VERSION"), "Oryxis MCP server started");

    let server = server::Server::new(vault);

    // Idle pooled connections are closed on a timer, so a host does not
    // keep a login from a conversation that went quiet.
    let sweeper = {
        let server = Arc::clone(&server);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(pool::SWEEP_INTERVAL).await;
                server.pool.sweep_idle();
            }
        })
    };

    stdio::serve(Arc::clone(&server), tokio::io::stdin(), tokio::io::stdout()).await;

    sweeper.abort();
    tracing::info!("Oryxis MCP server stopped");
}
