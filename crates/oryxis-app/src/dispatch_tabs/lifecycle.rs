//! Tab lifecycle handlers split out of `dispatch_tabs`: select,
//! close, reconnect, duplicate, plus the dormant-pin reopen path.
//! Called from `handle_tabs`.

#![allow(clippy::result_large_err)]

use iced::Task;
use oryxis_core::models::cloud::TransportKind;

use crate::app::{SettingsMessage, SshMessage, CloudMessage, Message, Oryxis, TabsMessage};
use crate::state::View;

impl Oryxis {
    pub(super) fn handle_select_tab(&mut self, idx: usize) -> Task<Message> {
        if idx < self.tabs.len() {
            // Lazy reopen: a dormant pinned tab (restored at boot) has
            // no live session; entering it the first time connects.
            if self.tabs[idx].pending_reopen.is_some() {
                return self.reopen_dormant_tab(idx);
            }
            // Switching tabs dismisses the session-group editor (it's
            // tied to the tab it was opened from).
            self.panels.session_group_panel = false;
            // Leaving a tab mid-composition clears its preedit so a stale
            // IME overlay can't re-show when the user tabs back.
            if let Some(old_idx) = self.active_tab
                && old_idx != idx
                && let Some(old) = self.tabs.get(old_idx)
                && let Ok(mut state) = old.active().terminal.lock()
            {
                state.set_preedit(String::new());
            }
            self.active_tab = Some(idx);
            // A hybrid tab in Files mode owns the live SFTP buffer
            // while shown (park/hoist, same invariant as the
            // standalone SFTP tabs).
            if self.tabs[idx].files_mode {
                let id = self.tabs[idx]._id;
                self.hoist_hybrid_sftp(id);
            }
            self.remember_terminal_tab_focus(idx);
            self.active_view = View::Terminal;
            // Viewing the tab consumes its smart-tab attention dot
            // (every pane: a split shows them all at once).
            for pane in self.tabs[idx].pane_grid.panes.values_mut() {
                pane.attention = None;
            }
            // The History tab is per-host; follow the tab switch.
            if self.sidebar_tab_shown(crate::state::TerminalSidebarTab::History) {
                self.refresh_command_history();
            }
            // The Files browser is per-pane; the landed tab's pane
            // may need a mount or a cwd catch-up (no-op otherwise).
            return Task::batch([
                self.tab_scroll_to_active(),
                self.sidebar_files_sync(),
            ]);
        }
        Task::none()
    }

    /// Tear down `self.tabs[idx]` and remove it: final session-log
    /// flush, active session close, chat-stream abort, stale-reference
    /// cleanup (placeholder pin, hybrid SFTP owner), monitor series
    /// reset, pinned-set persist, in-flight progress index adjustment
    /// and quick-connect pruning. Shared by `handle_close_tab` and the
    /// legacy rebuild path of `handle_reconnect_tab` so the two
    /// teardowns can never drift apart again (the rebuild path used to
    /// skip all of this and leak the live sessions). The caller is
    /// responsible for repointing `active_tab` afterwards (the two
    /// paths land on different tabs). `idx` must be in bounds.
    pub(super) fn teardown_tab_at(&mut self, idx: usize) {
        // Persist recorded output before the tab (and its
        // panes' buffers) are dropped.
        self.flush_session_logs_final();
        // Actively tear down the tab's remote sessions; the
        // connect streams hold their own Arcs, so dropping
        // the panes alone would leak the live sessions.
        Self::close_tab_sessions(&self.tabs[idx]);
        // Closing a tab that owns a live AI chat stream must
        // cancel it (per-tab now, so any tab, not just the
        // active one): otherwise the detached tool-followup
        // pipeline keeps polling a terminal that's being torn
        // down and keeps calling the model. The handle lives on
        // the tab (dropped with it), but abort first so the
        // detached task stops promptly rather than on next poll.
        self.abort_chat_task_for(self.tabs[idx]._id);
        // A pending placeholder replacement aimed at this tab
        // would otherwise go stale and hijack the next
        // unrelated cloud spawn.
        if self.pin_next_plugin_tab == Some(self.tabs[idx]._id) {
            self.pin_next_plugin_tab = None;
        }
        // A hybrid Files-mode owner dies with its tab: the
        // hoisted browsing state is discarded (any transfer on
        // it rode the session being torn down anyway).
        if self.hybrid_sftp_owner == Some(self.tabs[idx]._id) {
            self.hybrid_sftp_owner = None;
            self.sftp = crate::state::SftpState::default();
        }
        // Drop monitor series for hosts that lose their last live
        // pane with this tab. The async SshDisconnected lands after
        // the tab is gone and skips its reset (no pane resolves), so
        // without this the next session to the same host would diff
        // its first probe against the dead tab's counters and present
        // an average over the whole offline gap as a live reading.
        let closing_hosts: Vec<uuid::Uuid> = self.tabs[idx]
            .pane_grid
            .panes
            .values()
            .filter_map(|p| match p.origin {
                crate::state::PaneOrigin::Host(id) => Some(id),
                _ => None,
            })
            .collect();
        for host in closing_hosts {
            // "Last pane" is asked about the MACHINE (issue #156): the
            // window is shared by every row that points at it, so a
            // sibling tab still sitting on that server keeps it.
            if !self.monitor_machine_in_panes(&host, Some(idx)) {
                self.monitor_reset_host(&host);
            }
        }
        // Every pane in the tab takes its tmux listing with it. Keyed by
        // pane, so unlike the monitor series there is no "last live pane
        // of this host" question to ask.
        let closing_panes: Vec<uuid::Uuid> =
            self.tabs[idx].pane_grid.panes.values().map(|p| p.id).collect();
        for pane_id in closing_panes {
            self.tmux_reset_pane(&pane_id);
        }
        // Closing a pinned tab drops it from the persisted set.
        let was_pinned = self.tabs[idx].pinned;
        self.tabs.remove(idx);
        // Give back the local ports held by any callback tunnel the
        // tab's panes had open (each dropped `Arc` cancels its forward).
        self.prune_link_forwards();
        if was_pinned {
            self.persist_pinned_tabs();
        }
        // Keep the in-flight connection progress in sync with
        // the tab list. Closing the connecting tab clears the
        // progress (otherwise the stale screen, including a
        // failed/timeout state, leaks into the next session,
        // e.g. an ECS/SSM tab that doesn't set `connecting`).
        // Closing an earlier tab shifts the connecting tab's
        // index down by one so `SshRetry`/`SshCloseProgress`
        // still target the right `self.tabs[..]` entry.
        if let Some(ref mut progress) = self.connecting {
            match progress.tab_idx.cmp(&idx) {
                std::cmp::Ordering::Equal => self.connecting = None,
                std::cmp::Ordering::Greater => progress.tab_idx -= 1,
                std::cmp::Ordering::Less => {}
            }
        }
        self.adjust_last_terminal_tab_after_remove(idx);
        // Drop quick-connect entries (and their in-memory
        // credentials) that no pane references anymore.
        self.prune_quick_connects();
    }

    /// Close a tab, asking first when it is a group, or when it holds a
    /// live session and `confirm_close_session_tab` is on.
    ///
    /// A grouped tab is several live sessions behind one chip, and the
    /// close X is a small target sitting in the strip next to every
    /// other chip. Losing one session to a misplaced click is annoying;
    /// losing four at once is the report (#112).
    ///
    /// For a single-pane live tab the ask is an OPT-IN guard
    /// (Settings > Terminal): most SSH clients close straight through,
    /// so the default follows them, and users who would rather not
    /// drop a connection to a misplaced click turn it on. Dead tabs
    /// and local shells never prompt either way.
    ///
    /// The gate lives here rather than at the call sites so every close
    /// path is covered by construction: the strip's X, the tab context
    /// menu, Ctrl+W, and the terminal's own close handling all land
    /// here.
    pub(super) fn handle_close_tab(&mut self, idx: usize) -> Task<Message> {
        // The id is read here beside the pane count because the dialog's
        // action survives the modal being up, so it has to carry the tab
        // id rather than this index (see
        // `TabsMessage::ConfirmCloseGroupedTab`).
        let Some((panes, tab_id)) = self.tabs.get(idx).map(|t| (t.pane_count(), t._id)) else {
            return self.close_tab_now(idx);
        };
        if panes > 1 {
            self.overlay = None;
            self.error_dialog = Some(crate::state::ErrorDialog {
                title: crate::i18n::t("close_group_title").to_string(),
                body: crate::i18n::t("close_group_body")
                    .replacen("{n}", &panes.to_string(), 1),
                link: None,
                action: Some(crate::state::ErrorDialogAction {
                    label: crate::i18n::t("close_group_confirm").to_string(),
                    message: Box::new(Message::Tabs(TabsMessage::ConfirmCloseGroupedTab(tab_id))),
                    danger: true,
                }),
            });
            return Task::none();
        }
        if self.prefs.confirm_close_session_tab && self.tab_has_live_session(idx) {
            let Some(tab) = self.tabs.get(idx) else {
                return Task::none();
            };
            let tab_id = tab._id;
            let label = tab
                .label
                .trim_end_matches(" (disconnected)")
                .trim()
                .to_string();
            self.overlay = None;
            self.error_dialog = Some(crate::state::ErrorDialog {
                title: crate::i18n::t("close_session_title").to_string(),
                body: crate::i18n::t("close_session_body")
                    .replacen("{name}", &label, 1),
                link: None,
                action: Some(crate::state::ErrorDialogAction {
                    label: crate::i18n::t("close_session_confirm").to_string(),
                    message: Box::new(Message::Tabs(TabsMessage::ConfirmCloseLiveTab(tab_id))),
                    danger: true,
                }),
            });
            return Task::none();
        }
        self.close_tab_now(idx)
    }

    /// Whether closing the tab at `idx` would end a live remote session.
    ///
    /// Reads the same derived state the strip's status dot and the
    /// status bar's connection segment use, so "the X asked me first"
    /// and "the dot was green" can never disagree. Connecting /
    /// reconnecting dials count: cancelling a dial in flight is a live
    /// action too. A local shell, a dormant pin and a tab whose session
    /// already died are all safe to close without asking.
    pub(crate) fn tab_has_live_session(&self, idx: usize) -> bool {
        use crate::tab_conn_state::TabConnState;
        matches!(
            self.tab_conn_state(idx),
            TabConnState::Connecting
                | TabConnState::Reconnecting
                | TabConnState::Connected
                | TabConnState::NoContact
        ) || self
            .tabs
            .get(idx)
            .is_some_and(|t| t
                .pane_grid
                .panes
                .values()
                .any(|p| p.session.as_ref().is_some_and(|s| s.is_alive())))
    }

    /// How many of `idxs` would drop a live session.
    fn live_session_count(&self, idxs: &[usize]) -> usize {
        idxs.iter()
            .filter(|&&i| self.tab_has_live_session(i))
            .count()
    }

    /// How many open tabs currently hold a live session. The question
    /// behind the window-X and tray-Quit guards, which act on every
    /// tab at once.
    pub(crate) fn live_session_tab_count(&self) -> usize {
        (0..self.tabs.len())
            .filter(|&i| self.tab_has_live_session(i))
            .count()
    }

    /// The batch-close gate shared by "Close other tabs" and "Close all
    /// tabs": same opt-in as the single-tab guard (off by default, like
    /// most SSH clients), but it can only fire through the context menu
    /// rather than a stray X, so the ask names the count of live
    /// sessions about to be dropped, not one label.
    fn batch_close_confirm(
        &mut self,
        title_key: &str,
        body_key: &str,
        confirm_key: &str,
        live: usize,
        yes: Message,
    ) {
        self.overlay = None;
        self.error_dialog = Some(crate::state::ErrorDialog {
            title: crate::i18n::t(title_key).to_string(),
            body: crate::i18n::t(body_key).replacen("{n}", &live.to_string(), 1),
            link: None,
            action: Some(crate::state::ErrorDialogAction {
                label: crate::i18n::t(confirm_key).to_string(),
                message: Box::new(yes),
                danger: true,
            }),
        });
    }

    /// Close every tab except `idx` and the pinned ones, asking first
    /// when any closed tab holds a live session.
    pub(super) fn handle_close_other_tabs(&mut self, idx: usize) -> Task<Message> {
        self.overlay = None;
        if idx >= self.tabs.len() {
            return Task::none();
        }
        let target_id = self.tabs[idx]._id;
        let doomed: Vec<usize> = (0..self.tabs.len())
            .filter(|&i| self.tabs[i]._id != target_id && !self.tabs[i].pinned)
            .collect();
        let live = self.live_session_count(&doomed);
        if live > 0 && self.prefs.confirm_close_session_tab {
            self.batch_close_confirm(
                "close_others_title",
                "close_others_body",
                "close_others_confirm",
                live,
                Message::Tabs(TabsMessage::ConfirmCloseOtherTabs(target_id)),
            );
            return Task::none();
        }
        self.close_other_tabs_now(idx)
    }

    /// The "Close other tabs" close itself, with no prompt.
    pub(super) fn close_other_tabs_now(&mut self, idx: usize) -> Task<Message> {
        if idx < self.tabs.len() {
            // Keep the clicked tab and every pinned tab (pinned tabs
            // survive "close others", like a browser).
            let target_id = self.tabs[idx]._id;
            // Capture the connecting tab's id before filtering, so the
            // progress state can be re-anchored / dropped afterwards.
            let connecting_id = self
                .connecting
                .as_ref()
                .and_then(|p| self.tabs.get(p.tab_idx))
                .map(|t| t._id);
            // Tear each one down instead of dropping it: a bare
            // `retain` discards the struct while the connect stream
            // keeps its own Arc on the session, so the channel, the
            // engine tasks and the per-connection port forwards all
            // outlive the chip (see `close_tab_sessions`). Same reason
            // the recorded output has to be flushed and a live AI
            // stream aborted first: closing four tabs at once must cost
            // exactly what closing them one by one costs.
            // Reverse order so each index is still valid when its
            // turn comes.
            for i in (0..self.tabs.len()).rev() {
                if self.tabs[i]._id != target_id && !self.tabs[i].pinned {
                    // Each one lands on the reopen stack, exactly as if
                    // it had been closed on its own: a "close others"
                    // that drops a screenful is the case an undo is
                    // most wanted for.
                    self.remember_closed_tab(i);
                    self.teardown_tab_at(i);
                }
            }
            let new_active = self
                .tabs
                .iter()
                .position(|t| t._id == target_id)
                .unwrap_or(0);
            self.active_tab = Some(new_active);
            self.remember_terminal_tab_focus(new_active);
            self.reanchor_connecting_after_filter(connecting_id);
        }
        Task::none()
    }

    /// Close every unpinned tab, asking first when any closed tab holds
    /// a live session.
    pub(super) fn handle_close_all_tabs(&mut self) -> Task<Message> {
        self.overlay = None;
        let doomed: Vec<usize> = (0..self.tabs.len())
            .filter(|&i| !self.tabs[i].pinned)
            .collect();
        let live = self.live_session_count(&doomed);
        if live > 0 && self.prefs.confirm_close_session_tab {
            self.batch_close_confirm(
                "close_all_title",
                "close_all_body",
                "close_all_confirm",
                live,
                Message::Tabs(TabsMessage::ConfirmCloseAllTabs),
            );
            return Task::none();
        }
        self.close_all_tabs_now()
    }

    /// The "Close all tabs" close itself, with no prompt.
    pub(super) fn close_all_tabs_now(&mut self) -> Task<Message> {
        // Capture the connecting tab's id before filtering, so the
        // progress state can be re-anchored / dropped afterwards.
        let connecting_id = self
            .connecting
            .as_ref()
            .and_then(|p| self.tabs.get(p.tab_idx))
            .map(|t| t._id);
        // Pinned tabs survive "close all". Torn down one by one for the
        // reason in `close_other_tabs_now` above.
        for i in (0..self.tabs.len()).rev() {
            if !self.tabs[i].pinned {
                self.remember_closed_tab(i);
                self.teardown_tab_at(i);
            }
        }
        if self.tabs.is_empty() {
            self.active_tab = None;
            self.clear_terminal_tab_memory();
            self.active_view = View::Dashboard;
            self.connecting = None;
        } else {
            self.active_tab = Some(0);
            self.remember_terminal_tab_focus(0);
            self.reanchor_connecting_after_filter(connecting_id);
        }
        Task::none()
    }

    /// The close itself, with no prompt. Reached directly for a
    /// single-pane tab and from the grouped-tab confirmation.
    pub(super) fn close_tab_now(&mut self, idx: usize) -> Task<Message> {
        // Also dismiss any open context menu so the menu doesn't linger
        // after the user clicks Close from it.
        self.overlay = None;
        // Closing a tab dismisses the session-group editor it spawned.
        self.panels.session_group_panel = false;
        if idx < self.tabs.len() {
            // Before the teardown, which is also the reconnect rebuild's
            // path: the remembering lives here, on the user close, so a
            // reconnect cannot mint a "closed tab" for a session it is
            // putting straight back (issue #186).
            self.remember_closed_tab(idx);
            self.teardown_tab_at(idx);
            if self.tabs.is_empty() {
                self.active_tab = None;
                self.active_view = View::Dashboard;
            } else {
                let i = idx.min(self.tabs.len() - 1);
                // A dormant pinned tab (pinned, never opened, still
                // carries its reopen spec) is a placeholder, not a
                // real session to fall back onto: land on Home
                // instead of "focusing" an unopened pin.
                let fallback = &self.tabs[i];
                if fallback.pinned && fallback.pending_reopen.is_some() {
                    self.active_tab = None;
                    self.active_view = View::Dashboard;
                } else {
                    self.active_tab = Some(i);
                    self.remember_terminal_tab_focus(i);
                }
            }
        }
        Task::none()
    }

    pub(super) fn handle_reconnect_tab(&mut self, idx: usize) -> Task<Message> {
        self.overlay = None;
        // A dial already in flight for this tab makes Reconnect a
        // no-op: holding the hotkey chord (or an auto-reconnect tick
        // racing a manual click) must not stack a second dial. Each
        // in-place pass re-keys the pane, so every stacked dial's
        // completion would arrive for an id no pane holds and be
        // thrown away as an orphan.
        if let Some(tab) = self.tabs.get(idx) {
            let in_flight = self
                .connecting
                .as_ref()
                .is_some_and(|c| c.tab_idx == idx)
                || tab.pane_grid.panes.values().any(|p| p.connecting);
            if in_flight {
                return Task::none();
            }
        }
        // A SPLIT tab reconnects its FOCUSED PANE, never itself (issue
        // #208). Everything below this line is tab-wide: the in-place
        // branch refuses a multi-pane tab outright, so a split tab fell
        // through to the remove-and-rebuild fallback, which tore down
        // every live sibling to restart one dead pane. That made this
        // action the single most destructive gesture available on a
        // split tab, and it was reachable from the chord AND the tab
        // menu, which is why the guard lives here rather than at either
        // call site.
        //
        // Restarting a LIVE focused pane is not a special case: the
        // in-place branch below already treats a live pane as a "restart
        // this host", so a split tab now answers the same way an unsplit
        // one always has, one pane at a time.
        //
        // `AutoReconnectTick` never reaches this: it skips split tabs
        // explicitly, and a split tab is never relabeled "(disconnected)"
        // for it to find in the first place.
        if let Some(tab) = self.tabs.get(idx)
            && tab.pane_count() > 1
        {
            let pane_id = tab.active().id;
            return self.restart_pane(pane_id);
        }
        // Prefer an in-place reconnect that REUSES the pane's existing
        // terminal, so the scrollback the user was looking at survives
        // the round-trip instead of being wiped by a fresh tab. Only a
        // single-pane tab backed by a saved plain-SSH connection
        // qualifies: cloud transports (SSM / ECS / kubectl) need their
        // own PTY path, and a split tab's live sibling panes must not be
        // torn down. Everything else falls back to the legacy
        // remove-and-rebuild below.
        // What the in-place reconnect should respawn: a saved host
        // (by index) or a quick-connect entry (by id).
        enum ReuseTarget {
            Saved(usize),
            Quick(uuid::Uuid),
        }
        let reuse = self.tabs.get(idx).and_then(|tab| {
            if tab.pane_grid.panes.len() != 1 {
                return None;
            }
            // A live pane qualifies too (a "restart this host" from the
            // context menu on a still-connected tab). We close its old
            // session and re-key the pane below so the dying stream's
            // trailing messages don't stack a second session onto this
            // terminal; see the `new_pane_id` swap in the reuse branch.
            let base_label =
                tab.label.trim_end_matches(" (disconnected)").to_string();
            let pane_id = tab.active().id;
            // The pane origin is authoritative for ad-hoc hosts: they
            // have no row in `connections`, so resolve them straight
            // from the quick-connect store (always plain SSH).
            if let crate::state::PaneOrigin::QuickHost(qid) = &tab.active().origin
                && self.quick_connects.contains_key(qid)
            {
                return Some((ReuseTarget::Quick(*qid), pane_id, base_label));
            }
            // The pane origin's stable id wins over the display label
            // (labels collide across hosts and can be edited); the
            // label lookup only covers panes without a host uuid. A
            // dangling id (host deleted) resolves to nothing and rides
            // the legacy fallback below.
            let conn_idx = match &tab.active().origin {
                crate::state::PaneOrigin::Host(hid) => {
                    self.connections.iter().position(|c| c.id == *hid)?
                }
                _ => self.connections.iter().position(|c| c.label == base_label)?,
            };
            let plain_ssh = self.connections[conn_idx]
                .cloud_ref
                .as_ref()
                .is_none_or(|c| c.transport_pref == TransportKind::Ssh);
            plain_ssh.then_some((ReuseTarget::Saved(conn_idx), pane_id, base_label))
        });
        if let Some((target, pane_id, base_label)) = reuse {
            // Persist whatever this pane recorded before we tear its
            // session down, so the log tail isn't truncated.
            self.flush_session_logs_final();
            // Restore the live label (strip the "(disconnected)" suffix).
            // Keeping the tab in place means we never set
            // `self.connecting`, so the terminal (with its scrollback)
            // stays on screen through the reconnect instead of being
            // replaced by the full-screen progress view.
            self.tabs[idx].label = base_label.clone();
            // Re-key the pane before wiring the new session in. A live
            // pane's old stream task keeps emitting `PtyOutput` /
            // `SshDisconnected` for the id it was spawned with; routing
            // is by `Pane::id`, so a fresh id sends those trailing
            // messages to an id no pane holds (they get dropped) instead
            // of stacking a second session onto this terminal or
            // relabeling it "(disconnected)" mid-reconnect. `focused` is
            // a pane_grid handle, not the Uuid, so it survives the swap.
            let new_pane_id = uuid::Uuid::new_v4();
            let ended_log = self.tabs[idx].pane_by_id_mut(pane_id).and_then(|pane| {
                // Explicitly close a still-live session. Dropping the
                // pane's Arc alone never tears it down: the stream task
                // holds its own Arc, so without close() the engine
                // tasks, the SSH connection, and any per-connection
                // port-forward listeners leak (the "connection not
                // properly closed" the user saw).
                if let Some(session) = pane.session.take() {
                    session.close();
                }
                pane.id = new_pane_id;
                // Mark the dial in flight so a repeat Reconnect (held
                // chord, auto-tick race) is a no-op until this one
                // resolves (SshConnected / SshDisconnected /
                // PaneConnectError all clear it).
                pane.connecting = true;
                if let Ok(mut state) = pane.terminal.lock() {
                    // Dim marker so the reconnect reads as a continuation
                    // of the same pane, not a wipe. The scrollback above
                    // it is left untouched.
                    state.process(
                        format!(
                            "\r\n\x1b[2m[reconnecting to {base_label}...]\x1b[0m\r\n"
                        )
                        .as_bytes(),
                    );
                }
                // Hand back the old log id so it can be ended below: the
                // orphaned old-id stream won't reach the
                // `SshDisconnected` path that normally does it.
                pane.session_log_id.take()
            });
            if let Some(log_id) = ended_log
                && let Some(vault) = &self.vault
            {
                let _ = vault.end_session_log(&log_id);
            }
            // The re-key above orphans the old id's tmux listing: the
            // view reads the NEW id (no entry, so the tab sat on the
            // "reading" hint forever, issue #157) while the old entry
            // leaked in the map. Drop it like every other teardown
            // does; `SshConnected` re-lists a visible tab.
            self.tmux_reset_pane(&pane_id);
            // Toast "Reconnecting..." so the user sees feedback the
            // moment the attempt starts (a silent auto-reconnect can fire
            // up to 30s after the disconnect was first detected). Focus is
            // left alone: a manual reconnect is already on the active tab,
            // and a background auto-reconnect shouldn't yank the user away.
            self.set_toast(crate::i18n::t("disconnected_reconnecting").to_string());
            let spawn = match target {
                ReuseTarget::Saved(conn_idx) => {
                    self.spawn_ssh_for_pane(conn_idx, idx, new_pane_id)
                }
                ReuseTarget::Quick(qid) => {
                    self.spawn_ssh_for_pane_quick(qid, idx, new_pane_id)
                }
            };
            return Task::batch(vec![
                spawn,
                Task::perform(
                    async {
                        tokio::time::sleep(std::time::Duration::from_millis(2500))
                            .await;
                    },
                    |_| Message::ToastClear,
                ),
            ]);
        }
        // Legacy fallback (multi-pane, cloud transport, or a dead tab
        // with no live in-place path): remove the tab and rebuild via
        // ConnectSsh / QuickConnect / a local respawn / the tab's
        // stashed relaunch message. A tab nothing can rebuild is KEPT
        // (with a toast saying why), never silently destroyed.
        if let Some(tab) = self.tabs.get(idx) {
            let base_label = tab.label.trim_end_matches(" (disconnected)").to_string();
            // Same resolution rule as the in-place path above: the pane
            // origin's stable host id wins, the label lookup only
            // covers panes without one.
            let conn_idx = match &tab.active().origin {
                crate::state::PaneOrigin::Host(hid) => {
                    self.connections.iter().position(|c| c.id == *hid)
                }
                _ => self.connections.iter().position(|c| c.label == base_label),
            };
            // A quick-connect tab has no saved connection; rebuild it
            // from its stored entry instead of silently closing.
            let quick_entry = if conn_idx.is_none() {
                tab.pane_grid.panes.values().find_map(|p| match &p.origin {
                    crate::state::PaneOrigin::QuickHost(qid) => {
                        self.quick_connects.get(qid).cloned()
                    }
                    _ => None,
                })
            } else {
                None
            };
            // A local shell has no connection to look up, but "just
            // closed" is the wrong reading of Reconnect on it (worse now
            // that a chord can fire this): restart the same shell from
            // its pane origin. The spec AND the pane's last reported cwd
            // are captured now, because the respawn only runs after the
            // tab is removed and `active_tab` repointed, when a deferred
            // lookup would inherit a NEIGHBOR tab's directory.
            let local_respawn = if conn_idx.is_none() && quick_entry.is_none() {
                match &tab.active().origin {
                    crate::state::PaneOrigin::Local(spec) => {
                        Some((spec.clone(), tab.active().cwd.clone()))
                    }
                    _ => None,
                }
            } else {
                None
            };
            // What re-opens the tab once it's gone. Ephemeral cloud tabs
            // (ECS Exec / kubectl pod) rebuild via the relaunch message
            // stashed at spawn time, the same way Duplicate does.
            let rebuild = if let Some(ci) = conn_idx {
                Some(Message::Ssh(SshMessage::ConnectSsh(ci)))
            } else if let Some(entry) = quick_entry {
                Some(Message::Ssh(SshMessage::QuickConnect(Box::new(entry))))
            } else if local_respawn.is_none() {
                tab.relaunch.as_deref().cloned()
            } else {
                None
            };
            // Nothing can rebuild this tab (a session-group tab, a
            // deleted host, a pruned quick entry): keep it as it is and
            // say why nothing happened, instead of destroying a surface
            // the user may still be reading.
            if rebuild.is_none() && local_respawn.is_none() {
                return self.show_toast_secs(
                    crate::i18n::t("reconnect_unsupported").to_string(),
                    4,
                );
            }
            // Same teardown as CloseTab (shared helper), so the dying
            // tab's sessions, port-forward listeners, chat stream and
            // bookkeeping never leak or drift from the close path.
            self.teardown_tab_at(idx);
            if self.tabs.is_empty() {
                self.active_tab = None;
                self.active_view = View::Dashboard;
            } else {
                let i = idx.min(self.tabs.len() - 1);
                self.active_tab = Some(i);
                self.remember_terminal_tab_focus(i);
            }
            // Toast "Reconnecting..." so the user sees feedback the
            // moment the attempt actually starts (not when the
            // disconnect was first detected, up to 30s earlier).
            self.set_toast(crate::i18n::t("disconnected_reconnecting").to_string());
            let toast_clear = Task::perform(
                async {
                    tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
                },
                |_| Message::ToastClear,
            );
            // The local respawn spawns directly: the exact program the
            // pane ran (or the OS default shell when the spec's program
            // is empty), in the captured directory. Never the picker or
            // the "always open X" preference, since the tab WAS this
            // exact shell and the decision flow could otherwise pop a
            // picker over an already-destroyed tab.
            if let Some((spec, cwd)) = local_respawn {
                let pick = (!spec.program.is_empty()).then(|| {
                    (spec.program.clone(), spec.args.clone(), spec.label.clone())
                });
                let spawn = crate::dispatch_settings::spawn_local_shell_in(self, pick, cwd);
                return Task::batch(vec![spawn, toast_clear]);
            }
            if let Some(msg) = rebuild {
                return Task::batch(vec![Task::done(msg), toast_clear]);
            }
        }
        Task::none()
    }

    /// Arm the strip placement for the copy a Duplicate is about to
    /// spawn, per the `duplicate_tab_position` setting.
    ///
    /// Armed only when a tab is actually coming: a duplicate that spawns
    /// nothing would leave the request pending, and the next unrelated
    /// tab (minutes later, from anywhere) would take its slot. The TTL on
    /// [`crate::state::PendingTabPlacement`] is the backstop for the
    /// failures we cannot see coming from here (a cloud plugin that never
    /// starts, a PTY that refuses to spawn); this covers the one we can.
    fn arm_tab_placement(&mut self, source_id: uuid::Uuid) {
        use crate::state::{PendingTabPlacement, TabPlacement};
        let placement = TabPlacement::from_setting(&self.prefs.duplicate_tab_position);
        // Appending IS what an unarmed spawn does, so leave it unarmed
        // and there is nothing that can go stale.
        if placement == TabPlacement::End {
            return;
        }
        self.pending_tab_placement = Some(PendingTabPlacement {
            source_id,
            placement,
            armed_at: std::time::Instant::now(),
        });
    }

    pub(super) fn handle_duplicate_tab(&mut self, idx: usize) -> Task<Message> {
        self.overlay = None;
        // Local shell tabs aren't backed by a saved connection; for
        // those we just open a fresh shell tab. SSH tabs find their
        // connection by label and dispatch `ConnectSsh` so the user
        // gets a second live session into the same box. Cloud tabs
        // (ECS Exec / kubectl) re-open via the relaunch message
        // stashed on the tab at spawn time.
        if let Some(tab) = self.tabs.get(idx) {
            let source_id = tab._id;
            // Local shells are identified by their pane ORIGIN, not by
            // the label: only an unpicked default shell is ever labelled
            // "Local Shell", so a curated entry ("bash (default)",
            // "PowerShell", a WSL distro) used to miss this branch, miss
            // the connection-by-label lookup below too, and make
            // Duplicate a silent no-op on every configured terminal.
            //
            // The origin also carries the exact program + args, so the
            // copy respawns THAT shell instead of re-opening the picker,
            // the same resolution `reopen_dormant_tab` does for a pinned
            // `PinnedTabSpec::LocalShell`. An empty program means "the OS
            // default", which only `OpenLocalShell` can resolve.
            if let crate::state::PaneOrigin::Local(spec) = &tab.active().origin {
                let msg = if spec.program.is_empty() {
                    Message::Settings(SettingsMessage::OpenLocalShell)
                } else {
                    Message::Settings(SettingsMessage::OpenLocalShellWith {
                        program: spec.program.clone(),
                        args: spec.args.clone(),
                        label: spec.label.clone(),
                    })
                };
                self.arm_tab_placement(source_id);
                return Task::done(msg);
            }
            // Cloud tabs with no saved connection (ECS Exec,
            // kubectl pod) carry the message that re-opens them.
            if let Some(relaunch) = tab.relaunch.as_deref() {
                let msg = relaunch.clone();
                self.arm_tab_placement(source_id);
                return Task::done(msg);
            }
            // Connection-backed tabs (SSH, InstanceConnect, and
            // SSM-into-EC2) duplicate by re-finding the host by
            // label. SSM tabs carry a title prefix, strip it so
            // the lookup matches; ConnectSsh re-routes to SSM via
            // the cloud_ref transport check.
            let base_label = tab
                .label
                .trim_end_matches(" (disconnected)")
                .trim_start_matches(crate::app::SSM_TAB_PREFIX)
                .to_string();
            if let Some(ci) = self.connections.iter().position(|c| c.label == base_label) {
                // A remote-desktop host opens an OS client, never a tab
                // (`start_ssh_tab` punts to `launch_remote_desktop`), so
                // arming here would park the request on nothing.
                let spawns_tab = !matches!(
                    self.connections[ci].protocol,
                    oryxis_core::models::connection::ConnectionProtocol::RemoteDesktop
                );
                if spawns_tab {
                    self.arm_tab_placement(source_id);
                }
                return Task::done(Message::Ssh(SshMessage::ConnectSsh(ci)));
            }
        }
        Task::none()
    }

    /// The message that reopens `spec`, plus whether the tab it produces
    /// arrives ASYNCHRONOUSLY.
    ///
    /// Resolved fresh every time: a host id maps to a different index than
    /// it did last session, and the connection may have been deleted since
    /// (`None`, which every caller reads as "nothing to reopen"). Cloud
    /// sessions spawn through a plugin several updates later, so they
    /// cannot ride a synchronous "did a tab get appended" check, and the
    /// flag is what saves each caller from having to know which specs
    /// those are.
    ///
    /// One authority for both reopen paths: a dormant PIN selected for the
    /// first time, and a tab the user closed and asked back
    /// (`ReopenClosedTab`, issue #186).
    pub(super) fn spec_open_message(
        &self,
        spec: &crate::state::PinnedTabSpec,
    ) -> (Option<Message>, bool) {
        use crate::state::PinnedTabSpec;
        let mut cloud = false;
        let open = match spec {
            PinnedTabSpec::Host { id, .. } => self
                .connections
                .iter()
                .position(|c| c.id == *id)
                .map(|v| Message::Ssh(SshMessage::ConnectSsh(v))),
            PinnedTabSpec::LocalShell { program, args, label } => {
                Some(Message::Settings(SettingsMessage::OpenLocalShellWith {
                    program: program.clone(),
                    args: args.clone(),
                    label: label.clone(),
                }))
            }
            PinnedTabSpec::EcsExec {
                group_id,
                task_id,
                container,
                ..
            } => {
                cloud = true;
                // ECS task ids are ephemeral (services recycle tasks), so
                // a saved id is expected to go stale. Resolve the group
                // and connect to the task currently running; the saved id
                // only wins when it still exists.
                Some(Message::Cloud(CloudMessage::EcsExecConnectFreshTask {
                    group_id: *group_id,
                    container: container.clone(),
                    fallback_task_id: task_id.clone(),
                }))
            }
            PinnedTabSpec::KubectlExec {
                group_id,
                namespace,
                pod,
                container,
                ..
            } => {
                cloud = true;
                Some(Message::Cloud(CloudMessage::ConnectKubectlExecPod {
                    group_id: *group_id,
                    namespace: namespace.clone(),
                    pod: pod.clone(),
                    container: container.clone(),
                }))
            }
            // SFTP dormant tabs live in `sftp_tabs`, not `self.tabs`, and reopen
            // via `SelectSftpTab` (which re-mounts their panes), so this
            // terminal-tab reopen path never produces an open message for them.
            PinnedTabSpec::Sftp { .. } => None,
        };
        (open, cloud)
    }

    /// First select of a dormant pinned tab: drop the placeholder and fire
    /// the saved spec to reopen it (connect host / spawn local shell). The
    /// freshly-opened tab inherits the pin.
    fn reopen_dormant_tab(&mut self, idx: usize) -> Task<Message> {
        let Some(spec) = self
            .tabs
            .get_mut(idx)
            .and_then(|t| t.pending_reopen.take())
        else {
            return Task::none();
        };
        let (open, cloud) = self.spec_open_message(&spec);
        if cloud {
            // Cloud sessions spawn asynchronously. Keep the dormant
            // placeholder in the strip (so its chip doesn't blink out) and let
            // `spawn_plugin_tab` replace it in place by id, inheriting its slot
            // + pin. We don't persist here: the dormant spec stays in the
            // setting as a safety net until the live tab re-persists. Stay on
            // the placeholder pane with a connecting hint instead of bouncing
            // to Hosts while the session resolves + spawns.
            self.pin_next_plugin_tab = Some(self.tabs[idx]._id);
            self.active_tab = Some(idx);
            self.remember_terminal_tab_focus(idx);
            self.active_view = View::Terminal;
            if let Some(pane) = self.tabs[idx].pane_grid.panes.values().next()
                && let Ok(mut term) = pane.terminal.lock()
            {
                let hint = format!(
                    "\r\n\x1b[2m  {}\x1b[0m\r\n",
                    crate::i18n::t("connecting_status")
                );
                term.process(hint.as_bytes());
            }
            return open.map(|m| self.update(m)).unwrap_or_else(Task::none);
        }

        // Host / local: the connect appends a live tab synchronously, so
        // remove the placeholder and slot the live tab into its place.
        let dormant_id = self.tabs[idx]._id;
        // Where the placeholder sits in the strip, captured BEFORE the
        // remove. The reopen below is a nested `update`, and its
        // `reconcile_tab_order` drops this ref (its tab is gone by then),
        // so afterwards there is nothing left to rename and the reopened
        // tab would land at the end of its pin partition instead of the
        // slot the user arranged.
        let slot = self
            .tab_order
            .iter()
            .position(|r| matches!(r, crate::state::TabRef::Terminal(x) if *x == dormant_id));
        self.tabs.remove(idx);
        self.adjust_last_terminal_tab_after_remove(idx);

        let before = self.tabs.len();
        let task = open.map(|m| self.update(m)).unwrap_or_else(Task::none);
        if self.tabs.len() > before {
            // A live tab was appended at the end; move it back to the
            // dormant's old slot so reopening doesn't reorder, and pin it.
            let live = self.tabs.pop().expect("a tab was just appended");
            let at = idx.min(self.tabs.len());
            self.tabs.insert(at, live);
            self.tabs[at].pinned = true;
            // Keep the reopened tab at the dormant's spot in the unified strip
            // order (else reconcile would append the new id at the end).
            let live_id = self.tabs[at]._id;
            self.restore_tab_order_slot(dormant_id, live_id, slot);
            self.active_tab = Some(at);
            self.remember_terminal_tab_focus(at);
            self.active_view = View::Terminal;
            // ConnectSsh set `connecting.tab_idx` to the append index; the
            // move just shifted it, so retarget the progress overlay.
            if let Some(p) = &mut self.connecting
                && p.tab_idx == before
            {
                p.tab_idx = at;
            }
        } else if self.tabs.is_empty() {
            // Nothing reopened (e.g. the host was deleted) and no tabs left.
            self.active_tab = None;
            self.active_view = View::Dashboard;
        } else {
            // Nothing reopened but other tabs remain: clamp the selection so
            // `active_tab` never dangles past the removed placeholder.
            let i = idx.min(self.tabs.len() - 1);
            self.active_tab = Some(i);
            self.remember_terminal_tab_focus(i);
        }
        self.persist_pinned_tabs();
        Task::batch([task, self.tab_scroll_to_active()])
    }

}
