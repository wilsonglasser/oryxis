pub mod mdns;
pub mod signaling;
pub mod stun;

use uuid::Uuid;

/// How a peer was discovered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryMethod {
    Lan,
    Signaling,
}

/// A discovered peer endpoint.
#[derive(Debug, Clone)]
pub struct DiscoveredPeer {
    pub device_id: Uuid,
    pub addr: std::net::SocketAddr,
    pub method: DiscoveryMethod,
}
