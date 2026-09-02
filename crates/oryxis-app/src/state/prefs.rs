//! Every user preference the vault persists.
//!
//! These were 112 `setting_*` fields on `Oryxis`, which is 112
//! declarations, 112 boot initializers and one very long struct. They
//! are one thing: what the user configured, read at boot from the
//! `settings` table and written back by `persist_setting`.
//!
//! What is NOT here: the Settings screen's own state (which section is
//! open, its scroll offset, the search). That belongs to the view and
//! stays on `Oryxis` as `settings_*`.
//!
//! `Default` is written out rather than derived: these values ARE the
//! factory configuration, and several of them are deliberately not the
//! type's zero (`true` toggles, non-empty strings, sizes). Deriving
//! would silently reset them.

use uuid::Uuid;

#[derive(Debug, Clone)]
pub(crate) struct AppPrefs {
    /// Seconds between monitor probes, as typed in Settings. Parsed
    /// (and floored) by `monitor_interval_secs`; kept as a string so a
    /// half-typed value doesn't reset the field under the user.
    pub(crate) monitor_interval: String,
    pub(crate) sftp_default_editor: String,
    /// Persisted "Autosave" grant from the save-confirmation dialog:
    /// edited remote files upload on every save without asking.
    pub(crate) sftp_edit_autosave: bool,
    /// Renderer backend selection: "auto" (default), "opengl" (force
    /// wgpu's GL backend, still GPU), or "software" (tiny-skia / CPU).
    /// `main` translates this into `WGPU_BACKEND` / `ICED_BACKEND` at
    /// startup, an escape hatch for GPU/driver stacks that corrupt the
    /// wgpu surface. Read at boot only (the env vars are resolved before
    /// the runtime starts), so changing it asks the user to restart.
    pub(crate) renderer_backend: String,
    pub(crate) copy_on_select: bool,
    /// Careful paste (default on): a clipboard paste that contains a line
    /// break is parked in `pending_paste` and confirmed via a dialog with
    /// a line-count preview, so a hidden trailing newline can't auto-run
    /// a command. Off sends every paste straight through (power users).
    /// Persisted as `careful_paste`. Snippets are exempt, their content
    /// is user-authored, not whatever the clipboard happened to carry.
    pub(crate) careful_paste: bool,
    /// Sub-option of `setting_copy_on_select`: when both are on, a selection
    /// copies on right-click instead of on release. Ignored when
    /// `setting_copy_on_select` is off.
    pub(crate) right_click_copy: bool,
    /// What a terminal right-click does (Menu / Paste / Extend, PuTTY's
    /// three schemes). Persisted as `terminal_right_click`; default
    /// Paste (the prior behavior). `setting_right_click_copy` applies
    /// only under Paste.
    pub(crate) terminal_right_click: crate::util::RightClickMode,
    /// Jump the terminal back to the live edge when input reaches the PTY
    /// (PuTTY's "reset scrollback on keypress"). Persisted as
    /// `scrollback_reset_keypress`; default ON, matching every modern
    /// terminal (VTE's scroll-on-keystroke, Windows Terminal's
    /// `snapOnInput`, iTerm2, kitty): typing into a scrolled-up viewport
    /// must show what is being typed (issue #111). Applied in
    /// `write_bytes_to_pane`, the input funnel, not in the widget's key
    /// handler, so it follows the bytes the PTY actually receives.
    pub(crate) scrollback_reset_keypress: bool,
    /// Jump the terminal back to the live edge on new output (PuTTY's
    /// "reset scrollback on display activity"). Persisted as
    /// `scrollback_reset_output`; default off.
    pub(crate) scrollback_reset_output: bool,
    /// Content heuristics on paste (bidi/invisible chars, control
    /// bytes, curl|sh, homographs) park even single-line pastes behind
    /// the confirmation. Its own switch, independent of the multi-line
    /// careful-paste toggle. Persisted as `paste_guard`. Default on.
    pub(crate) paste_guard: bool,
    /// Offer stored credentials at a password prompt (issue #117):
    /// when a pane blocks on `[sudo] password for …:` and the vault
    /// holds a password for that host or an identity, a popup at the
    /// caret lists them. Persisted as `terminal_password_autofill`;
    /// default ON. Nothing is ever sent without the user picking a row:
    /// the popup is a suggestion, not an autofill that types itself.
    pub(crate) terminal_password_autofill: bool,
    /// Ask before handing a link from a REMOTE pane to the browser
    /// (default on, VS Code's behaviour for a remote window). The URL is
    /// text a remote host printed, and an OSC 8 link's label can differ
    /// from its target, so the prompt shows the target and names the
    /// host. Local panes never ask: their output came from a program
    /// running as the user. Persisted as `terminal_link_confirm`.
    pub(crate) terminal_link_confirm: bool,
    /// Tunnel a link's loopback callback through the pane's own SSH
    /// connection (default on). A CLI login prints an authorize URL
    /// whose `redirect_uri` is `127.0.0.1:<port>` on the machine running
    /// it; over SSH that is the remote machine, so without the tunnel the
    /// browser here follows the redirect to nothing. Persisted as
    /// `terminal_link_tunnel`.
    pub(crate) terminal_link_tunnel: bool,
    /// Command-history capture (default on): record commands executed on
    /// saved hosts into the vault's `command_history` table, surfaced in
    /// the terminal sidebar's History tab. Persisted as `command_history`.
    pub(crate) command_history: bool,
    /// Live-append every captured command to a per-host plain-text file
    /// (offline reference / support sharing), on top of the vault
    /// capture. Persisted as `command_history_file`. Default off.
    pub(crate) command_history_file: bool,
    /// Folder for the per-host command logs. `None` = the default
    /// `~/.oryxis/command-history/`. Persisted as
    /// `command_history_file_dir` (empty string = default).
    pub(crate) command_history_file_dir: Option<String>,
    /// Destination folder for ZMODEM downloads. Empty = the OS Downloads
    /// dir (or `~/.oryxis/downloads`). Persisted as `zmodem_download_dir`.
    pub(crate) zmodem_download_dir: String,
    /// Snippets sidebar: only show snippets sharing at least one tag
    /// with the focused host's tags. Persisted as `snippet_tag_filter`.
    pub(crate) snippet_tag_filter: bool,
    pub(crate) bold_is_bright: bool,
    /// Terminal background opacity in percent, 100 = opaque (the
    /// default). Persisted as `terminal_opacity`. Kept here only so
    /// Settings can render the picker: the render path reads
    /// `theme::terminal_bg_alpha()`, which also knows whether the
    /// window was actually created with a transparent surface.
    pub(crate) terminal_opacity: u8,
    /// Path to the global terminal background picture, empty = none.
    /// Persisted as `terminal_bg_image`. A host can override it (or opt
    /// out of it) through its own `terminal_appearance`.
    pub(crate) terminal_bg_image: String,
    /// How that picture is laid into the pane
    /// (`cover` / `contain` / `stretch` / `center` / `tile`), persisted
    /// as `terminal_bg_fit`.
    pub(crate) terminal_bg_fit: String,
    /// How far the picture is faded towards the terminal background
    /// colour, in percent. Defaults high: a photograph at full strength
    /// makes terminal text unreadable, and a first-run that looks broken
    /// is worse than one that looks subtle.
    pub(crate) terminal_bg_dim: u8,
    /// Draw the thin separator outline on UNFOCUSED panes. The focused
    /// pane's accent outline is not affected: with the panes flush there
    /// would otherwise be nothing at all marking where one ends.
    pub(crate) pane_border_inactive: bool,
    /// Gutter between split panes, in pixels, as a string ("0" = flush).
    /// Flush is the default: the seam is grabbable either way, because a
    /// pane hands a strip back to the grid on the edges it shares.
    pub(crate) pane_gap: String,
    pub(crate) keyword_highlight: bool,
    /// The user's own highlight rules, as stored (`terminal_highlight_rules`,
    /// a JSON array). This is the editable list; the terminal never sees
    /// it directly.
    pub(crate) highlight_rules: Vec<oryxis_core::models::HighlightRule>,
    /// The same rules compiled, shared with every pane's widget AND with
    /// its backend (which watches the output stream for the rules that
    /// carry an action). Rebuilt on every edit, never per frame: a
    /// pattern is compiled once and matched millions of times.
    pub(crate) compiled_highlight_rules: std::sync::Arc<oryxis_terminal::CompiledRules>,
    /// Performance mode: trade visual niceties for CPU on weak / software
    /// render paths. When on, the terminal skips the per-frame keyword /
    /// URL / IP / path highlight scan (kept only when Privacy Mode needs
    /// its spans) and the active tab uses a flat accent tint instead of
    /// the per-pixel gradient. Auto-enabled once on GPU stacks the boot
    /// probe redirects to software (see `renderer_probe`); the user can
    /// still toggle it. `"performance_mode"` setting.
    pub(crate) performance_mode: bool,
    /// Renders the terminal perf HUD (per-phase frame timing + fps) in
    /// the top-right of every pane. Off by default; the `ORYXIS_TERM_PERF`
    /// env var forces it on too. `"perf_overlay"` setting.
    pub(crate) perf_overlay: bool,
    /// The network tools panel (DNS, ping, traceroute, port test,
    /// HTTP/TLS, WHOIS, DNSBL). Off by default and, while off, its whole
    /// UI is hidden: no burger entry, no tab, no way in (the
    /// optional-features rule). `"network_tools_enabled"` setting.
    pub(crate) network_tools: bool,
    /// When the foreground and background of a cell render too close
    /// to each other (LS_COLORS' `ow` over a green palette,
    /// PowerShell's `$PSStyle.FileInfo.Directory` blue-on-blue, …),
    /// the renderer flips the foreground to a legible alternative.
    /// Off makes the renderer paint the cell exactly as the app
    /// asked, which some color-precise tools rely on.
    pub(crate) smart_contrast: bool,
    /// How the terminal bell (BEL / `\a`) is surfaced: off / visual flash /
    /// audible beep. Persisted as `terminal_bell_mode`; default beep.
    pub(crate) bell_mode: crate::util::BellMode,
    /// OSC 52 clipboard access policy: off / write-only / read-write.
    /// Persisted as `terminal_clipboard_access`; default write-only.
    pub(crate) clipboard_access: crate::util::ClipboardAccess,
    /// How an OSC 9 shell notification is surfaced: off / in-app toast / OS.
    /// Persisted as `terminal_notification`; default OS.
    pub(crate) notification_mode: crate::util::NotificationMode,
    /// Smart tabs: OSC 133-driven attention dots on background tabs plus
    /// long-command-finished / output-after-silence notifications
    /// (delivered per `setting_notification_mode`). Persisted as
    /// `smart_tabs`; default on.
    pub(crate) smart_tabs: bool,
    /// Minimum runtime (seconds) before a finished command earns a dot +
    /// notification; `0` turns the finished half off (activity detection
    /// stays). Persisted as `smart_tabs_long_seconds`; default 10.
    pub(crate) smart_long_secs: u32,
    /// Ask before closing a tab that holds a LIVE session (SSH /
    /// telnet / mosh / serial / cloud), so a misplaced click on the
    /// strip's X cannot silently drop a connection. Off by default,
    /// matching the common SSH-client behavior of closing straight
    /// through; the multi-pane group confirm is independent of this.
    /// Persisted as `confirm_close_session_tab`.
    pub(crate) confirm_close_session_tab: bool,
    /// Toggles the bottom status bar that shows current connection IP +
    /// Oryxis version. Off in `view_main` simply skips rendering it,
    /// reclaiming the row for the active content area.
    pub(crate) show_status_bar: bool,
    /// Status-bar element visibility (issue #83 follow-up). Version and
    /// the connection text exist today, so they default on (hiding is
    /// opt-in); the new latency / dimensions / cwd segments default off
    /// so an existing bar doesn't suddenly sprout segments.
    pub(crate) status_show_version: bool,
    pub(crate) status_show_connection: bool,
    pub(crate) status_show_latency: bool,
    pub(crate) status_show_dimensions: bool,
    pub(crate) status_show_cwd: bool,
    /// Align the status-bar content on the PHYSICAL left edge instead
    /// of the trailing edge (issue #83 follow-up), so it lines up with
    /// a left-docked panel layout. The panel dock is a physical edge
    /// like `sidebar_tab_sides` below, so RTL does not flip this
    /// either. Default off (trailing, the original behaviour).
    pub(crate) status_bar_align_left: bool,
    /// Per-tab placement for the terminal sidebar (issue #102): each
    /// tab lives in the LEFT or RIGHT region (both can be on screen
    /// at once) or is HIDDEN entirely. Only EXPLICIT user choices are
    /// stored (and persisted, as `"tab:placement"` CSV under
    /// `sidebar_tab_sides`); an absent tab resolves to
    /// `TerminalSidebarTab::default_placement()` (right, for every
    /// tab). Sides are physical edges like the #87 tab-bar dock, so
    /// RTL does not flip them. The pre-#102 whole-sidebar
    /// `terminal_sidebar_side` setting is migrated at boot into
    /// explicit entries.
    pub(crate) sidebar_tab_sides: std::collections::HashMap<
        crate::state::TerminalSidebarTab,
        crate::state::SidebarPlacement,
    >,
    /// Open the terminal sidebar automatically when a session opens
    /// (per-host `Connection.sidebar_auto_open` overrides this).
    pub(crate) sidebar_auto_open: bool,
    /// Which tab the terminal sidebar opens onto (issue #85). `None`
    /// keeps the last opened tab (the default, today's behavior); a
    /// specific tab overrides on every open, resolved against the pane's
    /// gates so an unreachable choice falls back to Snippets.
    pub(crate) sidebar_default_tab: Option<crate::state::TerminalSidebarTab>,
    /// Show the monitored host's vitals as a status-bar segment (issue
    /// #83, the MobaXterm-style bar). Off by default: it is a second,
    /// optional surface on the sidebar Monitor tab's engine, and an
    /// optional feature hides ALL its UI when off.
    pub(crate) monitor_status_bar: bool,
    /// Host dashboard view mode: responsive card grid (default),
    /// single-column list, or the mRemoteNG-style tree (issue #102).
    /// Persisted as `host_view_mode`; the pre-tree `host_list_view`
    /// bool is grandfathered at boot.
    pub(crate) host_view_mode: crate::state::HostViewMode,
    /// Monitor dashboard layout (issue #95): `true` caps the grid at
    /// two full-width columns (the "list" reading), `false` (default)
    /// uses the responsive card grid.
    pub(crate) monitor_dash_list_view: bool,
    /// When on (default), dashboard cards get a soft per-colour accent
    /// wash (the host brand / group colour fading left to right); when
    /// off, cards stay pure (no overlay).
    pub(crate) card_accent_glass: bool,
    /// When on, the host cards' subtitle shows the `user@host:port`
    /// address; when off (the default) it shows only the auth method,
    /// keeping addresses out of screenshots / screen shares. Port 22 is
    /// always omitted from the address regardless of this toggle.
    pub(crate) show_host_address: bool,
    /// When on, tabs show the connection address as a second line below
    /// the tab label, formatted and masked exactly like the host cards'
    /// subtitle (`host_address_label`). Off by default, for the same
    /// screenshot / screen-share reason as `setting_show_host_address`.
    pub(crate) show_tab_host_address: bool,
    /// Settings > Advanced debug logging: mirror of the `debug_logging`
    /// setting, true while tracing events are also written to the
    /// exportable `~/.oryxis/oryxis-debug.log` file (see `logging.rs`).
    pub(crate) debug_logging: bool,
    /// When on, clicking the window's close button hides to the
    /// system tray instead of quitting. Only honoured on Windows
    /// (the tray module is a no-op everywhere else). Default off
    /// so we don't surprise users who never knew there was a tray.
    pub(crate) close_to_tray: bool,
    /// When on, minimizing the window hides it from the taskbar and
    /// leaves only the tray icon visible. Windows-only. Default off.
    pub(crate) minimize_to_tray: bool,
    /// `"left"` (default, Termius-style: X replaces the OS badge on
    /// hover/active) or `"right"` (badge stays left, X gets its own
    /// slot at the trailing edge of the tab). Anything else is treated
    /// as `"left"`.
    pub(crate) tab_close_button_side: String,
    /// Pinned-tab visual style: "compact" (Chrome-style icon-only chip) or
    /// "full" (a normal tab with a special pinned border, stuck to the left).
    pub(crate) pinned_tab_style: String,
    /// Where "Duplicate Tab" puts the copy: `"next"` (default, beside the
    /// original), `"end"` (the pre-#110 append) or `"start"`. Parsed by
    /// [`crate::state::TabPlacement::from_setting`]; ordering only, never
    /// an index into `tabs`.
    pub(crate) duplicate_tab_position: String,
    /// Whether the Home (vault) area tab occupies the FIRST Ctrl+digit
    /// slot, pushing every tab's slot up by one (so the third tab
    /// answers to Ctrl+4).
    ///
    /// False on new installs: the slots are the tabs, which is what the
    /// tab numbers show and what every other tabbed app does. True for
    /// vaults that existed before the change, so nobody's muscle memory
    /// breaks under them; the boot migration decides which
    /// (`tab_slots_home_migrated`), and Settings > Shortcuts flips it.
    /// Home keeps its own binding either way (Ctrl+Shift+1, the vault
    /// section slot) plus the house icon in the strip.
    pub(crate) tab_slot_includes_home: bool,
    /// Tab numbering (`"off"` default / `"prefix"` / `"icon"`): off shows
    /// no number, prefix puts "12. " before the label, icon puts the
    /// number in the host badge's slot instead of the OS / host glyph.
    /// The number is the tab's position in the STRIP, which is what
    /// `ActivateStripSlot` (Ctrl+N) counts, and it is not capped at 9.
    pub(crate) tab_number_style: String,
    /// When on, each tab paints a small colored dot over its OS badge:
    /// green for an active SSH session, orange while connecting, red
    /// for a tab that lost its session. Defaults on; the user can hide
    /// it from Settings -> Interface.
    pub(crate) show_tab_status_dot: bool,
    /// When true (default), the hairline under the tab strip thickens
    /// to 2 px and tints itself with the active host's accent (per-
    /// host color → cloud brand → global accent). When false, it
    /// collapses to the same neutral 1 px border the non-tabbed
    /// screens use, so the user always sees a flat chrome regardless
    /// of which host is open.
    pub(crate) tab_accent_line: bool,
    /// When true (default), the whole top bar carries a subtle accent
    /// wash (tinted leading edge fading to the bar surface). Independent
    /// of `setting_tab_accent_line` (the bottom hairline) so the user can
    /// keep one without the other.
    pub(crate) tab_accent_wash: bool,
    /// When true (default), the active tab's LABEL (and its close X /
    /// mode chip) is tinted with the host accent, contrast-validated
    /// via `theme::readable_accent_on` (issue #79). When false, tab
    /// text always uses the theme's neutral text colours; the accent
    /// keeps living in the badge, active wash, pinned border and dots.
    pub(crate) tab_accent_text: bool,
    /// Where the strip's accent colour comes from: `"host"` (default,
    /// per-host custom colour, session-group colour, cloud brand or OS
    /// brand) or `"app"` (always the global app accent, disabling
    /// per-host colouring in the fill, wash, hairline and text at
    /// once). OS badges keep their brand colour either way, identity
    /// is the badge's job.
    pub(crate) tab_accent_color: String,
    /// Active-tab fill: `"gradient"` (default, the "lit from above"
    /// vertical accent fade) or `"solid"` (a single flat accent tint).
    /// Read by every tab/chip renderer via `active_tab_bg`.
    pub(crate) tab_fill_style: String,
    /// Where the tab strip docks: `"top"` (default, tabs share the bar
    /// with the window chrome), `"bottom"` (the strip sits above the
    /// status bar; a slim top bar keeps the burger, drag area and the
    /// minimize / maximize / close buttons) or `"left"` / `"right"`
    /// (vertical tab list on that window edge, issue #87). Anything
    /// else reads as top.
    pub(crate) tab_bar_position: String,
    /// Inactive-tab separation style (issue #87): `none` / `border` /
    /// `underline`. Mirrored into the process-wide `INACTIVE_TAB_STYLE`
    /// gate read by the tab renderer.
    pub(crate) inactive_tab_style: String,
    /// Tab sizing in the horizontal strip (issue #112): `adaptive`
    /// (default, active tab fattens) or `uniform` (one width for all,
    /// labels ellipsize). Uniform exists so selecting a tab stops
    /// relaying the whole bar under the pointer.
    pub(crate) tab_width_mode: String,
    /// Width ceiling for the uniform mode: `small` / `medium` / `large`.
    /// Only consulted when `setting_tab_width_mode == "uniform"`; the
    /// widest label still sets the width, this is how far it may go
    /// before every tab starts truncating instead.
    pub(crate) tab_uniform_size: String,
    /// Side dock only: pinned tabs live with the window chrome instead
    /// of scrolling inside the strip. Top bar visible: they dock next
    /// to Home up there; top bar hidden: they become a fixed group at
    /// the top of the strip (Zen-style essentials).
    pub(crate) pinned_tabs_top_bar: bool,
    /// Side dock only: hide the slim top bar entirely. The titlebar
    /// contract moves into the strip: a header row carries the burger,
    /// Home and compact window buttons, and the strip's empty area
    /// drags the window (double-click maximizes).
    pub(crate) side_hide_top_bar: bool,
    /// Side dock only: the strip runs to the window's bottom edge and
    /// the status bar spans only the content area.
    pub(crate) side_full_height: bool,
    /// Master toggle for the host-monitoring feature (issue #83), in
    /// Features & Plugins. Off by default: monitoring is niche and
    /// recurring, so ALL of its UI (the sidebar Monitor tab, the
    /// status-bar segment, the per-host opt-in, the interval + alerts)
    /// stays hidden until the user enables the feature here. Distinct
    /// from the per-host `Connection.monitor_enabled`, which decides
    /// WHICH hosts are probed once the feature is on.
    pub(crate) host_monitoring: bool,
    /// Whether enabling the feature has ever seeded its internal
    /// defaults (the status-bar segment). Set once on first enable so a
    /// later off/on can't clobber the user's own choices.
    pub(crate) host_monitoring_seeded: bool,
    /// "Enable for all hosts" (issue #83): when on, every host with a
    /// live session is monitored and the per-host editor toggle renders
    /// locked-on. When off, the per-host opt-in decides. The effective
    /// per-host value is `setting_monitor_all_hosts || conn.monitor_enabled`.
    pub(crate) monitor_all_hosts: bool,
    /// "Only hosts with a live session" (issue #197): when on, the
    /// Monitoring dashboard is limited to machines a terminal tab is
    /// already logged in to, so the one surface that opens connections
    /// of its own stops opening any. Off (the default) it dials every
    /// opted-in host, which is what the fleet view is for. It governs
    /// the DASHBOARD alone: the sidebar tab reads the pane it belongs
    /// to and has a live session by construction.
    pub(crate) monitor_dash_live_only: bool,
    /// Master toggle for the tmux session manager (issue #116), in
    /// Features & Plugins. Off by default: managing tmux from a panel
    /// is niche, so the sidebar tab and everything else it owns stay
    /// hidden until the user enables the feature here. Unlike
    /// monitoring there is no per-host flag: the tab costs nothing
    /// until it is opened, and whether a host runs tmux is a question
    /// the host itself answers.
    pub(crate) tmux_manager: bool,
    /// Open a second tab to an already-connected host on the existing
    /// SSH connection instead of dialling again (F2). On by default:
    /// it removes a handshake, a key exchange and an authentication
    /// (and, on a jump chain, all of that per hop) from every repeat
    /// open. Off makes every tab dial fresh, which is what a server
    /// with a low `MaxSessions` wants, though hitting that cap already
    /// falls back on its own.
    pub(crate) ssh_connection_reuse: bool,
    /// Vault navigation orientation: `"horizontal"` (default) renders the
    /// sub-sections as a pill strip beneath the top bar; `"vertical"`
    /// renders them as an icon rail on the left of the vault content. The
    /// top bar (session tabs + Home icon + Personal chip) is identical in
    /// both. Replaces the old classic/workspace `layout_mode` duality
    /// (classic users migrate to `"vertical"` on first load).
    pub(crate) nav_orientation: String,
    /// Language picker choice as persisted in the `language` setting:
    /// `"auto"` (default, follow the OS locale) or a concrete language
    /// code ("en", "pt-BR", ...). The *resolved* language always lives
    /// in `i18n::Language::active()`; this field only drives the
    /// Settings picker selection so "Auto (OS)" survives restarts as a
    /// choice instead of collapsing into the detected language.
    pub(crate) language_choice: String,
    /// When the vertical nav rail is showing, expand it to show section
    /// labels (wide rail) instead of the icon-only rail. Persisted so the
    /// choice sticks.
    pub(crate) nav_rail_expanded: bool,
    /// Default shape for host icons in the dashboard, sidebar tab
    /// badges and host cards: `"circular"` (default v0.7), `"square"`
    /// (legacy Termius-style), `"outline"`, or `"initials"`. Read by
    /// the host icon widget in PR 3; until then the value persists but
    /// the renderer keeps the current shape.
    pub(crate) default_host_icon: String,
    pub(crate) keepalive_interval: String,
    /// Defaults pre-filled into the form for a NEW connection, so the user
    /// doesn't re-set the same fields every time. Persisted as
    /// `default_agent_forwarding` / `default_port` / `default_keepalive` /
    /// `default_terminal_type`.
    pub(crate) default_agent_forwarding: bool,
    pub(crate) default_port: String,
    pub(crate) default_keepalive: String,
    pub(crate) default_terminal_type: String,
    /// Default "host profile" fields (extended new-connection defaults), so
    /// a fleet of identical hosts (same login / key / proxy / folder) needs
    /// no re-typing. Entity references are stored by UUID and resolved to a
    /// label when seeding the form; a deleted entity resolves to no default.
    /// Persisted as `default_username` / `default_auth_method` /
    /// `default_identity_id` / `default_key_id` / `default_group_id` /
    /// `default_proxy_identity_id` / `default_mcp_enabled` /
    /// `default_encoding` / `default_env_vars`.
    pub(crate) default_username: String,
    pub(crate) default_auth_method: oryxis_core::models::connection::AuthMethod,
    pub(crate) default_identity_id: Option<Uuid>,
    pub(crate) default_key_id: Option<Uuid>,
    pub(crate) default_group_id: Option<Uuid>,
    pub(crate) default_proxy_identity_id: Option<Uuid>,
    pub(crate) default_mcp_enabled: bool,
    pub(crate) default_encoding: Option<String>,
    pub(crate) default_env_vars: Vec<crate::state::EnvVarForm>,
    /// Collapsed state of the (now long) "New connection defaults" card in
    /// Settings → Connection. Persisted as `defaults_collapsed` so the
    /// choice sticks; the field rows are hidden behind the header when set.
    pub(crate) defaults_collapsed: bool,
    /// Background refresh of every cloud profile on a fixed interval.
    /// Off by default; opt-in to avoid surprise API calls.
    pub(crate) cloud_auto_refresh_enabled: bool,
    /// Minutes between auto-refresh ticks. Stored as a string to match
    /// the rest of the int-setting family (`setting_keepalive_interval`,
    /// etc.) and let the Settings UI accept partial typed input.
    pub(crate) cloud_auto_refresh_interval_minutes: String,
    /// When on, the next boot deletes orphaned cloud-imported hosts
    /// (resource gone upstream) older than `orphan_archive_days`.
    pub(crate) cloud_auto_archive_orphans: bool,
    pub(crate) cloud_orphan_archive_days: String,
    pub(crate) scrollback_rows: String,
    /// Characters that terminate a word for double-click selection in the
    /// terminal (the "word delimiters" set). Defaults to
    /// `oryxis_terminal::DEFAULT_WORD_DELIMITERS`; the Terminal settings
    /// panel lets the user customise or reset it.
    pub(crate) word_delimiters: String,
    /// How terminal teaching hints are surfaced (the mouse-capture toast
    /// and the "hold Ctrl and click" link toast). Persisted as the
    /// `terminal_hint_mode` setting. `Once` (default) shows each hint a
    /// single time per pane, tracked in-memory on `Pane`.
    pub(crate) hint_mode: crate::util::HintMode,
    /// What happens to a pane when its session ends (issue #208).
    /// Persisted as the `pane_end_action` setting. `Prompt` (default)
    /// keeps the pane and offers restart / close; `Close` drops it.
    /// A lone REMOTE pane never consults this (it relabels and
    /// auto-reconnects instead); a lone local shell does.
    pub(crate) pane_end_action: crate::util::PaneEndAction,
    /// Max parallel SFTP transfer slots (uploads/downloads). 1 = serial,
    /// up to 8 = aggressive. Each slot gets its own SFTP subsystem
    /// channel on the same SSH connection so they don't fight for the
    /// shared client mutex.
    pub(crate) sftp_concurrency: String,
    /// Ask for the destination folder on every download instead of using
    /// the local pane's current directory. Off by default: in the
    /// dual-pane surface the destination is already on screen, so asking
    /// every time would be noise for most users. The row menu's "Download
    /// to..." asks regardless.
    pub(crate) sftp_ask_download_dir: bool,
    /// Upload to a scratch name and rename into place on success, so the
    /// real name only ever appears finished (WinSCP's "transfer to
    /// temporary filename"). Off by default because the remote side has
    /// real objections: a directory that forbids rename, a watcher or
    /// deploy hook keyed on the final name appearing, a quota hook on
    /// create. The DOWNLOAD side does this unconditionally and is not a
    /// setting: turning it off would only restore the bug where an
    /// interrupted download leaves a truncated file under the real name.
    pub(crate) sftp_upload_temp_name: bool,
    /// Where an SFTP console opened on a live tab lands: stacked under
    /// the shell (default), beside it, or zoomed over it. Every option
    /// is a pane of that tab, so the Terminal / Console / SFTP switch
    /// works the same whichever one the user picked.
    pub(crate) sftp_console_layout: crate::state::SftpConsoleLayout,
    /// TCP connect + SSH transport handshake timeout, in seconds.
    pub(crate) sftp_connect_timeout: String,
    /// Authentication phase timeout, in seconds.
    pub(crate) sftp_auth_timeout: String,
    /// Per-channel open timeout (PTY session, SFTP subsystem, sibling
    /// channels), in seconds.
    pub(crate) sftp_session_timeout: String,
    /// Per-operation timeout for SFTP requests (list_dir, read, write,
    /// metadata). Caps the "Loading…" state so a hung server can't pin
    /// the UI forever.
    pub(crate) sftp_op_timeout: String,
    pub(crate) auto_reconnect: bool,
    pub(crate) max_reconnect_attempts: String,
    /// Vault auto-lock idle threshold, in minutes ("0" = off). When the
    /// user hasn't produced any input event for this long, a SOFT lock
    /// fires (`SoftLockVault`): key zeroized + lock screen, but live
    /// sessions and tabs survive, unlike the manual Lock teardown.
    pub(crate) auto_lock_minutes: String,
    /// What the manual Lock Vault button does: "ask" (default) opens the
    /// confirm dialog, "sleep" soft-locks directly (sessions survive),
    /// "lock" tears down directly. Saved from the dialog's "always use
    /// the selected option" opt-in; Settings > Security exposes it so
    /// the dialog can be brought back.
    pub(crate) manual_lock_action: String,
    /// Opt-in local unlock via the OS biometric / keystore. When on, a
    /// successful password unlock stores the master password under OS
    /// protection (Windows Hello / Touch ID / login keyring) so the lock
    /// screen can release it after a presence check. Persisted as
    /// `biometric_unlock_enabled` (off by default). NOT SSH auth; the
    /// vault stays encrypted with the password-derived key either way.
    pub(crate) biometric_unlock_enabled: bool,
    pub(crate) os_detection: bool,
    /// Global default for recording terminal sessions to the vault. A
    /// per-host `Connection.session_logging` override wins over this.
    pub(crate) session_logging: bool,
    /// Recording detail: `true` = full (arrival timing + resize events,
    /// what the asciicast `.cast` export needs; the export action only
    /// shows while this is on), `false` = the plain output log of old.
    pub(crate) session_log_full: bool,
    /// Deflate recorded chunks before sealing them (order matters:
    /// ciphertext doesn't compress). Long sessions shrink 5-20x.
    pub(crate) session_log_compress: bool,
    /// Mirror what is being recorded into a plain text file as the
    /// session runs (issue #187). Off by default and gated by the
    /// recording itself, so the per-host override still decides WHICH
    /// sessions produce one; the file is not encrypted, which is the
    /// whole point of the option and what its description says.
    /// Persisted as `session_log_file`.
    pub(crate) session_log_file: bool,
    /// Folder those files live in; `None` = `~/.oryxis/session-logs/`.
    /// Persisted as `session_log_file_dir` (a set value only).
    pub(crate) session_log_file_dir: Option<String>,
    /// Whether connection events (connect / disconnect / auth failure /
    /// error) are recorded to the vault log. Gates every `add_log` site.
    pub(crate) connection_history: bool,
    /// Auto-delete retention for Logs ("off", "1d", "3d", "7d",
    /// "14d", "30d", "90d"). Applied at boot and when changed.
    pub(crate) logs_retention: String,
    /// Ceiling on what ALL session recordings may occupy together, in
    /// bytes; `None` = no cap (the default). This is the user's own
    /// quota, not the safety net: reaching it drops the oldest FINISHED
    /// recordings (retention by size, sibling of `logs_retention`'s
    /// retention by age) and recording continues. The unconditional
    /// free-space floor in `dispatch_terminal::output` is what stops a
    /// runaway, and it is deliberately not a setting.
    ///
    /// Persisted as `session_log_max_bytes` ("off" or a byte count), so
    /// a future picker can offer other sizes without a migration.
    pub(crate) session_log_max_bytes: Option<u64>,
    pub(crate) auto_check_updates: bool,
    /// Release stream the updater follows (`stable` / `nightly`).
    pub(crate) update_channel: crate::update::UpdateChannel,
}

impl Default for AppPrefs {
    fn default() -> Self {
        Self {
            monitor_interval: "5".to_string(),
            sftp_default_editor: String::new(),
            sftp_edit_autosave: false,
            renderer_backend: "auto".to_string(),
            copy_on_select: true,
            careful_paste: true,
            right_click_copy: false,
            terminal_right_click: crate::util::RightClickMode::default(),
            scrollback_reset_keypress: true,
            scrollback_reset_output: false,
            paste_guard: true,
            terminal_password_autofill: true,
            terminal_link_confirm: true,
            terminal_link_tunnel: true,
            command_history: true,
            command_history_file: false,
            command_history_file_dir: None,
            zmodem_download_dir: String::new(),
            snippet_tag_filter: false,
            bold_is_bright: true,
            terminal_opacity: 100,
            terminal_bg_image: String::new(),
            terminal_bg_fit: oryxis_terminal::BgFit::default().as_str().to_string(),
            terminal_bg_dim: 55,
            pane_border_inactive: true,
            pane_gap: "0".to_string(),
            keyword_highlight: true,
            highlight_rules: Vec::new(),
            compiled_highlight_rules: std::sync::Arc::default(),
            performance_mode: false,
            perf_overlay: false,
            network_tools: false,
            smart_contrast: true,
            bell_mode: crate::util::BellMode::default(),
            clipboard_access: crate::util::ClipboardAccess::default(),
            notification_mode: crate::util::NotificationMode::default(),
            smart_tabs: true,
            smart_long_secs: 10,
            confirm_close_session_tab: false,
            show_status_bar: true,
            status_show_version: true,
            status_show_connection: true,
            status_show_latency: false,
            status_show_dimensions: false,
            status_show_cwd: false,
            status_bar_align_left: false,
            sidebar_tab_sides: std::collections::HashMap::new(),
            sidebar_auto_open: false,
            sidebar_default_tab: None,
            monitor_status_bar: false,
            host_view_mode: crate::state::HostViewMode::default(),
            monitor_dash_list_view: false,
            card_accent_glass: true,
            show_host_address: false,
            show_tab_host_address: false,
            debug_logging: false,
            close_to_tray: false,
            minimize_to_tray: false,
            tab_close_button_side: "left".into(),
            pinned_tab_style: "compact".into(),
            duplicate_tab_position: "next".into(),
            tab_slot_includes_home: false,
            tab_number_style: "off".into(),
            show_tab_status_dot: true,
            tab_accent_line: true,
            tab_accent_wash: true,
            tab_accent_text: true,
            tab_accent_color: "host".into(),
            tab_fill_style: "gradient".into(),
            tab_bar_position: "top".into(),
            inactive_tab_style: "none".into(),
            tab_width_mode: "adaptive".into(),
            tab_uniform_size: "medium".into(),
            pinned_tabs_top_bar: false,
            side_hide_top_bar: false,
            side_full_height: false,
            host_monitoring: false,
            host_monitoring_seeded: false,
            monitor_all_hosts: false,
            monitor_dash_live_only: false,
            tmux_manager: false,
            ssh_connection_reuse: true,
            nav_orientation: "horizontal".into(),
            language_choice: "auto".into(),
            nav_rail_expanded: false,
            default_host_icon: "circular".into(),
            keepalive_interval: "30".into(),
            default_agent_forwarding: false,
            default_port: "22".into(),
            default_keepalive: String::new(),
            default_terminal_type: "xterm-256color".into(),
            default_username: String::new(),
            default_auth_method: oryxis_core::models::connection::AuthMethod::Auto,
            default_identity_id: None,
            default_key_id: None,
            default_group_id: None,
            default_proxy_identity_id: None,
            default_mcp_enabled: true,
            default_encoding: None,
            default_env_vars: Vec::new(),
            defaults_collapsed: false,
            cloud_auto_refresh_enabled: false,
            cloud_auto_refresh_interval_minutes: "30".into(),
            cloud_auto_archive_orphans: false,
            cloud_orphan_archive_days: "7".into(),
            scrollback_rows: "10000".into(),
            word_delimiters: oryxis_terminal::DEFAULT_WORD_DELIMITERS.into(),
            hint_mode: crate::util::HintMode::default(),
            pane_end_action: crate::util::PaneEndAction::default(),
            sftp_concurrency: "2".into(),
            sftp_ask_download_dir: false,
            sftp_upload_temp_name: false,
            sftp_console_layout: crate::state::SftpConsoleLayout::default(),
            sftp_connect_timeout: "15".into(),
            sftp_auth_timeout: "30".into(),
            sftp_session_timeout: "10".into(),
            sftp_op_timeout: "30".into(),
            auto_reconnect: true,
            max_reconnect_attempts: "5".into(),
            auto_lock_minutes: "0".into(),
            manual_lock_action: "ask".into(),
            // Overwritten by `boot` right after this, which probed before the unlock, so the lock screen can offer it.
            biometric_unlock_enabled: false,
            os_detection: true,
            session_logging: false,
            session_log_full: true,
            session_log_compress: true,
            session_log_file: false,
            session_log_file_dir: None,
            connection_history: false,
            logs_retention: "off".into(),
            session_log_max_bytes: None,
            // Overwritten by `boot` right after this, which read pre-unlock: the boot check runs while the vault can still be locked.
            auto_check_updates: true,
            // Overwritten by `boot` right after this, which same pre-unlock read as `auto_check_updates`.
            update_channel: crate::update::UpdateChannel::default(),
        }
    }
}

/// Where downloads land (and where a download's file picker opens):
/// the `zmodem_download_dir` setting when set, else the OS Downloads
/// dir, else `~/.oryxis/downloads`.
///
/// A free function rather than a method so the resolution order itself
/// is testable; `Oryxis::default_download_dir` is how the app reads it.
pub(crate) fn resolve_download_dir(configured: &str) -> std::path::PathBuf {
    let configured = configured.trim();
    if !configured.is_empty() {
        return std::path::PathBuf::from(configured);
    }
    if let Some(dir) = dirs::download_dir() {
        return dir;
    }
    oryxis_core::paths::oryxis_dir()
        .unwrap_or_else(|| std::path::PathBuf::from(".").join(".oryxis"))
        .join("downloads")
}

impl AppPrefs {
    /// A sidebar tab's placement: the user's explicit choice when one
    /// exists, the tab's own default otherwise.
    pub(crate) fn sidebar_tab_placement(
        &self,
        tab: crate::state::TerminalSidebarTab,
    ) -> crate::state::SidebarPlacement {
        self.sidebar_tab_sides.get(&tab).copied().unwrap_or_else(|| tab.default_placement())
    }

    /// The region a sidebar tab is docked to; `None` = hidden.
    pub(crate) fn sidebar_tab_side(
        &self,
        tab: crate::state::TerminalSidebarTab,
    ) -> Option<crate::state::SidebarSide> {
        self.sidebar_tab_placement(tab).side()
    }

    /// Serialize the EXPLICIT placement choices as `"tab:placement"`
    /// CSV in `ALL` order (deterministic, so the persisted value is
    /// stable across sessions and sync-friendly). Defaults are
    /// omitted on purpose: a future change of a tab's default then
    /// applies to users who never touched that tab.
    pub(crate) fn encode_sidebar_tab_sides(&self) -> String {
        crate::state::TerminalSidebarTab::ALL
            .into_iter()
            .filter_map(|t| {
                self.sidebar_tab_sides.get(&t).map(|p| format!("{}:{}", t.code(), p.code()))
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Parse the persisted `sidebar_tab_sides` CSV. Unknown tabs and
    /// malformed pairs are skipped (a downgrade-then-upgrade must not
    /// wipe the surviving choices), and an explicit choice equal to
    /// the tab's default is still kept explicit.
    pub(crate) fn parse_sidebar_tab_sides(
        raw: &str,
    ) -> std::collections::HashMap<
        crate::state::TerminalSidebarTab,
        crate::state::SidebarPlacement,
    > {
        raw.split(',')
            .filter_map(|pair| {
                let (tab, placement) = pair.trim().split_once(':')?;
                Some((
                    crate::state::TerminalSidebarTab::from_code(tab.trim())?,
                    crate::state::SidebarPlacement::from_code(placement.trim())?,
                ))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_download_dir;

    #[test]
    fn configured_download_dir_wins_over_the_os_default() {
        assert_eq!(
            resolve_download_dir("/srv/incoming"),
            std::path::PathBuf::from("/srv/incoming")
        );
        // Settings stores the raw input, so a path typed with stray
        // whitespace must not become a directory nobody has.
        assert_eq!(
            resolve_download_dir("  /srv/incoming  "),
            std::path::PathBuf::from("/srv/incoming")
        );
        // Unset falls through to the OS Downloads dir (or the vault's
        // own), which is environment-dependent; all that is promised
        // here is that it never answers with the empty path.
        assert!(!resolve_download_dir("").as_os_str().is_empty());
    }
}
