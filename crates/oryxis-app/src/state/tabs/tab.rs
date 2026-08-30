//! A tab: the grid of panes, plus everything the strip needs to draw and
//! address one.
//!
//! `TerminalTab` owns the `pane_grid` and the zoom / focus operations
//! over it. The rest is identity and placement: how a tab is referenced
//! (`TabRef`), where a new one lands (`TabPlacement`), and what a pinned
//! one reopens as (`PinnedTabSpec`).

use super::super::*;

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
    /// Whether each terminal-sidebar region (left / right, issue #102)
    /// is open on this tab, indexed by `SidebarSide::idx()`. Which tab
    /// each region shows is `Oryxis::terminal_sidebar_tab` (also
    /// per-side); an open region with no available tabs simply doesn't
    /// render, so the remembered open survives a temporary gate loss.
    pub sidebar_open: [bool; 2],
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
    /// Message that re-creates this session, for "Duplicate Tab". Set
    /// only for tabs that have no saved `Connection` to look up
    /// by label (quick-connect shells over serial / telnet / local
    /// forwards). Connection-backed tabs duplicate via label lookup
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
    /// Pin spec this tab INHERITED by absorbing a pinned SFTP tab
    /// ("Open terminal" morphing the pair, H5). Read first by
    /// [`Self::pin_spec`], so the pin restores as the SFTP tab it was
    /// pinned as: a pin remembers what it was pinned AS, and the morph
    /// is session-only. Cleared when the user toggles the pin (an
    /// explicit re-pin is the one thing that changes it) or when the
    /// SFTP half is detached / closed again. Deliberately NOT
    /// `pending_reopen`, which means "dormant, reopen me on select".
    pub inherited_pin: Option<PinnedTabSpec>,
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

/// A tab the user closed, kept so `ReopenClosedTab` can bring it back
/// (issue #186).
///
/// The spec is the one a PIN persists, so reopening a closed tab and
/// reopening a pinned one resolve through the same code: a saved host by
/// id (immune to reordering and renaming), a local shell by program +
/// args, an SFTP tab by both its panes.
/// `pin_spec` answering `None` is what excludes a tab from the stack,
/// which keeps quick-connect out of it for the reason it is out of pins:
/// the entry dies with the tab, and holding it would mean keeping
/// credentials the user typed once alive past the session that used
/// them.
///
/// Session-only, never written to the vault. Pinning is this app's "keep
/// this tab across restarts"; an undo affordance that cost a disk write
/// per closed tab would be paying a persistence price for the one state
/// that is meant to be cheap.
#[derive(Debug, Clone)]
pub(crate) struct ClosedTab {
    pub spec: PinnedTabSpec,
    /// Strip id of the chip that sat immediately to its left, so the
    /// reopen lands back where the tab was rather than at the far end.
    /// `None` = it was the first chip. An id rather than an index because
    /// the neighbour may itself have closed since, and
    /// [`TabPlacement::NextToOriginal`] already answers "that tab is gone"
    /// by appending.
    pub after_id: Option<Uuid>,
}

/// A Duplicate whose new tab has not been born yet.
///
/// The copy is spawned by re-dispatching the source tab's own open
/// message (`ConnectSsh` / `OpenLocalShell` / a quick-connect
/// `relaunch`), and for those the answer can land several updates
/// later, so the placement has to be remembered rather than applied inline.
///
/// It is deliberately a STRIP placement keyed by tab id, not an index
/// into `Oryxis::tabs`: `active_tab`, `last_terminal_tab` and
/// `connecting.tab_idx` hold positions in that vec, and every removal
/// path fixes them up by hand
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
    /// A duplicate that never produces a tab (the PTY refuses to
    /// spawn) would otherwise leave this armed forever and reposition
    /// some unrelated tab opened minutes later. Nothing legitimate
    /// takes this long.
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

    /// A dormant SFTP tab: it shows in the strip with its saved label but
    /// holds no mount, and re-mounts its panes the first time it is
    /// selected (`SelectSftpTab`). Both restore paths mint one, a pin
    /// recreated at boot and a closed tab the user asked back
    /// (issue #186), so the local pane's starting directory is decided
    /// here rather than twice.
    pub(crate) fn new_dormant(label: String, spec: PinnedTabSpec) -> Self {
        let mut tab = Self::new(label);
        tab.state.left.local_path = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("/"));
        tab.pending_reopen = Some(spec);
        tab
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
/// reopens lazily on first select. Ephemeral tabs have no spec and
/// aren't persisted.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum PinnedTabSpec {
    /// A saved host, reopened with `ConnectSsh` (id resolved to an index
    /// fresh at reopen time, so it survives connection reordering).
    Host { id: Uuid, label: String },
    /// A local shell, reopened with the captured program / args.
    LocalShell { program: String, args: Vec<String>, label: String },
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
            PinnedTabSpec::Sftp { label, .. } => label,
        }
    }

    /// Identity key for de-duplicating pins.
    pub fn dedupe_key(&self) -> String {
        match self {
            PinnedTabSpec::Host { id, .. } => format!("host:{id}"),
            PinnedTabSpec::LocalShell { program, args, label } => {
                format!("local:{program}:{}:{label}", args.join("\u{1f}"))
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
    /// Whether the given sidebar region is open on this tab.
    pub fn sidebar_visible(&self, side: crate::state::SidebarSide) -> bool {
        self.sidebar_open[side.idx()]
    }

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
            sidebar_open: [false; 2],
            chat_always_run_commands: Vec::new(),
            chat_auto_run_history: Vec::new(),
            chat_auto_run_streak: 0,
            chat_loading: false,
            chat_task: None,
            chat_mode: default_chat_mode(),
            chat_last_md_parse: None,
            chat_saved_id: None,
            chat_persisted: 0,
            relaunch: None,
            session_group_id: None,
            pinned: false,
            pending_reopen: None,
            inherited_pin: None,
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
    /// reopened (an ephemeral pane with no stable reference). A dormant
    /// tab keeps the spec it was created with; a live tab derives one from
    /// its focused pane's origin.
    pub fn pin_spec(&self) -> Option<PinnedTabSpec> {
        // A tab that absorbed a pinned SFTP tab persists the pin it
        // inherited, not the terminal it became: morphing is a
        // session-only gesture, so a relaunch must not silently rewrite
        // an arrangement the user made. Before `pending_reopen`, which a
        // morphed tab never carries anyway.
        if let Some(spec) = &self.inherited_pin {
            return Some(spec.clone());
        }
        if let Some(spec) = &self.pending_reopen {
            return Some(spec.clone());
        }
        let base = self.label.trim_end_matches(" (disconnected)").to_string();
        match &self.active().origin {
            PaneOrigin::Host(id) => Some(PinnedTabSpec::Host { id: *id, label: base }),
            // Quick-connect hosts have no stable reference to restore from
            // (the entry dies with the app), so the pin is session-only.
            PaneOrigin::QuickHost(_) => None,
            PaneOrigin::Local(spec) => Some(PinnedTabSpec::LocalShell {
                program: spec.program.clone(),
                args: spec.args.clone(),
                label: spec.label.clone(),
            }),
            // An ephemeral pane has no stable reference to restore from.
            PaneOrigin::Ephemeral => None,
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

    /// Splitting while a pane is zoomed drops the zoom, which is what
    /// keeps the focus honest: `make_split_pane` (and the tab-merge drop)
    /// focus the pane they just created, and a zoom left armed would
    /// leave that focus on something the grid does not draw, the very
    /// state `focus_adjacent` above exists to avoid.
    ///
    /// The clearing happens in the fork (`State::split_node` takes
    /// `maximized`), not here, so this pins a dependency the app relies
    /// on silently: if a rebase ever drops it, the caret goes invisible
    /// and only this fails.
    #[test]
    fn split_while_zoomed_drops_the_zoom() {
        let mut tab = TerminalTab::new_single("a".into(), dummy_terminal());
        let _second = split(&mut tab, pane_grid::Axis::Vertical);
        tab.toggle_maximize();
        assert!(tab.pane_grid.maximized().is_some(), "zoom armed");

        let third = split(&mut tab, pane_grid::Axis::Vertical);
        assert!(
            tab.pane_grid.maximized().is_none(),
            "the split must leave no zoom behind"
        );
        assert_eq!(tab.focused, third, "and the new pane is the focused one");
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

}
