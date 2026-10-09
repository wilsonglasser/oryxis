//! Host editor: universal Host-card fields (label, parent group, tags,
//! connection target, protocol / cloud-transport pickers, numeric port).
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
                .style(crate::widgets::rounded_input_style).align_x(dir_align_x()).boxed(),
        );
        label_field
    }

    pub(super) fn hp_parent_combo(&self) -> Element<'_, Message> {
        // Parent Group is the same component as the manual group editor's
        // and the session-group panel's: a plain text_input (typing a
        // label works, a breadcrumb path too, empty = top level) with a
        // chevron that opens the shared group picker popover, which
        // carries a "Top level (no group)" row for this target. The
        // typed / picked value flows through `EditorGroupChanged` into
        // `editor_form.group_name`, so the save path (find-or-create by
        // path) is unchanged.
        //
        // It used to be the native combo_box, and that could not take a
        // host OUT of its folder: the fork's combo clears its own input
        // on focus WITHOUT publishing `on_input`, so a field that looked
        // empty still held the folder, and closing the drawer wrote the
        // old folder straight back. A text_input never shows a value it
        // does not hold.
        const PARENT_COMBO_HEIGHT: f32 = 36.0;
        let has_group = !self.editor_form.group_name.trim().is_empty();
        let rtl = crate::i18n::is_rtl_layout();
        // Trailing room for the clear button so it never covers typed
        // text; constant whether or not the button is live, so toggling
        // it does not reflow the field.
        let trailing = 32.0;
        let (pad_left, pad_right) = if rtl { (trailing, 10.0) } else { (10.0, trailing) };
        let parent_input: Element<'_, Message> = text_input(
            t("group_placeholder"),
            &self.editor_form.group_name,
        )
        .id(iced::widget::Id::new("editor-parent-group"))
        .on_input(|v| Message::Editor(EditorMessage::EditorGroupChanged(v)))
        .on_submit_maybe(self.hp_submit())
        .padding(Padding { top: 10.0, right: pad_right, bottom: 10.0, left: pad_left })
        .width(Length::Fill)
        .style(crate::widgets::rounded_input_style)
        .align_x(dir_align_x())
        .boxed();

        // The clear (×) empties the field in one click: the explicit
        // "remove from group" the user asked for, since a cleared field
        // IS top level on save. Always built (shape-stable, see
        // `select_ring_opt`): with nothing to clear it is inert and
        // invisible, and it is only a keyboard row while it is live.
        let clear_msg = Message::Editor(EditorMessage::EditorGroupChanged(String::new()));
        let clear_icon = iced_fonts::lucide::x::<iced::Theme>()
            .size(14)
            .color(if has_group { OryxisColors::t().text_muted } else { Color::TRANSPARENT });
        let clear_btn = button(clear_icon)
            .on_press_maybe(has_group.then(|| clear_msg.clone()))
            .padding(4)
            .style(move |_, status| {
                let bg = match (has_group, status) {
                    (true, BtnStatus::Hovered) => Color::from_rgba(1.0, 1.0, 1.0, 0.08),
                    (true, BtnStatus::Pressed) => Color::from_rgba(1.0, 1.0, 1.0, 0.12),
                    _ => Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                }
            });
        let clear_el: Element<'_, Message> = if has_group {
            self.panel_nav_slot(
                crate::keynav::RowAction::activate(clear_msg),
                6.0,
                crate::views::terminal::icon_tooltip(clear_btn.boxed(), t("editor_clear_group")),
            )
        } else {
            // Same wrapper as the ringed branch, with no tooltip to
            // hover: the tooltip is a widget of its own, so it is kept
            // out of the tree on BOTH sides and the ring wrapper alone
            // keeps the shape identical.
            crate::widgets::select_ring_opt(clear_btn.boxed(), 6.0, None)
        };
        let (clear_align, clear_pad) = if rtl {
            (iced::alignment::Horizontal::Left, Padding { left: 2.0, ..Padding::ZERO })
        } else {
            (iced::alignment::Horizontal::Right, Padding { right: 2.0, ..Padding::ZERO })
        };
        let clear_overlay = container::<_, iced::Theme>(clear_el)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(clear_align)
            .align_y(iced::alignment::Vertical::Center)
            .padding(clear_pad);
        let input_with_clear: Element<'_, Message> = self.panel_nav_slot(
            crate::keynav::RowAction::input(iced::widget::Id::new("editor-parent-group")),
            10.0,
            iced::widget::Stack::<iced::Element<'_, _>>::new()
                .push(parent_input)
                .push(clear_overlay.boxed())
                .width(Length::Fill)
                .boxed(),
        );

        // The chevron is its own keyboard row: Enter opens the picker,
        // same as a click.
        let picker_toggle = Message::Navigation(NavigationMessage::ToggleGroupPicker(
            crate::state::GroupPickerTarget::HostEditorParent,
        ));
        let parent_chevron = self.panel_nav_slot(
            crate::keynav::RowAction::activate(picker_toggle.clone()),
            8.0,
            button(
                container(
                    iced_fonts::lucide::chevron_down::<iced::Theme>()
                        .size(12)
                        .color(OryxisColors::t().text_muted),
                )
                .center_x(Length::Fixed(32.0))
                .center_y(Length::Fixed(PARENT_COMBO_HEIGHT)),
            )
            .on_press(picker_toggle)
            .padding(0)
            .style(|_, status| {
                let bg = match status {
                    BtnStatus::Hovered => OryxisColors::t().bg_hover,
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
            .boxed(),
        );
        crate::widgets::bounds_reporter(
            dir_row(vec![
                container(input_with_clear)
                    .width(Length::Fill)
                    .height(Length::Fixed(PARENT_COMBO_HEIGHT))
                    .boxed(),
                Space::new().width(6).boxed(),
                container(parent_chevron)
                    .height(Length::Fixed(PARENT_COMBO_HEIGHT))
                    .boxed(),
            ])
            .align_y(iced::Alignment::Center)
            .boxed(),
            self.host_editor_parent_combo_bounds.clone(),
        )
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
                .boxed(),
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
                .boxed(),
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
                .boxed()
        };

        // Hostname row (Connection).
        let hostname_row: Element<'_, Message> = dir_row(vec![
            icon_element,
            Space::new().width(10).boxed(),
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
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x()).boxed(),
            ),
        ]).align_y(iced::Alignment::Center).boxed();
        hostname_row
    }

    pub(super) fn hp_protocol_row(&self) -> Option<Element<'_, Message>> {
        use oryxis_core::models::connection::ConnectionProtocol as Proto;
        // Protocol picker (Connection). A cloud-imported host has its own
        // transport picker below and is always SSH-family, so the two are
        // mutually exclusive: hide the protocol picker on cloud hosts.
        //
        // Every protocol is in the ONE picker, remote desktop included.
        // It used to be a separate "Add remote desktop" entry in the add
        // menu, which meant a user looking for RDP opened this list,
        // failed to find it, and concluded the app had none.
        let protocol_row: Option<Element<'_, Message>> = if self.editor_form.cloud_transport
            .is_some()
        {
            None
        } else {
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
                    .boxed(),
            );
            Some(
                column![
                    text(t("protocol")).size(12).color(OryxisColors::t().text_muted),
                    Space::new().height(8),
                    picker,
                ].boxed(),
            )
        };
        protocol_row
    }

    pub(super) fn hp_cloud_transport_row(&self) -> Option<Element<'_, Message>> {
        // Cloud-managed transport picker (Connection), only when the
        // connection being edited carries a `cloud_ref` (i.e. it was
        // imported from a cloud provider). Lets the user flip between
        // SSH (default) and AWS Instance Connect / SSM transports.
        // Built here (before the SSH card widgets) so its keyboard row
        // records in visual order inside the Host card.
        let cloud_transport_row: Option<Element<'_, Message>> =
            self.editor_form.cloud_transport.map(|current| {
                use oryxis_core::models::cloud::TransportKind;
                let options = vec![
                    TransportKind::Ssh,
                    TransportKind::InstanceConnect,
                    TransportKind::Ssm,
                ];
                // Focusable select: Tab reaches it, Enter/Space open it,
                // the widget owns arrows/Esc while focused (fork support).
                let picker = self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new(
                        "editor-pick-cloud-transport",
                    )),
                    crate::widgets::INPUT_RADIUS,
                    pick_list(Some(current), options, |t| match t {
                        TransportKind::Ssh => "SSH".to_string(),
                        TransportKind::InstanceConnect => "EC2 Instance Connect".to_string(),
                        TransportKind::Ssm => "SSM Session".to_string(),
                        TransportKind::EcsExec => "ECS Exec".to_string(),
                        TransportKind::KubectlExec => "kubectl exec".to_string(),
                    })
                    .on_select(|v| Message::Editor(EditorMessage::EditorCloudTransportChanged(v)))
                    .id(iced::widget::Id::new("editor-pick-cloud-transport"))
                    .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                    .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                    .padding(10)
                    .style(crate::widgets::rounded_pick_list_style)
                    .boxed(),
                );
                column![
                    text(t("cloud_dynamic_form_transport")).size(12).color(OryxisColors::t().text_muted),
                    Space::new().height(8),
                    picker,
                ].boxed()
            });
        cloud_transport_row
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
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x()).boxed(),
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
                text(t("telnet_tls_insecure_desc")).size(11).color(OryxisColors::t().text_muted).boxed(),
            );
        }
        col.boxed()
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
                .boxed(),
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
                    text(t("mosh_enabled_desc")).size(11).color(OryxisColors::t().text_muted).boxed(),
                )
                .boxed();
        }

        col = col
            .push(Space::new().height(ROW_GAP).boxed())
            .push(panel_field(
                t("mosh_server_path"),
                self.hp_mosh_field(
                    "editor-mosh-server-path",
                    &self.editor_form.mosh_server_path,
                    "mosh_server_path_placeholder",
                    EditorMessage::EditorMoshServerPathChanged,
                ),
            ))
            .push(Space::new().height(ROW_GAP).boxed())
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
                text(t("mosh_port_range_desc")).size(11).color(OryxisColors::t().text_muted).boxed(),
            )
            .push(Space::new().height(ROW_GAP).boxed())
            .push(panel_field(
                t("mosh_command"),
                self.hp_mosh_field(
                    "editor-mosh-command",
                    &self.editor_form.mosh_command,
                    "mosh_command_placeholder",
                    EditorMessage::EditorMoshCommandChanged,
                ),
            ))
            .push(text(t("mosh_command_desc")).size(11).color(OryxisColors::t().text_muted).boxed());
        col.boxed()
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
                .boxed(),
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
                .boxed(),
        );
        let mut col = column![
            panel_field(t("local_terminal"), picker),
            Space::new().height(ROW_GAP),
            panel_field(t("local_cwd"), cwd_field),
        ];
        if entries.is_empty() {
            col = col.push(Space::new().height(ROW_GAP).boxed()).push(
                text(t("local_terminals_empty_hint"))
                    .size(11)
                    .color(OryxisColors::t().text_muted).boxed(),
            );
        }
        col.boxed()
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
    .boxed()
}
