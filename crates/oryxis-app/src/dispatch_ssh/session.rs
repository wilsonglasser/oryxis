//! A live session's lifecycle: wiring one up, learning what OS it
//! runs, and tearing it down.
//!
//! `SshConnected` is where a dial becomes a usable pane: attach the
//! session, forward resizes and query replies, send the startup
//! command, arm the recording. `SshDisconnected` has to undo
//! exactly that much without disturbing sibling panes.

use super::*;

impl Oryxis {
    pub(super) fn handle_ssh_session(&mut self, message: SshMessage) -> Task<Message> {
        match message {
            SshMessage::ReuseFailedDialFresh(pane_id) => {
                // The pooled connection could not carry another session
                // (a server at its `MaxSessions` cap, or a link that
                // died between the health check and the channel open).
                // Forget it and dial for real. Dropping the entry FIRST
                // is what makes this terminate: without it the retry
                // would find the same dead transport and bounce here
                // again. The key comes from the pending map (the exact
                // key the failed reuse looked up with, minted at dial
                // time), with a recompute as the fallback so the
                // termination guarantee never rests on the map alone.
                if let Some(key) = self
                    .pending_reuse_keys
                    .remove(&pane_id)
                    .or_else(|| self.reuse_key_for_pane(pane_id))
                {
                    self.ssh_transport_pool.remove(&key);
                }
                // The tab index is recomputed, never carried: a tab
                // closed while the reuse attempt was in flight shifts
                // every later index, and the session-log wiring in the
                // spawn indexes `self.tabs` directly.
                let Some(tab_idx) = self.pane_tab_index(pane_id) else {
                    return Task::none();
                };
                let Some(conn) = self.pane_origin_connection(pane_id).cloned() else {
                    return Task::none();
                };
                let quick_id = self
                    .tabs
                    .iter()
                    .find_map(|t| t.pane_grid.panes.values().find(|p| p.id == pane_id))
                    .and_then(|p| match p.origin {
                        crate::state::PaneOrigin::QuickHost(id) => Some(id),
                        _ => None,
                    });
                return self.spawn_ssh_for_pane_conn(conn, quick_id, tab_idx, pane_id);
            }
            SshMessage::SshConnected(pane_id, session) => {
                // A dial that outlived its pane: an in-place reconnect
                // re-keyed the pane (or its tab closed) while this
                // connect was still in flight, so no pane routes the
                // completion. Tear the fresh session down instead of
                // leaking it (it holds live engine tasks and any
                // per-connection port-forward listeners), and drop a
                // progress card still tracking this exact dial (its
                // tab is gone).
                if self.pane_tab_index(pane_id).is_none() {
                    session.close();
                    self.pending_reuse_keys.remove(&pane_id);
                    if self
                        .connecting
                        .as_ref()
                        .is_some_and(|c| c.pane_id == pane_id)
                    {
                        self.connecting = None;
                    }
                    return Task::none();
                }
                // A mosh host is an SSH host right up to here, and stops
                // being one on this line. The handover runs where every
                // dial path CONVERGES rather than at the three that mint
                // an SSH transport, so a fourth added later inherits it
                // and cannot be written without it. Re-entry is not a
                // risk: what comes back is a Mosh transport, whose
                // `ssh()` is None.
                if let Some(ssh) = session.ssh()
                    && let Some(options) = self.pane_mosh_options(pane_id)
                {
                    let ssh = std::sync::Arc::clone(ssh);
                    return self.begin_mosh_handover(pane_id, ssh, options);
                }
                // An SFTP console is an SSH host right up to here too,
                // and for the same reason it branches HERE rather than
                // at the dial sites: it wants the host key prompt, the
                // password prompt, the proxy consent and the expanded
                // jump chain exactly as they are, and a second dialler
                // of its own would be the second half of the connect
                // experience this convention exists to prevent.
                //
                // It sits after the mosh check on purpose. mosh CLOSES
                // the SSH session it was handed, so a host configured
                // for both would have its console opened on a link that
                // is about to be let go. Ordered this way the mosh
                // handover wins and the console is simply not offered on
                // that pane, which `transport.ssh()` returning None for
                // a Mosh transport already enforces at the menu.
                if let Some(ssh) = session.ssh()
                    && self.pane_purpose(pane_id) == crate::state::PanePurpose::SftpConsole
                {
                    let ssh = std::sync::Arc::clone(ssh);
                    return self.begin_sftp_console(pane_id, ssh);
                }
                // Terminfo fallback (issue #88): by the time the PTY is up
                // the progress card is gone, so the timeline log alone is
                // easy to miss; a toast tells the user why TERM differs
                // and points at the host's Terminal Type setting.
                if let Some(fb) = session.ssh().and_then(|s| s.term_fallback()) {
                    let msg = match fb.used.as_deref() {
                        Some(used) => crate::i18n::t("term_fallback_toast")
                            .replace("{requested}", &fb.requested)
                            .replace("{used}", used),
                        None => crate::i18n::t("term_missing_toast")
                            .replace("{requested}", &fb.requested),
                    };
                    // Returns Task::none(); the toast itself is state.
                    let _ = self.show_toast_secs(msg, 8);
                }
                // Park this connection in the reuse pool (F2) so the
                // next tab to the same host rides it instead of dialling
                // again. Registered HERE rather than inside the async
                // dial because the pool lives on `self`, and this is the
                // first point that holds both. The key is the one MINTED
                // AT DIAL TIME (pending map), never recomputed from the
                // live row: a host edited while its dial was in flight
                // would otherwise register the old endpoint's transport
                // under the new row's key. Re-registering after a reuse
                // is harmless: the key and the transport are the same,
                // so the insert is a no-op in effect.
                if let Some(key) = self.pending_reuse_keys.remove(&pane_id)
                    && let Some(ssh) = session.ssh()
                {
                    self.remember_transport(key, ssh);
                }
                let (detect_for, login_script_task) = match self.pane_tab_index(pane_id) {
                    Some(tab_idx) => self.wire_connected_pane(tab_idx, pane_id, &session),
                    None => (None, Task::none()),
                };
                // Clear progress, show terminal, but ONLY if this completion
                // is the connect the card is tracking. A split-pane or
                // background connect completing, or a stale completion from a
                // dial the user cancelled via "Edit host" (whose tab is
                // gone), must not wipe an unrelated Home connect's card.
                if self
                    .connecting
                    .as_ref()
                    .is_some_and(|c| c.pane_id == pane_id)
                {
                    self.connecting = None;
                }

                // A visible sidebar Files browser waiting on this session
                // (reconnect with the tab open) can mount now; without
                // this it would sit on the "Opening SFTP" placeholder
                // until the next pane/tab interaction. No-op otherwise.
                let files_sync = self.sidebar_files_sync();
                // Same idea for the tab's hybrid Files surface (visible
                // or parked): its mount died with the old session, so an
                // in-place reconnect remounts it on the fresh handle at
                // the same directory (issue #63). No-op when nothing was
                // mounted or the mount is still alive.
                let hybrid_sftp = match (self.pane_tab_index(pane_id), session.ssh()) {
                    (Some(t_idx), Some(ssh)) => {
                        let ssh = ssh.clone();
                        self.hybrid_sftp_remount_dead(t_idx, pane_id, &ssh)
                    }
                    _ => Task::none(),
                };
                // "Open SFTP session" asked for on a tab that had no
                // session yet (a dormant pinned tab the same click
                // reopened, or one mid-dial): honour it now that there is
                // one. One-shot, and matched by tab id, so a reconnect on
                // some other tab cannot inherit the request.
                let pending_files = match self.pending_files_mode {
                    Some(want) if self.pane_tab_index(pane_id).is_some_and(|i| {
                        self.tabs.get(i).is_some_and(|t| t._id == want)
                    }) =>
                    {
                        self.pending_files_mode = None;
                        self.pane_tab_index(pane_id)
                            .map(|i| {
                                Task::done(Message::Tabs(
                                    crate::app::TabsMessage::ToggleTabFilesMode(i),
                                ))
                            })
                            .unwrap_or_else(Task::none)
                    }
                    _ => Task::none(),
                };
                // A successful dial is the "network is back" signal the
                // port-forward retry ladder waits for after a local
                // outage (issue #144); without it a pending forward
                // sits out a backoff of up to 120 s that the host tabs
                // themselves never pay. A serial line says nothing
                // about the network, so it does not kick.
                let pf_kick = match &session {
                    crate::state::TerminalTransport::Serial(_) => Task::none(),
                    _ => self.pf_kick_pending_retries(),
                };
                // A visible tmux tab is reading THIS pane's host: list
                // it on the fresh transport (issue #157, the in-place
                // reconnect re-keys the pane and drops the old
                // listing). No-op with the tab hidden.
                let tmux_sync = self.tmux_sync();
                if let Some((conn_id, sess)) = detect_for {
                    return Task::batch([
                        files_sync,
                        hybrid_sftp,
                        pending_files,
                        login_script_task,
                        pf_kick,
                        tmux_sync,
                        Task::perform(
                            async move { (conn_id, sess.detect_os().await) },
                            |(id, os)| Message::Ssh(SshMessage::OsDetected(id, os)),
                        ),
                    ]);
                }
                return Task::batch([
                    files_sync,
                    hybrid_sftp,
                    pending_files,
                    login_script_task,
                    pf_kick,
                    tmux_sync,
                ]);
            }
            SshMessage::OsDetected(conn_id, os) => {
                // Persist + update in-memory list so the icon refreshes.
                // Quick-connect hosts update in memory only (tab badge,
                // save-host prefill); nothing is written to the vault.
                if let Some(conn) = self.connections.iter_mut().find(|c| c.id == conn_id) {
                    conn.detected_os = os.clone();
                    if let Some(vault) = &self.vault {
                        let _ = vault.set_detected_os(&conn_id, os.as_deref());
                    }
                } else if let Some(entry) = self.quick_connects.get_mut(&conn_id) {
                    entry.conn.detected_os = os.clone();
                }
                tracing::info!("OS detected for {}: {:?}", conn_id, os);
            }
            SshMessage::SshDisconnected(pane_id) => {
                // A disconnect for a pane whose transport is ALIVE is a
                // notification from a session this pane no longer has.
                // The mosh handover makes that ordinary rather than
                // exotic: the SSH stream that dialled the pane is still
                // running when the handover closes the SSH session it
                // started, so it reports the death of a connection the
                // pane replaced a moment earlier. Marking the pane
                // disconnected there is wrong twice over, because the
                // session it names is working and the one that died was
                // meant to.
                //
                // The test is the pane's own transport rather than which
                // session sent this, because a stale disconnect looks
                // the same however it arose: an in-place reconnect that
                // landed while the old session was still tearing down
                // reaches here too.
                //
                // What makes that test SAFE is an invariant every
                // transport now upholds: it reads as dead BEFORE its
                // output stream ends. The stream ending is what produces
                // this message, so a session that really died can never
                // still answer `is_alive()` here, and only a genuinely
                // superseded one can. Each reader stores its own death
                // flag before dropping the output sender, in the same
                // task with no await between (`reader_done` on the SSH /
                // Telnet / Serial sessions, `alive` on mosh). Before
                // that, SSH leaned on `JoinHandle::is_finished` and
                // Telnet / Serial on a channel the WRITER task closes,
                // both of which settle a scheduling decision later than
                // the message travels, so a real disconnect could be
                // discarded here and the tab would read connected until
                // the 30 s liveness sweep caught it. Do not weaken any
                // of the four without moving this guard first.
                if self
                    .tabs
                    .iter()
                    .find_map(|t| t.pane_grid.panes.values().find(|p| p.id == pane_id))
                    .and_then(|p| p.session.as_ref())
                    .is_some_and(|s| s.is_alive())
                {
                    return Task::none();
                }
                // Persist whatever this pane recorded before we mark the
                // log ended; otherwise the tail of the session is lost.
                self.flush_session_logs_final();
                // Drop reuse entries whose connection is gone. The pool
                // holds `Weak`s, so a dead entry is harmless, but
                // pruning here keeps it from growing one per host per
                // app run.
                self.prune_transport_pool();
                if let Some(tab_idx) = self.pane_tab_index(pane_id) {
                    let label = self.tabs[tab_idx].label.replace(" (disconnected)", "");
                    // Monitor samples belong to the dead session: the
                    // counters the next rate would diff against are gone,
                    // so keeping them would make the first post-reconnect
                    // reading a fabrication spanning the outage.
                    let monitored_host = self.tabs[tab_idx]
                        .pane_grid
                        .panes
                        .values()
                        .find(|p| p.id == pane_id)
                        .and_then(|p| match p.origin {
                            crate::state::PaneOrigin::Host(id) => Some(id),
                            _ => None,
                        });
                    if let Some(id) = monitored_host {
                        self.monitor_reset_host(&id);
                    }
                    // Same reasoning for the tmux listing, one level
                    // finer: it belongs to the PANE whose transport just
                    // died, and offering attaches over a dead session
                    // would be a list of buttons that cannot work.
                    self.tmux_reset_pane(&pane_id);
                    // Clear the disconnected pane's session + end its log.
                    let log_id = self.tabs[tab_idx].pane_by_id_mut(pane_id).and_then(|p| {
                        // Close (not just drop) the dead session: SFTP
                        // mounts hold their own Arc clones, so dropping
                        // the pane's alone would leak the writer/quality
                        // tasks and keep `is_alive()` true on a session
                        // whose transport is gone (the reader exiting is
                        // what delivered this message). `close()` is
                        // idempotent and the polite disconnect it sends
                        // is a no-op on a dead transport.
                        if let Some(session) = p.session.take() {
                            session.close();
                        }
                        // A reconnect dial that ended in the stream
                        // closing (instead of `Connected`) is over too;
                        // re-arm ReconnectTab for this pane.
                        p.connecting = false;
                        // A transfer in flight loses its transport here.
                        // Dropping the `ZmodemPane` drops its `wire_tx`, so
                        // the driver's `wire_in` closes, it returns an error,
                        // and the pane resumes (typable) instead of being
                        // stranded as a dead sink behind a frozen card.
                        p.zmodem = None;
                        // Same fate for an OS-drop upload: its SFTP
                        // channel rode the session that just died, so the
                        // task errors out on its own; raising abort makes
                        // the race harmless and dropping the card now
                        // (instead of on the task's Failed event) keeps
                        // the reconnect UI clean. Staged-but-undetected
                        // drop sources are void with the shell that never
                        // ran their `rz`.
                        if let Some(up) = p.drop_upload.take() {
                            up.abort
                                .store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                        p.pending_drop_sources.clear();
                        // A dead transport voids any in-flight command
                        // timing: the reconnect prompt would otherwise
                        // "finish" it with a duration spanning the outage.
                        p.running_cmd = None;
                        p.last_submitted = None;
                        // The reconnected shell has to prove its own
                        // integration: keeping `seen` would leave this pane
                        // waiting for marks the new shell may never send.
                        p.inband = crate::state::InbandCapture::default();
                        // The sidebar Files channel died with the session;
                        // a reconnect remounts lazily (preferences kept).
                        p.files.reset_for_disconnect();
                        // The login script has no transport left to
                        // answer (issue #122), and the password prompt
                        // frozen on the dead grid is not waiting for
                        // anything any more (issue #117).
                        p.login_script = None;
                        p.password_prompt_sig = None;
                        // Trigger grants and cooldowns belong to the
                        // session they were given for (C6): permission
                        // to let REMOTE output type into this shell must
                        // not be inherited by whatever the reconnect
                        // lands on.
                        p.triggers.clear();
                        // The ambiguous-width answer was pinned to the
                        // mosh screen this session handed over to (J4).
                        // With that session gone, the pane goes back to
                        // reading the host's current setting on every
                        // batch, like every other pane.
                        p.mosh_ambiguous_width = None;
                        // The emulator's modes belong to the session too,
                        // and the program that armed them is not around
                        // to disarm them. Until they are cleared the dead
                        // pane still reports the mouse to nobody instead
                        // of selecting text, and the wheel still sends
                        // arrow keys instead of walking the scrollback,
                        // so the user cannot copy the output they
                        // disconnected on. The alternate screen is left
                        // alone on purpose: that frozen frame IS what
                        // they are reading.
                        if let Ok(mut state) = p.terminal.lock() {
                            state.process(oryxis_terminal::SESSION_MODE_RESET);
                        }
                        p.session_log_id
                    });
                    // Same for the suggestion popup, if it was this
                    // pane's: sending a credential to a dead session
                    // would silently drop it. Scoped to the pane, since
                    // a split tab's siblings are still live.
                    self.dismiss_password_suggest_for(pane_id);
                    // Same for a confirmation still on screen: it asks
                    // about a session that no longer exists.
                    self.reset_triggers_for_pane(pane_id);
                    if let Some(log_id) = log_id
                        && let Some(vault) = &self.vault
                    {
                        let _ = vault.end_session_log(&log_id);
                    }
                    if self.should_record_history()
                        && let Some(vault) = &self.vault {
                        let entry = oryxis_core::models::log_entry::LogEntry::new(
                            &label, &label, oryxis_core::models::log_entry::LogEvent::Disconnected, "Session ended",
                        );
                        let _ = vault.add_log(&entry);
                    }
                    // Refresh session logs list (count + current page)
                    if let Some(vault) = &self.vault {
                        self.session_logs_total =
                            vault.count_session_logs().unwrap_or(0);
                        self.session_logs = vault
                            .list_session_logs_page(self.session_logs_page * 50, 50)
                            .unwrap_or_default();
                    }
                    // The tab-level "(disconnected)" relabel + idle toast +
                    // auto-reconnect only make sense when the tab IS this one
                    // session. A split tab has live sibling panes, relabeling
                    // it would make `AutoReconnectTick` rebuild the whole tab
                    // (`ReconnectTab` removes it), nuking the siblings. So for
                    // a multi-pane tab we just note the disconnect inside the
                    // pane and leave the tab alone.
                    if self.tabs[tab_idx].pane_grid.panes.len() > 1 {
                        if let Some(pane) = self.tabs[tab_idx].pane_by_id_mut(pane_id)
                            && let Ok(mut state) = pane.terminal.lock()
                        {
                            state.process(b"\r\n[disconnected]\r\n");
                        }
                        return Task::none();
                    }
                    self.tabs[tab_idx].label = format!("{} (disconnected)", label);
                    // Surface the disconnect to the user. Without this the
                    // terminal just goes silent and the silent auto-reconnect
                    // (up to 30s later) feels like the shell mysteriously
                    // reset itself. A second toast fires from `ReconnectTab`
                    // when the actual reconnect attempt starts, so the
                    // wording here is intentionally past-tense only.
                    self.set_toast(crate::i18n::t("disconnected_idle").to_string());
                    return Task::perform(
                        async {
                            tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
                        },
                        |_| Message::ToastClear,
                    );
                }
            }
            // The router sends only this family here, so anything
            // else is a grouping mistake, not a runtime case.
            m => return crate::dispatch::unrouted(m),
        }
        Task::none()
    }

    /// Wire a freshly-dialed session into the pane that asked for it:
    /// attach the transport, forward viewport resizes and query replies,
    /// fire the startup command, inject the OSC 7 prompt hook, and log
    /// the connection.
    ///
    /// Returns the argument for a silent OS detection when this host
    /// qualifies for one (feature on, never detected, no icon override),
    /// paired with the login script's first timeout tick (`Task::none()`
    /// when the host has no automation),
    /// so the caller can batch that probe with its other follow-ups.
    fn wire_connected_pane(
        &mut self,
        tab_idx: usize,
        pane_id: Uuid,
        session: &crate::state::TerminalTransport,
    ) -> (Option<(Uuid, Arc<SshSession>)>, Task<Message>) {
                let label = self.tabs[tab_idx].label.clone();
                // Attach the session to the specific pane that connected
                // and forward future viewport resizes to the server so
                // remote `top`/`vim` re-layout instead of overflowing.
                if let Some(pane) = self.tabs[tab_idx].pane_by_id_mut(pane_id) {
                    pane.session = Some(session.clone());
                    // The reconnect dial resolved; re-arm ReconnectTab.
                    pane.connecting = false;
                    if let Ok(mut state) = pane.terminal.lock() {
                        // Whatever armed the emulator's modes died with
                        // the previous session, and the fresh shell never
                        // re-issues them, so clear the leftovers before
                        // its first output arrives. `SshDisconnected`
                        // already cleared most of them, but this is the
                        // one point EVERY session (ssh / telnet / serial,
                        // fresh, reconnected or split) goes through, so
                        // it stays the fail-safe rather than trusting a
                        // disconnect message that a stalled transport may
                        // never deliver. Alt screen first: leaving it
                        // puts the cursor back in the real buffer, which
                        // is where the region reset must land.
                        state.process(oryxis_terminal::LEAVE_ALT_SCREEN);
                        state.process(oryxis_terminal::SESSION_MODE_RESET);
                        // Serial has no viewport, so no resize sender;
                        // SSH/Telnet forward window changes to the peer.
                        if let Some(rtx) = session.resize_sender() {
                            state.set_remote_resize_sender(rtx);
                        }
                        // Query replies (cursor position, DECRQM, ...) must
                        // reach the remote: programs block waiting for them
                        // (issue #48, docker compose's raw-mode prompt).
                        state.set_remote_reply_sender(session.write_sender());
                        session.resize(state.cols(), state.rows());
                    }
                }
                // Startup command, fired as keystrokes right after the
                // session is wired. The SSH channel buffers input until
                // the shell is ready, so the line lands cleanly; the
                // newline triggers `Enter` on the remote.
                //
                // A session-group per-pane script (keyed by pane_id) wins
                // over the host's own `initial_command`. The fallback is
                // resolved via the pane's origin rather than the tab label
                // so it stays correct for group tabs (whose label is the
                // group name) and for two panes sharing one host.
                // A live snippet reference (its body, looked up now so
                // snippet edits propagate) wins over the literal
                // `initial_command`; a dangling snippet id resolves to
                // nothing, never an error.
                // The snippet reference can come from the host or, when
                // the host names none, from its group chain (D4). The
                // literal `initial_command` is host-only by design: a
                // group hands down a snippet, which stays editable in
                // one place, never a copy of a command.
                let (startup_snip, startup_lit) = self
                    .pane_origin_connection(pane_id)
                    .map(|c| {
                        let inherited = self
                            .vault
                            .as_ref()
                            .and_then(|v| v.resolve_effective(c, &self.groups).ok())
                            .and_then(|e| e.startup_snippet_id.map(|(id, _)| id));
                        (c.startup_snippet_id.or(inherited), c.initial_command.clone())
                    })
                    .unwrap_or((None, None));
                let fallback_cmd = match startup_snip {
                    Some(id) => self
                        .snippets
                        .iter()
                        .find(|s| s.id == id)
                        .map(|s| s.command.clone()),
                    None => startup_lit,
                };
                let initial = self
                    .pane_script_overrides
                    .remove(&pane_id)
                    .filter(|s| !s.trim().is_empty())
                    .or(fallback_cmd)
                    .filter(|s| !s.trim().is_empty());
                // A login script (issue #122) takes ownership of the
                // startup command: sending it now would type it into the
                // bastion's menu, and a menu-driven bastion drains
                // type-ahead it did not ask for. The runner re-sends it
                // once the script has reached the asset's shell.
                let (initial, login_script_task) = self.arm_login_script(pane_id, initial);
                if let Some(cmd) = initial {
                    let payload = format!("{cmd}\n");
                    if let Err(e) = session.write(payload.as_bytes()) {
                        tracing::warn!(
                            target = "oryxis::dispatch_ssh",
                            error = %e,
                            "failed to send startup command"
                        );
                    } else {
                        tracing::info!(
                            target = "oryxis::dispatch_ssh",
                            bytes = payload.len(),
                            "sent startup command after session ready"
                        );
                    }
                }
                tracing::info!("SSH connected: {}", label);
                if self.should_record_history()
                    && let Some(vault) = &self.vault {
                    let entry = oryxis_core::models::log_entry::LogEntry::new(
                        &label, &label, oryxis_core::models::log_entry::LogEvent::Connected, "Session established",
                    );
                    let _ = vault.add_log(&entry);
                }
                // Reset the auto-reconnect counter for this connection.
                // Quick-connect hosts resolve through the same label
                // lookup (saved hosts win a collision), so their
                // counters reset and OS detection covers them too.
                let connected = self.any_connection_by_label(&label).map(|conn| {
                    (
                        conn.id,
                        conn.custom_icon.is_some() || conn.custom_color.is_some(),
                        conn.detected_os.is_none(),
                    )
                });
                if let Some((conn_id, has_custom, os_unknown)) = connected {
                    self.reconnect_counters.remove(&conn_id);
                    // Queue silent OS detection only if:
                    //   - the feature is enabled,
                    //   - we haven't detected this host before (runs once),
                    //   - and the user hasn't set a custom icon override.
                    // OS detection execs over SSH; Telnet panes skip it
                    // (their icon stays the generic server glyph).
                    if self.prefs.os_detection
                        && os_unknown
                        && !has_custom
                        && let Some(ssh) = session.ssh()
                    {
                        return (Some((conn_id, ssh.clone())), login_script_task);
                    }
                }
        (None, login_script_task)
    }
}
