//! Update-available modal: three choices (skip / later / update now) + a
//! short release-notes preview. During download we swap the action row
//! for a progress bar; errors stay inline in the same card.

use iced::border::Radius;
use iced::widget::{column, container, row, scrollable, text, MouseArea, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis};
use crate::theme::OryxisColors;
use crate::widgets::styled_button;

impl Oryxis {
    pub(crate) fn view_update_modal(&self) -> Element<'_, Message> {
        let info = match &self.pending_update {
            Some(i) => i,
            None => return Space::new().into(),
        };

        let current = env!("CARGO_PKG_VERSION");
        let title = text("Update available").size(18).font(iced::Font {
            weight: iced::font::Weight::Bold,
            ..iced::Font::with_name("Inter")
        }).color(OryxisColors::t().text_primary);

        let subtitle = text(format!("Oryxis {} is available. You're on {}.", info.version, current))
            .size(12)
            .color(OryxisColors::t().text_secondary);

        // Release notes preview — first ~40 lines, in a scrollable box so
        // long changelogs don't bloat the modal. Rendered as plain text
        // (we're not doing markdown rendering here).
        let notes_preview: String = info
            .body
            .lines()
            .take(40)
            .collect::<Vec<_>>()
            .join("\n");
        let notes: Element<'_, Message> = if notes_preview.trim().is_empty() {
            Space::new().height(0).into()
        } else {
            container(
                scrollable(
                    text(notes_preview)
                        .size(11)
                        .color(OryxisColors::t().text_muted)
                        .font(iced::Font::MONOSPACE),
                )
                .height(Length::Fixed(160.0)),
            )
            .padding(12)
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border { radius: Radius::from(8.0), color: OryxisColors::t().border, width: 1.0 },
                ..Default::default()
            })
            .into()
        };

        let release_link = MouseArea::new(
            text("Open release page on GitHub")
                .size(11)
                .color(OryxisColors::t().accent),
        )
        .on_press(Message::UpdateOpenRelease);

        // Action row OR progress bar depending on state.
        let action_area: Element<'_, Message> = if self.update_downloading {
            let pct = (self.update_progress * 100.0).clamp(0.0, 100.0) as u32;
            let bar = container(
                container(Space::new().height(6))
                    .width(Length::FillPortion((pct.max(1)) as u16))
                    .style(|_| container::Style {
                        background: Some(Background::Color(OryxisColors::t().accent)),
                        border: Border { radius: Radius::from(3.0), ..Default::default() },
                        ..Default::default()
                    }),
            )
            .width(Length::Fill)
            .height(Length::Fixed(6.0))
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_hover)),
                border: Border { radius: Radius::from(3.0), ..Default::default() },
                ..Default::default()
            });
            column![
                text(format!("Downloading installer… {}%", pct))
                    .size(11).color(OryxisColors::t().text_muted),
                Space::new().height(8),
                bar,
            ]
            .into()
        } else {
            row![
                styled_button(
                    "Skip this version",
                    Message::UpdateSkipVersion,
                    OryxisColors::t().bg_selected,
                ),
                Space::new().width(Length::Fill),
                styled_button(
                    "Later",
                    Message::UpdateLater,
                    OryxisColors::t().bg_hover,
                ),
                Space::new().width(8),
                styled_button(
                    "Update now",
                    Message::UpdateStartDownload,
                    OryxisColors::t().accent,
                ),
            ]
            .align_y(iced::Alignment::Center)
            .into()
        };

        let error_line: Element<'_, Message> = if let Some(err) = &self.update_error {
            container(
                text(format!("Error: {}", err))
                    .size(11)
                    .color(OryxisColors::t().error),
            )
            .padding(Padding { top: 8.0, right: 0.0, bottom: 0.0, left: 0.0 })
            .into()
        } else {
            Space::new().height(0).into()
        };

        let body = container(
            column![
                title,
                Space::new().height(6),
                subtitle,
                Space::new().height(16),
                notes,
                Space::new().height(8),
                release_link,
                Space::new().height(16),
                action_area,
                error_line,
            ],
        )
        .padding(24)
        .width(Length::Fixed(520.0))
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_primary)),
            border: Border {
                radius: Radius::from(12.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        });

        container(body)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }
}

/// Hard-modal: the update card is only dismissible via one of the three
/// buttons. We still expose a no-op backdrop helper here so other call
/// sites keep compiling, but the top-level dispatcher renders its own
/// non-interactive scrim that leaves the window chrome uncovered.
pub(crate) fn update_modal_backdrop<'a>() -> Element<'a, Message> {
    MouseArea::new(
        container(Space::new()).width(Length::Fill).height(Length::Fill),
    )
    .into()
}
