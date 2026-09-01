use iced::keyboard;
use iced::widget::svg;
use iced::{Point, Theme};

use oryxis_core::models::connection::Connection;
use oryxis_core::models::group::Group;
use oryxis_core::models::identity::Identity;
use oryxis_core::models::key::SshKey;
use oryxis_vault::VaultStore;

use std::sync::OnceLock;
use uuid::Uuid;

/// Cross-thread hand-off for `--connect <uuid>` CLI argument. Set by
/// `main.rs` before `iced::application` starts; read inside `Oryxis::boot`.
/// Using a `OnceLock` (instead of passing through `boot`) lets us keep
/// iced's zero-arg boot signature.
pub static AUTO_CONNECT: OnceLock<Uuid> = OnceLock::new();

/// Inherited vault master password, populated by `main.rs` when the
/// parent process spawned us with `--inherit-vault` and piped the
/// password through stdin. Used at boot to silently unlock the vault so
/// the user doesn't have to re-type for "Duplicate in New Window".
pub static AUTO_PASSWORD: OnceLock<String> = OnceLock::new();

/// Raw `oryxis://` URL from an OS scheme launch, when no running
/// instance claimed it (see the deep-link block in `main.rs`). Same
/// hand-off shape as [`AUTO_CONNECT`]; parsed and routed by boot.
pub static PENDING_DEEP_LINK: OnceLock<String> = OnceLock::new();
/// `oryxis user@host` target captured from argv at process start, when
/// no running instance was there to forward it to.
pub static PENDING_CONNECT_TARGET: OnceLock<String> = OnceLock::new();

/// True when the process was started in one of the headless harness
/// modes, read from argv in `main.rs` before iced boots. It is the
/// RUNTIME signal, not the `harness` cargo feature: a binary built
/// with that feature still runs the ordinary windowed app, which has a
/// user to serve an update to.
///
/// Boot reads it to leave the release lookup alone, and the cost it
/// avoids is not the request. The batch runner boots the app once per
/// `.ice` file inside ONE process, so a lookup that starts answering
/// with a rate limit turns its client timeout into a fixed tax on
/// every remaining boot, charged while the harness waits for the boot
/// task to settle. A test also has no business depending on a network
/// it cannot control.
pub static HARNESS_ACTIVE: OnceLock<bool> = OnceLock::new();

/// Whether this process is running under the headless harness.
pub fn harness_active() -> bool {
    HARNESS_ACTIVE.get().copied().unwrap_or(false)
}

/// True when this process is currently the primary (owns the system
/// tray icon). Stored as an AtomicBool rather than OnceLock so the
/// child-promotion path can flip it at runtime when the previous
/// primary dies and one of the surviving children takes over the
/// mutex. Dispatchers branch on this every TrayPoll tick to decide
/// whether to read the IPC registry + render the unified Windows
/// section (primary) or just publish their own state row (child).
pub static APP_IS_PRIMARY: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);

use crate::state::{
    ConnectionForm, ConnectionProgress, OverlayState, SettingsSection, TerminalTab, View,
};
use crate::theme::OryxisColors;

// `Message` lives in its own module; re-export so call sites that
// import `crate::app::Message` keep working.
pub use crate::messages::{Message, SettingsMessage, TabsMessage, EditorMessage, KeysMessage, SidebarFilesMessage, MonitorMessage, NetToolsMessage, TmuxMessage, TerminalMessage, SshMessage, CloudMessage, HistoryMessage, McpMessage, NavigationMessage, CommandHistoryMessage, UpdateMessage, ProxyIdentityMessage, PluginMessage, AgentMessage, ZmodemMessage, KnownHostMessage, RemoteDesktopMessage, TrayMessage, SessionGroupMessage, PortForwardMessage, VaultMessage, SnippetMessage, AiMessage, OnboardingMessage, PlayerMessage, ShareMessage, SftpMessage, SyncMessage};

// Layout constants
pub(crate) const DEFAULT_TERM_COLS: u32 = 120;
pub(crate) const DEFAULT_TERM_ROWS: u32 = 40;
/// Default width of the right-side editor drawer; the live value is
/// `Oryxis::panel_width` (drag-resizable, persisted). Layout math must
/// read the field, never this constant.
pub(crate) const PANEL_WIDTH: f32 = 420.0;
/// Clamp band for the drawer resize drag: the floor keeps the form
/// rows usable, the ceiling keeps some content visible next to it.
pub(crate) const PANEL_WIDTH_MIN: f32 = 340.0;
pub(crate) const PANEL_WIDTH_MAX: f32 = 720.0;
pub(crate) const SIDEBAR_WIDTH_COLLAPSED: f32 = 56.0;
/// Width of the vertical nav rail when expanded to show section labels.
pub(crate) const NAV_RAIL_WIDTH_EXPANDED: f32 = 190.0;
pub(crate) const CARD_WIDTH: f32 = 280.0;

// `DashNavItem` moved into the keynav module with the rest of the
// focus-zone types; re-export so existing call sites keep working.
pub(crate) use crate::keynav::DashNavItem;

/// Tab-title prefix for SSM-into-EC2 sessions (`format!("{SSM_TAB_PREFIX}{host}")`).
/// The middle dot is U+00B7 with a space on each side. Shared so the
/// spawn site and the duplicate-tab strip site can never drift, a
/// mismatch would silently break duplicating SSM tabs.
pub(crate) const SSM_TAB_PREFIX: &str = "SSM \u{00b7} ";

/// Fallback monospace font names offered when the system enumeration
/// returns nothing (boot-time scan still in flight, fontdb error, or
/// a stripped-down system with no installed monospace fonts beyond
/// the bundled SauceCodePro Nerd Font).
///
/// `SauceCodePro Nerd Font` is bundled with the binary (see `main.rs`).
/// The rest are looked up from the OS fontconfig; if a name doesn't
/// resolve, cosmic-text falls back gracefully to the system default
/// monospace.
const TERMINAL_FONT_FALLBACK: &[&str] = &[
    "SauceCodePro Nerd Font",
    "JetBrains Mono",
    "Fira Code",
    "Fira Mono",
    "Cascadia Code",
    "Ubuntu Mono",
    "DejaVu Sans Mono",
    "Droid Sans Mono",
    "PT Mono",
    "Andale Mono",
    "Anonymous Pro",
    "Inconsolata",
    "Inconsolata-g",
    "Meslo",
    "Operator Mono Book",
    "Operator Mono Medium",
    "Menlo",
    "Monaco",
    "Consolas",
];

/// Returns the list of monospace fonts available to the picker.
///
/// Builds a fresh `fontdb::Database`, loads the system fonts on this
/// platform, and filters to families that report `monospaced`. The
/// bundled SauceCodePro Nerd Font is always prepended so it's the
/// first option even on systems with rich font libraries.
///
/// On error or empty enumeration we fall back to
/// `TERMINAL_FONT_FALLBACK` so the picker is never empty.
///
/// The scan reads every system font file from disk, which is far too
/// heavy to repeat per frame (the Settings view calls this on every
/// redraw while the Terminal tab is open), so the result is computed
/// once and cached for the process lifetime. Fonts installed while
/// the app is running show up after a restart.
pub(crate) fn enumerate_terminal_fonts() -> &'static [String] {
    &font_scan().names
}

/// The one-time system font scan: the picker's family list plus the
/// weights each family can actually serve.
struct FontScan {
    names: Vec<String>,
    /// Weights (CSS numbers) keyed by family name. Every name a face
    /// reports is a key, not just the one the picker lists: the same
    /// family is spelled differently per language inside patched Nerd
    /// Font builds ("JetBrainsMono Nerd Font" en-US, "JetBrainsMono
    /// NF" en-GB), and fontdb resolves a request against any of them,
    /// so the weights must be findable under any of them too.
    weights: std::collections::HashMap<String, Vec<u16>>,
}

fn font_scan() -> &'static FontScan {
    static FONTS: std::sync::OnceLock<FontScan> = std::sync::OnceLock::new();
    FONTS.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();

        let mut names: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::new();
        let mut weights: std::collections::HashMap<String, Vec<u16>> =
            std::collections::HashMap::new();
        for face in db.faces() {
            // Weights are collected BEFORE the monospace filter, and
            // under every alias. Font matching consults neither: it
            // resolves a family by any of its names and then picks the
            // nearest weight, so a face this filter drops is still a
            // face the renderer will draw. Skipping it here would make
            // the picker claim a weight is missing while the terminal
            // renders at it.
            for (alias, _lang) in &face.families {
                let alias = alias.trim();
                if alias.is_empty() {
                    continue;
                }
                let entry = weights.entry(alias.to_string()).or_default();
                if !entry.contains(&face.weight.0) {
                    entry.push(face.weight.0);
                }
            }
            // The picker list itself IS monospace-only: it is what the
            // user chooses a terminal font from.
            if !face.monospaced {
                continue;
            }
            if let Some((family, _lang)) = face.families.first() {
                // Filter out empty / placeholder names defensively; some
                // systems carry symbol-only faces marked monospace.
                let trimmed = family.trim();
                if !trimmed.is_empty() {
                    names.insert(trimmed.to_string());
                }
            }
        }
        // The bundled and downloadable families never reach the system
        // database (they are loaded straight into the iced font
        // system), so their faces are declared rather than scanned.
        for (family, ws) in crate::fonts::BUNDLED_MONO_WEIGHTS {
            let entry = weights.entry((*family).to_string()).or_default();
            for w in *ws {
                if !entry.contains(w) {
                    entry.push(*w);
                }
            }
        }
        for pack in crate::fonts::PACK_FONTS {
            let entry = weights.entry(pack.family.to_string()).or_default();
            for face in pack.faces {
                if !entry.contains(&face.weight) {
                    entry.push(face.weight);
                }
            }
        }

        // Prepend the bundled family (always picker entry #1) and the
        // downloadable pack families (issue #109) regardless of what
        // the system scan returned. cosmic-text resolves all of them
        // by family name: the bundled one is registered in main.rs,
        // the pack ones via `iced::font::load` when picked (an
        // un-downloaded pack font is still listed; selecting it
        // triggers the download). Also applied to the scan-failed
        // fallback path, so the guaranteed-loadable families never
        // vanish from the picker on a system fontdb can't read.
        let mut head: Vec<&str> = vec!["SauceCodePro Nerd Font"];
        head.extend(crate::fonts::PACK_FONTS.iter().map(|p| p.family));

        if names.is_empty() {
            return FontScan {
                names: head
                    .iter()
                    .map(|s| s.to_string())
                    .chain(TERMINAL_FONT_FALLBACK.iter().map(|s| s.to_string()))
                    .collect(),
                weights,
            };
        }

        let mut out: Vec<String> = Vec::with_capacity(names.len() + head.len());
        for b in &head {
            out.push((*b).to_string());
        }
        for n in names {
            if !head.contains(&n.as_str()) {
                out.push(n);
            }
        }
        FontScan { names: out, weights }
    })
}

/// Whether `family` can render at `weight` (a CSS number).
///
/// False only when the family's faces are actually known AND none of
/// them is at least as heavy as the request: cosmic-text has no
/// synthetic emboldening, so that is precisely the case where picking
/// a heavier weight changes nothing on screen. A family nothing knows
/// about (a system fontdb that failed to read, a name typed into the
/// settings row by hand) is given the benefit of the doubt rather
/// than warned about.
pub(crate) fn terminal_font_serves_weight(family: &str, weight: u16) -> bool {
    match font_scan().weights.get(family) {
        Some(ws) => ws.iter().any(|w| *w >= weight),
        None => true,
    }
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

pub struct Oryxis {
    // Vault
    pub(crate) vault: Option<VaultStore>,
    pub(crate) vault_ui: crate::state::VaultUi,
    // Vector logo handles (see boot.rs). SVG goes through iced's
    // resvg/tiny-skia path instead of the wgpu image atlas, which on
    // GNOME Wayland fractional scaling corrupted the raster PNG into
    // garbage once the window got a real app_id and was composited at a
    // non-integer scale. The small/large split is kept for call-site
    // clarity even though both now point at the same asset.
    pub(crate) logo_handle: svg::Handle,

    // Data
    pub(crate) connections: Vec<Connection>,
    /// Ad-hoc quick-connect hosts, keyed by connection id. Never touches
    /// the vault or `connections` (so no grid / sync / export leakage).
    /// Entries carry the credentials typed in the editor flow so a
    /// reconnect can reuse them; swept on vault lock, pruned when the
    /// last pane referencing an entry closes.
    pub(crate) quick_connects: std::collections::HashMap<Uuid, crate::state::QuickConnectEntry>,
    pub(crate) groups: Vec<Group>,
    /// Saved split-panel arrangements. Each references hosts by id and/or
    /// local shells; opening one rebuilds a single splitted tab.
    pub(crate) session_groups: Vec<oryxis_core::models::SessionGroup>,

    // UI state
    pub(crate) active_view: View,
    pub(crate) active_group: Option<Uuid>,  // None = root, Some(id) = inside folder
    pub(crate) host_search: String,
    /// When set, the dashboard grid hides every host / group whose
    /// cloud origin doesn't match this profile id. Activated by
    /// clicking the small provider badge on a cloud-sourced host card,
    /// cleared from the chip at the top of the grid. None means no
    /// cloud filter.
    pub(crate) host_filter_cloud_profile: Option<Uuid>,
    /// Dashboard tag filter: only hosts carrying AT LEAST ONE of these
    /// tags (case-insensitive) are listed, and only groups whose
    /// subtree contains such a host render. In-memory like the search
    /// needle, not persisted; empty = no filter.
    pub(crate) host_filter_tags: Vec<String>,
    pub(crate) quick_host_input: String,

    // Tabs
    pub(crate) tabs: Vec<TerminalTab>,
    /// Tabs the user closed, newest last, capped: the stack
    /// `ReopenClosedTab` pops (issue #186). Terminal and SFTP tabs share
    /// it, so the chord always brings back the last chip that left the
    /// strip whichever kind it was. See [`crate::state::ClosedTab`] for
    /// why it holds a pin spec and why it is not persisted.
    pub(crate) closed_tabs: Vec<crate::state::ClosedTab>,
    /// Where the tab a Duplicate is about to spawn should land in the
    /// STRIP (never in `self.tabs`, whose indices half the app holds).
    /// Armed by `handle_duplicate_tab`, consumed by
    /// `reconcile_tab_order` when the new tab's id first shows up. See
    /// [`crate::state::PendingTabPlacement`].
    pub(crate) pending_tab_placement: Option<crate::state::PendingTabPlacement>,
    /// Set while the new-tab picker is open *to fill a split pane* rather
    /// than open a new tab: `(tab_id, pane_to_split, axis)`. The picker's
    /// selection (host or local shell) lands in a new pane next to the
    /// target instead of a new tab. `None` = picker opens new tabs.
    ///
    /// Keyed by tab ID, not by index, for the same reason
    /// `PendingTabPlacement` is: every path that removes a tab shifts the
    /// positions held elsewhere and fixes them up by hand, and this one
    /// was never in that list. A stale index would not fail loudly either,
    /// because `pane_grid::Pane` is a per-grid counter and every grid has
    /// a `Pane(0)`: the split would land in the wrong tab, beside the
    /// wrong pane. An id that no longer resolves simply opens a tab.
    pub(crate) pending_pane_split:
        Option<(uuid::Uuid, iced::widget::pane_grid::Pane, iced::widget::pane_grid::Axis)>,
    /// Protocol the quick-connect card dials when the typed text names
    /// no scheme (issue #174).
    ///
    /// A `telnet://` prefix always wins over this: the text is what the
    /// user wrote, and a picker that could contradict it would be a
    /// second source of truth. This only answers the question the text
    /// left open, and resets to SSH when the search box empties, so a
    /// choice made for one switch cannot silently follow an unrelated
    /// host typed ten minutes later.
    pub(crate) quick_connect_protocol: oryxis_core::models::connection::ConnectionProtocol,
    /// Startup commands of local hosts, parked until their pane has
    /// produced output and then gone QUIET.
    ///
    /// A local shell has no "session ready" event to hang the command
    /// on the way SSH does, and a PTY written to at spawn time is a
    /// shell that has not read its first byte yet. Waiting for the
    /// first byte is not enough either: a login shell prints a MOTD in
    /// several batches, and typing into the middle of that makes the
    /// command echo halfway up the banner. So each batch re-arms a
    /// short timer and the command goes out when one expires without a
    /// newer batch, which is as close to "the prompt is up" as a shell
    /// that reports nothing can get.
    ///
    /// Keyed by pane so two panes running the same host stay
    /// independent; the counter is what tells a stale timer from the
    /// live one, and the entry is removed on fire so a shell that keeps
    /// printing can never re-run it.
    pub(crate) pending_local_startup:
        std::collections::HashMap<uuid::Uuid, crate::state::PendingLocalStartup>,
    /// True while the cursor is over the `+` split popover itself. Lets the
    /// hover bridge keep the menu open when moving from the `+` button into
    /// the menu, and close it shortly after the cursor leaves both.
    pub(crate) split_menu_hovered: bool,
    pub(crate) active_tab: Option<usize>,
    /// Last terminal tab that had focus. Preserved when switching to nav-only
    /// views (Snippets, Keys, …) so snippet injection still targets that session.
    pub(crate) last_terminal_tab: Option<usize>,
    pub(crate) new_tab_picker_search: String,
    /// When set, the new-tab picker is drilled into this group, showing
    /// its members (or, for a cloud-query group, its resolved ECS tasks /
    /// K8s pods) instead of the top-level group + recent list. `None` is
    /// the top level. Reset to `None` whenever the picker opens or closes.
    pub(crate) new_tab_picker_group: Option<Uuid>,
    pub(crate) tab_jump_search: String,
    /// Command palette (C4): `Ctrl+Shift+P` fuzzy search over every
    /// action. Only the open flag + query are app state; the row
    /// selection rides the modal keynav layer
    /// (`ModalSurface::Modal(Modal::CommandPalette)`).
    pub(crate) palette: crate::state::PaletteState,


    /// Icon + color picker state (target routing, current selection,
    /// search). The open flag (`show_icon_picker`) and the HSV popover
    /// anchor (`icon_color_popover`) stay alongside it on `Oryxis`.
    pub(crate) icon_picker: crate::state::IconPickerState,
    /// When set, the icon picker's HSV color popover is open, anchored at
    /// this point (the cursor position when the swatch was clicked). None
    /// keeps the picker collapsed behind the swatch + hex row.
    pub(crate) icon_color_popover: Option<iced::Point>,
    /// Current slide index of the first-run onboarding carousel
    /// (`VaultState::NeedSetup`).
    pub(crate) onboarding_slide: usize,
    /// The onboarding import slide asked to import hosts. The vault
    /// does not exist while that screen is up, so the intent waits
    /// here and both vault-creation paths consume it.
    pub(crate) onboarding_import_pending: bool,
    pub(crate) chain_editor_adding: bool,
    pub(crate) chain_editor_search: String,
    pub(crate) connecting: Option<ConnectionProgress>,
    /// Counter that advances ~every 100ms while a connection is in progress.
    /// Used only to drive the pulsing "loading" ring on the active step dot.
    pub(crate) connect_anim_tick: u32,
    /// Frame counter for the tab strip's running-command indicator
    /// (issue #146), advanced by `TabsMessage::BusyAnimTick` while its
    /// subscription is mounted (some pane has a command in flight).
    pub(crate) busy_anim_tick: u32,
    /// Timestamp of the last `WindowDrag` / `WindowResizeDrag` we
    /// forwarded to the OS. iced's `MouseArea` fires `on_press` on
    /// **both** clicks of a double-click (before the `on_double_click`
    /// lands), and forwarding two `iced::window::drag(...)` calls in
    /// quick succession leaves the OS in a flaky state, Windows races
    /// it with our follow-up `toggle_maximize` / `resize` and the
    /// window snaps right back. We swallow press handlers within a
    /// short window after the first one to keep the double-click path
    /// clean.
    pub(crate) last_window_press_at: Option<std::time::Instant>,

    // MODAL FIELDS: the booleans / options below (and others scattered in
    // this struct: ui_theme_editor, show_theme_import, show_share_dialog,
    // cloud_import_confirm_visible, folder_rename, show_*_picker, ...) each
    // drive a modal overlay and remain the single source of truth for
    // whether that modal is open. They are now enumerated by
    // `crate::state::Modal`: `Oryxis::is_modal_open` / `close_modal`
    // (shortcuts.rs) are exhaustive matches over every variant, so
    // `any_modal_blocks_input` / `close_topmost_modal` can no longer miss a
    // modal (a new one fails to compile until it has a `Modal` variant +
    // both match arms). Render every blocking modal through
    // `widgets::modal_overlay` so the scrim can't reintroduce mouse
    // bleed-through.

    // Legacy-algorithm fallback dialog (server offers only cbc/sha1/...).
    pub(crate) pending_legacy_algo: Option<crate::state::PendingLegacyAlgo>,
    // Host key verification dialog.
    pub(crate) pending_host_key: Option<oryxis_ssh::HostKeyQuery>,
    // Staging slot: each connect writes its host-key responder here at
    // start. It is *consumed* into `active_host_key_tx` the moment the
    // prompt is shown (see `SshHostKeyVerify`), so a second connect that
    // starts while a prompt is up overwrites only the staging slot, never
    // the responder bound to the visible query.
    pub(crate) host_key_response_tx: Option<tokio::sync::mpsc::Sender<bool>>,
    // Responder paired with the currently-displayed `pending_host_key`.
    // The accept / reject handlers answer this, so the user's decision can
    // never be routed to a different connect's host (TOFU bypass).
    pub(crate) active_host_key_tx: Option<tokio::sync::mpsc::Sender<bool>>,

    // Command-proxy approval dialog, and the same staging / active pair
    // the host-key prompt uses, for the same reason: the answer must go
    // back to the dial whose command the user actually read, not to
    // whichever connect started last.
    pub(crate) pending_proxy_command: Option<oryxis_ssh::ProxyCommandQuery>,
    pub(crate) proxy_command_response_tx: Option<tokio::sync::mpsc::Sender<bool>>,
    pub(crate) active_proxy_command_tx: Option<tokio::sync::mpsc::Sender<bool>>,

    // Keyboard-interactive (2FA / OTP) prompt dialog. `pending_kbi_prompt`
    // is the current challenge round; `kbi_inputs` holds one answer buffer
    // per prompt (parallel to `prompts`); the response channel carries
    // `Some(answers)` on submit or `None` on cancel back to the engine.
    pub(crate) pending_kbi_prompt: Option<oryxis_ssh::KbiQuery>,
    pub(crate) kbi_inputs: Vec<String>,
    pub(crate) kbi_response_tx: Option<tokio::sync::mpsc::Sender<Option<Vec<String>>>>,
    // Quick-connect entry id bound to the displayed KBI prompt (`None` for
    // saved hosts). Unlocks the "use a saved identity / key instead"
    // selector in the prompt modal; set/cleared together with the prompt.
    pub(crate) pending_kbi_quick: Option<Uuid>,
    // Armed when the user picks an identity / key mid-prompt
    // (`QuickAuthSwitch` cancels the parked auth attempt): the resulting
    // connect error is consumed as "retry with the mutated entry" instead
    // of surfacing as a failure.
    pub(crate) pending_auth_switch: Option<Uuid>,
    // Armed when "Edit host" tears down a still-live connect (prompt
    // cancelled / dial abandoned): the provoked error arrives after the
    // progress card is gone and would otherwise land inside the editor
    // as `host_panel_error`. Consumed by the next `SshError` that finds
    // no progress card; a fresh connect clears the ambiguity because it
    // sets `connecting` again.
    pub(crate) pending_edit_cancel: bool,

    pub(crate) editor_form: ConnectionForm,
    /// Multi-line buffer for the host's initial command. Kept out of the
    /// form struct because `text_editor::Content` isn't Clone.
    pub(crate) editor_initial_command: iced::widget::text_editor::Content,
    pub(crate) host_panel_error: Option<String>,
    /// Which host-editor sections are expanded. Session-scoped UI state
    /// (never persisted) and deliberately NOT part of `editor_form`, so
    /// switching between hosts keeps the sections the user was in.
    pub(crate) host_editor_open_sections:
        std::collections::HashSet<crate::state::HostEditorSection>,

    pub(crate) editor_session_group: crate::state::SessionGroupForm,
    /// Multi-line buffer for the currently-shown pane's startup script. Kept
    /// out of the form struct because `text_editor::Content` isn't Clone.
    pub(crate) session_group_script_editor: iced::widget::text_editor::Content,
    pub(crate) session_group_panel_error: Option<String>,
    /// Per-pane initial-script overrides, keyed by the pane's stable id.
    /// Populated when a session group is opened; consumed (and removed)
    /// once the pane's shell is ready and the script is injected. Lets the
    /// override win over the host's own `initial_command` for that pane.
    pub(crate) pane_script_overrides: std::collections::HashMap<Uuid, String>,

    /// Unified vault-area keyboard navigation: active focus zone +
    /// selected item, plus the per-zone item lists recorded during
    /// render. Replaces the old dashboard-only `selected_nav` /
    /// `dashboard_nav` pair; see `keynav/mod.rs` for the model and
    /// `dispatch_keynav.rs` for the key router.
    pub(crate) keynav: crate::keynav::KeyNavState,
    /// One-shot preferred directory for the next SFTP mount, set by the
    /// sidebar Files "expand" promotion and consumed (with home-dir
    /// fallback) by the mount pipeline's `initial_remote_listing`.
    pub(crate) sftp_open_at_path: Option<String>,
    /// One-shot "show Files once this tab has a session", by tab id.
    ///
    /// "Open SFTP session" on a tab that is still DIALLING (a dormant
    /// pinned tab the same click just reopened, most of all) used to
    /// reconnect and then silently drop the half the user actually asked
    /// for: the handler needs a live SSH session and there is none yet.
    /// `SshConnected` consumes this and flips the mode then. Sibling of
    /// `sftp_open_at_path`, and one-shot for the same reason: a hint that
    /// outlived its request would open Files on some unrelated reconnect.
    pub(crate) pending_files_mode: Option<Uuid>,
    /// Where a console about to open should start, when the surface that
    /// asked for it knew (a console opened beside a live tab inherits
    /// that tab's OSC 7 working directory). One-shot for the same reason
    /// as its two siblings above: a hint that outlived its request would
    /// land a later console somewhere nobody asked for.
    pub(crate) pending_console_dir: Option<String>,
    /// Set by `open_sftp_console` and consumed by `start_ssh_tab` when it
    /// builds the pane, which is the only moment a pane's purpose can be
    /// decided before its dial carries it. Paired with
    /// `pending_console_dir`.
    pub(crate) pending_console_purpose: bool,
    /// Click counter behind the deferred slow-click rename: every row
    /// click, right-click and navigation bumps it, and a fire whose
    /// armed generation no longer matches gives up (see
    /// `dispatch_sftp::selection`). Lives HERE, not in `SftpState`: a
    /// per-state counter rides the park/hoist tab swap, and two tabs
    /// would then compare each other's numbers.
    pub(crate) sftp_click_gen: u64,
    /// Open reopen-or-redownload dialog: the user asked to open a remote
    /// file that is already being edited. Not in `SftpState` either, for
    /// the same reason plus one of its own: the collision can be raised
    /// from the sidebar Files browser, which has no SFTP state at all,
    /// and a parked dialog would strand unanswered.
    pub(crate) sftp_edit_reopen: Option<crate::state::EditReopenPrompt>,
    /// Loaded command history for `command_history_host`, most recent
    /// first (the History tab derives its "frequent" shortlist from this).
    pub(crate) command_history: Vec<oryxis_vault::CommandHistoryEntry>,
    /// Which saved host `command_history` was loaded for (`None` when the
    /// focused pane isn't a saved host).
    pub(crate) command_history_host: Option<Uuid>,
    /// Filter text of the sidebar History tab's search field (distinct
    /// from `history_search`, which filters the session-logs view).
    pub(crate) cmd_history_search: String,
    /// Snippet card whose `⋮` context menu (Edit / Delete) is open;
    /// keeps the kebab visible while the popup is up, mirroring
    /// `card_context_menu` for hosts.
    pub(crate) snippet_context_menu: Option<usize>,
    pub(crate) card_context_menu: Option<uuid::Uuid>,

    // Floating overlay menu
    pub(crate) overlay: Option<OverlayState>,
    /// Folder rename inline editor, `Some((group_id, current_input))`
    /// while the modal is open.
    pub(crate) folder_rename: Option<(Uuid, String)>,
    /// Tab rename dialog, `Some((tab_ref, current_input))` while open.
    /// Addressed by `TabRef` (stable uuid) so a reorder mid-dialog can't
    /// retarget the rename. The committed name is transient: it lives on
    /// the tab struct only, never on the host or the pin spec.
    pub(crate) tab_rename: Option<(crate::state::TabRef, String)>,
    /// Multi-line clipboard text parked by the careful-paste guard,
    /// waiting for the user to confirm or cancel the paste, with the tab
    /// index it is destined for. The target is captured when the paste is
    /// requested rather than resolved on confirm: see
    /// `dispatch_terminal::paste_text_into_tab`.
    pub(crate) pending_paste: Option<(uuid::Uuid, String)>,
    /// Set alongside `pending_paste` when the parked text is an INSTALL
    /// script (issue #147): `(snippet id, run)`. The confirm then sends
    /// through the snippet injection (newline outside the bracketed
    /// paste, so Run actually executes) and records the run into the
    /// host's install memory; an ordinary paste park clears it.
    pub(crate) pending_paste_install: Option<(uuid::Uuid, bool)>,
    /// A file row press on its way to becoming an OS drag-out (issue
    /// #167): crossing [`crate::drag_out::DRAG_THRESHOLD`] with the
    /// button held raises the ghost and resolves the payload, leaving
    /// the window hands it to the OS; the global left-release disarms.
    pub(crate) drag_out_arm: Option<crate::drag_out::DragOutArm>,
    /// Paths handed to the OS by the last local drag-out, held so a
    /// release back inside our own window isn't re-imported as a drop
    /// onto ourselves. Each path is forgotten as its drop arrives, so
    /// the guard empties itself (see `update`).
    pub(crate) drag_out_echo: Vec<std::path::PathBuf>,
    /// Paths from an in-flight OS drop onto the terminal, buffered so a
    /// multi-file gesture (one `FileDropped` per file) becomes one routed
    /// batch. Flushed by `TerminalDropFlush` after a short debounce; the
    /// target pane is resolved once, at flush.
    pub(crate) pending_terminal_drops: Vec<std::path::PathBuf>,
    /// True while the OS is dragging files over the window (issue #167).
    /// Mirrors `sftp.drop_active` for the surfaces the SFTP flag does
    /// not reach: the sidebar Files browser reads it to show its
    /// drop-to-upload hint. Display only, exactly like `drop_active`;
    /// the drop itself is never gated on it (a missed HoveredLeft would
    /// kill real gestures).
    pub(crate) os_drop_hover: bool,
    /// Manual host-group editor side panel (label + icon + color). Open
    /// when `group_edit_visible`; `group_edit_id` is the group being
    /// edited. `group_edit_icon` / `group_edit_color` are empty strings
    /// when unset (no override → folder default glyph / accent).
    pub(crate) group_edit: crate::state::GroupEditForm,
    /// Folder delete confirmation, group ID waiting for the user to
    /// pick "move hosts to root" / "delete with hosts" / cancel.
    pub(crate) folder_delete: Option<Uuid>,
    /// Connection ID to auto-open after the vault unlocks. Set from the
    /// `--connect` CLI flag captured at process start; cleared once the
    /// dispatch fires so a vault re-lock + unlock doesn't re-trigger it.
    pub(crate) pending_auto_connect: Option<Uuid>,
    /// Deep link waiting for the vault to unlock (a `oryxis://` click
    /// at the lock screen, or one that arrived while soft-locked).
    /// Drained by boot and by the unlock handler, like `--connect`.
    pub(crate) pending_deep_link: Option<crate::deep_link::DeepLink>,
    /// `oryxis user@host` target waiting for the vault to unlock. Kept
    /// apart from `pending_deep_link` because the two route differently
    /// (this one dials, a `ssh://` link only prefills), and merging them
    /// would make provenance a guess at drain time.
    pub(crate) pending_connect_target: Option<String>,
    /// Master password retained in memory for spawning child processes
    /// (Duplicate in New Window). Populated after a successful
    /// unlock / setup, cleared if the user explicitly re-locks.
    pub(crate) master_password: Option<String>,
    /// SFTP browser state of the **active** SFTP tab. A working buffer:
    /// the focused SFTP tab's live state lives here, the others park their
    /// state in `SftpTab::state` (swap-on-focus). With no SFTP tab focused
    /// this still holds the last-focused tab's state until it is parked.
    /// Which card / row / chip the cursor is over, for the floating
    /// hover-revealed actions every list uses. Twenty fields until
    /// they moved here; see `state::hover`.
    /// See [`crate::state::ThemeEditorUi`].
    pub(crate) theme_ui: crate::state::ThemeEditorUi,
    /// See [`crate::state::VaultImportState`].
    pub(crate) vault_import: crate::state::VaultImportState,
    /// See [`crate::state::ChatUi`].
    pub(crate) chat_ui: crate::state::ChatUi,
    /// Everything the user can change and the vault persists in its
    /// `settings` table, in one place instead of 112 fields.
    ///
    /// Named `prefs`, not `settings`, because the `settings_*`
    /// fields that stay behind are the Settings SCREEN's own state
    /// (open section, scroll offset, search) and belong to the view,
    /// not to the user's configuration.
    /// See [`crate::state::PanelsOpen`].
    pub(crate) panels: crate::state::PanelsOpen,
    /// See [`crate::state::CloudDiscoverUi`].
    pub(crate) cloud_discover: crate::state::CloudDiscoverUi,
    /// See [`crate::state::SnippetForm`].
    pub(crate) snippet_form: crate::state::SnippetForm,
    /// See [`crate::state::KeysUi`].
    pub(crate) keys_ui: crate::state::KeysUi,
    /// See [`crate::state::SftpChrome`].
    pub(crate) sftp_chrome: crate::state::SftpChrome,
    pub(crate) prefs: crate::state::AppPrefs,
    pub(crate) hover: crate::state::HoverState,
    pub(crate) sftp: crate::state::SftpState,
    /// Open SFTP browser tabs. Share the unified strip with terminal tabs.
    /// The active tab's live state is hoisted to `self.sftp`; inactive tabs
    /// hold their state in `SftpTab::state`. See `sftp_buf_mut`.
    pub(crate) sftp_tabs: Vec<crate::state::SftpTab>,
    /// Index into `sftp_tabs` of the focused SFTP tab, or `None` when no
    /// SFTP tab is focused. Invariant: at most one of `active_tab` /
    /// `active_sftp` is `Some`.
    pub(crate) active_sftp: Option<usize>,
    /// Hybrid tab (issue #61): id of the terminal tab whose Files-mode
    /// SFTP state currently lives in the `self.sftp` buffer. Mutually
    /// exclusive with `active_sftp` (hoisting one parks the other);
    /// see `park_hybrid_sftp` / `hoist_hybrid_sftp`.
    pub(crate) hybrid_sftp_owner: Option<Uuid>,
    /// Unified left-to-right order of the tab strip (terminal + SFTP). Both
    /// vecs (`tabs`, `sftp_tabs`) are id-addressed storage; this list drives
    /// display order and drag-reorder across the terminal/SFTP boundary.
    pub(crate) tab_order: Vec<crate::state::TabRef>,
    /// Most-recently-used order of open tabs (front = most recent), driving
    /// Ctrl+Tab "switch by last use". Terminal + SFTP tabs only; Home is not a
    /// member (it stays on Ctrl+1 / Alt+arrow). Maintained by
    /// `reconcile_tab_mru` after every message. See `tab_cycle.rs`.
    pub(crate) tab_mru: Vec<crate::state::TabRef>,
    /// In-progress Ctrl+Tab run: `Some` from the first Ctrl+Tab press until
    /// Ctrl is released. Holds the MRU snapshot the run walks so previews
    /// don't disturb the live order until the choice is committed. See
    /// `tab_cycle.rs`.
    pub(crate) tab_cycle: Option<crate::tab_cycle::TabCycle>,
    /// Set for the duration of an SFTP async-continuation dispatch to the id
    /// of the owning tab (whose state is temporarily swapped into `self.sftp`).
    /// Lets handlers stamp re-emitted continuation messages with the right
    /// owner instead of the focused tab. `None` outside such a dispatch.
    pub(crate) routing_sftp: Option<Uuid>,
    /// SFTP close pending a confirmation: set when the user tries to close a
    /// tab (or "close others") where some affected tab has an in-flight
    /// transfer or an unsaved edit-session. Drives the close-guard modal;
    /// `None` when no confirmation is pending.
    pub(crate) pending_sftp_close: Option<crate::state::PendingSftpClose>,
    /// Folder the last picked download destination lived in, for this
    /// run only. `rfd` applies a starting directory with
    /// `IFileDialog::SetFolder`, which OVERRIDES the shell's own
    /// last-used memory, so a surface that names a starting folder has
    /// to carry that memory itself or every dialog reopens on the
    /// configured default. `None` until the first pick, where the
    /// default download folder is used instead.
    pub(crate) last_download_dir: Option<std::path::PathBuf>,
    pub(crate) mouse_position: Point,
    pub(crate) window_size: iced::Size,
    /// The last size the window had while plain-windowed (not maximized,
    /// not fullscreen). This is what `persist_window_geometry` writes to
    /// the settings table so the next launch restores the floating size
    /// rather than whatever monitor-sized rectangle the window occupied
    /// at close. Committed by `WindowMaximizedSynced` (which carries the
    /// resize that triggered it) once the OS has confirmed the window is
    /// not maximized: judging against the optimistic flag in the resize
    /// handler let an OS-side maximize record its monitor-sized
    /// rectangle here before the reconcile landed.
    pub(crate) window_windowed_size: iced::Size,
    /// The last outer position the window had while plain-windowed, in
    /// logical desktop coordinates (negative on monitors left of / above
    /// the primary, which is how a multi-monitor placement round-trips).
    /// Same skip rules as `window_windowed_size`, plus a filter for the
    /// bogus (-32000, -32000) position Windows parks minimized windows
    /// at. `None` until the first `Moved` event; stays `None` for the
    /// whole session on Wayland, where window positions don't exist, so
    /// nothing is persisted and the next launch uses the WM's placement.
    pub(crate) window_windowed_pos: Option<Point>,
    /// The value `window_windowed_pos` held before its last write. An
    /// OS-side maximize (Win+Up, aero snap) parks the window at the
    /// monitor origin while `window_maximized` is still stale-false, so
    /// the accompanying `Moved` overwrites the real windowed position;
    /// the `WindowMaximizedSynced` reconcile rolls back to this slot
    /// when it detects that drift.
    pub(crate) window_windowed_pos_prev: Option<Point>,
    /// Whether the OS window currently has focus. Driven by the
    /// `Focused` / `Unfocused` window events. The cloud SSM/ECS
    /// keepalive only ticks while this is `false` (the user alt-tabbed
    /// away), since an active session resets the SSM idle timer on its
    /// own via the user's input.
    pub(crate) window_focused: bool,
    /// Terminal size `(cols, rows)` captured the moment the window lost
    /// focus, used as the anchor the SSM keepalive toggles around (it
    /// resizes to `rows - 1` and back so each tick produces a real
    /// SIGWINCH, which is what resets the SSM idle timer). `None` while
    /// focused.
    pub(crate) ssm_keepalive_base: Option<(u16, u16)>,
    /// Live keyboard modifier state, updated from `ModifiersChanged`
    /// keyboard events. Used by SFTP click logic for ctrl/shift-click
    /// selection, iced's MouseArea events don't include modifiers.
    pub(crate) modifiers: keyboard::Modifiers,
    /// Which physical Alt (Option) side is held, tracked from the Alt
    /// key's own press/release events because `Modifiers` can't tell the
    /// sides apart. Drives the macOS `OptionAsMeta` per-side quirk;
    /// cleared by `ModifiersChanged` without Alt and by focus loss so a
    /// swallowed release can't wedge a side down.
    pub(crate) alt_sides: crate::key_encode::OptionSides,
    /// Debounce stamp for the PrintScreen -> Snipping Tool remap. winit
    /// can deliver both a press and a release for VK_SNAPSHOT; we launch
    /// on either and use this to avoid firing the snip overlay twice.
    #[cfg(target_os = "windows")]
    pub(crate) last_printscreen: Option<std::time::Instant>,
    /// Whether the OS window is currently maximized. Used by the custom
    /// chrome to swap the maximize glyph for a "restore" glyph. Flipped
    /// optimistically by `WindowMaximizeToggle` and reconciled with the
    /// OS truth by `WindowMaximizedSynced` after a `WindowResized`
    /// (Win+Up/Down, aero snap and dragging the custom title bar down
    /// all change the OS state without firing a toggle message).
    pub(crate) window_maximized: bool,
    /// Whether the window is in native fullscreen mode. Flipped by F11.
    /// Same optimistic pattern as `window_maximized` because the OS-side
    /// transition is one-way from the app.
    pub(crate) window_fullscreen: bool,
    /// True for ~3 s after entering fullscreen so the "Press F11 to
    /// exit" banner renders. Cleared by a scheduled
    /// `Message::Tabs(TabsMessage::FullscreenHintHide)`. Mirrors Chrome / Firefox where
    /// the on-enter hint fades on its own and the X close affordance
    /// then only shows on top-edge hover.
    pub(crate) fullscreen_hint_visible: bool,
    /// Active hotkey bindings: defaults overlaid with user overrides
    /// loaded from the settings table. Mutated by the Shortcuts
    /// editor; read on every `KeyboardEvent` in dispatch_terminal.
    pub(crate) hotkey_bindings: crate::hotkeys::HotkeyMap,
    /// Action currently being re-bound from Settings → Shortcuts.
    /// `Some` puts the keyboard handler in capture mode: the next
    /// KeyPressed becomes the new binding (Esc cancels). `None` is
    /// normal dispatch.
    /// Which action, and which chord of its list, the Shortcuts editor
    /// is capturing for. `None` when no capture is live.
    pub(crate) editing_hotkey:
        Option<(crate::hotkeys::HotkeyAction, crate::hotkeys::HotkeySlot)>,

    // Keys
    pub(crate) keys: Vec<SshKey>,
    /// Parsed certificate on display in the read-only cert viewer modal
    /// (B2). `Some` = modal open; keyed to `Modal::CertificateViewer`.
    pub(crate) cert_viewer: Option<crate::state::CertViewerData>,
    /// Workspace-mode contextual search backing for Snippets view.
    /// Matches against snippet label + command.
    pub(crate) snippet_search: String,
    /// Workspace-mode contextual search backing for History view.
    /// Matches against the connection label / hostname recorded in
    /// each log row.
    pub(crate) history_search: String,
    /// History view: also search inside recorded session content
    /// (typed commands + output), the toggle chip inside the search
    /// field. Session-scoped UI state, not persisted.
    pub(crate) history_search_content: bool,
    /// Async results + scan progress for the content search above.
    pub(crate) history_content: crate::state::HistoryContentSearch,
    /// History view host-tag filter (multi-select, matches the host
    /// tags of each row's connection), mirroring `host_filter_tags`;
    /// empty = off.
    pub(crate) history_filter_tags: Vec<String>,

    // Identities
    pub(crate) identities: Vec<Identity>,
    // Cached set of identity ids whose `password` column is non-NULL.
    // Hydrated by `load_data_from_vault`. The keychain view reads this
    // per card to decide whether to render the masked-bullets badge,
    // a per-frame `get_identity_password` decrypt would otherwise run
    // for every identity on every view() rebuild and slow the main
    // loop enough to fill iced's 100-slot subscription channel.
    pub(crate) identities_with_password: std::collections::HashSet<Uuid>,
    pub(crate) identity_form: crate::state::IdentityForm,
    pub(crate) identity_context_menu: Option<usize>,

    // Per-list sort modes for the Hosts / Keychain / Snippets grids.
    // Persisted via the `hosts_sort` / `keys_sort` / `snippets_sort`
    // settings keys; loaded on boot and rewritten on each pick. The
    // active value drives both the trigger button's glyph and the
    // check mark in the dropdown.
    pub(crate) hosts_sort: crate::state::ListSort,
    pub(crate) keys_sort: crate::state::ListSort,
    pub(crate) snippets_sort: crate::state::ListSort,

    // Proxy Identities, reusable proxy configs edited inline inside
    // the Settings → Proxies section. The saved list lives here; the
    // inline editor's transient state is grouped in `proxy_identity_form`
    // (in-memory only until SaveProxyIdentity flushes to the vault).
    pub(crate) proxy_identities: Vec<oryxis_core::models::proxy_identity::ProxyIdentity>,
    pub(crate) proxy_identity_form: crate::state::ProxyIdentityForm,

    // Login scripts: reusable expect/send automations for hosts behind
    // an interactive bastion, referenced by `Connection.login_script_id`.
    // Created from the host editor (where the user is already looking at
    // the host that needs one) and managed in Settings → Connection.
    pub(crate) login_scripts: Vec<oryxis_core::models::LoginScript>,
    /// Bumped per armed run so a stale timeout tick from a finished
    /// run cannot abort the one that replaced it.
    pub(crate) login_script_generation: u64,
    pub(crate) login_script_form: crate::state::LoginScriptForm,
    /// Resolved + compiled highlight rules per host, keyed by connection
    /// id and validated by a signature of its inputs (see
    /// `highlight_rules_for`), so the entry can never go stale.
    /// A `RefCell` because BOTH consumers need it and one of them is
    /// `view()`: the widget paints from the same resolved set the
    /// backend watches with, and a rule that coloured one pattern while
    /// firing on another would be the worst kind of bug here. Same
    /// pattern as the keynav recording.
    pub(crate) highlight_rules_cache: std::cell::RefCell<
        std::collections::HashMap<uuid::Uuid, (u64, std::sync::Arc<oryxis_terminal::CompiledRules>)>,
    >,
    /// Settings > Terminal: the inline editor for one highlight rule.
    /// The list itself is a preference (`prefs.highlight_rules`).
    pub(crate) highlight_rule_form: crate::state::HighlightRuleForm,
    /// The pending "may this highlight rule run its snippet on this
    /// session" question (C6). A security prompt: the thing that asked
    /// for it is remote output.
    pub(crate) trigger_confirm: Option<crate::dispatch_terminal::TriggerConfirmCard>,

    // Cloud Accounts, CloudProfile rows + the wizard form. Wizard is
    // intentionally minimal in v0.6 PR 3: provider + AWS profile auth
    // only. Access key + SSO + the discover-and-pick step land in
    // follow-up PRs once the foundation is exercised.
    pub(crate) cloud_profiles: Vec<oryxis_core::models::cloud_profile::CloudProfile>,
    /// Transient state for the add/edit cloud-account wizard (covers all
    /// provider + auth combinations).
    pub(crate) cloud_form: crate::state::CloudForm,
    pub(crate) cloud_provider_registry: std::sync::Arc<oryxis_cloud::CloudProviderRegistry>,
    /// Concrete plugin providers kept here as well as inside the
    /// registry, so the install / update path can call
    /// `PluginProvider::rebind` after `cache::set_current` flips the
    /// active version. The registry only exposes the `CloudProvider`
    /// trait surface, which doesn't include rebind on purpose.
    pub(crate) plugin_providers:
        std::collections::HashMap<String, std::sync::Arc<crate::plugins::PluginProvider>>,

    // Plugins panel, one row per cloud-provider plugin. Cloud
    // providers run as downloaded subprocess plugins; this is where
    // the user installs, updates, pins, and rolls them back.
    pub(crate) plugins: Vec<crate::state::PluginUiEntry>,
    /// Global default for plugin auto-update. Per-plugin overrides
    /// live on each `PluginUiEntry`.
    pub(crate) plugins_auto_update_global: bool,
    /// When `Some(provider_id)`, the first-use install opt-in modal
    /// is shown for that provider.
    pub(crate) plugin_install_modal: Option<String>,
    /// Native combo_box state for the host editor's Parent Group field.
    /// Holds the (visible) group labels + the filtered subset and the
    /// live typed value. Rebuilt on editor-open via
    /// `rebuild_editor_combos`; the typed/selected value still
    /// flows through `editor_form.group_name` (the save path's single
    /// source of truth), so free-text "create on save" is unchanged.
    pub(crate) editor_parent_combo: iced::widget::combo_box::State<String>,
    /// Native combo_box state for the host editor's Initial Command /
    /// Snippet field. A forced-selection searchable combo: options are
    /// the None / Custom sentinels plus the snippet labels; the picked
    /// label commits through `EditorStartupChoiceChanged` (no free-text
    /// path). Rebuilt on editor-open via `rebuild_editor_combos`.
    pub(crate) editor_startup_combo: iced::widget::combo_box::State<String>,
    /// Login-automation picker: off / every saved script / new.
    pub(crate) editor_login_script_combo: iced::widget::combo_box::State<String>,
    /// Template picker inside the inline "new script" sub-form.
    pub(crate) editor_script_template_combo: iced::widget::combo_box::State<String>,
    /// Native combo_box state for the host editor's SSH Key field. Same
    /// forced-selection searchable pattern as the startup combo: options
    /// are the `(none)` sentinel plus the key labels; picking commits
    /// through `EditorKeyChanged`. Rebuilt on editor-open and cleared on
    /// focus (`EditorKeyComboOpened`) so search starts fresh.
    pub(crate) editor_key_combo: iced::widget::combo_box::State<String>,
    /// Shared search input for the group picker (used by both side
    /// panels' Parent Group fields). Reset on every open.
    pub(crate) group_picker_search: String,
    /// Host editor's startup-command source (None / a snippet / custom).
    pub(crate) editor_startup_choice: crate::state::StartupChoice,
    /// Bounds of the dynamic group editor's Parent Group combo row.
    pub(crate) dynamic_form_parent_combo_bounds: crate::widgets::BoundsCell,
    /// Bounds of the session-group editor's Folder combo row.
    pub(crate) session_group_folder_combo_bounds: crate::widgets::BoundsCell,
    /// Bounds of the manual group editor's Parent Group combo row.
    pub(crate) group_edit_parent_combo_bounds: crate::widgets::BoundsCell,
    /// Bounds of the `+` tab button, so the split hover popover anchors
    /// under it at a fixed position instead of following the cursor.
    pub(crate) plus_btn_bounds: crate::widgets::BoundsCell,
    /// Bounds of the Hosts-toolbar tag-filter button, so its dropdown
    /// anchors under the button (like the "+ Host" split menu) instead
    /// of at the cursor. Populated by a `bounds_reporter` wrapper.
    pub(crate) host_tag_filter_btn_bounds: crate::widgets::BoundsCell,
    /// Bounds of the Snippets-toolbar tag-filter button, same role as
    /// `host_tag_filter_btn_bounds` for the Snippets view.
    pub(crate) snippet_tag_filter_btn_bounds: crate::widgets::BoundsCell,
    /// Bounds of the History-toolbar tag-filter button, same role as
    /// `host_tag_filter_btn_bounds` for the History view.
    pub(crate) history_tag_filter_btn_bounds: crate::widgets::BoundsCell,
    /// Bounds of the active toolbar's "+ HOST ▾" / "+ ADD ▾" split
    /// group (only one renders at a time), so its dropdown anchors to
    /// the real button instead of a constant estimate that broke as
    /// soon as the layout (nav orientation, empty state, overflow)
    /// moved the button. Zeroed by `keynav_toolbar_reset` each build so
    /// a frame without the button falls back cleanly.
    pub(crate) toolbar_split_btn_bounds: crate::widgets::BoundsCell,
    /// Bounds of the active toolbar's sort button, same role.
    pub(crate) toolbar_sort_btn_bounds: crate::widgets::BoundsCell,
    /// Bounds of the active toolbar's `…` overflow button, same role.
    pub(crate) toolbar_overflow_btn_bounds: crate::widgets::BoundsCell,
    /// Modal that asks the user to pick the transport for the EC2
    /// hosts about to be imported. Only opened when there's at
    /// least one EC2 selected, pure-ECS imports skip straight to
    /// the import logic since dynamic groups always use ECS Exec.
    pub(crate) cloud_import_confirm_visible: bool,
    /// Per-dynamic-group resolve cache. Populated when the user opens
    /// the group (or hits Refresh inside it); reused on re-open until
    /// the user manually refreshes.
    pub(crate) cloud_dynamic_group_state:
        std::collections::HashMap<Uuid, crate::state::DynamicGroupState>,

    /// Edit-dynamic-group form. Opened from the ⋮ menu on a dynamic
    /// group card (root or nested). Edits the `cloud_query.template`
    /// fields: username, initial_command, transport, key, identity.
    pub(crate) cloud_dynamic_form: crate::state::CloudDynamicForm,



    // Snippets
    pub(crate) snippets: Vec<oryxis_core::models::snippet::Snippet>,
    /// Which install script (issue #147) already ran on which host,
    /// keyed (host id, snippet id) with the last run time. Mirror of
    /// the vault's `install_runs` table; drives the "installed here"
    /// hint on the snippet surfaces. Local bookkeeping, never synced.
    pub(crate) install_runs:
        std::collections::HashMap<(Uuid, Uuid), chrono::DateTime<chrono::Utc>>,
    /// User-defined terminal color schemes, shown in the theme pickers
    /// alongside the built-in presets and resolved by name.
    pub(crate) custom_terminal_themes:
        Vec<oryxis_core::models::custom_terminal_theme::CustomTerminalTheme>,
    /// User-defined chrome (UI) themes, shown in Interface alongside the
    /// built-in app themes and resolved by name.
    pub(crate) custom_ui_themes:
        Vec<oryxis_core::models::custom_ui_theme::CustomUiTheme>,
    /// Import-UI-theme modal (paste the Oryxis UI theme JSON), mirroring
    /// the terminal scheme import modal above.
    /// Single external editor used by the SFTP "Open with default text
    /// editor" action (issue #84). Empty = unset.
    /// Agentless host-monitor state (issue #83): per-host sample ring
    /// for the sidebar Monitor tab. RAM only, never persisted or synced.
    pub(crate) monitor: crate::monitor::MonitorState,
    /// tmux session manager state (issue #116): the last listing per
    /// PANE, keyed that way because the pane owns the SSH session the
    /// listing was read over. RAM only, never persisted or synced.
    pub(crate) tmux: crate::tmux::TmuxState,
    /// Live SSH connections a second tab can ride (F2). `Weak` only:
    /// the sessions own the transports, so the pool never keeps a
    /// connection alive and a dead entry is pruned on next lookup.
    pub(crate) ssh_transport_pool: crate::ssh_reuse::TransportPool,
    /// The reuse key each in-flight dial was keyed with, minted at DIAL
    /// time and consumed at `SshConnected`. Registration must not
    /// recompute the key from the live row: a host edited while its
    /// dial is in flight would register the old endpoint's transport
    /// under the new row's key, and the next open of the edited host
    /// would ride a connection to the old machine.
    pub(crate) pending_reuse_keys:
        std::collections::HashMap<Uuid, crate::ssh_reuse::ReuseKey>,
    /// Multi-host dashboard links (issue #95). Samples go into
    /// `monitor.series` like the sidebar's, so the two surfaces can
    /// never disagree about a host.
    pub(crate) monitor_dash: crate::state::MonitorDash,
    /// Bumped whenever a monitored host's series is invalidated
    /// (disconnect, opt-out, lock). Probes carry the stamp they were
    /// dispatched with, so a result from before the reset is dropped.
    pub(crate) monitor_stamp: u64,
    /// Last probe failure, shown in the Monitor tab. Cleared by the next
    /// successful sample.
    pub(crate) monitor_error: Option<String>,
    /// Whether the Monitor tab's listening-ports list is expanded. A
    /// busy host can listen on dozens of ports, so it starts collapsed
    /// behind a count.
    pub(crate) monitor_ports_open: bool,
    /// Whether the Monitor tab's disk list is expanded (issue #83
    /// follow-up). Starts open so the common one-or-two-mount host shows
    /// its disks at a glance, but a host with many mounts can collapse
    /// them behind the count, mirroring the ports disclosure.
    pub(crate) monitor_disks_open: bool,
    /// Session-only "Yes to all" grant from the same dialog (never
    /// persisted; dies with the app run).
    pub(crate) sftp_edit_upload_all: bool,
    pub(crate) ui_theme_import_content: iced::widget::text_editor::Content,
    pub(crate) ui_theme_import_name: String,
    pub(crate) ui_theme_import_error: Option<String>,
    /// Custom UI (chrome) theme editor modal + its color-picker popover and
    /// the hovered card (mirrors the terminal-theme editor).
    pub(crate) ui_theme_editor: Option<crate::state::UiThemeEditorForm>,
    pub(crate) ui_color_popover: Option<(usize, iced::Point)>,
    /// Name of the active app theme (built-in or custom UI theme). The
    /// `AppTheme` enum can't name a custom theme, so this tracks the
    /// selection for highlighting + delete/rename bookkeeping.
    pub(crate) active_app_theme_name: String,
    /// Snippet run/paste waiting on its `{placeholders}` (modal open
    /// while `Some`).
    pub(crate) pending_snippet_vars: Option<crate::state::PendingSnippetVars>,

    // Port forwards (standalone entity, independent of any terminal)
    pub(crate) port_forward_rules:
        Vec<oryxis_core::models::port_forward_rule::PortForwardRule>,
    /// Runtime-only registry of live forwards, keyed by rule id. Not
    /// persisted, the on/off state lives only here. Dropping the
    /// `ForwardSession` cancels its tasks.
    pub(crate) active_forwards:
        std::collections::HashMap<Uuid, std::sync::Arc<oryxis_ssh::ForwardSession>>,
    /// One shared, PTY-less SSH connection per HOST carrying every live
    /// forward to it (issue #126): rules attach as channels instead of
    /// each dialing its own connection. Keyed by the host's connection
    /// id. `Connecting` queues rules toggled on while the dial is in
    /// flight; the entry is dropped when the last forward of that host
    /// stops (which closes the SSH connection). See
    /// `dispatch_port_forwards::PfHostConn`.
    pub(crate) forward_conns:
        std::collections::HashMap<Uuid, crate::dispatch_port_forwards::PfHostConn>,
    /// Rules that were queued behind a shared dial the legacy-algorithm
    /// dialog aborted, keyed by host id. Consumed by the dialog's retry
    /// (`PortForwardHostRetry`) so the whole queue restarts together;
    /// discarded when a fresh dial for the host starts any other way.
    pub(crate) pf_aborted_pending: std::collections::HashMap<Uuid, Vec<Uuid>>,
    /// Live RDP/VNC-over-SSH tunnels, keyed by the host's connection id.
    /// A managed `-L` forward paired with its launch generation. The tunnel
    /// self-closes once the desktop client disconnects and it goes idle (see
    /// `spawn_autoclose_local_forward_task`); relaunching a host replaces its
    /// entry (dropping the old `ForwardSession` cancels it), and vault
    /// lock / app close clears the map. The generation lets a stale
    /// self-close from a superseded tunnel skip the current entry.
    pub(crate) remote_desktop_forwards: std::collections::HashMap<
        Uuid,
        (u64, std::sync::Arc<oryxis_ssh::ForwardSession>),
    >,
    /// Callback tunnels opened by Ctrl+clicking a link in a terminal
    /// pane, keyed by `(pane_id, port)`. Each is a `-L` forward riding
    /// that pane's own SSH connection so a CLI login's
    /// `http://127.0.0.1:<port>` redirect reaches the listener on the
    /// REMOTE machine. They close themselves once the redirect has been
    /// served (or once they have waited long enough for one), and the
    /// pane / tab close paths prune what is left; the app holds the only
    /// strong `Arc`, so removing an entry cancels its tunnel.
    pub(crate) link_forwards: std::collections::HashMap<
        (Uuid, u16),
        std::sync::Arc<oryxis_ssh::ForwardSession>,
    >,
    /// The pending "open this link?" question raised by a Ctrl+click in
    /// a REMOTE pane (`Modal::TerminalLinkConfirm` while `Some`).
    pub(crate) link_confirm: Option<crate::dispatch_terminal::LinkConfirmCard>,
    /// Monotonic launch counter feeding the generation in
    /// `remote_desktop_forwards`; bumped once per Open.
    pub(crate) remote_desktop_seq: u64,
    /// Opt-in toggle (`remote_desktop_enabled` setting, off by default):
    /// when off, all remote-desktop UI (the "Add remote desktop" entry,
    /// the settings row) is hidden so it doesn't clutter the light-user
    /// interface. Existing RemoteDesktop hosts stay visible and usable.
    pub(crate) remote_desktop_enabled: bool,
    /// Rules whose connect is in flight (drives the per-row spinner and
    /// prevents a double-start).
    pub(crate) port_forward_starting: std::collections::HashSet<Uuid>,
    /// Auto-start rules that failed to come up (or dropped after being up)
    /// and are scheduled to be re-attempted. Only ever holds `auto_start`
    /// rules; a manual Stop or delete removes the entry so a rule the user
    /// turned off never resurrects. Keyed by rule id. See
    /// `dispatch_port_forwards::PfRetry`.
    pub(crate) port_forward_retry:
        std::collections::HashMap<Uuid, crate::dispatch_port_forwards::PfRetry>,
    /// Last reading of the ssh-agent conditions, taken on each retry tick
    /// and compared against the next one: an agent appearing (or a key
    /// pushed into our own agent server) makes every pending rule due
    /// immediately instead of waiting out its backoff. `None` while
    /// nothing is pending. See `dispatch_port_forwards::PfAgentWatch`.
    pub(crate) port_forward_agent_watch:
        Option<crate::dispatch_port_forwards::PfAgentWatch>,
    pub(crate) port_forward_form: crate::state::PortForwardRuleForm,
    /// Index of the port-forward card whose kebab menu is open. Keeps the
    /// kebab mounted while the pointer travels to the menu.
    pub(crate) port_forward_context_menu: Option<usize>,
    pub(crate) port_forward_search: String,
    /// Toolbar search needles for the Cloud Accounts and Proxies views.
    pub(crate) cloud_search: String,
    pub(crate) proxy_search: String,

    // Known hosts & logs
    pub(crate) known_hosts: Vec<oryxis_core::models::known_host::KnownHost>,
    pub(crate) logs: Vec<oryxis_core::models::log_entry::LogEntry>,
    pub(crate) logs_page: usize,
    pub(crate) logs_total: usize,
    /// "Clear all" confirmation modal for the Logs view.
    pub(crate) clear_history_confirm: bool,

    // Session logs (terminal recording)
    pub(crate) session_logs: Vec<oryxis_vault::SessionLogEntry>,
    pub(crate) session_logs_page: usize,
    pub(crate) session_logs_total: usize,
    pub(crate) viewing_session_log: Option<crate::state::SessionLogViewer>,
    /// The in-app session player (issue #71), rendered as a full
    /// surface on the History view while `Some`. Mutually exclusive
    /// with `viewing_session_log` (opening either closes the other).
    pub(crate) session_player: Option<crate::state::SessionPlayer>,
    /// GIF export of a recording (issue #71): pending-install handoff +
    /// the one-render-at-a-time flag. Sibling of `session_player` (an
    /// export can run with the player closed), see
    /// [`crate::state::GifExportState`].
    pub(crate) gif_export: crate::state::GifExportState,

    // Terminal theme
    /// Theme derived from the active app theme, used as the global
    /// fallback when neither `terminal_theme_override` nor a per-host
    /// override is set.
    /// Cached resolved global terminal palette (built-in or custom).
    /// Applied to new tabs / local shells / cloud sessions; recomputed when
    /// the global theme or a custom theme changes.
    pub(crate) terminal_palette: oryxis_terminal::TerminalPalette,
    /// User pick that overrides the app-theme-derived terminal palette.
    /// `None` means "follow the app theme" (default). Stored as the
    /// theme's display name (e.g. "Dracula") so the value survives new
    /// theme additions without a migration.
    pub(crate) terminal_theme_override: Option<String>,
    /// Session-only theme override applied to local/ephemeral terminal
    /// panes (which have no saved Connection). `None` follows the global
    /// terminal theme. Set from the Host config sidebar tab when the
    /// focused pane is a local shell; not persisted unless the user saves
    /// it as the global default.
    pub(crate) local_terminal_theme: Option<String>,
    pub(crate) terminal_font_size: f32,
    pub(crate) terminal_font_name: String,
    /// Weight every terminal cell is drawn at (issue #155). Global,
    /// like the family and the size next to it: a per-host weight
    /// over a global family would let a host ask for a weight the
    /// family it can't choose has no face for.
    pub(crate) terminal_font_weight: crate::fonts::TerminalFontWeight,
    /// Stroke widening applied to every terminal glyph. Global for the
    /// same reason the weight is: it compensates for the rasterizer,
    /// not for a host.
    pub(crate) terminal_text_thickness: crate::fonts::TextThickness,

    // Settings
    pub(crate) settings_section: SettingsSection,
    /// Which panel tabs are in the strip (Settings since issue #120,
    /// network tools since the panel landed). Materialized on the first
    /// visit to the panel's view, exactly like `ensure_sftp_tab` does
    /// for the SFTP surface, so leaving it and coming back is one click
    /// instead of a hunt through the menus. Transient by design: never
    /// persisted, so a restart opens on real work.
    ///
    /// This is the existence test the strip and `reconcile_tab_order`
    /// read; `tab_order` holds the position and is kept in step with it.
    pub(crate) open_panel_tabs: std::collections::BTreeSet<crate::state::PanelKind>,
    /// The network tools panel's own state (target, tool, results).
    pub(crate) net_tools: crate::state::NetToolsState,
    /// Last scroll offset of each section, so returning to Settings lands
    /// where you left instead of at the top. Keyed by section because the
    /// sections are separate scrollables; the value is a relative offset
    /// (0.0..=1.0) fed straight back to `scroll_to`.
    pub(crate) settings_scroll: std::collections::HashMap<SettingsSection, f32>,
    /// Live query of the Settings sidebar search. Non-empty highlights
    /// every matching row in the open section (JetBrains style), tags
    /// the sections that contain matches, and auto-opens the best one;
    /// cleared when the Settings view is left.
    pub(crate) settings_search: String,
    /// Find-next cursor: index into `settings_ordered_matches` of the
    /// ACTIVE match (the one Enter / Shift+Enter last landed on, ringed
    /// accent and scrolled into view). Reset when the query changes.
    pub(crate) settings_active_match: usize,
    /// The graphics backend + adapter actually selected by the
    /// compositor, queried from iced once the Interface settings
    /// section is opened (the compositor exists by then). `(backend,
    /// adapter)`, e.g. `("Vulkan", "NVIDIA GeForce RTX 3080")`. Shows
    /// what "Automatic" resolved to so a backend fallback is diagnosable.
    pub(crate) renderer_active: Option<(String, String)>,
    /// The shell-integration key this vault mints once and every pane then
    /// demands from `OSC 633 ; E` (see `shell_integration.rs`). Held in
    /// state only so Settings can show and copy the snippet; the value the
    /// capture actually compares against lives in the terminal crate,
    /// installed at boot.
    pub(crate) shell_integration_nonce: String,
    /// Vault Snippets view: multi-select tag filter (in-memory, like
    /// the dashboard's `host_filter_tags`); empty = off.
    pub(crate) snippet_filter_tags: Vec<String>,
    /// Vault Snippets view: the snippet group currently opened as a
    /// folder (dashboard-style drill-in). `None` = root (group cards +
    /// ungrouped snippets).
    pub(crate) active_snippet_group: Option<String>,
    /// Terminal-sidebar Snippets tab: its own drill-in group (kept
    /// separate from the vault view's so the two surfaces navigate
    /// independently).
    pub(crate) sidebar_snippet_group: Option<String>,
    /// Recently visited Files-sidebar folders, keyed by saved-host id.
    ///
    /// The per-pane list is deliberately wiped on disconnect, because a
    /// reconnect can land on a different tree; keeping the history HERE,
    /// scoped to the host it belongs to, is what makes that wipe harmless
    /// and lets the list survive closing the tab (issue #114 / #85).
    /// Only `PaneOrigin::Host` panes qualify: a quick-connect id is
    /// in-memory and a local shell has no host to key on.
    pub(crate) files_recent_folders: std::collections::HashMap<uuid::Uuid, Vec<String>>,
    /// Set once at boot when [`setting_performance_mode`] was auto-enabled
    /// by the render probe, so the unlock path can raise a one-time toast
    /// explaining why. Cleared when the toast is emitted.
    pub(crate) pending_perf_mode_toast: bool,
    /// Privacy Mode (issue #78): global toggle, session override, hint
    /// flag, always/never mask lists, per-class gates and the Logs
    /// reveal toggle. See [`crate::state::PrivacyState`].
    pub(crate) privacy: crate::state::PrivacyState,
    /// Download-mirror block state (Settings > Advanced): persisted
    /// choice + custom-URL editing + probe outcome. The effective
    /// choice also lives in `net_mirror`'s process-wide slot so the
    /// download tasks can read it without `&Oryxis`.
    pub(crate) download_mirror: crate::net_mirror::MirrorUi,
    /// Signature of (tabs len, last tab uuid, connections len, max
    /// last_used timestamp) computed during the last tray menu
    /// rebuild. The TrayPoll handler recomputes the signature each
    /// tick and only rebuilds the menu when it differs. Avoids
    /// burning cycles rebuilding the dynamic submenus 10 times a
    /// second when nothing has changed.
    pub(crate) tray_menu_signature: u64,
    /// Signature of the recent-hosts set last pushed to the Windows
    /// taskbar JumpList (label + id per entry). Recomputed on the same
    /// unconditional-Windows TrayPoll tick; the JumpList only rebuilds
    /// when it changes. Independent of `tray_menu_signature` so the
    /// JumpList works even with the tray icon off.
    pub(crate) jumplist_signature: u64,
    /// True once the main window has been tagged with the JumpList AUMID
    /// (a one-shot done on the first TrayPoll after the window exists).
    pub(crate) jumplist_window_tagged: bool,
    /// True when the main window is currently hidden to the tray
    /// (Win32 ShowWindow(SW_HIDE), via TrayHide / close-to-tray /
    /// minimize-to-tray). Drives the primary's tray menu visibility
    /// rule (only show the icon when at least one window is hidden)
    /// and feeds the child-side tray_ipc state row so the primary
    /// knows which children to surface in the "Hidden windows"
    /// submenu. Defaults to false; flipped by TrayShow / TrayHide
    /// handlers.
    pub(crate) is_window_hidden: bool,
    /// Cached signature of (is_window_hidden, tab labels) the child
    /// last wrote to the tray_ipc registry. TrayPoll recomputes
    /// each tick and only re-writes when it differs so we don't
    /// churn the filesystem ten times a second.
    pub(crate) ipc_state_signature: u64,
    /// One-shot: set when reopening a *pinned cloud* dormant tab. Because the
    /// cloud spawn is async (the tab is born later, in `spawn_plugin_tab`),
    /// the pin intent can't ride the synchronous len-check the host / local
    /// paths use; this carries it instead and is consumed on the next
    /// plugin-tab spawn. `Some(dormant_id)` = replace the dormant placeholder
    /// (found by this id) in place, so its strip chip doesn't blink out during
    /// the async connect, and inherit its slot + pin.
    pub(crate) pin_next_plugin_tab: Option<uuid::Uuid>,
    /// See `state::PendingEcsAutoConnect`: deferred connect-to-current
    /// ECS task while the dynamic group re-resolves.
    pub(crate) pending_ecs_autoconnect: Option<crate::state::PendingEcsAutoConnect>,
    /// In-progress tab reorder drag (see `TabDrag`). `None` when not dragging.
    pub(crate) tab_drag: Option<crate::state::TabDrag>,
    /// Live width of the right-side editor drawer (host / key / identity /
    /// snippet / port-forward / cloud forms, all of them: they share one
    /// width, like they shared one constant). Defaults to `PANEL_WIDTH`,
    /// dragged via the drawer's edge handle, persisted as the
    /// `side_panel_width` setting on release.
    pub(crate) panel_width: f32,
    /// In-progress drawer resize drag: (cursor x at press, width at
    /// press). `None` when not dragging.
    pub(crate) panel_resize_drag: Option<(f32, f32)>,
    /// Signature of the editor form as of the last persist (or the
    /// baseline recorded when the editor opened): what
    /// `editor_autosave_dirty` compares against. `None` = no baseline
    /// yet; the first post-open editor message records it.
    pub(crate) editor_saved_snapshot: Option<String>,
    /// Toggles the SFTP feature entirely. Off hides the SFTP sidebar
    /// entry (both expanded and collapsed) so users who never transfer
    /// files don't have it taking up nav space. The SFTP settings panel
    /// still renders so the user can re-enable + tweak in one place,
    /// mirroring how `ai_enabled` works.
    pub(crate) sftp_enabled: bool,
    /// Secret fields currently revealed via their eye toggle. Render
    /// state only, never persisted; cleared per-field on toggle.
    pub(crate) revealed_secrets: std::collections::HashSet<crate::state::SecretField>,
    /// Instant of the last user input event (keyboard / mouse / IME),
    /// the idle anchor for `setting_auto_lock_minutes`. Not persisted.
    pub(crate) last_user_activity: std::time::Instant,
    /// Instant of the last free-space / size-cap check on the session
    /// recordings. The flush runs every 2 s (and on a 64 KiB burst),
    /// while a `statvfs` plus a `SUM(LENGTH(data))` are far too costly
    /// at that cadence, so the check is throttled to
    /// `SESSION_LOG_CAPACITY_INTERVAL`. Not persisted.
    pub(crate) last_session_log_capacity_check: std::time::Instant,
    /// Instant of the last successful password unlock. The Enter that
    /// submits the unlock password reaches the global key subscription
    /// one message AFTER the widget's on_submit unlocked the vault, so
    /// every consumer below the lock-screen gate (including PTY
    /// routing) would see it as an unlocked-app keystroke: with a
    /// terminal tab restored by the soft lock, that newline lands on
    /// the shell prompt and would RUN whatever was left typed there.
    /// The key router swallows key events for a breath after this
    /// stamp. `None` until the first unlock.
    pub(crate) last_unlock: Option<std::time::Instant>,
    /// Whether this platform / session can service biometric unlock at
    /// all (probed once at boot via the provider). The whole affordance
    /// (setting row + lock-screen button) hides when false. Not persisted.
    pub(crate) biometric_available: bool,

    // Update state (set by the async GitHub check on boot)
    pub(crate) pending_update: Option<crate::update::UpdateInfo>,
    pub(crate) update_downloading: bool,
    pub(crate) update_progress: f32,
    pub(crate) update_error: Option<String>,
    /// Last manual-check outcome shown near the "Check now" button in
    /// settings. `None` hides the line; the enum picks i18n + color at
    /// render time (Checking / UpToDate / Failed(cause)).
    pub(crate) update_check_status: Option<crate::update::UpdateStatus>,
    /// Attempt counters keyed by connection UUID, persists across tab recreations.
    pub(crate) reconnect_counters: std::collections::HashMap<Uuid, u32>,

    // AI Chat settings
    pub(crate) ai: crate::state::AiState,


    /// Transient bottom-of-chat status chip, e.g. the "Copied to
    /// clipboard" feedback after a Copy button click. `Some(text)` →
    /// render the chip. Auto-dismissal is deadline-driven: every setter
    /// (`set_toast` / `show_toast`) stamps `toast_deadline`, and the
    /// `ToastTick` subscription clears both once it passes. A single
    /// deadline (not per-call sleep timers) means a newer toast always
    /// wins and no toast is ever left stranded on screen.
    pub(crate) toast: Option<String>,
    /// When the current `toast` should auto-dismiss. Reset on every new
    /// toast so the latest one always gets its full dwell.
    pub(crate) toast_deadline: Option<std::time::Instant>,

    /// CJK language codes (`"ko"`/`"zh"`/`"ja"`) whose font has already
    /// been requested this session, so switching language back and forth
    /// doesn't re-download or re-load. A code is removed on a failed
    /// download so a later retry can happen. See `crate::fonts`.
    pub(crate) loaded_cjk_fonts: std::collections::HashSet<String>,

    /// Terminal-pack font families whose bytes have already been
    /// requested this session (same guard contract as
    /// `loaded_cjk_fonts`: inserted when the ensure task is spawned,
    /// removed on a failed download so a re-pick retries). Also drives
    /// the picker's "available to download" hint, which lists only the
    /// pack fonts not yet requested.
    pub(crate) loaded_pack_fonts: std::collections::HashSet<String>,

    /// Generic blocking error dialog. Use for cases the user must read
    /// (install instructions, fatal config errors) where a 1.8 s toast
    /// would vanish before they can act on it. `None` = no dialog.
    pub(crate) error_dialog: Option<crate::state::ErrorDialog>,

    /// Curated list of local terminals (PowerShell, cmd, WSL distros,
    /// manual entries, ...). The auto-scan runs once on first open and
    /// persists into the `local_terminals` setting; this caches that
    /// list. `None` means never scanned (the next open triggers the
    /// one-time scan). Machine-local: never synced or exported.
    pub(crate) local_terminals: Option<Vec<crate::state::LocalTerminalEntry>>,
    /// "Always open X" preference: the id of the terminal to open without
    /// a picker, or `None` for "always ask". Backed by the
    /// `local_terminal_default` setting.
    pub(crate) local_terminal_default: Option<uuid::Uuid>,
    /// "Add terminal manually" form, shown in a modal opened from the
    /// Settings → Terminal card.
    pub(crate) local_terminal_form: crate::state::LocalTerminalForm,
    /// True while the "add local terminal" modal is open.
    pub(crate) local_terminal_add_open: bool,
    /// True while the Local Shell picker overlay is showing. Only
    /// surfaces on Windows where there's a real choice between cmd /
    /// PowerShell / WSL distros, non-Windows just spawns the
    /// default shell directly.
    pub(crate) local_shell_picker_open: bool,

    /// Remembered active tab of each terminal-sidebar region, indexed
    /// by `SidebarSide::idx()` (issue #102). Read through
    /// `sidebar_region_tab()`, which re-resolves against the region's
    /// available tabs (a remembered tab may have moved sides or lost
    /// its gate since).
    pub(crate) terminal_sidebar_tab: [crate::state::TerminalSidebarTab; 2],
    /// Search needle for the Snippets tab of the terminal sidebar. Kept
    /// separate from `snippet_search` (the workspace view) so filtering
    /// one doesn't disturb the other.
    pub(crate) sidebar_snippet_search: String,
    /// Sort dropdown open in the Snippets tab (a sidebar-local popover, not
    /// the workspace's window-anchored overlay).
    pub(crate) sidebar_sort_open: bool,
    /// Search field expanded in the Snippets tab. Collapsed = a search
    /// icon; expanded = a focused input that replaces the New / sort row.
    pub(crate) sidebar_search_open: bool,
    /// Expanded group ids in the sidebar Hosts tree (issue #102).
    /// In-memory only: a fresh session starts collapsed, like the
    /// dashboard starts at root.
    pub(crate) hosts_tree_expanded: std::collections::HashSet<uuid::Uuid>,
    /// Search needle of the sidebar Hosts tree. While non-empty the
    /// tree shows every match with its ancestor chain force-expanded.
    pub(crate) hosts_tree_search: String,

    // MCP Server
    pub(crate) mcp: crate::state::McpState,

    // Sync (settings + runtime engine handles + pairing/SFTP forms)
    pub(crate) sync: crate::state::SyncState,

    // ── SSH-agent server (B1): expose vault keys over the standard
    // ssh-agent socket. Feature-gated (off by default), in-core (the
    // signing oracle needs the unlocked vault in-process).
    pub(crate) agent: crate::state::AgentState,
    /// When on, the dashboard root shows two sections, Groups (manual
    /// folder cards) and Hosts (a flat list of every connection,
    /// including those that live inside a group). When off, root
    /// matches the legacy behaviour: groups at top, only ungrouped
    /// hosts beneath. Default: on. (Dashboard layout, not a sync field.)
    pub(crate) flatten_hosts: bool,

    pub(crate) export_password: String,
    pub(crate) export_include_keys: bool,
    /// Which entity families to write into the export, one checkbox per
    /// category in the dialog. Reset to `all()` each time the dialog opens.
    pub(crate) export_selection: oryxis_vault::ExportSelection,
    pub(crate) export_status: Option<Result<String, String>>,
    /// SFTP backup target picker. Shown when the user routes an
    /// export/import through a remote host instead of a local file.
    /// `is_import` flips the same picker between writing the encrypted
    /// blob (export) and reading it back (import); the export/import
    /// password + selection state above is reused as-is.
    pub(crate) sftp_backup: crate::state::SftpBackupForm,
    /// Latest result of an `~/.ssh/config` import, `Ok(message)` is
    /// rendered as a green banner, `Err` as red, in the Security
    /// section's import card.
    pub(crate) ssh_config_import_status: Option<Result<String, String>>,

    pub(crate) share: crate::state::ShareForm,

    // SSH config import preview
    /// Hosts parsed from a picked `~/.ssh/config`, awaiting the user's
    /// pick of which to import. Non-empty drives the preview modal.
    pub(crate) ssh_import_hosts: Vec<crate::ssh_config::SshConfigHost>,
    /// A third-party import (PuTTY, ...) waiting in the same preview
    /// dialog. Mutually exclusive with `ssh_import_hosts`; the shared
    /// `ssh_import_selected` / `ssh_import_existing` vecs serve both.
    pub(crate) ssh_import_direct: Option<crate::importers::DirectImport>,
    /// Inline error of the Import hub modal ("couldn't recognize this
    /// file"); cleared on open and on a successful detection.
    pub(crate) import_hub_error: Option<String>,
    /// A protected mRemoteNG file held while the hub asks for its
    /// password. Swept with the hub (dismiss / open / success).
    pub(crate) import_hub_pending: Option<Vec<u8>>,
    /// The file password typed in the hub for `import_hub_pending`.
    pub(crate) import_hub_password: String,
    /// Per-host tick state, parallel to `ssh_import_hosts`.
    pub(crate) ssh_import_selected: Vec<bool>,
    /// Per-host "label already exists" flag, parallel to
    /// `ssh_import_hosts`; these are surfaced and default to unticked.
    pub(crate) ssh_import_existing: Vec<bool>,
}


// ---------------------------------------------------------------------------
// Application
// ---------------------------------------------------------------------------

// `boot`, `load_data_from_vault`, `persist_setting` live in `crate::boot`.

impl Oryxis {
    /// Days for a retention code; `None` = retention off.
    pub(crate) fn retention_days(code: &str) -> Option<i64> {
        match code {
            "1d" => Some(1),
            "3d" => Some(3),
            "7d" => Some(7),
            "14d" => Some(14),
            "30d" => Some(30),
            "90d" => Some(90),
            _ => None,
        }
    }

    /// Whether the Logs surface (sub-nav pill, sidebar entry, burger
    /// menu item) should render at all. Auto-hidden until the feature
    /// is real for this user: a recording toggle is on, or the vault
    /// already holds recorded data (issue #38, zero-config visibility).
    pub(crate) fn logs_surface_visible(&self) -> bool {
        self.prefs.session_logging
            || self.prefs.connection_history
            || self.logs_total > 0
            || self.session_logs_total > 0
    }

    /// Vertical offset (px) that toolbar dropdown anchors should use
    /// to land below the toolbar buttons on the dashboard, regardless
    /// of layout mode. Stack of contributions, top to bottom:
    /// tab bar (40) + hairline (2) + sub-nav (~36, Workspace vault
    /// only) + toolbar top (20) + button + gap (32) = ~94 (Classic)
    /// or ~130 (Workspace vault).
    ///
    /// The previous hardcoded 56 lined up against an older toolbar
    /// geometry; with the v0.7 sub-nav the menus were dropping over
    /// the trigger button. New values measured against the current
    /// toolbar and verified by user feedback.
    pub(crate) fn dashboard_dropdown_anchor_y(&self) -> f32 {
        use crate::state::View;
        // Toolbar geometry (top to bottom):
        //   tab_bar(40) + hairline(2) + toolbar_top_pad(20)
        //   + button(24 content + 10 default iced button padding = 34)
        //   + gap(8) = 104
        // The earlier estimate counted the button as 24 and skipped its
        // default vertical padding, which dropped the menu ~10 px too
        // high so it overlapped the trigger's bottom edge. This anchor is
        // shared by every toolbar split-menu (+ Host, keychain + Add, the
        // sort menu), so they all clear the button consistently.
        // Add the horizontal sub-nav (~50) on top only when it actually
        // renders (horizontal orientation + a vault view). The vertical
        // rail sits to the LEFT, not above, so it adds no vertical offset.
        // Measured ~20 px too low in practice (the split menus opened with
        // a visible gap below their trigger button), so the earlier 104
        // over-corrected. 84 seats the menu right under the button.
        const BASE_Y: f32 = 84.0;
        const SUBNAV_HEIGHT: f32 = 50.0;
        let horizontal_subnav = self.prefs.nav_orientation != "vertical"
            && self.active_tab.is_none()
            && matches!(
                self.active_view,
                View::Dashboard
                    | View::Keys
                    | View::Snippets
                    | View::PortForwarding
                    | View::History
            );
        if horizontal_subnav { BASE_Y + SUBNAV_HEIGHT } else { BASE_Y }
    }

    /// Anchor a toolbar dropdown to its trigger button's last-drawn
    /// bounds: 2 px below the button, trailing edges aligned (the menu's
    /// right edge on the button's right edge under LTR; left on left
    /// under RTL, pre-compensating the render path, which subtracts the
    /// menu width from `x` there). Anchoring to real bounds makes the
    /// menu follow the button through every layout the constant estimate
    /// broke in: vertical nav rail, empty-state toolbars, open side
    /// panels. Falls back to the legacy trailing-edge estimate when the
    /// cell is empty (before the first draw, or when the trigger moved
    /// into the `…` overflow, whose build zeroes the cells via
    /// `keynav_toolbar_reset`).
    pub(crate) fn toolbar_menu_anchor(
        &self,
        bounds: &crate::widgets::BoundsCell,
        menu_width: f32,
        panel_width: f32,
    ) -> (f32, f32) {
        let b = bounds.get();
        if b.width > 0.0 {
            let x = if crate::i18n::is_rtl_layout() {
                b.x + menu_width
            } else {
                b.x + b.width - menu_width
            };
            (x.max(0.0), b.y + b.height + 2.0)
        } else {
            let pad = 24.0;
            // A side-docked tab strip (issue #87) narrows the content
            // band; without the offsets the fallback anchor lands under
            // the strip on a right dock (or hugs the wrong edge on a
            // left dock + RTL).
            let strip_left = self.side_strip_left_offset();
            let strip_right = self.side_strip_reserve() - strip_left;
            let x = if crate::i18n::is_rtl_layout() {
                strip_left + panel_width + pad + menu_width
            } else {
                self.window_size.width - strip_right - panel_width - pad - menu_width
            };
            (x.max(0.0), self.dashboard_dropdown_anchor_y())
        }
    }

    pub(crate) fn snippet_injection_tab(&self) -> Option<usize> {
        let idx = self.active_tab.or(self.last_terminal_tab)?;
        (idx < self.tabs.len()).then_some(idx)
    }

    pub(crate) fn remember_terminal_tab_focus(&mut self, idx: usize) {
        if idx < self.tabs.len() {
            self.last_terminal_tab = Some(idx);
        }
        // Every terminal-tab activation funnels through here (tab strip
        // click, Ctrl+Tab MRU, new tabs, session groups), and a tab switch
        // is a context switch: a sidebar keynav ring engaged on the old
        // tab must not survive to silently consume Enter on the new one
        // (the sibling rule to the PTY-write disengage; both halves of
        // the same live-QA bug).
        self.keynav.sidebar_selected = None;
        // Same lifecycle rule for the pick_list dropdown gate: any
        // dropdown that was open belonged to the surface we just left
        // (Settings, a panel, the old tab's HostConfig) and unmounted
        // without firing on_close; a stuck flag swallows
        // Enter/Space/Esc/arrows process-wide.
        self.keynav.pick_open = false;
        // And for floating overlay menus (kebabs, the stay-open tag
        // filters): a stale `overlay` keeps the modal keyboard router
        // alive on the new tab, eating Enter/arrows invisibly (live
        // QA: 'the terminal stopped accepting commands', recovered
        // only by an explicit Esc elsewhere).
        self.overlay = None;
    }

    pub(crate) fn adjust_last_terminal_tab_after_remove(&mut self, removed_idx: usize) {
        if self.tabs.is_empty() {
            self.last_terminal_tab = None;
            return;
        }
        match self.last_terminal_tab {
            Some(l) if l == removed_idx => {
                self.last_terminal_tab = Some(removed_idx.min(self.tabs.len() - 1));
            }
            Some(l) if l > removed_idx => {
                self.last_terminal_tab = Some(l - 1);
            }
            _ => {}
        }
    }

    pub(crate) fn clear_terminal_tab_memory(&mut self) {
        self.last_terminal_tab = None;
    }

    pub fn title(&self) -> String {
        "Oryxis".into()
    }

    pub fn theme(&self) -> Theme {
        Theme::custom(
            String::from("Oryxis Dark"),
            iced::theme::palette::Seed {
                background: OryxisColors::t().bg_primary,
                text: OryxisColors::t().text_primary,
                primary: OryxisColors::t().accent,
                success: OryxisColors::t().success,
                warning: OryxisColors::t().warning,
                danger: OryxisColors::t().error,
            },
        )
    }


}

// `update`, `boot`, `subscription`, `view`, and the connect / SFTP
// helpers each live in their own sibling module. This file now only
// holds the struct definition, the `Message` re-export, layout
// constants, and the trivial `title` / `theme` accessors.

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
