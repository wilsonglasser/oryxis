//! Host keys a headless dial refused, and the one tool that can pin
//! one of them.
//!
//! The MCP dials strict: a server key the vault has not pinned fails
//! the handshake, because nobody is here to read a fingerprint. Before
//! this module the agent saw "Unknown server key" and nothing else, so
//! a host added five minutes ago could not be reached over MCP until
//! someone opened it in a tab. Now the check callback RECORDS what it
//! refused (the callback already receives host, port, key type and
//! fingerprint; the engine never had to change), the `ssh_execute`
//! answer spells it out, and `accept_host_key` pins exactly that key.
//!
//! Two guards keep this from being trust-on-first-use with extra steps:
//! the tool only pins a key this process saw refused, with the exact
//! four values the refusal recorded (the agent cannot invent a
//! fingerprint and have it trusted), and it never pins over a different
//! pin for the same (host, port, key type): a CHANGED key is the one
//! case a person has to look at, in the app's Known Hosts.

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::{json, Value};

use crate::server::Server;

/// A key the strict check turned away.
#[derive(Debug, Clone)]
pub struct Refusal {
    pub host: String,
    pub port: u16,
    pub key_type: String,
    pub fingerprint: String,
    /// The pinned fingerprint it disagreed with (`Changed`), or `None`
    /// for a host with no pin at all (`Unknown`).
    pub old_fingerprint: Option<String>,
}

type Key = (String, u16, String);

/// Every refusal this process has seen, latest per (host, port, key
/// type). Shared between the dial's check callback and the tool.
#[derive(Default)]
pub struct RefusalLog {
    entries: Mutex<HashMap<Key, Refusal>>,
}

impl RefusalLog {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Key, Refusal>> {
        match self.entries.lock() {
            Ok(g) => g,
            Err(poison) => poison.into_inner(),
        }
    }

    pub fn record(
        &self,
        host: &str,
        port: u16,
        key_type: &str,
        fingerprint: &str,
        old_fingerprint: Option<&str>,
    ) {
        self.lock().insert(
            (host.to_string(), port, key_type.to_string()),
            Refusal {
                host: host.to_string(),
                port,
                key_type: key_type.to_string(),
                fingerprint: fingerprint.to_string(),
                old_fingerprint: old_fingerprint.map(str::to_string),
            },
        );
    }

    /// The refusals recorded for `endpoints`, in the order given.
    pub fn for_endpoints(&self, endpoints: &[(String, u16)]) -> Vec<Refusal> {
        let entries = self.lock();
        endpoints
            .iter()
            .flat_map(|(host, port)| {
                entries
                    .values()
                    .filter(move |r| &r.host == host && r.port == *port)
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    pub fn get(&self, host: &str, port: u16, key_type: &str) -> Option<Refusal> {
        self.lock()
            .get(&(host.to_string(), port, key_type.to_string()))
            .cloned()
    }

    pub fn remove(&self, host: &str, port: u16, key_type: &str) {
        self.lock()
            .remove(&(host.to_string(), port, key_type.to_string()));
    }

    /// Forget every refusal recorded for `endpoints`: called before a
    /// dial, so a refusal an earlier dial left behind (never accepted)
    /// cannot describe a later failure of another kind (the server
    /// down, a DNS miss) as a host-key refusal.
    pub fn clear_endpoints(&self, endpoints: &[(String, u16)]) {
        self.lock()
            .retain(|_, r| !endpoints.iter().any(|(h, p)| &r.host == h && r.port == *p));
    }
}

/// The `ssh_execute` answer for a dial that failed on a refused key:
/// what was refused and what the agent can do about it. `None` when no
/// endpoint of the dial has a refusal on record, in which case the
/// failure was something else and the plain error stands.
pub fn describe_refusals(log: &RefusalLog, endpoints: &[(String, u16)]) -> Option<String> {
    let refusals = log.for_endpoints(endpoints);
    if refusals.is_empty() {
        return None;
    }
    let mut lines: Vec<String> = Vec::new();
    let mut keys: Vec<Value> = Vec::new();
    for r in &refusals {
        let endpoint = oryxis_core::net::host_port(&r.host, r.port);
        match &r.old_fingerprint {
            None => {
                lines.push(format!(
                    "Host key not trusted for {endpoint}: {} {} is not pinned in the Oryxis vault. \
                     If this fingerprint is the one the server's administrator published, call \
                     accept_host_key with exactly these values and retry; otherwise stop.",
                    r.key_type, r.fingerprint
                ));
                keys.push(json!({
                    "host": r.host,
                    "port": r.port,
                    "key_type": r.key_type,
                    "fingerprint": r.fingerprint,
                    "status": "unknown",
                    "accept_with": "accept_host_key",
                }));
            }
            Some(old) => {
                lines.push(format!(
                    "HOST KEY CHANGED for {endpoint}: the vault pins {} {old}, the server offered \
                     {}. This can be a reinstalled server or an interception; accept_host_key \
                     refuses changed keys. A person has to review it in Oryxis > Known Hosts.",
                    r.key_type, r.fingerprint
                ));
                keys.push(json!({
                    "host": r.host,
                    "port": r.port,
                    "key_type": r.key_type,
                    "fingerprint": r.fingerprint,
                    "pinned_fingerprint": old,
                    "status": "changed",
                }));
            }
        }
    }
    let detail = serde_json::to_string_pretty(&json!({ "refused_host_keys": keys }))
        .unwrap_or_default();
    Some(format!("{}\n{detail}", lines.join("\n")))
}

/// `accept_host_key`: pin a key a refused dial reported. Unknown only,
/// the exact four values, never over a different pin.
pub fn handle_accept_host_key(server: &Server, params: Option<&Value>) -> Result<Value, String> {
    let str_param = |name: &str| -> Result<String, String> {
        params
            .and_then(|p| p.get(name))
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("Missing required parameter: {name}"))
    };
    let host = str_param("host")?;
    let key_type = str_param("key_type")?;
    let fingerprint = str_param("fingerprint")?;
    let port = params
        .and_then(|p| p.get("port"))
        .and_then(|v| v.as_u64())
        .filter(|p| (1..=65535).contains(p))
        .map(|p| p as u16)
        .ok_or_else(|| "Missing or invalid parameter: port (1-65535)".to_string())?;

    // Guard one: only a key a dial of THIS process reported, verbatim.
    let Some(refusal) = server.refusals.get(&host, port, &key_type) else {
        return Err(format!(
            "No refused dial reported a {key_type} key for {}; run ssh_execute against the host \
             first and use the values its answer carries.",
            oryxis_core::net::host_port(&host, port)
        ));
    };
    if refusal.fingerprint != fingerprint {
        return Err(format!(
            "The fingerprint does not match what the server offered ({}); accept_host_key pins \
             only the key the refused dial reported.",
            refusal.fingerprint
        ));
    }
    if let Some(old) = &refusal.old_fingerprint {
        return Err(format!(
            "The key for {} changed (vault pins {old}, server offered {fingerprint}); a changed \
             key is never accepted here. Review it in Oryxis > Known Hosts.",
            oryxis_core::net::host_port(&host, port)
        ));
    }

    // Guard two: re-read the vault at the moment of writing. The app may
    // have pinned (or re-pinned) the host meanwhile, and `save_known_host`
    // replaces a different fingerprint without complaint.
    let vault = server.vault();
    let existing = vault
        .list_known_hosts()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|k| k.hostname == host && k.port == port && k.key_type == key_type);
    match existing {
        Some(k) if k.fingerprint == fingerprint => {
            server.refusals.remove(&host, port, &key_type);
            return Ok(json!({
                "pinned": true,
                "already_pinned": true,
                "host": host, "port": port, "key_type": key_type, "fingerprint": fingerprint,
            }));
        }
        Some(k) => {
            return Err(format!(
                "The vault now pins a different {key_type} key for {} ({}); not replacing it.",
                oryxis_core::net::host_port(&host, port),
                k.fingerprint
            ));
        }
        None => {}
    }
    let kh = oryxis_core::models::KnownHost::new(&host, port, &key_type, &fingerprint);
    vault.save_known_host(&kh).map_err(|e| e.to_string())?;
    drop(vault);
    server.refusals.remove(&host, port, &key_type);
    tracing::info!(host = %host, port, key_type = %key_type, fingerprint = %fingerprint, "host key pinned");
    // The running app keeps its Known Hosts in memory: tell it.
    if let Err(e) = oryxis_vault::change_notice::announce("oryxis-mcp") {
        tracing::warn!(error = %e, "could not announce the vault change");
    }
    Ok(json!({
        "pinned": true,
        "already_pinned": false,
        "host": host, "port": port, "key_type": key_type, "fingerprint": fingerprint,
    }))
}
