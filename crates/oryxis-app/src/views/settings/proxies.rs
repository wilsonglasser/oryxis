//! Settings -> Proxies list view + the proxy-identity side-panel form.
//! Split out of views/settings/mod.rs.

use super::*;
use iced::widget::column;

impl Oryxis {
    /// Settings → Proxies. List of saved `ProxyIdentity` rows + an
    /// inline create / edit form. Form is hidden by default; clicking
    /// "+ New" or a row's edit icon opens it pre-populated.
    pub(crate) fn view_settings_proxies(&self) -> Element<'_, Message> {
        // The standalone title binding became unused once the
        // toolbar block below inlines its own label; the previous
        // assignment leaked into the Text type-inference too. Drop
        // it explicitly so rustc doesn't try to pin down a generic
        // Theme parameter for an unread binding.
        // ── List rows ──
        let needle = self.proxy_search.trim().to_lowercase();
        let mut list = column![].spacing(8);
        // Keyboard-navigation order (one row each), collected as the
        // rows render so it always matches the filtered set on screen.
        let mut proxy_nav: Vec<Vec<crate::keynav::NavItem>> = Vec::new();
        for pi in self.proxy_identities.iter().filter(|pi| {
            needle.is_empty()
                || pi.label.to_lowercase().contains(&needle)
                || pi.host.to_lowercase().contains(&needle)
        }) {
            let kind_label = match &pi.proxy_type {
                oryxis_core::models::connection::ProxyType::Socks5 => "SOCKS5",
                oryxis_core::models::connection::ProxyType::Socks4 => "SOCKS4",
                oryxis_core::models::connection::ProxyType::Http => "HTTP",
                oryxis_core::models::connection::ProxyType::Command(_) => "CMD",
            };
            let summary = format!("{}, {}:{}", kind_label, pi.host, pi.port);
            let id = pi.id;
            proxy_nav.push(vec![crate::keynav::NavItem::Proxy(id)]);
            let kb_selected = self.keynav.selected_in(crate::keynav::FocusZone::Content)
                == Some(crate::keynav::NavItem::Proxy(id));
            let edit_btn = button(text(crate::i18n::t("edit")).size(12))
                .on_press(Message::ProxyIdentity(ProxyIdentityMessage::ShowProxyIdentityForm(Some(id))))
                .padding(Padding {
                    top: 4.0,
                    right: 10.0,
                    bottom: 4.0,
                    left: 10.0,
                })
                .style(|_, status| {
                    let bg = match status {
                        BtnStatus::Hovered => OryxisColors::t().bg_hover,
                        _ => Color::TRANSPARENT,
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border {
                            radius: Radius::from(4.0),
                            color: OryxisColors::t().border,
                            width: 1.0,
                        },
                        text_color: OryxisColors::t().text_secondary,
                        ..Default::default()
                    }
                });
            let delete_btn = button(text(crate::i18n::t("delete")).size(12))
                .on_press(Message::ProxyIdentity(ProxyIdentityMessage::DeleteProxyIdentity(id)))
                .padding(Padding {
                    top: 4.0,
                    right: 10.0,
                    bottom: 4.0,
                    left: 10.0,
                })
                .style(|_, status| {
                    let bg = match status {
                        BtnStatus::Hovered => Color { a: 0.10, ..OryxisColors::t().error },
                        _ => Color::TRANSPARENT,
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border {
                            radius: Radius::from(4.0),
                            color: OryxisColors::t().border,
                            width: 1.0,
                        },
                        text_color: OryxisColors::t().error,
                        ..Default::default()
                    }
                });
            // Card layout matching the Hosts / Keychain / Snippets
            // pattern: host_icon badge on the leading edge, label +
            // subtitle column in the middle, action buttons trailing.
            let proxy_style = crate::widgets::resolve_host_icon_style(
                None,
                &self.prefs.default_host_icon,
            );
            let glyph_el: Element<'_, Message> = iced_fonts::lucide::globe()
                .size(16)
                .line_height(1.0)
                .color(Color::WHITE)
                .into();
            let badge = crate::widgets::host_icon(
                proxy_style,
                OryxisColors::t().accent,
                &pi.label,
                Some(glyph_el),
                32.0,
            );
            let row_el = container(
                dir_row(vec![
                    badge,
                    Space::new().width(8).into(),
                    column![
                        text(&pi.label)
                            .size(13)
                            .color(OryxisColors::t().text_primary)
                            .wrapping(iced::widget::text::Wrapping::None),
                        Space::new().height(2),
                        text(summary)
                            .size(10)
                            .color(OryxisColors::t().text_muted)
                            .wrapping(iced::widget::text::Wrapping::None),
                    ]
                    .width(Length::Fill)
                    .align_x(dir_align_x())
                    .into(),
                    edit_btn.into(),
                    Space::new().width(8).into(),
                    delete_btn.into(),
                ])
                .align_y(iced::Alignment::Center),
            )
            .padding(Padding {
                top: 8.0,
                right: 12.0,
                bottom: 8.0,
                left: 8.0,
            })
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    radius: Radius::from(10.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            })
            .width(Length::Fill);
            let row_el = self.card_wash(row_el.into(), OryxisColors::t().accent);
            list = list.push(crate::widgets::select_ring_opt(
                row_el,
                10.0,
                kb_selected.then(|| OryxisColors::t().accent),
            ));
        }
        self.keynav_set_content_rows(proxy_nav);

        // "+ Proxy" button, same pattern as the other vault views: bold plus
        // glyph + bold label in the accent fill. Lives on the
        // trailing edge of the toolbar so the section header reads
        // exactly like Hosts / Keychain / Snippets.
        let add_btn: Element<'_, Message> = {
            let fg = OryxisColors::t().button_text;
            button(
                container(
                    dir_row(vec![
                        text("+").size(13).font(iced::Font {
                            weight: iced::font::Weight::Bold,
                            ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                        }).color(fg).into(),
                        Space::new().width(4).into(),
                        text(crate::i18n::t("new_proxy_identity"))
                            .size(11)
                            .font(iced::Font {
                                weight: iced::font::Weight::Bold,
                                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                            })
                            .color(fg)
                            .into(),
                    ])
                    .align_y(iced::Alignment::Center),
                )
                .center_y(Length::Fixed(24.0))
                .padding(Padding { top: 0.0, right: 14.0, bottom: 0.0, left: 14.0 }),
            )
            .on_press(Message::ProxyIdentity(ProxyIdentityMessage::ShowProxyIdentityForm(None)))
            .style(|_, status| {
                let bg = match status {
                    BtnStatus::Hovered => OryxisColors::t().button_bg_hover,
                    _ => OryxisColors::t().button_bg,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                }
            })
            .into()
        };

        // Empty + no form open → polished centered empty state (matches
        // Hosts / Keychain / Snippets), no toolbar (search hidden + the
        // "+ New" lives in the CTA).
        if self.proxy_identities.is_empty() && !self.proxy_identity_form.visible {
            let empty = crate::widgets::empty_state(
                iced_fonts::lucide::router()
                    .size(32)
                    .color(OryxisColors::t().text_muted)
                    .into(),
                crate::i18n::t("proxy_identities_empty_title").to_string(),
                crate::i18n::t("proxy_identities_empty").to_string(),
                Some((
                    crate::i18n::t("new_proxy_identity").to_string(),
                    Message::ProxyIdentity(ProxyIdentityMessage::ShowProxyIdentityForm(None)),
                )),
            );
            // No toolbar / rows on this path; drop anything recorded
            // by the previous frame so the keyboard router matches.
            self.keynav_toolbar_reset();
            self.keynav_clear_content();
            return column![empty]
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
        }

        // Toolbar: search on the leading edge (Fill), action button
        // trailing. The button hides while the form panel is open (the
        // panel carries its own Save/Cancel).
        // Responsive collapse: search yields first, then folds to an icon;
        // at the narrowest the action moves into the `…` overflow menu.
        // (The action is gone while the form panel is open.)
        // `keynav_toolbar_slot` records each rendered action for the
        // keyboard router (push order == visual order here).
        let (search_collapsed, buttons_overflow) = self.toolbar_tiers();
        self.keynav_toolbar_reset();
        let search_slot = self.vault_search_slot(search_collapsed);
        let search_slot = if search_collapsed {
            self.keynav_toolbar_slot(crate::keynav::ToolbarItem::SearchIcon, search_slot)
        } else {
            search_slot
        };
        let trailing: Element<'_, Message> = if self.proxy_identity_form.visible {
            Space::new().height(Length::Fixed(32.0)).into()
        } else if buttons_overflow {
            self.keynav_toolbar_slot(
                crate::keynav::ToolbarItem::Overflow,
                crate::widgets::bounds_reporter(
                    crate::widgets::toolbar_overflow_icon(matches!(
                        self.overlay.as_ref().map(|o| &o.content),
                        Some(crate::state::OverlayContent::ToolbarOverflow)
                    )),
                    self.toolbar_overflow_btn_bounds.clone(),
                ),
            )
        } else {
            self.keynav_toolbar_slot(crate::keynav::ToolbarItem::Primary, add_btn)
        };
        let toolbar = container(
            dir_row(vec![
                search_slot,
                Space::new().width(10).into(),
                trailing,
            ]).align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 16.0, right: 24.0, bottom: 16.0, left: 24.0 })
        .width(Length::Fill);

        let scroll = scrollable(
            column![list, Space::new().height(24)]
                .width(Length::Fill)
                .padding(Padding { top: 0.0, right: 24.0, bottom: 0.0, left: 24.0 })
                .align_x(dir_align_x()),
        )
        // Stable id so the keyboard router can keep the selected row
        // scrolled into view.
        .id(iced::widget::Id::new("proxies-list-scroll"))
        .height(Length::Fill);

        // The editor is a right-hand side panel hoisted to `view_main`
        // (active_side_panel) so it rises over the sub-nav band.
        column![toolbar, scroll]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// The inline create / edit form for a proxy identity. Used inside
    /// `view_settings_proxies` when `proxy_identity_form.visible` is on.
    pub(crate) fn view_proxy_identity_form(&self) -> Element<'_, Message> {
        use crate::state::ProxyKind;

        // Keyboard rows are recorded in visual order (row mode: Up/Down from any input).
        self.panel_nav_reset();

        // The picker only offers the four wire types, None / Identity
        // are not valid for a saved identity itself.
        let wire_kinds: &[ProxyKind] = &[
            ProxyKind::Socks5,
            ProxyKind::Socks4,
            ProxyKind::Http,
            ProxyKind::Command,
        ];

        // Focusable select: Tab reaches it, Enter/Space open it, the
        // widget owns arrows/Esc while focused (fork support).
        let kind_picker = pick_list(
            Some(self.proxy_identity_form.kind),
            wire_kinds,
            |k: &ProxyKind| k.to_string(),
        )
        .on_select(|v| Message::ProxyIdentity(ProxyIdentityMessage::ProxyIdentityFormKindChanged(v)))
        .id(iced::widget::Id::new("panel-proxy-identity-kind"))
        .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
        .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
        .padding(10)
        .style(crate::widgets::rounded_pick_list_style);

        let pw_placeholder: &str = crate::widgets::password_placeholder(
            self.proxy_identity_form.has_existing_password,
            self.proxy_identity_form.password.touched(),
            crate::i18n::t("proxy_password_placeholder"),
        );


        let save_label = if self.proxy_identity_form.editing_id.is_some() {
            crate::i18n::t("save")
        } else {
            crate::i18n::t("add")
        };
        // Shared form chrome: accent Save, muted Cancel.
        let save_btn =
            crate::widgets::form_save_button(save_label, Some(Message::ProxyIdentity(ProxyIdentityMessage::SaveProxyIdentity)));
        let cancel_btn = crate::widgets::form_cancel_button(Message::ProxyIdentity(ProxyIdentityMessage::HideProxyIdentityForm));

        // Use the shared `panel_field` helper for label/input pairs
        // gives the same 4-px gap between label and control as every
        // other form in the app, instead of glueing them together.
        use crate::widgets::panel_field;
        let col = column![
            panel_field(
                crate::i18n::t("proxy_identity_label"),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new(
                        "panel-proxy-identity-label",
                    )),
                    10.0,
                    text_input("home-bastion", &self.proxy_identity_form.label)
                        .id(iced::widget::Id::new("panel-proxy-identity-label"))
                        .on_input(|v| Message::ProxyIdentity(ProxyIdentityMessage::ProxyIdentityFormLabelChanged(v)))
                        .padding(10)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                        .into(),
                ),
            ),
            Space::new().height(12),
            panel_field(
                crate::i18n::t("proxy_type"),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new(
                        "panel-proxy-identity-kind",
                    )),
                    10.0,
                    kind_picker.into(),
                ),
            ),
            Space::new().height(12),
            panel_field(
                crate::i18n::t("proxy_host"),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new(
                        "panel-proxy-identity-host",
                    )),
                    10.0,
                    text_input(
                        crate::i18n::t("proxy_host_placeholder"),
                        &self.proxy_identity_form.host,
                    )
                    .id(iced::widget::Id::new("panel-proxy-identity-host"))
                    .on_input(|v| Message::ProxyIdentity(ProxyIdentityMessage::ProxyIdentityFormHostChanged(v)))
                    .padding(10)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                    .into(),
                ),
            ),
            Space::new().height(12),
            panel_field(
                crate::i18n::t("proxy_port"),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new(
                        "panel-proxy-identity-port",
                    )),
                    10.0,
                    text_input("1080", &self.proxy_identity_form.port)
                        .id(iced::widget::Id::new("panel-proxy-identity-port"))
                        .on_input(|v| Message::ProxyIdentity(ProxyIdentityMessage::ProxyIdentityFormPortChanged(v)))
                        .padding(6)
                        .width(70)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                        .into(),
                ),
            ),
            Space::new().height(12),
            panel_field(
                crate::i18n::t("proxy_username"),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new(
                        "panel-proxy-identity-username",
                    )),
                    10.0,
                    text_input(
                        crate::i18n::t("proxy_username_placeholder"),
                        &self.proxy_identity_form.username,
                    )
                    .id(iced::widget::Id::new("panel-proxy-identity-username"))
                    .on_input(|v| Message::ProxyIdentity(ProxyIdentityMessage::ProxyIdentityFormUsernameChanged(v)))
                    .padding(10)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                    .into(),
                ),
            ),
            Space::new().height(12),
            panel_field(crate::i18n::t("proxy_password"), {
                // Keyboard rows: the field, then its reveal eye (#52). The
                // field row is recorded first so the walk hits it before
                // the eye slot the wrap closure records.
                self.panel_nav_record(crate::keynav::RowAction::input(
                    iced::widget::Id::new("panel-proxy-identity-password"),
                ));
                crate::widgets::password_input_with_eye_nav(
                    pw_placeholder,
                    self.proxy_identity_form.password.as_str(),
                    |v| Message::ProxyIdentity(ProxyIdentityMessage::ProxyIdentityFormPasswordChanged(v.into())),
                    Some(Message::ProxyIdentity(ProxyIdentityMessage::SaveProxyIdentity)),
                    self.proxy_identity_form.password_visible,
                    Message::ProxyIdentity(ProxyIdentityMessage::ProxyIdentityFormPasswordToggleVisibility),
                    10.0,
                    Some(iced::widget::Id::new("panel-proxy-identity-password")),
                    |eye| {
                        self.panel_nav_slot(
                            crate::keynav::RowAction::activate(
                                Message::ProxyIdentity(ProxyIdentityMessage::ProxyIdentityFormPasswordToggleVisibility),
                            ),
                            6.0,
                            eye,
                        )
                    },
                )
            }),
        ];

        // ── Header (title + close), matching the host / session-group
        // side panels so every editor reads the same. The close (×) is
        // not a keyboard row: Esc already owns panel close. ──
        let title = if self.proxy_identity_form.editing_id.is_some() {
            crate::i18n::t("edit_proxy_identity")
        } else {
            crate::i18n::t("new_proxy_identity")
        };
        let panel_header = container(
            dir_row(vec![
                text(title)
                    .size(16)
                    .color(OryxisColors::t().text_primary)
                    .into(),
                Space::new().width(Length::Fill).into(),
                button(text("\u{00D7}").size(14).color(OryxisColors::t().text_muted))
                    .on_press(Message::ProxyIdentity(ProxyIdentityMessage::HideProxyIdentityForm))
                    .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(Color::TRANSPARENT)),
                        border: Border::default(),
                        ..Default::default()
                    })
                    .into(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 16.0, right: 16.0, bottom: 12.0, left: 16.0 });

        let form_scroll = scrollable(
            container(col).padding(Padding {
                top: 0.0,
                right: 16.0,
                bottom: 16.0,
                left: 16.0,
            }),
        )
        // Shared id: the keyboard router keeps the selected row in view.
        .id(iced::widget::Id::new("side-panel-scroll"))
        .height(Length::Fill);

        // Inline error sits OUTSIDE the scrollable, just above the footer,
        // so it stays visible regardless of scroll position.
        let error_el = crate::widgets::form_error(self.proxy_identity_form.error.as_deref());

        // Footer rows are recorded here (not where the buttons are
        // built above) so they land after the form fields.
        let footer = crate::widgets::form_footer(
            self.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::ProxyIdentity(ProxyIdentityMessage::HideProxyIdentityForm)),
                6.0,
                cancel_btn,
            ),
            self.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::ProxyIdentity(ProxyIdentityMessage::SaveProxyIdentity)),
                6.0,
                save_btn,
            ),
        );

        let panel_content = column![panel_header, form_scroll, error_el, footer].height(Length::Fill);

        container(panel_content)
            .width(self.panel_width)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    color: OryxisColors::t().border,
                    width: 1.0,
                    radius: Radius::from(0.0),
                },
                ..Default::default()
            })
            .into()
    }
}
