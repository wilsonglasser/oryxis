//! Right-side panel that edits the `cloud_query.template` of a dynamic
//! group. Mirrors the host editor layout (label header + X, scrollable
//! form, sticky save at bottom) so the visual pattern stays consistent
//! with the rest of the app.

use iced::border::Radius;
use iced::widget::{button, column, container, pick_list, scrollable, text, text_input, Space};
use iced::{Background, Border, Element, Length, Padding};

use crate::app::{Message, Oryxis, PANEL_WIDTH};
use crate::i18n::t;
use crate::theme::OryxisColors;
use crate::widgets::dir_row;

impl Oryxis {
    pub(crate) fn view_dynamic_group_form_panel(&self) -> Element<'_, Message> {
        use oryxis_core::models::cloud::TransportKind;

        // Header, label of the group + close button.
        let group_label = self
            .cloud_dynamic_form_group_id
            .and_then(|gid| self.groups.iter().find(|g| g.id == gid))
            .map(|g| g.label.clone())
            .unwrap_or_default();
        let title = container(
            dir_row(vec![
                column![
                    text(t("cloud_dynamic_form_title"))
                        .size(11)
                        .color(OryxisColors::t().text_muted),
                    Space::new().height(2),
                    text(group_label)
                        .size(16)
                        .color(OryxisColors::t().text_primary),
                ]
                .into(),
                Space::new().width(Length::Fill).into(),
                button(text("\u{00D7}").size(14).color(OryxisColors::t().text_muted))
                    .on_press(Message::HideDynamicGroupForm)
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

        // Transport picker, the four transports that make sense for
        // a dynamic group's children. SFTP-only transports stay out.
        let transports = vec![
            TransportKind::Ssh,
            TransportKind::EcsExec,
            TransportKind::Ssm,
            TransportKind::InstanceConnect,
        ];
        let transport_pick = pick_list(
            Some(self.cloud_dynamic_form_transport),
            transports,
            |t| match t {
                TransportKind::Ssh => "SSH".to_string(),
                TransportKind::InstanceConnect => "EC2 Instance Connect".to_string(),
                TransportKind::Ssm => "SSM Session".to_string(),
                TransportKind::EcsExec => "ECS Exec".to_string(),
                TransportKind::KubectlExec => "kubectl exec".to_string(),
            },
        )
        .on_select(Message::DynamicGroupFormTransportChanged)
        .padding(10)
        .style(crate::widgets::rounded_pick_list_style);

        // Key picker, list available keys + a "(none)" sentinel.
        let key_options = {
            let mut opts = vec!["(none)".to_string()];
            opts.extend(self.keys.iter().map(|k| k.label.clone()));
            opts
        };
        let key_pick = pick_list(
            Some(
                self.cloud_dynamic_form_selected_key
                    .clone()
                    .unwrap_or_else(|| "(none)".to_string()),
            ),
            key_options,
            |s: &String| s.clone(),
        )
        .on_select(Message::DynamicGroupFormKeyChanged)
        .padding(10)
        .style(crate::widgets::rounded_pick_list_style);

        // Identity picker, same shape as keys.
        let identity_options = {
            let mut opts = vec!["(none)".to_string()];
            opts.extend(self.identities.iter().map(|i| i.label.clone()));
            opts
        };
        let identity_pick = pick_list(
            Some(
                self.cloud_dynamic_form_selected_identity
                    .clone()
                    .unwrap_or_else(|| "(none)".to_string()),
            ),
            identity_options,
            |s: &String| s.clone(),
        )
        .on_select(Message::DynamicGroupFormIdentityChanged)
        .padding(10)
        .style(crate::widgets::rounded_pick_list_style);

        let form = column![
            text(t("cloud_dynamic_form_transport"))
                .size(12)
                .color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            transport_pick,
            Space::new().height(14),
            text(t("username"))
                .size(12)
                .color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            text_input("ec2-user", &self.cloud_dynamic_form_username)
                .on_input(Message::DynamicGroupFormUsernameChanged)
                .padding(10)
                .style(crate::widgets::rounded_input_style),
            Space::new().height(14),
            text(t("cloud_dynamic_form_initial_command"))
                .size(12)
                .color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            text_input("exec bash", &self.cloud_dynamic_form_initial_command)
                .on_input(Message::DynamicGroupFormInitialCommandChanged)
                .padding(10)
                .style(crate::widgets::rounded_input_style),
            Space::new().height(14),
            text(t("cloud_dynamic_form_key"))
                .size(12)
                .color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            key_pick,
            Space::new().height(14),
            text(t("cloud_dynamic_form_identity"))
                .size(12)
                .color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            identity_pick,
        ];

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
        .on_press(Message::SaveDynamicGroup)
        .width(Length::Fill)
        .style(|_, _| button::Style {
            background: Some(Background::Color(OryxisColors::t().accent)),
            border: Border {
                radius: Radius::from(8.0),
                ..Default::default()
            },
            ..Default::default()
        });

        let panel_content = column![
            title,
            container(
                column![
                    scrollable(form).height(Length::Fill),
                    Space::new().height(12),
                    save_btn,
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
