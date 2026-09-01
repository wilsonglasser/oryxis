//! Sidebar Monitor tab (agentless host vitals) messages, issue #83.

use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum MonitorMessage {
    /// Periodic poll: probe every monitored host that has a live session
    /// and an open Monitor tab. Mounted only while such a tab is
    /// visible, so idle screens never touch the network.
    PollHosts,
    /// A probe returned: the raw batched payload, or an error to
    /// surface on the card. The `MonitorKey` is the MACHINE the sample
    /// belongs to and is carried rather than re-derived, so an edit to
    /// the row mid-probe can't land the answer in a different window
    /// (or leak the in-flight guard); the `Uuid` is the row that ran
    /// the probe, for the error text and the alert's label. The `u64`
    /// is the request stamp captured at dispatch; a mismatch means the
    /// pane reconnected (or monitoring was turned off) while the probe
    /// was in flight and the result is dropped.
    Sampled(
        crate::monitor::endpoint::MonitorKey,
        Uuid,
        u64,
        Result<String, String>,
    ),
    /// Turn monitoring on for the host behind the focused pane, from the
    /// Monitor tab's own opt-in prompt (the same flag the host editor
    /// toggles).
    EnableHost(Uuid),
    /// Expand / collapse the listening-ports list in the Monitor tab.
    TogglePorts,
    /// Expand / collapse the disk list in the Monitor tab (issue #83
    /// follow-up: many mounts crowd the sidebar).
    ToggleDisks,
    /// "Forward this port": prefill a `-L` rule for the monitored host
    /// and open the port-forward editor, so the user reviews and saves
    /// it instead of a tunnel appearing behind their back. The `Option`
    /// is the listener's bind address (`None` = wildcard): a service
    /// bound to a specific address only answers there, so it becomes
    /// the rule's target host instead of 127.0.0.1.
    ForwardPort(Uuid, u16, Option<String>),
    /// Right-click (or the Menu key) on a port row: open its actions
    /// popover. Carries the whole row so the menu and the confirmation
    /// it opens describe exactly the socket the user pointed at,
    /// instead of re-looking it up in a sample that may have rolled
    /// over in between.
    ShowPortMenu(Box<crate::monitor::model::PortStat>),
    /// "Kill process" / "Force kill" from that menu: park the
    /// confirmation. Nothing reaches the host until it is confirmed.
    AskKillPort(Box<crate::monitor::model::PortStat>, crate::monitor::kill::KillSignal),
    /// Run the parked kill.
    ConfirmKillPort,
    /// Re-run it escalated, offered after a failure sudo could fix.
    RetryKillWithSudo,
    /// Dismiss the confirmation without touching the host.
    CancelKillPort,
    /// A kill run came back. The `u64` is the monitor request stamp
    /// captured at dispatch: a mismatch means the pane reconnected (or
    /// monitoring was swept) while the run was in flight, so the result
    /// belongs to state that no longer exists.
    KillFinished(u64, crate::monitor::kill::KillOutcome),
    /// One-second heartbeat of the multi-host dashboard (issue #95).
    /// Mounted only while the Monitoring view is up: drives the
    /// per-host probe stagger, dials missing links and redials dead
    /// ones.
    DashTick,
    /// A dashboard dial finished: the MACHINE it belongs to and the
    /// row it authenticated as (issue #156, one link per machine). The
    /// `u64` is `monitor_dash.stamp` captured at dispatch: a mismatch
    /// means a sweep (lock, toggle off, idle TTL) ran meanwhile, so a
    /// `Live` result is closed instead of stored.
    DashDialed(
        crate::monitor::endpoint::MonitorKey,
        Uuid,
        u64,
        Result<std::sync::Arc<oryxis_ssh::MonitorConn>, String>,
    ),
    /// Retry a failed card (the only retry path besides re-entering
    /// the view: the dashboard never hammers a down host on its own).
    /// Dials the card's OWN row: the cards of one machine differ in
    /// credentials, and retrying from a card means "try this one".
    DashRetry(Uuid),
    /// The idle TTL armed on leaving the Monitoring view expired. If
    /// the view is up again by then the pooled links survive (that is
    /// the point of the TTL: a quick round-trip elsewhere shouldn't
    /// redial the fleet); otherwise the links are swept and their
    /// dialed connections closed. The stamp de-duplicates timers: the
    /// sweep bumps it, so a second armed timer lands on a stale stamp
    /// and no-ops.
    DashSweepDue(u64),
    /// Open the host's terminal from the detail panel's explicit
    /// action: focus an existing tab when one is connected to the
    /// host, otherwise start the normal connect flow.
    DashOpenHost(Uuid),
    /// Card click: open the host's detail panel on the trailing edge.
    DashSelectHost(Uuid),
    /// Close the detail panel.
    DashCloseDetail,
    /// The view's search field (display-only filter over the cards).
    DashSearchChanged(String),
    /// Grid <-> list layout toggle, persisted like the host grid's.
    DashToggleListView,
    /// Table-mode header click: sort by this column, toggling the
    /// direction when it is already the active one.
    DashSortBy(crate::state::DashSortKey),
    /// Pause / resume the fleet's polling (issue #156 follow-up).
    /// Session-scoped and fleet-scoped: the sidebar Monitor tab reads
    /// over the session its own tab already holds and keeps going.
    DashTogglePause,
    /// "Refresh now": one sample from every machine on the board. The
    /// single-shot monitor while paused, since the connections it
    /// opens are closed again as soon as the sample lands.
    DashRefreshNow,
}
