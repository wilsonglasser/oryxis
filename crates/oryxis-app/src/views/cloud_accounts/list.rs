//! Cards list, the toolbar at the top of the Cloud Accounts panel
//! plus the responsive grid of `CloudProfile` cards. Empty state lives
//! here too. The wizard form panel is mounted on the right when
//! `cloud_form_visible` is on.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, scrollable, text, MouseArea, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis, CARD_WIDTH, PANEL_WIDTH};
use crate::i18n::t;
use crate::theme::OryxisColors;
use crate::widgets::{card_grid_columns, dir_row, distribute_card_grid};

impl Oryxis {
    pub(crate) fn view_cloud_accounts(&self) -> Element<'_, Message> {
        let toolbar = container(
            dir_row(vec![
                text(t("cloud_accounts"))
                    .size(20)
                    .color(OryxisColors::t().text_primary)
                    .into(),
                Space::new().width(Length::Fill).into(),
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
            top: 20.0,
            right: 24.0,
            bottom: 16.0,
            left: 24.0,
        })
        .width(Length::Fill);

        let main_content = if self.cloud_profiles.is_empty() {
            let empty_state = container(
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
                    text(t("cloud_empty_title"))
                        .size(20)
                        .color(OryxisColors::t().text_primary),
                    Space::new().height(8),
                    text(t("cloud_empty_desc"))
                        .size(13)
                        .color(OryxisColors::t().text_muted),
                    Space::new().height(24),
                    crate::widgets::cta_button(
                        t("cloud_new_account_btn").to_string(),
                        Message::ShowCloudForm(None),
                    ),
                ]
                .align_x(iced::Alignment::Center),
            )
            .center(Length::Fill);

            column![toolbar, empty_state]
                .width(Length::Fill)
                .height(Length::Fill)
        } else {
            let mut cards: Vec<Element<'_, Message>> = Vec::new();
            for cp in &self.cloud_profiles {
                // Brand glyph + brand colour from the bundled SVG set.
                // The icon tile keeps a neutral surface bg so the brand
                // colour reads on the glyph itself instead of fighting
                // with a saturated coloured square.
                let (glyph, brand_color) =
                    crate::os_icon::provider_icon(&cp.provider, OryxisColors::t().accent);
                let icon_box = container(glyph.view(20.0, brand_color))
                    .center(Length::Fixed(32.0))
                    .style(|_| container::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_surface)),
                        border: Border {
                            radius: Radius::from(8.0),
                            color: OryxisColors::t().border,
                            width: 1.0,
                        },
                        ..Default::default()
                    });

                let provider_label = match cp.provider.as_str() {
                    "aws" => "AWS",
                    "k8s" => "Kubernetes",
                    other => other,
                };

                let cp_id = cp.id;
                // ⋮ kebab, hover-revealed, mirrors the host / folder
                // / identity card pattern. Edit + Delete live behind
                // it so the card body can stay non-interactive (the
                // user shouldn't accidentally do anything destructive
                // by clicking on a profile they only meant to inspect).
                const DOTS_SLOT_W: f32 = 22.0;
                let show_dots = self.hovered_cloud_card == Some(cp_id);
                let dots_btn: Element<'_, Message> = if show_dots {
                    button(text("\u{22EE}").size(14).color(OryxisColors::t().text_muted))
                        .on_press(Message::ShowCloudCardMenu(cp_id))
                        .padding(Padding {
                            top: 1.0,
                            right: 6.0,
                            bottom: 1.0,
                            left: 6.0,
                        })
                        .style(|_, status| {
                            let bg = match status {
                                BtnStatus::Hovered => OryxisColors::t().bg_hover,
                                _ => Color::TRANSPARENT,
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
                } else {
                    Space::new()
                        .width(Length::Fixed(DOTS_SLOT_W))
                        .height(Length::Fixed(1.0))
                        .into()
                };

                let card_body = container(
                    dir_row(vec![
                        icon_box.into(),
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
                        .into(),
                        dots_btn,
                    ])
                    .align_y(iced::Alignment::Center),
                )
                .padding(16)
                .width(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().bg_surface)),
                    border: Border {
                        radius: Radius::from(10.0),
                        color: OryxisColors::t().border,
                        width: 1.0,
                    },
                    ..Default::default()
                });

                let wrapped = MouseArea::new(card_body)
                    .on_enter(Message::CloudCardHovered(cp_id))
                    .on_exit(Message::CloudCardUnhovered)
                    .on_right_press(Message::ShowCloudCardMenu(cp_id));

                cards.push(container(wrapped).width(Length::Fill).clip(true).into());
            }

            let nav_width = if self.sidebar_collapsed {
                crate::app::SIDEBAR_WIDTH_COLLAPSED
            } else {
                crate::app::SIDEBAR_WIDTH
            };
            let panel_width = if self.cloud_form_visible { PANEL_WIDTH } else { 0.0 };
            let available =
                (self.window_size.width - nav_width - panel_width - 48.0).max(0.0);
            let cols = card_grid_columns(available, CARD_WIDTH, 12.0);
            let cloud_grid = distribute_card_grid(cards, cols, 12.0, 12.0);

            let grid = scrollable(column![cloud_grid].padding(Padding {
                top: 0.0,
                right: 24.0,
                bottom: 24.0,
                left: 24.0,
            }))
            .height(Length::Fill);

            column![toolbar, grid]
                .width(Length::Fill)
                .height(Length::Fill)
        };

        // Settings → Cloud is CRUD-only (manage credentials). Discovery
        // / import lives in the Hosts view and is triggered from the
        // "+ Host [▾]" split button there, no overlay rendering here.
        if self.cloud_form_visible {
            let panel = self.view_cloud_form_panel();
            dir_row(vec![main_content.into(), panel])
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            main_content.into()
        }
    }
}
