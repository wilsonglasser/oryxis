//! `Oryxis::handle_monitor`: the sidebar Monitor tab's polling loop
//! (issue #83, plan J2).
//!
//! Probes run on an exec channel multiplexed on the focused pane's LIVE
//! SSH session, so no extra connection is opened and the host is only
//! ever read from. Parsing lives in `crate::monitor::probe`, which is
//! pure and unit-tested; this file owns the scheduling, the in-flight
//! guard and the stale-result rules.

use iced::Task;
use uuid::Uuid;

use crate::app::{Message, MonitorMessage, Oryxis};

/// Localized toast copy for a crossed threshold.
fn breach_message(host: &str, breach: &crate::monitor::alert::Breach) -> String {
    use crate::monitor::alert::Breach;
    let key = match breach {
        Breach::Cpu => "monitor_alert_cpu",
        Breach::Mem => "monitor_alert_mem",
        Breach::Disk(_) => "monitor_alert_disk",
    };
    let text = crate::i18n::t(key).replacen("{host}", host, 1);
    match breach {
        Breach::Disk(mount) => text.replacen("{mount}", mount, 1),
        _ => text,
    }
}

/// Cap on a single probe. Long enough for a loaded host to answer, short
/// enough that a wedged one frees its in-flight slot before the user
/// gives up on the tab.
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);

/// Default seconds between polls. Frequent enough to feel live, sparse
/// enough that one exec channel per interval is negligible next to an
/// interactive shell.
pub(crate) const MONITOR_INTERVAL_DEFAULT_SECS: u64 = 5;

/// Floor on the configured interval. Below this the probes start
/// overlapping their own round trips on a slow link, which costs the
/// host more than the readings are worth.
const MONITOR_INTERVAL_FLOOR_SECS: u64 = 2;

impl Oryxis {
    pub(crate) fn handle_monitor(&mut self, message: MonitorMessage) -> Task<Message> {
        match message {
            MonitorMessage::PollHosts => self.monitor_probe_active_pane(),
            MonitorMessage::Sampled(key, conn_id, stamp, result) => {
                self.monitor.probing.remove(&key);
                // A reconnect (or monitoring turned off) while the probe
                // was in flight bumps the stamp; that result belongs to a
                // series that no longer exists.
                if stamp != self.monitor_stamp {
                    return Task::none();
                }
                match result {
                    Ok(payload) => {
                        // The disk selection (issue #135) is applied
                        // HERE, before the sample enters the ring, so
                        // every reader inherits it: sidebar, dashboard,
                        // status bar, and the threshold alerts below,
                        // which must never announce a mount the user
                        // chose not to monitor. The window belongs to
                        // the MACHINE (issue #156), so the selection is
                        // the union over the rows that monitor it.
                        let patterns = self.monitor_disk_patterns(&key);
                        let series = self.monitor.series.entry(key).or_default();
                        let (mut sample, snapshot) = crate::monitor::probe::parse_linux(
                            &payload,
                            series.raw_prev,
                            std::time::Instant::now(),
                        );
                        sample.disks = crate::monitor::disks::select_disks(
                            patterns.as_deref(),
                            std::mem::take(&mut sample.disks),
                        );
                        series.push(sample, snapshot);
                        // Threshold check on the fresh window. Rising
                        // edge only, so a pegged host is announced once
                        // per crossing; foreground toasts by owner
                        // constraint, never background alerting.
                        let recent = series.tail(3);
                        let (flags, breaches) =
                            crate::monitor::alert::evaluate(&recent, series.breached);
                        series.breached = flags;
                        if !breaches.is_empty() {
                            // One toast per crossing per MACHINE, named
                            // after the row it was read through: three
                            // rows on one server would otherwise raise
                            // three toasts about the same disk.
                            let host = self
                                .connections
                                .iter()
                                .find(|c| c.id == conn_id)
                                .map(|c| c.label.clone())
                                .unwrap_or_default();
                            let mut tasks: Vec<Task<Message>> = Vec::new();
                            for b in breaches {
                                tasks.push(self.show_toast_secs(breach_message(&host, &b), 8));
                            }
                            self.monitor_error = None;
                            return Task::batch(tasks);
                        }
                    }
                    Err(e) => {
                        // Keep whatever the window already holds (the last
                        // good reading stays on screen) and surface the
                        // failure; the next tick retries.
                        self.monitor_error = Some(e);
                        return Task::none();
                    }
                }
                self.monitor_error = None;
                Task::none()
            }
            MonitorMessage::EnableHost(conn_id) => {
                if let Some(conn) = self.connections.iter_mut().find(|c| c.id == conn_id) {
                    conn.monitor_enabled = true;
                    let conn = conn.clone();
                    // A failed persist must be loud: the flag would work
                    // until restart and then silently vanish.
                    if let Some(vault) = &self.vault
                        && let Err(e) = vault.save_connection(&conn, None)
                    {
                        return self.show_toast_secs(e.to_string(), 6);
                    }
                    // Probe immediately so the tab fills in instead of
                    // waiting out a whole interval on an empty card.
                    return self.monitor_probe_active_pane();
                }
                Task::none()
            }
            MonitorMessage::TogglePorts => {
                self.monitor_ports_open = !self.monitor_ports_open;
                Task::none()
            }
            MonitorMessage::ToggleDisks => {
                self.monitor_disks_open = !self.monitor_disks_open;
                Task::none()
            }
            MonitorMessage::ForwardPort(conn_id, port, bind) => {
                // Prefill a local forward onto the same port and hand the
                // user the editor. The target is dialed FROM THE SERVER:
                // a wildcard or loopback listener answers on 127.0.0.1,
                // but one bound to a specific address only answers THERE,
                // so that address becomes the target instead of a
                // 127.0.0.1 that would dial a closed port.
                let target = match bind.as_deref() {
                    Some(addr) if addr != "127.0.0.1" && addr != "::1" => addr.to_string(),
                    _ => "127.0.0.1".to_string(),
                };
                let label = self
                    .connections
                    .iter()
                    .find(|c| c.id == conn_id)
                    .map(|c| format!("{} :{port}", c.label))
                    .unwrap_or_else(|| format!(":{port}"));
                self.panels.port_forward_panel = true;
                self.port_forward_form.editing_id = None;
                self.port_forward_form.label = label;
                self.port_forward_form.kind =
                    oryxis_core::models::port_forward_rule::ForwardKind::Local;
                self.port_forward_form.host_id = Some(conn_id);
                self.port_forward_form.listen_host = "127.0.0.1".into();
                self.port_forward_form.listen_port = port.to_string();
                self.port_forward_form.target_host = target;
                self.port_forward_form.target_port = port.to_string();
                self.port_forward_form.auto_start = false;
                self.port_forward_form.error = None;
                // The editor lives in the Port Forwarding view, so the
                // click navigates there: the rule is reviewed and saved
                // deliberately, never created silently.
                Task::done(Message::Navigation(
                    crate::app::NavigationMessage::ChangeView(
                        crate::state::View::PortForwarding,
                    ),
                ))
            }
            MonitorMessage::ShowPortMenu(port) => {
                let (x, y) = self.keynav_take_menu_anchor();
                self.overlay = Some(crate::state::OverlayState {
                    content: crate::state::OverlayContent::MonitorPortActions(port),
                    x,
                    y,
                });
                Task::none()
            }
            MonitorMessage::AskKillPort(port, signal) => {
                self.overlay = None;
                // The menu can only be open over a monitored pane, but
                // the lookup is redone here rather than trusted: a
                // disconnect between the right-click and the menu click
                // must leave the dialog unopened, not pointed at a dead
                // session.
                let Some(conn_id) = self.monitor_pane_connection() else {
                    return Task::none();
                };
                let host = self
                    .connections
                    .iter()
                    .find(|c| c.id == conn_id)
                    .map(|c| c.label.clone())
                    .unwrap_or_default();
                self.monitor.kill = Some(crate::monitor::kill::PendingKill::new(
                    conn_id, host, &port, signal,
                ));
                Task::none()
            }
            MonitorMessage::ConfirmKillPort => self.monitor_run_kill(false),
            MonitorMessage::RetryKillWithSudo => self.monitor_run_kill(true),
            MonitorMessage::CancelKillPort => {
                self.monitor.kill = None;
                Task::none()
            }
            // Multi-host dashboard group (issue #95): routed wholesale
            // to its own file, exhaustive there.
            m @ (MonitorMessage::DashTick
            | MonitorMessage::DashDialed(..)
            | MonitorMessage::DashRetry(..)
            | MonitorMessage::DashSweepDue(..)
            | MonitorMessage::DashOpenHost(..)
            | MonitorMessage::DashSelectHost(..)
            | MonitorMessage::DashCloseDetail
            | MonitorMessage::DashSearchChanged(..)
            | MonitorMessage::DashToggleListView
            | MonitorMessage::DashSortBy(..)
            | MonitorMessage::DashTogglePause
            | MonitorMessage::DashRefreshNow) => self
                .handle_monitor_dash(m)
                .unwrap_or_else(crate::dispatch::unrouted),
            MonitorMessage::KillFinished(stamp, outcome) => {
                // Same rule as `Sampled`: a reconnect or a sweep while
                // the run was in flight invalidates everything the
                // result would land in.
                if stamp != self.monitor_stamp {
                    return Task::none();
                }
                let settled = outcome.is_settled();
                let message = outcome.message();
                if !settled
                    && let Some(pending) = self.monitor.kill.as_mut()
                {
                    // Park the failure ON the dialog: "no PID even with
                    // sudo" or "sudo refused" is something the user has
                    // to read and possibly retry, not a toast that
                    // scrolls away.
                    let changed =
                        matches!(outcome, crate::monitor::kill::KillOutcome::Changed { .. });
                    pending.phase = crate::monitor::kill::KillPhase::Failed(outcome);
                    // "Refresh and try again" has to be actionable: the
                    // list still shows the PID we just refused, so pull
                    // a fresh sample now instead of making the user wait
                    // out a tick to see the owner that replaced it.
                    return if changed {
                        self.monitor_probe_active_pane()
                    } else {
                        Task::none()
                    };
                }
                // Settled, or the user dismissed the dialog while the
                // run was on the wire: report it and get out of the way.
                self.monitor.kill = None;
                let toast = self.show_toast_secs(message, 6);
                if settled {
                    // Refresh so the killed port leaves the list. The
                    // in-flight guard may skip this one; the next tick
                    // covers it either way, so no bypass is needed.
                    Task::batch([toast, self.monitor_probe_active_pane()])
                } else {
                    toast
                }
            }
        }
    }

    /// Run (or re-run, escalated) the parked kill on the focused pane's
    /// session. `escalate` is the "Retry with sudo" path; the initial
    /// run inherits whatever `PendingKill::new` decided.
    fn monitor_run_kill(&mut self, escalate: bool) -> Task<Message> {
        let Some(pending) = self.monitor.kill.as_mut() else {
            return Task::none();
        };
        // A second Enter while the first run is on the wire would put a
        // second signal behind it.
        if pending.phase == crate::monitor::kill::KillPhase::Running {
            return Task::none();
        }
        if escalate {
            pending.sudo = true;
        }
        pending.phase = crate::monitor::kill::KillPhase::Running;
        let (conn_id, port, proto, pid, signal, sudo) = (
            pending.conn_id,
            pending.port,
            pending.proto,
            pending.pid,
            pending.signal,
            pending.sudo,
        );

        // The dialog blocks input, so the focused pane can't have moved
        // under it; what CAN happen is the session dying meanwhile.
        let Some((target_id, session)) = self.monitor_target().filter(|(id, _)| *id == conn_id)
        else {
            if let Some(pending) = self.monitor.kill.as_mut() {
                pending.phase = crate::monitor::kill::KillPhase::Failed(
                    crate::monitor::kill::KillOutcome::Unreachable,
                );
            }
            return Task::none();
        };
        debug_assert_eq!(target_id, conn_id);

        // Read the stored host password only on the escalated path, and
        // only to hand it to the runner, which feeds it on stdin. It is
        // never logged, never shown, and never part of a command line.
        let password = sudo
            .then(|| {
                self.vault
                    .as_ref()
                    .and_then(|v| v.get_connection_password(&conn_id).ok().flatten())
            })
            .flatten();
        let stamp = self.monitor_stamp;
        Task::perform(
            crate::monitor::kill::run_kill(session, port, proto, pid, signal, sudo, password),
            move |outcome| Message::Monitor(MonitorMessage::KillFinished(stamp, outcome)),
        )
    }

    /// Probe the focused pane's host, when it is monitored, connected and
    /// not already being probed. Called from the tick and right after the
    /// user opts a host in.
    fn monitor_probe_active_pane(&mut self) -> Task<Message> {
        let Some((conn_id, session)) = self.monitor_target() else {
            return Task::none();
        };
        let Some(key) = self.monitor_key(&conn_id) else {
            return Task::none();
        };
        // A slow host is skipped rather than queueing probes behind each
        // other: the previous one is still holding a channel. Keyed by
        // MACHINE, so this also stops the dashboard from probing the
        // same server the sidebar is already reading (issue #156).
        if !self.monitor.probing.insert(key.clone()) {
            return Task::none();
        }
        let stamp = self.monitor_stamp;
        let command = crate::monitor::probe::linux_probe_command();
        Task::perform(
            async move {
                match session.probe(&command, PROBE_TIMEOUT).await {
                    Some(payload) => Ok(payload),
                    None => Err(crate::i18n::t("monitor_probe_failed").to_string()),
                }
            },
            move |result| {
                Message::Monitor(MonitorMessage::Sampled(key.clone(), conn_id, stamp, result))
            },
        )
    }

    /// The focused pane's `(connection id, live session)` when that host
    /// has monitoring enabled. `None` for local / ephemeral panes, hosts
    /// that never opted in, and dead sessions.
    pub(crate) fn monitor_target(&self) -> Option<(Uuid, std::sync::Arc<oryxis_ssh::SshSession>)> {
        if !self.prefs.host_monitoring {
            return None;
        }
        let conn_id = self.monitor_pane_connection()?;
        if !self.monitor_host_opted_in(&conn_id) {
            return None;
        }
        let idx = self.active_tab?;
        let pane = self.tabs.get(idx)?.active();
        let ssh = pane.session.as_ref().and_then(|s| s.ssh())?;
        ssh.is_alive().then(|| (conn_id, ssh.clone()))
    }

    /// Effective monitoring opt-in for a host: the global "all hosts"
    /// toggle OR the per-host flag. Shared by the probe target and the
    /// status-bar segment, so switching a host's flag off stops the
    /// RENDER as well as the probing (a lingering series must not keep
    /// painting frozen vitals as if they were live).
    pub(crate) fn monitor_host_opted_in(&self, conn_id: &Uuid) -> bool {
        self.connections
            .iter()
            .any(|c| c.id == *conn_id && self.monitor_conn_opted_in(c))
    }

    /// The same rule against a row already in hand.
    ///
    /// The protocol clamp is here rather than at each caller: probing
    /// reads `/proc` over an SSH exec channel, so a Telnet, serial or
    /// remote-desktop row can never be monitored, and the "all hosts"
    /// toggle must not sweep one into the fleet (where the dashboard
    /// would dial it as SSH). The host editor clamps `monitor_enabled`
    /// the same way on save; this covers the rows that arrive by sync,
    /// import or a protocol change made elsewhere.
    pub(crate) fn monitor_conn_opted_in(&self, conn: &oryxis_core::models::Connection) -> bool {
        conn.protocol == oryxis_core::models::connection::ConnectionProtocol::Ssh
            && (self.prefs.monitor_all_hosts || conn.monitor_enabled)
    }

    /// Connection id behind the focused pane, if it is a saved host.
    /// Quick-connect / local / cloud panes have no vault row to carry the
    /// opt-in flag, so they can't be monitored.
    pub(crate) fn monitor_pane_connection(&self) -> Option<Uuid> {
        let idx = self.active_tab?;
        match self.tabs.get(idx)?.active().origin {
            crate::state::PaneOrigin::Host(id) => Some(id),
            _ => None,
        }
    }

    /// Effective probe interval: the configured value, floored so a
    /// typo (or an empty field mid-edit) can't hammer the host.
    pub(crate) fn monitor_interval_secs(&self) -> u64 {
        self.prefs.monitor_interval
            .trim()
            .parse::<u64>()
            .ok()
            .filter(|s| *s > 0)
            .unwrap_or(MONITOR_INTERVAL_DEFAULT_SECS)
            .max(MONITOR_INTERVAL_FLOOR_SECS)
    }

    /// True while the Monitor tab is the visible sidebar tab, which is
    /// what mounts the tick: monitoring never polls a screen nobody is
    /// looking at.
    pub(crate) fn monitor_tab_visible(&self) -> bool {
        self.sidebar_tab_shown(crate::state::TerminalSidebarTab::Monitor)
    }

    /// Does any pane still sit on the machine behind this host?
    ///
    /// The tab / pane close paths drop a host's window once its last
    /// pane goes, and since the window is per machine (issue #156) the
    /// question has to be asked about the machine: closing the
    /// `deploy@srv` tab must not blank the vitals the `root@srv` tab
    /// next to it is still filling. `skip_tab` excludes the tab that is
    /// closing, whose panes are still in the grid at that point.
    pub(crate) fn monitor_machine_in_panes(&self, conn_id: &Uuid, skip_tab: Option<usize>) -> bool {
        let Some(key) = self.monitor_key(conn_id) else {
            return false;
        };
        self.tabs.iter().enumerate().any(|(i, tab)| {
            Some(i) != skip_tab
                && tab.pane_grid.panes.values().any(|p| {
                    matches!(p.origin, crate::state::PaneOrigin::Host(id)
                        if self.monitor_key(&id).is_some_and(|k| k == key))
                })
        })
    }

    /// Drop the window of the MACHINE behind a host (disconnect,
    /// monitoring turned off, a disk selection edited) and invalidate
    /// any probe still in flight for it.
    ///
    /// The window is per machine since issue #156, so a reset reached
    /// through one row drops what its siblings on the same server were
    /// reading too. That is the right answer for every caller: the
    /// resets exist because the window is no longer trustworthy (the
    /// selection changed, the session died), which is a fact about the
    /// machine, not about the row. Whoever still monitors it rebuilds
    /// the window on the next tick.
    pub(crate) fn monitor_reset_host(&mut self, conn_id: &Uuid) {
        let Some(key) = self.monitor_key(conn_id) else {
            return;
        };
        self.monitor_reset_key(&key, conn_id);
    }

    /// `monitor_reset_host` for a key resolved by the caller: the host
    /// editor's, taken from the row BEFORE the save, since the edit can
    /// move the row to another machine and the window to drop is the
    /// one it was filling until now.
    pub(crate) fn monitor_reset_key(
        &mut self,
        key: &crate::monitor::endpoint::MonitorKey,
        conn_id: &Uuid,
    ) {
        self.monitor.forget(key, conn_id);
        self.monitor_stamp = self.monitor_stamp.wrapping_add(1);
        self.monitor_error = None;
    }

    /// Drop EVERY host's window and invalidate all in-flight probes.
    /// Used by the feature toggle-off and the vault-lock sweeps: without
    /// the stamp bump, a probe already in flight would land after the
    /// sweep and repopulate the state it just cleared (and could fire a
    /// first-sample threshold toast right after the user turned the
    /// feature off).
    pub(crate) fn monitor_reset_all(&mut self) {
        self.monitor = Default::default();
        self.monitor_stamp = self.monitor_stamp.wrapping_add(1);
        self.monitor_error = None;
        // The dashboard rides the same sweeps: its dialed connections
        // close and in-flight dials/probes land on a dead stamp.
        self.monitor_dash.sweep();
    }
}

#[cfg(test)]
mod tests {
    /// The interval resolver's contract, exercised without an `Oryxis`
    /// (the parse + floor is the whole rule; the struct only supplies
    /// the string).
    fn resolve(raw: &str) -> u64 {
        raw.trim()
            .parse::<u64>()
            .ok()
            .filter(|s| *s > 0)
            .unwrap_or(super::MONITOR_INTERVAL_DEFAULT_SECS)
            .max(2)
    }

    #[test]
    fn interval_falls_back_and_floors() {
        assert_eq!(resolve("10"), 10);
        // Empty / half-typed / non-numeric fall back to the default
        // rather than freezing the tick at zero.
        assert_eq!(resolve(""), super::MONITOR_INTERVAL_DEFAULT_SECS);
        assert_eq!(resolve("   "), super::MONITOR_INTERVAL_DEFAULT_SECS);
        assert_eq!(resolve("abc"), super::MONITOR_INTERVAL_DEFAULT_SECS);
        // "0" would be a busy loop against the host; the floor catches
        // it and every sub-floor value.
        assert_eq!(resolve("0"), super::MONITOR_INTERVAL_DEFAULT_SECS);
        assert_eq!(resolve("1"), 2);
        assert_eq!(resolve("2"), 2);
    }
}
