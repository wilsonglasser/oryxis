//! Settings screen, terminal, AI, theme, shortcuts, security, sync, about.

pub(crate) use iced::border::Radius;
pub(crate) use iced::widget::{button, checkbox, container, pick_list, scrollable, text, text_input, Space};
// `column` carries both a fn and a `column!` macro; re-exporting it through the
// `use super::*` glob makes the macro ambiguous in the section submodules, so it
// is imported directly here and in each section file instead.
use iced::widget::column;
pub(crate) use iced::widget::button::Status as BtnStatus;
pub(crate) use iced::{Background, Border, Color, Element, Length, Padding};

pub(crate) use crate::app::{Message, Oryxis, NAV_RAIL_WIDTH_EXPANDED};
pub(crate) use crate::i18n::t;
pub(crate) use crate::mcp::mcp_info_panel;
pub(crate) use crate::state::SettingsSection;
pub(crate) use crate::theme::OryxisColors;
pub(crate) use crate::widgets::{
    dir_align_x, dir_row, key_badge, panel_field, panel_section, settings_row, shortcut_row,
    styled_button, styled_button_opt, toggle_row,
};

// Per-section view methods, split into sibling files.
mod about;
mod ai;
mod connection;
mod interface;
mod security;
mod sftp;
mod shortcuts;
mod sync;
mod terminal;

impl Oryxis {
    /// Settings → Terminal "Local terminals" management card: the
    /// "always open X vs always ask" default picker, a grid of per-item
    /// cards (each with a hover-revealed remove), and Re-scan / Add
    /// actions in the header's top-right corner. The "Add" button opens
    /// a modal form (see `view_local_terminal_add_modal`). The list is
    /// the persisted, machine-local one populated by the one-time scan.
    pub(crate) fn local_terminals_card(&self) -> Element<'_, Message> {
        let c = OryxisColors::t();
        let entries = self.local_terminals.as_deref().unwrap_or(&[]);

        // Header: title + description on the leading edge, the Re-scan and
        // Add actions pinned to the top-right corner.
        let header = dir_row(vec![
            column![
                text(t("local_terminals")).size(13).color(c.text_primary),
                Space::new().height(4),
                text(t("local_terminals_desc")).size(11).color(c.text_muted),
            ]
            .width(Length::Fill)
            .align_x(dir_align_x())
            .into(),
            dir_row(vec![
                styled_button(t("rescan_terminals"), Message::RescanLocalTerminals, c.bg_selected),
                Space::new().width(8).into(),
                styled_button(t("add_terminal"), Message::OpenLocalTerminalAddModal, c.accent),
            ])
            .align_y(iced::Alignment::Center)
            .into(),
        ])
        .align_y(iced::Alignment::Start);

        // Default-behavior picker. The `None` sentinel = "always ask
        // (picker)"; every other option's identity is the entry id, shown
        // via the captured id->label map.
        let mut options: Vec<Option<uuid::Uuid>> = Vec::with_capacity(entries.len() + 1);
        options.push(None);
        options.extend(entries.iter().map(|e| Some(e.id)));
        let label_map: Vec<(uuid::Uuid, String)> =
            entries.iter().map(|e| (e.id, e.label.clone())).collect();
        let picker_label = t("always_ask_picker").to_string();
        let selected = Some(self.local_terminal_default);
        let default_picker = pick_list(selected, options, move |o: &Option<uuid::Uuid>| match o {
            None => picker_label.clone(),
            Some(id) => label_map
                .iter()
                .find(|(eid, _)| eid == id)
                .map(|(_, l)| l.clone())
                .unwrap_or_else(|| id.to_string()),
        })
        .on_select(Message::SetDefaultLocalTerminal)
        .width(280)
        .padding(10)
        .style(crate::widgets::rounded_pick_list_style);

        // One card per curated terminal, with a hover-revealed remove
        // action in the corner (card-action-icon convention).
        let mut cards = column![].spacing(8);
        if entries.is_empty() {
            cards = cards.push(text(t("local_terminals_empty")).size(12).color(c.text_muted));
        }
        for (idx, entry) in entries.iter().enumerate() {
            cards = cards.push(self.local_terminal_item_card(idx, entry));
        }

        panel_section(column![
            header,
            Space::new().height(14),
            text(t("default_terminal_behavior")).size(12).color(c.text_secondary),
            Space::new().height(6),
            default_picker,
            Space::new().height(16),
            cards,
        ])
    }

    /// One terminal item rendered as a card: a colored icon chip, the
    /// label (+ "manual" badge) and the resolved command line, with
    /// floating edit / remove buttons revealed on hover.
    fn local_terminal_item_card<'a>(
        &self,
        idx: usize,
        entry: &'a crate::state::LocalTerminalEntry,
    ) -> Element<'a, Message> {
        let c = OryxisColors::t();
        // Icon chip: explicit override, else OS hint, else terminal glyph.
        let (glyph, col) = crate::os_icon::local_terminal_icon(
            entry.icon.as_deref(),
            &entry.label,
            entry.color.as_deref(),
            c.accent,
        );
        let chip = container(glyph.view(16.0, Color::WHITE))
            .width(Length::Fixed(32.0))
            .height(Length::Fixed(32.0))
            .center_x(Length::Fixed(32.0))
            .center_y(Length::Fixed(32.0))
            .style(move |_| container::Style {
                background: Some(Background::Color(col)),
                border: Border { radius: Radius::from(8.0), ..Default::default() },
                ..Default::default()
            });

        let mut title: Vec<Element<'a, Message>> =
            vec![text(entry.label.clone()).size(13).color(c.text_primary).into()];
        if entry.manual {
            title.push(Space::new().width(8).into());
            title.push(
                container(text(t("manual_badge")).size(10).color(c.text_secondary))
                    .padding(Padding { top: 1.0, right: 6.0, bottom: 1.0, left: 6.0 })
                    .style(|_| container::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_selected)),
                        border: Border { radius: Radius::from(4.0), ..Default::default() },
                        ..Default::default()
                    })
                    .into(),
            );
        }
        let cmdline = if entry.args.is_empty() {
            entry.program.clone()
        } else {
            format!("{} {}", entry.program, entry.args.join(" "))
        };
        let card = container(
            dir_row(vec![
                chip.into(),
                Space::new().width(12).into(),
                column![
                    dir_row(title).align_y(iced::Alignment::Center),
                    Space::new().height(3),
                    text(cmdline).size(11).color(c.text_muted),
                ]
                .width(Length::Fill)
                .align_x(dir_align_x())
                .into(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .padding(12)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border {
                radius: Radius::from(8.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        });

        let mut stack = iced::widget::Stack::new().push(card);
        if self.hovered_local_terminal_card == Some(idx) {
            let actions = dir_row(vec![
                local_terminal_card_btn(
                    iced_fonts::lucide::pencil(),
                    c.text_secondary,
                    Message::OpenLocalTerminalEditModal(entry.id),
                ),
                Space::new().width(4).into(),
                local_terminal_card_btn(
                    iced_fonts::lucide::trash(),
                    c.error,
                    Message::RemoveLocalTerminal(entry.id),
                ),
            ])
            .align_y(iced::Alignment::Center);
            stack = stack.push(
                container(actions)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Right)
                    .align_y(iced::alignment::Vertical::Top)
                    .padding(8),
            );
        }
        iced::widget::MouseArea::new(stack)
            .on_enter(Message::LocalTerminalCardHovered(idx))
            .on_exit(Message::LocalTerminalCardUnhovered)
            .into()
    }

    /// The "add local terminal" modal: label / program / arguments form
    /// with inline validation and a Cancel / Add footer. Layered by
    /// `root_view` when `local_terminal_add_open` is set.
    pub(crate) fn view_local_terminal_add_modal(&self) -> Element<'_, Message> {
        let c = OryxisColors::t();
        let editing = self.local_terminal_form.editing_id.is_some();
        let title = if editing { t("edit_terminal") } else { t("add_terminal") };
        let submit_label = if editing { t("save") } else { t("add_terminal") };

        // Appearance box: the live icon chip on its color, clickable to
        // open the shared host icon / color picker.
        let (glyph, col) = crate::os_icon::local_terminal_icon(
            self.local_terminal_form.icon.as_deref(),
            &self.local_terminal_form.label,
            self.local_terminal_form.color.as_deref(),
            c.accent,
        );
        let appearance = button(
            dir_row(vec![
                container(glyph.view(18.0, Color::WHITE))
                    .width(Length::Fixed(36.0))
                    .height(Length::Fixed(36.0))
                    .center_x(Length::Fixed(36.0))
                    .center_y(Length::Fixed(36.0))
                    .style(move |_| container::Style {
                        background: Some(Background::Color(col)),
                        border: Border { radius: Radius::from(8.0), ..Default::default() },
                        ..Default::default()
                    })
                    .into(),
                Space::new().width(12).into(),
                text(t("terminal_icon_color")).size(13).color(c.text_secondary).into(),
                Space::new().width(Length::Fill).into(),
                iced_fonts::lucide::pencil().size(13).color(c.text_muted).into(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::OpenLocalTerminalIconPicker)
        .padding(10)
        .width(Length::Fill)
        .style(|_, status| {
            let bg = match status {
                BtnStatus::Hovered => OryxisColors::t().bg_hover,
                BtnStatus::Pressed => OryxisColors::t().bg_selected,
                _ => OryxisColors::t().bg_primary,
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
        });

        let mut body = column![
            text(title)
                .size(15)
                .font(iced::Font {
                    weight: iced::font::Weight::Semibold,
                    ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                })
                .color(c.text_primary),
            Space::new().height(16),
            panel_field(
                t("terminal_label"),
                text_input("PowerShell", &self.local_terminal_form.label)
                    .on_input(Message::LocalTerminalFormLabelChanged)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x())
                    .into(),
            ),
            Space::new().height(10),
            panel_field(
                t("terminal_program"),
                text_input("/usr/bin/zsh", &self.local_terminal_form.program)
                    .on_input(Message::LocalTerminalFormProgramChanged)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x())
                    .into(),
            ),
            Space::new().height(10),
            panel_field(
                t("terminal_args"),
                text_input("-l", &self.local_terminal_form.args)
                    .on_input(Message::LocalTerminalFormArgsChanged)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x())
                    .into(),
            ),
            Space::new().height(12),
            panel_field(t("terminal_icon_color"), appearance.into()),
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());
        if let Some(err) = self.local_terminal_form.error {
            body = body
                .push(Space::new().height(8))
                .push(text(t(err)).size(11).color(c.error));
        }
        body = body.push(Space::new().height(18)).push(dir_row(vec![
            Space::new().width(Length::Fill).into(),
            styled_button(t("cancel"), Message::CloseLocalTerminalAddModal, c.bg_selected),
            Space::new().width(8).into(),
            styled_button(submit_label, Message::AddLocalTerminalSubmit, c.accent),
        ]));

        let dialog = iced::widget::MouseArea::new(
            container(body)
                .width(Length::Fixed(420.0))
                .padding(20)
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
                }),
        )
        .on_press(Message::NoOp);
        dialog.into()
    }

    /// Live preview of the tab strip under the current appearance
    /// settings. Mirrors `active_tab_bg` and the top-bar wash in
    /// `tab_bar.rs` so what the user sees here matches the real strip
    /// as they toggle: fill style (gradient/solid), the accent underline,
    /// the top-bar wash, and the connection status dot. Sample tab labels
    /// are literal demo content (same convention as the font preview).
    pub(crate) fn tab_appearance_preview(&self) -> Element<'_, Message> {
        let accent = OryxisColors::t().accent;
        let solid = self.setting_tab_fill_style == "solid";
        // Reuse the real strip's fill helper so the preview can never
        // drift from what `tab_bar.rs` actually paints.
        let active_bg = crate::views::tab_bar::active_tab_bg(accent, solid);
        // Connection status dot: the same green "connected" cue. Only
        // present (with its trailing gap) when the dot setting is on.
        let mut active_row: Vec<Element<'_, Message>> = Vec::new();
        if self.setting_show_tab_status_dot {
            active_row.push(
                container(Space::new().width(6).height(6))
                    .style(|_| container::Style {
                        background: Some(Background::Color(OryxisColors::t().success)),
                        border: Border { radius: Radius::from(3.0), ..Default::default() },
                        ..Default::default()
                    })
                    .into(),
            );
            active_row.push(Space::new().width(6).into());
        }
        active_row.push(text("production-web").size(12).color(accent).into());
        let active_tab = container(
            dir_row(active_row).align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 7.0, right: 12.0, bottom: 7.0, left: 12.0 })
        .style(move |_| container::Style {
            background: Some(active_bg),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        });
        let idle_tab = container(
            text("staging-db").size(12).color(OryxisColors::t().text_muted),
        )
        .padding(Padding { top: 7.0, right: 12.0, bottom: 7.0, left: 12.0 });
        // Bottom hairline: 2 px accent when the underline tint is on,
        // else the neutral 1 px chrome border (mirrors `view_main`).
        let (line_h, line_color) = if self.setting_tab_accent_line {
            (2.0_f32, accent)
        } else {
            (1.0_f32, OryxisColors::t().border)
        };
        let hairline = container(Space::new().width(Length::Fill).height(line_h))
            .style(move |_| container::Style {
                background: Some(Background::Color(line_color)),
                ..Default::default()
            });
        // Top-bar wash, identical direction + mix to the real strip.
        let bar_base = OryxisColors::t().bg_sidebar;
        let bar_bg: Background = if self.setting_tab_accent_wash {
            let washed = crate::theme::mix(bar_base, accent, 0.16);
            Background::Gradient(iced::Gradient::Linear(
                iced::gradient::Linear::new(iced::Radians(std::f32::consts::FRAC_PI_2))
                    .add_stop(0.0, washed)
                    .add_stop(0.9, bar_base),
            ))
        } else {
            Background::Color(bar_base)
        };
        let strip = container(
            dir_row(vec![
                active_tab.into(),
                Space::new().width(4).into(),
                idle_tab.into(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .padding(Padding { top: 6.0, right: 8.0, bottom: 6.0, left: 8.0 })
        .style(move |_| container::Style {
            background: Some(bar_bg),
            ..Default::default()
        });
        column![strip, hairline].width(Length::Fill).into()
    }

    /// Live preview of a dashboard host card under the current dashboard
    /// settings: the default host icon shape, the optional address line,
    /// and the accent glass wash. Reuses `host_icon` and
    /// `card_accent_wash` so it tracks the real card exactly. Sample host
    /// name / address are literal demo content (like the font preview).
    pub(crate) fn card_appearance_preview(&self) -> Element<'_, Message> {
        let accent = OryxisColors::t().accent;
        let style = crate::widgets::resolve_host_icon_style(None, &self.setting_default_host_icon);
        let icon = crate::widgets::host_icon(
            style,
            accent,
            "production-web",
            Some(iced_fonts::lucide::server().size(16).color(Color::WHITE).into()),
            32.0,
        );
        let mut text_col = column![
            text("production-web").size(13).color(OryxisColors::t().text_primary),
        ];
        if self.setting_show_host_address {
            text_col = text_col
                .push(Space::new().height(2))
                .push(
                    text("deploy@10.0.0.4")
                        .size(11)
                        .color(OryxisColors::t().text_muted),
                );
        }
        let card = container(
            dir_row(vec![
                icon,
                Space::new().width(10).into(),
                text_col.width(Length::Fill).align_x(dir_align_x()).into(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .padding(Padding { top: 10.0, right: 12.0, bottom: 10.0, left: 12.0 })
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border {
                radius: Radius::from(10.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        });
        let card_el: Element<'_, Message> = card.into();
        if self.setting_card_accent_glass {
            crate::widgets::card_accent_wash(card_el, accent)
        } else {
            card_el
        }
    }

    pub(crate) fn view_settings(&self) -> Element<'_, Message> {
        // ── Settings sidebar ──
        let settings_sidebar = {
            // Order: most-touched at the top (visual + everyday
            // configuration), then per-feature toggles, then network
            // resources, then plugin / system / about. The previous
            // order was historical (followed the implementation
            // sequence) and didn't reflect how users actually move
            // through the panel.
            // Core sections, then the "feature plugin" sections (AI /
            // MCP / SFTP / Sync / Cloud Sync) which only appear once the
            // feature is enabled on the Plugins screen, then About. The
            // enable/disable toggles live on the Plugins screen, not here.
            let mut items: Vec<(&str, SettingsSection)> = vec![
                (crate::i18n::t("interface"), SettingsSection::Interface),
                (crate::i18n::t("terminal_settings"), SettingsSection::Terminal),
                (crate::i18n::t("connection"), SettingsSection::Connection),
                (crate::i18n::t("shortcuts"), SettingsSection::Shortcuts),
                (crate::i18n::t("security_privacy"), SettingsSection::Security),
                (crate::i18n::t("features_and_plugins"), SettingsSection::Plugins),
            ];
            if self.ai.enabled {
                items.push((crate::i18n::t("ai_assistant"), SettingsSection::AI));
            }
            // MCP gets its settings section once the MCP plugin is
            // present (installed / dev build), mirroring how Cloud Sync
            // appears once a cloud provider plugin is installed. The
            // server on/off toggle lives inside that section.
            if self.cloud_provider_installed("mcp") {
                items.push((crate::i18n::t("mcp_server"), SettingsSection::Mcp));
            }
            if self.sftp_enabled {
                items.push(("SFTP", SettingsSection::Sftp));
            }
            if self.sync.enabled {
                items.push((crate::i18n::t("sync"), SettingsSection::Sync));
            }
            // Cloud Sync knobs only matter once a cloud provider plugin
            // is installed (the cloud accounts themselves live on the
            // top-level Cloud surface).
            if self.any_cloud_provider_installed() {
                items.push((crate::i18n::t("settings_cloud_section"), SettingsSection::Cloud));
            }
            items.push((crate::i18n::t("about"), SettingsSection::About));
            let mut col = column![]
                .spacing(4)
                .padding(Padding { top: 8.0, right: 8.0, bottom: 8.0, left: 8.0 });

            for (label, section) in items {
                let is_active = self.settings_section == section;
                let bg = if is_active {
                    Color { a: 0.15, ..OryxisColors::t().accent }
                } else {
                    Color::TRANSPARENT
                };
                let fg = if is_active {
                    OryxisColors::t().accent
                } else {
                    OryxisColors::t().text_secondary
                };
                let btn: Element<'_, Message> = button(
                    container(text(label).size(13).color(fg))
                        .width(Length::Fill)
                        .align_x(crate::widgets::dir_align_x())
                        .padding(Padding { top: 12.0, right: 16.0, bottom: 12.0, left: 16.0 }),
                )
                .on_press(Message::ChangeSettingsSection(section))
                // Zero the button's default padding so the container's
                // 16/12 is the exact content inset.
                .padding(0)
                .width(Length::Fill)
                .style(move |_, status| {
                    let hover_bg = match status {
                        BtnStatus::Hovered if !is_active => Color::from_rgba(1.0, 1.0, 1.0, 0.08),
                        BtnStatus::Pressed => Color { a: 0.25, ..OryxisColors::t().accent },
                        _ => bg,
                    };
                    button::Style {
                        background: Some(Background::Color(hover_bg)),
                        border: Border { radius: Radius::from(10.0), ..Default::default() },
                        ..Default::default()
                    }
                })
                .into();
                col = col.push(btn);
            }

            // Wrap the section list in a scrollable so a short window
            // doesn't clip the bottom entries (About / Plugins were
            // disappearing when the height dropped below ~520 px).
            // Width matches the main vertical nav rail; no side hairline
            // so it reads as the same sidebar surface.
            container(scrollable(col).height(Length::Fill))
                .width(NAV_RAIL_WIDTH_EXPANDED)
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                    ..Default::default()
                })
        };

        // ── Settings content ──
        let settings_content: Element<'_, Message> = match self.settings_section {
            SettingsSection::Terminal => self.view_settings_terminal(),

            SettingsSection::Connection => self.view_settings_connection(),

            SettingsSection::Sftp => self.view_settings_sftp(),

            SettingsSection::AI => self.view_settings_ai(),

            SettingsSection::Interface => self.view_settings_interface(),

            SettingsSection::Shortcuts => self.view_settings_shortcuts(),

            SettingsSection::Security => self.view_settings_security(),

            SettingsSection::Sync => self.view_settings_sync(),

            SettingsSection::About => self.view_settings_about(),
            SettingsSection::Cloud => self.view_cloud_sync_settings(),
            SettingsSection::Plugins => self.view_plugins_panel(),
            SettingsSection::Mcp => self.view_settings_mcp(),
        };

        let layout = container(crate::widgets::dir_row(vec![
            settings_sidebar.into(),
            container(settings_content)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        ]))
        .width(Length::Fill)
        .height(Length::Fill);

        // Overlay the SFTP-sync host picker modal across the whole page
        // when open (same scrim + centered dialog pattern as the SFTP
        // file browser's picker).
        if self.sync.sftp.picker_open {
            iced::widget::Stack::new()
                .push(layout)
                .push(sync_host_picker_modal(self))
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            layout.into()
        }
    }

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
            let edit_btn = button(text(crate::i18n::t("edit")).size(12))
                .on_press(Message::ShowProxyIdentityForm(Some(id)))
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
                .on_press(Message::DeleteProxyIdentity(id))
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
                &self.setting_default_host_icon,
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
            list = list.push(self.card_wash(row_el.into(), OryxisColors::t().accent));
        }

        // "+ Proxy" button, same Cloud-Accounts pattern: bold plus
        // glyph + bold label in the accent fill. Lives on the
        // trailing edge of the toolbar so the section header reads
        // exactly like Hosts / Keychain / Snippets / Cloud.
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
            .on_press(Message::ShowProxyIdentityForm(None))
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
                    Message::ShowProxyIdentityForm(None),
                )),
            );
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
        let (search_collapsed, buttons_overflow) = self.toolbar_tiers();
        let trailing: Element<'_, Message> = if self.proxy_identity_form.visible {
            Space::new().width(0).height(Length::Fixed(32.0)).into()
        } else if buttons_overflow {
            crate::widgets::toolbar_overflow_icon(matches!(
                self.overlay.as_ref().map(|o| &o.content),
                Some(crate::state::OverlayContent::ToolbarOverflow)
            ))
        } else {
            add_btn
        };
        let toolbar = container(
            dir_row(vec![
                self.vault_search_slot(search_collapsed),
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
        .height(Length::Fill);

        // The editor is a right-hand side panel hoisted to `view_main`
        // (active_side_panel) so it rises over the sub-nav band.
        column![toolbar, scroll]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Standalone MCP Server settings section. Was nested inside the
    /// Security panel in 0.6 when MCP shipped with the installer; in
    /// 0.7 it lives in its own Settings sidebar entry because the
    /// plugin distribution + setup-guide affordances deserve room
    /// without competing with the Security toggles.
    fn view_settings_mcp(&self) -> Element<'_, Message> {
        // The MCP plugin is managed (installed / updated) from the
        // Plugins screen; the server's own on/off lives here.
        let mcp_guide_btn = button(
            container(text(crate::i18n::t("mcp_setup_guide")).size(12).color(OryxisColors::t().accent))
                .padding(Padding { top: 6.0, right: 16.0, bottom: 6.0, left: 16.0 }),
        )
        .on_press(if self.mcp.show_info { Message::HideMcpInfo } else { Message::ShowMcpInfo })
        .style(|_, status| {
            let bg = match status {
                BtnStatus::Hovered => Color { a: 0.1, ..OryxisColors::t().accent },
                _ => Color::TRANSPARENT,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(6.0), color: OryxisColors::t().accent, width: 1.0 },
                ..Default::default()
            }
        });
        let mut mcp_col = column![
            toggle_row(
                crate::i18n::t("mcp_server"),
                self.mcp.server_enabled,
                Message::ToggleMcpServer,
            ),
            Space::new().height(12),
            dir_row(vec![
                text(crate::i18n::t("mcp_server_desc")).size(11).color(OryxisColors::t().text_muted).into(),
                Space::new().width(Length::Fill).into(),
                mcp_guide_btn.into(),
            ]).align_y(iced::Alignment::Center),
        ];
        if self.mcp.show_info {
            mcp_col = mcp_col
                .push(Space::new().height(12))
                .push(mcp_info_panel(
                    self.mcp.config_copied,
                    &self.mcp.install_status,
                    &self.mcp.server_token,
                    self.mcp.token_visible,
                    self.mcp.target_wsl,
                ));
        }

        scrollable(
            container(
                column![
                    panel_section(mcp_col),
                    Space::new().height(24),
                ]
                .width(Length::Fill)
                .align_x(dir_align_x()),
            )
            .padding(Padding { top: 24.0, right: 24.0, bottom: 24.0, left: 24.0 }),
        )
        .height(Length::Fill)
        .into()
    }

    /// The inline create / edit form for a proxy identity. Used inside
    /// `view_settings_proxies` when `proxy_identity_form.visible` is on.
    pub(crate) fn view_proxy_identity_form(&self) -> Element<'_, Message> {
        use crate::state::ProxyKind;

        // The picker only offers the four wire types, None / Identity
        // are not valid for a saved identity itself.
        let wire_kinds: &[ProxyKind] = &[
            ProxyKind::Socks5,
            ProxyKind::Socks4,
            ProxyKind::Http,
            ProxyKind::Command,
        ];

        let kind_picker = pick_list(
            Some(self.proxy_identity_form.kind),
            wire_kinds,
            |k: &ProxyKind| k.to_string(),
        )
        .on_select(Message::ProxyIdentityFormKindChanged)
        .padding(10)
        .style(crate::widgets::rounded_pick_list_style);

        let pw_placeholder: &str = if self.proxy_identity_form.has_existing_password
            && !self.proxy_identity_form.password_touched
        {
            crate::i18n::t("proxy_password_existing")
        } else {
            crate::i18n::t("proxy_password_placeholder")
        };

        let pw_input = text_input(pw_placeholder, &self.proxy_identity_form.password)
            .on_input(Message::ProxyIdentityFormPasswordChanged)
            .secure(!self.proxy_identity_form.password_visible)
            .padding(10)
            .style(crate::widgets::rounded_input_style).align_x(dir_align_x());

        let save_label = if self.proxy_identity_form.editing_id.is_some() {
            crate::i18n::t("save")
        } else {
            crate::i18n::t("add")
        };
        // Match the keychain / vault buttons: bold accent for the
        // primary action, muted color for cancel.
        let save_btn = styled_button(
            save_label,
            Message::SaveProxyIdentity,
            OryxisColors::t().accent,
        );
        let cancel_btn = styled_button(
            crate::i18n::t("cancel"),
            Message::HideProxyIdentityForm,
            OryxisColors::t().text_muted,
        );

        // Use the shared `panel_field` helper for label/input pairs
        // gives the same 4-px gap between label and control as every
        // other form in the app, instead of glueing them together.
        use crate::widgets::panel_field;
        let col = column![
            panel_field(
                crate::i18n::t("proxy_identity_label"),
                text_input("home-bastion", &self.proxy_identity_form.label)
                    .on_input(Message::ProxyIdentityFormLabelChanged)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                    .into(),
            ),
            Space::new().height(12),
            panel_field(crate::i18n::t("proxy_type"), kind_picker.into()),
            Space::new().height(12),
            panel_field(
                crate::i18n::t("proxy_host"),
                text_input(
                    crate::i18n::t("proxy_host_placeholder"),
                    &self.proxy_identity_form.host,
                )
                .on_input(Message::ProxyIdentityFormHostChanged)
                .padding(10)
                .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                .into(),
            ),
            Space::new().height(12),
            panel_field(
                crate::i18n::t("proxy_port"),
                text_input("1080", &self.proxy_identity_form.port)
                    .on_input(Message::ProxyIdentityFormPortChanged)
                    .padding(6)
                    .width(70)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                    .into(),
            ),
            Space::new().height(12),
            panel_field(
                crate::i18n::t("proxy_username"),
                text_input(
                    crate::i18n::t("proxy_username_placeholder"),
                    &self.proxy_identity_form.username,
                )
                .on_input(Message::ProxyIdentityFormUsernameChanged)
                .padding(10)
                .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                .into(),
            ),
            Space::new().height(12),
            panel_field(crate::i18n::t("proxy_password"), pw_input.into()),
        ];

        // ── Header (title + close), matching the host / session-group
        // side panels so every editor reads the same. ──
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
                    .on_press(Message::HideProxyIdentityForm)
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
        .height(Length::Fill);

        // Inline error sits OUTSIDE the scrollable, just above the footer,
        // so it stays visible regardless of scroll position.
        let error_el: Element<'_, Message> = if let Some(err) = &self.proxy_identity_form.error {
            container(text(err.as_str()).size(12).color(OryxisColors::t().error))
                .padding(Padding { top: 0.0, right: 16.0, bottom: 8.0, left: 16.0 })
                .into()
        } else {
            Space::new().height(0).into()
        };

        let footer = container(
            dir_row(vec![cancel_btn, Space::new().width(8).into(), save_btn])
                .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 8.0, right: 16.0, bottom: 16.0, left: 16.0 });

        let panel_content = column![panel_header, form_scroll, error_el, footer].height(Length::Fill);

        container(panel_content)
            .width(crate::app::PANEL_WIDTH)
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

    /// Single row in the Shortcuts editor list. Renders:
    /// - capture state ("Press a key…") when this row is currently
    ///   being edited;
    /// - badges + clickable surface in normal state;
    /// - reset button only when the binding differs from the
    ///   factory default, so the user can spot overrides at a glance.
    pub(crate) fn hotkey_editor_row(
        &self,
        action: crate::hotkeys::HotkeyAction,
        default: Option<crate::hotkeys::HotkeyBinding>,
    ) -> Element<'_, Message> {
        let binding = self.hotkey_bindings.get(&action).copied();
        let is_editing = self.editing_hotkey == Some(action);
        let is_overridden = matches!(
            (binding, default),
            (Some(b), Some(d)) if b != d
        );

        // Badge cluster: either the captured-mode placeholder or the
        // serialized binding pills. For family actions the suffix
        // badge is rendered with a distinct muted style so the user
        // sees at a glance which slot is fixed.
        let pills: Element<'_, Message> = if is_editing {
            text(crate::i18n::t("hotkey_press_a_key"))
                .size(12)
                .color(OryxisColors::t().accent)
                .into()
        } else if let Some(b) = binding {
            let labels = b.badges();
            let n = labels.len();
            let primary_editable = action.primary_editable();
            let badges: Vec<Element<'_, Message>> = labels
                .into_iter()
                .enumerate()
                .map(|(i, lbl)| {
                    let is_suffix = i == n - 1;
                    if is_suffix && !primary_editable {
                        // Fixed-suffix badge: same solid pill as the
                        // modifiers so it stays legible, but with a
                        // dashed-feel via a tinted border + muted
                        // text. The earlier alpha-40 background
                        // washed out completely against the dark
                        // button surface; this keeps the visual
                        // distinction without losing contrast.
                        container(
                            text(lbl)
                                .size(11)
                                .color(OryxisColors::t().text_secondary),
                        )
                        .padding(Padding {
                            top: 3.0,
                            right: 6.0,
                            bottom: 3.0,
                            left: 6.0,
                        })
                        .style(|_| container::Style {
                            background: Some(Background::Color(OryxisColors::t().bg_selected)),
                            border: Border {
                                radius: Radius::from(4.0),
                                color: OryxisColors::t().border,
                                width: 1.0,
                            },
                            ..Default::default()
                        })
                        .into()
                    } else {
                        key_badge_owned(lbl)
                    }
                })
                .collect();
            iced::widget::Row::with_children(badges)
                .spacing(4)
                .align_y(iced::Alignment::Center)
                .into()
        } else {
            // Unbound (post-conflict). Render a dashed placeholder.
            text(crate::i18n::t("hotkey_unbound"))
                .size(11)
                .color(OryxisColors::t().text_muted)
                .into()
        };

        // Wrap badges in a clickable surface (button with neutral
        // style). The editing-state row gets an accent border so it
        // reads "pending input" against the other rows.
        let pills_btn = button(pills).on_press(Message::StartEditingHotkey(action)).style(
            move |_, status| {
                let bg = match status {
                    BtnStatus::Hovered => OryxisColors::t().button_bg_hover,
                    _ => OryxisColors::t().button_bg,
                };
                let border_color = if is_editing {
                    OryxisColors::t().accent
                } else {
                    OryxisColors::t().border
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border {
                        radius: Radius::from(6.0),
                        color: border_color,
                        width: 1.0,
                    },
                    ..Default::default()
                }
            },
        );
        let pills_box = container(pills_btn)
            .width(220)
            .align_x(dir_align_x());

        let label = text(crate::i18n::t(action.label_key()))
            .size(13)
            .color(OryxisColors::t().text_secondary);

        let reset_el: Element<'_, Message> = if is_overridden {
            button(
                text(crate::i18n::t("hotkey_reset"))
                    .size(11)
                    .color(OryxisColors::t().text_muted),
            )
            .on_press(Message::ResetHotkey(action))
            .style(|_, status| {
                let bg = match status {
                    BtnStatus::Hovered => Some(Background::Color(OryxisColors::t().button_bg_hover)),
                    _ => None,
                };
                button::Style {
                    background: bg,
                    border: Border {
                        radius: Radius::from(4.0),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            })
            .into()
        } else {
            Space::new().width(0).into()
        };

        dir_row(vec![
            pills_box.into(),
            label.into(),
            Space::new().width(Length::Fill).into(),
            reset_el,
        ])
        .align_y(iced::Alignment::Center)
        .into()
    }
}

/// Floating icon button for a local-terminal card's hover actions
/// (edit / remove). `fg` tints the glyph; the background carries the
/// hover / press feedback.
fn local_terminal_card_btn<'a>(
    icon: iced::widget::Text<'a>,
    fg: Color,
    msg: Message,
) -> Element<'a, Message> {
    button(
        container(icon.size(13).color(fg))
            .center_x(Length::Fixed(24.0))
            .center_y(Length::Fixed(24.0)),
    )
    .on_press(msg)
    .padding(0)
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().bg_hover,
            BtnStatus::Pressed => OryxisColors::t().bg_selected,
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
    .into()
}

/// Owned-label variant of `widgets::key_badge`. The editor builds
/// labels at runtime from `HotkeyBinding::badges()` so we can't reuse
/// the `&'a str` shape directly without leaking.
fn key_badge_owned(label: String) -> Element<'static, Message> {
    container(text(label).size(11).color(OryxisColors::t().text_primary))
        .padding(Padding {
            top: 3.0,
            right: 6.0,
            bottom: 3.0,
            left: 6.0,
        })
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_selected)),
            border: Border {
                radius: Radius::from(4.0),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

/// OS-icon avatar for a host, matching the dashboard card and the SFTP
/// file-browser picker. Output lifetime is tied to `conn` (the glyph and
/// label borrow it); `default_icon` only feeds the owned style lookup.
pub(crate) fn host_badge<'a>(
    conn: &'a oryxis_core::models::connection::Connection,
    default_icon: &str,
    size: f32,
) -> Element<'a, Message> {
    let (glyph, default_color) =
        crate::os_icon::resolve_icon(conn.detected_os.as_deref(), OryxisColors::t().accent);
    let badge_style =
        crate::widgets::resolve_host_icon_style(conn.icon_style.as_deref(), default_icon);
    let badge_color = conn
        .custom_color
        .as_deref()
        .or(conn.color.as_deref())
        .and_then(crate::widgets::parse_hex_color)
        .unwrap_or(default_color);
    let glyph_el: Element<'a, Message> = glyph.view(size * 0.58, Color::WHITE);
    crate::widgets::host_icon(badge_style, badge_color, &conn.label, Some(glyph_el), size)
}

/// The "Select a host" modal for the SFTP-sync backup host. Mirrors the
/// SFTP file-browser picker: a searchable list of saved hosts, each row an
/// OS badge + label + address. Rendered as a dimming scrim plus a centered
/// dialog; the caller stacks it over the settings page.
fn sync_host_picker_modal(app: &Oryxis) -> Element<'_, Message> {
    let q = app.sync.sftp.picker_search.to_lowercase();
    let mut list = column![].spacing(2);
    for conn in app.connections.iter().filter(|c| {
        q.is_empty()
            || c.label.to_lowercase().contains(&q)
            || c.hostname.to_lowercase().contains(&q)
    }) {
        let badge = host_badge(conn, &app.setting_default_host_icon, 24.0);
        let row_btn = button(
            dir_row(vec![
                badge,
                Space::new().width(10).into(),
                column![
                    text(conn.label.clone())
                        .size(13)
                        .color(OryxisColors::t().text_primary),
                    text(conn.hostname.clone())
                        .size(10)
                        .color(OryxisColors::t().text_muted),
                ]
                .width(Length::Fill)
                .align_x(dir_align_x())
                .into(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::SyncSftpHostChanged(conn.id))
        .padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 })
        .width(Length::Fill)
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
        });
        list = list.push(row_btn);
    }

    let dialog = container(
        column![
            dir_row(vec![
                text(t("select_a_host"))
                    .size(15)
                    .color(OryxisColors::t().text_primary)
                    .into(),
                Space::new().width(Length::Fill).into(),
                button(text("\u{2715}").size(13).color(OryxisColors::t().text_muted))
                    .on_press(Message::SyncSftpClosePicker)
                    .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                    .style(|_, status| {
                        let bg = match status {
                            BtnStatus::Hovered => OryxisColors::t().bg_hover,
                            _ => Color::TRANSPARENT,
                        };
                        button::Style {
                            background: Some(Background::Color(bg)),
                            border: Border {
                                radius: Radius::from(4.0),
                                ..Default::default()
                            },
                            ..Default::default()
                        }
                    })
                    .into(),
            ])
            .align_y(iced::Alignment::Center)
            .width(Length::Fill),
            Space::new().height(8),
            text_input(t("search_hosts"), &app.sync.sftp.picker_search)
                .on_input(Message::SyncSftpPickerSearch)
                .padding(10)
                .style(crate::widgets::rounded_input_style)
                .align_x(dir_align_x()),
            Space::new().height(8),
            scrollable(list).height(Length::Fixed(360.0)),
        ]
        .padding(20)
        .width(Length::Fixed(440.0))
        .align_x(dir_align_x()),
    )
    .style(|_| container::Style {
        background: Some(Background::Color(OryxisColors::t().bg_surface)),
        border: Border {
            radius: Radius::from(12.0),
            color: OryxisColors::t().border,
            width: 1.0,
        },
        ..Default::default()
    });

    let scrim: Element<'_, Message> = iced::widget::opaque(
        iced::widget::MouseArea::new(
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5))),
                    ..Default::default()
                }),
        )
        .on_press(Message::SyncSftpClosePicker),
    );

    let centered = container(iced::widget::MouseArea::new(dialog).on_press(Message::NoOp))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill);

    iced::widget::Stack::new()
        .push(scrim)
        .push(centered)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// i18n key for an export/import category's checkbox label.
pub(crate) fn category_label_key(c: oryxis_vault::ExportCategory) -> &'static str {
    use oryxis_vault::ExportCategory as C;
    match c {
        C::Connections => "cat_connections",
        C::Groups => "cat_groups",
        C::Keys => "cat_keys",
        C::Identities => "cat_identities",
        C::ProxyIdentities => "cat_proxies",
        C::CloudProfiles => "cat_cloud_profiles",
        C::Snippets => "cat_snippets",
        C::KnownHosts => "cat_known_hosts",
        C::PortForwardRules => "cat_port_forwards",
        C::SessionGroups => "cat_session_layouts",
        C::Settings => "cat_settings",
    }
}
