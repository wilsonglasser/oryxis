//! Vault setup / unlock / error screens.

use iced::border::Radius;
use iced::widget::{button, column, container, image, text, text_input, Space};
use iced::widget::button::Status as BtnStatus;
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis};
use crate::theme::OryxisColors;
use crate::views::chrome::window_chrome_bar;
use crate::widgets::styled_button;

/// Wrap a vault screen body with the top window chrome so the user can still
/// drag / minimize / maximize / close before unlocking the vault. Also adds
/// the edge-resize border so the lock screen is as resizable as the main app.
fn with_chrome<'a>(body: Element<'a, Message>, maximized: bool) -> Element<'a, Message> {
    // 1 px hairline between the chrome bar and the screen body — matches the
    // separator that sits below the tab bar on the main view.
    let h_separator = iced::widget::container(iced::widget::Space::new().height(1))
        .width(Length::Fill)
        .style(|_| iced::widget::container::Style {
            background: Some(iced::Background::Color(OryxisColors::t().border)),
            ..Default::default()
        });
    let content: Element<'a, Message> =
        iced::widget::column![window_chrome_bar(), h_separator, body]
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    let overlay = if maximized { None } else { Some(crate::views::layout::resize_border()) };
    crate::views::layout::wrap_with_resize(content, overlay)
}

impl Oryxis {
    pub(crate) fn view_vault_setup(&self) -> Element<'_, Message> {
        let logo = image(self.logo_handle.clone())
            .width(64)
            .height(64);
        let title = text(crate::i18n::t("welcome")).size(28).color(OryxisColors::t().text_primary);
        let subtitle = text(crate::i18n::t("vault_setup_subtitle"))
            .size(14)
            .color(OryxisColors::t().text_secondary);

        let input = text_input(crate::i18n::t("master_password_optional"), &self.vault_password_input)
            .on_input(Message::VaultPasswordChanged)
            .on_submit(Message::VaultSetup)
            .secure(true)
            .padding(12)
            .width(300)
            .style(crate::widgets::rounded_input_style);

        let btn = styled_button(crate::i18n::t("create_vault"), Message::VaultSetup, OryxisColors::t().accent);

        let skip_btn = button(
            text(crate::i18n::t("continue_without_password")).size(13).color(OryxisColors::t().text_secondary),
        )
        .on_press(Message::VaultSkipPassword)
        .padding(Padding { top: 8.0, right: 16.0, bottom: 8.0, left: 16.0 })
        .style(|_, status| {
            let bg = match status {
                BtnStatus::Hovered => OryxisColors::t().bg_hover,
                _ => Color::TRANSPARENT,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(6.0), ..Default::default() },
                ..Default::default()
            }
        });

        let error = if let Some(err) = &self.vault_error {
            Element::from(text(err.clone()).size(13).color(OryxisColors::t().error))
        } else {
            Space::new().height(0).into()
        };

        let body: Element<'_, Message> = container(
            column![logo, Space::new().height(16), title, Space::new().height(8), subtitle, Space::new().height(24), input, Space::new().height(12), btn, Space::new().height(6), skip_btn, Space::new().height(8), error]
                .align_x(iced::Alignment::Center),
        )
        .center(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_primary)),
            ..Default::default()
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into();
        with_chrome(body, self.window_maximized)
    }

    pub(crate) fn view_vault_unlock(&self) -> Element<'_, Message> {
        let logo = image(self.logo_handle.clone())
            .width(64)
            .height(64);
        let title = text("Oryxis").size(28).color(OryxisColors::t().accent);
        let subtitle = text(crate::i18n::t("enter_password"))
            .size(14)
            .color(OryxisColors::t().text_secondary);

        let input = text_input(crate::i18n::t("master_password_placeholder"), &self.vault_password_input)
            .on_input(Message::VaultPasswordChanged)
            .on_submit(Message::VaultUnlock)
            .secure(true)
            .padding(12)
            .width(300)
            .style(crate::widgets::rounded_input_style);

        let btn = styled_button(crate::i18n::t("unlock"), Message::VaultUnlock, OryxisColors::t().accent);

        let error = if let Some(err) = &self.vault_error {
            Element::from(text(err.clone()).size(13).color(OryxisColors::t().error))
        } else {
            Space::new().height(0).into()
        };

        let destroy_section: Element<'_, Message> = if self.vault_destroy_confirm {
            column![
                text(crate::i18n::t("vault_destroy_confirm")).size(12).color(OryxisColors::t().error),
                Space::new().height(6),
                styled_button(crate::i18n::t("destroy_vault"), Message::VaultDestroy, OryxisColors::t().error),
            ].align_x(iced::Alignment::Center).into()
        } else {
            button(
                text(crate::i18n::t("forgot_password")).size(12).color(OryxisColors::t().text_muted),
            )
            .on_press(Message::VaultDestroyConfirm)
            .padding(Padding { top: 6.0, right: 12.0, bottom: 6.0, left: 12.0 })
            .style(|_, _| button::Style::default())
            .into()
        };

        let body: Element<'_, Message> = container(
            column![logo, Space::new().height(16), title, Space::new().height(8), subtitle, Space::new().height(24), input, Space::new().height(12), btn, Space::new().height(8), error, Space::new().height(16), destroy_section]
                .align_x(iced::Alignment::Center),
        )
        .center(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_primary)),
            ..Default::default()
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into();
        with_chrome(body, self.window_maximized)
    }

    pub(crate) fn view_vault_error(&self, msg: &str) -> Element<'_, Message> {
        let msg = msg.to_string();
        let body: Element<'_, Message> = container(
            text(msg).size(16).color(OryxisColors::t().error),
        )
        .center(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_primary)),
            ..Default::default()
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into();
        with_chrome(body, self.window_maximized)
    }

    // -- Main layout --
}
