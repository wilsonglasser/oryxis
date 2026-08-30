//! Monitor sidebar tab: agentless host vitals for the focused pane's
//! session (issue #83). CPU / memory / load / network / disks read from
//! `/proc` over the live SSH handle, rendered as compact gauges.
//!
//! Everything here is informational, so the only keyboard row is the
//! opt-in button shown while the host hasn't enabled monitoring.

use iced::border::Radius;
use iced::widget::{column, container, text, Space};
use iced::{Background, Border, Element, Length, Padding};

use crate::app::{Message, MonitorMessage, Oryxis};
use crate::i18n::t;
use crate::state::TerminalSidebarTab;
use crate::theme::OryxisColors;
use crate::widgets::dir_row;

/// Which surface is rendering [`Oryxis::monitor_vitals_body`]. Decides
/// the keyboard-walk recorder and whether port rows carry the kill
/// menu (sidebar only: it resolves through the focused pane).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MonitorVitalsSurface {
    Sidebar,
    Dashboard,
}

impl Oryxis {
    pub(crate) fn monitor_tab_content(&self) -> Element<'_, Message> {
        // Disconnected mid-view (the tab button hides next frame).
        let Some(idx) = self.active_tab else {
            return placeholder(t("files_no_session"));
        };
        let Some(tab) = self.tabs.get(idx) else {
            return placeholder(t("files_no_session"));
        };
        if tab.active().session.as_ref().and_then(|s| s.ssh()).is_none() {
            return placeholder(t("files_no_session"));
        }

        // Only saved hosts carry the opt-in flag: a quick-connect, local
        // pane has no vault row to enable it on.
        let Some(conn_id) = self.monitor_pane_connection() else {
            return placeholder(t("monitor_requires_host"));
        };
        // One rule for the opt-in, shared with the probe target, the
        // status bar and the dashboard's fleet.
        if !self.monitor_host_opted_in(&conn_id) {
            return self.monitor_opt_in(conn_id);
        }

        let Some(body) = self.monitor_vitals_body(conn_id, MonitorVitalsSurface::Sidebar)
        else {
            // Probing, or the first probe failed.
            return match &self.monitor_error {
                Some(e) => placeholder(e),
                None => placeholder(t("monitor_sampling")),
            };
        };

        iced::widget::scrollable(body)
            .id(crate::keynav::sidebar_scroll_id(crate::state::TerminalSidebarTab::Monitor))
            .height(Length::Fill)
            .into()
    }

    /// The vitals body shared by the sidebar Monitor tab and the
    /// dashboard's detail panel (issue #95 follow-up, owner call: the
    /// two surfaces must present a host identically, collapsible
    /// sections included). `None` until the host has a sample.
    ///
    /// The surface picks how interactive rows join the keyboard walk
    /// (sidebar rows vs dashboard content actions) and what a port
    /// row offers: the kill menu resolves its session through the
    /// focused pane, which the dashboard doesn't have, so there the
    /// rows keep only the local-forward action (which just prefills
    /// the rule editor and works from anywhere).
    pub(crate) fn monitor_vitals_body<'a>(
        &'a self,
        conn_id: uuid::Uuid,
        surface: MonitorVitalsSurface,
    ) -> Option<Element<'a, Message>> {
        // Keyed on the MACHINE, not the row (issue #156): a tab on
        // `deploy@srv` and the dashboard card for `root@srv` read the
        // same window, so they cannot report the same server twice.
        let sample = self.monitor_sample(&conn_id)?;
        let spark = self
            .monitor_series(&conn_id)
            .map(|s| s.cpu_series())
            .unwrap_or_default();

        // One recorder for the interactive rows, so the walk matches
        // the surface that is actually on screen.
        let nav_row = |action: Message,
                       menu: Option<Message>,
                       el: Element<'a, Message>|
         -> Element<'a, Message> {
            match surface {
                MonitorVitalsSurface::Sidebar => {
                    let mut row = crate::keynav::SidebarRow::button(action);
                    if let Some(menu) = menu {
                        row = row.with_menu(menu);
                    }
                    self.sidebar_nav_slot(row, TerminalSidebarTab::Monitor, 6.0, el)
                }
                MonitorVitalsSurface::Dashboard => self.content_action_slot(
                    crate::keynav::RowAction::activate(action),
                    6.0,
                    el,
                ),
            }
        };

        let mut body = column![].spacing(14).padding(Padding {
            top: 12.0,
            right: 12.0,
            bottom: 12.0,
            left: 12.0,
        });

        // CPU: percentage bar plus a sparkline over the window. The
        // first sample after mount has no percentage (it is a delta), so
        // the gauge says "sampling" rather than showing a fake zero.
        body = body.push(match sample.cpu {
            Some(cpu) => gauge_block(t("monitor_cpu"), cpu.pct, &format!("{:.0}%", cpu.pct)),
            None => pending_block(t("monitor_cpu")),
        });
        if spark.len() > 1 {
            body = body.push(sparkline(&spark));
        }

        if let Some(mem) = sample.mem {
            body = body.push(gauge_block(
                t("monitor_mem"),
                mem.pct(),
                &format!("{} / {}", fmt_bytes(mem.used), fmt_bytes(mem.total)),
            ));
            if mem.swap_total > 0 {
                let pct = (mem.swap_used as f32 / mem.swap_total as f32) * 100.0;
                body = body.push(gauge_block(
                    t("monitor_swap"),
                    pct,
                    &format!("{} / {}", fmt_bytes(mem.swap_used), fmt_bytes(mem.swap_total)),
                ));
            }
        }

        if let Some(load) = sample.load {
            body = body.push(stat_row(
                t("monitor_load"),
                format!("{:.2}  {:.2}  {:.2}", load.one, load.five, load.fifteen),
            ));
            if load.procs_total > 0 {
                body = body.push(stat_row(
                    t("monitor_procs"),
                    format!("{} / {}", load.procs_running, load.procs_total),
                ));
            }
        }

        if let Some(net) = sample.net {
            body = body.push(stat_row(
                t("monitor_net"),
                format!(
                    "↓ {}/s   ↑ {}/s",
                    fmt_bytes(net.rx_bps),
                    fmt_bytes(net.tx_bps)
                ),
            ));
        }

        // GPU gauges (roadmap: host monitoring): rendered only when the
        // probe answered, so a host without a GPU (or without nvidia-smi
        // / the amdgpu sysfs) shows nothing rather than a dead section.
        // The gauge tracks utilization; VRAM and temperature ride in the
        // value line when the device reports them.
        for (i, gpu) in sample.gpus.iter().enumerate() {
            let label = match (&gpu.name, sample.gpus.len() > 1) {
                (Some(name), _) => name.clone(),
                (None, true) => format!("{} {}", t("monitor_gpu"), i),
                (None, false) => t("monitor_gpu").to_string(),
            };
            let mut value = format!("{:.0}%", gpu.util_pct);
            if let (Some(used), Some(total)) = (gpu.mem_used, gpu.mem_total) {
                value.push_str(&format!("   {} / {}", fmt_bytes(used), fmt_bytes(total)));
            }
            if let Some(temp) = gpu.temp_c {
                value.push_str(&format!("   {temp}°C"));
            }
            body = body.push(gauge_block(&label, gpu.util_pct, &value));
        }

        if let Some(up) = sample.uptime_secs {
            body = body.push(stat_row(t("monitor_uptime"), fmt_uptime(up)));
        }

        // Disks (issue #83 follow-up): a disclosure header like the ports
        // below, so a host with many mounts can collapse them. Starts open
        // (`monitor_disks_open`), so the common one-or-two-mount host is
        // unchanged; the count on the header is the affordance either way.
        // A host on Custom (issue #135) whose patterns match nothing
        // keeps the section, with a line saying so: dropping it would
        // read as "this host reports no disks", when what happened is
        // that a `/dat` typo, or a mount that went away, matched none of
        // them. On Auto an empty list really does mean the probe found
        // nothing, and the section stays out of the way.
        let custom_selection = self
            .connections
            .iter()
            .any(|c| c.id == conn_id && c.monitor_disks.is_some());
        if !sample.disks.is_empty() || custom_selection {
            body = body.push(nav_row(
                Message::Monitor(MonitorMessage::ToggleDisks),
                None,
                disks_header(sample.disks.len(), self.monitor_disks_open),
            ));
            if self.monitor_disks_open {
                for disk in &sample.disks {
                    body = body.push(gauge_block(
                        &disk.mount,
                        disk.pct(),
                        &format!("{} / {}", fmt_bytes(disk.used), fmt_bytes(disk.total)),
                    ));
                }
                if sample.disks.is_empty() {
                    body = body.push(
                        container(
                            text(t("monitor_disks_no_match"))
                                .size(11)
                                .color(OryxisColors::t().text_muted),
                        )
                        .padding(Padding { top: 2.0, right: 6.0, bottom: 6.0, left: 8.0 }),
                    );
                }
            }
        }

        // Listening ports (issue #83): collapsed behind a count, since a
        // busy host listens on dozens. Each row offers a local forward,
        // which is the whole point of surfacing them here.
        if !sample.ports.is_empty() {
            body = body.push(nav_row(
                Message::Monitor(MonitorMessage::TogglePorts),
                None,
                ports_header(sample.ports.len(), self.monitor_ports_open),
            ));
            if self.monitor_ports_open {
                for p in &sample.ports {
                    // Only TCP can be tunnelled: SSH port forwarding has
                    // no UDP mode, so a UDP row stays informational
                    // rather than offering an action that would fail.
                    let forward = (p.proto == "tcp").then(|| {
                        Message::Monitor(MonitorMessage::ForwardPort(
                            conn_id,
                            p.port,
                            p.bind.clone(),
                        ))
                    });
                    // The kill menu (issue #96) resolves its session
                    // through the focused pane, so it only exists on
                    // the sidebar surface; the dashboard's rows never
                    // offer an action that would silently no-op.
                    let menu = matches!(surface, MonitorVitalsSurface::Sidebar).then(|| {
                        Message::Monitor(MonitorMessage::ShowPortMenu(Box::new(p.clone())))
                    });
                    let row = port_row(p, forward.clone(), menu.clone());
                    // Enter keeps doing what a left click does (forward
                    // a TCP port); rows with no primary action activate
                    // their menu instead of being a dead stop. A
                    // dashboard UDP row has neither and stays
                    // informational (not recorded).
                    match forward.or_else(|| menu.clone()) {
                        Some(action) => body = body.push(nav_row(action, menu, row)),
                        None => body = body.push(row),
                    }
                }
            }
        }

        // A probe that failed after we already have data: keep the last
        // reading on screen and say so, rather than blanking the tab.
        if let Some(e) = &self.monitor_error {
            body = body.push(
                text(e.clone())
                    .size(11)
                    .color(OryxisColors::t().warning),
            );
        }

        Some(body.into())
    }

    /// Opt-in prompt for a host that hasn't enabled monitoring. The
    /// button is the tab's only keyboard row.
    fn monitor_opt_in(&self, conn_id: uuid::Uuid) -> Element<'_, Message> {
        let btn = crate::widgets::styled_button(
            t("monitor_enable_host"),
            Message::Monitor(MonitorMessage::EnableHost(conn_id)),
            OryxisColors::t().accent,
        );
        column![
            container(
                text(t("monitor_opt_in_hint"))
                    .size(12)
                    .color(OryxisColors::t().text_muted)
            )
            .padding(Padding { top: 24.0, right: 14.0, bottom: 12.0, left: 14.0 }),
            container(self.sidebar_nav_slot(
                crate::keynav::SidebarRow::button(Message::Monitor(
                    MonitorMessage::EnableHost(conn_id),
                )),
                TerminalSidebarTab::Monitor,
                8.0,
                btn,
            ))
            .padding(Padding { top: 0.0, right: 14.0, bottom: 0.0, left: 14.0 }),
        ]
        .width(Length::Fill)
        .into()
    }
}

/// "Ports (N)" disclosure row: clicking it expands the list. The
/// collapsed chevron points along the reading direction, so RTL flips
/// it to the left.
fn ports_header<'a>(count: usize, open: bool) -> Element<'a, Message> {
    let chevron = if open {
        iced_fonts::lucide::chevron_down()
    } else if crate::i18n::is_rtl_layout() {
        iced_fonts::lucide::chevron_left()
    } else {
        iced_fonts::lucide::chevron_right()
    };
    iced::widget::button(
        dir_row(vec![
            chevron.size(12).color(OryxisColors::t().text_muted).into(),
            Space::new().width(6).into(),
            text(t("monitor_ports"))
                .size(11)
                .color(OryxisColors::t().text_secondary)
                .into(),
            Space::new().width(6).into(),
            text(count.to_string())
                .size(11)
                .color(OryxisColors::t().text_muted)
                .into(),
        ])
        .align_y(iced::Alignment::Center),
    )
    .on_press(Message::Monitor(MonitorMessage::TogglePorts))
    .padding(Padding { top: 4.0, right: 6.0, bottom: 4.0, left: 2.0 })
    .width(Length::Fill)
    .style(|_, status| {
        let bg = match status {
            iced::widget::button::Status::Hovered
            | iced::widget::button::Status::Pressed => OryxisColors::t().bg_hover,
            _ => iced::Color::TRANSPARENT,
        };
        iced::widget::button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    })
    .into()
}

/// "Disks (N)" disclosure row (issue #83 follow-up): clicking it
/// expands / collapses the per-mount list, mirroring `ports_header`
/// (RTL-flipped collapsed chevron included).
fn disks_header<'a>(count: usize, open: bool) -> Element<'a, Message> {
    let chevron = if open {
        iced_fonts::lucide::chevron_down()
    } else if crate::i18n::is_rtl_layout() {
        iced_fonts::lucide::chevron_left()
    } else {
        iced_fonts::lucide::chevron_right()
    };
    iced::widget::button(
        dir_row(vec![
            chevron.size(12).color(OryxisColors::t().text_muted).into(),
            Space::new().width(6).into(),
            text(t("monitor_disk"))
                .size(11)
                .color(OryxisColors::t().text_secondary)
                .into(),
            Space::new().width(6).into(),
            text(count.to_string())
                .size(11)
                .color(OryxisColors::t().text_muted)
                .into(),
        ])
        .align_y(iced::Alignment::Center),
    )
    .on_press(Message::Monitor(MonitorMessage::ToggleDisks))
    .padding(Padding { top: 4.0, right: 6.0, bottom: 4.0, left: 2.0 })
    .width(Length::Fill)
    .style(|_, status| {
        let bg = match status {
            iced::widget::button::Status::Hovered
            | iced::widget::button::Status::Pressed => OryxisColors::t().bg_hover,
            _ => iced::Color::TRANSPARENT,
        };
        iced::widget::button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    })
    .into()
}

/// One listening socket: `port/proto`, the process name when the host
/// let us see it, and (TCP only) a click that prefills a local forward.
/// Right-click always opens the row's actions popover (forward, kill,
/// force kill), so a UDP row is a real target too even though it has no
/// primary click.
fn port_row<'a>(
    port: &'a crate::monitor::model::PortStat,
    forward: Option<Message>,
    menu: Option<Message>,
) -> Element<'a, Message> {
    let name = port.process.clone().unwrap_or_else(|| "-".to_string());
    // A specific bind is shown next to the process: it tells the user
    // the service is NOT on every interface, and it is where the
    // click-to-forward rule will point its target.
    let name = match &port.bind {
        Some(bind) => format!("{name}  {bind}"),
        None => name,
    };
    let content = dir_row(vec![
        text(format!("{}/{}", port.port, port.proto))
            .size(11)
            .font(iced::Font::MONOSPACE)
            .color(OryxisColors::t().text_primary)
            .width(Length::Fixed(74.0))
            .into(),
        text(name)
            .size(11)
            .color(OryxisColors::t().text_secondary)
            .width(Length::Fill)
            .into(),
        // The arrow only appears where a forward is actually possible.
        if forward.is_some() {
            iced_fonts::lucide::arrow_right_left()
                .size(11)
                .color(OryxisColors::t().accent)
                .into()
        } else {
            Space::new().width(11).into()
        },
    ])
    .align_y(iced::Alignment::Center);

    let padding = Padding { top: 3.0, right: 8.0, bottom: 3.0, left: 20.0 };
    // A UDP row has no primary click, but where it owns a menu it
    // renders as a button too: without one it would be the only row in
    // the tab with no hover feedback, which reads as disabled. On the
    // dashboard surface (no menu) a UDP row is plain text on purpose:
    // there is genuinely nothing it can do there.
    let body: Element<'a, Message> = match (forward, menu.clone()) {
        (Some(msg), _) => {
            let btn = iced::widget::button(content)
                .on_press(msg)
                .padding(padding)
                .width(Length::Fill)
                .style(port_row_style);
            crate::views::terminal::icon_tooltip(btn.into(), t("monitor_forward_port"))
        }
        (None, Some(menu_msg)) => iced::widget::button(content)
            .on_press(menu_msg)
            .padding(padding)
            .width(Length::Fill)
            .style(port_row_style)
            .into(),
        (None, None) => container(content).padding(padding).width(Length::Fill).into(),
    };
    match menu {
        Some(menu) => iced::widget::MouseArea::new(body).on_right_press(menu).into(),
        None => body,
    }
}

/// Shared hover/press feedback for a port row.
fn port_row_style(
    _: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let bg = match status {
        iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed => {
            OryxisColors::t().bg_hover
        }
        _ => iced::Color::TRANSPARENT,
    };
    iced::widget::button::Style {
        background: Some(Background::Color(bg)),
        border: Border { radius: Radius::from(6.0), ..Default::default() },
        ..Default::default()
    }
}

fn placeholder(label: &str) -> Element<'_, Message> {
    container(text(label.to_string()).size(12).color(OryxisColors::t().text_muted))
        .center_x(Length::Fill)
        .padding(Padding { top: 40.0, right: 12.0, bottom: 0.0, left: 12.0 })
        .width(Length::Fill)
        .into()
}

/// Label + value line above a filled bar. The fill colour follows the
/// theme's semantic colours so a host in trouble reads as such at a
/// glance.
// `label` is converted to an owned string right away (as is `value`), so
// the returned element's lifetime is NOT tied to it: callers can pass a
// loop-local `String` (the GPU gauges build "GPU 0" labels per device).
pub(crate) fn gauge_block<'a>(label: &str, pct: f32, value: &str) -> Element<'a, Message> {
    let pct = pct.clamp(0.0, 100.0);
    let fill = if pct >= 90.0 {
        OryxisColors::t().error
    } else if pct >= 75.0 {
        OryxisColors::t().warning
    } else {
        OryxisColors::t().accent
    };
    // The old note here claimed a sub-1% gauge "renders empty, which is the
    // honest reading anyway". It rendered FULL: a weightless
    // `FillPortion(0)` takes the whole track in iced instead of vanishing,
    // so an idle host showed a saturated bar and a pegged one showed an
    // empty bar. `progress_track` omits the weightless side (issue #107 is
    // the same defect on the transfer bar).
    let bar = crate::widgets::progress_track(
        pct / 100.0,
        6.0,
        fill,
        OryxisColors::t().bg_surface,
    );
    column![
        dir_row(vec![
            text(label.to_string())
                .size(11)
                .color(OryxisColors::t().text_secondary)
                .width(Length::Fill)
                .into(),
            text(value.to_string())
                .size(11)
                .color(OryxisColors::t().text_primary)
                .into(),
        ])
        .align_y(iced::Alignment::Center),
        Space::new().height(4),
        bar,
    ]
    .width(Length::Fill)
    .into()
}

/// A metric that needs a second sample before it can be reported.
fn pending_block<'a>(label: &'a str) -> Element<'a, Message> {
    dir_row(vec![
        text(label.to_string())
            .size(11)
            .color(OryxisColors::t().text_secondary)
            .width(Length::Fill)
            .into(),
        text(t("monitor_sampling"))
            .size(11)
            .color(OryxisColors::t().text_muted)
            .into(),
    ])
    .align_y(iced::Alignment::Center)
    .into()
}

pub(crate) fn stat_row<'a>(label: &'a str, value: String) -> Element<'a, Message> {
    dir_row(vec![
        text(label.to_string())
            .size(11)
            .color(OryxisColors::t().text_secondary)
            .width(Length::Fill)
            .into(),
        text(value)
            .size(11)
            .color(OryxisColors::t().text_primary)
            .font(iced::Font::MONOSPACE)
            .into(),
    ])
    .align_y(iced::Alignment::Center)
    .into()
}

/// CPU history as a row of bars, oldest to newest. A canvas would be
/// smoother but this reuses the gauge's vocabulary and costs nothing.
pub(crate) fn sparkline<'a>(series: &[f32]) -> Element<'a, Message> {
    // Only the tail fits the sidebar's width at a readable bar size.
    let tail = series.len().saturating_sub(40);
    let bars: Vec<Element<'a, Message>> = series[tail..]
        .iter()
        .map(|pct| {
            let h = (pct.clamp(0.0, 100.0) / 100.0 * 24.0).max(1.0);
            container(
                container(Space::new().width(Length::Fill).height(Length::Fixed(h)))
                    .style(|_| container::Style {
                        background: Some(Background::Color(OryxisColors::t().accent)),
                        border: Border { radius: Radius::from(1.0), ..Default::default() },
                        ..Default::default()
                    }),
            )
            .height(Length::Fixed(24.0))
            .width(Length::Fill)
            .align_y(iced::alignment::Vertical::Bottom)
            .into()
        })
        .collect();
    container(iced::widget::Row::with_children(bars).spacing(1))
        .width(Length::Fill)
        .height(Length::Fixed(24.0))
        .into()
}

/// `fmt_bytes` without the space, for the status bar where horizontal
/// room is scarce.
pub(crate) fn fmt_bytes_short(bytes: u64) -> String {
    fmt_bytes(bytes).replace(' ', "")
}

/// Human-readable byte count (1024-based, matching what `free` / `df`
/// report on the hosts these numbers come from).
fn fmt_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Uptime as the coarsest useful unit, the way `uptime(1)` reads.
pub(crate) fn fmt_uptime(secs: u64) -> String {
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let minutes = (secs % 3_600) / 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_scale_to_readable_units() {
        assert_eq!(fmt_bytes(512), "512 B");
        assert_eq!(fmt_bytes(1024), "1.0 KB");
        assert_eq!(fmt_bytes(1024 * 1024 * 3 / 2), "1.5 MB");
        // Three digits drop the decimal so the column stays narrow.
        assert_eq!(fmt_bytes(1024 * 1024 * 512), "512 MB");
        assert_eq!(fmt_bytes(1024u64.pow(4)), "1.0 TB");
    }

    #[test]
    fn uptime_reads_like_uptime_1() {
        assert_eq!(fmt_uptime(45), "0m");
        assert_eq!(fmt_uptime(3_600 + 120), "1h 2m");
        assert_eq!(fmt_uptime(86_400 * 4 + 3_600 * 5), "4d 5h");
    }
}
