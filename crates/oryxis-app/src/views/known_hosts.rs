//! Known SSH hosts list screen.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis};
use crate::theme::OryxisColors;

impl Oryxis {
    pub(crate) fn view_known_hosts(&self) -> Element<'_, Message> {
        // Header with a "Re-verify all" / "Clear all" action so users can wipe
        // every entry at once (e.g. when every host was auto-accepted by an
        // older build and they want the verification dialog back).
        let has_entries = !self.known_hosts.is_empty();
        let clear_all_btn: Element<'_, Message> = if has_entries {
            button(
                container(
                    row![
                        iced_fonts::lucide::trash().size(12).color(OryxisColors::t().error),
                        Space::new().width(6),
                        text("Re-verify all").size(11).font(iced::Font {
                            weight: iced::font::Weight::Bold,
                            ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                        }).color(OryxisColors::t().error),
                    ].align_y(iced::Alignment::Center),
                )
                .center_y(Length::Fixed(24.0))
                .padding(Padding { top: 0.0, right: 12.0, bottom: 0.0, left: 10.0 }),
            )
            .on_press(Message::ClearAllKnownHosts)
            .style(|_, status| {
                let bg = match status {
                    BtnStatus::Hovered => Color { a: 0.15, ..OryxisColors::t().error },
                    _ => Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(6.0), color: OryxisColors::t().error, width: 1.0 },
                    ..Default::default()
                }
            })
            .into()
        } else {
            Space::new().width(0).into()
        };

        let toolbar = container(
            row![
                text("Known Hosts").size(20).color(OryxisColors::t().text_primary),
                Space::new().width(Length::Fill),
                clear_all_btn,
            ].align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 24.0, bottom: 16.0, left: 24.0 })
        .width(Length::Fill);

        let mut rows: Vec<Element<'_, Message>> = Vec::new();

        if self.known_hosts.is_empty() {
            rows.push(
                container(
                    column![
                        text("No known hosts yet. Entries appear here when you connect and approve a host's fingerprint.")
                            .size(13).color(OryxisColors::t().text_muted),
                        Space::new().height(8),
                        text("Remove an entry to force the verification dialog to appear again on the next connect.")
                            .size(12).color(OryxisColors::t().text_muted),
                    ],
                )
                .padding(16)
                .into(),
            );
        }

        for (idx, kh) in self.known_hosts.iter().enumerate() {
            let fp_short = if kh.fingerprint.len() > 40 {
                format!("{}...", &kh.fingerprint[..40])
            } else {
                kh.fingerprint.clone()
            };
            let seen = kh.last_seen.format("%Y-%m-%d %H:%M").to_string();

            // Trash button: icon + label, visible hover state in error red.
            let del_btn = button(
                container(
                    row![
                        iced_fonts::lucide::trash().size(12).color(OryxisColors::t().error),
                        Space::new().width(6),
                        text("Remove").size(11).font(iced::Font {
                            weight: iced::font::Weight::Bold,
                            ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                        }).color(OryxisColors::t().error),
                    ].align_y(iced::Alignment::Center),
                )
                .center_y(Length::Fixed(22.0))
                .padding(Padding { top: 0.0, right: 8.0, bottom: 0.0, left: 8.0 }),
            )
            .on_press(Message::DeleteKnownHost(idx))
            .style(|_, status| {
                let bg = match status {
                    BtnStatus::Hovered => Color { a: 0.15, ..OryxisColors::t().error },
                    _ => Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(4.0), ..Default::default() },
                    ..Default::default()
                }
            });

            let entry = container(
                row![
                    iced_fonts::lucide::shield_check().size(14).color(OryxisColors::t().success),
                    Space::new().width(12),
                    column![
                        text(format!("{}:{}", kh.hostname, kh.port)).size(13).color(OryxisColors::t().text_primary),
                        Space::new().height(2),
                        text(format!("{} · {}", kh.key_type, fp_short)).size(10).color(OryxisColors::t().text_muted).font(iced::Font::MONOSPACE),
                        Space::new().height(2),
                        text(format!("Last seen: {}", seen)).size(10).color(OryxisColors::t().text_muted),
                    ].width(Length::Fill),
                    del_btn,
                ].align_y(iced::Alignment::Center),
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
