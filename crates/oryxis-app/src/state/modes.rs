//! Top-level UI modes (split out of `state.rs`).

use super::*;

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

/// Where an SFTP console lands when it opens on a tab that already
/// has a session.
///
/// All three are placements INSIDE that tab, never a tab of its own.
/// The console the user asked for is the one on the host in front of
/// them, and its first shape (a tab of its own) is what made that read
/// as a second session on the same machine. `Full` is not a fourth
/// mechanism either: it is the split, zoomed, so the way back is the
/// same toggle the split already has and no state is invented for it.
///
/// A console opened from a host CARD still opens its own tab, because
/// there is no tab to place it in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SftpConsoleLayout {
    /// Stacked under the shell. The default because a console keeps
    /// the full width there, and a listing is the one thing it prints
    /// that a halved column count would wrap.
    #[default]
    SplitBelow,
    /// Side by side with the shell.
    SplitSide,
    /// Split (stacked) and then zoomed, so the console fills the tab
    /// while the shell keeps running behind it.
    Full,
}

impl SftpConsoleLayout {
    /// Every layout, in picker order.
    pub const ALL: [SftpConsoleLayout; 3] = [
        SftpConsoleLayout::SplitBelow,
        SftpConsoleLayout::SplitSide,
        SftpConsoleLayout::Full,
    ];

    /// Stable code persisted in the `sftp_console_layout` setting.
    pub fn code(self) -> &'static str {
        match self {
            SftpConsoleLayout::SplitBelow => "split_below",
            SftpConsoleLayout::SplitSide => "split_side",
            SftpConsoleLayout::Full => "full",
        }
    }

    /// Parse a persisted code; an unknown one falls back to the
    /// default rather than failing, like every other stored mode here.
    pub fn from_code(code: &str) -> Option<SftpConsoleLayout> {
        SftpConsoleLayout::ALL.into_iter().find(|l| l.code() == code)
    }

    pub fn label_key(self) -> &'static str {
        match self {
            SftpConsoleLayout::SplitBelow => "sftp_console_layout_below",
            SftpConsoleLayout::SplitSide => "sftp_console_layout_side",
            SftpConsoleLayout::Full => "sftp_console_layout_full",
        }
    }

    /// The split the console pane is created with. `Full` splits
    /// stacked too: un-zooming it has to land on a usable layout, and
    /// stacked is the one this picker calls the default.
    pub fn axis(self) -> iced::widget::pane_grid::Axis {
        match self {
            SftpConsoleLayout::SplitSide => iced::widget::pane_grid::Axis::Vertical,
            _ => iced::widget::pane_grid::Axis::Horizontal,
        }
    }

    /// Whether the console pane is zoomed the moment it is created.
    pub fn starts_maximized(self) -> bool {
        matches!(self, SftpConsoleLayout::Full)
    }
}

/// Which of a terminal tab's surfaces is on screen.
///
/// The tab chip and the status-bar segments switch between these, and
/// they are NOT three flags: `Files` is the tab-level `files_mode`,
/// while `Terminal` and `Console` are two panes of the same grid. What
/// makes one control serve all three is that every switch is expressed
/// as "show this one", never as a toggle of whichever mechanism
/// happens to back it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabSurface {
    Terminal,
    Console,
    Files,
}

impl TabSurface {
    /// i18n key for the status-bar segment (a noun: what it shows).
    pub fn label_key(self) -> &'static str {
        match self {
            TabSurface::Terminal => "tab_mode_terminal",
            TabSurface::Console => "tab_mode_console",
            TabSurface::Files => "tab_mode_files",
        }
    }

    /// i18n key for the chip tooltip (a verb: what clicking does).
    pub fn action_key(self) -> &'static str {
        match self {
            TabSurface::Terminal => "tab_show_terminal",
            TabSurface::Console => "tab_show_console",
            TabSurface::Files => "tab_show_files",
        }
    }

    /// The surface a cycling control moves to next.
    ///
    /// The tab chip has room for ONE glyph, so with three surfaces it
    /// cycles; the status bar names them all and jumps straight there.
    /// A `current` that is not in the list (a frame behind the state)
    /// starts the cycle at the first entry rather than declining, so
    /// the chip is never a dead button.
    pub fn next_in(surfaces: &[TabSurface], current: TabSurface) -> Option<TabSurface> {
        if surfaces.len() < 2 {
            return None;
        }
        let at = surfaces.iter().position(|s| *s == current).unwrap_or(0);
        Some(surfaces[(at + 1) % surfaces.len()])
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
    /// Sync signaling token (Settings > Sync).
    SyncSignalingToken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Dashboard,
    Terminal,
    Keys,
    Snippets,
    PortForwarding,
    /// Cloud-account CRUD. Promoted to a top-level vault surface
    /// (sub-nav pill / sidebar entry); the Cloud Sync settings block
    /// stays behind in Settings.
    Cloud,
    /// Proxy-identity CRUD. Promoted to a top-level vault surface.
    Proxies,
    /// Known-host management. Promoted back to a top-level vault
    /// surface alongside Cloud / Proxies (was a SettingsSection in
    /// v0.7).
    KnownHosts,
    History,
    Sftp,
    Settings,
    /// Multi-host monitor dashboard (issue #95): live vitals across
    /// every opted-in host. Not a sub-nav pill: entered through the
    /// Hosts toolbar's monitor icon, which only renders while the
    /// master `host_monitoring` toggle is on (optional-features rule).
    Monitoring,
    /// Network tools (DNS, ping, traceroute, port test, HTTP/TLS,
    /// WHOIS, DNSBL). A panel tab like Settings rather than a vault
    /// surface, and reachable only while `network_tools_enabled` is on.
    NetworkTools,
}

/// One row in the Plugins panel: a cloud-provider plugin and its
/// install / update state. Cloud providers ship as downloaded
/// subprocess plugins (see `crate::plugins`); this is the UI-side
/// view of one.
#[derive(Debug, Clone)]
pub struct PluginUiEntry {
    /// Provider id, matches `CloudProvider::id()` (`"aws"`, ...).
    pub provider_id: String,
    /// Human-readable name shown in the panel.
    pub display_name: String,
    /// Current install / update state.
    pub status: PluginUiStatus,
    /// Per-plugin auto-update override, resolved against the global
    /// default when the panel loads.
    pub auto_update: bool,
    /// User-pinned version. When set, the updater won't move off it.
    pub pinned_version: Option<String>,
    /// Downloaded binaries exist in the plugin cache (or, for MCP,
    /// the launcher copy). Lets a dev build still offer "remove
    /// downloaded files" for the cache it shadows.
    pub cached_install: bool,
    /// Last successfully fetched manifest. Drives the install modal's
    /// size / changelog. `None` until a check runs (and on every
    /// machine until the manifest host exists, see PR 6).
    pub manifest: Option<crate::plugins::PluginManifest>,
    /// Why the last manifest fetch failed, verbatim from the fetch.
    ///
    /// The row badge deliberately stays quiet about a failed check
    /// (see the `PluginManifestFetched` handler), but the install
    /// modal has to say something, and "could not reach the host" was
    /// a lie for every non-network cause: an over-ceiling body, an
    /// HTTP 403 from the API's unauthenticated rate limit, a release
    /// window that no longer carries the plugin's tag. Discussion
    /// #163 cost three round trips to a user whose network was fine,
    /// so the cause now travels to the surface that shows the error.
    pub manifest_error: Option<String>,
}

/// Install / update lifecycle state for a [`PluginUiEntry`].
#[derive(Debug, Clone, PartialEq)]
pub enum PluginUiStatus {
    /// No binary on disk and no dev build, the plugin must be
    /// downloaded before its provider can be used.
    NotInstalled,
    /// Running from a freshly-built `target/debug` binary (the dev
    /// loop). No version directory, no manifest involved.
    DevBuild,
    /// Installed from the cache at this version.
    Installed(String),
    /// Installed, and the manifest advertises a newer compatible
    /// version.
    UpdateAvailable { current: String, latest: String },
    /// A manifest fetch is in flight.
    Checking,
    /// A binary download + verify is in flight (indeterminate).
    Downloading,
    /// The last check / install failed; carries a user-facing message.
    Failed(String),
}

/// Cloud provider picked in the wizard. AWS authenticates via named
/// profile / access key / SSO; Kubernetes via a kubeconfig; GCP via the
/// already-authenticated `gcloud` CLI (scoped to an optional project);
/// Azure via the already-authenticated `az` CLI (scoped to an optional
/// subscription).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CloudProviderChoice {
    #[default]
    Aws,
    K8s,
    Gcp,
    Azure,
}

/// Which kind of `PodSelector` a K8s dynamic group's editor produces.
/// `Labels` takes a `k=v,k=v` string; the rest take a single resource
/// name (the resolver expands it to that workload's / pod's selector).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum K8sSelectorKind {
    #[default]
    Labels,
    Deployment,
    StatefulSet,
    Name,
}

impl K8sSelectorKind {
    pub const ALL: [K8sSelectorKind; 4] = [
        K8sSelectorKind::Labels,
        K8sSelectorKind::Deployment,
        K8sSelectorKind::StatefulSet,
        K8sSelectorKind::Name,
    ];
}

impl std::fmt::Display for K8sSelectorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            K8sSelectorKind::Labels => "Labels",
            K8sSelectorKind::Deployment => "Deployment",
            K8sSelectorKind::StatefulSet => "StatefulSet",
            K8sSelectorKind::Name => "Pod name",
        })
    }
}

impl CloudProviderChoice {
    pub fn id(self) -> &'static str {
        match self {
            Self::Aws => "aws",
            Self::K8s => "k8s",
            Self::Gcp => "gcp",
            Self::Azure => "azure",
        }
    }

    pub fn from_id(s: &str) -> Self {
        match s {
            "k8s" => Self::K8s,
            "gcp" => Self::Gcp,
            "azure" => Self::Azure,
            _ => Self::Aws,
        }
    }
}

/// Auth strategy chosen in the wizard. Only `Profile` is implemented in
/// v0.6 PR 3; the other variants render disabled with a hint and route
/// to `CloudError::Unsupported` if somehow selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CloudAuthChoice {
    #[default]
    Profile,
    AccessKey,
    Sso,
    Kubeconfig,
    /// GCP: the ambient `gcloud` login (`gcloud auth login`); no secret
    /// stored, just an optional project scope.
    GcloudCli,
    /// Azure: the ambient `az` login (`az login`); no secret stored, just
    /// an optional subscription scope.
    AzCli,
}

impl CloudAuthChoice {
    pub fn id(self) -> &'static str {
        match self {
            Self::Profile => "profile",
            Self::AccessKey => "access_key",
            Self::Sso => "sso",
            Self::Kubeconfig => "kubeconfig",
            Self::GcloudCli => "gcloud",
            Self::AzCli => "az",
        }
    }

    pub fn from_id(s: &str) -> Self {
        match s {
            "access_key" => Self::AccessKey,
            "sso" => Self::Sso,
            "kubeconfig" => Self::Kubeconfig,
            "gcloud" => Self::GcloudCli,
            "az" => Self::AzCli,
            _ => Self::Profile,
        }
    }
}

/// Live state of the "Test credentials" button in the wizard.
#[derive(Debug, Clone, Default)]
pub enum CloudTestState {
    #[default]
    Idle,
    Running,
    Ok,
    Failed(String),
}

/// State of the wizard's "Discover & pick" panel, owns the in-flight
/// or completed discovery result so the user can scroll/select without
/// re-hitting the cloud.
#[derive(Debug, Clone, Default)]
pub enum CloudDiscoverState {
    #[default]
    Idle,
    Running,
    Loaded(oryxis_cloud::DiscoveryResult),
    Failed(String),
}


/// Per-dynamic-group resolve state. Lives in a `HashMap<group_id, _>`
/// on `Oryxis` so opening one group doesn't blow away another's
/// cached resolve. TTL handling lives on the call site.
#[derive(Debug, Clone)]
pub enum DynamicGroupState {
    Loading,
    Loaded {
        hosts: Vec<oryxis_cloud::DiscoveredHost>,
        // When this list was fetched. `OpenGroup` compares against
        // `Utc::now()` and re-resolves past the cache TTL so a recycled
        // ECS task doesn't sit as a dead row until a manual Refresh.
        fetched_at: chrono::DateTime<chrono::Utc>,
    },
    Failed(String),
}

/// One mDNS-discovered peer the user could pair with. Lives in
/// `Oryxis.sync_discovered`, deduped by `device_id`, rebuilt as
/// `SyncEngineEvent::PeerDiscovered` arrives.
#[derive(Debug, Clone)]
pub(crate) struct DiscoveredPeerInfo {
    pub device_id: Uuid,
    pub device_name: String,
    pub addr: std::net::SocketAddr,
}

/// Which pairing sub-view the Sync settings panel is showing. The hosted
/// code and the join inputs live alongside this in
/// [`SyncPairingForm`](super::SyncPairingForm) on `Oryxis.sync_pairing`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SyncPairingState {
    /// Default: just the two "Host" / "Join" entry buttons.
    #[default]
    Idle,
    /// This device is hosting a code, waiting for a peer to join.
    Hosting,
    /// This device is entering another device's code + address.
    Joining,
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
    Sync,
    /// SSH agent server configuration: per-signature confirm, external
    /// key adds, the OpenSSH pipe alias (Windows) and the socket path +
    /// setup snippets. The enable toggle stays on the Features screen
    /// (like AI / SFTP / Sync); this section only appears while the
    /// agent is enabled.
    Agent,
    /// Cloud Sync preferences (auto-refresh interval, orphan
    /// auto-archive). The cloud *account* CRUD moved to the top-level
    /// `View::Cloud` surface; this section keeps only the sync knobs.
    Cloud,
    /// Cloud provider plugins management: install, update, uninstall
    /// the subprocess plugins each cloud provider runs as. Sits next
    /// to `Cloud` because every cloud account here needs a matching
    /// plugin to actually function.
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
            SettingsSection::Sync => "settings-sync-scroll",
            SettingsSection::Agent => "settings-agent-scroll",
            SettingsSection::Cloud => "settings-cloud-scroll",
            SettingsSection::Plugins => "settings-plugins-scroll",
            SettingsSection::Advanced => "settings-advanced-scroll",
            SettingsSection::About => "settings-about-scroll",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SftpConsoleLayout, SidebarPlacement, TabSurface, TerminalSidebarTab};

    #[test]
    fn the_surface_chip_cycles_through_every_surface() {
        use TabSurface::{Console, Files, Terminal};
        // One surface is not a switch, and the chip is not drawn.
        assert_eq!(TabSurface::next_in(&[Terminal], Terminal), None);
        assert_eq!(TabSurface::next_in(&[], Terminal), None);
        // Two surfaces are the historical toggle, both ways.
        assert_eq!(TabSurface::next_in(&[Terminal, Files], Terminal), Some(Files));
        assert_eq!(TabSurface::next_in(&[Terminal, Files], Files), Some(Terminal));
        // Three wrap, and every surface is reachable from every other
        // one by clicking: a cycle that stuck on two of the three is
        // the failure this is here for.
        let all = [Terminal, Console, Files];
        let mut seen = vec![Terminal];
        let mut at = Terminal;
        for _ in 0..2 {
            at = TabSurface::next_in(&all, at).expect("three surfaces cycle");
            seen.push(at);
        }
        assert_eq!(seen, vec![Terminal, Console, Files]);
        assert_eq!(TabSurface::next_in(&all, Files), Some(Terminal));
        // A surface the tab no longer has (a frame behind the state)
        // leaves the chip live rather than dead.
        assert_eq!(TabSurface::next_in(&[Terminal, Console], Files), Some(Console));
    }

    #[test]
    fn console_layout_code_roundtrips() {
        // The persisted `sftp_console_layout` setting resolves back
        // exactly; junk resolves to None so the caller falls back to
        // the default placement instead of a wrong one.
        for l in SftpConsoleLayout::ALL {
            assert_eq!(SftpConsoleLayout::from_code(l.code()), Some(l));
        }
        assert_eq!(SftpConsoleLayout::from_code(""), None);
        assert_eq!(SftpConsoleLayout::from_code("bogus"), None);
        // Full is a zoomed split, so it must still name an axis: the
        // restore has to land on a layout, not on nothing.
        assert!(SftpConsoleLayout::Full.starts_maximized());
        assert_eq!(
            SftpConsoleLayout::Full.axis(),
            SftpConsoleLayout::SplitBelow.axis(),
        );
        assert!(!SftpConsoleLayout::SplitBelow.starts_maximized());
    }

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
