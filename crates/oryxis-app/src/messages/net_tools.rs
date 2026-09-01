//! Network tools panel messages, wrapped by
//! [`crate::messages::Message::NetTools`]. Handled by
//! `Oryxis::handle_net_tools` (`dispatch_net_tools.rs`).

use crate::net_tools::{NetTool, NetToolCard};

/// Drives the optional network tools surface (Settings > Advanced >
/// Network tools). Every variant here is unreachable while the feature
/// is off: the tab cannot be opened, so nothing can send them.
#[derive(Debug, Clone)]
pub enum NetToolsMessage {
    /// Pick the tool to run. Clears the previous run's results, which
    /// belong to a different question.
    Select(NetTool),
    /// Target field edited (a host name, URL or address, per tool).
    Target(String),
    /// Port field edited (port test only).
    Ports(String),
    /// Run the selected tool against the current target.
    Run,
    /// A run finished. The `u64` is the run's sequence number, checked
    /// against the panel's own before the result is shown: a run the
    /// user replaced must not overwrite the one they are waiting for.
    Finished(u64, Result<Vec<NetToolCard>, String>),
    /// Stop waiting for the in-flight run. The request itself cannot be
    /// recalled; this drops the panel's interest in the answer.
    Cancel,
    /// Copy one result card to the clipboard (index into `cards`).
    CopyCard(usize),
    /// Pointer entered / left a result card, for the hover-revealed copy
    /// action. Named `Result*` rather than `Card*` because
    /// `TabsMessage` already owns a `CardHovered(usize)`: two sub-enums
    /// declaring one name with one payload compile at every send site,
    /// so the pair would be a wrong-wrapper landmine.
    ResultHovered(usize),
    ResultUnhovered(usize),
}
