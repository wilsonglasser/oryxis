//! Add / edit wizard for a `CloudProfile`. Renders a right-side panel
//! with provider + auth pickers and the per-auth-kind input fields,
//! plus a "Test credentials" button and the save / delete actions at
//! the bottom.

use iced::border::Radius;
use iced::widget::{button, column, container, pick_list, scrollable, text, text_input, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis, PANEL_WIDTH};
use crate::i18n::t;
use crate::state::{CloudAuthChoice, CloudProviderChoice, CloudTestState};
use crate::theme::OryxisColors;
use crate::widgets::dir_row;

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

        // ── Provider picker ── AWS-only in v0.6. Kubernetes ships
        // in v0.7 as its own crate; keeping it out of the picker
        // until then avoids surfacing dead options.
        let provider_options = vec![CloudProviderChoice::Aws];
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
                // K8s lives in v0.7, kubeconfig path isn't wired yet.
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

        // Region is shared across all AWS auth kinds, workload region.
        let region_field = column![
            text(t("cloud_aws_region"))
                .size(12)
                .color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            text_input("us-east-1", &self.cloud_form_aws_region)
                .on_input(Message::CloudFormAwsRegionChanged)
                .padding(10)
                .style(crate::widgets::rounded_input_style),
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
                    .style(crate::widgets::rounded_input_style),
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
                        .style(crate::widgets::rounded_input_style),
                    Space::new().height(14),
                    text(t("cloud_aws_access_key_secret"))
                        .size(12)
                        .color(OryxisColors::t().text_secondary),
                    Space::new().height(4),
                    text_input(secret_placeholder, &self.cloud_form_aws_access_key_secret)
                        .on_input(Message::CloudFormAwsAccessKeySecretChanged)
                        .secure(!self.cloud_form_aws_access_key_secret_visible)
                        .padding(10)
                        .style(crate::widgets::rounded_input_style),
                    Space::new().height(14),
                    text(t("cloud_aws_access_key_session_token"))
                        .size(12)
                        .color(OryxisColors::t().text_secondary),
                    Space::new().height(4),
                    text_input(t("cloud_aws_access_key_session_token_ph"), &self.cloud_form_aws_access_key_session_token)
                        .on_input(Message::CloudFormAwsAccessKeySessionTokenChanged)
                        .padding(10)
                        .style(crate::widgets::rounded_input_style),
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
                    .style(crate::widgets::rounded_input_style),
                Space::new().height(14),
                text(t("cloud_aws_sso_region"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                text_input("us-east-1", &self.cloud_form_aws_sso_region)
                    .on_input(Message::CloudFormAwsSsoRegionChanged)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style),
                Space::new().height(14),
                text(t("cloud_aws_sso_account_id"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                text_input("123456789012", &self.cloud_form_aws_sso_account_id)
                    .on_input(Message::CloudFormAwsSsoAccountIdChanged)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style),
                Space::new().height(14),
                text(t("cloud_aws_sso_role_name"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                text_input("AdministratorAccess", &self.cloud_form_aws_sso_role_name)
                    .on_input(Message::CloudFormAwsSsoRoleNameChanged)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style),
                Space::new().height(14),
                region_field,
                Space::new().height(8),
                text(t("cloud_aws_sso_hint"))
                    .size(11)
                    .color(OryxisColors::t().text_muted),
            ]
            .into(),
            CloudAuthChoice::Kubeconfig => column![
                text(t("cloud_coming_soon"))
                    .size(12)
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

        let test_button_disabled = matches!(
            self.cloud_form_test_state,
            CloudTestState::Running
        ) || matches!(self.cloud_form_auth_kind, CloudAuthChoice::Kubeconfig);

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
