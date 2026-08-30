use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use oryxis_core::models::{
    Connection, CustomTerminalTheme, Group, Identity, KnownHost, LoginScript,
    PortForwardRule, ProxyIdentity, SessionGroup, Snippet, SshKey,
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
    /// Reusable login automations referenced from connections via
    /// `login_script_id`. No secrets of their own (a step can only
    /// reference one), so the bare model travels here. Defaults to
    /// empty for backwards compat with older export files.
    #[serde(default)]
    login_scripts: Vec<LoginScript>,
    /// Portable application preferences (theme, language, terminal +
    /// SFTP prefs, AI provider/model/key, …). Device-local and
    /// security-sensitive keys are filtered out on the way in and out
    /// (see `is_portable_setting`). The `ai_api_key` value is shipped
    /// **decrypted** here so it round-trips onto the target vault's own
    /// master key, the whole payload is encrypted with the export
    /// password, so it never lands in plaintext on disk. Defaults to
    /// empty for backwards compat with export files written before this
    /// field existed.
    #[serde(default)]
    settings: Vec<ExportSetting>,
    /// User-created terminal themes (their own vault table, not a
    /// settings row). They travel with the Settings category: they are
    /// preferences, and a per-host `terminal_theme` override in a
    /// connection references them BY NAME, so a migration without them
    /// would leave hosts pointing at missing themes. Defaults to empty
    /// for backwards compat with older export files.
    #[serde(default)]
    custom_terminal_themes: Vec<CustomTerminalTheme>,
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
    /// TOTP secret from the encrypted `totp_secret` column. Defaults
    /// to None on import of older files.
    #[serde(default)]
    totp_secret: Option<String>,
    /// The credential a login script types at the asset's own prompt,
    /// from the encrypted `target_password` column. Defaults to None on
    /// import of older files.
    #[serde(default)]
    target_password: Option<String>,
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
    Snippets,
    KnownHosts,
    PortForwardRules,
    SessionGroups,
    Settings,
}

impl ExportCategory {
    /// Every category, in display order. Drives the checkbox lists in
    /// the export / import dialogs and the `all()` / `none()` helpers.
    pub const ALL: [ExportCategory; 10] = [
        ExportCategory::Connections,
        ExportCategory::Groups,
        ExportCategory::Keys,
        ExportCategory::Identities,
        ExportCategory::ProxyIdentities,
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
    // Settings whose VALUE is a trust decision, not a preference. An
    // export file is untrusted input (a picked file is not a read file),
    // and the import dialog shows a category count, never a key list, so
    // nothing on that path gives the user a chance to see one of these
    // change. They are per-device policy and stay behind.
    //
    // The service-activation reasoning below already covers the sync and
    // MCP listeners; these are the same category, found later:
    // - `download_mirror` re-points every GitHub-bound request, including
    //   the updater's release metadata, whose `html_url` the UI offers as
    //   a link. Mirrors are untrusted by design (`net_mirror`), which is
    //   exactly why choosing one must stay local.
    // - `terminal_clipboard_access` at `readwrite` hands every connected
    //   server an OSC 52 clipboard READ, the default is write-only for
    //   that reason.
    // - the `agent_server_*` four activate the local signing service and
    //   strip its per-signature confirmation.
    // - `auto_lock_minutes` disarms the idle lock; `update_channel` moves
    //   the install to another release strand; `ai_api_url` re-points the
    //   AI requests (which carry the user's own key) at another endpoint;
    //   `terminal_password_autofill` decides when a stored password is
    //   typed into a session.
    const DENY_SECURITY: &[&str] = &[
        "download_mirror",
        "terminal_clipboard_access",
        "agent_server_enabled",
        "agent_server_confirm",
        "agent_server_allow_add",
        "agent_server_openssh_pipe",
        "auto_lock_minutes",
        "update_channel",
        "ai_api_url",
        "terminal_password_autofill",
    ];
    const DENY_EXACT: &[&str] = &[
        "sync_device_identity",
        "has_user_password",
        "sync_device_name",
        "sync_listen_port",
        "sync_signaling_token",
        // Encrypted under THIS vault's master key, so the verbatim
        // value `list_settings` hands out arrives as bytes the target
        // cannot read. Inert on its own, but the next master-password
        // change walks every encrypted setting in STRICT mode
        // (`convert_all_fields`) and aborts on the one it cannot
        // decrypt, which would wedge the target's password change.
        // Same reasoning as `files_recent_folders` below.
        "sync_sftp_passphrase",
        "sync_webdav_password",
        // The sync-transport ENDPOINTS are per-device config, not vault
        // content, and the Git remote in particular can legitimately
        // carry inline credentials (`https://user:token@host/...`):
        // shipping it in an export would put that token in cleartext in
        // the portable file. WebDAV keeps its password in the encrypted
        // setting above; these are the plaintext locators next to it.
        "sync_git_remote",
        "sync_webdav_url",
        "sync_webdav_user",
        "sync_folder_path",
        "mcp_server_token",
        "sync_enabled",
        "sync_mode",
        "mcp_server_enabled",
        "skipped_update_version",
        "pinned_tabs",
        // Files-sidebar folder history: per-host browsing trail, and
        // encrypted under THIS vault's master key, so the verbatim value
        // `list_settings` hands out would arrive in the target vault as
        // undecryptable bytes. Local state like `pinned_tabs`, not a
        // preference worth carrying.
        "files_recent_folders",
    ];
    if DENY_EXACT.contains(&key) || DENY_SECURITY.contains(&key) {
        return false;
    }
    // One-time UI hints and one-shot migration / boot markers are
    // per-install bookkeeping, never a user preference worth carrying.
    if key.starts_with("hint_") || key.ends_with("_migrated") || key.ends_with("_applied") {
        return false;
    }
    true
}

#[derive(Clone, Debug)]
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
    pub snippets_added: usize,
    pub snippets_skipped: usize,
    pub port_forward_rules_added: usize,
    pub port_forward_rules_skipped: usize,
    /// Imported rules whose `auto_start` was cleared on the way in. They
    /// are counted separately so the UI can say the forwards landed but
    /// will not dial on their own until someone enables them here.
    pub port_forward_rules_disarmed: usize,
    pub known_hosts_added: usize,
    pub known_hosts_skipped: usize,
    pub session_groups_added: usize,
    pub session_groups_skipped: usize,
    pub login_scripts_added: usize,
    pub login_scripts_skipped: usize,
    /// Portable preferences written (or overwritten) on import. Settings
    /// have no `updated_at`, so an imported value always wins, hence a
    /// single counter rather than added/updated/skipped.
    pub settings_imported: usize,
    pub custom_themes_added: usize,
    pub custom_themes_skipped: usize,
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
    // A locked store still answers every list_* call below while the
    // per-field decrypts degrade to None through their unwrap_or, so
    // without this gate a caller racing the (auto-)lock would get a
    // structurally valid export with every password silently missing.
    // The UI guards its confirms too, but the invariant belongs to the
    // function that writes the file, not to whoever calls it.
    if store.is_locked() {
        return Err(VaultError::Locked);
    }
    // Collect all data from vault
    let all_groups = store.list_groups()?;
    let all_connections = store.list_connections()?;
    let all_keys = store.list_keys()?;
    let all_identities = store.list_identities()?;
    let all_proxy_identities = store.list_proxy_identities()?;
    let all_snippets = store.list_snippets()?;
    let all_port_forward_rules = store.list_port_forward_rules()?;
    let all_known_hosts = store.list_known_hosts()?;
    let all_session_groups = store.list_session_groups()?;
    let all_login_scripts = store.list_login_scripts()?;

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
            let totp = store.get_connection_totp_secret(&conn.id).unwrap_or(None);
            let target_pw = store.get_connection_target_password(&conn.id).unwrap_or(None);
            // A trust decision made about ONE appliance on ONE machine
            // is not host data: an export is a file that travels, and
            // "accept an invalid certificate" must be re-made where it
            // lands. Same rule the sync wire applies.
            let mut connection = (*conn).clone();
            connection.strip_local_trust();
            connections.push(ExportConnection {
                connection,
                password: pw,
                proxy_password: proxy_pw,
                totp_secret: totp,
                target_password: target_pw,
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

    // Login scripts ride the Connections category rather than owning a
    // checkbox of their own: a script is meaningless without a host
    // that references it, and a box the user did not notice would
    // import hosts whose automation silently resolves to nothing.
    // Host-scoped exports ship only the scripts those hosts use.
    let login_scripts: Vec<LoginScript> = if options.selection.connections {
        let dep_ids: Vec<uuid::Uuid> = filtered_connections
            .iter()
            .filter_map(|c| c.login_script_id)
            .collect();
        all_login_scripts
            .iter()
            .filter(|s| !is_filtered || dep_ids.contains(&s.id))
            .cloned()
            .collect()
    } else {
        Vec::new()
    };

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

    // Custom terminal themes ride the Settings category (they are
    // preferences; per-host `terminal_theme` overrides reference them
    // by name).
    let custom_terminal_themes = if full_export && options.selection.settings {
        store.list_custom_terminal_themes()?
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
        snippets,
        port_forward_rules,
        known_hosts,
        session_groups,
        login_scripts,
        settings,
        custom_terminal_themes,
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
        snippets: payload.snippets.len(),
        known_hosts: payload.known_hosts.len(),
        port_forward_rules: payload.port_forward_rules.len(),
        session_groups: payload.session_groups.len(),
        // Custom themes ride the Settings category, so they count
        // toward its presence: a file carrying only themes must still
        // light the Settings checkbox in the import dialog.
        settings: payload.settings.len() + payload.custom_terminal_themes.len(),
        includes_private_keys: export_includes_keys(data),
    })
}

pub fn import_vault(
    store: &VaultStore,
    data: &[u8],
    password: &str,
    selection: &ExportSelection,
) -> Result<ImportResult, VaultError> {
    // Fail before touching the database: plaintext families (groups,
    // snippets, known hosts, password-less connections) save fine on a
    // locked store, and the first encrypted field would then abort the
    // loop halfway, leaving a partial import behind.
    if store.is_locked() {
        return Err(VaultError::Locked);
    }
    let mut payload = decrypt_payload(data, password)?;

    // Drop unchecked categories up front so the existing per-entity
    // loops below don't each need a guard. Dropping a category that a
    // surviving one references (e.g. keys when connections stay) just
    // leaves a dangling id, which the app tolerates like a deleted key
    // (FK enforcement is off on the vault).
    // Login scripts ride the Connections box on the way in too, for the
    // same reason they do on the way out: they exist only to serve a
    // host, and importing hosts without them would silently drop the
    // automation the user came for.
    if !selection.connections {
        payload.connections.clear();
        payload.login_scripts.clear();
    }
    if !selection.groups { payload.groups.clear(); }
    if !selection.keys { payload.keys.clear(); }
    if !selection.identities { payload.identities.clear(); }
    if !selection.proxy_identities { payload.proxy_identities.clear(); }
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
        snippets_added: 0,
        snippets_skipped: 0,
        port_forward_rules_added: 0,
        port_forward_rules_skipped: 0,
        port_forward_rules_disarmed: 0,
        known_hosts_added: 0,
        known_hosts_skipped: 0,
        session_groups_added: 0,
        session_groups_skipped: 0,
        login_scripts_added: 0,
        login_scripts_skipped: 0,
        settings_imported: 0,
        custom_themes_added: 0,
        custom_themes_skipped: 0,
    };

    // Existing data for merge checks
    let existing_groups = store.list_groups()?;
    let existing_session_groups = store.list_session_groups()?;
    let existing_login_scripts = store.list_login_scripts()?;
    let existing_connections = store.list_connections()?;
    let existing_keys = store.list_keys()?;
    let existing_identities = store.list_identities()?;
    let existing_proxy_identities = store.list_proxy_identities()?;
    let existing_port_forward_rules = store.list_port_forward_rules()?;
    let existing_snippets = store.list_snippets()?;
    let existing_known_hosts = store.list_known_hosts()?;

    // Reconcile dangling references before writing anything. A partial
    // selection (or a hand-crafted file) can leave a connection pointing
    // at a group/key/identity that is being imported by
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

    for ec in &mut payload.connections {
        let c = &mut ec.connection;
        // A picked file is not a read file: an import may carry an
        // "accept an invalid certificate" flag from a machine whose
        // appliance this user has never seen, so it is dropped on the
        // way in as well as on the way out.
        c.strip_local_trust();
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
    }
    // Identities can reference a key; same NULL-if-absent rule.
    for ei in &mut payload.identities {
        if ei.identity.key_id.is_some_and(|id| !will_have(&payload_key_ids, &existing_key_ids, &id)) {
            ei.identity.key_id = None;
        }
    }
    // Groups carry a parent (folder tree); a dangling parent hides the
    // group.
    for g in &mut payload.groups {
        if g.parent_id.is_some_and(|id| !will_have(&payload_group_ids, &existing_group_ids, &id)) {
            g.parent_id = None;
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

    // Imported folders merge with folders already here, so the combined
    // tree can loop even when neither side did on its own (the file and
    // the vault each re-parented the other's folder). Checked once over
    // the merged result, for the same reason the sync apply path checks
    // once per batch: a partially-imported tree can look cyclic while
    // the finished one is not. Detaching to root is the same
    // degradation the dashboard already applies, made durable so the
    // stored tree stays acyclic no matter which writer produced it.
    if !payload.groups.is_empty() {
        let merged = store.list_groups()?;
        for id in Group::cycle_breakers(&merged) {
            if let Some(group) = merged.iter().find(|g| g.id == id) {
                let mut repaired = group.clone();
                repaired.parent_id = None;
                store.save_group(&repaired)?;
                tracing::warn!(
                    "import: parent cycle detected, detaching group {} ({}) to root",
                    repaired.label,
                    repaired.id
                );
            }
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
            // Persist the proxy password and TOTP secret (or clear
            // them) only when we actually wrote the connection,
            // skipped (older) entries keep their existing columns
            // intact.
            store.set_proxy_password(
                &export_conn.connection.id,
                export_conn.proxy_password.as_deref(),
            )?;
            store.set_connection_totp_secret(
                &export_conn.connection.id,
                export_conn.totp_secret.as_deref(),
            )?;
            store.set_connection_target_password(
                &export_conn.connection.id,
                export_conn.target_password.as_deref(),
            )?;
        }
    }

    // Login scripts (skip if exists). Written after the connections
    // that reference them, which is safe in either order: resolution
    // treats a missing script as no automation rather than an error.
    for script in &payload.login_scripts {
        if existing_login_scripts.iter().any(|s| s.id == script.id) {
            result.login_scripts_skipped += 1;
        } else {
            store.save_login_script(script)?;
            result.login_scripts_added += 1;
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

    // Port forward rules (skip if exists). An imported rule never
    // self-arms: `auto_start` is what turns a stored rule into a DIAL at
    // the next launch, with nobody present, and that dial resolves the
    // rule's host, proxy included. A file the user merely picked must
    // not be able to schedule that. The rule still imports, disabled,
    // and the forwards panel is where it gets turned back on by someone
    // who can see what they are enabling.
    let mut auto_start_disarmed = 0usize;
    for rule in &payload.port_forward_rules {
        if existing_port_forward_rules.iter().any(|r| r.id == rule.id) {
            result.port_forward_rules_skipped += 1;
        } else {
            let mut rule = rule.clone();
            if rule.auto_start {
                rule.auto_start = false;
                auto_start_disarmed += 1;
            }
            store.save_port_forward_rule(&rule)?;
            result.port_forward_rules_added += 1;
        }
    }
    result.port_forward_rules_disarmed = auto_start_disarmed;

    // Known hosts. A pin is a trust decision somebody made at a
    // fingerprint prompt, so an import may only introduce pins for
    // endpoints this vault has NOT pinned yet (the fresh-device
    // migration case). Dedup has to be by the SEMANTIC key, not by id:
    // `save_known_host` keeps one row per (hostname, port, key_type) and
    // DELETES the others first, so a row carrying a fresh id would not
    // be "new", it would replace the local pin with the file's
    // fingerprint (and tombstone the real one on the way out, which
    // then propagates to sync peers). Silent, and it makes the next
    // connect to that host trust whatever the file said.
    for kh in &payload.known_hosts {
        let already_pinned = existing_known_hosts.iter().any(|k| {
            k.id == kh.id
                || (k.hostname == kh.hostname
                    && k.port == kh.port
                    && k.key_type == kh.key_type)
        });
        if already_pinned {
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

    // Custom terminal themes (Settings category). Skip when the id
    // already exists (same record) or when a DIFFERENT theme already
    // owns the name: theme names are the reference key for per-host
    // overrides and must stay unique, so an import never silently
    // replaces a local theme that happens to share a name.
    if selection.settings {
        let existing_themes = store.list_custom_terminal_themes()?;
        for theme in &payload.custom_terminal_themes {
            let same_id = existing_themes.iter().any(|t| t.id == theme.id);
            let name_taken = existing_themes
                .iter()
                .any(|t| t.id != theme.id && t.name == theme.name);
            if same_id || name_taken {
                result.custom_themes_skipped += 1;
            } else {
                store.save_custom_terminal_theme(theme)?;
                result.custom_themes_added += 1;
            }
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
