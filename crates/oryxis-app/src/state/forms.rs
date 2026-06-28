//! Connection / session-group editor forms (split out of `state.rs`).

use super::*;

/// Add / edit form for a local terminal, shown in a modal from the
/// Settings → Terminal card. `args` is a single space-separated string
/// here and split on submit.
#[derive(Debug, Clone, Default)]
pub(crate) struct LocalTerminalForm {
    /// `Some` when editing an existing entry (update in place); `None`
    /// when adding a new one.
    pub editing_id: Option<Uuid>,
    pub label: String,
    pub program: String,
    pub args: String,
    /// `#RRGGBB` accent override chosen via the icon picker.
    pub color: Option<String>,
    /// Icon id chosen via the icon picker.
    pub icon: Option<String>,
    /// Inline validation error (i18n key), shown under the form on a bad submit.
    pub error: Option<&'static str>,
}

/// One editable row in the session-group editor: a pane's display label
/// (read-only) plus its per-pane initial script. Rows are ordered the same
/// as the layout's leaf walk, so scripts merge back by index on save.
#[derive(Debug, Clone, Default)]
pub(crate) struct PaneScriptRow {
    /// Read-only label for the pane ("user@host", "Local Shell", ...).
    pub label: String,
    /// Per-pane initial script (override-with-fallback).
    pub script: String,
}

/// Session-group editor form state. The structural `layout` is snapshotted
/// from the tab when the editor opens; `pane_rows` exposes each leaf's script
/// for editing and merges back into the layout (by leaf order) on save.
#[derive(Debug, Clone, Default)]
pub(crate) struct SessionGroupForm {
    pub label: String,
    /// Folder (Group) label, same convention as `ConnectionForm.group_name`.
    pub group_name: String,
    pub color: Option<String>,
    pub icon_style: Option<String>,
    /// Some when editing an existing session group (update in place).
    pub editing_id: Option<Uuid>,
    /// Index of the tab this group was snapshotted from, so saving can stamp
    /// its `session_group_id`.
    pub source_tab: Option<usize>,
    /// Structural snapshot of the split tree. Leaf scripts are placeholders
    /// here; the live values live in `pane_rows` and merge back on save.
    pub layout: Option<oryxis_core::models::PaneLayout>,
    pub pane_rows: Vec<PaneScriptRow>,
    /// Which pane's script is currently shown in the editor (the chevrons
    /// step this). The live multi-line buffer for it lives in
    /// `Oryxis::session_group_script_editor` (text_editor::Content isn't
    /// Clone, so it can't sit in this form struct).
    pub current_pane: usize,
}

/// Connection editor form state.
#[derive(Debug, Clone)]
pub(crate) struct ConnectionForm {
    pub label: String,
    pub hostname: String,
    pub port: String,
    pub username: String,
    pub password: String,
    pub auth_method: AuthMethod,
    pub group_name: String,
    pub selected_key: Option<String>,
    /// Ordered jump-host chain (connection ids). The session tunnels
    /// through each hop in order before reaching this host. Mirrors
    /// `Connection.jump_chain` one-to-one; edited via the chain editor.
    pub jump_chain: Vec<Uuid>,
    /// Selected identity label (if any).
    pub selected_identity: Option<String>,
    /// If editing, the connection ID.
    pub editing_id: Option<Uuid>,
    /// Whether the connection already has a password stored in the vault.
    pub has_existing_password: bool,
    /// Whether the user has modified the password field.
    pub password_touched: bool,
    /// Whether to show the password in plain text.
    pub password_visible: bool,
    /// Whether the username field is focused (shows identity autocomplete).
    pub username_focused: bool,
    /// Port forwarding rules (local -L style).
    pub port_forwards: Vec<PortForwardForm>,
    pub env_vars: Vec<EnvVarForm>,
    /// Whether this host is exposed via MCP.
    pub mcp_enabled: bool,
    /// Forward the local ssh-agent socket to the remote shell. See the
    /// matching field on `Connection`.
    pub agent_forwarding: bool,
    /// Per-host session-recording override. `None` follows the global
    /// setting; `Some(true)`/`Some(false)` force on/off. See the matching
    /// field on `Connection`.
    pub session_logging: Option<bool>,
    /// Proxy kind selection (None = disabled). The picker stores the
    /// typed enum so language switches don't break selection identity.
    pub proxy_kind: ProxyKind,
    pub proxy_host: String,
    pub proxy_port: String,
    pub proxy_username: String,
    pub proxy_password: String,
    pub proxy_command: String,
    /// Mirrors `has_existing_password` / `password_touched`: avoids
    /// pre-loading the encrypted proxy password into form state on edit
    /// and lets save distinguish "preserve" from "explicitly cleared".
    pub has_existing_proxy_password: bool,
    pub proxy_password_touched: bool,
    /// Per-host terminal palette override. `None` means "inherit the
    /// global pick"; `Some(name)` pins this host to the named palette.
    /// Mirrors `Connection.terminal_theme` while the editor is open.
    pub terminal_theme: Option<String>,
    /// Per-host SSH keepalive override (raw text). Empty string means
    /// inherit the global setting; "0" disables keepalive on this host;
    /// any positive integer overrides the global value. Stored as a
    /// string while the editor is open so the input field can show
    /// what the user typed; serialized to `Option<u32>` on save.
    pub keepalive_interval: String,
    /// Per-host auto-title (OSC 0/2) override. Mirrors `Connection.auto_title`:
    /// `None` inherits the global setting, `Some(true/false)` forces it on/off
    /// for this host.
    pub auto_title: Option<bool>,
    /// Cloud-managed transport selection. Only meaningful when the
    /// connection being edited has a `cloud_ref`, the editor renders
    /// the picker conditionally. `None` here = "no cloud_ref to
    /// edit". The actual `cloud_ref.transport_pref` field is
    /// preserved when the user doesn't touch this picker.
    pub cloud_transport:
        Option<oryxis_core::models::cloud::TransportKind>,
    /// Per-host icon shape override. `None` falls back to the global
    /// `default_host_icon` setting. Mirrors `Connection.icon_style`.
    pub icon_style: Option<String>,
    pub encoding: Option<String>,
    /// Mirrors `Connection.terminal_type`; `None` = default `xterm-256color`.
    pub terminal_type: Option<String>,
    /// Per-host SSH algorithm overrides (legacy ciphers). `None` = Auto
    /// (russh defaults); `Some(list)` pins exactly those wire names.
    /// Mirror `Connection.{ciphers,kex,macs,host_key_algorithms}`.
    pub ciphers: Option<Vec<String>>,
    pub kex: Option<Vec<String>>,
    pub macs: Option<Vec<String>>,
    pub host_key_algorithms: Option<Vec<String>>,
    /// Per-host Privacy Mode override. Mirrors `Connection.privacy_mode`:
    /// `None` inherits the global setting, `Some(true/false)` forces it
    /// on/off for this host.
    pub privacy_mode: Option<bool>,
}

/// One SSH algorithm negotiation category, used to drive the per-host
/// override UI generically (one block per category).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AlgoCategory {
    Cipher,
    Kex,
    Mac,
    HostKey,
}

impl AlgoCategory {
    pub(crate) const ALL: [AlgoCategory; 4] =
        [Self::Cipher, Self::Kex, Self::Mac, Self::HostKey];

    /// All algorithm names selectable for this category (incl. legacy).
    pub(crate) fn supported(self) -> Vec<&'static str> {
        match self {
            Self::Cipher => oryxis_ssh::algorithms::supported_ciphers(),
            Self::Kex => oryxis_ssh::algorithms::supported_kex(),
            Self::Mac => oryxis_ssh::algorithms::supported_macs(),
            Self::HostKey => oryxis_ssh::algorithms::supported_host_keys(),
        }
    }

    /// The safe default subset (used to seed a fresh custom pin).
    pub(crate) fn defaults(self) -> Vec<String> {
        let v = match self {
            Self::Cipher => oryxis_ssh::algorithms::default_ciphers(),
            Self::Kex => oryxis_ssh::algorithms::default_kex(),
            Self::Mac => oryxis_ssh::algorithms::default_macs(),
            Self::HostKey => oryxis_ssh::algorithms::default_host_keys(),
        };
        v.into_iter().map(|s| s.to_string()).collect()
    }

    /// i18n key for the category's section label.
    pub(crate) fn label_key(self) -> &'static str {
        match self {
            Self::Cipher => "algo_ciphers",
            Self::Kex => "algo_kex",
            Self::Mac => "algo_macs",
            Self::HostKey => "algo_host_keys",
        }
    }
}

impl ConnectionForm {
    pub(crate) fn algo_list(&self, cat: AlgoCategory) -> &Option<Vec<String>> {
        match cat {
            AlgoCategory::Cipher => &self.ciphers,
            AlgoCategory::Kex => &self.kex,
            AlgoCategory::Mac => &self.macs,
            AlgoCategory::HostKey => &self.host_key_algorithms,
        }
    }

    pub(crate) fn algo_list_mut(&mut self, cat: AlgoCategory) -> &mut Option<Vec<String>> {
        match cat {
            AlgoCategory::Cipher => &mut self.ciphers,
            AlgoCategory::Kex => &mut self.kex,
            AlgoCategory::Mac => &mut self.macs,
            AlgoCategory::HostKey => &mut self.host_key_algorithms,
        }
    }
}

/// UI-side proxy kind. Includes a `None` (disabled) variant, the
/// model's `ProxyType` doesn't have a "disabled" since that's
/// represented by `Connection.proxy = None`. The `Identity(Uuid)`
/// variant points at a saved `ProxyIdentity`; when present, the
/// connection's `proxy_identity_id` is stored instead of an inline
/// `ProxyConfig`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProxyKind {
    None,
    Socks5,
    Socks4,
    Http,
    Command,
    Identity(Uuid),
}

impl ProxyKind {
    /// The static (non-identity) variants, in picker display order.
    /// Used as the base of the editor's proxy picker; the host panel
    /// concatenates the user's saved proxy identities afterwards.
    pub const STATIC: &[ProxyKind] = &[
        ProxyKind::None,
        ProxyKind::Socks5,
        ProxyKind::Socks4,
        ProxyKind::Http,
        ProxyKind::Command,
    ];

    /// i18n key for the localized label rendered in the picker. `None`
    /// is returned for `Identity(_)`, saved-identity rendering uses
    /// the identity's `label`, not a static key.
    pub fn label_key(&self) -> Option<&'static str> {
        match self {
            ProxyKind::None => Some("proxy_type_none"),
            ProxyKind::Socks5 => Some("proxy_type_socks5"),
            ProxyKind::Socks4 => Some("proxy_type_socks4"),
            ProxyKind::Http => Some("proxy_type_http"),
            ProxyKind::Command => Some("proxy_type_command"),
            ProxyKind::Identity(_) => None,
        }
    }

    /// Default port for the proxy type, pre-filled when the user
    /// switches kind and the port field is still empty.
    pub fn default_port(&self) -> Option<u16> {
        match self {
            ProxyKind::Socks5 | ProxyKind::Socks4 => Some(1080),
            ProxyKind::Http => Some(8080),
            ProxyKind::None | ProxyKind::Command | ProxyKind::Identity(_) => None,
        }
    }

    /// Whether the host/port/username trio applies. `Command` runs a
    /// process directly, `None` disables the proxy, and `Identity`
    /// pulls those fields from the saved identity instead.
    pub fn needs_endpoint(&self) -> bool {
        matches!(self, ProxyKind::Socks5 | ProxyKind::Socks4 | ProxyKind::Http)
    }

    /// Whether a password field makes sense. SOCKS4 has no password
    /// concept; Command, None and Identity don't either (Identity
    /// edits its password in the saved-identity form).
    pub fn supports_password(&self) -> bool {
        matches!(self, ProxyKind::Socks5 | ProxyKind::Http)
    }
}

impl std::fmt::Display for ProxyKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Localized at render time. The picker compares variants via
        // PartialEq, so language switches do not invalidate the
        // selected value. `Identity(_)` falls back to a generic label
        //, the host panel installs a custom mapper that swaps in the
        // identity's user-chosen label at render time.
        match self.label_key() {
            Some(k) => write!(f, "{}", crate::i18n::t(k)),
            None => write!(f, "{}", crate::i18n::t("proxy_type_identity_fallback")),
        }
    }
}

/// Transient state for the "Share / Export hosts" dialog. The dialog-open
/// flag stays on `Oryxis` (`show_share_dialog`); this groups everything the
/// dialog edits. In group mode the effective `filter` is computed from the
/// ticked `groups` (+ `include_ungrouped`) on confirm; a single-host share
/// sets `filter` directly.
#[derive(Debug, Clone, Default)]
pub(crate) struct ShareForm {
    pub password: String,
    pub include_keys: bool,
    pub filter: Option<oryxis_vault::ExportFilter>,
    pub status: Option<Result<String, String>>,
    /// Default file name suggested in the save dialog, derived from the
    /// connection label (single host) or group label.
    pub suggested_name: Option<String>,
    /// True when opened via "Export hosts…" (renders the per-folder
    /// include/exclude checklist); false for a single-host share.
    pub group_mode: bool,
    /// Folders whose hosts are included in a group-mode export.
    pub groups: std::collections::HashSet<uuid::Uuid>,
    /// Whether ungrouped (root) hosts are included in a group-mode export.
    pub include_ungrouped: bool,
}

/// Add / edit form for a saved identity (username + optional password /
/// key), shown in the keychain editor panel. The saved list lives in
/// `Oryxis::identities`; this is editor state only. Password follows the
/// tri-state convention (see [`ProxyIdentityForm`]).
#[derive(Debug, Clone, Default)]
pub(crate) struct IdentityForm {
    pub label: String,
    pub username: String,
    pub password: String,
    /// Selected SSH key label, when the identity authenticates by key.
    pub key: Option<String>,
    pub password_visible: bool,
    pub password_touched: bool,
    pub has_existing_password: bool,
    /// `Some` when editing an existing identity (update in place).
    pub editing_id: Option<Uuid>,
}

/// Add / edit form for a saved proxy identity, shown inline in the
/// Settings → Proxies section. State is in-memory only until
/// `SaveProxyIdentity` flushes it to the vault. The saved list itself
/// lives in `Oryxis::proxy_identities` (this is form state only).
///
/// Password follows the tri-state convention: `has_existing_password`
/// records whether the stored row carries one, `password_touched`
/// tracks whether the user edited the field this session, so save can
/// distinguish "leave as-is" from "clear" from "set".
#[derive(Debug, Clone)]
pub(crate) struct ProxyIdentityForm {
    /// Whether the inline editor is currently shown.
    pub visible: bool,
    pub label: String,
    pub kind: ProxyKind,
    pub host: String,
    pub port: String,
    pub username: String,
    pub password: String,
    pub password_visible: bool,
    pub password_touched: bool,
    pub has_existing_password: bool,
    /// `Some` when editing an existing identity (update in place); `None`
    /// when adding a new one.
    pub editing_id: Option<Uuid>,
    /// Inline validation error, shown under the form on a bad submit.
    pub error: Option<String>,
}

impl Default for ProxyIdentityForm {
    fn default() -> Self {
        Self {
            visible: false,
            label: String::new(),
            // SOCKS5 is the most common proxy kind, matching the host
            // editor's default proxy selection.
            kind: ProxyKind::Socks5,
            host: String::new(),
            port: String::new(),
            username: String::new(),
            password: String::new(),
            password_visible: false,
            password_touched: false,
            has_existing_password: false,
            editing_id: None,
            error: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PortForwardForm {
    pub local_port: String,
    pub remote_host: String,
    pub remote_port: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct EnvVarForm {
    pub key: String,
    pub value: String,
}

impl Default for ConnectionForm {
    fn default() -> Self {
        Self {
            label: String::new(),
            hostname: String::new(),
            port: "22".into(),
            username: String::new(),
            password: String::new(),
            auth_method: AuthMethod::Auto,
            group_name: String::new(),
            selected_key: None,
            jump_chain: Vec::new(),
            selected_identity: None,
            editing_id: None,
            has_existing_password: false,
            password_touched: false,
            password_visible: false,
            username_focused: false,
            port_forwards: Vec::new(),
            env_vars: Vec::new(),
            mcp_enabled: true,
            agent_forwarding: false,
            session_logging: None,
            proxy_kind: ProxyKind::None,
            proxy_host: String::new(),
            proxy_port: String::new(),
            proxy_username: String::new(),
            proxy_password: String::new(),
            proxy_command: String::new(),
            has_existing_proxy_password: false,
            proxy_password_touched: false,
            terminal_theme: None,
            keepalive_interval: String::new(),
            auto_title: None,
            cloud_transport: None,
            icon_style: None,
            encoding: None,
            terminal_type: None,
            ciphers: None,
            kex: None,
            macs: None,
            host_key_algorithms: None,
            privacy_mode: None,
        }
    }
}
