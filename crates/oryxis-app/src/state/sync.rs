//! Sync feature state: settings, live engine handles, and the two transient
//! sync forms (device pairing + the SFTP transport). Grouped off the `Oryxis`
//! god-struct as the deferred `SyncState` bag, part of the modules-by-feature
//! direction (field grouping only; the dispatch/view split is separate).

use tokio::sync::oneshot;

use oryxis_vault::SyncPeerRow;

use super::{DiscoveredPeerInfo, SftpSyncForm, SyncPairingForm};
use crate::sync_runtime::SyncRuntime;

/// All sync settings + runtime + transient form state. Settings hydrate from
/// the `settings` table on boot; the runtime handles (`runtime`, `abort_tx`)
/// live only while sync is active. Not `Clone` (holds a oneshot sender and the
/// live engine); the manual `Default` reproduces the boot-time defaults.
pub(crate) struct SyncState {
    /// Whether sync is enabled.
    pub(crate) enabled: bool,
    /// `"manual"` or `"auto"`.
    pub(crate) mode: String,
    /// When on, sync wraps connection / identity / proxy-identity payloads
    /// with their decrypted passwords so peers can mirror them. Off by
    /// default; passwords stay device-local until the user opts in.
    pub(crate) passwords: bool,
    /// This device's display name in the peer list.
    pub(crate) device_name: String,
    /// Signaling endpoint URL (empty == not set).
    pub(crate) signaling_url: String,
    /// Bearer token for the signaling endpoint. Empty == not configured.
    pub(crate) signaling_token: String,
    /// Relay endpoint URL (empty == not set).
    pub(crate) relay_url: String,
    /// Listen port as a string (`"0"` == ephemeral).
    pub(crate) listen_port: String,
    /// Paired peers loaded from the vault.
    pub(crate) peers: Vec<SyncPeerRow>,
    /// Last status line shown in the Sync panel.
    pub(crate) status: Option<String>,
    /// Live P2P sync engine, present only while sync is enabled. Holds a
    /// dedicated vault handle plus the QUIC / mDNS background tasks.
    pub(crate) runtime: Option<SyncRuntime>,
    /// Mirrors `runtime.is_some()` for cheap UI checks.
    pub(crate) engine_running: bool,
    /// Transient device-pairing UI (hosted code / link, join inputs, and which
    /// pairing sub-view the Sync panel shows).
    pub(crate) pairing: SyncPairingForm,
    /// Live mDNS-discovered peers on the LAN. Deduped by `device_id`.
    pub(crate) discovered: Vec<DiscoveredPeerInfo>,
    /// `Sync Now` in flight. Drives the Cancel button + suppresses re-clicks.
    pub(crate) in_progress: bool,
    /// One-shot abort channel for the in-flight `Sync Now` task. The task
    /// races `sync_now().await` against this receiver, so `Cancel` immediately
    /// drops the QUIC connection.
    pub(crate) abort_tx: Option<oneshot::Sender<()>>,
    /// Visible heartbeat counter for signaling re-registers. Bumps on every
    /// successful `SignalingRegistered` event so the user can confirm the
    /// heartbeat is alive.
    pub(crate) signaling_tick: u32,
    /// Sync transport: `"p2p"` (QUIC + mDNS + relay, the default) or `"sftp"`
    /// (reconcile against one encrypted snapshot file on an SFTP host). A
    /// device runs one transport at a time; the two don't bridge.
    pub(crate) transport: String,
    /// Transient state for the SFTP sync transport (snapshot host, remote
    /// path, group passphrase, host picker) plus the in-flight round's
    /// progress + last outcome.
    pub(crate) sftp: SftpSyncForm,
}

impl Default for SyncState {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: "manual".into(),
            passwords: false,
            device_name: String::new(),
            // The engine config exposes `signaling_url` / `signaling_token`
            // as `Option<String>`; the app state uses a plain `String`
            // (empty == not set) so a Settings text input can drive it.
            signaling_url: oryxis_sync::SyncConfig::default()
                .signaling_url
                .unwrap_or_default(),
            signaling_token: oryxis_sync::SyncConfig::default()
                .signaling_token
                .unwrap_or_default(),
            relay_url: String::new(),
            listen_port: "0".into(),
            peers: Vec::new(),
            status: None,
            runtime: None,
            engine_running: false,
            pairing: SyncPairingForm::default(),
            discovered: Vec::new(),
            in_progress: false,
            abort_tx: None,
            signaling_tick: 0,
            transport: "p2p".into(),
            sftp: SftpSyncForm::default(),
        }
    }
}
