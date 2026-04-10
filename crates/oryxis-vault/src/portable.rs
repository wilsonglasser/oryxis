use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use oryxis_core::models::{
    Connection, Group, Identity, KnownHost, Snippet, SshKey,
};

use crate::store::{encrypt, decrypt, VaultError, VaultStore};

// ---------------------------------------------------------------------------
// File format constants
// ---------------------------------------------------------------------------

const MAGIC: &[u8; 6] = b"ORYXIS";
const FORMAT_VERSION: u16 = 1;
const HEADER_LEN: usize = 12; // 6 magic + 2 version + 4 flags
const FLAG_INCLUDES_KEYS: u32 = 1;

// ---------------------------------------------------------------------------
// Export types
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct ExportPayload {
    version: u16,
    exported_at: DateTime<Utc>,
    includes_private_keys: bool,
    groups: Vec<Group>,
    connections: Vec<ExportConnection>,
    keys: Vec<ExportKey>,
    identities: Vec<ExportIdentity>,
    snippets: Vec<Snippet>,
    known_hosts: Vec<KnownHost>,
}

#[derive(Serialize, Deserialize)]
struct ExportConnection {
    #[serde(flatten)]
    connection: Connection,
    password: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct ExportKey {
    #[serde(flatten)]
    key: SshKey,
    private_key: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct ExportIdentity {
    #[serde(flatten)]
    identity: Identity,
    password: Option<String>,
}

// ---------------------------------------------------------------------------
// Public API types
// ---------------------------------------------------------------------------

pub struct ExportOptions {
    pub include_private_keys: bool,
    pub filter: ExportFilter,
}

#[derive(Clone)]
pub enum ExportFilter {
    /// Export everything.
    All,
    /// Export only specific connections (+ their dependencies).
    Hosts(Vec<uuid::Uuid>),
    /// Export all connections in a group and subgroups (+ their dependencies).
    Group(uuid::Uuid),
}

pub struct ImportResult {
    pub connections_added: usize,
    pub connections_updated: usize,
    pub connections_skipped: usize,
    pub keys_added: usize,
    pub keys_skipped: usize,
    pub groups_added: usize,
    pub groups_skipped: usize,
    pub identities_added: usize,
    pub identities_updated: usize,
    pub identities_skipped: usize,
    pub snippets_added: usize,
    pub snippets_skipped: usize,
    pub known_hosts_added: usize,
    pub known_hosts_skipped: usize,
}

// ---------------------------------------------------------------------------
// Header validation
// ---------------------------------------------------------------------------

fn validate_header(data: &[u8]) -> Result<(u16, u32), VaultError> {
    if data.len() < HEADER_LEN {
        return Err(VaultError::Crypto("File too short".into()));
    }
    if &data[..6] != MAGIC {
        return Err(VaultError::Crypto("Invalid file format".into()));
    }
    let version = u16::from_le_bytes([data[6], data[7]]);
    let flags = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    if version > FORMAT_VERSION {
        return Err(VaultError::Crypto(format!(
            "Unsupported format version {} (max supported: {})",
            version, FORMAT_VERSION
        )));
    }
    Ok((version, flags))
}

fn build_header(flags: u32) -> Vec<u8> {
    let mut header = Vec::with_capacity(HEADER_LEN);
    header.extend_from_slice(MAGIC);
    header.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    header.extend_from_slice(&flags.to_le_bytes());
    header
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

pub fn export_vault(
    store: &VaultStore,
    password: &str,
    options: ExportOptions,
) -> Result<Vec<u8>, VaultError> {
    // Collect all data from vault
    let all_groups = store.list_groups()?;
    let all_connections = store.list_connections()?;
    let all_keys = store.list_keys()?;
    let all_identities = store.list_identities()?;
    let all_snippets = store.list_snippets()?;
    let all_known_hosts = store.list_known_hosts()?;

    // Apply filter to select which connections to export
    let filtered_connections: Vec<&Connection> = match &options.filter {
        ExportFilter::All => all_connections.iter().collect(),
        ExportFilter::Hosts(ids) => {
            let mut selected: Vec<&Connection> = all_connections.iter()
                .filter(|c| ids.contains(&c.id))
                .collect();
            // Include jump hosts as dependencies
            let jump_ids: Vec<uuid::Uuid> = selected.iter()
                .flat_map(|c| c.jump_chain.iter().copied())
                .collect();
            for jid in &jump_ids {
                if !selected.iter().any(|c| c.id == *jid)
                    && let Some(jc) = all_connections.iter().find(|c| c.id == *jid)
                {
                    selected.push(jc);
                }
            }
            selected
        }
        ExportFilter::Group(group_id) => {
            // Collect group + subgroups recursively
            let mut group_ids = vec![*group_id];
            let mut i = 0;
            while i < group_ids.len() {
                let gid = group_ids[i];
                for g in &all_groups {
                    if g.parent_id == Some(gid) && !group_ids.contains(&g.id) {
                        group_ids.push(g.id);
                    }
                }
                i += 1;
            }
            all_connections.iter()
                .filter(|c| c.group_id.is_some_and(|gid| group_ids.contains(&gid)))
                .collect()
        }
    };

    // Resolve dependencies: groups, keys, identities referenced by filtered connections
    let is_filtered = !matches!(options.filter, ExportFilter::All);

    let dep_group_ids: Vec<uuid::Uuid> = if is_filtered {
        filtered_connections.iter()
            .filter_map(|c| c.group_id)
            .collect()
    } else {
        all_groups.iter().map(|g| g.id).collect()
    };

    let dep_key_ids: Vec<uuid::Uuid> = if is_filtered {
        let mut ids: Vec<uuid::Uuid> = filtered_connections.iter()
            .filter_map(|c| c.key_id)
            .collect();
        // Also include keys from referenced identities
        for c in &filtered_connections {
            if let Some(iid) = c.identity_id
                && let Some(ident) = all_identities.iter().find(|i| i.id == iid)
                && let Some(kid) = ident.key_id
                && !ids.contains(&kid)
            {
                ids.push(kid);
            }
        }
        ids
    } else {
        all_keys.iter().map(|k| k.id).collect()
    };

    let dep_identity_ids: Vec<uuid::Uuid> = if is_filtered {
        filtered_connections.iter()
            .filter_map(|c| c.identity_id)
            .collect()
    } else {
        all_identities.iter().map(|i| i.id).collect()
    };

    // Filter groups
    let groups: Vec<Group> = if is_filtered {
        all_groups.into_iter()
            .filter(|g| dep_group_ids.contains(&g.id))
            .collect()
    } else {
        all_groups
    };

    // Wrap connections with decrypted passwords
    let mut connections = Vec::with_capacity(filtered_connections.len());
    for conn in filtered_connections {
        let pw = store.get_connection_password(&conn.id).unwrap_or(None);
        connections.push(ExportConnection {
            connection: conn.clone(),
            password: pw,
        });
    }

    // Wrap keys with optional private key (filtered by deps)
    let mut keys = Vec::new();
    for key in &all_keys {
        if !is_filtered || dep_key_ids.contains(&key.id) {
            let pk = if options.include_private_keys {
                store.get_key_private(&key.id).unwrap_or(None)
            } else {
                None
            };
            keys.push(ExportKey {
                key: key.clone(),
                private_key: pk,
            });
        }
    }

    // Wrap identities with decrypted passwords (filtered by deps)
    let mut identities = Vec::new();
    for ident in &all_identities {
        if !is_filtered || dep_identity_ids.contains(&ident.id) {
            let pw = store.get_identity_password(&ident.id).unwrap_or(None);
            identities.push(ExportIdentity {
                identity: ident.clone(),
                password: pw,
            });
        }
    }

    // Snippets and known_hosts: only included in full export
    let snippets = if is_filtered { Vec::new() } else { all_snippets };
    let known_hosts = if is_filtered { Vec::new() } else { all_known_hosts };

    let payload = ExportPayload {
        version: FORMAT_VERSION,
        exported_at: Utc::now(),
        includes_private_keys: options.include_private_keys,
        groups,
        connections,
        keys,
        identities,
        snippets,
        known_hosts,
    };

    let json = serde_json::to_vec(&payload)
        .map_err(|e| VaultError::Crypto(format!("Serialization failed: {}", e)))?;

    let encrypted = encrypt(&json, password.as_bytes())?;

    let flags = if options.include_private_keys { FLAG_INCLUDES_KEYS } else { 0 };
    let mut result = build_header(flags);
    result.extend_from_slice(&encrypted);
    Ok(result)
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

pub fn import_vault(
    store: &VaultStore,
    data: &[u8],
    password: &str,
) -> Result<ImportResult, VaultError> {
    let (_version, _flags) = validate_header(data)?;
    let encrypted = &data[HEADER_LEN..];
    let json_bytes = decrypt(encrypted, password.as_bytes())?;

    let payload: ExportPayload = serde_json::from_slice(&json_bytes)
        .map_err(|e| VaultError::Crypto(format!("Invalid export data: {}", e)))?;

    let mut result = ImportResult {
        connections_added: 0,
        connections_updated: 0,
        connections_skipped: 0,
        keys_added: 0,
        keys_skipped: 0,
        groups_added: 0,
        groups_skipped: 0,
        identities_added: 0,
        identities_updated: 0,
        identities_skipped: 0,
        snippets_added: 0,
        snippets_skipped: 0,
        known_hosts_added: 0,
        known_hosts_skipped: 0,
    };

    // Existing data for merge checks
    let existing_groups = store.list_groups()?;
    let existing_connections = store.list_connections()?;
    let existing_keys = store.list_keys()?;
    let existing_identities = store.list_identities()?;
    let existing_snippets = store.list_snippets()?;
    let existing_known_hosts = store.list_known_hosts()?;

    // Import order: groups → keys → identities → connections → snippets → known_hosts

    // Groups (no updated_at comparison — skip if exists)
    for group in &payload.groups {
        if existing_groups.iter().any(|g| g.id == group.id) {
            result.groups_skipped += 1;
        } else {
            store.save_group(group)?;
            result.groups_added += 1;
        }
    }

    // Keys (skip if exists)
    for export_key in &payload.keys {
        if existing_keys.iter().any(|k| k.id == export_key.key.id) {
            result.keys_skipped += 1;
        } else {
            store.save_key(&export_key.key, export_key.private_key.as_deref())?;
            result.keys_added += 1;
        }
    }

    // Identities (LWW by updated_at)
    for export_ident in &payload.identities {
        if let Some(existing) = existing_identities.iter().find(|i| i.id == export_ident.identity.id) {
            if export_ident.identity.updated_at > existing.updated_at {
                store.save_identity(&export_ident.identity, export_ident.password.as_deref())?;
                result.identities_updated += 1;
            } else {
                result.identities_skipped += 1;
            }
        } else {
            store.save_identity(&export_ident.identity, export_ident.password.as_deref())?;
            result.identities_added += 1;
        }
    }

    // Connections (LWW by updated_at)
    for export_conn in &payload.connections {
        if let Some(existing) = existing_connections.iter().find(|c| c.id == export_conn.connection.id) {
            if export_conn.connection.updated_at > existing.updated_at {
                store.save_connection(&export_conn.connection, export_conn.password.as_deref())?;
                result.connections_updated += 1;
            } else {
                result.connections_skipped += 1;
            }
        } else {
            store.save_connection(&export_conn.connection, export_conn.password.as_deref())?;
            result.connections_added += 1;
        }
    }

    // Snippets (skip if exists)
    for snippet in &payload.snippets {
        if existing_snippets.iter().any(|s| s.id == snippet.id) {
            result.snippets_skipped += 1;
        } else {
            store.save_snippet(snippet)?;
            result.snippets_added += 1;
        }
    }

    // Known hosts (skip if exists)
    for kh in &payload.known_hosts {
        if existing_known_hosts.iter().any(|k| k.id == kh.id) {
            result.known_hosts_skipped += 1;
        } else {
            store.save_known_host(kh)?;
            result.known_hosts_added += 1;
        }
    }

    Ok(result)
}

/// Check if a file looks like a valid .oryxis export (by header).
pub fn is_valid_export(data: &[u8]) -> bool {
    validate_header(data).is_ok()
}

/// Check if an export file includes private keys (from header flags).
pub fn export_includes_keys(data: &[u8]) -> bool {
    validate_header(data)
        .map(|(_, flags)| flags & FLAG_INCLUDES_KEYS != 0)
        .unwrap_or(false)
}
