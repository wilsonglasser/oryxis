//! Cards list, the toolbar at the top of the Cloud Accounts panel
//! plus the responsive grid of `CloudProfile` cards. Empty state lives
//! here too. The wizard form panel is mounted on the right when
//! `cloud_form_visible` is on.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, scrollable, text, text_input, MouseArea, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis, CARD_WIDTH, PANEL_WIDTH};
use crate::i18n::t;
use crate::theme::OryxisColors;
use crate::widgets::{
    card_grid_columns, dir_align_x, dir_row, distribute_card_grid, panel_section,
    rounded_input_style, toggle_row,
};

impl Oryxis {
    pub(crate) fn view_cloud_accounts(&self) -> Element<'_, Message> {
        let toolbar = container(
            dir_row(vec![
                // Search fills the leading space (hidden + Fill spacer when
                // there are no accounts, so the action stays trailing).
                self.vault_search_field(),
                Space::new().width(10).into(),
                {
                    let fg = OryxisColors::t().button_text;
                    button(
                        container(
                            dir_row(vec![
                                text("+")
                                    .size(13)
                                    .font(iced::Font {
                                        weight: iced::font::Weight::Bold,
                                        ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                                    })
                                    .color(fg)
                                    .into(),
                                Space::new().width(4).into(),
                                text(t("cloud_new_account_btn"))
                                    .size(11)
                                    .font(iced::Font {
                                        weight: iced::font::Weight::Bold,
                                        ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                                    })
                                    .color(fg)
                                    .into(),
                            ])
                            .align_y(iced::Alignment::Center),
                        )
                        .center_y(Length::Fixed(24.0))
                        .padding(Padding {
                            top: 0.0,
                            right: 14.0,
                            bottom: 0.0,
                            left: 14.0,
                        }),
                    )
                    .on_press(Message::ShowCloudForm(None))
                    .style(|_, status| {
                        let bg = match status {
                            BtnStatus::Hovered => OryxisColors::t().button_bg_hover,
                            _ => OryxisColors::t().button_bg,
                        };
                        button::Style {
                            background: Some(Background::Color(bg)),
                            border: Border {
                                radius: Radius::from(6.0),
                                ..Default::default()
                            },
                            ..Default::default()
                        }
                    })
                    .into()
                },
            ])
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding {
            top: 16.0,
            right: 24.0,
            bottom: 16.0,
            left: 24.0,
        })
        .width(Length::Fill);

        let main_content = if !self.any_cloud_provider_installed() {
            // No cloud-provider plugin installed: a static explainer
            // (what Cloud Accounts are for + a route to the Plugins
            // panel to install a provider). Accounts can't function
            // without a provider plugin, so the list and +Account are
            // intentionally replaced rather than shown empty.
            let explainer = container(
                column![
                    container(
                        iced_fonts::lucide::cloud()
                            .size(32)
                            .color(OryxisColors::t().text_muted),
                    )
                    .padding(16)
                    .style(|_| container::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_surface)),
                        border: Border {
                            radius: Radius::from(12.0),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                    Space::new().height(20),
                    text(t("cloud_no_provider_title"))
                        .size(20)
                        .color(OryxisColors::t().text_primary),
                    Space::new().height(8),
                    text(t("cloud_no_provider_desc"))
                        .size(13)
                        .color(OryxisColors::t().text_muted),
                    Space::new().height(24),
                    crate::widgets::cta_button(
                        t("cloud_no_provider_btn").to_string(),
                        Message::ChangeSettingsSection(
                            crate::state::SettingsSection::Plugins,
                        ),
                    ),
                ]
                .align_x(iced::Alignment::Center),
            )
            .center(Length::Fill);

            column![explainer]
                .width(Length::Fill)
                .height(Length::Fill)
        } else if self
            .cloud_profiles
            .iter()
            .all(|p| !self.cloud_provider_installed(&p.provider))
        {
            // At least one provider plugin is installed, but no account
            // belongs to an installed provider (none saved, or all
            // saved accounts target a provider whose plugin was
            // removed). Show the regular empty state + toolbar.
            let empty_state = crate::widgets::empty_state(
                iced_fonts::lucide::cloud()
                    .size(32)
                    .color(OryxisColors::t().text_muted)
                    .into(),
                t("cloud_empty_title").to_string(),
                t("cloud_empty_desc").to_string(),
                Some((
                    t("cloud_new_account_btn").to_string(),
                    Message::ShowCloudForm(None),
                )),
            );

            // No toolbar when empty: search is hidden and the "+ Account"
            // lives in the empty-state CTA (avoids an orphaned button).
            column![empty_state]
                .width(Length::Fill)
                .height(Length::Fill)
        } else {
            let mut cards: Vec<Element<'_, Message>> = Vec::new();
            let needle = self.cloud_search.trim().to_lowercase();
            // Hide accounts whose provider plugin isn't installed; they
            // stay in the vault and reappear when the plugin is back.
            // Also apply the toolbar search needle (label / provider match).
            for cp in self
                .cloud_profiles
                .iter()
                .filter(|p| self.cloud_provider_installed(&p.provider))
                .filter(|p| {
                    needle.is_empty()
                        || p.label.to_lowercase().contains(&needle)
                        || p.provider.to_lowercase().contains(&needle)
                })
            {
                // Brand glyph + brand colour from the bundled SVG set.
                // The icon tile keeps a neutral surface bg so the brand
                // colour reads on the glyph itself instead of fighting
                // with a saturated coloured square.
                let (glyph, brand_color) =
                    crate::os_icon::provider_icon(&cp.provider, OryxisColors::t().accent);
                // Match the host/group cards: a filled avatar in the
                // user's chosen icon shape, brand colour fill, white logo,
                // instead of a one-off bordered surface box.
                let host_style = crate::widgets::resolve_host_icon_style(
                    None,
                    &self.setting_default_host_icon,
                );
                let icon_box = crate::widgets::host_icon(
                    host_style,
                    brand_color,
                    &cp.label,
                    Some(glyph.view(18.0, Color::WHITE)),
                    32.0,
                );

                let provider_label = match cp.provider.as_str() {
                    "aws" => "AWS",
                    "k8s" => "Kubernetes",
                    other => other,
                };

                let cp_id = cp.id;
                // Floating ⋮ kebab in a Stack overlay (trailing corner)
                // so it doesn't take inline width inside the dir_row.
                // Always-mounted with a transparent glyph + no-hover bg
                // when not active so the surrounding MouseArea sees
                // stable child bounds (avoids hover event loop).
                let show_dots = self.hovered_cloud_card == Some(cp_id);
                let rtl = crate::i18n::is_rtl_layout();
                let pad_trailing = 30.0_f32;
                let card_padding = if rtl {
                    Padding { top: 16.0, right: 16.0, bottom: 16.0, left: pad_trailing }
                } else {
                    Padding { top: 16.0, right: pad_trailing, bottom: 16.0, left: 16.0 }
                };

                let card_body = container(
                    dir_row(vec![
                        icon_box,
                        Space::new().width(12).into(),
                        column![
                            text(&cp.label)
                                .size(13)
                                .color(OryxisColors::t().text_primary)
                                .wrapping(iced::widget::text::Wrapping::None),
                            Space::new().height(2),
                            text(format!("{} · {}", provider_label, cp.auth_kind))
                                .size(10)
                                .color(OryxisColors::t().text_muted)
                                .wrapping(iced::widget::text::Wrapping::None),
                        ]
                        .width(Length::Fill)
                        .align_x(crate::widgets::dir_align_x())
                        .clip(true)
                        .into(),
                    ])
                    .align_y(iced::Alignment::Center),
                )
                .padding(card_padding)
                .width(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().bg_surface)),
                    border: Border {
                        radius: Radius::from(10.0),
                        // Accent border on hover, matching host / key cards.
                        color: if show_dots {
                            OryxisColors::t().accent
                        } else {
                            OryxisColors::t().border
                        },
                        width: 1.0,
                    },
                    ..Default::default()
                });

                let dots_glyph_color = if show_dots {
                    OryxisColors::t().text_muted
                } else {
                    Color::TRANSPARENT
                };
                let dots_btn = crate::widgets::card_kebab_button(
                    dots_glyph_color,
                    show_dots,
                    Message::ShowCloudCardMenu(cp_id),
                );
                let dots_align = if rtl {
                    iced::alignment::Horizontal::Left
                } else {
                    iced::alignment::Horizontal::Right
                };
                let dots_pad = if rtl {
                    Padding { top: 0.0, right: 0.0, bottom: 0.0, left: 8.0 }
                } else {
                    Padding { top: 0.0, right: 8.0, bottom: 0.0, left: 0.0 }
                };
                let dots_overlay = container(dots_btn)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(dots_align)
                    .align_y(iced::alignment::Vertical::Center)
                    .padding(dots_pad);
                let card_element: Element<'_, Message> = iced::widget::Stack::new()
                    .push(card_body)
                    .push(dots_overlay)
                    .into();

                let wrapped = MouseArea::new(card_element)
                    .on_enter(Message::CloudCardHovered(cp_id))
                    .on_exit(Message::CloudCardUnhovered)
                    .on_right_press(Message::ShowCloudCardMenu(cp_id));

                let card_el: Element<'_, Message> =
                    container(wrapped).width(Length::Fill).clip(true).into();
                cards.push(self.card_wash(card_el, brand_color));
            }

            let nav_width = self.vault_rail_width();
            let panel_width = if self.cloud_form_visible { PANEL_WIDTH } else { 0.0 };
            let available =
                (self.window_size.width - nav_width - panel_width - 48.0).max(0.0);
            let cols = card_grid_columns(available, CARD_WIDTH, 12.0);
            let cloud_grid = distribute_card_grid(cards, cols, 12.0, 12.0);

            // Cloud Sync settings (auto-refresh / orphan archive) moved
            // to Settings -> Cloud (`view_cloud_sync_settings`); this
            // surface is now just the account grid.
            let grid = scrollable(
                column![cloud_grid].padding(Padding {
                    top: 0.0,
                    right: 24.0,
                    bottom: 24.0,
                    left: 24.0,
                }),
            )
            .height(Length::Fill);

            column![toolbar, grid]
                .width(Length::Fill)
                .height(Length::Fill)
        };

        // The account form panel is hoisted to `view_main`
        // (active_side_panel) so it rises over the sub-nav band.
        main_content.into()
    }

    /// Cloud Sync preferences (auto-refresh interval, orphan
    /// auto-archive). Lives in Settings -> Cloud; the cloud *account*
    /// CRUD moved to the top-level `View::Cloud` surface. Interval /
    /// days inputs accept partial typed input and clamp on commit via
    /// the sanitize helper in the dispatcher.
    pub(crate) fn view_cloud_sync_settings(&self) -> Element<'_, Message> {
        let refresh_interval_input = text_input(
            "30",
            &self.setting_cloud_auto_refresh_interval_minutes,
        )
        .on_input(Message::SettingCloudAutoRefreshIntervalChanged)
        .padding(8)
        .width(120)
        .style(rounded_input_style)
        .align_x(dir_align_x());
        let orphan_days_input = text_input(
            "7",
            &self.setting_cloud_orphan_archive_days,
        )
        .on_input(Message::SettingCloudOrphanArchiveDaysChanged)
        .padding(8)
        .width(120)
        .style(rounded_input_style)
        .align_x(dir_align_x());
        let cloud_sync_settings = panel_section(column![
            // Title dropped (redundant with the settings nav label).
            toggle_row(
                t("settings_cloud_auto_refresh"),
                self.setting_cloud_auto_refresh_enabled,
                Message::SettingCloudAutoRefreshToggle,
            ),
            Space::new().height(8),
            dir_row(vec![
                text(t("settings_cloud_auto_refresh_interval"))
                    .size(12)
                    .color(OryxisColors::t().text_muted)
                    .into(),
                Space::new().width(Length::Fill).into(),
                refresh_interval_input.into(),
            ])
            .align_y(iced::Alignment::Center),
            Space::new().height(14),
            toggle_row(
                t("settings_cloud_auto_archive"),
                self.setting_cloud_auto_archive_orphans,
                Message::SettingCloudAutoArchiveToggle,
            ),
            Space::new().height(8),
            dir_row(vec![
                text(t("settings_cloud_orphan_archive_days"))
                    .size(12)
                    .color(OryxisColors::t().text_muted)
                    .into(),
                Space::new().width(Length::Fill).into(),
                orphan_days_input.into(),
            ])
            .align_y(iced::Alignment::Center),
        ]);

        scrollable(
            container(cloud_sync_settings)
                .padding(Padding { top: 24.0, right: 24.0, bottom: 24.0, left: 24.0 })
                .width(Length::Fill),
        )
        .height(Length::Fill)
        .into()
    }
}
