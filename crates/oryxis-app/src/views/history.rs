//! History / session logs screen.

use iced::border::Radius;
use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::widget::button::Status as BtnStatus;
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis};
use crate::theme::OryxisColors;
use crate::util::format_data_size;

impl Oryxis {
    pub(crate) fn view_history(&self) -> Element<'_, Message> {
        // Pagination state: 50 rows / page; labels reflect current page and
        // total count from the vault. Prev/Next are disabled at the edges.
        let per_page: usize = 50;
        let max_page = self.logs_total.saturating_sub(1) / per_page.max(1);
        let can_prev = self.logs_page > 0;
        let can_next = self.logs_page < max_page;
        let range_label = if self.logs_total == 0 {
            format!("0 {}", crate::i18n::t("entries"))
        } else {
            let start = self.logs_page * per_page + 1;
            let end = ((self.logs_page + 1) * per_page).min(self.logs_total);
            format!("{}\u{2013}{} {} {}", start, end, crate::i18n::t("of"), self.logs_total)
        };

        let prev_btn = nav_btn(iced_fonts::lucide::chevron_left(), Message::LogsPagePrev, can_prev);
        let next_btn = nav_btn(iced_fonts::lucide::chevron_right(), Message::LogsPageNext, can_next);

        let clear_btn = button(
            container(text(crate::i18n::t("clear").to_uppercase()).size(11).font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            }).color(OryxisColors::t().text_muted))
                .center_y(Length::Fixed(24.0))
                .padding(Padding { top: 0.0, right: 14.0, bottom: 0.0, left: 14.0 }),
        )
        .on_press(Message::ClearLogs)
        .style(|_, status| {
            let bg = match status {
                BtnStatus::Hovered => Color { a: 0.15, ..OryxisColors::t().error },
                _ => Color::TRANSPARENT,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(6.0), color: OryxisColors::t().border, width: 1.0 },
                ..Default::default()
            }
        });
        let toolbar = container(
            crate::widgets::dir_row(vec![
                text(crate::i18n::t("history")).size(20).color(OryxisColors::t().text_primary).into(),
                Space::new().width(Length::Fill).into(),
                text(range_label).size(11).color(OryxisColors::t().text_muted).into(),
                Space::new().width(8).into(),
                prev_btn,
                Space::new().width(4).into(),
                next_btn,
                Space::new().width(12).into(),
                clear_btn.into(),
            ]).align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 24.0, bottom: 16.0, left: 24.0 })
        .width(Length::Fill);

        let mut rows: Vec<Element<'_, Message>> = Vec::new();

        if self.logs.is_empty() {
            rows.push(
                container(
                    text(crate::i18n::t("no_activity"))
                        .size(13).color(OryxisColors::t().text_muted),
                )
                .padding(16)
                .into(),
            );
        }

        for entry in &self.logs {
            let (event_icon, event_color) = match entry.event {
                oryxis_core::models::log_entry::LogEvent::Connected => {
                    (iced_fonts::lucide::circle_check(), OryxisColors::t().success)
                }
                oryxis_core::models::log_entry::LogEvent::Disconnected => {
                    (iced_fonts::lucide::circle_minus(), OryxisColors::t().text_muted)
                }
                oryxis_core::models::log_entry::LogEvent::AuthFailed => {
                    (iced_fonts::lucide::circle_x(), OryxisColors::t().warning)
                }
                oryxis_core::models::log_entry::LogEvent::Error => {
                    (iced_fonts::lucide::circle_alert(), OryxisColors::t().error)
                }
            };

            let ts = entry.timestamp.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S").to_string();

            // Inner column children (title row + message text) stick to
            // the *leading* edge of the column — left under LTR, right
            // under RTL — so the title sits next to the icon (which the
            // outer dir_row places on the leading side) instead of
            // ending up glued to the timestamp on the trailing side.
            let log_row = container(
                crate::widgets::dir_row(vec![
                    event_icon.size(14).color(event_color).into(),
                    Space::new().width(12).into(),
                    column![
                        crate::widgets::dir_row(vec![
                            text(&entry.connection_label).size(13).color(OryxisColors::t().text_primary).into(),
                            Space::new().width(8).into(),
                            text(format!("{}", entry.event)).size(11).color(event_color).into(),
                        ]).align_y(iced::Alignment::Center),
                        Space::new().height(2),
                        text(&entry.message).size(11).color(OryxisColors::t().text_muted),
                    ]
                    .width(Length::Fill)
                    .align_x(crate::widgets::dir_align_x())
                    .into(),
                    text(ts).size(10).color(OryxisColors::t().text_muted).into(),
                ]).align_y(iced::Alignment::Center),
            )
            .padding(Padding { top: 8.0, right: 16.0, bottom: 8.0, left: 16.0 })
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border { radius: Radius::from(8.0), ..Default::default() },
                ..Default::default()
            });

            rows.push(log_row.into());
            rows.push(Space::new().height(4).into());
        }

        // ── Session Logs section ──
        rows.push(Space::new().height(16).into());
        rows.push(
            container(
                text(crate::i18n::t("session_logs")).size(16).color(OryxisColors::t().text_primary),
            )
            .padding(Padding { top: 0.0, right: 0.0, bottom: 8.0, left: 0.0 })
            .into(),
        );

        if self.session_logs.is_empty() {
            rows.push(
                container(
                    text(crate::i18n::t("no_session_recordings"))
                        .size(13).color(OryxisColors::t().text_muted),
                )
                .padding(16)
                .into(),
            );
        }

        for (idx, entry) in self.session_logs.iter().enumerate() {
            let ts = entry.started_at.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S").to_string();
            let duration = if let Some(ended) = entry.ended_at {
                let dur = ended.signed_duration_since(entry.started_at);
                let secs = dur.num_seconds();
                if secs < 60 {
                    format!("{}s", secs)
                } else if secs < 3600 {
                    format!("{}m {}s", secs / 60, secs % 60)
                } else {
                    format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
                }
            } else {
                crate::i18n::t("in_progress").to_string()
            };
            let size_str = format_data_size(entry.data_size);
            let log_id = entry.id;

            let view_btn = button(
                container(text(crate::i18n::t("view")).size(11).color(OryxisColors::t().accent))
                    .padding(Padding { top: 4.0, right: 10.0, bottom: 4.0, left: 10.0 }),
            )
            .on_press(Message::ViewSessionLog(log_id))
            .style(|_, status| {
                let bg = match status {
                    BtnStatus::Hovered => Color { a: 0.15, ..OryxisColors::t().accent },
                    _ => Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(6.0), color: OryxisColors::t().accent, width: 1.0 },
                    ..Default::default()
                }
            });
            let delete_btn = button(
                container(text(crate::i18n::t("delete")).size(11).color(OryxisColors::t().error))
                    .padding(Padding { top: 4.0, right: 10.0, bottom: 4.0, left: 10.0 }),
            )
            .on_press(Message::DeleteSessionLog(idx))
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
            });
            let session_row = container(
                crate::widgets::dir_row(vec![
                    iced_fonts::lucide::file_text().size(14).color(OryxisColors::t().accent).into(),
                    Space::new().width(12).into(),
                    column![
                        text(&entry.label).size(13).color(OryxisColors::t().text_primary),
                        Space::new().height(2),
                        crate::widgets::dir_row(vec![
                            text(ts).size(10).color(OryxisColors::t().text_muted).into(),
                            Space::new().width(12).into(),
                            text(duration).size(10).color(OryxisColors::t().text_muted).into(),
                            Space::new().width(12).into(),
                            text(size_str).size(10).color(OryxisColors::t().text_muted).into(),
                        ]),
                    ]
                    .width(Length::Fill)
                    .align_x(crate::widgets::dir_align_x())
                    .into(),
                    view_btn.into(),
                    Space::new().width(8).into(),
                    delete_btn.into(),
                ]).align_y(iced::Alignment::Center),
            )
            .padding(Padding { top: 8.0, right: 16.0, bottom: 8.0, left: 16.0 })
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border { radius: Radius::from(8.0), ..Default::default() },
                ..Default::default()
            });

            rows.push(session_row.into());
            rows.push(Space::new().height(4).into());
        }

        let list = scrollable(
            column(rows).padding(Padding { top: 0.0, right: 24.0, bottom: 24.0, left: 24.0 }),
        ).height(Length::Fill);

        // Session log viewer overlay
        if let Some((_log_id, ref rendered_text)) = self.viewing_session_log {
            let viewer = container(
                column![
                    // Header
                    container(
                        row![
                            text(crate::i18n::t("session_log")).size(16).color(OryxisColors::t().text_primary),
                            Space::new().width(Length::Fill),
                            button(
                                container(text(crate::i18n::t("close")).size(11).font(iced::Font {
                                    weight: iced::font::Weight::Bold,
                                    ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                                }).color(OryxisColors::t().text_muted))
                                    .center_y(Length::Fixed(24.0))
                                    .padding(Padding { top: 0.0, right: 14.0, bottom: 0.0, left: 14.0 }),
                            )
                            .on_press(Message::CloseSessionLogView)
                            .style(|_, status| {
                                let bg = match status {
                                    BtnStatus::Hovered => Color { a: 0.15, ..OryxisColors::t().error },
                                    _ => Color::TRANSPARENT,
                                };
                                button::Style {
                                    background: Some(Background::Color(bg)),
                                    border: Border { radius: Radius::from(6.0), color: OryxisColors::t().border, width: 1.0 },
                                    ..Default::default()
                                }
                            }),
                        ].align_y(iced::Alignment::Center),
                    )
                    .padding(Padding { top: 16.0, right: 20.0, bottom: 12.0, left: 20.0 }),
                    // Content
                    scrollable(
                        container(
                            text(rendered_text)
                                .size(12)
                                .color(OryxisColors::t().text_primary)
                                .font(iced::Font::MONOSPACE),
                        )
                        .padding(16)
                        .width(Length::Fill),
                    ).height(Length::Fill),
                ]
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_primary)),
                border: Border { radius: Radius::from(0.0), ..Default::default() },
                ..Default::default()
            });

            return viewer.into();
        }

        column![toolbar, list]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }


}

/// Pagination chevron button. Disabled state has no `on_press` and a muted
/// look so it reads as unclickable at the boundaries.
fn nav_btn<'a>(
    icon: iced::widget::Text<'a>,
    msg: Message,
    enabled: bool,
) -> Element<'a, Message> {
    let icon = icon.size(12).color(if enabled {
        OryxisColors::t().text_secondary
    } else {
        OryxisColors::t().text_muted
    });
    let mut b = button(
        container(icon)
            .center(Length::Fixed(24.0))
            .height(Length::Fixed(24.0))
            .width(Length::Fixed(28.0)),
    )
    .style(move |_, status| {
        let bg = if enabled {
            match status {
                BtnStatus::Hovered => Color::from_rgba(1.0, 1.0, 1.0, 0.08),
                _ => Color::TRANSPARENT,
            }
        } else {
            Color::TRANSPARENT
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), color: OryxisColors::t().border, width: 1.0 },
            ..Default::default()
        }
    });
    if enabled {
        b = b.on_press(msg);
    }
    b.into()
}
