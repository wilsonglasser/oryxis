//! Terminal tabs and panes (split out of `state.rs`).

use super::*;
use crate::messages::CloudMessage;

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
}

impl TerminalTransport {
    /// The inner SSH session, for the SSH-only feature paths.
    pub fn ssh(&self) -> Option<&Arc<SshSession>> {
        match self {
            TerminalTransport::Ssh(s) => Some(s),
            TerminalTransport::Telnet(_) | TerminalTransport::Serial(_) => None,
        }
    }

    pub fn write(&self, data: &[u8]) -> Result<(), String> {
        match self {
            TerminalTransport::Ssh(s) => s.write(data).map_err(|e| e.to_string()),
            TerminalTransport::Telnet(s) => s.write(data).map_err(|e| e.to_string()),
            TerminalTransport::Serial(s) => s.write(data).map_err(|e| e.to_string()),
        }
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        match self {
            TerminalTransport::Ssh(s) => s.resize(cols, rows),
            TerminalTransport::Telnet(s) => s.resize(cols, rows),
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
        }
    }

    pub fn is_alive(&self) -> bool {
        match self {
            TerminalTransport::Ssh(s) => s.is_alive(),
            TerminalTransport::Telnet(s) => s.is_alive(),
            TerminalTransport::Serial(s) => s.is_alive(),
        }
    }

    /// Tear the session down (idempotent on every arm).
    pub fn close(&self) {
        match self {
            TerminalTransport::Ssh(s) => s.close(),
            TerminalTransport::Telnet(s) => s.close(),
            TerminalTransport::Serial(s) => s.close(),
        }
    }
}

/// Sidebar Files tab state, one instance per pane: an SFTP channel
/// multiplexed on this pane's SSH session plus the browsing state.
/// The channel dies with the session, so `SshDisconnected` resets the
/// whole struct (keeping only the user's follow / hidden preferences).
#[derive(Default)]
pub(crate) struct PaneFiles {
    /// The SFTP channel on this pane's live `client::Handle`. `None`
    /// until the Files tab is first opened (mounted lazily so panes
    /// that never browse pay nothing).
    pub client: Option<SftpClient>,
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
    /// What this pane reconnects to when restored from a saved session group.
    /// Defaults to `Ephemeral`; the creating site overrides it to `Host` or
    /// `Local` when the pane is referenceable.
    pub origin: PaneOrigin,
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
    /// True once the force-OSC7 PROMPT_COMMAND was injected into this
    /// pane's shell, so toggling the setting on (and reconnects) don't
    /// stack duplicate emitters. Reset on disconnect.
    pub osc7_injected: bool,
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
            connecting: false,
            session_log_id: None,
            session_log_buf: Vec::new(),
            session_log_t0: None,
            session_log_marks: Vec::new(),
            session_log_resizes: Vec::new(),
            session_log_last_size: None,
            origin: PaneOrigin::Ephemeral,
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
            zmodem: None,
            drop_upload: None,
            pending_drop_sources: Vec::new(),
            bounds: crate::widgets::new_bounds_cell(),
            mouse_hint_shown: false,
            link_hint_shown: false,
            files: PaneFiles::default(),
            osc7_injected: false,
            search_open: false,
            search_query: String::new(),
            broadcast_opt_out: false,
            // Xterm defaults until resolved for a real host at connect.
            quirks: oryxis_core::models::terminal_quirks::DEFAULT_QUIRKS,
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
    }
}

/// The force-OSC7 setup: defines a helper that emits a BEL-terminated
/// OSC 7 (`file://host/cwd`), then registers it as a pre-prompt hook in
/// BOTH shell families so the terminal Files sidebar can follow the exact
/// cwd. `${HOSTNAME:-…}` covers shells that don't export HOSTNAME.
///
/// Works on bash AND zsh with no shell detection, by registering through
/// each shell's own mechanism and letting the other one no-op. bash reads
/// `PROMPT_COMMAND` (we prepend the helper, keeping any existing value),
/// and its `precmd_functions+=(…)` just creates an array bash never reads.
/// zsh has no `PROMPT_COMMAND` (that assignment sets an unused var) and
/// runs `precmd_functions`, the array we append the helper to. So the same
/// line lights up cwd following on either shell, and neither mechanism
/// errors in the other.
///
/// It also cleans up its own echo instead of leaving the setup text on
/// screen. An interactive shell runs through readline (raw mode), so the
/// tty echoes what we send and no `stty` trick can suppress it; and we
/// can't send raw control bytes as input (readline would interpret them).
/// So we send two ordinary-text commands in one write. The first, `printf
/// '\x1b7'`, saves the cursor (DECSC) at the clean prompt baseline, before
/// the big line below echoes. The second defines + registers the helper,
/// then `printf '\x1b8\x1b[1A\x1b[J'` restores the cursor (DECRC), steps
/// over the tiny first line, and erases to the end of the screen. That
/// wipes the whole echoed block regardless of how many rows it wrapped to,
/// without touching the MOTD above it. Only literal backslash escapes are
/// sent; printf turns them into the real control bytes at run time, so the
/// remote line editor only ever sees plain text. The DECSC/DECRC bytes use
/// `\x1b` hex (bounded to two hex digits) rather than octal `\033`, because
/// the octal form would merge the trailing `7` of DECSC into the escape
/// (`\0337` parses as one octal byte, not ESC + `7`); `\x1b` is safe here
/// since the feature is bash/zsh-only and both printf builtins accept
/// `\xHH`.
pub(crate) const OSC7_PROMPT_INJECT: &str = "printf '\\x1b7'\n\
     __oryxis_o7(){ printf '\\033]7;file://%s%s\\007' \
     \"${HOSTNAME:-$(hostname 2>/dev/null)}\" \"$PWD\"; }; \
     PROMPT_COMMAND=\"__oryxis_o7${PROMPT_COMMAND:+;$PROMPT_COMMAND}\"; \
     precmd_functions+=(__oryxis_o7); printf '\\x1b8\\x1b[1A\\x1b[J'\n";

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
    Done,
    Failed(String),
    Cancelled,
}

/// A terminal tab. Its panes live in an iced `pane_grid::State`, which owns
/// the split layout (N-way horizontal / vertical splits) and resizing. A
/// fresh tab has exactly one pane; the user can split it.
pub(crate) struct TerminalTab {
    pub _id: Uuid,
    pub label: String,
    /// User-set tab name from "Rename tab". Transient by design: it lives
    /// for this tab's lifetime only, is never written to the host or the
    /// pin spec, and wins over every automatic label source (session
    /// group, OSC title, pane label) in `display_label`. `None` = auto.
    pub custom_name: Option<String>,
    /// The pane tree (1+ panes). `pane_grid` owns the geometry.
    pub pane_grid: pane_grid::State<Pane>,
    /// Handle of the currently focused pane. Kept valid by the split /
    /// close / focus handlers; `active()` falls back to the first pane if
    /// it ever goes stale so we never index a closed pane.
    pub focused: pane_grid::Pane,
    /// AI chat history for this terminal session.
    pub chat_history: Vec<ChatMessage>,
    /// Whether the terminal sidebar is visible (Chat / Snippets / History
    /// tabs share this flag; the active tab is `Oryxis::terminal_sidebar_tab`).
    pub chat_visible: bool,
    /// First-token allow-list for AI tool execution. Populated when the
    /// user clicks "ALWAYS RUN" on a confirmation prompt, future tool
    /// calls whose first whitespace-delimited token matches an entry
    /// here skip the prompt and run immediately. Per-tab so an
    /// "always run rm" decision on one host doesn't leak to others.
    pub chat_always_run_commands: Vec<String>,
    /// Commands auto-executed by the AI (judge-approved or allow-listed)
    /// since the last user message. A proposed command already in this
    /// list is refused auto-execution and surfaced for explicit approval
    /// instead, the guard that stops the model re-running the same
    /// command (e.g. `docker --version`) forever. Cleared whenever the
    /// user retakes control (new message, reset, or an explicit approval).
    pub chat_auto_run_history: Vec<String>,
    /// Count of consecutive AI-auto-executed commands since the last user
    /// message. A backstop for the "many different commands" runaway that
    /// exact-repeat detection can't catch: once it passes
    /// `CHAT_AUTO_RUN_STREAK_MAX` further auto-exec is refused and the
    /// command is surfaced for explicit approval. Reset alongside
    /// `chat_auto_run_history`.
    pub chat_auto_run_streak: usize,
    /// True while a chat stream (assistant reply or a tool-followup
    /// pipeline) is in flight for THIS tab. Per-tab, not global: a chat on
    /// one tab keeps streaming while the user works in another, and the
    /// "Thinking..."/Stop affordances read the active tab's flag.
    pub chat_loading: bool,
    /// Abort handle for this tab's in-flight chat stream (reply + any
    /// detached tool-followup it feeds). Aborting drops the receiver so the
    /// detached tokio task's `tx.send` fails and it stops too. Per-tab so
    /// Stop / close / reset target the right conversation and starting a
    /// chat on one tab never cancels another's. `None` when idle.
    pub chat_task: Option<iced::task::Handle>,
    /// How this tab's assistant gates tool calls: `Auto` (allow-list +
    /// judge auto-exec safe commands), `Ask` (every command needs explicit
    /// approval), or `Plan` (read-only investigation only, writes blocked).
    /// Per-tab so it travels with the conversation; seeded from the global
    /// `ai_default_mode` setting when the tab is created.
    pub chat_mode: crate::state::ChatMode,
    /// Last time the streaming markdown re-parse ran for this tab. Throttles
    /// the O(content) parse to ~10/s during streaming. Per-tab (not a single
    /// global) because two tabs can stream at once now: a shared static would
    /// see alternating tab ids and never throttle, re-parsing every chunk.
    pub chat_last_md_parse: Option<std::time::Instant>,
    /// Row id of this conversation in `chat_conversations`, minted the
    /// first time a turn is saved. `None` until then, so a tab whose chat
    /// was never used leaves nothing behind.
    pub chat_saved_id: Option<Uuid>,
    /// How many entries of `chat_history` are already in the vault.
    /// Persistence is append-only, so this is where the next flush starts.
    /// The history can also SHRINK (an empty assistant placeholder or a
    /// pending-tool bubble is popped), which is why the flush compares
    /// against the current length instead of trusting this blindly.
    pub chat_persisted: usize,
    /// True for cloud SSM / ECS-Exec tabs (a `session-manager-plugin`
    /// PTY). These talk SSM over a websocket whose idle timer kills the
    /// session after ~20 min of inactivity, so they get the
    /// resize-based keepalive while the window is unfocused. Plain SSH /
    /// local tabs leave this `false`.
    pub ssm_keepalive: bool,
    /// Message that re-creates this session, for "Duplicate Tab". Set
    /// only for cloud tabs that have no saved `Connection` to look up
    /// by label (ECS Exec, kubectl pod). SSH / InstanceConnect / SSM
    /// tabs are connection-backed and duplicate via label lookup
    /// instead, so they leave this `None`.
    pub relaunch: Option<Box<crate::messages::Message>>,
    /// Set when this tab was opened from a saved session group (or just
    /// saved as one). Drives the tab context menu label ("Save group" vs
    /// "Edit group") and lets the editor update the existing group in place.
    pub session_group_id: Option<Uuid>,
    /// Pinned tabs render first in the strip (compact icon chip or a
    /// bordered tab, per the `pinned_tab_style` setting) and are restored on
    /// the next launch. Toggled from the tab context menu.
    pub pinned: bool,
    /// Set on a *dormant* pinned tab recreated at boot: the tab shows in the
    /// strip but isn't connected. The first time it's selected, this spec
    /// reopens it (connect host / spawn local shell), then clears. `None` on
    /// a live tab.
    pub pending_reopen: Option<PinnedTabSpec>,
    /// Hybrid tab state (issue #61): when set, this SSH tab shows its
    /// host's files (the full dual-pane SFTP surface) instead of the
    /// terminal. The PTY keeps running underneath; the tab glyph /
    /// status-bar segment / hotkey toggle it back.
    pub files_mode: bool,
    /// Parked SFTP browsing state for `files_mode`, hoisted into the
    /// live `Oryxis::sftp` buffer while this tab owns the surface
    /// (`hybrid_sftp_owner`), same swap-on-focus invariant as the
    /// standalone `SftpTab::state`. Boxed: most tabs never browse.
    pub files_state: Box<SftpState>,
    /// Broadcast input (C2): while true, every keystroke / paste / snippet
    /// injection fans out to ALL of this tab's panes at once (minus panes
    /// that opted out or are mid-ZMODEM), for running the same commands on
    /// several hosts. Session-scoped: not persisted, reset on teardown; a
    /// single-pane tab may arm it but it does nothing until the tab splits.
    pub broadcast: bool,
}

/// Where the copy a "Duplicate Tab" spawns lands in the strip
/// (`duplicate_tab_position` setting). Ordering only: this never decides
/// anything about `Oryxis::tabs`, whose indices are load-bearing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum TabPlacement {
    /// Immediately after the tab it was duplicated from, so a copy made
    /// from a tab early in a long strip stays next to its source instead
    /// of landing off-screen at the far end (PR #110's report).
    #[default]
    NextToOriginal,
    /// The historical behaviour: appended at the far end of the strip.
    End,
    /// At the head of the strip (after the pinned partition).
    Start,
}

impl TabPlacement {
    /// Parse the `duplicate_tab_position` setting value; anything
    /// unrecognized falls back to the default, mirroring
    /// `TabBarPos::from_setting`.
    pub(crate) fn from_setting(v: &str) -> Self {
        match v {
            "end" => Self::End,
            "start" => Self::Start,
            _ => Self::NextToOriginal,
        }
    }
}

/// A Duplicate whose new tab has not been born yet.
///
/// The copy is spawned by re-dispatching the source tab's own open
/// message (`ConnectSsh` / `OpenLocalShell` / a cloud `relaunch`), and
/// for cloud tabs that answer lands several updates later, so the
/// placement has to be remembered rather than applied inline.
///
/// It is deliberately a STRIP placement keyed by tab id, not an index
/// into `Oryxis::tabs`: `active_tab`, `last_terminal_tab`,
/// `connecting.tab_idx` and `pending_pane_split` all hold positions in
/// that vec, and every removal path fixes them up by hand
/// (`teardown_tab_at`, `adjust_last_terminal_tab_after_remove`).
/// Inserting into the middle of it would silently invalidate all four,
/// so the copy is appended like any other tab and only its `tab_order`
/// entry moves.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PendingTabPlacement {
    /// The tab that was duplicated. An id, so closing or reordering
    /// tabs while the copy is still connecting cannot repoint it.
    pub(crate) source_id: Uuid,
    pub(crate) placement: TabPlacement,
    /// When it was armed, for [`Self::is_expired`].
    pub(crate) armed_at: std::time::Instant,
}

impl PendingTabPlacement {
    /// A duplicate that never produces a tab (the cloud plugin fails to
    /// start, the PTY refuses to spawn) would otherwise leave this armed
    /// forever and reposition some unrelated tab opened minutes later.
    /// Nothing legitimate takes this long: even a cloud session that has
    /// to download its plugin answers well inside it.
    const TTL: std::time::Duration = std::time::Duration::from_secs(20);

    pub(crate) fn is_expired(&self) -> bool {
        self.armed_at.elapsed() > Self::TTL
    }
}

/// Reference to an open tab in the unified strip. Terminal and SFTP tabs
/// share one reorderable, pinnable row; identity is by `Uuid` (stable
/// across reorder / close) rather than a vec index. Reserved for the full
/// cross-type interleave / drag-reorder (deferred): SFTP tabs render grouped
/// after terminal tabs today, so `Terminal` is not yet constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum TabRef {
    Terminal(Uuid),
    Sftp(Uuid),
    /// The Settings tab (issue #120). Carries no id because there is at
    /// most one: Settings is a single global surface, so a second entry
    /// would be the same screen twice. `Oryxis::settings_tab_open` says
    /// whether it is in the strip; `tab_order` says where.
    Settings,
}

/// Stable synthetic id the Settings tab answers with inside the
/// uuid-keyed strip machinery (drag / live-slide / reorder). `TabRef`
/// itself keeps no id, because there is only ever one Settings tab; this
/// constant is what lets it ride the same reorder code as every other
/// tab instead of needing a parallel path.
pub(crate) const SETTINGS_TAB_ID: Uuid = Uuid::from_u128(0x5E11_1465_0000_0000_0000_0000_0000_0001);

impl TabRef {
    /// Id used by the reorder machinery. Real for terminal / SFTP tabs,
    /// the synthetic `SETTINGS_TAB_ID` for Settings.
    pub(crate) fn strip_id(&self) -> Uuid {
        match self {
            TabRef::Terminal(id) | TabRef::Sftp(id) => *id,
            TabRef::Settings => SETTINGS_TAB_ID,
        }
    }
}

/// An SFTP browser tab. Unlike terminal tabs, the **active** SFTP tab's
/// live state lives in `Oryxis::sftp` (a working buffer); this struct's
/// `state` field is a default placeholder while this tab is focused, and
/// holds the parked state while it is not. See the swap-on-focus invariant
/// in `SFTP_TABS_PLAN.md`: never read the active tab's state from the vec,
/// route by id through `Oryxis::route_sftp_async`.
pub(crate) struct SftpTab {
    pub id: Uuid,
    pub label: String,
    /// User-set tab name from "Rename tab". Transient, mirrors
    /// `TerminalTab::custom_name`: display-only, never persisted.
    pub custom_name: Option<String>,
    /// Pinned SFTP tabs render first in the strip.
    pub pinned: bool,
    /// Set on a dormant pinned SFTP tab recreated at boot: reopens (re-mounts
    /// its panes) the first time it's selected, then clears. Reserved for
    /// pin-restore-on-boot (deferred); not read yet.
    #[allow(dead_code)]
    pub pending_reopen: Option<PinnedTabSpec>,
    /// Parked state while this tab is not focused; a default placeholder while
    /// it IS the active tab (live state hoisted to `Oryxis::sftp`).
    pub state: SftpState,
}

impl SftpTab {
    pub(crate) fn new(label: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            label,
            custom_name: None,
            pinned: false,
            pending_reopen: None,
            state: SftpState::default(),
        }
    }

    /// Label to show in the tab strip: the user's transient rename when
    /// set, else the mount label. Lookups (host colour, detected OS)
    /// must keep using `label`, the custom name is display-only.
    pub(crate) fn display_label(&self) -> &str {
        self.custom_name.as_deref().unwrap_or(&self.label)
    }
}

/// Persisted restore spec for a pinned tab. Stored as JSON in the
/// `pinned_tabs` setting; on boot each becomes a dormant pinned tab that
/// reopens lazily on first select. Cloud / ephemeral tabs have no spec and
/// aren't persisted.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum PinnedTabSpec {
    /// A saved host, reopened with `ConnectSsh` (id resolved to an index
    /// fresh at reopen time, so it survives connection reordering).
    Host { id: Uuid, label: String },
    /// A local shell, reopened with the captured program / args.
    LocalShell { program: String, args: Vec<String>, label: String },
    /// An ECS Exec session, reopened with `ConnectEcsExecTask` (same
    /// mechanism the in-session reconnect uses; the task id may have
    /// recycled, in which case the reconnect re-resolves the group).
    EcsExec {
        group_id: Uuid,
        task_id: String,
        task_label: String,
        container: String,
        label: String,
    },
    /// A kubectl exec session, reopened with `ConnectKubectlExecPod`.
    KubectlExec {
        group_id: Uuid,
        namespace: String,
        pod: String,
        container: String,
        label: String,
    },
    /// A pinned SFTP browser tab. Captures both panes (Local vs which
    /// connection); reopened dormant and re-mounts its remote pane(s) on first
    /// focus.
    Sftp {
        left: SftpPaneSpec,
        right: SftpPaneSpec,
        label: String,
    },
}

/// Restore spec for one SFTP pane: Local browsing, or a remote host by saved
/// connection id (resolved fresh at reopen so it survives reordering).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum SftpPaneSpec {
    Local,
    Remote(Uuid),
}

/// In-progress drag of a tab in the strip, for reordering. Started on press
/// (`SelectTab`), promoted to `active` once the cursor moves past a small
/// threshold (so a plain click isn't a drag), committed on mouse release
/// onto the hovered tab. Reorder is restricted to within the same group
/// (pinned among pinned, normal among normal).
#[derive(Debug, Clone, Copy)]
pub(crate) struct TabDrag {
    /// The tab being dragged, by id so it survives any reindexing (a tab
    /// closing mid-drag) and resolves to the right source at drop time.
    pub from_id: Uuid,
    /// Cursor position at press, for the move threshold.
    pub start: iced::Point,
    /// Promoted past the threshold (a real drag, not a click).
    pub active: bool,
}

impl PinnedTabSpec {
    pub fn label(&self) -> &str {
        match self {
            PinnedTabSpec::Host { label, .. } => label,
            PinnedTabSpec::LocalShell { label, .. } => label,
            PinnedTabSpec::EcsExec { label, .. } => label,
            PinnedTabSpec::KubectlExec { label, .. } => label,
            PinnedTabSpec::Sftp { label, .. } => label,
        }
    }

    /// Identity key for de-duplicating pins. Ephemeral resource ids
    /// (ECS task, K8s pod) are excluded on purpose: a recycled task
    /// produces a spec with a different task_id but it is still the
    /// same pin, and keeping both is how duplicate chips appear.
    pub fn dedupe_key(&self) -> String {
        match self {
            PinnedTabSpec::Host { id, .. } => format!("host:{id}"),
            PinnedTabSpec::LocalShell { program, args, label } => {
                format!("local:{program}:{}:{label}", args.join("\u{1f}"))
            }
            PinnedTabSpec::EcsExec { group_id, container, .. } => {
                format!("ecs:{group_id}:{container}")
            }
            PinnedTabSpec::KubectlExec { group_id, namespace, container, .. } => {
                format!("k8s:{group_id}:{namespace}:{container}")
            }
            PinnedTabSpec::Sftp { left, right, .. } => {
                let key = |p: &SftpPaneSpec| match p {
                    SftpPaneSpec::Local => "local".to_string(),
                    SftpPaneSpec::Remote(id) => format!("remote:{id}"),
                };
                format!("sftp:{}:{}", key(left), key(right))
            }
        }
    }
}

impl TerminalTab {
    /// Build a new tab with a single pane. Split it later via
    /// `pane_grid.split(...)`.
    pub fn new_single(label: String, terminal: Arc<Mutex<TerminalState>>) -> Self {
        let (pane_grid, focused) = pane_grid::State::new(Pane::new(label.clone(), terminal));
        Self {
            _id: Uuid::new_v4(),
            label,
            custom_name: None,
            pane_grid,
            focused,
            chat_history: Vec::new(),
            chat_visible: false,
            chat_always_run_commands: Vec::new(),
            chat_auto_run_history: Vec::new(),
            chat_auto_run_streak: 0,
            chat_loading: false,
            chat_task: None,
            chat_mode: default_chat_mode(),
            chat_last_md_parse: None,
            chat_saved_id: None,
            chat_persisted: 0,
            ssm_keepalive: false,
            relaunch: None,
            session_group_id: None,
            pinned: false,
            pending_reopen: None,
            files_mode: false,
            files_state: Box::default(),
            broadcast: false,
        }
    }

    /// A dormant pinned tab recreated at boot: shows in the strip with the
    /// saved label but holds no live session. The placeholder pane carries a
    /// hint; selecting the tab the first time fires `spec` to reopen it.
    pub fn new_dormant_pinned(label: String, spec: PinnedTabSpec) -> Self {
        let mut term = TerminalState::new_no_pty(80, 24).unwrap();
        let hint = format!("\x1b[2m  {}\x1b[0m\r\n", crate::i18n::t("pinned_tab_dormant_hint"));
        term.process(hint.as_bytes());
        let mut tab = Self::new_single(label, Arc::new(Mutex::new(term)));
        tab.pinned = true;
        tab.pending_reopen = Some(spec);
        tab
    }

    /// Restore spec for persisting this pinned tab, or `None` if it can't be
    /// reopened (cloud / ephemeral pane with no stable reference). A dormant
    /// tab keeps the spec it was created with; a live tab derives one from
    /// its focused pane's origin.
    pub fn pin_spec(&self) -> Option<PinnedTabSpec> {
        if let Some(spec) = &self.pending_reopen {
            return Some(spec.clone());
        }
        let base = self.label.trim_end_matches(" (disconnected)").to_string();
        match &self.active().origin {
            PaneOrigin::Host(id) => Some(PinnedTabSpec::Host { id: *id, label: base }),
            // Quick-connect hosts have no stable reference to restore from
            // (the entry dies with the app), so the pin is session-only,
            // like SSM tabs.
            PaneOrigin::QuickHost(_) => None,
            PaneOrigin::Local(spec) => Some(PinnedTabSpec::LocalShell {
                program: spec.program.clone(),
                args: spec.args.clone(),
                label: spec.label.clone(),
            }),
            // Cloud exec tabs have no saved Connection, but carry the
            // relaunch message that recreates them; mirror it into a
            // serializable spec. SSM (relaunch None) and anything else stay
            // unpersisted.
            PaneOrigin::Ephemeral => match self.relaunch.as_deref() {
                Some(crate::messages::Message::Cloud(CloudMessage::ConnectEcsExecTask {
                    group_id,
                    task_id,
                    task_label,
                    container,
                })) => Some(PinnedTabSpec::EcsExec {
                    group_id: *group_id,
                    task_id: task_id.clone(),
                    task_label: task_label.clone(),
                    container: container.clone(),
                    label: base,
                }),
                Some(crate::messages::Message::Cloud(CloudMessage::ConnectKubectlExecPod {
                    group_id,
                    namespace,
                    pod,
                    container,
                })) => Some(PinnedTabSpec::KubectlExec {
                    group_id: *group_id,
                    namespace: namespace.clone(),
                    pod: pod.clone(),
                    container: container.clone(),
                    label: base,
                }),
                _ => None,
            },
        }
    }

    /// Move focus to the adjacent pane in `dir`, carrying the zoom with
    /// it. Returns whether focus moved.
    ///
    /// The zoom is the reason this is a method rather than three lines in
    /// the dispatcher: while a pane is maximized the grid renders only
    /// that one, so moving focus without moving the zoom would put the
    /// caret on a pane nobody can see. Carrying it means the directional
    /// keys stay the way to walk the panes whether or not one is zoomed,
    /// which is what makes a separate pane list unnecessary (#113). The
    /// harness cannot reach arrow chords (its grammar takes a single
    /// character after the modifiers), so this is covered by the tests
    /// below rather than by an `.ice`.
    pub fn focus_adjacent(&mut self, dir: pane_grid::Direction) -> bool {
        let Some(adj) = self.pane_grid.adjacent(self.focused, dir) else {
            return false;
        };
        self.focused = adj;
        if self.pane_grid.maximized().is_some() {
            self.pane_grid.maximize(adj);
        }
        true
    }

    /// Zoom the focused pane to the whole tab, or restore the split.
    ///
    /// A lone pane already fills the tab, so zooming it would change
    /// nothing except hide the affordance that undoes it.
    pub fn toggle_maximize(&mut self) {
        if self.pane_grid.maximized().is_some() {
            self.pane_grid.restore();
        } else if self.pane_grid.panes.len() > 1 {
            self.pane_grid.maximize(self.focused);
        }
    }

    /// Currently focused pane. Falls back to the first pane if `focused`
    /// is stale (e.g. just after a close), so this never panics.
    pub fn active(&self) -> &Pane {
        self.pane_grid
            .get(self.focused)
            .or_else(|| self.pane_grid.panes.values().next())
            .expect("a tab always has at least one pane")
    }

    pub fn active_mut(&mut self) -> &mut Pane {
        // Resolve a valid key first (repairing `focused` if it went
        // stale), then take the mutable borrow.
        let key = if self.pane_grid.panes.contains_key(&self.focused) {
            self.focused
        } else {
            let k = *self
                .pane_grid
                .panes
                .keys()
                .next()
                .expect("a tab always has at least one pane");
            self.focused = k;
            k
        };
        self.pane_grid.get_mut(key).expect("valid pane key")
    }

    /// Look up a pane by its stable `Uuid` (for routing PTY output /
    /// session events).
    pub fn pane_by_id_mut(&mut self, id: Uuid) -> Option<&mut Pane> {
        self.pane_grid.panes.values_mut().find(|p| p.id == id)
    }

    /// Number of panes in this tab. `> 1` means the tab is split.
    pub fn pane_count(&self) -> usize {
        self.pane_grid.panes.len()
    }

    /// Broadcast input (C2): the pane ids a user-input write reaches. When
    /// armed, every participating pane (not opted out, not mid-ZMODEM); when
    /// disarmed, only the active pane (unless it is mid-ZMODEM, which owns its
    /// byte channel). The single routing source of truth, shared by the write
    /// funnel and its test. `files_mode` suppression is the caller's early
    /// return, not modeled here.
    pub fn broadcast_target_ids(&self) -> Vec<Uuid> {
        if self.broadcast {
            self.pane_grid
                .panes
                .values()
                .filter(|p| !p.broadcast_opt_out && p.zmodem.is_none())
                .map(|p| p.id)
                .collect()
        } else {
            let active = self.active();
            if active.zmodem.is_some() {
                Vec::new()
            } else {
                vec![active.id]
            }
        }
    }

    /// Label to show in the tab strip. A tab opened from (or saved as) a
    /// session group shows the group's name. Otherwise a split tab follows
    /// the *focused* pane (so a tab split across two hosts reads as whichever
    /// pane you're in); a single-pane tab uses the tab's own label, which
    /// carries the "(disconnected)" suffix the focused-pane label doesn't.
    /// Label to show in the tab strip. `auto_title` is the effective per-tab
    /// auto-title decision (resolved by the caller from the focused host's
    /// override and the global default), kept as a parameter because a
    /// `TerminalTab` can't reach the connection list to resolve it itself.
    pub fn display_label(&self, auto_title: bool) -> &str {
        // An explicit rename wins over every automatic source: the user
        // asked for this exact name, so neither the group name nor a
        // shell-set OSC title may overwrite it.
        if let Some(name) = self.custom_name.as_deref() {
            return name;
        }
        self.auto_label(auto_title)
    }

    /// Re-anchor the tab's own label on the pane that is left when a
    /// split collapses back to a single pane.
    ///
    /// `auto_label` reads the FOCUSED PANE's label while a tab is split
    /// but falls back to `self.label` once it is not, and `self.label` is
    /// fixed when the tab is created. So closing the first pane of a
    /// two-pane tab left the tab wearing the name of the pane that just
    /// went away, while showing the survivor's terminal (issue #108).
    ///
    /// This repairs the tab's identity rather than just its caption:
    /// `self.label` is also what the host-accent and detected-OS lookups
    /// key on, so a stale value mismatched those too.
    ///
    /// No-op while the tab is still split (the focused pane's label is
    /// what shows then) or when the survivor carries no label of its own.
    pub fn sync_label_to_sole_pane(&mut self) {
        if self.pane_count() != 1 {
            return;
        }
        let Some(survivor) = self.pane_grid.panes.values().next() else {
            return;
        };
        if survivor.label.is_empty() {
            return;
        }
        self.label = survivor.label.clone();
    }

    /// The automatic label, ignoring any user rename. This is what
    /// lookups (host accent, detected-OS badge) key on: a custom name is
    /// display-only and must never leak into a `Connection`-by-label
    /// match.
    pub fn auto_label(&self, auto_title: bool) -> &str {
        // A session group keeps its own name; OSC titles never override it.
        if self.session_group_id.is_some() {
            return &self.label;
        }
        // The focused pane's shell-set title wins when auto-title is on, so
        // the tab reads as the running program / remote prompt.
        if auto_title
            && let Some(t) = self.active().osc_title.as_deref()
            && !t.is_empty()
        {
            return t;
        }
        if self.pane_count() > 1 {
            &self.active().label
        } else {
            &self.label
        }
    }
}


#[cfg(test)]
mod terminal_tab_tests {
    use super::*;

    fn dummy_terminal() -> Arc<Mutex<TerminalState>> {
        Arc::new(Mutex::new(TerminalState::new_no_pty(80, 24).unwrap()))
    }

    fn split(tab: &mut TerminalTab, axis: pane_grid::Axis) -> pane_grid::Pane {
        let (handle, _) = tab
            .pane_grid
            .split(axis, tab.focused, Pane::new("p".into(), dummy_terminal()))
            .expect("split");
        tab.focused = handle;
        handle
    }

    /// Zooming a lone pane is refused: it already fills the tab, so the
    /// only thing it would change is hiding the way back.
    #[test]
    fn a_single_pane_tab_does_not_zoom() {
        let mut tab = TerminalTab::new_single("a".into(), dummy_terminal());
        tab.toggle_maximize();
        assert!(tab.pane_grid.maximized().is_none());
    }

    /// The toggle is a real toggle, and restoring puts the split back
    /// untouched: `maximize` only changes what is DRAWN, never the layout.
    #[test]
    fn zoom_toggles_and_restores_the_same_layout() {
        let mut tab = TerminalTab::new_single("a".into(), dummy_terminal());
        let second = split(&mut tab, pane_grid::Axis::Vertical);
        tab.toggle_maximize();
        assert_eq!(tab.pane_grid.maximized(), Some(second));
        tab.toggle_maximize();
        assert!(tab.pane_grid.maximized().is_none());
        assert_eq!(tab.pane_grid.panes.len(), 2, "both panes survive the round trip");
    }

    /// The zoom follows the focus. Without this, walking the panes while
    /// one is zoomed would move the caret to a pane the grid is not
    /// drawing, and the user would be typing into something invisible.
    #[test]
    fn zoom_follows_focus_across_panes() {
        let mut tab = TerminalTab::new_single("a".into(), dummy_terminal());
        let second = split(&mut tab, pane_grid::Axis::Vertical);
        tab.toggle_maximize();
        assert_eq!(tab.pane_grid.maximized(), Some(second));

        assert!(tab.focus_adjacent(pane_grid::Direction::Left));
        assert_ne!(tab.focused, second, "focus moved to the other pane");
        assert_eq!(
            tab.pane_grid.maximized(),
            Some(tab.focused),
            "the zoom must land on whichever pane now has focus"
        );
    }

    /// And with nothing zoomed, moving focus must not start a zoom.
    #[test]
    fn moving_focus_alone_never_zooms() {
        let mut tab = TerminalTab::new_single("a".into(), dummy_terminal());
        let _ = split(&mut tab, pane_grid::Axis::Vertical);
        assert!(tab.focus_adjacent(pane_grid::Direction::Left));
        assert!(tab.pane_grid.maximized().is_none());
    }

    /// A direction with no neighbour reports that nothing moved, so the
    /// caller can tell a no-op from a walk.
    #[test]
    fn focus_does_not_move_past_the_edge() {
        let mut tab = TerminalTab::new_single("a".into(), dummy_terminal());
        let second = split(&mut tab, pane_grid::Axis::Vertical);
        assert!(!tab.focus_adjacent(pane_grid::Direction::Right));
        assert_eq!(tab.focused, second);
    }

    /// Split a tab named after its first pane, add a second pane for a
    /// different host, then close the FIRST one. The tab is now showing the
    /// second host's terminal, so it must not keep the first host's name
    /// (issue #108): the unsplit label comes from the tab, which was named
    /// at creation.
    fn named_split(tab: &mut TerminalTab, label: &str) -> pane_grid::Pane {
        let (handle, _) = tab
            .pane_grid
            .split(
                pane_grid::Axis::Vertical,
                tab.focused,
                Pane::new(label.into(), dummy_terminal()),
            )
            .expect("split");
        tab.focused = handle;
        handle
    }

    #[test]
    fn collapsing_a_split_renames_the_tab_after_the_surviving_pane() {
        let mut tab = TerminalTab::new_single("Local Shell".into(), dummy_terminal());
        let first = tab.focused;
        named_split(&mut tab, "root@prod-db");
        // Split: the focused pane's label is what shows.
        assert_eq!(tab.auto_label(false), "root@prod-db");

        // Close the pane the tab was named after.
        let (_, sibling) = tab.pane_grid.close(first).expect("close");
        tab.focused = sibling;
        tab.sync_label_to_sole_pane();

        assert_eq!(
            tab.auto_label(false),
            "root@prod-db",
            "an unsplit tab must wear the surviving pane's name, not the closed one's"
        );
    }

    /// The repair must not fight a user rename: `custom_name` wins over
    /// every automatic source, including this one.
    #[test]
    fn collapsing_a_split_does_not_override_a_user_rename() {
        let mut tab = TerminalTab::new_single("Local Shell".into(), dummy_terminal());
        let first = tab.focused;
        named_split(&mut tab, "root@prod-db");
        tab.custom_name = Some("deploy box".into());

        let (_, sibling) = tab.pane_grid.close(first).expect("close");
        tab.focused = sibling;
        tab.sync_label_to_sole_pane();

        assert_eq!(tab.display_label(false), "deploy box");
        // The automatic label underneath still tracks the survivor.
        assert_eq!(tab.auto_label(false), "root@prod-db");
    }

    /// Still split = still the focused pane's label, so the repair must
    /// leave a multi-pane tab's own label alone.
    #[test]
    fn a_still_split_tab_keeps_its_own_label() {
        let mut tab = TerminalTab::new_single("Local Shell".into(), dummy_terminal());
        named_split(&mut tab, "root@prod-db");
        named_split(&mut tab, "root@web-01");
        tab.sync_label_to_sole_pane();
        assert_eq!(tab.label, "Local Shell", "three panes: nothing to re-anchor");
    }

    #[test]
    fn split_then_close_keeps_focused_on_a_live_pane() {
        let mut tab = TerminalTab::new_single("t".into(), dummy_terminal());
        assert_eq!(tab.pane_grid.panes.len(), 1);
        split(&mut tab, pane_grid::Axis::Vertical);
        split(&mut tab, pane_grid::Axis::Horizontal);
        assert_eq!(tab.pane_grid.panes.len(), 3);

        // Close the focused pane the way `ClosePane` does, then point
        // `focused` at the sibling that took over.
        let (_, sibling) = tab.pane_grid.close(tab.focused).expect("close");
        tab.focused = sibling;
        assert_eq!(tab.pane_grid.panes.len(), 2);

        // `active()` must resolve to one of the surviving panes, never panic.
        let active_id = tab.active().id;
        assert!(tab.pane_grid.panes.values().any(|p| p.id == active_id));
    }

    #[test]
    fn active_falls_back_when_focused_is_stale() {
        let mut tab = TerminalTab::new_single("t".into(), dummy_terminal());
        let handle = split(&mut tab, pane_grid::Axis::Vertical);
        // Close the focused pane WITHOUT repairing `focused` (simulating a
        // missed update): `active()` must still return a live pane.
        tab.pane_grid.close(handle);
        let _ = tab.active().id; // must not panic
        // `active_mut()` repairs `focused` to a valid handle.
        let id = tab.active_mut().id;
        assert!(tab.pane_grid.panes.values().any(|p| p.id == id));
    }

    #[test]
    fn pane_by_id_mut_targets_the_right_pane() {
        let mut tab = TerminalTab::new_single("t".into(), dummy_terminal());
        let id1 = tab.active().id;
        let h2 = split(&mut tab, pane_grid::Axis::Vertical);
        let id2 = tab.pane_grid.get(h2).unwrap().id;
        assert_ne!(id1, id2);
        assert_eq!(tab.pane_by_id_mut(id1).map(|p| p.id), Some(id1));
        assert_eq!(tab.pane_by_id_mut(id2).map(|p| p.id), Some(id2));
        assert!(tab.pane_by_id_mut(Uuid::new_v4()).is_none());
    }

    #[test]
    fn broadcast_target_ids_routing() {
        let mut tab = TerminalTab::new_single("t".into(), dummy_terminal());
        let id1 = tab.active().id;
        let h2 = split(&mut tab, pane_grid::Axis::Vertical);
        let h3 = split(&mut tab, pane_grid::Axis::Horizontal);
        let id2 = tab.pane_grid.get(h2).unwrap().id;
        let id3 = tab.pane_grid.get(h3).unwrap().id;
        assert_eq!(tab.pane_grid.panes.len(), 3);

        // Disarmed: only the active pane receives.
        assert!(!tab.broadcast);
        let active_id = tab.active().id;
        assert_eq!(tab.broadcast_target_ids(), vec![active_id]);

        // Armed: every pane receives.
        tab.broadcast = true;
        let mut all = tab.broadcast_target_ids();
        all.sort();
        let mut expect = vec![id1, id2, id3];
        expect.sort();
        assert_eq!(all, expect);

        // An opted-out pane is skipped while the rest still receive.
        tab.pane_by_id_mut(id2).unwrap().broadcast_opt_out = true;
        assert!(!tab.broadcast_target_ids().contains(&id2));
        assert_eq!(tab.broadcast_target_ids().len(), 2);

        // A pane mid-ZMODEM is skipped even when not opted out (its byte
        // channel belongs to the transfer).
        tab.pane_by_id_mut(id2).unwrap().broadcast_opt_out = false;
        let (wire_tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        tab.pane_by_id_mut(id3).unwrap().zmodem = Some(ZmodemPane {
            direction: oryxis_zmodem::Direction::Download,
            wire_tx,
            abort: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            file_name: None,
            batch: None,
            transferred: 0,
            total: None,
            late: Vec::new(),
        });
        let targets = tab.broadcast_target_ids();
        assert!(!targets.contains(&id3), "zmodem pane must be skipped");
        assert!(targets.contains(&id1) && targets.contains(&id2));

        // Disarmed with the active pane mid-ZMODEM: nothing receives (a
        // keystroke must never interleave with the transfer).
        tab.broadcast = false;
        tab.focused = h3;
        assert!(tab.broadcast_target_ids().is_empty());
    }

    #[test]
    fn custom_name_wins_over_every_automatic_label_source() {
        let mut tab = TerminalTab::new_single("host-a".into(), dummy_terminal());
        assert_eq!(tab.display_label(true), "host-a");

        // Custom name beats the plain label...
        tab.custom_name = Some("prod db".into());
        assert_eq!(tab.display_label(true), "prod db");
        // ...an OSC title with auto-title on...
        tab.active_mut().osc_title = Some("vim main.rs".into());
        assert_eq!(tab.display_label(true), "prod db");
        // ...and the session-group name.
        tab.session_group_id = Some(Uuid::new_v4());
        assert_eq!(tab.display_label(true), "prod db");

        // `auto_label` keeps ignoring the rename, so lookups (host
        // accent, OS badge) still key on the automatic label.
        assert_eq!(tab.auto_label(true), "host-a");

        // Clearing the name restores the automatic sources.
        tab.custom_name = None;
        tab.session_group_id = None;
        assert_eq!(tab.display_label(true), "vim main.rs");
        assert_eq!(tab.display_label(false), "host-a");
    }

    #[test]
    fn sftp_custom_name_is_display_only() {
        let mut tab = SftpTab::new("host-a".into());
        assert_eq!(tab.display_label(), "host-a");
        tab.custom_name = Some("files".into());
        assert_eq!(tab.display_label(), "files");
        assert_eq!(tab.label, "host-a");
        tab.custom_name = None;
        assert_eq!(tab.display_label(), "host-a");
    }

    #[test]
    fn osc7_inject_is_plain_text_and_self_clearing() {
        let s = OSC7_PROMPT_INJECT;
        // The remote shell runs through readline (raw mode): sending a raw
        // control byte would be interpreted as a key, not inserted. Every
        // escape must travel as literal backslash text for printf to
        // expand at run time, so the on-the-wire bytes stay printable
        // (plus the two command-terminating newlines).
        for b in s.bytes() {
            assert!(
                b == b'\n' || (b' '..=b'~').contains(&b),
                "OSC7 injection must be plain text, found control byte {b:#04x}",
            );
        }
        // Pin the exact wire bytes: this catches escaping/spacing slips in
        // the multi-line string literal (a glued `}; PROMPT_COMMAND` or a
        // dropped space would break the shell parse on connect, which we
        // can't unit-test directly).
        let expected = "printf '\\x1b7'\n\
            __oryxis_o7(){ printf '\\033]7;file://%s%s\\007' \
            \"${HOSTNAME:-$(hostname 2>/dev/null)}\" \"$PWD\"; }; \
            PROMPT_COMMAND=\"__oryxis_o7${PROMPT_COMMAND:+;$PROMPT_COMMAND}\"; \
            precmd_functions+=(__oryxis_o7); printf '\\x1b8\\x1b[1A\\x1b[J'\n";
        assert_eq!(s, expected);
        // DECSC uses hex `\x1b7`, never octal `\0337` (which printf would
        // read as a single octal byte because `7` is an octal digit).
        assert!(!s.contains(r"\0337"), "octal DECSC would merge the trailing 7");
        // Registered in both shell families: bash via PROMPT_COMMAND, zsh
        // via precmd_functions. One without the other silently drops cwd
        // following on that shell.
        assert!(s.contains("PROMPT_COMMAND=\""), "missing bash registration");
        assert!(
            s.contains("precmd_functions+=(__oryxis_o7)"),
            "missing zsh registration",
        );
        // Two commands, so two newlines: the DECSC save, then the setup
        // plus the self-clear trailer.
        assert_eq!(s.matches('\n').count(), 2, "expected exactly two commands");
    }
}
