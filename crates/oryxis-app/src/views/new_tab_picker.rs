//! New-tab picker — centered modal overlay with a search bar and the list of
//! recent connections. Triggered from the `+` button in the tab bar.
//!
//! Visually modeled on Termius' "New Tab" screen: big rounded search at the
//! top, then a grouped list with host-icon badges and a "Personal / Group"
//! breadcrumb on the right.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, row, scrollable, text, text_input, MouseArea, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis};
use crate::theme::OryxisColors;

impl Oryxis {
    /// Build the new-tab picker modal. The caller is responsible for checking
    /// `self.show_new_tab_picker` before rendering and stacking it on top of
    /// the base view.
    pub(crate) fn view_new_tab_picker(&self) -> Element<'_, Message> {
        let search = text_input("Search hosts or tabs", &self.new_tab_picker_search)
            .on_input(Message::NewTabPickerSearchChanged)
            .padding(14)
            .size(14)
            .style(|_, status| text_input::Style {
                background: Background::Color(OryxisColors::t().bg_surface),
                border: Border {
                    radius: Radius::from(8.0),
                    width: match status {
                        text_input::Status::Focused { .. } => 1.5,
                        _ => 1.0,
                    },
                    color: match status {
                        text_input::Status::Focused { .. } => OryxisColors::t().accent,
                        _ => OryxisColors::t().border,
                    },
                },
                icon: OryxisColors::t().text_muted,
                placeholder: OryxisColors::t().text_muted,
                value: OryxisColors::t().text_primary,
                selection: OryxisColors::t().accent,
            });

        let ctrl_k_hint = text("Ctrl+K").size(11).color(OryxisColors::t().text_muted);

        let search_bar = container(
            row![
                search,
            ]
            .align_y(iced::Alignment::Center),
        )
        .width(Length::Fill);

        // Hint to the right of the search (overlaid-ish; for now a separate
        // column row since we don't have input-suffix in iced stable).
        let search_block = column![
            search_bar,
            Space::new().height(4),
            row![
                Space::new().width(Length::Fill),
                ctrl_k_hint,
                Space::new().width(12),
            ],
        ];

        // Filter + order: most-recently-used first, fall back to stable order.
        let needle = self.new_tab_picker_search.to_lowercase();
        let mut idxs: Vec<usize> = (0..self.connections.len())
            .filter(|&i| {
                if needle.is_empty() {
                    return true;
                }
                let c = &self.connections[i];
                c.label.to_lowercase().contains(&needle)
                    || c.hostname.to_lowercase().contains(&needle)
            })
            .collect();
        // Sort by last_used desc (most recent first). None → pushed to end.
        idxs.sort_by(|a, b| {
            let la = self.connections[*a].last_used;
            let lb = self.connections[*b].last_used;
            lb.cmp(&la)
        });

        // Header row of the list.
        let list_header = row![
            text("Recent connections").size(13).font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..iced::Font::with_name("Inter")
            }).color(OryxisColors::t().text_primary),
            Space::new().width(Length::Fill),
        ]
        .align_y(iced::Alignment::Center);

        // Connection rows.
        let mut rows: Vec<Element<'_, Message>> = Vec::new();
        for (pos, ci) in idxs.iter().enumerate() {
            let conn = &self.connections[*ci];
            // Group breadcrumb: "Personal / <group>" if grouped, else "Personal".
            let group_name = conn.group_id.and_then(|gid| {
                self.groups.iter().find(|g| g.id == gid).map(|g| g.label.clone())
            });
            let breadcrumb = match group_name {
                Some(g) => format!("Personal / {}", g),
                None => "Personal".to_string(),
            };
            // Zebra stripe: odd rows get a subtle lighter bg.
            let zebra_bg = if pos % 2 == 1 {
                OryxisColors::t().bg_hover
            } else {
                Color::TRANSPARENT
            };
            rows.push(picker_row(*ci, &conn.label, breadcrumb, zebra_bg));
        }

        if rows.is_empty() {
            rows.push(
                container(
                    text(if needle.is_empty() {
                        "No connections yet. Create one from the dashboard."
                    } else {
                        "No matches."
                    })
                    .size(13)
                    .color(OryxisColors::t().text_muted),
                )
                .padding(Padding { top: 24.0, right: 16.0, bottom: 24.0, left: 16.0 })
                .center_x(Length::Fill)
                .into(),
            );
        }

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
                search_block,
                Space::new().height(16),
                list_scroll,
            ],
        )
        .padding(24)
        .width(Length::Fixed(780.0))
        .height(Length::Fixed(640.0))
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

fn picker_row<'a>(
    conn_idx: usize,
    label: &'a str,
    breadcrumb: String,
    zebra_bg: Color,
) -> Element<'a, Message> {
    // Icon badge — 26×26 accent square with server glyph.
    let icon_box = container(
        iced_fonts::lucide::server().size(12).color(Color::WHITE),
    )
    .width(Length::Fixed(26.0))
    .height(Length::Fixed(26.0))
    .center_x(Length::Fixed(26.0))
    .center_y(Length::Fixed(26.0))
    .style(|_| container::Style {
        background: Some(Background::Color(OryxisColors::t().accent)),
        border: Border { radius: Radius::from(6.0), ..Default::default() },
        ..Default::default()
    });

    let label_text = text(label.to_string()).size(13).font(iced::Font {
        weight: iced::font::Weight::Semibold,
        ..iced::Font::with_name("Inter")
    }).color(OryxisColors::t().text_primary);

    let breadcrumb_text = text(breadcrumb).size(12).color(OryxisColors::t().accent);

    let inner = row![
        icon_box,
        Space::new().width(12),
        label_text,
        Space::new().width(Length::Fill),
        breadcrumb_text,
    ]
    .align_y(iced::Alignment::Center);

    button(
        container(inner)
            .padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 })
            .width(Length::Fill),
    )
    .on_press(Message::ConnectSsh(conn_idx))
    .width(Length::Fill)
    .style(move |_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().bg_hover,
            _ => zebra_bg,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    })
    .into()
}

/// Transparent backdrop that dismisses the picker on click. Meant to be
/// stacked below the picker body.
pub(crate) fn new_tab_picker_backdrop<'a>() -> Element<'a, Message> {
    MouseArea::new(
        container(Space::new()).width(Length::Fill).height(Length::Fill),
    )
    .on_press(Message::HideNewTabPicker)
    .into()
}
