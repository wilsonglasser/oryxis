//! `create_host` / `update_host`: the MCP's two writes to the host
//! table, behind a switch the user turns on in Settings > MCP Server
//! (`mcp_server_allow_writes`, off by default, read per call like the
//! server switch itself).
//!
//! The rules are the editor's, applied by a caller that is not a
//! person:
//!
//! - A folder is RESOLVED (path or label), never created: a typo must
//!   not mint a folder that then rides sync. The error lists the folders
//!   that exist.
//! - A key, an identity and a jump hop are named by id or by label; an
//!   ambiguous label is refused with a request for the id, never guessed.
//!   Hops resolve among ALL hosts (a bastion is a route, not a tool
//!   target), and a host is never its own hop.
//! - The password is the one secret accepted, tri-state like the vault's
//!   API: absent keeps what is stored, `null` clears it, a string stores
//!   it encrypted. Nothing a write returns ever carries it.
//! - No proxy fields: `ProxyType::Command` becomes a LOCAL process, and
//!   only what the user TYPED is ever pre-approved; an AI-supplied proxy
//!   is not typed. A host that needs one is edited in the app.
//! - `updated_at` is stamped here: `save_connection` writes it as given,
//!   and an unstamped edit loses under sync's last-writer-wins.
//! - Every write announces itself (`oryxis_vault::change_notice`) so a
//!   running app re-reads its lists.

use std::collections::HashSet;

use serde_json::{json, Value};
use uuid::Uuid;

use oryxis_core::models::connection::{AuthMethod, Connection, ConnectionProtocol};
use oryxis_core::models::group::Group;
use oryxis_vault::VaultStore;

use crate::handlers::handle_get_host;
use crate::server::Server;

/// The setting that unlocks both tools.
pub const ALLOW_WRITES_SETTING: &str = "mcp_server_allow_writes";

/// Refuse unless the user switched writes on. Read per call so the
/// toggle takes effect on the next request without a restart.
fn gate(vault: &VaultStore) -> Result<(), String> {
    let on = vault
        .get_setting(ALLOW_WRITES_SETTING)
        .ok()
        .flatten()
        .is_some_and(|v| v == "true");
    if on {
        Ok(())
    } else {
        Err("Host writes over MCP are off. Turn on \"Allow the MCP server to add and edit hosts\" \
             in Oryxis Settings > MCP Server."
            .to_string())
    }
}

/// A field the call may carry: absent (keep), `null` (clear) or a value.
enum Field<T> {
    Absent,
    Clear,
    Set(T),
}

fn str_field(params: Option<&Value>, name: &str) -> Result<Field<String>, String> {
    match params.and_then(|p| p.get(name)) {
        None => Ok(Field::Absent),
        Some(Value::Null) => Ok(Field::Clear),
        Some(Value::String(s)) => Ok(Field::Set(s.trim().to_string())),
        Some(_) => Err(format!("Parameter {name} must be a string")),
    }
}

fn list_field(params: Option<&Value>, name: &str) -> Result<Field<Vec<String>>, String> {
    match params.and_then(|p| p.get(name)) {
        None => Ok(Field::Absent),
        Some(Value::Null) => Ok(Field::Clear),
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| {
                v.as_str()
                    .map(|s| s.trim().to_string())
                    .ok_or_else(|| format!("Parameter {name} must be an array of strings"))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Field::Set),
        Some(_) => Err(format!("Parameter {name} must be an array of strings")),
    }
}

fn bool_field(params: Option<&Value>, name: &str) -> Result<Option<bool>, String> {
    match params.and_then(|p| p.get(name)) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(format!("Parameter {name} must be a boolean")),
    }
}

fn port_field(params: Option<&Value>) -> Result<Option<u16>, String> {
    match params.and_then(|p| p.get("port")) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .filter(|p| (1..=65535).contains(p))
            .map(|p| Some(p as u16))
            .ok_or_else(|| "Parameter port must be an integer from 1 to 65535".to_string()),
    }
}

fn parse_auth_method(s: &str) -> Result<AuthMethod, String> {
    Ok(match s.trim().to_ascii_lowercase().as_str() {
        "auto" => AuthMethod::Auto,
        "password" => AuthMethod::Password,
        "key" => AuthMethod::Key,
        "agent" => AuthMethod::Agent,
        "interactive" => AuthMethod::Interactive,
        "password_prompt" => AuthMethod::PasswordPrompt,
        other => {
            return Err(format!(
                "Unknown auth_method \"{other}\"; one of auto, password, key, agent, interactive, \
                 password_prompt (certificate and security-key hosts are set up in the app)"
            ))
        }
    })
}

/// Resolve `input` to one entry of `items` by id or by exact label,
/// naming the entries when it fails. An ambiguous label is refused: the
/// caller must say which one by id.
fn resolve_named<T>(
    what: &str,
    input: &str,
    items: &[T],
    id_of: impl Fn(&T) -> Uuid,
    label_of: impl Fn(&T) -> &str,
) -> Result<Uuid, String> {
    if let Ok(id) = Uuid::parse_str(input) {
        return items
            .iter()
            .map(&id_of)
            .find(|i| *i == id)
            .ok_or_else(|| format!("No {what} with id {id}"));
    }
    let matches: Vec<&T> = items.iter().filter(|i| label_of(i) == input).collect();
    match matches.len() {
        1 => Ok(id_of(matches[0])),
        0 => {
            let mut labels: Vec<&str> = items.iter().map(&label_of).collect();
            labels.sort_unstable();
            Err(format!(
                "No {what} named \"{input}\". Existing: {}",
                if labels.is_empty() { "(none)".to_string() } else { labels.join(", ") }
            ))
        }
        n => Err(format!(
            "{n} {what}s are named \"{input}\"; pass the id instead ({})",
            matches
                .iter()
                .map(|m| id_of(m).to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// Resolve a folder by breadcrumb path or label among the MANUAL
/// folders. Never creates one.
fn resolve_group(vault: &VaultStore, input: &str) -> Result<Uuid, String> {
    let groups = vault.list_groups().map_err(|e| e.to_string())?;
    Group::resolve_path_or_label(&groups, input, &HashSet::new()).ok_or_else(|| {
        let mut paths: Vec<String> = groups
            .iter()
            .filter(|g| g.cloud_query.is_none())
            .map(|g| Group::path_of(&groups, g.id))
            .collect();
        paths.sort_unstable();
        paths.dedup();
        format!(
            "No folder \"{input}\". Folders are never created over MCP; existing: {}",
            if paths.is_empty() { "(none)".to_string() } else { paths.join(", ") }
        )
    })
}

/// Apply the call's fields to `conn`. Returns the password tri-state
/// for `save_connection` (`None` keep, `Some("")` clear, `Some(pw)`).
fn apply_fields(
    vault: &VaultStore,
    conn: &mut Connection,
    params: Option<&Value>,
) -> Result<Option<String>, String> {
    if let Field::Set(label) = str_field(params, "label")? {
        if label.is_empty() {
            return Err("Parameter label must not be empty".to_string());
        }
        conn.label = label;
    }
    if let Field::Set(hostname) = str_field(params, "hostname")? {
        if hostname.is_empty() {
            return Err("Parameter hostname must not be empty".to_string());
        }
        conn.hostname = hostname;
    }
    if let Some(port) = port_field(params)? {
        conn.port = port;
    }
    match str_field(params, "username")? {
        Field::Absent => {}
        Field::Clear => conn.username = None,
        Field::Set(u) => conn.username = (!u.is_empty()).then_some(u),
    }
    if let Field::Set(m) = str_field(params, "auth_method")? {
        conn.auth_method = parse_auth_method(&m)?;
    }
    match str_field(params, "notes")? {
        Field::Absent => {}
        Field::Clear => conn.notes = None,
        Field::Set(n) => conn.notes = (!n.is_empty()).then_some(n),
    }
    match list_field(params, "tags")? {
        Field::Absent => {}
        Field::Clear => conn.tags.clear(),
        Field::Set(tags) => {
            conn.tags = tags.into_iter().filter(|t| !t.is_empty()).collect();
        }
    }
    if let Some(on) = bool_field(params, "mcp_enabled")? {
        conn.mcp_enabled = on;
    }
    match str_field(params, "group")? {
        Field::Absent => {}
        Field::Clear => conn.group_id = None,
        Field::Set(g) if g.is_empty() => conn.group_id = None,
        Field::Set(g) => conn.group_id = Some(resolve_group(vault, &g)?),
    }
    match str_field(params, "key")? {
        Field::Absent => {}
        Field::Clear => conn.key_id = None,
        Field::Set(k) if k.is_empty() => conn.key_id = None,
        Field::Set(k) => {
            let keys = vault.list_keys().map_err(|e| e.to_string())?;
            conn.key_id = Some(resolve_named("key", &k, &keys, |x| x.id, |x| x.label.as_str())?);
        }
    }
    match str_field(params, "identity")? {
        Field::Absent => {}
        Field::Clear => conn.identity_id = None,
        Field::Set(i) if i.is_empty() => conn.identity_id = None,
        Field::Set(i) => {
            let identities = vault.list_identities().map_err(|e| e.to_string())?;
            conn.identity_id = Some(resolve_named(
                "identity",
                &i,
                &identities,
                |x| x.id,
                |x| x.label.as_str(),
            )?);
        }
    }
    match list_field(params, "jump_chain")? {
        Field::Absent => {}
        Field::Clear => conn.jump_chain.clear(),
        Field::Set(hops) => {
            let all = vault.list_connections().map_err(|e| e.to_string())?;
            let mut chain = Vec::with_capacity(hops.len());
            for hop in hops.iter().filter(|h| !h.is_empty()) {
                let id = resolve_named("host", hop, &all, |x| x.id, |x| x.label.as_str())?;
                if id == conn.id {
                    return Err("A host cannot be its own jump hop".to_string());
                }
                if !chain.contains(&id) {
                    chain.push(id);
                }
            }
            conn.jump_chain = chain;
        }
    }
    for refused in ["proxy", "proxy_identity", "proxy_identity_id", "proxy_command"] {
        if params.and_then(|p| p.get(refused)).is_some() {
            return Err(
                "Proxies are not set over MCP (a command proxy is a local process only the user \
                 may approve); edit the host in the app."
                    .to_string(),
            );
        }
    }
    Ok(match str_field(params, "password")? {
        Field::Absent => None,
        Field::Clear => Some(String::new()),
        Field::Set(pw) => Some(pw),
    })
}

fn finish(vault: &VaultStore, conn: &Connection, password: Option<&str>) -> Result<Value, String> {
    vault
        .save_connection(conn, password)
        .map_err(|e| e.to_string())?;
    if let Err(e) = oryxis_vault::change_notice::announce("oryxis-mcp") {
        tracing::warn!(error = %e, "could not announce the vault change");
    }
    handle_get_host(vault, Some(&json!({ "id": conn.id.to_string() })))
}

/// `create_host`: a new SSH host from `label` + `hostname` and whatever
/// else the call carries.
pub fn handle_create_host(server: &Server, params: Option<&Value>) -> Result<Value, String> {
    let vault = server.vault();
    gate(&vault)?;
    let Field::Set(label) = str_field(params, "label")? else {
        return Err("Missing required parameter: label".to_string());
    };
    let Field::Set(hostname) = str_field(params, "hostname")? else {
        return Err("Missing required parameter: hostname".to_string());
    };
    if label.is_empty() || hostname.is_empty() {
        return Err("label and hostname must not be empty".to_string());
    }
    let mut conn = Connection::new(label, hostname);
    let password = apply_fields(&vault, &mut conn, params)?;
    conn.updated_at = chrono::Utc::now();
    tracing::info!(host = %conn.label, id = %conn.id, "create_host");
    finish(&vault, &conn, password.as_deref())
}

/// `update_host`: change the given fields of an MCP-visible SSH host;
/// every field not named keeps its value.
pub fn handle_update_host(server: &Server, params: Option<&Value>) -> Result<Value, String> {
    let vault = server.vault();
    gate(&vault)?;
    let id_str = params
        .and_then(|p| p.get("id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required parameter: id".to_string())?;
    let id = Uuid::parse_str(id_str).map_err(|_| "Invalid UUID".to_string())?;
    let conns = vault.list_mcp_connections().map_err(|e| e.to_string())?;
    let mut conn = conns
        .into_iter()
        .find(|c| c.id == id)
        .ok_or_else(|| "Host not found or not MCP-enabled".to_string())?;
    if conn.protocol != ConnectionProtocol::Ssh {
        return Err("Only SSH hosts are edited over MCP".to_string());
    }
    let password = apply_fields(&vault, &mut conn, params)?;
    conn.updated_at = chrono::Utc::now();
    tracing::info!(host = %conn.label, id = %conn.id, "update_host");
    finish(&vault, &conn, password.as_deref())
}
