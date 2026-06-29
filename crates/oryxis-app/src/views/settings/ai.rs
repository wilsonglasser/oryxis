//! Settings -> AI assistant section view. Split out of views/settings/mod.rs.

use super::*;
use iced::widget::column;

impl Oryxis {
    pub(crate) fn view_settings_ai(&self) -> Element<'_, Message> {
        // Enable/disable lives on the Plugins screen now; this
        // section only renders while AI is enabled.
        let mut content_col = column![
            // The assistant runs commands on connected servers
            // (some auto-execute); keep the warning in view.
            text(crate::i18n::t("ai_enable_warning")).size(12).color(OryxisColors::t().text_muted),
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());

        if self.ai.enabled {
            let current_info = crate::ai::provider_info(&self.ai.provider);
            let provider_options: Vec<String> = crate::ai::PROVIDERS
                .iter()
                .map(|p| p.display.to_string())
                .collect();

            let provider_pick: Element<'_, Message> = pick_list(
                Some(current_info.display.to_string()),
                provider_options,
                |s: &String| s.clone(),
            )
            .on_select(Message::AiProviderChanged)
            .width(220)
            .padding(10)
            .style(crate::widgets::rounded_pick_list_style)
            .into();

            let model_input: Element<'_, Message> = text_input(t("ai_model_placeholder"), &self.ai.model)
                .on_input(Message::AiModelChanged)
                .padding(10)
                .width(300)
                .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                .into();

            // When a key is already stored, the input is cleared
            // for security but the placeholder communicates that
            // a key exists, typing replaces it on save.
            let key_placeholder = if self.ai.api_key_set {
                t("ai_key_saved_placeholder")
            } else {
                "sk-..."
            };
            let key_input: Element<'_, Message> = container(
                crate::widgets::password_input_with_eye(
                    key_placeholder,
                    &self.ai.api_key,
                    Message::AiApiKeyChanged,
                    Some(Message::SaveAiApiKey),
                    self.revealed_secrets
                        .contains(&crate::state::SecretField::AiApiKey),
                    Message::ToggleSecretVisibility(
                        crate::state::SecretField::AiApiKey,
                    ),
                    10.0,
                ),
            )
            .width(280)
            .into();
            let save_btn = styled_button(crate::i18n::t("save"), Message::SaveAiApiKey, OryxisColors::t().accent);
            let key_status: Element<'_, Message> = if self.ai.api_key_set {
                dir_row(vec![
                    iced_fonts::lucide::circle_check().size(13).color(OryxisColors::t().success).into(),
                    Space::new().width(6).into(),
                    text(t("api_key_saved")).size(12).color(OryxisColors::t().success).into(),
                ])
                .align_y(iced::Alignment::Center)
                .into()
            } else {
                dir_row(vec![
                    iced_fonts::lucide::circle_alert().size(13).color(OryxisColors::t().text_muted).into(),
                    Space::new().width(6).into(),
                    text(t("no_api_key")).size(12).color(OryxisColors::t().text_muted).into(),
                ])
                .align_y(iced::Alignment::Center)
                .into()
            };

            let mut provider_col = column![
                panel_field(t("provider"), provider_pick),
                Space::new().height(12),
                panel_field(t("model"), model_input),
            ];

            if current_info.kind == crate::ai::ProviderKind::Custom {
                let url_input: Element<'_, Message> = text_input("https://api.example.com/v1/chat/completions", &self.ai.api_url)
                    .on_input(Message::AiApiUrlChanged)
                    .padding(10)
                    .width(300)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                    .into();
                provider_col = provider_col
                    .push(Space::new().height(12))
                    .push(panel_field(crate::i18n::t("api_url"), url_input));
            }

            provider_col = provider_col
                .push(Space::new().height(12))
                .push(panel_field(
                    "API Key",
                    dir_row(vec![key_input, Space::new().width(8).into(), save_btn])
                        .align_y(iced::Alignment::Center)
                        .into(),
                ))
                .push(Space::new().height(4))
                .push(key_status);

            content_col = content_col
                .push(Space::new().height(12))
                .push(panel_section(provider_col));

            // System prompt, multi-line editor that grows with the
            // content. `Length::Shrink` lets the editor auto-resize
            // to fit its text, capped by the panel's scroll area.
            let prompt_editor: Element<'_, Message> = iced::widget::text_editor(&self.ai.system_prompt)
                .placeholder(t("ai_system_prompt_placeholder"))
                .on_action(Message::AiSystemPromptAction)
                .padding(10)
                .height(Length::Shrink)
                .style(|_theme, status| {
                    let c = OryxisColors::t();
                    let (border_color, border_width) = match status {
                        iced::widget::text_editor::Status::Focused { .. } => (c.accent, 1.5),
                        _ => (c.border, 1.0),
                    };
                    iced::widget::text_editor::Style {
                        background: iced::Background::Color(c.bg_surface),
                        border: iced::Border {
                            radius: iced::border::Radius::from(crate::widgets::INPUT_RADIUS),
                            width: border_width,
                            color: border_color,
                        },
                        placeholder: c.text_muted,
                        value: c.text_primary,
                        selection: c.accent,
                    }
                })
                .into();
            let prompt_section = panel_section(column![
                panel_field(t("additional_system_prompt"), prompt_editor),
                Space::new().height(4),
                text(t("ai_system_prompt_desc"))
                    .size(11).color(OryxisColors::t().text_muted),
            ]);
            content_col = content_col
                .push(Space::new().height(12))
                .push(prompt_section);
        }

        scrollable(
            container(content_col)
                .padding(Padding { top: 24.0, right: 24.0, bottom: 24.0, left: 24.0 }),
        )
        .height(Length::Fill)
        .into()
    }
}
