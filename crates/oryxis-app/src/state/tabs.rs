//! Terminal tabs and panes (split out of `state.rs`).

use super::*;

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
    /// A local terminal; the spec is captured so the same shell is restored.
    Local(LocalShellSpec),
    /// Cloud/SSM/ECS or otherwise non-referenceable pane.
    Ephemeral,
}

/// One terminal pane, owns its alacritty grid and (optionally) the SSH
/// session feeding it. A `TerminalTab` holds one or more panes in a
/// `pane_grid::State`, which owns their split layout.
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
    /// SSH session handle (None for local shell).
    pub ssh_session: Option<Arc<SshSession>>,
    /// Session log ID for terminal recording.
    pub session_log_id: Option<Uuid>,
    /// Recorded bytes not yet flushed to the vault. PTY output appends
    /// here; `Oryxis::flush_session_logs` drains it (size threshold, a
    /// periodic tick, disconnect, or window close). Batching keeps the
    /// vault from taking one write per SSH chunk.
    pub session_log_buf: Vec<u8>,
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
    /// Working directory the shell last reported via OSC 7. Used so a new
    /// local shell can open in the focused pane's directory.
    pub cwd: Option<String>,
    /// OSC 133 shell-integration marks captured for this pane (bounded ring).
    /// Groundwork for the planned command-history feature; nothing reads it
    /// yet, hence the allow, the value is the captured command boundaries
    /// waiting for a consumer.
    #[allow(dead_code)]
    pub shell_marks: Vec<oryxis_terminal::ShellMark>,
    /// Latest OSC 9;4 progress the shell reported, drawn as a growing border
    /// around the tab. `None` (or state 0) means no active progress.
    pub progress: Option<oryxis_terminal::Progress>,
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

impl Pane {
    pub fn new(label: String, terminal: Arc<Mutex<TerminalState>>) -> Self {
        Self {
            id: Uuid::new_v4(),
            label,
            terminal,
            ssh_session: None,
            session_log_id: None,
            session_log_buf: Vec::new(),
            origin: PaneOrigin::Ephemeral,
            sync_flush_scheduled: false,
            osc_title: None,
            bell_flash: false,
            cwd: None,
            shell_marks: Vec::new(),
            progress: None,
        }
    }
}

/// A terminal tab. Its panes live in an iced `pane_grid::State`, which owns
/// the split layout (N-way horizontal / vertical splits) and resizing. A
/// fresh tab has exactly one pane; the user can split it.
pub(crate) struct TerminalTab {
    pub _id: Uuid,
    pub label: String,
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
            pinned: false,
            pending_reopen: None,
            state: SftpState::default(),
        }
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
            pane_grid,
            focused,
            chat_history: Vec::new(),
            chat_visible: false,
            chat_always_run_commands: Vec::new(),
            chat_auto_run_history: Vec::new(),
            chat_auto_run_streak: 0,
            ssm_keepalive: false,
            relaunch: None,
            session_group_id: None,
            pinned: false,
            pending_reopen: None,
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
                Some(crate::messages::Message::ConnectEcsExecTask {
                    group_id,
                    task_id,
                    task_label,
                    container,
                }) => Some(PinnedTabSpec::EcsExec {
                    group_id: *group_id,
                    task_id: task_id.clone(),
                    task_label: task_label.clone(),
                    container: container.clone(),
                    label: base,
                }),
                Some(crate::messages::Message::ConnectKubectlExecPod {
                    group_id,
                    namespace,
                    pod,
                    container,
                }) => Some(PinnedTabSpec::KubectlExec {
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
}
