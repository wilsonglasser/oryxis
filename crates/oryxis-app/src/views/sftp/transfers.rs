//! SFTP view helpers: transfers. Split out of views/sftp/mod.rs.

use super::*;
use iced::widget::{column, row};
/// Per-file progress panel that rises above the transfer strip when the
/// user clicks it. Lists finished items, the one in flight, and what's
/// still queued, so a multi-file (or slow single-file) transfer shows
/// exactly where it is.
pub(crate) fn transfer_file_panel<'a>(
    transfer: &crate::state::TransferState,
    done_log: &[String],
) -> Element<'a, Message> {
    fn marker_row<'b>(
        glyph: &str,
        glyph_color: Color,
        label: String,
        label_color: Color,
    ) -> Element<'b, Message> {
        crate::widgets::dir_row(vec![
            text(glyph.to_string()).size(12).color(glyph_color).into(),
            Space::new().width(8).into(),
            text(label).size(12).color(label_color).into(),
        ])
        .align_y(iced::Alignment::Center)
        .into()
    }

    let theme = OryxisColors::t();
    let mut list = column![].spacing(3);
    for label in done_log {
        list = list.push(marker_row("\u{2713}", theme.success, label.clone(), theme.text_secondary));
    }
    if let Some(cur) = &transfer.current {
        list = list.push(marker_row("\u{25B8}", theme.accent, cur.clone(), theme.text_primary));
    }
    for item in &transfer.queue {
        let label = crate::sftp_helpers::transfer_item_label(item);
        list = list.push(marker_row("\u{2022}", theme.text_muted, label, theme.text_muted));
    }
    container(scrollable(list).height(Length::Fixed(180.0)))
        .padding(10)
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border {
                radius: Radius::from(8.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        })
        .into()
}

/// Bottom-of-view strip that surfaces an in-progress folder transfer:
/// kind label, current item, count, slim progress bar, and a cancel
/// button. Stays compact so the file panes lose as little vertical
/// space as possible.
pub(crate) fn transfer_progress_strip<'a>(
    transfer: &crate::state::TransferState,
    bytes_done: u64,
    bytes_total: u64,
) -> Element<'a, Message> {
    let label = match transfer.kind {
        crate::state::TransferKind::Upload => t("transfer_uploading"),
        crate::state::TransferKind::Download => t("transfer_downloading"),
        crate::state::TransferKind::DuplicateLocal => t("transfer_duplicating"),
        crate::state::TransferKind::Relay => t("transfer_relaying"),
    };
    let current = transfer
        .current
        .clone()
        .unwrap_or_else(|| t("transfer_preparing").to_string());
    // When the total byte size is known, drive the bar + count by bytes so a
    // single large file shows live movement; otherwise fall back to item
    // counts (e.g. all-directory transfers, where sizes are 0).
    let (count, pct) = if bytes_total > 0 {
        let done = bytes_done.min(bytes_total);
        (
            format!("{} / {}", format_size(done), format_size(bytes_total)),
            (done as f32 / bytes_total as f32).clamp(0.0, 1.0),
        )
    } else if transfer.total == 0 {
        (format!("{} / {}", transfer.completed, transfer.total), 0.0)
    } else {
        (
            format!("{} / {}", transfer.completed, transfer.total),
            (transfer.completed as f32 / transfer.total as f32).clamp(0.0, 1.0),
        )
    };
    // Ratio-based progress bar: a filled portion + a remaining portion in
    // a row so the `FillPortion` weights actually divide the track. (A lone
    // FillPortion child fills 100% regardless of its weight, which made the
    // bar look full even at 0%.)
    let filled = (pct * 1000.0) as u16;
    let remaining = 1000u16.saturating_sub(filled);
    let bar = container(
        row![
            container(Space::new())
                .width(Length::FillPortion(filled))
                .height(Length::Fixed(4.0))
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().accent)),
                    border: Border { radius: Radius::from(2.0), ..Default::default() },
                    ..Default::default()
                }),
            container(Space::new())
                .width(Length::FillPortion(remaining))
                .height(Length::Fixed(4.0)),
        ]
        .width(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fixed(4.0))
    .style(|_| container::Style {
        background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.05))),
        border: Border { radius: Radius::from(2.0), ..Default::default() },
        ..Default::default()
    });

    let info = column![
        row![
            text(format!("{} {}", label, transfer.root_label))
                .size(11)
                .color(OryxisColors::t().text_secondary),
            Space::new().width(Length::Fill),
            text(count).size(11).color(OryxisColors::t().text_muted),
        ]
        .align_y(iced::Alignment::Center),
        Space::new().height(2),
        text(current)
            .size(10)
            .color(OryxisColors::t().text_muted),
        Space::new().height(6),
        bar,
    ]
    .width(Length::Fill);

    let cancel_btn = button(
        text(t("cancel")).size(11).color(OryxisColors::t().text_secondary),
    )
    .on_press(Message::SftpCancelTransfer)
    .padding(Padding { top: 4.0, right: 10.0, bottom: 4.0, left: 10.0 })
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().bg_hover,
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
    });

    container(
        row![info, Space::new().width(12), cancel_btn]
            .align_y(iced::Alignment::Center),
    )
    .padding(Padding { top: 8.0, right: 14.0, bottom: 8.0, left: 14.0 })
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
        border: Border {
            width: 1.0,
            color: OryxisColors::t().border,
            radius: Radius::from(0.0),
        },
        ..Default::default()
    })
    .into()
}

/// The always-visible footer bar carrying the message-log toggle. Shows
/// the log entry count and highlights when the panel is open.
pub(crate) fn sftp_log_bar<'a>(open: bool, count: usize) -> Element<'a, Message> {
    let tint = if open {
        OryxisColors::t().accent
    } else {
        OryxisColors::t().text_muted
    };
    let label_color = if open {
        OryxisColors::t().text_primary
    } else {
        OryxisColors::t().text_muted
    };
    let chevron = if open {
        iced_fonts::lucide::chevron_down()
    } else {
        iced_fonts::lucide::chevron_up()
    };
    let toggle = button(
        row![
            iced_fonts::lucide::list().size(13).color(tint),
            Space::new().width(6),
            text(format!("{} ({})", t("sftp_log"), count))
                .size(11)
                .color(label_color),
            Space::new().width(6),
            chevron.size(10).color(tint),
        ]
        .align_y(iced::Alignment::Center),
    )
    .on_press(Message::SftpToggleLog)
    .padding(Padding { top: 3.0, right: 10.0, bottom: 3.0, left: 10.0 })
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().bg_hover,
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(4.0), ..Default::default() },
            ..Default::default()
        }
    });
    container(
        row![toggle, Space::new().width(Length::Fill)].align_y(iced::Alignment::Center),
    )
    .width(Length::Fill)
    .padding(Padding { top: 2.0, right: 8.0, bottom: 2.0, left: 8.0 })
    .style(|_| container::Style {
        background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
        border: Border {
            width: 1.0,
            color: OryxisColors::t().border,
            radius: Radius::from(0.0),
        },
        ..Default::default()
    })
    .into()
}

/// Draggable horizontal divider sitting above the message-log panel. Pressing
/// it arms a vertical resize (handled by the global mouse plumbing, shared
/// with the other SFTP drag handlers).
pub(crate) fn sftp_log_divider<'a>() -> Element<'a, Message> {
    MouseArea::new(
        container(Space::new().width(Length::Fill).height(Length::Fixed(5.0)))
            .width(Length::Fill)
            .height(Length::Fixed(5.0))
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().border)),
                ..Default::default()
            }),
    )
    .on_press(Message::SftpLogResizeStart)
    .interaction(iced::mouse::Interaction::ResizingVertically)
    .into()
}

/// FileZilla-style message-log panel: a scrollable list of timestamped events,
/// colour-coded by level, at the user-resizable `height`. Newest entries sit
/// at the bottom. Strings are cloned so the element doesn't borrow the log.
pub(crate) fn sftp_log_panel<'a>(log: &[crate::state::SftpLogEntry], height: f32) -> Element<'a, Message> {
    use crate::state::SftpLogLevel;
    let mut col = column![]
        .spacing(1)
        .padding(Padding { top: 6.0, right: 10.0, bottom: 6.0, left: 10.0 })
        .width(Length::Fill);
    if log.is_empty() {
        col = col.push(
            text(t("sftp_log_empty"))
                .size(11)
                .color(OryxisColors::t().text_muted),
        );
    } else {
        for e in log {
            let color = match e.level {
                SftpLogLevel::Info => OryxisColors::t().text_secondary,
                SftpLogLevel::Ok => OryxisColors::t().success,
                SftpLogLevel::Warn => OryxisColors::t().warning,
                SftpLogLevel::Error => OryxisColors::t().error,
            };
            col = col.push(
                row![
                    text(e.time.clone())
                        .size(11)
                        .color(OryxisColors::t().text_muted)
                        .width(Length::Fixed(64.0)),
                    text(e.text.clone()).size(11).color(color).width(Length::Fill),
                ]
                .spacing(8),
            );
        }
    }
    container(scrollable(col).width(Length::Fill).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fixed(height))
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border {
                width: 1.0,
                color: OryxisColors::t().border,
                radius: Radius::from(0.0),
            },
            ..Default::default()
        })
        .into()
}
