//! Settings dispatch helpers: appearance. Tab / card / nav styling
//! arms (including the issue #79 tab-contrast family). Split out of
//! dispatch_settings/mod.rs.

use super::*;

impl Oryxis {
    /// Appearance arms: tab bar styling, host cards, nav rail and the
    /// status bar.
    pub(super) fn handle_settings_appearance(
        &mut self,
        message: SettingsMessage,
    ) -> Result<Task<Message>, SettingsMessage> {
        match message {
            SettingsMessage::FlattenHostsToggle => {
                self.flatten_hosts = !self.flatten_hosts;
                self.persist_setting(
                    "flatten_hosts",
                    if self.flatten_hosts { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingToggleShowStatusBar => {
                self.setting_show_status_bar = !self.setting_show_status_bar;
                self.persist_setting(
                    "show_status_bar",
                    if self.setting_show_status_bar { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingToggleStatusVersion => {
                self.setting_status_show_version = !self.setting_status_show_version;
                self.persist_setting("status_show_version", if self.setting_status_show_version { "true" } else { "false" });
            }
            SettingsMessage::SettingToggleStatusConnection => {
                self.setting_status_show_connection = !self.setting_status_show_connection;
                self.persist_setting("status_show_connection", if self.setting_status_show_connection { "true" } else { "false" });
            }
            SettingsMessage::SettingToggleStatusLatency => {
                self.setting_status_show_latency = !self.setting_status_show_latency;
                self.persist_setting("status_show_latency", if self.setting_status_show_latency { "true" } else { "false" });
            }
            SettingsMessage::SettingToggleStatusDimensions => {
                self.setting_status_show_dimensions = !self.setting_status_show_dimensions;
                self.persist_setting("status_show_dimensions", if self.setting_status_show_dimensions { "true" } else { "false" });
            }
            SettingsMessage::SettingToggleStatusAlignLeft => {
                self.setting_status_bar_align_left = !self.setting_status_bar_align_left;
                self.persist_setting(
                    "status_bar_align_left",
                    if self.setting_status_bar_align_left { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingToggleStatusCwd => {
                self.setting_status_show_cwd = !self.setting_status_show_cwd;
                self.persist_setting("status_show_cwd", if self.setting_status_show_cwd { "true" } else { "false" });
            }
            SettingsMessage::SettingToggleMonitorStatusBar => {
                self.setting_monitor_status_bar = !self.setting_monitor_status_bar;
                self.persist_setting(
                    "monitor_status_bar",
                    if self.setting_monitor_status_bar { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingMonitorIntervalChanged(v) => {
                // Digits only, same shaping as the other interval fields:
                // the value is validated on read, not on every keystroke.
                // Cap at an hour: anything longer is indistinguishable
                // from turning the tab off, and the field stays narrow.
                self.setting_monitor_interval = crate::util::sanitize_uint(&v, 3_600);
                self.persist_setting(
                    "monitor_interval_seconds",
                    &self.setting_monitor_interval.clone(),
                );
            }
            SettingsMessage::SettingToggleHostMonitoring => {
                self.setting_host_monitoring = !self.setting_host_monitoring;
                self.persist_setting(
                    "host_monitoring_enabled",
                    if self.setting_host_monitoring { "true" } else { "false" },
                );
                if self.setting_host_monitoring {
                    // First enable ever: seed the internal defaults ON so
                    // the feature shows immediate value instead of being
                    // "enabled but invisible". Guarded by a marker so a
                    // later off/on never clobbers the user's own choices.
                    if !self.setting_host_monitoring_seeded {
                        self.setting_host_monitoring_seeded = true;
                        self.persist_setting("host_monitoring_seeded", "true");
                        self.setting_monitor_status_bar = true;
                        self.persist_setting("monitor_status_bar", "true");
                    }
                } else {
                    // Turning the feature off stops all probing and drops
                    // the in-memory samples; nothing monitoring-related
                    // renders anymore, so a stale ring would only leak.
                    // `monitor_reset_all` also bumps the stamp so a probe
                    // still in flight can't repopulate (or toast) after
                    // the toggle.
                    self.monitor_reset_all();
                    self.monitor_ports_open = false;
                    self.monitor_disks_open = true;
                }
            }
            SettingsMessage::SettingToggleMonitorAllHosts => {
                self.setting_monitor_all_hosts = !self.setting_monitor_all_hosts;
                self.persist_setting(
                    "monitor_all_hosts",
                    if self.setting_monitor_all_hosts { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingToggleTerminalSidebarLeft => {
                self.setting_terminal_sidebar_left = !self.setting_terminal_sidebar_left;
                self.persist_setting(
                    "terminal_sidebar_side",
                    if self.setting_terminal_sidebar_left { "left" } else { "right" },
                );
            }
            SettingsMessage::SettingToggleSidebarAutoOpen => {
                self.setting_sidebar_auto_open = !self.setting_sidebar_auto_open;
                self.persist_setting(
                    "sidebar_auto_open",
                    if self.setting_sidebar_auto_open { "true" } else { "false" },
                );
            }
            SettingsMessage::ToggleHostListView => {
                // Dismiss the `…` overflow menu when toggled from there
                // (no-op for the inline toolbar button).
                self.overlay = None;
                self.setting_host_list_view = !self.setting_host_list_view;
                self.persist_setting(
                    "host_list_view",
                    if self.setting_host_list_view { "true" } else { "false" },
                );
            }
            SettingsMessage::ToggleCardAccentGlass => {
                self.setting_card_accent_glass = !self.setting_card_accent_glass;
                self.persist_setting(
                    "card_accent_glass",
                    if self.setting_card_accent_glass { "true" } else { "false" },
                );
            }
            SettingsMessage::ToggleShowHostAddress => {
                self.setting_show_host_address = !self.setting_show_host_address;
                self.persist_setting(
                    "show_host_address",
                    if self.setting_show_host_address { "true" } else { "false" },
                );
            }
            SettingsMessage::ToggleShowTabHostAddress => {
                self.setting_show_tab_host_address = !self.setting_show_tab_host_address;
                self.persist_setting(
                    "show_tab_host_address",
                    if self.setting_show_tab_host_address { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingToggleShowTabStatusDot => {
                self.setting_show_tab_status_dot = !self.setting_show_tab_status_dot;
                self.persist_setting(
                    "show_tab_status_dot",
                    if self.setting_show_tab_status_dot { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingToggleTabAccentLine => {
                self.setting_tab_accent_line = !self.setting_tab_accent_line;
                self.persist_setting(
                    "tab_accent_line",
                    if self.setting_tab_accent_line { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingToggleTabAccentWash => {
                self.setting_tab_accent_wash = !self.setting_tab_accent_wash;
                self.persist_setting(
                    "tab_accent_wash",
                    if self.setting_tab_accent_wash { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingToggleTabAccentText => {
                self.setting_tab_accent_text = !self.setting_tab_accent_text;
                self.persist_setting(
                    "tab_accent_text",
                    if self.setting_tab_accent_text { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingNavOrientationChanged(val) => {
                let normalized = match val.as_str() {
                    "vertical" => "vertical",
                    _ => "horizontal",
                };
                self.setting_nav_orientation = normalized.into();
                self.persist_setting("nav_orientation", normalized);
            }
            SettingsMessage::ToggleNavRailExpanded => {
                self.setting_nav_rail_expanded = !self.setting_nav_rail_expanded;
                self.persist_setting(
                    "nav_rail_expanded",
                    if self.setting_nav_rail_expanded { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingDefaultHostIconChanged(val) => {
                let normalized = match val.as_str() {
                    "square" => "square",
                    "rounded" => "rounded",
                    "outline" => "outline",
                    "initials" => "initials",
                    _ => "circular",
                };
                self.setting_default_host_icon = normalized.into();
                self.persist_setting("default_host_icon", normalized);
            }
            SettingsMessage::SettingTabCloseButtonSideChanged(val) => {
                // Only accept the two known values; anything else
                // collapses to the default so an unknown pick from a
                // future build can't wedge the tab bar.
                let normalized = match val.as_str() {
                    "right" => "right",
                    _ => "left",
                };
                self.setting_tab_close_button_side = normalized.into();
                self.persist_setting("tab_close_button_side", normalized);
            }
            SettingsMessage::SettingPinnedTabStyleChanged(val) => {
                let normalized = match val.as_str() {
                    "full" => "full",
                    _ => "compact",
                };
                self.setting_pinned_tab_style = normalized.into();
                self.persist_setting("pinned_tab_style", normalized);
            }
            SettingsMessage::SettingTabFillStyleChanged(val) => {
                let normalized = match val.as_str() {
                    "solid" => "solid",
                    _ => "gradient",
                };
                self.setting_tab_fill_style = normalized.into();
                self.persist_setting("tab_fill_style", normalized);
            }
            SettingsMessage::SettingTabAccentColorChanged(val) => {
                let normalized = match val.as_str() {
                    "app" => "app",
                    _ => "host",
                };
                self.setting_tab_accent_color = normalized.into();
                self.persist_setting("tab_accent_color", normalized);
            }
            SettingsMessage::SettingTabBarPositionChanged(val) => {
                let normalized = match val.as_str() {
                    "bottom" => "bottom",
                    "left" => "left",
                    "right" => "right",
                    _ => "top",
                };
                // The active-tab gradient direction lives in a process-wide
                // gate (read by `active_tab_bg` at render time, same shape
                // as the auto-title gate) so the "lit from the frame" fade
                // can flip without threading a flag through every tab
                // renderer.
                crate::views::tab_bar::set_tab_bar_pos(
                    crate::views::tab_bar::TabBarPos::from_setting(normalized),
                );
                self.setting_tab_bar_position = normalized.into();
                self.persist_setting("tab_bar_position", normalized);
            }
            SettingsMessage::SettingInactiveTabStyleChanged(val) => {
                let normalized = match val.as_str() {
                    "border" => "border",
                    "underline" => "underline",
                    _ => "none",
                };
                // Same process-wide gate shape as `tab_bar_position`: the
                // tab renderer reads it at draw time (issue #87).
                crate::views::tab_bar::set_inactive_tab_style(
                    crate::views::tab_bar::InactiveTabStyle::from_setting(normalized),
                );
                self.setting_inactive_tab_style = normalized.into();
                self.persist_setting("inactive_tab_style", normalized);
            }
            SettingsMessage::SettingTabWidthModeChanged(val) => {
                let normalized = if val == "uniform" { "uniform" } else { "adaptive" };
                self.setting_tab_width_mode = normalized.into();
                self.persist_setting("tab_width_mode", normalized);
            }
            SettingsMessage::SettingTabUniformSizeChanged(val) => {
                let normalized = match val.as_str() {
                    "small" => "small",
                    "large" => "large",
                    _ => "medium",
                };
                self.setting_tab_uniform_size = normalized.into();
                self.persist_setting("tab_uniform_size", normalized);
            }
            SettingsMessage::SettingTogglePinnedTabsTopBar => {
                self.setting_pinned_tabs_top_bar = !self.setting_pinned_tabs_top_bar;
                self.persist_setting(
                    "pinned_tabs_top_bar",
                    if self.setting_pinned_tabs_top_bar { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingToggleSideHideTopBar => {
                self.setting_side_hide_top_bar = !self.setting_side_hide_top_bar;
                self.persist_setting(
                    "side_hide_top_bar",
                    if self.setting_side_hide_top_bar { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingToggleSideFullHeight => {
                self.setting_side_full_height = !self.setting_side_full_height;
                self.persist_setting(
                    "side_full_height",
                    if self.setting_side_full_height { "true" } else { "false" },
                );
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }
}
