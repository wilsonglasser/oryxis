//! Cloud Accounts panel — lists `CloudProfile` rows and houses the
//! add/edit form.
//!
//! v0.6 PR 3 ships only the AWS profile-auth path. The provider picker
//! exposes Kubernetes for visibility, the Auth picker exposes Access
//! Key + SSO + Kubeconfig — all of those render with an "experimental
//! / coming soon" hint and the Save button stays enabled only for
//! Profile auth.

use iced::border::Radius;
use iced::widget::{
    button, column, container, pick_list, row, scrollable, text, text_input, MouseArea, Space,
};
use iced::widget::button::Status as BtnStatus;
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis, CARD_WIDTH, PANEL_WIDTH};
use crate::i18n::t;
use crate::state::{CloudAuthChoice, CloudDiscoverState, CloudProviderChoice, CloudTestState};
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
                // ⋮ kebab — hover-revealed, mirrors the host / folder
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
        // "+ Host [▾]" split button there — no overlay rendering here.
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

    fn view_cloud_form_panel(&self) -> Element<'_, Message> {
        let is_editing = self.editing_cloud_profile_id.is_some();
        let title = if is_editing {
            t("cloud_edit_account")
        } else {
            t("cloud_new_account")
        };

        let panel_header = container(
            dir_row(vec![
                text(title)
                    .size(18)
                    .color(OryxisColors::t().text_primary)
                    .into(),
                Space::new().width(Length::Fill).into(),
                button(text("\u{00D7}").size(14).color(OryxisColors::t().text_muted))
                    .on_press(Message::HideCloudForm)
                    .padding(Padding {
                        top: 4.0,
                        right: 8.0,
                        bottom: 4.0,
                        left: 8.0,
                    })
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_surface)),
                        border: Border {
                            radius: Radius::from(6.0),
                            ..Default::default()
                        },
                        ..Default::default()
                    })
                    .into(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding {
            top: 20.0,
            right: 20.0,
            bottom: 16.0,
            left: 20.0,
        });

        // ── Provider picker ── (AWS only fully wired in v0.6 PR 3.)
        let provider_options = vec![
            CloudProviderChoice::Aws,
            CloudProviderChoice::K8s,
        ];
        let provider_pick = pick_list(
            Some(self.cloud_form_provider),
            provider_options,
            |c| match c {
                CloudProviderChoice::Aws => "AWS".to_string(),
                CloudProviderChoice::K8s => format!("Kubernetes ({})", t("cloud_k8s_experimental")),
            },
        )
        .on_select(Message::CloudFormProviderChanged)
        .padding(10)
        .style(crate::widgets::rounded_pick_list_style);

        // ── Auth picker ── (only Profile is implemented today.)
        let auth_options = match self.cloud_form_provider {
            CloudProviderChoice::Aws => vec![
                CloudAuthChoice::Profile,
                CloudAuthChoice::AccessKey,
                CloudAuthChoice::Sso,
            ],
            CloudProviderChoice::K8s => vec![CloudAuthChoice::Kubeconfig],
        };
        let auth_pick = pick_list(
            Some(self.cloud_form_auth_kind),
            auth_options,
            |a| match a {
                CloudAuthChoice::Profile => t("cloud_auth_profile").to_string(),
                CloudAuthChoice::AccessKey => format!(
                    "{} ({})",
                    t("cloud_auth_access_key"),
                    t("cloud_coming_soon")
                ),
                CloudAuthChoice::Sso => {
                    format!("{} ({})", t("cloud_auth_sso"), t("cloud_coming_soon"))
                }
                CloudAuthChoice::Kubeconfig => format!(
                    "{} ({})",
                    t("cloud_auth_kubeconfig"),
                    t("cloud_coming_soon")
                ),
            },
        )
        .on_select(Message::CloudFormAuthKindChanged)
        .padding(10)
        .style(crate::widgets::rounded_pick_list_style);

        // ── AWS-specific fields (only meaningful when provider == AWS
        //    && auth_kind == Profile). Shown unconditionally for now —
        //    the form is tiny enough that hiding adds more complexity
        //    than it removes. The save handler will be the gatekeeper.)
        let aws_fields = column![
            text(t("cloud_aws_profile_name"))
                .size(12)
                .color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            text_input("default", &self.cloud_form_aws_profile_name)
                .on_input(Message::CloudFormAwsProfileNameChanged)
                .padding(10)
                .style(crate::widgets::rounded_input_style),
            Space::new().height(14),
            text(t("cloud_aws_region"))
                .size(12)
                .color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            text_input("us-east-1", &self.cloud_form_aws_region)
                .on_input(Message::CloudFormAwsRegionChanged)
                .padding(10)
                .style(crate::widgets::rounded_input_style),
        ];

        // ── Test credentials button + result line ──
        let test_status: Element<'_, Message> = match &self.cloud_form_test_state {
            CloudTestState::Idle => Space::new().height(0).into(),
            CloudTestState::Running => text(t("cloud_test_running"))
                .size(11)
                .color(OryxisColors::t().text_muted)
                .into(),
            CloudTestState::Ok => text(t("cloud_test_ok"))
                .size(11)
                .color(OryxisColors::t().success)
                .into(),
            CloudTestState::Failed(msg) => {
                text(format!("{}: {msg}", t("cloud_test_failed")))
                    .size(11)
                    .color(OryxisColors::t().error)
                    .into()
            }
        };

        let test_button_disabled = matches!(
            self.cloud_form_test_state,
            CloudTestState::Running
        ) || !matches!(self.cloud_form_auth_kind, CloudAuthChoice::Profile);

        let test_btn = {
            let mut btn = button(
                container(
                    text(t("cloud_test_credentials"))
                        .size(13)
                        .color(OryxisColors::t().text_primary),
                )
                .padding(Padding {
                    top: 8.0,
                    right: 0.0,
                    bottom: 8.0,
                    left: 0.0,
                })
                .width(Length::Fill)
                .center_x(Length::Fill),
            )
            .width(Length::Fill)
            .style(|_, _| button::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    radius: Radius::from(8.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            });
            if !test_button_disabled {
                btn = btn.on_press(Message::CloudFormTestCredentials);
            }
            btn
        };

        let form = column![
            text(t("name"))
                .size(12)
                .color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            text_input("prod-aws", &self.cloud_form_label)
                .on_input(Message::CloudFormLabelChanged)
                .padding(10)
                .style(crate::widgets::rounded_input_style),
            Space::new().height(14),
            text(t("cloud_provider"))
                .size(12)
                .color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            provider_pick,
            Space::new().height(14),
            text(t("cloud_auth_method"))
                .size(12)
                .color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            auth_pick,
            Space::new().height(14),
            aws_fields,
            Space::new().height(16),
            test_btn,
            Space::new().height(6),
            test_status,
        ];

        let panel_error: Element<'_, Message> = if let Some(err) = &self.cloud_form_error {
            text(err.clone())
                .size(11)
                .color(OryxisColors::t().error)
                .into()
        } else {
            Space::new().height(0).into()
        };

        let save_btn = button(
            container(
                text(t("save"))
                    .size(13)
                    .color(OryxisColors::t().text_primary),
            )
            .padding(Padding {
                top: 10.0,
                right: 0.0,
                bottom: 10.0,
                left: 0.0,
            })
            .width(Length::Fill)
            .center_x(Length::Fill),
        )
        .on_press(Message::SaveCloudProfile)
        .width(Length::Fill)
        .style(|_, _| button::Style {
            background: Some(Background::Color(OryxisColors::t().accent)),
            border: Border {
                radius: Radius::from(8.0),
                ..Default::default()
            },
            ..Default::default()
        });

        let mut bottom = column![save_btn];
        if let Some(edit_id) = self.editing_cloud_profile_id {
            let del_btn = button(
                container(
                    text(t("delete"))
                        .size(13)
                        .color(OryxisColors::t().error),
                )
                .padding(Padding {
                    top: 10.0,
                    right: 0.0,
                    bottom: 10.0,
                    left: 0.0,
                })
                .width(Length::Fill)
                .center_x(Length::Fill),
            )
            .on_press(Message::DeleteCloudProfile(edit_id))
            .width(Length::Fill)
            .style(|_, _| button::Style {
                background: Some(Background::Color(Color::TRANSPARENT)),
                border: Border {
                    radius: Radius::from(8.0),
                    color: OryxisColors::t().error,
                    width: 1.0,
                },
                ..Default::default()
            });
            bottom = bottom.push(Space::new().height(8));
            bottom = bottom.push(del_btn);
        }

        let panel_content = column![
            panel_header,
            container(
                column![
                    scrollable(form).height(Length::Fill),
                    Space::new().height(12),
                    panel_error,
                    Space::new().height(8),
                    bottom,
                ]
                .height(Length::Fill),
            )
            .padding(Padding {
                top: 0.0,
                right: 20.0,
                bottom: 20.0,
                left: 20.0,
            })
            .height(Length::Fill),
        ]
        .height(Length::Fill);

        container(panel_content)
            .width(PANEL_WIDTH)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                border: Border {
                    color: OryxisColors::t().border,
                    width: 1.0,
                    radius: Radius::from(0.0),
                },
                ..Default::default()
            })
            .into()
    }
}

impl Oryxis {
    pub(crate) fn view_cloud_discover_panel(&self) -> Element<'_, Message> {
        // Header: title + small refresh-icon button + close (X). The
        // refresh icon lives to the left of the close so the layout
        // mirrors the "title — actions — close" idiom of the host
        // editor panel; both header buttons share the same square chip
        // style so they read as a paired action group.
        let icon_btn_style = |_: &iced::Theme, status: BtnStatus| {
            let bg = match status {
                BtnStatus::Hovered => OryxisColors::t().bg_hover,
                _ => OryxisColors::t().bg_surface,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border {
                    radius: Radius::from(6.0),
                    ..Default::default()
                },
                ..Default::default()
            }
        };
        let refresh_icon_btn = button(
            iced_fonts::lucide::refresh_cw()
                .size(13)
                .color(OryxisColors::t().text_muted),
        )
        .on_press(Message::CloudDiscoverRefresh)
        .padding(Padding {
            top: 4.0,
            right: 8.0,
            bottom: 4.0,
            left: 8.0,
        })
        .style(icon_btn_style);
        let close_btn = button(text("\u{00D7}").size(14).color(OryxisColors::t().text_muted))
            .on_press(Message::HideCloudDiscover)
            .padding(Padding {
                top: 4.0,
                right: 8.0,
                bottom: 4.0,
                left: 8.0,
            })
            .style(icon_btn_style);
        let title = container(
            dir_row(vec![
                text(t("cloud_discover"))
                    .size(18)
                    .color(OryxisColors::t().text_primary)
                    .into(),
                Space::new().width(Length::Fill).into(),
                refresh_icon_btn.into(),
                Space::new().width(6).into(),
                close_btn.into(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding {
            top: 20.0,
            right: 20.0,
            bottom: 8.0,
            left: 20.0,
        });

        // Search bar — only meaningful when results are loaded, but
        // we render it always so the panel layout doesn't shift when
        // the state transitions.
        let search = container(
            text_input(t("cloud_discover_search_ph"), &self.cloud_discover_filter)
                .on_input(Message::CloudDiscoverFilterChanged)
                .padding(Padding {
                    top: 8.0,
                    right: 10.0,
                    bottom: 8.0,
                    left: 10.0,
                })
                .style(crate::widgets::rounded_input_style),
        )
        .padding(Padding {
            top: 0.0,
            right: 20.0,
            bottom: 12.0,
            left: 20.0,
        });

        // Body content varies by state — keep each branch self-
        // contained so the layout above stays readable.
        let body: Element<'_, Message> = match &self.cloud_discover_state {
            CloudDiscoverState::Idle => Space::new().height(0).into(),
            CloudDiscoverState::Running => container(
                text(t("cloud_discover_running"))
                    .size(13)
                    .color(OryxisColors::t().text_muted),
            )
            .center(Length::Fill)
            .into(),
            CloudDiscoverState::Failed(msg) => container(
                column![
                    text(format!("{}: {msg}", t("cloud_test_failed")))
                        .size(13)
                        .color(OryxisColors::t().error),
                    Space::new().height(12),
                    button(
                        container(
                            text(t("cloud_discover_refresh"))
                                .size(12)
                                .color(OryxisColors::t().text_primary),
                        )
                        .padding(Padding {
                            top: 6.0,
                            right: 12.0,
                            bottom: 6.0,
                            left: 12.0,
                        }),
                    )
                    .on_press(Message::CloudDiscoverRefresh)
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_surface)),
                        border: Border {
                            radius: Radius::from(6.0),
                            color: OryxisColors::t().border,
                            width: 1.0,
                        },
                        ..Default::default()
                    }),
                ]
                .align_x(iced::Alignment::Center),
            )
            .center(Length::Fill)
            .into(),
            CloudDiscoverState::Loaded(result) => self.view_discover_result_body(result),
        };

        // Footer: action buttons. Disabled / enabled depending on what
        // the current state allows. We re-render every frame so the
        // selection counter stays live.
        let import_count = self.cloud_discover_selected_ec2.len()
            + self.cloud_discover_selected_ecs.len();
        let can_import = matches!(
            self.cloud_discover_state,
            CloudDiscoverState::Loaded(_)
        ) && import_count > 0;

        let import_btn = {
            let label = if import_count == 0 {
                t("cloud_discover_import_none").to_string()
            } else {
                format!("{} {import_count}", t("cloud_discover_import_n"))
            };
            let mut b = button(
                container(
                    text(label)
                        .size(13)
                        .color(OryxisColors::t().text_primary),
                )
                .padding(Padding {
                    top: 10.0,
                    right: 0.0,
                    bottom: 10.0,
                    left: 0.0,
                })
                .width(Length::Fill)
                .center_x(Length::Fill),
            )
            .width(Length::Fill)
            .style(move |_, _| button::Style {
                background: Some(Background::Color(if can_import {
                    OryxisColors::t().accent
                } else {
                    OryxisColors::t().bg_surface
                })),
                border: Border {
                    radius: Radius::from(8.0),
                    ..Default::default()
                },
                ..Default::default()
            });
            if can_import {
                b = b.on_press(Message::CloudDiscoverImport);
            }
            b
        };

        // Refresh moved to a header icon — footer carries only the
        // Import action now (the previous wide Refresh button at the
        // bottom was redundant).
        let footer = column![import_btn];

        let panel_content = column![
            title,
            search,
            container(body).height(Length::Fill).padding(Padding {
                top: 0.0,
                right: 20.0,
                bottom: 8.0,
                left: 20.0,
            }),
            container(footer).padding(Padding {
                top: 0.0,
                right: 20.0,
                bottom: 20.0,
                left: 20.0,
            }),
        ]
        .height(Length::Fill);

        container(panel_content)
            .width(PANEL_WIDTH)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                border: Border {
                    color: OryxisColors::t().border,
                    width: 1.0,
                    radius: Radius::from(0.0),
                },
                ..Default::default()
            })
            .into()
    }

    /// Render the EC2 (and future ECS / K8s) section of the loaded
    /// discovery result. Already-imported instances are shown but
    /// disabled — the user can tell what's new at a glance.
    fn view_discover_result_body(
        &self,
        result: &oryxis_cloud::DiscoveryResult,
    ) -> Element<'_, Message> {
        if result.ec2.is_empty() && result.ecs_services.is_empty() {
            return container(
                text(t("cloud_discover_no_results"))
                    .size(13)
                    .color(OryxisColors::t().text_muted),
            )
            .center(Length::Fill)
            .into();
        }

        // Index of currently-imported (profile, instance_id) pairs so
        // we can grey out duplicates instead of letting the user
        // re-import them.
        let already: std::collections::HashSet<String> = self
            .connections
            .iter()
            .filter_map(|c| {
                let cr = c.cloud_ref.as_ref()?;
                if Some(cr.profile_id) == self.cloud_discover_profile_id {
                    Some(cr.resource_id.clone())
                } else {
                    None
                }
            })
            .collect();

        // Apply the live filter — case-insensitive substring match
        // across name, instance-id, region, public/private DNS+IP.
        // The total count above the section reflects unfiltered size
        // so the user sees how much got hidden vs. the raw discovery
        // total.
        let needle = self.cloud_discover_filter.trim().to_lowercase();
        let matches_filter = |e: &oryxis_cloud::DiscoveredEc2| -> bool {
            if needle.is_empty() {
                return true;
            }
            let mut hay = String::new();
            if let Some(n) = &e.name { hay.push_str(n); hay.push(' '); }
            hay.push_str(&e.instance_id);
            hay.push(' ');
            hay.push_str(&e.region);
            for v in [&e.public_dns, &e.public_ip, &e.private_dns, &e.private_ip]
                .iter()
                .copied()
                .flatten()
            {
                hay.push(' ');
                hay.push_str(v);
            }
            hay.to_lowercase().contains(&needle)
        };

        // Group EC2 by region so the user sees the cloud's natural
        // boundary instead of an undifferentiated flat list.
        let mut by_region: std::collections::BTreeMap<String, Vec<&oryxis_cloud::DiscoveredEc2>> =
            std::collections::BTreeMap::new();
        let mut filtered_count = 0usize;
        for e in &result.ec2 {
            if matches_filter(e) {
                by_region.entry(e.region.clone()).or_default().push(e);
                filtered_count += 1;
            }
        }

        let mut sections: Vec<Element<'_, Message>> = Vec::new();
        // Hide the EC2 section entirely when zero entries match —
        // showing an empty header reads as broken / loading. Same
        // policy applies to ECS below. The "no matches" hint at the
        // very bottom catches the case where every section is empty.
        let ec2_collapsed = self.cloud_discover_collapsed.contains("ec2");
        let show_ec2_section = filtered_count > 0;
        if show_ec2_section {
            let header_text = if needle.is_empty() {
                format!("EC2 ({})", result.ec2.len())
            } else {
                format!("EC2 ({} / {})", filtered_count, result.ec2.len())
            };
            sections.push(section_header("ec2", &header_text, ec2_collapsed));
            sections.push(Space::new().height(6).into());
        }

        if !show_ec2_section || ec2_collapsed {
            // Skip rendering EC2 rows entirely — header alone wraps
            // the section, the rest of the panel reflows to give the
            // collapsed state real space-saving value.
        } else {
        for (region, items) in by_region {
            sections.push(
                text(format!("📍 {region}"))
                    .size(11)
                    .color(OryxisColors::t().text_muted)
                    .into(),
            );
            sections.push(Space::new().height(4).into());
            for e in items {
                let is_imported = already.contains(&e.instance_id);
                let checked = self.cloud_discover_selected_ec2.contains(&e.instance_id);
                let id_for_msg = e.instance_id.clone();
                let label_text = match (&e.name, e.public_dns.as_deref().or(e.public_ip.as_deref()))
                {
                    (Some(name), Some(addr)) => format!("{name}  ({})  {addr}", e.instance_id),
                    (Some(name), None) => format!("{name}  ({})", e.instance_id),
                    (None, Some(addr)) => format!("{}  {addr}", e.instance_id),
                    (None, None) => e.instance_id.clone(),
                };
                let label_text = if is_imported {
                    format!("{label_text}  ·  {}", t("cloud_discover_already_imported"))
                } else {
                    label_text
                };
                let row_el: Element<'_, Message> = if is_imported {
                    text(label_text)
                        .size(11)
                        .color(OryxisColors::t().text_muted)
                        .into()
                } else {
                    let mark = if checked {
                        iced_fonts::lucide::circle_check()
                            .size(13)
                            .color(OryxisColors::t().accent)
                    } else {
                        iced_fonts::lucide::circle_minus()
                            .size(13)
                            .color(OryxisColors::t().text_muted)
                    };
                    button(
                        row![
                            mark,
                            Space::new().width(8),
                            text(label_text)
                                .size(11)
                                .color(OryxisColors::t().text_secondary),
                        ]
                        .align_y(iced::Alignment::Center),
                    )
                    .on_press(Message::CloudDiscoverToggleEc2(id_for_msg))
                    .padding(Padding {
                        top: 3.0,
                        right: 6.0,
                        bottom: 3.0,
                        left: 4.0,
                    })
                    .style(|_, status| {
                        let bg = match status {
                            BtnStatus::Hovered => OryxisColors::t().bg_hover,
                            _ => Color::TRANSPARENT,
                        };
                        button::Style {
                            background: Some(Background::Color(bg)),
                            border: Border {
                                radius: Radius::from(4.0),
                                ..Default::default()
                            },
                            ..Default::default()
                        }
                    })
                    .into()
                };
                sections.push(row_el);
                sections.push(Space::new().height(2).into());
            }
            sections.push(Space::new().height(8).into());
        }
        } // end `if !ec2_collapsed` block

        // ── ECS section ──
        // ECS services are imported as *dynamic groups* (one per
        // service+container) rather than individual hosts, since
        // task IDs are ephemeral. Already-imported services greyed
        // out so the user doesn't dupe them.
        let already_ecs: std::collections::HashSet<String> = self
            .groups
            .iter()
            .filter_map(|g| {
                let q = g.cloud_query.as_ref()?;
                if q.profile_id != self.cloud_discover_profile_id? {
                    return None;
                }
                match &q.kind {
                    oryxis_core::models::cloud::CloudQueryKind::EcsTasks {
                        cluster,
                        service,
                        container,
                    } => Some(format!("{cluster}/{service}/{container}")),
                    _ => None,
                }
            })
            .collect();

        let ecs_match_filter = |s: &oryxis_cloud::DiscoveredEcsService| -> bool {
            if needle.is_empty() {
                return true;
            }
            let hay = format!(
                "{} {} {} {}",
                s.cluster, s.service, s.container, s.region
            )
            .to_lowercase();
            hay.contains(&needle)
        };

        let ecs_filtered: Vec<&oryxis_cloud::DiscoveredEcsService> = result
            .ecs_services
            .iter()
            .filter(|s| ecs_match_filter(s))
            .collect();

        // Same auto-hide policy as EC2: only emit the ECS section if
        // there's at least one entry surviving the filter.
        if !ecs_filtered.is_empty() {
            sections.push(Space::new().height(8).into());
            let ecs_header = if needle.is_empty() {
                format!("ECS Services ({})", result.ecs_services.len())
            } else {
                format!(
                    "ECS Services ({} / {})",
                    ecs_filtered.len(),
                    result.ecs_services.len()
                )
            };
            let ecs_collapsed = self.cloud_discover_collapsed.contains("ecs");
            sections.push(section_header("ecs", &ecs_header, ecs_collapsed));
            sections.push(Space::new().height(6).into());

            if ecs_collapsed {
                // collapsed — skip body
            } else {

            // Group by region → cluster so the user reads
            // `📍 region / 🗂 cluster` then services. Tasks are
            // ephemeral; the import unit is the (service, container)
            // pair, which becomes a dynamic Group.
            let mut by_region_cluster: std::collections::BTreeMap<
                (String, String),
                Vec<&oryxis_cloud::DiscoveredEcsService>,
            > = std::collections::BTreeMap::new();
            for s in &ecs_filtered {
                by_region_cluster
                    .entry((s.region.clone(), s.cluster.clone()))
                    .or_default()
                    .push(s);
            }

            for ((region, cluster), items) in by_region_cluster {
                sections.push(
                    text(format!("📍 {region}  ·  {cluster}"))
                        .size(11)
                        .color(OryxisColors::t().text_muted)
                        .into(),
                );
                sections.push(Space::new().height(4).into());
                for s in items {
                    let key = format!("{}/{}/{}", s.cluster, s.service, s.container);
                    let is_imported = already_ecs.contains(&key);
                    let checked = self.cloud_discover_selected_ecs.contains(&key);
                    let label_text = format!(
                        "{} / {}  ·  {} task(s)",
                        s.service, s.container, s.running_task_count
                    );
                    let label_text = if is_imported {
                        format!("{label_text}  ·  {}", t("cloud_discover_already_imported"))
                    } else {
                        label_text
                    };
                    let row_el: Element<'_, Message> = if is_imported {
                        text(label_text)
                            .size(11)
                            .color(OryxisColors::t().text_muted)
                            .into()
                    } else {
                        let mark = if checked {
                            iced_fonts::lucide::circle_check()
                                .size(13)
                                .color(OryxisColors::t().accent)
                        } else {
                            iced_fonts::lucide::circle_minus()
                                .size(13)
                                .color(OryxisColors::t().text_muted)
                        };
                        let key_for_msg = key.clone();
                        button(
                            row![
                                mark,
                                Space::new().width(8),
                                text(label_text)
                                    .size(11)
                                    .color(OryxisColors::t().text_secondary),
                            ]
                            .align_y(iced::Alignment::Center),
                        )
                        .on_press(Message::CloudDiscoverToggleEcs(key_for_msg))
                        .padding(Padding {
                            top: 3.0,
                            right: 6.0,
                            bottom: 3.0,
                            left: 4.0,
                        })
                        .style(|_, status| {
                            let bg = match status {
                                BtnStatus::Hovered => OryxisColors::t().bg_hover,
                                _ => Color::TRANSPARENT,
                            };
                            button::Style {
                                background: Some(Background::Color(bg)),
                                border: Border {
                                    radius: Radius::from(4.0),
                                    ..Default::default()
                                },
                                ..Default::default()
                            }
                        })
                        .into()
                    };
                    sections.push(row_el);
                    sections.push(Space::new().height(2).into());
                }
                sections.push(Space::new().height(8).into());
            }
            } // end `if !ecs_collapsed` block
        }

        // Both sections hid themselves under the active filter — show
        // a friendly hint instead of an empty scroll area so the
        // panel doesn't read as "broken".
        if !show_ec2_section && ecs_filtered.is_empty() && !needle.is_empty() {
            sections.push(
                container(
                    text(t("cloud_discover_no_matches"))
                        .size(12)
                        .color(OryxisColors::t().text_muted),
                )
                .center_x(Length::Fill)
                .padding(Padding {
                    top: 24.0,
                    right: 0.0,
                    bottom: 0.0,
                    left: 0.0,
                })
                .into(),
            );
        }

        scrollable(column(sections)).height(Length::Fill).into()
    }
}

/// Collapsible section header — chevron + label, the whole row is a
/// click target that toggles `cloud_discover_collapsed[key]`. Same
/// chevron convention used by file trees: down = expanded, right =
/// collapsed.
fn section_header<'a>(key: &'static str, label: &str, collapsed: bool) -> Element<'a, Message> {
    let chevron = if collapsed {
        iced_fonts::lucide::chevron_right::<iced::Theme, iced::Renderer>()
    } else {
        iced_fonts::lucide::chevron_down::<iced::Theme, iced::Renderer>()
    };
    button(
        row![
            chevron.size(12).color(OryxisColors::t().text_muted),
            Space::new().width(6),
            text(label.to_owned())
                .size(13)
                .color(OryxisColors::t().text_secondary),
        ]
        .align_y(iced::Alignment::Center),
    )
    .on_press(Message::CloudDiscoverToggleSection(key.to_string()))
    .padding(Padding {
        top: 4.0,
        right: 6.0,
        bottom: 4.0,
        left: 4.0,
    })
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().bg_hover,
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                radius: Radius::from(4.0),
                ..Default::default()
            },
            ..Default::default()
        }
    })
    .into()
}

// `CloudProviderChoice` and `CloudAuthChoice` need `Display` for
// `pick_list`'s default mapper, but we use the closure form above so
// these are unused here. Kept the imports tight in this module.
impl std::fmt::Display for CloudProviderChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Aws => write!(f, "AWS"),
            Self::K8s => write!(f, "Kubernetes"),
        }
    }
}

impl std::fmt::Display for CloudAuthChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Profile => write!(f, "Profile"),
            Self::AccessKey => write!(f, "Access Key"),
            Self::Sso => write!(f, "SSO"),
            Self::Kubeconfig => write!(f, "Kubeconfig"),
        }
    }
}
