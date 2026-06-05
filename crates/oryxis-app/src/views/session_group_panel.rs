//! Session-group editor modal: name, folder, color, and a per-pane startup
//! script for each pane in the saved arrangement. Rendered as a centered
//! modal (over a scrim) from `root_view`, so it works whether it was opened
//! from a terminal tab or a dashboard card without leaking input into the
//! terminal underneath.

use iced::border::Radius;
use iced::widget::{button, column, container, scrollable, text, text_input, MouseArea, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis};
use crate::i18n::t;
use crate::theme::OryxisColors;
use crate::widgets::{dir_align_x, dir_row, panel_field, panel_section};

impl Oryxis {
    pub(crate) fn view_session_group_panel(&self) -> Element<'_, Message> {
        let form = &self.editor_session_group;
        let is_editing = form.editing_id.is_some();
        let title = if is_editing {
            t("session_group_edit_title")
        } else {
            t("session_group_new_title")
        };

        // ── Header ──
        let panel_header = container(
            dir_row(vec![
                text(title)
                    .size(16)
                    .color(OryxisColors::t().text_primary)
                    .into(),
                Space::new().width(Length::Fill).into(),
                button(
                    iced_fonts::lucide::x()
                        .size(14)
                        .color(OryxisColors::t().text_muted),
                )
                .on_press(Message::SessionGroupFormCancel)
                .padding(Padding {
                    top: 4.0,
                    right: 8.0,
                    bottom: 4.0,
                    left: 8.0,
                })
                .style(|_, _| button::Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    border: Border::default(),
                    ..Default::default()
                })
                .into(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding {
            top: 16.0,
            right: 16.0,
            bottom: 12.0,
            left: 16.0,
        });

        // ── Section: General ──
        let general_section = panel_section(column![
            panel_field(
                t("session_group_label"),
                text_input(t("session_group_label_placeholder"), &form.label)
                    .on_input(Message::SessionGroupFormLabelChanged)
                    .on_submit(Message::SessionGroupFormSave)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x())
                    .into(),
            ),
            Space::new().height(10),
            panel_field(
                t("session_group_folder"),
                text_input(t("group_placeholder"), &form.group_name)
                    .on_input(Message::SessionGroupFormGroupChanged)
                    .on_submit(Message::SessionGroupFormSave)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x())
                    .into(),
            ),
            Space::new().height(10),
            panel_field(
                t("session_group_color"),
                text_input("#3b82f6", form.color.as_deref().unwrap_or(""))
                    .on_input(Message::SessionGroupFormColorChanged)
                    .on_submit(Message::SessionGroupFormSave)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x())
                    .into(),
            ),
        ]);

        // ── Section: Panes (one pane shown at a time; chevrons step) ──
        let panes_section: Element<'_, Message> = if form.pane_rows.is_empty() {
            Space::new().height(0).into()
        } else {
            let total = form.pane_rows.len();
            let cur = form.current_pane.min(total - 1);
            let current_label = form
                .pane_rows
                .get(cur)
                .map(|r| r.label.clone())
                .unwrap_or_default();

            // Chevron nav buttons. Disabled (no on_press, dimmed) at the ends.
            let nav_btn = |glyph: iced::widget::Text<'static>, enabled: bool, msg: Message| {
                let color = if enabled {
                    OryxisColors::t().text_secondary
                } else {
                    OryxisColors::t().text_muted
                };
                let mut b = button(glyph.size(14).color(color))
                    .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                    .style(move |_, status| {
                        let bg = match status {
                            iced::widget::button::Status::Hovered if enabled => {
                                OryxisColors::t().bg_hover
                            }
                            _ => Color::TRANSPARENT,
                        };
                        button::Style {
                            background: Some(Background::Color(bg)),
                            border: Border { radius: Radius::from(6.0), ..Default::default() },
                            ..Default::default()
                        }
                    });
                if enabled {
                    b = b.on_press(msg);
                }
                b
            };

            // Header: [<]  "Pane i/N + label"  [>]
            let counter = column![
                text(format!("{} / {}", cur + 1, total))
                    .size(11)
                    .color(OryxisColors::t().text_muted),
                text(current_label)
                    .size(13)
                    .color(OryxisColors::t().text_primary)
                    .wrapping(iced::widget::text::Wrapping::None),
            ]
            .spacing(2)
            .align_x(iced::Alignment::Center)
            .width(Length::Fill);

            let header = dir_row(vec![
                nav_btn(
                    iced_fonts::lucide::chevron_left(),
                    cur > 0,
                    Message::SessionGroupPaneNav(false),
                )
                .into(),
                counter.into(),
                nav_btn(
                    iced_fonts::lucide::chevron_right(),
                    cur + 1 < total,
                    Message::SessionGroupPaneNav(true),
                )
                .into(),
            ])
            .align_y(iced::Alignment::Center)
            .width(Length::Fill);

            let editor = container(
                iced::widget::text_editor(&self.session_group_script_editor)
                    .placeholder(t("session_group_pane_script_placeholder"))
                    .on_action(Message::SessionGroupScriptAction)
                    .padding(10)
                    .height(Length::Shrink)
                    .style(crate::widgets::rounded_editor_style),
            )
            .max_height(200.0);

            panel_section(
                column![
                    text(t("session_group_panes"))
                        .size(12)
                        .color(OryxisColors::t().text_muted),
                    Space::new().height(8),
                    header,
                    Space::new().height(8),
                    editor,
                ]
                .width(Length::Fill),
            )
        };

        // ── Error ──
        let panel_error: Element<'_, Message> = if let Some(err) = &self.session_group_panel_error {
            container(Element::from(
                text(err.clone()).size(11).color(OryxisColors::t().error),
            ))
            .padding(Padding {
                top: 4.0,
                right: 16.0,
                bottom: 4.0,
                left: 16.0,
            })
            .into()
        } else {
            Space::new().height(0).into()
        };

        // ── Bottom actions ──
        let save_btn_bg = if form.label.trim().is_empty() {
            OryxisColors::t().bg_surface
        } else {
            OryxisColors::t().accent
        };
        let save_btn = button(
            container(
                text(t("save"))
                    .size(14)
                    .color(OryxisColors::t().text_primary),
            )
            .padding(Padding {
                top: 12.0,
                right: 0.0,
                bottom: 12.0,
                left: 0.0,
            })
            .width(Length::Fill)
            .center_x(Length::Fill),
        )
        .on_press(Message::SessionGroupFormSave)
        .width(Length::Fill)
        .style(move |_, _| button::Style {
            background: Some(Background::Color(save_btn_bg)),
            border: Border {
                radius: Radius::from(8.0),
                ..Default::default()
            },
            ..Default::default()
        });

        let bottom = column![panel_error, save_btn].spacing(8);

        // Body scrolls inside a capped height so the modal never grows past
        // the window on a many-pane group.
        let form_scroll = scrollable(
            column![general_section, Space::new().height(8), panes_section,].padding(Padding {
                top: 0.0,
                right: 16.0,
                bottom: 16.0,
                left: 16.0,
            }),
        )
        .height(Length::Fixed(380.0));

        let panel_content = column![
            panel_header,
            form_scroll,
            container(bottom).padding(Padding {
                top: 8.0,
                right: 16.0,
                bottom: 16.0,
                left: 16.0,
            }),
        ];

        // Fixed-width card, centered over the scrim. The MouseArea(NoOp)
        // swallows clicks on the card so they don't reach the scrim and
        // dismiss it (same pattern as the local-shell picker).
        let card = MouseArea::new(
            container(panel_content)
                .width(Length::Fixed(440.0))
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().bg_surface)),
                    border: Border {
                        color: OryxisColors::t().border,
                        width: 1.0,
                        radius: Radius::from(12.0),
                    },
                    shadow: iced::Shadow {
                        color: Color::from_rgba(0.0, 0.0, 0.0, 0.30),
                        offset: iced::Vector::new(0.0, 8.0),
                        blur_radius: 24.0,
                    },
                    ..Default::default()
                }),
        )
        .on_press(Message::NoOp);

        container(card)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }
}
