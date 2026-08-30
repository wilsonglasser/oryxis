//! `Oryxis::handle_plugins`, the Plugins panel dispatch: review and
//! uninstall of locally installed plugin binaries.
//!
//! This module owns the UI-side lifecycle: it keeps the per-provider
//! rows (`app.plugins`) in sync with what's on disk and drives removal.
//! The app performs no network fetches here: plugins are whatever the
//! local cache / dev build provides.

#![allow(clippy::result_large_err)]

use iced::Task;

use crate::app::{PluginMessage, Message, Oryxis};
use crate::plugins::cache;
use crate::state::{PluginUiEntry, PluginUiStatus};

/// Providers the app knows how to surface in the Plugins panel.
/// `(provider_id, display_name)`. MCP is a plugin from a distribution
/// standpoint (download / verify / cache) but the binary is spawned
/// by external clients (Claude Desktop, Code), not the app,
/// see [`crate::mcp_install`].
const KNOWN_PLUGINS: &[(&str, &str)] = &[
    ("mcp", "Oryxis MCP Server"),
    // Distribution-only like MCP, but spawned by the app itself: the
    // History screen's "Export GIF" renders a recording through it
    // (see `crate::gif_export`). No JSON-RPC protocol, just a CLI.
    ("gif", "GIF Export (agg)"),
];

/// Build the initial `PluginUiEntry` rows from the on-disk cache.
/// Called once from `boot::load_data_from_vault`.
pub(crate) fn load_plugin_entries() -> Vec<PluginUiEntry> {
    KNOWN_PLUGINS
        .iter()
        .map(|&(provider_id, display_name)| {
            PluginUiEntry {
                provider_id: provider_id.to_string(),
                display_name: display_name.to_string(),
                status: detect_status(provider_id),
                cached_install: cached_install_present(provider_id),
            }
        })
        .collect()
}

/// True when the plugin cache holds downloaded files for this
/// provider (any cached version, or the MCP launcher copy). Drives
/// the remove action even when a dev binary shadows the cache.
pub(crate) fn cached_install_present(provider_id: &str) -> bool {
    let cached = cache::installed_versions(provider_id)
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    if cached {
        return true;
    }
    provider_id == "mcp" && crate::mcp_install::is_installed()
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
/// executable. Debug builds only.
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
    /// True when `provider_id`'s plugin is present on disk in a usable
    /// state (`DevBuild` / `Installed`).
    pub(crate) fn plugin_installed(&self, provider_id: &str) -> bool {
        self.plugins
            .iter()
            .find(|p| p.provider_id == provider_id)
            .is_some_and(|e| {
                matches!(
                    e.status,
                    PluginUiStatus::DevBuild | PluginUiStatus::Installed(_)
                )
            })
    }

    pub(crate) fn handle_plugins(
        &mut self,
        message: PluginMessage,
    ) -> Task<Message> {
        match message {
            PluginMessage::ShowPluginMenu(id) => {
                use crate::state::{OverlayContent, OverlayState};
                // Toggle, mirroring the other card kebabs.
                let already = matches!(
                    self.overlay.as_ref().map(|o| &o.content),
                    Some(OverlayContent::PluginActions(i)) if *i == id
                );
                if already {
                    self.overlay = None;
                } else {
                    let anchor = self.keynav_take_menu_anchor();
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::PluginActions(id),
                        x: anchor.0,
                        y: anchor.1,
                    });
                }
                Task::none()
            }

            PluginMessage::PluginUninstall(id) => {
                // Reached from the row kebab: the confirm dialog takes
                // over, the menu must not linger under it.
                self.overlay = None;
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
                        message: Box::new(Message::Plugin(PluginMessage::PluginUninstallConfirmed(id))),
                        danger: true,
                    }),
                });
                Task::none()
            }
            PluginMessage::PluginUninstallConfirmed(id) => {
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
                    self.mcp.server_enabled = false;
                    if let Some(vault) = &self.vault {
                        let _ = vault.set_setting("mcp_server_enabled", "false");
                    }
                }
                if let Some(entry) =
                    self.plugins.iter_mut().find(|p| p.provider_id == id)
                {
                    entry.status = detect_status(&id);
                    entry.cached_install = cached_install_present(&id);
                }
                Task::none()
            }
        }
    }
}
