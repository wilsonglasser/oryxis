//! Keys screen + identity panel + SSH key import panel.

use iced::border::Radius;
use iced::widget::{
    button, column, container, pick_list, scrollable, text, text_editor, text_input, MouseArea,
    Space,
};
use iced::widget::button::Status as BtnStatus;
use iced::{Background, Border, Color, Element, Length, Padding};

use oryxis_core::models::connection::Connection;
use oryxis_core::models::identity::Identity;
use oryxis_core::models::key::SshKey;

use crate::app::{Message, Oryxis, CARD_WIDTH, PANEL_WIDTH};
use crate::i18n::t;
use crate::theme::OryxisColors;
use crate::widgets::{
    card_grid_columns, dir_align_x, dir_row, distribute_card_grid, password_input_with_eye,
};

impl Oryxis {
    pub(crate) fn view_keys(&self) -> Element<'_, Message> {
        // ── Header toolbar ──
        // Split button: leading half "+ ADD" (opens menu), vertical
        // separator, trailing half "▼" chevron (also opens menu). Both
        // halves invoke the same toggle so the dropdown appears below
        // regardless of which half the user clicks. The leading half
        // gets its outer corners rounded; under RTL `dir_row` swaps the
        // order, so we also swap which physical corners each half
        // rounds, otherwise the rounded edge ends up in the middle.
        let rtl = crate::i18n::is_rtl_layout();
        let label_radius = if rtl {
            // Label sits on the right edge in RTL → round right corners.
            Radius { top_left: 0.0, bottom_left: 0.0, top_right: 6.0, bottom_right: 6.0 }
        } else {
            Radius { top_left: 6.0, bottom_left: 6.0, top_right: 0.0, bottom_right: 0.0 }
        };
        let chevron_radius = if rtl {
            Radius { top_left: 6.0, bottom_left: 6.0, top_right: 0.0, bottom_right: 0.0 }
        } else {
            Radius { top_left: 0.0, bottom_left: 0.0, top_right: 6.0, bottom_right: 6.0 }
        };

        let add_label = button(
            container(
                dir_row(vec![
                    text("+").size(13).font(iced::Font {
                        weight: iced::font::Weight::Bold,
                        ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                    }).color(OryxisColors::t().button_text).into(),
                    Space::new().width(4).into(),
                    text(t("add_btn")).size(11).font(iced::Font {
                        weight: iced::font::Weight::Bold,
                        ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                    }).color(OryxisColors::t().button_text).into(),
                ])
                .align_y(iced::Alignment::Center),
            )
            .center_y(Length::Fixed(24.0))
            .padding(Padding { top: 0.0, right: 14.0, bottom: 0.0, left: 14.0 }),
        )
        .on_press(Message::ToggleKeychainAddMenu)
        .style(move |_, status| {
            let bg = match status {
                BtnStatus::Hovered => OryxisColors::t().button_bg_hover,
                _ => OryxisColors::t().button_bg,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border {
                    radius: label_radius,
                    ..Default::default()
                },
                ..Default::default()
            }
        });

        let separator = container(Space::new().width(1).height(16))
            .style(|_| container::Style {
                background: Some(Background::Color(Color { a: 0.3, ..Color::BLACK })),
                ..Default::default()
            });

        // Chevron half, match the left half's vertical metrics so both halves
        // render at identical heights. Lateral padding is kept to the minimum
        // that still gives the glyph breathing room.
        let add_chevron = button(
            container(
                iced_fonts::lucide::chevron_down::<iced::Theme, iced::Renderer>()
                    .size(12).color(OryxisColors::t().button_text),
            )
            .center_y(Length::Fixed(24.0))
            .padding(Padding { top: 0.0, right: 4.0, bottom: 0.0, left: 4.0 }),
        )
        .on_press(Message::ToggleKeychainAddMenu)
        .style(move |_, status| {
            let bg = match status {
                BtnStatus::Hovered => OryxisColors::t().button_bg_hover,
                _ => OryxisColors::t().button_bg,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border {
                    radius: chevron_radius,
                    ..Default::default()
                },
                ..Default::default()
            }
        });

        let add_btn: Element<'_, Message> = dir_row(vec![
            add_label.into(),
            separator.into(),
            add_chevron.into(),
        ])
        .align_y(iced::Alignment::Center)
        .into();

        let sort_btn = crate::widgets::sort_toolbar_button(
            crate::state::SortMenuKind::Keys,
            self.keys_sort,
        );

        let toolbar = container(
            dir_row(vec![
                text(t("keychain")).size(20).color(OryxisColors::t().text_primary).into(),
                Space::new().width(Length::Fill).into(),
                sort_btn,
                Space::new().width(8).into(),
                add_btn,
            ])
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 24.0, bottom: 16.0, left: 24.0 })
        .width(Length::Fill);

        // ── Search bar ──
        // Collapses to zero height in Workspace mode where the search
        // lives on the contextual sub-nav (`view_vault_sub_nav`),
        // matching the host-grid / snippets / history treatment.
        let workspace_mode = self.setting_layout_mode == "workspace";
        let search_bar: Element<'_, Message> = if workspace_mode {
            Space::new().height(0).into()
        } else {
            container(
                text_input(t("search_keys_identities"), &self.key_search)
                    .id(iced::widget::Id::new("search-keys"))
                    .on_input(Message::KeySearchChanged)
                    .padding(10)
                    .size(13)
                    .width(Length::Fill)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
            )
            .padding(Padding { top: 0.0, right: 24.0, bottom: 12.0, left: 24.0 })
            .width(Length::Fill)
            .into()
        };

        // ── Status message ──
        // While the import / identity sidebars are open, the panel surfaces
        // its own error/success right next to the field that caused it
        // duplicating the message in the main keychain area is just noise.
        let panel_open = self.show_key_panel || self.show_identity_panel;
        let status: Element<'_, Message> = if panel_open {
            Space::new().height(0).into()
        } else if let Some(err) = &self.key_error {
            container(Element::from(text(err.clone()).size(12).color(OryxisColors::t().error)))
                .padding(Padding { top: 0.0, right: 24.0, bottom: 8.0, left: 24.0 })
                .into()
        } else if let Some(ok) = &self.key_success {
            container(Element::from(text(ok.clone()).size(12).color(OryxisColors::t().success)))
                .padding(Padding { top: 0.0, right: 24.0, bottom: 8.0, left: 24.0 })
                .into()
        } else {
            Space::new().height(0).into()
        };

        // ── Keys grid ──
        // Section title in a Fill container so it anchors to the
        // card grid's leading edge (column align_x can push a
        // shrink-fit text past the card border otherwise).
        let section_title = container(
            container(
                text(t("keys_section")).size(14).color(OryxisColors::t().text_muted),
            )
            .width(Length::Fill)
            .align_x(crate::widgets::dir_align_x()),
        )
        .padding(Padding { top: 4.0, right: 0.0, bottom: 8.0, left: 0.0 });

        // Filter keys by search query. Apply the toolbar sort by
        // reordering the index list first so EditKey(idx) / DeleteKey
        // still target the canonical vault index, even though the
        // rendered order changes.
        let search_lower = self.key_search.to_lowercase();
        let mut key_order: Vec<usize> = (0..self.keys.len()).collect();
        self.keys_sort.sort_items(
            &mut key_order,
            |&i| self.keys[i].label.clone(),
            |&i| self.keys[i].created_at,
        );
        let filtered_keys: Vec<(usize, &SshKey)> = key_order
            .into_iter()
            .map(|i| (i, &self.keys[i]))
            .filter(|(_, k)| {
                search_lower.is_empty() || k.label.to_lowercase().contains(&search_lower)
            })
            .collect();

        let mut cards: Vec<Element<'_, Message>> = Vec::new();

        if filtered_keys.is_empty() && self.keys.is_empty() {
            let empty_state = container(
                column![
                    container(
                        iced_fonts::lucide::key_round().size(32).color(OryxisColors::t().text_muted),
                    )
                    .padding(16)
                    .style(|_| container::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_surface)),
                        border: Border { radius: Radius::from(12.0), ..Default::default() },
                        ..Default::default()
                    }),
                    Space::new().height(20),
                    text(crate::i18n::t("add_key_title")).size(20).color(OryxisColors::t().text_primary),
                    Space::new().height(8),
                    text(crate::i18n::t("add_key_desc"))
                        .size(13).color(OryxisColors::t().text_muted),
                    Space::new().height(24),
                    crate::widgets::cta_button(
                        crate::i18n::t("import_key").to_string(),
                        Message::ShowKeyPanel,
                    ),
                ]
                .align_x(iced::Alignment::Center),
            )
            .center(Length::Fill);

            let main_content = column![toolbar, search_bar, status, empty_state]
                .width(Length::Fill)
                .height(Length::Fill);

            if self.show_key_panel {
                let panel = self.view_key_import_panel();
                return dir_row(vec![main_content.into(), panel])
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into();
            } else if self.show_identity_panel {
                let panel = self.view_identity_panel();
                return dir_row(vec![main_content.into(), panel])
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into();
            }
            return main_content.into();
        } else if filtered_keys.is_empty() {
            let no_results = container(
                text(t("no_keys_match")).size(13).color(OryxisColors::t().text_muted),
            )
            .padding(24)
            .width(CARD_WIDTH);
            cards.push(no_results.into());
        }

        for (idx, key) in filtered_keys {
            let algo = format!("{} {}", t("type_label"), key.algorithm);
            let key_style = crate::widgets::resolve_host_icon_style(
                None,
                &self.setting_default_host_icon,
            );
            let glyph_el: Element<'_, Message> = iced_fonts::lucide::key_round()
                .size(16)
                .line_height(1.0)
                .color(Color::WHITE)
                .into();
            let icon_box = crate::widgets::host_icon(
                key_style,
                OryxisColors::t().accent,
                &key.label,
                Some(glyph_el),
                32.0,
            );

            // Floating ⋮ kebab: lives in a Stack overlay on the trailing
            // corner so it doesn't take inline width. Always mounted with
            // a transparent glyph + no-hover bg when not active so the
            // surrounding MouseArea bounds stay stable.
            let key_show_dots =
                self.hovered_key_card == Some(idx) || self.key_context_menu == Some(idx);
            let key_rtl = crate::i18n::is_rtl_layout();
            // Match the dashboard host-card geometry exactly: the host
            // card wraps its row in `container(...).padding(...)` and
            // lets the outer button add its `DEFAULT_PADDING` (5/10/5
            // /10), producing a 13/16/13/12 effective padding. Since
            // keychain cards override `button.padding()` directly,
            // they need explicit values that match that effective
            // size, otherwise they render ~10 px shorter and ~4 px
            // tighter on the leading edge than the host cards next to
            // them. Trailing stays at 24 to clear the kebab overlay.
            let card_pad_trailing = 24.0_f32;
            let card_padding = if key_rtl {
                Padding { top: 13.0, right: 12.0, bottom: 13.0, left: card_pad_trailing }
            } else {
                Padding { top: 13.0, right: card_pad_trailing, bottom: 13.0, left: 12.0 }
            };

            let card = button(
                dir_row(vec![
                    icon_box,
                    Space::new().width(8).into(),
                    column![
                        text(&key.label)
                            .size(13)
                            .color(OryxisColors::t().text_primary)
                            .wrapping(iced::widget::text::Wrapping::None),
                        Space::new().height(2),
                        text(algo)
                            .size(11)
                            .color(OryxisColors::t().text_muted)
                            .wrapping(iced::widget::text::Wrapping::None),
                    ]
                    .width(Length::Fill)
                    .align_x(crate::widgets::dir_align_x())
                    .clip(true)
                    .into(),
                ]).align_y(iced::Alignment::Center),
            )
            .on_press(Message::EditKey(idx))
            .padding(card_padding)
            .width(Length::Fill)
            .style(|_, status| {
                let (bg, border_color, border_width) = match status {
                    BtnStatus::Hovered => (OryxisColors::t().bg_hover, OryxisColors::t().accent, 1.5),
                    BtnStatus::Pressed => (OryxisColors::t().bg_selected, OryxisColors::t().accent, 2.0),
                    _ => (OryxisColors::t().bg_surface, OryxisColors::t().border, 1.0),
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(10.0), color: border_color, width: border_width },
                    ..Default::default()
                }
            });

            let key_dots_glyph_color = if key_show_dots {
                OryxisColors::t().text_muted
            } else {
                Color::TRANSPARENT
            };
            let dots_btn = button(
                text("\u{22EE}").size(14).color(key_dots_glyph_color),
            )
            .on_press(Message::ShowKeyMenu(idx))
            .padding(Padding { top: 1.0, right: 6.0, bottom: 1.0, left: 6.0 })
            .style(move |_, status| {
                let bg = match status {
                    BtnStatus::Hovered if key_show_dots => OryxisColors::t().bg_hover,
                    _ => Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                }
            });
            let key_dots_align = if key_rtl {
                iced::alignment::Horizontal::Left
            } else {
                iced::alignment::Horizontal::Right
            };
            let key_dots_pad = if key_rtl {
                Padding { top: 0.0, right: 0.0, bottom: 0.0, left: 8.0 }
            } else {
                Padding { top: 0.0, right: 8.0, bottom: 0.0, left: 0.0 }
            };
            let dots_overlay = container(dots_btn)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(key_dots_align)
                .align_y(iced::alignment::Vertical::Center)
                .padding(key_dots_pad);
            let card_element: Element<'_, Message> = iced::widget::Stack::new()
                .push(card)
                .push(dots_overlay)
                .into();

            // Wrap in MouseArea for right-click + hover events that
            // drive the dots-button visibility.
            let wrapped = MouseArea::new(card_element)
                .on_enter(Message::KeyCardHovered(idx))
                .on_exit(Message::KeyCardUnhovered)
                .on_right_press(Message::ShowKeyMenu(idx));

            cards.push(
                container(wrapped)
                    .width(Length::Fill)
                    .clip(true)
                    .into(),
            );
        }

        // Responsive grid: column count derived from the current window
        // width minus the visible chrome (left nav + optional right panel
        // + horizontal padding around the grid). When the user resizes
        // the window or opens/closes the side panel, the next view()
        // recomputes `cols` and the cards rewrap accordingly instead of
        // disappearing into clipped overflow.
        let nav_width = if self.sidebar_collapsed {
            crate::app::SIDEBAR_WIDTH_COLLAPSED
        } else {
            crate::app::SIDEBAR_WIDTH
        };
        let panel_width = if self.show_key_panel || self.show_identity_panel {
            crate::app::PANEL_WIDTH
        } else {
            0.0
        };
        // 24 px of horizontal padding on each side of the grid column,
        // plus ~12 px reserved for the scrollbar gutter on the trailing
        // edge. Keep this in sync with the `padding` set on the
        // scrollable column further down.
        let available = (self.window_size.width - nav_width - panel_width - 60.0).max(0.0);
        let cols = card_grid_columns(available, CARD_WIDTH, 12.0);
        let keys_grid_elem = distribute_card_grid(cards, cols, 12.0, 12.0);

        // ── Identities section ──
        let identity_section_title = container(
            container(
                text(t("identities")).size(14).color(OryxisColors::t().text_muted),
            )
            .width(Length::Fill)
            .align_x(crate::widgets::dir_align_x()),
        )
        .padding(Padding { top: 16.0, right: 0.0, bottom: 8.0, left: 0.0 });

        let mut identity_order: Vec<usize> = (0..self.identities.len()).collect();
        self.keys_sort.sort_items(
            &mut identity_order,
            |&i| self.identities[i].label.clone(),
            |&i| self.identities[i].created_at,
        );
        let filtered_identities: Vec<(usize, &Identity)> = identity_order
            .into_iter()
            .map(|i| (i, &self.identities[i]))
            .filter(|(_, i)| {
                search_lower.is_empty() || i.label.to_lowercase().contains(&search_lower)
            })
            .collect();

        let mut identity_cards: Vec<Element<'_, Message>> = Vec::new();

        if filtered_identities.is_empty() && self.identities.is_empty() {
            // Don't show identities section at all when empty
        } else if filtered_identities.is_empty() {
            let no_results = container(
                text(t("no_identities_match")).size(13).color(OryxisColors::t().text_muted),
            )
            .padding(24)
            .width(CARD_WIDTH);
            identity_cards.push(no_results.into());
        }

        for (idx, identity) in &filtered_identities {
            let idx = *idx;
            // Build subtitle describing auth methods
            let mut parts: Vec<String> = Vec::new();
            if let Some(u) = &identity.username {
                parts.push(u.clone());
            }
            let has_pw = self.identities_with_password.contains(&identity.id);
            if has_pw {
                parts.push("\u{25CF}\u{25CF}\u{25CF}\u{25CF}".into());
            }
            if let Some(kid) = identity.key_id
                && let Some(k) = self.keys.iter().find(|k| k.id == kid) {
                    parts.push(k.label.clone());
            }
            let subtitle = if parts.is_empty() { t("no_credentials").to_string() } else { parts.join(", ") };

            let id_style = crate::widgets::resolve_host_icon_style(
                None,
                &self.setting_default_host_icon,
            );
            let id_glyph_el: Element<'_, Message> = iced_fonts::lucide::user()
                .size(16)
                .line_height(1.0)
                .color(Color::WHITE)
                .into();
            let icon_box = crate::widgets::host_icon(
                id_style,
                OryxisColors::t().accent,
                &identity.label,
                Some(id_glyph_el),
                32.0,
            );

            // Floating ⋮ kebab in a Stack overlay on the trailing corner,
            // same pattern as host / key cards.
            let id_show_dots =
                self.hovered_identity_card == Some(idx) || self.identity_context_menu == Some(idx);
            let id_rtl = crate::i18n::is_rtl_layout();
            // Match the host-card geometry (see key card comment
            // above): 13 top/bottom + 12 leading + 24 trailing brings
            // the identity card to the same visible footprint as the
            // host folder cards on the dashboard, fixing the "card has
            // no padding" feel (was 2 leading) and the 9-px height
            // gap to host cards (was 8 top/bottom).
            let id_pad_trailing = 24.0_f32;
            let id_card_padding = if id_rtl {
                Padding { top: 13.0, right: 12.0, bottom: 13.0, left: id_pad_trailing }
            } else {
                Padding { top: 13.0, right: id_pad_trailing, bottom: 13.0, left: 12.0 }
            };

            let card = button(
                dir_row(vec![
                    icon_box,
                    Space::new().width(8).into(),
                    column![
                        text(&identity.label)
                            .size(13)
                            .color(OryxisColors::t().text_primary)
                            .wrapping(iced::widget::text::Wrapping::None),
                        Space::new().height(2),
                        text(subtitle)
                            .size(11)
                            .color(OryxisColors::t().text_muted)
                            .wrapping(iced::widget::text::Wrapping::None),
                    ]
                    .width(Length::Fill)
                    .align_x(crate::widgets::dir_align_x())
                    .clip(true)
                    .into(),
                ]).align_y(iced::Alignment::Center),
            )
            .on_press(Message::EditIdentity(idx))
            .padding(id_card_padding)
            .width(Length::Fill)
            .style(|_, status| {
                let (bg, border_color, border_width) = match status {
                    BtnStatus::Hovered => (OryxisColors::t().bg_hover, OryxisColors::t().accent, 1.5),
                    BtnStatus::Pressed => (OryxisColors::t().bg_selected, OryxisColors::t().accent, 2.0),
                    _ => (OryxisColors::t().bg_surface, OryxisColors::t().border, 1.0),
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(10.0), color: border_color, width: border_width },
                    ..Default::default()
                }
            });

            let id_dots_glyph_color = if id_show_dots {
                OryxisColors::t().text_muted
            } else {
                Color::TRANSPARENT
            };
            let dots_btn = button(
                text("\u{22EE}").size(14).color(id_dots_glyph_color),
            )
            .on_press(Message::ShowIdentityMenu(idx))
            .padding(Padding { top: 1.0, right: 6.0, bottom: 1.0, left: 6.0 })
            .style(move |_, status| {
                let bg = match status {
                    BtnStatus::Hovered if id_show_dots => OryxisColors::t().bg_hover,
                    _ => Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                }
            });
            let id_dots_align = if id_rtl {
                iced::alignment::Horizontal::Left
            } else {
                iced::alignment::Horizontal::Right
            };
            let id_dots_pad = if id_rtl {
                Padding { top: 0.0, right: 0.0, bottom: 0.0, left: 8.0 }
            } else {
                Padding { top: 0.0, right: 8.0, bottom: 0.0, left: 0.0 }
            };
            let dots_overlay = container(dots_btn)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(id_dots_align)
                .align_y(iced::alignment::Vertical::Center)
                .padding(id_dots_pad);
            let card_element: Element<'_, Message> = iced::widget::Stack::new()
                .push(card)
                .push(dots_overlay)
                .into();

            let wrapped = MouseArea::new(card_element)
                .on_enter(Message::IdentityCardHovered(idx))
                .on_exit(Message::IdentityCardUnhovered)
                .on_right_press(Message::ShowIdentityMenu(idx));

            identity_cards.push(
                container(wrapped)
                    .width(Length::Fill)
                    .clip(true)
                    .into(),
            );
        }

        let identity_grid_elem = distribute_card_grid(identity_cards, cols, 12.0, 12.0);

        // Combine keys and identities into one scrollable area
        let mut all_rows: Vec<Element<'_, Message>> = Vec::new();
        all_rows.push(section_title.into());
        all_rows.push(keys_grid_elem);
        if !self.identities.is_empty() {
            all_rows.push(identity_section_title.into());
            all_rows.push(identity_grid_elem);
        }

        // Right padding here also pushes the content away from the
        // scrollbar, keep it slim so the scrollbar reads as flush
        // against the panel edge rather than floating in dead space.
        // The column needs `Length::Fill` for `align_x` to have any
        // slack to align inside, without it the column shrinks to
        // content and rows hug the leading edge regardless.
        let grid = scrollable(
            column(all_rows)
                .width(Length::Fill)
                .padding(Padding { top: 0.0, right: 24.0, bottom: 24.0, left: 24.0 })
                .align_x(crate::widgets::dir_align_x()),
        )
        .height(Length::Fill);

        // ── Main content ──
        let main_content = column![toolbar, search_bar, status, grid]
            .width(Length::Fill)
            .height(Length::Fill);

        // ── Side panel ──
        if self.show_key_panel {
            let panel = self.view_key_import_panel();
            dir_row(vec![main_content.into(), panel])
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else if self.show_identity_panel {
            let panel = self.view_identity_panel();
            dir_row(vec![main_content.into(), panel])
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            main_content.into()
        }
    }

    pub(crate) fn view_key_import_panel(&self) -> Element<'_, Message> {
        let has_content = !self.key_import_pem.is_empty();
        let panel_title = if self.editing_key_id.is_some() { t("edit_key") } else { t("add_key") };

        // Panel header
        let panel_header = container(
            dir_row(vec![
                text(panel_title).size(18).color(OryxisColors::t().text_primary).into(),
                Space::new().width(Length::Fill).into(),
                button(text("\u{00D7}").size(14).color(OryxisColors::t().text_muted))
                    .on_press(Message::HideKeyPanel)
                    .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_surface)),
                        border: Border { radius: Radius::from(6.0), ..Default::default() },
                        ..Default::default()
                    }).into(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 20.0, bottom: 16.0, left: 20.0 });

        // Name field
        let name_field = column![
            text(t("name")).size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(6),
            text_input("my-server-key", &self.key_import_label)
                .on_input(Message::KeyImportLabelChanged)
                .padding(10)
                .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());

        // File selector button
        let browse_btn = button(
            container(
                dir_row(vec![
                    text(t("select_file"))
                        .size(13)
                        .font(iced::Font {
                            weight: iced::font::Weight::Semibold,
                            ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                        })
                        .color(crate::theme::contrast_text_for(OryxisColors::t().accent))
                        .into(),
                ])
                .align_y(iced::Alignment::Center),
            )
            .padding(Padding { top: 8.0, right: 16.0, bottom: 8.0, left: 16.0 }),
        )
        .on_press(Message::BrowseKeyFile)
        .width(Length::Fill)
        .style(|_, _| button::Style {
            background: Some(Background::Color(OryxisColors::t().accent)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });

        // Status indicator
        let file_status: Element<'_, Message> = if has_content {
            container(
                dir_row(vec![
                    iced_fonts::lucide::circle_check()
                        .size(13)
                        .color(OryxisColors::t().success)
                        .into(),
                    Space::new().width(6).into(),
                    text(
                        t("loaded_bytes")
                            .replacen("{bytes}", &self.key_import_pem.len().to_string(), 1),
                    )
                    .size(12).color(OryxisColors::t().success).into(),
                ]).align_y(iced::Alignment::Center),
            )
            .padding(Padding { top: 4.0, right: 0.0, bottom: 4.0, left: 0.0 })
            .into()
        } else {
            Space::new().height(0).into()
        };

        // Editable key content (text_editor = multi-line)
        let editor = text_editor(&self.key_import_content)
            .on_action(Message::KeyContentAction)
            .padding(10)
            .height(180)
            .font(iced::Font::MONOSPACE)
            .size(11);

        // Passphrase prompt, shown only after import_key signals the key
        // is encrypted. The hint explains the one-time-decrypt model so
        // users understand we're not storing the passphrase anywhere.
        let passphrase_section: Element<'_, Message> = if self.key_import_passphrase_required {
            column![
                Space::new().height(12),
                text(t("key_passphrase_label")).size(12).color(OryxisColors::t().text_secondary),
                Space::new().height(6),
                dir_row(vec![
                    iced_fonts::lucide::lock().size(13).color(OryxisColors::t().text_muted).into(),
                    Space::new().width(10).into(),
                    password_input_with_eye(
                        t("key_passphrase_placeholder"),
                        &self.key_import_passphrase,
                        Message::KeyImportPassphraseChanged,
                        Some(Message::ImportKey),
                        self.key_import_passphrase_visible,
                        Message::KeyImportPassphraseToggleVisibility,
                        10.0,
                    ),
                ]).align_y(iced::Alignment::Center),
                Space::new().height(6),
                text(t("key_passphrase_hint")).size(11).color(OryxisColors::t().text_muted),
            ]
            .width(Length::Fill)
            .align_x(dir_align_x())
            .into()
        } else {
            Space::new().height(0).into()
        };

        // Error in panel
        let panel_error: Element<'_, Message> = if let Some(err) = &self.key_error {
            Element::from(text(err.clone()).size(11).color(OryxisColors::t().error))
        } else {
            Space::new().height(0).into()
        };

        // Save button
        let save_label = if self.editing_key_id.is_some() { t("update_key") } else { t("save_key") };
        let save_btn = button(
            container(text(save_label).size(13).color(OryxisColors::t().text_primary))
                .padding(Padding { top: 10.0, right: 0.0, bottom: 10.0, left: 0.0 })
                .width(Length::Fill)
                .center_x(Length::Fill),
        )
        .on_press(Message::ImportKey)
        .width(Length::Fill)
        .style(move |_, _| {
            let bg = if has_content { OryxisColors::t().accent } else { OryxisColors::t().bg_surface };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(8.0), ..Default::default() },
                ..Default::default()
            }
        });

        let panel_content = column![
            panel_header,
            container(
                column![
                    name_field,
                    Space::new().height(16),
                    text(t("private_key")).size(12).color(OryxisColors::t().text_secondary),
                    Space::new().height(6),
                    browse_btn,
                    Space::new().height(8),
                    file_status,
                    Space::new().height(8),
                    text(t("key_content")).size(12).color(OryxisColors::t().text_secondary),
                    Space::new().height(6),
                    editor,
                    passphrase_section,
                    Space::new().height(8),
                    panel_error,
                    Space::new().height(Length::Fill),
                    save_btn,
                ]
                .height(Length::Fill)
                .width(Length::Fill)
                .align_x(dir_align_x()),
            )
            .padding(Padding { top: 0.0, right: 20.0, bottom: 20.0, left: 20.0 })
            .height(Length::Fill),
        ]
        .height(Length::Fill);

        container(panel_content)
            .width(PANEL_WIDTH)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                border: Border { color: OryxisColors::t().border, width: 1.0, radius: Radius::from(0.0) },
                ..Default::default()
            })
            .into()
    }

    pub(crate) fn view_identity_panel(&self) -> Element<'_, Message> {
        let panel_title = if self.editing_identity_id.is_some() { t("edit_identity") } else { t("new_identity") };

        // Panel header
        let panel_header = container(
            dir_row(vec![
                text(panel_title).size(18).color(OryxisColors::t().text_primary).into(),
                Space::new().width(Length::Fill).into(),
                button(text("\u{00D7}").size(14).color(OryxisColors::t().text_muted))
                    .on_press(Message::HideIdentityPanel)
                    .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_surface)),
                        border: Border { radius: Radius::from(6.0), ..Default::default() },
                        ..Default::default()
                    }).into(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 20.0, bottom: 16.0, left: 20.0 });

        // Label field
        let label_field = column![
            text(t("label")).size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(6),
            text_input(t("my_identity_placeholder"), &self.identity_form_label)
                .on_input(Message::IdentityLabelChanged)
                .padding(10)
                .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());

        // Username field
        let username_field = column![
            text(t("username")).size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(6),
            dir_row(vec![
                iced_fonts::lucide::user().size(13).color(OryxisColors::t().text_muted).into(),
                Space::new().width(10).into(),
                text_input("root", &self.identity_form_username)
                    .on_input(Message::IdentityUsernameChanged)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                    .into(),
            ]).align_y(iced::Alignment::Center),
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());

        // Password field with eye toggle
        let identity_pw_placeholder: &'static str = if self.identity_form_has_existing_password
            && !self.identity_form_password_touched
        {
            "\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}"
        } else {
            t("password")
        };
        let password_field = column![
            text(t("password")).size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(6),
            dir_row(vec![
                iced_fonts::lucide::keyboard().size(13).color(OryxisColors::t().text_muted).into(),
                Space::new().width(10).into(),
                password_input_with_eye(
                    identity_pw_placeholder,
                    &self.identity_form_password,
                    Message::IdentityPasswordChanged,
                    None,
                    self.identity_form_password_visible,
                    Message::IdentityTogglePasswordVisibility,
                    10.0,
                ),
            ]).align_y(iced::Alignment::Center),
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());

        // Key selector
        let key_options = {
            let mut opts = vec!["(none)".to_string()];
            opts.extend(self.keys.iter().map(|k| k.label.clone()));
            opts
        };
        let key_field = column![
            text(t("ssh_key")).size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(6),
            dir_row(vec![
                text(t("add_key_btn")).size(12).color(OryxisColors::t().accent).into(),
                Space::new().width(16).into(),
                pick_list(
                    Some(self.identity_form_key.clone().unwrap_or_else(|| "(none)".into())),
                    key_options,
                    |s: &String| s.clone(),
                )
                .on_select(Message::IdentityKeyChanged)
                .padding(10).style(crate::widgets::rounded_pick_list_style)
                .into(),
            ]).align_y(iced::Alignment::Center),
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());

        // Linked connections (only when editing)
        let linked_section: Element<'_, Message> = if let Some(editing_id) = self.editing_identity_id {
            let linked: Vec<&Connection> = self.connections.iter()
                .filter(|c| c.identity_id == Some(editing_id))
                .collect();
            if linked.is_empty() {
                column![
                    Space::new().height(16),
                    text(t("linked_to")).size(12).color(OryxisColors::t().text_muted),
                    Space::new().height(4),
                    text(t("no_connections_identity")).size(11).color(OryxisColors::t().text_muted),
                ].into()
            } else {
                let mut items: Vec<Element<'_, Message>> = vec![
                    Space::new().height(16).into(),
                    Element::from(text(t("linked_to")).size(12).color(OryxisColors::t().text_muted)),
                    Space::new().height(4).into(),
                ];
                for conn in linked {
                    items.push(
                        container(
                            dir_row(vec![
                                iced_fonts::lucide::server().size(11).color(OryxisColors::t().text_muted).into(),
                                Space::new().width(8).into(),
                                text(&conn.label).size(12).color(OryxisColors::t().text_secondary).into(),
                            ]).align_y(iced::Alignment::Center),
                        )
                        .padding(Padding { top: 4.0, right: 0.0, bottom: 4.0, left: 0.0 })
                        .into()
                    );
                }
                column(items).into()
            }
        } else {
            Space::new().height(0).into()
        };

        // Save button
        let save_label = if self.editing_identity_id.is_some() { "Update Identity" } else { "Save Identity" };
        let has_label = !self.identity_form_label.trim().is_empty();
        let save_btn = button(
            container(text(save_label).size(13).color(OryxisColors::t().text_primary))
                .padding(Padding { top: 10.0, right: 0.0, bottom: 10.0, left: 0.0 })
                .width(Length::Fill)
                .center_x(Length::Fill),
        )
        .on_press(Message::SaveIdentity)
        .width(Length::Fill)
        .style(move |_, _| {
            let bg = if has_label { OryxisColors::t().accent } else { OryxisColors::t().bg_surface };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(8.0), ..Default::default() },
                ..Default::default()
            }
        });

        let panel_content = column![
            panel_header,
            container(
                column![
                    label_field,
                    Space::new().height(16),
                    username_field,
                    Space::new().height(16),
                    password_field,
                    Space::new().height(16),
                    key_field,
                    linked_section,
                    Space::new().height(Length::Fill),
                    save_btn,
                ]
                .height(Length::Fill)
                .width(Length::Fill)
                .align_x(dir_align_x()),
            )
            .padding(Padding { top: 0.0, right: 20.0, bottom: 20.0, left: 20.0 })
            .height(Length::Fill),
        ]
        .height(Length::Fill);

        container(panel_content)
            .width(PANEL_WIDTH)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                border: Border { color: OryxisColors::t().border, width: 1.0, radius: Radius::from(0.0) },
                ..Default::default()
            })
            .into()
    }
}
