//! History view: unified timeline of connection errors + recorded
//! sessions, replacing the separate "History" / "Session Logs"
//! containers from v0.6. Successful connect/disconnect events are
//! folded into the corresponding session log row (start/end times,
//! data size) so the list reads as one chronological feed.

use iced::border::Radius;
use iced::widget::{button, column, container, scrollable, text, Space};
use iced::widget::button::Status as BtnStatus;
use iced::{Background, Border, Color, Element, Length, Padding};

use chrono::{DateTime, Utc};

use crate::app::{Message, Oryxis};
use crate::theme::OryxisColors;
use crate::util::format_data_size;

/// A single row in the unified timeline, either a failed-connect log
/// entry or a recorded session. Ordered by `ts` descending across
/// both kinds.
enum TimelineKind<'a> {
    /// Connection attempt that didn't go anywhere useful (auth
    /// failure or transport error). Carries the original LogEntry
    /// so we can show the underlying message.
    Failure(&'a oryxis_core::models::log_entry::LogEntry),
    /// A recorded session, successful start through end (or still
    /// in progress). Carries the session row + its index in
    /// `self.session_logs` so the View/Delete buttons can target it.
    Session {
        idx: usize,
        entry: &'a oryxis_vault::SessionLogEntry,
    },
}

struct TimelineRow<'a> {
    ts: DateTime<Utc>,
    label: &'a str,
    hostname: Option<&'a str>,
    kind: TimelineKind<'a>,
}

impl Oryxis {
    pub(crate) fn view_history(&self) -> Element<'_, Message> {
        use oryxis_core::models::log_entry::LogEvent;

        // ── Toolbar ──
        let per_page: usize = 50;
        let needle = self.history_search.to_lowercase();

        // Build the unified timeline. Failed log entries (auth fail,
        // transport error) stay as their own rows; everything else
        // about a successful connect is already captured by its
        // session log row, so we drop Connected/Disconnected events
        // here to avoid showing the same connection twice.
        let mut rows: Vec<TimelineRow<'_>> = Vec::new();
        for entry in &self.logs {
            if !matches!(entry.event, LogEvent::AuthFailed | LogEvent::Error) {
                continue;
            }
            rows.push(TimelineRow {
                ts: entry.timestamp,
                label: &entry.connection_label,
                hostname: Some(&entry.hostname),
                kind: TimelineKind::Failure(entry),
            });
        }
        // One-pass lookup maps so each row resolves its connection in
        // O(1) instead of scanning the full connection list per row on
        // every frame. `conn_by_label` keeps the first match to mirror
        // the old `find` semantics for duplicate labels.
        let hostname_by_id: std::collections::HashMap<uuid::Uuid, &str> = self
            .connections
            .iter()
            .map(|c| (c.id, c.hostname.as_str()))
            .collect();
        let mut conn_by_label: std::collections::HashMap<&str, _> =
            std::collections::HashMap::new();
        for c in &self.connections {
            conn_by_label.entry(c.label.as_str()).or_insert(c);
        }
        for (idx, entry) in self.session_logs.iter().enumerate() {
            // Look up the connection by id so we can show its
            // hostname next to the label (matches the Termius row).
            let hostname = hostname_by_id.get(&entry.connection_id).copied();
            rows.push(TimelineRow {
                ts: entry.started_at,
                label: &entry.label,
                hostname,
                kind: TimelineKind::Session { idx, entry },
            });
        }

        // Filter by the contextual sub-nav search before paginating
        // so the page counts reflect what the user actually sees.
        // Filtering before the sort also keeps the sort to the rows
        // that survive.
        if !needle.is_empty() {
            rows.retain(|r| {
                r.label.to_lowercase().contains(&needle)
                    || r.hostname.is_some_and(|h| h.to_lowercase().contains(&needle))
            });
        }
        rows.sort_by_key(|r| std::cmp::Reverse(r.ts));

        let total = rows.len();
        let max_page = total.saturating_sub(1) / per_page.max(1);
        let page = self.logs_page.min(max_page);
        let can_prev = page > 0;
        let can_next = page < max_page;
        let range_label = if total == 0 {
            format!("0 {}", crate::i18n::t("entries"))
        } else {
            let start = page * per_page + 1;
            let end = ((page + 1) * per_page).min(total);
            format!(
                "{}\u{2013}{} {} {}",
                start, end, crate::i18n::t("of"), total
            )
        };

        let prev_btn = nav_btn(
            iced_fonts::lucide::chevron_left(),
            Message::LogsPagePrev,
            can_prev,
        );
        let next_btn = nav_btn(
            iced_fonts::lucide::chevron_right(),
            Message::LogsPageNext,
            can_next,
        );

        // "Clear all" reads as a destructive main action (solid error
        // fill, same look as the confirm modal's primary button) and
        // only *requests* the wipe; the actual ClearLogs runs from the
        // confirmation modal in layout.rs.
        let clear_btn = button(
            container(
                text(crate::i18n::t("clear_all").to_uppercase())
                    .size(11)
                    .font(iced::Font {
                        weight: iced::font::Weight::Bold,
                        ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                    })
                    .color(OryxisColors::t().button_text),
            )
            .center_y(Length::Fixed(24.0))
            .padding(Padding {
                top: 0.0,
                right: 14.0,
                bottom: 0.0,
                left: 14.0,
            }),
        )
        .on_press(Message::RequestClearHistory)
        .style(|_, status| {
            let base = OryxisColors::t().error;
            let bg = match status {
                BtnStatus::Hovered => Color { a: 0.85, ..base },
                BtnStatus::Pressed => Color { a: 0.70, ..base },
                _ => base,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border {
                    radius: Radius::from(6.0),
                    ..Default::default()
                },
                ..Default::default()
            }
        });

        let toolbar = container(
            crate::widgets::dir_row(vec![
                text(crate::i18n::t("logs"))
                    .size(20)
                    .color(OryxisColors::t().text_primary)
                    .into(),
                Space::new().width(Length::Fill).into(),
                text(range_label)
                    .size(11)
                    .color(OryxisColors::t().text_muted)
                    .into(),
                Space::new().width(8).into(),
                prev_btn,
                Space::new().width(4).into(),
                next_btn,
                Space::new().width(12).into(),
                clear_btn.into(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 24.0, bottom: 16.0, left: 24.0 })
        .width(Length::Fill);

        // ── Rows ──
        let mut row_elements: Vec<Element<'_, Message>> = Vec::new();
        if rows.is_empty() {
            row_elements.push(
                container(
                    text(crate::i18n::t("no_activity"))
                        .size(13)
                        .color(OryxisColors::t().text_muted),
                )
                .padding(16)
                .into(),
            );
        } else {
            let start = page * per_page;
            let end = ((page + 1) * per_page).min(total);
            for row_data in &rows[start..end] {
                let conn = conn_by_label.get(row_data.label).copied();
                row_elements.push(self.render_timeline_row(row_data, conn));
                row_elements.push(Space::new().height(4).into());
            }
        }

        let list = scrollable(
            column(row_elements).padding(Padding {
                top: 0.0,
                right: 24.0,
                bottom: 24.0,
                left: 24.0,
            }),
        )
        .height(Length::Fill);

        // ── Session viewer overlay ──
        if let Some((_log_id, ref spans)) = self.viewing_session_log {
            // Recreate the terminal's look: palette colors parsed from
            // the recording's SGR sequences over the theme background.
            let palette = self.resolve_global_terminal_palette();
            let default_fg = palette.foreground;
            let term_bg = palette.background;
            let rich_spans: Vec<iced::widget::text::Span<'_, ()>> = spans
                .iter()
                .map(|s| {
                    iced::widget::text::Span::new(s.text.as_str())
                        .color(s.color.unwrap_or(default_fg))
                })
                .collect();
            let body = iced::widget::text::Rich::<'_, (), Message>::with_spans(rich_spans)
                .size(12)
                .font(iced::Font::MONOSPACE)
                .selectable(true);
            let viewer = container(
                column![
                    container(
                        crate::widgets::dir_row(vec![
                            text(crate::i18n::t("session_log"))
                                .size(16)
                                .color(OryxisColors::t().text_primary)
                                .into(),
                            Space::new().width(Length::Fill).into(),
                            button(
                                container(
                                    text(crate::i18n::t("close")).size(11).font(iced::Font {
                                        weight: iced::font::Weight::Bold,
                                        ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                                    }).color(OryxisColors::t().text_muted),
                                )
                                .center_y(Length::Fixed(24.0))
                                .padding(Padding {
                                    top: 0.0, right: 14.0, bottom: 0.0, left: 14.0,
                                }),
                            )
                            .on_press(Message::CloseSessionLogView)
                            .style(|_, status| {
                                let bg = match status {
                                    BtnStatus::Hovered => Color { a: 0.15, ..OryxisColors::t().error },
                                    _ => Color::TRANSPARENT,
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
                            })
                            .into(),
                        ]).align_y(iced::Alignment::Center),
                    )
                    .padding(Padding {
                        top: 16.0, right: 20.0, bottom: 12.0, left: 20.0,
                    }),
                    scrollable(
                        container(body)
                            .padding(16)
                            .width(Length::Fill)
                            .style(move |_| container::Style {
                                background: Some(Background::Color(term_bg)),
                                ..Default::default()
                            }),
                    )
                    .height(Length::Fill),
                ],
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_primary)),
                border: Border {
                    radius: Radius::from(0.0),
                    ..Default::default()
                },
                ..Default::default()
            });

            return viewer.into();
        }

        // Inline search bar in Classic mode (Workspace puts it on
        // the contextual sub-nav). Collapses to zero height in
        // Workspace so the input doesn't render twice.
        let workspace_mode = self.setting_layout_mode == "workspace";
        let search_bar: Element<'_, Message> = if workspace_mode {
            Space::new().height(0).into()
        } else {
            container(
                iced::widget::text_input(
                    crate::i18n::t("search_history"),
                    &self.history_search,
                )
                .on_input(Message::HistorySearchChanged)
                .padding(10)
                .size(13)
                .style(crate::widgets::rounded_input_style)
                .align_x(crate::widgets::dir_align_x()),
            )
            .padding(Padding { top: 0.0, right: 24.0, bottom: 12.0, left: 24.0 })
            .width(Length::Fill)
            .into()
        };

        column![toolbar, search_bar, list]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Render one row of the unified timeline. Layout (LTR):
    ///   [host_icon] [label / hostname] [event chip] [meta] [actions] [ts]
    /// `event chip` is "Session" / "Auth Failed" / "Error".
    /// `meta` and `actions` only show for session rows; failure rows
    /// add their underlying message under the label instead.
    fn render_timeline_row<'a>(
        &'a self,
        row: &TimelineRow<'a>,
        conn: Option<&'a oryxis_core::models::connection::Connection>,
    ) -> Element<'a, Message> {
        use oryxis_core::models::log_entry::LogEvent;

        // Host badge through the shared host_icon helper so per-host
        // shape + accent color are honoured here too. `conn` is the
        // connection matching the row's label, resolved by the caller
        // through a map built once per view call; missing connections
        // (host deleted but log row stays) fall back to the global
        // accent.
        let icon_style = crate::widgets::resolve_host_icon_style(
            conn.and_then(|c| c.icon_style.as_deref()),
            &self.setting_default_host_icon,
        );
        let detected_os = conn.and_then(|c| c.detected_os.as_deref());
        let (glyph, default_color) = crate::os_icon::resolve_icon(
            detected_os,
            OryxisColors::t().accent,
        );
        let icon_color = conn
            .and_then(|c| c.custom_color.as_deref().or(c.color.as_deref()))
            .and_then(crate::widgets::parse_hex_color)
            .unwrap_or(default_color);
        let glyph_el: Element<'_, Message> = glyph.view(14.0, Color::WHITE);
        let badge = crate::widgets::host_icon(
            icon_style,
            icon_color,
            row.label,
            Some(glyph_el),
            28.0,
        );

        let ts = row
            .ts
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();

        // Event chip + per-kind colour. Session rows render in the
        // accent colour so they read as "primary" content vs. the
        // warning/error tint reserved for failures.
        let (chip_text, chip_color): (String, Color) = match &row.kind {
            TimelineKind::Failure(e) => match e.event {
                LogEvent::AuthFailed => (
                    crate::i18n::t("event_auth_failed").to_string(),
                    OryxisColors::t().warning,
                ),
                _ => (
                    crate::i18n::t("event_error").to_string(),
                    OryxisColors::t().error,
                ),
            },
            TimelineKind::Session { .. } => (
                crate::i18n::t("event_session").to_string(),
                OryxisColors::t().accent,
            ),
        };
        let chip = container(
            text(chip_text)
                .size(10)
                .color(chip_color),
        )
        .padding(Padding { top: 2.0, right: 8.0, bottom: 2.0, left: 8.0 })
        .style(move |_| container::Style {
            background: Some(Background::Color(Color { a: 0.12, ..chip_color })),
            border: Border {
                radius: Radius::from(4.0),
                ..Default::default()
            },
            ..Default::default()
        });

        // Subtitle line: hostname for sessions, "hostname · message"
        // for failures (collapses to hostname when there's no message).
        let subtitle = match &row.kind {
            TimelineKind::Failure(e) => {
                if e.message.is_empty() {
                    row.hostname.unwrap_or("").to_string()
                } else if let Some(h) = row.hostname {
                    format!("{h} · {}", e.message)
                } else {
                    e.message.clone()
                }
            }
            TimelineKind::Session { entry, .. } => {
                let hostname = row.hostname.unwrap_or("");
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
                if hostname.is_empty() {
                    format!("{} · {}", duration, size_str)
                } else {
                    format!("{hostname} · {duration} · {size_str}")
                }
            }
        };

        // Trailing controls. Session rows: timestamp then Delete in
        // the last column; opening the recording is the row click
        // itself (no View button). Failure rows: timestamp only.
        let trailing: Element<'_, Message> = match &row.kind {
            TimelineKind::Session { idx, .. } => {
                let idx = *idx;
                let delete_btn = button(
                    container(
                        text(crate::i18n::t("delete"))
                            .size(11)
                            .color(OryxisColors::t().error),
                    )
                    .padding(Padding {
                        top: 4.0, right: 10.0, bottom: 4.0, left: 10.0,
                    }),
                )
                .on_press(Message::RequestDeleteSessionLog(idx))
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
                crate::widgets::dir_row(vec![
                    text(ts)
                        .size(10)
                        .color(OryxisColors::t().text_muted)
                        .into(),
                    Space::new().width(12).into(),
                    delete_btn.into(),
                ])
                .align_y(iced::Alignment::Center)
                .into()
            }
            TimelineKind::Failure(_) => text(ts)
                .size(10)
                .color(OryxisColors::t().text_muted)
                .into(),
        };

        // Session rows are clickable (the whole row opens the
        // recording) and highlight on hover; failure rows have nothing
        // to open, so they keep the flat card look.
        let viewable = match &row.kind {
            TimelineKind::Session { entry, .. } => Some(entry.id),
            TimelineKind::Failure(_) => None,
        };
        let hovered = viewable.is_some() && viewable == self.hovered_log_row;

        let card = container(
            crate::widgets::dir_row(vec![
                badge,
                Space::new().width(12).into(),
                column![
                    crate::widgets::dir_row(vec![
                        text(row.label)
                            .size(13)
                            .color(OryxisColors::t().text_primary)
                            .into(),
                        Space::new().width(8).into(),
                        chip.into(),
                    ])
                    .align_y(iced::Alignment::Center),
                    Space::new().height(2),
                    text(subtitle)
                        .size(11)
                        .color(OryxisColors::t().text_muted),
                ]
                .width(Length::Fill)
                .align_x(crate::widgets::dir_align_x())
                .into(),
                Space::new().width(12).into(),
                trailing,
            ])
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 8.0, right: 16.0, bottom: 8.0, left: 16.0 })
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(if hovered {
                OryxisColors::t().bg_hover
            } else {
                OryxisColors::t().bg_surface
            })),
            border: Border {
                radius: Radius::from(8.0),
                ..Default::default()
            },
            ..Default::default()
        });

        match viewable {
            Some(log_id) => iced::widget::MouseArea::new(card)
                .on_press(Message::ViewSessionLog(log_id))
                .on_enter(Message::LogRowHovered(log_id))
                .on_exit(Message::LogRowUnhovered)
                .interaction(iced::mouse::Interaction::Pointer)
                .into(),
            None => card.into(),
        }
    }
}

/// Pagination chevron button. Disabled state has no `on_press` and a
/// muted look so it reads as unclickable at the boundaries.
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
            border: Border {
                radius: Radius::from(6.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        }
    });
    if enabled {
        b = b.on_press(msg);
    }
    b.into()
}
