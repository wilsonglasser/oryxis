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
/// `(provider_id, display_name)`. AWS runs as a subprocess spawned
/// by the app via `PluginProvider`; MCP is also a plugin from a
/// distribution standpoint (download / verify / cache) but the
/// binary is spawned by external clients (Claude Desktop, Code),
/// not the app, see [`crate::mcp_install`].
const KNOWN_PLUGINS: &[(&str, &str)] = &[
    ("aws", "Amazon Web Services"),
    ("k8s", "Kubernetes"),
    ("mcp", "Oryxis MCP Server"),
];

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
pub(crate) fn dev_binary_present(provider_id: &str) -> bool {
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
    /// True when the provider's plugin is in a state that can answer
    /// trait calls right now (`DevBuild` / `Installed` /
    /// `UpdateAvailable`). Providers that don't need a subprocess
    /// plugin (Kubernetes is in-process today) report `true`
    /// unconditionally.
    ///
    /// Drives the "AWS plugin not installed" banner + the
    /// Test-Credentials gate in the Cloud Accounts wizard.
    pub(crate) fn is_plugin_ready(
        &self,
        choice: crate::state::CloudProviderChoice,
    ) -> bool {
        let id = match choice {
            crate::state::CloudProviderChoice::Aws => "aws",
            crate::state::CloudProviderChoice::K8s => return true,
        };
        self.plugins
            .iter()
            .find(|p| p.provider_id == id)
            .is_some_and(|e| {
                matches!(
                    e.status,
                    PluginUiStatus::DevBuild
                        | PluginUiStatus::Installed(_)
                        | PluginUiStatus::UpdateAvailable { .. }
                )
            })
    }

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
                let id_for_task = id.clone();
                let id_for_msg = id.clone();
                Ok(Task::perform(
                    async move {
                        crate::plugins::download::fetch_manifest(&id_for_task)
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
                            // dev build or an installed version. For
                            // a `NotInstalled` provider we also keep
                            // the status: a failed manifest fetch is
                            // typically a transient network blip (no
                            // release yet, GitHub API throttled,
                            // offline) and the install modal that
                            // triggered the fetch already surfaces
                            // "Download size unavailable", no need to
                            // also escalate the row badge to error.
                            let _ = msg;
                            entry.status = detect_status(&id);
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
                        let id_log = id_for_task.clone();
                        crate::plugins::download::download_and_install(
                            &id_for_task,
                            &best,
                            |_, _| {},
                        )
                        .await
                        .map(|_| best.version.clone())
                        .map_err(|e| {
                            // Detailed error goes to the log file so
                            // we can debug crashes without polluting
                            // the UI with raw PluginError::Display
                            // text (sha mismatch hashes, HTTP codes,
                            // file paths). The UI gets the stable
                            // i18n key only.
                            tracing::warn!(
                                target = "oryxis::plugins",
                                provider = %id_log,
                                error = %e,
                                "plugin install failed"
                            );
                            e.i18n_key().to_string()
                        })
                    },
                    move |result| Message::PluginInstallDone(id.clone(), result),
                ))
            }

            Message::PluginInstallDone(id, result) => {
                let token = self.mcp_server_token.clone();
                let rebind_provider = self.plugin_providers.get(&id).cloned();
                if let Some(entry) =
                    self.plugins.iter_mut().find(|p| p.provider_id == id)
                {
                    entry.status = match result {
                        Ok(version) => {
                            // `download_and_install` wrote the binary
                            // but left `current` untouched, flip it now.
                            match cache::set_current(&id, &version) {
                                Ok(()) => {
                                    // MCP needs the stable launcher
                                    // (`~/.oryxis/bin/oryxis-mcp`)
                                    // refreshed so external clients
                                    // keep finding the right binary
                                    // across version bumps.
                                    if id == "mcp" {
                                        crate::mcp_install::post_install_refresh(
                                            &token,
                                        );
                                    }
                                    PluginUiStatus::Installed(version)
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        target = "oryxis::plugins",
                                        provider = %id,
                                        error = %e,
                                        "post-install set_current failed"
                                    );
                                    PluginUiStatus::Failed(
                                        crate::i18n::t("plugin_err_io").to_string(),
                                    )
                                }
                            }
                        }
                        Err(key) => {
                            PluginUiStatus::Failed(crate::i18n::t(&key).to_string())
                        }
                    };
                }
                // Repoint the live PluginProvider at the new binary
                // and tear down any in-flight subprocess so the next
                // call respawns from the freshly-installed version.
                // Without this the host would keep its frozen-at-boot
                // PathBuf and respawn the previous version (or fail
                // with BinaryNotFound if pruning removed it).
                let task = if let Some(provider) = rebind_provider {
                    Task::perform(
                        async move { provider.rebind().await },
                        |()| Message::NoOp,
                    )
                } else {
                    Task::none()
                };
                Ok(task)
            }

            Message::PluginUninstall(id) => {
                // Destructive: route through a confirmation dialog whose
                // primary action carries the real removal message.
                let display = self
                    .plugins
                    .iter()
                    .find(|p| p.provider_id == id)
                    .map(|p| p.display_name.clone())
                    .unwrap_or_else(|| id.clone());
                self.error_dialog = Some(crate::state::ErrorDialog {
                    title: crate::i18n::t("plugin_uninstall_confirm_title").to_string(),
                    body: format!(
                        "{display}: {}",
                        crate::i18n::t("plugin_uninstall_confirm_body")
                    ),
                    link: None,
                    action: Some(crate::state::ErrorDialogAction {
                        label: crate::i18n::t("plugin_action_uninstall").to_string(),
                        message: Box::new(Message::PluginUninstallConfirmed(id)),
                    }),
                });
                Ok(Task::none())
            }
            Message::PluginUninstallConfirmed(id) => {
                if let Ok(dir) = cache::provider_dir(&id) {
                    let _ = std::fs::remove_dir_all(&dir);
                }
                // The MCP plugin also keeps a stable launcher copy in
                // ~/.oryxis/bin that external clients spawn; removing
                // the plugin must remove it too (Windows fallback: a
                // held-open exe is renamed aside and swept next boot).
                if id == "mcp" {
                    if let Ok(launcher) = crate::mcp_install::launcher_path()
                        && launcher.exists()
                        && std::fs::remove_file(&launcher).is_err()
                    {
                        let _ = std::fs::rename(
                            &launcher,
                            launcher.with_extension("old.exe"),
                        );
                    }
                    // A removed server shouldn't stay toggled on.
                    self.mcp_server_enabled = false;
                    if let Some(vault) = &self.vault {
                        let _ = vault.set_setting("mcp_server_enabled", "false");
                    }
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
