//! Host editor: universal Host-card fields (label, parent group, tags,
//! connection target, protocol picker, numeric port).
use super::*;
use iced::widget::column;

impl Oryxis {
    pub(super) fn hp_label_field(&self) -> Element<'_, Message> {
        // ── Section: Host (label + parent group) ──
        // Built before the Connection widgets so their keyboard rows
        // record ahead of the hostname's (the assembly at the bottom
        // lays the Host card out first).
        let label_field: Element<'_, Message> = self.panel_nav_slot(
            crate::keynav::RowAction::input(iced::widget::Id::new("editor-label")),
            10.0,
            text_input(t("my_server_placeholder"), &self.editor_form.label)
                .id(iced::widget::Id::new("editor-label"))
                .on_input(|v| Message::Editor(EditorMessage::EditorLabelChanged(v))).on_submit_maybe(self.hp_submit()).padding(10)
                .style(crate::widgets::rounded_input_style).align_x(dir_align_x()).into(),
        );
        label_field
    }

    pub(super) fn hp_parent_combo(&self) -> Element<'_, Message> {
        // Parent Group is a native iced combo_box: a single field that
        // filters the existing (visible) groups as you type and lets you
        // pick one, while still accepting a brand new name. The typed /
        // picked value flows through `EditorGroupChanged` into
        // `editor_form.group_name`, so the save path (find-or-create by
        // label) is unchanged. The `selection` prop drives the unfocused
        // display (the combo clears its internal value after a pick).
        let parent_selection = (!self.editor_form.group_name.is_empty())
            .then_some(&self.editor_form.group_name);
        // Keyboard row: Left/Right cycle the existing group names (the
        // fork's combo_box has no id hook, so Enter cannot focus it;
        // free-text entry stays a mouse/typing affordance).
        let (group_prev, group_next) = crate::keynav::slots::cycle_pair(
            self.editor_parent_combo.options(),
            &self.editor_form.group_name,
            |v| Message::Editor(EditorMessage::EditorGroupChanged(v)),
        );
        let parent_combo: Element<'_, Message> = self.panel_nav_slot(
            crate::keynav::RowAction::picker(group_prev, group_next),
            10.0,
            iced::widget::combo_box(
                &self.editor_parent_combo,
                t("group_placeholder"),
                parent_selection,
                |v| Message::Editor(EditorMessage::EditorGroupChanged(v)),
            )
            .on_input(|v| Message::Editor(EditorMessage::EditorGroupChanged(v)))
            .padding(10)
            .input_style(crate::widgets::rounded_input_style)
            .menu_style(crate::widgets::combo_menu_style)
            .width(Length::Fill)
            .into(),
        );
        parent_combo
    }

    pub(super) fn hp_tags_field(&self) -> Element<'_, Message> {
        // Tags: comma-separated free text, parsed on save. Feeds the
        // snippet sidebar's filter-by-host-tags toggle.
        let tags_field: Element<'_, Message> = self.panel_nav_slot(
            crate::keynav::RowAction::input(iced::widget::Id::new("editor-tags")),
            10.0,
            text_input(t("tags_placeholder"), &self.editor_form.tags_text)
                .id(iced::widget::Id::new("editor-tags"))
                .on_input(|v| Message::Editor(EditorMessage::EditorTagsChanged(v)))
                .on_submit_maybe(self.hp_submit())
                .padding(10)
                .style(crate::widgets::rounded_input_style)
                .align_x(dir_align_x())
                .into(),
        );
        tags_field
    }

    pub(super) fn hp_hostname_row(&self, is_serial: bool) -> Element<'_, Message> {
        // ── Section: Address ──
        // Icon + color reflect the detected OS (once the silent probe has
        // run) or a user-picked override.
        let editing_conn = self.editor_form.editing_id.and_then(|id| {
            self.connections.iter().find(|c| c.id == id)
        });
        let (addr_glyph, addr_color) = crate::os_icon::resolve_for(
            editing_conn.and_then(|c| c.detected_os.as_deref()),
            editing_conn.and_then(|c| c.custom_icon.as_deref()),
            editing_conn.and_then(|c| c.custom_color.as_deref()),
            editing_conn.and_then(|c| c.username.as_deref()),
            OryxisColors::t().accent,
        );
        // Icon is a button when we're editing an existing host, clicking it
        // opens the icon/color picker so the user can override the OS mark.
        // For new (unsaved) hosts the id doesn't exist yet, so it's just a
        // static badge until the first save (and not a keyboard row).
        let icon_element: Element<'_, Message> = if let Some(id) = self.editor_form.editing_id {
            self.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::Tabs(TabsMessage::ShowIconPicker(id))),
                8.0,
                button(
                    container(addr_glyph.view(18.0, Color::WHITE))
                        .width(Length::Fixed(32.0))
                        .height(Length::Fixed(32.0))
                        .center_x(Length::Fixed(32.0))
                        .center_y(Length::Fixed(32.0)),
                )
                .on_press(Message::Tabs(TabsMessage::ShowIconPicker(id)))
                .padding(0)
                .style(move |_, status| {
                    let ring = match status {
                        BtnStatus::Hovered => Color::from_rgba(1.0, 1.0, 1.0, 0.25),
                        _ => Color::TRANSPARENT,
                    };
                    button::Style {
                        background: Some(Background::Color(addr_color)),
                        border: Border { radius: Radius::from(8.0), color: ring, width: 1.5 },
                        ..Default::default()
                    }
                })
                .into(),
            )
        } else {
            container(addr_glyph.view(18.0, Color::WHITE))
                .width(Length::Fixed(32.0))
                .height(Length::Fixed(32.0))
                .center_x(Length::Fixed(32.0))
                .center_y(Length::Fixed(32.0))
                .style(move |_| container::Style {
                    background: Some(Background::Color(addr_color)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                })
                .into()
        };

        // Hostname row (Connection).
        let hostname_row: Element<'_, Message> = dir_row(vec![
            icon_element,
            Space::new().width(10).into(),
            self.panel_nav_slot(
                crate::keynav::RowAction::input(iced::widget::Id::new("editor-hostname")),
                10.0,
                text_input(
                    if is_serial { t("serial_port_path_ph") } else { t("ip_or_hostname") },
                    &self.editor_form.hostname,
                )
                    .id(iced::widget::Id::new("editor-hostname"))
                    .on_input(|v| Message::Editor(EditorMessage::EditorHostnameChanged(v)))
                    .on_submit_maybe(self.hp_submit())
                    .padding(10)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x()).into(),
            ),
        ]).align_y(iced::Alignment::Center).into();
        hostname_row
    }

    pub(super) fn hp_protocol_row(&self) -> Option<Element<'_, Message>> {
        use oryxis_core::models::connection::ConnectionProtocol as Proto;
        // Protocol picker (Connection).
        //
        // Every protocol is in the ONE picker, remote desktop included.
        // It used to be a separate "Add remote desktop" entry in the add
        // menu, which meant a user looking for RDP opened this list,
        // failed to find it, and concluded the app had none.
        let mut options =
            vec![Proto::Ssh, Proto::Telnet, Proto::Raw, Proto::Serial, Proto::Local];
        // Remote desktop stays behind its opt-in feature flag, so it
        // is offered only where it can actually be used; a host that
        // already IS one keeps the option visible, or editing it
        // would silently rewrite its protocol on the next pick.
        if self.remote_desktop_enabled || self.editor_form.protocol == Proto::RemoteDesktop {
            options.push(Proto::RemoteDesktop);
        }
        let picker = self.panel_nav_slot(
            crate::keynav::RowAction::input(iced::widget::Id::new("editor-pick-protocol")),
            crate::widgets::INPUT_RADIUS,
            pick_list(Some(self.editor_form.protocol), options, |p| p.to_string())
                .on_select(|v| Message::Editor(EditorMessage::EditorProtocolChanged(v)))
                .id(iced::widget::Id::new("editor-pick-protocol"))
                .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                .width(120)
                .padding(10)
                .style(crate::widgets::rounded_pick_list_style)
                .into(),
        );
        Some(
            column![
                text(t("protocol")).size(12).color(OryxisColors::t().text_muted),
                Space::new().height(8),
                picker,
            ]
            .into(),
        )
    }

    pub(super) fn hp_port_input(&self, is_serial: bool) -> Element<'_, Message> {
        // ── Connection / Credentials / SSH fields ──
        // The host editor is being reorganised into a universal region
        // (General, Connection, Credentials, Terminal) and an SSH-only
        // region (Authentication, Network, Integration) so a future
        // protocol switch can hide the SSH block wholesale. Each widget
        // is extracted into a local here, then composed into sections in
        // the assembly at the bottom; nothing about the form state, save
        // path, or messages changes. Locals are built in the same order
        // the assembly lays them out so keyboard rows record in visual
        // order.

        // Numeric port, dropped inline into the SSH/Telnet card header
        // ("SSH ........ [22] port"). Serial and Local have no TCP port,
        // so it is gated off (empty) and their headers omit it.
        let port_input: Element<'_, Message> = if is_serial {
            empty()
        } else {
            self.panel_nav_slot(
                crate::keynav::RowAction::input(iced::widget::Id::new("editor-port")),
                10.0,
                text_input("22", &self.editor_form.port)
                    .id(iced::widget::Id::new("editor-port"))
                    .on_input(|v| Message::Editor(EditorMessage::EditorPortChanged(v)))
                    .on_submit_maybe(self.hp_submit())
                    .padding(6)
                    .width(56)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x()).into(),
            )
        };
        port_input
    }

    /// Telnet-over-TLS rows (`telnets`, conventionally port 992): the
    /// toggle, and, only while it is on, the per-host escape for a
    /// certificate the trust store rejects.
    ///
    /// The escape is nested under the toggle rather than sitting beside
    /// it because it is meaningless without TLS, and a visible "accept
    /// invalid certificate" on a plain-Telnet host reads as a setting
    /// that is protecting something.
    pub(super) fn hp_telnet_tls_block(&self) -> Element<'_, Message> {
        let tls_on = self.editor_form.telnet_tls;
        let tls_row = self.panel_nav_slot(
            crate::keynav::RowAction::activate(Message::Editor(
                EditorMessage::EditorToggleTelnetTls,
            )),
            8.0,
            panel_option_row(
                iced_fonts::lucide::lock(),
                t("telnet_tls"),
                hp_toggle_button(tls_on, Message::Editor(EditorMessage::EditorToggleTelnetTls)),
            ),
        );
        let mut col = column![tls_row];
        if tls_on {
            let insecure_row = self.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::Editor(
                    EditorMessage::EditorToggleTelnetTlsInsecure,
                )),
                8.0,
                panel_option_row(
                    iced_fonts::lucide::shield_alert(),
                    t("telnet_tls_insecure"),
                    hp_toggle_button(
                        self.editor_form.telnet_tls_insecure,
                        Message::Editor(EditorMessage::EditorToggleTelnetTlsInsecure),
                    ),
                ),
            );
            col = col.push(insecure_row).push(
                text(t("telnet_tls_insecure_desc")).size(11).color(OryxisColors::t().text_muted),
            );
        }
        col.into()
    }

    /// One of the mosh text rows, recorded on the panel ring so the
    /// keyboard reaches it like every other field here.
    fn hp_mosh_field<'a>(
        &'a self,
        id: &'static str,
        value: &'a str,
        placeholder: &'static str,
        make: fn(String) -> EditorMessage,
    ) -> Element<'a, Message> {
        self.panel_nav_slot(
            crate::keynav::RowAction::input(iced::widget::Id::new(id)),
            10.0,
            text_input(t(placeholder), value)
                .id(iced::widget::Id::new(id))
                .on_input(move |v| Message::Editor(make(v)))
                .on_submit_maybe(self.hp_submit())
                .padding(10)
                .style(crate::widgets::rounded_input_style)
                .align_x(dir_align_x())
                .into(),
        )
    }

    /// The mosh rows: one toggle, and three settings that only mean
    /// anything while it is on.
    ///
    /// On the SSH form rather than under a protocol of its own, because
    /// mosh is carried over SSH and cannot exist without it: the server
    /// is started by an SSH session and answers over the same channel,
    /// so a mosh host needs the username, the key, the jump chain and
    /// the proxy this form already collects. Same shape as
    /// Telnet-over-TLS being a toggle on the Telnet form.
    ///
    /// The three are nested under the toggle for the reason the TLS
    /// escape is: a server path on a host that does not use mosh reads
    /// as a setting that is doing something.
    pub(super) fn hp_mosh_block(&self) -> Element<'_, Message> {
        let on = self.editor_form.mosh_enabled;
        let toggle_row = self.panel_nav_slot(
            crate::keynav::RowAction::activate(Message::Editor(EditorMessage::EditorToggleMosh)),
            8.0,
            panel_option_row(
                iced_fonts::lucide::radio(),
                t("mosh_enabled"),
                hp_toggle_button(on, Message::Editor(EditorMessage::EditorToggleMosh)),
            ),
        );
        let mut col = column![toggle_row];
        if !on {
            return col
                .push(
                    text(t("mosh_enabled_desc")).size(11).color(OryxisColors::t().text_muted),
                )
                .into();
        }

        col = col
            .push(Space::new().height(ROW_GAP))
            .push(panel_field(
                t("mosh_server_path"),
                self.hp_mosh_field(
                    "editor-mosh-server-path",
                    &self.editor_form.mosh_server_path,
                    "mosh_server_path_placeholder",
                    EditorMessage::EditorMoshServerPathChanged,
                ),
            ))
            .push(Space::new().height(ROW_GAP))
            .push(panel_field(
                t("mosh_port_range"),
                self.hp_mosh_field(
                    "editor-mosh-port-range",
                    &self.editor_form.mosh_port_range,
                    "mosh_port_range_placeholder",
                    EditorMessage::EditorMoshPortRangeChanged,
                ),
            ))
            .push(
                text(t("mosh_port_range_desc")).size(11).color(OryxisColors::t().text_muted),
            )
            .push(Space::new().height(ROW_GAP))
            .push(panel_field(
                t("mosh_command"),
                self.hp_mosh_field(
                    "editor-mosh-command",
                    &self.editor_form.mosh_command,
                    "mosh_command_placeholder",
                    EditorMessage::EditorMoshCommandChanged,
                ),
            ))
            .push(text(t("mosh_command_desc")).size(11).color(OryxisColors::t().text_muted));
        col.into()
    }

    /// Local-host rows: which curated terminal to spawn, and the folder
    /// it starts in.
    ///
    /// The terminal is a REFERENCE into the Settings > Terminal list,
    /// never a program path typed here: that list is where local shells
    /// are curated, and a second copy of "which PowerShell" would drift
    /// from it. When the list is empty the picker says so and points at
    /// the place that fills it, rather than offering nothing.
    pub(super) fn hp_local_block(&self) -> Element<'_, Message> {
        let entries = self.local_terminals.as_deref().unwrap_or(&[]);
        // The default-shell row is a real option, not an empty
        // selection: "whatever this machine's shell is" is a choice a
        // local host can legitimately make.
        let mut labels: Vec<String> = vec![t("local_default_shell").to_string()];
        labels.extend(entries.iter().map(|e| e.label.clone()));
        let selected = self
            .editor_form
            .local_terminal_id
            .and_then(|id| entries.iter().find(|e| e.id == id))
            .map(|e| e.label.clone())
            .unwrap_or_else(|| t("local_default_shell").to_string());
        // Map back by label: the picker hands us a String, and the ids
        // live beside them in the same list.
        let ids: Vec<Option<uuid::Uuid>> =
            std::iter::once(None).chain(entries.iter().map(|e| Some(e.id))).collect();
        let by_label: std::collections::HashMap<String, Option<uuid::Uuid>> =
            labels.iter().cloned().zip(ids.iter().copied()).collect();
        let (prev, next) = crate::keynav::slots::cycle_pair(&labels, &selected, {
            let by_label = by_label.clone();
            move |v| {
                Message::Editor(EditorMessage::EditorLocalTerminalChanged(
                    by_label.get(&v).copied().flatten(),
                ))
            }
        });
        let picker = self.panel_nav_slot(
            crate::keynav::RowAction::picker(prev, next),
            crate::widgets::INPUT_RADIUS,
            pick_list(Some(selected), labels, |l: &String| l.clone())
                .on_select(move |v: String| {
                    Message::Editor(EditorMessage::EditorLocalTerminalChanged(
                        by_label.get(&v).copied().flatten(),
                    ))
                })
                .padding(10)
                .style(crate::widgets::rounded_pick_list_style)
                .into(),
        );
        let cwd_field = self.panel_nav_slot(
            crate::keynav::RowAction::input(iced::widget::Id::new("editor-local-cwd")),
            10.0,
            text_input(t("local_cwd_placeholder"), &self.editor_form.local_cwd)
                .id(iced::widget::Id::new("editor-local-cwd"))
                .on_input(|v| Message::Editor(EditorMessage::EditorLocalCwdChanged(v)))
                .on_submit_maybe(self.hp_submit())
                .padding(10)
                .style(crate::widgets::rounded_input_style)
                .align_x(dir_align_x())
                .into(),
        );
        let mut col = column![
            panel_field(t("local_terminal"), picker),
            Space::new().height(ROW_GAP),
            panel_field(t("local_cwd"), cwd_field),
        ];
        if entries.is_empty() {
            col = col.push(Space::new().height(ROW_GAP)).push(
                text(t("local_terminals_empty_hint"))
                    .size(11)
                    .color(OryxisColors::t().text_muted),
            );
        }
        col.into()
    }
}

/// The editor's on/off pill (same shape as the SSH toggles): the
/// background carries the state, the label says which one it is, and
/// hover / press swap it for the accent so the control answers the
/// pointer like every other button in the app.
pub(super) fn hp_toggle_button<'a>(on: bool, msg: Message) -> Element<'a, Message> {
    let bg = if on { OryxisColors::t().success } else { OryxisColors::t().bg_hover };
    let fg = crate::theme::contrast_text_for(bg);
    button(
        text(if on { crate::i18n::t("toggle_on") } else { crate::i18n::t("toggle_off") })
            .size(12)
            .color(fg),
    )
    .on_press(msg)
    .style(move |_theme, status| button::Style {
        background: Some(Background::Color(match status {
            button::Status::Hovered | button::Status::Pressed => OryxisColors::t().accent,
            _ => bg,
        })),
        border: Border { radius: Radius::from(4.0), ..Default::default() },
        text_color: fg,
        ..Default::default()
    })
    .into()
}
