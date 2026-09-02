//! A pane: one live session inside a tab.
//!
//! What it is connected to (`PaneOrigin`, `TerminalTransport`), what the
//! shell has told us about itself (`PromptState` and the capture state
//! command history reads), the Files sidebar mounted beside it, and the
//! transfer surfaces that ride on it (ZMODEM, OS drops).

use super::super::*;

/// What a pane reconnects to, so a saved session group can reference it.
/// This is an explicit discriminator rather than inferring "local" from a
/// missing connection id: cloud/SSM/ECS panes also lack a saved
/// `Connection`, so `None`-means-local would mis-save them. `Ephemeral`
/// covers those (and any pane we can't reference by id); they are pruned
/// when a tab is saved as a session group.
#[derive(Debug, Clone)]
pub(crate) enum PaneOrigin {
    /// Live reference to a saved Connection by id.
    Host(Uuid),
    /// Quick-connect host: the id points into `Oryxis.quick_connects`, an
    /// in-memory store that is never persisted. Kept apart from `Host` so
    /// vault-backed features (edit in place, session groups, pin restore)
    /// opt in deliberately instead of dereferencing a dangling vault id.
    QuickHost(Uuid),
    /// A local terminal; the spec is captured so the same shell is restored.
    Local(LocalShellSpec),
    /// Cloud/SSM/ECS or otherwise non-referenceable pane.
    Ephemeral,
}

/// What a pane's session is FOR, which is not the same question as what
/// it connects to (`PaneOrigin`) or how (`TerminalTransport`).
///
/// It exists because the SFTP console (issue #188) dials through the
/// ordinary SSH path: it needs the host key prompt, the password prompt,
/// the proxy consent and the expanded jump chain, and the repo's answer
/// to "reuse the whole connect experience" is the one mosh already
/// established, which is to branch inside the `SshConnected` handler
/// rather than grow a second dialler.
///
/// So the dial has to carry the intent, and carrying it on the PANE is
/// what makes reconnect correct for free: `spawn_ssh_for_pane_conn`
/// rebuilds a pane's session in place, and without this a console whose
/// link dropped would come back as a SHELL, changing what the tab is
/// without anybody asking.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum PanePurpose {
    /// An interactive session with whatever is on the far side.
    #[default]
    Shell,
    /// An SFTP console. The dial ends with `open_sftp()` and the
    /// transport becomes `TerminalTransport::SftpShell`.
    SftpConsole,
}

/// Where a pane's remote shell stands in the OSC 133 prompt cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PromptState {
    /// No OSC 133 mark seen yet: the host has no shell integration and the
    /// command-history capture falls back to the echo heuristic.
    NoIntegration,
    /// `PromptEnd` (B) seen: the shell is reading a command line that starts
    /// at `col` of absolute grid row `abs_line`.
    AtPrompt { abs_line: i64, col: u16 },
    /// A command is running or the prompt is being redrawn; input is a
    /// program's stdin and must never be recorded.
    Busy,
}

/// A command submitted while `AtPrompt` whose echo had not reached the grid
/// yet (a paste with a trailing newline arrives before the round trip). The
/// echoed line is read back from these coordinates when `OutputStart`
/// confirms the shell accepted a command.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PendingCapture {
    pub b_abs: i64,
    pub b_col: u16,
}

/// In-band command capture: what the shell itself reported it parsed, via
/// `OSC 633 ; E`. This is the only capture path that survives a multiplexer
/// (under tmux the app's grid is tmux's repaint of every pane, so reading
/// the command back from it would splice the neighbouring pane's row into
/// the text), and the only one that can't mistake a keystroke for a command:
/// the text comes from the shell, not from the screen.
#[derive(Debug, Default, Clone)]
pub(crate) struct InbandCapture {
    /// True once this pane saw its first `E`. From then on the grid-reading
    /// and heuristic paths are off for this pane: keeping them alongside
    /// would double-record every command, and under tmux the outer grid's
    /// prompt belongs to whichever pane tmux drew there last.
    pub seen: bool,
    /// The reported command line, held until the `OutputStart` that confirms
    /// the shell actually ran it (a bare Enter or a Ctrl+C never reaches one).
    pub pending: Option<String>,
}

/// The live remote transport feeding a terminal pane. SSH and Telnet
/// expose the same session surface (write / resize / senders /
/// is_alive / close), so every generic pane path calls through this
/// enum; features that need the SSH machinery underneath (SFTP mounts,
/// OS detection, exec channels) reach it via [`TerminalTransport::ssh`]
/// and simply don't exist for Telnet panes. An enum rather than a
/// trait object because only the pane path branches, and the SSH arm
/// must keep handing out its concrete `Arc<SshSession>`.
#[derive(Debug, Clone)]
pub(crate) enum TerminalTransport {
    Ssh(Arc<SshSession>),
    Telnet(Arc<oryxis_telnet::TelnetSession>),
    Serial(Arc<oryxis_serial::SerialSession>),
    /// A session carried over mosh. It was STARTED by SSH and does not
    /// hold on to it: the SSH connection is TCP and dies the moment the
    /// address changes, which is the moment mosh exists for, so keeping
    /// it would give the tab two lifetimes and let half of it break
    /// exactly when the other half proved its worth. What needs SSH
    /// asks for its own; see `mosh_files_open_in_new_tab`.
    Mosh(Arc<oryxis_mosh::MoshSession>),
    /// An interactive SFTP console (issue #188): the pane's far side is
    /// not a shell but a REPL of ours, speaking `sftp(1)`'s command set
    /// over a channel on a live SSH session.
    ///
    /// `ssh()` answers `None` even though there IS an SSH session
    /// underneath, and that is the point rather than an oversight. What
    /// that accessor gates is the feature set a SHELL pane offers (an
    /// SFTP mount beside it, OS detection, the AI exec channel), and
    /// none of those mean anything pointed at a console: a mount beside
    /// a mount, a probe that would have to type into a prompt that does
    /// not run commands. The console holds its own `Arc<SshSession>`
    /// for the one thing it needs it for, which is knowing whether the
    /// link is still there.
    SftpShell(Arc<oryxis_ssh::sftp_shell::SftpShellSession>),
}

impl TerminalTransport {
    /// The inner SSH session, for the SSH-only feature paths.
    pub fn ssh(&self) -> Option<&Arc<SshSession>> {
        match self {
            TerminalTransport::Ssh(s) => Some(s),
            TerminalTransport::Telnet(_)
            | TerminalTransport::Serial(_)
            | TerminalTransport::Mosh(_)
            | TerminalTransport::SftpShell(_) => None,
        }
    }

    /// The inner mosh session, for the one thing only mosh can answer.
    ///
    /// Every other transport reports its health by being up or down, and
    /// `is_alive` covers both. mosh is up while it is out of touch, on
    /// purpose, so "alive" stops being the whole answer and something
    /// has to carry the rest.
    pub fn mosh(&self) -> Option<&Arc<oryxis_mosh::MoshSession>> {
        match self {
            TerminalTransport::Mosh(s) => Some(s),
            TerminalTransport::Ssh(_)
            | TerminalTransport::Telnet(_)
            | TerminalTransport::Serial(_)
            | TerminalTransport::SftpShell(_) => None,
        }
    }

    /// Whether this session outlives the network changing underneath
    /// it.
    ///
    /// What it decides is where the tab's file browsing LIVES. The
    /// hybrid surfaces multiplex SFTP on the pane's own SSH connection,
    /// and a session that survives roaming does not have one to
    /// multiplex on: the SSH that started it was let go precisely
    /// because it would not have survived. So those surfaces open a
    /// tab of their own instead, where the connection is visibly its
    /// own thing and can die without taking the shell with it.
    pub fn survives_roaming(&self) -> bool {
        matches!(self, TerminalTransport::Mosh(_))
    }

    pub fn write(&self, data: &[u8]) -> Result<(), String> {
        match self {
            TerminalTransport::Ssh(s) => s.write(data).map_err(|e| e.to_string()),
            TerminalTransport::Telnet(s) => s.write(data).map_err(|e| e.to_string()),
            TerminalTransport::Serial(s) => s.write(data).map_err(|e| e.to_string()),
            TerminalTransport::Mosh(s) => s.write(data).map_err(|e| e.to_string()),
            TerminalTransport::SftpShell(s) => s.write(data).map_err(|e| e.to_string()),
        }
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        match self {
            TerminalTransport::Ssh(s) => s.resize(cols, rows),
            TerminalTransport::Telnet(s) => s.resize(cols, rows),
            TerminalTransport::Mosh(s) => s.resize(cols, rows),
            // The console redraws its own prompt and re-columnizes its
            // own listings, so it wants the width even though nothing
            // remote is listening for it.
            TerminalTransport::SftpShell(s) => s.resize(cols, rows),
            // A serial line has no window size; resize is a no-op.
            TerminalTransport::Serial(_) => {}
        }
    }

    /// Clone of the resize sender (SSH window-change / Telnet NAWS) so
    /// the terminal state forwards viewport changes directly. `None`
    /// for serial, which has no viewport concept.
    pub fn resize_sender(&self) -> Option<tokio::sync::mpsc::UnboundedSender<(u16, u16)>> {
        match self {
            TerminalTransport::Ssh(s) => Some(s.resize_sender()),
            TerminalTransport::Telnet(s) => Some(s.resize_sender()),
            TerminalTransport::Mosh(s) => Some(s.resize_sender()),
            TerminalTransport::SftpShell(s) => Some(s.resize_sender()),
            TerminalTransport::Serial(_) => None,
        }
    }

    /// Clone of the input sender for in-band query replies (cursor
    /// position report, DECRQM, ...), which remote programs block on.
    pub fn write_sender(&self) -> tokio::sync::mpsc::UnboundedSender<Vec<u8>> {
        match self {
            TerminalTransport::Ssh(s) => s.write_sender(),
            TerminalTransport::Telnet(s) => s.write_sender(),
            TerminalTransport::Serial(s) => s.write_sender(),
            TerminalTransport::Mosh(s) => s.write_sender(),
            TerminalTransport::SftpShell(s) => s.write_sender(),
        }
    }

    /// Whether the session behind this pane is still usable.
    ///
    /// **Every arm owes one ordering guarantee: it must read as DEAD
    /// BEFORE its output stream ends.** A new transport added here
    /// inherits that debt, and the compiler cannot name it: the match
    /// below will demand an arm, and say nothing about when the answer
    /// has to be settled.
    ///
    /// The reason is `SshDisconnected`. The end of a transport's output
    /// stream is what produces that message, and the handler discards it
    /// while this still says "alive", because since the mosh handover a
    /// superseded session reporting its own death is ordinary rather
    /// than exotic. So a session that really died and still answered
    /// "alive" here would get its own disconnect thrown away, leaving
    /// the tab reading connected over a dead link until the 30 s
    /// liveness sweep caught it (longer while the vault is soft-locked,
    /// which unmounts that sweep).
    ///
    /// The four existing arms satisfy it the same way: the reader task
    /// stores its death flag and only then drops the output sender, in
    /// one task with no await between (`reader_done` on SSH / Telnet /
    /// Serial, `alive` on mosh). Anything that settles a task later
    /// (a `JoinHandle`, a channel some OTHER task closes) is a race, not
    /// a guarantee, and was exactly what this replaced.
    pub fn is_alive(&self) -> bool {
        match self {
            TerminalTransport::Ssh(s) => s.is_alive(),
            TerminalTransport::Telnet(s) => s.is_alive(),
            TerminalTransport::Serial(s) => s.is_alive(),
            TerminalTransport::Mosh(s) => s.is_alive(),
            TerminalTransport::SftpShell(s) => s.is_alive(),
        }
    }

    /// Tear the session down (idempotent on every arm).
    pub fn close(&self) {
        match self {
            TerminalTransport::Ssh(s) => s.close(),
            TerminalTransport::Telnet(s) => s.close(),
            TerminalTransport::Serial(s) => s.close(),
            TerminalTransport::Mosh(s) => s.close(),
            TerminalTransport::SftpShell(s) => s.close(),
        }
    }
}

/// Sidebar Files tab state, one instance per pane: an SFTP channel
/// multiplexed on this pane's SSH session plus the browsing state.
/// The channel dies with the session, so `SshDisconnected` resets the
/// whole struct (keeping only the user's follow / hidden preferences).
#[derive(Default)]
pub(crate) struct PaneFiles {
    /// The browser's backend: the SFTP channel on this pane's live
    /// `client::Handle`, or the app's own filesystem for a local shell
    /// (issue #145). `None` until the Files tab is first opened
    /// (mounted lazily so panes that never browse pay nothing).
    pub client: Option<crate::local_files::FilesClient>,
    /// True while the initial mount (open channel + first listing) is
    /// in flight, the guard against double-mounting on rapid clicks.
    pub mounting: bool,
    /// Current directory (absolute remote POSIX path). Empty until the
    /// first listing lands.
    pub path: String,
    /// The session's home directory, resolved at mount. Expands the
    /// `~`-relative cwd the OSC 0/2 title fallback produces.
    pub home: Option<String>,
    /// In-progress manual path edit (the header path is clickable,
    /// mirroring the SFTP pane's path editing); `None` = display mode.
    pub path_editing: Option<String>,
    /// In-progress inline rename: `(full remote path, edited name)`.
    pub rename: Option<(String, String)>,
    /// In-progress inline create: `(kind, typed name)`, rendered as an
    /// input row at the top of the list.
    pub new_entry: Option<(SftpEntryKind, String)>,
    /// Entries of `path`, sorted dirs-first / name-insensitive.
    pub entries: Vec<SftpEntry>,
    /// True while a `list_dir` (navigation or cwd follow) is in flight.
    pub loading: bool,
    /// Monotonic request stamp: every mount / list task carries the
    /// value at dispatch time and its completion is dropped unless it
    /// still matches (latest request wins). Bumped by
    /// `reset_for_disconnect` too, so a mount racing a reconnect can't
    /// install a client whose channel rode the dead session.
    pub req_seq: u64,
    pub error: Option<String>,
    /// Whether the browser follows the shell's OSC 7 cwd. `true` for a
    /// fresh pane; the pin toggle flips it.
    pub follow_disabled: bool,
    pub show_hidden: bool,
    /// Directories this browser actually adopted, most recent first,
    /// deduped, capped (issue #85; the SFTP pane's `path_history`
    /// sibling). Feeds the path combo-box dropdown. In-memory and
    /// host-scoped: `reset_for_disconnect` clears it.
    pub path_history: Vec<String>,
    /// Back / forward stacks; see `PaneState`'s for why they are not the
    /// recency list.
    pub nav_back: Vec<String>,
    pub nav_fwd: Vec<String>,
    pub nav_replay: bool,
    /// Whether the path combo-box dropdown is open.
    pub path_history_open: bool,
    /// Full paths of the selected rows (the SFTP pane's multi-select
    /// rule): a plain click selects one, Ctrl/Cmd-click toggles,
    /// Shift-click extends a range from `selection_anchor`. Cleared on
    /// navigation / mount / disconnect.
    pub selected: Vec<String>,
    /// Anchor row for shift-click range selection (mirrors the SFTP
    /// pane's `selection_anchor`); follows the last click that set a
    /// single selection or toggled a row.
    pub selection_anchor: Option<String>,
    /// Timestamp + path of the last single click, for double-click
    /// detection (matching the SFTP pane's rule).
    pub last_click: Option<(std::time::Instant, String)>,
    /// This browser's own transfer, run by the same queue machinery the
    /// dual-pane SFTP surface uses.
    ///
    /// Per PANE rather than per tab, because that is where the browser
    /// lives: a split tab can have two panes browsing two hosts, and one
    /// shared slot would make them fight. It also outlives the sidebar
    /// being closed and the tab losing focus, which is what lets a long
    /// transfer keep running and keep reporting from the tab strip.
    ///
    /// Deliberately NOT cleared by `reset_for_disconnect`: a transfer
    /// that died with its session still has to show why.
    pub transfer: crate::state::TransferSlot,
}

impl PaneFiles {
    /// Follow-cwd is stored inverted so `Default` gives "on".
    pub fn follow(&self) -> bool {
        !self.follow_disabled
    }

    /// Drop everything tied to the dead SSH session, keeping only the
    /// user's preferences (follow / hidden) for the reconnect. The
    /// request stamp bumps so any in-flight mount / listing on the old
    /// session is dropped when it completes.
    pub fn reset_for_disconnect(&mut self) {
        self.client = None;
        self.mounting = false;
        self.path.clear();
        self.home = None;
        self.path_editing = None;
        self.rename = None;
        self.new_entry = None;
        self.entries.clear();
        self.loading = false;
        self.req_seq += 1;
        self.error = None;
        // Host-scoped: a reconnect may land on another host's tree, so the
        // in-memory list goes. It is no longer a loss: the history is
        // persisted per host and `hydrate_files_recent` refills this on the
        // next mount (issue #114). Before that, closing the host was the
        // end of it.
        self.path_history.clear();
        self.path_history_open = false;
        self.selected.clear();
        self.selection_anchor = None;
        self.last_click = None;
    }

    /// Record an adopted directory in the combo-box history: most
    /// recent first, a revisit moves the entry to the top, capped so
    /// the dropdown stays scannable (issue #85, the SFTP pane's rule).
    /// Record leaving `previous` for a new directory. Clears the forward
    /// stack, because branching off mid-history is a new future.
    pub fn push_nav(&mut self, previous: String) {
        const NAV_CAP: usize = 100;
        // A back / forward step consumes the flag instead of recording:
        // its arrival is the history being replayed, not a new visit.
        if std::mem::take(&mut self.nav_replay) {
            return;
        }
        if previous.is_empty() {
            return;
        }
        self.nav_back.push(previous);
        self.nav_fwd.clear();
        if self.nav_back.len() > NAV_CAP {
            self.nav_back.remove(0);
        }
    }

    /// Pop the previous directory, remembering `current` so Forward can
    /// come back to it. `None` when there is nowhere to go.
    pub fn nav_go_back(&mut self, current: String) -> Option<String> {
        let target = self.nav_back.pop()?;
        self.nav_fwd.push(current);
        self.nav_replay = true;
        Some(target)
    }

    /// The mirror of [`Self::nav_go_back`].
    pub fn nav_go_forward(&mut self, current: String) -> Option<String> {
        let target = self.nav_fwd.pop()?;
        self.nav_back.push(current);
        self.nav_replay = true;
        Some(target)
    }

    pub fn push_path_history(&mut self, path: String) {
        const PATH_HISTORY_CAP: usize = 20;
        if path.is_empty() {
            return;
        }
        self.path_history.retain(|p| p != &path);
        self.path_history.insert(0, path);
        self.path_history.truncate(PATH_HISTORY_CAP);
    }

    /// Stamp a new outgoing request (mount or listing) and return its
    /// sequence value for the completion message to carry.
    pub fn next_req(&mut self) -> u64 {
        self.req_seq += 1;
        self.req_seq
    }
}

/// A login script answering an interactive bastion on one pane.
///
/// Holds NO secrets: `conn_id` is what a fired `SecretRef` is resolved
/// against at send time, so a soft vault lock (which zeroizes the key)
/// simply makes the next resolution fail instead of leaving a decrypted
/// credential parked in app state.
#[derive(Debug)]
pub(crate) struct LoginScriptRun {
    pub runner: oryxis_core::login_script::ScriptRunner,
    /// Whose credentials the script's `SecretRef` steps mean.
    pub conn_id: Uuid,
    /// Deferred until the run finishes: sending it up front would land
    /// in the bastion's menu instead of the asset's shell, and a
    /// menu-driven bastion drains type-ahead it did not ask for.
    pub pending_startup: Option<String>,
    /// Generation for the timeout tick, so a stale wake-up from a run
    /// that already finished cannot abort the one after it.
    pub generation: u64,
}

impl Pane {
    /// The SAVED connection this pane runs on, if any. `None` for local
    /// shells, cloud / ephemeral panes, and quick-connect hosts, which
    /// live outside `Oryxis::connections` and so carry no stored
    /// per-host settings to look up.
    pub fn saved_conn_id(&self) -> Option<Uuid> {
        match &self.origin {
            PaneOrigin::Host(id) => Some(*id),
            PaneOrigin::QuickHost(_) | PaneOrigin::Local(_) | PaneOrigin::Ephemeral => None,
        }
    }
}

/// What one highlight rule has done on one pane this session (C6).
#[derive(Debug, Default)]
pub(crate) struct TriggerRuntime {
    /// When the rule's action last ran, so a log full of the same word
    /// produces one notification rather than one per line.
    pub last_fired: Option<std::time::Instant>,
    /// The user's answer to "may this rule type into this session".
    /// `None` = never asked, `Some(false)` = refused, and a refusal is
    /// remembered for the session so a hostile stream cannot re-ask
    /// until the user clicks the wrong button.
    pub snippet_allowed: Option<bool>,
    /// Set while the confirmation is on screen, so the next matching
    /// line does not queue a second one behind it.
    pub asking: bool,
}

/// How long a rule waits before its action can run again on the same
/// pane. Output arrives in bursts (a `tail -f` on a failing service, a
/// build printing the same warning per file), and a notification per
/// line is not a notification, it is a denial of service on the user's
/// attention.
pub(crate) const TRIGGER_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(5);

impl TriggerRuntime {
    /// Whether the action may run now, stamping the time when it may.
    pub fn take_turn(&mut self, now: std::time::Instant) -> bool {
        if let Some(last) = self.last_fired
            && now.duration_since(last) < TRIGGER_COOLDOWN
        {
            return false;
        }
        self.last_fired = Some(now);
        true
    }
}

/// One terminal pane, owns its alacritty grid and (optionally) the
/// remote session feeding it. A `TerminalTab` holds one or more panes
/// in a `pane_grid::State`, which owns their split layout.
pub(crate) struct Pane {
    /// Stable identity used to route PTY output / session events to the
    /// right pane (the `pane_grid::Pane` handle is only unique within a
    /// tab's grid, this `Uuid` is unique across all tabs).
    pub id: Uuid,
    /// This pane's own connection label ("user@host", "Local Shell", ...).
    /// The tab bar shows the *focused* pane's label + icon, so a tab split
    /// across two hosts reads as whichever pane you're in.
    pub label: String,
    pub terminal: Arc<Mutex<TerminalState>>,
    /// Remote transport handle (SSH or Telnet; None for local shell).
    pub session: Option<TerminalTransport>,
    /// What this pane's session is for. Set before the dial and read by
    /// `wire_connected_pane`, which is what makes a console survive a
    /// reconnect as a console. See [`PanePurpose`].
    pub purpose: PanePurpose,
    /// True while an in-place reconnect dial for this pane is in
    /// flight, making a repeat `ReconnectTab` a no-op (a held chord or
    /// an auto-reconnect tick racing a manual click must not stack a
    /// second dial). Set when the reconnect spawns the dial; cleared by
    /// every completion (`SshConnected` attach, `SshDisconnected`,
    /// `PaneConnectError`).
    pub connecting: bool,
    /// Session log ID for terminal recording.
    pub session_log_id: Option<Uuid>,
    /// Recorded bytes not yet flushed to the vault. PTY output appends
    /// here; `Oryxis::flush_session_logs` drains it (size threshold, a
    /// periodic tick, disconnect, or window close). Batching keeps the
    /// vault from taking one write per SSH chunk.
    pub session_log_buf: Vec<u8>,
    /// Recording clock zero: set on the first recorded output batch, so
    /// chunk offsets (asciicast timing) count from the session's first
    /// byte rather than the connect handshake.
    pub session_log_t0: Option<std::time::Instant>,
    /// Arrival marks into `session_log_buf`: (byte position, ms since
    /// `session_log_t0`), one per PTY output batch. The flush splits
    /// the drained bytes at newline-aligned marks so the stored chunks
    /// carry real replay timing without extra writes mid-session.
    pub session_log_marks: Vec<(usize, i64)>,
    /// Resize marks into `session_log_buf`: (byte position, ms since
    /// `session_log_t0`, cols, rows), recorded when an output batch is
    /// processed on a grid whose size differs from the last recorded
    /// one. The flush interleaves these as `kind='r'` rows between the
    /// output chunks, so replay resizes at the same stream position the
    /// live grid did (the first batch records the initial geometry).
    pub session_log_resizes: Vec<(usize, i64, u16, u16)>,
    /// Last terminal geometry written to the recording; a change
    /// appends a resize mark (output-batch path, or the flush-cadence
    /// fallback for a resize with no output after it).
    pub session_log_last_size: Option<(u16, u16)>,
    /// Plain-text mirror of this recording on disk, resolved on the
    /// first flush that has bytes for it and kept for the rest of the
    /// session so the name cannot drift mid-recording. `None` while the
    /// mirror is off, or before the first flush.
    pub session_log_file: Option<std::path::PathBuf>,
    /// What this pane reconnects to when restored from a saved session group.
    /// Defaults to `Ephemeral`; the creating site overrides it to `Host` or
    /// `Local` when the pane is referenceable.
    pub origin: PaneOrigin,
    /// This pane's session ended and the pane is still here, waiting for
    /// the user to restart it or close it (issue #208).
    ///
    /// Set on any pane of a SPLIT tab, and on a LOCAL shell that is a tab
    /// on its own, whichever origin opened that shell (the picker, a
    /// saved Local host, a quick-connect one). Not on a lone remote pane:
    /// that tab is relabelled "(disconnected)" and the auto-reconnect
    /// sweep picks it up, which is a better answer and only possible
    /// because it has no siblings.
    /// Neither of those can serve a split tab, since both are tab-wide
    /// and `ReconnectTab` rebuilds the tab, taking the live siblings with
    /// it. So the pane keeps the verdict instead, and the grid draws it.
    ///
    /// Cleared by the restart that replaces the session, so a pane that is
    /// dialling again never shows the card it was raised from.
    pub ended: bool,
    /// Which local PTY this pane is currently listening to. Bumped every
    /// time one is wired in; `LocalPaneEnded` carries the value it was
    /// armed with, so the exit of a PTY this pane has already replaced
    /// is discarded instead of declaring a live shell dead. Unused by
    /// remote panes, which have a transport handle to test instead.
    pub local_generation: u64,
    /// True while a one-shot `TerminalSyncFlush` timer is armed for this
    /// pane. A DEC `?2026` synchronized update buffers output in vte until
    /// the matching ESU, a 2 MiB overflow, or a host-driven flush; an app
    /// that opens one and then blocks on input (docker compose's `(y/N)`
    /// prompt) would otherwise freeze the screen on the pre-update frame.
    /// The flag is the rising-edge guard so a long sync burst (one
    /// `PtyOutput` per coalesced batch) arms a single timer, not one each.
    pub sync_flush_scheduled: bool,
    /// Latest window title the shell set via OSC 0/2 (`None` once an OSC
    /// ResetTitle, or never set). When auto-title is on, the tab strip shows
    /// this instead of the connection label so a tab reads as the running
    /// program / remote prompt, like every other terminal.
    pub osc_title: Option<String>,
    /// True while the visual bell flash is showing on this pane (bell mode =
    /// Flash). Set when the shell rings, cleared by a short
    /// `TerminalBellFlashEnd` timer; drives a brief overlay in the widget.
    pub bell_flash: bool,
    /// Working directory the shell last reported via OSC 7, or (fallback)
    /// parsed from the OSC 0/2 title when the shell has no OSC 7
    /// integration (default Debian/Ubuntu PS1 titles `\u@\h: \w`, so the
    /// title carries the cwd, possibly `~`-relative). Used by the sidebar
    /// Files follow and so a new local shell can open in the focused
    /// pane's directory.
    pub cwd: Option<String>,
    /// True once a real OSC 7 report arrived; from then on the title
    /// fallback is ignored (OSC 7 is exact, titles are a heuristic).
    pub cwd_from_osc7: bool,
    /// Where the remote shell stands in the OSC 133 prompt cycle, driven by
    /// the marks drained per output batch. Gates the command-history capture:
    /// only input submitted while `AtPrompt` can be a command; everything
    /// else is a running program's stdin (sudo passwords, editor keystrokes)
    /// and is never recorded.
    pub prompt: PromptState,
    /// Mirror of the remote line editor, fed with every byte of user input
    /// so the capture knows what was on the command line at Enter.
    pub input_tracker: oryxis_terminal::InputTracker,
    /// A command submitted at the prompt whose echo had not reached the grid
    /// yet (paste with a trailing newline). Resolved when `OutputStart`
    /// arrives, at which point the echoed line is read back from the grid.
    pub pending_capture: Option<PendingCapture>,
    /// In-band command capture (`OSC 633 ; E`) for this pane.
    pub inband: InbandCapture,
    /// Latest OSC 9;4 progress the shell reported, drawn as a growing border
    /// around the tab. `None` (or state 0) means no active progress.
    pub progress: Option<oryxis_terminal::Progress>,
    /// Smart tabs: the command currently running here, stamped at the OSC
    /// 133 `OutputStart` mark and resolved at `CommandEnd` / next prompt.
    /// Only integrated hosts ever set one. Cleared on disconnect (a dead
    /// transport voids any in-flight timing).
    pub running_cmd: Option<crate::smart_tabs::CommandRun>,
    /// Smart tabs: the last command line the input capture saw submitted,
    /// consumed by the next `OutputStart` to label `running_cmd`.
    pub last_submitted: Option<String>,
    /// Smart tabs: why this pane's tab wants the user's eye (attention
    /// dot on the tab strip); the tab shows its panes' highest-priority
    /// cause. Cleared when the tab is viewed.
    pub attention: Option<crate::smart_tabs::TabAttention>,
    /// Instant of the last PTY output batch, driving the quiet-period
    /// (output-after-silence) detection.
    pub last_output: Option<std::time::Instant>,
    /// ZMODEM initiation sniffer, fed every output batch while NOT already
    /// transferring. Cheap (a few bytes of held-back state); it flags a
    /// `sz` / `rz` on the remote and hands over the byte stream.
    pub zmodem_detector: oryxis_zmodem::ZmodemDetector,
    /// `Some` while a login script is answering an interactive bastion
    /// on this pane (issue #122). Armed at session-ready, fed every
    /// output batch, and dropped the moment the run ends, the user
    /// types, or the session dies, so a lingering runner can never
    /// answer a prompt from the shell the user is now driving.
    pub login_script: Option<LoginScriptRun>,
    /// Per-rule trigger bookkeeping for this pane's session (C6), keyed
    /// by rule id: when it last fired, and whether the user has allowed
    /// its snippet to be sent.
    ///
    /// Session-scoped on purpose. The grant is consent to let REMOTE
    /// output type into this shell, so it dies with the session it was
    /// given for, exactly like the agent server's per-fingerprint
    /// grants die at vault lock.
    pub triggers: std::collections::HashMap<String, TriggerRuntime>,
    /// The ambiguous-width answer this pane was HANDED OVER with, set
    /// only on a mosh pane (J4).
    ///
    /// Every other pane reads the host's current setting on every output
    /// batch, so an edit applies to new output. A mosh pane cannot: the
    /// `AlacrittyScreen` inside the protocol was built with one answer at
    /// handover and there is no path to re-`set_options` it, so letting
    /// the funnel flip the PANE afterwards would leave the model and the
    /// screen it feeds disagreeing about how wide `│` is, which is the
    /// exact failure the mosh screen exists to prevent. Pinned here, so
    /// the setting behaves like encoding and TERM on a mosh host: it
    /// applies on the next connect.
    pub mosh_ambiguous_width: Option<bool>,
    /// The password prompt this pane last raised a suggestion popup for
    /// (issue #117): `(prompt text, absolute grid row)`.
    ///
    /// Reading the prompt off the grid is stateless, so EVERY output
    /// batch re-detects a prompt that is still on screen. This makes the
    /// popup edge-triggered, and it doubles as the dismissal memory: Esc
    /// leaves the signature set, so the next byte that arrives cannot
    /// undo the dismissal. The retry after a wrong password lands on a
    /// new row, which is why the row is part of the identity.
    pub password_prompt_sig: Option<(String, i64)>,
    /// `Some` while a ZMODEM transfer owns this pane's byte stream: output
    /// is diverted to the driver (not the emulator) and input is frozen.
    /// Cleared when the transfer ends, which resumes the terminal.
    pub zmodem: Option<ZmodemPane>,
    /// `Some` while an OS drag-and-drop upload runs over SFTP on this
    /// pane's session (`drop.rs`). Unlike `zmodem` this does NOT divert
    /// the byte stream: the upload rides its own subsystem channel, so
    /// the terminal stays fully interactive. Drives the same overlay
    /// card the ZMODEM transfers use.
    pub drop_upload: Option<DropUploadPane>,
    /// Files from an OS drop waiting for the ZMODEM detector: the app
    /// typed `rz -y` and, when the detector sees the remote receiver
    /// start, `begin_zmodem_transfer` consumes these instead of opening
    /// the file picker. Cleared by the detect-timeout (remote has no
    /// lrzsz) and on disconnect.
    pub pending_drop_sources: Vec<std::path::PathBuf>,
    /// Screen rect of this pane's canvas as last drawn, written by a
    /// `bounds_reporter` wrapper each frame. Read by the OS-drop router
    /// to find the pane under the cursor: a split tab can hold panes on
    /// different hosts, so "the focused pane" is not always the pane the
    /// user dropped onto.
    pub bounds: crate::widgets::BoundsCell,
    /// `HintMode::Once` bookkeeping: set once the "hold Shift to select"
    /// mouse-capture toast has fired for this pane, so it retires here.
    /// In-memory only, a fresh pane (new tab / host) starts over.
    pub mouse_hint_shown: bool,
    /// `HintMode::Once` bookkeeping: set once the "hold Ctrl and click"
    /// link toast has fired for this pane, or once the user has
    /// ctrl-clicked a link here (either way the gesture is known),
    /// retiring the hint for the pane.
    pub link_hint_shown: bool,
    /// Sidebar Files tab: the SFTP browser multiplexed on this pane's
    /// SSH session. Lazily mounted; reset on disconnect.
    pub files: PaneFiles,
    /// Scrollback find-bar (C1): true while the overlay is shown. The match
    /// set + active index live on the widget's `TerminalState.search`; this
    /// flag and `search_query` are the app-owned UI mirror so the find-bar's
    /// `text_input` renders without locking the terminal mutex in `view()`.
    pub search_open: bool,
    /// Mirror of the find-bar needle (drives the `text_input` value).
    pub search_query: String,
    /// Broadcast input (C2): while its tab is armed (`TerminalTab.broadcast`),
    /// a pane with this set is excluded, staying an observer. Cleared when the
    /// tab disarms so a later re-arm starts clean.
    pub broadcast_opt_out: bool,
    /// Legacy keyboard modes + feature toggles (C5), RESOLVED for this pane's
    /// host at connect (a `None` on the connection resolves to
    /// `DEFAULT_QUIRKS`). Read on the hot key path (`key_to_named_bytes`) and
    /// by the widget (mouse / title / OSC 52 gates), so the vault is never
    /// consulted per keystroke. Local shells keep `DEFAULT_QUIRKS`.
    pub quirks: oryxis_core::models::terminal_quirks::TerminalQuirks,
    /// Whether the emulator sat on the alternate screen after the last
    /// output batch. The `PtyOutput` funnel compares it against the
    /// fresh value to edge-detect the flip, which is the closest thing
    /// to an attach/detach signal a tmux client gives: attaching draws
    /// the alternate screen, detaching leaves it. The tmux tab refreshes
    /// on that edge (issue #158) and the falling edge also retires the
    /// pane's "attached here" hint (issue #159). vim/htop flip it too;
    /// a spare listing on a visible tmux tab is the accepted cost.
    pub alt_screen: bool,
}

/// Process-wide auto-title gate (OSC 0/2). Mirrors the `LayoutDirection`
/// global: set once at boot and whenever the user toggles it, read at
/// display time by `display_label` so the per-pane `osc_title` capture stays
/// unconditional (toggling never loses the captured title, it just hides it).
///
/// Default OFF: Oryxis is connection-oriented (like PuTTY / Termius), so the
/// curated tab label ("Local Shell", the host name) is the better default than
/// the shell's `\u@\h: \w` title. Users who want emulator-style titles (the
/// running program in the tab) opt in via the Terminal setting.
static AUTO_TITLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Enable/disable showing the shell-set OSC title in the tab strip.
pub(crate) fn set_auto_title(on: bool) {
    AUTO_TITLE.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Whether the tab strip shows the shell-set OSC title (the user setting).
pub(crate) fn auto_title_enabled() -> bool {
    AUTO_TITLE.load(std::sync::atomic::Ordering::Relaxed)
}

/// Process-wide default AI chat mode for freshly created tabs. Mirrors the
/// `AUTO_TITLE` pattern: set once at boot and whenever the user changes the
/// "Default mode" setting, read in `TerminalTab::new_single` so every tab
/// starts on the user's chosen default without threading it through every
/// construction site. Stored as the `ChatMode` discriminant (0 = Plan,
/// 1 = Ask, 2 = Auto).
static DEFAULT_CHAT_MODE: std::sync::atomic::AtomicU8 =
    std::sync::atomic::AtomicU8::new(2);

/// Set the default chat mode applied to new tabs.
pub(crate) fn set_default_chat_mode(mode: crate::state::ChatMode) {
    let v = match mode {
        crate::state::ChatMode::Plan => 0,
        crate::state::ChatMode::Ask => 1,
        crate::state::ChatMode::Auto => 2,
    };
    DEFAULT_CHAT_MODE.store(v, std::sync::atomic::Ordering::Relaxed);
}

/// The default chat mode for a new tab (the user's "Default mode" setting).
pub(crate) fn default_chat_mode() -> crate::state::ChatMode {
    match DEFAULT_CHAT_MODE.load(std::sync::atomic::Ordering::Relaxed) {
        0 => crate::state::ChatMode::Plan,
        1 => crate::state::ChatMode::Ask,
        _ => crate::state::ChatMode::Auto,
    }
}

impl Pane {
    pub fn new(label: String, terminal: Arc<Mutex<TerminalState>>) -> Self {
        Self {
            id: Uuid::new_v4(),
            label,
            terminal,
            session: None,
            purpose: PanePurpose::default(),
            connecting: false,
            session_log_id: None,
            session_log_buf: Vec::new(),
            session_log_t0: None,
            session_log_marks: Vec::new(),
            session_log_resizes: Vec::new(),
            session_log_last_size: None,
            session_log_file: None,
            origin: PaneOrigin::Ephemeral,
            ended: false,
            local_generation: 0,
            sync_flush_scheduled: false,
            osc_title: None,
            bell_flash: false,
            cwd: None,
            cwd_from_osc7: false,
            prompt: PromptState::NoIntegration,
            input_tracker: oryxis_terminal::InputTracker::new(),
            pending_capture: None,
            inband: InbandCapture::default(),
            progress: None,
            running_cmd: None,
            last_submitted: None,
            attention: None,
            last_output: None,
            zmodem_detector: oryxis_zmodem::ZmodemDetector::new(),
            login_script: None,
            triggers: std::collections::HashMap::new(),
            mosh_ambiguous_width: None,
            password_prompt_sig: None,
            zmodem: None,
            drop_upload: None,
            pending_drop_sources: Vec::new(),
            bounds: crate::widgets::new_bounds_cell(),
            mouse_hint_shown: false,
            link_hint_shown: false,
            files: PaneFiles::default(),
            search_open: false,
            search_query: String::new(),
            broadcast_opt_out: false,
            // Xterm defaults until resolved for a real host at connect.
            quirks: oryxis_core::models::terminal_quirks::DEFAULT_QUIRKS,
            alt_screen: false,
        }
    }

    /// Attach a fresh session log to this pane, resetting the whole
    /// recording state. A reconnect reuses the pane, and a stale clock
    /// zero / last recorded geometry would leak the previous log's
    /// timeline into the new recording: offsets counting from the old
    /// session's first byte, and no initial resize row (the grid
    /// matches the "last recorded" size, so the change detector stays
    /// quiet and replay falls back to 80x24).
    pub fn start_session_log(&mut self, log_id: Uuid) {
        self.session_log_id = Some(log_id);
        self.session_log_buf.clear();
        self.session_log_t0 = None;
        self.session_log_marks.clear();
        self.session_log_resizes.clear();
        self.session_log_last_size = None;
        // A reconnect reuses the pane and starts a NEW recording, so the
        // mirror starts a new file too rather than appending the next
        // session onto the end of the last one.
        self.session_log_file = None;
    }
}

/// Live state of a ZMODEM transfer that has seized a pane's byte stream.
/// While present, `PtyOutput` for the pane is routed into `wire_tx`
/// (the driver's input) instead of the emulator, and keyboard input is
/// suppressed; the fields below drive the progress overlay.
pub(crate) struct ZmodemPane {
    pub direction: oryxis_zmodem::Direction,
    /// Feeds diverted terminal output into the transfer driver.
    pub wire_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    /// Set to request a cooperative cancel (drives a ZCAN).
    pub abort: Arc<std::sync::atomic::AtomicBool>,
    /// Current file name (once the peer advertises it).
    pub file_name: Option<String>,
    /// `(k, n)` on a multi-file upload; `None` for single files and
    /// downloads.
    pub batch: Option<(usize, usize)>,
    /// Bytes moved so far, and the advertised total when known.
    pub transferred: u64,
    pub total: Option<u64>,
    /// Output that arrived after the driver ended but before its
    /// terminal `Progress` cleared the divert (the `wire_tx` send
    /// fails once the driver drops its receiver). Replayed into the
    /// emulator at teardown so a fast prompt is never swallowed.
    pub late: Vec<u8>,
}

/// Live state of an OS-drop upload running over SFTP on a pane's
/// session. Same overlay card as [`ZmodemPane`], different plumbing: the
/// upload rides its own subsystem channel, so nothing is diverted and
/// the terminal stays interactive throughout.
pub(crate) struct DropUploadPane {
    /// Current top-level entry being sent (file or folder root).
    pub file_name: Option<String>,
    /// `(k, n)` position across the drop's top-level entries.
    pub batch: Option<(usize, usize)>,
    /// Bytes moved so far across the whole drop, and the pre-walked
    /// total (known up front, unlike ZMODEM's advertised sizes).
    pub transferred: u64,
    pub total: Option<u64>,
    /// Set by the overlay's Cancel; the upload task checks it on its
    /// progress tick, aborts the in-flight file and removes the partial.
    pub abort: Arc<std::sync::atomic::AtomicBool>,
    /// Remote directory the drop lands in. Read at `Done` to refresh
    /// the sidebar Files browser when it is showing this directory, so
    /// the uploaded entries appear without a manual refresh.
    pub dest_dir: String,
    /// Context parked while a destination conflict paused the upload
    /// and the overwrite modal is up; the resolve handler resumes the
    /// upload with it once the user answers.
    pub paused: Option<DropUploadPaused>,
}

/// Everything the resume handler needs to continue a drop upload after
/// the user answered an overwrite prompt. The SFTP client is NOT kept:
/// it is re-opened from the pane's live session on resume, so this stays
/// `Debug + Clone` (it rides `DropProgress`).
#[derive(Debug, Clone)]
pub(crate) struct DropUploadPaused {
    /// Remaining top-level plans; the first entry's first item is the
    /// conflicted file the answer applies to.
    pub plans: Vec<(String, Vec<crate::state::TransferItem>)>,
    /// Bytes already transferred before the pause.
    pub completed: u64,
    /// 0-based position of the paused entry across the whole drop, so
    /// the resume keeps the same displayed `(k, n)` batch position.
    pub index: usize,
    pub of: usize,
    /// Remote directory the drop lands in (kept so the resume stream
    /// can refresh the sidebar Files browser at `Done`).
    pub dest_dir: String,
    pub temp_name: bool,
}

/// Progress events streamed by the OS-drop SFTP upload task
/// (`begin_drop_sftp_upload`) back to the update loop. One terminal
/// event (`Done` / `Failed` / `Cancelled`) is guaranteed; it clears
/// [`Pane::drop_upload`] and toasts the outcome.
#[derive(Debug, Clone)]
pub(crate) enum DropProgress {
    /// Emitted once after the local walk: the whole drop's byte total.
    Plan { total: u64 },
    /// A top-level entry started uploading. `(k, n)` across entries.
    Entry { name: String, index: usize, of: usize },
    /// Cumulative bytes moved across the whole drop.
    Advanced { transferred: u64 },
    /// A destination file already exists: the upload paused and the
    /// overwrite modal is up. `paused` is what the resolve handler
    /// resumes with once the user answers.
    Conflict {
        prompt: Box<crate::state::OverwritePrompt>,
        item: crate::state::TransferItem,
        paused: DropUploadPaused,
    },
    Done,
    Failed(String),
    Cancelled,
}

#[cfg(test)]
mod trigger_tests {
    use super::{TriggerRuntime, TRIGGER_COOLDOWN};
    use std::time::Instant;

    #[test]
    fn a_rule_fires_once_and_then_waits_out_its_cooldown() {
        // The case this exists for: `tail -f` on a log that repeats the
        // same word. One notification, not one per line.
        let mut rt = TriggerRuntime::default();
        let t0 = Instant::now();
        assert!(rt.take_turn(t0));
        assert!(!rt.take_turn(t0));
        assert!(!rt.take_turn(t0 + TRIGGER_COOLDOWN - std::time::Duration::from_millis(1)));
        assert!(rt.take_turn(t0 + TRIGGER_COOLDOWN));
    }

    #[test]
    fn a_refusal_is_remembered_and_a_grant_is_too() {
        // Both answers stick for the session: a rule that could re-ask
        // on the next matching line would be a way to wear the user
        // down, and a grant that expired per line would be a dialog
        // storm.
        let mut rt = TriggerRuntime::default();
        assert_eq!(rt.snippet_allowed, None);
        rt.snippet_allowed = Some(false);
        assert_eq!(rt.snippet_allowed, Some(false));
        rt.snippet_allowed = Some(true);
        assert_eq!(rt.snippet_allowed, Some(true));
    }
}
