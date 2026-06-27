//! Hermetic integration test for the legacy-cipher fallback. Stands up an
//! in-process russh server that offers ONLY a legacy cbc cipher (kex / mac
//! / host-key stay at the secure defaults, so the mismatch lands precisely
//! on the cipher) and drives the real `SshEngine` against it over loopback
//! TCP:
//!
//! 1. An `Auto` client (safe defaults, no cbc) fails the handshake, and the
//!    failure classifies as `NegCategory::Cipher` with the server's offer,
//!    which is exactly what the UI fallback dialog keys off.
//! 2. A client pinned to the expanded (legacy-inclusive) cipher set
//!    completes the handshake, proving the "connect anyway" expansion
//!    actually negotiates with such a server.
//!
//! This is the path neither maintainer can exercise against a real legacy
//! server, so it is validated here instead.

use std::sync::Arc;

use crate::sftp_harness::HARNESS_HOST_KEY;
use crate::{HostKeyCheckCallback, HostKeyStatus, NegCategory, SshEngine};
use oryxis_core::models::Connection;

/// Minimal server handler: the transport handshake never reaches auth or
/// channels, so the trait defaults suffice.
struct LegacyHandler;

impl russh::server::Handler for LegacyHandler {
    type Error = russh::Error;
}

/// Spawn a loopback russh server that advertises `cipher_only` as its sole
/// cipher. Returns the bound port. Loops on accept so one server can serve
/// both connection attempts in the test.
async fn spawn_legacy_cipher_server(cipher_only: russh::cipher::Name) -> u16 {
    use russh::keys::PrivateKey;

    let mut config = russh::server::Config::default();
    config
        .keys
        .push(PrivateKey::from_openssh(HARNESS_HOST_KEY).expect("parse host key"));
    let mut preferred = russh::Preferred::DEFAULT;
    preferred.cipher = std::borrow::Cow::Owned(vec![cipher_only]);
    config.preferred = preferred;
    let config = Arc::new(config);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback");
    let port = listener.local_addr().expect("local_addr").port();

    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            let config = config.clone();
            tokio::spawn(async move {
                if let Ok(running) =
                    russh::server::run_stream(config, socket, LegacyHandler).await
                {
                    let _ = running.await;
                }
            });
        }
    });

    port
}

fn loopback_conn(port: u16) -> Connection {
    let mut conn = Connection::new("legacy-cipher-test", "127.0.0.1");
    conn.port = port;
    conn
}

#[tokio::test]
async fn auto_fails_with_cipher_category_then_expanded_connects() {
    let accept_all: HostKeyCheckCallback = Arc::new(|_, _, _, _| HostKeyStatus::Known);
    let port = spawn_legacy_cipher_server(russh::cipher::AES_256_CBC).await;

    // 1. Auto client: the safe defaults carry no cbc cipher, so the
    //    handshake fails with a structured negotiation error on the cipher.
    let auto = SshEngine::new().with_host_key_check(accept_all.clone());
    let err = match auto.establish_transport(&loopback_conn(port), None).await {
        Ok(_) => panic!("Auto must fail against a cbc-only server"),
        Err(e) => e,
    };
    let nf = err
        .negotiation_failure()
        .expect("a no-common-algorithm failure");
    assert_eq!(nf.category, NegCategory::Cipher);
    assert!(
        nf.server_offers.iter().any(|s| s == "aes256-cbc"),
        "server offers should include aes256-cbc, got {:?}",
        nf.server_offers
    );

    // 2. Expanded client: the legacy-inclusive cipher set shares aes256-cbc
    //    with the server, so the transport handshake completes.
    let expanded_ciphers: Vec<String> = crate::algorithms::expanded_ciphers()
        .into_iter()
        .map(|s| s.to_string())
        .collect();
    let expanded = SshEngine::new()
        .with_host_key_check(accept_all)
        .with_algorithm_overrides(Some(expanded_ciphers), None, None, None);
    expanded
        .establish_transport(&loopback_conn(port), None)
        .await
        .expect("expanded client should complete the handshake");
}
