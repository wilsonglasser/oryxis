//! Which surface owns the keyboard right now.
//!
//! A blocking modal takes the keyboard entirely (only Esc passes), so
//! "is anything open" and "close the topmost thing" are what the key
//! router consults before it does anything else. That makes this the
//! gate in front of the two routers, not a detail of either.


use crate::app::Oryxis;

impl Oryxis {
    /// Whether the given blocking modal is currently open. The `show_*`
    /// flag / `Option<_>` data field on `Oryxis` is the source of truth;
    /// this exhaustive `match` is what makes `any_modal_blocks_input`
    /// compiler-complete (a new `Modal` variant cannot compile without an
    /// arm here).
    pub(crate) fn is_modal_open(&self, m: crate::state::Modal) -> bool {
        use crate::state::Modal;
        match m {
            Modal::NewTabPicker => self.panels.new_tab_picker,
            Modal::TabJump => self.panels.tab_jump,
            Modal::CommandPalette => self.palette.open,
            Modal::IconPicker => self.panels.icon_picker,
            Modal::ThemePicker => self.panels.theme_picker,
            Modal::ChainEditor => self.panels.chain_editor,
            Modal::SessionGroupPanel => self.panels.session_group_panel,
            Modal::FolderRename => self.folder_rename.is_some(),
            Modal::FolderDelete => self.folder_delete.is_some(),
            Modal::TabRename => self.tab_rename.is_some(),
            Modal::CarefulPaste => self.pending_paste.is_some(),
            Modal::SnippetVars => self.pending_snippet_vars.is_some(),
            Modal::KbiPrompt => self.pending_kbi_prompt.is_some(),
            // Mirrors KbiPrompt: the flag alone is the source of truth. The
            // inline connect-progress host-key path (which renders inside
            // the progress screen and has no focused PTY behind it) is
            // gated separately by `connecting.is_none()` at the render site.
            Modal::HostKey => self.pending_host_key.is_some(),
            // Same shape as HostKey, and gated the same way at the
            // render site: the flag alone decides, the connect-progress
            // screen is what decides where it is drawn.
            Modal::ProxyCommand => self.pending_proxy_command.is_some(),
            Modal::AgentConfirm => self.agent.pending_confirm.is_some(),
            Modal::TriggerConfirm => self.trigger_confirm.is_some(),
            Modal::TerminalLinkConfirm => self.link_confirm.is_some(),
            Modal::TerminalThemeGallery => self.panels.terminal_theme_gallery,
            Modal::UiThemeGallery => self.panels.ui_theme_gallery,
            Modal::ThemeEditor => self.theme_ui.editor.is_some(),
            Modal::ThemeImport => self.panels.theme_import,
            Modal::UiThemeEditor => self.ui_theme_editor.is_some(),
            Modal::UiThemeImport => self.panels.ui_theme_import,
            Modal::ShareDialog => self.panels.share_dialog,
            Modal::CloudImportConfirm => self.cloud_import_confirm_visible,
            Modal::ErrorDialog => self.error_dialog.is_some(),
            Modal::ClearHistoryConfirm => self.clear_history_confirm,
            Modal::SshImport => self.panels.ssh_import_dialog,
            Modal::SftpRename => self.sftp.rename.is_some(),
            Modal::SftpNewEntry => self.sftp.new_entry.is_some(),
            Modal::SftpProperties => self.sftp.properties.is_some(),
            Modal::SftpOverwrite => self.sftp.overwrite_prompt.is_some(),
            // Any surface's watch: the dialog layers globally, so a save
            // waiting on a parked tab owns the keyboard just the same.
            Modal::SftpEditPrompt => self.pending_edit_save().is_some(),
            Modal::SftpEditReopen => self.sftp_edit_reopen.is_some(),
            Modal::SftpPicker => self.sftp.picker_open,
            Modal::CertificateViewer => self.cert_viewer.is_some(),
            Modal::MonitorKill => self.monitor.kill.is_some(),
            // Gated on the surface that owns the list being up, the same
            // predicate the render site uses, so the flag can never say
            // "open" over a screen that isn't showing it.
            Modal::HighlightRuleEditor => self.highlight_rule_editor_open(),
            Modal::LockVaultConfirm => self.vault_ui.lock_confirm,
        }
    }

    /// Close a specific modal: clear its `show_*` flag / `Option<_>` field
    /// plus any companion state, mirroring each modal's own Cancel handler
    /// so Esc leaves nothing stale. The exhaustive `match` is what makes
    /// `close_topmost_modal` compiler-complete. (The chain editor's
    /// two-stage Esc is handled by the caller before this is reached.)
    pub(crate) fn close_modal(&mut self, m: crate::state::Modal) {
        use crate::state::Modal;
        match m {
            Modal::NewTabPicker => {
                // Mirror HideNewTabPicker: abandoning the picker also
                // abandons any pending split-fill intent, so a later
                // unrelated open can't inherit it.
                self.panels.new_tab_picker = false;
                self.pending_pane_split = None;
                self.new_tab_picker_group = None;
            }
            Modal::TabJump => self.panels.tab_jump = false,
            Modal::CommandPalette => {
                self.palette.open = false;
                self.palette.query.clear();
            }
            Modal::IconPicker => {
                self.panels.icon_picker = false;
                self.icon_picker.for_id = None;
            }
            Modal::ThemePicker => self.panels.theme_picker = false,
            Modal::ChainEditor => self.panels.chain_editor = false,
            Modal::SessionGroupPanel => {
                self.panels.session_group_panel = false;
                self.session_group_panel_error = None;
            }
            Modal::FolderRename => self.folder_rename = None,
            Modal::FolderDelete => self.folder_delete = None,
            Modal::TabRename => self.tab_rename = None,
            Modal::CarefulPaste => self.pending_paste = None,
            Modal::SnippetVars => self.pending_snippet_vars = None,
            // Full mirror of SshKbiCancel: the engine must receive the
            // cancel (`None`) or the in-flight auth stays parked forever.
            Modal::KbiPrompt => {
                self.pending_kbi_prompt = None;
                self.pending_kbi_quick = None;
                self.kbi_inputs.clear();
                if let Some(ref tx) = self.kbi_response_tx {
                    let _ = tx.try_send(None);
                }
            }
            // Esc rejects the host key: a security prompt's safe default is
            // never to accept an unknown / changed key. Full mirror of
            // SshHostKeyReject: the engine's verifier must receive `false`
            // or the in-flight connect stays parked forever.
            Modal::HostKey => {
                self.pending_host_key = None;
                if let Some(tx) = self.active_host_key_tx.take() {
                    let _ = tx.try_send(false);
                }
            }
            // Esc refuses to spawn the command proxy, the only safe
            // default when the answer runs a local process. Full mirror
            // of SshProxyCommandReject: the parked dial must receive the
            // `false` or it waits forever.
            Modal::ProxyCommand => {
                self.pending_proxy_command = None;
                if let Some(tx) = self.active_proxy_command_tx.take() {
                    let _ = tx.try_send(false);
                }
            }
            // Esc denies the signature (safe default), firing the
            // responder so the waiting sign task gets its answer. The
            // caller then promotes any queued prompt via
            // `advance_confirm_queue`.
            Modal::AgentConfirm => {
                if let Some(card) = self.agent.pending_confirm.take() {
                    card.respond(false);
                }
            }
            // Esc refuses, and the refusal is remembered for the
            // session: a rule that could re-ask on the next matching
            // line would be a way to wear the user down.
            Modal::TriggerConfirm => self.resolve_trigger_confirm(false),
            // Esc opens nothing. No state to remember either way: the
            // next click on the link asks again, which is right for a
            // question about one specific target.
            Modal::TerminalLinkConfirm => self.link_confirm = None,
            Modal::ThemeEditor => {
                self.theme_ui.editor = None;
                self.theme_ui.color_popover = None;
            }
            Modal::TerminalThemeGallery => self.panels.terminal_theme_gallery = false,
            Modal::UiThemeGallery => self.panels.ui_theme_gallery = false,
            Modal::ThemeImport => self.panels.theme_import = false,
            Modal::UiThemeEditor => {
                self.ui_theme_editor = None;
                self.ui_color_popover = None;
            }
            Modal::UiThemeImport => self.panels.ui_theme_import = false,
            Modal::ShareDialog => {
                self.panels.share_dialog = false;
                self.share.filter = None;
                self.share.status = None;
                self.share.suggested_name = None;
            }
            Modal::CloudImportConfirm => {
                self.cloud_import_confirm_visible = false;
                self.cloud_discover.default_group_picker_open = false;
            }
            // Esc on the error dialog is always Dismiss, never the
            // dialog's action (mirrors ErrorDialogDismiss).
            Modal::ErrorDialog => self.error_dialog = None,
            // Mirrors CancelClearHistory.
            Modal::ClearHistoryConfirm => self.clear_history_confirm = false,
            // Mirrors SshImportDismiss, companion state included.
            Modal::SshImport => {
                self.panels.ssh_import_dialog = false;
                self.ssh_import_hosts.clear();
                self.ssh_import_selected.clear();
                self.ssh_import_existing.clear();
            }
            Modal::SftpRename => self.sftp.rename = None,
            Modal::SftpNewEntry => self.sftp.new_entry = None,
            Modal::SftpProperties => self.sftp.properties = None,
            // Raw dismissal only: dropping the prompt without an answer
            // leaves a queue-raised conflict parked (paused, item taken).
            // Esc never lands here; `close_topmost_modal` routes it to
            // `SftpResolveOverwrite(Cancel)` instead.
            Modal::SftpOverwrite => self.sftp.overwrite_prompt = None,
            // Closing the save prompt without a button press means "skip
            // this save" (the safe default): re-arm so the next save
            // prompts again, never upload by accident.
            Modal::SftpEditPrompt => self.skip_pending_edit_save(),
            // Esc on the reopen dialog = do nothing at all, the safest of
            // its three answers (neither branch runs).
            Modal::SftpEditReopen => self.sftp_edit_reopen = None,
            Modal::SftpPicker => self.sftp.picker_open = false,
            Modal::CertificateViewer => self.cert_viewer = None,
            // Esc abandons the working copy, exactly like the Cancel
            // button; the list's own state (the scope it addresses, a
            // pending delete confirmation) is left alone because those
            // rows render in the list behind the modal, not in it.
            Modal::HighlightRuleEditor => self.close_highlight_rule_editor(),
            // Esc while a kill run is in flight only drops the DIALOG;
            // the exec channel is already on its way and its result is
            // reported as a toast instead (`KillFinished`). Nothing is
            // re-sent, so there is no window where Esc doubles a signal.
            Modal::MonitorKill => self.monitor.kill = None,
            // Esc mirrors CancelLockVaultConfirm: don't lock.
            Modal::LockVaultConfirm => self.vault_ui.lock_confirm = false,
        }
    }

    /// `true` when a global picker / modal overlay is open and should
    /// swallow keyboard input instead of letting it fall through to the
    /// PTY underneath. Mirrors the set checked by `close_topmost_modal`
    /// (minus the burger menu, which carries no text field). Used by the
    /// keyboard router in `dispatch_terminal.rs` so typing in a picker's
    /// search field doesn't also leak into the terminal behind it.
    /// True when a blocking modal owns the keyboard, so the global key
    /// subscription must NOT route the press to the active PTY.
    ///
    /// INVARIANT: every modal that contains a text field MUST appear here.
    /// The terminal input arrives via a global subscription
    /// (`subscription.rs`) that bypasses the widget tree, so a modal's own
    /// focused `text_input` does not stop the same press from also reaching
    /// the PTY, only this predicate does. Every modal here MUST also be a
    /// full-window overlay (so a set flag always means a visible, input-
    /// owning modal) and SHOULD appear in `close_topmost_modal` so Esc
    /// dismisses it. The SFTP modals now layer at the app root via
    /// `layer_sftp_modals`, so they satisfy that invariant too.
    pub(crate) fn any_modal_blocks_input(&self) -> bool {
        // Exhaustive over every modal via `is_modal_open` (compiler-checked
        // match) + `Modal::ALL`: a new modal variant can't be added without
        // an `is_modal_open` arm, so it can never silently leak keystrokes
        // into the PTY behind it. The keyboard-interactive (2FA / OTP)
        // prompt is included here (its text fields own the keyboard); the
        // inline connect-progress path is gated separately by
        // `connecting.is_none()`.
        crate::state::Modal::ALL
            .iter()
            .any(|&m| m.blocks_input() && self.is_modal_open(m))
    }

    /// Closes the topmost open modal / overlay if any, and returns
    /// `Some(task)` when something was closed (`None` lets the Esc
    /// handler in `dispatch_terminal.rs` forward the byte to the
    /// active PTY). Most closes return an inert task; the answer-
    /// bearing modals route their safe default through the real
    /// handler instead, which is why this returns a task at all.
    /// Priority follows visual stacking order: pickers on top of
    /// the chrome are checked before background panels.
    pub(crate) fn close_topmost_modal(&mut self) -> Option<iced::Task<crate::app::Message>> {
        // Open dropdown / popover overlay (sort menu, kebab menus, the
        // floating toolbar search + overflow). Esc dismisses it first,
        // matching the click-outside backdrop. Lightweight, so it takes
        // priority over the heavier modals below.
        if self.overlay.is_some() {
            self.overlay = None;
            return Some(iced::Task::none());
        }
        // The SFTP right-click row menu is the same weight as an overlay
        // dropdown; Esc dismisses it like its click-outside backdrop.
        if self.sftp.row_menu.is_some() {
            self.sftp.row_menu = None;
            return Some(iced::Task::none());
        }
        // The SFTP path-history dropdown (issue #85) is the same weight;
        // Esc mirrors its scrim click.
        if self.sftp.left.path_history_open || self.sftp.right.path_history_open {
            self.sftp.left.path_history_open = false;
            self.sftp.right.path_history_open = false;
            return Some(iced::Task::none());
        }
        // Walk the Esc-close priority order and dismiss the first open
        // modal. `close_modal` is a compiler-checked exhaustive match, so a
        // new modal can't be added without deciding its cleanup; adding it
        // to `ESC_ORDER` then makes Esc dismiss it.
        for &m in crate::state::Modal::ESC_ORDER {
            if self.is_modal_open(m) {
                // The chain editor's Esc is two-stage: in "add a hop" mode
                // the first Esc pops back to the chain list, only a second
                // closes the whole editor.
                if m == crate::state::Modal::ChainEditor && self.chain_editor_adding {
                    self.chain_editor_adding = false;
                    self.chain_editor_search.clear();
                    return Some(iced::Task::none());
                }
                // Same two-stage rule for the new-tab picker drilled
                // into a group: first Esc backs out to the top level
                // (mirrors the Back header), second Esc closes.
                if m == crate::state::Modal::NewTabPicker
                    && self.new_tab_picker_group.is_some()
                {
                    self.new_tab_picker_group = None;
                    self.new_tab_picker_search.clear();
                    return Some(iced::Task::none());
                }
                // Esc on the overwrite conflict is the Cancel BUTTON,
                // not a dismissal: the conflict arm already parked the
                // item and paused the queue, so the answer must reach
                // the transfer that asked or it stays paused forever.
                // Same rule as KbiPrompt / HostKey: route the safe
                // default through the real handler.
                if m == crate::state::Modal::SftpOverwrite {
                    return Some(self.update(crate::app::Message::Sftp(
                        crate::app::SftpMessage::SftpResolveOverwrite(
                            crate::state::OverwriteAction::Cancel,
                        ),
                    )));
                }
                self.close_modal(m);
                // Closing an agent-confirm prompt promotes the next
                // queued one (no-op for every other modal).
                return Some(self.advance_confirm_queue());
            }
        }
        // Burger menu last; it's a dropdown rather than a modal but
        // Esc still feels right for it.
        if self.panels.burger_menu {
            self.panels.burger_menu = false;
            return Some(iced::Task::none());
        }
        None
    }
}
