//! `Oryxis::handle_update`: match arms for the auto-update machinery
//! (check settings + channel, manual/boot checks, download + install),
//! split out of dispatch_ssh.rs. Returns `Err(message)` for anything
//! it doesn't claim so the try_handler! chain falls through.

// Domain handlers return `Err(Message)` to pass an unclaimed message
// back up the chain. The Message enum is large (~200 bytes) but
// boxing it would force every handler-call site to allocate; the
// pattern is intentional, allow the lint.
#![allow(clippy::result_large_err)]

use iced::Task;

use crate::app::{UpdateMessage, Message, Oryxis};
use crate::util::open_in_browser;

impl Oryxis {
    pub(crate) fn handle_update(
        &mut self,
        message: UpdateMessage,
    ) -> Task<Message> {
        // Inside an MSIX package the Store services the app: WindowsApps
        // is read-only, so running our installer would only produce a
        // second, unpackaged copy. Settings > About hides the whole
        // update panel there, but the boot check fires without any UI, so
        // the check / download / install arms are refused here too. The
        // settings-mutation arms stay reachable (harmless, and they keep
        // the persisted preferences intact for a later unpackaged build).
        if crate::packaged::is_packaged()
            && matches!(
                message,
                UpdateMessage::CheckForUpdate
                    | UpdateMessage::CheckForUpdateManual
                    | UpdateMessage::UpdateCheckResult(_)
                    | UpdateMessage::UpdateStartDownload
                    | UpdateMessage::UpdateDownloadProgress(_)
                    | UpdateMessage::UpdateDownloadComplete(_)
            )
        {
            return Task::none();
        }
        match message {
            UpdateMessage::SettingToggleAutoCheckUpdates => {
                self.prefs.auto_check_updates = !self.prefs.auto_check_updates;
                self.persist_setting(
                    "auto_check_updates",
                    if self.prefs.auto_check_updates { "true" } else { "false" },
                );
            }
            UpdateMessage::SettingUpdateChannelChanged(channel) => {
                self.prefs.update_channel = channel;
                self.persist_setting("update_channel", channel.as_setting());
                // A channel switch invalidates any "skip this version" so
                // the user is offered the other stream's build right away.
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("skipped_update_version", "");
                }
                // Switching channel is an explicit intent to follow that
                // stream, so re-check immediately (surfacing the same
                // "Checking…" status + toast as a manual check) instead of
                // waiting for the next boot check.
                self.update_error = None;
                self.update_check_status = Some(crate::update::UpdateStatus::Checking);
                self.set_toast(crate::i18n::t("update_check_checking").to_string());
                return Task::perform(
                    crate::update::check_latest_release(channel),
                    |res| match res {
                        Ok(info) => Message::Update(UpdateMessage::UpdateCheckResult(info)),
                        Err(e) => Message::Update(UpdateMessage::UpdateCheckFailed(e.to_string())),
                    },
                );
            }
            UpdateMessage::CheckForUpdate => {
                if !self.prefs.auto_check_updates {
                    return Task::none();
                }
                // Also respect a persisted "skip this version" so we never
                // nag about the same tag twice.
                let skipped = self
                    .vault
                    .as_ref()
                    .and_then(|v| v.get_setting("skipped_update_version").ok().flatten());
                return Task::perform(
                    crate::update::check_latest_release(self.prefs.update_channel),
                    move |res| {
                        match res {
                            Ok(Some(info)) if Some(&info.version) != skipped.as_ref() => {
                                Message::Update(UpdateMessage::UpdateCheckResult(Some(info)))
                            }
                            // Boot check is best-effort: log the failure
                            // but never surface it in the UI.
                            Err(e) => {
                                tracing::warn!("update check failed: {e}");
                                Message::Update(UpdateMessage::UpdateCheckResult(None))
                            }
                            _ => Message::Update(UpdateMessage::UpdateCheckResult(None)),
                        }
                    },
                );
            }
            UpdateMessage::CheckForUpdateManual => {
                // Manual trigger from the settings button OR the burger
                // menu. Navigate to Settings > About so the result
                // (up-to-date / error + retry) is on screen regardless
                // of where the check was fired from (issue #38: the
                // burger-menu path previously looked like a no-op).
                self.panels.burger_menu = false;
                self.editing_hotkey = None;
                self.active_view = crate::state::View::Settings;
                self.settings_section = crate::state::SettingsSection::About;
                self.active_tab = None;
                // Sets the view directly rather than going through
                // ChangeView, so it has to mint the strip entry itself or
                // Settings would show with no chip (issue #120).
                self.ensure_panel_tab(crate::state::PanelKind::Settings);
                self.update_error = None;
                self.update_check_status = Some(crate::update::UpdateStatus::Checking);
                self.set_toast(crate::i18n::t("update_check_checking").to_string());
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("skipped_update_version", "");
                }
                return Task::perform(
                    crate::update::check_latest_release(self.prefs.update_channel),
                    |res| match res {
                        Ok(info) => Message::Update(UpdateMessage::UpdateCheckResult(info)),
                        Err(e) => Message::Update(UpdateMessage::UpdateCheckFailed(e.to_string())),
                    },
                );
            }
            UpdateMessage::UpdateCheckResult(info) => {
                match info {
                    Some(i) => {
                        // Surface the new version as a toast too so a
                        // burger-menu-triggered check confirms the
                        // result even before the update modal renders.
                        self.set_toast(format!(
                            "{} {}",
                            crate::i18n::t("update_check_available"),
                            i.version,
                        ));
                        self.pending_update = Some(i);
                        self.update_check_status = None;
                    }
                    None => {
                        // Only surface the "up to date" message if a manual
                        // check is in flight (status was set to Checking).
                        // A silent boot check that finds nothing should not
                        // change the settings UI.
                        if self.update_check_status.is_some() {
                            self.update_check_status =
                                Some(crate::update::UpdateStatus::UpToDate);
                            self.set_toast(format!(
                                "{} ({})",
                                crate::i18n::t("update_check_up_to_date"),
                                env!("CARGO_PKG_VERSION"),
                            ));
                        }
                    }
                }
                // Auto-dismiss the toast after the standard 1.8 s
                // window matches the existing "copied to clipboard"
                // toast cadence so users get consistent feedback timing.
                return Task::perform(
                    async {
                        tokio::time::sleep(std::time::Duration::from_millis(2_500)).await;
                    },
                    |_| Message::ToastClear,
                );
            }
            UpdateMessage::UpdateCheckFailed(cause) => {
                // Same gating as the up-to-date arm: only a manual check
                // (status in flight) reports; boot checks already logged.
                if self.update_check_status.is_some() {
                    self.update_check_status =
                        Some(crate::update::UpdateStatus::Failed(cause.clone()));
                    self.set_toast(format!(
                        "{}: {}",
                        crate::i18n::t("update_check_failed"),
                        cause,
                    ));
                }
                return Task::perform(
                    async {
                        tokio::time::sleep(std::time::Duration::from_millis(2_500)).await;
                    },
                    |_| Message::ToastClear,
                );
            }
            UpdateMessage::UpdateSkipVersion => {
                if let Some(info) = self.pending_update.take()
                    && let Some(vault) = &self.vault {
                    let _ = vault.set_setting("skipped_update_version", &info.version);
                }
            }
            UpdateMessage::UpdateLater => {
                self.pending_update = None;
            }
            UpdateMessage::UpdateOpenRelease => {
                if let Some(info) = &self.pending_update {
                    let _ = open_in_browser(&info.html_url);
                }
            }
            UpdateMessage::UpdateStartDownload => {
                let Some(info) = self.pending_update.clone() else {
                    return Task::none();
                };
                let Some(url) = info.installer_url.clone() else {
                    self.update_error = Some("No installer asset for this platform".into());
                    return Task::none();
                };
                let name = info
                    .installer_name
                    .clone()
                    .unwrap_or_else(|| format!("oryxis-update-{}", info.version));
                self.update_downloading = true;
                self.update_progress = 0.0;
                self.update_error = None;
                // Stream so the modal's progress bar moves with the
                // download instead of jumping 0 to done. The sync
                // progress closure forwards into the async sink via an
                // unbounded channel.
                let stream = iced::stream::channel::<Message>(
                    100,
                    move |mut sender: iced::futures::channel::mpsc::Sender<Message>| async move {
                        use iced::futures::SinkExt as _;
                        let (ptx, mut prx) = tokio::sync::mpsc::unbounded_channel::<f32>();
                        let mut dl = tokio::spawn(async move {
                            crate::update::download_installer(&url, &name, move |p| {
                                let _ = ptx.send(p);
                            })
                            .await
                        });
                        loop {
                            tokio::select! {
                                Some(p) = prx.recv() => {
                                    let _ = sender
                                        .send(Message::Update(UpdateMessage::UpdateDownloadProgress(p)))
                                        .await;
                                }
                                res = &mut dl => {
                                    let result =
                                        res.unwrap_or_else(|e| Err(e.to_string()));
                                    let _ = sender
                                        .send(Message::Update(UpdateMessage::UpdateDownloadComplete(result)))
                                        .await;
                                    break;
                                }
                            }
                        }
                    },
                );
                return Task::stream(stream);
            }
            UpdateMessage::UpdateDownloadProgress(p) => {
                self.update_progress = p;
            }
            UpdateMessage::UpdateDownloadComplete(result) => {
                self.update_downloading = false;
                match result {
                    Ok(path) => {
                        // Nightly ships a bare binary we swap in place; a
                        // portable stable extracts its exe from the zip and
                        // takes the same swap; an AppImage replaces the
                        // image file; an installed stable hands the
                        // downloaded installer to the OS.
                        use crate::update::UpdateArtifact;
                        let apply = match self.pending_update.as_ref().map(|i| i.artifact) {
                            Some(UpdateArtifact::Binary) => {
                                crate::update::apply_binary_update(&path)
                            }
                            Some(UpdateArtifact::PortableArchive) => {
                                crate::update::extract_portable_exe(&path)
                                    .and_then(|exe| crate::update::apply_binary_update(&exe))
                            }
                            Some(UpdateArtifact::AppImage) => {
                                crate::update::apply_appimage_update(&path)
                            }
                            Some(UpdateArtifact::Installer) | None => {
                                crate::update::launch_installer(&path)
                            }
                        };
                        if let Err(e) = apply {
                            self.update_error = Some(e);
                        } else {
                            // Installer launched (or new binary spawned),
                            // exit so the old binary is released. Graceful
                            // quit via window close.
                            self.pending_update = None;
                            // The updated binary should reopen with
                            // today's geometry.
                            self.persist_window_geometry();
                            return iced::window::latest().then(|id_opt| match id_opt {
                                Some(id) => iced::window::close(id),
                                None => Task::none(),
                            });
                        }
                    }
                    Err(e) => self.update_error = Some(e),
                }
            }
        }
        Task::none()
    }
}
