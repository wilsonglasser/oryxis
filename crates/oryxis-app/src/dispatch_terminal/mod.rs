//! `Oryxis::handle_terminal`, match arms for terminal I/O: PTY bytes
//! coming back, keyboard events routed to the active tab, split-pane
//! management, the scrollback find-bar, broadcast input, the paste
//! paths and the terminal context menu. The router fans `Message`
//! variants out to per-area submodules:
//!
//! - `output`  : the `PtyOutput` firehose + the batched session-log
//!   flush machinery (and its timing/alignment helpers).
//! - `keyboard`: the `KeyboardEvent` chord resolver and PTY key
//!   routing.
//! - `triggers`: what a user highlight rule DOES when it matches
//!   (notification / beep / snippet), including the confirmation that
//!   keeps remote output from choosing which snippet runs.
//! - `links`  : opening a URL the terminal printed: the confirmation a
//!   remote host's link gets, and the SSH tunnel a loopback OAuth
//!   callback needs before a browser here can complete it.
//!
//! The small arms (pane focus/split/close, search bar, broadcast
//! toggles, paste/copy, context menu, IME commit) stay here.

#![allow(clippy::result_large_err)]

mod drop;
mod keyboard;
mod links;
mod output;
mod triggers;

pub(crate) use links::LinkConfirmCard;
pub(crate) use triggers::TriggerConfirmCard;

use iced::Task;

use crate::app::{TabsMessage, TerminalMessage, Message, Oryxis};

/// Paste-funnel tracing (debug log), one line per stage: the gesture
/// asking for the clipboard, which buffer a selection paste resolved
/// against, what came back, which gate the text hit, and the bytes
/// handed to the session. Issue #181 is why it exists:
/// a paste that "does nothing" is invisible in a log today, because a
/// chord consumed by the router never reaches the `key-encode` line and
/// every stage after it is silent, so a report cannot tell a gesture
/// that never arrived from a clipboard that read back empty from text
/// that reached the wire and was swallowed by the remote application.
///
/// Never the CONTENT: a clipboard carries whatever the user copied last,
/// and a debug log is the one place a password would end up on disk in
/// clear. Shape only (character count, line count, bracketed mode).
fn paste_trace(stage: &str, outcome: &str, text: &str, bracketed: Option<bool>) {
    tracing::debug!(
        stage,
        outcome,
        chars = text.chars().count(),
        lines = text.lines().count(),
        ?bracketed,
        "paste"
    );
}

impl Oryxis {
    /// Tear down every remote session (SSH or Telnet) in a tab.
    /// Dropping the pane alone is not enough: the connect stream task
    /// holds its own Arc to the session, so without an explicit
    /// close() the engine tasks, the channel, and any per-connection
    /// port-forward listeners keep running (and generating UI
    /// messages) forever.
    pub(crate) fn close_tab_sessions(tab: &crate::state::TerminalTab) {
        for pane in tab.pane_grid.panes.values() {
            if let Some(session) = &pane.session {
                session.close();
            }
        }
    }

    /// Paste `text` into `tab_idx`'s session. Careful-paste gate: when the
    /// setting is on (default) and the text contains a line break, the paste
    /// is parked in `pending_paste` and a confirmation dialog (line count +
    /// preview) takes over, so a hidden trailing newline can't auto-run a
    /// command. Single-line pastes, and every paste when the guard is off, go
    /// straight to the session.
    ///
    /// The target tab is a parameter, not `self.active_tab`: clipboard reads
    /// resolve one or more `update()`s after the gesture that asked for them
    /// (the runtime performs them off-thread, and an RDS / delayed-rendering
    /// clipboard owner can hold `GetClipboardData` for a long time), so every
    /// paste path captures its tab when the read is requested. Re-resolving
    /// the active tab at delivery time would drop the text into whatever tab
    /// the user switched to meanwhile, i.e. into a different host's shell.
    ///
    /// It is the tab's stable `_id`, not its index, for the second half of
    /// the same reason: an index captured before the read names a POSITION,
    /// and closing any earlier tab in the meantime slides another session
    /// into it. The text would still land in a live shell, just the wrong
    /// one, and "the index still exists" cannot tell the two apart. Same
    /// hazard `pending_pane_split` had.
    pub(crate) fn paste_text_into_tab(&mut self, tab_id: uuid::Uuid, text: &str) {
        if self.tab_index_by_id(tab_id).is_none() {
            paste_trace("gate", "target tab gone", text, None);
            return;
        }
        // Two independent gates (owner call: each has its own setting):
        // careful paste parks multi-line text; the paste guard parks
        // suspicious CONTENT (bidi/invisible chars, raw control bytes,
        // curl|sh one-liners, homograph tokens) even on one line.
        if (self.prefs.careful_paste
            && (text.contains('\n') || text.contains('\r')))
            || (self.prefs.paste_guard
                && !crate::paste_guard::paste_warnings(text).is_empty())
        {
            paste_trace("gate", "parked for confirm", text, None);
            self.pending_paste = Some((tab_id, text.to_string()));
            // An ordinary paste park replaces whatever was parked; a
            // stale install marker (issue #147) must not survive to
            // claim this text's confirm.
            self.pending_paste_install = None;
            return;
        }
        self.write_paste_to_tab(tab_id, text);
    }

    /// Write `text` into `tab_idx`'s session, wrapping it for
    /// bracketed-paste when the focused app enabled it (`\e[?2004h`).
    /// Routes to the SSH session when one is attached, otherwise the
    /// local PTY. Shared by the clipboard (right-click / Ctrl+Shift+V)
    /// paste paths and the careful-paste confirmation. Explicit tab for the
    /// reason in [`Self::paste_text_into_tab`].
    pub(crate) fn write_paste_to_tab(&mut self, tab_id: uuid::Uuid, text: &str) {
        let Some(tab_idx) = self.tab_index_by_id(tab_id) else {
            paste_trace("write", "target tab gone", text, None);
            return;
        };
        let Some(tab) = self.tabs.get(tab_idx) else {
            paste_trace("write", "target tab gone", text, None);
            return;
        };
        let bracketed = tab
            .active()
            .terminal
            .lock()
            .map(|s| s.bracketed_paste_enabled())
            .unwrap_or(false);
        let payload = oryxis_terminal::wrap_paste(text, bracketed);
        paste_trace("write", "to session", text, Some(bracketed));
        self.write_input_to_tab(tab_idx, &payload);
    }

    /// Read the system clipboard and paste it into the active session.
    /// Shared by the Ctrl+Shift+V / Shift+Insert / Cmd+V (macOS) key paths,
    /// the terminal context menu and the widget's paste hook, so the
    /// bracketed-paste handling lives in exactly one place.
    ///
    /// The read is a `Task`: the iced runtime owns the clipboard and serves
    /// one access at a time. Reading it inline here (arboard on the UI
    /// thread) raced the runtime's own paste read and killed the process on
    /// Windows with `STATUS_HEAP_CORRUPTION` inside `GetClipboardData`
    /// (field crash 2026-07-29; see `oryxis_terminal::host_clipboard`).
    pub(crate) fn paste_clipboard_into_active(&mut self) -> Task<Message> {
        // Capture the target tab NOW, not when the text comes back, and
        // capture its id rather than its index: see `paste_text_into_tab`.
        let Some(tab_id) = self.active_tab.and_then(|i| self.tabs.get(i)).map(|t| t._id) else {
            paste_trace("request", "no active tab", "", None);
            return Task::none();
        };
        paste_trace("request", "clipboard read", "", None);
        crate::dispatch_global::read_clipboard_text(move |text| {
            Message::Terminal(TerminalMessage::TerminalPasteResolved(
                tab_id,
                text.map(Into::into),
            ))
        })
    }

    /// Dispatch a terminal message: `PtyOutput` and `KeyboardEvent`
    /// route straight to the `output` / `keyboard` submodule handlers,
    /// the remaining small arms match inline. Exhaustive on purpose: a
    /// new `TerminalMessage` variant fails to compile until it gets an
    /// arm, so it can never be silently dropped.
    pub(crate) fn handle_terminal(
        &mut self,
        message: TerminalMessage,
    ) -> Task<Message> {
        match message {
            TerminalMessage::PtyOutput(..) | TerminalMessage::LocalStartupDue(..) => {
                return self
                    .handle_terminal_output(message)
                    .unwrap_or_else(crate::dispatch::unrouted);
            }
            TerminalMessage::KeyboardEvent(..) => {
                return self
                    .handle_terminal_keyboard(message)
                    .unwrap_or_else(crate::dispatch::unrouted);
            }
            // -- Split panes --
            TerminalMessage::FocusPane(pane) => {
                if let Some(tab_idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(tab_idx)
                {
                    // A composition interrupted mid-way (focus clicks onto
                    // another pane) leaves stale preedit on the pane being
                    // left; clear it or the overlay would re-show an
                    // already-abandoned composition when the user returns.
                    let old = tab.focused;
                    tab.focused = pane;
                    if old != pane
                        && let Some(old_pane) = tab.pane_grid.get(old)
                        && let Ok(mut state) = old_pane.terminal.lock()
                    {
                        state.set_preedit(String::new());
                    }
                }
                // Clicking a terminal pane takes the keyboard back from the
                // sidebar ring (see write_input_to_tab for the rationale),
                // and drops the dropdown gate defensively: a click outside
                // an open pick_list normally fires on_close, but if the
                // widget unmounted first the stuck flag would swallow
                // Enter/Space/Esc/arrows forever.
                self.keynav.sidebar_selected = None;
                self.keynav.pick_open = false;
                // A click on a pane is the user answering the prompt
                // themselves (or moving on): the suggestion popup goes
                // with it. This is also the popup's click-outside
                // dismissal, since it renders without a backdrop.
                self.dismiss_password_suggest();
                // The History tab is per-host; follow the focused pane.
                if self.sidebar_tab_shown(crate::state::TerminalSidebarTab::History) {
                    self.refresh_command_history();
                }
                // The Files browser is per-pane; the newly focused pane may
                // need a mount or a cwd catch-up (no-op otherwise).
                return self.sidebar_files_sync();
            }
            TerminalMessage::ResizePane(ev) => {
                if let Some(tab_idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(tab_idx)
                {
                    tab.pane_grid.resize(ev.split, ev.ratio);
                }
            }
            TerminalMessage::SplitPane(axis) => {
                // Open the connection picker to choose what fills the new
                // pane (a host, or a local shell). The selection routes into
                // a split via `pending_pane_split` instead of a new tab.
                self.overlay = None; // dismiss the `+` hover popover if open
                if let Some(tab_idx) = self.active_tab
                    && let Some(tab) = self.tabs.get(tab_idx)
                {
                    self.pending_pane_split = Some((tab._id, tab.focused, axis));
                    self.panels.new_tab_picker = true;
                    self.new_tab_picker_search.clear();
                    self.new_tab_picker_group = None;
                    // Same focus-the-search behavior as ShowNewTabPicker.
                    return crate::widgets::focus_input(iced::widget::Id::new(
                        crate::state::NEW_TAB_PICKER_SEARCH_ID,
                    ));
                }
            }
            TerminalMessage::SplitTabPane(tab_idx, axis) => {
                // From a tab's right-click menu: focus that tab first, then
                // open the picker to fill the new split pane.
                self.overlay = None;
                if let Some(tab) = self.tabs.get(tab_idx) {
                    let target = tab.focused;
                    let tab_id = tab._id;
                    self.active_tab = Some(tab_idx);
                    self.active_view = crate::state::View::Terminal;
                    self.remember_terminal_tab_focus(tab_idx);
                    self.pending_pane_split = Some((tab_id, target, axis));
                    self.panels.new_tab_picker = true;
                    self.new_tab_picker_search.clear();
                    self.new_tab_picker_group = None;
                    // Same focus-the-search behavior as ShowNewTabPicker.
                    return crate::widgets::focus_input(iced::widget::Id::new(
                        crate::state::NEW_TAB_PICKER_SEARCH_ID,
                    ));
                }
            }
            TerminalMessage::ClosePane(target_id) => {
                // Dismiss the terminal context menu when its "Close
                // pane" row fired this (no-op on the hotkey path).
                self.overlay = None;
                // Resolve the target at dispatch time. The context-menu
                // row carries the right-clicked pane's id: focus and the
                // active tab can change via hotkeys while the menu is
                // open (the overlay is not a modal), so "the focused
                // pane of the active tab" may no longer be the pane the
                // user clicked. A pane that is gone entirely (its tab
                // closed under the menu) is a safe no-op.
                let resolved = match target_id {
                    Some(pane_id) => {
                        self.pane_tab_index(pane_id).and_then(|tab_idx| {
                            self.tabs[tab_idx]
                                .pane_grid
                                .panes
                                .iter()
                                .find(|(_, p)| p.id == pane_id)
                                .map(|(handle, _)| (tab_idx, *handle))
                        })
                    }
                    // The hotkey path acts on the focused pane of the
                    // active tab, as before.
                    None => self
                        .active_tab
                        .filter(|&i| i < self.tabs.len())
                        .map(|i| (i, self.tabs[i].focused)),
                };
                let Some((tab_idx, target)) = resolved else {
                    return Task::none();
                };
                // Last pane in the tab: closing it closes the whole tab.
                // `tab_idx` is the pane's OWN tab by construction above,
                // so a stale focus can never close an unrelated tab.
                if self.tabs[tab_idx].pane_grid.panes.len() <= 1 {
                    return self.update(Message::Tabs(TabsMessage::CloseTab(tab_idx)));
                }
                // Persist the closing pane's recorded output before it goes.
                self.flush_session_logs_final();
                let tab = &mut self.tabs[tab_idx];
                // Tear down the pane's remote session (the connect stream
                // holds its own Arc; see close_tab_sessions) and collect
                // the end-of-session bookkeeping targets. This must be
                // synchronous: the `SshDisconnected` the close provokes
                // lands after the pane is gone and resolves nothing, so
                // deferring to it would leave the vault log row open
                // forever and the monitor primed to diff the next
                // session against the dead pane's counters.
                let mut ended_log = None;
                let mut closed_host = None;
                let mut closed_pane = None;
                if let Some(pane) = tab.pane_grid.panes.get_mut(&target) {
                    if let Some(session) = pane.session.take() {
                        session.close();
                    }
                    ended_log = pane.session_log_id.take();
                    closed_pane = Some(pane.id);
                    closed_host = match pane.origin {
                        crate::state::PaneOrigin::Host(id) => Some(id),
                        _ => None,
                    };
                }
                if let Some((_closed, sibling)) = tab.pane_grid.close(target) {
                    // Only a close of the focused pane moves focus;
                    // closing a background pane from its context menu
                    // must not yank the keyboard to its sibling.
                    if tab.focused == target {
                        tab.focused = sibling;
                    }
                }
                // Back to a single pane: disarm broadcast (its control
                // surfaces are hidden for unsplit tabs, so a lingering
                // armed state would be invisible) and drop the survivor's
                // opt-out so a later re-arm starts clean.
                if !tab.broadcast_capable() && tab.broadcast {
                    tab.broadcast = false;
                    for pane in tab.pane_grid.panes.values_mut() {
                        pane.broadcast_opt_out = false;
                    }
                }
                // A collapsed split has to re-anchor the tab on the pane
                // that is left: the unsplit label comes from the TAB, which
                // was named after the pane that just closed (issue #108).
                tab.sync_label_to_sole_pane();
                if let Some(log_id) = ended_log
                    && let Some(vault) = &self.vault
                {
                    let _ = vault.end_session_log(&log_id);
                }
                // The tmux listing is per PANE, so it goes with the pane
                // unconditionally: no "is the host still open elsewhere"
                // question, because another pane owns its own listing.
                if let Some(pane_id) = closed_pane {
                    self.tmux_reset_pane(&pane_id);
                }
                // A callback tunnel belongs to the pane that opened it,
                // and the local port it holds should go back with it.
                self.prune_link_forwards();
                // Same rule as CloseTab: drop the monitor series only
                // when the closed pane was the machine's last live one
                // anywhere (the closed pane is already out of the grid,
                // and the window is shared by every row that points at
                // that server, issue #156).
                if let Some(host) = closed_host
                    && !self.monitor_machine_in_panes(&host, None)
                {
                    self.monitor_reset_host(&host);
                }
                // Drop quick-connect entries (and their in-memory
                // credentials) that no pane references anymore.
                self.prune_quick_connects();
            }
            TerminalMessage::FocusPaneDir(dir) => {
                if let Some(tab_idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(tab_idx)
                {
                    tab.focus_adjacent(dir);
                }
            }
            TerminalMessage::ToggleMaximizePane(target) => {
                // Dismiss the tab context menu when its row fired this.
                self.overlay = None;
                let Some(tab_idx) = target.or(self.active_tab) else {
                    return Task::none();
                };
                if target.is_some() {
                    self.active_tab = Some(tab_idx);
                    self.active_view = crate::state::View::Terminal;
                    self.remember_terminal_tab_focus(tab_idx);
                }
                if let Some(tab) = self.tabs.get_mut(tab_idx) {
                    tab.toggle_maximize();
                }
            }
            TerminalMessage::ToggleMaximizePaneAt(pane_id) => {
                self.overlay = None;
                // Resolved from the pane, so the zoom lands on the one
                // that was right-clicked even if focus moved while the
                // menu was open. Zooming also FOCUSES it: the grid draws
                // only the zoomed pane, so leaving the caret behind would
                // type into something invisible.
                let Some(tab_idx) = self.pane_tab_index(pane_id) else {
                    return Task::none();
                };
                self.active_tab = Some(tab_idx);
                self.active_view = crate::state::View::Terminal;
                self.remember_terminal_tab_focus(tab_idx);
                if let Some(tab) = self.tabs.get_mut(tab_idx)
                    && let Some((handle, _)) =
                        tab.pane_grid.panes.iter().find(|(_, p)| p.id == pane_id)
                {
                    let handle = *handle;
                    if tab.pane_grid.maximized().is_some() {
                        tab.pane_grid.restore();
                        tab.focused = handle;
                    } else {
                        tab.maximize_handle(handle);
                    }
                }
            }
            TerminalMessage::FlipPaneSplit(pane_id) => {
                self.overlay = None;
                let Some(tab_idx) = self.pane_tab_index(pane_id) else {
                    return Task::none();
                };
                if let Some(tab) = self.tabs.get_mut(tab_idx)
                    && let Some((handle, _)) =
                        tab.pane_grid.panes.iter().find(|(_, p)| p.id == pane_id)
                {
                    let handle = *handle;
                    tab.flip_split_at(handle);
                }
            }
            TerminalMessage::TerminalBellFlashEnd(pane_id) => {
                if let Some(pane) = self
                    .tabs
                    .iter_mut()
                    .flat_map(|t| t.pane_grid.panes.values_mut())
                    .find(|p| p.id == pane_id)
                {
                    pane.bell_flash = false;
                }
            }
            TerminalMessage::TerminalLinkActivated(pane_id, url) => {
                return self.activate_terminal_link(pane_id, url);
            }
            TerminalMessage::TerminalLinkDecision(open) => {
                return self.resolve_link_confirm(open);
            }
            TerminalMessage::TerminalLinkCopy => {
                return self.copy_link_confirm();
            }
            TerminalMessage::TerminalLinkTunnelReady(pane_id, port, url, outcome) => {
                return self.link_tunnel_ready(pane_id, port, url, outcome);
            }
            TerminalMessage::TerminalLinkTunnelClosed(pane_id, port) => {
                self.link_tunnel_closed(pane_id, port);
            }
            TerminalMessage::TriggerConfirmDecision(allow) => {
                self.resolve_trigger_confirm(allow);
            }
            TerminalMessage::TerminalSyncFlush(pane_id) => {
                if let Some(pane) = self
                    .tabs
                    .iter_mut()
                    .flat_map(|t| t.pane_grid.panes.values_mut())
                    .find(|p| p.id == pane_id)
                {
                    pane.sync_flush_scheduled = false;
                    let mut reschedule: Option<std::time::Duration> = None;
                    if let Ok(mut state) = pane.terminal.lock() {
                        match state.sync_timeout() {
                            // The app extended the update past our deadline
                            // (a fresh BSU reset vte's 150 ms timer): re-arm
                            // for the new deadline instead of flushing
                            // mid-update, matching alacritty's behavior.
                            Some(deadline) if deadline > std::time::Instant::now() => {
                                reschedule = Some(deadline.saturating_duration_since(
                                    std::time::Instant::now(),
                                ));
                            }
                            // Deadline reached, update still open: force the
                            // buffered frame onto the grid.
                            Some(_) => state.flush_sync(),
                            // Closed normally in the meantime; nothing to do.
                            None => {}
                        }
                    }
                    if let Some(remaining) = reschedule {
                        pane.sync_flush_scheduled = true;
                        return Task::perform(
                            async move {
                                tokio::time::sleep(remaining).await;
                            },
                            move |_| Message::Terminal(TerminalMessage::TerminalSyncFlush(pane_id)),
                        );
                    }
                }
            }
            // ── Scrollback find-bar (C1) ──
            TerminalMessage::TerminalSearchOpen => {
                if let Some(idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(idx)
                {
                    let pane = tab.active_mut();
                    pane.search_open = true;
                    if let Ok(mut state) = pane.terminal.lock() {
                        state.search_open();
                        // Re-scan for the current needle so re-opening on the
                        // same query lands on live matches immediately.
                        if !pane.search_query.is_empty() {
                            state.search_set_query(&pane.search_query);
                        }
                    }
                    return crate::widgets::focus_input(iced::widget::Id::new(
                        "terminal-buffer-search",
                    ));
                }
            }
            TerminalMessage::TerminalSearchInput(v) => {
                if let Some(idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(idx)
                {
                    let pane = tab.active_mut();
                    pane.search_query = v;
                    if let Ok(mut state) = pane.terminal.lock() {
                        state.search_set_query(&pane.search_query);
                    }
                }
            }
            TerminalMessage::TerminalSearchStep(forward) => {
                if let Some(idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(idx)
                    && let Ok(mut state) = tab.active_mut().terminal.lock()
                {
                    state.search_step(forward);
                }
            }
            TerminalMessage::TerminalSearchClose => {
                if let Some(idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(idx)
                {
                    let pane = tab.active_mut();
                    pane.search_open = false;
                    if let Ok(mut state) = pane.terminal.lock() {
                        state.search_close();
                    }
                }
            }
            TerminalMessage::LoginScriptTick(pane_id, generation) => {
                return self.tick_login_script(pane_id, generation);
            }
            // ── Password-suggest popup (issue #117) ──
            m @ (TerminalMessage::PasswordSuggestNavigate(_)
            | TerminalMessage::PasswordSuggestPick(_)
            | TerminalMessage::PasswordSuggestDismiss
            | TerminalMessage::PasswordSuggestScrolled(_)) => {
                return self.handle_password_suggest(m);
            }
            // ── Broadcast input (C2) ──
            TerminalMessage::ToggleTabBroadcast(idx) => {
                if let Some(tab) = self.tabs.get_mut(idx) {
                    // Broadcast only exists across split panes: an unsplit
                    // tab refuses to arm and says why. The status segment
                    // and menu entry are hidden there, so this path is only
                    // reachable via the hotkey / command palette. Disarming
                    // stays unconditional so no state can ever get stuck.
                    if !tab.broadcast && !tab.broadcast_capable() {
                        self.set_toast(crate::i18n::t("broadcast_needs_split_hint").to_string());
                        return crate::shortcuts::toast_clear_after_secs(4);
                    }
                    tab.broadcast = !tab.broadcast;
                    if !tab.broadcast {
                        // Disarm: clear every opt-out so a later re-arm
                        // starts clean (all panes participate).
                        for pane in tab.pane_grid.panes.values_mut() {
                            pane.broadcast_opt_out = false;
                        }
                    }
                }
            }
            TerminalMessage::TogglePaneBroadcastOptOut(pane_id) => {
                if let Some(pane) = self
                    .tabs
                    .iter_mut()
                    .flat_map(|t| t.pane_grid.panes.values_mut())
                    .find(|p| p.id == pane_id)
                {
                    pane.broadcast_opt_out = !pane.broadcast_opt_out;
                }
            }
            // Periodic batched write of recorded output. Only mounted by
            // the subscription while at least one pane is recording.
            TerminalMessage::SessionLogFlushTick => {
                self.flush_session_logs();
            }
            // Right-click paste from the terminal widget. Mirrors the
            // Ctrl+Shift+V path below: SSH session if active, local PTY
            // otherwise. Without this, the widget's fallback write only
            // reached the local PTY and right-click looked broken on
            // every SSH tab.
            TerminalMessage::TerminalPasteFromClipboard => {
                // Also the terminal context-menu Paste row: dismiss the
                // menu (its item sits over the backdrop, so the backdrop
                // never sees the click). Idempotent for the other callers
                // (widget paste hook, middle-click, keyboard), which run
                // with no overlay open.
                self.overlay = None;
                return self.paste_clipboard_into_active();
            }
            // Clipboard text came back from the runtime (the only place
            // allowed to touch it). `None` = empty or unavailable.
            TerminalMessage::TerminalPasteResolved(tab_id, text) => {
                match text.map(crate::messages::Redacted::into_inner) {
                    Some(text) if !text.is_empty() => {
                        paste_trace("resolved", "clipboard text", &text, None);
                        self.paste_text_into_tab(tab_id, &text);
                    }
                    Some(_) => paste_trace("resolved", "clipboard empty", "", None),
                    None => paste_trace("resolved", "clipboard unavailable", "", None),
                }
            }
            TerminalMessage::ShowTerminalContextMenu(pane_id, x, y, selection) => {
                // Focus the right-clicked pane first (standard context-menu
                // behavior), so all rows act on the same pane: Copy All /
                // Clear Scrollback are pane-targeted by id, and Paste
                // routes through the focused pane.
                if let Some(tab_idx) = self.pane_tab_index(pane_id) {
                    self.active_tab = Some(tab_idx);
                    if let Some(tab) = self.tabs.get_mut(tab_idx)
                        && let Some(gp) = tab
                            .pane_grid
                            .panes
                            .iter()
                            .find(|(_, p)| p.id == pane_id)
                            .map(|(gp, _)| *gp)
                    {
                        tab.focused = gp;
                    }
                }
                // Right-click scheme = Menu: anchor the overlay at the
                // click point (window-absolute, same space as every menu).
                self.overlay = Some(crate::state::OverlayState {
                    content: crate::state::OverlayContent::TerminalContextMenu(pane_id, selection),
                    x,
                    y,
                });
            }
            TerminalMessage::TerminalCopySelection(text) => {
                self.overlay = None;
                if !text.is_empty() {
                    return crate::dispatch_global::write_clipboard_text(text);
                }
            }
            TerminalMessage::TerminalPasteSelection(pane_id, text) => {
                self.overlay = None;
                let text = text.into_inner();
                // Paste into the pane the gesture came from, not the
                // focused one: they agree for middle-click and the chord,
                // but the context menu can outlive a focus change.
                let Some(tab_id) = self.pane_tab_index(pane_id).and_then(|i| self.tabs.get(i)).map(|t| t._id)
                else {
                    paste_trace("selection", "pane has no tab", &text, None);
                    return Task::none();
                };
                // On X11 / Wayland the desktop owns PRIMARY, so ask it
                // first: the user may have highlighted in another window,
                // and answering from our own buffer would ignore that.
                // The widget publishes here even with nothing remembered
                // for exactly this case.
                if oryxis_terminal::has_primary_selection() {
                    paste_trace("selection", "primary read", &text, None);
                    return crate::dispatch_global::read_primary_text(move |primary| {
                        Message::Terminal(TerminalMessage::TerminalPasteSelectionResolved(
                            tab_id,
                            primary.map(Into::into),
                            text.clone().into(),
                        ))
                    });
                }
                if text.is_empty() {
                    paste_trace("selection", "nothing remembered", &text, None);
                    return Task::none();
                }
                paste_trace("selection", "remembered text", &text, None);
                // Deliberately does NOT touch the system clipboard: PRIMARY
                // is a separate buffer, and `copy_on_select` is the setting
                // for people who also want selections on the clipboard.
                // Pasting through the normal path keeps the careful-paste
                // and paste-guard gates.
                self.paste_text_into_tab(tab_id, &text);
            }
            TerminalMessage::TerminalPasteSelectionResolved(tab_id, primary, remembered) => {
                let primary = primary.map(crate::messages::Redacted::into_inner);
                let remembered = remembered.into_inner();

                match primary.filter(|text| !text.is_empty()) {
                    Some(text) => {
                        paste_trace("selection", "system primary", &text, None);
                        self.paste_text_into_tab(tab_id, &text)
                    }
                    None if !remembered.is_empty() => {
                        paste_trace("selection", "remembered text", &remembered, None);
                        self.paste_text_into_tab(tab_id, &remembered);
                    }
                    // Nothing anywhere: fall through to the clipboard, the
                    // long-standing behaviour of the gesture in a pane that
                    // was never selected in.
                    None => {
                        paste_trace("selection", "clipboard fallback", "", None);
                        return crate::dispatch_global::read_clipboard_text(move |text| {
                            Message::Terminal(TerminalMessage::TerminalPasteResolved(
                                tab_id,
                                text.map(Into::into),
                            ))
                        });
                    }
                }
            }
            TerminalMessage::TerminalDropFlush => {
                return self.handle_terminal_drop_flush();
            }
            TerminalMessage::TerminalDropProgress(pane_id, progress) => {
                return self.handle_terminal_drop_progress(pane_id, progress);
            }
            TerminalMessage::TerminalDropCancel(pane_id) => {
                // Cooperative: the upload task sees the flag on its next
                // progress tick, kills the in-flight write and sweeps the
                // partial file. The card clears when `Cancelled` arrives.
                if let Some(pane) = self.pane_by_id(pane_id)
                    && let Some(up) = pane.drop_upload.as_ref()
                {
                    up.abort.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
            TerminalMessage::TerminalCopyAll(pane_id) => {
                self.overlay = None;
                if let Some(pane) = self.pane_by_id(pane_id)
                    && let Ok(state) = pane.terminal.lock()
                {
                    let text = state.all_text();
                    drop(state);
                    if !text.is_empty() {
                        return crate::dispatch_global::write_clipboard_text(text);
                    }
                }
            }
            TerminalMessage::TerminalCopyScreen(pane_id) => {
                self.overlay = None;
                // The pane lock ends with the read, before the copy is
                // dispatched: the write resolves through the runtime and
                // comes back as a toast, so nothing downstream needs the
                // render lock, and nothing should be holding it.
                let text = self.pane_by_id(pane_id).and_then(|pane| {
                    let state = pane.terminal.lock().ok()?;
                    let text = state.visible_text();
                    (!text.is_empty()).then_some(text)
                });
                if let Some(text) = text {
                    return self.update(Message::CopyToClipboard(text));
                }
            }
            TerminalMessage::TerminalClearScrollback(pane_id) => {
                self.overlay = None;
                if let Some(pane) = self.pane_by_id(pane_id)
                    && let Ok(mut state) = pane.terminal.lock()
                {
                    state.clear_scrollback();
                }
            }
            // Careful-paste confirmation: release the parked multi-line
            // text into the session, or drop it.
            TerminalMessage::ConfirmPendingPaste => {
                if let Some((tab_id, text)) = self.pending_paste.take() {
                    match self.pending_paste_install.take() {
                        // A confirmed INSTALL script (issue #147) sends
                        // through the snippet injection, so Run's
                        // newline lands OUTSIDE the bracketed paste and
                        // actually executes, then lands in the host's
                        // install memory.
                        Some((snippet_id, run)) => {
                            if let Some(tab_idx) = self.tab_index_by_id(tab_id) {
                                self.inject_snippet_text_into(tab_idx, &text, run);
                                if run {
                                    self.record_install_run_for_tab(tab_idx, snippet_id);
                                }
                            }
                        }
                        None => self.write_paste_to_tab(tab_id, &text),
                    }
                }
            }
            TerminalMessage::CancelPendingPaste => {
                self.pending_paste = None;
                self.pending_paste_install = None;
            }
            // Synthesized input from the terminal widget: mouse-tracking
            // reports (tmux `mouse on`, vim `mouse=a`, htop, ...) and the
            // wheel-to-arrow translation in alt-screen. Same SSH-or-local
            // routing as keystrokes; without this the widget's local-PTY
            // fallback would never reach the remote session.
            TerminalMessage::TerminalInput(bytes) => {
                if let Some(tab_idx) = self.active_tab {
                    self.write_input_to_tab(tab_idx, &bytes);
                }
            }
            TerminalMessage::TerminalMouseCaptureHint => {
                // Mark the focused pane so HintMode::Once retires the hint
                // (harmless under Always, where the view ignores the flag).
                if let Some(tab_idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(tab_idx)
                {
                    tab.active_mut().mouse_hint_shown = true;
                }
                // Longer dwell than the default toast: this one is a sentence
                // to read, not a one-word "Copied" confirmation.
                return self.show_toast_secs(crate::i18n::t("mouse_capture_hint").to_string(), 5);
            }
            TerminalMessage::TerminalLinkClickHint => {
                // Plain click on a link without Ctrl: teach the gesture with
                // a toast at the moment it missed (replaces the old hover
                // tooltip). Mark the focused pane so HintMode::Once retires
                // it (harmless under Always, where the view ignores the flag).
                if let Some(tab_idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(tab_idx)
                {
                    tab.active_mut().link_hint_shown = true;
                }
                // Same longer dwell as the mouse-capture hint: a sentence.
                return self.show_toast_secs(crate::i18n::t("terminal_link_hint").to_string(), 5);
            }
            // Text committed by the OS IME (composed CJK characters, etc.),
            // delivered by the global subscription separately from
            // KeyboardEvent. Forward to the PTY under the same conditions as
            // a keystroke: no host editor panel or modal stealing focus, and
            // the cursor not over the chat sidebar. Deliberately does NOT
            // gate on active_view: in workspace mode a focused terminal runs
            // under the Dashboard view, not a dedicated Terminal view, so the
            // KeyboardEvent path doesn't check it either. When a text_input is
            // focused it handles its own Commit and inserts the text itself;
            // the host-panel / modal guards keep that from also hitting the
            // session.
            TerminalMessage::TerminalImeCommit(text) => {
                // Which gate a commit died on is the other half of the IME
                // evidence (the subscription traces delivery); lengths only,
                // never content.
                let trace_ime = crate::logging::is_enabled();
                let commit_len = text.chars().count();
                if text.is_empty() || self.panels.host_panel || self.any_modal_blocks_input() {
                    if trace_ime && !text.is_empty() {
                        tracing::debug!(
                            len = commit_len,
                            host_panel = self.panels.host_panel,
                            modal = self.any_modal_blocks_input(),
                            "ime-commit dropped by surface gate"
                        );
                    }
                    return Task::none();
                }
                // `cursor_over_sidebar` honors the dock side (issue #85)
                // and the side tab strip (issue #87); the old inline
                // right-edge math leaked IME commits into the PTY when
                // the sidebar was docked left.
                if self.cursor_over_sidebar() {
                    if trace_ime {
                        tracing::debug!(len = commit_len, "ime-commit dropped: cursor over sidebar");
                    }
                    return Task::none();
                }
                // Same per-tab scoping as the KeyboardEvent PTY gate: the
                // connect screen covers only its own tab, so a tab switched
                // away from an in-flight / failed connect keeps its IME
                // commits (the old app-global `connecting.is_none()` ate
                // them until the connecting tab was closed).
                if let Some(tab_idx) = self.active_tab
                    && !self
                        .connecting
                        .as_ref()
                        .is_some_and(|cp| Some(cp.tab_idx) == self.active_tab)
                {
                    let bytes = text.into_bytes();
                    self.write_input_to_tab(tab_idx, &bytes);
                    // The commit ends any active composition; winit usually
                    // sends an empty Preedit first, but clear defensively so
                    // a stale preedit can never linger on the overlay.
                    self.clear_focused_pane_preedit();
                    if trace_ime {
                        tracing::debug!(len = commit_len, tab = tab_idx, "ime-commit written to PTY");
                    }
                } else if trace_ime {
                    tracing::debug!(
                        len = commit_len,
                        no_tab = self.active_tab.is_none(),
                        "ime-commit dropped: connecting tab or no tab"
                    );
                }
            }
            // IME composition (preedit) update, e.g. pinyin syllables. Stored
            // on the focused pane's TerminalState; the `ime_host` widget
            // reports it to the iced runtime on the next redraw so the
            // over-the-spot overlay renders it at the caret. An empty string
            // clears it. Purely visual: nothing is written to the PTY until
            // the composition commits.
            TerminalMessage::TerminalImePreedit(text) => {
                // Same surface guards as `TerminalImeCommit`: IME events
                // reach this subscription even while a text_input owns the
                // composition (host panel field, modal, sidebar chat), and
                // without these the grid would paint someone else's
                // composition at the caret. An empty preedit (composition
                // ended or the IME closed) always lands, so a surface
                // opening mid-composition can never strand a ghost on the
                // grid.
                if !text.is_empty()
                    && (self.panels.host_panel
                        || self.any_modal_blocks_input()
                        || self.cursor_over_sidebar())
                {
                    return Task::none();
                }
                if let Some(tab_idx) = self.active_tab
                    && let Some(tab) = self.tabs.get(tab_idx)
                    && let Ok(mut state) = tab.active().terminal.lock()
                {
                    state.set_preedit(text);
                }
            }
        }
        Task::none()
    }

    /// Index of the tab whose grid contains the pane with `pane_id`.
    /// Used to route per-pane session events (connect / disconnect).
    pub(crate) fn pane_tab_index(&self, pane_id: uuid::Uuid) -> Option<usize> {
        self.tabs
            .iter()
            .position(|t| t.pane_grid.panes.values().any(|p| p.id == pane_id))
    }

    /// Clear the IME preedit of the focused pane of the active tab. Called
    /// when a composition commits so a stale overlay can never linger.
    fn clear_focused_pane_preedit(&mut self) {
        if let Some(tab_idx) = self.active_tab
            && let Some(tab) = self.tabs.get(tab_idx)
            && let Ok(mut state) = tab.active().terminal.lock()
        {
            state.set_preedit(String::new());
        }
    }

    /// Find a pane by its stable id across every tab (shared read).
    pub(crate) fn pane_by_id(&self, pane_id: uuid::Uuid) -> Option<&crate::state::Pane> {
        self.tabs
            .iter()
            .flat_map(|t| t.pane_grid.panes.values())
            .find(|p| p.id == pane_id)
    }

    /// Find a pane by its stable id across every tab (mutable).
    pub(crate) fn pane_by_id_mut(&mut self, pane_id: uuid::Uuid) -> Option<&mut crate::state::Pane> {
        self.tabs
            .iter_mut()
            .flat_map(|t| t.pane_grid.panes.values_mut())
            .find(|p| p.id == pane_id)
    }
}
