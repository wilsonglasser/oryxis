//! State for the network tools panel (Settings > Advanced > Network
//! tools, off by default).

use crate::net_tools::{NetTool, NetToolCard};

#[derive(Debug, Default)]
pub(crate) struct NetToolsState {
    pub tool: NetTool,
    pub target: String,
    /// Port list for the port test. Kept across tool switches on
    /// purpose: someone alternating between "is the port open" and "does
    /// the name resolve" should not retype it every time.
    pub ports: String,
    /// The run currently in flight, if any. Holding the number rather
    /// than a bool is what lets a finished run prove it is still the
    /// current one (see `seq`).
    pub running: Option<u64>,
    pub cards: Vec<NetToolCard>,
    /// Why the run could not start. Distinct from a card: this is the
    /// panel failing to ask the question, not the network answering it.
    pub error: Option<String>,
    /// What the last completed run was about, so the results keep a
    /// heading after the inputs are edited for the next one.
    pub last_run: Option<String>,
    /// Monotonic run counter. A result carrying anything but the current
    /// value belongs to a run the user has already replaced, and is
    /// dropped: cancelling cannot un-send a request that is already out.
    pub seq: u64,
}

impl NetToolsState {
    /// Start a run and return its sequence number.
    pub(crate) fn begin(&mut self) -> u64 {
        self.seq = self.seq.wrapping_add(1);
        self.running = Some(self.seq);
        self.error = None;
        self.cards.clear();
        self.last_run = Some(self.heading());
        self.seq
    }

    /// Whether a finished run is the one being waited for.
    pub(crate) fn is_current(&self, seq: u64) -> bool {
        self.running == Some(seq)
    }

    /// What the run is being done against, for the results heading.
    pub(crate) fn heading(&self) -> String {
        let target = self.target.trim();
        if self.tool.needs_ports() && !self.ports.trim().is_empty() {
            return format!("{target}   {}", self.ports.trim());
        }
        target.to_string()
    }

    /// Drop everything a run produced. Called when the panel's own tab
    /// closes and when the feature is switched off, so nothing a probe
    /// returned outlives the surface that asked for it.
    pub(crate) fn reset(&mut self) {
        self.running = None;
        self.cards.clear();
        self.error = None;
        self.last_run = None;
    }
}
