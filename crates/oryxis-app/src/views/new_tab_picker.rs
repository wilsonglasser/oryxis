//! New-tab picker, centered modal overlay with a search bar and a
//! drill-down list: top level shows groups (folders) + the recent
//! connections, and clicking a group drills into it. Manual groups reveal
//! their sub-groups and member connections. Triggered from the `+` button
//! in the tab bar, or from a pane split.
//!
//! Visually modeled on Termius' "New Tab" screen: big rounded search at the
//! top, then a grouped list with host-icon badges and a "Personal / Group"
//! breadcrumb on the right.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, scrollable, text, text_input, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use oryxis_core::models::Group;

use crate::app::{SftpMessage, TabsMessage, SshMessage, Message, Oryxis};
use crate::i18n::t;
use crate::theme::OryxisColors;
use crate::widgets::{dir_align_x, dir_row};

impl Oryxis {
    /// Build the new-tab picker modal. The caller is responsible for checking
    /// `self.panels.new_tab_picker` before rendering and stacking it on top of
    /// the base view.
    pub(crate) fn view_new_tab_picker(&self) -> Element<'_, Message> {
        // Keyboard rows are recorded by the row builders below in
        // visual order; Up/Down move a selection, Enter is owned by
        // the search's on_submit (selection-aware, see
        // NewTabPickerSubmit) so the two paths can't double-fire.
        self.modal_nav_reset();
        // Shortcut hint, resolved from the live binding table (never
        // hard-coded, the tab-jump default changed once already, issue
        // #100). `None` when the action is unbound: then no chip at
        // all, an empty styled pill reads as a rendering glitch.
        let hotkey_hint =
            self.hotkey_label_for_action(crate::hotkeys::HotkeyAction::ShowNewTabPicker);
        // Internal right-padding leaves room for the floating hotkey
        // affordance so the typed value never slides under the hint.
        let search = text_input(t("search_hosts_or_tabs"), &self.new_tab_picker_search)
            .id(iced::widget::Id::new(crate::state::NEW_TAB_PICKER_SEARCH_ID))
            .on_input(|v| Message::Tabs(TabsMessage::NewTabPickerSearchChanged(v)))
            .on_submit(Message::Tabs(TabsMessage::NewTabPickerSubmit))
            .padding(Padding {
                top: 14.0,
                right: if hotkey_hint.is_some() { 64.0 } else { 14.0 },
                bottom: 14.0,
                left: 14.0,
            })
            .size(14)
            .style(crate::widgets::rounded_input_style).align_x(dir_align_x());

        let mut search_block = iced::widget::Stack::new()
            .push(search)
            .width(Length::Fill);
        // Right-anchored shortcut hint inside a styled chip so it reads
        // as a keyboard affordance rather than placeholder text. Lives
        // in a Stack on top of the input, `text` has no click handler,
        // so focus-on-click still works on the wider left portion.
        if let Some(hint) = hotkey_hint {
            let ctrl_k_chip = container(
                text(hint)
                    .size(11)
                    .color(OryxisColors::t().text_muted),
            )
            .padding(Padding {
                top: 2.0,
                right: 6.0,
                bottom: 2.0,
                left: 6.0,
            })
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_hover)),
                border: Border {
                    radius: Radius::from(4.0),
                    ..Default::default()
                },
                ..Default::default()
            });
            let ctrl_k_overlay = container(ctrl_k_chip)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(iced::alignment::Horizontal::Right)
                .align_y(iced::alignment::Vertical::Center)
                .padding(Padding {
                    top: 0.0,
                    right: 12.0,
                    bottom: 0.0,
                    left: 0.0,
                });
            search_block = search_block.push(ctrl_k_overlay);
        }

        let needle = self.new_tab_picker_search.to_lowercase();

        // Resolve the drilled-into group (if any) up front so the body
        // builder and the back-header agree on the level being shown.
        let drilled = self
            .new_tab_picker_group
            .and_then(|gid| self.groups.iter().find(|g| g.id == gid));

        let list_inner: Vec<Element<'_, Message>> = match drilled {
            Some(group) => self.picker_group_rows(group, &needle),
            None => self.picker_top_level_rows(&needle),
        };

        let list_panel = container(column(list_inner).spacing(2))
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

        let list_scroll = scrollable(list_panel)
            // Stable id so the keyboard selection can be kept in view.
            .id(iced::widget::Id::new("new-tab-picker-scroll"))
            .height(Length::Fill);

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

        // Bare card; `widgets::modal_overlay` (the caller) owns centering,
        // the absorbing scrim, and the click-trap.
        body.into()
    }

    /// Top-level rows: a "Groups" section (root groups as drillable
    /// folders) followed by the flat "Recent connections" list.
    fn picker_top_level_rows(&self, needle: &str) -> Vec<Element<'_, Message>> {
        let mut rows: Vec<Element<'_, Message>> = Vec::new();

        // Ad-hoc quick connect: a search input that parses as
        // `user@host[:port]` offers an immediate, unsaved connect as the
        // very first row (Enter activates it via `NewTabPickerSubmit`).
        // Raw input, not the lowercased needle: usernames keep their case.
        if let Some(conn) = self.quick_connect_target(&self.new_tab_picker_search) {
            let msg = Message::Ssh(SshMessage::QuickConnect(Box::new(crate::state::QuickConnectEntry::bare(
                conn.clone(),
            ))));
            rows.push(self.modal_nav_slot(
                crate::keynav::RowAction::activate(msg),
                6.0,
                false,
                quick_connect_row(conn),
            ));
            rows.push(Space::new().height(14).into());
        }

        // Local shell, always first. Routes into the pending pane (split)
        // or a fresh tab, handled by `Message::Tabs(TabsMessage::PickLocalShell)`. SFTP follows
        // when enabled, routed through
        // `Message::Sftp(SftpMessage::NewSftpTab)`.
        let want_local =
            needle.is_empty() || t("local_shell").to_lowercase().contains(needle);
        let want_sftp = self.sftp_enabled
            && (needle.is_empty() || t("sftp").to_lowercase().contains(needle));
        if want_local {
            rows.push(self.modal_nav_slot(
                crate::keynav::RowAction::activate(Message::Tabs(TabsMessage::PickLocalShell)),
                6.0,
                false,
                // Hints resolve from the live bindings, so a rebind flows
                // through here without a rebuild, same as the burger menu.
                // Both entries exist in that menu too and carry the hint
                // there; showing it only in one of the two places is how a
                // user concludes the shortcut doesn't exist.
                local_shell_row(
                    self.hotkey_label_for_action(crate::hotkeys::HotkeyAction::OpenLocalShell),
                ),
            ));
        }
        if want_sftp {
            rows.push(self.modal_nav_slot(
                crate::keynav::RowAction::activate(Message::Sftp(SftpMessage::NewSftpTab)),
                6.0,
                false,
                sftp_row(self.hotkey_label_for_action(crate::hotkeys::HotkeyAction::OpenSftp)),
            ));
        }
        if want_local || want_sftp {
            rows.push(Space::new().height(14).into());
        }

        // Root groups (parent_id == None). Sub-groups surface when the user
        // drills into their parent, mirroring the dashboard hierarchy.
        let mut group_rows: Vec<Element<'_, Message>> = Vec::new();
        for g in self.groups.iter().filter(|g| g.parent_id.is_none()) {
            // Hide empty folders (no hosts, no sub-groups): there's
            // nothing to open inside, so they'd just be dead rows. Mirrors
            // the dashboard, which only renders a root folder with a direct
            // connection or a sub-group.
            if self.picker_group_child_count(g.id) == 0 {
                continue;
            }
            if !needle.is_empty() && !g.label.to_lowercase().contains(needle) {
                continue;
            }
            group_rows.push(self.picker_group_row(g));
        }
        if !group_rows.is_empty() {
            rows.push(section_header(t("groups_section")));
            rows.push(Space::new().height(8).into());
            rows.extend(group_rows);
            rows.push(Space::new().height(14).into());
        }

        // Recent connections: every saved host, most-recently-used first.
        let mut idxs: Vec<usize> = (0..self.connections.len())
            .filter(|&i| {
                if needle.is_empty() {
                    return true;
                }
                let c = &self.connections[i];
                c.label.to_lowercase().contains(needle)
                    || c.hostname.to_lowercase().contains(needle)
            })
            .collect();
        idxs.sort_by(|a, b| {
            let la = self.connections[*a].last_used;
            let lb = self.connections[*b].last_used;
            lb.cmp(&la)
        });

        rows.push(section_header(t("recent_connections")));
        rows.push(Space::new().height(8).into());
        if idxs.is_empty() {
            rows.push(info_row(if needle.is_empty() {
                t("no_connections_yet")
            } else {
                t("no_matches")
            }));
        } else {
            let privacy_terms = self.privacy_terms();
            for (pos, ci) in idxs.iter().enumerate() {
                rows.push(self.connection_row(*ci, pos, &privacy_terms));
            }
        }
        rows
    }

    /// Rows for a drilled-into group: a back header, then the
    /// group's sub-groups + member connections.
    fn picker_group_rows<'a>(
        &'a self,
        group: &'a Group,
        needle: &str,
    ) -> Vec<Element<'a, Message>> {
        let mut rows: Vec<Element<'a, Message>> = vec![self.modal_nav_slot(
            crate::keynav::RowAction::activate(Message::Tabs(TabsMessage::NewTabPickerBack)),
            6.0,
            false,
            back_header(&group.label),
        )];
        rows.push(Space::new().height(8).into());

        // Sub-groups first, then member connections.
        let mut any = false;
        for g in self.groups.iter().filter(|g| g.parent_id == Some(group.id)) {
            // Same empty-folder hiding as the top level (see there).
            if self.picker_group_child_count(g.id) == 0 {
                continue;
            }
            if !needle.is_empty() && !g.label.to_lowercase().contains(needle) {
                continue;
            }
            rows.push(self.picker_group_row(g));
            any = true;
        }
        let mut member_idxs: Vec<usize> = (0..self.connections.len())
            .filter(|&i| self.connections[i].group_id == Some(group.id))
            .filter(|&i| {
                if needle.is_empty() {
                    return true;
                }
                let c = &self.connections[i];
                c.label.to_lowercase().contains(needle)
                    || c.hostname.to_lowercase().contains(needle)
            })
            .collect();
        member_idxs.sort_by(|a, b| {
            self.connections[*b].last_used.cmp(&self.connections[*a].last_used)
        });
        let privacy_terms = self.privacy_terms();
        for (pos, ci) in member_idxs.iter().enumerate() {
            rows.push(self.connection_row(*ci, pos, &privacy_terms));
            any = true;
        }
        if !any {
            rows.push(info_row(if needle.is_empty() {
                t("no_connections_yet")
            } else {
                t("no_matches")
            }));
        }
        rows
    }

    /// Direct child count of a manual folder: its own connections plus its
    /// immediate sub-groups. Drives both the trailing count badge and the
    /// empty-folder hiding, so the number shown always matches whether the
    /// folder is shown at all (count 0 -> hidden).
    fn picker_group_child_count(&self, gid: uuid::Uuid) -> usize {
        let conns = self
            .connections
            .iter()
            .filter(|c| c.group_id == Some(gid))
            .count();
        let subs = self
            .groups
            .iter()
            .filter(|g| g.parent_id == Some(gid))
            .count();
        conns + subs
    }

    /// A drillable folder row for `group`, emitting `NewTabPickerOpenGroup`:
    /// a folder glyph + a child count.
    fn picker_group_row<'a>(&self, group: &'a Group) -> Element<'a, Message> {
        let glyph: Element<'a, Message> = iced_fonts::lucide::folder()
            .size(15)
            .color(OryxisColors::t().accent)
            .into();

        let subtitle = self.picker_group_child_count(group.id).to_string();

        // Trailing chevron points into the group; mirror it under RTL.
        let chevron: Element<'a, Message> = if crate::i18n::is_rtl_layout() {
            iced_fonts::lucide::chevron_left()
        } else {
            iced_fonts::lucide::chevron_right()
        }
        .size(15)
        .color(OryxisColors::t().text_muted)
        .into();

        let inner = dir_row(vec![
            glyph,
            Space::new().width(12).into(),
            text(group.label.clone())
                .size(13)
                .font(iced::Font {
                    weight: iced::font::Weight::Semibold,
                    ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                })
                .color(OryxisColors::t().text_primary)
                .into(),
            Space::new().width(Length::Fill).into(),
            text(subtitle).size(12).color(OryxisColors::t().text_muted).into(),
            Space::new().width(10).into(),
            chevron,
        ])
        .align_y(iced::Alignment::Center);

        let row: Element<'a, Message> = button(
            container(inner)
                .padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 })
                .width(Length::Fill),
        )
        .on_press(Message::Tabs(TabsMessage::NewTabPickerOpenGroup(group.id)))
        .width(Length::Fill)
        .style(hover_row_style)
        .into();
        self.modal_nav_slot(
            crate::keynav::RowAction::activate(Message::Tabs(TabsMessage::NewTabPickerOpenGroup(group.id))),
            6.0,
            false,
            row,
        )
    }

    /// A saved-connection row (mirrors the host card badge + breadcrumb),
    /// emitting `ConnectSsh`. `pos` drives the zebra stripe. `terms` is
    /// the caller's one-per-list `privacy_terms()` pass; Privacy Mode
    /// redacts the rendered label with it (issue #78). No hover reveal
    /// here: the picker is transient and the search input echoes raw.
    fn connection_row(&self, ci: usize, pos: usize, terms: &[String]) -> Element<'_, Message> {
        let conn = &self.connections[ci];
        let display_label = if self.privacy_active(conn) {
            crate::widgets::redact_for_display(&conn.label, terms, self.privacy_classes())
        } else {
            conn.label.clone()
        };
        let group_name = conn.group_id.and_then(|gid| {
            self.groups.iter().find(|g| g.id == gid).map(|g| g.label.clone())
        });
        let breadcrumb = match group_name {
            Some(g) => format!("{} / {}", t("personal"), g),
            None => t("personal").to_string(),
        };
        let zebra_bg = if pos % 2 == 1 {
            OryxisColors::t().bg_hover
        } else {
            Color::TRANSPARENT
        };
        let badge_style = crate::widgets::resolve_host_icon_style(
            conn.icon_style.as_deref(),
            &self.prefs.default_host_icon,
        );
        let (glyph, default_color) = crate::os_icon::resolve_icon(
            conn.detected_os.as_deref(),
            OryxisColors::t().accent,
        );
        let badge_color = conn
            .custom_color
            .as_deref()
            .or(conn.color.as_deref())
            .and_then(crate::widgets::parse_hex_color)
            .unwrap_or(default_color);
        let glyph_el: Element<'_, Message> = glyph.view(12.0, Color::WHITE);
        let badge = crate::widgets::host_icon(badge_style, badge_color, &display_label, Some(glyph_el), 26.0);
        self.modal_nav_slot(
            crate::keynav::RowAction::activate(Message::Ssh(SshMessage::ConnectSsh(ci))),
            6.0,
            false,
            picker_row(ci, &display_label, breadcrumb, zebra_bg, badge),
        )
    }
}

/// Trailing hotkey hint for a picker row, in the burger menu's style
/// (muted, small, hugging the trailing edge). `None` renders nothing, so
/// an unbound action just shows no hint.
fn row_shortcut_hint<'a>(shortcut: Option<String>) -> Vec<Element<'a, Message>> {
    match shortcut {
        Some(s) => vec![
            Space::new().width(Length::Fill).into(),
            text(s).size(11).color(OryxisColors::t().text_muted).into(),
        ],
        // No filler either: without a hint the row keeps its natural
        // width, exactly as it did before hints existed.
        None => Vec::new(),
    }
}

/// "Local Shell" entry, emitting `PickLocalShell` (fills the pending split
/// pane, or opens a local shell in a new tab).
fn local_shell_row<'a>(shortcut: Option<String>) -> Element<'a, Message> {
    let mut items: Vec<Element<'a, Message>> = vec![
        iced_fonts::lucide::terminal()
            .size(15)
            .color(OryxisColors::t().accent)
            .into(),
        Space::new().width(12).into(),
        text(t("local_shell"))
            .size(13)
            .font(iced::Font {
                weight: iced::font::Weight::Semibold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            })
            .color(OryxisColors::t().text_primary)
            .into(),
    ];
    items.extend(row_shortcut_hint(shortcut));
    let inner = dir_row(items).align_y(iced::Alignment::Center);
    button(
        container(inner)
            .padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 })
            .width(Length::Fill),
    )
    .on_press(Message::Tabs(TabsMessage::PickLocalShell))
    .width(Length::Fill)
    .style(hover_row_style)
    .into()
}

/// "SFTP" entry, emitting `NewSftpTab` (opens a fresh SFTP browser tab).
/// Shown right under Local Shell when SFTP is enabled.
fn sftp_row<'a>(shortcut: Option<String>) -> Element<'a, Message> {
    let mut items: Vec<Element<'a, Message>> = vec![
        iced_fonts::lucide::folder_tree()
            .size(15)
            .color(OryxisColors::t().accent)
            .into(),
        Space::new().width(12).into(),
        text(t("sftp"))
            .size(13)
            .font(iced::Font {
                weight: iced::font::Weight::Semibold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            })
            .color(OryxisColors::t().text_primary)
            .into(),
    ];
    items.extend(row_shortcut_hint(shortcut));
    let inner = dir_row(items).align_y(iced::Alignment::Center);
    button(
        container(inner)
            .padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 })
            .width(Length::Fill),
    )
    .on_press(Message::Sftp(SftpMessage::NewSftpTab))
    .width(Length::Fill)
    .style(hover_row_style)
    .into()
}

/// Bold section label ("Groups", "Recent connections").
fn section_header<'a>(label: &str) -> Element<'a, Message> {
    dir_row(vec![
        text(label.to_string())
            .size(13)
            .font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            })
            .color(OryxisColors::t().text_primary)
            .into(),
        Space::new().width(Length::Fill).into(),
    ])
    .align_y(iced::Alignment::Center)
    .into()
}

/// Back-navigation header shown when drilled into a group. The leading
/// arrow + the group label form one click target returning to the top.
fn back_header<'a>(label: &str) -> Element<'a, Message> {
    let arrow: Element<'a, Message> = iced_fonts::lucide::arrow_left()
        .size(16)
        .color(OryxisColors::t().text_primary)
        .into();
    let inner = dir_row(vec![
        arrow,
        Space::new().width(10).into(),
        text(label.to_string())
            .size(14)
            .font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            })
            .color(OryxisColors::t().text_primary)
            .into(),
    ])
    .align_y(iced::Alignment::Center);
    button(
        container(inner)
            .padding(Padding { top: 6.0, right: 8.0, bottom: 6.0, left: 4.0 })
            .width(Length::Fill),
    )
    .on_press(Message::Tabs(TabsMessage::NewTabPickerBack))
    .width(Length::Fill)
    .style(hover_row_style)
    .into()
}

/// Muted, centered informational row (empty / loading / error states).
fn info_row<'a>(msg: &str) -> Element<'a, Message> {
    container(text(msg.to_string()).size(13).color(OryxisColors::t().text_muted))
        .padding(Padding { top: 18.0, right: 16.0, bottom: 18.0, left: 16.0 })
        .center_x(Length::Fill)
        .into()
}

/// Distinct top row offering an ad-hoc connect for a search input that
/// parses as `user@host[:port]`. Accent border + zap glyph so it reads
/// apart from saved hosts; the secondary line spells out that nothing is
/// saved to the vault.
fn quick_connect_row<'a>(conn: oryxis_core::models::Connection) -> Element<'a, Message> {
    // Names the protocol whenever it is not the default one, so a
    // Telnet or Serial target says so before Enter rather than after
    // the tab opens.
    let primary = match conn.protocol == oryxis_core::models::connection::ConnectionProtocol::Ssh {
        true => format!("{}: {}", t("quick_connect"), conn.label),
        false => format!("{} ({}): {}", t("quick_connect"), conn.protocol, conn.label),
    };
    button(
        dir_row(vec![
            iced_fonts::lucide::zap()
                .size(16)
                .color(OryxisColors::t().accent)
                .into(),
            Space::new().width(10).into(),
            iced::widget::Column::with_children(vec![
                text(primary)
                    .size(13)
                    .color(OryxisColors::t().text_primary)
                    .wrapping(iced::widget::text::Wrapping::None)
                    .into(),
                Space::new().height(2).into(),
                text(t("quick_connect_not_saved"))
                    .size(10)
                    .color(OryxisColors::t().text_muted)
                    .wrapping(iced::widget::text::Wrapping::None)
                    .into(),
            ])
            .width(Length::Fill)
            .align_x(dir_align_x())
            .clip(true)
            .into(),
        ])
        .align_y(iced::Alignment::Center),
    )
    .on_press(Message::Ssh(SshMessage::QuickConnect(Box::new(
        crate::state::QuickConnectEntry::bare(conn),
    ))))
    .padding(Padding { top: 10.0, right: 12.0, bottom: 10.0, left: 12.0 })
    .width(Length::Fill)
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
                color: OryxisColors::t().accent,
                width: 1.0,
            },
            ..Default::default()
        }
    })
    .into()
}


/// Shared button style: transparent until hover, used by group / back /
/// retry rows that don't carry their own zebra stripe.
fn hover_row_style(_: &iced::Theme, status: BtnStatus) -> button::Style {
    let bg = match status {
        BtnStatus::Hovered => OryxisColors::t().bg_hover,
        _ => Color::TRANSPARENT,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        border: Border { radius: Radius::from(6.0), ..Default::default() },
        ..Default::default()
    }
}

fn picker_row<'a>(
    conn_idx: usize,
    // Plain `&str` (not `&'a str`): the caller passes a per-frame
    // redacted label under Privacy Mode (issue #78).
    label: &str,
    breadcrumb: String,
    zebra_bg: Color,
    badge: Element<'a, Message>,
) -> Element<'a, Message> {
    let label_text = text(label.to_string()).size(13).font(iced::Font {
        weight: iced::font::Weight::Semibold,
        ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
    }).color(OryxisColors::t().text_primary);

    let breadcrumb_text = text(breadcrumb).size(12).color(OryxisColors::t().accent);

    let inner = dir_row(vec![
        badge,
        Space::new().width(12).into(),
        label_text.into(),
        Space::new().width(Length::Fill).into(),
        breadcrumb_text.into(),
    ])
    .align_y(iced::Alignment::Center);

    button(
        container(inner)
            .padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 })
            .width(Length::Fill),
    )
    .on_press(Message::Ssh(SshMessage::ConnectSsh(conn_idx)))
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
