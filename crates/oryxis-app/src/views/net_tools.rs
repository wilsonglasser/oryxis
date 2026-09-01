//! The network tools panel: pick a tool, name a target, read the cards.
//!
//! A panel tab like Settings (`PanelKind::NetTools`), not a vault
//! surface: it answers questions ABOUT the network rather than managing
//! anything stored in the vault, and it is reached from the burger menu
//! only while `network_tools_enabled` is on.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, pick_list, scrollable, text, text_input, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, NetToolsMessage, Oryxis};
use crate::i18n::t;
use crate::keynav::RowAction;
use crate::net_tools::{CardStatus, NetTool, NetToolCard};
use crate::theme::OryxisColors;
use crate::widgets::{dir_align_x, dir_row, panel_field, panel_section, styled_button};

/// Id of the target input, so the panel can focus it on open the way
/// every other surface focuses its first field.
pub(crate) const TARGET_INPUT_ID: &str = "net-tools-target";

impl Oryxis {
    pub(crate) fn view_net_tools(&self) -> Element<'_, Message> {
        // Keyboard rows are recorded in visual order.
        self.keynav_settings_reset();
        let running = self.net_tools.running.is_some();

        let header = column![
            text(t("network_tools")).size(20).color(OryxisColors::t().text_primary),
            Space::new().height(4),
            text(t(self.net_tools.tool.hint_key()))
                .size(12)
                .color(OryxisColors::t().text_muted),
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());

        let controls = panel_section(column![
            self.tool_picker(),
            Space::new().height(12),
            self.target_row(running),
        ]);

        let mut body: Vec<Element<'_, Message>> = vec![
            header.into(),
            Space::new().height(16).into(),
            controls,
            Space::new().height(16).into(),
        ];

        if let Some(err) = &self.net_tools.error {
            body.push(
                container(text(err.clone()).size(12).color(OryxisColors::t().error))
                    .padding(12)
                    .width(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(Background::Color(Color {
                            a: 0.12,
                            ..OryxisColors::t().error
                        })),
                        border: Border {
                            radius: Radius::from(8.0),
                            color: OryxisColors::t().error,
                            width: 1.0,
                        },
                        ..Default::default()
                    })
                    .into(),
            );
            body.push(Space::new().height(12).into());
        }

        if running {
            body.push(
                text(format!(
                    "{} {}",
                    t("net_running"),
                    self.net_tools.last_run.clone().unwrap_or_default()
                ))
                .size(12)
                .color(OryxisColors::t().text_muted)
                .into(),
            );
            body.push(Space::new().height(12).into());
        } else if let Some(heading) = &self.net_tools.last_run
            && !self.net_tools.cards.is_empty()
        {
            body.push(
                text(heading.clone()).size(13).color(OryxisColors::t().text_secondary).into(),
            );
            body.push(Space::new().height(8).into());
        }

        for (idx, card) in self.net_tools.cards.iter().enumerate() {
            body.push(self.result_card(idx, card));
            body.push(Space::new().height(10).into());
        }

        if self.net_tools.cards.is_empty() && !running && self.net_tools.error.is_none() {
            body.push(
                text(t("net_empty_hint")).size(12).color(OryxisColors::t().text_muted).into(),
            );
        }

        container(
            scrollable(
                container(column(body).width(Length::Fill))
                    .padding(Padding {
                        top: 24.0,
                        right: 28.0,
                        bottom: 24.0,
                        left: 28.0,
                    })
                    .width(Length::Fill),
            )
            .id(iced::widget::Id::new("net-tools-scroll"))
            .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_primary)),
            ..Default::default()
        })
        .into()
    }

    /// The tool selector. A `pick_list` rather than a pill row: seven
    /// entries would wrap on a narrow window, and the hint line below
    /// already carries what each one does.
    fn tool_picker(&self) -> Element<'_, Message> {
        let current = self.net_tools.tool;
        let (prev, next) = crate::keynav::slots::cycle_pair(&NetTool::ALL, &current, |tool| {
            Message::NetTools(NetToolsMessage::Select(tool))
        });
        let picker = pick_list(Some(current), NetTool::ALL, |tool: &NetTool| tool.to_string())
            .on_select(|tool| Message::NetTools(NetToolsMessage::Select(tool)))
            .padding(10)
            .width(260)
            .into();
        self.settings_nav_slot(
            RowAction::picker(prev, next),
            8.0,
            panel_field(t("net_tool"), picker),
        )
    }

    /// Target (+ ports for the port test) and the Run / Cancel button.
    fn target_row(&self, running: bool) -> Element<'_, Message> {
        let tool = self.net_tools.tool;
        let target = text_input(t(tool.target_placeholder_key()), &self.net_tools.target)
            .id(iced::widget::Id::new(TARGET_INPUT_ID))
            .on_input(|v| Message::NetTools(NetToolsMessage::Target(v)))
            // Enter runs, which is the whole gesture for a tool like
            // this. The panel is a full view, never under a modal, so
            // the fork's unfocused-`on_submit` trap does not apply here.
            .on_submit(Message::NetTools(NetToolsMessage::Run))
            .padding(10)
            .width(Length::Fill)
            .style(crate::widgets::rounded_input_style)
            .align_x(dir_align_x());
        let target = self.settings_nav_slot(
            RowAction::input(iced::widget::Id::new(TARGET_INPUT_ID)),
            8.0,
            panel_field(t("net_target"), target.into()),
        );

        let mut row: Vec<Element<'_, Message>> = vec![container(target).width(Length::Fill).into()];

        if tool.needs_ports() {
            let ports = text_input(t("net_ports_ph"), &self.net_tools.ports)
                .id(iced::widget::Id::new("net-tools-ports"))
                .on_input(|v| Message::NetTools(NetToolsMessage::Ports(v)))
                .on_submit(Message::NetTools(NetToolsMessage::Run))
                .padding(10)
                .width(200)
                .style(crate::widgets::rounded_input_style)
                .align_x(dir_align_x());
            row.push(Space::new().width(12).into());
            row.push(self.settings_nav_slot(
                RowAction::input(iced::widget::Id::new("net-tools-ports")),
                8.0,
                panel_field(t("net_ports"), ports.into()),
            ));
        }

        // Run becomes Cancel while a run is in flight, so the control
        // never lies about what pressing it does.
        let (label, msg, color) = if running {
            (t("cancel"), NetToolsMessage::Cancel, OryxisColors::t().bg_selected)
        } else {
            (t("net_run"), NetToolsMessage::Run, OryxisColors::t().accent)
        };
        let action = self.settings_nav_slot(
            RowAction::activate(Message::NetTools(msg.clone())),
            6.0,
            styled_button(label, Message::NetTools(msg), color),
        );
        row.push(Space::new().width(12).into());
        // The button sits on the field row, so it needs the label's own
        // height above it to line up with the inputs beside it.
        row.push(
            column![Space::new().height(18), action]
                .align_x(iced::Alignment::Start)
                .into(),
        );

        dir_row(row).align_y(iced::Alignment::Start).into()
    }

    /// One result card: a status-tinted left edge, the title, the lines,
    /// and a hover-revealed copy action floating over the top corner.
    fn result_card<'a>(&'a self, idx: usize, card: &'a NetToolCard) -> Element<'a, Message> {
        let accent = status_color(card.status);
        let lines = card.lines.iter().fold(
            column![
                text(card.title.clone()).size(13).color(OryxisColors::t().text_primary),
                Space::new().height(6),
            ],
            |col, line| {
                col.push(
                    text(line.clone())
                        .size(12)
                        .font(iced::Font::MONOSPACE)
                        .color(OryxisColors::t().text_secondary),
                )
                .push(Space::new().height(2))
            },
        );
        let body = container(lines.width(Length::Fill).align_x(dir_align_x()))
            .padding(Padding { top: 12.0, right: 14.0, bottom: 12.0, left: 14.0 })
            .width(Length::Fill)
            .style(move |_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_hover)),
                border: Border { radius: Radius::from(8.0), color: accent, width: 1.0 },
                ..Default::default()
            });

        // Floating, hover-revealed, in a Stack overlay: the card-action
        // convention, so the copy icon reserves no inline width and the
        // card content never shifts when the pointer arrives.
        let overlay: Element<'a, Message> = if self.hover.net_tools_card == Some(idx) {
            // The TRAILING corner, which is the physical left one under
            // RTL. `dir_align_x` answers for the leading edge, so this is
            // its opposite rather than a second call to it; the padding
            // has to swap sides with it or the icon would hug the frame.
            let rtl = crate::i18n::is_rtl_layout();
            let align = if rtl {
                iced::alignment::Horizontal::Left
            } else {
                iced::alignment::Horizontal::Right
            };
            let (left, right) = if rtl { (10.0, 0.0) } else { (0.0, 10.0) };
            container(copy_button(idx))
                .width(Length::Fill)
                .align_x(align)
                .padding(Padding { top: 8.0, right, bottom: 0.0, left })
                .into()
        } else {
            Space::new().into()
        };

        iced::widget::MouseArea::new(
            iced::widget::Stack::new().push(body).push(overlay).width(Length::Fill),
        )
        .on_enter(Message::NetTools(NetToolsMessage::ResultHovered(idx)))
        .on_exit(Message::NetTools(NetToolsMessage::ResultUnhovered(idx)))
        .into()
    }
}

/// The copy affordance, with the hover / press feedback every clickable
/// control owes the user plus a tooltip (it is icon-only).
fn copy_button<'a>(idx: usize) -> Element<'a, Message> {
    let btn = button(
        container(iced_fonts::lucide::copy().size(12).color(OryxisColors::t().text_secondary))
            .center_x(Length::Fixed(22.0))
            .center_y(Length::Fixed(22.0)),
    )
    .padding(0)
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().bg_selected,
            BtnStatus::Pressed => OryxisColors::t().accent,
            _ => OryxisColors::t().bg_surface,
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
    .on_press(Message::NetTools(NetToolsMessage::CopyCard(idx)));
    iced::widget::tooltip(
        btn,
        container(text(t("net_copy_tip")).size(11).color(OryxisColors::t().text_primary))
            .padding(6)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    radius: Radius::from(6.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            }),
        iced::widget::tooltip::Position::Left,
    )
    .into()
}

/// Border colour per verdict. Neutral cards take the ordinary border, so
/// only cards that MEAN something carry a colour.
fn status_color(status: CardStatus) -> Color {
    match status {
        CardStatus::Ok => OryxisColors::t().success,
        CardStatus::Warn => OryxisColors::t().warning,
        CardStatus::Bad => OryxisColors::t().error,
        CardStatus::Neutral => OryxisColors::t().border,
    }
}
