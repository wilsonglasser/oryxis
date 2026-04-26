//! Termius-style "Jump to" modal — invoked from the `⋯` button in the
//! tab bar or via `Ctrl+J`. Lists every open tab plus the same Quick
//! connect entries that the new-tab picker offers (Local Terminal,
//! Serial, etc.), and includes a search box so the user can filter
//! down to a target tab without reaching for the mouse.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, row, scrollable, text, text_input, MouseArea, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis};
use crate::theme::{OryxisColors, SYSTEM_UI_SEMIBOLD};

impl Oryxis {
    pub(crate) fn view_tab_jump_modal(&self) -> Element<'_, Message> {
        let needle = self.tab_jump_search.to_lowercase();

        // ── Tabs section ───────────────────────────────────────────────
        // Every open tab is a row; current one gets the accent bg.
        let mut tabs_col = column![].spacing(2);
        let mut had_match = false;
        for (idx, tab) in self.tabs.iter().enumerate() {
            let label = tab.label.trim_end_matches(" (disconnected)").to_string();
            if !needle.is_empty() && !label.to_lowercase().contains(&needle) {
                continue;
            }
            had_match = true;
            let is_active = self.active_tab == Some(idx);
            // Match the tab-bar's OS-coloured badge so users recognise
            // the same visual cue from the strip up here.
            let detected_os = self
                .connections
                .iter()
                .find(|c| c.label == label)
                .and_then(|c| c.detected_os.clone());
            let fallback = if tab.label.ends_with(" (disconnected)") {
                OryxisColors::t().text_muted
            } else {
                OryxisColors::t().accent
            };
            let (glyph, mut badge_color) =
                crate::os_icon::resolve_icon(detected_os.as_deref(), fallback);
            if tab.label.ends_with(" (disconnected)") {
                badge_color = OryxisColors::t().text_muted;
            }
            let badge: Element<'_, Message> =
                container(glyph.size(11).color(Color::WHITE))
                    .center_x(Length::Fixed(20.0))
                    .center_y(Length::Fixed(20.0))
                    .style(move |_| container::Style {
                        background: Some(Background::Color(badge_color)),
                        border: Border {
                            radius: Radius::from(4.0),
                            ..Default::default()
                        },
                        ..Default::default()
                    })
                    .into();

            tabs_col = tabs_col.push(jump_row(
                badge,
                label,
                is_active,
                Message::SelectTab(idx),
            ));
        }
        // Inline "New Tab" entry — shortcut to the existing new-tab
        // picker without leaving this modal first.
        let new_tab_badge: Element<'_, Message> = container(
            iced_fonts::lucide::circle_check()
                .size(13)
                .color(OryxisColors::t().text_muted),
        )
        .center_x(Length::Fixed(20.0))
        .center_y(Length::Fixed(20.0))
        .into();
        if "new tab".contains(&needle) || needle.is_empty() {
            had_match = true;
            tabs_col = tabs_col.push(jump_row(
                new_tab_badge,
                "New Tab".to_string(),
                false,
                Message::ShowNewTabPicker,
            ));
        }

        let tabs_section: Element<'_, Message> = column![
            section_header("Tabs"),
            Space::new().height(4),
            tabs_col,
        ]
        .into();

        // ── Quick connect section ──────────────────────────────────────
        // Mirrors the "categories" of the new-tab picker so the user
        // can also kick off a fresh session from this modal.
        let quick_local: Element<'_, Message> = container(
            iced_fonts::lucide::monitor()
                .size(13)
                .color(OryxisColors::t().accent),
        )
        .center_x(Length::Fixed(20.0))
        .center_y(Length::Fixed(20.0))
        .into();
        let quick_section: Element<'_, Message> = column![
            section_header("Quick connect"),
            Space::new().height(4),
            jump_row(
                quick_local,
                "Local Terminal".to_string(),
                false,
                Message::OpenLocalShell,
            ),
        ]
        .into();

        // ── Search header ──────────────────────────────────────────────
        let search_input = text_input("Search tabs", &self.tab_jump_search)
            .on_input(Message::TabJumpSearchChanged)
            .padding(Padding { top: 8.0, right: 10.0, bottom: 8.0, left: 10.0 })
            .size(13)
            .style(crate::widgets::rounded_input_style);

        // "Jump to" pill on the left of the search row gives the modal
        // its identity; a Ctrl+J hint on the right reinforces the
        // shortcut so users learn it.
        let pill: Element<'_, Message> = container(
            text("Jump to")
                .size(11)
                .color(OryxisColors::t().accent)
                .font(SYSTEM_UI_SEMIBOLD),
        )
        .padding(Padding { top: 3.0, right: 8.0, bottom: 3.0, left: 8.0 })
        .style(|_| container::Style {
            background: Some(Background::Color(Color {
                a: 0.15,
                ..OryxisColors::t().accent
            })),
            border: Border {
                radius: Radius::from(10.0),
                ..Default::default()
            },
            ..Default::default()
        })
        .into();
        let shortcut_hint: Element<'_, Message> = text("Ctrl+J")
            .size(11)
            .color(OryxisColors::t().text_muted)
            .into();

        let search_header = container(
            row![
                iced_fonts::lucide::globe()
                    .size(13)
                    .color(OryxisColors::t().text_muted),
                Space::new().width(8),
                pill,
                Space::new().width(8),
                container(search_input).width(Length::Fill),
                Space::new().width(12),
                shortcut_hint,
            ]
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 4.0, right: 14.0, bottom: 4.0, left: 14.0 })
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_hover)),
            border: Border {
                radius: Radius::from(8.0),
                ..Default::default()
            },
            ..Default::default()
        });

        // Empty state — when search filters out everything.
        let body: Element<'_, Message> = if !had_match {
            container(
                text("No matching tabs.")
                    .size(12)
                    .color(OryxisColors::t().text_muted),
            )
            .padding(20)
            .into()
        } else {
            scrollable(
                column![
                    Space::new().height(8),
                    tabs_section,
                    Space::new().height(12),
                    quick_section,
                    Space::new().height(8),
                ]
                .padding(Padding {
                    top: 0.0,
                    right: 6.0,
                    bottom: 0.0,
                    left: 0.0,
                }),
            )
            .height(Length::Fixed(420.0))
            .into()
        };

        let dialog = container(
            column![search_header, Space::new().height(4), body]
                .padding(12)
                .width(Length::Fixed(540.0)),
        )
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border {
                radius: Radius::from(12.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            shadow: iced::Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.30),
                offset: iced::Vector::new(0.0, 8.0),
                blur_radius: 24.0,
            },
            ..Default::default()
        });

        // Compose as a single tree (no internal Stack) so the dark fill
        // on the scrim layer actually occludes the screen behind. Two
        // MouseAreas: outer = dismiss-on-empty, inner = absorb clicks
        // on the dialog itself so they don't bubble out.
        let dialog_capture: Element<'_, Message> = MouseArea::new(dialog)
            .on_press(Message::NoOp)
            .into();

        let centered = container(dialog_capture)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill);

        MouseArea::new(
            container(centered)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5))),
                    ..Default::default()
                }),
        )
        .on_press(Message::HideTabJump)
        .into()
    }
}

fn section_header<'a>(label: &'a str) -> Element<'a, Message> {
    text(label.to_owned())
        .size(11)
        .color(OryxisColors::t().text_muted)
        .into()
}

fn jump_row<'a>(
    icon: Element<'a, Message>,
    label: String,
    is_active: bool,
    on_select: Message,
) -> Element<'a, Message> {
    let bg = if is_active {
        Color { a: 0.15, ..OryxisColors::t().accent }
    } else {
        Color::TRANSPARENT
    };
    let label_color = if is_active {
        OryxisColors::t().accent
    } else {
        OryxisColors::t().text_primary
    };
    button(
        row![
            icon,
            Space::new().width(8),
            text(label).size(13).color(label_color),
        ]
        .align_y(iced::Alignment::Center),
    )
    .on_press_with(move || {
        // Two-step dispatch: select first, then close — keeps the
        // modal from flashing closed before the select handler runs.
        // SequencedSelect is wired in app.rs to fire both messages.
        Message::TabJumpSelect(Box::new(on_select.clone()))
    })
    .padding(Padding { top: 6.0, right: 12.0, bottom: 6.0, left: 12.0 })
    .width(Length::Fill)
    .style(move |_, status| {
        let hover_bg = match status {
            BtnStatus::Hovered if !is_active => OryxisColors::t().bg_hover,
            _ => bg,
        };
        button::Style {
            background: Some(Background::Color(hover_bg)),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    })
    .into()
}
