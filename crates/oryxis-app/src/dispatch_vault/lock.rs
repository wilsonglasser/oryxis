//! Leaving the vault, by hand or by idle timer.
//!
//! The two paths differ on purpose and are worth reading side by
//! side: `LockVault` is a full teardown (sessions, tabs, secrets),
//! `SoftLockVault` is soft (zeroize the key, show the lock screen,
//! keep the live SSH sessions, since an established channel never
//! needs the key again).

use super::*;

impl Oryxis {
    pub(super) fn handle_vault_lock(
        &mut self,
        message: VaultMessage,
    ) -> Task<Message> {
        match message {

            // ── Manual lock confirmation ──
            VaultMessage::LockVaultConfirm => {
                // Lock Vault tears every live session and tab down, so the
                // button asks first (an accidental click would sever all
                // open connections). Close the surface that fired it (the
                // burger menu; the palette already closes on activate) so
                // the confirm layers over a plain view, mirroring
                // RequestClearHistory's overlay close.
                self.panels.burger_menu = false;
                if self.vault_ui.has_user_password {
                    // A standing choice saved from the dialog's "always
                    // use the selected option" opt-in commits directly;
                    // Settings > Security exposes the same value so the
                    // dialog can be brought back ("ask", the default).
                    match self.prefs.manual_lock_action.as_str() {
                        "sleep" => {
                            return Task::done(Message::Vault(VaultMessage::SoftLockVault));
                        }
                        "lock" => {
                            return Task::done(Message::Vault(VaultMessage::LockVault));
                        }
                        _ => {
                            // The opt-in never carries over from an
                            // earlier arming: a stale check would turn a
                            // one-off choice into a standing one.
                            self.vault_ui.lock_confirm_remember = false;
                            self.vault_ui.lock_confirm = true;
                        }
                    }
                }
            }
            VaultMessage::CancelLockVaultConfirm => {
                self.vault_ui.lock_confirm = false;
            }
            VaultMessage::LockVaultConfirmRememberToggled => {
                self.vault_ui.lock_confirm_remember = !self.vault_ui.lock_confirm_remember;
            }
            VaultMessage::LockVaultConfirmProceed { sleep } => {
                // Persist the standing choice BEFORE committing: both lock
                // paths sweep dialog state, and the soft lock zeroizes the
                // key (settings survive; the vault stays readable for
                // settings without the master key).
                if self.vault_ui.lock_confirm_remember {
                    let value = if sleep { "sleep" } else { "lock" };
                    self.prefs.manual_lock_action = value.into();
                    self.persist_setting("manual_lock_action", value);
                }
                return Task::done(Message::Vault(if sleep {
                    VaultMessage::SoftLockVault
                } else {
                    VaultMessage::LockVault
                }));
            }

            // ── Vault lock (manual + idle auto-lock) ──
            VaultMessage::SoftLockVault => {
                // Soft lock: the user walked away, not "I'm done". Zeroize
                // the master key and drop to the lock screen, but keep
                // live SSH sessions and tabs so long-running remote work
                // survives the idle period (established channels never
                // need the key again; credentials are only read at
                // connect time). The manual LockVault stays a full
                // teardown. While locked, the session-log flush and
                // auto-reconnect tickers unmount (subscription.rs), so
                // nothing hits the sealed vault; pane buffers accumulate
                // and drain after unlock.
                // A debouncing host-editor auto-save needs the key;
                // persist it before the vault seals. Interrupted: an
                // idle lock concluded nothing, so a half-typed Parent
                // Group name must not become a vault group.
                self.editor_flush_interrupted();
                if let Some(vault) = &mut self.vault
                    && self.vault_ui.has_user_password
                {
                    vault.lock();
                    self.vault_ui.state = VaultState::Locked;
                    self.master_password = None;
                    // The lock screen leads with biometrics when enrolled;
                    // a fallback choice from a previous lock must not stick.
                    self.vault_ui.password_fallback = false;
                    // The confirm dialog cannot survive either lock path: it
                    // is moot once the lock screen is up, and a stale latch
                    // would re-open over the unlocked app.
                    self.vault_ui.lock_confirm = false;
                    // Sweep UI that may hold typed or revealed secrets;
                    // everything else (tabs, terminals) stays.
                    self.revealed_secrets.clear();
                    self.panels.host_panel = false;
                    self.host_panel_error = None;
                    self.editor_form = crate::state::ConnectionForm::default();
                    // The highlight-rule editor edits a copy of a row in
                    // one of those lists; with its list gone, close it so
                    // nothing reopens over the unlocked screen.
                    self.close_highlight_rule_editor();
                    // The key-generation panel carries export
                    // passphrases and a public-key view; sweep it (a
                    // still-running generation task is dropped on
                    // completion by the locked-vault check).
                    self.panels.key_generate_panel = false;
                    self.keys_ui.generate_form = crate::state::KeyGenerateForm::default();
                    // The key import panel (holds a pasted cert / PEM) and
                    // the cert viewer are vault-area surfaces; drop them.
                    // The live PEM editor buffer is private material, so it
                    // is reset too, matching the generate-panel sweep.
                    self.panels.key_panel = false;
                    self.keys_ui.import_form = crate::state::KeyImportForm::default();
                    self.keys_ui.import_content = iced::widget::text_editor::Content::new();
                    self.keys_ui.import_public_content = iced::widget::text_editor::Content::new();
                    self.keys_ui.import_cert_content = iced::widget::text_editor::Content::new();
                    self.cert_viewer = None;
                    // The session player / log viewer hold DECRYPTED
                    // recording bytes (a session that ran `cat
                    // /etc/shadow` keeps that output in the emulator grid
                    // / rendered spans). That is secret-bearing UI like a
                    // revealed secret, so it must not sit in RAM behind
                    // the lock screen; it can only be rebuilt from the
                    // vault after unlock anyway.
                    self.session_player = None;
                    self.viewing_session_log = None;
                    // The History content-search results hold decrypted
                    // command lines / output excerpts; same rule.
                    self.history_content_reset();
                    // The ssh-agent goes dark (keys ungated) while locked;
                    // the listener stays up so a `git` sees an empty agent.
                    self.agent_on_lock();
                    self.overlay = None;
                    self.card_context_menu = None;
                    // The burger menu and the SFTP row menu are dropdown
                    // overlays, not Modals, so the modal sweep below never
                    // reaches them. Left armed, their flag survives the
                    // lock screen, and the Enter that submits the unlock
                    // password reaches the modal keynav router in the same
                    // update batch the vault unlocks in: the router then
                    // activates the menu's remembered default row against
                    // its stale pre-lock recording (issue #169, the unlock
                    // that landed on a fresh SFTP tab with the host picker
                    // up, because the SFTP row recorded slot 0).
                    self.panels.burger_menu = false;
                    self.sftp.row_menu = None;
                    // Top-strip pickers (command palette, tab-jump,
                    // new-tab picker) are NOT rendered over the lock
                    // screen, but their `show_*` flags still make
                    // `any_modal_blocks_input()` true, so the modal key
                    // router would keep processing arrows / Enter for the
                    // hidden surface behind the lock screen (the command
                    // palette could even dispatch an action while locked).
                    // Close them like the SFTP modals below.
                    self.close_modal(crate::state::Modal::CommandPalette);
                    self.close_modal(crate::state::Modal::TabJump);
                    self.close_modal(crate::state::Modal::NewTabPicker);
                    // A careful-paste / snippet-variables confirm parked
                    // over a live session is ARMED state: its keynav
                    // default is the Confirm row, the lock screen never
                    // re-records the modal ring, and both flags keep
                    // `any_modal_blocks_input()` true while locked, so a
                    // stray Enter on the lock screen (an empty or failed
                    // unlock) would reach the modal key router and
                    // activate the stale default, injecting the staged
                    // text into the still-live session, the exact
                    // auto-run the careful paste exists to stop. Close
                    // them and drop the stale modal recording with them.
                    self.close_modal(crate::state::Modal::CarefulPaste);
                    self.close_modal(crate::state::Modal::SnippetVars);
                    self.pending_paste_install = None;
                    self.modal_nav_reset();
                    // A master-password candidate typed into the change /
                    // set-password form must not survive the soft lock.
                    self.vault_ui.new_password.clear();
                    self.vault_ui.confirm_password.clear();
                    // Abort an in-flight KDF calibration too: its snapshot is
                    // secret material and the apply must not land post-lock.
                    self.vault_ui.pending_kdf_pw = None;
                    // Same for the MCP panel's master-password confirm.
                    self.mcp.vault_pw_prompt = None;
                    self.mcp.vault_pw_error = false;
                    // SFTP modals carry remote paths and live action buttons;
                    // root_view already stops rendering them while locked, but
                    // sweep the state so none reappears after unlock. A watch
                    // holding a pending save is dropped with it, matching the
                    // soft-lock promise (secret-bearing UI is discarded, the
                    // live session survives).
                    self.sftp.picker_open = false;
                    self.sftp.new_entry = None;
                    self.sftp.delete_confirm.clear();
                    self.sftp_edit_reopen = None;
                    self.sftp.edit_watches.clear();
                    // Watches parked in standalone / hybrid tab states feed
                    // the same 2s tick; left alive they would keep uploading
                    // local saves (under an autosave grant) behind the lock
                    // screen, and dirty ones would re-prompt after unlock.
                    for tab in self.sftp_tabs.iter_mut() {
                                                tab.state.edit_watches.clear();
                    }
                    for tab in self.tabs.iter_mut() {
                                                tab.files_state.edit_watches.clear();
                    }
                    // Monitor samples are host telemetry gathered while
                    // unlocked; drop them with the rest of the sweep so a
                    // locked screen shows nothing about the fleet. The
                    // stamp bump inside makes a probe still in flight land
                    // dead instead of repopulating the swept state.
                    self.monitor_reset_all();
                    // Same reasoning for the tmux listings: what runs on
                    // a host is telemetry too, and a locked screen owes
                    // the fleet nothing.
                    self.tmux_reset_all();
                    self.sftp.overwrite_prompt = None;
                    self.sftp.properties = None;
                    // A pending keyboard-interactive prompt belongs to an
                    // in-flight connect; cancel it cleanly (the engine
                    // treats `None` as auth abort).
                    if self.pending_kbi_prompt.take().is_some() {
                        self.kbi_inputs.clear();
                        if let Some(ref tx) = self.kbi_response_tx {
                            let _ = tx.try_send(None);
                        }
                    }
                    // A pending host-key prompt is a security dialog for an
                    // in-flight backgrounded connect; reject it (safe
                    // default) rather than leaving it rendered over the lock
                    // screen. Mirrors SshHostKeyReject.
                    if self.pending_host_key.take().is_some()
                        && let Some(tx) = self.active_host_key_tx.take()
                    {
                        let _ = tx.try_send(false);
                    }
                    self.pending_kbi_quick = None;
                    // A parked identity/key switch must not fire a
                    // reconnect behind the lock screen.
                    self.pending_auth_switch = None;
                    // Quick-connect entries hold typed plaintext credentials;
                    // sweep the secrets but keep the connections themselves,
                    // matching the soft-lock promise that live tabs survive.
                    // A post-unlock reconnect of a password-based quick host
                    // falls back to the interactive prompt.
                    for entry in self.quick_connects.values_mut() {
                        entry.password = None;
                        entry.totp_secret = None;
                        entry.proxy_password = None;
                    }
                    // The sync passphrase field is an edit buffer for the
                    // shared group secret; a passphrase typed this session
                    // must not sit in RAM behind the lock screen (the
                    // stored value itself rides the encrypted setting).
                    self.sync.passphrase_input.clear();
                    self.sync.passphrase_matches = None;
                    self.sync.passphrase_editing = false;
                    self.sync.passphrase_field_id = None;
                    // Same for a round's armed key: a locked vault cannot
                    // store it anyway (`set_sync_sftp_passphrase` needs
                    // the master key), so the round that comes back
                    // finds nothing to commit.
                    self.sync.passphrase_sealed = None;
                    // Land the keyboard in the unlock field so the user
                    // returning to the machine just types the password.
                    return crate::widgets::focus_input(iced::widget::Id::new(
                        "vault-unlock-password",
                    ));
                }
            }
            VaultMessage::LockVault => {
                // Same as the soft lock: flush a debouncing host-editor
                // auto-save while the key still exists, and on the same
                // interrupted terms.
                self.editor_flush_interrupted();
                if let Some(vault) = &mut self.vault {
                    vault.lock();
                    // The dialog that armed this is committed; clear the
                    // latch so it cannot re-open over the unlocked app.
                    self.vault_ui.lock_confirm = false;
                    if self.vault_ui.has_user_password {
                        self.vault_ui.state = VaultState::Locked;
                        // The in-memory master password dies with the
                        // lock, like the soft lock already does (it
                        // feeds biometric enroll and the MCP config
                        // embed; neither may outlive the vault key).
                        self.master_password = None;
                        // ssh-agent goes dark on lock (listener stays up).
                        self.agent_on_lock();
                        // And the MCP panel's typed confirm buffer.
                        self.mcp.vault_pw_prompt = None;
                        self.mcp.vault_pw_error = false;
                        // Same reset as the soft lock: lead with biometrics.
                        self.vault_ui.password_fallback = false;
                        self.connections.clear();
                        self.quick_connects.clear();
                        self.keys.clear();
                        self.snippets.clear();
                        self.groups.clear();
                        // Close live remote sessions, not just the panes
                        // referencing them, so locking the vault really
                        // severs the remote connections.
                        for tab in &self.tabs {
                            Self::close_tab_sessions(tab);
                        }
                        // Drop RDP/VNC tunnels too (each Arc drop cancels
                        // the -L forward); locking severs everything. The
                        // terminal's callback tunnels go the same way, and
                        // a link waiting on an answer stops waiting.
                        self.remote_desktop_forwards.clear();
                        self.link_forwards.clear();
                        self.link_confirm = None;
                        // Standalone SFTP tabs ride their own SSH channels:
                        // close every mounted session and drop the tabs (plus
                        // their tab-order entries and the hoisted buffer), mirroring
                        // the terminal teardown above. Without this an SFTP tab's
                        // session Arc would outlive the lock screen and reappear
                        // mounted after unlock.
                        for tab in &self.sftp_tabs {
                            if let Some(session) = &tab.state.left.session {
                                session.close();
                            }
                            if let Some(session) = &tab.state.right.session {
                                session.close();
                            }
                        }
                        // The ACTIVE standalone tab's live state rides the
                        // swap-on-focus buffer (`self.sftp`), not its parked
                        // `state` slot (a taken default), so its sessions must
                        // be closed here too: replacing the buffer below only
                        // drops the Arc, and a clone held by an in-flight
                        // transfer would keep the connection alive behind the
                        // lock screen. A hybrid owner's mounts sit on pane
                        // sessions already closed above; close() is idempotent.
                        if let Some(session) = &self.sftp.left.session {
                            session.close();
                        }
                        if let Some(session) = &self.sftp.right.session {
                            session.close();
                        }
                        self.sftp_tabs.clear();
                        self.tab_order.retain(|r| !matches!(r, crate::state::TabRef::Sftp(_)));
                        self.sftp = crate::state::SftpState::default();
                        self.active_sftp = None;
                        self.tabs.clear();
                        self.active_tab = None;
                        // The reopen stack goes with them (issue #186).
                        // A manual lock is an explicit "I'm done" that
                        // severs every session; leaving a chord that
                        // dials one of them back afterwards would be the
                        // same stale state this sweep exists to clear.
                        // The SOFT lock deliberately keeps it, along with
                        // the tabs and sessions it also keeps.
                        self.closed_tabs.clear();
                        self.clear_terminal_tab_memory();
                        self.active_view = View::Dashboard;
                        // Mirror the soft-lock UI sweep: the manual lock
                        // used to leave overlays, side panels, revealed
                        // secrets and pending auth prompts armed behind
                        // the lock screen (stale state a stray key or a
                        // late async completion could act on, and typed
                        // or revealed secrets have no business surviving
                        // an explicit "I'm done").
                        self.revealed_secrets.clear();
                        // History content-search results hold decrypted
                        // command lines / output excerpts; sweep like the
                        // soft lock does.
                        self.history_content_reset();
                        self.overlay = None;
                        self.card_context_menu = None;
                        // Same dropdown sweep as the soft lock: the confirm
                        // dialog already closed the burger menu on its way
                        // in, but the sweep must hold on its own so no
                        // trigger site can leave the menu armed behind the
                        // lock screen (issue #169). The SFTP row menu dies
                        // with the buffer reset above.
                        self.panels.burger_menu = false;
                        // Top-strip pickers: same reason as the soft lock,
                        // a stray key must not drive the hidden surface (the
                        // command palette could dispatch an action) behind
                        // the lock screen.
                        self.close_modal(crate::state::Modal::CommandPalette);
                        self.close_modal(crate::state::Modal::TabJump);
                        self.close_modal(crate::state::Modal::NewTabPicker);
                        // Same paste-confirm sweep as the soft lock. The
                        // sessions are gone here, so a stale Confirm could
                        // not inject anywhere, but the armed flags would
                        // still hold `any_modal_blocks_input()` true and
                        // feed the modal router stale rows behind the lock
                        // screen.
                        self.close_modal(crate::state::Modal::CarefulPaste);
                        self.close_modal(crate::state::Modal::SnippetVars);
                        self.pending_paste_install = None;
                        self.modal_nav_reset();
                        self.error_dialog = None;
                        self.panels.host_panel = false;
                        self.host_panel_error = None;
                        self.editor_form = crate::state::ConnectionForm::default();
                        self.panels.key_generate_panel = false;
                        self.keys_ui.generate_form = crate::state::KeyGenerateForm::default();
                        self.panels.key_panel = false;
                        self.keys_ui.import_form = crate::state::KeyImportForm::default();
                        self.keys_ui.import_content = iced::widget::text_editor::Content::new();
                        self.keys_ui.import_public_content = iced::widget::text_editor::Content::new();
                        self.keys_ui.import_cert_content = iced::widget::text_editor::Content::new();
                        self.cert_viewer = None;
                        // Decrypted session-recording bytes (player grid /
                        // rendered viewer spans) are secret-bearing and
                        // have no business surviving an explicit "I'm
                        // done"; the soft lock sweeps these too.
                        self.session_player = None;
                        self.viewing_session_log = None;
                        self.vault_ui.new_password.clear();
                        self.vault_ui.confirm_password.clear();
                        // Abort an in-flight KDF calibration (snapshot is
                        // secret material; the apply must not land post-lock).
                        self.vault_ui.pending_kdf_pw = None;
                        self.sftp.picker_open = false;
                        self.sftp.new_entry = None;
                        self.sftp.delete_confirm.clear();
                        self.sftp_edit_reopen = None;
                        self.sftp.edit_watches.clear();
                        for tab in self.sftp_tabs.iter_mut() {
                                                        tab.state.edit_watches.clear();
                        }
                        for tab in self.tabs.iter_mut() {
                                                        tab.files_state.edit_watches.clear();
                        }
                        self.monitor_reset_all();
                        self.tmux_reset_all();
                        self.sftp.overwrite_prompt = None;
                        self.sftp.properties = None;
                        // Cancel a pending keyboard-interactive / host-key
                        // prompt from an in-flight connect (the sessions
                        // were just torn down; the engine treats `None` /
                        // `false` as a clean abort).
                        if self.pending_kbi_prompt.take().is_some() {
                            self.kbi_inputs.clear();
                            if let Some(ref tx) = self.kbi_response_tx {
                                let _ = tx.try_send(None);
                            }
                        }
                        if self.pending_host_key.take().is_some()
                            && let Some(tx) = self.active_host_key_tx.take()
                        {
                            let _ = tx.try_send(false);
                        }
                        self.pending_kbi_quick = None;
                        self.pending_auth_switch = None;
                        // Same auto-focus as the soft lock: the unlock
                        // field is the only thing to interact with.
                        return crate::widgets::focus_input(iced::widget::Id::new(
                            "vault-unlock-password",
                        ));
                    } else {
                        // No user password: re-open immediately
                        let _ = vault.open_without_password();
                    }
                }
            }
            // The parent routed us here, so anything else is a
            // grouping mistake, not a runtime case.
            m => return crate::dispatch::unrouted(m),
        }
        Task::none()
    }
}
