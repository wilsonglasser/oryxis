use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use oryxis_core::models::{
    CloudProfile, Connection, Group, Identity, KnownHost, PortForwardRule, ProxyIdentity,
    SessionGroup, Snippet, SshKey,
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
    /// Reusable proxy configurations referenced from connections via
    /// `proxy_identity_id`. Defaults to empty for backwards compat with
    /// `.oryxis` files written before this field existed.
    #[serde(default)]
    proxy_identities: Vec<ExportProxyIdentity>,
    /// Cloud account profiles referenced from `Connection.cloud_ref` and
    /// `Group.cloud_query`. Defaults to empty for backwards compat with
    /// pre-v0.6 export files.
    #[serde(default)]
    cloud_profiles: Vec<ExportCloudProfile>,
    snippets: Vec<Snippet>,
    /// Standalone port forward rules. Defaults to empty for backwards compat
    /// with `.oryxis` files written before this field existed.
    #[serde(default)]
    port_forward_rules: Vec<PortForwardRule>,
    known_hosts: Vec<KnownHost>,
    /// Saved split-panel arrangements. No credentials (they reference hosts
    /// by id or are local shells). Defaults to empty for backwards compat
    /// with export files written before this field existed.
    #[serde(default)]
    session_groups: Vec<SessionGroup>,
    /// Portable application preferences (theme, language, terminal +
    /// SFTP + cloud prefs, AI provider/model/key, …). Device-local and
    /// security-sensitive keys are filtered out on the way in and out
    /// (see `is_portable_setting`). The `ai_api_key` value is shipped
    /// **decrypted** here so it round-trips onto the target vault's own
    /// master key, the whole payload is encrypted with the export
    /// password, so it never lands in plaintext on disk. Defaults to
    /// empty for backwards compat with export files written before this
    /// field existed.
    #[serde(default)]
    settings: Vec<ExportSetting>,
}

#[derive(Serialize, Deserialize)]
struct ExportSetting {
    key: String,
    value: String,
}

#[derive(Serialize, Deserialize)]
struct ExportConnection {
    #[serde(flatten)]
    connection: Connection,
    password: Option<String>,
    /// Proxy password from the encrypted `proxy_password` column
    /// shipped here so a portable export round-trips inline proxies
    /// with auth. Defaults to None on import of older files.
    #[serde(default)]
    proxy_password: Option<String>,
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

#[derive(Serialize, Deserialize)]
struct ExportProxyIdentity {
    #[serde(flatten)]
    proxy_identity: ProxyIdentity,
    password: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct ExportCloudProfile {
    #[serde(flatten)]
    profile: CloudProfile,
    /// Encrypted-on-disk secret blob (access key secret, kubeconfig
    /// inline contents, …), round-tripped here so a fresh device picks
    /// up working cloud credentials. `None` means no secret was set.
    secret: Option<String>,
}

// ---------------------------------------------------------------------------
// Public API types
// ---------------------------------------------------------------------------

pub struct ExportOptions {
    pub include_private_keys: bool,
    pub filter: ExportFilter,
    /// Which entity families to include. Each category is an
    /// independent toggle, dropping a dependency (e.g. exporting
    /// connections without their keys) leaves a dangling reference that
    /// the app tolerates exactly like a deleted key, FK enforcement is
    /// off on the vault so an import never errors on a missing parent.
    pub selection: ExportSelection,
}

/// The selectable entity families for a vault export / import. Mirrors
/// the sections of `ExportPayload`; `settings` rides only on a full
/// (unfiltered) export.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExportCategory {
    Connections,
    Groups,
    Keys,
    Identities,
    ProxyIdentities,
    CloudProfiles,
    Snippets,
    KnownHosts,
    PortForwardRules,
    SessionGroups,
    Settings,
}

impl ExportCategory {
    /// Every category, in display order. Drives the checkbox lists in
    /// the export / import dialogs and the `all()` / `none()` helpers.
    pub const ALL: [ExportCategory; 11] = [
        ExportCategory::Connections,
        ExportCategory::Groups,
        ExportCategory::Keys,
        ExportCategory::Identities,
        ExportCategory::ProxyIdentities,
        ExportCategory::CloudProfiles,
        ExportCategory::Snippets,
        ExportCategory::KnownHosts,
        ExportCategory::PortForwardRules,
        ExportCategory::SessionGroups,
        ExportCategory::Settings,
    ];
}

/// Per-category include flags for an export / import. Built `all()` by
/// default (the UI checks every box); the user unchecks to narrow.
#[derive(Clone, Copy, Debug)]
pub struct ExportSelection {
    pub connections: bool,
    pub groups: bool,
    pub keys: bool,
    pub identities: bool,
    pub proxy_identities: bool,
    pub cloud_profiles: bool,
    pub snippets: bool,
    pub known_hosts: bool,
    pub port_forward_rules: bool,
    pub session_groups: bool,
    pub settings: bool,
}

impl ExportSelection {
    /// Everything selected, the default for the full-export dialog and
    /// the host/group share path.
    pub fn all() -> Self {
        Self {
            connections: true,
            groups: true,
            keys: true,
            identities: true,
            proxy_identities: true,
            cloud_profiles: true,
            snippets: true,
            known_hosts: true,
            port_forward_rules: true,
            session_groups: true,
            settings: true,
        }
    }

    /// Nothing selected, the starting point when an import inspection
    /// turns categories on only for the families actually present.
    pub fn none() -> Self {
        Self {
            connections: false,
            groups: false,
            keys: false,
            identities: false,
            proxy_identities: false,
            cloud_profiles: false,
            snippets: false,
            known_hosts: false,
            port_forward_rules: false,
            session_groups: false,
            settings: false,
        }
    }

    pub fn get(&self, c: ExportCategory) -> bool {
        match c {
            ExportCategory::Connections => self.connections,
            ExportCategory::Groups => self.groups,
            ExportCategory::Keys => self.keys,
            ExportCategory::Identities => self.identities,
            ExportCategory::ProxyIdentities => self.proxy_identities,
            ExportCategory::CloudProfiles => self.cloud_profiles,
            ExportCategory::Snippets => self.snippets,
            ExportCategory::KnownHosts => self.known_hosts,
            ExportCategory::PortForwardRules => self.port_forward_rules,
            ExportCategory::SessionGroups => self.session_groups,
            ExportCategory::Settings => self.settings,
        }
    }

    pub fn set(&mut self, c: ExportCategory, v: bool) {
        match c {
            ExportCategory::Connections => self.connections = v,
            ExportCategory::Groups => self.groups = v,
            ExportCategory::Keys => self.keys = v,
            ExportCategory::Identities => self.identities = v,
            ExportCategory::ProxyIdentities => self.proxy_identities = v,
            ExportCategory::CloudProfiles => self.cloud_profiles = v,
            ExportCategory::Snippets => self.snippets = v,
            ExportCategory::KnownHosts => self.known_hosts = v,
            ExportCategory::PortForwardRules => self.port_forward_rules = v,
            ExportCategory::SessionGroups => self.session_groups = v,
            ExportCategory::Settings => self.settings = v,
        }
    }

    pub fn toggle(&mut self, c: ExportCategory) {
        self.set(c, !self.get(c));
    }
}

/// Settings keys that must never leave (or enter) a vault through a
/// portable export. They split three ways:
///
/// - **Device identity / per-vault state** that would corrupt the
///   target if cloned: the sync device identity blob, the
///   `has_user_password` lock flag, device name + listen port.
/// - **Per-device secrets stored in plaintext** in the settings table
///   (so the denylist is their only protection): the MCP and signaling
///   bearer tokens.
/// - **Service-activation toggles** that would silently flip on a
///   network listener on the importing machine (sync engine, MCP
///   server), surprising for an "import my preferences" action and
///   inconsistent with the device identity being withheld.
/// - **Per-install / transient state**: skipped update version, pinned
///   tab ids (reference local sessions), one-time hint flags and any
///   one-shot migration / `*_applied` marker.
///
/// `ai_api_key` is deliberately **not** here, it's a portable secret
/// handled specially (decrypted on export, re-encrypted on import).
pub(crate) fn is_portable_setting(key: &str) -> bool {
    const DENY_EXACT: &[&str] = &[
        "sync_device_identity",
        "has_user_password",
        "sync_device_name",
        "sync_listen_port",
        "sync_signaling_token",
        "mcp_server_token",
        "sync_enabled",
        "sync_mode",
        "mcp_server_enabled",
        "skipped_update_version",
        "pinned_tabs",
    ];
    if DENY_EXACT.contains(&key) {
        return false;
    }
    // One-time UI hints and one-shot migration / boot markers are
    // per-install bookkeeping, never a user preference worth carrying.
    if key.starts_with("hint_") || key.ends_with("_migrated") || key.ends_with("_applied") {
        return false;
    }
    true
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
    pub proxy_identities_added: usize,
    pub proxy_identities_updated: usize,
    pub proxy_identities_skipped: usize,
    pub cloud_profiles_added: usize,
    pub cloud_profiles_updated: usize,
    pub cloud_profiles_skipped: usize,
    pub snippets_added: usize,
    pub snippets_skipped: usize,
    pub port_forward_rules_added: usize,
    pub port_forward_rules_skipped: usize,
    pub known_hosts_added: usize,
    pub known_hosts_skipped: usize,
    pub session_groups_added: usize,
    pub session_groups_skipped: usize,
    /// Portable preferences written (or overwritten) on import. Settings
    /// have no `updated_at`, so an imported value always wins, hence a
    /// single counter rather than added/updated/skipped.
    pub settings_imported: usize,
}

/// Per-category contents of an export file, produced by
/// [`inspect_export`] so the import dialog can show the user exactly
/// which families are present (and how many of each) before they pick
/// what to apply. Counting requires decryption, so this carries the
/// export password just like [`import_vault`].
pub struct ExportSummary {
    pub connections: usize,
    pub groups: usize,
    pub keys: usize,
    pub identities: usize,
    pub proxy_identities: usize,
    pub cloud_profiles: usize,
    pub snippets: usize,
    pub known_hosts: usize,
    pub port_forward_rules: usize,
    pub session_groups: usize,
    pub settings: usize,
    /// Whether the file ships private key material (header flag).
    pub includes_private_keys: bool,
}

impl ExportSummary {
    /// How many records of `category` the file holds.
    pub fn count(&self, c: ExportCategory) -> usize {
        match c {
            ExportCategory::Connections => self.connections,
            ExportCategory::Groups => self.groups,
            ExportCategory::Keys => self.keys,
            ExportCategory::Identities => self.identities,
            ExportCategory::ProxyIdentities => self.proxy_identities,
            ExportCategory::CloudProfiles => self.cloud_profiles,
            ExportCategory::Snippets => self.snippets,
            ExportCategory::KnownHosts => self.known_hosts,
            ExportCategory::PortForwardRules => self.port_forward_rules,
            ExportCategory::SessionGroups => self.session_groups,
            ExportCategory::Settings => self.settings,
        }
    }

    /// Whether the file carries at least one record of `category`.
    pub fn present(&self, c: ExportCategory) -> bool {
        self.count(c) > 0
    }

    /// A selection that turns on exactly the categories present in the
    /// file, the default state when the import dialog opens its
    /// checkbox list.
    pub fn default_selection(&self) -> ExportSelection {
        let mut sel = ExportSelection::none();
        for c in ExportCategory::ALL {
            sel.set(c, self.present(c));
        }
        sel
    }
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
    let all_proxy_identities = store.list_proxy_identities()?;
    let all_cloud_profiles = store.list_cloud_profiles()?;
    let all_snippets = store.list_snippets()?;
    let all_port_forward_rules = store.list_port_forward_rules()?;
    let all_known_hosts = store.list_known_hosts()?;
    let all_session_groups = store.list_session_groups()?;

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

    // Proxy identities pulled in by `connection.proxy_identity_id`.
    let dep_proxy_identity_ids: Vec<uuid::Uuid> = if is_filtered {
        filtered_connections.iter()
            .filter_map(|c| c.proxy_identity_id)
            .collect()
    } else {
        all_proxy_identities.iter().map(|pi| pi.id).collect()
    };

    // Filter groups
    let groups: Vec<Group> = if !options.selection.groups {
        Vec::new()
    } else if is_filtered {
        all_groups.into_iter()
            .filter(|g| dep_group_ids.contains(&g.id))
            .collect()
    } else {
        all_groups
    };

    // Wrap connections with decrypted passwords. Proxy password is
    // shipped alongside so an inline-proxy host round-trips with auth
    // (it lives in its own encrypted column and isn't part of the
    // serialized `Connection.proxy` JSON). Skipped entirely when the
    // Connections category is unchecked, no point decrypting passwords
    // we won't ship.
    let mut connections = Vec::with_capacity(filtered_connections.len());
    if options.selection.connections {
        for conn in &filtered_connections {
            let pw = store.get_connection_password(&conn.id).unwrap_or(None);
            let proxy_pw = store.get_proxy_password(&conn.id).unwrap_or(None);
            connections.push(ExportConnection {
                connection: (*conn).clone(),
                password: pw,
                proxy_password: proxy_pw,
            });
        }
    }

    // Wrap keys with optional private key (filtered by deps)
    let mut keys = Vec::new();
    for key in &all_keys {
        if options.selection.keys && (!is_filtered || dep_key_ids.contains(&key.id)) {
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
        if options.selection.identities && (!is_filtered || dep_identity_ids.contains(&ident.id)) {
            let pw = store.get_identity_password(&ident.id).unwrap_or(None);
            identities.push(ExportIdentity {
                identity: ident.clone(),
                password: pw,
            });
        }
    }

    // Same shape for proxy identities, included on full export, or
    // filtered by `proxy_identity_id` references when host-scoped.
    let mut proxy_identities = Vec::new();
    for pi in &all_proxy_identities {
        if options.selection.proxy_identities && (!is_filtered || dep_proxy_identity_ids.contains(&pi.id)) {
            let pw = store.get_proxy_identity_password(&pi.id).unwrap_or(None);
            proxy_identities.push(ExportProxyIdentity {
                proxy_identity: pi.clone(),
                password: pw,
            });
        }
    }

    // Cloud profiles referenced from `Connection.cloud_ref` (filtered)
    // or all of them (full export). The dynamic-group `cloud_query`
    // path will land in the same dep set in a later PR, for now only
    // `cloud_ref` is wired.
    let dep_cloud_profile_ids: Vec<uuid::Uuid> = if is_filtered {
        filtered_connections
            .iter()
            .filter_map(|c| c.cloud_ref.as_ref().map(|r| r.profile_id))
            .collect()
    } else {
        all_cloud_profiles.iter().map(|cp| cp.id).collect()
    };
    let mut cloud_profiles = Vec::new();
    for cp in &all_cloud_profiles {
        if options.selection.cloud_profiles && (!is_filtered || dep_cloud_profile_ids.contains(&cp.id)) {
            let secret = store.get_cloud_profile_secret(&cp.id).unwrap_or(None);
            cloud_profiles.push(ExportCloudProfile {
                profile: cp.clone(),
                secret,
            });
        }
    }

    // Cross-cutting entities (snippets, port forward rules, known_hosts,
    // session groups, settings) only ship in a full export, and only
    // when their category is checked. Session groups reference hosts
    // across arbitrary folders, so a filtered subset can't carry them
    // without dangling references, same reasoning as snippets. Settings
    // are inherently vault-wide and have nothing to do with a host/group
    // share.
    let full_export = !is_filtered;
    let snippets = if full_export && options.selection.snippets { all_snippets } else { Vec::new() };
    let port_forward_rules = if full_export && options.selection.port_forward_rules { all_port_forward_rules } else { Vec::new() };
    let known_hosts = if full_export && options.selection.known_hosts { all_known_hosts } else { Vec::new() };
    let session_groups = if full_export && options.selection.session_groups { all_session_groups } else { Vec::new() };

    // Portable preferences. `ai_api_key` is stored as base64 of
    // master-key-encrypted bytes, useless to a target vault with a
    // different master key, so we substitute its decrypted value and
    // let the import path re-encrypt it. Every other portable setting
    // ships its column value verbatim. Device-local / security keys are
    // filtered by `is_portable_setting`.
    let settings: Vec<ExportSetting> = if full_export && options.selection.settings {
        let mut out = Vec::new();
        for (key, value) in store.list_settings()? {
            if !is_portable_setting(&key) {
                continue;
            }
            if key == "ai_api_key" {
                // Ship the decrypted key; skip if it can't be read
                // (corrupt / key rotated) rather than exporting an
                // undecryptable blob.
                match store.get_ai_api_key() {
                    Ok(Some(plain)) => out.push(ExportSetting { key, value: plain }),
                    _ => continue,
                }
            } else {
                out.push(ExportSetting { key, value });
            }
        }
        out
    } else {
        Vec::new()
    };

    let payload = ExportPayload {
        version: FORMAT_VERSION,
        exported_at: Utc::now(),
        includes_private_keys: options.include_private_keys,
        groups,
        connections,
        keys,
        identities,
        proxy_identities,
        cloud_profiles,
        snippets,
        port_forward_rules,
        known_hosts,
        session_groups,
        settings,
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

/// Validate the header, decrypt the body with `password` and parse the
/// payload. Shared by [`inspect_export`] (counts only) and
/// [`import_vault`] (applies records). A wrong password surfaces as
/// [`VaultError::InvalidPassword`] from `decrypt`.
fn decrypt_payload(data: &[u8], password: &str) -> Result<ExportPayload, VaultError> {
    validate_header(data)?;
    let encrypted = &data[HEADER_LEN..];
    let json_bytes = decrypt(encrypted, password.as_bytes())?;
    serde_json::from_slice(&json_bytes)
        .map_err(|e| VaultError::Crypto(format!("Invalid export data: {}", e)))
}

/// Decrypt an export and report how many records of each category it
/// holds, without writing anything. Drives the import dialog's
/// content-aware checkbox list (a category absent from the file is shown
/// disabled). The caller re-decrypts on confirm, so this never leaks the
/// parsed payload back to the UI layer.
pub fn inspect_export(data: &[u8], password: &str) -> Result<ExportSummary, VaultError> {
    let payload = decrypt_payload(data, password)?;
    Ok(ExportSummary {
        connections: payload.connections.len(),
        groups: payload.groups.len(),
        keys: payload.keys.len(),
        identities: payload.identities.len(),
        proxy_identities: payload.proxy_identities.len(),
        cloud_profiles: payload.cloud_profiles.len(),
        snippets: payload.snippets.len(),
        known_hosts: payload.known_hosts.len(),
        port_forward_rules: payload.port_forward_rules.len(),
        session_groups: payload.session_groups.len(),
        settings: payload.settings.len(),
        includes_private_keys: export_includes_keys(data),
    })
}

pub fn import_vault(
    store: &VaultStore,
    data: &[u8],
    password: &str,
    selection: &ExportSelection,
) -> Result<ImportResult, VaultError> {
    let mut payload = decrypt_payload(data, password)?;

    // Drop unchecked categories up front so the existing per-entity
    // loops below don't each need a guard. Dropping a category that a
    // surviving one references (e.g. keys when connections stay) just
    // leaves a dangling id, which the app tolerates like a deleted key
    // (FK enforcement is off on the vault).
    if !selection.connections { payload.connections.clear(); }
    if !selection.groups { payload.groups.clear(); }
    if !selection.keys { payload.keys.clear(); }
    if !selection.identities { payload.identities.clear(); }
    if !selection.proxy_identities { payload.proxy_identities.clear(); }
    if !selection.cloud_profiles { payload.cloud_profiles.clear(); }
    if !selection.snippets { payload.snippets.clear(); }
    if !selection.known_hosts { payload.known_hosts.clear(); }
    if !selection.port_forward_rules { payload.port_forward_rules.clear(); }
    if !selection.session_groups { payload.session_groups.clear(); }
    if !selection.settings { payload.settings.clear(); }

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
        proxy_identities_added: 0,
        proxy_identities_updated: 0,
        proxy_identities_skipped: 0,
        cloud_profiles_added: 0,
        cloud_profiles_updated: 0,
        cloud_profiles_skipped: 0,
        snippets_added: 0,
        snippets_skipped: 0,
        port_forward_rules_added: 0,
        port_forward_rules_skipped: 0,
        known_hosts_added: 0,
        known_hosts_skipped: 0,
        session_groups_added: 0,
        session_groups_skipped: 0,
        settings_imported: 0,
    };

    // Existing data for merge checks
    let existing_groups = store.list_groups()?;
    let existing_session_groups = store.list_session_groups()?;
    let existing_connections = store.list_connections()?;
    let existing_keys = store.list_keys()?;
    let existing_identities = store.list_identities()?;
    let existing_proxy_identities = store.list_proxy_identities()?;
    let existing_cloud_profiles = store.list_cloud_profiles()?;
    let existing_port_forward_rules = store.list_port_forward_rules()?;
    let existing_snippets = store.list_snippets()?;
    let existing_known_hosts = store.list_known_hosts()?;

    // Reconcile dangling references before writing anything. A partial
    // selection (or a hand-crafted file) can leave a connection pointing
    // at a group/key/identity/cloud profile that is being imported by
    // neither this file nor already present in the target. The app's own
    // invariant is that such a reference is NULL, not a dangling id (a
    // deleted parent cascade-NULLs its referrers), and the host list
    // relies on it: a connection with `group_id = Some(missing)` matches
    // no folder and silently vanishes from the dashboard. So we NULL any
    // reference whose target will exist in neither the payload nor the
    // vault. A reference to a parent that already lives in the target
    // (re-import of connections only) is preserved.
    let will_have = |payload_ids: &[uuid::Uuid], existing_ids: &[uuid::Uuid], id: &uuid::Uuid| {
        payload_ids.contains(id) || existing_ids.contains(id)
    };
    let payload_group_ids: Vec<uuid::Uuid> = payload.groups.iter().map(|g| g.id).collect();
    let existing_group_ids: Vec<uuid::Uuid> = existing_groups.iter().map(|g| g.id).collect();
    let payload_key_ids: Vec<uuid::Uuid> = payload.keys.iter().map(|k| k.key.id).collect();
    let existing_key_ids: Vec<uuid::Uuid> = existing_keys.iter().map(|k| k.id).collect();
    let payload_identity_ids: Vec<uuid::Uuid> = payload.identities.iter().map(|i| i.identity.id).collect();
    let existing_identity_ids: Vec<uuid::Uuid> = existing_identities.iter().map(|i| i.id).collect();
    let payload_pi_ids: Vec<uuid::Uuid> = payload.proxy_identities.iter().map(|p| p.proxy_identity.id).collect();
    let existing_pi_ids: Vec<uuid::Uuid> = existing_proxy_identities.iter().map(|p| p.id).collect();
    let payload_cp_ids: Vec<uuid::Uuid> = payload.cloud_profiles.iter().map(|c| c.profile.id).collect();
    let existing_cp_ids: Vec<uuid::Uuid> = existing_cloud_profiles.iter().map(|c| c.id).collect();

    for ec in &mut payload.connections {
        let c = &mut ec.connection;
        if c.group_id.is_some_and(|id| !will_have(&payload_group_ids, &existing_group_ids, &id)) {
            c.group_id = None;
        }
        if c.key_id.is_some_and(|id| !will_have(&payload_key_ids, &existing_key_ids, &id)) {
            c.key_id = None;
        }
        if c.identity_id.is_some_and(|id| !will_have(&payload_identity_ids, &existing_identity_ids, &id)) {
            c.identity_id = None;
        }
        if c.proxy_identity_id.is_some_and(|id| !will_have(&payload_pi_ids, &existing_pi_ids, &id)) {
            c.proxy_identity_id = None;
        }
        if c.cloud_ref.as_ref().is_some_and(|r| !will_have(&payload_cp_ids, &existing_cp_ids, &r.profile_id)) {
            c.cloud_ref = None;
        }
    }
    // Identities can reference a key; same NULL-if-absent rule.
    for ei in &mut payload.identities {
        if ei.identity.key_id.is_some_and(|id| !will_have(&payload_key_ids, &existing_key_ids, &id)) {
            ei.identity.key_id = None;
        }
    }
    // Groups carry a parent (folder tree) and an optional dynamic cloud
    // query. A dangling parent hides the group the same way; a dangling
    // query profile breaks discovery.
    for g in &mut payload.groups {
        if g.parent_id.is_some_and(|id| !will_have(&payload_group_ids, &existing_group_ids, &id)) {
            g.parent_id = None;
        }
        if g.cloud_query.as_ref().is_some_and(|q| !will_have(&payload_cp_ids, &existing_cp_ids, &q.profile_id)) {
            g.cloud_query = None;
        }
    }
    // Session groups live inside a folder by `group_id`.
    for sg in &mut payload.session_groups {
        if sg.group_id.is_some_and(|id| !will_have(&payload_group_ids, &existing_group_ids, &id)) {
            sg.group_id = None;
        }
    }

    // Import order: groups → keys → identities → connections → snippets → known_hosts

    // Groups (no updated_at comparison, skip if exists)
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

    // Proxy identities (LWW by updated_at), must come before
    // connections so `proxy_identity_id` references resolve once the
    // connections land in the next loop.
    for export_pi in &payload.proxy_identities {
        if let Some(existing) = existing_proxy_identities
            .iter()
            .find(|p| p.id == export_pi.proxy_identity.id)
        {
            if export_pi.proxy_identity.updated_at > existing.updated_at {
                store.save_proxy_identity(
                    &export_pi.proxy_identity,
                    export_pi.password.as_deref(),
                )?;
                result.proxy_identities_updated += 1;
            } else {
                result.proxy_identities_skipped += 1;
            }
        } else {
            store.save_proxy_identity(
                &export_pi.proxy_identity,
                export_pi.password.as_deref(),
            )?;
            result.proxy_identities_added += 1;
        }
    }

    // Cloud profiles (LWW by updated_at), must come before connections
    // so `cloud_ref.profile_id` references resolve once the connections
    // land in the next loop. Same pattern as proxy identities above.
    for export_cp in &payload.cloud_profiles {
        if let Some(existing) = existing_cloud_profiles
            .iter()
            .find(|p| p.id == export_cp.profile.id)
        {
            if export_cp.profile.updated_at > existing.updated_at {
                store.save_cloud_profile(&export_cp.profile, export_cp.secret.as_deref())?;
                result.cloud_profiles_updated += 1;
            } else {
                result.cloud_profiles_skipped += 1;
            }
        } else {
            store.save_cloud_profile(&export_cp.profile, export_cp.secret.as_deref())?;
            result.cloud_profiles_added += 1;
        }
    }

    // Connections (LWW by updated_at). After save, restore the proxy
    // password into its own encrypted column, `save_connection` only
    // touches the main connection password.
    for export_conn in &payload.connections {
        let added_or_updated = if let Some(existing) = existing_connections
            .iter()
            .find(|c| c.id == export_conn.connection.id)
        {
            if export_conn.connection.updated_at > existing.updated_at {
                store.save_connection(&export_conn.connection, export_conn.password.as_deref())?;
                result.connections_updated += 1;
                true
            } else {
                result.connections_skipped += 1;
                false
            }
        } else {
            store.save_connection(&export_conn.connection, export_conn.password.as_deref())?;
            result.connections_added += 1;
            true
        };
        if added_or_updated {
            // Persist the proxy password (or clear it) only when we
            // actually wrote the connection, skipped (older) entries
            // keep their existing column intact.
            store.set_proxy_password(
                &export_conn.connection.id,
                export_conn.proxy_password.as_deref(),
            )?;
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

    // Port forward rules (skip if exists)
    for rule in &payload.port_forward_rules {
        if existing_port_forward_rules.iter().any(|r| r.id == rule.id) {
            result.port_forward_rules_skipped += 1;
        } else {
            store.save_port_forward_rule(rule)?;
            result.port_forward_rules_added += 1;
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

    // Session groups (skip if exists). No credentials; their host references
    // are by id and resolve against whatever hosts the import brought in.
    for sg in &payload.session_groups {
        if existing_session_groups.iter().any(|g| g.id == sg.id) {
            result.session_groups_skipped += 1;
        } else {
            store.save_session_group(sg)?;
            result.session_groups_added += 1;
        }
    }

    // Settings (overwrite, no `updated_at` to compare). The denylist is
    // re-applied here, an export file is untrusted input and a
    // hand-crafted or older one could carry a device-identity / lock
    // flag that must never land in this vault. `ai_api_key` arrives
    // decrypted and is routed back through `set_ai_api_key` so it's
    // re-encrypted under this vault's master key.
    for setting in &payload.settings {
        if !is_portable_setting(&setting.key) {
            continue;
        }
        if setting.key == "ai_api_key" {
            store.set_ai_api_key(&setting.value)?;
        } else {
            store.set_setting(&setting.key, &setting.value)?;
        }
        result.settings_imported += 1;
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
