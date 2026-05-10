//! Jump host picker. Centered modal opened from the "Jump Host" row in
//! the host editor's Advanced section. Lists every saved connection
//! (except the one being edited), filtered by a search input that
//! matches label, hostname, username, or group name. Selecting a row
//! fires `EditorJumpHostChanged(label)`; the "(none)" entry clears it.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, scrollable, text, text_input, MouseArea, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis};
use crate::i18n::t;
use crate::theme::OryxisColors;
use crate::widgets::dir_row;

impl Oryxis {
    pub(crate) fn view_jump_host_picker(&self) -> Element<'_, Message> {
        let needle = self.jump_host_search.to_lowercase();
        let editing_id = self.editor_form.editing_id;

        let search = text_input(t("search_hosts_or_tabs"), &self.jump_host_search)
            .on_input(Message::JumpHostSearchChanged)
            .padding(Padding { top: 14.0, right: 14.0, bottom: 14.0, left: 14.0 })
            .size(14)
            .style(crate::widgets::rounded_input_style);

        let mut rows: Vec<Element<'_, Message>> = Vec::new();

        // "(none)" entry always at the top, so the user can clear the
        // current jump host without leaving the picker.
        let none_selected = self.editor_form.jump_host.is_none();
        rows.push(picker_row_simple(
            t("disabled"),
            None,
            none_selected,
            Color::TRANSPARENT,
            Message::EditorJumpHostChanged("(none)".into()),
        ));

        let mut idxs: Vec<usize> = (0..self.connections.len())
            .filter(|&i| {
                let c = &self.connections[i];
                if Some(c.id) == editing_id {
                    return false;
                }
                if needle.is_empty() {
                    return true;
                }
                let group = c
                    .group_id
                    .and_then(|gid| self.groups.iter().find(|g| g.id == gid))
                    .map(|g| g.label.to_lowercase())
                    .unwrap_or_default();
                let user = c.username.as_deref().unwrap_or("").to_lowercase();
                c.label.to_lowercase().contains(&needle)
                    || c.hostname.to_lowercase().contains(&needle)
                    || user.contains(&needle)
                    || group.contains(&needle)
            })
            .collect();
        idxs.sort_by(|a, b| {
            self.connections[*a]
                .label
                .to_lowercase()
                .cmp(&self.connections[*b].label.to_lowercase())
        });

        for (pos, ci) in idxs.iter().enumerate() {
            let conn = &self.connections[*ci];
            let group_name = conn
                .group_id
                .and_then(|gid| self.groups.iter().find(|g| g.id == gid))
                .map(|g| g.label.clone());
            let breadcrumb = match group_name {
                Some(g) => format!("{} / {}", t("personal"), g),
                None => t("personal").to_string(),
            };
            let zebra_bg = if pos % 2 == 1 {
                OryxisColors::t().bg_hover
            } else {
                Color::TRANSPARENT
            };
            let is_selected = self.editor_form.jump_host.as_deref() == Some(conn.label.as_str());
            rows.push(picker_row_simple(
                &conn.label,
                Some(breadcrumb),
                is_selected,
                zebra_bg,
                Message::EditorJumpHostChanged(conn.label.clone()),
            ));
        }

        if rows.len() == 1 && !needle.is_empty() {
            // Only the "(none)" row matched; show empty state below it.
            rows.push(
                container(
                    text(t("no_matches")).size(13).color(OryxisColors::t().text_muted),
                )
                .padding(Padding { top: 24.0, right: 16.0, bottom: 24.0, left: 16.0 })
                .center_x(Length::Fill)
                .into(),
            );
        }

        let list_header = dir_row(vec![
            text(t("jump_host")).size(13).font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            }).color(OryxisColors::t().text_primary).into(),
            Space::new().width(Length::Fill).into(),
        ])
        .align_y(iced::Alignment::Center);

        let list_panel = container(
            column![list_header, Space::new().height(8), column(rows)],
        )
        .padding(Padding { top: 14.0, right: 16.0, bottom: 14.0, left: 16.0 })
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border {
                radius: Radius::from(10.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        });

        let list_scroll = scrollable(list_panel).height(Length::Fill);

        let body = container(
            column![
                search,
                Space::new().height(16),
                list_scroll,
            ],
        )
        .padding(24)
        .width(Length::Fixed(640.0))
        .height(Length::Fixed(560.0))
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_primary)),
            border: Border {
                radius: Radius::from(12.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        });

        let body_trap: Element<'_, Message> = MouseArea::new(body)
            .on_press(Message::NoOp)
            .into();

        let centered = container(body_trap)
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
        .on_press(Message::HideJumpHostPicker)
        .into()
    }
}

fn picker_row_simple<'a>(
    label: &'a str,
    breadcrumb: Option<String>,
    is_selected: bool,
    zebra_bg: Color,
    on_press: Message,
) -> Element<'a, Message> {
    let label_text = text(label.to_string()).size(13).font(iced::Font {
        weight: iced::font::Weight::Semibold,
        ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
    }).color(OryxisColors::t().text_primary);

    let mut items: Vec<Element<'a, Message>> = vec![
        label_text.into(),
        Space::new().width(Length::Fill).into(),
    ];
    if let Some(b) = breadcrumb {
        items.push(text(b).size(12).color(OryxisColors::t().accent).into());
    }
    let inner = dir_row(items).align_y(iced::Alignment::Center);

    let resting_bg = if is_selected { OryxisColors::t().bg_selected } else { zebra_bg };
    let border_color = if is_selected { OryxisColors::t().accent } else { Color::TRANSPARENT };
    let border_width = if is_selected { 1.0 } else { 0.0 };

    button(
        container(inner)
            .padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 })
            .width(Length::Fill),
    )
    .on_press(on_press)
    .width(Length::Fill)
    .style(move |_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().bg_hover,
            _ => resting_bg,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), color: border_color, width: border_width },
            ..Default::default()
        }
    })
    .into()
}
