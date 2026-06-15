//! Known SSH hosts list screen.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, scrollable, text, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis};
use crate::i18n::t;
use crate::theme::OryxisColors;
use crate::widgets::dir_row;

impl Oryxis {
    pub(crate) fn view_known_hosts(&self) -> Element<'_, Message> {
        // Header with a "Re-verify all" / "Clear all" action so users can wipe
        // every entry at once (e.g. when every host was auto-accepted by an
        // older build and they want the verification dialog back).
        let has_entries = !self.known_hosts.is_empty();
        // Solid destructive style, matching the Logs "CLEAR ALL"
        // primary action. Only requests the wipe; the actual
        // ClearAllKnownHosts runs from the confirm dialog.
        let clear_all_btn: Element<'_, Message> = if has_entries {
            button(
                container(
                    text(t("re_verify_all").to_uppercase())
                        .size(11)
                        .font(iced::Font {
                            weight: iced::font::Weight::Bold,
                            ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                        })
                        .color(OryxisColors::t().button_text),
                )
                .center_y(Length::Fixed(24.0))
                .padding(Padding { top: 0.0, right: 14.0, bottom: 0.0, left: 14.0 }),
            )
            .on_press(Message::RequestClearAllKnownHosts)
            .style(|_, status| {
                let base = OryxisColors::t().error;
                let bg = match status {
                    BtnStatus::Hovered => Color { a: 0.85, ..base },
                    BtnStatus::Pressed => Color { a: 0.70, ..base },
                    _ => base,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                }
            })
            .into()
        } else {
            Space::new().width(0).into()
        };

        let toolbar = container(
            dir_row(vec![
                // Title dropped (the nav shows the active section).
                Space::new().width(0).into(),
                Space::new().width(Length::Fill).into(),
                clear_all_btn,
            ]).align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 16.0, right: 24.0, bottom: 16.0, left: 24.0 })
        .width(Length::Fill);

        if self.known_hosts.is_empty() {
            let empty = crate::widgets::empty_state(
                iced_fonts::lucide::shield_check()
                    .size(32)
                    .color(OryxisColors::t().text_muted)
                    .into(),
                t("no_known_hosts_yet").to_string(),
                t("known_hosts_empty_desc").to_string(),
                None,
            );
            return column![toolbar, empty]
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
        }

        let mut rows: Vec<Element<'_, Message>> = Vec::new();

        for (idx, kh) in self.known_hosts.iter().enumerate() {
            let fp_short = if kh.fingerprint.len() > 40 {
                format!("{}...", &kh.fingerprint[..40])
            } else {
                kh.fingerprint.clone()
            };
            // Stored UTC; show it in the user's local timezone.
            let seen = kh
                .last_seen
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string();

            // Bordered destructive style, matching the Logs per-row
            // "Delete" button. Opens the confirm dialog first.
            let del_btn = button(
                container(
                    text(t("remove"))
                        .size(11)
                        .color(OryxisColors::t().error),
                )
                .padding(Padding { top: 4.0, right: 10.0, bottom: 4.0, left: 10.0 }),
            )
            .on_press(Message::RequestDeleteKnownHost(idx))
            .style(|_, status| {
                let bg = match status {
                    BtnStatus::Hovered => Color { a: 0.15, ..OryxisColors::t().error },
                    _ => Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border {
                        radius: Radius::from(6.0),
                        color: OryxisColors::t().error,
                        width: 1.0,
                    },
                    ..Default::default()
                }
            });

            let entry = container(
                dir_row(vec![
                    iced_fonts::lucide::shield_check().size(14).color(OryxisColors::t().success).into(),
                    Space::new().width(12).into(),
                    column![
                        text(format!("{}:{}", kh.hostname, kh.port)).size(13).color(OryxisColors::t().text_primary),
                        Space::new().height(2),
                        text(format!("{} · {}", kh.key_type, fp_short)).size(10).color(OryxisColors::t().text_muted).font(iced::Font::MONOSPACE),
                        Space::new().height(2),
                        text(format!("{} {}", t("last_seen"), seen)).size(10).color(OryxisColors::t().text_muted),
                    ]
                    .width(Length::Fill)
                    .align_x(crate::widgets::dir_align_x())
                    .into(),
                    Space::new().width(12).into(),
                    del_btn.into(),
                ]).align_y(iced::Alignment::Center),
            )
            .padding(Padding { top: 10.0, right: 16.0, bottom: 10.0, left: 16.0 })
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border { radius: Radius::from(8.0), ..Default::default() },
                ..Default::default()
            });

            rows.push(entry.into());
            rows.push(Space::new().height(6).into());
        }

        let list = scrollable(
            column(rows).padding(Padding { top: 0.0, right: 24.0, bottom: 24.0, left: 24.0 }),
        ).height(Length::Fill);

        column![toolbar, list]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}
