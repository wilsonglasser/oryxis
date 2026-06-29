//! Dashboard grid: host cards. Split out of views/dashboard/grid/mod.rs.

use super::*;
impl Oryxis {
    /// Host cards for the dashboard grid, in the resolved display order.
    pub(crate) fn dashboard_host_cards(&self) -> Vec<(Element<'_, Message>, Color, DashNavItem)> {
        let mut host_cards: Vec<(Element<'_, Message>, Color, DashNavItem)> = Vec::new();
        let host_order = self.dashboard_host_order();
        for idx in host_order.into_iter() {
            let conn = &self.connections[idx];
            let is_connected = self.tabs.iter().any(|t| t.label == conn.label);
            let auth_label = match conn.auth_method {
                AuthMethod::Auto => t("auth_auto"),
                AuthMethod::Password => t("auth_password"),
                AuthMethod::Key => t("auth_key"),
                AuthMethod::Agent => t("auth_agent"),
                AuthMethod::Interactive => t("auth_interactive"),
            };
            // Address shown only when the (off-by-default) setting is on,
            // so addresses stay out of screenshots / screen shares by
            // default. Port 22 is the SSH default, so it's always omitted.
            let subtitle = if self.setting_show_host_address {
                let port_part = if conn.port == 22 {
                    String::new()
                } else {
                    format!(":{}", conn.port)
                };
                let address = format!(
                    "{}@{}{}",
                    conn.username.as_deref().unwrap_or("root"),
                    conn.hostname,
                    port_part
                );
                // Privacy Mode masks the address behind muted blocks,
                // revealed when the card is hovered. The auth method label
                // is not sensitive, so it stays readable.
                let address = if self.privacy_active(conn) && self.hovered_card != Some(idx) {
                    crate::widgets::mask_blocks(&address)
                } else {
                    address
                };
                format!("{} · {}", address, auth_label)
            } else {
                auth_label.to_string()
            };

            // Resolve icon + brand color from detected OS (if any). Disconnected
            // hosts use the app accent; connected ones use the brand color or
            // success green as fallback.
            let default_fallback = if is_connected {
                OryxisColors::t().success
            } else {
                OryxisColors::t().accent
            };
            let (os_glyph, icon_color) = crate::os_icon::resolve_for(
                conn.detected_os.as_deref(),
                conn.custom_icon.as_deref(),
                conn.custom_color.as_deref(),
                conn.username.as_deref(),
                default_fallback,
            );
            // Fixed 32x32 badge. Shape and color come from the per-host
            // override (icon_style + color) when set; otherwise fall back
            // to the global default_host_icon setting and the OS-derived
            // brand color. Initials style ignores the glyph and renders
            // the leading letters of the label instead.
            let host_style = crate::widgets::resolve_host_icon_style(
                conn.icon_style.as_deref(),
                &self.setting_default_host_icon,
            );
            let badge_color = conn.custom_color.as_deref()
                .or(conn.color.as_deref())
                .and_then(crate::widgets::parse_hex_color)
                .unwrap_or(icon_color);
            let glyph_el: Element<'_, Message> = os_glyph.view(18.0, Color::WHITE);
            let icon_box = crate::widgets::host_icon(
                host_style,
                badge_color,
                &conn.label,
                Some(glyph_el),
                32.0,
            );

            // Floating ⋮ kebab: lives in a Stack overlay on the trailing
            // corner so it doesn't take inline width inside the dir_row.
            // The card reserves a fixed trailing pad so subtitles never
            // collide with the overlay, geometry stays constant regardless
            // of hover state. The button itself is always mounted (so the
            // surrounding MouseArea sees stable child bounds, no hover
            // event loop) and just toggles its glyph color + hover bg.
            let show_dots = self.hovered_card == Some(idx) || self.card_context_menu == Some(idx);
            let rtl = crate::i18n::is_rtl_layout();
            let pad_trailing = 24.0_f32;
            let card_padding = if rtl {
                Padding { top: 8.0, right: 2.0, bottom: 8.0, left: pad_trailing }
            } else {
                Padding { top: 8.0, right: pad_trailing, bottom: 8.0, left: 2.0 }
            };

            // Cloud-origin badge: small brand glyph that used to sit
            // inline with the label (and got clipped on long names).
            // Moved to the LEADING edge of the subtitle row so it
            // never competes with the title for horizontal space.
            // Stored as (brand_key, badge_color, is_orphan) so the
            // glyph can be re-resolved at the use site instead of
            // moved out of a shared tuple (`BrandIcon::view` consumes
            // self and `BrandIcon` doesn't impl Clone).
            let cloud_decoration: Option<(&'static str, Color, bool)> =
                conn.cloud_ref.as_ref().map(|cr| {
                    let provider = self
                        .cloud_profiles
                        .iter()
                        .find(|p| p.id == cr.profile_id)
                        .map(|p| p.provider.as_str())
                        .unwrap_or("cloud");
                    let brand_key: &'static str = match provider {
                        "aws" => "aws",
                        "k8s" | "kubernetes" => "kubernetes",
                        _ => "cloud",
                    };
                    let is_orphan = cr.orphaned_at.is_some();
                    let (_brand_glyph, brand_color_default) = crate::os_icon::provider_icon(
                        brand_key,
                        OryxisColors::t().accent,
                    );
                    let badge_color = if is_orphan {
                        OryxisColors::t().text_muted
                    } else {
                        brand_color_default
                    };
                    (brand_key, badge_color, is_orphan)
                });

            let label_color = match &cloud_decoration {
                Some((_, _, true)) => OryxisColors::t().text_muted,
                _ => OryxisColors::t().text_primary,
            };
            let label_el: Element<'_, Message> = if let Some((_, _, true)) = &cloud_decoration {
                // Orphan: keep the pill next to the label so the user
                // sees it at the title's eye level.
                let muted = OryxisColors::t().text_muted;
                let pill = container(
                    text(t("host_orphan_label"))
                        .size(9)
                        .color(OryxisColors::t().text_muted),
                )
                .padding(Padding {
                    top: 1.0,
                    right: 6.0,
                    bottom: 1.0,
                    left: 6.0,
                })
                .style(move |_| container::Style {
                    background: Some(Background::Color(Color { a: 0.10, ..muted })),
                    border: Border {
                        radius: Radius::from(6.0),
                        color: Color { a: 0.30, ..muted },
                        width: 1.0,
                    },
                    ..Default::default()
                });
                dir_row(vec![
                    text(&conn.label)
                        .size(13)
                        .color(label_color)
                        .wrapping(iced::widget::text::Wrapping::None)
                        .into(),
                    Space::new().width(6).into(),
                    pill.into(),
                ])
                .align_y(iced::Alignment::Center)
                .into()
            } else {
                text(&conn.label)
                    .size(13)
                    .color(label_color)
                    .wrapping(iced::widget::text::Wrapping::None)
                    .into()
            };

            // Subtitle row carries the brand badge on its leading edge
            // when this host is cloud-sourced. Manual hosts get just
            // the subtitle text (no leading gap).
            let subtitle_el: Element<'_, Message> = match &cloud_decoration {
                Some((brand_key, color, _)) => {
                    let glyph = crate::os_icon::custom_icon_glyph(brand_key);
                    dir_row(vec![
                        glyph.view(10.0, *color),
                        Space::new().width(6).into(),
                        text(subtitle)
                            .size(10)
                            .color(OryxisColors::t().text_muted)
                            .wrapping(iced::widget::text::Wrapping::None)
                            .into(),
                    ])
                    .align_y(iced::Alignment::Center)
                    .into()
                }
                None => text(subtitle)
                    .size(10)
                    .color(OryxisColors::t().text_muted)
                    .wrapping(iced::widget::text::Wrapping::None)
                    .into(),
            };

            let card_btn = button(
                container(
                    dir_row(vec![
                        icon_box,
                        Space::new().width(8).into(),
                        iced::widget::Column::with_children(vec![
                            label_el,
                            Space::new().height(2).into(),
                            subtitle_el,
                        ])
                        .width(Length::Fill)
                        .align_x(crate::widgets::dir_align_x())
                        .clip(true)
                        .into(),
                    ]).align_y(iced::Alignment::Center),
                )
                .padding(card_padding),
            )
            .on_press(Message::ConnectSsh(idx))
            .width(Length::Fill)
            .style(move |_, status| {
                let bg = match status {
                    BtnStatus::Hovered => OryxisColors::t().bg_hover,
                    BtnStatus::Pressed => OryxisColors::t().bg_selected,
                    _ => OryxisColors::t().bg_surface,
                };
                // Same rounded card in grid and list mode: list mode is just
                // a single column with a small gap (History-style rows), so
                // each card stays independently rounded (radius matches the
                // accent wash) instead of a connected divider list. The
                // keyboard-selection highlight is drawn as an outer ring in
                // the assembly, not here.
                let (bc, bw) = match status {
                    BtnStatus::Hovered => (OryxisColors::t().accent, 1.5),
                    BtnStatus::Pressed => (OryxisColors::t().accent, 2.0),
                    _ => (OryxisColors::t().border, 1.0),
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(10.0), color: bc, width: bw },
                    ..Default::default()
                }
            });

            let dots_glyph_color = if show_dots {
                OryxisColors::t().text_muted
            } else {
                Color::TRANSPARENT
            };
            let dots_btn = crate::widgets::card_kebab_button(
                dots_glyph_color,
                show_dots,
                Message::ShowCardMenu(idx),
            );
            let dots_align = if rtl {
                iced::alignment::Horizontal::Left
            } else {
                iced::alignment::Horizontal::Right
            };
            let dots_pad = if rtl {
                Padding { top: 0.0, right: 0.0, bottom: 0.0, left: 4.0 }
            } else {
                Padding { top: 0.0, right: 4.0, bottom: 0.0, left: 0.0 }
            };
            let dots_overlay = container(dots_btn)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(dots_align)
                .align_y(iced::alignment::Vertical::Center)
                .padding(dots_pad);
            let card_element: Element<'_, Message> = iced::widget::Stack::new()
                .push(card_btn)
                .push(dots_overlay)
                .into();

            // Wrap in MouseArea for hover tracking and right-click
            let wrapped = MouseArea::new(card_element)
                .on_enter(Message::CardHovered(idx))
                .on_exit(Message::CardUnhovered)
                .on_right_press(Message::ShowCardMenu(idx));

            host_cards.push((Element::from(container(wrapped).width(Length::Fill).clip(true)), badge_color, DashNavItem::Host(idx)));
        }
        host_cards
    }
}
