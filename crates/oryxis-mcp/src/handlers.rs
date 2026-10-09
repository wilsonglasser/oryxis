use std::hash::{Hash, Hasher};
use std::time::Instant;

use serde_json::{json, Value};
use uuid::Uuid;

use oryxis_core::models::connection::Connection;
use oryxis_ssh::SshEngine;
use oryxis_vault::VaultStore;

use crate::pool;
use crate::server::Server;

/// Host-key verification against the vault's known-host pins, the same
/// rows the app's accept-and-save flow writes. Mirrors
/// `oryxis-app`'s `connect_methods::make_host_key_check`: a different
/// offered algorithm reads as `Unknown` (verify + accept) rather than a
/// "Changed" MITM warning, since the pin is per (host, port, key_type).
///
/// Paired with `with_strict_host_key(true)` at the call site, because
/// this is a headless dialer with no UI to raise a fingerprint prompt.
/// Without both, `ClientHandler::check_server_key` falls through to its
/// legacy arm and returns `Ok(true)` for ANY server key, which hands the
/// stored password and a live TOTP code to whoever answers the dial.
/// Every other dial site in the workspace wires this; this one is the
/// odd one out and the same policy the boot port-forward and SFTP-sync
/// dials already use applies here.
///
/// Every refusal is RECORDED in `refusals` (host, port, key type,
/// fingerprint, and the pin it disagreed with): the callback is the one
/// place that sees the key, and `accept_host_key` pins only what it
/// recorded (`hostkey.rs`).
fn make_host_key_check(
    vault: &VaultStore,
    refusals: std::sync::Arc<crate::hostkey::RefusalLog>,
) -> oryxis_ssh::HostKeyCheckCallback {
    let pinned = vault.list_known_hosts().unwrap_or_default();
    std::sync::Arc::new(move |host, port, key_type, fingerprint| {
        if let Some(existing) = pinned
            .iter()
            .find(|h| h.hostname == host && h.port == port && h.key_type == key_type)
        {
            if existing.fingerprint != fingerprint {
                refusals.record(host, port, key_type, fingerprint, Some(&existing.fingerprint));
                return oryxis_ssh::HostKeyStatus::Changed {
                    old_fingerprint: existing.fingerprint.clone(),
                };
            }
            return oryxis_ssh::HostKeyStatus::Known;
        }
        refusals.record(host, port, key_type, fingerprint, None);
        oryxis_ssh::HostKeyStatus::Unknown
    })
}

/// Who a host logs in as, for the listing tools.
///
/// `username` in a host's JSON is the FIELD, and it is null on a host
/// that leaves the user to its folder or its identity; a client reading
/// only that would report (or reason about) a login the dial never
/// uses. `effective_username` is the resolution, from the same core
/// ordering `VaultStore::apply_effective` collapses onto the dial copy
/// (`oryxis_core::models::inheritance::effective_login`), so what a
/// listing says and what `ssh_execute` authenticates as are one answer.
///
/// Loaded once per call: the groups, the identities and the set of
/// hosts that store a password (the gate on a folder's identity
/// default), three reads however many hosts are listed.
struct LoginResolver {
    groups: Vec<oryxis_core::models::Group>,
    identities: Vec<oryxis_core::models::Identity>,
    with_password: std::collections::HashSet<Uuid>,
}

impl LoginResolver {
    fn load(vault: &VaultStore) -> Self {
        Self {
            groups: vault.list_groups().unwrap_or_default(),
            identities: vault.list_identities().unwrap_or_default(),
            with_password: vault.list_connection_ids_with_password().unwrap_or_default(),
        }
    }

    /// The name the dial offers the server, the engine's own fallback
    /// included: never empty, because a dial always logs in as someone.
    fn username(&self, conn: &Connection) -> String {
        let chain = conn
            .group_id
            .map(|gid| oryxis_core::models::Group::ancestry(&self.groups, gid).chain)
            .unwrap_or_default();
        let answers_credentials =
            conn.key_id.is_some() || self.with_password.contains(&conn.id);
        oryxis_core::models::inheritance::effective_login(
            conn,
            &chain,
            &self.identities,
            answers_credentials,
        )
        .username_or_default()
        .to_string()
    }
}

pub fn handle_list_hosts(vault: &VaultStore, params: Option<&Value>) -> Result<Value, String> {
    let conns = vault.list_mcp_connections().map_err(|e| e.to_string())?;
    let logins = LoginResolver::load(vault);

    let group_filter = params
        .and_then(|p| p.get("group_id"))
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok());

    let tag_filter = params
        .and_then(|p| p.get("tag"))
        .and_then(|v| v.as_str());

    let hosts: Vec<Value> = conns
        .iter()
        .filter(|c| {
            if let Some(gid) = group_filter {
                if c.group_id != Some(gid) {
                    return false;
                }
            }
            if let Some(tag) = tag_filter {
                if !c.tags.iter().any(|t| t == tag) {
                    return false;
                }
            }
            true
        })
        .map(|c| {
            json!({
                "id": c.id.to_string(),
                "label": c.label,
                "hostname": c.hostname,
                "port": c.port,
                "username": c.username,
                "effective_username": logins.username(c),
                "auth_method": format!("{:?}", c.auth_method),
                "group_id": c.group_id.map(|g| g.to_string()),
                "tags": c.tags,
                "notes": c.notes,
                "last_used": c.last_used.map(|d| d.to_rfc3339()),
            })
        })
        .collect();

    Ok(json!(hosts))
}

pub fn handle_list_groups(vault: &VaultStore) -> Result<Value, String> {
    let groups = vault.list_groups().map_err(|e| e.to_string())?;
    let result: Vec<Value> = groups
        .iter()
        .map(|g| {
            json!({
                "id": g.id.to_string(),
                "label": g.label,
                "parent_id": g.parent_id.map(|p| p.to_string()),
                "color": g.color,
                "icon": g.icon,
            })
        })
        .collect();
    Ok(json!(result))
}

pub fn handle_list_keys(vault: &VaultStore) -> Result<Value, String> {
    let keys = vault.list_keys().map_err(|e| e.to_string())?;
    let result: Vec<Value> = keys
        .iter()
        .map(|k| {
            json!({
                "id": k.id.to_string(),
                "label": k.label,
                "fingerprint": k.fingerprint,
                "algorithm": format!("{}", k.algorithm),
                "has_passphrase": k.has_passphrase,
            })
        })
        .collect();
    Ok(json!(result))
}

pub fn handle_get_host(vault: &VaultStore, params: Option<&Value>) -> Result<Value, String> {
    let id_str = params
        .and_then(|p| p.get("id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required parameter: id".to_string())?;

    let id = Uuid::parse_str(id_str).map_err(|_| "Invalid UUID".to_string())?;

    let conns = vault.list_mcp_connections().map_err(|e| e.to_string())?;
    let conn = conns
        .iter()
        .find(|c| c.id == id)
        .ok_or_else(|| "Host not found or not MCP-enabled".to_string())?;

    Ok(json!({
        "id": conn.id.to_string(),
        "label": conn.label,
        "hostname": conn.hostname,
        "port": conn.port,
        "username": conn.username,
        "effective_username": LoginResolver::load(vault).username(conn),
        "auth_method": format!("{:?}", conn.auth_method),
        "group_id": conn.group_id.map(|g| g.to_string()),
        "identity_id": conn.identity_id.map(|i| i.to_string()),
        "key_id": conn.key_id.map(|k| k.to_string()),
        "tags": conn.tags,
        "notes": conn.notes,
        "color": conn.color,
        "last_used": conn.last_used.map(|d| d.to_rfc3339()),
        "created_at": conn.created_at.to_rfc3339(),
        "updated_at": conn.updated_at.to_rfc3339(),
    }))
}

/// Everything one dial needs, resolved from the vault in one sync pass
/// and carried into the async half. The vault lock is a `std` mutex
/// held for this pass only; nothing here borrows it, so the dial and
/// the command run with the vault free for the next request.
pub struct DialPlan {
    pub conn_id: Uuid,
    pub label: String,
    /// `host:port`, for the log line.
    pub endpoint: String,
    /// The EFFECTIVE connection: group inheritance collapsed, username
    /// filled, the proxy on `proxy`.
    pub auth_conn: Connection,
    pub password: Option<String>,
    pub private_key: Option<String>,
    pub certificate: Option<String>,
    /// The jump route's hops, resolved (`None` for a direct host).
    pub resolver: Option<oryxis_ssh::ConnectionResolver>,
    pub engine: SshEngine,
    /// [`reuse_signature`] of `auth_conn`, the pool's reuse key.
    pub signature: u64,
    /// Every (host, port) the dial touches, the target last: what a
    /// refused host key is looked up by when the dial fails.
    pub endpoints: Vec<(String, u16)>,
}

/// Why a plan could not be resolved. `NotFound` is its own case so the
/// caller can drop a pooled connection for a host the user just
/// withdrew from MCP, rather than serving it until idle eviction.
pub enum PlanError {
    NotFound,
    Other(String),
}

impl From<PlanError> for String {
    fn from(e: PlanError) -> String {
        match e {
            PlanError::NotFound => "Host not found or not MCP-enabled".into(),
            PlanError::Other(s) => s,
        }
    }
}

/// Hash of the fields that decide WHERE a dial lands and WHO it
/// authenticates as, on the effective connection. Deliberately not the
/// whole row: the app stamps `last_used` on every connect and
/// `detected_os` after it, narrow updates that change the row without
/// changing the dial, and hashing them would redial every time the user
/// also opened the host in a tab. A change to any field here means the
/// pooled connection no longer represents the host as configured, and
/// the next call dials fresh.
pub fn dial_signature(conn: &Connection) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    conn.hostname.hash(&mut h);
    conn.port.hash(&mut h);
    conn.username.hash(&mut h);
    format!("{:?}", conn.auth_method).hash(&mut h);
    conn.key_id.hash(&mut h);
    conn.identity_id.hash(&mut h);
    conn.use_disk_key.hash(&mut h);
    conn.identity_file.hash(&mut h);
    conn.jump_chain.hash(&mut h);
    conn.proxy_identity_id.hash(&mut h);
    serde_json::to_string(&conn.proxy)
        .unwrap_or_default()
        .hash(&mut h);
    format!("{:?}", conn.address_family).hash(&mut h);
    conn.ciphers.hash(&mut h);
    conn.kex.hash(&mut h);
    conn.macs.hash(&mut h);
    conn.host_key_algorithms.hash(&mut h);
    h.finish()
}

/// The reuse key the pool compares: [`dial_signature`] plus the two
/// POLICY answers that decided whether the pooled connection was
/// allowed to be dialled at all, the host key pinned for its (host,
/// port) and, for a command proxy, whether that line is approved on
/// this device. Revoking either is the user withdrawing trust from the
/// route; a connection kept from before must not outlive that for the
/// idle window (it would keep serving a host whose pin was removed as
/// suspect). `pins` is the vault's known-hosts list; only the entries
/// for this endpoint are hashed, sorted, so an unrelated pin never
/// redials anything.
///
/// `hops` are the endpoints of the jump chain, in order: a route is
/// trusted hop by hop, and a pin withdrawn from a bastion withdraws the
/// route as surely as one withdrawn from the target. (The chain's ids
/// are already in [`dial_signature`]; this adds what each hop's key is
/// trusted as.)
///
/// `first_hop_proxy` is the effective proxy of the route's FIRST hop,
/// the one the engine actually dials through when there is a route (the
/// target's own proxy is not used then). Its command-proxy approval
/// counts the same way the target's does: revoking the line that reaches
/// the bastion withdraws the route.
pub fn reuse_signature(
    conn: &Connection,
    hops: &[(&str, u16)],
    pins: &[oryxis_core::models::known_host::KnownHost],
    trusted_proxy_commands: &std::collections::HashSet<String>,
    first_hop_proxy: Option<&oryxis_core::models::connection::ProxyConfig>,
) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    dial_signature(conn).hash(&mut h);
    let pins_of = |host: &str, port: u16| {
        let mut own: Vec<(&str, &str)> = pins
            .iter()
            .filter(|p| p.hostname == host && p.port == port)
            .map(|p| (p.key_type.as_str(), p.fingerprint.as_str()))
            .collect();
        own.sort_unstable();
        own
    };
    pins_of(&conn.hostname, conn.port).hash(&mut h);
    for (host, port) in hops {
        pins_of(host, *port).hash(&mut h);
    }
    let command_approval = |proxy: Option<&oryxis_core::models::connection::ProxyConfig>| {
        match proxy.map(|p| &p.proxy_type) {
            Some(oryxis_core::models::connection::ProxyType::Command(cmd)) => Some(
                trusted_proxy_commands
                    .contains(&oryxis_core::models::connection::proxy_command_fingerprint(cmd)),
            ),
            _ => None,
        }
    };
    command_approval(conn.proxy.as_ref()).hash(&mut h);
    command_approval(first_hop_proxy).hash(&mut h);
    h.finish()
}

/// A host's credentials as the app's `resolve_credentials` resolves
/// them: the host's own password and key first, a linked identity's
/// second, the disk key filling a still-empty key slot. Serves the
/// target AND every hop of its route, because a bastion must not
/// authenticate differently depending on whether it is dialled directly
/// or on the way to another host.
fn resolve_credentials(
    vault: &VaultStore,
    conn: &Connection,
    identities: &[oryxis_core::models::Identity],
    all_keys: &[oryxis_core::models::SshKey],
) -> (Option<String>, Option<String>, Option<String>) {
    // The certificate (B2) is resolved from the SAME key as the pem, so
    // it can never pair with the wrong key.
    let cert_for = |kid: &uuid::Uuid| -> Option<String> {
        all_keys
            .iter()
            .find(|k| k.id == *kid)
            .and_then(|k| k.certificate.clone())
    };

    // Resolve credentials
    let password = vault.get_connection_password(&conn.id).unwrap_or(None);
    let private_key = conn
        .key_id
        .and_then(|kid| vault.get_key_private(&kid).ok().flatten());
    let conn_cert = conn.key_id.as_ref().and_then(cert_for);

    // If identity linked (the host's own or a group-inherited one), get
    // identity credentials
    let (ident_password, ident_key, ident_cert) = if let Some(iid) = conn.identity_id {
        let ident_pw = vault.get_identity_password(&iid).unwrap_or(None);
        let ident_key_id = identities.iter().find(|i| i.id == iid).and_then(|i| i.key_id);
        let ident_pk = ident_key_id.and_then(|kid| vault.get_key_private(&kid).ok().flatten());
        let ident_cert = ident_key_id.as_ref().and_then(cert_for);
        (ident_pw, ident_pk, ident_cert)
    } else {
        (None, None, None)
    };

    let final_password = password.or(ident_password);
    // Certificate follows the key that wins (conn-preferred, same as the
    // key), so the pair never desyncs.
    let (final_key, final_cert) = if private_key.is_some() {
        (private_key, conn_cert)
    } else {
        (ident_key, ident_cert)
    };
    // The disk key fills a still-empty slot, exactly as the app's
    // `resolve_credentials` does: a host that authenticates in the UI
    // must not fail here for want of a key source. Its certificate is
    // the `<key>-cert.pub` sibling, so the pair still describes ONE key.
    //
    // The MCP never offers a security-key file off disk to a Key / Auto
    // host: it runs unattended, so the engine would refuse to ask for the
    // touch anyway. A `SecurityKey` host resolves its `_sk` file so the
    // refusal it gets names the real reason (nobody at the keyboard)
    // rather than a missing key.
    use oryxis_core::models::connection::AuthMethod;
    let (final_key, final_cert) = match final_key {
        Some(pem) => (Some(pem), final_cert),
        None if matches!(
            conn.auth_method,
            AuthMethod::Key | AuthMethod::Auto | AuthMethod::Certificate | AuthMethod::SecurityKey
        ) =>
        {
            let wanted = if conn.auth_method == AuthMethod::SecurityKey {
                oryxis_vault::DiskKeyWanted::SecurityKey
            } else {
                oryxis_vault::DiskKeyWanted::Software
            };
            match oryxis_vault::resolve_disk_key(
                conn.use_disk_key,
                conn.identity_file.as_deref(),
                wanted,
            )
            .material()
            {
                Some((pem, disk_cert)) => (Some(pem), disk_cert),
                None => (None, final_cert),
            }
        }
        None => (None, final_cert),
    };
    (final_password, final_key, final_cert)
}

/// Resolve the dial for `id`: the effective connection, its
/// credentials and an engine configured for a headless caller. Sync,
/// and the only part of `ssh_execute` that reads the vault.
pub fn resolve_dial_plan(
    vault: &VaultStore,
    id: Uuid,
    refusals: std::sync::Arc<crate::hostkey::RefusalLog>,
) -> Result<DialPlan, PlanError> {
    let conns = vault
        .list_mcp_connections()
        .map_err(|e| PlanError::Other(e.to_string()))?;
    let conn = conns
        .iter()
        .find(|c| c.id == id)
        .ok_or(PlanError::NotFound)?;

    // Group inheritance (D4), the SAME collapse every app dial site
    // applies (`VaultStore::apply_effective`): the effective proxy lands
    // on `conn.proxy`, an inherited username / identity fills the empty
    // fields. Skipping it here is how a headless dial once authenticated
    // differently (as "root") than a tab to the very same host.
    let groups = vault.list_groups().unwrap_or_default();
    let identities = vault.list_identities().unwrap_or_default();
    let mut conn = conn.clone();
    vault.apply_effective(&mut conn, &groups, &identities);
    let conn = &conn;

    let all_keys = vault.list_keys().unwrap_or_default();
    let (final_password, final_key, final_cert) =
        resolve_credentials(vault, conn, &identities, &all_keys);
    let username = conn
        .username
        .clone()
        .unwrap_or_else(|| oryxis_core::models::inheritance::DEFAULT_USERNAME.into());

    // Build a temporary Connection with resolved username for auth. The
    // effective proxy is already collapsed onto `conn.proxy` by
    // `apply_effective` above (re-resolving here would overwrite a
    // group-inherited proxy with the host's own).
    let mut auth_conn = conn.clone();
    auth_conn.username = Some(username);

    // The route, nested hops expanded the way the app expands it
    // (`oryxis_core::jump_route`), and each hop resolved like a host of
    // its own: group inheritance, credentials, the effective proxy.
    // Hops are looked up among ALL hosts, not only the ones exposed to
    // MCP: a bastion is part of the route whether or not it is itself a
    // tool target. A dangling id stays in the route for the engine to
    // report as "jump host not found".
    let all_hosts = if auth_conn.jump_chain.is_empty() {
        Vec::new()
    } else {
        vault.list_connections().unwrap_or_default()
    };
    let resolver = if auth_conn.jump_chain.is_empty() {
        None
    } else {
        auth_conn.jump_chain = oryxis_core::jump_route::expanded_jump_chain(
            auth_conn.id,
            &auth_conn.jump_chain,
            &all_hosts,
        );
        let mut resolver = oryxis_ssh::ConnectionResolver {
            connections: Vec::with_capacity(auth_conn.jump_chain.len()),
            passwords: Default::default(),
            private_keys: Default::default(),
            certificates: Default::default(),
            proxies: Default::default(),
            totp_secrets: Default::default(),
        };
        for jid in &auth_conn.jump_chain {
            let Some(hop) = all_hosts.iter().find(|c| c.id == *jid) else {
                continue;
            };
            let mut hop = hop.clone();
            vault.apply_effective(&mut hop, &groups, &identities);
            let (pw, pk, cert) = resolve_credentials(vault, &hop, &identities, &all_keys);
            if let Some(pw) = pw {
                resolver.passwords.insert(*jid, pw);
            }
            if let Some(pk) = pk {
                resolver.private_keys.insert(*jid, pk);
            }
            if let Some(cert) = cert {
                resolver.certificates.insert(*jid, cert);
            }
            if let Some(proxy) = hop.proxy.clone() {
                resolver.proxies.insert(*jid, proxy);
            }
            // The hop's own second factor; the target's never reaches it.
            if let Some(secret) = vault.get_connection_totp_secret(jid).ok().flatten() {
                resolver.totp_secrets.insert(*jid, secret);
            }
            resolver.connections.push(hop);
        }
        Some(resolver)
    };

    // Build the engine. Honor any per-host legacy-algorithm overrides
    // the user pinned in the app (MCP is headless, so there is no
    // interactive fallback dialog, only the pinned settings apply). The
    // stored TOTP secret rides along for the same reason: an OTP-gated
    // host is unreachable headlessly without the autofill.
    let totp_secret = vault
        .get_connection_totp_secret(&conn.id)
        .ok()
        .flatten();
    // Agent-auth pin (B3): the referenced key's public line (connection
    // key preferred, then the identity's), offered first when the Auto
    // ladder reaches agent auth.
    let pinned_agent = conn
        .key_id
        .or_else(|| {
            conn.identity_id.and_then(|iid| {
                identities
                    .iter()
                    .find(|i| i.id == iid)
                    .and_then(|i| i.key_id)
            })
        })
        .and_then(|kid| all_keys.iter().find(|k| k.id == kid))
        .map(|k| k.public_key.clone())
        .filter(|p| !p.trim().is_empty());
    // Command-proxy approval, resolved from the vault's own list and
    // answered with no UI in the loop, because there is none here.
    // Same authority and same helper the app's unattended dials use, so
    // a host that runs over MCP is exactly a host the user approved in
    // the app, never one a sync peer wrote into the vault.
    let trusted_proxy_commands: std::collections::HashSet<String> = vault
        .list_trusted_proxy_commands()
        .map(|list| list.into_iter().map(|t| t.fingerprint).collect())
        .unwrap_or_default();
    // Each hop's endpoint, along the expanded route. A dangling id has
    // no endpoint and no pin to follow.
    let hops: Vec<(&str, u16)> = auth_conn
        .jump_chain
        .iter()
        .filter_map(|id| all_hosts.iter().find(|c| c.id == *id))
        .map(|c| (c.hostname.as_str(), c.port))
        .collect();
    let endpoints: Vec<(String, u16)> = hops
        .iter()
        .map(|(h, p)| ((*h).to_string(), *p))
        .chain(std::iter::once((conn.hostname.clone(), conn.port)))
        .collect();
    let signature = reuse_signature(
        &auth_conn,
        &hops,
        &vault.list_known_hosts().unwrap_or_default(),
        &trusted_proxy_commands,
        auth_conn
            .jump_chain
            .first()
            .and_then(|first| resolver.as_ref()?.proxies.get(first)),
    );
    let engine = SshEngine::new()
        // Verify the server key against the vault's pins and reject
        // unknown/changed ones: there is no terminal here to surface a
        // fingerprint prompt, so a host reached over MCP must already
        // have been trusted interactively in the app.
        .with_host_key_check(make_host_key_check(vault, refusals))
        .with_strict_host_key(true)
        .with_proxy_command_ask(oryxis_ssh::trusted_only_proxy_command_ask(
            trusted_proxy_commands,
        ))
        .with_totp_secret(totp_secret.as_deref())
        .with_address_family(auth_conn.address_family)
        .with_pinned_agent_key(pinned_agent.as_deref())
        .with_algorithm_overrides(
            auth_conn.ciphers.clone(),
            auth_conn.kex.clone(),
            auth_conn.macs.clone(),
            auth_conn.host_key_algorithms.clone(),
        )
        // Headless bounds. Nobody is typing a second factor here, so
        // the auth window is what a key exchange plus an autofilled
        // OTP need, not sshd's LoginGraceTime; the client waiting on
        // the other end of the pipe has a budget of its own, and every
        // second spent on a wedged handshake comes out of it. The
        // keepalive is what lets a pooled connection notice a link
        // that died silently (russh closes after three unanswered).
        .with_connect_timeout(pool::CONNECT_TIMEOUT)
        .with_auth_timeout(pool::AUTH_TIMEOUT)
        .with_keepalive(Some(pool::KEEPALIVE_INTERVAL));

    Ok(DialPlan {
        conn_id: conn.id,
        label: conn.label.clone(),
        endpoint: oryxis_core::net::host_port(&conn.hostname, conn.port),
        signature,
        auth_conn,
        password: final_password,
        private_key: final_key,
        certificate: final_cert,
        resolver,
        engine,
        endpoints,
    })
}

/// Default and ceiling for `timeout_secs`. The ceiling is what keeps a
/// first call (connect + auth + command) inside the budget of the
/// clients this serves; a longer command belongs in a shell.
pub const DEFAULT_TIMEOUT_SECS: u64 = 30;
pub const MAX_TIMEOUT_SECS: u64 = 180;

pub async fn handle_ssh_execute(
    server: &Server,
    params: Option<&Value>,
    cancel: tokio::sync::watch::Receiver<bool>,
) -> Result<Value, String> {
    let id_str = params
        .and_then(|p| p.get("id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required parameter: id".to_string())?;
    let command = params
        .and_then(|p| p.get("command"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required parameter: command".to_string())?;
    let timeout_secs = params
        .and_then(|p| p.get("timeout_secs"))
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
        .clamp(1, MAX_TIMEOUT_SECS);

    let id = Uuid::parse_str(id_str).map_err(|_| "Invalid UUID".to_string())?;

    let started = Instant::now();
    let plan = match resolve_dial_plan(&server.vault(), id, std::sync::Arc::clone(&server.refusals)) {
        Ok(plan) => plan,
        Err(PlanError::NotFound) => {
            // A host withdrawn from MCP (or deleted) takes its pooled
            // connection with it now, not at idle eviction.
            server.pool.forget(id);
            return Err(PlanError::NotFound.into());
        }
        Err(e) => return Err(e.into()),
    };
    let resolve_ms = started.elapsed().as_millis();
    tracing::debug!(host = %plan.label, command, "ssh_execute command");

    let timeout = std::time::Duration::from_secs(timeout_secs);
    let endpoints = plan.endpoints.clone();
    let outcome = server.pool.exec(plan, command, timeout, cancel).await;
    match outcome {
        Ok(run) => {
            tracing::info!(
                host = %run.label,
                endpoint = %run.endpoint,
                reused = run.reused,
                resolve_ms,
                dial_ms = run.dial_ms,
                exec_ms = run.exec_ms,
                exit_code = run.result.exit_code,
                "ssh_execute done"
            );
            Ok(json!({
                "exit_code": run.result.exit_code,
                "stdout": run.result.stdout,
                "stderr": run.result.stderr,
            }))
        }
        Err(e) => {
            tracing::info!(
                host = %e.label,
                endpoint = %e.endpoint,
                stage = e.stage,
                elapsed_ms = started.elapsed().as_millis(),
                error = %e.error,
                "ssh_execute failed"
            );
            // A handshake that died on a refused host key says which key
            // and what to do about it, instead of russh's "Unknown server
            // key": the refusal was recorded by the check callback while
            // this very dial ran.
            if e.stage == "connect" {
                if let Some(text) =
                    crate::hostkey::describe_refusals(&server.refusals, &endpoints)
                {
                    return Err(format!("{}: {text}", e.stage_title()));
                }
            }
            Err(format!("{}: {}", e.stage_title(), e.error))
        }
    }
}
