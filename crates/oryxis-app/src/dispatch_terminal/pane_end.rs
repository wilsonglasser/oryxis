//! What a pane does when its session ends (issue #208).
//!
//! A lone REMOTE pane already has an answer: `dispatch_ssh::session`
//! relabels its tab "(disconnected)" and the auto-reconnect sweep picks
//! it up. That answer is tab-wide, and works precisely because such a
//! tab has no siblings to endanger. It cannot serve a split tab, where
//! `ReconnectTab` would rebuild the whole thing and take the dead pane's
//! LIVE siblings with it, which is why the disconnect arm used to bail
//! out early for a multi-pane tab, leaving `[disconnected]` in the grid
//! and nothing else. And it never covered a local shell at all.
//!
//! So the verdict lives on the pane instead. The pane keeps whatever it
//! was showing, records `ended`, and the grid draws a card offering the
//! two answers a tab-wide reconnect cannot give it: restart THIS pane in
//! place, or close THIS pane. Which of those happens automatically is
//! the `pane_end_action` setting.
//!
//! The restart is the pane-scoped twin of the in-place reconnect in
//! `dispatch_tabs::lifecycle`: same teardown, same re-key, same dim
//! marker, scoped to one pane instead of a one-pane tab.

use iced::Task;
use uuid::Uuid;

use crate::app::{Message, Oryxis, TerminalMessage};
use crate::util::PaneEndAction;

impl Oryxis {
    /// Arm a local PTY stream for `pane_id` and hand back the generation
    /// to tag its `LocalPaneEnded` with.
    ///
    /// Every local spawn goes through here so the exit of a PTY the pane
    /// has already replaced can be told apart from the exit of the one it
    /// is listening to. Returns 0 for a pane that no longer exists; no
    /// pane holds generation 0 after arming, so the message is inert.
    pub(crate) fn arm_local_stream(&mut self, pane_id: Uuid) -> u64 {
        let Some(tab_idx) = self.pane_tab_index(pane_id) else {
            return 0;
        };
        let Some(pane) = self.tabs[tab_idx].pane_by_id_mut(pane_id) else {
            return 0;
        };
        pane.local_generation += 1;
        pane.local_generation
    }

    /// Wire a freshly spawned local PTY into `pane_id`: its output
    /// stream, plus the child-exit signal that says the shell is gone.
    ///
    /// Every local spawn goes through here, so none can be the one that
    /// forgets. `exited` comes from `PtyHandle::take_child_exit` and must
    /// be taken before the `TerminalState` is wrapped for the pane; a
    /// `None` (already taken) simply means nobody is told, which is the
    /// old behaviour rather than a wrong one.
    pub(crate) fn local_pane_stream(
        &mut self,
        pane_id: Uuid,
        exited: Option<tokio::sync::oneshot::Receiver<()>>,
        rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    ) -> Task<Message> {
        let output = Task::stream(tokio_stream::wrappers::UnboundedReceiverStream::new(rx))
            .map(move |bytes| Message::Terminal(TerminalMessage::PtyOutput(pane_id, bytes)));
        let Some(exited) = exited else {
            tracing::debug!(
                target: "oryxis::pane_end",
                %pane_id, "local pane wired WITHOUT an exit signal",
            );
            return output;
        };
        let generation = self.arm_local_stream(pane_id);
        Task::batch([
            output,
            Task::perform(
                async move {
                    let _ = exited.await;
                },
                move |()| {
                    Message::Terminal(TerminalMessage::LocalPaneEnded(pane_id, generation))
                },
            ),
        ])
    }

    /// A pane's session ended. Decide what happens to the pane itself.
    ///
    /// Answers every pane it is CALLED for, and the gating lives in the
    /// callers rather than here. A lone remote pane keeps the existing
    /// relabel-and-reconnect behaviour, which is the better answer and
    /// is correct precisely because it has no siblings to endanger, so
    /// the remote arm simply does not call for one. Every local shell
    /// does, split or alone, whichever origin opened it.
    ///
    /// Callers on the remote path have already done the session teardown
    /// by the time they reach here; the local path has its own, since a
    /// PTY leaves no transport handle behind to close.
    pub(crate) fn note_pane_ended(&mut self, pane_id: Uuid) -> Task<Message> {
        let Some(tab_idx) = self.pane_tab_index(pane_id) else {
            return Task::none();
        };
        // No lone-pane guard here, and that is the point: the only
        // caller that can reach this with a single pane IS the local
        // one. The remote arm gates on `panes.len() > 1` before calling
        // (`dispatch_ssh::session`), because a lone remote pane already
        // has a better answer, the tab relabelling itself
        // "(disconnected)" and the auto-reconnect sweep picking it up,
        // and that answer is only safe because there are no siblings to
        // endanger.
        //
        // A local shell has never had any answer, and which ORIGIN
        // opened it says nothing about that: the picker mints
        // `PaneOrigin::Local`, but a saved Local host mints
        // `PaneOrigin::Host` and a quick-connect one `QuickHost`. Keying
        // the guard on the origin let the last two keep freezing on
        // their last frame, which is the very case issue #209 reports.
        // A new caller must do its own gating, the way the remote arm
        // does.

        // A pane already holding the verdict must not be re-marked: the
        // notice would be printed twice into the same grid.
        if self.tabs[tab_idx].pane_by_id(pane_id).is_some_and(|p| p.ended) {
            return Task::none();
        }
        if self.prefs.pane_end_action == PaneEndAction::Close {
            // Routed through `ClosePane` rather than closing here, so the
            // pane's log flush, monitor reset and sibling re-focus all
            // stay in the one place that already knows how to do them.
            return Task::done(Message::Terminal(TerminalMessage::ClosePane(Some(
                pane_id,
            ))));
        }
        if let Some(pane) = self.tabs[tab_idx].pane_by_id_mut(pane_id) {
            pane.ended = true;
            if let Ok(mut state) = pane.terminal.lock() {
                state.process(b"\r\n[disconnected]\r\n");
            }
        }
        Task::none()
    }

    /// Re-dial ONE pane in place, keeping its terminal and its
    /// scrollback. The pane-scoped counterpart of `ReconnectTab`.
    ///
    /// Mirrors the in-place branch of `handle_reconnect_tab`, including
    /// the re-key: the dead pane's stream task can still emit trailing
    /// messages for the id it was spawned with, and routing is by
    /// `Pane::id`, so a fresh id sends those to an id no pane holds
    /// instead of stacking a second session onto this terminal.
    pub(crate) fn restart_pane(&mut self, pane_id: Uuid) -> Task<Message> {
        // Dismiss the context menu when its "Restart pane" row fired
        // this (a no-op on the card's own button).
        self.overlay = None;
        let Some(tab_idx) = self.pane_tab_index(pane_id) else {
            return Task::none();
        };
        // A dial already in flight for this pane: a second one would
        // stack two sessions onto one terminal.
        if self.tabs[tab_idx].pane_by_id(pane_id).is_some_and(|p| p.connecting) {
            return Task::none();
        }
        let Some(target) = self.pane_restart_target(tab_idx, pane_id) else {
            return self.show_toast_secs(crate::i18n::t("reconnect_unsupported").to_string(), 4);
        };
        // Persist whatever this pane recorded before its session is torn
        // down, so the log tail is not truncated.
        self.flush_session_logs_final();
        let label = target.label();
        let new_pane_id = Uuid::new_v4();
        let ended_log = self.tabs[tab_idx].pane_by_id_mut(pane_id).and_then(|pane| {
            // Explicitly close a still-live session: dropping the pane's
            // Arc alone never tears it down, because the stream task
            // holds its own.
            if let Some(session) = pane.session.take() {
                session.close();
            }
            pane.id = new_pane_id;
            pane.ended = false;
            pane.connecting = true;
            if let Ok(mut state) = pane.terminal.lock() {
                // Dim marker, so the restart reads as a continuation of
                // this pane rather than a wipe. The scrollback above it
                // is left exactly as the user was reading it.
                state.process(
                    format!("\r\n\x1b[2m[reconnecting to {label}...]\x1b[0m\r\n").as_bytes(),
                );
            }
            pane.session_log_id.take()
        });
        if let Some(log_id) = ended_log
            && let Some(vault) = &self.vault
        {
            let _ = vault.end_session_log(&log_id);
        }
        // The re-key orphans the old id's tmux listing: the view reads
        // the NEW id, so without this the old entry leaks in the map and
        // the pane sits on the "reading" hint forever (issue #157).
        self.tmux_reset_pane(&pane_id);
        match target {
            PaneRestartTarget::Saved(conn_idx, _) => {
                self.spawn_ssh_for_pane(conn_idx, tab_idx, new_pane_id)
            }
            PaneRestartTarget::Quick(qid, _) => {
                self.spawn_ssh_for_pane_quick(qid, tab_idx, new_pane_id)
            }
            PaneRestartTarget::Local(spec) => {
                self.respawn_local_pane(tab_idx, new_pane_id, &spec)
            }
        }
    }

    /// What `pane_id` should be restarted as, resolved from its origin.
    /// `None` for a pane nothing can rebuild (a cloud transport, a
    /// deleted host, a pruned quick-connect entry).
    fn pane_restart_target(
        &self,
        tab_idx: usize,
        pane_id: Uuid,
    ) -> Option<PaneRestartTarget> {
        let pane = self.tabs.get(tab_idx)?.pane_by_id(pane_id)?;
        match &pane.origin {
            // The origin's stable id wins over the display label:
            // labels collide across hosts and can be edited.
            crate::state::PaneOrigin::Host(hid) => {
                let idx = self.connections.iter().position(|c| c.id == *hid)?;
                Some(PaneRestartTarget::Saved(
                    idx,
                    self.connections[idx].label.clone(),
                ))
            }
            crate::state::PaneOrigin::QuickHost(qid) => {
                let entry = self.quick_connects.get(qid)?;
                Some(PaneRestartTarget::Quick(*qid, entry.conn.label.clone()))
            }
            crate::state::PaneOrigin::Local(spec) => {
                Some(PaneRestartTarget::Local(spec.clone()))
            }
            // The field's DEFAULT, not a claim that the pane is remote:
            // a cloud pane lands here and has no in-place path.
            crate::state::PaneOrigin::Ephemeral => None,
        }
    }
}

/// What a restart should respawn into the pane.
enum PaneRestartTarget {
    /// A saved host, by index into `connections`.
    Saved(usize, String),
    /// A quick-connect entry, by id into `quick_connects`.
    Quick(Uuid, String),
    /// A local shell, respawned from the exact spec the pane recorded.
    Local(crate::state::LocalShellSpec),
}

impl PaneRestartTarget {
    /// The name the dim `[reconnecting to ...]` marker says.
    fn label(&self) -> String {
        match self {
            PaneRestartTarget::Saved(_, label) | PaneRestartTarget::Quick(_, label) => {
                label.clone()
            }
            PaneRestartTarget::Local(spec) => spec.label.clone(),
        }
    }
}
