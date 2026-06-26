//! `Oryxis::handle_editor`, match arms for the connection editor:
//! field changes, save/cancel/duplicate/delete, port-forwarding edits,
//! identity selection, MCP-enabled toggle, OS detection.

#![allow(clippy::result_large_err)]

use iced::Task;

use oryxis_core::models::connection::{AuthMethod, Connection, ProxyType};
use oryxis_core::models::group::Group;

use crate::app::{Message, Oryxis};
use crate::state::{ConnectionForm, PortForwardForm, ProxyKind};

impl Oryxis {
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
        let visible = self.visible_group_ids();
        let mut labels: Vec<String> = self
            .groups
            .iter()
            .filter(|g| visible.contains(&g.id))
            .map(|g| g.label.clone())
            .collect();
        labels.sort_by_key(|s| s.to_lowercase());
        labels.dedup();
        let selection = self.editor_form.group_name.clone();
        let selection = (!selection.is_empty()).then_some(selection);
        self.editor_parent_combo =
            iced::widget::combo_box::State::with_selection(labels, selection.as_ref());

        self.reset_editor_startup_combo();
        self.reset_editor_key_combo();
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
    /// then every saved key's label.
    fn editor_key_options(&self) -> Vec<String> {
        let mut opts = vec!["(none)".to_string()];
        opts.extend(self.keys.iter().map(|k| k.label.clone()));
        opts
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

    pub(crate) fn handle_editor(
        &mut self,
        message: Message,
    ) -> Result<Task<Message>, Message> {
        match message {
            // -- Connection editor --
            Message::ShowNewConnection => {
                // Mutually exclusive right-panel slot, close any
                // other panel before opening the host editor.
                self.cloud_form_visible = false;
                self.cloud_dynamic_form_visible = false;
                self.cloud_discover_visible = false;
                self.show_session_group_panel = false;
                self.group_edit_visible = false;
                self.show_host_panel = true;
                self.editor_form = ConnectionForm::default();
                self.editor_initial_command = iced::widget::text_editor::Content::new();
                self.editor_startup_choice = crate::state::StartupChoice::None;
                if let Some(gid) = self.active_group
                    && let Some(g) = self.groups.iter().find(|g| g.id == gid)
                {
                    self.editor_form.group_name = g.label.clone();
                }
                self.host_panel_error = None;
                self.rebuild_editor_combos();
                // Land the cursor in the first field so the very first
                // Tab keypress walks the form (focus_next with nothing
                // focused would otherwise grab the grid search input).
                return Ok(iced::widget::operation::focus(iced::widget::Id::new(
                    "editor-hostname",
                )));
            }
            Message::EditConnection(idx) => {
                self.card_context_menu = None;
                self.overlay = None;
                if let Some(conn) = self.connections.get(idx) {
                    // Mutually exclusive right-panel slot.
                    self.cloud_form_visible = false;
                    self.cloud_dynamic_form_visible = false;
                    self.cloud_discover_visible = false;
                    self.show_session_group_panel = false;
                    self.group_edit_visible = false;
                    self.show_host_panel = true;
                    self.host_panel_error = None;
                    let has_pw = self.vault.as_ref()
                        .and_then(|v| v.get_connection_password(&conn.id).ok())
                        .flatten()
                        .is_some();
                    let has_proxy_pw = self.vault.as_ref()
                        .and_then(|v| v.get_proxy_password(&conn.id).ok())
                        .flatten()
                        .is_some();
                    self.editor_form = ConnectionForm {
                        label: conn.label.clone(),
                        hostname: conn.hostname.clone(),
                        port: conn.port.to_string(),
                        username: conn.username.clone().unwrap_or_default(),
                        password: String::new(),
                        auth_method: conn.auth_method.clone(),
                        group_name: conn
                            .group_id
                            .and_then(|gid| {
                                self.groups.iter().find(|g| g.id == gid).map(|g| g.label.clone())
                            })
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
                        password_touched: false,
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
                        agent_forwarding: conn.agent_forwarding,
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
                        // Never pre-fill proxy_password from the encrypted vault, keep it empty
                        // and let `proxy_password_touched` decide whether to overwrite on save,
                        // mirroring the main connection-password flow.
                        proxy_password: String::new(),
                        proxy_command: conn.proxy.as_ref().and_then(|p| match &p.proxy_type {
                            ProxyType::Command(cmd) => Some(cmd.clone()),
                            _ => None,
                        }).unwrap_or_default(),
                        has_existing_proxy_password: has_proxy_pw,
                        proxy_password_touched: false,
                        terminal_theme: conn.terminal_theme.clone(),
                        keepalive_interval: conn
                            .keepalive_interval
                            .map(|n| n.to_string())
                            .unwrap_or_default(),
                        auto_title: conn.auto_title,
                        cloud_transport: conn
                            .cloud_ref
                            .as_ref()
                            .map(|r| r.transport_pref),
                        icon_style: conn.icon_style.clone(),
                        encoding: conn.encoding.clone(),
                    };
                    let cmd = conn.initial_command.as_deref().unwrap_or_default();
                    self.editor_initial_command =
                        iced::widget::text_editor::Content::with_text(cmd);
                    // Recover the startup source: a live snippet reference
                    // (whose snippet still exists) wins; else a non-empty
                    // literal command is Custom; else None. A dangling
                    // snippet id falls back to None.
                    self.editor_startup_choice = match conn.startup_snippet_id {
                        Some(id) if self.snippets.iter().any(|s| s.id == id) => {
                            crate::state::StartupChoice::Snippet(id)
                        }
                        _ if !cmd.trim().is_empty() => crate::state::StartupChoice::Custom,
                        _ => crate::state::StartupChoice::None,
                    };
                    self.rebuild_editor_combos();
                    return Ok(iced::widget::operation::focus(iced::widget::Id::new(
                        "editor-hostname",
                    )));
                }
            }
            Message::EditorLabelChanged(v) => { self.editor_form.label = v; self.editor_form.username_focused = false; }
            Message::EditorHostnameChanged(v) => { self.editor_form.hostname = v; self.editor_form.username_focused = false; }
            Message::EditorPortChanged(v) => { self.editor_form.port = v; self.editor_form.username_focused = false; }
            Message::EditorUsernameChanged(v) => {
                self.editor_form.username = v;
                self.editor_form.username_focused = true;
            }
            Message::EditorPasswordChanged(v) => {
                self.editor_form.password_touched = true;
                self.editor_form.username_focused = false;
                self.editor_form.password = v;
            }
            Message::EditorTogglePasswordVisibility => {
                self.editor_form.password_visible = !self.editor_form.password_visible;
            }
            Message::EditorAuthMethodChanged(v) => {
                use crate::i18n::t;
                // Match against the *localized* labels emitted by the
                // pick_list. Falling back to English keeps stale-label
                // dispatches (e.g. setting persisted in another locale)
                // still resolvable.
                self.editor_form.auth_method = if v == t("auth_password") || v == "Password" {
                    AuthMethod::Password
                } else if v == t("auth_key") || v == "Key" {
                    AuthMethod::Key
                } else if v == t("auth_agent") || v == "Agent" {
                    AuthMethod::Agent
                } else if v == t("auth_interactive") || v == "Interactive" {
                    AuthMethod::Interactive
                } else {
                    AuthMethod::Auto
                };
            }
            Message::EditorGroupChanged(v) => self.editor_form.group_name = v,
            Message::EditorKeyChanged(v) => {
                self.editor_form.selected_key = if v == "(none)" { None } else { Some(v) };
            }
            Message::EditorKeyComboOpened => {
                // Focus clears the typed value so the dropdown opens on
                // the full key list, not pre-filtered by the current pick.
                self.reset_editor_key_combo();
            }
            Message::OpenChainEditor => {
                self.show_chain_editor = true;
                self.chain_editor_adding = false;
                self.chain_editor_search.clear();
            }
            Message::CloseChainEditor => {
                self.show_chain_editor = false;
                self.chain_editor_adding = false;
                self.chain_editor_search.clear();
            }
            Message::ChainEditorStartAdd => {
                self.chain_editor_adding = true;
                self.chain_editor_search.clear();
            }
            Message::ChainEditorCancelAdd => {
                self.chain_editor_adding = false;
                self.chain_editor_search.clear();
            }
            Message::ChainEditorSearchChanged(v) => {
                self.chain_editor_search = v;
            }
            Message::ChainEditorAddHop(id) => {
                // Append the hop, ignoring duplicates so the same host
                // can't appear twice in one chain.
                if !self.editor_form.jump_chain.contains(&id) {
                    self.editor_form.jump_chain.push(id);
                }
                self.chain_editor_adding = false;
                self.chain_editor_search.clear();
            }
            Message::ChainEditorRemoveHop(idx) => {
                if idx < self.editor_form.jump_chain.len() {
                    self.editor_form.jump_chain.remove(idx);
                }
            }
            Message::ChainEditorMoveHopUp(idx) => {
                if idx > 0 && idx < self.editor_form.jump_chain.len() {
                    self.editor_form.jump_chain.swap(idx, idx - 1);
                }
            }
            Message::ChainEditorMoveHopDown(idx) => {
                if idx + 1 < self.editor_form.jump_chain.len() {
                    self.editor_form.jump_chain.swap(idx, idx + 1);
                }
            }
            Message::EditorProxyKindChanged(kind) => {
                let prev = self.editor_form.proxy_kind;
                self.editor_form.proxy_kind = kind;
                match kind {
                    ProxyKind::Identity(_) => {
                        // Switching to a saved identity, wipe inline state
                        // so a later switch back to Custom starts clean.
                        // The identity carries its own host/port/username/
                        // password, all hydrated by `resolve_proxy` at
                        // connect time.
                        self.editor_form.proxy_host.clear();
                        self.editor_form.proxy_port.clear();
                        self.editor_form.proxy_username.clear();
                        self.editor_form.proxy_password.clear();
                        self.editor_form.proxy_command.clear();
                        self.editor_form.proxy_password_touched = false;
                    }
                    _ => {
                        // Coming back from an Identity selection: empty
                        // form, fall through to default-port pre-fill.
                        if matches!(prev, ProxyKind::Identity(_)) {
                            self.editor_form.proxy_host.clear();
                            self.editor_form.proxy_port.clear();
                            self.editor_form.proxy_username.clear();
                            self.editor_form.proxy_password.clear();
                            self.editor_form.proxy_command.clear();
                            self.editor_form.proxy_password_touched = false;
                        }
                        // Pre-fill the canonical port for the chosen type
                        // when the field is still blank, saves the user a
                        // hop and is easy to override by typing.
                        if self.editor_form.proxy_port.is_empty()
                            && let Some(default_port) = kind.default_port()
                        {
                            self.editor_form.proxy_port = default_port.to_string();
                        }
                    }
                }
            }
            Message::EditorProxyHostChanged(v) => { self.editor_form.proxy_host = v; }
            Message::EditorProxyPortChanged(v) => { self.editor_form.proxy_port = v; }
            Message::EditorProxyUsernameChanged(v) => { self.editor_form.proxy_username = v; }
            Message::EditorProxyPasswordChanged(v) => {
                self.editor_form.proxy_password_touched = true;
                self.editor_form.proxy_password = v;
            }
            Message::EditorProxyCommandChanged(v) => { self.editor_form.proxy_command = v; }
            Message::EditorOpenThemePicker => {
                self.show_theme_picker = true;
            }
            Message::EditorCloseThemePicker => {
                self.show_theme_picker = false;
            }
            Message::EditorTerminalThemeChanged(name) => {
                // Empty string == "inherit the global pick".
                self.editor_form.terminal_theme =
                    if name.is_empty() { None } else { Some(name) };
                self.show_theme_picker = false;
            }
            Message::EditorCloudTransportChanged(t) => {
                self.editor_form.cloud_transport = Some(t);
            }
            Message::EditorInitialCommandChanged(action) => {
                self.editor_initial_command.perform(action);
            }
            Message::EditorStartupComboOpened => {
                // Focus clears the typed value so the dropdown opens on
                // the full snippet list, not pre-filtered by the current
                // selection (the committed choice is preserved untouched).
                self.reset_editor_startup_combo();
            }
            Message::EditorStartupChoiceChanged(label) => {
                use crate::state::StartupChoice;
                // Map the picker label back to a source. The None / Custom
                // sentinels come from i18n; anything else is a snippet
                // label. A snippet is stored as a live reference (its id),
                // resolved to the snippet body at connect time, so we
                // don't copy the body into the custom text editor here.
                if label == crate::i18n::t("startup_none") {
                    self.editor_startup_choice = StartupChoice::None;
                    self.editor_initial_command =
                        iced::widget::text_editor::Content::new();
                } else if label == crate::i18n::t("startup_custom") {
                    self.editor_startup_choice = StartupChoice::Custom;
                } else if let Some(s) =
                    self.snippets.iter().find(|s| s.label == label)
                {
                    self.editor_startup_choice = StartupChoice::Snippet(s.id);
                }
            }
            Message::EditorIconStyleChanged(v) => {
                // "" clears the override; anything else is normalized to
                // the known set so a stale UI value can't smuggle in a
                // string the renderer doesn't understand.
                self.editor_form.icon_style = match v.as_str() {
                    "circular" | "square" | "rounded" | "outline" | "initials" => Some(v),
                    _ => None,
                };
            }
            Message::EditorEncodingChanged(v) => {
                // "UTF-8" is the implicit default, stored as None so the
                // SSH engine skips transcoding entirely.
                self.editor_form.encoding = if v == "UTF-8" { None } else { Some(v) };
            }
            Message::EditorKeepaliveChanged(v) => {
                // Digits only; preserve empty (= inherit global). Cap at
                // 86_400s (1 day) like the global setting field, so users
                // can't accidentally type a runaway value.
                let digits: String = v.chars().filter(|c| c.is_ascii_digit()).collect();
                self.editor_form.keepalive_interval = if digits.is_empty() {
                    String::new()
                } else {
                    let n: u64 = digits.parse().unwrap_or(86_400);
                    n.min(86_400).to_string()
                };
            }
            Message::EditorAutoTitleChanged(v) => {
                use crate::i18n::t;
                // Map the localized pick label back to the tri-state override.
                self.editor_form.auto_title = if v == t("host_auto_title_show") {
                    Some(true)
                } else if v == t("host_auto_title_hide") {
                    Some(false)
                } else {
                    None
                };
            }
            Message::EditorSave => {
                if self.editor_form.label.is_empty() || self.editor_form.hostname.is_empty() {
                    self.host_panel_error = Some("Label and hostname are required".into());
                    return Ok(Task::none());
                }
                let port: u16 = self.editor_form.port.parse().unwrap_or(22);

                // Find or create group
                let group_id = if !self.editor_form.group_name.is_empty() {
                    let existing = self
                        .groups
                        .iter()
                        .find(|g| g.label == self.editor_form.group_name);
                    match existing {
                        Some(g) => Some(g.id),
                        None => {
                            let g = Group::new(&self.editor_form.group_name);
                            let gid = g.id;
                            if let Some(vault) = &self.vault {
                                let _ = vault.save_group(&g);
                            }
                            self.groups.push(g);
                            Some(gid)
                        }
                    }
                } else {
                    None
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
                conn.mcp_enabled = self.editor_form.mcp_enabled;
                conn.agent_forwarding = self.editor_form.agent_forwarding;
                conn.session_logging = self.editor_form.session_logging;
                conn.terminal_theme = self.editor_form.terminal_theme.clone();
                conn.icon_style = self.editor_form.icon_style.clone();
                conn.encoding = self.editor_form.encoding.clone();
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
                // Map the editor form into either an inline ProxyConfig
                // or a `proxy_identity_id` reference. Validates host /
                // port / command up-front so the user gets an error
                // instead of a silently-broken proxy entry.
                match build_proxy_resolution(&self.editor_form) {
                    Ok(r) => {
                        conn.proxy = r.proxy;
                        conn.proxy_identity_id = r.proxy_identity_id;
                    }
                    Err(msg) => {
                        self.host_panel_error = Some(msg);
                        return Ok(Task::none());
                    }
                }
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

                let password = if !self.editor_form.password_touched {
                    None // User didn't touch the field, preserve existing password
                } else if self.editor_form.password.is_empty() {
                    Some("") // User cleared the password, remove it
                } else {
                    Some(self.editor_form.password.as_str())
                };

                if let Some(vault) = &self.vault {
                    match vault.save_connection(&conn, password) {
                        Ok(()) => {
                            // Persist the encrypted proxy password in its own
                            // column. We only touch it when the user edited
                            // the field, mirroring `password_touched` for the
                            // main connection password.
                            if self.editor_form.proxy_password_touched {
                                let pw = if self.editor_form.proxy_password.is_empty() {
                                    None
                                } else {
                                    Some(self.editor_form.proxy_password.as_str())
                                };
                                let _ = vault.set_proxy_password(&conn.id, pw);
                            }
                            // If the proxy was disabled in this save, drop any
                            // previously stored proxy password, keeping a
                            // dangling encrypted credential would be surprising.
                            if conn.proxy.is_none() {
                                let _ = vault.set_proxy_password(&conn.id, None);
                            }
                            self.show_host_panel = false;
                            self.host_panel_error = None;
                            // Re-paint any open tabs of this host so a
                            // newly chosen palette takes effect without
                            // a reconnect.
                            let host_label = conn.label.clone();
                            self.load_data_from_vault();
                            self.repaint_terminal_palettes_for_label(&host_label);
                        }
                        Err(e) => {
                            self.host_panel_error = Some(e.to_string());
                        }
                    }
                }
            }
            Message::EditorCancel => {
                self.show_host_panel = false;
                self.host_panel_error = None;
            }
            Message::RequestDeleteConnection(idx) => {
                if let Some(conn) = self.connections.get(idx) {
                    let name = conn.label.clone();
                    self.confirm_remove(name, Message::DeleteConnection(idx));
                }
            }
            Message::DeleteConnection(idx) => {
                self.card_context_menu = None;
                self.overlay = None;
                if let Some(conn) = self.connections.get(idx) {
                    let id = conn.id;
                    if let Some(vault) = &self.vault {
                        let _ = vault.delete_connection(&id);
                        self.show_host_panel = false;
                        self.load_data_from_vault();
                    }
                }
            }
            Message::DuplicateConnection(idx) => {
                self.card_context_menu = None;
                self.overlay = None;
                if let Some(conn) = self.connections.get(idx).cloned() {
                    let mut dup = Connection::new(
                        format!("{} (copy)", conn.label),
                        &conn.hostname,
                    );
                    dup.port = conn.port;
                    dup.username = conn.username.clone();
                    dup.auth_method = conn.auth_method.clone();
                    dup.key_id = conn.key_id;
                    dup.group_id = conn.group_id;
                    dup.jump_chain = conn.jump_chain.clone();
                    dup.port_forwards = conn.port_forwards.clone();
                    dup.proxy = conn.proxy.clone();
                    dup.tags = conn.tags.clone();
                    dup.notes = conn.notes.clone();
                    dup.color = conn.color.clone();
                    dup.agent_forwarding = conn.agent_forwarding;
                    if let Some(vault) = &self.vault {
                        // Copy password and proxy password to the duplicate.
                        let pw = vault.get_connection_password(&conn.id).ok().flatten();
                        let proxy_pw = vault.get_proxy_password(&conn.id).ok().flatten();
                        let _ = vault.save_connection(&dup, pw.as_deref());
                        if proxy_pw.is_some() {
                            let _ = vault.set_proxy_password(&dup.id, proxy_pw.as_deref());
                        }
                        self.load_data_from_vault();
                    }
                }
            }
            // ── Connection identity ──
            Message::EditorIdentityChanged(v) => {
                self.editor_form.username_focused = false;
                if v == "(none)" {
                    self.editor_form.selected_identity = None;
                } else {
                    self.editor_form.selected_identity = Some(v);
                }
            }

            m => return Err(m),
        }
        Ok(Task::none())
    }
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
