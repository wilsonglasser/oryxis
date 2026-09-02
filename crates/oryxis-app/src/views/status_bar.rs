//! Bottom status bar, connection state, keepalive info, and host summary.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, text, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{SettingsMessage, TabsMessage, TerminalMessage, Message, Oryxis};
use crate::tab_conn_state::TabConnState;
use crate::theme::OryxisColors;

impl Oryxis {
    pub(crate) fn view_status_bar(&self) -> Element<'_, Message> {
        // The segment used to hard-code "connected" for any active tab,
        // so a dialing, a dead and an established session all read the
        // same. It now spells out the tab's real state, derived by the
        // same `tab_conn_state` the strip's status dot reads (one
        // authority, so the two can't disagree on one frame).
        let (status_text, status_color) = if let Some(idx) = self.active_tab
            && let Some(tab) = self.tabs.get(idx)
        {
            // The FOCUSED pane's name, not the tab's own (issue #208).
            // `tab_conn_state` below has always described the focused
            // pane, and the tab chip has always been captioned by it, so
            // reading `tab.label` here made the bar the one surface that
            // named the session that created the split while reporting
            // the state of the one being typed into. Same accessors the
            // chip uses, so the two cannot drift.
            //
            // Privacy Mode redacts the label here too (issue #78): the
            // status bar sits in every screenshot. No hover reveal on a
            // passive text line; the tab strip has one.
            let auto_title = self.tab_auto_title(tab);
            let label = self.privacy_display_label(
                tab.auto_label(auto_title),
                tab.display_label(auto_title),
                &self.privacy_terms(),
            );
            // The "(disconnected)" suffix is the strip's own way of
            // saying what this segment now says in full; keeping both
            // reads as "host (disconnected), disconnected".
            let label = label.trim_end_matches(" (disconnected)");
            let c = OryxisColors::t();
            // Status words are lower-case state labels ("connected"),
            // not sentences, so they sit after the comma the way the
            // original segment read.
            let with = |key: &str, color: Color| {
                (format!("● {}, {}", label, crate::i18n::t(key)), color)
            };
            match self.tab_conn_state(idx) {
                TabConnState::Connecting => with("status_bar_connecting", c.warning),
                TabConnState::Reconnecting => {
                    with("status_bar_reconnecting", c.warning)
                }
                TabConnState::Connected => with("status_bar_connected", c.success),
                // The always-visible half of a mosh link's health: this
                // segment is on by default and the latency one is not,
                // so if the state only rode the latency segment the
                // people who never turned it on would still be told a
                // silent link was connected. How long, and in which
                // direction, is the detail the latency segment carries.
                TabConnState::NoContact => with("status_bar_no_contact", c.warning),
                TabConnState::Lost => with("status_bar_disconnected", c.error),
                // A local shell has no connection to report, and a
                // dormant pinned tab hasn't dialed yet: name the tab and
                // claim nothing.
                TabConnState::Idle => {
                    (format!("● {}", label), c.text_secondary)
                }
            }
        } else {
            (crate::i18n::t("no_active_connection").into(), OryxisColors::t().text_muted)
        };

        // 1 px hairline on top only, iced's Border has a single width that
        // applies to all four sides, so a dedicated separator widget is the
        // way to keep just the top edge.
        let top_hairline = container(Space::new().height(1))
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().border)),
                ..Default::default()
            });

        // The connection text is opt-out (issue #83 follow-up): hiding it
        // declutters the bar and keeps the host name out of screenshots.
        // Content alignment (issue #83 follow-up): the vitals cluster hugs
        // the trailing edge by default; `status_bar_align_left` moves it to
        // the PHYSICAL left edge instead, so it lines up with a left-docked
        // panel layout. The panel dock is a physical edge that RTL does not
        // flip, and every language's label literally promises "left", so
        // this lever must not ride `dir_row`'s logical reversal: under RTL
        // the flexible spacer enters the vector on the opposite (leading)
        // side, which the reversal then lands on the physical right.
        let align_left = self.prefs.status_bar_align_left;
        let spacer_leads = align_left && crate::i18n::is_rtl_layout();
        let mut items: Vec<Element<'_, Message>> = Vec::new();
        if spacer_leads {
            items.push(Space::new().width(Length::Fill).into());
        }
        if self.prefs.status_show_connection {
            items.push(text(status_text).size(12).color(status_color).into());
            // Leading-aligned: keep a gap between the label and the cluster
            // that the flexible spacer used to provide.
            if align_left {
                items.push(Space::new().width(16).into());
            }
        }
        if !align_left {
            items.push(Space::new().width(Length::Fill).into());
        }

        // Terminal-status segments read from the focused pane: latency
        // (the SSH RTT probe), grid size and cwd. Each is off by default
        // and individually toggleable, so the bar only carries what the
        // user asked for. Muted labels, so they read as ambient info.
        if let Some(pane) = self.active_tab.and_then(|i| self.tabs.get(i)).map(|t| t.active()) {
            // One slot, read from whichever transport the pane holds.
            // mosh rides the latency toggle rather than a setting of its
            // own because this is the "how is the network under this
            // pane" slot, and a second toggle would make the user work
            // out which transport they were on before knowing which one
            // to turn on. It has no round trip to report, so it reports
            // silence, exactly as the SSH arm already does when its
            // probe goes unanswered.
            if self.prefs.status_show_latency
                && let Some(transport) = pane.session.as_ref()
            {
                let segment = if let Some(ssh) = transport.ssh() {
                    latency_segment(&ssh.net_quality(), self.pane_shares_connection(pane))
                } else {
                    transport.mosh().and_then(|m| mosh_link_segment(m.link_state()))
                };
                if let Some(segment) = segment {
                    items.push(segment);
                    items.push(Space::new().width(12).into());
                }
            }
            if self.prefs.status_show_dimensions
                && let Ok(term) = pane.terminal.lock()
            {
                let (c, r) = (term.cols(), term.rows());
                if c > 0 && r > 0 {
                    items.push(vital(
                        crate::i18n::t("status_dimensions"),
                        format!("{c}×{r}"),
                        OryxisColors::t().text_secondary,
                    ));
                    items.push(Space::new().width(12).into());
                }
            }
            if self.prefs.status_show_cwd
                && let Some(cwd) = pane.cwd.as_deref().filter(|c| !c.is_empty())
            {
                // Privacy Mode redacts the path like the connection label
                // above: a home path carries the username, and the bar
                // sits in every screenshot.
                let cwd = self.privacy_display_label(cwd, cwd, &self.privacy_terms());
                // Middle-truncate deep paths so the bar never overflows;
                // the tail is the informative half of a path.
                let shown = middle_truncate(&cwd, 60);
                items.push(vital(
                    crate::i18n::t("status_cwd"),
                    shown,
                    OryxisColors::t().text_secondary,
                ));
                items.push(Space::new().width(12).into());
            }
        }
        // Tab surface segments (issue #61, widened for the SFTP
        // console): redundant with the tab's own glyph on purpose, the
        // status bar is optional (`show_status_bar`), so it can carry a
        // switch but never THE switch. Where the glyph cycles (one
        // chip's worth of room), the bar names every surface and goes
        // straight to the one clicked, which is what makes a three-way
        // switch readable.
        if let Some(idx) = self.active_tab {
            let surfaces = self.tab_surfaces(idx);
            if surfaces.len() > 1 {
                let current = self.tab_surface(idx);
                for (n, surface) in surfaces.into_iter().enumerate() {
                    if n > 0 {
                        items.push(Space::new().width(2).into());
                    }
                    items.push(mode_segment_btn(idx, surface, surface == current));
                }
                items.push(Space::new().width(10).into());
            }
        }
        // Broadcast input segment (C2): a single toggle for the active
        // terminal tab. Redundant with the tab menu + hotkey by design (the
        // status bar is optional). Armed state is warning-tinted so the "keys
        // go everywhere" mode is loud even from the bar. Only rendered when
        // two panes would actually take it, same precondition-gating as the
        // surface segments above (broadcast is inert on a single pane, and
        // an SFTP console never takes the fan-out).
        if let Some(idx) = self.active_tab
            && let Some(tab) = self.tabs.get(idx)
            && tab.broadcast_capable()
        {
            items.push(broadcast_segment_btn(idx, tab.broadcast));
            items.push(Space::new().width(10).into());
        }
        // Login automation progress (issue #122). Present only while a
        // script is actually running, which is a few seconds at connect,
        // so it costs nothing the rest of the time. Read-only: the way
        // to stop a run is to type, which is also the way to take over.
        if let Some((step, total)) = self.login_script_progress() {
            items.push(
                text(
                    crate::i18n::t("login_script_progress")
                        .replace("{step}", &step.to_string())
                        .replace("{total}", &total.to_string()),
                )
                .size(11)
                .color(OryxisColors::t().accent)
                .into(),
            );
            items.push(Space::new().width(10).into());
        }
        // Host vitals (issue #83, the MobaXterm-style bar): the same
        // samples the sidebar Monitor tab renders, condensed to one line.
        // Behind its own setting AND the host's monitoring opt-in, so it
        // costs nothing until the user asks for both.
        if self.prefs.host_monitoring
            && self.prefs.monitor_status_bar
            && let Some(conn_id) = self.monitor_pane_connection()
            // Effective opt-in, not just "a series exists": after the
            // host (or the all-hosts toggle) opts out the probing stops
            // but the series lingers, and painting its last sample would
            // present frozen numbers as live vitals.
            && self.monitor_host_opted_in(&conn_id)
            && let Some(sample) = self.monitor_sample(&conn_id)
        {
            let c = OryxisColors::t();
            // Thresholds tint the value, not the label, so a pegged host
            // reads at a glance without the bar turning into a wall of
            // colour.
            let tint = |pct: f32| {
                if pct >= 90.0 {
                    c.error
                } else if pct >= 75.0 {
                    c.warning
                } else {
                    c.text_secondary
                }
            };
            let mut seg: Vec<Element<'_, Message>> = Vec::new();
            if let Some(cpu) = sample.cpu {
                seg.push(vital(
                    crate::i18n::t("monitor_cpu"),
                    format!("{:.0}%", cpu.pct),
                    tint(cpu.pct),
                ));
            }
            if let Some(mem) = sample.mem {
                seg.push(vital(
                    crate::i18n::t("monitor_mem"),
                    format!("{:.0}%", mem.pct()),
                    tint(mem.pct()),
                ));
            }
            if let Some(net) = sample.net {
                seg.push(vital(
                    crate::i18n::t("monitor_net"),
                    // The "/s" stays even though room is scarce: without
                    // it the reading looks like a total, not a rate.
                    format!(
                        "↓{}/s ↑{}/s",
                        crate::views::sidebar_monitor::fmt_bytes_short(net.rx_bps),
                        crate::views::sidebar_monitor::fmt_bytes_short(net.tx_bps)
                    ),
                    c.text_secondary,
                ));
            }
            // Disks (issue #83 follow-up): the busiest mount rides the bar
            // (the one worth a glance), a `+N` suffix marks the rest, and a
            // hover tooltip lists every mount so the collapsed badge never
            // hides anything. Status-bar idiom: hover reveals detail, like
            // the terminal `icon_tooltip`.
            if let Some(disk) = sample
                .disks
                .iter()
                .max_by(|a, b| a.pct().total_cmp(&b.pct()))
            {
                let extra = sample.disks.len().saturating_sub(1);
                let value = if extra > 0 {
                    format!("{:.0}% +{extra}", disk.pct())
                } else {
                    format!("{:.0}%", disk.pct())
                };
                // Privacy Mode redacts mount paths like the cwd above:
                // `/home/<user>` carries the username and `/srv/<project>`
                // a company name, and the tooltip lists every mount.
                let terms = self.privacy_terms();
                let mount =
                    self.privacy_display_label(&disk.mount, &disk.mount, &terms);
                let badge = vital(&mount, value, tint(disk.pct()));
                if extra > 0 {
                    let list = sample
                        .disks
                        .iter()
                        .map(|d| {
                            format!(
                                "{}  {:.0}%  {} / {}",
                                self.privacy_display_label(&d.mount, &d.mount, &terms),
                                d.pct(),
                                crate::views::sidebar_monitor::fmt_bytes_short(d.used),
                                crate::views::sidebar_monitor::fmt_bytes_short(d.total),
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    seg.push(
                        iced::widget::tooltip(
                            badge,
                            container(
                                text(list).size(11).color(OryxisColors::t().text_primary),
                            )
                            .padding(Padding { top: 6.0, right: 8.0, bottom: 6.0, left: 8.0 })
                            .style(|_| container::Style {
                                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                                border: Border {
                                    radius: Radius::from(6.0),
                                    color: OryxisColors::t().border,
                                    width: 1.0,
                                },
                                ..Default::default()
                            }),
                            iced::widget::tooltip::Position::Top,
                        )
                        .into(),
                    );
                } else {
                    seg.push(badge);
                }
            }
            for el in seg {
                items.push(el);
                items.push(Space::new().width(12).into());
            }
        }
        // Privacy Mode chip (issue #78): visible whenever masking is
        // globally effective or a session override is armed, so the
        // state is never silent (the original #53 confusion). Clicking
        // toggles the session override, same as the Ctrl+Shift+M
        // hotkey.
        if self.privacy_global_active() || self.privacy.session_override.is_some() {
            items.push(privacy_segment_btn(
                self.privacy_global_active(),
                self.privacy.session_override.is_some(),
            ));
            items.push(Space::new().width(10).into());
        }
        // The version is opt-out too; hidden it leaves the trailing edge
        // clean.
        if self.prefs.status_show_version {
            items.push(
                text(concat!("Oryxis v", env!("CARGO_PKG_VERSION")))
                    .size(12)
                    .color(OryxisColors::t().text_muted)
                    .into(),
            );
        }
        // Left-aligned: the flexible spacer trails the cluster instead,
        // pushing everything to the physical left edge (under RTL it was
        // pushed at the head above, `dir_row`'s reversal moves it here).
        if align_left && !spacer_leads {
            items.push(Space::new().width(Length::Fill).into());
        }
        let bar = container(
            crate::widgets::dir_row(items)
                .align_y(iced::Alignment::Center)
                .padding(Padding { top: 3.0, right: 12.0, bottom: 3.0, left: 12.0 }),
        )
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
            ..Default::default()
        });

        column![top_hairline, bar].into()
    }
}

/// Middle-truncate a path-like string to at most `max` characters,
/// keeping the head and the (more informative) tail around a single
/// `…`. Char-based so multibyte paths never split inside a code point.
fn middle_truncate(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    let head = max / 3;
    let tail = max - head - 1;
    let start: String = s.chars().take(head).collect();
    let end: String = s
        .chars()
        .skip(count - tail)
        .collect();
    format!("{start}…{end}")
}

/// One host-vital readout in the status bar: muted label, tinted value
/// (issue #83). Passive text, not a control: the sidebar Monitor tab is
/// where the numbers are actionable. Owns its label copy, so callers
/// can pass short-lived strings (the privacy-masked mount below).
/// A link answering slower than this reads as degraded (amber); slower
/// than [`RTT_BAD_MS`] reads as bad (red). Round numbers on purpose:
/// they are a glanceable band, not a measurement, and the tooltip
/// carries the real figures for anyone who wants them.
const RTT_OK_MS: u128 = 80;
const RTT_BAD_MS: u128 = 250;

/// Colour and text for a link's current state.
///
/// A STALLED link is the first branch and the reason this function
/// exists: `last_rtt` keeps reporting the last good round trip after
/// the server goes quiet, so the bar used to show a healthy number on a
/// connection that had stopped answering. Silence is the more important
/// fact, so it replaces the number instead of colouring it.
fn latency_reading(snapshot: &oryxis_ssh::NetQualitySnapshot) -> Option<(String, Color)> {
    let c = OryxisColors::t();
    if let Some(silent) = snapshot.silent_for {
        return Some((
            crate::i18n::t("net_stalled").replace("{s}", &silent.as_secs().to_string()),
            c.error,
        ));
    }
    let rtt = snapshot.last_rtt?;
    let ms = rtt.as_millis();
    let color = if ms < RTT_OK_MS {
        c.success
    } else if ms < RTT_BAD_MS {
        c.warning
    } else {
        c.error
    };
    Some((format!("{ms} ms"), color))
}

/// The latency segment, with the fuller figures on hover. `None` when
/// the link has never answered (a session still authenticating has no
/// round trip to report, and inventing one would be worse than silence).
fn latency_segment(
    snapshot: &oryxis_ssh::NetQualitySnapshot,
    shared: bool,
) -> Option<Element<'static, Message>> {
    let (value, color) = latency_reading(snapshot)?;
    let segment = vital(crate::i18n::t("status_latency"), value, color);
    // avg / peak / jitter belong on hover: they are what you look at
    // once the dot has already told you something is off.
    let ms = |d: Option<std::time::Duration>| {
        d.map(|d| d.as_millis().to_string())
            .unwrap_or_else(|| "-".to_string())
    };
    let mut tip = crate::i18n::t("net_latency_tip")
        .replace("{avg}", &ms(snapshot.avg_rtt))
        .replace("{peak}", &ms(snapshot.peak_rtt))
        .replace("{jitter}", &ms(snapshot.jitter))
        .replace("{timeouts}", &snapshot.timeouts.to_string());
    // A shared connection is a user-visible fact, not an implementation
    // detail: every tab riding it dies at the same instant when it
    // drops, and without saying so that reads as several tabs breaking
    // at once for no reason.
    if shared {
        tip.push('\n');
        tip.push_str(crate::i18n::t("net_shared_connection"));
    }
    Some(crate::views::terminal::icon_tooltip_owned(segment, tip))
}

/// Colour and text for a mosh link, or `None` while it is in touch.
///
/// Silence IS the reading here, rather than a fallback for when the
/// measurement is missing: mosh reports no round trip, and the number
/// people actually want from it is how long it has been out of touch.
/// Its own client shows exactly this and nothing when the link is fine.
///
/// Amber, where the SSH arm's stall is red, and the difference is not
/// cosmetic. A silent SSH session is very probably a dead one; a silent
/// mosh session is a working one whose network is away, which is the
/// case the protocol was built for. Red would report a loss that has not
/// happened.
fn mosh_link_reading(state: oryxis_mosh::LinkState) -> Option<(String, Color)> {
    let (key, ms) = match state {
        oryxis_mosh::LinkState::Healthy => return None,
        oryxis_mosh::LinkState::NoContact { ms } => ("net_no_contact", ms),
        oryxis_mosh::LinkState::NoReply { ms } => ("net_no_reply", ms),
    };
    let text = crate::i18n::t(key).replace("{t}", &elapsed_short(ms / 1000));
    Some((text, OryxisColors::t().warning))
}

/// Elapsed seconds as a status bar can afford to print them: `12s`, then
/// `5:12`, then `1:05:12`. A link that has been away for five minutes
/// reading `312s` is a number the reader has to do arithmetic on.
fn elapsed_short(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    } else {
        format!("{}:{:02}:{:02}", seconds / 3600, (seconds / 60) % 60, seconds % 60)
    }
}

/// The mosh link segment, with what silence actually means on hover.
///
/// The tooltip is the half that stops the amber from reading as a
/// failure: a session out of touch is still there, with its shell and
/// its scrollback, and it picks up where it left off. Nobody guesses
/// that from a coloured dot.
fn mosh_link_segment(state: oryxis_mosh::LinkState) -> Option<Element<'static, Message>> {
    let (value, color) = mosh_link_reading(state)?;
    let segment = vital(crate::i18n::t("status_link"), value, color);
    Some(crate::views::terminal::icon_tooltip_owned(
        segment,
        crate::i18n::t("net_mosh_quiet_tip").to_string(),
    ))
}

fn vital(label: &str, value: String, color: Color) -> Element<'static, Message> {
    crate::widgets::dir_row(vec![
        text(label.to_string())
            .size(11)
            .color(OryxisColors::t().text_muted)
            .into(),
        Space::new().width(4).into(),
        text(value).size(11).color(color).into(),
    ])
    .align_y(iced::Alignment::Center)
    .into()
}

/// One half of the status-bar Terminal/Files segment. The active half
/// is an accent-tinted indicator; the inactive half is the clickable
/// action (clicking the active one would be a no-op, so it gets no
/// `on_press` and no misleading hover state).
fn mode_segment_btn<'a>(
    idx: usize,
    surface: crate::state::TabSurface,
    active: bool,
) -> Element<'a, Message> {
    let c = OryxisColors::t();
    let fg = if active { c.accent } else { c.text_muted };
    let mut btn = button(text(crate::i18n::t(surface.label_key())).size(11).color(fg))
        .padding(Padding { top: 1.0, right: 8.0, bottom: 1.0, left: 8.0 })
        .style(move |_, status| {
            let c = OryxisColors::t();
            let bg = if active {
                Color { a: 0.12, ..c.accent }
            } else {
                match status {
                    BtnStatus::Hovered | BtnStatus::Pressed => c.bg_hover,
                    _ => Color::TRANSPARENT,
                }
            };
            let border_color = if active { Color { a: 0.35, ..c.accent } } else { Color::TRANSPARENT };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(5.0), color: border_color, width: 1.0 },
                ..Default::default()
            }
        });
    if !active {
        btn = btn.on_press(Message::Tabs(TabsMessage::ShowTabSurface(idx, surface)));
    }
    btn.into()
}

/// Privacy Mode chip in the status bar (issue #78). Accent-tinted
/// while masking is effective; muted with a visible border while a
/// session override forces it OFF (so "my per-host privacy is
/// suspended" is readable from the bar). Clicking flips the session
/// override, mirroring the hotkey.
fn privacy_segment_btn(masking: bool, overridden: bool) -> Element<'static, Message> {
    let c = OryxisColors::t();
    let fg = if masking { c.accent } else { c.text_muted };
    button(text(crate::i18n::t("privacy_chip")).size(11).color(fg))
        .padding(Padding { top: 1.0, right: 8.0, bottom: 1.0, left: 8.0 })
        .on_press(Message::Settings(SettingsMessage::TogglePrivacySessionOverride))
        .style(move |_, status| {
            let c = OryxisColors::t();
            let bg = if masking {
                // Same accent tint at idle; the alpha rises on hover and a
                // bit more on press so the active chip still gives feedback.
                let a = match status {
                    BtnStatus::Pressed => 0.24,
                    BtnStatus::Hovered => 0.18,
                    _ => 0.12,
                };
                Color { a, ..c.accent }
            } else {
                match status {
                    BtnStatus::Hovered | BtnStatus::Pressed => c.bg_hover,
                    _ => Color::TRANSPARENT,
                }
            };
            let border_color = if masking {
                Color { a: 0.35, ..c.accent }
            } else if overridden {
                Color { a: 0.60, ..c.text_muted }
            } else {
                Color::TRANSPARENT
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(5.0), color: border_color, width: 1.0 },
                ..Default::default()
            }
        })
        .into()
}

/// Broadcast-input toggle in the status bar (C2). A single button
/// (unlike the two-half mode segment): clickable in both states, warning-
/// tinted when armed so the "keys go everywhere" mode reads loudly.
fn broadcast_segment_btn(idx: usize, armed: bool) -> Element<'static, Message> {
    let c = OryxisColors::t();
    let fg = if armed { c.warning } else { c.text_muted };
    button(text(crate::i18n::t("broadcast_input")).size(11).color(fg))
        .padding(Padding { top: 1.0, right: 8.0, bottom: 1.0, left: 8.0 })
        .on_press(Message::Terminal(TerminalMessage::ToggleTabBroadcast(idx)))
        .style(move |_, status| {
            let c = OryxisColors::t();
            let bg = if armed {
                // Same warning tint at idle; the alpha rises on hover and a
                // bit more on press so the armed chip still gives feedback.
                let a = match status {
                    BtnStatus::Pressed => 0.26,
                    BtnStatus::Hovered => 0.20,
                    _ => 0.14,
                };
                Color { a, ..c.warning }
            } else {
                match status {
                    BtnStatus::Hovered | BtnStatus::Pressed => c.bg_hover,
                    _ => Color::TRANSPARENT,
                }
            };
            let border_color = if armed { Color { a: 0.40, ..c.warning } } else { Color::TRANSPARENT };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(5.0), color: border_color, width: 1.0 },
                ..Default::default()
            }
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn snapshot(
        last_rtt: Option<u64>,
        silent_for: Option<u64>,
    ) -> oryxis_ssh::NetQualitySnapshot {
        oryxis_ssh::NetQualitySnapshot {
            last_rtt: last_rtt.map(Duration::from_millis),
            avg_rtt: last_rtt.map(Duration::from_millis),
            peak_rtt: last_rtt.map(Duration::from_millis),
            jitter: Some(Duration::from_millis(1)),
            timeouts: 0,
            silent_for: silent_for.map(Duration::from_secs),
        }
    }

    /// The reason this branch is first: `last_rtt` keeps reporting the
    /// last good round trip after the server goes quiet, so a stalled
    /// link used to show a healthy number. Silence has to win over a
    /// stale measurement, however good that measurement was.
    #[test]
    fn a_stalled_link_reports_silence_not_its_last_good_rtt() {
        let (text, color) = latency_reading(&snapshot(Some(5), Some(12))).unwrap();
        assert!(text.contains("12"), "expected the silence in seconds: {text}");
        assert!(!text.contains("5 ms"), "the stale rtt must not show: {text}");
        assert_eq!(color, OryxisColors::t().error);
    }

    #[test]
    fn the_colour_bands_follow_the_round_trip() {
        let c = OryxisColors::t();
        assert_eq!(latency_reading(&snapshot(Some(0), None)).unwrap().1, c.success);
        assert_eq!(latency_reading(&snapshot(Some(79), None)).unwrap().1, c.success);
        // The boundaries belong to the worse band, so a link sitting
        // exactly on one never reads better than it is.
        assert_eq!(latency_reading(&snapshot(Some(80), None)).unwrap().1, c.warning);
        assert_eq!(latency_reading(&snapshot(Some(249), None)).unwrap().1, c.warning);
        assert_eq!(latency_reading(&snapshot(Some(250), None)).unwrap().1, c.error);
        assert_eq!(latency_reading(&snapshot(Some(4000), None)).unwrap().1, c.error);
    }

    /// A session that has not completed a probe yet has nothing to say.
    /// Rendering "0 ms" there would be an invented measurement.
    #[test]
    fn a_link_that_never_answered_renders_nothing() {
        assert!(latency_reading(&snapshot(None, None)).is_none());
    }

    #[test]
    fn the_reading_is_in_milliseconds() {
        let (text, _) = latency_reading(&snapshot(Some(42), None)).unwrap();
        assert_eq!(text, "42 ms");
    }

    /// mosh's client says nothing while the link is fine, and so does
    /// this: the reading answers a question, it is not a status display.
    #[test]
    fn a_mosh_link_in_touch_renders_nothing() {
        assert!(mosh_link_reading(oryxis_mosh::LinkState::Healthy).is_none());
    }

    /// A silent mosh session is a WORKING session whose network is
    /// away, so it must not borrow the colour the bar uses for a
    /// connection that is gone.
    #[test]
    fn a_quiet_mosh_link_reads_amber_not_red() {
        let c = OryxisColors::t();
        let (_, color) =
            mosh_link_reading(oryxis_mosh::LinkState::NoContact { ms: 30_000 }).unwrap();
        assert_eq!(color, c.warning);
        assert_ne!(color, c.error);
    }

    /// The two directions are different failures and the segment has to
    /// name the one that happened.
    #[test]
    fn the_two_directions_read_differently() {
        let (contact, _) =
            mosh_link_reading(oryxis_mosh::LinkState::NoContact { ms: 12_000 }).unwrap();
        let (reply, _) =
            mosh_link_reading(oryxis_mosh::LinkState::NoReply { ms: 12_000 }).unwrap();
        assert_ne!(contact, reply, "both directions said the same thing");
    }

    #[test]
    fn elapsed_grows_units_rather_than_digits() {
        assert_eq!(elapsed_short(0), "0s");
        assert_eq!(elapsed_short(59), "59s");
        assert_eq!(elapsed_short(60), "1:00");
        assert_eq!(elapsed_short(312), "5:12");
        assert_eq!(elapsed_short(3599), "59:59");
        assert_eq!(elapsed_short(3600), "1:00:00");
        assert_eq!(elapsed_short(3912), "1:05:12");
    }
}
