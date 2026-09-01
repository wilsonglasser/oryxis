//! Connection / session-group editor forms (split out of `state.rs`).

use zeroize::Zeroize;

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
    /// Comma-separated tags as typed; parsed on save (host-tag rules).
    pub tags: String,
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

/// A secret-bearing text buffer that makes the vault's tri-state
/// password contract structural instead of a copy-pasted `String` +
/// `touched: bool` pair in every editor form.
///
/// The vault convention (every password setter in `VaultStore`, e.g.
/// `save_connection` / `save_identity` / `save_proxy_identity`): the
/// `Option` handed to a save call means
/// - `None`: preserve the stored secret untouched,
/// - `Some("")`: clear the stored secret,
/// - `Some(pw)`: encrypt + store `pw`.
///
/// [`resolve`](Self::resolve) derives that `Option` from the buffer:
/// an unedited field resolves to `None` (preserve), an edited-empty
/// field to `Some("")` (clear), an edited non-empty field to
/// `Some(value)` (store). The fields are private so the only path
/// that marks the buffer edited is [`set`](Self::set) (the `*Changed`
/// message arms), and the only ways back are [`clear`](Self::clear) /
/// [`prefill`](Self::prefill) (form open / reset / hydration).
///
/// Every replacement of the buffer zeroizes what it displaces, and so
/// does the drop, because the eye toggle now decrypts STORED secrets
/// into it: a plain `self.value = other` would hand the old
/// allocation back to the allocator with the plaintext still in it.
/// This covers the buffer only. The `text_input` the buffer is bound
/// to keeps its own copy of the value for rendering, and that copy is
/// beyond our reach, so this narrows the exposure rather than closing
/// it.
#[derive(Clone, Default)]
pub(crate) struct SecretInput {
    value: String,
    touched: bool,
}

/// Redacted by hand: the derived `Debug` would print the plaintext
/// into any log line or panic message that formats a form. The
/// touched flag is the only part worth debugging, and it is not
/// secret.
impl std::fmt::Debug for SecretInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretInput")
            .field("value", &"<redacted>")
            .field("touched", &self.touched)
            .finish()
    }
}

impl Drop for SecretInput {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}

impl SecretInput {
    /// User edit: assign the buffer and mark it touched, so a later
    /// [`resolve`](Self::resolve) writes (or clears) the vault value.
    pub fn set(&mut self, value: String) {
        self.value.zeroize();
        self.value = value;
        self.touched = true;
    }

    /// Seed the buffer WITHOUT marking it touched: an untouched field
    /// still resolves to `None` (preserve the stored secret). This is
    /// the hydration primitive; [`clear`](Self::clear) is its
    /// empty-string shorthand.
    pub fn prefill(&mut self, value: String) {
        self.value.zeroize();
        self.value = value;
        self.touched = false;
    }

    /// Form open / reset / sweep: empty the buffer and forget any
    /// edit, back to the "preserve the stored secret" state.
    pub fn clear(&mut self) {
        self.prefill(String::new());
    }

    /// The tri-state vault argument: `None` preserve, `Some("")`
    /// clear, `Some(pw)` store. Exactly the contract every password
    /// save call in the vault API expects.
    pub fn resolve(&self) -> Option<&str> {
        self.touched.then_some(self.value.as_str())
    }

    /// The raw buffer, for binding to a `text_input` value.
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Whether the user edited the field this session. Drives the
    /// masked "existing secret" placeholders in the views.
    pub fn touched(&self) -> bool {
        self.touched
    }
}

/// Which template the inline "new login script" sub-form starts from.
/// Both expand to the same three-step shape; JumpServer just arrives
/// pre-filled with what KoKo prints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ScriptTemplate {
    /// Any menu-driven jump box; the user edits the three prompts.
    #[default]
    Bastion,
    /// JumpServer / KoKo, pre-filled.
    JumpServer,
}

/// Which of the draft's three prompt patterns an edit targets.
///
/// One discriminant instead of three near-identical messages, which
/// also keeps the word "password" out of a message name: these hold the
/// text the BASTION prints, never anything we type back, and a variant
/// called `...PasswordChanged(String)` is exactly what the
/// `secret_bearing_variants_carry_redacted` guard exists to catch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScriptPromptField {
    Asset,
    User,
    Credential,
}

/// Inline sub-form for creating a login script from the host editor.
/// The full step editor lives in Settings; this covers the shape every
/// interactive bastion in the field actually has, which is what keeps
/// the common case from being an authoring exercise.
#[derive(Debug, Clone)]
pub(crate) struct LoginScriptDraft {
    pub name: String,
    pub template: ScriptTemplate,
    pub asset_prompt: String,
    pub user_prompt: String,
    pub password_prompt: String,
}

impl LoginScriptDraft {
    pub fn new(template: ScriptTemplate) -> Self {
        let preset = match template {
            ScriptTemplate::Bastion => {
                oryxis_core::login_script::BastionPreset::generic()
            }
            ScriptTemplate::JumpServer => {
                oryxis_core::login_script::BastionPreset::jumpserver()
            }
        };
        Self {
            name: String::new(),
            template,
            asset_prompt: preset.asset_prompt,
            user_prompt: preset.user_prompt,
            password_prompt: preset.password_prompt,
        }
    }

    pub fn preset(&self) -> oryxis_core::login_script::BastionPreset {
        oryxis_core::login_script::BastionPreset {
            asset_prompt: self.asset_prompt.clone(),
            user_prompt: self.user_prompt.clone(),
            password_prompt: self.password_prompt.clone(),
        }
    }
}

/// Connection editor form state.
#[derive(Debug, Clone)]
pub(crate) struct ConnectionForm {
    pub label: String,
    /// Wire protocol picked in the editor. Drives the reduced Telnet /
    /// Serial forms (hide every SSH-only field) and, on save,
    /// `Connection.protocol`.
    pub protocol: oryxis_core::models::connection::ConnectionProtocol,
    /// Serial-line parameters shown by the reduced Serial form. `None`
    /// until the host becomes Serial, then materialized to defaults.
    /// Saved onto `Connection.serial` only when `protocol` is Serial.
    pub serial: Option<oryxis_core::models::serial::SerialParams>,
    /// Remote-desktop kind (RDP/VNC), shown by the reduced RemoteDesktop
    /// form. Saved to `Connection.rd_kind` when `protocol` is
    /// RemoteDesktop.
    pub rd_kind: oryxis_core::models::remote_desktop::RemoteDesktopKind,
    /// SSH host to tunnel the remote-desktop connection through, or
    /// `None` for a direct connection. Saved to `Connection.rd_gateway_id`.
    pub rd_gateway_id: Option<uuid::Uuid>,
    /// Telnet over TLS (`telnets`), shown by the reduced Telnet form.
    /// Saved into `Connection.telnet` when `protocol` is Telnet.
    pub telnet_tls: bool,
    /// Accept a server certificate the trust store rejects. Only
    /// meaningful (and only shown) while `telnet_tls` is on.
    pub telnet_tls_insecure: bool,
    /// Carry this SSH host's session over mosh. The three below are
    /// meaningful (and only shown) while it is on, and they are KEPT
    /// when it goes off: a server path somebody had to look up is not
    /// something to make them find again.
    pub mosh_enabled: bool,
    /// Where `mosh-server` lives on the host. Empty means find it on
    /// `PATH`.
    pub mosh_server_path: String,
    /// UDP ports the server may bind, in mosh's `-p` spelling. Empty
    /// lets it choose from its own range.
    pub mosh_port_range: String,
    /// What to run instead of the login shell. Not the connection's
    /// startup command: that one is typed at a shell once it is up,
    /// this one REPLACES the shell and is what survives a disconnect.
    pub mosh_command: String,
    /// Which curated local terminal a Local host spawns, or `None` for
    /// the user's default shell. Saved into `Connection.local`.
    pub local_terminal_id: Option<uuid::Uuid>,
    /// Working directory a Local host starts in (`~` allowed). Empty =
    /// the process default.
    pub local_cwd: String,
    /// Outbound address-family preference (Auto / IPv4 / IPv6), shown in
    /// SSH > Network. Saved to `Connection.address_family`.
    pub address_family: oryxis_core::models::connection::AddressFamily,
    /// Editor opened from a quick connect's progress screen ("Edit
    /// host"): the flow edits the TEMPORARY host, so Connect (without
    /// saving) takes the primary footer slot and Save the secondary.
    pub quick_flow: bool,
    pub hostname: String,
    pub port: String,
    pub username: String,
    /// Connection password buffer; tri-state per [`SecretInput`].
    pub password: SecretInput,
    pub auth_method: AuthMethod,
    pub group_name: String,
    /// Comma-separated tags as typed; parsed (trim/dedup/drop-empty)
    /// into `Connection.tags` on save. Feeds the snippet sidebar's
    /// filter-by-host-tags toggle.
    pub tags_text: String,
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
    /// Whether to show the password in plain text.
    pub password_visible: bool,
    /// Whether the username field is focused (shows identity autocomplete).
    pub username_focused: bool,
    /// Port forwarding rules (local -L style).
    pub port_forwards: Vec<PortForwardForm>,
    pub env_vars: Vec<EnvVarForm>,
    /// Whether this host is exposed via MCP.
    pub mcp_enabled: bool,
    /// Opt-in agentless monitoring (issue #83), mirrored from
    /// `Connection.monitor_enabled`.
    pub monitor_enabled: bool,
    /// Custom disk selection is ON for this host (issue #135), i.e.
    /// `Connection.monitor_disks` is `Some`. Kept apart from the list so
    /// the editor can hold an empty Custom list ("report no disks")
    /// without it collapsing back into Auto on save.
    pub monitor_disks_custom: bool,
    /// The mount patterns behind that choice, one per editor row.
    /// Meaningless while `monitor_disks_custom` is false, and preserved
    /// across a toggle so flipping to Auto and back doesn't lose what
    /// the user typed.
    pub monitor_disks: Vec<String>,
    /// Forward the local ssh-agent socket to the remote shell. See the
    /// matching field on `Connection`.
    pub agent_forwarding: bool,
    /// Forward X11 so remote GUI apps draw on the local display. See the
    /// matching field on `Connection`.
    pub x11_forwarding: bool,
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
    /// Inline proxy password buffer; tri-state per [`SecretInput`].
    pub proxy_password: SecretInput,
    pub proxy_command: String,
    /// Mirrors `has_existing_password`: avoids pre-loading the
    /// encrypted proxy password into form state on edit and lets save
    /// distinguish "preserve" from "explicitly cleared".
    pub has_existing_proxy_password: bool,
    /// Whether the proxy password is shown in plain text. Lives on the
    /// form, like the other three eyes, so opening another host starts
    /// masked: the shared `revealed_secrets` set survives a form swap,
    /// which left this one eye armed over an empty buffer.
    pub proxy_password_visible: bool,
    /// TOTP secret input (bare Base32 or an otpauth:// URI) feeding the
    /// keyboard-interactive 2FA autofill. Same tri-state discipline as
    /// the passwords above.
    pub totp_secret: SecretInput,
    pub has_existing_totp: bool,
    pub totp_visible: bool,
    /// "Use TOTP" disclosure: the secret field only renders while this
    /// is on. Seeded from `has_existing_totp` on edit; turning it off
    /// clears any stored secret on save.
    pub use_totp: bool,
    /// "Use a key from ~/.ssh" opt-in, mirroring
    /// `Connection.use_disk_key`.
    pub use_disk_key: bool,
    /// The `IdentityFile` path, as typed. Empty = scan the default
    /// OpenSSH names, which is what the field's placeholder says.
    pub identity_file: String,
    /// What the two fields above resolve to right now, recomputed on
    /// the arms that can change it rather than per frame: the answer
    /// costs a file read, and `view()` runs on every one.
    pub disk_key_status: oryxis_vault::DiskKeyStatus,
    /// Per-host terminal palette override. `None` means "inherit the
    /// global pick"; `Some(name)` pins this host to the named palette.
    /// Mirrors `Connection.terminal_theme` while the editor is open.
    pub terminal_theme: Option<String>,
    /// Mirrors `Connection.terminal_appearance` while the editor is
    /// open. `None` on every field is "inherit the global setting",
    /// which is what an untouched host carries.
    pub terminal_appearance: oryxis_core::models::TerminalAppearance,
    /// Mirrors `Connection.highlight_rules` while the editor is open
    /// (C6): this host's own rules plus the append / replace choice.
    pub highlight_rules: oryxis_core::models::HostHighlightRules,
    /// Per-host SSH keepalive override (raw text). Empty string means
    /// inherit the global setting; "0" disables keepalive on this host;
    /// any positive integer overrides the global value. Stored as a
    /// string while the editor is open so the input field can show
    /// what the user typed; serialized to `Option<u32>` on save.
    pub keepalive_interval: String,
    /// Wake-on-LAN MAC address as typed. Empty means "no MAC" (the
    /// card action stays hidden); validated and normalized to the
    /// canonical colon form on save, a malformed value blocks the save
    /// with an inline error rather than being dropped silently.
    pub mac_address: String,
    /// Login automation for hosts behind an interactive bastion.
    /// `None` = off. Mirrors `Connection.login_script_id`; a dangling
    /// id (script deleted while the editor was open) renders as off.
    pub login_script_id: Option<Uuid>,
    /// Values for the selected script's `{placeholder}` variables on
    /// this host, keyed by name. Plaintext by construction: a
    /// credential is a `SecretRef` in the script, never a variable.
    pub login_script_vars: Vec<(String, String)>,
    /// The credential the script types at the ASSET's own prompt. The
    /// host's `password` is spent on the bastion login, so this is a
    /// second secret with the same tri-state discipline.
    pub target_password: SecretInput,
    pub has_existing_target_password: bool,
    pub target_password_visible: bool,
    /// Secrets parked by a DERIVED clear (persist_editor_form): when a
    /// toggle-driven auto-save wipes a stored side-column secret
    /// (proxy disabled, "Use TOTP" off, login script detached), the
    /// plaintext moves here first, so re-enabling the toggle within
    /// the same editor session writes it back instead of leaving a
    /// misclick permanently destructive. An empty buffer is "nothing
    /// parked". Swept with the other secret buffers on close / lock.
    pub proxy_password_rescue: SecretInput,
    pub totp_rescue: SecretInput,
    pub target_password_rescue: SecretInput,
    /// Inline "new script" sub-form, open only while the user is
    /// creating one from the host editor. `None` = closed.
    pub login_script_draft: Option<LoginScriptDraft>,
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
    /// Mirrors `Connection.ambiguous_width`; `Auto` follows the encoding.
    pub ambiguous_width: oryxis_core::models::connection::AmbiguousWidth,
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
    /// Per-host sidebar auto-open override. Mirrors
    /// `Connection.sidebar_auto_open` (`None` inherits the global
    /// `sidebar_auto_open` setting).
    pub sidebar_auto_open: Option<bool>,
    /// Per-host legacy keyboard modes + feature toggles (C5). Edited
    /// directly; saved as `Connection.quirks` only when it differs from
    /// the default (so an untouched host stays `None`).
    pub quirks: oryxis_core::models::terminal_quirks::TerminalQuirks,
    /// Per-host SSH rekey threshold in MB as typed (empty = default).
    /// Maps to `Connection.rekey_limit_mb`.
    pub rekey_limit_mb: String,
    /// Directory a fresh SFTP mount of this host lands in, as typed
    /// (empty = the login directory). Maps to
    /// `Connection.sftp_initial_path`.
    pub sftp_initial_path: String,
    /// Drag-and-drop uploads to this host ride ZMODEM (`rz`) instead of
    /// SFTP, for shells that run inside a container. Maps to
    /// `Connection.zmodem_drops`.
    pub zmodem_drops: bool,
}

/// One collapsible section of the host editor's two-tier form. The
/// essential fields (label / address / port / username / password)
/// stay always visible; everything else lives under one of these
/// headers, closed by default. Open state is per app session (a
/// `HashSet` on `Oryxis`), shared across hosts: reopening the editor
/// keeps the sections the user was working in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum HostEditorSection {
    Authentication,
    Network,
    Compatibility,
    Integration,
    Terminal,
}

/// Create-flow starting points (P3 of the two-tier rework): one-shot
/// verbs on the new-host editor, not a persisted mode. Each prepares
/// the form and the section state for a common shape of host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostEditorPreset {
    /// Plain SSH host: protocol/port reset, every section closed.
    BasicSsh,
    /// Host reached through a jump host: opens the Network section and
    /// (when the vault has candidates) the chain editor's add flow.
    ViaBastion,
    /// Import from a cloud provider: hands the flow to the Cloud view.
    Cloud,
}

impl HostEditorPreset {
    /// i18n key for the chip label.
    pub(crate) fn label_key(self) -> &'static str {
        match self {
            Self::BasicSsh => "preset_basic_ssh",
            Self::ViaBastion => "preset_via_bastion",
            Self::Cloud => "preset_cloud",
        }
    }
}

impl HostEditorSection {
    /// i18n key for the section's header title.
    pub(crate) fn title_key(self) -> &'static str {
        match self {
            Self::Authentication => "authentication",
            Self::Network => "network",
            Self::Compatibility => "section_compatibility",
            Self::Integration => "integration",
            Self::Terminal => "terminal_settings",
        }
    }
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

    /// The four secret buffers plus their eyes, as one walk. Written
    /// once here so a fifth secret field joins the reveal, the sweep
    /// and the close paths by adding a row instead of by being
    /// remembered at each of them.
    fn secret_fields(&mut self) -> [(&mut SecretInput, &mut bool); 4] {
        [
            (&mut self.password, &mut self.password_visible),
            (&mut self.proxy_password, &mut self.proxy_password_visible),
            (&mut self.totp_secret, &mut self.totp_visible),
            (&mut self.target_password, &mut self.target_password_visible),
        ]
    }

    /// Drop what the eye toggles revealed, once the host editor's
    /// panel is gone. An UNTOUCHED buffer holds the stored plaintext
    /// the eye decrypted, which the vault hands back on demand the
    /// next time it is opened, so nothing is lost by emptying it. A
    /// TOUCHED buffer is the user's own typing, and a panel that was
    /// merely switched away from must not eat work in progress, so it
    /// survives. [`SecretInput::clear`] never sets the touched flag,
    /// which is what keeps the tri-state save semantics intact.
    ///
    /// The eyes close either way: a closed panel with an eye still
    /// armed reopens showing an empty field in plaintext, which reads
    /// as "no secret stored" and takes two clicks to undo.
    pub(crate) fn sweep_secrets(&mut self) {
        for (buffer, visible) in self.secret_fields() {
            if !buffer.touched() {
                buffer.clear();
            }
            *visible = false;
        }
        // Rescue stashes are never the user's own typing, so they are
        // dropped unconditionally: past the sweep the derived clear
        // they were parked for is final.
        self.proxy_password_rescue.clear();
        self.totp_rescue.clear();
        self.target_password_rescue.clear();
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

/// Transient state for the device-pairing flow (Settings → Sync). Holds
/// the codes / links this device generates and the inputs for joining a
/// peer. The persisted sync settings, the engine runtime handles and the
/// discovered-peer list stay on `Oryxis`; this is just the pairing UI.
#[derive(Debug, Clone, Default)]
pub(crate) struct SyncPairingForm {
    /// Pairing code this device is currently hosting (shown to the peer).
    pub code: Option<String>,
    /// Shareable `oryxis://pair` link / QR payload for this device.
    pub link: Option<String>,
    pub state: SyncPairingState,
    /// Peer's pairing code typed into the Join box.
    pub join_code_input: String,
    /// Peer's address typed into the Join box.
    pub join_target_input: String,
    /// Peer's `oryxis://pair` link pasted into the Join box.
    pub join_link_input: String,
}

/// Git sync transport: the snapshot committed to a Git remote.
///
/// The one backend that keeps HISTORY, which is why it exists next to
/// the folder transport: every round is a commit, so a vault wrecked by
/// a bad import can be read back from an earlier one.
#[derive(Debug, Clone, Default)]
pub(crate) struct GitSyncForm {
    /// Remote URL, anything `git clone` accepts.
    pub remote: String,
    pub in_progress: bool,
    pub status: Option<Result<String, String>>,
    /// Whether a usable `git` is on PATH. `None` = probe in flight (boot,
    /// a transport switch or opening the Sync section resolves it within
    /// a moment); the card
    /// disables "Sync now" until it is known. Probed on a worker thread
    /// ONLY: the old per-render `git_available()` call inside `view()`
    /// spawned a subprocess on the UI thread, freezing the app and
    /// flashing a console window per call on Windows.
    pub git_available: Option<bool>,
}

/// WebDAV sync transport: the snapshot on a Nextcloud / ownCloud /
/// Synology or plain WebDAV server.
///
/// The only file transport with a real compare-and-swap (`If-Match` on
/// the ETag), which is why it is worth having next to the folder
/// transport that already reaches those servers through their desktop
/// client: not every machine can run that client.
#[derive(Debug, Clone, Default)]
pub(crate) struct WebdavSyncForm {
    /// Collection or file URL. A trailing `/` means "put the shared
    /// snapshot name in here".
    pub url: String,
    /// Account name on the server.
    pub user: String,
    /// Account password, ideally an app password. Stored encrypted and
    /// deliberately NOT the group passphrase (which lives on
    /// `SyncState.passphrase_input`).
    pub password: String,
    pub in_progress: bool,
    pub status: Option<Result<String, String>>,
}

/// Folder sync transport: one encrypted snapshot in a local directory.
///
/// The directory is whatever the OS already mounts, which is the whole
/// point: a cloud client's folder (OneDrive, Drive, Dropbox, iCloud), a
/// network share, an external disk, a Syncthing directory. No OAuth, no
/// client secret, no provider API to keep working, and every one of
/// those destinations arrives at once.
#[derive(Debug, Clone, Default)]
pub(crate) struct FolderSyncForm {
    /// Directory (or full file path) the snapshot lives in.
    pub path: String,
    pub in_progress: bool,
    pub status: Option<Result<String, String>>,
}

/// Transient state for syncing the vault over SFTP (the SFTP sync
/// transport form in Settings → Sync), plus the in-flight progress. The
/// persisted transport choice stays on `Oryxis`.
#[derive(Debug, Clone, Default)]
pub(crate) struct SftpSyncForm {
    /// Host the vault blob is synced through, `None` until picked.
    pub host_id: Option<uuid::Uuid>,
    pub remote_path: String,
    pub picker_open: bool,
    pub picker_search: String,
    /// True while the SFTP sync task is in flight.
    pub in_progress: bool,
    pub status: Option<Result<String, String>>,
}

/// Icon + color picker state (opened from a host editor's icon box and
/// every other editor that carries an icon). The `for_*` flags route the
/// picked result to the right target on confirm: a Connection in the
/// vault (none set), or one of the deferred-save editor forms. The
/// open/closed flag (`show_icon_picker`) and the HSV popover anchor
/// (`icon_color_popover`) stay on `Oryxis`.
#[derive(Debug, Clone, Default)]
pub(crate) struct IconPickerState {
    /// Connection the picker edits directly (saves straight to the vault);
    /// `None` when one of the `for_*` deferred-save targets is set.
    pub for_id: Option<Uuid>,
    /// Route the result into the dynamic-group editor form instead of a
    /// Connection (deferred save).
    pub for_group_form: bool,
    /// Route into the session-group editor form (deferred save).
    pub for_session_group: bool,
    /// Route into the manual host-group editor panel (deferred save).
    pub for_group_edit: bool,
    /// Route into the local-terminal add/edit modal form (deferred save).
    pub for_local_terminal: bool,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub hex_input: String,
    /// Search query for the full-library Lucide search. Empty shows the
    /// curated preset grid; non-empty shows matches.
    pub icon_search: String,
}

/// SFTP backup target picker state, shown when an export/import is routed
/// through a remote host instead of a local file. `is_import` flips the
/// picker between writing the encrypted blob (export) and reading it back
/// (import); the export/import password + selection state is reused.
#[derive(Debug, Clone, Default)]
pub(crate) struct SftpBackupForm {
    /// Whether the picker is currently shown.
    pub open: bool,
    pub is_import: bool,
    /// Index into `connections` of the chosen host, `None` until picked.
    pub host: Option<usize>,
    /// Remote path the blob is written to / read from.
    pub path: String,
    /// True while the connect + transfer task is in flight.
    pub busy: bool,
    pub status: Option<Result<String, String>>,
}

/// Rename / restyle form for a user group (folder), shown in the group
/// edit panel. Distinct from the dynamic-group editor ([`CloudDynamicForm`])
/// which edits cloud-backed groups.
#[derive(Debug, Clone, Default)]
pub(crate) struct GroupEditForm {
    /// Whether the edit panel is currently shown.
    pub visible: bool,
    /// `Some` for the group being edited; `None` creates a new group
    /// on Save (the folder kebab's "New subgroup" path).
    pub id: Option<Uuid>,
    pub label: String,
    pub icon: String,
    pub color: String,
    /// Parent-group combo, label-matched on Save like the dynamic
    /// group editor's `parent_label`. Empty / unmatched = root.
    pub parent_label: String,
    /// Whether the Defaults section is expanded. Collapsed by default:
    /// most groups are just folders, and seven inheritance fields
    /// would otherwise be the loudest thing in a panel whose usual job
    /// is renaming.
    pub defaults_open: bool,
    /// The inheritance fields (D4), held as edit-friendly strings and
    /// labels the way the rest of the panel does, resolved to a
    /// `GroupDefaults` on Save. An empty string is "not set", which is
    /// what makes a host fall through to the next ancestor.
    pub username: String,
    /// Selected identity label, or `None` for "not set". The picker
    /// shows an explicit inherit option rather than a blank row.
    pub identity_label: Option<String>,
    pub proxy_identity_label: Option<String>,
    /// Port a new host in this group is created with. Empty = not set;
    /// deliberately never applied to hosts that already exist.
    pub port: String,
    pub terminal_theme: Option<String>,
    pub startup_snippet_label: Option<String>,
    /// Environment variables the group contributes, merged by name
    /// with what the host and the other ancestors provide.
    pub env_vars: Vec<oryxis_core::models::connection::EnvVar>,
}

/// Edit form for a dynamic (cloud-backed) group. Opened from the ⋮ menu
/// on a dynamic group card; edits the `cloud_query.template` (username,
/// initial_command, transport, key, identity) plus the group's general
/// fields (label, color, icon, parent) and its cloud source. `is_k8s`
/// flips the source section between the ECS (`cluster`/`service`/
/// `container`) and Kubernetes (`k8s_*`) field sets.
#[derive(Debug, Clone)]
pub(crate) struct CloudDynamicForm {
    /// Whether the edit form is currently shown.
    pub visible: bool,
    pub group_id: Option<Uuid>,
    pub username: String,
    pub initial_command: String,
    pub transport: oryxis_core::models::cloud::TransportKind,
    /// Selected key label (or `"(none)"`); resolved to a `key_id` on save.
    pub selected_key: Option<String>,
    /// Selected identity label (or `"(none)"`); resolved to an `identity_id` on save.
    pub selected_identity: Option<String>,
    /// General-section fields, parity with the host editor (rename, color,
    /// icon, move under any user group). Persisted on Save.
    pub label: String,
    pub color: String,
    pub icon: String,
    pub parent_label: String,
    /// Cloud-source fields (ECS variant).
    pub cluster: String,
    pub service: String,
    pub container: String,
    /// K8s dynamic-group source fields, used when the edited group's query
    /// is `K8sPods`. The selector value's meaning depends on
    /// `k8s_selector_kind`: a `k=v,k=v` string for `Labels`, otherwise a
    /// single resource name.
    pub is_k8s: bool,
    pub k8s_context: String,
    pub namespace: String,
    pub k8s_selector_kind: K8sSelectorKind,
    pub k8s_selector_value: String,
}

impl Default for CloudDynamicForm {
    fn default() -> Self {
        Self {
            visible: false,
            group_id: None,
            username: String::new(),
            initial_command: String::new(),
            // ECS Exec is the most common dynamic-group transport; the
            // editor swaps it to KubectlExec when `is_k8s` is set.
            transport: oryxis_core::models::cloud::TransportKind::EcsExec,
            selected_key: None,
            selected_identity: None,
            label: String::new(),
            color: String::new(),
            icon: String::new(),
            parent_label: String::new(),
            cluster: String::new(),
            service: String::new(),
            container: String::new(),
            is_k8s: false,
            k8s_context: String::new(),
            namespace: String::new(),
            k8s_selector_kind: K8sSelectorKind::Labels,
            k8s_selector_value: String::new(),
        }
    }
}

/// Add / edit wizard form for a cloud account (`CloudProfile`). Covers
/// every provider + auth combination (AWS profile / access key / SSO,
/// Kubernetes kubeconfig); only the fields for the selected
/// `provider` + `auth_kind` are rendered. The saved profiles live in
/// `Oryxis::cloud_profiles`; this is wizard state only.
#[derive(Debug, Clone, Default)]
pub(crate) struct CloudForm {
    /// Whether the wizard is currently shown.
    pub visible: bool,
    pub label: String,
    pub provider: CloudProviderChoice,
    pub auth_kind: CloudAuthChoice,
    pub aws_profile_name: String,
    /// Workload regions; the first entry is the default region and the
    /// full list drives discovery fan-out. Persisted as both `region`
    /// (= first) and `regions` (= full list) for forward compat.
    pub aws_regions: Vec<String>,
    /// Draft text in the region input box, committed to `aws_regions`
    /// on Enter.
    pub aws_region_draft: String,
    /// Access Key auth fields. The secret follows the password-tri-state
    /// convention (`*_touched` differentiates "leave alone" from
    /// "explicitly cleared").
    pub aws_access_key_id: String,
    pub aws_access_key_secret: String,
    pub aws_access_key_secret_touched: bool,
    pub aws_access_key_secret_visible: bool,
    pub aws_access_key_session_token: String,
    pub aws_has_existing_secret: bool,
    /// SSO (IAM Identity Center) auth fields.
    pub aws_sso_start_url: String,
    pub aws_sso_region: String,
    pub aws_sso_account_id: String,
    pub aws_sso_role_name: String,
    /// Kubernetes (Kubeconfig) auth fields. Both optional: blank
    /// kubeconfig = kubectl's default, blank context = current-context.
    pub kubeconfig_path: String,
    pub context: String,
    /// GCP project id to scope discovery to. Optional: blank = whatever
    /// `gcloud config get-value project` resolves (the active project).
    pub gcp_project: String,
    /// Azure subscription id (or name) to scope discovery to. Optional:
    /// blank = whatever `az account show` resolves (the active
    /// subscription).
    pub azure_subscription: String,
    /// `Some` when editing an existing profile (update in place).
    pub editing_id: Option<Uuid>,
    pub error: Option<String>,
    pub test_state: CloudTestState,
}

/// Import / edit form for an SSH key, shown in the keychain key panel.
/// The multi-line PEM editor buffer (`key_import_content`) stays on
/// `Oryxis` because `text_editor::Content` is not `Clone`; this struct
/// holds the surrounding scalar inputs. The saved keys live in
/// `Oryxis::keys`.
#[derive(Debug, Clone, Default)]
pub(crate) struct KeyImportForm {
    pub label: String,
    /// Raw PEM string for import (mirrors the live editor buffer).
    pub pem: String,
    /// Passphrase for an encrypted private key. In memory only; once the
    /// key is decrypted on import it is re-encoded unencrypted and the
    /// vault's master key takes over for at-rest protection.
    pub passphrase: String,
    /// Set when import_key returns `KeyNeedsPassphrase`; drives the
    /// passphrase row in the import panel.
    pub passphrase_required: bool,
    pub passphrase_visible: bool,
    /// The key's OpenSSH public line (`ssh-ed25519 AAAA... comment`),
    /// editable (B2.1, Termius parity; it is also what the ssh-agent
    /// serves). Empty = derived from the private key on save; non-empty
    /// input must parse and certify the same key data as the private key
    /// (editing the trailing comment is fine).
    pub public_key: String,
    /// The attached OpenSSH certificate line (`ssh-*-cert-v01@... AAAA...`),
    /// B2. Public material, so it lives in plain form state like the
    /// public key. Empty = no certificate.
    pub certificate: String,
    /// Set when the browse flow auto-probed a `<key>-cert.pub` next to the
    /// picked private key and prefilled `certificate`; drives the
    /// dismissible "certificate detected" hint. Cleared on manual edit.
    pub cert_detected: bool,
    /// `Some` when editing an existing key (update in place).
    pub editing_id: Option<Uuid>,
}

/// Parsed, display-ready view of a key's attached OpenSSH certificate,
/// built once when the viewer modal opens (B2) so `view()` never
/// re-parses. All fields are already localized / formatted strings
/// except the flags. `key_idx` targets the "Remove certificate" action
/// back at the owning key.
#[derive(Debug, Clone)]
pub(crate) struct CertViewerData {
    pub key_idx: usize,
    pub key_label: String,
    pub key_id: String,
    pub serial: u64,
    pub is_host: bool,
    pub principals: Vec<String>,
    /// Local-time validity bounds, preformatted (empty when unbounded).
    pub valid_from: String,
    pub valid_until: String,
    /// SHA256 fingerprint of the signing CA key.
    pub ca_fingerprint: String,
    pub expired: bool,
}

/// Top-level algorithm choice in the key-generation panel; the
/// bits/curve sub-picker renders only for RSA/ECDSA.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum KeyGenAlgo {
    #[default]
    Ed25519,
    Rsa,
    Ecdsa,
}

impl std::fmt::Display for KeyGenAlgo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Ed25519 => "Ed25519",
            Self::Rsa => "RSA",
            Self::Ecdsa => "ECDSA",
        })
    }
}

/// Read-only view of a freshly generated key for the result screen.
/// The private PEM is saved to the vault immediately on success and
/// deliberately NOT retained here; export actions re-read it from the
/// vault so a soft lock leaves nothing secret in form state.
#[derive(Debug, Clone)]
pub(crate) struct GeneratedKeyView {
    pub id: Uuid,
    pub label: String,
    pub fingerprint: String,
    pub public_key: String,
}

/// The key-generation panel (keychain > ADD > Generate key). Secret
/// hygiene: this struct never holds private key material; see
/// [`GeneratedKeyView`]. Swept on soft lock like every secret-bearing
/// editor.
#[derive(Debug, Clone)]
pub(crate) struct KeyGenerateForm {
    pub label: String,
    /// Key comment (lands in the public line); optional.
    pub comment: String,
    pub algo: KeyGenAlgo,
    pub rsa_bits: oryxis_vault::RsaBits,
    pub ecdsa_curve: oryxis_vault::EcdsaCurveChoice,
    /// A generation task is in flight (spinner, Generate disabled).
    pub working: bool,
    pub error: Option<String>,
    /// Set after a successful generation; flips the panel to the
    /// result screen.
    pub result: Option<GeneratedKeyView>,
    /// Export-private-key passphrase pair (result screen). Cleared
    /// with the form.
    pub export_passphrase: String,
    pub export_passphrase_confirm: String,
    /// Reveal toggles for the pair's eye buttons, one per field.
    pub export_passphrase_visible: bool,
    pub export_passphrase_confirm_visible: bool,
}

impl Default for KeyGenerateForm {
    fn default() -> Self {
        Self {
            label: String::new(),
            comment: String::new(),
            algo: KeyGenAlgo::Ed25519,
            // Owner-confirmed defaults: Ed25519 primary, RSA at 4096.
            rsa_bits: oryxis_vault::RsaBits::B4096,
            ecdsa_curve: oryxis_vault::EcdsaCurveChoice::P256,
            working: false,
            error: None,
            result: None,
            export_passphrase: String::new(),
            export_passphrase_confirm: String::new(),
            export_passphrase_visible: false,
            export_passphrase_confirm_visible: false,
        }
    }
}

/// Add / edit form for a standalone port-forward rule (the
/// `PortForwardRule` entity, independent of any terminal session), shown
/// in the Port Forwards panel. Distinct from [`PortForwardForm`], which
/// holds the inline forwards edited within a connection. The saved rules
/// live in `Oryxis::port_forward_rules`; this is editor state only.
#[derive(Debug, Clone)]
pub(crate) struct PortForwardRuleForm {
    pub label: String,
    pub kind: oryxis_core::models::port_forward_rule::ForwardKind,
    /// Host this rule tunnels through (`None` until picked).
    pub host_id: Option<Uuid>,
    pub listen_host: String,
    pub listen_port: String,
    pub target_host: String,
    pub target_port: String,
    pub auto_start: bool,
    /// `Some` when editing an existing rule (update in place).
    pub editing_id: Option<Uuid>,
    pub error: Option<String>,
}

impl Default for PortForwardRuleForm {
    fn default() -> Self {
        Self {
            label: String::new(),
            kind: oryxis_core::models::port_forward_rule::ForwardKind::Local,
            host_id: None,
            // Loopback is the safe default listen address for a local
            // forward; binding 0.0.0.0 is opt-in.
            listen_host: "127.0.0.1".into(),
            listen_port: String::new(),
            target_host: String::new(),
            target_port: String::new(),
            auto_start: false,
            editing_id: None,
            error: None,
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
    /// Password buffer; tri-state per [`SecretInput`].
    pub password: SecretInput,
    /// Selected SSH key label, when the identity authenticates by key.
    pub key: Option<String>,
    pub password_visible: bool,
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
/// records whether the stored row carries one, [`SecretInput`] tracks
/// whether the user edited the field this session, so save can
/// distinguish "leave as-is" from "clear" from "set".
/// The inline editor for one highlight rule (Settings > Terminal).
///
/// A working copy, not a live edit: the rule list is what the terminal
/// paints and watches from, so a half-typed pattern must not reach it.
/// `editing` is the index being edited, `None` when the editor is
/// closed; a new rule is edited at the index it will occupy.
/// Which list a highlight-rule edit is aimed at. The editor, its
/// messages and its handler are shared between Settings (the global
/// list) and the host editor (that host's own), because they edit the
/// same kind of thing; only the list they commit to differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum RuleScope {
    #[default]
    Global,
    Host,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct HighlightRuleForm {
    /// Whose list is being edited.
    pub scope: RuleScope,
    /// Which rule the editor is open on. `None` = list only.
    pub editing: Option<usize>,
    /// Whether that index is a rule being CREATED (it is not in the list
    /// yet) rather than an existing one being changed.
    pub creating: bool,
    pub rule: oryxis_core::models::HighlightRule,
    /// Inline validation error (bad regex, empty pattern).
    pub error: Option<String>,
    /// Index pending delete confirmation.
    pub confirm_delete: Option<usize>,
}

/// Settings > Connection management surface for login scripts. The
/// host editor creates them (that is where the user already is when
/// they need one); this is where they are renamed, re-authored step by
/// step, and deleted.
#[derive(Debug, Clone, Default)]
pub(crate) struct LoginScriptForm {
    /// Which script's step editor is expanded. `None` = list only.
    pub editing_id: Option<Uuid>,
    pub name: String,
    /// Working copy of the steps; committed to the vault on save so a
    /// half-edited list never reaches a live connect.
    pub steps: Vec<oryxis_core::login_script::LoginStep>,
    /// Inline validation error (bad regex, empty script).
    pub error: Option<String>,
    /// Id pending delete confirmation.
    pub confirm_delete: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub(crate) struct ProxyIdentityForm {
    /// Whether the inline editor is currently shown.
    pub visible: bool,
    pub label: String,
    pub kind: ProxyKind,
    pub host: String,
    pub port: String,
    pub username: String,
    pub password: SecretInput,
    pub password_visible: bool,
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
            password: SecretInput::default(),
            password_visible: false,
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
            protocol: oryxis_core::models::connection::ConnectionProtocol::Ssh,
            serial: None,
            rd_kind: oryxis_core::models::remote_desktop::RemoteDesktopKind::default(),
            rd_gateway_id: None,
            telnet_tls: false,
            telnet_tls_insecure: false,
            mosh_enabled: false,
            mosh_server_path: String::new(),
            mosh_port_range: String::new(),
            mosh_command: String::new(),
            local_terminal_id: None,
            local_cwd: String::new(),
            address_family: oryxis_core::models::connection::AddressFamily::default(),
            quick_flow: false,
            hostname: String::new(),
            port: "22".into(),
            username: String::new(),
            password: SecretInput::default(),
            auth_method: AuthMethod::Auto,
            group_name: String::new(),
            tags_text: String::new(),
            selected_key: None,
            jump_chain: Vec::new(),
            selected_identity: None,
            editing_id: None,
            has_existing_password: false,
            password_visible: false,
            username_focused: false,
            port_forwards: Vec::new(),
            env_vars: Vec::new(),
            mcp_enabled: true,
            monitor_enabled: false,
            monitor_disks_custom: false,
            monitor_disks: Vec::new(),
            agent_forwarding: false,
            x11_forwarding: false,
            session_logging: None,
            proxy_kind: ProxyKind::None,
            proxy_host: String::new(),
            proxy_port: String::new(),
            proxy_username: String::new(),
            proxy_password: SecretInput::default(),
            proxy_command: String::new(),
            has_existing_proxy_password: false,
            proxy_password_visible: false,
            totp_secret: SecretInput::default(),
            has_existing_totp: false,
            totp_visible: false,
            use_totp: false,
            use_disk_key: false,
            identity_file: String::new(),
            disk_key_status: Default::default(),
            terminal_theme: None,
            terminal_appearance: Default::default(),
            highlight_rules: Default::default(),
            keepalive_interval: String::new(),
            mac_address: String::new(),
            login_script_id: None,
            login_script_vars: Vec::new(),
            target_password: SecretInput::default(),
            has_existing_target_password: false,
            target_password_visible: false,
            proxy_password_rescue: SecretInput::default(),
            totp_rescue: SecretInput::default(),
            target_password_rescue: SecretInput::default(),
            login_script_draft: None,
            auto_title: None,
            cloud_transport: None,
            icon_style: None,
            encoding: None,
            ambiguous_width: Default::default(),
            terminal_type: None,
            ciphers: None,
            kex: None,
            macs: None,
            host_key_algorithms: None,
            privacy_mode: None,
            sidebar_auto_open: None,
            quirks: oryxis_core::models::terminal_quirks::TerminalQuirks::default(),
            rekey_limit_mb: String::new(),
            sftp_initial_path: String::new(),
            zmodem_drops: false,
        }
    }
}

#[cfg(test)]
mod secret_input_tests {
    use super::SecretInput;

    #[test]
    fn untouched_resolves_to_none_preserving_stored_secret() {
        // Fresh field: nothing typed, the stored secret must survive.
        let field = SecretInput::default();
        assert_eq!(field.resolve(), None);
        // Hydration-style prefill must not count as an edit either.
        let mut field = SecretInput::default();
        field.prefill("hunter2".into());
        assert_eq!(field.resolve(), None);
        assert_eq!(field.as_str(), "hunter2");
        assert!(!field.touched());
    }

    #[test]
    fn edited_empty_resolves_to_some_empty_clearing_stored_secret() {
        // Typing then erasing everything is an explicit clear.
        let mut field = SecretInput::default();
        field.set("secret".into());
        field.set(String::new());
        assert_eq!(field.resolve(), Some(""));
        assert!(field.touched());
    }

    #[test]
    fn edited_value_resolves_to_some_value_storing_it() {
        let mut field = SecretInput::default();
        field.set("s3cret".into());
        assert_eq!(field.resolve(), Some("s3cret"));
        assert_eq!(field.as_str(), "s3cret");
        assert!(field.touched());
    }

    #[test]
    fn clear_returns_to_the_preserve_state() {
        // Form reset after an edit: back to "leave the vault value".
        let mut field = SecretInput::default();
        field.set("s3cret".into());
        field.clear();
        assert_eq!(field.resolve(), None);
        assert_eq!(field.as_str(), "");
        assert!(!field.touched());
    }

    #[test]
    fn debug_never_prints_the_value() {
        // A form formatted into a log line or a panic message must not
        // carry the secret with it.
        let mut field = SecretInput::default();
        field.set("s3cret".into());
        let rendered = format!("{field:?}");
        assert!(!rendered.contains("s3cret"), "{rendered}");
        assert!(rendered.contains("touched: true"), "{rendered}");
    }
}

#[cfg(test)]
mod sweep_secrets_tests {
    use super::ConnectionForm;

    /// The whole contract of the panel-close sweep: what the eye
    /// revealed goes, what the user typed stays, and every eye closes.
    #[test]
    fn sweeps_revealed_stored_secrets_and_keeps_typed_ones() {
        let mut form = ConnectionForm::default();
        // Two fields revealed from the vault (prefill = untouched)...
        form.password.prefill("stored-pw".into());
        form.password_visible = true;
        form.proxy_password.prefill("stored-proxy".into());
        form.proxy_password_visible = true;
        // ...and two the user typed into.
        form.totp_secret.set("JBSWY3DPEHPK3PXP".into());
        form.totp_visible = true;
        form.target_password.set("typed-target".into());

        form.sweep_secrets();

        // Revealed stored plaintext is gone, and still resolves to
        // "preserve" so the save semantics never changed.
        assert_eq!(form.password.as_str(), "");
        assert_eq!(form.password.resolve(), None);
        assert_eq!(form.proxy_password.as_str(), "");
        assert_eq!(form.proxy_password.resolve(), None);
        // Work in progress survives a panel switch untouched.
        assert_eq!(form.totp_secret.resolve(), Some("JBSWY3DPEHPK3PXP"));
        assert_eq!(form.target_password.resolve(), Some("typed-target"));
        // Every eye closes, including the ones over a kept buffer.
        assert!(!form.password_visible);
        assert!(!form.proxy_password_visible);
        assert!(!form.totp_visible);
        assert!(!form.target_password_visible);
    }

    /// An edited-empty buffer is an explicit "clear the stored secret",
    /// which the sweep must not silently downgrade to "preserve".
    #[test]
    fn keeps_an_explicit_clear_pending() {
        let mut form = ConnectionForm::default();
        form.password.set(String::new());
        form.sweep_secrets();
        assert_eq!(form.password.resolve(), Some(""));
    }
}
