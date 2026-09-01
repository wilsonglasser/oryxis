//! `Oryxis::handle_editor`, match arms for the connection editor:
//! field changes, save/cancel/duplicate/delete, port-forwarding edits,
//! identity selection, MCP-enabled toggle, OS detection.

#![allow(clippy::result_large_err)]

use iced::Task;

use oryxis_core::models::connection::{AuthMethod, Connection, ProxyType};
use oryxis_core::models::group::Group;

use crate::app::{EditorMessage, SshMessage, Message, Oryxis};
use crate::state::{ConnectionForm, EnvVarForm, PortForwardForm, ProxyKind};

/// Which of the host editor's four secret fields an eye toggle acts
/// on. The four flows are the same flow, differing only in the buffer
/// and the encrypted column behind it, so they share one handler
/// instead of four copies that can drift apart.
#[derive(Debug, Clone, Copy)]
pub(crate) enum EditorSecret {
    Password,
    ProxyPassword,
    Totp,
    TargetPassword,
}

/// What building a `Connection` from the form may do about a typed
/// Parent Group value. Split three ways because the auto-save debounce
/// sits between "just checking" and "the user is done": a half-typed
/// name must never mint a vault group ("Pro" while typing
/// "Production"), yet the completed name must materialize once the
/// editor commits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GroupWrite {
    /// Find-or-create (the explicit Save and the closing flush): an
    /// unmatched value materializes as a breadcrumb path.
    Create,
    /// Keep the host's stored group (the auto-save tick, and the
    /// flushes no user gesture concluded: a lock, a window close). A
    /// debounce tick lands mid-word, and a mid-word value is not an
    /// answer about the group: "Prod" while typing "Production" is an
    /// exact label match for a DIFFERENT group, so resolving it would
    /// reparent the host (and ride sync) until the next keystroke, and
    /// creating it would mint "Sta" out of an interrupted "Staging".
    /// A blank value still means root, that is a deliberate clear.
    Keep,
    /// No group resolution at all (signature builds and
    /// connect-without-saving): nothing may be written, not even into
    /// the in-memory list.
    Skip,
}

/// Why a persist refused, split by what dropping the error would cost.
/// `Invalid` means the form does not build (half-typed state): the
/// vault keeps the last valid save and the auto-save paths stay
/// silent by design. `Vault` means the write itself failed: silence
/// here is data loss, so every caller must surface it.
#[derive(Debug)]
pub(super) enum PersistError {
    Invalid(String),
    Vault(String),
}

impl PersistError {
    /// The user-facing text, for surfaces that show both kinds.
    pub(super) fn into_message(self) -> String {
        match self {
            PersistError::Invalid(m) | PersistError::Vault(m) => m,
        }
    }
}

impl Oryxis {
    /// Flip one secret field's eye.
    ///
    /// Revealing DECRYPTS the stored value into the form buffer for
    /// exactly as long as it is shown, hiding drops it again, and the
    /// panel-close sweep ([`ConnectionForm::sweep_secrets`]) is the
    /// backstop for the paths that never see a second click. The value
    /// arrives through [`SecretInput::prefill`], which leaves the
    /// buffer untouched, so a revealed-but-unedited field still
    /// resolves to `None` (preserve the stored secret) on save.
    ///
    /// A buffer the user typed into is never overwritten by the
    /// reveal, nor emptied by the hide: it is their text, not the
    /// vault's, and it is already on screen.
    pub(super) fn toggle_editor_secret(&mut self, which: EditorSecret) {
        // Disjoint field borrows: the buffer below is borrowed out of
        // the form, so anything else the form knows is read first.
        let Self { editor_form: form, vault, .. } = self;
        let editing_id = form.editing_id;
        let (buffer, visible) = match which {
            EditorSecret::Password => (&mut form.password, &mut form.password_visible),
            EditorSecret::ProxyPassword => {
                (&mut form.proxy_password, &mut form.proxy_password_visible)
            }
            EditorSecret::Totp => (&mut form.totp_secret, &mut form.totp_visible),
            EditorSecret::TargetPassword => {
                (&mut form.target_password, &mut form.target_password_visible)
            }
        };
        if *visible {
            if !buffer.touched() {
                buffer.clear();
            }
            *visible = false;
            return;
        }
        // A new host has no row to read from, and a sealed vault
        // answers `Err`; both leave the field empty, which is what it
        // already showed.
        if !buffer.touched()
            && let Some(id) = editing_id
            && let Some(store) = vault.as_ref()
            && let Ok(Some(secret)) = match which {
                EditorSecret::Password => store.get_connection_password(&id),
                EditorSecret::ProxyPassword => store.get_proxy_password(&id),
                EditorSecret::Totp => store.get_connection_totp_secret(&id),
                EditorSecret::TargetPassword => store.get_connection_target_password(&id),
            }
        {
            buffer.prefill(secret);
        }
        *visible = true;
    }

    /// Open the host editor on a blank form, pre-filled with the user's
    /// new-connection defaults (agent forwarding, port, keepalive,
    /// TERM), so they don't re-set the same fields on every new host.
    ///
    /// Always SSH: every other protocol is one pick away in the form's
    /// own picker, which is where a user looks for it (an "Add remote
    /// desktop" entry used to seed RemoteDesktop here, and its absence
    /// from that picker read as "this app has no RDP").
    pub(crate) fn open_new_host_editor(&mut self) -> iced::Task<crate::app::Message> {
        let protocol = oryxis_core::models::connection::ConnectionProtocol::Ssh;
        // An existing host may still be open with a debouncing
        // auto-save; persist it before the form resets to blank.
        self.editor_flush_pending();
        // Dismiss the `…` overflow menu if it launched this.
        self.overlay = None;
        // Mutually exclusive right-panel slot, close any other panel
        // before opening the host editor.
        self.cloud_form.visible = false;
        self.cloud_dynamic_form.visible = false;
        self.cloud_discover.visible = false;
        self.panels.session_group_panel = false;
        self.group_edit.visible = false;
        self.panels.host_panel = true;
        self.panel_nav_clear();
        self.editor_form = self.new_connection_form();
        self.editor_form.protocol = protocol;
        if let Some(p) = protocol.default_port() {
            self.editor_form.port = p.to_string();
        }
        self.editor_initial_command = iced::widget::text_editor::Content::new();
        self.editor_startup_choice = crate::state::StartupChoice::None;
        // Creating a host while inside a folder (root or subgroup)
        // lands it there: prefill with the full breadcrumb path, which
        // is what the combo displays and what the save resolves first.
        if let Some(gid) = self.active_group
            && self.groups.iter().any(|g| g.id == gid && g.cloud_query.is_none())
        {
            self.editor_form.group_name =
                oryxis_core::models::Group::path_of(&self.groups, gid);
            // The group's default port (D4) prefills a host born here,
            // and this is the ONLY place it applies: resolving it at
            // connect time would move an existing host's destination
            // the moment its group gained a default. The protocol's own
            // default still wins, since choosing Telnet is a more
            // specific statement about the port than the folder is.
            if protocol.default_port().is_none()
                && let Some(port) = self.group_default_port(Some(gid))
            {
                self.editor_form.port = port.to_string();
            }
        }
        self.host_panel_error = None;
        self.rebuild_editor_combos();
        // Land the cursor in the first field so the very first Tab keypress
        // walks the form (focus_next with nothing focused would otherwise
        // grab the grid search input).
        crate::widgets::focus_input(iced::widget::Id::new("editor-hostname"))
    }

    pub(crate) fn new_connection_form(&self) -> crate::state::ConnectionForm {
        let term = &self.prefs.default_terminal_type;
        // Resolve the entity-reference defaults (identity / key / group /
        // proxy) to the label the form uses, dropping any that point at a
        // deleted entity so a stale default never blocks a new host.
        let default_identity = self.prefs.default_identity_id.and_then(|id| {
            self.identities.iter().find(|i| i.id == id).map(|i| i.label.clone())
        });
        let default_key = self
            .prefs.default_key_id
            .and_then(|id| self.keys.iter().find(|k| k.id == id))
            // A Certificate default only accepts cert-carrying keys (the
            // combo filters them out, so a bare default would be stuck).
            .filter(|k| {
                self.prefs.default_auth_method
                    != oryxis_core::models::connection::AuthMethod::Certificate
                    || k.certificate.is_some()
            })
            .map(|k| k.label.clone());
        let default_group = self
            .prefs.default_group_id
            .filter(|id| self.groups.iter().any(|g| g.id == *id))
            .map(|id| oryxis_core::models::Group::path_of(&self.groups, id))
            .unwrap_or_default();
        // A default proxy is a saved Proxy Identity reference; inline
        // proxies are per-host by nature and aren't defaulted. Drop a
        // dangling reference (identity deleted) back to no proxy.
        let proxy_kind = self
            .prefs.default_proxy_identity_id
            .filter(|id| self.proxy_identities.iter().any(|p| p.id == *id))
            .map(crate::state::ProxyKind::Identity)
            .unwrap_or(crate::state::ProxyKind::None);
        crate::state::ConnectionForm {
            agent_forwarding: self.prefs.default_agent_forwarding,
            port: if self.prefs.default_port.is_empty() || self.prefs.default_port == "0" {
                "22".to_string()
            } else {
                self.prefs.default_port.clone()
            },
            keepalive_interval: self.prefs.default_keepalive.clone(),
            terminal_type: if term.is_empty() || term == "xterm-256color" {
                None
            } else {
                Some(term.clone())
            },
            username: self.prefs.default_username.clone(),
            auth_method: self.prefs.default_auth_method.clone(),
            selected_identity: default_identity,
            selected_key: default_key,
            group_name: default_group,
            proxy_kind,
            mcp_enabled: self.prefs.default_mcp_enabled,
            monitor_enabled: false,
            encoding: self.prefs.default_encoding.clone(),
            env_vars: self.prefs.default_env_vars.clone(),
            ..crate::state::ConnectionForm::default()
        }
    }

    /// Rebuild the native combo_box states backing the host editor's
    /// Parent Group and Initial Command / Snippet fields. Called on
    /// editor-open.
    ///
    /// Parent Group: options are the visible (non-phantom) groups and
    /// the current `group_name` seeds the selection so an existing host
    /// pre-fills its folder. Typing / picking drives
    /// `editor_form.group_name`, so the save path (find-or-create by
    /// label) is untouched.
    ///
    /// Initial Command / Snippet: a forced-selection searchable combo.
    /// Options are the `None` / `Custom` sentinels first, then every
    /// snippet label. Picking commits via `EditorStartupChoiceChanged`;
    /// there is no free-text path (no `on_input`), so typing only
    /// filters. The current choice seeds the selection for prefill.
    pub(crate) fn rebuild_editor_combos(&mut self) {
        // Options are full breadcrumb paths ("Prod / Frontend"), so
        // subgroups are visibly nested and same-named folders under
        // different parents stay distinguishable. Alphabetical sort
        // naturally clusters a parent with its children.
        let visible = self.visible_group_ids();
        let mut labels: Vec<String> = self
            .groups
            .iter()
            .filter(|g| visible.contains(&g.id))
            .map(|g| oryxis_core::models::Group::path_of(&self.groups, g.id))
            .collect();
        labels.sort_by_key(|s| s.to_lowercase());
        labels.dedup();
        // The live selection travels through the widget's `selection`
        // argument at view time (host_panel/basics.rs); since the
        // upstream unify-text-editing refactor the State only carries
        // the options (`with_selection` is gone, the widget restores
        // the selected value itself).
        self.editor_parent_combo = iced::widget::combo_box::State::new(labels);

        self.reset_editor_startup_combo();
        self.reset_editor_key_combo();
        self.editor_login_script_combo =
            iced::widget::combo_box::State::new(self.login_script_options());
        self.editor_script_template_combo = iced::widget::combo_box::State::new(vec![
            crate::i18n::t("login_script_tpl_jumpserver").to_string(),
            crate::i18n::t("login_script_tpl_bastion").to_string(),
        ]);
    }

    /// Option list for the Initial Command / Snippet combo: the
    /// `None` / `Custom` sentinels first, then every snippet label.
    fn editor_startup_options(&self) -> Vec<String> {
        let mut opts: Vec<String> = vec![
            crate::i18n::t("startup_none").to_string(),
            crate::i18n::t("startup_custom").to_string(),
        ];
        for s in &self.snippets {
            opts.push(s.label.clone());
        }
        opts
    }

    /// (Re)build the startup combo with an *empty* typed value. The
    /// committed choice is shown via the widget's `selection` prop, not
    /// the internal value, so the field still displays the current pick
    /// while focusing clears the input for a fresh search over the full
    /// list. Called on editor-open and again on every focus (`on_open`)
    /// so a previous abandoned search doesn't pre-filter the list.
    pub(crate) fn reset_editor_startup_combo(&mut self) {
        self.editor_startup_combo =
            iced::widget::combo_box::State::new(self.editor_startup_options());
    }

    /// Option list for the SSH Key combo: the `(none)` sentinel first,
    /// then every saved key's label. Under `AuthMethod::Certificate`
    /// (B2.1) only keys carrying a certificate are listed, the method
    /// offers the cert and nothing else, so a bare key is never a valid
    /// pick there. Under `Agent` (B3) every key qualifies (the pick is
    /// the preferred agent identity) with security keys sorted first,
    /// they are the reason the pin exists.
    fn editor_key_options(&self) -> Vec<String> {
        use oryxis_core::models::connection::AuthMethod;
        let filter = match self.editor_form.auth_method {
            AuthMethod::Certificate => KeyComboFilter::CertificateOnly,
            AuthMethod::Agent => KeyComboFilter::SecurityKeysFirst,
            _ => KeyComboFilter::All,
        };
        key_combo_options(&self.keys, filter)
    }

    /// (Re)build the SSH Key combo with an empty typed value. Same
    /// forced-selection pattern as `reset_editor_startup_combo`: the
    /// committed key (`editor_form.selected_key`) drives the display via
    /// the widget's `selection` prop, so focusing clears the input for a
    /// fresh search while the current pick is preserved.
    pub(crate) fn reset_editor_key_combo(&mut self) {
        self.editor_key_combo =
            iced::widget::combo_box::State::new(self.editor_key_options());
    }

    /// Refresh a forced-selection combo's options ONLY when they
    /// actually changed.
    ///
    /// The three combos that clear on focus (`key`, `startup`,
    /// `login_script`) used to rebuild their `State` unconditionally
    /// from their `on_open` handler. That is now the opposite of what
    /// it reads like: the fork's `combo_box` already empties its own
    /// input when it takes focus, and a rebuilt `State` carries a fresh
    /// version, which makes the next `diff` overwrite the input with
    /// the current selection AND filter the menu by it. The user then
    /// sees a menu holding exactly the option they already had, i.e.
    /// their other keys look like they vanished (found in the #156 QA:
    /// an imported key could not be picked at all).
    ///
    /// Comparing first keeps the reason the rebuild existed, picking up
    /// a key or snippet created while the editor is open, without
    /// paying the re-filter on every focus.
    fn refresh_combo(state: &mut iced::widget::combo_box::State<String>, options: Vec<String>) {
        if state.options() != options {
            *state = iced::widget::combo_box::State::new(options);
        }
    }

    /// Display label for the current startup-command choice (the
    /// `None` / `Custom` sentinels or the referenced snippet's label).
    /// Shared by the combo's selection prop and its rebuild seed; a
    /// dangling snippet id falls back to `Custom`.
    pub(crate) fn editor_startup_label(&self) -> String {
        match &self.editor_startup_choice {
            crate::state::StartupChoice::None => crate::i18n::t("startup_none").to_string(),
            crate::state::StartupChoice::Custom => crate::i18n::t("startup_custom").to_string(),
            crate::state::StartupChoice::Snippet(id) => self
                .snippets
                .iter()
                .find(|s| s.id == *id)
                .map(|s| s.label.clone())
                .unwrap_or_else(|| crate::i18n::t("startup_custom").to_string()),
        }
    }


    /// Build a `Connection` from the host-editor form: everything
    /// `EditorSave` persists except the tri-state secrets (main password,
    /// proxy password, TOTP secret), which each flow handles itself.
    /// `groups` decides what the typed Parent Group value may do (see
    /// [`GroupWrite`]); apart from `GroupWrite::Create` the build has
    /// no side effects, which the auto-save dirty check depends on.
    /// Errors are user-facing strings for `host_panel_error`.
    fn connection_from_editor_form(
        &mut self,
        groups: GroupWrite,
    ) -> Result<Connection, String> {
        let port: u16 = self.editor_form.port.parse().unwrap_or(22);

        // Group resolution. The combo displays breadcrumb paths, so it
        // tries the full path first, then a bare label (typed by
        // hand). Under `Create`, an unmatched value is materialised as
        // a breadcrumb PATH: "Prod / NewTeam" builds the nested chain
        // (reusing existing segments), never a single root group named
        // with the separator inside it (which would then impersonate a
        // real path).
        let group_name = self.editor_form.group_name.trim().to_string();
        let group_id = match groups {
            GroupWrite::Skip => None,
            _ if group_name.is_empty() => None,
            GroupWrite::Create => {
                let mut created = Vec::new();
                let gid =
                    Group::resolve_or_create_path(&mut self.groups, &group_name, &mut created);
                if let Some(vault) = &self.vault {
                    for g in &created {
                        let _ = vault.save_group(g);
                    }
                }
                gid
            }
            GroupWrite::Keep => self.editor_form.editing_id.and_then(|id| {
                self.connections
                    .iter()
                    .find(|c| c.id == id)
                    .and_then(|c| c.group_id)
            }),
        };

        // Snapshot the pre-edit Connection (when editing an
        // existing row) so we can diff the user's changes after
        // all the per-field assignments below. The diff feeds
        // `customized_fields`, which the cloud reimport flow
        // honours to leave user-edited values alone on refresh.
        let original: Option<Connection> = self
            .editor_form
            .editing_id
            .and_then(|id| self.connections.iter().find(|c| c.id == id).cloned());

        let mut conn = original
            .clone()
            .unwrap_or_else(|| Connection::new("", ""));

        conn.label = self.editor_form.label.clone();
        conn.protocol = self.editor_form.protocol;
        // Serial params are only meaningful on a Serial host; clear them
        // otherwise so a host switched away from Serial doesn't carry a
        // stale config.
        conn.serial = if self.editor_form.protocol
            == oryxis_core::models::connection::ConnectionProtocol::Serial
        {
            Some(self.editor_form.serial.unwrap_or_default())
        } else {
            None
        };
        // Remote-desktop fields: kind rides on every host (harmless
        // scalar); the SSH gateway is meaningful only for a RemoteDesktop
        // host, cleared on any other protocol.
        conn.rd_kind = self.editor_form.rd_kind;
        conn.rd_gateway_id = if self.editor_form.protocol
            == oryxis_core::models::connection::ConnectionProtocol::RemoteDesktop
        {
            self.editor_form.rd_gateway_id
        } else {
            None
        };
        // Telnet options: only meaningful on a Telnet host, and written
        // as `None` when they are all default, so a host that never
        // touched TLS stays byte-identical to one saved before the
        // field existed.
        conn.telnet = if self.editor_form.protocol
            == oryxis_core::models::connection::ConnectionProtocol::Telnet
        {
            let opts = oryxis_core::models::telnet::TelnetOptions {
                tls: self.editor_form.telnet_tls,
                // The escape is meaningless without TLS, and leaving it
                // set would silently arm "skip verification" for a
                // later re-enable the user never confirmed.
                tls_insecure: self.editor_form.telnet_tls && self.editor_form.telnet_tls_insecure,
            };
            (!opts.is_default()).then_some(opts)
        } else {
            None
        };
        // mosh options, same rule and the same reason: cleared on any
        // protocol that is not SSH, because mosh is carried over SSH
        // and nothing else, and `None` when nothing was configured so a
        // host nobody touched stays byte-identical to one saved before
        // the field existed.
        //
        // The three settings are kept when the toggle is off, unlike
        // the Telnet escape above: they are facts about the host rather
        // than a trust decision, so nothing is armed by remembering
        // them, and a server path somebody had to look up is not
        // something to make them find again.
        conn.mosh = if self.editor_form.protocol
            == oryxis_core::models::connection::ConnectionProtocol::Ssh
        {
            let opts = oryxis_core::models::mosh::MoshOptions {
                enabled: self.editor_form.mosh_enabled,
                server_path: self.editor_form.mosh_server_path.trim().to_string(),
                port_range: self.editor_form.mosh_port_range.trim().to_string(),
                command: self.editor_form.mosh_command.trim().to_string(),
            };
            (!opts.is_default()).then_some(opts)
        } else {
            None
        };
        // Local-shell settings, same rule: cleared on any other
        // protocol, and `None` when the host just takes the default
        // shell in the default directory.
        conn.local = if self.editor_form.protocol
            == oryxis_core::models::connection::ConnectionProtocol::Local
        {
            // The label is saved beside the id as the cross-machine
            // fallback: ids are minted per machine, so a synced host
            // would otherwise resolve to nothing on the second one.
            let label = self.editor_form.local_terminal_id.and_then(|id| {
                self.local_terminals
                    .as_deref()
                    .unwrap_or(&[])
                    .iter()
                    .find(|e| e.id == id)
                    .map(|e| e.label.clone())
            });
            let cfg = oryxis_core::models::local::LocalConfig {
                terminal_id: self.editor_form.local_terminal_id,
                terminal_label: label,
                cwd: Some(self.editor_form.local_cwd.trim().to_string())
                    .filter(|c| !c.is_empty()),
            };
            (!cfg.is_default()).then_some(cfg)
        } else {
            None
        };
        // Address-family preference rides on every host (harmless scalar;
        // only the SSH dial paths read it today).
        conn.address_family = self.editor_form.address_family;
        conn.hostname = self.editor_form.hostname.clone();
        conn.port = port;
        conn.username = if self.editor_form.username.is_empty() {
            None
        } else {
            Some(self.editor_form.username.clone())
        };
        conn.auth_method = self.editor_form.auth_method.clone();
        conn.group_id = group_id;
        conn.key_id = self.editor_form.selected_key.as_ref().and_then(|label| {
            self.keys.iter().find(|k| k.label == *label).map(|k| k.id)
        });
        conn.use_disk_key = self.editor_form.use_disk_key;
        // Blank is "scan the default names", which is what the field's
        // placeholder promises, so an emptied path is not a path.
        conn.identity_file = Some(self.editor_form.identity_file.trim().to_string())
            .filter(|p| !p.is_empty());
        conn.identity_id = self.editor_form.selected_identity.as_ref().and_then(|label| {
            self.identities.iter().find(|i| i.label == *label).map(|i| i.id)
        });
        // Persist the full ordered chain. Drop any hop pointing
        // at a host that no longer exists or at this host itself
        // (a self-reference would be a connect-time loop), so a
        // stale form never writes a broken chain.
        let self_id = self.editor_form.editing_id;
        conn.jump_chain = self
            .editor_form
            .jump_chain
            .iter()
            .filter(|id| Some(**id) != self_id)
            .filter(|id| self.connections.iter().any(|c| c.id == **id))
            .copied()
            .collect();
        conn.port_forwards = self.editor_form.port_forwards.iter().filter_map(|pf| {
            let local_port = pf.local_port.parse::<u16>().ok()?;
            let remote_port = pf.remote_port.parse::<u16>().ok()?;
            if pf.remote_host.is_empty() { return None; }
            Some(oryxis_core::models::connection::PortForward {
                local_port,
                remote_host: pf.remote_host.clone(),
                remote_port,
            })
        }).collect();
        // Env vars: keep rows with a non-empty key (value may be
        // empty); trim the key so accidental whitespace doesn't
        // create a bogus variable name.
        conn.env_vars = self.editor_form.env_vars.iter().filter_map(|e| {
            let key = e.key.trim();
            if key.is_empty() { return None; }
            Some(oryxis_core::models::connection::EnvVar {
                key: key.to_string(),
                value: e.value.clone(),
            })
        }).collect();
        // MCP exposure is SSH-only (the handler resolves through the SSH
        // engine). The reduced Telnet/Serial editor hides the toggle, so
        // clamp here too: a host switched away from SSH must not stay
        // MCP-advertised. `list_mcp_connections` filters by protocol as
        // the source-of-truth guard for synced / imported hosts.
        conn.mcp_enabled = self.editor_form.protocol
            == oryxis_core::models::connection::ConnectionProtocol::Ssh
            && self.editor_form.mcp_enabled;
        // Same SSH clamp as `mcp_enabled`: monitoring reads /proc over an
        // SSH exec channel, so a host switched to Telnet / serial /
        // remote-desktop can't stay monitored.
        conn.monitor_enabled = self.editor_form.protocol
            == oryxis_core::models::connection::ConnectionProtocol::Ssh
            && self.editor_form.monitor_enabled;
        // Disk selection (issue #135). Auto is `None`; Custom keeps the
        // rows the user typed, blanks dropped (an unfinished row is not
        // a pattern) but an all-blank list still stored as `Some(vec![])`,
        // because Custom-with-nothing is the deliberate "report no disks
        // here" answer and must not silently mean Auto.
        conn.monitor_disks = self.editor_form.monitor_disks_custom.then(|| {
            self.editor_form
                .monitor_disks
                .iter()
                .map(|m| m.trim().to_string())
                .filter(|m| !m.is_empty())
                .collect()
        });
        conn.agent_forwarding = self.editor_form.agent_forwarding;
        // Same SSH clamp as `mcp_enabled` / `monitor_enabled`: `x11-req`
        // is an SSH channel request, so a host switched to Telnet /
        // serial / remote-desktop can't keep the flag set.
        conn.x11_forwarding = self.editor_form.protocol
            == oryxis_core::models::connection::ConnectionProtocol::Ssh
            && self.editor_form.x11_forwarding;
        conn.session_logging = self.editor_form.session_logging;
        conn.terminal_theme = self.editor_form.terminal_theme.clone();
        // Stored as `None` when nothing is overridden, so a host whose
        // overrides were cleared goes back to being byte-identical to
        // one that never had any (and stops shipping an empty object
        // over sync).
        conn.terminal_appearance =
            self.editor_form.terminal_appearance.clone().into_option();
        conn.highlight_rules = self.editor_form.highlight_rules.clone().into_option();
        conn.icon_style = self.editor_form.icon_style.clone();
        conn.encoding = self.editor_form.encoding.clone();
        conn.ambiguous_width = self.editor_form.ambiguous_width;
        conn.terminal_type = self.editor_form.terminal_type.clone();
        conn.ciphers = self.editor_form.ciphers.clone();
        conn.kex = self.editor_form.kex.clone();
        conn.macs = self.editor_form.macs.clone();
        conn.host_key_algorithms = self.editor_form.host_key_algorithms.clone();
        // Startup command source. Snippet -> store the live id and
        // clear the literal; Custom -> store the trimmed text (empty
        // == None); None -> clear both. `.text()` appends a trailing
        // newline, so trim before checking.
        match &self.editor_startup_choice {
            crate::state::StartupChoice::Snippet(id) => {
                conn.startup_snippet_id = Some(*id);
                conn.initial_command = None;
            }
            crate::state::StartupChoice::Custom => {
                conn.startup_snippet_id = None;
                let initial_command = self.editor_initial_command.text();
                conn.initial_command = if initial_command.trim().is_empty() {
                    None
                } else {
                    Some(initial_command.trim_end().to_string())
                };
            }
            crate::state::StartupChoice::None => {
                conn.startup_snippet_id = None;
                conn.initial_command = None;
            }
        }
        // If the host is cloud-imported (carries a cloud_ref)
        // and the user picked a transport in the editor,
        // persist it onto the existing CloudRef. Don't touch
        // anything else (resource_id, region, profile_id).
        if let Some(picked) = self.editor_form.cloud_transport
            && let Some(cref) = conn.cloud_ref.as_mut()
        {
            cref.transport_pref = picked;
        }
        // Empty string == inherit global; "0" == explicitly disabled
        // on this host; positive integer == per-host override.
        conn.keepalive_interval = if self.editor_form.keepalive_interval.is_empty() {
            None
        } else {
            self.editor_form.keepalive_interval.parse::<u32>().ok()
        };
        conn.auto_title = self.editor_form.auto_title;
        // Empty == no MAC (hides the card's Wake on LAN action). A
        // malformed value blocks the save with an inline error instead
        // of being silently dropped; a valid one is stored canonical
        // ("AA:BB:CC:DD:EE:FF") whatever notation was typed.
        let mac_raw = self.editor_form.mac_address.trim();
        conn.mac_address = if mac_raw.is_empty() {
            None
        } else {
            match crate::wol::parse_mac(mac_raw) {
                Some(mac) => Some(crate::wol::format_mac(mac)),
                None => return Err(crate::i18n::t("host_mac_invalid").to_string()),
            }
        };
        // Login automation. SSH-only, so a host switched to another
        // protocol drops the reference rather than carrying an
        // automation nothing will ever run (same clamp as `mcp_enabled`
        // and the TOTP secret). Variables are pruned to the ones the
        // selected script actually references, so a value left over
        // from a previous script cannot be typed at an unrelated
        // prompt.
        let is_ssh_now = self.editor_form.protocol
            == oryxis_core::models::connection::ConnectionProtocol::Ssh;
        conn.login_script_id = is_ssh_now
            .then_some(self.editor_form.login_script_id)
            .flatten()
            .filter(|sid| self.login_scripts.iter().any(|s| s.id == *sid));
        conn.login_script_vars = match conn.login_script_id {
            Some(sid) => {
                let names: Vec<String> = self
                    .login_scripts
                    .iter()
                    .find(|s| s.id == sid)
                    .map(|s| {
                        crate::util::login_script_placeholders(&s.steps)
                            .into_iter()
                            .map(|(n, _)| n)
                            .collect()
                    })
                    .unwrap_or_default();
                self.editor_form
                    .login_script_vars
                    .iter()
                    .filter(|(n, _)| names.contains(n))
                    .map(|(n, v)| oryxis_core::models::connection::ScriptVar {
                        name: n.clone(),
                        value: v.clone(),
                    })
                    .collect()
            }
            None => Vec::new(),
        };
        conn.tags = crate::util::parse_tags(&self.editor_form.tags_text);
        conn.privacy_mode = self.editor_form.privacy_mode;
        conn.sidebar_auto_open = self.editor_form.sidebar_auto_open;
        // Blank means "land in the login directory", the default; storing
        // an empty string instead of NULL would make the mount canonicalize
        // "" and fall back anyway, so normalize here.
        conn.sftp_initial_path = {
            let p = self.editor_form.sftp_initial_path.trim();
            (!p.is_empty()).then(|| p.to_string())
        };
        // SSH-only, like the other transport behaviour flags: `rz` rides
        // the session's byte stream, which only an SSH shell carries, so
        // a host switched to Telnet / serial / remote-desktop can't keep
        // the flag set.
        conn.zmodem_drops = self.editor_form.protocol
            == oryxis_core::models::connection::ConnectionProtocol::Ssh
            && self.editor_form.zmodem_drops;
        // C5: store quirks only when they differ from the xterm default,
        // so an untouched host keeps `quirks = None` (old-payload parity).
        conn.quirks = (self.editor_form.quirks
            != oryxis_core::models::terminal_quirks::TerminalQuirks::default())
        .then_some(self.editor_form.quirks);
        conn.rekey_limit_mb = self
            .editor_form
            .rekey_limit_mb
            .trim()
            .parse::<u32>()
            .ok()
            .filter(|&n| n > 0);
        // Map the editor form into either an inline ProxyConfig
        // or a `proxy_identity_id` reference. Validates host /
        // port / command up-front so the user gets an error
        // instead of a silently-broken proxy entry.
        let proxy_resolution = build_proxy_resolution(&self.editor_form)?;
        // Authored here, so approved here: a line the user just typed
        // must not meet a "did you mean it?" modal on the next dial.
        //
        // Only when it CHANGED, though. The form was filled from the
        // stored value, so a command proxy that arrived over sync or in
        // an imported file sits in that field already; approving on
        // every save would hand it the grant the moment the user opens
        // the host to change its colour, without them ever reading the
        // line. Unchanged means the user did not author it here.
        let previous_command = original.as_ref().and_then(|o| match o.proxy.as_ref() {
            Some(p) => match &p.proxy_type {
                oryxis_core::models::connection::ProxyType::Command(c) => Some(c.clone()),
                _ => None,
            },
            None => None,
        });
        let authored = proxy_resolution.proxy.as_ref().filter(|p| {
            match &p.proxy_type {
                oryxis_core::models::connection::ProxyType::Command(c) => {
                    previous_command.as_deref() != Some(c.as_str())
                }
                _ => false,
            }
        });
        self.trust_authored_proxy_command(authored, &conn.label);
        conn.proxy = proxy_resolution.proxy;
        conn.proxy_identity_id = proxy_resolution.proxy_identity_id;
        conn.updated_at = chrono::Utc::now();

        // Track user edits on cloud-imported hosts so the next
        // refresh from AWS doesn't clobber them. Only the
        // fields that discovery actually pushes are tracked,
        // anything else (port, color, group_id, ...) is fully
        // user-controlled on imported hosts already and doesn't
        // need a flag.
        if conn.cloud_ref.is_some()
            && let Some(orig) = &original
        {
            let mut customized = conn.customized_fields.clone();
            let mark = |list: &mut Vec<String>, name: &str| {
                if !list.iter().any(|s| s == name) {
                    list.push(name.to_string());
                }
            };
            if conn.label != orig.label {
                mark(&mut customized, "label");
            }
            if conn.hostname != orig.hostname {
                mark(&mut customized, "hostname");
            }
            if conn.username != orig.username {
                mark(&mut customized, "username");
            }
            conn.customized_fields = customized;
        }
        // Validate a newly typed TOTP secret before anything is
        // written, so a typo'd secret can't be stored and then
        // silently fail at connect time. Cleared/untouched skip, and a
        // disabled "Use TOTP" hides the field, so leftover text in the
        // buffer must not block the save (it gets cleared instead).
        if self.editor_form.use_totp
            && let Some(secret) = self.editor_form.totp_secret.resolve()
            && !secret.trim().is_empty()
            && let Err(e) = oryxis_core::totp::Totp::parse(secret)
        {
            return Err(format!("{}: {e}", crate::i18n::t("totp_invalid")));
        }

        Ok(conn)
    }


    /// Build a fully populated `ConnectionForm` from an existing
    /// `Connection` (labels resolved against the current groups / keys /
    /// identities lists). Secrets are never prefilled: the `has_*` flags
    /// drive the masked placeholders and the `SecretInput` tri-state
    /// decides what a later save writes. Shared by `EditConnection`
    /// (vault hosts)
    /// and `SaveQuickHost` (ad-hoc hosts being persisted).
    fn form_from_connection(
        &self,
        conn: &Connection,
        has_pw: bool,
        has_proxy_pw: bool,
        has_totp: bool,
        has_target_pw: bool,
    ) -> ConnectionForm {
        ConnectionForm {
            label: conn.label.clone(),
            protocol: conn.protocol,
            serial: conn.serial,
            rd_kind: conn.rd_kind,
            rd_gateway_id: conn.rd_gateway_id,
            telnet_tls: conn.telnet.map(|t| t.tls).unwrap_or(false),
            telnet_tls_insecure: conn.telnet.map(|t| t.tls_insecure).unwrap_or(false),
            mosh_enabled: conn.mosh.as_ref().is_some_and(|m| m.enabled),
            mosh_server_path: conn
                .mosh
                .as_ref()
                .map(|m| m.server_path.clone())
                .unwrap_or_default(),
            mosh_port_range: conn
                .mosh
                .as_ref()
                .map(|m| m.port_range.clone())
                .unwrap_or_default(),
            mosh_command: conn.mosh.as_ref().map(|m| m.command.clone()).unwrap_or_default(),
            // The saved id first, then the saved label: on the machine
            // that wrote the host the id resolves; on a second one it
            // was minted elsewhere, and the label is what still names
            // the same shell. Neither resolving leaves the picker on
            // "default shell", which is what the connect path would
            // also report.
            local_terminal_id: conn.local.as_ref().and_then(|l| {
                let entries = self.local_terminals.as_deref().unwrap_or(&[]);
                l.terminal_id
                    .filter(|id| entries.iter().any(|e| e.id == *id))
                    .or_else(|| {
                        let label = l.terminal_label.as_deref()?.trim();
                        entries
                            .iter()
                            .find(|e| e.label.eq_ignore_ascii_case(label))
                            .map(|e| e.id)
                    })
            }),
            local_cwd: conn
                .local
                .as_ref()
                .and_then(|l| l.cwd.clone())
                .unwrap_or_default(),
            address_family: conn.address_family,
            quick_flow: false,
            hostname: conn.hostname.clone(),
            port: conn.port.to_string(),
            username: conn.username.clone().unwrap_or_default(),
            // Never pre-fill the connection password: an untouched
            // SecretInput resolves to None (preserve on save).
            password: Default::default(),
            auth_method: conn.auth_method.clone(),
            // Combo value is the full breadcrumb path (see
            // `rebuild_editor_combos`); a dangling group id prefills
            // empty (root), matching how the grid renders it.
            group_name: conn
                .group_id
                .filter(|gid| self.groups.iter().any(|g| g.id == *gid))
                .map(|gid| oryxis_core::models::Group::path_of(&self.groups, gid))
                .unwrap_or_default(),
            selected_key: conn.key_id.and_then(|kid| {
                self.keys.iter().find(|k| k.id == kid).map(|k| k.label.clone())
            }),
            jump_chain: conn.jump_chain.clone(),
            selected_identity: conn.identity_id.and_then(|iid| {
                self.identities.iter().find(|i| i.id == iid).map(|i| i.label.clone())
            }),
            editing_id: Some(conn.id),
            has_existing_password: has_pw,
            password_visible: false,
            username_focused: false,
            port_forwards: conn.port_forwards.iter().map(|pf| PortForwardForm {
                local_port: pf.local_port.to_string(),
                remote_host: pf.remote_host.clone(),
                remote_port: pf.remote_port.to_string(),
            }).collect(),
            env_vars: conn.env_vars.iter().map(|e| crate::state::EnvVarForm {
                key: e.key.clone(),
                value: e.value.clone(),
            }).collect(),
            mcp_enabled: conn.mcp_enabled,
            monitor_enabled: conn.monitor_enabled,
            // `None` is Auto with an empty row list; Custom keeps its
            // patterns, the empty list included.
            monitor_disks_custom: conn.monitor_disks.is_some(),
            monitor_disks: conn.monitor_disks.clone().unwrap_or_default(),
            agent_forwarding: conn.agent_forwarding,
            x11_forwarding: conn.x11_forwarding,
            session_logging: conn.session_logging,
            // Saved-identity reference takes precedence over
            // an inline proxy when both are populated, mirroring
            // the runtime resolver in `Vault::resolve_proxy`.
            proxy_kind: if let Some(pid) = conn.proxy_identity_id {
                ProxyKind::Identity(pid)
            } else {
                conn.proxy.as_ref().map(|p| match &p.proxy_type {
                    ProxyType::Socks5 => ProxyKind::Socks5,
                    ProxyType::Socks4 => ProxyKind::Socks4,
                    ProxyType::Http => ProxyKind::Http,
                    ProxyType::Command(_) => ProxyKind::Command,
                }).unwrap_or(ProxyKind::None)
            },
            proxy_host: conn.proxy.as_ref().map(|p| p.host.clone()).unwrap_or_default(),
            proxy_port: conn.proxy.as_ref().map(|p| p.port.to_string()).unwrap_or_default(),
            proxy_username: conn.proxy.as_ref().and_then(|p| p.username.clone()).unwrap_or_default(),
            // Never pre-fill proxy_password from the encrypted vault, keep it
            // empty and untouched so save preserves the stored value,
            // mirroring the main connection-password flow.
            proxy_password: Default::default(),
            proxy_command: conn.proxy.as_ref().and_then(|p| match &p.proxy_type {
                ProxyType::Command(cmd) => Some(cmd.clone()),
                _ => None,
            }).unwrap_or_default(),
            has_existing_proxy_password: has_proxy_pw,
            proxy_password_visible: false,
            // Never pre-fill the TOTP secret either; the
            // masked placeholder signals one is stored.
            totp_secret: Default::default(),
            has_existing_totp: has_totp,
            totp_visible: false,
            use_totp: has_totp,
            use_disk_key: conn.use_disk_key,
            identity_file: conn.identity_file.clone().unwrap_or_default(),
            // Resolved once here so the hint is right the moment the
            // drawer opens; every arm that can change it refreshes it
            // through `editor_refresh_disk_key`.
            disk_key_status: oryxis_vault::resolve_disk_key(
                conn.use_disk_key,
                conn.identity_file.as_deref(),
            )
            .status(),
            terminal_theme: conn.terminal_theme.clone(),
            terminal_appearance: conn.terminal_appearance.clone().unwrap_or_default(),
            highlight_rules: conn.highlight_rules.clone().unwrap_or_default(),
            keepalive_interval: conn
                .keepalive_interval
                .map(|n| n.to_string())
                .unwrap_or_default(),
            mac_address: conn.mac_address.clone().unwrap_or_default(),
            // A script deleted while this host still referenced it
            // renders as "off" rather than a blank picker entry, the
            // same rule `resolve_proxy` follows for proxy identities.
            login_script_id: conn
                .login_script_id
                .filter(|sid| self.login_scripts.iter().any(|s| s.id == *sid)),
            login_script_vars: conn
                .login_script_vars
                .iter()
                .map(|v| (v.name.clone(), v.value.clone()))
                .collect(),
            // Never pre-fill the target password either.
            target_password: Default::default(),
            has_existing_target_password: has_target_pw,
            target_password_visible: false,
            // A fresh form has no derived clear parked.
            proxy_password_rescue: Default::default(),
            totp_rescue: Default::default(),
            target_password_rescue: Default::default(),
            login_script_draft: None,
            auto_title: conn.auto_title,
            tags_text: conn.tags.join(", "),
            cloud_transport: conn
                .cloud_ref
                .as_ref()
                .map(|r| r.transport_pref),
            icon_style: conn.icon_style.clone(),
            encoding: conn.encoding.clone(),
            ambiguous_width: conn.ambiguous_width,
            terminal_type: conn.terminal_type.clone(),
            ciphers: conn.ciphers.clone(),
            kex: conn.kex.clone(),
            macs: conn.macs.clone(),
            host_key_algorithms: conn.host_key_algorithms.clone(),
            privacy_mode: conn.privacy_mode,
            sidebar_auto_open: conn.sidebar_auto_open,
            quirks: conn.quirks.unwrap_or_default(),
            rekey_limit_mb: conn
                .rekey_limit_mb
                .map(|n| n.to_string())
                .unwrap_or_default(),
            sftp_initial_path: conn.sftp_initial_path.clone().unwrap_or_default(),
            zmodem_drops: conn.zmodem_drops,
        }
    }

    /// Route one editor message to the part of the host it edits.
    ///
    /// Was a 820-line `match` with 95 arms. The groups are the sections
    /// of the host editor itself, so a new field lands next to the ones
    /// it is shown beside.
    pub(crate) fn handle_editor(&mut self, message: EditorMessage) -> Task<Message> {
        match message {
            m @ (
                EditorMessage::ShowNewConnection
                | EditorMessage::EditConnection(..)
                | EditorMessage::SaveQuickHost(..)
                | EditorMessage::EditQuickHost(..)
                | EditorMessage::EditorSave
                | EditorMessage::EditorConnectWithoutSaving
                | EditorMessage::EditorCancel
                | EditorMessage::RequestDeleteConnection(..)
                | EditorMessage::DeleteConnection(..)
                | EditorMessage::DuplicateConnection(..)
                | EditorMessage::EditorSectionToggled(..)
                | EditorMessage::EditorPresetPicked(..)
            ) => self.handle_editor_lifecycle(m),
            m @ (
                EditorMessage::EditorLabelChanged(..)
                | EditorMessage::EditorTagsChanged(..)
                | EditorMessage::EditorHostnameChanged(..)
                | EditorMessage::EditorPortChanged(..)
                | EditorMessage::EditorUsernameChanged(..)
                | EditorMessage::EditorPasswordChanged(..)
                | EditorMessage::EditorTogglePasswordVisibility
                | EditorMessage::EditorTotpChanged(..)
                | EditorMessage::EditorToggleTotpVisibility
                | EditorMessage::EditorUseTotpToggled
                | EditorMessage::EditorUseDiskKeyToggled
                | EditorMessage::EditorIdentityFileChanged(..)
                | EditorMessage::EditorBrowseIdentityFile
                | EditorMessage::EditorAuthMethodChanged(..)
                | EditorMessage::EditorGroupChanged(..)
                | EditorMessage::EditorKeyChanged(..)
                | EditorMessage::EditorKeyComboOpened
                | EditorMessage::EditorIdentityChanged(..)
                | EditorMessage::EditorIconStyleChanged(..)
                | EditorMessage::EditorProtocolChanged(..)
                | EditorMessage::EditorAddressFamilyChanged(..)
                | EditorMessage::EditorToggleTelnetTls
                | EditorMessage::EditorToggleTelnetTlsInsecure
                | EditorMessage::EditorToggleMosh
                | EditorMessage::EditorMoshServerPathChanged(_)
                | EditorMessage::EditorMoshPortRangeChanged(_)
                | EditorMessage::EditorMoshCommandChanged(_)
                | EditorMessage::EditorLocalTerminalChanged(..)
                | EditorMessage::EditorLocalCwdChanged(..)
            ) => self.handle_editor_identity(m),
            m @ (
                EditorMessage::EditorProxyKindChanged(..)
                | EditorMessage::EditorProxyHostChanged(..)
                | EditorMessage::EditorProxyPortChanged(..)
                | EditorMessage::EditorProxyUsernameChanged(..)
                | EditorMessage::EditorProxyPasswordChanged(..)
                | EditorMessage::EditorToggleProxyPasswordVisibility
                | EditorMessage::EditorProxyCommandChanged(..)
                | EditorMessage::OpenChainEditor
                | EditorMessage::CloseChainEditor
                | EditorMessage::ChainEditorStartAdd
                | EditorMessage::ChainEditorCancelAdd
                | EditorMessage::ChainEditorSearchChanged(..)
                | EditorMessage::ChainEditorAddHop(..)
                | EditorMessage::ChainEditorRemoveHop(..)
                | EditorMessage::ChainEditorMoveHopUp(..)
                | EditorMessage::ChainEditorMoveHopDown(..)
                | EditorMessage::EditorAddPortForward
                | EditorMessage::EditorRemovePortForward(..)
                | EditorMessage::EditorPortFwdLocalPortChanged(..)
                | EditorMessage::EditorPortFwdRemoteHostChanged(..)
                | EditorMessage::EditorPortFwdRemotePortChanged(..)
                | EditorMessage::EditorKeepaliveChanged(..)
                | EditorMessage::EditorMacAddressChanged(..)
            ) => self.handle_editor_network(m),
            m @ (
                EditorMessage::EditorOpacityChanged(..)
                | EditorMessage::EditorBgImageBrowse
                | EditorMessage::EditorBgImagePicked(..)
                | EditorMessage::EditorBgImageModeChanged(..)
                | EditorMessage::EditorBgFitChanged(..)
                | EditorMessage::EditorBgDimChanged(..)
                | EditorMessage::EditorOpenThemePicker
                | EditorMessage::EditorCloseThemePicker
                | EditorMessage::EditorThemePickerFilterChanged(..)
                | EditorMessage::EditorTerminalThemeChanged(..)
                | EditorMessage::EditorEncodingChanged(..)
                | EditorMessage::EditorAmbiguousWidthChanged(..)
                | EditorMessage::EditorTerminalTypeChanged(..)
                | EditorMessage::EditorAutoTitleChanged(..)
                | EditorMessage::EditorPrivacyModeChanged(..)
                | EditorMessage::EditorSidebarAutoOpenChanged(..)
                | EditorMessage::EditorSftpInitialPathChanged(..)
                | EditorMessage::EditorStartupComboOpened
                | EditorMessage::EditorStartupChoiceChanged(..)
                | EditorMessage::EditorInitialCommandChanged(..)
                | EditorMessage::EditorQuirkBackspaceChanged(..)
                | EditorMessage::EditorQuirkHomeEndChanged(..)
                | EditorMessage::EditorQuirkFnKeysChanged(..)
                | EditorMessage::EditorQuirkMouseReportingChanged(..)
                | EditorMessage::EditorQuirkTitleChangeChanged(..)
                | EditorMessage::EditorQuirkOsc52Changed(..)
                | EditorMessage::EditorQuirkOptionAsMetaChanged(..)
                | EditorMessage::EditorQuirkRekeyChanged(..)
                | EditorMessage::EditorAlgoSetAuto(..)
                | EditorMessage::EditorAlgoToggle(..)
            ) => self.handle_editor_terminal(m),
            m @ (
                EditorMessage::EditorToggleMcpEnabled
                | EditorMessage::EditorToggleMonitorEnabled
                | EditorMessage::EditorMonitorDisksCustom(..)
                | EditorMessage::EditorAddMonitorDisk
                | EditorMessage::EditorRemoveMonitorDisk(..)
                | EditorMessage::EditorMonitorDiskChanged(..)
                | EditorMessage::EditorToggleAgentForwarding
                | EditorMessage::EditorToggleX11Forwarding
                | EditorMessage::EditorToggleZmodemDrops
                | EditorMessage::EditorCycleSessionLogging
                | EditorMessage::EditorAddEnvVar
                | EditorMessage::EditorRemoveEnvVar(..)
                | EditorMessage::EditorEnvVarKeyChanged(..)
                | EditorMessage::EditorEnvVarValueChanged(..)
                | EditorMessage::EditorCloudTransportChanged(..)
            ) => self.handle_editor_integration(m),
            m @ (
                EditorMessage::EditorSerialBaudChanged(..)
                | EditorMessage::EditorSerialDataBitsChanged(..)
                | EditorMessage::EditorSerialParityChanged(..)
                | EditorMessage::EditorSerialStopBitsChanged(..)
                | EditorMessage::EditorSerialFlowChanged(..)
                | EditorMessage::EditorSerialLineEndingChanged(..)
                | EditorMessage::EditorSerialLocalEchoToggled
                | EditorMessage::EditorRdKindChanged(..)
                | EditorMessage::EditorRdGatewayChanged(..)
            ) => self.handle_editor_transports(m),
            m @ (
                EditorMessage::HostConfigThemeChanged(..)
                | EditorMessage::HostConfigEncodingChanged(..)
                | EditorMessage::HostConfigAmbiguousWidthChanged(..)
                | EditorMessage::HostConfigTerminalTypeChanged(..)
                | EditorMessage::HostConfigAutoTitleChanged(..)
            ) => self.handle_editor_host_config(m),
            m @ (
                EditorMessage::EditorLoginScriptChanged(..)
                | EditorMessage::EditorLoginScriptComboOpened
                | EditorMessage::EditorLoginScriptVarChanged(..)
                | EditorMessage::EditorTargetPasswordChanged(..)
                | EditorMessage::EditorToggleTargetPasswordVisibility
                | EditorMessage::EditorScriptDraftTemplateChanged(..)
                | EditorMessage::EditorScriptDraftNameChanged(..)
                | EditorMessage::EditorScriptDraftPromptChanged(..)
                | EditorMessage::EditorScriptDraftCreate
                | EditorMessage::EditorScriptDraftCancel
            ) => self.handle_editor_login_script(m),
        }
    }

    /// Resolve the focused pane's connection index, apply `mutate`, persist
    /// it (preserving the password), and refresh in-memory state. When
    /// `repaint` is set (theme changes) the running terminal is repainted
    /// for instant preview. A no-op when the focused pane isn't a saved host.
    pub(crate) fn host_config_apply<F: FnOnce(&mut oryxis_core::models::connection::Connection)>(
        &mut self,
        mutate: F,
        repaint: bool,
    ) {
        let Some(id) = self
            .active_tab
            .and_then(|i| self.tabs.get(i))
            .and_then(|tab| match &tab.active().origin {
                crate::state::PaneOrigin::Host(id) => Some(*id),
                _ => None,
            })
        else {
            return;
        };
        let Some(idx) = self.connections.iter().position(|c| c.id == id) else {
            return;
        };
        mutate(&mut self.connections[idx]);
        let label = self.connections[idx].label.clone();
        if let Some(vault) = &self.vault {
            // `None` preserves the encrypted password column untouched.
            let _ = vault.save_connection(&self.connections[idx], None);
        }
        if repaint {
            self.repaint_terminal_palettes_for_label(&label);
        }
    }
}

/// How the host editor's SSH Key combo narrows / orders the key list,
/// per auth method.
#[derive(Clone, Copy, PartialEq, Eq)]
enum KeyComboFilter {
    /// Every key, vault order (the `Key` method).
    All,
    /// Only certificate-carrying keys (the `Certificate` method, B2.1).
    CertificateOnly,
    /// Every key, security keys first (the `Agent` method's preferred-
    /// identity pick, B3).
    SecurityKeysFirst,
}

/// Option list for the host editor's SSH Key combo, pure so it
/// unit-tests: the `(none)` sentinel first, then the key labels per
/// the filter. `Key` and `Certificate` both decode the private key
/// locally to sign, so they only list rows that HOLD a private
/// (`has_private`); a security-key / public-only row belongs under
/// `Agent`, where the hardware token signs.
fn key_combo_options(
    keys: &[oryxis_core::models::key::SshKey],
    filter: KeyComboFilter,
) -> Vec<String> {
    let mut opts = vec!["(none)".to_string()];
    match filter {
        KeyComboFilter::All => opts.extend(
            keys.iter()
                .filter(|k| k.has_private)
                .map(|k| k.label.clone()),
        ),
        KeyComboFilter::CertificateOnly => opts.extend(
            keys.iter()
                .filter(|k| k.certificate.is_some() && k.has_private)
                .map(|k| k.label.clone()),
        ),
        KeyComboFilter::SecurityKeysFirst => {
            opts.extend(
                keys.iter()
                    .filter(|k| k.algorithm.is_security_key())
                    .map(|k| k.label.clone()),
            );
            opts.extend(
                keys.iter()
                    .filter(|k| !k.algorithm.is_security_key())
                    .map(|k| k.label.clone()),
            );
        }
    }
    opts
}

/// Result of resolving the editor form's proxy section into model
/// fields. `Identity(_)` selections route to `proxy_identity_id`, the
/// other static kinds populate the inline `ProxyConfig`. Note that
/// `password` is left as `None` here, it's persisted in the encrypted
/// `proxy_password` column via `set_proxy_password`, never inside the
/// serialized inline JSON.
pub(crate) struct ProxyResolution {
    pub proxy: Option<oryxis_core::models::connection::ProxyConfig>,
    pub proxy_identity_id: Option<uuid::Uuid>,
}

fn build_proxy_resolution(form: &ConnectionForm) -> Result<ProxyResolution, String> {
    use oryxis_core::models::connection::ProxyConfig;

    match form.proxy_kind {
        ProxyKind::None => Ok(ProxyResolution {
            proxy: None,
            proxy_identity_id: None,
        }),
        ProxyKind::Identity(id) => Ok(ProxyResolution {
            proxy: None,
            proxy_identity_id: Some(id),
        }),
        ProxyKind::Command => {
            if form.proxy_command.trim().is_empty() {
                return Err(crate::i18n::t("proxy_err_command_required").into());
            }
            Ok(ProxyResolution {
                proxy: Some(ProxyConfig {
                    proxy_type: ProxyType::Command(form.proxy_command.clone()),
                    host: String::new(),
                    port: 0,
                    username: None,
                    password: None,
                }),
                proxy_identity_id: None,
            })
        }
        kind @ (ProxyKind::Socks5 | ProxyKind::Socks4 | ProxyKind::Http) => {
            if form.proxy_host.trim().is_empty() {
                return Err(crate::i18n::t("proxy_err_host_required").into());
            }
            let port = form
                .proxy_port
                .parse::<u16>()
                .ok()
                .filter(|p| *p > 0)
                .ok_or_else(|| crate::i18n::t("proxy_err_port_invalid").to_string())?;

            let proxy_type = match kind {
                ProxyKind::Socks5 => ProxyType::Socks5,
                ProxyKind::Socks4 => ProxyType::Socks4,
                ProxyKind::Http => ProxyType::Http,
                _ => unreachable!(),
            };

            Ok(ProxyResolution {
                proxy: Some(ProxyConfig {
                    proxy_type,
                    host: form.proxy_host.clone(),
                    port,
                    username: if form.proxy_username.is_empty() {
                        None
                    } else {
                        Some(form.proxy_username.clone())
                    },
                    password: None,
                }),
                proxy_identity_id: None,
            })
        }
    }
}

mod autosave;
mod lifecycle;
mod identity;
mod login_script;
mod network;
mod terminal;
mod integration;
mod transports;
mod host_config;

#[cfg(test)]
mod key_combo_tests {
    use super::{key_combo_options, KeyComboFilter};
    use oryxis_core::models::key::{KeyAlgorithm, SshKey};

    // A normal key holds a private (has_private = true).
    fn key(label: &str, with_cert: bool) -> SshKey {
        let mut k = SshKey::new(label, KeyAlgorithm::Ed25519);
        k.has_private = true;
        if with_cert {
            k.certificate = Some("ssh-ed25519-cert-v01@openssh.com AAAA... u@h".into());
        }
        k
    }

    // A security key is public-only (has_private = false).
    fn sk(label: &str) -> SshKey {
        SshKey::new(label, KeyAlgorithm::SkEd25519)
    }

    #[test]
    fn unfiltered_lists_every_key_after_the_sentinel() {
        let keys = vec![key("bare", false), key("certified", true)];
        assert_eq!(
            key_combo_options(&keys, KeyComboFilter::All),
            vec!["(none)", "bare", "certified"]
        );
    }

    #[test]
    fn key_mode_excludes_public_only_rows() {
        // A security key (no private) can never authenticate under `Key`;
        // it must not appear in the combo.
        let keys = vec![key("bare", false), sk("yubi")];
        assert_eq!(
            key_combo_options(&keys, KeyComboFilter::All),
            vec!["(none)", "bare"]
        );
    }

    #[test]
    fn certificate_mode_lists_only_cert_carrying_keys() {
        let keys = vec![key("bare", false), key("certified", true), key("plain2", false)];
        assert_eq!(
            key_combo_options(&keys, KeyComboFilter::CertificateOnly),
            vec!["(none)", "certified"]
        );
    }

    #[test]
    fn certificate_mode_excludes_public_only_even_with_a_cert() {
        // A public-only row can carry a cert (delegation), but `Certificate`
        // auth signs with the local private, which it lacks.
        let mut yubi_cert = sk("yubi-cert");
        yubi_cert.certificate = Some("sk-ssh-ed25519-cert-v01@openssh.com AAAA... u@h".into());
        let keys = vec![key("certified", true), yubi_cert];
        assert_eq!(
            key_combo_options(&keys, KeyComboFilter::CertificateOnly),
            vec!["(none)", "certified"]
        );
    }

    #[test]
    fn certificate_mode_with_no_certs_keeps_the_sentinel_only() {
        let keys = vec![key("bare", false)];
        assert_eq!(
            key_combo_options(&keys, KeyComboFilter::CertificateOnly),
            vec!["(none)"]
        );
    }

    #[test]
    fn agent_mode_lists_security_keys_first_including_public_only() {
        // Agent delegates signing, so public-only rows DO belong here.
        let keys = vec![key("bare", false), sk("yubi"), key("other", true), sk("solo")];
        assert_eq!(
            key_combo_options(&keys, KeyComboFilter::SecurityKeysFirst),
            vec!["(none)", "yubi", "solo", "bare", "other"]
        );
    }
}
