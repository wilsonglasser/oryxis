//! `Oryxis::handle_plugins`, the Plugins panel dispatch: install,
//! update-check, uninstall, and the auto-update toggles.
//!
//! Cloud providers run as downloaded subprocess plugins (see
//! `crate::plugins`). This module owns the UI-side lifecycle: it
//! drives manifest fetches and downloads through `Task::perform`,
//! persists the auto-update settings, and keeps the per-provider
//! rows (`app.plugins`) in sync with what's on disk.

#![allow(clippy::result_large_err)]

use iced::Task;

use oryxis_vault::VaultStore;

use crate::app::{Message, Oryxis};
use crate::plugins::cache;
use crate::state::{PluginUiEntry, PluginUiStatus};

/// Providers the app knows how to surface in the Plugins panel.
/// `(provider_id, display_name)`. Today just AWS; gcp / azure / k8s
/// append here as their plugins land.
const KNOWN_PLUGINS: &[(&str, &str)] = &[("aws", "Amazon Web Services")];

/// Build the initial `PluginUiEntry` rows from the on-disk cache plus
/// the per-plugin settings. Called once from `boot::load_data_from_vault`.
pub(crate) fn load_plugin_entries(
    vault: &VaultStore,
    global_auto: bool,
) -> Vec<PluginUiEntry> {
    KNOWN_PLUGINS
        .iter()
        .map(|&(provider_id, display_name)| {
            let auto_update = vault
                .get_setting(&format!("plugins_{provider_id}_auto_update"))
                .ok()
                .flatten()
                .map(|s| s != "false")
                .unwrap_or(global_auto);
            let pinned_version = vault
                .get_setting(&format!("plugins_{provider_id}_pinned_version"))
                .ok()
                .flatten()
                .filter(|s| !s.is_empty());
            PluginUiEntry {
                provider_id: provider_id.to_string(),
                display_name: display_name.to_string(),
                status: detect_status(provider_id),
                auto_update,
                pinned_version,
                manifest: None,
            }
        })
        .collect()
}

/// Resolve a provider's install status from disk: a freshly-built
/// `target/debug` binary wins (the dev loop), otherwise the active
/// cached version, otherwise not installed.
fn detect_status(provider_id: &str) -> PluginUiStatus {
    if dev_binary_present(provider_id) {
        return PluginUiStatus::DevBuild;
    }
    match cache::current_binary(provider_id) {
        Ok(Some(_)) => match cache::current_version(provider_id) {
            Ok(Some(v)) => PluginUiStatus::Installed(v),
            _ => PluginUiStatus::NotInstalled,
        },
        _ => PluginUiStatus::NotInstalled,
    }
}

/// True when a freshly-built plugin binary sits next to the app
/// executable. Debug builds only, matches `PluginProvider::resolve_binary`.
fn dev_binary_present(provider_id: &str) -> bool {
    #[cfg(debug_assertions)]
    {
        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            return dir.join(cache::binary_name(provider_id)).exists();
        }
    }
    let _ = provider_id;
    false
}

impl Oryxis {
    pub(crate) fn handle_plugins(
        &mut self,
        message: Message,
    ) -> Result<Task<Message>, Message> {
        match message {
            Message::PluginToggleGlobalAutoUpdate(on) => {
                self.plugins_auto_update_global = on;
                self.persist_setting(
                    "plugins_auto_update_global",
                    if on { "true" } else { "false" },
                );
                // Rows without an explicit per-plugin override follow
                // the global default.
                if let Some(vault) = &self.vault {
                    for entry in &mut self.plugins {
                        let has_override = vault
                            .get_setting(&format!(
                                "plugins_{}_auto_update",
                                entry.provider_id
                            ))
                            .ok()
                            .flatten()
                            .is_some();
                        if !has_override {
                            entry.auto_update = on;
                        }
                    }
                }
                Ok(Task::none())
            }

            Message::PluginToggleAutoUpdate(id, on) => {
                if let Some(entry) =
                    self.plugins.iter_mut().find(|p| p.provider_id == id)
                {
                    entry.auto_update = on;
                }
                self.persist_setting(
                    &format!("plugins_{id}_auto_update"),
                    if on { "true" } else { "false" },
                );
                Ok(Task::none())
            }

            Message::PluginCheckUpdates(id) => {
                if let Some(entry) =
                    self.plugins.iter_mut().find(|p| p.provider_id == id)
                {
                    entry.status = PluginUiStatus::Checking;
                }
                let url = crate::plugins::manifest_url(&id);
                let id_for_msg = id.clone();
                Ok(Task::perform(
                    async move {
                        crate::plugins::download::fetch_manifest(&url)
                            .await
                            .map(Box::new)
                            .map_err(|e| e.to_string())
                    },
                    move |result| {
                        Message::PluginManifestFetched(id_for_msg.clone(), result)
                    },
                ))
            }

            Message::PluginManifestFetched(id, result) => {
                if let Some(entry) =
                    self.plugins.iter_mut().find(|p| p.provider_id == id)
                {
                    match result {
                        Ok(manifest) => {
                            // Highest version this app build can run.
                            let latest = manifest
                                .best(
                                    env!("CARGO_PKG_VERSION"),
                                    oryxis_plugin_protocol::SUPPORTED_PROTOCOL_VERSIONS,
                                )
                                .map(|m| m.version.clone());
                            entry.manifest = Some(*manifest);
                            // Re-derive the base status from disk (the
                            // `Checking` placeholder discarded it), then
                            // layer "update available" on top.
                            let base = detect_status(&id);
                            entry.status = match (&base, latest) {
                                (PluginUiStatus::Installed(current), Some(latest))
                                    if cache_version_newer(&latest, current) =>
                                {
                                    PluginUiStatus::UpdateAvailable {
                                        current: current.clone(),
                                        latest,
                                    }
                                }
                                _ => base,
                            };
                        }
                        Err(msg) => {
                            // A failed fetch must not mask a working
                            // dev build or an installed version.
                            let base = detect_status(&id);
                            entry.status =
                                if matches!(base, PluginUiStatus::NotInstalled) {
                                    PluginUiStatus::Failed(msg)
                                } else {
                                    base
                                };
                        }
                    }
                }
                Ok(Task::none())
            }

            Message::ShowPluginInstallModal(id) => {
                self.plugin_install_modal = Some(id.clone());
                // Fetch the manifest so the modal can show download
                // size + changelog, unless one is already cached.
                let needs_fetch = self
                    .plugins
                    .iter()
                    .find(|p| p.provider_id == id)
                    .map(|p| p.manifest.is_none())
                    .unwrap_or(false);
                if needs_fetch {
                    self.handle_plugins(Message::PluginCheckUpdates(id))
                } else {
                    Ok(Task::none())
                }
            }

            Message::HidePluginInstallModal => {
                self.plugin_install_modal = None;
                Ok(Task::none())
            }

            Message::PluginInstall(id) => {
                // Installing needs a manifest entry to download.
                let best = self
                    .plugins
                    .iter()
                    .find(|p| p.provider_id == id)
                    .and_then(|p| p.manifest.as_ref())
                    .and_then(|m| {
                        m.best(
                            env!("CARGO_PKG_VERSION"),
                            oryxis_plugin_protocol::SUPPORTED_PROTOCOL_VERSIONS,
                        )
                        .cloned()
                    });
                let Some(best) = best else {
                    if let Some(entry) =
                        self.plugins.iter_mut().find(|p| p.provider_id == id)
                    {
                        entry.status = PluginUiStatus::Failed(
                            crate::i18n::t("plugin_err_no_manifest").to_string(),
                        );
                    }
                    self.plugin_install_modal = None;
                    return Ok(Task::none());
                };
                self.plugin_install_modal = None;
                if let Some(entry) =
                    self.plugins.iter_mut().find(|p| p.provider_id == id)
                {
                    entry.status = PluginUiStatus::Downloading;
                }
                let id_for_task = id.clone();
                Ok(Task::perform(
                    async move {
                        // Progress is a no-op for now, the panel shows
                        // an indeterminate "downloading" state.
                        crate::plugins::download::download_and_install(
                            &id_for_task,
                            &best,
                            |_, _| {},
                        )
                        .await
                        .map(|_| best.version.clone())
                        .map_err(|e| e.to_string())
                    },
                    move |result| Message::PluginInstallDone(id.clone(), result),
                ))
            }

            Message::PluginInstallDone(id, result) => {
                if let Some(entry) =
                    self.plugins.iter_mut().find(|p| p.provider_id == id)
                {
                    entry.status = match result {
                        Ok(version) => {
                            // `download_and_install` wrote the binary
                            // but left `current` untouched, flip it now.
                            match cache::set_current(&id, &version) {
                                Ok(()) => PluginUiStatus::Installed(version),
                                Err(e) => PluginUiStatus::Failed(e.to_string()),
                            }
                        }
                        Err(msg) => PluginUiStatus::Failed(msg),
                    };
                }
                Ok(Task::none())
            }

            Message::PluginUninstall(id) => {
                if let Ok(dir) = cache::provider_dir(&id) {
                    let _ = std::fs::remove_dir_all(&dir);
                }
                if let Some(entry) =
                    self.plugins.iter_mut().find(|p| p.provider_id == id)
                {
                    entry.status = detect_status(&id);
                    entry.manifest = None;
                }
                Ok(Task::none())
            }

            other => Err(other),
        }
    }
}

/// `true` when `candidate` is a strictly newer version string than
/// `current`. Reuses the manifest crate's tolerant version parser.
fn cache_version_newer(candidate: &str, current: &str) -> bool {
    crate::plugins::manifest::version_key(candidate)
        > crate::plugins::manifest::version_key(current)
}
