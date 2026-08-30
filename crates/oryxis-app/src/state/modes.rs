//! Top-level UI modes (split out of `state.rs`).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum VaultState {
    #[default]
    Loading,
    NeedSetup,
    Locked,
    Unlocked,
}

/// Active tab inside the terminal-side panel. `Chat` is only reachable
/// when AI is enabled; the dispatch falls back to `Snippets` otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TerminalSidebarTab {
    #[default]
    Chat,
    Snippets,
    /// Per-host command history (top frequent + recent), captured by the
    /// OSC 133 / input-mirror pipeline.
    History,
    /// Remote file browser for the focused pane's SSH session (an SFTP
    /// channel multiplexed on the live handle), with follow-cwd via the
    /// OSC 7 the terminal already captures. SSH-only: the tab button is
    /// hidden (and the dispatch falls back to `Snippets`) when the pane
    /// has no SSH transport.
    Files,
    /// Agentless resource monitor for the focused pane's host: CPU /
    /// memory / load / disk / network read from `/proc` over the live
    /// session (issue #83). SSH-only and opt-in per host, like Files.
    Monitor,
    /// tmux session manager for the focused pane's host: list, create,
    /// attach and kill, all by running tmux itself over the live
    /// session (issue #116). SSH-only, and behind its own feature
    /// toggle like Monitor.
    Tmux,
    /// Per-host appearance/behavior settings for the focused pane's
    /// connection, edited live with the terminal visible alongside.
    HostConfig,
    /// mRemoteNG-style tree view of the vault's groups + hosts
    /// (issue #102): expand/collapse nested folders, click a host to
    /// open a session. Session-independent like Snippets, so a region
    /// holding only this tab is always available.
    HostsTree,
}

/// Which of the two terminal-sidebar regions a tab is docked to. Sides
/// are PHYSICAL edges (like the #87 tab-bar dock): the user picked
/// them, so RTL must not flip the placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SidebarSide {
    Left,
    Right,
}

impl SidebarSide {
    pub const BOTH: [SidebarSide; 2] = [SidebarSide::Left, SidebarSide::Right];

    /// Index into per-side state arrays (`[T; 2]`).
    pub fn idx(self) -> usize {
        match self {
            SidebarSide::Left => 0,
            SidebarSide::Right => 1,
        }
    }

    pub fn other(self) -> SidebarSide {
        match self {
            SidebarSide::Left => SidebarSide::Right,
            SidebarSide::Right => SidebarSide::Left,
        }
    }
}

/// Where a sidebar tab lives (the per-tab location setting): one of
/// the two regions, or hidden entirely. A hidden tab disappears from
/// the strips, the chrome toggles and the FocusSidebarList cycle,
/// the "I never use this" escape hatch the location picker offers
/// next to Left / Right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarPlacement {
    Left,
    Right,
    Hidden,
}

impl SidebarPlacement {
    pub const ALL: [SidebarPlacement; 3] =
        [SidebarPlacement::Left, SidebarPlacement::Right, SidebarPlacement::Hidden];

    /// The region this placement docks to; `None` = hidden.
    pub fn side(self) -> Option<SidebarSide> {
        match self {
            SidebarPlacement::Left => Some(SidebarSide::Left),
            SidebarPlacement::Right => Some(SidebarSide::Right),
            SidebarPlacement::Hidden => None,
        }
    }

    /// Stable code persisted in the `sidebar_tab_sides` setting.
    pub fn code(self) -> &'static str {
        match self {
            SidebarPlacement::Left => "left",
            SidebarPlacement::Right => "right",
            SidebarPlacement::Hidden => "hidden",
        }
    }

    pub fn from_code(code: &str) -> Option<SidebarPlacement> {
        SidebarPlacement::ALL.into_iter().find(|p| p.code() == code)
    }

    /// i18n key for the Left / Right / Hidden picker labels.
    pub fn label_key(self) -> &'static str {
        match self {
            SidebarPlacement::Left => "sidebar_side_left",
            SidebarPlacement::Right => "sidebar_side_right",
            SidebarPlacement::Hidden => "sidebar_side_hidden",
        }
    }
}

impl TerminalSidebarTab {
    /// Every tab, in strip order. Backs the "Default sidebar tab"
    /// picker (issue #85).
    pub const ALL: [TerminalSidebarTab; 8] = [
        TerminalSidebarTab::HostsTree,
        TerminalSidebarTab::Chat,
        TerminalSidebarTab::Snippets,
        TerminalSidebarTab::History,
        TerminalSidebarTab::Files,
        TerminalSidebarTab::Monitor,
        TerminalSidebarTab::Tmux,
        TerminalSidebarTab::HostConfig,
    ];

    /// Stable code persisted in the `sidebar_default_tab` setting.
    pub fn code(self) -> &'static str {
        match self {
            TerminalSidebarTab::Chat => "chat",
            TerminalSidebarTab::Snippets => "snippets",
            TerminalSidebarTab::History => "history",
            TerminalSidebarTab::Files => "files",
            TerminalSidebarTab::Monitor => "monitor",
            TerminalSidebarTab::Tmux => "tmux",
            TerminalSidebarTab::HostConfig => "hostconfig",
            TerminalSidebarTab::HostsTree => "hosts",
        }
    }

    /// Parse a persisted code back to a tab; unknown codes (and the
    /// "last opened" sentinel) return `None`.
    pub fn from_code(code: &str) -> Option<TerminalSidebarTab> {
        TerminalSidebarTab::ALL.into_iter().find(|t| t.code() == code)
    }

    /// i18n key for this tab's label, reusing the tab-strip tooltip
    /// keys so the picker and the strip never drift.
    pub fn label_key(self) -> &'static str {
        match self {
            TerminalSidebarTab::Chat => "tab_tip_chat",
            TerminalSidebarTab::Snippets => "snippets",
            TerminalSidebarTab::History => "tab_tip_history",
            TerminalSidebarTab::Files => "tab_tip_files",
            TerminalSidebarTab::Monitor => "tab_tip_monitor",
            TerminalSidebarTab::Tmux => "tab_tip_tmux",
            TerminalSidebarTab::HostConfig => "tab_tip_host_config",
            TerminalSidebarTab::HostsTree => "tab_tip_hosts",
        }
    }

    /// Where a tab lives when the user never chose: every tab starts
    /// in the historical RIGHT region (owner call: the hosts tree
    /// too, so a fresh install keeps one sidebar and one toggle; the
    /// left region only exists once someone moves a tab there).
    pub fn default_placement(self) -> SidebarPlacement {
        SidebarPlacement::Right
    }
}

/// How the Hosts dashboard lays its content out (issue #102 follow
/// up): the responsive card grid, a single-column list, or the
/// mRemoteNG-style tree (every level visible, folders expand in
/// place, no drill-down).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HostViewMode {
    #[default]
    Grid,
    List,
    Tree,
}

impl HostViewMode {
    /// The toolbar button walks the three modes in this order.
    pub fn next(self) -> HostViewMode {
        match self {
            HostViewMode::Grid => HostViewMode::List,
            HostViewMode::List => HostViewMode::Tree,
            HostViewMode::Tree => HostViewMode::Grid,
        }
    }

    /// Stable code persisted in the `host_view_mode` setting.
    pub fn code(self) -> &'static str {
        match self {
            HostViewMode::Grid => "grid",
            HostViewMode::List => "list",
            HostViewMode::Tree => "tree",
        }
    }

    pub fn from_code(code: &str) -> Option<HostViewMode> {
        [HostViewMode::Grid, HostViewMode::List, HostViewMode::Tree]
            .into_iter()
            .find(|m| m.code() == code)
    }
}

/// Identifies a secret text field whose reveal/eye toggle is on. One
/// shared enum + a `HashSet` in app state instead of a bool per field,
/// so adding the eye to a new password input is a one-variant change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecretField {
    /// Password on the Share (portable export) dialog.
    SharePassword,
    /// AI assistant API key (Settings > AI).
    AiApiKey,
    /// New master password (Settings > Security).
    VaultNewPassword,
    /// Confirm new master password (Settings > Security).
    VaultConfirmPassword,
    /// Current master password in the change-password form (Settings > Security).
    VaultCurrentPassword,
    /// Portable export password (Settings > Security).
    ExportPassword,
    /// Portable import password (Settings > Security).
    ImportPassword,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Dashboard,
    Terminal,
    Keys,
    Snippets,
    PortForwarding,
    /// Proxy-identity CRUD. Promoted to a top-level vault surface.
    Proxies,
    /// Known-host management. Promoted back to a top-level vault
    /// surface (was a SettingsSection in v0.7).
    KnownHosts,
    History,
    Sftp,
    Settings,
    /// Multi-host monitor dashboard (issue #95): live vitals across
    /// every opted-in host. Not a sub-nav pill: entered through the
    /// Hosts toolbar's monitor icon, which only renders while the
    /// master `host_monitoring` toggle is on (optional-features rule).
    Monitoring,
}

/// One row in the Plugins panel: a locally present plugin and its
/// state. Plugins ship as binaries (see `crate::plugins`); this is
/// the UI-side view of one. The app never fetches them from the
/// network — the cache holds whatever was placed on disk.
#[derive(Debug, Clone)]
pub struct PluginUiEntry {
    /// Plugin provider id (`"mcp"`, `"gif"`, ...).
    pub provider_id: String,
    /// Human-readable name shown in the panel.
    pub display_name: String,
    /// Current install state.
    pub status: PluginUiStatus,
    /// Downloaded binaries exist in the plugin cache (or, for MCP,
    /// the launcher copy). Lets a dev build still offer "remove
    /// downloaded files" for the cache it shadows.
    pub cached_install: bool,
}

/// Install lifecycle state for a [`PluginUiEntry`].
#[derive(Debug, Clone, PartialEq)]
pub enum PluginUiStatus {
    /// No binary on disk and no dev build: the provider can't be
    /// used until a build is placed in the plugin cache.
    NotInstalled,
    /// Running from a freshly-built `target/debug` binary (the dev
    /// loop). No version directory involved.
    DevBuild,
    /// Installed from the cache at this version.
    Installed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SettingsSection {
    Terminal,
    /// SSH connection behaviour shared across hosts: keepalive
    /// interval, auto-reconnect, OS detection. Split out of the
    /// Terminal section, which had grown into a grab-bag of terminal
    /// display, connection and logging knobs.
    Connection,
    Sftp,
    /// Host monitoring config (issue #83), shown only while the
    /// monitoring feature is enabled in Features & Plugins.
    Monitoring,
    AI,
    /// Visual + layout preferences. Absorbs the legacy "Theme" section
    /// and adds toggles for status bar visibility and (in later PRs)
    /// layout mode, tab close button position, host icon style, etc.
    Interface,
    /// MCP server (Model Context Protocol). Was bundled into the
    /// installer in 0.6 and lived inside the Security section; in
    /// 0.7 it's distributed as a plugin and gets its own section
    /// in the Settings sidebar so the setup-guide affordances and
    /// the enable toggle aren't buried.
    Mcp,
    Shortcuts,
    Security,
    /// SSH agent server configuration: per-signature confirm, external
    /// key adds, the OpenSSH pipe alias (Windows) and the socket path +
    /// setup snippets. The enable toggle stays on the Features screen
    /// (like AI / SFTP); this section only appears while the
    /// agent is enabled.
    Agent,
    /// Plugin management: install, update, uninstall the
    /// distributed plugins.
    Plugins,
    /// Troubleshooting surface: the debug-logging file toggle and the
    /// environment report to paste into GitHub issues. Sits between the
    /// feature sections and About; nothing here is everyday config.
    Advanced,
    About,
}

impl SettingsSection {
    /// Stable id of the section's content scrollable. Static literals
    /// because the fork's `widget::Id::new` only takes `&'static str`.
    /// The keyboard router snaps these to keep the selected row in
    /// view; each section view sets the same id on its scrollable.
    pub(crate) fn scroll_id(self) -> &'static str {
        match self {
            SettingsSection::Terminal => "settings-terminal-scroll",
            SettingsSection::Connection => "settings-connection-scroll",
            SettingsSection::Sftp => "settings-sftp-scroll",
            SettingsSection::Monitoring => "settings-monitoring-scroll",
            SettingsSection::AI => "settings-ai-scroll",
            SettingsSection::Interface => "settings-interface-scroll",
            SettingsSection::Mcp => "settings-mcp-scroll",
            SettingsSection::Shortcuts => "settings-shortcuts-scroll",
            SettingsSection::Security => "settings-security-scroll",
            SettingsSection::Agent => "settings-agent-scroll",
            SettingsSection::Plugins => "settings-plugins-scroll",
            SettingsSection::Advanced => "settings-advanced-scroll",
            SettingsSection::About => "settings-about-scroll",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SidebarPlacement, TerminalSidebarTab};

    #[test]
    fn sidebar_placement_code_roundtrips() {
        // Every placement survives code -> from_code so the persisted
        // `sidebar_tab_sides` setting and the location pickers resolve
        // back exactly; junk resolves to None (fall back to the tab's
        // default placement), never a wrong one.
        for p in SidebarPlacement::ALL {
            assert_eq!(SidebarPlacement::from_code(p.code()), Some(p));
        }
        assert_eq!(SidebarPlacement::from_code(""), None);
        assert_eq!(SidebarPlacement::from_code("bogus"), None);
    }

    #[test]
    fn sidebar_tab_code_roundtrips_and_rejects_sentinels() {
        // Every tab survives code -> from_code so the persisted
        // `sidebar_default_tab` setting resolves back exactly (issue #85).
        for t in TerminalSidebarTab::ALL {
            assert_eq!(TerminalSidebarTab::from_code(t.code()), Some(t));
        }
        // The "last opened" sentinel and any junk resolve to None (keep
        // the last tab), never a wrong tab.
        assert_eq!(TerminalSidebarTab::from_code("last"), None);
        assert_eq!(TerminalSidebarTab::from_code(""), None);
        assert_eq!(TerminalSidebarTab::from_code("bogus"), None);
    }
}
