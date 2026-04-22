//! Keys screen + identity panel + SSH key import panel.

use iced::border::Radius;
use iced::widget::{
    button, column, container, pick_list, row, scrollable, text, text_editor, text_input, MouseArea,
    Space,
};
use iced::widget::button::Status as BtnStatus;
use iced::{Background, Border, Color, Element, Length, Padding};

use oryxis_core::models::connection::Connection;
use oryxis_core::models::identity::Identity;
use oryxis_core::models::key::SshKey;

use crate::app::{Message, Oryxis, CARD_WIDTH, PANEL_WIDTH};
use crate::theme::OryxisColors;

impl Oryxis {
    pub(crate) fn view_keys(&self) -> Element<'_, Message> {
        // ── Header toolbar ──
        // Split button: left half "+ ADD" (opens menu), vertical separator,
        // right half "▼" chevron (also opens menu). Matches Termius' NEW HOST
        // control — both halves invoke the same toggle so the dropdown
        // appears below regardless of which half the user clicks.
        let add_label = button(
            container(
                row![
                    text("+").size(13).font(iced::Font {
                        weight: iced::font::Weight::Bold,
                        ..iced::Font::with_name("Inter")
                    }).color(OryxisColors::t().text_primary),
                    Space::new().width(4),
                    text("ADD").size(11).font(iced::Font {
                        weight: iced::font::Weight::Bold,
                        ..iced::Font::with_name("Inter")
                    }).color(OryxisColors::t().text_primary),
                ]
                .align_y(iced::Alignment::Center),
            )
            .center_y(Length::Fixed(24.0))
            .padding(Padding { top: 0.0, right: 14.0, bottom: 0.0, left: 14.0 }),
        )
        .on_press(Message::ToggleKeychainAddMenu)
        .style(|_, status| {
            let bg = match status {
                BtnStatus::Hovered => OryxisColors::t().accent_hover,
                _ => OryxisColors::t().accent,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border {
                    radius: Radius {
                        top_left: 6.0, bottom_left: 6.0,
                        top_right: 0.0, bottom_right: 0.0,
                    },
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

        // Chevron half — match the left half's vertical metrics so both halves
        // render at identical heights. Lateral padding is kept to the minimum
        // that still gives the glyph breathing room.
        let add_chevron = button(
            container(
                iced_fonts::lucide::chevron_down::<iced::Theme, iced::Renderer>()
                    .size(12).color(OryxisColors::t().text_primary),
            )
            .center_y(Length::Fixed(24.0))
            .padding(Padding { top: 0.0, right: 4.0, bottom: 0.0, left: 4.0 }),
        )
        .on_press(Message::ToggleKeychainAddMenu)
        .style(|_, status| {
            let bg = match status {
                BtnStatus::Hovered => OryxisColors::t().accent_hover,
                _ => OryxisColors::t().accent,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border {
                    radius: Radius {
                        top_left: 0.0, bottom_left: 0.0,
                        top_right: 6.0, bottom_right: 6.0,
                    },
                    ..Default::default()
                },
                ..Default::default()
            }
        });

        let add_btn: Element<'_, Message> = row![
            add_label,
            separator,
            add_chevron,
        ]
        .align_y(iced::Alignment::Center)
        .into();

        let toolbar = container(
            row![
                text("Keychain").size(20).color(OryxisColors::t().text_primary),
                Space::new().width(Length::Fill),
                add_btn,
            ]
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 24.0, bottom: 16.0, left: 24.0 })
        .width(Length::Fill);

        // ── Search bar ──
        let search_bar = container(
            text_input("Search keys & identities...", &self.key_search)
                .on_input(Message::KeySearchChanged)
                .padding(10)
                .width(Length::Fill),
        )
        .padding(Padding { top: 0.0, right: 24.0, bottom: 12.0, left: 24.0 })
        .width(Length::Fill);

        // ── Status message ──
        let status: Element<'_, Message> = if let Some(err) = &self.key_error {
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
        let section_title = container(
            text("Keys").size(14).color(OryxisColors::t().text_muted),
        )
        .padding(Padding { top: 4.0, right: 24.0, bottom: 8.0, left: 24.0 });

        // Filter keys by search query
        let search_lower = self.key_search.to_lowercase();
        let filtered_keys: Vec<(usize, &SshKey)> = self.keys.iter().enumerate()
            .filter(|(_, k)| search_lower.is_empty() || k.label.to_lowercase().contains(&search_lower))
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
                    button(
                        container(text(crate::i18n::t("import_key")).size(14).color(OryxisColors::t().text_primary))
                            .padding(Padding { top: 12.0, right: 0.0, bottom: 12.0, left: 0.0 })
                            .width(380)
                            .center_x(380),
                    )
                    .on_press(Message::ShowKeyPanel)
                    .width(380)
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(OryxisColors::t().accent)),
                        border: Border { radius: Radius::from(8.0), ..Default::default() },
                        ..Default::default()
                    }),
                ]
                .align_x(iced::Alignment::Center),
            )
            .center(Length::Fill);

            let main_content = column![toolbar, search_bar, status, empty_state]
                .width(Length::Fill)
                .height(Length::Fill);

            if self.show_key_panel {
                let panel = self.view_key_import_panel();
                return row![main_content, panel]
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into();
            } else if self.show_identity_panel {
                let panel = self.view_identity_panel();
                return row![main_content, panel]
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into();
            }
            return main_content.into();
        } else if filtered_keys.is_empty() {
            let no_results = container(
                text("No keys match your search").size(13).color(OryxisColors::t().text_muted),
            )
            .padding(24)
            .width(CARD_WIDTH);
            cards.push(no_results.into());
        }

        for (idx, key) in filtered_keys {
            let algo = format!("Type {}", key.algorithm);
            let icon_box = container(iced_fonts::lucide::key_round().size(18).color(Color::WHITE))
                .padding(Padding { top: 8.0, right: 10.0, bottom: 8.0, left: 10.0 })
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().accent)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                });

            // "..." menu button
            let dots_btn = button(
                text("···").size(14).color(OryxisColors::t().text_muted),
            )
            .on_press(Message::ShowKeyMenu(idx))
            .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
            .style(|_, status| {
                let bg = match status {
                    BtnStatus::Hovered => OryxisColors::t().bg_hover,
                    _ => Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                }
            });

            let card = button(
                row![
                    icon_box,
                    Space::new().width(12),
                    column![
                        text(&key.label).size(13).color(OryxisColors::t().text_primary),
                        Space::new().height(2),
                        text(algo).size(11).color(OryxisColors::t().text_muted),
                    ].width(Length::Fill),
                    dots_btn,
                ].align_y(iced::Alignment::Center),
            )
            .on_press(Message::EditKey(idx))
            .padding(16)
            .width(CARD_WIDTH)
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

            // Wrap in MouseArea for right-click
            let wrapped = MouseArea::new(card)
                .on_right_press(Message::ShowKeyMenu(idx));

            cards.push(container(wrapped).width(CARD_WIDTH).into());
        }

        // Key grid layout (3 cols)
        let mut grid_rows: Vec<Element<'_, Message>> = Vec::new();
        let mut current_row: Vec<Element<'_, Message>> = Vec::new();
        for card in cards {
            current_row.push(card);
            if current_row.len() == 3 {
                grid_rows.push(row(std::mem::take(&mut current_row)).spacing(12).into());
                grid_rows.push(Space::new().height(12).into());
            }
        }
        if !current_row.is_empty() {
            while current_row.len() < 3 {
                current_row.push(Space::new().width(CARD_WIDTH).into());
            }
            grid_rows.push(row(std::mem::take(&mut current_row)).spacing(12).into());
        }

        // ── Identities section ──
        let identity_section_title = container(
            text("Identities").size(14).color(OryxisColors::t().text_muted),
        )
        .padding(Padding { top: 16.0, right: 24.0, bottom: 8.0, left: 24.0 });

        let filtered_identities: Vec<(usize, &Identity)> = self.identities.iter().enumerate()
            .filter(|(_, i)| search_lower.is_empty() || i.label.to_lowercase().contains(&search_lower))
            .collect();

        let mut identity_cards: Vec<Element<'_, Message>> = Vec::new();

        if filtered_identities.is_empty() && self.identities.is_empty() {
            // Don't show identities section at all when empty
        } else if filtered_identities.is_empty() {
            let no_results = container(
                text("No identities match your search").size(13).color(OryxisColors::t().text_muted),
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
            let has_pw = self.vault.as_ref()
                .and_then(|v| v.get_identity_password(&identity.id).ok().flatten())
                .is_some();
            if has_pw {
                parts.push("\u{25CF}\u{25CF}\u{25CF}\u{25CF}".into());
            }
            if let Some(kid) = identity.key_id
                && let Some(k) = self.keys.iter().find(|k| k.id == kid) {
                    parts.push(k.label.clone());
            }
            let subtitle = if parts.is_empty() { "No credentials".into() } else { parts.join(", ") };

            let icon_box = container(iced_fonts::lucide::user().size(18).color(Color::WHITE))
                .padding(Padding { top: 8.0, right: 10.0, bottom: 8.0, left: 10.0 })
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().accent)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                });

            let dots_btn = button(
                text("···").size(14).color(OryxisColors::t().text_muted),
            )
            .on_press(Message::ShowIdentityMenu(idx))
            .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
            .style(|_, status| {
                let bg = match status {
                    BtnStatus::Hovered => OryxisColors::t().bg_hover,
                    _ => Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                }
            });

            let card = button(
                row![
                    icon_box,
                    Space::new().width(12),
                    column![
                        text(&identity.label).size(13).color(OryxisColors::t().text_primary),
                        Space::new().height(2),
                        text(subtitle).size(11).color(OryxisColors::t().text_muted),
                    ].width(Length::Fill),
                    dots_btn,
                ].align_y(iced::Alignment::Center),
            )
            .on_press(Message::EditIdentity(idx))
            .padding(16)
            .width(CARD_WIDTH)
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

            let wrapped = MouseArea::new(card)
                .on_right_press(Message::ShowIdentityMenu(idx));

            identity_cards.push(container(wrapped).width(CARD_WIDTH).into());
        }

        // Identity grid layout (3 cols)
        let mut identity_grid_rows: Vec<Element<'_, Message>> = Vec::new();
        let mut current_row: Vec<Element<'_, Message>> = Vec::new();
        for card in identity_cards {
            current_row.push(card);
            if current_row.len() == 3 {
                identity_grid_rows.push(row(std::mem::take(&mut current_row)).spacing(12).into());
                identity_grid_rows.push(Space::new().height(12).into());
            }
        }
        if !current_row.is_empty() {
            while current_row.len() < 3 {
                current_row.push(Space::new().width(CARD_WIDTH).into());
            }
            identity_grid_rows.push(row(std::mem::take(&mut current_row)).spacing(12).into());
        }

        // Combine keys and identities into one scrollable area
        let mut all_rows: Vec<Element<'_, Message>> = Vec::new();
        all_rows.push(section_title.into());
        all_rows.extend(grid_rows);
        if !self.identities.is_empty() {
            all_rows.push(identity_section_title.into());
            all_rows.extend(identity_grid_rows);
        }

        let grid = scrollable(
            column(all_rows).padding(Padding { top: 0.0, right: 24.0, bottom: 24.0, left: 24.0 }),
        )
        .height(Length::Fill);

        // ── Main content ──
        let main_content = column![toolbar, search_bar, status, grid]
            .width(Length::Fill)
            .height(Length::Fill);

        // ── Side panel ──
        if self.show_key_panel {
            let panel = self.view_key_import_panel();
            row![main_content, panel]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else if self.show_identity_panel {
            let panel = self.view_identity_panel();
            row![main_content, panel]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            main_content.into()
        }
    }

    pub(crate) fn view_key_import_panel(&self) -> Element<'_, Message> {
        let has_content = !self.key_import_pem.is_empty();
        let panel_title = if self.editing_key_id.is_some() { "Edit Key" } else { "Add Key" };

        // Panel header
        let panel_header = container(
            row![
                text(panel_title).size(18).color(OryxisColors::t().text_primary),
                Space::new().width(Length::Fill),
                button(text("X").size(14).color(OryxisColors::t().text_muted))
                    .on_press(Message::HideKeyPanel)
                    .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_surface)),
                        border: Border { radius: Radius::from(6.0), ..Default::default() },
                        ..Default::default()
                    }),
            ]
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 20.0, bottom: 16.0, left: 20.0 });

        // Name field
        let name_field = column![
            text("Name").size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(6),
            text_input("my-server-key", &self.key_import_label)
                .on_input(Message::KeyImportLabelChanged)
                .padding(10),
        ];

        // File selector button
        let browse_btn = button(
            container(
                row![
                    text("Select File...").size(13).color(OryxisColors::t().text_primary),
                ]
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
                row![
                    iced_fonts::lucide::circle_check()
                        .size(13)
                        .color(OryxisColors::t().success),
                    Space::new().width(6),
                    text(format!("Loaded ({} bytes)", self.key_import_pem.len()))
                        .size(12).color(OryxisColors::t().success),
                ].align_y(iced::Alignment::Center),
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

        // Error in panel
        let panel_error: Element<'_, Message> = if let Some(err) = &self.key_error {
            Element::from(text(err.clone()).size(11).color(OryxisColors::t().error))
        } else {
            Space::new().height(0).into()
        };

        // Save button
        let save_label = if self.editing_key_id.is_some() { "Update Key" } else { "Save Key" };
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
                    text("Private Key").size(12).color(OryxisColors::t().text_secondary),
                    Space::new().height(6),
                    browse_btn,
                    Space::new().height(8),
                    file_status,
                    Space::new().height(8),
                    text("Key Content").size(12).color(OryxisColors::t().text_secondary),
                    Space::new().height(6),
                    editor,
                    Space::new().height(8),
                    panel_error,
                    Space::new().height(Length::Fill),
                    save_btn,
                ]
                .height(Length::Fill),
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
        let panel_title = if self.editing_identity_id.is_some() { "Edit Identity" } else { "New Identity" };

        // Panel header
        let panel_header = container(
            row![
                text(panel_title).size(18).color(OryxisColors::t().text_primary),
                Space::new().width(Length::Fill),
                button(text("X").size(14).color(OryxisColors::t().text_muted))
                    .on_press(Message::HideIdentityPanel)
                    .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_surface)),
                        border: Border { radius: Radius::from(6.0), ..Default::default() },
                        ..Default::default()
                    }),
            ]
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 20.0, bottom: 16.0, left: 20.0 });

        // Label field
        let label_field = column![
            text("Label").size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(6),
            text_input("My Identity", &self.identity_form_label)
                .on_input(Message::IdentityLabelChanged)
                .padding(10),
        ];

        // Username field
        let username_field = column![
            text("Username").size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(6),
            row![
                iced_fonts::lucide::user().size(13).color(OryxisColors::t().text_muted),
                Space::new().width(10),
                text_input("root", &self.identity_form_username)
                    .on_input(Message::IdentityUsernameChanged)
                    .padding(10),
            ].align_y(iced::Alignment::Center),
        ];

        // Password field with eye toggle
        let password_field = column![
            text("Password").size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(6),
            row![
                iced_fonts::lucide::keyboard().size(13).color(OryxisColors::t().text_muted),
                Space::new().width(10),
                text_input(
                    if self.identity_form_has_existing_password && !self.identity_form_password_touched {
                        "\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}"
                    } else {
                        "Password"
                    },
                    &self.identity_form_password,
                )
                    .on_input(Message::IdentityPasswordChanged)
                    .secure(!self.identity_form_password_visible)
                    .padding(10),
                Space::new().width(6),
                button(
                    if self.identity_form_password_visible {
                        iced_fonts::lucide::eye_off().size(14).color(OryxisColors::t().text_muted)
                    } else {
                        iced_fonts::lucide::eye().size(14).color(OryxisColors::t().text_muted)
                    }
                )
                    .on_press(Message::IdentityTogglePasswordVisibility)
                    .style(|_t, _s| button::Style::default())
                    .padding(8),
            ].align_y(iced::Alignment::Center),
        ];

        // Key selector
        let key_options = {
            let mut opts = vec!["(none)".to_string()];
            opts.extend(self.keys.iter().map(|k| k.label.clone()));
            opts
        };
        let key_field = column![
            text("SSH Key").size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(6),
            row![
                text("+ Key").size(12).color(OryxisColors::t().accent),
                Space::new().width(16),
                pick_list(
                    key_options,
                    Some(self.identity_form_key.clone().unwrap_or_else(|| "(none)".into())),
                    Message::IdentityKeyChanged,
                ),
            ].align_y(iced::Alignment::Center),
        ];

        // Linked connections (only when editing)
        let linked_section: Element<'_, Message> = if let Some(editing_id) = self.editing_identity_id {
            let linked: Vec<&Connection> = self.connections.iter()
                .filter(|c| c.identity_id == Some(editing_id))
                .collect();
            if linked.is_empty() {
                column![
                    Space::new().height(16),
                    text("Linked to").size(12).color(OryxisColors::t().text_muted),
                    Space::new().height(4),
                    text("No connections using this identity").size(11).color(OryxisColors::t().text_muted),
                ].into()
            } else {
                let mut items: Vec<Element<'_, Message>> = vec![
                    Space::new().height(16).into(),
                    Element::from(text("Linked to").size(12).color(OryxisColors::t().text_muted)),
                    Space::new().height(4).into(),
                ];
                for conn in linked {
                    items.push(
                        container(
                            row![
                                iced_fonts::lucide::server().size(11).color(OryxisColors::t().text_muted),
                                Space::new().width(8),
                                text(&conn.label).size(12).color(OryxisColors::t().text_secondary),
                            ].align_y(iced::Alignment::Center),
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
                .height(Length::Fill),
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
