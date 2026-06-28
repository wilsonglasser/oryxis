pub mod config;
pub mod conflict;
#[cfg(test)]
mod tests;
pub mod crypto;
pub mod discovery;
pub mod engine;
pub mod error;
pub mod peer;
pub mod protocol;
pub mod relay;
pub mod transport;

pub use config::{SyncConfig, SyncMode, SyncTransport};
pub use engine::{
    build_full_snapshot, format_pairing_link, merge_snapshot, parse_pairing_link, SyncEngine,
    SyncEvent, SyncHandle,
};
pub use error::SyncError;
pub use crypto::DeviceIdentity;
pub use peer::{PeerInfo, PeerStatus, SyncPeer};
