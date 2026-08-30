//! Host editor / connection editor side panel.

use iced::border::Radius;
use iced::widget::{button, column, container, pick_list, scrollable, text, text_editor, text_input, Space};
use iced::widget::button::Status as BtnStatus;
use iced::{Background, Border, Color, Element, Length, Padding};

use oryxis_core::models::connection::AuthMethod;
use oryxis_core::models::identity::Identity;

use crate::app::{TabsMessage, EditorMessage, KeysMessage, NavigationMessage, Message, Oryxis};
use crate::i18n::t;
use crate::state::ProxyKind;
use crate::theme::OryxisColors;
use crate::widgets::{
    dir_align_x, dir_row, panel_divider, panel_field, panel_option_row,
    panel_section,
};

const GROUP_GAP: f32 = 16.0;
const ROW_GAP: f32 = 10.0;

mod auth;
mod basics;
mod credentials;
mod footer;
mod integration;
mod inherited;
mod login_script;
mod network;
mod sections;
mod terminal_settings;

/// Empty placeholder element for gated-off (hidden) rows: the reduced
/// Telnet / Serial / RemoteDesktop forms drop the SSH-only widgets, and
/// because `panel_nav_slot` records at build time an ungated build would
/// record invisible Tab targets. Each gated builder resolves to this.
fn empty<'a>() -> Element<'a, Message> {
    Space::new().into()
}

impl Oryxis {
    /// The `on_submit` payload every host-editor text input carries
    /// this frame. `None` while the panel ring sits on a non-input row:
    /// no input is focused then, so Enter belongs to the ringed row,
    /// and the fork's `text_input` would otherwise fire the binding
    /// WITHOUT focus (its on_submit shortcut sits in front of the
    /// `is_focused` gate) and capture the key away from the panel
    /// router, turning "Enter opens the ringed section" into a phantom
    /// save. Known residue: a mouse click into an input doesn't clear
    /// the ring (focus is unobservable), so an Enter right after such
    /// a click activates the still-ringed row instead of saving; the
    /// next Tab re-syncs, and nothing is lost either way.
    pub(super) fn hp_submit(&self) -> Option<Message> {
        (!self.panel_ring_on_noninput())
            .then(|| Message::Editor(EditorMessage::EditorSave))
    }

    pub(crate) fn view_host_panel(&self) -> Element<'_, Message> {
        // Keyboard rows are recorded in visual order (row mode: Up/Down from any input).
        self.panel_nav_reset();
        let is_editing = self.editor_form.editing_id.is_some();
        let title = if is_editing { crate::i18n::t("edit_host") } else { crate::i18n::t("new_host") };
        let mut has_address = !self.editor_form.hostname.is_empty();
        // Telnet hosts hide the whole SSH block (keys, identities,
        // agent-fwd, jump chain, proxy, port-forwards, TOTP, MCP,
        // algorithms, initial command); the reduced form keeps only
        // label/parent/tags, host/port, username/password, encoding and
        // the terminal theme. `is_ssh` gates every SSH-only piece below.
        use oryxis_core::models::connection::ConnectionProtocol as Proto;
        let is_ssh = self.editor_form.protocol == Proto::Ssh;
        // Serial is even more reduced than Telnet: no auth (no
        // username/password), no numeric port, and its own line-param
        // block instead. `is_serial` additionally gates the shared
        // credentials + numeric-port widgets off.
        let is_serial = self.editor_form.protocol == Proto::Serial;
        // Remote desktop: the endpoint (host/port), a login (username /
        // password), a kind (RDP/VNC) and an optional SSH gateway. All the
        // SSH-only rows below are `is_ssh`-gated, so they drop for free.
        let is_rd = self.editor_form.protocol == Proto::RemoteDesktop;
        // Raw is Telnet minus the credentials: a bare socket has nobody
        // to authenticate to, so the device prompts in band if it wants
        // anything.
        let is_raw = self.editor_form.protocol == Proto::Raw;
        // Local reaches no endpoint at all: no address, no port, no
        // credentials. What it needs instead is which curated terminal
        // to spawn and where to start it.
        let is_local = self.editor_form.protocol == Proto::Local;
        // Whether the host has a network address to type. Drives both
        // the address row and the Connect button's enable gate: a local
        // host is connectable with no address at all.
        let takes_address = !is_local;
        // Telnet needs no flag of its own: it is the trailing `else` of
        // the protocol-card branch below (it dials TCP too, so that
        // branch builds the IP-version row inline).
        // ── Header ──
        // The close (×) is intentionally not a keyboard row: Esc already
        // owns panel close, and recording it would make the header the
        // first Down target instead of the form.
        let panel_header = container(
            dir_row(vec![
                text(title).size(16).color(OryxisColors::t().text_primary).into(),
                Space::new().width(Length::Fill).into(),
                button(text("\u{00D7}").size(20).color(OryxisColors::t().text_muted))
                    .on_press(Message::Editor(EditorMessage::EditorCancel))
                    .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(Color::TRANSPARENT)),
                        border: Border::default(),
                        ..Default::default()
                    }).into(),
            ]).align_y(iced::Alignment::Center),
        )
        // top 12 (not 16): the taller ×-button row centres the title, so a
        // 16 top padding optically reads ~4px lower than the 16 left. 12
        // lands the title's top edge level with the left gutter.
        .padding(Padding { top: 12.0, right: 16.0, bottom: 12.0, left: 16.0 });

        // Create-flow starting points (P3): fixed under the header,
        // above the scroll, so they build (= keyboard-record) before
        // the form fields. New-host flow only: an existing host IS its
        // shape already, and the quick flow edits a live dial.
        let preset_row: Option<Element<'_, Message>> =
            (!is_editing && !self.editor_form.quick_flow).then(|| self.hp_preset_row());

        // Two-tier form: the essential fields build (= keynav-record)
        // first, then each collapsible section records its header and,
        // only while open, its body (`hp_section` runs the body closure
        // after the header). Build order stays render order throughout,
        // which is the panel keyboard contract.
        // A local host names no endpoint, so the Connect button gates on
        // the label instead: that IS its whole target.
        if is_local {
            has_address = !self.editor_form.label.trim().is_empty();
        }
        let label_field = self.hp_label_field();
        let parent_combo = self.hp_parent_combo();
        let tags_field = self.hp_tags_field();
        let hostname_row = takes_address.then(|| self.hp_hostname_row(is_serial));
        let protocol_row = self.hp_protocol_row();
        let port_input = self.hp_port_input(!self.editor_form.protocol.uses_network_port());

        // ── Compose one card per semantic group ──
        // Host (label / parent / connection target), the essential
        // protocol card (port + login), then the collapsible sections
        // (`hp_section`) and the bottom actions, in that build order,
        // because build order is keyboard-record order.
        //
        // Spacing: GROUP_GAP (Space + divider + Space) between subgroups,
        // ROW_GAP between rows. No per-row dividers, so nothing hugs a
        // field.
        let group_sep = || -> Element<'_, Message> {
            column![
                Space::new().height(GROUP_GAP),
                panel_divider(),
                Space::new().height(GROUP_GAP),
            ].into()
        };

        // Host card: label, parent group, then the connection target.
        let mut host_col = column![
            section_header(t("host")),
            Space::new().height(ROW_GAP),
            panel_field(t("label"), label_field),
            Space::new().height(ROW_GAP),
            panel_field(t("parent_group"), parent_combo),
            Space::new().height(ROW_GAP),
            panel_field(t("tags"), tags_field),
        ];
        host_col = host_col
            .push(group_sep())
            .push(section_header(t("connection")));
        if let Some(hr) = hostname_row {
            host_col = host_col.push(Space::new().height(ROW_GAP)).push(hr);
        }
        if let Some(pr) = protocol_row {
            host_col = host_col.push(Space::new().height(ROW_GAP)).push(pr);
        }
        let host_section = panel_section(host_col);

        // Protocol card header: "<PROTO> .......... [port] port". The
        // accent label names the active protocol so the card reads the
        // same whether it holds the full SSH block or the reduced
        // Telnet one.
        let proto_label = if is_ssh {
            t("ssh")
        } else if is_serial {
            t("serial")
        } else if is_local {
            t("local")
        } else if is_raw {
            t("raw")
        } else if is_rd {
            t("remote_desktop")
        } else {
            t("telnet")
        };
        // Serial and Local have no numeric port, so their header is just
        // the label; everything else appends the "[22] port" field.
        let proto_header = if !self.editor_form.protocol.uses_network_port() {
            dir_row(vec![
                text(proto_label).size(14).color(OryxisColors::t().accent).into(),
                Space::new().width(Length::Fill).into(),
            ])
            .align_y(iced::Alignment::Center)
        } else {
            dir_row(vec![
                text(proto_label).size(14).color(OryxisColors::t().accent).into(),
                Space::new().width(Length::Fill).into(),
                port_input,
                Space::new().width(8).into(),
                text(t("port")).size(12).color(OryxisColors::t().text_muted).into(),
            ])
            .align_y(iced::Alignment::Center)
        };

        // Essential protocol card: the port header plus the login.
        // Everything else protocol-specific lives in the collapsible
        // sections below; the reduced Serial / RD / Telnet forms keep
        // their few extra rows inline, they already ARE the disclosure.
        // Serial, Raw and Local have no login of their own, so the
        // credential rows are not built at all (building them would
        // record dead keyboard stops for fields nothing reads).
        let cred_items = self.hp_cred_items(is_serial || is_raw || is_local, is_ssh);
        let protocol_section: Element<'_, Message> = if is_ssh {
            // mosh sits under the credentials rather than in a card of
            // its own: it is how this SSH session is CARRIED, not a
            // different kind of host, and every field above is a field
            // it needs.
            let mosh_block = self.hp_mosh_block();
            panel_section(
                column![proto_header]
                    .push(group_sep())
                    .push(section_header(t("credentials")))
                    .push(Space::new().height(ROW_GAP))
                    .push(cred_items)
                    .push(group_sep())
                    .push(section_header(t("mosh_section")))
                    .push(Space::new().height(ROW_GAP))
                    .push(mosh_block),
            )
        } else if is_serial {
            // Serial card: the line-parameter block under the header.
            // No credentials (serial has no auth); the port path lives
            // in the Host card's connection target above.
            let serial_params_block = self.hp_serial_params_block(true);
            panel_section(
                column![proto_header]
                    .push(group_sep())
                    .push(section_header(t("serial_line")))
                    .push(Space::new().height(ROW_GAP))
                    .push(serial_params_block),
            )
        } else if is_rd {
            // Remote-desktop card: the endpoint login (Credentials) plus the
            // kind + SSH gateway rows. No SSH auth/network/integration.
            let rd_block = self.hp_rd_block(true);
            panel_section(
                column![proto_header]
                    .push(group_sep())
                    .push(section_header(t("credentials")))
                    .push(Space::new().height(ROW_GAP))
                    .push(cred_items)
                    .push(group_sep())
                    .push(section_header(t("remote_desktop")))
                    .push(Space::new().height(ROW_GAP))
                    .push(rd_block),
            )
        } else if is_local {
            // Local card: which curated terminal to spawn and where it
            // starts. No port, no login, no network rows.
            let local_block = self.hp_local_block();
            panel_section(
                column![proto_header]
                    .push(group_sep())
                    .push(section_header(t("local_shell")))
                    .push(Space::new().height(ROW_GAP))
                    .push(local_block),
            )
        } else {
            // Telnet and Raw keep their two network rows inline (built
            // here, in render order, so they record after the credential
            // rows).
            let tls_block = (!is_raw).then(|| self.hp_telnet_tls_block());
            let row_address_family = self.hp_row_address_family(false, true);
            let row_mac_address = self.hp_row_mac_address(true);
            // Cleartext note: honest UX, not a lecture. The user is the
            // only party on the path without a secure option. It drops
            // once TLS is on, because then the statement is false.
            let tls_on = self.editor_form.telnet_tls && !is_raw;
            let cleartext_note: Option<Element<'_, Message>> = (!tls_on).then(|| {
                dir_row(vec![
                    iced_fonts::lucide::triangle_alert()
                        .size(13)
                        .color(OryxisColors::t().warning)
                        .into(),
                    Space::new().width(8).into(),
                    text(if is_raw { t("raw_cleartext_note") } else { t("telnet_cleartext_note") })
                        .size(11)
                        .color(OryxisColors::t().text_muted)
                        .into(),
                ])
                .align_y(iced::Alignment::Center)
                .into()
            });
            let mut telnet_col = column![proto_header];
            // Raw has no credentials block at all, so it also skips the
            // header that would announce an empty group.
            if !is_raw {
                telnet_col = telnet_col
                    .push(group_sep())
                    .push(section_header(t("credentials")))
                    .push(Space::new().height(ROW_GAP))
                    .push(cred_items);
            }
            if let Some(tls) = tls_block {
                telnet_col = telnet_col.push(Space::new().height(ROW_GAP)).push(tls);
            }
            telnet_col = telnet_col
                .push(Space::new().height(ROW_GAP))
                .push(row_address_family)
                .push(Space::new().height(ROW_GAP))
                .push(row_mac_address);
            if let Some(note) = cleartext_note {
                telnet_col = telnet_col.push(Space::new().height(GROUP_GAP)).push(note);
            }
            panel_section(telnet_col)
        };

        // ── Collapsible tier ──
        // Only the sections a protocol actually has are rendered: the
        // Authentication / Network / Integration machinery is SSH-only,
        // and an RDP/VNC host drives no terminal pane (no
        // Compatibility). Each `hp_section` body closure runs only
        // while its section is open, so a closed section builds (and
        // keyboard-records) nothing.
        use crate::state::HostEditorSection as S;
        let auth_section = is_ssh.then(|| {
            self.hp_section(S::Authentication, || {
                let row_auth_method = self.hp_row_auth_method(true);
                let ssh_key_row = self.hp_ssh_key_row(true);
                let disk_key_block = self.hp_disk_key_block(true);
                let row_agent_fwd = self.hp_row_agent_fwd(true);
                let row_x11_fwd = self.hp_row_x11_fwd(true);
                let totp_block = self.hp_totp_block(true);
                let mut col = column![row_auth_method];
                // The chosen method's field: Key / Certificate / Agent
                // show a key picker; the other methods need no extra
                // input here (password lives in Credentials).
                if let Some(k) = ssh_key_row {
                    col = col.push(Space::new().height(ROW_GAP)).push(k);
                }
                // Below the vault-key picker, which is the precedence
                // the resolver applies: a linked key wins, and the disk
                // only fills the gap it leaves.
                col.push(Space::new().height(ROW_GAP))
                    .push(disk_key_block)
                    .push(Space::new().height(ROW_GAP))
                    .push(row_agent_fwd)
                    .push(Space::new().height(ROW_GAP))
                    .push(row_x11_fwd)
                    .push(Space::new().height(ROW_GAP))
                    .push(totp_block)
                    .into()
            })
        });
        let network_section = is_ssh.then(|| {
            self.hp_section(S::Network, || {
                let row_chaining = self.hp_row_chaining(true);
                let proxy_rows: Element<'_, Message> = self.build_proxy_rows().into();
                let pf_items = self.hp_pf_items(true);
                let row_keepalive = self.hp_row_keepalive(true);
                let row_address_family = self.hp_row_address_family(true, false);
                let row_mac_address = self.hp_row_mac_address(true);
                let row_auto_title = self.hp_row_auto_title(true);
                column![row_chaining]
                    .push(Space::new().height(ROW_GAP))
                    .push(proxy_rows)
                    .push(Space::new().height(ROW_GAP))
                    .push(pf_items)
                    .push(Space::new().height(ROW_GAP))
                    .push(row_keepalive)
                    .push(Space::new().height(ROW_GAP))
                    .push(row_address_family)
                    .push(Space::new().height(ROW_GAP))
                    .push(row_mac_address)
                    .push(Space::new().height(ROW_GAP))
                    .push(row_auto_title)
                    .into()
            })
        });
        // Compatibility (P2): the four legacy-algorithm pickers (SSH
        // only) and the C5 legacy keyboard modes + feature toggles
        // (every terminal protocol; an RDP/VNC host drives no terminal
        // pane, so the whole section drops out for it).
        let compat_section = (!is_rd).then(|| {
            self.hp_section(S::Compatibility, || {
                let mut col = column![];
                if is_ssh {
                    col = col
                        .push(self.algo_overrides_section())
                        .push(group_sep());
                }
                col.push(self.hp_advanced_terminal_items()).into()
            })
        });
        // A local host has an Integration section too, reduced to the
        // two rows that mean something without a remote: the shell's
        // environment and the command it opens with (which is the whole
        // point of saving one).
        let local_integration_section = is_local.then(|| {
            self.hp_section(S::Integration, || {
                let env_items = self.hp_env_items(true);
                let startup_block = self.hp_startup_block(true);
                column![env_items]
                    .push(group_sep())
                    .push(startup_block)
                    .into()
            })
        });
        let integration_section = is_ssh.then(|| {
            self.hp_section(S::Integration, || {
                let row_mcp = self.hp_row_mcp(true);
                let row_monitor = self.hp_row_monitor(true);
                let row_monitor_disks = self.hp_row_monitor_disks(true);
                let row_sftp_initial_path = self.hp_row_sftp_initial_path(true);
                let row_zmodem_drops = self.hp_row_zmodem_drops(true);
                let env_items = self.hp_env_items(true);
                let startup_block = self.hp_startup_block(true);
                let login_script_block = self.hp_login_script_block(true);
                column![row_mcp, row_monitor, row_monitor_disks, row_sftp_initial_path, row_zmodem_drops]
                    .push(Space::new().height(ROW_GAP))
                    .push(env_items)
                    .push(group_sep())
                    .push(startup_block)
                    // Login automation sits right after the startup
                    // command on purpose: both are "what happens once
                    // the session opens", and the script has to finish
                    // before the startup command is sent (see
                    // `dispatch_ssh/session`).
                    .push(group_sep())
                    .push(login_script_block)
                    .into()
            })
        });
        // Terminal section: appearance + session logging, every
        // protocol (an RDP host still carries the recording / privacy
        // overrides its file transfers and future panes read).
        let terminal_section = self.hp_section(S::Terminal, || {
            let appearance_items = self.hp_appearance_items();
            let row_session_logging = self.hp_row_session_logging();
            let row_privacy_mode = self.hp_row_privacy_mode();
            let row_sidebar_auto_open = self.hp_row_sidebar_auto_open();
            column![appearance_items]
                .push(Space::new().height(GROUP_GAP))
                .push(row_session_logging)
                .push(Space::new().height(GROUP_GAP))
                .push(row_privacy_mode)
                .push(Space::new().height(GROUP_GAP))
                .push(row_sidebar_auto_open)
                .into()
        });

        // ── Error ──
        // The gap toward the actions row lives in this container's own
        // bottom padding (not a `.spacing` on the column below): the
        // no-error placeholder is a present Shrink Space (constant tree
        // shape, see main_layout's slot skeleton note), and a column
        // spacing would open a stray 8px band above the buttons when
        // there is no error to show.
        let panel_error: Element<'_, Message> = if let Some(err) = &self.host_panel_error {
            container(Element::from(text(err.clone()).size(11).color(OryxisColors::t().error)))
                .padding(Padding { top: 4.0, right: 16.0, bottom: 12.0, left: 16.0 })
                .into()
        } else {
            Space::new().into()
        };
        // Built last: the footer buttons render below every card, so
        // their keyboard rows must record after them.
        let actions_row = self.hp_actions_row(has_address);
        // The error must live OUTSIDE the scrollable so it sits above
        // the Save button at the bottom of the panel, otherwise long
        // forms hide it below the fold and the user clicks Save again
        // wondering why nothing happens.
        let bottom = column![panel_error, actions_row];

        // ── Layout ──
        let mut form_col = column![host_section, Space::new().height(10), protocol_section];
        for section in [
            auth_section,
            network_section,
            compat_section,
            integration_section,
            local_integration_section,
        ]
        .into_iter()
        .flatten()
        {
            form_col = form_col.push(Space::new().height(10)).push(section);
        }
        form_col = form_col.push(Space::new().height(10)).push(terminal_section);
        let form_scroll = scrollable(
            form_col.padding(Padding { top: 0.0, right: 16.0, bottom: 16.0, left: 16.0 }),
        )
        // Shared id: the keyboard router keeps the selected row in view.
        .id(iced::widget::Id::new("side-panel-scroll"))
        .height(Length::Fill);

        let mut panel_content = column![panel_header];
        if let Some(pr) = preset_row {
            panel_content = panel_content.push(pr);
        }
        let panel_content = panel_content
            .push(form_scroll)
            .push(
                container(bottom)
                    .padding(Padding { top: 8.0, right: 16.0, bottom: 16.0, left: 16.0 }),
            )
            .height(Length::Fill);

        crate::widgets::side_panel_frame(panel_content.into(), OryxisColors::t().bg_surface, self.panel_width)
    }
}

/// Muted section title used to head each card in the host editor
/// (General / Connection / Credentials / Authentication / ...). Keeps
/// the cards visually labeled so the form reads as semantic groups.
fn section_header<'a>(label: &'a str) -> Element<'a, Message> {
    text(label).size(12).color(OryxisColors::t().text_muted).into()
}

/// Full-width "click to open the theme picker" tile, painted in a
/// terminal palette: `label` in the theme foreground, ANSI swatches on
/// the trailing edge, the theme background as the fill. Used for both a
/// chosen per-host theme and the "use global" state (where it previews
/// the inherited global theme).
fn terminal_theme_trigger<'a>(
    palette: oryxis_terminal::TerminalPalette,
    label: String,
) -> Element<'a, Message> {
    let bg = palette.background;
    let fg = palette.foreground;
    let swatches: Vec<Element<'a, Message>> = [1usize, 2, 3, 4, 5, 6]
        .iter()
        .map(|&i| {
            let color = palette.ansi[i];
            container(
                Space::new()
                    .width(Length::Fixed(10.0))
                    .height(Length::Fixed(10.0)),
            )
            .style(move |_| container::Style {
                background: Some(Background::Color(color)),
                border: Border { radius: Radius::from(5.0), ..Default::default() },
                ..Default::default()
            })
            .into()
        })
        .collect();
    button(
        container(
            dir_row(vec![
                text(label).size(13).color(fg).into(),
                Space::new().width(Length::Fill).into(),
                iced::widget::Row::with_children(swatches).spacing(4).into(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 10.0, right: 12.0, bottom: 10.0, left: 12.0 })
        .width(Length::Fill),
    )
    .on_press(Message::Editor(EditorMessage::EditorOpenThemePicker))
    .padding(0)
    .width(Length::Fill)
    .style(move |_, _| button::Style {
        background: Some(Background::Color(bg)),
        border: Border { radius: Radius::from(8.0), ..Default::default() },
        ..Default::default()
    })
    .into()
}
