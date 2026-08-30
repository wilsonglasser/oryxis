//! Settings dispatch helpers: advanced. Download mirror, debug
//! logging, renderer backend and relaunch. Split out of
//! dispatch_settings/mod.rs.

use super::*;

/// Dispatch `Message::ToastClear` after the standard 1.8s toast dwell,
/// same cadence as the copy-to-clipboard confirmation.
fn toast_clear_task() -> Task<Message> {
    Task::perform(
        async {
            tokio::time::sleep(std::time::Duration::from_millis(1800)).await;
        },
        |_| Message::ToastClear,
    )
}

impl Oryxis {
    /// Task that asks iced for the graphics backend the compositor
    /// actually selected, but only when a settings section that displays
    /// it (Interface, plus Advanced for the environment report) is
    /// showing and it hasn't loaded yet. By then the compositor exists,
    /// so the oneshot resolves instead of being dropped. Returns
    /// [`Task::none`] otherwise. Fired both when switching into the
    /// section and when opening Settings on it.
    pub(crate) fn renderer_info_task(&self) -> Task<Message> {
        if matches!(
            self.settings_section,
            crate::state::SettingsSection::Interface | crate::state::SettingsSection::Advanced
        ) && self.renderer_active.is_none()
        {
            iced::system::graphics_information()
                .map(|info| Message::Settings(SettingsMessage::RendererInfoLoaded(info.backend, info.adapter)))
        } else {
            Task::none()
        }
    }
}

impl Oryxis {
    /// Advanced arms: download mirror, debug logging, the renderer
    /// backend picker and app relaunch.
    pub(super) fn handle_settings_advanced(
        &mut self,
        message: SettingsMessage,
    ) -> Result<Task<Message>, SettingsMessage> {
        match message {
            SettingsMessage::RendererInfoLoaded(backend, adapter) => {
                self.renderer_active = Some((backend, adapter));
            }
            SettingsMessage::SettingRendererBackendChanged(mode) => {
                // No-op if the pick didn't change (re-selecting the same
                // option shouldn't nag about a restart).
                if mode == self.prefs.renderer_backend {
                    return Ok(Task::none());
                }
                self.prefs.renderer_backend = mode.clone();
                self.persist_setting("renderer_backend", &mode);
                // The backend is read once at process start, so the change
                // only takes effect on the next launch. Offer to restart
                // now (applies immediately) or later (applies on next open).
                self.error_dialog = Some(crate::state::ErrorDialog {
                    title: crate::i18n::t("renderer_restart_title").to_string(),
                    body: crate::i18n::t("renderer_restart_body").to_string(),
                    link: None,
                    action: Some(crate::state::ErrorDialogAction {
                        label: crate::i18n::t("renderer_restart_now").to_string(),
                        message: Box::new(Message::Settings(SettingsMessage::RelaunchApp)),
                        danger: false,
                    }),
                });
            }
            SettingsMessage::RelaunchApp => {
                // Spawns a fresh process and exits this one; never returns
                // on success. If the spawn fails it falls through and the
                // app keeps running.
                self.relaunch_self();
            }
            SettingsMessage::SettingToggleDebugLogging => {
                if crate::logging::is_forced() {
                    // --debug-log pinned the sink on. Say so instead of
                    // flipping a switch that `logging::disable` ignores,
                    // which would leave the row lying about the state.
                    self.set_toast(crate::i18n::t("debug_logging_forced").to_string());
                    return Ok(toast_clear_task());
                }
                if self.prefs.debug_logging {
                    // Emitted before the sink closes so the file records
                    // its own switch-off.
                    tracing::info!("debug logging disabled from Settings");
                    crate::logging::disable();
                    self.prefs.debug_logging = false;
                } else {
                    match crate::logging::enable() {
                        Ok(path) => {
                            self.prefs.debug_logging = true;
                            tracing::info!("debug logging enabled -> {}", path.display());
                        }
                        Err(e) => {
                            // Leave the toggle off and surface the cause;
                            // a silently dead sink would defeat the whole
                            // point of the feature.
                            tracing::warn!("failed to enable debug logging: {e}");
                            self.set_toast(format!("{}: {e}", crate::i18n::t("debug_logging")));
                            return Ok(toast_clear_task());
                        }
                    }
                }
                self.persist_setting(
                    "debug_logging",
                    if self.prefs.debug_logging { "true" } else { "false" },
                );
            }
            SettingsMessage::RevealDebugLog => {
                if let Some(path) = crate::logging::log_path() {
                    // Fall back to the data folder when nothing was
                    // written yet so the button never silently no-ops.
                    let result = if path.exists() {
                        crate::util::reveal_in_file_manager(&path, false)
                    } else if let Some(dir) = path.parent() {
                        crate::util::reveal_in_file_manager(dir, true)
                    } else {
                        Ok(())
                    };
                    if let Err(e) = result {
                        tracing::warn!("failed to reveal debug log: {e}");
                    }
                }
            }
            SettingsMessage::ClearDebugLog => {
                match crate::logging::clear() {
                    Ok(true) => {
                        self.set_toast(crate::i18n::t("debug_log_cleared").to_string());
                    }
                    Ok(false) => {
                        self.set_toast(crate::i18n::t("debug_log_missing").to_string());
                    }
                    Err(e) => {
                        tracing::warn!("failed to clear debug log: {e}");
                        self.set_toast(format!("{}: {e}", crate::i18n::t("debug_log_clear")));
                    }
                }
                return Ok(toast_clear_task());
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }
}
