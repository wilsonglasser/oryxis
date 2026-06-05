//! Add / edit wizard for a `CloudProfile`. Renders a right-side panel
//! with provider + auth pickers and the per-auth-kind input fields,
//! plus a "Test credentials" button and the save / delete actions at
//! the bottom.

use iced::border::Radius;
use iced::widget::{button, column, container, pick_list, row, scrollable, text, text_input, Row, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis, PANEL_WIDTH};
use crate::i18n::t;
use crate::state::{CloudAuthChoice, CloudProviderChoice, CloudTestState};
use crate::theme::OryxisColors;
use crate::widgets::{dir_align_x, dir_row};

impl Oryxis {
    pub(super) fn view_cloud_form_panel(&self) -> Element<'_, Message> {
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

        // ── Provider picker ── AWS + Kubernetes.
        let provider_options = vec![CloudProviderChoice::Aws, CloudProviderChoice::K8s];
        let provider_pick = pick_list(
            Some(self.cloud_form_provider),
            provider_options,
            |c| match c {
                CloudProviderChoice::Aws => "AWS".to_string(),
                CloudProviderChoice::K8s => "Kubernetes".to_string(),
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
                CloudAuthChoice::AccessKey => t("cloud_auth_access_key").to_string(),
                CloudAuthChoice::Sso => t("cloud_auth_sso").to_string(),
                CloudAuthChoice::Kubeconfig => t("cloud_auth_kubeconfig").to_string(),
            },
        )
        .on_select(Message::CloudFormAuthKindChanged)
        .padding(10)
        .style(crate::widgets::rounded_pick_list_style);

        // Workload regions, chip list shared across all AWS auth kinds.
        // First chip = default region for single-region API calls; the
        // full list drives discovery fan-out. SSO has its own
        // `sso_region` separately (the IdC endpoint, not workload).
        let chips: Vec<Element<'_, Message>> = self
            .cloud_form_aws_regions
            .iter()
            .enumerate()
            .map(|(i, r)| region_chip(r.as_str(), i))
            .collect();
        let chips_block: Element<'_, Message> = if chips.is_empty() {
            Space::new().height(0).into()
        } else {
            // Plain Row, not dir_row, the chips are content-flow not
            // structural layout and don't need to mirror under RTL.
            container(Row::with_children(chips).spacing(6))
                .padding(Padding {
                    top: 0.0,
                    right: 0.0,
                    bottom: 6.0,
                    left: 0.0,
                })
                .into()
        };
        let region_field = column![
            text(t("cloud_aws_regions"))
                .size(12)
                .color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            chips_block,
            text_input("us-east-1", &self.cloud_form_aws_region_draft)
                .on_input(Message::CloudFormAwsRegionDraftChanged)
                .on_submit(Message::CloudFormAwsRegionAdd)
                .padding(10)
                .style(crate::widgets::rounded_input_style)
                .align_x(dir_align_x()),
            Space::new().height(4),
            text(t("cloud_aws_regions_hint"))
                .size(10)
                .color(OryxisColors::t().text_muted),
        ];

        // Auth-kind-specific fields. We render only the ones that
        // apply to the current pick so the form doesn't sprawl with
        // irrelevant inputs.
        let aws_fields: Element<'_, Message> = match self.cloud_form_auth_kind {
            CloudAuthChoice::Profile => column![
                text(t("cloud_aws_profile_name"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                text_input("default", &self.cloud_form_aws_profile_name)
                    .on_input(Message::CloudFormAwsProfileNameChanged)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
                Space::new().height(14),
                region_field,
            ]
            .into(),
            CloudAuthChoice::AccessKey => {
                let secret_placeholder = if self.cloud_form_aws_has_existing_secret {
                    t("cloud_aws_access_key_secret_kept")
                } else {
                    t("cloud_aws_access_key_secret_ph")
                };
                column![
                    text(t("cloud_aws_access_key_id"))
                        .size(12)
                        .color(OryxisColors::t().text_secondary),
                    Space::new().height(4),
                    text_input("AKIAIOSFODNN7EXAMPLE", &self.cloud_form_aws_access_key_id)
                        .on_input(Message::CloudFormAwsAccessKeyIdChanged)
                        .padding(10)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
                    Space::new().height(14),
                    text(t("cloud_aws_access_key_secret"))
                        .size(12)
                        .color(OryxisColors::t().text_secondary),
                    Space::new().height(4),
                    text_input(secret_placeholder, &self.cloud_form_aws_access_key_secret)
                        .on_input(Message::CloudFormAwsAccessKeySecretChanged)
                        .secure(!self.cloud_form_aws_access_key_secret_visible)
                        .padding(10)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
                    Space::new().height(14),
                    text(t("cloud_aws_access_key_session_token"))
                        .size(12)
                        .color(OryxisColors::t().text_secondary),
                    Space::new().height(4),
                    text_input(t("cloud_aws_access_key_session_token_ph"), &self.cloud_form_aws_access_key_session_token)
                        .on_input(Message::CloudFormAwsAccessKeySessionTokenChanged)
                        .padding(10)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
                    Space::new().height(14),
                    region_field,
                ]
                .into()
            }
            CloudAuthChoice::Sso => column![
                text(t("cloud_aws_sso_start_url"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                text_input("https://acme.awsapps.com/start", &self.cloud_form_aws_sso_start_url)
                    .on_input(Message::CloudFormAwsSsoStartUrlChanged)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
                Space::new().height(14),
                text(t("cloud_aws_sso_region"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                text_input("us-east-1", &self.cloud_form_aws_sso_region)
                    .on_input(Message::CloudFormAwsSsoRegionChanged)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
                Space::new().height(14),
                text(t("cloud_aws_sso_account_id"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                text_input("123456789012", &self.cloud_form_aws_sso_account_id)
                    .on_input(Message::CloudFormAwsSsoAccountIdChanged)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
                Space::new().height(14),
                text(t("cloud_aws_sso_role_name"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                text_input("AdministratorAccess", &self.cloud_form_aws_sso_role_name)
                    .on_input(Message::CloudFormAwsSsoRoleNameChanged)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
                Space::new().height(14),
                region_field,
                Space::new().height(8),
                text(t("cloud_aws_sso_hint"))
                    .size(11)
                    .color(OryxisColors::t().text_muted),
            ]
            .into(),
            CloudAuthChoice::Kubeconfig => column![
                text(t("cloud_k8s_kubeconfig_path"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                text_input(t("cloud_k8s_kubeconfig_ph"), &self.cloud_form_kubeconfig_path)
                    .on_input(Message::CloudFormKubeconfigPathChanged)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x()),
                Space::new().height(4),
                text(t("cloud_k8s_kubeconfig_hint"))
                    .size(10)
                    .color(OryxisColors::t().text_muted),
                Space::new().height(14),
                text(t("cloud_k8s_context"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                text_input(t("cloud_k8s_context_ph"), &self.cloud_form_context)
                    .on_input(Message::CloudFormContextChanged)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x()),
                Space::new().height(4),
                text(t("cloud_k8s_context_hint"))
                    .size(10)
                    .color(OryxisColors::t().text_muted),
            ]
            .into(),
        };

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

        // Test Credentials shells out to the provider plugin; if it's
        // not installed, the call would fail with a cryptic
        // `BinaryNotFound` error, so block it at the button level and
        // surface the install banner above.
        let plugin_missing = !self.is_plugin_ready(self.cloud_form_provider);
        let test_button_disabled =
            matches!(self.cloud_form_test_state, CloudTestState::Running) || plugin_missing;

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

        // Plugin-missing banner: surfaces *above* every form field
        // when the provider chosen above has no installed plugin, so
        // the user can't fill out the form and then hit a cryptic
        // "binary not found" wall on Test Credentials. K8s is
        // in-process and never triggers this.
        let plugin_banner: Element<'_, Message> = if plugin_missing
            && !matches!(self.cloud_form_provider, CloudProviderChoice::K8s)
        {
            let provider_id_str = match self.cloud_form_provider {
                CloudProviderChoice::Aws => "aws",
                CloudProviderChoice::K8s => "k8s",
            };
            let install_btn = button(
                container(
                    text(t("plugin_action_install"))
                        .size(12)
                        .color(OryxisColors::t().accent),
                )
                .padding(Padding {
                    top: 6.0,
                    right: 14.0,
                    bottom: 6.0,
                    left: 14.0,
                }),
            )
            .on_press(Message::ShowPluginInstallModal(provider_id_str.to_string()))
            .style(|_, _| button::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    radius: Radius::from(6.0),
                    color: OryxisColors::t().accent,
                    width: 1.0,
                },
                ..Default::default()
            });
            let banner: Element<'_, Message> = container(
                column![
                    dir_row(vec![
                        iced_fonts::lucide::circle_alert()
                            .size(14)
                            .color(OryxisColors::t().warning)
                            .into(),
                        Space::new().width(8).into(),
                        text(t("cloud_plugin_missing_title"))
                            .size(13)
                            .color(OryxisColors::t().text_primary)
                            .into(),
                    ])
                    .align_y(iced::Alignment::Center),
                    Space::new().height(4),
                    text(t("cloud_plugin_missing_body"))
                        .size(11)
                        .color(OryxisColors::t().text_secondary),
                    Space::new().height(8),
                    container(install_btn)
                        .width(Length::Fill)
                        .align_x(dir_align_x()),
                ]
                .width(Length::Fill),
            )
            .padding(12)
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(Color {
                    a: 0.10,
                    ..OryxisColors::t().warning
                })),
                border: Border {
                    radius: Radius::from(8.0),
                    color: OryxisColors::t().warning,
                    width: 1.0,
                },
                ..Default::default()
            })
            .into();
            column![banner, Space::new().height(14)].into()
        } else {
            Space::new().into()
        };

        let form = column![
            plugin_banner,
            text(t("name"))
                .size(12)
                .color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            text_input("prod-aws", &self.cloud_form_label)
                .on_input(Message::CloudFormLabelChanged)
                .padding(10)
                .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
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
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());

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
                .height(Length::Fill)
                .width(Length::Fill)
                .align_x(dir_align_x()),
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
            // Standardised side-panel chrome (matches host editor,
            // discovery, dynamic-group editor) so every right-panel
            // editor shares the same background surface.
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
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

fn region_chip(label: &str, idx: usize) -> Element<'_, Message> {
    let accent = OryxisColors::t().accent;
    container(
        row![
            text(label.to_string())
                .size(11)
                .color(OryxisColors::t().text_primary),
            Space::new().width(2),
            button(
                text("\u{00D7}")
                    .size(13)
                    .color(OryxisColors::t().text_muted),
            )
            .padding(Padding {
                top: 0.0,
                right: 6.0,
                bottom: 0.0,
                left: 6.0,
            })
            .on_press(Message::CloudFormAwsRegionRemove(idx))
            .style(|_, _| button::Style {
                background: None,
                ..Default::default()
            }),
        ]
        .align_y(iced::Alignment::Center),
    )
    .padding(Padding {
        top: 2.0,
        right: 0.0,
        bottom: 2.0,
        left: 10.0,
    })
    .style(move |_| container::Style {
        background: Some(Background::Color(Color {
            a: 0.12,
            ..accent
        })),
        border: Border {
            radius: Radius::from(12.0),
            color: Color { a: 0.30, ..accent },
            width: 1.0,
        },
        ..Default::default()
    })
    .into()
}
