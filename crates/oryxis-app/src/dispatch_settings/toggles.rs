//! One-field settings with no machinery: performance mode, tray
//! behaviour, SFTP limits, cloud refresh, keepalive.
//!
//! Grouped by having nothing to say for themselves. Each writes a field
//! and persists it; anything that needed more than that is in one of the
//! other modules.

use super::*;

impl Oryxis {
    pub(super) fn handle_settings_toggles(
        &mut self,
        message: SettingsMessage,
    ) -> Result<Task<Message>, SettingsMessage> {
        match message {
            SettingsMessage::SettingTogglePerformanceMode => {
                self.prefs.performance_mode = !self.prefs.performance_mode;
                self.persist_setting(
                    "performance_mode",
                    if self.prefs.performance_mode { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingTogglePerfOverlay => {
                self.prefs.perf_overlay = !self.prefs.perf_overlay;
                self.persist_setting(
                    "perf_overlay",
                    if self.prefs.perf_overlay { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingToggleNetworkTools => {
                self.prefs.network_tools = !self.prefs.network_tools;
                self.persist_setting(
                    "network_tools_enabled",
                    if self.prefs.network_tools { "true" } else { "false" },
                );
                if !self.prefs.network_tools {
                    // Switching the feature off has to take the panel's
                    // tab with it: a chip that reopens a surface the
                    // user can no longer reach is the optional-features
                    // rule broken in the one state nobody tests.
                    return Ok(self.close_panel_tab(crate::state::PanelKind::NetTools));
                }
            }
            SettingsMessage::SettingToggleRemoteDesktop => {
                self.remote_desktop_enabled = !self.remote_desktop_enabled;
                self.persist_setting(
                    "remote_desktop_enabled",
                    if self.remote_desktop_enabled { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingToggleCloseToTray => {
                self.prefs.close_to_tray = !self.prefs.close_to_tray;
                self.persist_setting(
                    "close_to_tray",
                    if self.prefs.close_to_tray { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingToggleMinimizeToTray => {
                self.prefs.minimize_to_tray = !self.prefs.minimize_to_tray;
                // The Win32 subclass that intercepts the OS minimize
                // verbs can't read app state, so the toggle has to be
                // mirrored down to it or it keeps acting on the value
                // this process booted with.
                crate::tray::set_minimize_to_tray(self.prefs.minimize_to_tray);
                self.persist_setting(
                    "minimize_to_tray",
                    if self.prefs.minimize_to_tray { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingToggleSftpEnabled => {
                self.sftp_enabled = !self.sftp_enabled;
                self.persist_setting(
                    "sftp_enabled",
                    if self.sftp_enabled { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingKeepaliveChanged(val) => {
                // Accept only digits; cap at 86_400 (1 day) so users can't
                // accidentally type a runaway value.
                self.prefs.keepalive_interval = sanitize_uint(&val, 86_400);
                self.persist_setting("keepalive_interval", &self.prefs.keepalive_interval);
            }
            SettingsMessage::SettingCloudAutoRefreshToggle => {
                self.prefs.cloud_auto_refresh_enabled =
                    !self.prefs.cloud_auto_refresh_enabled;
                self.persist_setting(
                    "cloud_auto_refresh_enabled",
                    if self.prefs.cloud_auto_refresh_enabled { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingCloudAutoRefreshIntervalChanged(val) => {
                // Floor of 1 minute, ceiling of 1 day. AWS rate limits
                // are well above a per-minute pace for the discovery
                // calls we make, but the ceiling is just a sanity cap.
                self.prefs.cloud_auto_refresh_interval_minutes =
                    sanitize_uint(&val, 1_440);
                if self.prefs.cloud_auto_refresh_interval_minutes == "0" {
                    self.prefs.cloud_auto_refresh_interval_minutes = "1".into();
                }
                self.persist_setting(
                    "cloud_auto_refresh_interval_minutes",
                    &self.prefs.cloud_auto_refresh_interval_minutes,
                );
            }
            SettingsMessage::SettingCloudAutoArchiveToggle => {
                self.prefs.cloud_auto_archive_orphans =
                    !self.prefs.cloud_auto_archive_orphans;
                self.persist_setting(
                    "cloud_auto_archive_orphans",
                    if self.prefs.cloud_auto_archive_orphans { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingCloudOrphanArchiveDaysChanged(val) => {
                // Floor of 1 day (an orphan needs at least one full day
                // to "settle" so a transient AWS API hiccup doesn't
                // wipe legitimate hosts). Ceiling of one year.
                self.prefs.cloud_orphan_archive_days = sanitize_uint(&val, 365);
                if self.prefs.cloud_orphan_archive_days == "0" {
                    self.prefs.cloud_orphan_archive_days = "1".into();
                }
                self.persist_setting(
                    "cloud_orphan_archive_days",
                    &self.prefs.cloud_orphan_archive_days,
                );
            }
            SettingsMessage::SettingSftpConcurrencyChanged(val) => {
                // Cap at 8, beyond that the SSH channel multiplexer
                // overhead outweighs the throughput gain on most links.
                self.prefs.sftp_concurrency = sanitize_uint(&val, 8);
                if self.prefs.sftp_concurrency == "0" {
                    self.prefs.sftp_concurrency = "1".into();
                }
                self.persist_setting("sftp_concurrency", &self.prefs.sftp_concurrency);
            }
            SettingsMessage::SettingSftpConnectTimeoutChanged(val) => {
                self.prefs.sftp_connect_timeout = sanitize_uint(&val, 600);
                if self.prefs.sftp_connect_timeout == "0" {
                    self.prefs.sftp_connect_timeout = "1".into();
                }
                self.persist_setting(
                    "sftp_connect_timeout",
                    &self.prefs.sftp_connect_timeout,
                );
            }
            SettingsMessage::SettingSftpAuthTimeoutChanged(val) => {
                self.prefs.sftp_auth_timeout = sanitize_uint(&val, 600);
                if self.prefs.sftp_auth_timeout == "0" {
                    self.prefs.sftp_auth_timeout = "1".into();
                }
                self.persist_setting("sftp_auth_timeout", &self.prefs.sftp_auth_timeout);
            }
            SettingsMessage::SettingSftpSessionTimeoutChanged(val) => {
                self.prefs.sftp_session_timeout = sanitize_uint(&val, 600);
                if self.prefs.sftp_session_timeout == "0" {
                    self.prefs.sftp_session_timeout = "1".into();
                }
                self.persist_setting(
                    "sftp_session_timeout",
                    &self.prefs.sftp_session_timeout,
                );
            }
            SettingsMessage::SettingSftpOpTimeoutChanged(val) => {
                self.prefs.sftp_op_timeout = sanitize_uint(&val, 600);
                if self.prefs.sftp_op_timeout == "0" {
                    self.prefs.sftp_op_timeout = "1".into();
                }
                // Apply live to both panes' active SFTP clients so the
                // user doesn't have to reconnect to feel the change.
                let to = self.sftp_op_timeout();
                if let Some(client) = &self.sftp.left.client {
                    client.set_op_timeout(to);
                }
                if let Some(client) = &self.sftp.right.client {
                    client.set_op_timeout(to);
                }
                self.persist_setting("sftp_op_timeout", &self.prefs.sftp_op_timeout);
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }
}
