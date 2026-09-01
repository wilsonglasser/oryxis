//! Plugins panel, manage the downloaded cloud-provider plugins.
//!
//! Cloud providers (AWS + Kubernetes today, gcp / azure later) run as
//! subprocess plugins downloaded on demand. This screen is the
//! IDE-style management surface: per-provider status, install /
//! update / uninstall, and the auto-update toggles. The first-use
//! install opt-in modal (`view_plugin_install_modal`) lives here too
//! and is layered by `root_view`.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, scrollable, text, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{SettingsMessage, PluginMessage, AgentMessage, AiMessage, SyncMessage, Message, Oryxis};
use crate::state::{PluginUiEntry, PluginUiStatus};
use crate::theme::OryxisColors;
use crate::widgets::{dir_align_x, dir_row, panel_section, toggle_row_desc};

impl Oryxis {
    pub(crate) fn view_plugins_panel(&self) -> Element<'_, Message> {
        // Keyboard rows are recorded in visual order.
        self.keynav_settings_reset();
        // Built-in "feature plugins": SFTP / AI / Sync / MCP are enabled
        // here (their Settings sections only appear once enabled), the
        // same surface as the downloadable provider plugins below.
        let mut rows: Vec<Element<'_, Message>> = vec![
            text(crate::i18n::t("features"))
                .size(13)
                .color(OryxisColors::t().text_primary)
                .into(),
            Space::new().height(8).into(),
            panel_section(column![
                self.settings_nav_slot_labeled(
                    crate::i18n::t("ai_assistant"),
                    crate::keynav::RowAction::activate(Message::Ai(AiMessage::ToggleAiEnabled)),
                    8.0,
                    toggle_row_desc(
                        crate::i18n::t("ai_assistant"),
                        crate::i18n::t("feature_ai_desc"),
                        self.ai.enabled,
                        Message::Ai(AiMessage::ToggleAiEnabled),
                    ),
                ),
                Space::new().height(12),
                // MCP is not listed here: it's a real plugin binary (the
                // "Oryxis MCP Server" card below), so it's activated and
                // managed there, and its server on/off lives in the MCP
                // settings section that appears once the plugin is present.
                self.settings_nav_slot_labeled(
                    crate::i18n::t("sftp"),
                    crate::keynav::RowAction::activate(Message::Settings(SettingsMessage::SettingToggleSftpEnabled)),
                    8.0,
                    toggle_row_desc(
                        "SFTP",
                        crate::i18n::t("feature_sftp_desc"),
                        self.sftp_enabled,
                        Message::Settings(SettingsMessage::SettingToggleSftpEnabled),
                    ),
                ),
                Space::new().height(12),
                self.settings_nav_slot_labeled(
                    crate::i18n::t("sync"),
                    crate::keynav::RowAction::activate(Message::Sync(SyncMessage::ToggleEnabled)),
                    8.0,
                    toggle_row_desc(
                        crate::i18n::t("sync"),
                        crate::i18n::t("feature_sync_desc"),
                        self.sync.enabled,
                        Message::Sync(SyncMessage::ToggleEnabled),
                    ),
                ),
                Space::new().height(12),
                self.settings_nav_slot_labeled(
                    crate::i18n::t("remote_desktop"),
                    crate::keynav::RowAction::activate(Message::Settings(SettingsMessage::SettingToggleRemoteDesktop)),
                    8.0,
                    toggle_row_desc(
                        crate::i18n::t("remote_desktop"),
                        crate::i18n::t("feature_remote_desktop_desc"),
                        self.remote_desktop_enabled,
                        Message::Settings(SettingsMessage::SettingToggleRemoteDesktop),
                    ),
                ),
                Space::new().height(12),
                // Host monitoring (issue #83): niche + recurring, so the
                // whole subsystem hides until enabled here (the sidebar
                // Monitor tab, status-bar segment, per-host opt-in and
                // interval all appear only once this is on).
                self.settings_nav_slot_labeled(
                    crate::i18n::t("feature_monitoring"),
                    crate::keynav::RowAction::activate(Message::Settings(SettingsMessage::SettingToggleHostMonitoring)),
                    8.0,
                    toggle_row_desc(
                        crate::i18n::t("feature_monitoring"),
                        crate::i18n::t("feature_monitoring_desc"),
                        self.prefs.host_monitoring,
                        Message::Settings(SettingsMessage::SettingToggleHostMonitoring),
                    ),
                ),
                Space::new().height(12),
                // tmux manager (issue #116): same rule as monitoring.
                // Managing tmux from a panel is niche, so the sidebar
                // tab only exists once this is on.
                self.settings_nav_slot_labeled(
                    crate::i18n::t("feature_tmux"),
                    crate::keynav::RowAction::activate(Message::Settings(SettingsMessage::SettingToggleTmuxManager)),
                    8.0,
                    toggle_row_desc(
                        crate::i18n::t("feature_tmux"),
                        crate::i18n::t("feature_tmux_desc"),
                        self.prefs.tmux_manager,
                        Message::Settings(SettingsMessage::SettingToggleTmuxManager),
                    ),
                ),
                Space::new().height(12),
                // Network tools (H1): same rule again. It lives HERE and
                // not in Advanced, because this list is what a user reads
                // to find out what the app can be made to do; a feature
                // reachable only from a settings page nobody browses does
                // not exist to them.
                self.settings_nav_slot_labeled(
                    crate::i18n::t("setting_network_tools"),
                    crate::keynav::RowAction::activate(Message::Settings(SettingsMessage::SettingToggleNetworkTools)),
                    8.0,
                    toggle_row_desc(
                        crate::i18n::t("setting_network_tools"),
                        crate::i18n::t("setting_network_tools_hint"),
                        self.prefs.network_tools,
                        Message::Settings(SettingsMessage::SettingToggleNetworkTools),
                    ),
                ),
                Space::new().height(12),
                // Features holds only the enable toggle; the confirm +
                // socket rows live in the Settings sidebar's SSH Agent
                // section, which appears while the agent is enabled.
                self.agent_server_toggle(),
            ]),
            Space::new().height(18).into(),
            // Plugins list header: subtitle on the leading edge; the
            // list-wide actions (one update check for every installed
            // row + the global auto-update toggle) on the trailing
            // edge, one line. Per-row copies of both moved into the
            // row kebab as override / retry affordances.
            dir_row(vec![
                text(crate::i18n::t("plugins_subtitle"))
                    .size(12)
                    .color(OryxisColors::t().text_muted)
                    .into(),
                Space::new().width(Length::Fill).into(),
                self.settings_nav_slot_labeled(
                    crate::i18n::t("plugin_action_check_updates"),
                    crate::keynav::RowAction::activate(Message::Plugin(PluginMessage::PluginCheckAllUpdates)),
                    6.0,
                    pill_button(
                        crate::i18n::t("plugin_action_check_updates"),
                        Some(Message::Plugin(PluginMessage::PluginCheckAllUpdates)),
                        OryxisColors::t().text_secondary,
                        false,
                    ),
                ),
                Space::new().width(14).into(),
                self.settings_nav_slot_labeled(
                    crate::i18n::t("plugins_auto_update_global"),
                    crate::keynav::RowAction::activate(Message::Plugin(PluginMessage::PluginToggleGlobalAutoUpdate(
                        !self.plugins_auto_update_global,
                    ))),
                    8.0,
                    crate::widgets::toggle_switch_labeled(
                        crate::i18n::t("plugins_auto_update_global"),
                        self.plugins_auto_update_global,
                        Message::Plugin(PluginMessage::PluginToggleGlobalAutoUpdate(!self.plugins_auto_update_global)),
                    ),
                ),
            ])
            .align_y(iced::Alignment::Center)
            .width(Length::Fill)
            .into(),
            Space::new().height(14).into(),
        ];

        if self.plugins.is_empty() {
            rows.push(
                container(
                    text(crate::i18n::t("plugins_empty"))
                        .size(13)
                        .color(OryxisColors::t().text_muted),
                )
                .padding(16)
                .into(),
            );
        }

        for entry in &self.plugins {
            rows.push(plugin_card(self, entry));
            rows.push(Space::new().height(8).into());
        }

        scrollable(
            column(rows).padding(Padding {
                top: 24.0,
                right: 24.0,
                bottom: 24.0,
                left: 24.0,
            }),
        )
        // Stable id so the keyboard router can keep the selected row
        // in view.
        .id(iced::widget::Id::new("settings-plugins-scroll"))
        .on_scroll(|v| Message::Settings(SettingsMessage::SectionScrolled(v.relative_offset().y)))
        .height(Length::Fill)
        .width(Length::Fill)
        .into()
    }

    /// First-use install opt-in modal. Returns just the dialog;
    /// `root_view` wraps it in the scrim. Only call when
    /// `plugin_install_modal` is `Some`.
    pub(crate) fn view_plugin_install_modal(&self) -> Element<'_, Message> {
        let provider_id = self.plugin_install_modal.as_deref().unwrap_or("");
        let entry = self
            .plugins
            .iter()
            .find(|p| p.provider_id == provider_id);
        let display_name = entry
            .map(|e| e.display_name.as_str())
            .unwrap_or(provider_id);

        // The manifest's best compatible entry drives the size +
        // changelog. Until the manifest host exists (PR 6) this is
        // always `None`, so the modal degrades to "size unknown".
        let best = entry.and_then(|e| e.manifest.as_ref()).and_then(|m| {
            m.best(
                env!("CARGO_PKG_VERSION"),
                oryxis_plugin_protocol::SUPPORTED_PROTOCOL_VERSIONS,
            )
        });
        let checking = matches!(
            entry.map(|e| &e.status),
            Some(PluginUiStatus::Checking)
        );

        let size_line: Element<'_, Message> = match best {
            Some(b) => {
                let bin = b.binary_for_current_platform();
                let size = bin.map(|x| x.size).unwrap_or(0);
                text(format!(
                    "{}: {}",
                    crate::i18n::t("plugin_install_modal_size"),
                    crate::util::format_data_size(size as usize),
                ))
                .size(12)
                .color(OryxisColors::t().text_secondary)
                .into()
            }
            None if checking => text(crate::i18n::t("plugin_status_checking"))
                .size(12)
                .color(OryxisColors::t().text_muted)
                .into(),
            // The manifest DID arrive and every version in it was
            // filtered out (min_app / protocol / platform). Nothing
            // about the network is wrong here, so this must not show
            // the firewall block: the install button says the same
            // thing, and claiming an unreachable host sends the user
            // to debug a proxy that is working fine (#163).
            None if entry.is_some_and(|e| e.manifest.is_some()) => {
                text(crate::i18n::t("plugin_err_needs_update"))
                    .size(12)
                    .color(OryxisColors::t().warning)
                    .into()
            }
            // The fetch itself failed. A bare "unavailable" was a dead
            // end (discussion #163, a mainland-China network with
            // GitHub blocked), so the error carries its own way out:
            // the exact hosts a firewall would need to allow, the
            // cause the fetch reported, and a jump to the Download
            // mirror setting.
            None => {
                let hosts = crate::net_mirror::consulted_hosts().join("\n");
                let mut block = column![
                    text(crate::i18n::t("plugin_install_modal_unknown_size"))
                        .size(12)
                        .color(OryxisColors::t().warning),
                ];
                // What the fetch actually reported, verbatim. Without
                // it every cause reads as "blocked by a firewall",
                // which is what made #163 undebuggable from the
                // outside.
                if let Some(cause) = entry.and_then(|e| e.manifest_error.as_deref()) {
                    block = block.push(Space::new().height(6)).push(
                        text(format!("{}: {cause}", crate::i18n::t("plugin_err_cause")))
                            .size(11)
                            .font(iced::Font::MONOSPACE)
                            .color(OryxisColors::t().text_muted),
                    );
                }
                block
                    .push(Space::new().height(10))
                    .push(
                        text(crate::i18n::t("plugin_hosts_hint"))
                            .size(12)
                            .color(OryxisColors::t().text_secondary),
                    )
                    .push(Space::new().height(4))
                    .push(
                        container(
                            text(hosts)
                                .size(11)
                                .font(iced::Font::MONOSPACE)
                                .color(OryxisColors::t().text_muted),
                        )
                        .padding(Padding { top: 8.0, right: 10.0, bottom: 8.0, left: 10.0 })
                        .width(Length::Fill)
                        .style(|_| iced::widget::container::Style {
                            background: Some(iced::Background::Color(
                                OryxisColors::t().bg_primary,
                            )),
                            border: iced::Border {
                                radius: iced::border::Radius::from(6.0),
                                color: OryxisColors::t().border,
                                width: 1.0,
                            },
                            ..Default::default()
                        }),
                    )
                    .push(Space::new().height(10))
                    .push(
                        text(crate::i18n::t("plugin_mirror_hint"))
                            .size(12)
                            .color(OryxisColors::t().text_secondary),
                    )
                    .push(Space::new().height(8))
                    .push(dir_row(vec![pill_button(
                        crate::i18n::t("download_mirror"),
                        Some(Message::Plugin(PluginMessage::OpenMirrorSetting)),
                        OryxisColors::t().accent,
                        false,
                    )]))
                    .into()
            }
        };

        let mut body = column![
            text(crate::i18n::t("plugin_install_modal_body"))
                .size(13)
                .color(OryxisColors::t().text_primary),
            Space::new().height(10),
            size_line,
        ]
        .spacing(0);

        // Changelog, when the manifest carried one.
        if let Some(notes) = best.and_then(|b| b.changelog.as_deref()) {
            body = body.push(Space::new().height(12));
            body = body.push(
                text(crate::i18n::t("plugin_changelog"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary),
            );
            body = body.push(Space::new().height(4));
            body = body.push(
                text(notes.to_string())
                    .size(12)
                    .color(OryxisColors::t().text_muted),
            );
        }

        let can_install = best.is_some();
        let install_btn = pill_button(
            crate::i18n::t("plugin_install_confirm"),
            can_install.then(|| Message::Plugin(PluginMessage::PluginInstall(provider_id.to_string()))),
            OryxisColors::t().accent,
            true,
        );
        let cancel_btn = pill_button(
            crate::i18n::t("cancel"),
            Some(Message::Plugin(PluginMessage::HidePluginInstallModal)),
            OryxisColors::t().text_muted,
            false,
        );

        let header = container(
            text(format!(
                "{} {}",
                crate::i18n::t("plugin_install_modal_title"),
                display_name,
            ))
            .size(15)
            .font(iced::Font {
                weight: iced::font::Weight::Semibold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            })
            .color(OryxisColors::t().text_primary),
        )
        .padding(Padding { top: 16.0, right: 20.0, bottom: 8.0, left: 20.0 });

        let footer = container(
            dir_row(vec![
                Space::new().width(Length::Fill).into(),
                cancel_btn,
                Space::new().width(8).into(),
                install_btn,
            ])
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 8.0, right: 16.0, bottom: 16.0, left: 16.0 });

        let dialog = iced::widget::MouseArea::new(
            container(
                column![
                    header,
                    container(body).padding(Padding {
                        top: 4.0,
                        right: 20.0,
                        bottom: 12.0,
                        left: 20.0,
                    }),
                    footer,
                ],
            )
            .width(Length::Fixed(420.0))
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    radius: Radius::from(12.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                shadow: iced::Shadow {
                    color: Color::from_rgba(0.0, 0.0, 0.0, 0.30),
                    offset: iced::Vector::new(0.0, 8.0),
                    blur_radius: 24.0,
                },
                ..Default::default()
            }),
        )
        .on_press(Message::NoOp);

        // Bare card; `widgets::modal_overlay` (the caller) centers + scrims.
        dialog.into()
    }

    /// The ssh-agent ENABLE toggle, shown in the Features section. Any
    /// runtime error is surfaced inline under it. Off unix (pre-Phase-3
    /// Windows) with no listener, the whole row is hidden. The confirm +
    /// socket rows live in the sidebar's SSH Agent section
    /// (`view_settings_agent`), visible only while the agent is on.
    fn agent_server_toggle(&self) -> Element<'_, Message> {
        // No socket path means no listener on this platform: hide it.
        if crate::agent_server::listener_socket_display().is_none() {
            return Space::new().into();
        }

        let toggle = self.settings_nav_slot_labeled(
            crate::i18n::t("agent_server"),
            crate::keynav::RowAction::activate(Message::Agent(AgentMessage::AgentServerToggled(!self.agent.enabled))),
            8.0,
            toggle_row_desc(
                crate::i18n::t("agent_server"),
                crate::i18n::t("agent_server_desc"),
                self.agent.enabled,
                Message::Agent(AgentMessage::AgentServerToggled(!self.agent.enabled)),
            ),
        );

        if let Some(err) = &self.agent.error {
            return column![
                toggle,
                Space::new().height(6),
                text(err.clone()).size(11).color(OryxisColors::t().error),
            ]
            .into();
        }
        toggle
    }

}

/// One provider row, single-line: brand icon + name + version +
/// status badge on the leading edge, the status's primary action and
/// the kebab trigger on the trailing edge. Secondary actions (check
/// for updates, the auto-update override, uninstall) live in the
/// kebab menu (`build_menu_plugin_actions`), also reachable by
/// right-clicking the row. Only an error / dev-build hint adds a
/// second line. Takes the app so every control registers as a
/// keyboard row at construction, in visual order.
fn plugin_card<'a>(app: &Oryxis, entry: &'a PluginUiEntry) -> Element<'a, Message> {
    let id = entry.provider_id.clone();

    let (badge_label, badge_color) = match &entry.status {
        PluginUiStatus::DevBuild => (
            crate::i18n::t("plugin_status_dev_build"),
            OryxisColors::t().accent,
        ),
        PluginUiStatus::Installed(_) => (
            crate::i18n::t("plugin_status_installed"),
            OryxisColors::t().success,
        ),
        PluginUiStatus::UpdateAvailable { .. } => (
            crate::i18n::t("plugin_status_update_available"),
            OryxisColors::t().warning,
        ),
        PluginUiStatus::NotInstalled => (
            crate::i18n::t("plugin_status_not_installed"),
            OryxisColors::t().text_muted,
        ),
        PluginUiStatus::Checking => (
            crate::i18n::t("plugin_status_checking"),
            OryxisColors::t().text_secondary,
        ),
        PluginUiStatus::Downloading => (
            crate::i18n::t("plugin_status_downloading"),
            OryxisColors::t().accent,
        ),
        PluginUiStatus::Failed(_) => (
            crate::i18n::t("plugin_status_error"),
            OryxisColors::t().error,
        ),
    };

    // Inline version tail next to the name: current version, or the
    // available transition. The pin rides the same slot so the row
    // stays one line.
    let mut version = match &entry.status {
        PluginUiStatus::Installed(v) => Some(format!("v{v}")),
        PluginUiStatus::UpdateAvailable { current, latest } => {
            Some(format!("v{current} \u{2192} v{latest}"))
        }
        _ => None,
    };
    if let Some(pinned) = &entry.pinned_version {
        let pin = format!("{} v{pinned}", crate::i18n::t("plugin_pinned"));
        version = Some(match version {
            Some(v) => format!("{v} \u{00B7} {pin}"),
            None => pin,
        });
    }

    // Second line only where one is genuinely needed: an install /
    // fetch error, or the dev-build explainer.
    let detail: Option<(String, Color)> = match &entry.status {
        PluginUiStatus::DevBuild => Some((
            crate::i18n::t("plugin_dev_build_hint").to_string(),
            OryxisColors::t().text_muted,
        )),
        PluginUiStatus::Failed(msg) => {
            Some((msg.clone(), OryxisColors::t().error))
        }
        _ => None,
    };

    let badge = container(
        text(badge_label)
            .size(10)
            .color(badge_color),
    )
    .padding(Padding { top: 2.0, right: 8.0, bottom: 2.0, left: 8.0 })
    .style(move |_| container::Style {
        background: Some(Background::Color(Color { a: 0.14, ..badge_color })),
        border: Border { radius: Radius::from(6.0), ..Default::default() },
        ..Default::default()
    });

    // Provider brand logo (AWS smile, Kubernetes wheel, ...) instead of
    // a generic package box. MCP has no brand SVG, so it gets a server
    // glyph; unknown cloud providers fall back to the cloud glyph.
    let (brand_icon, brand_icon_color) = if entry.provider_id == "mcp" {
        (
            crate::os_icon::BrandIcon::Glyph(iced_fonts::lucide::server()),
            OryxisColors::t().accent,
        )
    } else {
        crate::os_icon::provider_icon(&entry.provider_id, OryxisColors::t().accent)
    };

    let mut row_items: Vec<Element<'_, Message>> = vec![
        brand_icon.view(16.0, brand_icon_color),
        Space::new().width(10).into(),
        text(&entry.display_name)
            .size(14)
            .color(OryxisColors::t().text_primary)
            .into(),
    ];
    if let Some(v) = version {
        row_items.push(Space::new().width(8).into());
        row_items.push(
            text(v).size(11).color(OryxisColors::t().text_secondary).into(),
        );
    }
    row_items.push(Space::new().width(10).into());
    row_items.push(badge.into());
    row_items.push(Space::new().width(Length::Fill).into());

    // Primary action per status, trailing edge. Everything secondary
    // is in the kebab, so a healthy installed row carries no inline
    // button at all. Every button here has a real message (a disabled
    // `pill_button(None)` never reaches this match), so each one is
    // recorded as a keyboard row, in visual order.
    match &entry.status {
        PluginUiStatus::NotInstalled => {
            row_items.push(app.settings_nav_slot(
                crate::keynav::RowAction::activate(Message::Plugin(PluginMessage::ShowPluginInstallModal(id.clone()))),
                6.0,
                pill_button(
                    crate::i18n::t("plugin_action_install"),
                    Some(Message::Plugin(PluginMessage::ShowPluginInstallModal(id.clone()))),
                    OryxisColors::t().accent,
                    true,
                ),
            ));
        }
        PluginUiStatus::UpdateAvailable { .. } => {
            row_items.push(app.settings_nav_slot(
                crate::keynav::RowAction::activate(Message::Plugin(PluginMessage::PluginInstall(id.clone()))),
                6.0,
                pill_button(
                    crate::i18n::t("plugin_action_update"),
                    Some(Message::Plugin(PluginMessage::PluginInstall(id.clone()))),
                    OryxisColors::t().accent,
                    true,
                ),
            ));
        }
        PluginUiStatus::Failed(_) => {
            row_items.push(app.settings_nav_slot(
                crate::keynav::RowAction::activate(Message::Plugin(PluginMessage::PluginCheckUpdates(id.clone()))),
                6.0,
                pill_button(
                    crate::i18n::t("plugin_action_retry"),
                    Some(Message::Plugin(PluginMessage::PluginCheckUpdates(id.clone()))),
                    OryxisColors::t().text_secondary,
                    false,
                ),
            ));
            row_items.push(Space::new().width(8).into());
            row_items.push(app.settings_nav_slot(
                crate::keynav::RowAction::activate(Message::Plugin(PluginMessage::ShowPluginInstallModal(id.clone()))),
                6.0,
                pill_button(
                    crate::i18n::t("plugin_action_install"),
                    Some(Message::Plugin(PluginMessage::ShowPluginInstallModal(id.clone()))),
                    OryxisColors::t().accent,
                    false,
                ),
            ));
        }
        // Installed / dev-build rows act through the kebab; in-flight
        // rows (checking / downloading) have nothing to click.
        PluginUiStatus::Installed(_)
        | PluginUiStatus::DevBuild
        | PluginUiStatus::Checking
        | PluginUiStatus::Downloading => {}
    }

    // Kebab trigger, on every row that has secondary actions. Always
    // visible (Settings controls aren't hover-revealed cards) and a
    // recorded keyboard row like the pills.
    let has_menu = matches!(
        entry.status,
        PluginUiStatus::Installed(_) | PluginUiStatus::UpdateAvailable { .. }
    ) || (matches!(entry.status, PluginUiStatus::DevBuild) && entry.cached_install);
    if has_menu {
        row_items.push(Space::new().width(8).into());
        row_items.push(app.settings_nav_slot(
            crate::keynav::RowAction::activate(Message::Plugin(PluginMessage::ShowPluginMenu(id.clone()))),
            6.0,
            crate::widgets::card_kebab_button(
                OryxisColors::t().text_muted,
                true,
                Message::Plugin(PluginMessage::ShowPluginMenu(id.clone())),
            )
            .into(),
        ));
    }

    // The detail line must hug the leading edge under RTL, hence the
    // dir-aware alignment (the width:Fill gives it slack to align in).
    let mut card = column![dir_row(row_items).align_y(iced::Alignment::Center)]
        .spacing(6)
        .width(Length::Fill)
        .align_x(dir_align_x());
    if let Some((line, color)) = detail {
        card = card.push(text(line).size(11).color(color));
    }

    let styled = container(card)
        .padding(Padding { top: 10.0, right: 16.0, bottom: 10.0, left: 16.0 })
        .width(Length::Fill)
        .style(|_| container::Style {
            // Match the `panel_section` bg used elsewhere in Settings
            // so a plugin card looks like every other settings panel
            // instead of the lighter `bg_surface` it used before.
            background: Some(Background::Color(OryxisColors::t().bg_hover)),
            border: Border {
                radius: Radius::from(8.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        });

    if has_menu {
        // Right-click anywhere on the row opens the same kebab menu
        // (app-wide card convention).
        iced::widget::MouseArea::new(styled)
            .on_right_press(Message::Plugin(PluginMessage::ShowPluginMenu(id)))
            .into()
    } else {
        styled.into()
    }
}

/// Small action button. `accent_color` tints the border + label;
/// `filled` makes it a solid accent button (used for the primary
/// action). `None` message renders it disabled.
fn pill_button<'a>(
    label: &'a str,
    msg: Option<Message>,
    accent_color: Color,
    filled: bool,
) -> Element<'a, Message> {
    let enabled = msg.is_some();
    let label_color = if !enabled {
        OryxisColors::t().text_muted
    } else if filled {
        OryxisColors::t().bg_primary
    } else {
        accent_color
    };
    let mut b = button(
        container(
            text(label)
                .size(11)
                .color(label_color)
                .font(iced::Font {
                    weight: iced::font::Weight::Semibold,
                    ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                }),
        )
        .padding(Padding { top: 5.0, right: 12.0, bottom: 5.0, left: 12.0 }),
    )
    .style(move |_, status| {
        let bg = if !enabled {
            Color::TRANSPARENT
        } else if filled {
            match status {
                BtnStatus::Hovered => Color { a: 0.85, ..accent_color },
                BtnStatus::Pressed => Color { a: 0.70, ..accent_color },
                _ => accent_color,
            }
        } else {
            match status {
                BtnStatus::Hovered => Color { a: 0.15, ..accent_color },
                _ => Color::TRANSPARENT,
            }
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                radius: Radius::from(6.0),
                color: if enabled { accent_color } else { OryxisColors::t().border },
                width: 1.0,
            },
            ..Default::default()
        }
    });
    if let Some(msg) = msg {
        b = b.on_press(msg);
    }
    b.into()
}

