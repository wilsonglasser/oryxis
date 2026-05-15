//! Wire protocol for Oryxis cloud-provider plugins.
//!
//! Cloud providers (`aws`, future `gcp` / `azure` / `k8s`) ship as
//! separate binaries the app spawns as subprocesses, speaking
//! line-delimited JSON-RPC 2.0 over stdio, the same framing the
//! `oryxis-mcp` server already uses.
//!
//! This crate is *only the types*: no tokio, no stdio, no spawn
//! logic. The host side (`oryxis-app::plugins`) and every plugin
//! binary (`oryxis-cloud-aws-plugin`, ...) both depend on it so the
//! contract lives in exactly one place.
//!
//! ## Versioning
//!
//! The contract is versioned by *protocol version*, not by app or
//! plugin version, the same model LSP / DAP / MCP use. A plugin
//! minor/patch bump that doesn't change the wire shape leaves
//! [`PROTOCOL_VERSION`] untouched. Each side announces the protocol
//! versions it understands in `initialize`; the plugin selects the
//! highest value common to both (see [`negotiate_version`]).
//!
//! In practice the app and every plugin link the *same*
//! `oryxis-plugin-protocol`, so they always agree, the negotiation
//! exists so a plugin pinned to an older release can still be driven
//! by a newer app (and vice versa) without a hard failure.

pub mod error;
pub mod jsonrpc;
pub mod methods;

pub use error::{cloud_error_to_rpc, rpc_error_to_cloud};
pub use jsonrpc::{error_codes, JsonRpcError, JsonRpcRequest, JsonRpcResponse};
pub use methods::*;

// Re-export the canonical shared types so the host and plugins reach
// for one path. `CloudError` stays canonical in `oryxis-cloud`; the
// persisted model types stay canonical in `oryxis-core`. This crate
// never redefines a DTO it can borrow.
pub use oryxis_cloud::{CloudError, DiscoveredHost, DiscoveryResult, SessionPayload};
pub use oryxis_core::models::{CloudProfile, CloudQuery, CloudResourceType, TransportKind};

/// Protocol versions this build of the contract understands, newest
/// last. Bump by appending a value here (and keeping the old ones)
/// whenever a wire-incompatible change lands.
pub const SUPPORTED_PROTOCOL_VERSIONS: &[u32] = &[1];

/// The newest protocol version this build speaks. Always the last
/// entry of [`SUPPORTED_PROTOCOL_VERSIONS`].
pub const PROTOCOL_VERSION: u32 = 1;

/// Seed for the development plugin-signing keypair.
///
/// Shared between `oryxis-plugin-signer` (signs locally built
/// binaries) and the app's `plugins::verify` (verifies them in debug
/// builds). The dev key has no authority on a release build (see
/// the app-side trust set), so committing it is fine and lets the
/// pipeline be exercised end-to-end without the CI key.
pub const DEV_PLUGIN_SIGNING_SEED: [u8; 32] = [0x42u8; 32];

/// Pick the highest protocol version present in *both* sets, or
/// `None` when the host and plugin share no common version (the host
/// should then refuse to use the plugin and tell the user to update).
///
/// Order-independent: neither slice has to be sorted.
pub fn negotiate_version(host: &[u32], plugin: &[u32]) -> Option<u32> {
    host.iter()
        .filter(|v| plugin.contains(v))
        .copied()
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiate_picks_highest_common() {
        assert_eq!(negotiate_version(&[1, 2, 3], &[2, 3, 4]), Some(3));
        assert_eq!(negotiate_version(&[1], &[1]), Some(1));
        // Order must not matter.
        assert_eq!(negotiate_version(&[3, 1, 2], &[2]), Some(2));
    }

    #[test]
    fn negotiate_returns_none_when_disjoint() {
        assert_eq!(negotiate_version(&[1, 2], &[3, 4]), None);
        assert_eq!(negotiate_version(&[], &[1]), None);
    }

    #[test]
    fn protocol_version_is_last_supported() {
        assert_eq!(
            Some(&PROTOCOL_VERSION),
            SUPPORTED_PROTOCOL_VERSIONS.last(),
            "PROTOCOL_VERSION must track the newest supported version"
        );
    }
}
