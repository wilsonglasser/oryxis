//! Turning a freshly connected SSH session into an SFTP console
//! (issue #188).
//!
//! Same shape and same reasoning as the mosh handover next door: this
//! runs at the point where EVERY dial path converges rather than at the
//! sites that mint an SSH transport, so a dial site added later inherits
//! it and cannot be written without it. Landing here also means the
//! whole SSH connect experience is reused as it stands, host keys and
//! password prompts and proxy consent and the expanded jump chain and
//! all: a console host IS an SSH host right up to this line.
//!
//! Unlike mosh, the SSH session is KEPT. The console's channel rides it,
//! and it is what answers whether the link is still there, so the
//! session lives exactly as long as the console does. It may also be a
//! link a terminal tab is holding (the reuse pool hands out the same
//! transport), which is why closing the console never closes it.

use std::sync::{Arc, Mutex};

use oryxis_ssh::SshSession;
use oryxis_ssh::sftp_shell::SftpShellSession;
use oryxis_terminal::widget::TerminalState;

use crate::app::{DEFAULT_TERM_COLS, DEFAULT_TERM_ROWS, Oryxis};
use crate::messages::{Message, SshMessage, TabsMessage, TerminalMessage};
use crate::state::{PanePurpose, TabSurface, TerminalTransport};

impl Oryxis {
    /// Open an SFTP console for `conn`, in a tab of its own.
    ///
    /// There is ONE path here whether or not a session to that host is
    /// already up, and that is the point. `start_ssh_tab` consults the
    /// reuse pool before it dials (F2), so an open terminal tab to the
    /// same host lends its connection and the console costs no handshake
    /// at all, while a cold open gets the full connect experience.
    /// Branching on "is there a session" would have meant writing the
    /// cold path twice.
    ///
    /// This is the door for a console asked for where there is no tab to
    /// put it in: a host card, the command palette on the dashboard. One
    /// opened ON a session takes `open_sftp_console_in_tab` instead and
    /// lands as a pane of that tab, because a console is a second view
    /// of the machine in front of the user, not a second session.
    pub(crate) fn open_sftp_console(
        &mut self,
        conn: oryxis_core::models::Connection,
        start_dir: Option<String>,
    ) -> iced::Task<Message> {
        // Checked again here even though every caller filters already,
        // because what is at stake is a flag that only the SSH dial path
        // consumes: setting it for a host that will not take that path
        // leaves it armed for whatever opens next.
        if !Self::host_can_console(&conn) {
            return iced::Task::none();
        }
        // Consumed by `start_ssh_tab` when it builds the pane, and by
        // `begin_sftp_console` when the dial lands. One-shot, like every
        // other hint of this shape in the app.
        self.pending_console_purpose = true;
        self.pending_console_dir = start_dir;
        let origin = crate::state::ProgressOrigin::Saved(conn.id);
        self.start_ssh_tab(conn, origin)
    }

    /// Open (or reveal) the SFTP console of the tab at `idx`, as a pane
    /// of that tab.
    ///
    /// The placement is the user's (`sftp_console_layout`), and all
    /// three options are panes: "Full" is the split, zoomed. That is
    /// what lets ONE control (the tab chip, the status-bar segments,
    /// this hotkey) move between shell and console whichever placement
    /// they chose, and it is why the console can be reached from Files
    /// mode at all.
    ///
    /// A tab gets at most ONE console: asking again reveals the one it
    /// has. Splitting a second would leave two channels on one link
    /// with nothing to tell them apart, and the toggle would have no
    /// answer to "which console".
    ///
    /// The purpose is written straight onto the pane here rather than
    /// through `pending_console_purpose`. That flag exists because
    /// `start_ssh_tab` builds its pane deep inside the dial; here the
    /// pane is in hand before anything is dialled, and a flag that
    /// outlived its request is the exact failure it is documented for.
    pub(crate) fn open_sftp_console_in_tab(
        &mut self,
        tab_idx: usize,
        conn: oryxis_core::models::Connection,
        start_dir: Option<String>,
    ) -> iced::Task<Message> {
        if !Self::host_can_console(&conn) {
            return iced::Task::none();
        }
        let Some(tab) = self.tabs.get(tab_idx) else {
            return iced::Task::none();
        };
        // Already there: reveal it. The layout is deliberately NOT
        // re-applied, because by then it is the user's own arrangement,
        // not the default they picked once in Settings.
        if tab.console_pane().is_some() {
            return self.show_tab_surface(tab_idx, TabSurface::Console);
        }
        let layout = self.prefs.sftp_console_layout;
        let target = tab.focused;
        // Files mode hides the whole grid, so a console split behind it
        // would open where nobody can see it. Leaving first also makes
        // "console from the SFTP screen" one gesture rather than two.
        let leave_files = if tab.files_mode {
            self.update(Message::Tabs(TabsMessage::ToggleTabFilesMode(tab_idx)))
        } else {
            iced::Task::none()
        };
        let Ok(mut term) =
            TerminalState::new_no_pty(DEFAULT_TERM_COLS as u16, DEFAULT_TERM_ROWS as u16)
        else {
            return leave_files;
        };
        term.set_palette(self.resolve_terminal_palette_for_connection(&conn));
        // Same seed line the split-pane connect writes: the pane exists
        // before the dial answers, and an empty black rectangle is the
        // one thing that reads as a crash.
        term.process(
            format!("Connecting to {} ({}:{})...\r\n", conn.label, conn.hostname, conn.port)
                .as_bytes(),
        );
        let Some(pane_id) = self.make_split_pane(
            tab_idx,
            target,
            layout.axis(),
            conn.label.clone(),
            Arc::new(Mutex::new(term)),
            crate::state::PaneOrigin::Host(conn.id),
        ) else {
            return leave_files;
        };
        if let Some(tab) = self.tabs.get_mut(tab_idx) {
            if let Some(pane) = tab.pane_by_id_mut(pane_id) {
                pane.purpose = PanePurpose::SftpConsole;
            }
            if layout.starts_maximized() {
                let handle = tab.focused;
                tab.maximize_handle(handle);
            }
        }
        // Read by `begin_sftp_console` when the dial lands, exactly as
        // on the tab path.
        self.pending_console_dir = start_dir;
        self.active_tab = Some(tab_idx);
        self.active_view = crate::state::View::Terminal;
        self.remember_terminal_tab_focus(tab_idx);
        iced::Task::batch([
            leave_files,
            self.spawn_ssh_for_pane_conn(conn, None, tab_idx, pane_id),
        ])
    }

    /// Which surface the tab at `idx` is showing.
    ///
    /// Files is a tab-level mode and the other two are panes, so the
    /// question is answered in that order: a tab in Files mode is
    /// showing Files whatever its grid holds underneath.
    pub(crate) fn tab_surface(&self, idx: usize) -> TabSurface {
        let Some(tab) = self.tabs.get(idx) else {
            return TabSurface::Terminal;
        };
        if tab.files_mode {
            return TabSurface::Files;
        }
        match tab.pane_grid.get(tab.focused) {
            Some(p) if p.purpose == PanePurpose::SftpConsole => TabSurface::Console,
            _ => TabSurface::Terminal,
        }
    }

    /// The surfaces the tab at `idx` can switch between, in switch
    /// order. Fewer than two means there is nothing to switch and no
    /// control is drawn.
    ///
    /// Terminal is conditional like the rest: a console opened from a
    /// host card is a tab with no shell in it, and offering a switch to
    /// a pane that does not exist is how a control starts reading as
    /// broken.
    pub(crate) fn tab_surfaces(&self, idx: usize) -> Vec<TabSurface> {
        let Some(tab) = self.tabs.get(idx) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(3);
        if tab.shell_pane().is_some() {
            out.push(TabSurface::Terminal);
        }
        if tab.console_pane().is_some() {
            out.push(TabSurface::Console);
        }
        // Unchanged gate (issue #61): the chip only exists once the tab
        // HAS an SFTP session, which the tab menu's "Open SFTP session"
        // creates. A tab already in Files mode keeps it, which is the
        // way back.
        if self.tab_has_sftp_session(tab) {
            out.push(TabSurface::Files);
        }
        out
    }

    /// The surface the switch control moves to next, cycling in the
    /// order `tab_surfaces` lists. `None` when there is nothing to
    /// switch to.
    pub(crate) fn tab_next_surface(&self, idx: usize) -> Option<TabSurface> {
        TabSurface::next_in(&self.tab_surfaces(idx), self.tab_surface(idx))
    }

    /// Show one of the tab's surfaces.
    ///
    /// Every switch is "show this one", never a toggle of the mechanism
    /// behind it, which is what lets the chip, the status bar and the
    /// hotkey share one path across two different mechanisms (a
    /// tab-level mode and a pane of the grid).
    ///
    /// Focus CARRIES THE ZOOM (`focus_handle`), so a maximized console
    /// switched back to the terminal maximizes the terminal instead of
    /// focusing a pane hidden behind the zoom. In a split, the same call
    /// simply moves focus, which is the whole "easy way to switch
    /// between them" the split needs.
    pub(crate) fn show_tab_surface(
        &mut self,
        idx: usize,
        surface: TabSurface,
    ) -> iced::Task<Message> {
        let Some(tab) = self.tabs.get(idx) else {
            return iced::Task::none();
        };
        let in_files = tab.files_mode;
        // Resolved BEFORE the first `update`, which takes `self`
        // mutably: the pane a surface names cannot change under a tab
        // selection, and reading it after would only cost a re-lookup.
        let handle = match surface {
            TabSurface::Console => tab.console_pane(),
            _ => tab.shell_pane(),
        };
        // Clicking a background tab's chip brings the tab to front,
        // whichever surface it lands on (the Files toggle's own rule).
        let select = if self.active_tab != Some(idx) {
            self.update(Message::Tabs(TabsMessage::SelectTab(idx)))
        } else {
            iced::Task::none()
        };
        if surface == TabSurface::Files {
            if in_files {
                return select;
            }
            let enter = self.update(Message::Tabs(TabsMessage::ToggleTabFilesMode(idx)));
            return iced::Task::batch([select, enter]);
        }
        // No such pane: the control is not offering this surface, so
        // this can only be a stale message. Doing nothing beats moving
        // focus somewhere the user did not ask for.
        let Some(handle) = handle else {
            return select;
        };
        let leave = if in_files {
            self.update(Message::Tabs(TabsMessage::ToggleTabFilesMode(idx)))
        } else {
            iced::Task::none()
        };
        if let Some(tab) = self.tabs.get_mut(idx) {
            tab.focus_handle(handle);
        }
        iced::Task::batch([select, leave])
    }

    /// The console entry for the tab at `idx`: its host, and the working
    /// directory its shell had reached.
    ///
    /// `None` when the tab has no saved host behind it. A quick-connect
    /// tab is deliberately included only when it resolves, for the same
    /// reason quick hosts stay out of pins: their credentials live in a
    /// store that does not outlive the session.
    pub(crate) fn tab_console_target(
        &self,
        idx: usize,
    ) -> Option<(oryxis_core::models::Connection, Option<String>)> {
        let tab = self.tabs.get(idx)?;
        let pane = tab.active();
        // A console needs a live SSH session to multiplex on, or a host
        // it can dial. A mosh pane has neither: it let its SSH go on
        // purpose, which is why asking it for files opens a tab of its
        // own rather than a surface beside it.
        let conn_id = match pane.origin {
            crate::state::PaneOrigin::Host(id) => id,
            _ => return None,
        };
        let conn = self.connections.iter().find(|c| c.id == conn_id)?.clone();
        if !Self::host_can_console(&conn) {
            return None;
        }
        // The shell's own directory, when it reported one. This is the
        // trick SecureCRT's SFTP tab needs an escape sequence for; OSC 7
        // already told us.
        let dir = pane.cwd.clone().filter(|d| d.starts_with('/'));
        Some((conn, dir))
    }

    /// Whether a host can carry an SFTP console at all.
    ///
    /// Two exclusions, and both are about the dial landing somewhere
    /// else than where the console waits:
    ///
    /// - **Not SSH.** `start_ssh_tab` forwards every other protocol to
    ///   its own connect path, none of which reaches `SshConnected`, so
    ///   the console would never open AND the one-shot purpose flag
    ///   would never be consumed. The next ordinary SSH tab would then
    ///   be born a console: a hint that outlived its request, which is
    ///   the failure this app documents in three other places.
    /// - **mosh.** A mosh host branches one line ABOVE the console in
    ///   `SshConnected`, deliberately, because mosh closes the SSH
    ///   session it is handed. So asking for a console on one would
    ///   silently deliver a mosh shell instead. `transport.ssh()`
    ///   already keeps the entry off an OPEN mosh tab; this is what
    ///   keeps it off the host card, where there is no tab to ask.
    pub(crate) fn host_can_console(conn: &oryxis_core::models::Connection) -> bool {
        conn.protocol == oryxis_core::models::connection::ConnectionProtocol::Ssh
            && conn.mosh.is_none()
    }

    /// What a pane's session is for. `Shell` for anything that never
    /// asked to be anything else, which is every pane but the ones the
    /// console opened.
    pub(crate) fn pane_purpose(&self, pane_id: uuid::Uuid) -> PanePurpose {
        self.tabs
            .iter()
            .find_map(|t| t.pane_grid.panes.values().find(|p| p.id == pane_id))
            .map(|p| p.purpose)
            .unwrap_or_default()
    }

    /// Open the SFTP subsystem on `ssh` and hand the pane a console over
    /// it.
    ///
    /// The starting directory is the shell's own working directory when
    /// one is known (a console opened beside a live tab inherits where
    /// that tab had navigated, which is the one thing SecureCRT's own
    /// SFTP tab needs an escape sequence to do), and the session's home
    /// otherwise.
    pub(crate) fn begin_sftp_console(
        &mut self,
        pane_id: uuid::Uuid,
        ssh: Arc<SshSession>,
    ) -> iced::Task<Message> {
        let cols = self
            .tabs
            .iter()
            .find_map(|t| t.pane_grid.panes.values().find(|p| p.id == pane_id))
            .and_then(|p| p.terminal.lock().ok().map(|t| t.cols()))
            .map_or(80, |c| c.max(1));
        let label = self
            .tabs
            .iter()
            .find_map(|t| t.pane_grid.panes.values().find(|p| p.id == pane_id))
            .map(|p| p.label.clone())
            .unwrap_or_else(|| "host".to_string());
        let start_dir = self.pending_console_dir.take();
        let local_cwd = std::env::current_dir().unwrap_or_else(|_| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from("."))
        });

        let stream = iced::stream::channel::<Message>(
            128,
            move |mut sender: iced::futures::channel::mpsc::Sender<Message>| async move {
                use iced::futures::SinkExt as _;
                let _ = sender
                    .send(Message::Ssh(SshMessage::SshProgress(
                        pane_id,
                        crate::state::ConnectionStep::OpeningSession,
                        crate::i18n::t("sftp_console_opening").to_string(),
                    )))
                    .await;

                let client = match ssh.open_sftp().await {
                    Ok(client) => client,
                    Err(error) => {
                        let _ = sender
                            .send(Message::Ssh(SshMessage::SshError(
                                pane_id,
                                crate::i18n::t("sftp_console_error_open")
                                    .replace("{reason}", &error.to_string()),
                            )))
                            .await;
                        return;
                    }
                };

                // The home is resolved once, here, so `cd` with no
                // argument and a leading `~` both have an answer without
                // a round trip later. A server that will not canonicalize
                // `.` leaves the console rooted at `/`, which is wrong
                // but navigable, rather than failing to open at all.
                let home = client
                    .canonicalize(".")
                    .await
                    .unwrap_or_else(|_| "/".to_string());
                // Wipe what SSH left on the pane. The dial opened a
                // shell of its own on the way in, so by now the pane is
                // carrying a login banner ("Last login: ...") and a
                // prompt that belong to a session the user never asked
                // to see and that is about to be closed. Left there they
                // would sit above the console's own banner for the rest
                // of its life.
                //
                // Word for word the reason the mosh handover clears at
                // the same point, and placed the same way: after the
                // round trip that opened the subsystem, by which time
                // the shell has finished saying its piece, and before
                // the console prints anything of its own.
                let _ = sender
                    .send(Message::Terminal(TerminalMessage::PtyOutput(
                        pane_id,
                        b"\x1b[H\x1b[2J\x1b[3J\x1b[m".to_vec(),
                    )))
                    .await;

                let (session, mut rx) = SftpShellSession::spawn(
                    Arc::clone(&ssh),
                    client,
                    home,
                    local_cwd,
                    cols,
                    label,
                );
                if let Some(dir) = start_dir {
                    // Delivered as typed input rather than as a
                    // constructor argument: it goes through the same
                    // parse, resolution and error reporting a user's own
                    // `cd` does, so a directory that has vanished since
                    // the shell was there says so instead of leaving the
                    // console silently somewhere else.
                    let _ = session.write(format!("cd {dir}\r").as_bytes());
                }

                let transport = TerminalTransport::SftpShell(Arc::new(session));
                let _ = sender
                    .send(Message::Ssh(SshMessage::SshConnected(pane_id, transport)))
                    .await;

                // The dial opened a SHELL channel on the way here, with a
                // PTY and a login banner and a prompt, and its byte
                // stream is pointed at this very pane. Nobody is going to
                // read it: the pane renders the console now. Left running
                // it would interleave a login banner and a shell prompt
                // into the console's output, so it is let go.
                //
                // Closing the SESSION does not close the CONNECTION: the
                // transport is reference-counted and the console's SFTP
                // channel rides it, so what dies here is one channel that
                // had no reader. The `Arc<SshSession>` the console holds
                // is what keeps that transport alive.
                //
                // Done AFTER the transport was published, not before,
                // because the dying shell stream ends with a
                // `SshDisconnected` for this pane. The handler discards
                // one whose pane already has a live transport, and this
                // ordering is what guarantees it finds one. Same shape as
                // the mosh handover, which closes its SSH for a different
                // reason and relies on the same rule.
                ssh.close();

                while let Some(data) = rx.recv().await {
                    if sender
                        .send(Message::Terminal(TerminalMessage::PtyOutput(pane_id, data)))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                let _ = sender
                    .send(Message::Ssh(SshMessage::SshDisconnected(pane_id)))
                    .await;
            },
        );

        iced::Task::stream(stream)
    }
}
