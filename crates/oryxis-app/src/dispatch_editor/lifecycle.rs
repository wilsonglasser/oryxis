//! Opening the editor, saving, and what happens to the host itself.
//!
//! The arms that create, save, delete or duplicate a `Connection`, plus
//! "connect without saving". These are the only ones here that touch the
//! vault; everything else in this module edits the form in memory.

use super::*;

impl Oryxis {
    pub(super) fn handle_editor_lifecycle(&mut self, message: EditorMessage) -> Task<Message> {
        match message {
            EditorMessage::ShowNewConnection => {
                return self.open_new_host_editor();
            }
            EditorMessage::EditConnection(id) => {
                // Another host may still be open with a debouncing
                // auto-save; persist it before its form is replaced.
                // The message carries the ID, same rationale as
                // `DeleteConnection`: this flush reloads and re-sorts
                // the list (an auto-saved rename), so an index
                // captured at click time could open, and then
                // auto-save into, a different host.
                self.editor_flush_pending();
                self.card_context_menu = None;
                self.overlay = None;
                if let Some(conn) = self.connections.iter().find(|c| c.id == id) {
                    // Mutually exclusive right-panel slot.
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
                    // Fresh host in the form: drop the previous
                    // baseline; the post-dispatch kick records the new
                    // one before any edit can land.
                    self.editor_saved_snapshot = None;
                    return crate::widgets::focus_input(iced::widget::Id::new(
                        "editor-hostname",
                    ));
                }
            }
            EditorMessage::SaveQuickHost(id) => {
                // Same guard as EditConnection: an existing host's
                // pending auto-save must not die with the form swap.
                self.editor_flush_pending();
                self.overlay = None;
                self.card_context_menu = None;
                let Some(entry) = self.quick_connects.get(&id).cloned() else {
                    return Task::none();
                };
                // Mutually exclusive right-panel slot, and the panel lives
                // on the dashboard (the menu was clicked from a terminal).
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
                // One Enter, one save. The same fork shortcut fires the
                // binding from EVERY visible input of this panel, so a
                // focused-field Enter arrives as a burst of identical
                // EditorSave messages; on a NEW host each one would
                // persist another copy (five duplicates in the field
                // repro). The first save closes the panel (below), so
                // "panel already closed" is exactly "this is a phantom
                // repeat": every legitimate sender (footer button,
                // focused on_submit, ringed footer row) only exists
                // while the panel is up.
                if !self.panels.host_panel {
                    return Task::none();
                }
                match self.persist_editor_form(super::GroupWrite::Create) {
                    Ok(_) => {
                        self.panels.host_panel = false;
                        self.panel_nav_clear();
                        self.host_panel_error = None;
                        // The save consumed the secrets; drop any
                        // plaintext the eye may have revealed.
                        self.editor_form.sweep_secrets();
                        self.editor_saved_snapshot = None;
                    }
                    // Explicit Save surfaces both kinds inline: the
                    // panel stays up either way.
                    Err(e) => {
                        self.host_panel_error = Some(e.into_message());
                    }
                }
            }
            EditorMessage::EditorConnectWithoutSaving => {
                // Ad-hoc connect from the "+ Host" flow: build the full
                // Connection from the form but persist nothing. Only the
                // hostname is required; an empty label defaults to the
                // canonical user@host[:port].
                // Nothing persists on this path, so the split runs here
                // too: the ad-hoc dial reads the same form and would
                // otherwise hand `user@host` straight to the resolver.
                self.editor_split_host_field();
                // Local is the one protocol with nothing to dial: its
                // label is what identifies the session, so that is what
                // this path requires instead of an address.
                if self.editor_form.protocol
                    == oryxis_core::models::connection::ConnectionProtocol::Local
                {
                    if self.editor_form.label.trim().is_empty() {
                        self.host_panel_error =
                            Some(crate::i18n::t("editor_label_required").into());
                        return Task::none();
                    }
                } else if self.editor_form.hostname.trim().is_empty() {
                    self.host_panel_error =
                        Some(crate::i18n::t("quick_connect_hostname_required").into());
                    return Task::none();
                }
                let mut conn = match self.connection_from_editor_form(super::GroupWrite::Skip) {
                    Ok(conn) => conn,
                    Err(msg) => {
                        self.host_panel_error = Some(msg);
                        return Task::none();
                    }
                };
                if conn.label.is_empty() {
                    conn.label = oryxis_core::ssh_target::SshTarget {
                        username: conn.username.clone(),
                        // The label is a plaintext column; the typed
                        // password rides the ephemeral entry below.
                        password: None,
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
                self.editor_form.sweep_secrets();
                return self.update(Message::Ssh(SshMessage::QuickConnect(Box::new(entry))));
            }
            EditorMessage::EditorCancel => {
                // Editing an existing host is auto-save territory: the
                // X / Esc means "done", not "discard", so persist what
                // the debounce still holds. A new host keeps the old
                // meaning (nothing was written, nothing survives).
                self.editor_flush_pending();
                self.panels.host_panel = false;
                self.panel_nav_clear();
                self.host_panel_error = None;
                self.editor_form.sweep_secrets();
                self.editor_saved_snapshot = None;
            }
            EditorMessage::RequestDeleteConnection(idx) => {
                if let Some(conn) = self.connections.get(idx) {
                    let name = conn.label.clone();
                    // The confirmed action carries the ID: the list can
                    // re-sort while the dialog is up (an auto-saved
                    // rename, a sync apply), so a captured index could
                    // name a different host by confirm time.
                    self.confirm_remove(
                        name,
                        Message::Editor(EditorMessage::DeleteConnection(conn.id)),
                    );
                }
            }
            EditorMessage::DeleteConnection(id) => {
                self.card_context_menu = None;
                self.overlay = None;
                if self.connections.iter().any(|c| c.id == id) {
                    // The delete closes the editor below; a pending
                    // auto-save on a DIFFERENT host must not die with
                    // it. The deleted host itself is never flushed:
                    // resurrecting a row the user just removed would
                    // be worse than dropping its last keystrokes.
                    if self.editor_form.editing_id != Some(id) {
                        self.editor_flush_pending();
                    }
                    if let Some(vault) = &self.vault {
                        let _ = vault.delete_connection(&id);
                        // Saved AI conversations reference the host by id, so
                        // they go with it instead of dangling against an id
                        // that no longer resolves.
                        let _ = vault.delete_chat_conversations_for_connection(&id);
                        self.panels.host_panel = false;
                        self.panel_nav_clear();
                        self.editor_form.sweep_secrets();
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
                    if let Some(vault) = &self.vault {
                        // Secrets live in their own encrypted columns, so
                        // they are copied explicitly rather than riding
                        // the clone. Each decrypted plaintext is wrapped
                        // in `Zeroizing` so the copy is scrubbed when this
                        // block ends rather than freed intact.
                        let pw = vault
                            .get_connection_password(&conn.id)
                            .ok()
                            .flatten()
                            .map(zeroize::Zeroizing::new);
                        let proxy_pw = vault
                            .get_proxy_password(&conn.id)
                            .ok()
                            .flatten()
                            .map(zeroize::Zeroizing::new);
                        let totp = vault
                            .get_connection_totp_secret(&conn.id)
                            .ok()
                            .flatten()
                            .map(zeroize::Zeroizing::new);
                        let target_pw = vault
                            .get_connection_target_password(&conn.id)
                            .ok()
                            .flatten()
                            .map(zeroize::Zeroizing::new);
                        let _ = vault.save_connection(&dup, pw.as_ref().map(|s| s.as_str()));
                        if let Some(proxy_pw) = proxy_pw.as_ref() {
                            let _ = vault.set_proxy_password(&dup.id, Some(proxy_pw.as_str()));
                        }
                        if let Some(totp) = totp.as_ref() {
                            let _ = vault.set_connection_totp_secret(&dup.id, Some(totp.as_str()));
                        }
                        if let Some(target_pw) = target_pw.as_ref() {
                            let _ = vault
                                .set_connection_target_password(&dup.id, Some(target_pw.as_str()));
                        }
                        self.load_data_from_vault();
                    }
                }
            }
            EditorMessage::EditorPresetPicked(preset) => {
                use crate::state::{HostEditorPreset as P, HostEditorSection as S};
                match preset {
                    P::BasicSsh => {
                        // Also the "back to plain" verb: protocol and
                        // port reset, the section state folds shut.
                        self.editor_form.protocol =
                            oryxis_core::models::connection::ConnectionProtocol::Ssh;
                        self.editor_form.port = "22".to_string();
                        self.host_editor_open_sections.clear();
                    }
                    P::ViaBastion => {
                        self.editor_form.protocol =
                            oryxis_core::models::connection::ConnectionProtocol::Ssh;
                        self.host_editor_open_sections.insert(S::Network);
                        // Jump straight into picking the bastion when
                        // the vault has candidates; on an empty vault
                        // the opened section explains itself and an
                        // empty picker would only confuse.
                        if !self.connections.is_empty() {
                            self.panels.chain_editor = true;
                            self.chain_editor_adding = true;
                            self.chain_editor_search.clear();
                        }
                    }
                }
            }
            EditorMessage::EditorSectionToggled(section) => {
                // The keynav ring is left alone on purpose: the header's
                // own index is stable across its toggle (every row before
                // it is unchanged), so Enter-open / Enter-close keeps the
                // ring on the header, same as the Use-TOTP and algo
                // Auto/Custom rows that also reveal rows below themselves.
                if !self.host_editor_open_sections.remove(&section) {
                    self.host_editor_open_sections.insert(section);
                }
            }
            // Routed here by the parent; anything else is a
            // grouping mistake, not a runtime case.
            m => return crate::dispatch::unrouted(m),
        }
        Task::none()
    }

    /// Move everything that is not a host OUT of the Host field, at
    /// the two points a form becomes a destination: the vault write and
    /// the ad-hoc connect. Pasting a whole `user@host:port` into a box
    /// labelled Host is the ordinary way to fill it (every other client
    /// splits it), and nothing downstream ever did: the dial builds
    /// `{hostname}:{port}` verbatim, so the resolver got `user@host:22`
    /// and answered with a DNS error naming the symptom and never the
    /// cause (issue #171).
    ///
    /// Splitting, not rejecting. A reject would have to surface through
    /// `PersistError::Invalid`, which the auto-save flush swallows
    /// (`Err(Invalid(_)) => {}`), so refusing the save on a drawer that
    /// is already leaving the screen would silently discard the whole
    /// editing session.
    ///
    /// The dedicated field always wins, because it is the more specific
    /// statement: a Username already filled in keeps its value and the
    /// one riding the Host field is dropped, loudly (`host_panel_error`,
    /// still on screen for every gesture flush). The port only moves for
    /// the protocols whose numeric field means a TCP port the user
    /// typed by hand; RemoteDesktop refines its default through the
    /// VNC kind picker and Serial has no port at all, so those two get
    /// the hostname cleaned and their field left alone.
    fn editor_split_host_field(&mut self) {
        // "The form already has a password" is both halves of the
        // tri-state that mean one: a secret typed this session and one
        // already in the vault column.
        let has_password = !self.editor_form.password.as_str().is_empty()
            || self.editor_form.has_existing_password;
        let Some(split) = host_field_split(
            &self.editor_form.hostname,
            &self.editor_form.username,
            &self.editor_form.port,
            self.editor_form.protocol,
            has_password,
        ) else {
            return;
        };
        self.editor_form.hostname = split.hostname;
        if let Some(user) = split.username {
            self.editor_form.username = user;
        }
        if let Some(port) = split.port {
            self.editor_form.port = port;
        }
        let moved_password = split.password.is_some();
        if let Some(pw) = split.password {
            // `set` marks the field touched, which is what makes the
            // persist write it to the ENCRYPTED column. It is also the
            // only place the value is allowed to land: it came out of a
            // plaintext field and must not go back into one.
            self.editor_form.password.set(pw);
        }
        if split.dropped_user.is_some() || split.dropped_password || moved_password {
            // A TOAST, not the inline panel error: the flush that
            // reaches here most often is the one the drawer's own close
            // fires, and every close path clears `host_panel_error` on
            // its way out, so the inline slot would report a dropped
            // value to a surface already leaving the screen.
            let warning = self.host_field_split_warning(
                split.dropped_user.as_deref(),
                moved_password,
                split.dropped_password,
            );
            self.set_toast(warning);
        }
    }

    /// The user-facing sentence for a split that moved or dropped
    /// something. Credentials are named, never printed: the password is
    /// only ever reported as "a password", and the usernames go through
    /// Privacy Mode, since a toast is exactly the kind of thing that
    /// lands on a screen being shared.
    fn host_field_split_warning(
        &self,
        dropped_user: Option<&str>,
        moved_password: bool,
        dropped_password: bool,
    ) -> String {
        let mask = |value: &str| -> String {
            // The host being edited may carry its own override, so the
            // gate is the per-host one, not the global default.
            if self.privacy_active_for_override(self.editor_form.privacy_mode) {
                crate::widgets::mask_blocks(value)
            } else {
                value.to_string()
            }
        };
        let mut lines: Vec<String> = Vec::new();
        if let Some(dropped) = dropped_user {
            // Distinct placeholder names, not `{user}`/`{username}`:
            // the first is a PREFIX of the second, so substituting it
            // first would eat the head of the other and leave a
            // dangling `name}` in the message.
            lines.push(
                crate::i18n::t("editor_host_user_dropped")
                    .replace("{dropped}", &mask(dropped))
                    .replace("{kept}", &mask(&self.editor_form.username)),
            );
        }
        if moved_password {
            lines.push(crate::i18n::t("editor_host_password_moved").to_string());
        }
        if dropped_password {
            lines.push(crate::i18n::t("editor_host_password_dropped").to_string());
        }
        lines.join(" ")
    }

    /// The vault-write half of `EditorSave`, shared with the auto-save
    /// tick and flush (`dispatch_editor/autosave.rs`): validate, build
    /// the `Connection`, persist the row plus its side-column secrets,
    /// refresh app data and repaint any open tabs of the host. Does
    /// NOT close the panel or sweep the form; each caller decides what
    /// the save means for the surface, and what each `PersistError`
    /// kind means for it (`Invalid` = the form does not build, the
    /// vault keeps the last valid save; `Vault` = the write failed,
    /// dropping it silently is data loss).
    pub(super) fn persist_editor_form(
        &mut self,
        groups: super::GroupWrite,
    ) -> Result<oryxis_core::models::Connection, super::PersistError> {
        use super::PersistError;
        self.editor_split_host_field();
        // A local host names no endpoint, so its label IS the target and
        // is the only thing required; every other protocol needs both.
        let takes_address = self.editor_form.protocol
            != oryxis_core::models::connection::ConnectionProtocol::Local;
        if self.editor_form.label.is_empty()
            || (takes_address && self.editor_form.hostname.trim().is_empty())
        {
            return Err(PersistError::Invalid(
                crate::i18n::t(match takes_address {
                    true => "editor_label_host_required",
                    false => "editor_label_required",
                })
                .to_string(),
            ));
        }
        let conn = self
            .connection_from_editor_form(groups)
            .map_err(PersistError::Invalid)?;
        // Monitoring housekeeping, HERE rather than in the builder:
        // the auto-save dirty check builds the same `Connection` on
        // every editor-domain message, and a reset there wiped the
        // machine's shared sample window (issue #156) as fast as the
        // user could type. Only a real save may drop the series:
        // opting out (the status bar must not keep painting the last
        // sample as live) or a disk-selection change (the ring was
        // filtered by the OLD selection, issue #135). The key comes
        // from the row as it was BEFORE the edit: that is the window
        // this host has been filling until now.
        let reset_key = self
            .connections
            .iter()
            .find(|c| c.id == conn.id)
            .filter(|original| {
                (original.monitor_enabled && !conn.monitor_enabled)
                    || original.monitor_disks != conn.monitor_disks
            })
            .map(crate::monitor::endpoint::MonitorKey::new);
        if let Some(key) = reset_key {
            self.monitor_reset_key(&key, &conn.id);
        }
        // Disjoint borrows: the derived-clear rescues below write into
        // the form while the vault handle is held.
        let Self { vault, editor_form: form, .. } = self;
        let Some(vault) = vault.as_ref() else {
            // Unreachable in practice: the editor only renders over an
            // unlocked vault. A hard error beats a silent drop.
            return Err(PersistError::Vault(crate::i18n::t("error").to_string()));
        };
        // Tri-state: untouched preserves the stored password,
        // cleared removes it, typed stores (SecretInput::resolve).
        vault
            .save_connection(&conn, form.password.resolve())
            .map_err(|e| PersistError::Vault(e.to_string()))?;
        // The three side-column secrets share one shape. While the
        // owning feature is ON, the field's tri-state decides (typed
        // stores, edited-empty clears, untouched preserves). The
        // moment it turns OFF the column is cleared, keeping a
        // dangling encrypted credential would be surprising, but the
        // plaintext parks in the form's rescue stash first: under the
        // auto-save debounce a misclicked toggle would otherwise
        // DELETE a stored secret 700ms later with no confirm and no
        // way back. Re-enabling with an untouched field writes the
        // parked value back, so the mistake stays reversible for as
        // long as the editor is open (the stash dies with the sweep).
        //
        // Proxy password: "off" is the proxy being disabled.
        if conn.proxy.is_none() {
            if form.has_existing_proxy_password
                && form.proxy_password_rescue.as_str().is_empty()
                && let Ok(Some(pw)) = vault.get_proxy_password(&conn.id)
            {
                form.proxy_password_rescue.prefill(pw);
            }
            let _ = vault.set_proxy_password(&conn.id, None);
            form.has_existing_proxy_password = false;
        } else if let Some(pw) = form.proxy_password.resolve() {
            let _ = vault.set_proxy_password(&conn.id, (!pw.is_empty()).then_some(pw));
            // The persist owns the flag: `editor_autosave_settle` must
            // not second-guess what landed in the column.
            form.has_existing_proxy_password = !pw.is_empty();
            // Edited by hand: the stash is stale either way.
            form.proxy_password_rescue.clear();
        } else if !form.has_existing_proxy_password
            && !form.proxy_password_rescue.as_str().is_empty()
        {
            let _ =
                vault.set_proxy_password(&conn.id, Some(form.proxy_password_rescue.as_str()));
            form.has_existing_proxy_password = true;
        }
        // TOTP secret: TOTP is SSH-only (keyboard-interactive 2FA),
        // so a protocol switch counts as the toggle going off, same
        // clamp as `mcp_enabled`.
        let totp_on = conn.protocol
            == oryxis_core::models::connection::ConnectionProtocol::Ssh
            && form.use_totp;
        if !totp_on {
            if form.has_existing_totp
                && form.totp_rescue.as_str().is_empty()
                && let Ok(Some(secret)) = vault.get_connection_totp_secret(&conn.id)
            {
                form.totp_rescue.prefill(secret);
            }
            let _ = vault.set_connection_totp_secret(&conn.id, None);
            form.has_existing_totp = false;
        } else if let Some(secret) = form.totp_secret.resolve() {
            let s = secret.trim();
            let s = (!s.is_empty()).then_some(s);
            let _ = vault.set_connection_totp_secret(&conn.id, s);
            form.has_existing_totp = s.is_some();
            form.totp_rescue.clear();
        } else if !form.has_existing_totp && !form.totp_rescue.as_str().is_empty() {
            let _ = vault
                .set_connection_totp_secret(&conn.id, Some(form.totp_rescue.as_str()));
            form.has_existing_totp = true;
        }
        // Target password: only meaningful while a login script is
        // attached, so detaching the script is that toggle.
        if conn.login_script_id.is_none() {
            if form.has_existing_target_password
                && form.target_password_rescue.as_str().is_empty()
                && let Ok(Some(pw)) = vault.get_connection_target_password(&conn.id)
            {
                form.target_password_rescue.prefill(pw);
            }
            let _ = vault.set_connection_target_password(&conn.id, None);
            form.has_existing_target_password = false;
        } else if let Some(pw) = form.target_password.resolve() {
            let _ = vault.set_connection_target_password(&conn.id, Some(pw));
            form.has_existing_target_password = !pw.is_empty();
            form.target_password_rescue.clear();
        } else if !form.has_existing_target_password
            && !form.target_password_rescue.as_str().is_empty()
        {
            let _ = vault.set_connection_target_password(
                &conn.id,
                Some(form.target_password_rescue.as_str()),
            );
            form.has_existing_target_password = true;
        }
        // Re-paint any open tabs of this host so a
        // newly chosen palette takes effect without
        // a reconnect.
        let host_label = conn.label.clone();
        self.load_data_from_vault();
        self.repaint_terminal_palettes_for_label(&host_label);
        Ok(conn)
    }
}

/// What the Host field's split decided. Every field is what CHANGES:
/// `username` / `port` are `Some` only when the host string had one to
/// give AND the dedicated field was still free to take it, and
/// `dropped_user` names the one that lost so the caller can say so.
pub(super) struct HostFieldSplit {
    pub hostname: String,
    pub username: Option<String>,
    pub port: Option<String>,
    pub dropped_user: Option<String>,
    /// A `user:secret@host` password, to be written to the ENCRYPTED
    /// password column and nowhere else. `None` both when the value
    /// carried none and when the form already holds one.
    pub password: Option<String>,
    /// A password was present and is NOT being applied, because the
    /// form already has one. The value is dropped here rather than
    /// carried: the caller has no use for it and every place it could
    /// be shown or stored is plaintext.
    pub dropped_password: bool,
}

/// The decision half of `editor_split_host_field`, pure so the rules
/// are testable without an `Oryxis` behind them. `None` means the form
/// is already correct (a plain host) or unreadable as a target at all,
/// which are the two cases that must leave it untouched.
///
/// The dedicated field always wins, because it is the more specific
/// statement. The port only moves for the protocols whose numeric
/// field means a TCP port the user typed by hand: RemoteDesktop
/// refines its default through the VNC kind picker and Serial has no
/// port at all, so those two get the hostname cleaned and their field
/// left exactly as it was.
pub(super) fn host_field_split(
    hostname: &str,
    username: &str,
    port: &str,
    protocol: oryxis_core::models::connection::ConnectionProtocol,
    has_password: bool,
) -> Option<HostFieldSplit> {
    use oryxis_core::models::connection::ConnectionProtocol;

    // `None` is not a target in any reading. Rewriting it would corrupt
    // an entry the user can still fix by hand, and the connect-time
    // hint names the problem either way.
    let target = oryxis_core::ssh_target::SshTarget::from_host_field(hostname)?;
    if target.host == hostname && target.username.is_none() && target.port.is_none() {
        return None;
    }

    let mut split = HostFieldSplit {
        hostname: target.host,
        username: None,
        port: None,
        dropped_user: None,
        password: None,
        dropped_password: false,
    };
    if let Some(user) = target.username {
        if username.trim().is_empty() {
            split.username = Some(user);
        } else if username != user {
            split.dropped_user = Some(user);
        }
    }
    // The password is the one part that CANNOT stay where it was: the
    // host field is a plaintext column, so leaving it there is the leak.
    // It moves to the encrypted field when that is free, and is dropped
    // outright otherwise, on the same "the dedicated field wins" rule as
    // the username. Dropping loses a value the user can retype; keeping
    // it would overwrite a stored credential from a paste.
    if target.password.is_some() {
        if has_password {
            split.dropped_password = true;
        } else {
            split.password = target.password;
        }
    }
    if let Some(p) = target.port
        && matches!(
            protocol,
            ConnectionProtocol::Ssh | ConnectionProtocol::Telnet
        )
    {
        let field = port.trim();
        let default = protocol.default_port();
        if field.is_empty() || default.is_some_and(|d| d.to_string() == field) {
            split.port = Some(p.to_string());
        }
    }
    Some(split)
}

#[cfg(test)]
mod tests {
    use super::host_field_split;
    use oryxis_core::models::connection::ConnectionProtocol::{
        RemoteDesktop, Serial, Ssh, Telnet,
    };

    #[test]
    fn a_plain_host_is_left_alone() {
        assert!(host_field_split("web01", "", "22", Ssh, false).is_none());
        assert!(host_field_split("10.0.0.7", "root", "22", Ssh, false).is_none());
        // Already-bracketed IPv6 canonicalises rather than reporting a
        // change nobody asked for; a bare one is untouched.
        assert!(host_field_split("::1", "", "22", Ssh, false).is_none());
    }

    #[test]
    fn a_pasted_password_lands_in_the_encrypted_field_not_the_host() {
        // `sftp://user:pass@host` is how much of the world's
        // documentation writes a connect string. Before the split it
        // rode into `username`, which is a PLAINTEXT column.
        let s = host_field_split("ssh://root:hunter2@web01", "", "22", Ssh, false).unwrap();
        assert_eq!(s.hostname, "web01");
        assert_eq!(s.username.as_deref(), Some("root"));
        assert_eq!(s.password.as_deref(), Some("hunter2"));
        assert!(!s.dropped_password);
        // Nothing of the secret may survive in a field that is not the
        // password one.
        assert!(!s.hostname.contains("hunter2"));
        assert!(!s.username.as_deref().unwrap_or_default().contains("hunter2"));
    }

    #[test]
    fn a_filled_password_field_outranks_the_pasted_one() {
        // Same "the dedicated field wins" rule as the username, and the
        // dropped value is not carried anywhere: overwriting a stored
        // credential from a paste is the worse of the two losses.
        let s = host_field_split("root:hunter2@web01", "", "22", Ssh, true).unwrap();
        assert_eq!(s.username.as_deref(), Some("root"));
        assert_eq!(s.password, None);
        assert!(s.dropped_password);
    }

    #[test]
    fn a_host_without_a_password_reports_neither() {
        let s = host_field_split("root@web01", "", "22", Ssh, true).unwrap();
        assert_eq!(s.password, None);
        assert!(!s.dropped_password);
    }

    #[test]
    fn an_unreadable_value_is_left_alone() {
        // The case the connect-time hint exists for: guessing here
        // would corrupt a row the user can still fix by hand.
        assert!(host_field_split("root@10.0.0.7/srv", "", "22", Ssh, false).is_none());
        assert!(host_field_split("root@", "", "22", Ssh, false).is_none());
    }

    #[test]
    fn a_whole_connect_string_lands_in_three_fields() {
        let s = host_field_split("root@10.0.0.7:2222", "", "22", Ssh, false).unwrap();
        assert_eq!(s.hostname, "10.0.0.7");
        assert_eq!(s.username.as_deref(), Some("root"));
        assert_eq!(s.port.as_deref(), Some("2222"));
        assert_eq!(s.dropped_user, None);
    }

    #[test]
    fn a_filled_username_keeps_its_value_and_reports_the_loss() {
        // Silently overwriting what the user typed into the dedicated
        // field would be the worse of the two data losses.
        let s = host_field_split("root@10.0.0.7", "deploy", "22", Ssh, false).unwrap();
        assert_eq!(s.hostname, "10.0.0.7");
        assert_eq!(s.username, None);
        assert_eq!(s.dropped_user.as_deref(), Some("root"));
    }

    #[test]
    fn the_same_username_on_both_sides_is_not_a_loss() {
        let s = host_field_split("root@10.0.0.7", "root", "22", Ssh, false).unwrap();
        assert_eq!(s.hostname, "10.0.0.7");
        assert_eq!(s.dropped_user, None);
    }

    #[test]
    fn a_hand_typed_port_outranks_the_host_string() {
        let s = host_field_split("root@10.0.0.7:2222", "", "2022", Ssh, false).unwrap();
        assert_eq!(s.hostname, "10.0.0.7");
        assert_eq!(s.port, None);
    }

    #[test]
    fn telnet_moves_its_port_off_its_own_default() {
        let s = host_field_split("admin@10.0.0.9:2323", "", "23", Telnet, false).unwrap();
        assert_eq!(s.port.as_deref(), Some("2323"));
        // 22 is not Telnet's default, so it reads as hand-typed.
        let s = host_field_split("admin@10.0.0.9:2323", "", "22", Telnet, false).unwrap();
        assert_eq!(s.port, None);
    }

    #[test]
    fn the_port_never_moves_for_a_protocol_that_refines_its_own() {
        // RemoteDesktop's 3389 becomes VNC's 5900 through the kind
        // picker, and Serial has no port at all: both get the hostname
        // cleaned and the numeric field left as it was.
        let s = host_field_split("admin@10.0.0.9:3390", "", "3389", RemoteDesktop, false).unwrap();
        assert_eq!(s.hostname, "10.0.0.9");
        assert_eq!(s.username.as_deref(), Some("admin"));
        assert_eq!(s.port, None);
        let s = host_field_split("admin@10.0.0.9:9600", "", "", Serial, false).unwrap();
        assert_eq!(s.port, None);
    }

    #[test]
    fn an_empty_port_field_takes_the_one_from_the_host_string() {
        let s = host_field_split("10.0.0.7:2222", "", "", Ssh, false).unwrap();
        assert_eq!(s.port.as_deref(), Some("2222"));
    }

    #[test]
    fn padding_alone_is_enough_to_report_a_change() {
        // The untrimmed value reached the resolver as typed, so the
        // trim has to count as a split even with nothing to move.
        let s = host_field_split("  web01  ", "", "22", Ssh, false).unwrap();
        assert_eq!(s.hostname, "web01");
        assert_eq!(s.username, None);
        assert_eq!(s.port, None);
    }

    #[test]
    fn a_pasted_url_loses_its_scheme() {
        let s = host_field_split("ssh://root@web01:2222", "", "22", Ssh, false).unwrap();
        assert_eq!(s.hostname, "web01");
        assert_eq!(s.username.as_deref(), Some("root"));
        assert_eq!(s.port.as_deref(), Some("2222"));
    }
}
