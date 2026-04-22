//! Root layout — `view_main`, `render_overlay_menu`, and the content dispatcher.

use iced::border::Radius;
use iced::widget::{button, column, container, row, text, text_input, MouseArea, Space, Stack};
use iced::{Background, Border, Color, Element, Length};

use crate::app::{Message, Oryxis};
use crate::state::{OverlayContent, OverlayState, View};
use crate::theme::OryxisColors;
use crate::widgets::{context_menu_item, styled_button};

impl Oryxis {
    pub(crate) fn view_main(&self) -> Element<'_, Message> {
        let sidebar = self.view_sidebar();
        let tab_bar = self.view_tab_bar();
        let content = self.view_content();
        let status_bar = self.view_status_bar();

        let right_side = column![tab_bar, content].height(Length::Fill);
        let main_row = row![sidebar, right_side].height(Length::Fill);
        let layout = column![main_row, status_bar];

        let base: Element<'_, Message> = container(layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_primary)),
                ..Default::default()
            })
            .into();

        // Share dialog overlay
        if self.show_share_dialog {
            let share_include_keys = self.share_include_keys;
            let dialog_content = container(
                column![
                    text(crate::i18n::t("share")).size(16).color(OryxisColors::t().text_primary),
                    Space::new().height(12),
                    text_input(crate::i18n::t("export_password"), &self.share_password)
                        .on_input(Message::SharePasswordChanged)
                        .secure(true)
                        .padding(10)
                        .width(280),
                    Space::new().height(8),
                    row![
                        text(crate::i18n::t("include_private_keys")).size(13).color(OryxisColors::t().text_secondary),
                        Space::new().width(Length::Fill),
                        button(
                            text(if share_include_keys { "ON" } else { "OFF" }).size(12)
                        ).on_press(Message::ShareToggleKeys).style(move |_theme, _status| {
                            button::Style {
                                background: Some(Background::Color(if share_include_keys { OryxisColors::t().success } else { OryxisColors::t().bg_hover })),
                                border: Border { radius: Radius::from(4.0), ..Default::default() },
                                text_color: OryxisColors::t().text_primary,
                                ..Default::default()
                            }
                        }),
                    ].align_y(iced::Alignment::Center).width(280),
                    Space::new().height(12),
                    row![
                        styled_button(crate::i18n::t("share"), Message::ShareConfirm, OryxisColors::t().accent),
                        Space::new().width(8),
                        styled_button(crate::i18n::t("cancel"), Message::ShareDismiss, OryxisColors::t().text_muted),
                    ],
                    if let Some(status) = &self.share_status {
                        let (msg, color) = match status {
                            Ok(m) => (m.as_str(), OryxisColors::t().success),
                            Err(m) => (m.as_str(), OryxisColors::t().error),
                        };
                        Element::from(column![Space::new().height(8), text(msg).size(12).color(color)])
                    } else {
                        Element::from(Space::new().height(0))
                    },
                ]
                .padding(24),
            )
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border { radius: Radius::from(12.0), color: OryxisColors::t().border, width: 1.0 },
                ..Default::default()
            });

            let backdrop: Element<'_, Message> = MouseArea::new(
                container(Space::new()).width(Length::Fill).height(Length::Fill),
            )
            .on_press(Message::ShareDismiss)
            .into();

            let centered = container(dialog_content)
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill);

            return Stack::new()
                .push(base)
                .push(backdrop)
                .push(centered)
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
        }

        // New-tab picker (opens via the "+" button in the tab bar).
        if self.show_new_tab_picker {
            let picker = self.view_new_tab_picker();
            let backdrop = crate::views::new_tab_picker::new_tab_picker_backdrop();
            return Stack::new()
                .push(base)
                .push(backdrop)
                .push(picker)
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
        }

        // Icon/color picker (from the host editor).
        if self.show_icon_picker {
            let picker = self.view_icon_picker();
            let backdrop = crate::views::icon_picker::icon_picker_backdrop();
            return Stack::new()
                .push(base)
                .push(backdrop)
                .push(picker)
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
        }

        // Note: the update modal is rendered at the top-level `view()`
        // dispatcher (see `Oryxis::view`) so it overlays the lock screen
        // too. Don't re-render it here.

        if let Some(ref overlay) = self.overlay {
            let menu = self.render_overlay_menu(overlay);

            // Transparent backdrop that dismisses the menu on click
            let backdrop: Element<'_, Message> = MouseArea::new(
                container(Space::new())
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .on_press(Message::HideOverlayMenu)
            .into();

            // Position the menu, clamping to window bounds to prevent clipping
            let menu_width = 180.0_f32;
            let menu_height = 80.0_f32; // approximate menu height
            let x = overlay.x.min(self.window_size.width - menu_width).max(0.0);
            let y = overlay.y.min(self.window_size.height - menu_height).max(0.0);
            let positioned_menu: Element<'_, Message> = column![
                Space::new().height(y),
                row![
                    Space::new().width(x),
                    menu,
                ],
            ]
            .into();

            Stack::new()
                .push(base)
                .push(backdrop)
                .push(positioned_menu)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            base
        }
    }

    pub(crate) fn render_overlay_menu(&self, overlay: &OverlayState) -> Element<'_, Message> {
        let menu_width = 180.0;
        let items: Element<'_, Message> = match &overlay.content {
            OverlayContent::HostActions(idx) => {
                let idx = *idx;
                column![
                    context_menu_item(iced_fonts::lucide::play(), crate::i18n::t("connect"), Message::ConnectSsh(idx), OryxisColors::t().success),
                    context_menu_item(iced_fonts::lucide::pencil(), crate::i18n::t("edit"), Message::EditConnection(idx), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::copy(), crate::i18n::t("duplicate"), Message::DuplicateConnection(idx), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::share(), crate::i18n::t("share"), Message::ShareConnection(idx), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::trash(), crate::i18n::t("remove"), Message::DeleteConnection(idx), OryxisColors::t().error),
                ].into()
            }
            OverlayContent::KeyActions(idx) => {
                let idx = *idx;
                column![
                    context_menu_item(iced_fonts::lucide::pencil(), crate::i18n::t("edit"), Message::EditKey(idx), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::trash(), crate::i18n::t("remove"), Message::DeleteKey(idx), OryxisColors::t().error),
                ].into()
            }
            OverlayContent::IdentityActions(idx) => {
                let idx = *idx;
                column![
                    context_menu_item(iced_fonts::lucide::pencil(), crate::i18n::t("edit"), Message::EditIdentity(idx), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::trash(), crate::i18n::t("remove"), Message::DeleteIdentity(idx), OryxisColors::t().error),
                ].into()
            }
            OverlayContent::KeychainAdd => {
                column![
                    context_menu_item(iced_fonts::lucide::key_round(), crate::i18n::t("import_key"), Message::ShowKeyPanel, OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::user(), crate::i18n::t("new_identity"), Message::ShowIdentityPanel, OryxisColors::t().text_secondary),
                ].into()
            }
            OverlayContent::TabActions(idx) => {
                let idx = *idx;
                column![
                    context_menu_item(iced_fonts::lucide::rotate_cw(), crate::i18n::t("reconnect"), Message::ReconnectTab(idx), OryxisColors::t().accent),
                    context_menu_item(iced_fonts::lucide::x(), crate::i18n::t("close_tab"), Message::CloseTab(idx), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::x(), crate::i18n::t("close_other_tabs"), Message::CloseOtherTabs(idx), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::x(), crate::i18n::t("close_all_tabs"), Message::CloseAllTabs, OryxisColors::t().error),
                ].into()
            }
        };

        container(items)
            .width(menu_width)
            .padding(4)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    radius: Radius::from(8.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                shadow: iced::Shadow {
                    color: Color::from_rgba(0.0, 0.0, 0.0, 0.3),
                    offset: iced::Vector::new(0.0, 4.0),
                    blur_radius: 12.0,
                },
                ..Default::default()
            })
            .into()
    }
    pub(crate) fn view_content(&self) -> Element<'_, Message> {
        // If a terminal tab is active, show terminal
        // Otherwise show the grid view for the current nav item
        let content: Element<'_, Message> = if self.connecting.is_some() && self.active_tab.is_some() {
            self.view_connection_progress()
        } else if self.active_tab.is_some() && self.connecting.is_none() {
            self.view_terminal()
        } else {
            match self.active_view {
                View::Dashboard => self.view_dashboard(),
                View::Keys => self.view_keys(),
                View::Snippets => self.view_snippets(),
                View::KnownHosts => self.view_known_hosts(),
                View::History => self.view_history(),
                View::Settings => self.view_settings(),
                View::Terminal => self.view_terminal(),
            }
        };

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_primary)),
                ..Default::default()
            })
            .into()
    }
}
