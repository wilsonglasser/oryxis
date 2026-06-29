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

use crate::app::{Message, Oryxis};
use crate::state::{PluginUiEntry, PluginUiStatus};
use crate::theme::OryxisColors;
use crate::widgets::{dir_align_x, dir_row, panel_section, toggle_row_desc};

impl Oryxis {
    pub(crate) fn view_plugins_panel(&self) -> Element<'_, Message> {
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
                toggle_row_desc(
                    crate::i18n::t("ai_assistant"),
                    crate::i18n::t("feature_ai_desc"),
                    self.ai.enabled,
                    Message::ToggleAiEnabled,
                ),
                Space::new().height(12),
                // MCP is not listed here: it's a real plugin binary (the
                // "Oryxis MCP Server" card below), so it's activated and
                // managed there, and its server on/off lives in the MCP
                // settings section that appears once the plugin is present.
                toggle_row_desc(
                    "SFTP",
                    crate::i18n::t("feature_sftp_desc"),
                    self.sftp_enabled,
                    Message::SettingToggleSftpEnabled,
                ),
                Space::new().height(12),
                toggle_row_desc(
                    crate::i18n::t("sync"),
                    crate::i18n::t("feature_sync_desc"),
                    self.sync.enabled,
                    Message::SyncToggleEnabled,
                ),
            ]),
            Space::new().height(18).into(),
            // Plugins list header: subtitle on the leading edge, the
            // global auto-update toggle on the trailing edge, one line.
            dir_row(vec![
                text(crate::i18n::t("plugins_subtitle"))
                    .size(12)
                    .color(OryxisColors::t().text_muted)
                    .into(),
                Space::new().width(Length::Fill).into(),
                crate::widgets::toggle_switch_labeled(
                    crate::i18n::t("plugins_auto_update_global"),
                    self.plugins_auto_update_global,
                    Message::PluginToggleGlobalAutoUpdate(!self.plugins_auto_update_global),
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
            rows.push(plugin_card(entry));
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
            None => text(crate::i18n::t("plugin_install_modal_unknown_size"))
                .size(12)
                .color(OryxisColors::t().warning)
                .into(),
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
            can_install.then(|| Message::PluginInstall(provider_id.to_string())),
            OryxisColors::t().accent,
            true,
        );
        let cancel_btn = pill_button(
            crate::i18n::t("cancel"),
            Some(Message::HidePluginInstallModal),
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
}

/// One provider row: icon + name + status badge, a version / hint
/// line, the action buttons, and (when installed) the per-plugin
/// auto-update toggle.
fn plugin_card(entry: &PluginUiEntry) -> Element<'_, Message> {
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

    // Secondary line: version, version transition, hint, or error.
    let detail: Option<(String, Color)> = match &entry.status {
        PluginUiStatus::DevBuild => Some((
            crate::i18n::t("plugin_dev_build_hint").to_string(),
            OryxisColors::t().text_muted,
        )),
        PluginUiStatus::Installed(v) => {
            Some((format!("v{v}"), OryxisColors::t().text_secondary))
        }
        PluginUiStatus::UpdateAvailable { current, latest } => Some((
            format!("v{current}  \u{2192}  v{latest}"),
            OryxisColors::t().text_secondary,
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
    let header = dir_row(vec![
        brand_icon.view(16.0, brand_icon_color),
        Space::new().width(10).into(),
        text(&entry.display_name)
            .size(14)
            .color(OryxisColors::t().text_primary)
            .into(),
        Space::new().width(10).into(),
        badge.into(),
        Space::new().width(Length::Fill).into(),
    ])
    .align_y(iced::Alignment::Center);

    let mut card = column![header].spacing(6);

    if let Some((line, color)) = detail {
        card = card.push(text(line).size(11).color(color));
    }

    // A user-pinned version, when set, holds the updater on a
    // specific release. Surfaced here so the pin is visible.
    if let Some(pinned) = &entry.pinned_version {
        card = card.push(
            text(format!("{} v{pinned}", crate::i18n::t("plugin_pinned")))
                .size(10)
                .color(OryxisColors::t().text_muted),
        );
    }

    // Action buttons, per status.
    let mut actions: Vec<Element<'_, Message>> = Vec::new();
    match &entry.status {
        PluginUiStatus::NotInstalled => {
            actions.push(pill_button(
                crate::i18n::t("plugin_action_install"),
                Some(Message::ShowPluginInstallModal(id.clone())),
                OryxisColors::t().accent,
                true,
            ));
        }
        PluginUiStatus::UpdateAvailable { .. } => {
            actions.push(pill_button(
                crate::i18n::t("plugin_action_update"),
                Some(Message::PluginInstall(id.clone())),
                OryxisColors::t().accent,
                true,
            ));
            actions.push(Space::new().width(8).into());
            actions.push(pill_button(
                crate::i18n::t("plugin_action_uninstall"),
                Some(Message::PluginUninstall(id.clone())),
                OryxisColors::t().error,
                false,
            ));
        }
        PluginUiStatus::Installed(_) => {
            actions.push(pill_button(
                crate::i18n::t("plugin_action_check_updates"),
                Some(Message::PluginCheckUpdates(id.clone())),
                OryxisColors::t().text_secondary,
                false,
            ));
            actions.push(Space::new().width(8).into());
            actions.push(pill_button(
                crate::i18n::t("plugin_action_uninstall"),
                Some(Message::PluginUninstall(id.clone())),
                OryxisColors::t().error,
                false,
            ));
        }
        PluginUiStatus::DevBuild => {
            // No "Check for updates" here: a locally built dev binary
            // can't be updated by this panel, so the button was just
            // noise repeated on every card. Only the cached downloads it
            // shadows (and the MCP launcher copy) are removable.
            if entry.cached_install {
                actions.push(pill_button(
                    crate::i18n::t("plugin_action_remove_downloads"),
                    Some(Message::PluginUninstall(id.clone())),
                    OryxisColors::t().error,
                    false,
                ));
            }
        }
        PluginUiStatus::Failed(_) => {
            actions.push(pill_button(
                crate::i18n::t("plugin_action_retry"),
                Some(Message::PluginCheckUpdates(id.clone())),
                OryxisColors::t().text_secondary,
                false,
            ));
            actions.push(Space::new().width(8).into());
            actions.push(pill_button(
                crate::i18n::t("plugin_action_install"),
                Some(Message::ShowPluginInstallModal(id.clone())),
                OryxisColors::t().accent,
                false,
            ));
        }
        // Checking / Downloading: in-flight, no actions.
        PluginUiStatus::Checking | PluginUiStatus::Downloading => {}
    }

    if !actions.is_empty() {
        card = card.push(Space::new().height(2));
        card = card.push(dir_row(actions).align_y(iced::Alignment::Center));
    }

    // Per-plugin auto-update toggle, only meaningful once installed.
    if matches!(
        entry.status,
        PluginUiStatus::Installed(_) | PluginUiStatus::UpdateAvailable { .. }
    ) {
        card = card.push(Space::new().height(2));
        card = card.push(
            container(crate::widgets::toggle_switch_labeled(
                crate::i18n::t("plugins_auto_update"),
                entry.auto_update,
                Message::PluginToggleAutoUpdate(id.clone(), !entry.auto_update),
            ))
            .width(Length::Fill)
            .align_x(dir_align_x()),
        );
    }

    container(card)
        .padding(Padding { top: 12.0, right: 16.0, bottom: 12.0, left: 16.0 })
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
        })
        .into()
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

