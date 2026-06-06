//! Host-chaining editor. Centered modal opened from the "Host
//! Chaining" row in the host editor's Advanced section. Edits the
//! ordered `editor_form.jump_chain` (a `Vec<Uuid>`): the SSH session
//! tunnels through each hop in order before reaching the host being
//! edited.
//!
//! Two modes share one modal, switched by `chain_editor_adding`:
//!   - list mode: the current chain as ordered cards (reorder + remove)
//!     ending in the destination host, plus an "Add a Host" button;
//!   - add mode: a searchable host list whose selection appends a hop.
//!
//! Replaces the old single-host `jump_host_picker`: that picker and the
//! read-only "Host Chaining" row both edited the same field and only
//! ever exposed one hop, even though the model and SSH engine already
//! support arbitrary-length chains.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, scrollable, text, text_input, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use oryxis_core::models::Connection;

use crate::app::{Message, Oryxis};
use crate::i18n::t;
use crate::theme::OryxisColors;
use crate::widgets::{dir_align_x, dir_row, HostIconStyle};

impl Oryxis {
    pub(crate) fn view_chain_editor(&self) -> Element<'_, Message> {
        let inner: Element<'_, Message> = if self.chain_editor_adding {
            self.chain_editor_add_view()
        } else {
            self.chain_editor_list_view()
        };

        let body = container(inner)
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

        // Bare card; `widgets::modal_overlay` (the caller) owns centering,
        // the absorbing scrim, and the click-trap. The caller also picks the
        // context-dependent scrim dismiss (pop the add sub-view vs close).
        body.into()
    }

    /// List mode: the ordered chain, the destination host, and the
    /// "Add a Host" / "Done" actions.
    fn chain_editor_list_view(&self) -> Element<'_, Message> {
        let header = dir_row(vec![
            text(t("host_chaining"))
                .size(16)
                .color(OryxisColors::t().text_primary)
                .into(),
            Space::new().width(Length::Fill).into(),
            chain_icon_button(iced_fonts::lucide::x(), Message::CloseChainEditor, false),
        ])
        .align_y(iced::Alignment::Center);

        let desc = text(t("chain_editor_desc"))
            .size(12)
            .color(OryxisColors::t().text_muted);

        let mut items: Vec<Element<'_, Message>> = Vec::new();
        let total = self.editor_form.jump_chain.len();

        if total == 0 {
            items.push(
                container(
                    text(t("chain_editor_empty"))
                        .size(13)
                        .color(OryxisColors::t().text_muted),
                )
                .padding(Padding {
                    top: 18.0,
                    right: 14.0,
                    bottom: 18.0,
                    left: 14.0,
                })
                .center_x(Length::Fill)
                .into(),
            );
        }

        for (idx, id) in self.editor_form.jump_chain.iter().enumerate() {
            let conn = self.connections.iter().find(|c| c.id == *id);
            let badge = self.chain_host_badge(conn);
            let (label, breadcrumb) = match conn {
                Some(c) => (c.label.clone(), self.breadcrumb_for(c)),
                None => (t("unknown").to_string(), t("chain_hop_missing").to_string()),
            };
            items.push(self.chain_hop_card(idx, total, badge, label, breadcrumb));
            items.push(chain_connector());
        }

        // Destination card: the host being edited, shown as the final
        // node so the user reads the chain end to end. Not removable.
        // Resolve its badge from the saved host (a brand-new host has no
        // id yet, so it falls back to the generic badge).
        let dest_conn = self
            .editor_form
            .editing_id
            .and_then(|id| self.connections.iter().find(|c| c.id == id));
        let dest_badge = self.chain_host_badge(dest_conn);
        let dest_label = if self.editor_form.label.trim().is_empty() {
            t("new_host").to_string()
        } else {
            self.editor_form.label.clone()
        };
        items.push(destination_card(dest_badge, dest_label));

        // Right padding keeps the cards (and their trash buttons) clear
        // of the scrollbar once the chain is long enough to scroll.
        let list = scrollable(
            container(column(items).spacing(0)).padding(Padding {
                top: 0.0,
                right: 10.0,
                bottom: 0.0,
                left: 0.0,
            }),
        )
        .height(Length::Fill);

        let add_btn = container(button(
            dir_row(vec![
                iced_fonts::lucide::plus()
                    .size(14)
                    .color(OryxisColors::t().accent)
                    .into(),
                Space::new().width(8).into(),
                text(t("add_a_host"))
                    .size(13)
                    .color(OryxisColors::t().accent)
                    .into(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::ChainEditorStartAdd)
        .padding(Padding {
            top: 10.0,
            right: 14.0,
            bottom: 10.0,
            left: 14.0,
        })
        .width(Length::Fill)
        .style(|_, status| {
            let bg = match status {
                BtnStatus::Hovered => OryxisColors::t().bg_hover,
                _ => OryxisColors::t().bg_surface,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border {
                    radius: Radius::from(8.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            }
        }))
        .center_x(Length::Fill);

        column![
            header,
            Space::new().height(8),
            desc,
            Space::new().height(16),
            list,
            Space::new().height(12),
            add_btn,
        ]
        .into()
    }

    /// Add mode: searchable host list whose selection appends a hop.
    fn chain_editor_add_view(&self) -> Element<'_, Message> {
        let header = dir_row(vec![
            chain_icon_button(
                iced_fonts::lucide::arrow_left(),
                Message::ChainEditorCancelAdd,
                false,
            ),
            Space::new().width(10).into(),
            text(t("add_a_host"))
                .size(16)
                .color(OryxisColors::t().text_primary)
                .into(),
        ])
        .align_y(iced::Alignment::Center);

        let search = text_input(t("search_hosts_or_tabs"), &self.chain_editor_search)
            .on_input(Message::ChainEditorSearchChanged)
            .padding(Padding {
                top: 14.0,
                right: 14.0,
                bottom: 14.0,
                left: 14.0,
            })
            .size(14)
            .style(crate::widgets::rounded_input_style)
            .align_x(dir_align_x());

        let needle = self.chain_editor_search.to_lowercase();
        let editing_id = self.editor_form.editing_id;

        let mut idxs: Vec<usize> = (0..self.connections.len())
            .filter(|&i| {
                let c = &self.connections[i];
                // Exclude the host being edited (a self-hop is a loop)
                // and any host already in the chain (no duplicates).
                if Some(c.id) == editing_id {
                    return false;
                }
                if self.editor_form.jump_chain.contains(&c.id) {
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

        let mut rows: Vec<Element<'_, Message>> = Vec::new();
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
            let badge = self.chain_host_badge(Some(conn));
            rows.push(pick_row(
                badge,
                &conn.label,
                breadcrumb,
                zebra_bg,
                Message::ChainEditorAddHop(conn.id),
            ));
        }

        if rows.is_empty() {
            rows.push(
                container(
                    text(t("no_matches"))
                        .size(13)
                        .color(OryxisColors::t().text_muted),
                )
                .padding(Padding {
                    top: 24.0,
                    right: 16.0,
                    bottom: 24.0,
                    left: 16.0,
                })
                .center_x(Length::Fill)
                .into(),
            );
        }

        let list = scrollable(
            container(column(rows)).padding(Padding {
                top: 0.0,
                right: 10.0,
                bottom: 0.0,
                left: 0.0,
            }),
        )
        .height(Length::Fill);

        column![header, Space::new().height(16), search, Space::new().height(12), list].into()
    }

    /// "Personal / Group" breadcrumb for a connection, or just
    /// "Personal" when it has no group.
    fn breadcrumb_for(&self, c: &Connection) -> String {
        match c
            .group_id
            .and_then(|gid| self.groups.iter().find(|g| g.id == gid))
        {
            Some(g) => format!("{} / {}", t("personal"), g.label),
            None => t("personal").to_string(),
        }
    }

    /// Build the colored host badge (custom icon + color, falling back
    /// to the detected-OS glyph) shown on each chain node. Mirrors the
    /// resolution used in the tab bar and new-tab picker so a host looks
    /// the same everywhere. A `None` connection (a since-deleted hop)
    /// gets a neutral muted circle.
    fn chain_host_badge<'a>(&self, conn: Option<&Connection>) -> Element<'a, Message> {
        const SIZE: f32 = 26.0;
        match conn {
            Some(c) => {
                let fallback = OryxisColors::t().accent;
                let (glyph, badge_color) = if let Some(name) = c.custom_icon.as_deref() {
                    (
                        crate::os_icon::custom_icon_glyph(name),
                        c.custom_color
                            .as_deref()
                            .and_then(crate::widgets::parse_hex_color)
                            .unwrap_or(fallback),
                    )
                } else {
                    let (g, default_color) =
                        crate::os_icon::resolve_icon(c.detected_os.as_deref(), fallback);
                    let color = c
                        .custom_color
                        .as_deref()
                        .or(c.color.as_deref())
                        .and_then(crate::widgets::parse_hex_color)
                        .unwrap_or(default_color);
                    (g, color)
                };
                let style = crate::widgets::resolve_host_icon_style(
                    c.icon_style.as_deref(),
                    &self.setting_default_host_icon,
                );
                let glyph_el: Element<'a, Message> = glyph.view(13.0, Color::WHITE);
                crate::widgets::host_icon(style, badge_color, &c.label, Some(glyph_el), SIZE)
            }
            None => {
                let glyph_el: Element<'a, Message> = iced_fonts::lucide::server()
                    .size(13)
                    .color(Color::WHITE)
                    .into();
                crate::widgets::host_icon(
                    HostIconStyle::Circular,
                    OryxisColors::t().text_muted,
                    "",
                    Some(glyph_el),
                    SIZE,
                )
            }
        }
    }

    /// One hop card with reorder (up/down) and remove controls.
    fn chain_hop_card<'a>(
        &self,
        idx: usize,
        total: usize,
        badge: Element<'a, Message>,
        label: String,
        breadcrumb: String,
    ) -> Element<'a, Message> {
        let info = column![
            text(label)
                .size(13)
                .font(iced::Font {
                    weight: iced::font::Weight::Semibold,
                    ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                })
                .color(OryxisColors::t().text_primary),
            text(breadcrumb).size(11).color(OryxisColors::t().text_muted),
        ]
        .spacing(2);

        let up_msg = (idx > 0).then_some(Message::ChainEditorMoveHopUp(idx));
        let down_msg = (idx + 1 < total).then_some(Message::ChainEditorMoveHopDown(idx));

        let controls = dir_row(vec![
            opt_icon_button(iced_fonts::lucide::chevron_up(), up_msg),
            Space::new().width(2).into(),
            opt_icon_button(iced_fonts::lucide::chevron_down(), down_msg),
            Space::new().width(2).into(),
            chain_icon_button(
                iced_fonts::lucide::trash(),
                Message::ChainEditorRemoveHop(idx),
                true,
            ),
        ])
        .align_y(iced::Alignment::Center);

        let row = dir_row(vec![
            badge,
            Space::new().width(12).into(),
            info.into(),
            Space::new().width(Length::Fill).into(),
            controls.into(),
        ])
        .align_y(iced::Alignment::Center);

        container(row)
            .padding(Padding {
                top: 10.0,
                right: 12.0,
                bottom: 10.0,
                left: 12.0,
            })
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
}

/// The "→ destination" node closing the chain (the host being edited).
/// Accent-tinted and non-interactive so it reads as the endpoint.
fn destination_card<'a>(badge: Element<'a, Message>, label: String) -> Element<'a, Message> {
    let row = dir_row(vec![
        badge,
        Space::new().width(12).into(),
        text(label)
            .size(13)
            .font(iced::Font {
                weight: iced::font::Weight::Semibold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            })
            .color(OryxisColors::t().text_primary)
            .into(),
    ])
    .align_y(iced::Alignment::Center);

    container(row)
        .padding(Padding {
            top: 10.0,
            right: 12.0,
            bottom: 10.0,
            left: 12.0,
        })
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_selected)),
            border: Border {
                radius: Radius::from(8.0),
                color: OryxisColors::t().accent,
                width: 1.0,
            },
            ..Default::default()
        })
        .into()
}

/// The downward arrow drawn between chain nodes.
fn chain_connector<'a>() -> Element<'a, Message> {
    container(
        iced_fonts::lucide::arrow_down()
            .size(14)
            .color(OryxisColors::t().text_muted),
    )
    .padding(Padding {
        top: 4.0,
        right: 0.0,
        bottom: 4.0,
        left: 0.0,
    })
    .center_x(Length::Fill)
    .into()
}

/// Selectable host row used by the add-a-hop list.
fn pick_row<'a>(
    badge: Element<'a, Message>,
    label: &'a str,
    breadcrumb: String,
    zebra_bg: Color,
    on_press: Message,
) -> Element<'a, Message> {
    let inner = dir_row(vec![
        badge,
        Space::new().width(12).into(),
        text(label.to_string())
            .size(13)
            .font(iced::Font {
                weight: iced::font::Weight::Semibold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            })
            .color(OryxisColors::t().text_primary)
            .into(),
        Space::new().width(Length::Fill).into(),
        text(breadcrumb)
            .size(12)
            .color(OryxisColors::t().accent)
            .into(),
    ])
    .align_y(iced::Alignment::Center);

    button(
        container(inner)
            .padding(Padding {
                top: 8.0,
                right: 12.0,
                bottom: 8.0,
                left: 12.0,
            })
            .width(Length::Fill),
    )
    .on_press(on_press)
    .width(Length::Fill)
    .style(move |_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().bg_hover,
            _ => zebra_bg,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                radius: Radius::from(6.0),
                ..Default::default()
            },
            ..Default::default()
        }
    })
    .into()
}

/// Small square icon button. `danger` tints the glyph in the error
/// color (used for remove).
fn chain_icon_button<'a>(
    icon: iced::widget::Text<'a>,
    on_press: Message,
    danger: bool,
) -> Element<'a, Message> {
    let color = if danger {
        OryxisColors::t().error
    } else {
        OryxisColors::t().text_muted
    };
    button(icon.size(14).color(color))
        .on_press(on_press)
        .padding(6)
        .style(|_, status| {
            let bg = match status {
                BtnStatus::Hovered => OryxisColors::t().bg_hover,
                _ => Color::TRANSPARENT,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border {
                    radius: Radius::from(6.0),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .into()
}

/// Reorder arrow button: interactive when `on_press` is `Some`,
/// otherwise a dimmed non-interactive glyph (at the chain ends).
fn opt_icon_button<'a>(
    icon: iced::widget::Text<'a>,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    match on_press {
        Some(msg) => chain_icon_button(icon, msg, false),
        None => container(icon.size(14).color(OryxisColors::t().border))
            .padding(6)
            .into(),
    }
}
