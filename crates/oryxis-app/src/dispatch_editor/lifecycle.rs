//! Opening the editor, saving, and what happens to the host itself.
//!
//! The arms that create, save, delete or duplicate a `Connection`, plus
//! "connect without saving". These are the only ones here that touch the
//! vault; everything else in this module edits the form in memory.

use super::*;

impl Oryxis {
    /// Sweep the host editor's secret buffers once the panel closes, so
    /// a revealed (decrypted) stored value doesn't keep sitting in RAM
    /// behind a closed panel. Only untouched buffers are cleared: those
    /// hold the prefilled STORED plaintext shown by the eye toggle,
    /// which the vault re-decrypts on demand the next time the eye is
    /// opened. A touched buffer holds the user's own typing (new host /
    /// replaced password); the author's original code always let that
    /// linger until the next editor open, and sweeping it would eat
    /// work-in-progress when the panel is merely switched to another
    /// one. `clear()` never sets the touched flag, so the tri-state save
    /// semantics are untouched.
    pub(crate) fn sweep_editor_secrets(&mut self) {
        if !self.editor_form.password.touched() {
            self.editor_form.password.clear();
        }
        if !self.editor_form.proxy_password.touched() {
            self.editor_form.proxy_password.clear();
        }
        if !self.editor_form.totp_secret.touched() {
            self.editor_form.totp_secret.clear();
        }
        if !self.editor_form.target_password.touched() {
            self.editor_form.target_password.clear();
        }
    }

    pub(super) fn handle_editor_lifecycle(&mut self, message: EditorMessage) -> Task<Message> {
        match message {
            EditorMessage::ShowNewConnection => {
                return self.open_new_host_editor(
                    oryxis_core::models::connection::ConnectionProtocol::Ssh,
                );
            }
            EditorMessage::ShowNewRemoteDesktop => {
                return self.open_new_host_editor(
                    oryxis_core::models::connection::ConnectionProtocol::RemoteDesktop,
                );
            }
            EditorMessage::EditConnection(idx) => {
                self.card_context_menu = None;
                self.overlay = None;
                if let Some(conn) = self.connections.get(idx) {
                    // Mutually exclusive right-panel slot.
                    self.cloud_form.visible = false;
                    self.cloud_dynamic_form.visible = false;
                    self.cloud_discover.visible = false;
                    self.panels.session_group_panel = false;
                    self.group_edit.visible = false;
                    self.panels.host_panel = true;
                    // When invoked from a focused terminal tab (the
                    // OpenPortForwards / edit-host hotkey), leave the
                    // terminal surface so the right-panel editor actually
                    // renders: it only shows when no tab is focused.
                    // Without this the flag sticks true and silently
                    // disables Ctrl+Tab MRU, IME routing and sidebar
                    // keynav. The tab keeps running in the background.
                    if self.active_tab.is_some() {
                        self.active_tab = None;
                        self.active_view = crate::state::View::Dashboard;
                    }
                    // Inline panel_nav_clear: a method call would
                    // borrow all of self while `conn` holds it.
                    self.keynav.panel_selected = None;
                    self.keynav.panel_last_row.set(None);
                    self.host_panel_error = None;
                    let has_pw = self.vault.as_ref()
                        .and_then(|v| v.get_connection_password(&conn.id).ok())
                        .flatten()
                        .is_some();
                    let has_proxy_pw = self.vault.as_ref()
                        .and_then(|v| v.get_proxy_password(&conn.id).ok())
                        .flatten()
                        .is_some();
                    let has_totp = self.vault.as_ref()
                        .and_then(|v| v.get_connection_totp_secret(&conn.id).ok())
                        .flatten()
                        .is_some();
                    let has_target_pw = self.vault.as_ref()
                        .and_then(|v| v.get_connection_target_password(&conn.id).ok())
                        .flatten()
                        .is_some();
                    self.editor_form = self.form_from_connection(
                        conn,
                        has_pw,
                        has_proxy_pw,
                        has_totp,
                        has_target_pw,
                    );
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
                    return crate::widgets::focus_input(iced::widget::Id::new(
                        "editor-hostname",
                    ));
                }
            }
            EditorMessage::SaveQuickHost(id) => {
                self.overlay = None;
                self.card_context_menu = None;
                let Some(entry) = self.quick_connects.get(&id).cloned() else {
                    return Task::none();
                };
                // Mutually exclusive right-panel slot, and the panel lives
                // on the dashboard (the menu was clicked from a terminal).
                self.cloud_form.visible = false;
                self.cloud_dynamic_form.visible = false;
                self.cloud_discover.visible = false;
                self.panels.session_group_panel = false;
                self.group_edit.visible = false;
                self.panels.host_panel = true;
                self.panel_nav_clear();
                self.host_panel_error = None;
                self.active_view = crate::state::View::Dashboard;
                let mut form = self.form_from_connection(&entry.conn, false, false, false, false);
                // Prefill as a NEW host: saving must insert a fresh row,
                // never overwrite; the open tab stays ephemeral until its
                // next reconnect.
                form.editing_id = None;
                // Re-seed the credentials typed in the editor flow so the
                // save persists them (set marks touched => tri-state writes).
                if let Some(pw) = entry.password.clone() {
                    form.password.set(pw);
                }
                if let Some(secret) = entry.totp_secret.clone() {
                    form.totp_secret.set(secret);
                    form.use_totp = true;
                }
                if let Some(pw) = entry.proxy_password.clone() {
                    form.proxy_password.set(pw);
                }
                self.editor_form = form;
                let cmd = entry.conn.initial_command.as_deref().unwrap_or_default();
                self.editor_initial_command =
                    iced::widget::text_editor::Content::with_text(cmd);
                self.editor_startup_choice = match entry.conn.startup_snippet_id {
                    Some(sid) if self.snippets.iter().any(|s| s.id == sid) => {
                        crate::state::StartupChoice::Snippet(sid)
                    }
                    _ if !cmd.trim().is_empty() => crate::state::StartupChoice::Custom,
                    _ => crate::state::StartupChoice::None,
                };
                self.rebuild_editor_combos();
                return crate::widgets::focus_input(iced::widget::Id::new(
                    "editor-hostname",
                ));
            }
            EditorMessage::EditQuickHost(id) => {
                // Same prefill as SaveQuickHost; the flag only swaps the
                // footer emphasis (Connect primary / Save secondary) so
                // the flow reads as "edit the temporary host and dial",
                // never an implicit vault write.
                let task = self.update(Message::Editor(EditorMessage::SaveQuickHost(id)));
                if self.panels.host_panel {
                    self.editor_form.quick_flow = true;
                }
                return task;
            }
            EditorMessage::EditorSave => {
                // Every field in this panel carries `on_submit(EditorSave)`,
                // and the fork's `text_input` runs that binding on ANY
                // Enter, focused or not (the `is_focused` gate in
                // `from_key_press` sits behind the on_submit shortcut).
                // A blocking modal owns the keyboard through
                // `any_modal_blocks_input`, but that governs the global key
                // subscription, not the widget tree, so an Enter aimed at a
                // modal OVER this panel also reached here and rebuilt the
                // working copy under it (a highlight rule added in the modal
                // was dropped the moment Enter saved it). The panel's own
                // Save button cannot be clicked while a modal is up (the
                // scrim absorbs the click), so this only ever discards a
                // stray Enter.
                if self.any_modal_blocks_input() {
                    return Task::none();
                }
                if self.editor_form.label.is_empty() || self.editor_form.hostname.is_empty() {
                    self.host_panel_error =
                        Some(crate::i18n::t("editor_label_host_required").to_string());
                    return Task::none();
                }
                let conn = match self.connection_from_editor_form(true) {
                    Ok(conn) => conn,
                    Err(msg) => {
                        self.host_panel_error = Some(msg);
                        return Task::none();
                    }
                };
                // Tri-state: untouched preserves the stored password,
                // cleared removes it, typed stores (SecretInput::resolve).
                let password = self.editor_form.password.resolve();

                if let Some(vault) = &self.vault {
                    match vault.save_connection(&conn, password) {
                        Ok(()) => {
                            // Persist the encrypted proxy password in its own
                            // column. We only touch it when the user edited
                            // the field (resolve returns Some), mirroring the
                            // main connection password; an edited-empty field
                            // maps to None = remove for this setter.
                            if let Some(pw) = self.editor_form.proxy_password.resolve() {
                                let _ = vault.set_proxy_password(
                                    &conn.id,
                                    (!pw.is_empty()).then_some(pw),
                                );
                            }
                            // If the proxy was disabled in this save, drop any
                            // previously stored proxy password, keeping a
                            // dangling encrypted credential would be surprising.
                            if conn.proxy.is_none() {
                                let _ = vault.set_proxy_password(&conn.id, None);
                            }
                            // TOTP secret, same touched tri-state as the
                            // proxy password (empty input clears). TOTP is
                            // SSH-only (keyboard-interactive 2FA); if the
                            // protocol was switched to Telnet/Serial/RDP the
                            // field is hidden, so clear any secret rather than
                            // persisting dead credential material, mirroring
                            // the `mcp_enabled` SSH clamp above. Toggling
                            // "Use TOTP" off clears the same way: the field
                            // is gone from the form, keeping the secret
                            // would be surprising.
                            let is_ssh = self.editor_form.protocol
                                == oryxis_core::models::connection::ConnectionProtocol::Ssh;
                            if !is_ssh || !self.editor_form.use_totp {
                                let _ = vault.set_connection_totp_secret(&conn.id, None);
                            } else if let Some(secret) =
                                self.editor_form.totp_secret.resolve()
                            {
                                let s = secret.trim();
                                let s = (!s.is_empty()).then_some(s);
                                let _ = vault.set_connection_totp_secret(&conn.id, s);
                            }
                            // Target password, same clamp: it only means
                            // anything while a login script is attached
                            // (the save above already dropped the
                            // reference on a non-SSH protocol), so
                            // detaching the script clears the credential
                            // instead of leaving it stranded.
                            if conn.login_script_id.is_none() {
                                let _ = vault.set_connection_target_password(&conn.id, None);
                            } else if let Some(pw) =
                                self.editor_form.target_password.resolve()
                            {
                                let _ = vault.set_connection_target_password(&conn.id, Some(pw));
                            }
                            self.panels.host_panel = false;
                            self.panel_nav_clear();
                            self.host_panel_error = None;
                            // The save consumed the secrets; drop any
                            // plaintext the eye may have revealed.
                            self.sweep_editor_secrets();
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
            EditorMessage::EditorConnectWithoutSaving => {
                // Ad-hoc connect from the "+ Host" flow: build the full
                // Connection from the form but persist nothing. Only the
                // hostname is required; an empty label defaults to the
                // canonical user@host[:port].
                if self.editor_form.hostname.is_empty() {
                    self.host_panel_error =
                        Some(crate::i18n::t("quick_connect_hostname_required").into());
                    return Task::none();
                }
                let mut conn = match self.connection_from_editor_form(false) {
                    Ok(conn) => conn,
                    Err(msg) => {
                        self.host_panel_error = Some(msg);
                        return Task::none();
                    }
                };
                if conn.label.is_empty() {
                    conn.label = oryxis_core::ssh_target::SshTarget {
                        username: conn.username.clone(),
                        host: conn.hostname.clone(),
                        port: (conn.port != 22).then_some(conn.port),
                    }
                    .canonical();
                }
                // Typed credentials ride the ephemeral entry (there is no
                // vault row to hydrate from at connect time). Untouched or
                // cleared fields stay None.
                let form = &self.editor_form;
                let password = form
                    .password
                    .resolve()
                    .filter(|pw| !pw.is_empty())
                    .map(str::to_string);
                let totp_secret = form
                    .totp_secret
                    .resolve()
                    .filter(|_| form.use_totp)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
                let proxy_password = if conn.proxy.is_some() {
                    form.proxy_password
                        .resolve()
                        .filter(|pw| !pw.is_empty())
                        .map(str::to_string)
                } else {
                    None
                };
                let entry = crate::state::QuickConnectEntry {
                    conn,
                    password,
                    totp_secret,
                    proxy_password,
                };
                self.panels.host_panel = false;
                self.panel_nav_clear();
                self.host_panel_error = None;
                // The typed secrets rode into the quick-connect entry;
                // drop any revealed plaintext from the form buffers.
                self.sweep_editor_secrets();
                return self.update(Message::Ssh(SshMessage::QuickConnect(Box::new(entry))));
            }
            EditorMessage::EditorCancel => {
                self.panels.host_panel = false;
                self.panel_nav_clear();
                self.host_panel_error = None;
                // Nothing was saved; sweep revealed stored plaintext
                // (typed edits survive, the author's original behavior).
                self.sweep_editor_secrets();
            }
            EditorMessage::RequestDeleteConnection(idx) => {
                if let Some(conn) = self.connections.get(idx) {
                    let name = conn.label.clone();
                    self.confirm_remove(name, Message::Editor(EditorMessage::DeleteConnection(idx)));
                }
            }
            EditorMessage::DeleteConnection(idx) => {
                self.card_context_menu = None;
                self.overlay = None;
                if let Some(conn) = self.connections.get(idx) {
                    let id = conn.id;
                    if let Some(vault) = &self.vault {
                        let _ = vault.delete_connection(&id);
                        // Saved AI conversations reference the host by id, so
                        // they go with it instead of dangling against an id
                        // that no longer resolves.
                        let _ = vault.delete_chat_conversations_for_connection(&id);
                        self.panels.host_panel = false;
                        self.panel_nav_clear();
                        self.sweep_editor_secrets();
                        self.load_data_from_vault();
                    }
                }
            }
            EditorMessage::DuplicateConnection(idx) => {
                self.card_context_menu = None;
                self.overlay = None;
                if let Some(conn) = self.connections.get(idx).cloned() {
                    // Clone, then reset what must NOT carry. The
                    // previous hand-written copy list had silently
                    // fallen behind the model (a duplicate lost its
                    // MAC, startup command, terminal theme, monitoring
                    // opt-in and keepalive), and every new field would
                    // have joined that list. Copying by default and
                    // naming the exceptions is the version that cannot
                    // drift.
                    let now = chrono::Utc::now();
                    let mut dup = conn.clone();
                    dup.id = uuid::Uuid::new_v4();
                    dup.label = format!("{} (copy)", conn.label);
                    dup.created_at = now;
                    dup.updated_at = now;
                    // A fresh host has never been used.
                    dup.last_used = None;
                    // A cloud-imported host is bound to one discovered
                    // resource; a copy pointing at the same one would be
                    // clobbered (or orphaned) by the next refresh, and
                    // `customized_fields` only means anything next to it.
                    dup.cloud_ref = None;
                    dup.customized_fields.clear();
                    if let Some(vault) = &self.vault {
                        // Secrets live in their own encrypted columns, so
                        // they are copied explicitly rather than riding
                        // the clone.
                        let pw = vault.get_connection_password(&conn.id).ok().flatten();
                        let proxy_pw = vault.get_proxy_password(&conn.id).ok().flatten();
                        let totp = vault.get_connection_totp_secret(&conn.id).ok().flatten();
                        let target_pw =
                            vault.get_connection_target_password(&conn.id).ok().flatten();
                        let _ = vault.save_connection(&dup, pw.as_deref());
                        if proxy_pw.is_some() {
                            let _ = vault.set_proxy_password(&dup.id, proxy_pw.as_deref());
                        }
                        if totp.is_some() {
                            let _ = vault.set_connection_totp_secret(&dup.id, totp.as_deref());
                        }
                        if target_pw.is_some() {
                            let _ = vault
                                .set_connection_target_password(&dup.id, target_pw.as_deref());
                        }
                        self.load_data_from_vault();
                    }
                }
            }
            // Routed here by the parent; anything else is a
            // grouping mistake, not a runtime case.
            m => return crate::dispatch::unrouted(m),
        }
        Task::none()
    }
}
