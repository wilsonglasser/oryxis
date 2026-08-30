//! Host editor: the collapsible-section machinery of the two-tier form.
//!
//! The essential fields (label / address / port / username / password)
//! stay always visible in `mod.rs`; everything else lives under one of
//! the [`HostEditorSection`] headers, closed by default. Two contracts
//! keep this honest:
//!
//! - A closed section's body is NEVER BUILT (the `body` closure only
//!   runs while open): `panel_nav_slot` records keyboard rows at build
//!   time, so an ungated build would record invisible Tab targets.
//!   Same `empty()` discipline as the reduced Telnet / Serial forms.
//! - A closed header carries a summary of every non-default value it
//!   hides ([`Oryxis::hp_section_summary`]), so progressive disclosure
//!   never silently hides configured state. "Non-default" is measured
//!   against the untouched-host baseline, not against the user's
//!   new-connection defaults: the summary reports what is ACTIVE on
//!   this host, wherever the value came from. Group-inherited values
//!   (D4) are deliberately absent, they are not stored on the host and
//!   already announce themselves through the inline "inherited from"
//!   hints once the section is open.

use super::*;
use crate::state::HostEditorSection;
use iced::widget::column;

/// How many summary tokens a closed header spells out before the rest
/// collapse into a "+N" tail. Keeps a fully-configured host's header
/// to one or two lines in the ~400px drawer.
const SUMMARY_TOKENS_SHOWN: usize = 4;

impl Oryxis {
    /// One collapsible section card: a chevron header that toggles on
    /// click / Enter (recorded as a panel keyboard row), the non-default
    /// summary while closed, and the body built by `body` only while
    /// open. The closure runs AFTER the header records, so keyboard
    /// rows land in visual order.
    pub(super) fn hp_section<'a>(
        &'a self,
        section: HostEditorSection,
        body: impl FnOnce() -> Element<'a, Message>,
    ) -> Element<'a, Message> {
        let open = self.host_editor_open_sections.contains(&section);
        let toggle = Message::Editor(EditorMessage::EditorSectionToggled(section));
        // Closed points at the content along the reading direction,
        // open points down (the platform-wide disclosure idiom).
        let chevron = if open {
            iced_fonts::lucide::chevron_down()
        } else if crate::i18n::is_rtl_layout() {
            iced_fonts::lucide::chevron_left()
        } else {
            iced_fonts::lucide::chevron_right()
        };
        // A glyph per section gives the closed stack a scannable
        // hierarchy: the eye finds "the key one" / "the globe one"
        // before reading a word.
        let glyph = match section {
            HostEditorSection::Authentication => iced_fonts::lucide::key(),
            HostEditorSection::Network => iced_fonts::lucide::globe(),
            HostEditorSection::Compatibility => iced_fonts::lucide::wrench(),
            HostEditorSection::Integration => iced_fonts::lucide::blocks(),
            HostEditorSection::Terminal => iced_fonts::lucide::terminal(),
        };
        let mut header_col = column![
            dir_row(vec![
                chevron.size(14).color(OryxisColors::t().text_muted).into(),
                Space::new().width(8).into(),
                glyph.size(13).color(OryxisColors::t().text_secondary).into(),
                Space::new().width(7).into(),
                text(t(section.title_key()))
                    .size(13)
                    .color(OryxisColors::t().text_secondary)
                    .into(),
                Space::new().width(Length::Fill).into(),
            ])
            .align_y(iced::Alignment::Center),
        ];
        if !open {
            let summary = summary_line(self.hp_section_summary(section));
            if let Some(summary) = summary {
                // Accent, not muted: the line flags configured state,
                // and it must not read like a description.
                header_col = header_col.push(
                    container(
                        text(summary).size(11).color(OryxisColors::t().accent),
                    )
                    // Align under the title, past the chevron + glyph.
                    .padding(Padding { top: 4.0, right: 0.0, bottom: 0.0, left: 42.0 }),
                );
            }
        }
        let header: Element<'a, Message> = self.panel_nav_slot(
            crate::keynav::RowAction::activate(toggle.clone()),
            6.0,
            button(container(header_col).width(Length::Fill))
                .on_press(toggle)
                .width(Length::Fill)
                .padding(Padding { top: 6.0, right: 6.0, bottom: 6.0, left: 6.0 })
                .style(|_, status| {
                    let bg = match status {
                        BtnStatus::Hovered | BtnStatus::Pressed => OryxisColors::t().bg_selected,
                        _ => Color::TRANSPARENT,
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border { radius: Radius::from(6.0), ..Default::default() },
                        ..Default::default()
                    }
                })
                .into(),
        );
        let mut col = column![header];
        if open {
            col = col.push(Space::new().height(GROUP_GAP)).push(body());
        }
        panel_section(col)
    }

    /// The non-default values a closed `section` header must not hide:
    /// one short token per configured thing, existing row labels and
    /// raw values only (so no summary-specific i18n keys to keep in 23
    /// languages). Order follows the rows inside the section.
    pub(super) fn hp_section_summary(&self, section: HostEditorSection) -> Vec<String> {
        use oryxis_core::models::connection::{AddressFamily, AuthMethod};
        let form = &self.editor_form;
        let mut tokens: Vec<String> = Vec::new();
        match section {
            HostEditorSection::Authentication => {
                // An identity announces itself in the always-visible
                // credentials banner, so it is not repeated here; the
                // method / key only matter while no identity overrides
                // them.
                if form.selected_identity.is_none() {
                    if form.auth_method != AuthMethod::Auto {
                        tokens.push(crate::util::auth_method_label(&form.auth_method));
                    }
                    if matches!(
                        form.auth_method,
                        AuthMethod::Key | AuthMethod::Certificate | AuthMethod::Agent
                    ) && let Some(key) = &form.selected_key
                        && key != "(none)"
                    {
                        tokens.push(key.clone());
                    }
                }
                if form.agent_forwarding {
                    tokens.push(t("forward_ssh_agent").into());
                }
                if form.x11_forwarding {
                    tokens.push(t("forward_x11").into());
                }
                if form.use_totp || form.has_existing_totp {
                    // A protocol name, not a translatable label.
                    tokens.push("TOTP".into());
                }
            }
            HostEditorSection::Network => {
                if !form.jump_chain.is_empty() {
                    tokens.push(format!(
                        "{} ({})",
                        t("host_chaining"),
                        form.jump_chain.len()
                    ));
                }
                match form.proxy_kind {
                    crate::state::ProxyKind::None => {}
                    crate::state::ProxyKind::Identity(id) => tokens.push(
                        self.proxy_identities
                            .iter()
                            .find(|pi| pi.id == id)
                            .map(|pi| pi.label.clone())
                            .unwrap_or_else(|| {
                                t("proxy_type_identity_deleted").into()
                            }),
                    ),
                    kind => tokens.push(kind.to_string()),
                }
                if !form.port_forwards.is_empty() {
                    tokens.push(format!(
                        "{} ({})",
                        t("port_forwarding"),
                        form.port_forwards.len()
                    ));
                }
                if !form.keepalive_interval.is_empty() {
                    tokens.push(format!(
                        "{}: {}",
                        t("host_keepalive"),
                        form.keepalive_interval
                    ));
                }
                if form.address_family != AddressFamily::Auto {
                    tokens.push(form.address_family.to_string());
                }
                if !form.mac_address.trim().is_empty() {
                    tokens.push(t("host_mac_address").into());
                }
                if form.auto_title.is_some() {
                    tokens.push(t("host_auto_title").into());
                }
            }
            HostEditorSection::Compatibility => {
                let pinned = crate::state::AlgoCategory::ALL
                    .iter()
                    .filter(|cat| form.algo_list(**cat).is_some())
                    .count();
                if pinned > 0 {
                    tokens.push(format!("{} ({pinned})", t("algo_overrides")));
                }
                if form.quirks
                    != oryxis_core::models::terminal_quirks::TerminalQuirks::default()
                {
                    tokens.push(t("quirks_section_title").into());
                }
                if !form.rekey_limit_mb.trim().is_empty() {
                    tokens.push(t("quirks_rekey_limit").into());
                }
            }
            HostEditorSection::Integration => {
                // Default is ON, so the configured state worth flagging
                // is the opt-out.
                if !form.mcp_enabled {
                    tokens.push(format!(
                        "{}: {}",
                        t("expose_to_mcp"),
                        t("toggle_off")
                    ));
                }
                if form.monitor_enabled {
                    tokens.push(t("monitor_enable_host").into());
                }
                if !form.env_vars.is_empty() {
                    tokens.push(format!("{} ({})", t("env_vars"), form.env_vars.len()));
                }
                if !matches!(self.editor_startup_choice, crate::state::StartupChoice::None) {
                    tokens.push(t("initial_command_label").into());
                }
                if form.login_script_id.is_some() {
                    tokens.push(t("login_script_label").into());
                }
                if !form.sftp_initial_path.trim().is_empty() {
                    tokens.push(t("host_sftp_initial_path").into());
                }
                if form.zmodem_drops {
                    tokens.push(t("host_zmodem_drops").into());
                }
            }
            HostEditorSection::Terminal => {
                if let Some(theme) = &form.terminal_theme {
                    tokens.push(theme.clone());
                }
                if form.icon_style.is_some() {
                    tokens.push(t("host_icon_style").into());
                }
                if let Some(encoding) = form.encoding.as_deref()
                    && encoding != "UTF-8"
                {
                    tokens.push(encoding.to_string());
                }
                if let Some(term) = form.terminal_type.as_deref()
                    && term != "xterm-256color"
                {
                    tokens.push(term.to_string());
                }
                if form.terminal_appearance.opacity.is_some() {
                    tokens.push(t("terminal_opacity").into());
                }
                if form.terminal_appearance.image.is_some() {
                    tokens.push(t("terminal_bg_image").into());
                }
                if !form.highlight_rules.rules.is_empty() || form.highlight_rules.replace {
                    tokens.push(format!(
                        "{} ({})",
                        t("highlight_rules"),
                        form.highlight_rules.rules.len()
                    ));
                }
                if let Some(on) = form.session_logging {
                    tokens.push(format!(
                        "{}: {}",
                        t("session_logging"),
                        t(if on { "session_log_on" } else { "session_log_off" })
                    ));
                }
                if form.privacy_mode.is_some() {
                    tokens.push(t("host_privacy_mode").into());
                }
                if form.sidebar_auto_open.is_some() {
                    tokens.push(t("sidebar_auto_open").into());
                }
            }
        }
        tokens
    }

    /// Create-flow starting points (P3): a row of one-shot chips under
    /// the panel header, new-host flow only. Each chip prepares the
    /// form for a common shape of host (`HostEditorPreset`); none is a
    /// persisted mode, so there is no selected state, they are verbs.
    /// Built (= keyboard-recorded) before the form fields, matching
    /// their on-screen position above the scroll.
    pub(super) fn hp_preset_row(&self) -> Element<'_, Message> {
        use crate::state::HostEditorPreset as P;
        let chip = |app: &Self, preset: P| -> Element<'_, Message> {
            let icon = match preset {
                P::BasicSsh => iced_fonts::lucide::server(),
                P::ViaBastion => iced_fonts::lucide::route(),
            };
            let msg = Message::Editor(EditorMessage::EditorPresetPicked(preset));
            app.panel_nav_slot(
                crate::keynav::RowAction::activate(msg.clone()),
                4.0,
                button(
                    dir_row(vec![
                        icon.size(13).color(OryxisColors::t().text_secondary).into(),
                        Space::new().width(6).into(),
                        text(t(preset.label_key()))
                            .size(12)
                            .color(OryxisColors::t().text_secondary)
                            .into(),
                    ])
                    .align_y(iced::Alignment::Center),
                )
                .on_press(msg)
                .padding(Padding { top: 5.0, right: 10.0, bottom: 5.0, left: 10.0 })
                .style(|_, status| {
                    let bg = match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            OryxisColors::t().bg_hover
                        }
                        _ => OryxisColors::t().bg_surface,
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border {
                            radius: Radius::from(14.0),
                            width: 1.0,
                            color: OryxisColors::t().border,
                        },
                        ..Default::default()
                    }
                })
                .into(),
            )
        };
        container(
            dir_row(vec![
                text(t("editor_preset_heading"))
                    .size(11)
                    .color(OryxisColors::t().text_muted)
                    .into(),
                Space::new().width(8).into(),
                chip(self, P::BasicSsh),
                Space::new().width(6).into(),
                chip(self, P::ViaBastion),
                Space::new().width(6).into(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 0.0, right: 16.0, bottom: 10.0, left: 16.0 })
        .into()
    }
}

/// Join the summary tokens into the closed header's one-liner,
/// spelling out the first few and folding the rest into "+N".
fn summary_line(tokens: Vec<String>) -> Option<String> {
    if tokens.is_empty() {
        return None;
    }
    let extra = tokens.len().saturating_sub(SUMMARY_TOKENS_SHOWN);
    let mut line = tokens
        .iter()
        .take(SUMMARY_TOKENS_SHOWN)
        .cloned()
        .collect::<Vec<_>>()
        .join(" · ");
    if extra > 0 {
        line.push_str(&format!(" · +{extra}"));
    }
    Some(line)
}
