//! Command-history plumbing: the central user-input write sink (which feeds
//! the capture), vault recording, the sidebar list refresh and the History
//! tab's message handlers.

// The `Err(message)` pass-through of the try_handler! chain carries the full
// Message enum by design; same allowance as the sibling dispatch modules.
#![allow(clippy::result_large_err)]

use crate::app::Oryxis;
use crate::messages::{CommandHistoryMessage, Message};
use iced::Task;
use uuid::Uuid;

impl Oryxis {
    pub(crate) fn handle_command_history(
        &mut self,
        message: CommandHistoryMessage,
    ) -> Task<Message> {
        match message {
            CommandHistoryMessage::HistoryCardHovered(idx) => {
                self.hover.history_card = Some(idx);
            }
            CommandHistoryMessage::HistoryCardUnhovered(idx) => {
                self.hover.leave_history_card(idx);
            }
            CommandHistoryMessage::CmdHistorySearchChanged(v) => {
                self.cmd_history_search = v;
            }
            CommandHistoryMessage::ExportCommandHistory => {
                let Some(host) = self.command_history_host else {
                    return Task::none();
                };
                if self.command_history.is_empty() {
                    return Task::none();
                }
                let label = self
                    .connections
                    .iter()
                    .find(|c| c.id == host)
                    .map(|c| c.label.clone())
                    .unwrap_or_else(|| host.to_string());
                let body = render_history_txt(&label, &self.command_history);
                let default_name = format!("oryxis-history-{}.txt", crate::util::sanitize_file_stem(&label));
                return Task::perform(
                    tokio::task::spawn_blocking(move || {
                        let path = rfd::FileDialog::new()
                            .set_title("Export command history")
                            .set_file_name(&default_name)
                            .add_filter("Text", &["txt"])
                            .save_file()?;
                        Some(
                            std::fs::write(&path, body)
                                .map(|_| path.display().to_string())
                                .map_err(|e| e.to_string()),
                        )
                    }),
                    |res| match res {
                        Ok(Some(outcome)) => Message::CommandHistory(CommandHistoryMessage::CommandHistoryExported(outcome)),
                        // Dialog dismissed or the blocking task died:
                        // nothing to report.
                        _ => Message::NoOp,
                    },
                );
            }
            CommandHistoryMessage::CommandHistoryExported(result) => {
                return match result {
                    Ok(path) => self.show_toast(
                        crate::i18n::t("history_export_done").replace("{path}", &path),
                    ),
                    Err(e) => self.show_toast(
                        crate::i18n::t("history_export_failed").replace("{error}", &e),
                    ),
                };
            }
            CommandHistoryMessage::ToggleCommandHistoryFile => {
                self.prefs.command_history_file = !self.prefs.command_history_file;
                self.persist_setting(
                    "command_history_file",
                    if self.prefs.command_history_file { "true" } else { "false" },
                );
            }
            CommandHistoryMessage::PickCommandHistoryDir => {
                return Task::perform(
                    tokio::task::spawn_blocking(|| {
                        rfd::FileDialog::new()
                            .set_title("Command log folder")
                            .pick_folder()
                            .map(|p| p.display().to_string())
                    }),
                    |res| Message::CommandHistory(CommandHistoryMessage::CommandHistoryDirPicked(res.ok().flatten())),
                );
            }
            CommandHistoryMessage::CommandHistoryDirPicked(dir) => {
                if let Some(dir) = dir {
                    self.persist_setting("command_history_file_dir", &dir);
                    self.prefs.command_history_file_dir = Some(dir);
                }
            }
            CommandHistoryMessage::RunHistoryCommand(id) => {
                self.inject_history_command(id, true);
            }
            CommandHistoryMessage::PasteHistoryCommand(id) => {
                self.inject_history_command(id, false);
            }
            CommandHistoryMessage::RequestDeleteHistoryCommand(id) => {
                // Deleting is destructive and the trash icon floats over
                // the row on hover, one pixel from the paste click, so it
                // goes through the shared confirm (Enter confirms via the
                // modal keyboard layer, like every other destructive).
                if let Some(entry) = self.command_history.iter().find(|e| e.id == id) {
                    let name: String = entry.command.lines().next().unwrap_or("").chars().take(48).collect();
                    self.confirm_remove(name, Message::CommandHistory(CommandHistoryMessage::DeleteHistoryCommand(id)));
                }
            }
            CommandHistoryMessage::DeleteHistoryCommand(id) => {
                if let Some(ref vault) = self.vault {
                    let _ = vault.delete_command_history_entry(&id);
                }
                // Tripwire for the debug log: history rows only ever
                // leave the vault through here (or a host deletion), so
                // any future "my history vanished" report is
                // attributable at a glance.
                tracing::info!(%id, "command-history: entry deleted by user");
                self.command_history.retain(|e| e.id != id);
            }
        }
        Task::none()
    }

    /// Write user-originated `bytes` to the tab's focused pane (SSH session
    /// or local PTY) and mirror them into the command-history capture. Every
    /// user input path funnels through here; the one deliberate exception is
    /// the sudo-password autofill, which writes directly so a secret never
    /// touches the capture's line mirror.
    pub(crate) fn write_input_to_tab(&mut self, tab_idx: usize, bytes: &[u8]) {
        // Typing into the terminal DISENGAGES the sidebar keynav ring: the
        // user has moved on, and a lingering ring would keep consuming
        // Enter (live-QA bug: Enter appeared dead on an SSH tab because a
        // forgotten ring from a Ctrl+Shift+H test was swallowing it).
        // Sidebar-originated injections use the `_ring_injection_` variant
        // below, which keeps the ring so arrow-Enter-arrow-Enter works.
        self.keynav.sidebar_selected = None;
        // The user typed, so they have taken over: a login script racing
        // them is worse than no script (issue #122). Their own keystroke
        // still reaches the PTY below.
        if let Some(tab) = self.tabs.get(tab_idx) {
            let pane_id = tab.active().id;
            self.abort_login_script(pane_id);
        }
        // Same reasoning for the password-suggest popup (issue #117):
        // the user is answering the prompt themselves. The key router
        // already dismisses on a printable key so the keystroke is not
        // swallowed; this catches every other input path (paste, IME,
        // snippet Run) with one rule.
        self.dismiss_password_suggest();
        self.write_ring_injection_to_tab(tab_idx, bytes);
    }

    /// [`Self::write_input_to_tab`] without the ring disengage, for
    /// injections the sidebar itself fires (row Paste / Run actions).
    pub(crate) fn write_ring_injection_to_tab(&mut self, tab_idx: usize, bytes: &[u8]) {
        let mut active_received = false;
        // Scroll-on-input (issue #111): the pane that receives the bytes
        // jumps back to the live edge, so what the user types is on screen
        // instead of hidden below a scrolled-up viewport. Read before the
        // mutable borrow of `self.tabs`.
        let snap = self.prefs.scrollback_reset_keypress;
        if let Some(tab) = self.tabs.get_mut(tab_idx) {
            // Hybrid tab showing its Files (SFTP) surface: the terminal
            // is hidden and the keyboard belongs to the SFTP view (its
            // filter inputs, modals, shortcuts). Bytes must not leak
            // into the invisible PTY.
            if tab.files_mode {
                return;
            }
            // The set of panes this write reaches: every participating pane
            // when the tab broadcasts (C2), else just the active pane. A
            // ZMODEM transfer owns its pane's byte channel, so such panes are
            // excluded from either path (a stray keystroke would corrupt the
            // protocol; cancel is the overlay's own button).
            let targets = tab.broadcast_target_ids();
            active_received = targets.contains(&tab.active().id);
            for pane in tab.pane_grid.panes.values_mut() {
                if targets.contains(&pane.id) {
                    Self::write_bytes_to_pane(pane, bytes, snap);
                }
            }
        }
        // Capture mirrors ONCE per tab, and only when the ACTIVE pane
        // actually received the bytes. A pane mid-ZMODEM (or, under
        // broadcast, one opted out) is not in `targets`, so a keystroke it
        // never got must not land in its command history; this preserves
        // the pre-broadcast funnel, which returned before capture when the
        // active pane was transferring. Broadcast still records the command
        // a single time against the active pane, never N× (see
        // `feed_input_capture`).
        if active_received {
            self.feed_input_capture(tab_idx, bytes);
        }
    }

    /// Write `bytes` to a single pane's transport: the remote session (SSH /
    /// Telnet / Serial) when connected, otherwise the local PTY. Errors are
    /// swallowed the same way the single-pane path always has (a disconnected
    /// pane shows its own dead state).
    ///
    /// `snap_to_bottom` queues the widget's scroll-back reset (issue #111,
    /// the `scrollback_reset_keypress` setting): the next draw consumes
    /// `pending_scroll` and paints the live edge. Queuing it here, on the
    /// byte funnel, is what keeps it honest: it fires for exactly the input
    /// the PTY receives (keys, paste, snippet Run / Paste, every broadcast
    /// target) and never for typing that belongs to another surface, such as
    /// the sidebar's chat or search inputs.
    pub(crate) fn write_bytes_to_pane(
        pane: &mut crate::state::Pane,
        bytes: &[u8],
        snap_to_bottom: bool,
    ) {
        if let Some(ref session) = pane.session {
            let _ = session.write(bytes);
            if snap_to_bottom
                && let Ok(state) = pane.terminal.lock()
            {
                state.pending_scroll.set(Some(0));
            }
        } else if let Ok(mut state) = pane.terminal.lock() {
            state.write(bytes);
            if snap_to_bottom {
                state.pending_scroll.set(Some(0));
            }
        }
    }

    /// Write bytes to ONE pane that the user did not type.
    ///
    /// The documented input-capture bypass, used by the login-script
    /// runner and the sudo-password autofill. Everything it skips is
    /// deliberate:
    ///
    /// - `observe_input`, so a credential never reaches the command
    ///   history mirror. Guaranteed by construction rather than by a
    ///   test: this function simply does not call `feed_input_capture`,
    ///   and every secret write in the app goes through here;
    /// - the broadcast fan-out, because a secret goes to exactly one
    ///   pane, never to every pane of a synchronized tab;
    /// - bracketed paste and the paste guard, because a password prompt
    ///   is not a paste target and a confirm dialog mid-login would be
    ///   worse than useless.
    ///
    /// Suppressed while a ZMODEM transfer owns the pane's byte stream
    /// or the tab is showing its Files surface: in both cases these
    /// bytes would be read by something that is not a shell.
    pub(crate) fn write_secret_to_pane(&mut self, pane_id: uuid::Uuid, bytes: &[u8]) {
        let Some(tab_idx) = self.pane_tab_index(pane_id) else {
            return;
        };
        // Unlike a keystroke, neither caller is guaranteed to be looking
        // at this tab (a login script runs while the user reads another
        // one), and yanking a background pane's viewport out from under
        // a reader is the bug scroll-on-input exists to avoid. Same gate
        // `snap_tab_to_live_edge` documents for the AI-exec path.
        let snap = self.prefs.scrollback_reset_keypress && self.active_tab == Some(tab_idx);
        if self.tabs.get(tab_idx).is_some_and(|t| t.files_mode) {
            return;
        }
        if let Some(tab) = self.tabs.get_mut(tab_idx)
            && let Some(pane) = tab.pane_by_id_mut(pane_id)
            && pane.zmodem.is_none()
        {
            Self::write_bytes_to_pane(pane, bytes, snap);
        }
    }

    /// Queue the active pane of `tab_idx`'s jump back to the live edge,
    /// honoring the `scrollback_reset_keypress` setting. For the
    /// user-initiated writes that bypass [`Self::write_input_to_tab`] on
    /// purpose: the sudo-password autofill (keeps a secret out of the
    /// capture mirror) and the AI chat's command execution (needs the
    /// write's success). The funnel itself has the snap built in, so it
    /// never calls this.
    ///
    /// Unlike a keystroke, neither caller is guaranteed to be looking at
    /// that tab (the AI's write lands whenever the model answers, the tab
    /// may have moved on or switched to its Files surface), and yanking a
    /// background pane's viewport out from under a reader is exactly the
    /// bug this whole path exists to avoid. So the snap is gated on the tab
    /// being the visible terminal.
    pub(crate) fn snap_tab_to_live_edge(&self, tab_idx: usize) {
        if !self.prefs.scrollback_reset_keypress || self.active_tab != Some(tab_idx) {
            return;
        }
        if let Some(tab) = self.tabs.get(tab_idx)
            && !tab.files_mode
            && let Ok(state) = tab.active().terminal.lock()
        {
            state.pending_scroll.set(Some(0));
        }
    }

    /// Capture half of [`Self::write_input_to_tab`], for the rare call site
    /// that must write directly (the AI tool-exec path needs the write's
    /// success) but still wants the bytes mirrored into the history capture.
    pub(crate) fn feed_input_capture(&mut self, tab_idx: usize, bytes: &[u8]) {
        // Smart tabs reuses the capture to label a running command with
        // its command line, and an active session recording stores the
        // captured commands as 'c' chunks (the input-only export), so
        // the mirror runs for any of the three. The capture itself is
        // origin-agnostic and secret-safe by construction; only the
        // command-history vault write below needs a saved host.
        let want_history = self.prefs.command_history;
        let want_smart = self.prefs.smart_tabs;
        let mut captured: Vec<(Uuid, String)> = Vec::new();
        let mut session_cmds: Vec<(Uuid, Option<i64>, String)> = Vec::new();
        if let Some(tab) = self.tabs.get_mut(tab_idx) {
            let pane = tab.active_mut();
            let want_session = pane.session_log_id.is_some();
            if !want_history && !want_smart && !want_session {
                return;
            }
            // The per-host history takes shell commands only. An SFTP
            // console is a saved host with a prompt, so it satisfies
            // every other condition here, but its vocabulary is
            // `sftp(1)`'s and the history exists to be re-inserted into
            // a SHELL, where `get access.log` is not a command.
            //
            // Gated on both capture paths, because there are two: this
            // one reads the input, the other (`observe_output_marks`)
            // reads the OSC 133 marks. Missing either lets the console
            // in through the side the reviewer was not looking at.
            let host = match &pane.origin {
                crate::state::PaneOrigin::Host(hid)
                    if pane.purpose != crate::state::PanePurpose::SftpConsole =>
                {
                    Some(*hid)
                }
                _ => None,
            };
            let cmds = crate::command_capture::observe_input(pane, bytes);
            // The next OSC 133 OutputStart adopts this as the command
            // that just started running.
            if want_smart && let Some(cmd) = cmds.last() {
                pane.last_submitted = Some(cmd.clone());
            }
            // Session recording keys on the log, not the host, so
            // quick-connect / local panes are covered too.
            if let Some(log_id) = pane.session_log_id {
                let off = pane
                    .session_log_t0
                    .map(|t| t.elapsed().as_millis() as i64);
                session_cmds.extend(cmds.iter().map(|c| (log_id, off, c.clone())));
            }
            // Only saved hosts get history (quick-connect / local
            // panes have no persistable identity to key it on).
            if want_history && let Some(hid) = host {
                captured.extend(cmds.into_iter().map(|cmd| (hid, cmd)));
            }
        }
        for (host, cmd) in captured {
            self.record_command_history(host, cmd);
        }
        for (log_id, off, cmd) in session_cmds {
            self.record_session_command(&log_id, off, &cmd);
        }
    }

    /// Persist one captured command and keep the open History tab live.
    pub(crate) fn record_command_history(&mut self, host: Uuid, cmd: String) {
        // Captured commands get the same two defenses as the
        // session-recording 'c' chunks: the redaction pass runs BEFORE
        // anything is persisted (an ECHOED inline secret is a
        // legitimate command line to the capture gates, so it must be
        // scrubbed here), and the vault seals the text with the content
        // key at rest (`command_enc`; the plaintext column carries only
        // a keyed dedup hash). The optional plain-text mirror below
        // also sees only the scrubbed text.
        let cmd = crate::session_redact::redact_command(&cmd);
        if let Some(ref vault) = self.vault {
            match vault.record_command(&host, &cmd) {
                Ok(()) => {}
                // Soft auto-lock keeps sessions alive while the master
                // key is zeroized; recording is paused, not broken, and
                // no sink (vault or file) runs without it.
                Err(oryxis_vault::VaultError::Locked) => return,
                Err(e) => tracing::warn!("command-history write failed: {e}"),
            }
        }
        // Optional plain-text mirror: append to the host's log file for
        // offline reference / support sharing. Plain filesystem write on
        // purpose (no vault), that is the feature.
        if self.prefs.command_history_file {
            let label = self
                .connections
                .iter()
                .find(|c| c.id == host)
                .map(|c| c.label.clone())
                .unwrap_or_else(|| host.to_string());
            if let Err(e) = self.append_command_log(&host, &label, &cmd) {
                tracing::warn!("command-history file append failed: {e}");
            }
        }
        if self.sidebar_tab_shown(crate::state::TerminalSidebarTab::History)
            && self.command_history_host == Some(host)
        {
            self.refresh_command_history();
        }
    }

    /// Persist one captured command into the pane's active session
    /// recording as a 'c' chunk (the input-only export). Same
    /// redaction pass as the output chunks, so an inline secret
    /// (`mysql -p...`) is scrubbed before it reaches the vault.
    pub(crate) fn record_session_command(
        &self,
        log_id: &Uuid,
        offset_ms: Option<i64>,
        cmd: &str,
    ) {
        let Some(vault) = &self.vault else { return };
        let text = crate::session_redact::redact_command(cmd);
        if let Err(e) = vault.append_session_command(log_id, offset_ms, &text) {
            tracing::warn!("session command append failed for {log_id}: {e}");
        }
    }

    /// The folder the per-host command logs live in: the configured
    /// setting, or `~/.oryxis/command-history/` by default.
    pub(crate) fn command_history_dir(&self) -> std::path::PathBuf {
        match &self.prefs.command_history_file_dir {
            Some(dir) => std::path::PathBuf::from(dir),
            None => oryxis_core::paths::oryxis_dir()
                .unwrap_or_else(|| std::path::PathBuf::from(".").join(".oryxis"))
                .join("command-history"),
        }
    }

    /// Append one captured command to the host's log file
    /// (`<dir>/<label>-<uuid8>.txt`, one `timestamp<TAB>command` line).
    /// The uuid suffix keeps two hosts with the same label apart and
    /// the file name stable across renames of neither.
    fn append_command_log(
        &self,
        host: &Uuid,
        label: &str,
        cmd: &str,
    ) -> std::io::Result<()> {
        use std::io::Write;
        let dir = self.command_history_dir();
        // Owner-only like the vault file (0700 dir / 0600 file): the log
        // content is plaintext by design, but that is no reason to let
        // other local users read a per-host command trail.
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt as _;
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&dir)?;
        }
        #[cfg(not(unix))]
        std::fs::create_dir_all(&dir)?;
        let short: String = host.to_string().chars().take(8).collect();
        let path = dir.join(format!("{}-{}.txt", crate::util::sanitize_file_stem(label), short));
        let mut opts = std::fs::OpenOptions::new();
        opts.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            opts.mode(0o600);
        }
        let mut f = opts.open(path)?;
        // Multi-line commands (bracketed paste) stay one log line each:
        // continuation lines are indented so the file remains greppable
        // per entry.
        let cmd_one = cmd.replace('\n', "\n    ");
        writeln!(f, "{}\t{}", chrono::Utc::now().to_rfc3339(), cmd_one)
    }
}

/// Human-readable export body: a small header, then one line per
/// captured command, oldest first (the in-memory list is
/// most-recent-first). Multi-line commands indent their continuation
/// lines, same convention as the live-append log.
fn render_history_txt(
    label: &str,
    entries: &[oryxis_vault::CommandHistoryEntry],
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# Oryxis command history: {label}\n# Exported {}\n\n",
        chrono::Utc::now().to_rfc3339()
    ));
    for e in entries.iter().rev() {
        // Rows are already scrubbed at record time, but re-redacting
        // here covers rows recorded before redaction existed, so a
        // legacy secret can't be copied into a plaintext export file.
        let cmd_one = crate::session_redact::redact_command(&e.command).replace('\n', "\n    ");
        let uses = if e.use_count > 1 {
            format!("\t(x{})", e.use_count)
        } else {
            String::new()
        };
        out.push_str(&format!(
            "{}{}\t{}\n",
            e.last_used_at.to_rfc3339(),
            uses,
            cmd_one
        ));
    }
    out
}


impl Oryxis {
    /// Reload the sidebar list for the focused pane's host. Called when the
    /// History tab is opened, when tab/pane focus moves, and after a record
    /// while the tab is showing that host.
    pub(crate) fn refresh_command_history(&mut self) {
        let host = self
            .active_tab
            .and_then(|i| self.tabs.get(i))
            .and_then(|t| match &t.active().origin {
                crate::state::PaneOrigin::Host(id) => Some(*id),
                _ => None,
            });
        self.command_history_host = host;
        self.command_history = match (host, &self.vault) {
            (Some(h), Some(v)) => v.list_command_history(&h).unwrap_or_default(),
            _ => Vec::new(),
        };
        self.hover.history_card = None;
    }

    /// Re-insert a history entry into the active terminal, exactly like a
    /// snippet: bracketed-paste wrapped, with the submit newline outside the
    /// bracket when `run`. Goes through the capture sink, so running it
    /// counts another use.
    fn inject_history_command(&mut self, id: Uuid, run: bool) {
        let Some(cmd) = self
            .command_history
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.command.clone())
        else {
            return;
        };
        let Some(tab_idx) = self.snippet_injection_tab() else {
            return;
        };
        let Some(tab) = self.tabs.get(tab_idx) else {
            return;
        };
        let bracketed = tab
            .active()
            .terminal
            .lock()
            .map(|s| s.bracketed_paste_enabled())
            .unwrap_or(false);
        let mut payload = oryxis_terminal::wrap_paste(&cmd, bracketed);
        if run {
            payload.push(b'\n');
        }
        // Ring-preserving: Enter on a ringed row must not drop the ring,
        // so the user can arrow to the next command and Enter again.
        self.write_ring_injection_to_tab(tab_idx, &payload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_export_redacts_legacy_rows() {
        // Rows written before record-time redaction existed may still
        // carry inline secrets; the .txt export must scrub them again
        // so they never reach a plaintext file on disk.
        let entry = oryxis_vault::CommandHistoryEntry {
            id: Uuid::new_v4(),
            connection_id: Uuid::new_v4(),
            command: "export AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI/K7MDENG".into(),
            use_count: 3,
            last_used_at: chrono::Utc::now(),
        };
        let body = render_history_txt("web-01", &[entry]);
        assert!(!body.contains("wJalrXUtnFEMI"), "secret leaked into export: {body}");
        assert!(body.contains("AWS_SECRET_ACCESS_KEY"), "key name should survive");
        assert!(body.contains("[REDACTED]"));
    }

    /// A pane with no live session still writes into its local terminal
    /// state, which is all this test needs: the assertion is about the
    /// queued scroll, not the transport.
    fn test_pane() -> crate::state::Pane {
        let term = oryxis_terminal::TerminalState::new_no_pty(80, 24).unwrap();
        crate::state::Pane::new("test".into(), std::sync::Arc::new(std::sync::Mutex::new(term)))
    }

    /// Issue #111: input sent to the PTY queues the jump back to the live
    /// edge, so a user typing into a scrolled-up viewport sees what they
    /// type. The draw consumes `pending_scroll`; queuing 0 is the whole
    /// contract on this side.
    #[test]
    fn input_queues_the_live_edge_snap() {
        let mut pane = test_pane();
        Oryxis::write_bytes_to_pane(&mut pane, b"ls\n", true);
        let queued = pane.terminal.lock().unwrap().pending_scroll.get();
        assert_eq!(queued, Some(0), "input must queue a scroll back to the live edge");
    }

    /// With the setting off (PuTTY's own default), the viewport stays where
    /// the user parked it.
    #[test]
    fn input_leaves_the_scroll_alone_when_disabled() {
        let mut pane = test_pane();
        Oryxis::write_bytes_to_pane(&mut pane, b"ls\n", false);
        let queued = pane.terminal.lock().unwrap().pending_scroll.get();
        assert_eq!(queued, None, "the snap must stay opt-out-able");
    }
}
