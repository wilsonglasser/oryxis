//! Hybrid terminal+SFTP tab handlers (issue #61) split out of
//! `dispatch_tabs`: toggle Files mode, detach the SFTP session to a
//! standalone tab, close just the SFTP session, and open a terminal
//! for a standalone SFTP tab's host. Called from `handle_tabs`.

#![allow(clippy::result_large_err)]

use iced::Task;

use crate::app::{TabsMessage, SshMessage, Message, Oryxis, SftpMessage};

impl Oryxis {
    /// Open a standalone SFTP tab against the host the focused pane of
    /// `idx` belongs to.
    ///
    /// `None` when the pane has no saved connection behind it: an
    /// ad-hoc session has nothing to mount by, and offering a tab that
    /// asks for a host would be answering a different question than the
    /// one the gesture asked.
    pub(super) fn sftp_tab_for_pane_host(&mut self, idx: usize) -> Option<Task<Message>> {
        let conn_id = match self.tabs.get(idx)?.sftp_source().origin {
            crate::state::PaneOrigin::Host(id) => id,
            _ => return None,
        };
        let position = self.connections.iter().position(|c| c.id == conn_id)?;
        Some(self.update(Message::Sftp(
            crate::messages::SftpMessage::OpenSftpForConnection(position),
        )))
    }

    pub(super) fn handle_toggle_tab_files_mode(&mut self, idx: usize) -> Task<Message> {
        // Fired from the tab context menu (among others):
        // dismiss it so it doesn't linger over the new surface.
        self.overlay = None;
        // Hybrid tab (issue #61): flip this SSH tab between its
        // terminal and its host's files (the full dual-pane SFTP
        // surface). The PTY keeps running underneath; the SFTP
        // state parks in `TerminalTab::files_state` when hidden.
        let Some(tab) = self.tabs.get(idx) else {
            return Task::none();
        };
        let tab_id = tab._id;
        // Clicking the glyph on a background tab also brings the
        // tab to front, whichever direction it flips.
        let select = if self.active_tab != Some(idx) {
            self.update(Message::Tabs(TabsMessage::SelectTab(idx)))
        } else {
            Task::none()
        };
        if self.tabs[idx].files_mode {
            // Back to the terminal: the browsing state goes home.
            // A stray one-shot directory hint dies here too.
            self.sftp_open_at_path = None;
            self.tabs[idx].files_mode = false;
            self.park_hybrid_sftp();
            return select;
        }
        // Turning ON requires the SFTP feature (optional, hidden
        // when off; this guards the hotkey path which bypasses
        // the gated UI). Turning OFF above is always allowed.
        if !self.sftp_enabled {
            self.sftp_open_at_path = None;
            return select;
        }
        // A session that survives roaming has no SSH to multiplex on:
        // the one that started it was let go because it would not have
        // survived either, and a Files surface inside this tab would
        // have stopped working the first time the address changed while
        // the shell kept going. So the request becomes a tab of its
        // own, against the same host, where the connection is visibly
        // separate and can die without taking the session with it.
        if self.tabs[idx].sftp_source().session.as_ref().is_some_and(|s| s.survives_roaming()) {
            self.sftp_open_at_path = None;
            return match self.sftp_tab_for_pane_host(idx) {
                Some(task) => Task::batch(vec![select, task]),
                None => select,
            };
        }
        // Files mode needs a live SSH session (SFTP is an SSH
        // subsystem; local / Telnet / serial tabs never show the
        // glyph, this guards the hotkey path). Read off the shell
        // pane: the console's own transport has no `ssh()` to give,
        // which is exactly what keeps its handover from re-entering
        // itself, and would read here as "this tab has no session".
        let Some(session) = self.tabs[idx]
            .sftp_source()
            .session
            .as_ref()
            .and_then(|s| s.ssh())
            .cloned()
        else {
            self.sftp_open_at_path = None;
            // No session YET, rather than never: the `select` above
            // reopened a dormant pinned tab, or one was already dialling.
            // Dropping the request here is the same defect as the SFTP
            // tab opening a second tab instead of becoming the pair
            // (owner, 2026-08-07): the gesture reconnects and quietly
            // ignores the half that was actually asked for. Remember it
            // and let `SshConnected` finish the job.
            if self.connecting.as_ref().is_some_and(|c| c.tab_idx == idx) {
                self.pending_files_mode = Some(self.tabs[idx]._id);
            }
            return select;
        };
        // Resolve by the focused SHELL pane (a split tab can host two
        // different servers; the tab label only names the first):
        // its label for the ad-hoc mount, its origin id for the
        // saved-connection lookup (immune to renames). `sftp_source`
        // is what makes "focused" mean the focused shell, so asking
        // for Files while standing in the SFTP console works instead
        // of declining on a tab that plainly has a session.
        let base = self.tabs[idx]
            .sftp_source()
            .label
            .trim_end_matches(" (disconnected)")
            .to_string();
        let origin_conn = match &self.tabs[idx].sftp_source().origin {
            crate::state::PaneOrigin::Host(hid) => Some(*hid),
            _ => None,
        };
        self.tabs[idx].files_mode = true;
        self.hoist_hybrid_sftp(tab_id);
        // Already mounted from an earlier visit: just show it,
        // navigating to the one-shot directory hint when an
        // expand/context-menu affordance carried one. Only a mount
        // whose session is still alive qualifies; a dead one (the tab
        // reconnected while Files was parked and the automatic remount
        // didn't land, issue #63) falls through to the mount pipeline
        // below, which reuses this tab's fresh session.
        if self.sftp.right.is_remote && self.sftp.right.host_label.is_some() {
            if self
                .sftp
                .right
                .session
                .as_ref()
                .is_some_and(|s| s.is_alive())
            {
                if let Some(p) = self.sftp_open_at_path.take() {
                    let nav = self.update(Message::Sftp(SftpMessage::SftpNavigateRemote(
                        crate::state::SftpPaneSide::Right,
                        p,
                    )));
                    return Task::batch([select, nav]);
                }
                return select;
            }
            // Land the remount at the previous directory (home
            // fallback); an explicit pending hint keeps priority.
            if self.sftp_open_at_path.is_none() {
                self.sftp_open_at_path = Some(self.sftp.right.remote_path.clone())
                    .filter(|p| !p.is_empty());
            }
        }
        // First open: seed the Local pane like a fresh SFTP tab,
        // then mount the host into the right pane.
        if self.sftp.left.local_path.as_os_str().is_empty() {
            self.sftp.left.local_path = std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from("/"));
            self.sftp.left.columns = self.sftp_chrome.columns_template.clone();
            self.sftp.right.columns = self.sftp_chrome.columns_template.clone();
        }
        self.refresh_sftp_local(crate::state::SftpPaneSide::Left);
        // Saved host: the shared mount pipeline (reuse-or-connect)
        // finds this tab's live session by label and multiplexes
        // an SFTP channel on it, no second dial. Origin id wins
        // over the label match (rename-proof).
        if let Some(ci) = self
            .connections
            .iter()
            .position(|c| {
                origin_conn == Some(c.id)
                    && c.protocol
                        == oryxis_core::models::connection::ConnectionProtocol::Ssh
            })
            .or_else(|| {
                self.connections.iter().position(|c| {
                    c.label == base
                        && c.protocol
                            == oryxis_core::models::connection::ConnectionProtocol::Ssh
                })
            })
        {
            let mount = self.update(Message::Sftp(SftpMessage::SftpRemountPane(
                crate::state::SftpPaneSide::Right,
                ci,
            )));
            return Task::batch([select, mount]);
        }
        // Ad-hoc host (quick connect / cloud): mount the live
        // session directly, mirroring OpenSftpForTab's fallback.
        {
            let pane = self.sftp.pane_mut(crate::state::SftpPaneSide::Right);
            pane.is_remote = true;
            pane.host_label = Some(base.clone());
            pane.remote_loading = true;
            pane.error = None;
            pane.remote_entries.clear();
        }
        let target = crate::state::SftpPaneSide::Right;
        let session_for_task = session.clone();
        let label = base;
        // One-shot directory hint from the expand affordances.
        let initial_hint = self.sftp_open_at_path.take();
        let mount = Task::perform(
            async move {
                let client = session_for_task
                    .open_sftp()
                    .await
                    .map_err(|e| e.to_string())?;
                let (initial, entries) =
                    crate::dispatch_sftp::initial_remote_listing(
                        &client,
                        initial_hint,
                    )
                    .await?;
                Ok::<_, String>((client, initial, entries))
            },
            // Completion stamped with THIS hybrid tab (hoisted just
            // above): a park/hoist swap while the mount is in
            // flight must not land the result in whichever buffer
            // is live by then. `route_sftp_async` swaps the owner's
            // state back in, or drops the result if the tab closed.
            move |result| match result {
                Ok((client, path, entries)) => Message::sftp_owned(
                    Some(tab_id),
                    SftpMessage::HostMounted(
                        target,
                        label.clone(),
                        session.clone(),
                        client,
                        path,
                        entries,
                    ),
                ),
                Err(e) => Message::sftp_owned(
                    Some(tab_id),
                    SftpMessage::RemoteError(target, e),
                ),
            },
        );
        Task::batch([select, mount])
    }

    pub(super) fn handle_detach_tab_sftp(&mut self, idx: usize) -> Task<Message> {
        // Promote the tab's SFTP session to a standalone SFTP tab
        // (the dual-remote / server-to-server surface). The hybrid
        // state moves out wholesale: live channel, panes, log,
        // any in-flight transfer keeps running under the new
        // owner id via route_sftp_async.
        self.overlay = None;
        let Some(tab) = self.tabs.get(idx) else {
            return Task::none();
        };
        let tab_id = tab._id;
        if !self.tab_has_sftp_session(tab) {
            return Task::none();
        }
        // An in-flight transfer's continuations are stamped with
        // THIS tab's id; moving the state under a new SftpTab id
        // would orphan them mid-run. Decline until it finishes.
        {
            let st: &crate::state::SftpState =
                if self.hybrid_sftp_owner == Some(tab_id) {
                    &self.sftp
                } else {
                    &tab.files_state
                };
            if st.transfer.state.is_some() {
                self.set_toast(
                    crate::i18n::t("tab_detach_sftp_busy").to_string(),
                );
                return Task::perform(
                    async {
                        tokio::time::sleep(std::time::Duration::from_millis(
                            2500,
                        ))
                        .await;
                    },
                    |_| Message::ToastClear,
                );
            }
        }
        // The state must be home (parked) before it can move.
        if self.hybrid_sftp_owner == Some(tab_id) {
            self.park_hybrid_sftp();
        }
        let Some(tab) = self.tabs.get_mut(idx) else {
            return Task::none();
        };
        tab.files_mode = false;
        // The SFTP half is leaving, so an inherited SFTP pin stops being
        // true of this tab: it is a plain terminal tab again, and the
        // detached browser is a brand new unpinned SFTP tab. Keeping the
        // spec would persist an SFTP pin for a tab with no SFTP in it.
        tab.inherited_pin = None;
        let state = std::mem::take(&mut *tab.files_state);
        let label = state
            .right
            .host_label
            .clone()
            .or_else(|| state.left.host_label.clone())
            .unwrap_or_else(|| crate::i18n::t("sftp").to_string());
        let mut stab = crate::state::SftpTab::new(label);
        stab.state = state;
        let sid = stab.id;
        self.sftp_tabs.push(stab);
        self.tab_order.push(crate::state::TabRef::Sftp(sid));
        let new_idx = self.sftp_tabs.len() - 1;
        self.focus_sftp_tab(new_idx);
        self.active_tab = None;
        self.active_view = crate::state::View::Sftp;
        Task::none()
    }

    pub(super) fn handle_close_tab_sftp_session(&mut self, idx: usize) -> Task<Message> {
        // Close ONLY the hybrid tab's SFTP session: drop the
        // browsing state + channel, back to a plain terminal
        // tab (the mode glyph disappears with the session). The
        // terminal keeps running untouched.
        self.overlay = None;
        let Some(tab) = self.tabs.get(idx) else {
            return Task::none();
        };
        let tab_id = tab._id;
        if !self.tab_has_sftp_session(tab) {
            return Task::none();
        }
        // An in-flight transfer would be killed by dropping the
        // state mid-run, and its continuations are stamped with
        // this still-existing tab id (so they would land on the
        // freshly-reset state); decline until it finishes (same
        // guard as the detach path).
        {
            let st: &crate::state::SftpState =
                if self.hybrid_sftp_owner == Some(tab_id) {
                    &self.sftp
                } else {
                    &tab.files_state
                };
            if st.transfer.state.is_some() {
                self.set_toast(crate::i18n::t("tab_detach_sftp_busy").to_string());
                return Task::perform(
                    async {
                        tokio::time::sleep(std::time::Duration::from_millis(
                            2500,
                        ))
                        .await;
                    },
                    |_| Message::ToastClear,
                );
            }
            // Unsaved work beyond the transfer (a dirty external
            // edit whose upload hasn't landed): route through the
            // same confirm modal as the standalone tab close
            // instead of silently discarding the pending save.
            if crate::sftp_methods::sftp_state_has_unsaved(st) {
                self.pending_sftp_close = Some(
                    crate::state::PendingSftpClose::HybridSession(tab_id),
                );
                return Task::none();
            }
        }
        self.close_tab_sftp_session(tab_id)
    }

    pub(super) fn handle_open_terminal_for_sftp_tab(&mut self, idx: usize) -> Task<Message> {
        // From an SFTP tab's menu: the way back to a shell on the mounted
        // host, and the tab JOINS that shell instead of leaving itself
        // behind (H5). The reverse gesture already turns a terminal tab
        // into the pair ("Open SFTP session"), and the owner expects the
        // symmetry.
        //
        // "The same tab" cannot be the same object: the pair is a TERMINAL
        // tab that can show Files (it owns the session and the pane grid),
        // and a standalone SFTP tab has neither. So the SFTP tab is
        // REPLACED by a terminal tab carrying its state as `files_state`,
        // in the same strip slot. From the user's side that IS the same
        // tab: Ctrl+Shift+F returns to the browser they had.
        self.overlay = None;
        // Through the one authority: reading `self.sftp` because
        // `active_sftp` points here is wrong whenever a terminal tab in
        // Files mode has hoisted the buffer, and it answers with THAT
        // tab's host instead of this one's. It also covers a dormant
        // pinned tab, whose panes have no label yet.
        let Some(host) = self.sftp_tab_terminal_host(idx) else {
            return Task::none();
        };
        // Can this tab travel with the gesture? One case where it cannot:
        // two panes on two DIFFERENT remote hosts is the server-to-server
        // surface, which CLAUDE.md keeps standalone on purpose, because a
        // morph would have to elect one of the hosts and then claim to be
        // "the pair" of a tab that owns half the surface. That one keeps
        // the old behaviour (go to a terminal, the SFTP tab stays).
        //
        // A DORMANT pinned tab DOES travel, owner report 2026-08-07: it
        // opened a new tab beside the pinned chip instead of becoming it.
        // Having no mount yet is not a reason to leave the tab behind, it
        // only means the pair starts with its Files half unmounted, which
        // "Open SFTP session" fills in on demand like any terminal tab.
        let (movable, busy) = match self.sftp_tab_state(idx) {
            Some(st) => (
                !(st.left.is_remote
                    && st.right.is_remote
                    && st.left.host_label != st.right.host_label),
                st.transfer.state.is_some(),
            ),
            None => (false, false),
        };
        // An in-flight transfer's continuations are stamped with THIS
        // tab's id, and `route_sftp_async` resolves that id against
        // `sftp_tabs`, the terminal tabs and the sidebar panes. Once the
        // SFTP tab is gone all three miss and the transfer dies silently,
        // so decline until it finishes: the same answer `DetachTabSftp`
        // and `CloseTabSftpSession` give, for the same reason.
        if movable && busy {
            self.set_toast(crate::i18n::t("tab_detach_sftp_busy").to_string());
            return Task::perform(
                async {
                    tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
                },
                |_| Message::ToastClear,
            );
        }
        // A live pane on that host wins (any pane, split included).
        // Prefer a tab that is not ALREADY browsing this host, so the
        // browser has somewhere to land.
        let live: Vec<usize> = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                t.pane_grid.panes.values().any(|p| {
                    p.label.trim_end_matches(" (disconnected)") == host
                        && p.session.as_ref().and_then(|s| s.ssh()).is_some()
                })
            })
            .map(|(i, _)| i)
            .collect();
        if let Some(&first) = live.first() {
            let free = live
                .iter()
                .copied()
                .find(|&i| !self.tab_has_sftp_session(&self.tabs[i]));
            return match free.filter(|_| movable) {
                // The destination already had a slot the user arranged, so
                // it keeps it: only the absorbed tab's entry goes away.
                Some(dest) => self.morph_sftp_tab_into(idx, dest, false),
                // Every candidate already browses this host: the pair the
                // user is asking for exists, so going there IS the answer
                // and the standalone tab stays (nothing is discarded).
                None => self.update(Message::Tabs(TabsMessage::SelectTab(first))),
            };
        }
        let Some(ci) = self.connections.iter().position(|c| {
            c.label == host
                && c.protocol
                    == oryxis_core::models::connection::ConnectionProtocol::Ssh
        }) else {
            return Task::none();
        };
        // Nothing live: connect, and morph into the tab the connect
        // appends SYNCHRONOUSLY (`start_ssh_tab` pushes before returning
        // the dial task; `reopen_dormant_tab` rides the same len-check).
        let before = self.tabs.len();
        let task = self.update(Message::Ssh(SshMessage::ConnectSsh(ci)));
        // No tab appeared, so there is nothing to morph into: an
        // SSM-transport host short-circuits, a terminal can fail to
        // allocate, and an armed pending split routes the pick into an
        // existing pane. Today's behaviour, with the SFTP tab intact.
        if !movable || self.tabs.len() <= before {
            return task;
        }
        let dest = self.tabs.len() - 1;
        // Born from THIS gesture, so it inherits the SFTP tab's slot
        // rather than landing at the end of the strip.
        Task::batch([task, self.morph_sftp_tab_into(idx, dest, true)])
    }

    /// Move the SFTP tab at `idx` into terminal tab `dest` as its Files
    /// surface and close it.
    ///
    /// `born_here` says whether `dest` was created by this very gesture.
    /// Only then does it inherit the SFTP tab's strip slot: a terminal tab
    /// that already existed keeps the position the user gave it, because
    /// moving it would rewrite an arrangement for the same reason the pin
    /// rule exists.
    ///
    /// Every step is synchronous inside one `update`, and the order is
    /// load-bearing (see the comments below).
    fn morph_sftp_tab_into(
        &mut self,
        idx: usize,
        dest: usize,
        born_here: bool,
    ) -> Task<Message> {
        let Some(old_id) = self.sftp_tabs.get(idx).map(|t| t.id) else {
            return Task::none();
        };
        let Some(dest_id) = self.tabs.get(dest).map(|t| t._id) else {
            return Task::none();
        };
        // A pin remembers what it was pinned AS (owner): the morphed tab
        // persists the SFTP spec, so a relaunch restores the SFTP tab the
        // user arranged instead of silently rewriting it into a terminal.
        // That is what makes the gesture reversible enough to ship.
        let inherited = self.sftp_tabs[idx]
            .pinned
            .then(|| self.sftp_pin_spec(idx))
            .flatten();
        let custom = self.sftp_tabs[idx].custom_name.clone();
        // Take the LIVE state through the swap-on-focus invariant: reading
        // the vec hands back a stale parked copy whenever this tab is the
        // one hoisted into the buffer. Same predicate as `sftp_tab_state`.
        let state = if self.active_sftp == Some(idx) && self.hybrid_sftp_owner.is_none() {
            std::mem::take(&mut self.sftp)
        } else {
            std::mem::take(&mut self.sftp_tabs[idx].state)
        };
        {
            // A dormant tab brings no mount, so there is no Files surface
            // to show yet: the pair starts on its terminal and "Open SFTP
            // session" fills the other half in, exactly as it would on any
            // terminal tab. Showing Files mode over an empty state would
            // just be the host picker wearing the pair's clothes.
            let mounted =
                state.right.host_label.is_some() || state.left.host_label.is_some();
            let tab = &mut self.tabs[dest];
            *tab.files_state = state;
            tab.files_mode = mounted;
            // Inherit the pin, but never over one the destination already
            // has: a tab the user pinned in its own right keeps its own
            // spec. An unpinned destination simply carries the pin across,
            // so absorbing a pinned tab cannot silently lose the pin.
            if let Some(spec) = inherited.filter(|_| !tab.pinned) {
                tab.inherited_pin = Some(spec);
                tab.pinned = true;
            }
            // A renamed SFTP tab keeps its name: to the user this is the
            // same tab. Never over a name the destination already has.
            if tab.custom_name.is_none() {
                tab.custom_name = custom;
            }
        }
        // Selecting a Files-mode tab hoists its state into the live
        // buffer, which is the render invariant, so no manual hoist here.
        let select = self.update(Message::Tabs(TabsMessage::SelectTab(dest)));
        // BEFORE the close, which retains the SFTP ref out of `tab_order`:
        // afterwards there would be no slot left to inherit. A tab that
        // already had its own slot just lets the close drop the SFTP entry.
        if born_here {
            self.morph_tab_order_slot(old_id, dest_id);
        }
        // AFTER the select, so `close_sftp_tab` cannot decide the SFTP
        // surface was left empty and emit the ChangeView that would yank
        // the user off the tab they just landed on. Its state is already
        // an empty default, so nothing is discarded.
        //
        // Re-resolved by ID rather than reusing `idx`: the select above is
        // a full nested `update`, and an index captured before it would
        // silently close a DIFFERENT tab (discarding its state) if
        // anything in that path dropped an earlier `sftp_tabs` entry.
        if let Some(i) = self.sftp_tabs.iter().position(|t| t.id == old_id) {
            let _ = self.close_sftp_tab(i);
        }
        self.persist_pinned_tabs();
        select
    }
}
