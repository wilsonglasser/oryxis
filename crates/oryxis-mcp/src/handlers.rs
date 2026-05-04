use serde_json::{json, Value};
use uuid::Uuid;

use oryxis_ssh::SshEngine;
use oryxis_vault::VaultStore;

pub fn handle_list_hosts(vault: &VaultStore, params: Option<&Value>) -> Result<Value, String> {
    let conns = vault.list_mcp_connections().map_err(|e| e.to_string())?;

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

pub async fn handle_ssh_execute(
    vault: &VaultStore,
    params: Option<&Value>,
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
        .unwrap_or(30)
        .min(300);

    let id = Uuid::parse_str(id_str).map_err(|_| "Invalid UUID".to_string())?;

    // Find connection
    let conns = vault.list_mcp_connections().map_err(|e| e.to_string())?;
    let conn = conns
        .iter()
        .find(|c| c.id == id)
        .ok_or_else(|| "Host not found or not MCP-enabled".to_string())?;

    // Resolve credentials
    let password = vault.get_connection_password(&conn.id).unwrap_or(None);
    let private_key = conn
        .key_id
        .and_then(|kid| vault.get_key_private(&kid).ok().flatten());

    // If identity linked, get identity credentials
    let (ident_password, ident_key) = if let Some(iid) = conn.identity_id {
        let ident_pw = vault.get_identity_password(&iid).unwrap_or(None);
        let identities = vault.list_identities().unwrap_or_default();
        let ident_key_id = identities.iter().find(|i| i.id == iid).and_then(|i| i.key_id);
        let ident_pk = ident_key_id.and_then(|kid| vault.get_key_private(&kid).ok().flatten());
        (ident_pw, ident_pk)
    } else {
        (None, None)
    };

    let final_password = password.or(ident_password);
    let final_key = private_key.or(ident_key);
    let username = conn.username.clone().unwrap_or_else(|| "root".into());

    // Build a temporary Connection with resolved username for auth
    let mut auth_conn = conn.clone();
    auth_conn.username = Some(username);

    // Hydrate proxy password from the encrypted vault column — it isn't
    // part of the persisted ProxyConfig JSON, so we attach it in-memory
    // just before the SSH engine consumes the Connection.
    if let Some(proxy) = auth_conn.proxy.as_mut() {
        proxy.password = vault.get_proxy_password(&auth_conn.id).ok().flatten();
    }

    // Build engine and connect
    let engine = SshEngine::new();

    let mut handle = engine
        .establish_transport(&auth_conn, None)
        .await
        .map_err(|e| format!("Connection failed: {}", e))?;

    engine
        .do_authenticate(
            &mut handle,
            &auth_conn,
            final_password.as_deref(),
            final_key.as_deref(),
        )
        .await
        .map_err(|e| format!("Authentication failed: {}", e))?;

    let timeout = std::time::Duration::from_secs(timeout_secs);
    let result = engine
        .exec_command(handle, command, timeout)
        .await
        .map_err(|e| format!("Execution failed: {}", e))?;

    Ok(json!({
        "exit_code": result.exit_code,
        "stdout": result.stdout,
        "stderr": result.stderr,
    }))
}
