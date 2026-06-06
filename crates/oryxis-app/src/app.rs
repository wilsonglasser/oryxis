use iced::keyboard;
use iced::widget::{svg, text_editor};
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
    ConnectionForm, ConnectionProgress, OverlayState, SettingsSection, TerminalTab, VaultState,
    View,
};
use crate::theme::OryxisColors;

// `Message` lives in its own module; re-export so call sites that
// import `crate::app::Message` keep working.
pub use crate::messages::Message;

// Layout constants
pub(crate) const DEFAULT_TERM_COLS: u32 = 120;
pub(crate) const DEFAULT_TERM_ROWS: u32 = 40;
pub(crate) const PANEL_WIDTH: f32 = 420.0;
pub(crate) const SIDEBAR_WIDTH: f32 = 180.0;
pub(crate) const SIDEBAR_WIDTH_COLLAPSED: f32 = 56.0;
pub(crate) const CARD_WIDTH: f32 = 280.0;

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
pub(crate) fn enumerate_terminal_fonts() -> Vec<String> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();

    let mut names: std::collections::BTreeSet<String> =
        std::collections::BTreeSet::new();
    for face in db.faces() {
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

    if names.is_empty() {
        return TERMINAL_FONT_FALLBACK.iter().map(|s| s.to_string()).collect();
    }

    // Prepend the bundled family so it's always picker entry #1
    // regardless of what the system scan returned. cosmic-text
    // resolves it by family name, fontdb has it registered via
    // `application.font(include_bytes!(...))` in main.rs.
    let bundled: &[&str] = &["SauceCodePro Nerd Font"];
    let mut out: Vec<String> = Vec::with_capacity(names.len() + bundled.len());
    for b in bundled {
        out.push((*b).to_string());
    }
    for n in names {
        if !bundled.contains(&n.as_str()) {
            out.push(n);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

pub struct Oryxis {
    // Vault
    pub(crate) vault: Option<VaultStore>,
    pub(crate) vault_state: VaultState,
    pub(crate) vault_password_input: String,
    pub(crate) vault_password_visible: bool,
    pub(crate) vault_error: Option<String>,
    // Vector logo handles (see boot.rs). SVG goes through iced's
    // resvg/tiny-skia path instead of the wgpu image atlas, which on
    // GNOME Wayland fractional scaling corrupted the raster PNG into
    // garbage once the window got a real app_id and was composited at a
    // non-integer scale. The small/large split is kept for call-site
    // clarity even though both now point at the same asset.
    pub(crate) logo_handle: svg::Handle,
    pub(crate) logo_small_handle: svg::Handle,

    // Data
    pub(crate) connections: Vec<Connection>,
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
    pub(crate) quick_host_input: String,
    pub(crate) sidebar_collapsed: bool,

    // Tabs
    pub(crate) tabs: Vec<TerminalTab>,
    /// Set while the new-tab picker is open *to fill a split pane* rather
    /// than open a new tab: `(tab_idx, pane_to_split, axis)`. The picker's
    /// selection (host or local shell) lands in a new pane next to the
    /// target instead of a new tab. `None` = picker opens new tabs.
    pub(crate) pending_pane_split:
        Option<(usize, iced::widget::pane_grid::Pane, iced::widget::pane_grid::Axis)>,
    /// True while the cursor is over the `+` split popover itself. Lets the
    /// hover bridge keep the menu open when moving from the `+` button into
    /// the menu, and close it shortly after the cursor leaves both.
    pub(crate) split_menu_hovered: bool,
    pub(crate) active_tab: Option<usize>,
    /// Last terminal tab that had focus. Preserved when switching to nav-only
    /// views (Snippets, Keys, …) so snippet injection still targets that session.
    pub(crate) last_terminal_tab: Option<usize>,
    pub(crate) hovered_tab: Option<usize>,
    pub(crate) show_new_tab_picker: bool,
    pub(crate) new_tab_picker_search: String,
    /// When set, the new-tab picker is drilled into this group, showing
    /// its members (or, for a cloud-query group, its resolved ECS tasks /
    /// K8s pods) instead of the top-level group + recent list. `None` is
    /// the top level. Reset to `None` whenever the picker opens or closes.
    pub(crate) new_tab_picker_group: Option<Uuid>,
    /// Termius-style "Jump to" modal, lists all open tabs (plus Quick
    /// connect entries) for direct navigation when the bar runs out of
    /// horizontal room. Triggered by the `⋯` button in the tab bar or
    /// Ctrl+J anywhere.
    pub(crate) show_tab_jump: bool,
    pub(crate) tab_jump_search: String,
    /// Top-left burger menu visibility. Mirrors Termius's `☰` strip at
    /// the start of the tab bar: Settings / Updates / About / Exit.
    /// Toggled via the burger button or by pressing the same button
    /// again to dismiss.
    pub(crate) show_burger_menu: bool,

    // Icon/color picker (from the host editor's icon box).
    pub(crate) show_icon_picker: bool,
    pub(crate) icon_picker_for: Option<Uuid>,
    /// When true, the icon picker writes its result back to the
    /// dynamic group editor form fields (`cloud_dynamic_form_icon` /
    /// `_color`) instead of persisting straight to a Connection in the
    /// vault. Lets the same picker serve both the host editor (saves
    /// directly) and the form-driven dynamic group editor (deferred
    /// save when the user clicks the form's Save).
    pub(crate) icon_picker_for_group_form: bool,
    /// Same idea, targeting the session-group editor form. Deferred save:
    /// the choice flows into `editor_session_group` and persists on the
    /// form's Save.
    pub(crate) icon_picker_for_session_group: bool,
    pub(crate) icon_picker_icon: Option<String>,
    pub(crate) icon_picker_color: Option<String>,
    pub(crate) icon_picker_hex_input: String,
    /// Search query for the icon picker's full-library Lucide search.
    /// Empty shows the curated preset grid; non-empty shows matches.
    pub(crate) icon_picker_icon_search: String,
    /// When set, the icon picker's HSV color popover is open, anchored at
    /// this point (the cursor position when the swatch was clicked). None
    /// keeps the picker collapsed behind the swatch + hex row.
    pub(crate) icon_color_popover: Option<iced::Point>,
    /// Whether the per-host terminal theme picker modal is open.
    /// Drawn on top of the host editor; the form's
    /// `terminal_theme` field is updated as soon as the user picks
    /// a card.
    pub(crate) show_theme_picker: bool,
    /// Whether the jump host picker modal is open. Opened from the
    /// Chain editor (Termius-style multi-hop jump-host editor), opened
    /// from the "Host Chaining" row in the host editor. `adding` flips
    /// the modal into "pick a host to append" mode; the search filters
    /// that list by label, hostname, group, or username.
    pub(crate) show_chain_editor: bool,
    pub(crate) chain_editor_adding: bool,
    pub(crate) chain_editor_search: String,
    pub(crate) connecting: Option<ConnectionProgress>,
    /// Counter that advances ~every 100ms while a connection is in progress.
    /// Used only to drive the pulsing "loading" ring on the active step dot.
    pub(crate) connect_anim_tick: u32,
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
    // this struct: theme_editor, ui_theme_editor, show_theme_import,
    // show_share_dialog, cloud_import_confirm_visible, folder_rename,
    // folder_delete, show_*_picker, ...) each drive a modal overlay. Any new
    // one that carries a text field MUST be added to
    // `any_modal_blocks_input()` (and, if global, `close_topmost_modal()`)
    // in shortcuts.rs, or its keystrokes leak into the terminal behind it.
    // Render every blocking modal through `widgets::modal_overlay` so the
    // scrim can't reintroduce mouse bleed-through.

    // Host key verification dialog
    pub(crate) pending_host_key: Option<oryxis_ssh::HostKeyQuery>,
    pub(crate) host_key_response_tx: Option<tokio::sync::mpsc::Sender<bool>>,

    // Keyboard-interactive (2FA / OTP) prompt dialog. `pending_kbi_prompt`
    // is the current challenge round; `kbi_inputs` holds one answer buffer
    // per prompt (parallel to `prompts`); the response channel carries
    // `Some(answers)` on submit or `None` on cancel back to the engine.
    pub(crate) pending_kbi_prompt: Option<oryxis_ssh::KbiQuery>,
    pub(crate) kbi_inputs: Vec<String>,
    pub(crate) kbi_response_tx: Option<tokio::sync::mpsc::Sender<Option<Vec<String>>>>,

    // Connection editor
    pub(crate) show_host_panel: bool,
    pub(crate) editor_form: ConnectionForm,
    /// Multi-line buffer for the host's initial command. Kept out of the
    /// form struct because `text_editor::Content` isn't Clone.
    pub(crate) editor_initial_command: iced::widget::text_editor::Content,
    pub(crate) host_panel_error: Option<String>,

    // Session group editor (save / edit a split arrangement)
    pub(crate) show_session_group_panel: bool,
    pub(crate) editor_session_group: crate::state::SessionGroupForm,
    /// Multi-line buffer for the currently-shown pane's startup script. Kept
    /// out of the form struct because `text_editor::Content` isn't Clone.
    pub(crate) session_group_script_editor: iced::widget::text_editor::Content,
    pub(crate) session_group_panel_error: Option<String>,
    /// Hovered session-group card on the dashboard, drives the `⋮` menu,
    /// mirroring `hovered_card`.
    pub(crate) hovered_session_group_card: Option<usize>,
    /// Per-pane initial-script overrides, keyed by the pane's stable id.
    /// Populated when a session group is opened; consumed (and removed)
    /// once the pane's shell is ready and the script is injected. Lets the
    /// override win over the host's own `initial_command` for that pane.
    pub(crate) pane_script_overrides: std::collections::HashMap<Uuid, String>,

    // Card hover & context menu
    pub(crate) hovered_card: Option<usize>,
    /// Hovered folder card on the dashboard (root view), drives the
    /// `⋮` menu visibility, mirroring `hovered_card` for hosts.
    pub(crate) hovered_folder_card: Option<Uuid>,
    /// Hovered key card / identity card in the keychain view, same
    /// hover-only-dots UX as host cards.
    pub(crate) hovered_key_card: Option<usize>,
    pub(crate) hovered_identity_card: Option<usize>,
    pub(crate) hovered_snippet_card: Option<usize>,
    /// Snippet card whose `⋮` context menu (Edit / Delete) is open;
    /// keeps the kebab visible while the popup is up, mirroring
    /// `card_context_menu` for hosts.
    pub(crate) snippet_context_menu: Option<usize>,
    pub(crate) card_context_menu: Option<usize>,

    // Floating overlay menu
    pub(crate) overlay: Option<OverlayState>,
    /// Folder rename inline editor, `Some((group_id, current_input))`
    /// while the modal is open.
    pub(crate) folder_rename: Option<(Uuid, String)>,
    /// Folder delete confirmation, group ID waiting for the user to
    /// pick "move hosts to root" / "delete with hosts" / cancel.
    pub(crate) folder_delete: Option<Uuid>,
    /// Connection ID to auto-open after the vault unlocks. Set from the
    /// `--connect` CLI flag captured at process start; cleared once the
    /// dispatch fires so a vault re-lock + unlock doesn't re-trigger it.
    pub(crate) pending_auto_connect: Option<Uuid>,
    /// Master password retained in memory for spawning child processes
    /// (Duplicate in New Window). Populated after a successful
    /// unlock / setup, cleared if the user explicitly re-locks.
    pub(crate) master_password: Option<String>,
    /// SFTP browser state. Lives at the top level so the user can pick
    /// a different host without losing their local-pane navigation.
    pub(crate) sftp: crate::state::SftpState,
    pub(crate) mouse_position: Point,
    pub(crate) window_size: iced::Size,
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
    /// Whether the OS window is currently maximized. Used by the custom
    /// chrome to swap the maximize glyph for a "restore" glyph. Toggled
    /// optimistically on `WindowMaximizeToggle` since our chrome is the only
    /// path that can change this state (native titlebar is disabled).
    pub(crate) window_maximized: bool,
    /// Whether the window is in native fullscreen mode. Flipped by F11.
    /// Same optimistic pattern as `window_maximized` because the OS-side
    /// transition is one-way from the app.
    pub(crate) window_fullscreen: bool,
    /// True for ~3 s after entering fullscreen so the "Press F11 to
    /// exit" banner renders. Cleared by a scheduled
    /// `Message::FullscreenHintHide`. Mirrors Chrome / Firefox where
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
    pub(crate) editing_hotkey: Option<crate::hotkeys::HotkeyAction>,

    // Keys
    pub(crate) keys: Vec<SshKey>,
    pub(crate) show_key_panel: bool,
    pub(crate) key_import_label: String,
    pub(crate) key_import_content: text_editor::Content,
    pub(crate) key_import_pem: String,  // raw string for import
    /// Passphrase for an encrypted private key. Lives in memory only, once
    /// the key is decrypted on import, it is re-encoded unencrypted and the
    /// vault's master key takes over for at-rest protection.
    pub(crate) key_import_passphrase: String,
    /// Set when import_key returns `KeyNeedsPassphrase`. Drives the
    /// passphrase row in the import panel.
    pub(crate) key_import_passphrase_required: bool,
    pub(crate) key_import_passphrase_visible: bool,
    pub(crate) key_error: Option<String>,
    pub(crate) key_success: Option<String>,
    pub(crate) key_context_menu: Option<usize>,
    pub(crate) editing_key_id: Option<Uuid>,
    pub(crate) key_search: String,
    /// Workspace-mode contextual search backing for Snippets view.
    /// Matches against snippet label + command.
    pub(crate) snippet_search: String,
    /// Workspace-mode contextual search backing for History view.
    /// Matches against the connection label / hostname recorded in
    /// each log row.
    pub(crate) history_search: String,

    // Identities
    pub(crate) identities: Vec<Identity>,
    // Cached set of identity ids whose `password` column is non-NULL.
    // Hydrated by `load_data_from_vault`. The keychain view reads this
    // per card to decide whether to render the masked-bullets badge,
    // a per-frame `get_identity_password` decrypt would otherwise run
    // for every identity on every view() rebuild and slow the main
    // loop enough to fill iced's 100-slot subscription channel.
    pub(crate) identities_with_password: std::collections::HashSet<Uuid>,
    pub(crate) show_identity_panel: bool,
    pub(crate) identity_form_label: String,
    pub(crate) identity_form_username: String,
    pub(crate) identity_form_password: String,
    pub(crate) identity_form_key: Option<String>,
    pub(crate) identity_form_password_visible: bool,
    pub(crate) identity_form_password_touched: bool,
    pub(crate) identity_form_has_existing_password: bool,
    pub(crate) editing_identity_id: Option<Uuid>,
    pub(crate) identity_context_menu: Option<usize>,
    pub(crate) show_keychain_add_menu: bool,

    // Per-list sort modes for the Hosts / Keychain / Snippets grids.
    // Persisted via the `hosts_sort` / `keys_sort` / `snippets_sort`
    // settings keys; loaded on boot and rewritten on each pick. The
    // active value drives both the trigger button's glyph and the
    // check mark in the dropdown.
    pub(crate) hosts_sort: crate::state::ListSort,
    pub(crate) keys_sort: crate::state::ListSort,
    pub(crate) snippets_sort: crate::state::ListSort,

    // Proxy Identities, reusable proxy configs edited inline inside
    // the Settings → Proxies section. Form state is in-memory only
    // until SaveProxyIdentity flushes to the vault.
    pub(crate) proxy_identities: Vec<oryxis_core::models::proxy_identity::ProxyIdentity>,
    pub(crate) proxy_identity_form_visible: bool,
    pub(crate) proxy_identity_form_label: String,
    pub(crate) proxy_identity_form_kind: crate::state::ProxyKind,
    pub(crate) proxy_identity_form_host: String,
    pub(crate) proxy_identity_form_port: String,
    pub(crate) proxy_identity_form_username: String,
    pub(crate) proxy_identity_form_password: String,
    pub(crate) proxy_identity_form_password_visible: bool,
    pub(crate) proxy_identity_form_password_touched: bool,
    pub(crate) proxy_identity_form_has_existing_password: bool,
    pub(crate) editing_proxy_identity_id: Option<Uuid>,
    pub(crate) proxy_identity_form_error: Option<String>,

    // Cloud Accounts, CloudProfile rows + the wizard form. Wizard is
    // intentionally minimal in v0.6 PR 3: provider + AWS profile auth
    // only. Access key + SSO + the discover-and-pick step land in
    // follow-up PRs once the foundation is exercised.
    pub(crate) cloud_profiles: Vec<oryxis_core::models::cloud_profile::CloudProfile>,
    pub(crate) cloud_form_visible: bool,
    pub(crate) cloud_form_label: String,
    pub(crate) cloud_form_provider: crate::state::CloudProviderChoice,
    pub(crate) cloud_form_auth_kind: crate::state::CloudAuthChoice,
    pub(crate) cloud_form_aws_profile_name: String,
    /// Workload regions, the first entry is the default region and the
    /// full list drives discovery fan-out. Persisted as both `region`
    /// (= first) and `regions` (= full list) for forward compat with
    /// older builds.
    pub(crate) cloud_form_aws_regions: Vec<String>,
    /// Draft text in the region input box, committed to
    /// `cloud_form_aws_regions` on Enter.
    pub(crate) cloud_form_aws_region_draft: String,
    /// Access Key auth fields. The secret access key follows the
    /// password-tri-state convention (`*_touched` differentiates
    /// "leave alone" from "explicitly cleared").
    pub(crate) cloud_form_aws_access_key_id: String,
    pub(crate) cloud_form_aws_access_key_secret: String,
    pub(crate) cloud_form_aws_access_key_secret_touched: bool,
    pub(crate) cloud_form_aws_access_key_secret_visible: bool,
    pub(crate) cloud_form_aws_access_key_session_token: String,
    pub(crate) cloud_form_aws_has_existing_secret: bool,
    /// SSO (IAM Identity Center) auth fields.
    pub(crate) cloud_form_aws_sso_start_url: String,
    pub(crate) cloud_form_aws_sso_region: String,
    pub(crate) cloud_form_aws_sso_account_id: String,
    pub(crate) cloud_form_aws_sso_role_name: String,
    /// Kubernetes (Kubeconfig) auth fields. Both optional: blank
    /// kubeconfig = kubectl's default, blank context = current-context.
    pub(crate) cloud_form_kubeconfig_path: String,
    pub(crate) cloud_form_context: String,
    pub(crate) editing_cloud_profile_id: Option<Uuid>,
    pub(crate) cloud_form_error: Option<String>,
    pub(crate) cloud_form_test_state: crate::state::CloudTestState,
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
    /// Discovery panel state, opened from a profile card or from the
    /// post-save flow. Carries the in-flight or completed result so
    /// the user picks resources without paying another API round-trip.
    pub(crate) cloud_discover_visible: bool,
    pub(crate) cloud_discover_profile_id: Option<Uuid>,
    pub(crate) cloud_discover_state: crate::state::CloudDiscoverState,
    /// EC2 instance-ids currently checked in the discovery panel.
    pub(crate) cloud_discover_selected_ec2: std::collections::HashSet<String>,
    /// ECS service identifiers checked in the discovery panel.
    /// Key format: `cluster/service/container` (the same triple a
    /// `CloudQuery::EcsTasks` carries), guarantees a stable id even
    /// when service or container names collide across clusters.
    pub(crate) cloud_discover_selected_ecs: std::collections::HashSet<String>,
    /// Kubernetes workload identifiers checked in the discovery panel.
    /// Key format: `namespace/kind/name` (the workload identity the
    /// import looks back up to build a `K8sPods` dynamic group).
    pub(crate) cloud_discover_selected_k8s: std::collections::HashSet<String>,
    /// Live filter for the discovery panel, matches against label,
    /// instance-id, hostname, IP. Lowercased substring match.
    pub(crate) cloud_discover_filter: String,
    /// Section names currently collapsed in the discovery panel
    /// ("ec2" / "ecs" / "k8s"). Persisted only in memory, re-opens
    /// default to expanded.
    pub(crate) cloud_discover_collapsed: std::collections::HashSet<String>,
    /// Default transport applied to every EC2 host imported in this
    /// discovery session. Lets the user pick "Instance Connect" once
    /// instead of editing 10 hosts after the fact. Stored at the
    /// `Oryxis` level (not on the `OverlayState`) so the choice
    /// survives discovery refreshes.
    pub(crate) cloud_discover_default_transport:
        oryxis_core::models::cloud::TransportKind,
    /// Target group name for the next import. Empty string = no
    /// parent (drop at root). Otherwise the import flow finds a group
    /// with this label or creates it on the spot, so the user can
    /// type any name (existing or new) and have it materialised.
    /// Decoupled from the pick_list-based approach so typing a brand
    /// new folder name doesn't require a pre-existing entry.
    pub(crate) cloud_discover_default_group_name: String,
    /// Whether the floating group picker overlay (inside the import
    /// confirmation modal) is open. Chevron toggles it; picking an
    /// entry or clicking the scrim closes it.
    pub(crate) cloud_discover_default_group_picker_open: bool,
    /// Screen-space bounds of the Import-into combo row, populated
    /// by a `bounds_reporter` wrapper. Read by the toggle handler to
    /// anchor the picker overlay right under the chevron without
    /// guessing layout offsets.
    pub(crate) cloud_discover_default_group_combo_bounds: crate::widgets::BoundsCell,
    /// Shared search input for the group picker (used by both side
    /// panels' Parent Group fields). Reset on every open.
    pub(crate) group_picker_search: String,
    /// Bounds of the host editor's Parent Group combo row.
    pub(crate) editor_parent_combo_bounds: crate::widgets::BoundsCell,
    /// Bounds of the dynamic group editor's Parent Group combo row.
    pub(crate) dynamic_form_parent_combo_bounds: crate::widgets::BoundsCell,
    /// Bounds of the session-group editor's Folder combo row.
    pub(crate) session_group_folder_combo_bounds: crate::widgets::BoundsCell,
    /// Bounds of the `+` tab button, so the split hover popover anchors
    /// under it at a fixed position instead of following the cursor.
    pub(crate) plus_btn_bounds: crate::widgets::BoundsCell,
    /// Search text inside the group picker overlay. Independent of
    /// `cloud_discover_default_group_name` (the input box) so typing
    /// in the picker's filter doesn't overwrite the user's chosen
    /// folder name.
    pub(crate) cloud_discover_default_group_picker_search: String,
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
    pub(crate) cloud_dynamic_form_visible: bool,
    pub(crate) cloud_dynamic_form_group_id: Option<Uuid>,
    pub(crate) cloud_dynamic_form_username: String,
    pub(crate) cloud_dynamic_form_initial_command: String,
    pub(crate) cloud_dynamic_form_transport: oryxis_core::models::cloud::TransportKind,
    /// Selected key label (or `"(none)"`); resolved to a `key_id` on save.
    pub(crate) cloud_dynamic_form_selected_key: Option<String>,
    /// Selected identity label (or `"(none)"`); resolved to an `identity_id` on save.
    pub(crate) cloud_dynamic_form_selected_identity: Option<String>,
    /// General-section fields, parity with the host editor so a
    /// dynamic group is a first-class entity (rename, color, icon,
    /// move under any user group). Persisted on Save.
    pub(crate) cloud_dynamic_form_label: String,
    pub(crate) cloud_dynamic_form_color: String,
    pub(crate) cloud_dynamic_form_icon: String,
    pub(crate) cloud_dynamic_form_parent_label: String,
    /// Cloud-source fields (ECS variant). Editing these repoints the group
    /// at a different upstream collection so the next resolve hits the new
    /// cluster/service/container. K8s groups use the `_k8s_*` fields below.
    pub(crate) cloud_dynamic_form_cluster: String,
    pub(crate) cloud_dynamic_form_service: String,
    pub(crate) cloud_dynamic_form_container: String,
    /// K8s dynamic-group source fields, used when the edited group's query
    /// is `K8sPods`. `is_k8s` flips the editor between the ECS and K8s
    /// source sections. The selector value's meaning depends on
    /// `selector_kind`: a `k=v,k=v` string for `Labels`, otherwise a single
    /// resource name.
    pub(crate) cloud_dynamic_form_is_k8s: bool,
    pub(crate) cloud_dynamic_form_k8s_context: String,
    pub(crate) cloud_dynamic_form_namespace: String,
    pub(crate) cloud_dynamic_form_k8s_selector_kind: crate::state::K8sSelectorKind,
    pub(crate) cloud_dynamic_form_k8s_selector_value: String,

    /// Hover tracking for the kebab on dynamic-group cards (root + nested).
    pub(crate) hovered_dynamic_group_card: Option<Uuid>,

    /// Card-hover state for the kebab "..." button on cloud profile
    /// cards in Settings → Cloud, mirroring `hovered_card` /
    /// `hovered_folder_card` for hosts and folders.
    pub(crate) hovered_cloud_card: Option<Uuid>,

    // Snippets
    pub(crate) snippets: Vec<oryxis_core::models::snippet::Snippet>,
    /// User-defined terminal color schemes, shown in the theme pickers
    /// alongside the built-in presets and resolved by name.
    pub(crate) custom_terminal_themes:
        Vec<oryxis_core::models::custom_terminal_theme::CustomTerminalTheme>,
    /// User-defined chrome (UI) themes, shown in Interface alongside the
    /// built-in app themes and resolved by name.
    pub(crate) custom_ui_themes:
        Vec<oryxis_core::models::custom_ui_theme::CustomUiTheme>,
    /// Open custom-theme editor modal. `None` = closed.
    pub(crate) theme_editor: Option<crate::state::ThemeEditorForm>,
    /// Hovered custom terminal theme card (index into
    /// `custom_terminal_themes`), for the floating edit / delete icons.
    pub(crate) hovered_theme_card: Option<usize>,
    /// Open color-picker popover in the theme editor: `(slot, anchor)`.
    /// `None` = closed. Clicking a slot's swatch opens a compact picker
    /// (SV square + hue + hex + presets) anchored at the click.
    pub(crate) theme_color_popover: Option<(crate::state::ThemeColorSlot, iced::Point)>,
    /// Import-theme modal (paste an iTerm / Windows Terminal / base16
    /// scheme). On import the parsed colors open in the editor for review.
    pub(crate) show_theme_import: bool,
    pub(crate) theme_import_content: iced::widget::text_editor::Content,
    pub(crate) theme_import_name: String,
    pub(crate) theme_import_error: Option<String>,
    /// Custom UI (chrome) theme editor modal + its color-picker popover and
    /// the hovered card (mirrors the terminal-theme editor).
    pub(crate) ui_theme_editor: Option<crate::state::UiThemeEditorForm>,
    pub(crate) ui_color_popover: Option<(usize, iced::Point)>,
    pub(crate) hovered_ui_theme_card: Option<usize>,
    /// Name of the active app theme (built-in or custom UI theme). The
    /// `AppTheme` enum can't name a custom theme, so this tracks the
    /// selection for highlighting + delete/rename bookkeeping.
    pub(crate) active_app_theme_name: String,
    pub(crate) show_snippet_panel: bool,
    pub(crate) snippet_label: String,
    pub(crate) snippet_command: text_editor::Content,
    pub(crate) snippet_editing_id: Option<Uuid>,
    pub(crate) snippet_error: Option<String>,

    // Port forwards (standalone entity, independent of any terminal)
    pub(crate) port_forward_rules:
        Vec<oryxis_core::models::port_forward_rule::PortForwardRule>,
    /// Runtime-only registry of live forwards, keyed by rule id. Not
    /// persisted, the on/off state lives only here. Dropping the
    /// `ForwardSession` cancels its tasks.
    pub(crate) active_forwards:
        std::collections::HashMap<Uuid, std::sync::Arc<oryxis_ssh::ForwardSession>>,
    /// Rules whose connect is in flight (drives the per-row spinner and
    /// prevents a double-start).
    pub(crate) port_forward_starting: std::collections::HashSet<Uuid>,
    pub(crate) show_port_forward_panel: bool,
    pub(crate) pf_label: String,
    pub(crate) pf_kind: oryxis_core::models::port_forward_rule::ForwardKind,
    pub(crate) pf_host_id: Option<Uuid>,
    pub(crate) pf_listen_host: String,
    pub(crate) pf_listen_port: String,
    pub(crate) pf_target_host: String,
    pub(crate) pf_target_port: String,
    pub(crate) pf_auto_start: bool,
    pub(crate) pf_editing_id: Option<Uuid>,
    pub(crate) pf_error: Option<String>,
    pub(crate) hovered_port_forward_card: Option<usize>,
    pub(crate) port_forward_search: String,

    // Known hosts & logs
    pub(crate) known_hosts: Vec<oryxis_core::models::known_host::KnownHost>,
    pub(crate) logs: Vec<oryxis_core::models::log_entry::LogEntry>,
    pub(crate) logs_page: usize,
    pub(crate) logs_total: usize,

    // Session logs (terminal recording)
    pub(crate) session_logs: Vec<oryxis_vault::SessionLogEntry>,
    pub(crate) session_logs_page: usize,
    pub(crate) session_logs_total: usize,
    pub(crate) viewing_session_log: Option<(Uuid, String)>, // (log_id, rendered_text)

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
    pub(crate) terminal_font_size: f32,
    pub(crate) terminal_font_name: String,

    // Settings
    pub(crate) settings_section: SettingsSection,
    /// Renderer backend selection: "auto" (default), "opengl" (force
    /// wgpu's GL backend, still GPU), or "software" (tiny-skia / CPU).
    /// `main` translates this into `WGPU_BACKEND` / `ICED_BACKEND` at
    /// startup, an escape hatch for GPU/driver stacks that corrupt the
    /// wgpu surface. Read at boot only (the env vars are resolved before
    /// the runtime starts), so changing it asks the user to restart.
    pub(crate) setting_renderer_backend: String,
    pub(crate) setting_copy_on_select: bool,
    /// Sub-option of `setting_copy_on_select`: when both are on, a selection
    /// copies on right-click instead of on release. Ignored when
    /// `setting_copy_on_select` is off.
    pub(crate) setting_right_click_copy: bool,
    pub(crate) setting_bold_is_bright: bool,
    pub(crate) setting_keyword_highlight: bool,
    /// When the foreground and background of a cell render too close
    /// to each other (LS_COLORS' `ow` over a green palette,
    /// PowerShell's `$PSStyle.FileInfo.Directory` blue-on-blue, …),
    /// the renderer flips the foreground to a legible alternative.
    /// Off makes the renderer paint the cell exactly as the app
    /// asked, which some color-precise tools rely on.
    pub(crate) setting_smart_contrast: bool,
    /// Toggles the bottom status bar that shows current connection IP +
    /// Oryxis version. Off in `view_main` simply skips rendering it,
    /// reclaiming the row for the active content area.
    pub(crate) setting_show_status_bar: bool,
    /// When on, clicking the window's close button hides to the
    /// system tray instead of quitting. Only honoured on Windows
    /// (the tray module is a no-op everywhere else). Default off
    /// so we don't surprise users who never knew there was a tray.
    pub(crate) setting_close_to_tray: bool,
    /// When on, minimizing the window hides it from the taskbar and
    /// leaves only the tray icon visible. Windows-only. Default off.
    pub(crate) setting_minimize_to_tray: bool,
    /// Signature of (tabs len, last tab uuid, connections len, max
    /// last_used timestamp) computed during the last tray menu
    /// rebuild. The TrayPoll handler recomputes the signature each
    /// tick and only rebuilds the menu when it differs. Avoids
    /// burning cycles rebuilding the dynamic submenus 10 times a
    /// second when nothing has changed.
    pub(crate) tray_menu_signature: u64,
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
    /// `"left"` (default, Termius-style: X replaces the OS badge on
    /// hover/active) or `"right"` (badge stays left, X gets its own
    /// slot at the trailing edge of the tab). Anything else is treated
    /// as `"left"`.
    pub(crate) setting_tab_close_button_side: String,
    /// When on, each tab paints a small colored dot over its OS badge:
    /// green for an active SSH session, orange while connecting, red
    /// for a tab that lost its session. Defaults on; the user can hide
    /// it from Settings -> Interface.
    pub(crate) setting_show_tab_status_dot: bool,
    /// When true (default), the hairline under the tab strip thickens
    /// to 2 px and tints itself with the active host's accent (per-
    /// host color → cloud brand → global accent). When false, it
    /// collapses to the same neutral 1 px border the non-tabbed
    /// screens use, so the user always sees a flat chrome regardless
    /// of which host is open.
    pub(crate) setting_tab_accent_line: bool,
    /// Toggles the SFTP feature entirely. Off hides the SFTP sidebar
    /// entry (both expanded and collapsed) so users who never transfer
    /// files don't have it taking up nav space. The SFTP settings panel
    /// still renders so the user can re-enable + tweak in one place,
    /// mirroring how `ai_enabled` works.
    pub(crate) sftp_enabled: bool,
    /// `"classic"` (current sidebar nav) or `"workspace"` (top tabs +
    /// contextual sidebar + burger, PR 6). Persisted ahead of the
    /// workspace mode landing so we can flip the default and migrate
    /// settings in a single later PR without touching boot logic again.
    pub(crate) setting_layout_mode: String,
    /// Default shape for host icons in the dashboard, sidebar tab
    /// badges and host cards: `"circular"` (default v0.7), `"square"`
    /// (legacy Termius-style), `"outline"`, or `"initials"`. Read by
    /// the host icon widget in PR 3; until then the value persists but
    /// the renderer keeps the current shape.
    pub(crate) setting_default_host_icon: String,
    pub(crate) setting_keepalive_interval: String,
    /// Background refresh of every cloud profile on a fixed interval.
    /// Off by default; opt-in to avoid surprise API calls.
    pub(crate) setting_cloud_auto_refresh_enabled: bool,
    /// Minutes between auto-refresh ticks. Stored as a string to match
    /// the rest of the int-setting family (`setting_keepalive_interval`,
    /// etc.) and let the Settings UI accept partial typed input.
    pub(crate) setting_cloud_auto_refresh_interval_minutes: String,
    /// When on, the next boot deletes orphaned cloud-imported hosts
    /// (resource gone upstream) older than `orphan_archive_days`.
    pub(crate) setting_cloud_auto_archive_orphans: bool,
    pub(crate) setting_cloud_orphan_archive_days: String,
    pub(crate) setting_scrollback_rows: String,
    /// Max parallel SFTP transfer slots (uploads/downloads). 1 = serial,
    /// up to 8 = aggressive. Each slot gets its own SFTP subsystem
    /// channel on the same SSH connection so they don't fight for the
    /// shared client mutex.
    pub(crate) setting_sftp_concurrency: String,
    /// TCP connect + SSH transport handshake timeout, in seconds.
    pub(crate) setting_sftp_connect_timeout: String,
    /// Authentication phase timeout, in seconds.
    pub(crate) setting_sftp_auth_timeout: String,
    /// Per-channel open timeout (PTY session, SFTP subsystem, sibling
    /// channels), in seconds.
    pub(crate) setting_sftp_session_timeout: String,
    /// Per-operation timeout for SFTP requests (list_dir, read, write,
    /// metadata). Caps the "Loading…" state so a hung server can't pin
    /// the UI forever.
    pub(crate) setting_sftp_op_timeout: String,
    pub(crate) setting_auto_reconnect: bool,
    pub(crate) setting_max_reconnect_attempts: String,
    pub(crate) setting_os_detection: bool,
    pub(crate) setting_auto_check_updates: bool,
    /// Release stream the updater follows (`stable` / `nightly`).
    pub(crate) setting_update_channel: crate::update::UpdateChannel,

    // Update state (set by the async GitHub check on boot)
    pub(crate) pending_update: Option<crate::update::UpdateInfo>,
    pub(crate) update_downloading: bool,
    pub(crate) update_progress: f32,
    pub(crate) update_error: Option<String>,
    /// Last manual-check outcome shown near the "Check now" button in
    /// settings. `Some("")` → in-flight; `Some("Up to date.")` → no newer
    /// release; `Some("Error: …")` on failure. `None` hides the line.
    pub(crate) update_check_status: Option<String>,
    /// Attempt counters keyed by connection UUID, persists across tab recreations.
    pub(crate) reconnect_counters: std::collections::HashMap<Uuid, u32>,

    // AI Chat settings
    pub(crate) ai_enabled: bool,
    pub(crate) ai_provider: String,
    pub(crate) ai_model: String,
    pub(crate) ai_api_key: String,
    pub(crate) ai_api_key_set: bool,
    pub(crate) ai_api_url: String,
    pub(crate) ai_system_prompt: text_editor::Content,

    // Vault password settings
    pub(crate) vault_has_user_password: bool,
    pub(crate) vault_new_password: String,
    pub(crate) vault_password_error: Option<String>,
    pub(crate) vault_destroy_confirm: bool,

    /// Transient bottom-of-chat status chip, currently used for the
    /// "Copied to clipboard" feedback after a Copy button click.
    /// `Some(text)` → render the chip; cleared after ~1.8 s by a
    /// `Task::perform`-spawned `ToastClear` round-trip.
    pub(crate) toast: Option<String>,

    /// Generic blocking error dialog. Use for cases the user must read
    /// (install instructions, fatal config errors) where a 1.8 s toast
    /// would vanish before they can act on it. `None` = no dialog.
    pub(crate) error_dialog: Option<crate::state::ErrorDialog>,

    /// Cached list of available local shells (PowerShell, cmd, WSL
    /// distros, etc.), populated lazily when the user opens the
    /// Local Shell picker so we don't pay the `wsl --list` spawn on
    /// every boot. `None` means not detected yet.
    pub(crate) local_shells: Option<Vec<crate::state::LocalShellSpec>>,
    /// True while the Local Shell picker overlay is showing. Only
    /// surfaces on Windows where there's a real choice between cmd /
    /// PowerShell / WSL distros, non-Windows just spawns the
    /// default shell directly.
    pub(crate) local_shell_picker_open: bool,

    // AI chat sidebar
    pub(crate) chat_input: text_editor::Content,
    pub(crate) chat_loading: bool,
    /// True when the user's scroll is anchored at (or very near) the bottom
    /// of the chat history, used to decide whether new assistant messages
    /// should auto-scroll. If the user has scrolled up to read older
    /// content, we leave them where they are.
    pub(crate) chat_scroll_at_bottom: bool,
    /// Active tab in the terminal side panel (Chat / Snippets / History).
    pub(crate) terminal_sidebar_tab: crate::state::TerminalSidebarTab,
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
    /// User-resizable width of the chat sidebar in pixels.
    pub(crate) chat_sidebar_width: f32,
    /// Some((cursor_x_at_drag_start, sidebar_width_at_drag_start)) while
    /// the user is dragging the resize handle on the sidebar's left edge.
    pub(crate) chat_sidebar_drag: Option<(f32, f32)>,

    // MCP Server
    pub(crate) mcp_server_enabled: bool,
    pub(crate) show_mcp_info: bool,
    pub(crate) mcp_config_copied: bool,
    pub(crate) mcp_install_status: Option<Result<String, String>>,
    /// Token MCP clients must present (via `ORYXIS_MCP_TOKEN` env)
    /// to talk to the server. Empty disables auth (backward-compat).
    pub(crate) mcp_server_token: String,
    /// When true, the token is rendered as plain text in the panel;
    /// otherwise as a row of bullets. Sensitive enough to keep masked
    /// by default, the user opts in to seeing it.
    pub(crate) mcp_token_visible: bool,
    /// Which client the setup snippet / Copy / Install target: the
    /// native client (`false`) or one running inside WSL (`true`). Only
    /// reachable on Windows, where the toggle that flips it renders.
    pub(crate) mcp_target_wsl: bool,

    // Sync
    pub(crate) sync_enabled: bool,
    pub(crate) sync_mode: String,
    /// When on, sync wraps connection / identity / proxy-identity
    /// payloads with their decrypted passwords so peers can mirror
    /// them. Off by default, passwords stay device-local until the
    /// user explicitly opts in via Settings → Sync.
    pub(crate) sync_passwords: bool,
    /// When on, the dashboard root shows two sections, Groups (manual
    /// folder cards) and Hosts (a flat list of every connection,
    /// including those that live inside a group). When off, root
    /// matches the legacy behaviour: groups at top, only ungrouped
    /// hosts beneath. Default: on.
    pub(crate) flatten_hosts: bool,
    pub(crate) sync_device_name: String,
    pub(crate) sync_signaling_url: String,
    /// Bearer token for the signaling endpoint. Sent on every
    /// `POST /register` / `GET /lookup`. Empty == not configured.
    pub(crate) sync_signaling_token: String,
    pub(crate) sync_relay_url: String,
    pub(crate) sync_listen_port: String,
    pub(crate) sync_peers: Vec<oryxis_vault::SyncPeerRow>,
    pub(crate) sync_pairing_code: Option<String>,
    pub(crate) sync_status: Option<String>,
    /// Live P2P sync engine, present only while sync is enabled. Holds
    /// a dedicated vault handle plus the QUIC / mDNS background tasks.
    pub(crate) sync_runtime: Option<crate::sync_runtime::SyncRuntime>,
    /// Mirrors `sync_runtime.is_some()` for cheap UI checks.
    pub(crate) sync_engine_running: bool,
    /// Which pairing sub-view the Sync settings panel shows.
    pub(crate) sync_pairing_state: crate::state::SyncPairingState,
    /// The 6-digit code typed in when joining another device's pairing.
    pub(crate) sync_join_code_input: String,
    /// The host address (`ip:port`) typed in when joining a pairing.
    pub(crate) sync_join_target_input: String,
    /// Shareable `oryxis://pair/...` link for the currently-hosted
    /// pairing code, cleared on cancel / complete.
    pub(crate) sync_pairing_link: Option<String>,
    /// `oryxis://pair/...` link pasted in by the joiner as an
    /// alternative to typing code + `ip:port`. Resolved via signaling.
    pub(crate) sync_join_link_input: String,
    /// Live mDNS-discovered peers on the LAN. Deduped by `device_id`.
    pub(crate) sync_discovered: Vec<crate::state::DiscoveredPeerInfo>,
    /// `Sync Now` in flight. Drives the Cancel button + suppresses
    /// re-clicks on Sync Now while a sync is already running.
    pub(crate) sync_in_progress: bool,
    /// One-shot abort channel for the in-flight `Sync Now` Task. The
    /// task races `sync_now().await` against this receiver, so
    /// `Cancel` immediately drops the QUIC connection.
    pub(crate) sync_abort_tx: Option<tokio::sync::oneshot::Sender<()>>,
    /// Visible heartbeat counter for signaling re-registers. Bumps on
    /// every successful `SignalingRegistered` event so the user can
    /// confirm the heartbeat is alive (otherwise re-registers on the
    /// same IP look identical and the status freezes).
    pub(crate) sync_signaling_tick: u32,

    // Export/Import
    pub(crate) show_export_dialog: bool,
    pub(crate) export_password: String,
    pub(crate) export_include_keys: bool,
    pub(crate) export_status: Option<Result<String, String>>,
    pub(crate) show_import_dialog: bool,
    pub(crate) import_password: String,
    pub(crate) import_file_data: Option<Vec<u8>>,
    pub(crate) import_status: Option<Result<String, String>>,
    /// Latest result of an `~/.ssh/config` import, `Ok(message)` is
    /// rendered as a green banner, `Err` as red, in the Security
    /// section's import card.
    pub(crate) ssh_config_import_status: Option<Result<String, String>>,

    // Share
    pub(crate) show_share_dialog: bool,
    pub(crate) share_password: String,
    pub(crate) share_include_keys: bool,
    pub(crate) share_filter: Option<oryxis_vault::ExportFilter>,
    pub(crate) share_status: Option<Result<String, String>>,
    /// Default file name suggested in the save dialog, derived from the
    /// connection label (single host) or group label. `None` falls back
    /// to a generic name.
    pub(crate) share_suggested_name: Option<String>,
}


// ---------------------------------------------------------------------------
// Application
// ---------------------------------------------------------------------------

// `boot`, `load_data_from_vault`, `persist_setting` live in `crate::boot`.

impl Oryxis {
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
        //   + button(24) + gap(4) = 90
        // Add sub-nav (~40) on top when Workspace+vault renders it.
        // Classic was getting the Workspace value before, so the menu
        // hung well below the trigger button.
        const BASE_Y: f32 = 95.0;
        const SUBNAV_HEIGHT: f32 = 50.0;
        let in_workspace_vault = self.setting_layout_mode == "workspace"
            && self.active_tab.is_none()
            && matches!(
                self.active_view,
                View::Dashboard
                    | View::Keys
                    | View::Snippets
                    | View::PortForwarding
                    | View::History
            );
        if in_workspace_vault { BASE_Y + SUBNAV_HEIGHT } else { BASE_Y }
    }

    pub(crate) fn snippet_injection_tab(&self) -> Option<usize> {
        let idx = self.active_tab.or(self.last_terminal_tab)?;
        (idx < self.tabs.len()).then_some(idx)
    }

    pub(crate) fn remember_terminal_tab_focus(&mut self, idx: usize) {
        if idx < self.tabs.len() {
            self.last_terminal_tab = Some(idx);
        }
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
