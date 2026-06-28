//! Right-side panel that edits the `cloud_query.template` of a dynamic
//! group. Mirrors the host editor layout (label header + X, scrollable
//! form, sticky save at bottom) so the visual pattern stays consistent
//! with the rest of the app.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, pick_list, scrollable, text, text_input, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis, PANEL_WIDTH};
use crate::i18n::t;
use crate::theme::OryxisColors;
use crate::widgets::{dir_align_x, dir_row};

impl Oryxis {
    pub(crate) fn view_dynamic_group_form_panel(&self) -> Element<'_, Message> {
        use oryxis_core::models::cloud::TransportKind;

        // Header: single-line title + chevron-right close, matches
        // the host editor (`view_host_panel`) so every right-side
        // panel reads with the same chrome. The group label appears
        // inside the General section below as the editable Label
        // field instead of doubling as a header subtitle.
        let title = container(
            dir_row(vec![
                text(t("cloud_dynamic_form_title"))
                    .size(16)
                    .color(OryxisColors::t().text_primary)
                    .into(),
                Space::new().width(Length::Fill).into(),
                button(
                    text("\u{00D7}")
                        .size(20)
                        .color(OryxisColors::t().text_muted),
                )
                .on_press(Message::HideDynamicGroupForm)
                .padding(Padding {
                    top: 4.0,
                    right: 8.0,
                    bottom: 4.0,
                    left: 8.0,
                })
                .style(|_, _| button::Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    border: Border::default(),
                    ..Default::default()
                })
                .into(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding {
            top: 16.0,
            right: 16.0,
            bottom: 12.0,
            left: 16.0,
        });

        // Transport picker, the four transports that make sense for
        // a dynamic group's children. SFTP-only transports stay out.
        let transports = vec![
            TransportKind::Ssh,
            TransportKind::EcsExec,
            TransportKind::Ssm,
            TransportKind::InstanceConnect,
            TransportKind::KubectlExec,
        ];
        let transport_pick = pick_list(
            Some(self.cloud_dynamic_form.transport),
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
                self.cloud_dynamic_form.selected_key
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
                self.cloud_dynamic_form.selected_identity
                    .clone()
                    .unwrap_or_else(|| "(none)".to_string()),
            ),
            identity_options,
            |s: &String| s.clone(),
        )
        .on_select(Message::DynamicGroupFormIdentityChanged)
        .padding(10)
        .style(crate::widgets::rounded_pick_list_style);

        // Icon + color preview: same widget shape the host editor
        // uses (a 32 px rounded square with the glyph centred). Click
        // opens the shared icon picker modal pre-filled from the form
        // state; the picker writes the user's choice back to
        // `cloud_dynamic_form.icon` / `_color` instead of persisting
        // straight to the vault, so the form's Save button stays the
        // single commit point.
        let icon_id_for_preview = if self.cloud_dynamic_form.icon.trim().is_empty() {
            "server"
        } else {
            self.cloud_dynamic_form.icon.trim()
        };
        let preview_color = if self.cloud_dynamic_form.color.trim().is_empty() {
            OryxisColors::t().accent
        } else {
            crate::widgets::parse_hex_color(self.cloud_dynamic_form.color.trim())
                .unwrap_or(OryxisColors::t().accent)
        };
        let preview_glyph = crate::os_icon::custom_icon_glyph(icon_id_for_preview);
        let preview_box: Element<'_, Message> = button(
            container(preview_glyph.view(18.0, Color::WHITE))
                .width(Length::Fixed(32.0))
                .height(Length::Fixed(32.0))
                .center_x(Length::Fixed(32.0))
                .center_y(Length::Fixed(32.0)),
        )
        .on_press(Message::ShowIconPickerForDynamicGroupForm)
        .padding(0)
        .style(move |_, status| {
            let ring = match status {
                BtnStatus::Hovered => Color::from_rgba(1.0, 1.0, 1.0, 0.25),
                _ => Color::TRANSPARENT,
            };
            button::Style {
                background: Some(Background::Color(preview_color)),
                border: Border {
                    radius: Radius::from(8.0),
                    color: ring,
                    width: 1.5,
                },
                ..Default::default()
            }
        })
        .into();

        // Parent Group combo, same shape as the host editor: text
        // input + chevron that opens the shared group picker popover.
        // Typing a brand new name still creates the group on Save
        // (existing `DynamicGroupFormParentChanged` path unchanged).
        const PARENT_COMBO_HEIGHT: f32 = 36.0;
        let parent_input = text_input(
            t("group_placeholder"),
            &self.cloud_dynamic_form.parent_label,
        )
        .on_input(Message::DynamicGroupFormParentChanged)
        .padding(10)
        .width(Length::Fill)
        .style(crate::widgets::rounded_input_style)
        .align_x(dir_align_x());
        let parent_chevron = button(
            container(
                iced_fonts::lucide::chevron_down::<iced::Theme, iced::Renderer>()
                    .size(12)
                    .color(OryxisColors::t().text_muted),
            )
            .center_x(Length::Fixed(32.0))
            .center_y(Length::Fixed(PARENT_COMBO_HEIGHT)),
        )
        .on_press(Message::ToggleGroupPicker(
            crate::state::GroupPickerTarget::DynamicFormParent,
        ))
        .padding(0)
        .style(|_, status| {
            let bg = match status {
                BtnStatus::Hovered => OryxisColors::t().bg_hover,
                _ => OryxisColors::t().bg_surface,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border {
                    radius: Radius::from(6.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            }
        });
        let parent_combo: Element<'_, Message> = crate::widgets::bounds_reporter(
            dir_row(vec![
                container(parent_input)
                    .width(Length::Fill)
                    .height(Length::Fixed(PARENT_COMBO_HEIGHT))
                    .into(),
                Space::new().width(6).into(),
                container(parent_chevron)
                    .height(Length::Fixed(PARENT_COMBO_HEIGHT))
                    .into(),
            ])
            .align_y(iced::Alignment::Center),
            self.dynamic_form_parent_combo_bounds.clone(),
        );

        // General section: rename the group, swap icon/color via the
        // shared picker, re-parent under any other user folder. Parity
        // with the host editor so a dynamic group is a first-class
        // entity.
        let general_section = column![
            text(t("cloud_dynamic_form_general"))
                .size(14)
                .color(OryxisColors::t().text_primary),
            Space::new().height(10),
            dir_row(vec![
                preview_box,
                Space::new().width(10).into(),
                text_input("group label", &self.cloud_dynamic_form.label)
                    .on_input(Message::DynamicGroupFormLabelChanged)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x())
                    .into(),
            ])
            .align_y(iced::Alignment::Center),
            Space::new().height(14),
            text(t("parent_group"))
                .size(12)
                .color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            parent_combo,
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());

        // Cloud source section: the query backing the dynamic group
        // (ECS tasks or K8s pods, picked by `cloud_dynamic_form.is_k8s`)
        // followed by the template applied to every transient child the
        // resolver returns.
        // Source query fields differ by kind: ECS groups edit
        // cluster/service/container, K8s groups edit context/namespace and a
        // pod selector (label string or a Deployment/StatefulSet/pod name).
        let kind_fields: Element<'_, Message> = if self.cloud_dynamic_form.is_k8s {
            use crate::state::K8sSelectorKind;
            let selector_kind = self.cloud_dynamic_form.k8s_selector_kind;
            let selector_pick = pick_list(
                Some(selector_kind),
                K8sSelectorKind::ALL.to_vec(),
                |k| k.to_string(),
            )
            .on_select(Message::DynamicGroupFormK8sSelectorKindChanged)
            .padding(10)
            .style(crate::widgets::rounded_pick_list_style);
            // The value field's placeholder + hint adapt to the kind: a
            // label string for `Labels`, a single resource name otherwise.
            let (value_ph, value_hint): (&str, &str) = match selector_kind {
                K8sSelectorKind::Labels => {
                    ("app=nginx,tier=frontend", t("cloud_k8s_selector_hint"))
                }
                K8sSelectorKind::Deployment => ("my-deployment", t("cloud_k8s_name_hint")),
                K8sSelectorKind::StatefulSet => ("my-statefulset", t("cloud_k8s_name_hint")),
                K8sSelectorKind::Name => ("my-pod", t("cloud_k8s_name_hint")),
            };
            column![
                text(t("cloud_k8s_context"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                text_input(t("cloud_k8s_context_ph"), &self.cloud_dynamic_form.k8s_context)
                    .on_input(Message::DynamicGroupFormK8sContextChanged)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x()),
                Space::new().height(14),
                text(t("cloud_k8s_namespace"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                text_input("default", &self.cloud_dynamic_form.namespace)
                    .on_input(Message::DynamicGroupFormNamespaceChanged)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x()),
                Space::new().height(14),
                text(t("cloud_k8s_selector"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                selector_pick,
                Space::new().height(8),
                text_input(value_ph, &self.cloud_dynamic_form.k8s_selector_value)
                    .on_input(Message::DynamicGroupFormK8sSelectorValueChanged)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x()),
                Space::new().height(6),
                text(value_hint)
                    .size(10)
                    .color(OryxisColors::t().text_muted),
            ]
            .width(Length::Fill)
            .align_x(dir_align_x())
            .into()
        } else {
            column![
                text(t("cloud_dynamic_form_cluster"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                text_input("my-cluster", &self.cloud_dynamic_form.cluster)
                    .on_input(Message::DynamicGroupFormClusterChanged)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x()),
                Space::new().height(14),
                text(t("cloud_dynamic_form_service"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                text_input("my-service", &self.cloud_dynamic_form.service)
                    .on_input(Message::DynamicGroupFormServiceChanged)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x()),
                Space::new().height(14),
                text(t("cloud_dynamic_form_container"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                text_input(t("cloud_dynamic_form_container_ph"), &self.cloud_dynamic_form.container)
                    .on_input(Message::DynamicGroupFormContainerChanged)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x()),
                Space::new().height(6),
                text(t("cloud_dynamic_form_query_hint"))
                    .size(10)
                    .color(OryxisColors::t().text_muted),
            ]
            .width(Length::Fill)
            .align_x(dir_align_x())
            .into()
        };

        let source_section = column![
            text(t("cloud_dynamic_form_source"))
                .size(14)
                .color(OryxisColors::t().text_primary),
            Space::new().height(10),
            kind_fields,
            Space::new().height(14),
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
            text_input("ec2-user", &self.cloud_dynamic_form.username)
                .on_input(Message::DynamicGroupFormUsernameChanged)
                .padding(10)
                .style(crate::widgets::rounded_input_style)
                .align_x(dir_align_x()),
            Space::new().height(14),
            text(t("cloud_dynamic_form_initial_command"))
                .size(12)
                .color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            text_input("exec bash", &self.cloud_dynamic_form.initial_command)
                .on_input(Message::DynamicGroupFormInitialCommandChanged)
                .padding(10)
                .style(crate::widgets::rounded_input_style)
                .align_x(dir_align_x()),
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
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());

        // Wrap each section in a `panel_section` card so the visual
        // grouping matches the host editor (cards-on-surface layout
        // instead of loose fields against the panel background).
        let form = column![
            crate::widgets::panel_section(general_section),
            Space::new().height(8),
            crate::widgets::panel_section(source_section),
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());

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

        // Push the right padding INSIDE the scrollable so the
        // scrollbar overlay lands in the panel's empty right margin
        // instead of sitting on top of the input fields. Mirrors the
        // host editor layout so both side panels feel consistent.
        let form_scroll = scrollable(
            column![form]
                .width(Length::Fill)
                .align_x(dir_align_x())
                .padding(Padding {
                    top: 0.0,
                    right: 20.0,
                    bottom: 0.0,
                    left: 20.0,
                }),
        )
        .height(Length::Fill);

        let panel_content = column![
            title,
            form_scroll,
            container(column![Space::new().height(12), save_btn])
                .padding(Padding {
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
            // Standardised side-panel chrome: same `bg_surface` +
            // border as the host editor (`view_host_panel`) so all
            // right-side editors share the same visual frame.
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
