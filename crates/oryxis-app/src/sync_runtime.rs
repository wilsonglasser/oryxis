//! `SyncRuntime`: owns the live `SyncEngine` while P2P sync is enabled.
//!
//! The engine needs an `Arc<Mutex<VaultStore>>`, but `Oryxis` holds its
//! vault as a plain `Option<VaultStore>` (one owner, ~160 call sites).
//! Rather than refactor every one, the runtime opens its OWN
//! `VaultStore` handle on the same database file. SQLite WAL mode makes
//! concurrent handles safe; the `busy_timeout` set in `VaultStore::open`
//! covers the rare two-writer overlap.
//!
//! Known v1 limitation: rotating the vault master password leaves this
//! handle's derived key stale, so the engine has to be restarted
//! (toggle sync off then on). The persisted `DeviceIdentity` blob
//! itself survives rotation via `re_encrypt_sync_device_identity`.

use std::sync::{Arc, Mutex};

use iced::Task;
use tokio::sync::mpsc;

use oryxis_sync::crypto::DeviceIdentity;
use oryxis_sync::{SyncConfig, SyncEngine, SyncError, SyncEvent, SyncHandle, SyncMode};
use oryxis_vault::VaultStore;

use crate::app::{Message, Oryxis};

/// Live sync engine owned by `Oryxis` while sync is enabled.
pub(crate) struct SyncRuntime {
    engine: SyncEngine,
    handle: SyncHandle,
}

impl SyncRuntime {
    /// Open a dedicated vault handle, build the engine, start its
    /// background tasks, and hand back the event receiver so the caller
    /// can pump it into a `Task::stream`.
    ///
    /// `master_password` is `Some` when the vault has a user password
    /// (we unlock the second handle with it) and `None` when the vault
    /// auto-opens without one (mirrors the boot path in `boot.rs`).
    pub(crate) fn spawn(
        config: SyncConfig,
        device_name: &str,
        db_path: &std::path::Path,
        master_password: Option<&str>,
    ) -> Result<(Self, mpsc::UnboundedReceiver<SyncEvent>), SyncError> {
        // Dedicated handle on the same SQLite file as the app's vault.
        let mut vault = VaultStore::open(db_path)
            .map_err(|e| SyncError::Vault(format!("open sync vault handle: {e}")))?;
        match master_password {
            Some(pw) => vault
                .unlock(pw)
                .map_err(|e| SyncError::Vault(format!("unlock sync vault handle: {e}")))?,
            None => vault
                .open_without_password()
                .map_err(|e| SyncError::Vault(format!("open sync vault handle: {e}")))?,
        }

        // Persistent device identity, generated + stored on first run.
        let identity = DeviceIdentity::load_or_generate(&vault, device_name)?;

        let vault = Arc::new(Mutex::new(vault));
        let mut engine = SyncEngine::new(config, identity, vault);
        let event_rx = engine
            .take_events()
            .expect("a freshly created engine always has its event receiver");
        engine.start()?;
        let handle = engine.handle();

        Ok((Self { engine, handle }, event_rx))
    }

    /// A cloneable handle for triggering a manual sync off-thread.
    pub(crate) fn handle(&self) -> SyncHandle {
        self.handle.clone()
    }

    /// Stop all background tasks. Idempotent.
    pub(crate) fn stop(&mut self) {
        self.engine.stop();
    }
}

impl Drop for SyncRuntime {
    fn drop(&mut self) {
        // Belt-and-braces: an explicit `stop_sync_engine` is the normal
        // path, but a dropped runtime must never leave the QUIC socket
        // and background tasks dangling.
        self.engine.stop();
    }
}

/// Render a pairing link as a PNG-encoded QR code. Returns `None` on
/// either side of the pipeline failing (too much data, encoder error);
/// the caller falls back to showing just the text link.
pub(crate) fn render_pairing_qr(text: &str) -> Option<Vec<u8>> {
    let code = qrcode::QrCode::new(text.as_bytes()).ok()?;
    let img = code
        .render::<image::Luma<u8>>()
        .min_dimensions(220, 220)
        .max_dimensions(280, 280)
        .quiet_zone(true)
        .build();
    let mut png: Vec<u8> = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut png),
        image::ImageFormat::Png,
    )
    .ok()?;
    Some(png)
}

impl Oryxis {
    /// Build a `SyncConfig` from the current in-memory sync settings.
    fn build_sync_config(&self) -> SyncConfig {
        let mut config = SyncConfig {
            enabled: true,
            mode: if self.sync_mode == "auto" {
                SyncMode::Auto
            } else {
                SyncMode::Manual
            },
            relay_url: if self.sync_relay_url.trim().is_empty() {
                None
            } else {
                Some(self.sync_relay_url.clone())
            },
            listen_port: self.sync_listen_port.trim().parse().unwrap_or(0),
            auto_interval_secs: 300,
            ..SyncConfig::default()
        };
        // A signaling URL typed in Settings overrides the build-time
        // default; an explicit empty string switches the engine back
        // to LAN-only (`None`).
        let typed = self.sync_signaling_url.trim();
        if !typed.is_empty() {
            config.signaling_url = Some(typed.to_string());
        } else {
            config.signaling_url = None;
        }
        config
    }

    /// Spawn the sync engine from current settings and return a `Task`
    /// that pumps its event stream into `Message::SyncEngineEvent`.
    /// No-op (`Task::none`) if the engine is already running or the
    /// vault isn't available.
    pub(crate) fn start_sync_engine(&mut self) -> Task<Message> {
        if self.sync_runtime.is_some() {
            return Task::none();
        }
        let Some(vault) = &self.vault else {
            return Task::none();
        };
        let db_path = vault.db_path().to_path_buf();
        let config = self.build_sync_config();
        let device_name = if self.sync_device_name.trim().is_empty() {
            "oryxis-device".to_string()
        } else {
            self.sync_device_name.clone()
        };
        let master_password = self.master_password.clone();

        match SyncRuntime::spawn(config, &device_name, &db_path, master_password.as_deref()) {
            Ok((runtime, event_rx)) => {
                self.sync_runtime = Some(runtime);
                self.sync_engine_running = true;
                self.sync_status = Some(crate::i18n::t("sync_status_running").to_string());
                let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(event_rx);
                Task::stream(stream).map(Message::SyncEngineEvent)
            }
            Err(e) => {
                self.sync_engine_running = false;
                self.sync_status =
                    Some(format!("{}: {e}", crate::i18n::t("sync_status_failed")));
                tracing::warn!("sync engine failed to start: {e}");
                Task::none()
            }
        }
    }

    /// Stop the sync engine if it is running. Idempotent.
    pub(crate) fn stop_sync_engine(&mut self) {
        if let Some(mut runtime) = self.sync_runtime.take() {
            runtime.stop();
        }
        self.sync_engine_running = false;
    }
}
