//! Top-level UI modes (split out of `state.rs`).

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VaultState {
    Loading,
    NeedSetup,
    Locked,
    Unlocked,
}

/// Active tab inside the terminal-side panel. `Chat` is only reachable
/// when AI is enabled; the dispatch falls back to `Snippets` otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TerminalSidebarTab {
    #[default]
    Chat,
    Snippets,
}

/// Identifies a secret text field whose reveal/eye toggle is on. One
/// shared enum + a `HashSet` in app state instead of a bool per field,
/// so adding the eye to a new password input is a one-variant change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecretField {
    /// Inline proxy password in the host editor.
    ProxyPassword,
    /// Password on the Share (portable export) dialog.
    SharePassword,
    /// AI assistant API key (Settings > AI).
    AiApiKey,
    /// New master password (Settings > Security).
    VaultNewPassword,
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
/// profile / access key / SSO; Kubernetes via a kubeconfig.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CloudProviderChoice {
    #[default]
    Aws,
    K8s,
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
        }
    }

    pub fn from_id(s: &str) -> Self {
        match s {
            "k8s" => Self::K8s,
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
}

impl CloudAuthChoice {
    pub fn id(self) -> &'static str {
        match self {
            Self::Profile => "profile",
            Self::AccessKey => "access_key",
            Self::Sso => "sso",
            Self::Kubeconfig => "kubeconfig",
        }
    }

    pub fn from_id(s: &str) -> Self {
        match s {
            "access_key" => Self::AccessKey,
            "sso" => Self::Sso,
            "kubeconfig" => Self::Kubeconfig,
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

/// Which pairing sub-view the Sync settings panel is showing. The
/// hosted code itself lives in `Oryxis.sync_pairing_code`; the join
/// inputs live in `sync_join_code_input` / `sync_join_target_input`.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsSection {
    Terminal,
    /// SSH connection behaviour shared across hosts: keepalive
    /// interval, auto-reconnect, OS detection. Split out of the
    /// Terminal section, which had grown into a grab-bag of terminal
    /// display, connection and logging knobs.
    Connection,
    Sftp,
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
    /// Cloud Sync preferences (auto-refresh interval, orphan
    /// auto-archive). The cloud *account* CRUD moved to the top-level
    /// `View::Cloud` surface; this section keeps only the sync knobs.
    Cloud,
    /// Cloud provider plugins management: install, update, uninstall
    /// the subprocess plugins each cloud provider runs as. Sits next
    /// to `Cloud` because every cloud account here needs a matching
    /// plugin to actually function.
    Plugins,
    About,
}
