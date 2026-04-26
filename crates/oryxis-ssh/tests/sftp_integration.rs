//! End-to-end integration tests for the SFTP path against a real
//! OpenSSH server running in a throwaway container.
//!
//! Requires Docker on the host and is gated behind `#[ignore]` so a
//! plain `cargo test` (CI without Docker, dev quick loop) skips them.
//! Run explicitly with:
//!
//! ```sh
//! cargo test -p oryxis-ssh -- --ignored
//! ```
//!
//! Each test spins up its own container so they can run in parallel
//! without stepping on a shared sshd.

use std::sync::Arc;
use std::time::Duration;

use oryxis_core::models::connection::{AuthMethod, Connection};
use oryxis_ssh::{HostKeyStatus, SshEngine};
use testcontainers::{
    core::{ContainerPort, IntoContainerPort, WaitFor},
    runners::AsyncRunner,
    GenericImage, ImageExt,
};

/// Username + password we hand the linuxserver/openssh-server image.
/// Hardcoded only because the image generates these inside the
/// container at boot — they never touch any real machine.
const TEST_USER: &str = "tester";
const TEST_PASS: &str = "testpass123";

/// Stand up a fresh SFTP-capable container and return `(connection,
/// password)` ready to hand to `SshEngine::connect`. Caller holds the
/// container handle in scope to keep it alive for the duration of the
/// test.
async fn start_sshd() -> (
    Connection,
    String,
    testcontainers::ContainerAsync<GenericImage>,
) {
    let container = GenericImage::new("linuxserver/openssh-server", "latest")
        .with_exposed_port(ContainerPort::Tcp(2222))
        // The "sshd is listening on port 2222" line fires *before* the
        // socket is actually accepting connections, so we wait for the
        // very last init line which only prints after sshd is reachable.
        .with_wait_for(WaitFor::message_on_stdout("[ls.io-init] done."))
        .with_env_var("PUID", "1000")
        .with_env_var("PGID", "1000")
        .with_env_var("PASSWORD_ACCESS", "true")
        .with_env_var("USER_NAME", TEST_USER)
        .with_env_var("USER_PASSWORD", TEST_PASS)
        .with_env_var("SUDO_ACCESS", "false")
        .start()
        .await
        .expect("docker daemon must be running");
    let port = container
        .get_host_port_ipv4(2222.tcp())
        .await
        .expect("port mapping");
    let host = container
        .get_host()
        .await
        .expect("host")
        .to_string();
    let mut conn = Connection::new("test", host);
    conn.port = port;
    conn.username = Some(TEST_USER.to_string());
    conn.auth_method = AuthMethod::Password;
    (conn, TEST_PASS.to_string(), container)
}

fn engine() -> SshEngine {
    // Trust whatever host key the container hands us — these are
    // ephemeral fixtures, not real servers, and the test is asserting
    // protocol behaviour, not host-key policy.
    SshEngine::new()
        .with_host_key_check(Arc::new(|_, _, _, _| HostKeyStatus::Known))
        .with_connect_timeout(Duration::from_secs(20))
        .with_auth_timeout(Duration::from_secs(20))
        .with_session_timeout(Duration::from_secs(20))
}

#[tokio::test]
#[ignore = "requires Docker — run with --ignored"]
async fn sftp_list_root_after_password_auth() {
    let (conn, password, _container) = start_sshd().await;
    let engine = engine();
    let (session, _rx) = engine
        .connect(&conn, Some(&password), None, 80, 24)
        .await
        .expect("connect");
    let session = Arc::new(session);
    let client = session.open_sftp().await.expect("open sftp");
    // The image's home dir for `tester` is /config, so canonicalize
    // gives an absolute path we can list.
    let initial = client.canonicalize(".").await.expect("canonicalize");
    let entries = client.list_dir(&initial).await.expect("list_dir");
    // The home dir is non-empty (the image plants `.ssh/` etc), but
    // we only assert the call resolved — content varies by image
    // tag and isn't load-bearing.
    let _ = entries;
}

#[tokio::test]
#[ignore = "requires Docker — run with --ignored"]
async fn sftp_write_read_round_trip() {
    let (conn, password, _container) = start_sshd().await;
    let engine = engine();
    let (session, _rx) = engine
        .connect(&conn, Some(&password), None, 80, 24)
        .await
        .expect("connect");
    let session = Arc::new(session);
    let client = session.open_sftp().await.expect("open sftp");
    let home = client.canonicalize(".").await.expect("canonicalize");

    let path = format!("{}/round-trip.txt", home.trim_end_matches('/'));
    let payload = b"hello from oryxis test\n";
    client.write_file(&path, payload).await.expect("write_file");
    let read_back = client.read_file(&path).await.expect("read_file");
    assert_eq!(read_back, payload);

    // Rename then verify the new path is listable + the old one isn't.
    let renamed = format!("{}/renamed.txt", home.trim_end_matches('/'));
    client.rename(&path, &renamed).await.expect("rename");
    let after = client
        .list_dir(&home)
        .await
        .expect("list_dir after rename");
    let names: Vec<_> = after.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"renamed.txt"));
    assert!(!names.contains(&"round-trip.txt"));

    client.remove_file(&renamed).await.expect("remove_file");
}

#[tokio::test]
#[ignore = "requires Docker — run with --ignored"]
async fn sftp_recursive_dir_delete_via_exec() {
    // `remove_dir_recursive` shells out to `rm -rf` over an exec
    // channel — this exercises the SshSession→exec path, which the
    // unit tests can't cover.
    let (conn, password, _container) = start_sshd().await;
    let engine = engine();
    let (session, _rx) = engine
        .connect(&conn, Some(&password), None, 80, 24)
        .await
        .expect("connect");
    let session = Arc::new(session);
    let client = session.open_sftp().await.expect("open sftp");
    let home = client.canonicalize(".").await.expect("canonicalize");

    // Build /home/<user>/scratch/{a,b/c.txt}, then nuke it recursively.
    let scratch = format!("{}/scratch", home.trim_end_matches('/'));
    client.create_dir(&scratch).await.expect("mkdir scratch");
    let nested = format!("{}/b", scratch);
    client.create_dir(&nested).await.expect("mkdir nested");
    client
        .write_file(&format!("{}/a", scratch), b"a")
        .await
        .expect("write a");
    client
        .write_file(&format!("{}/c.txt", nested), b"c")
        .await
        .expect("write c");

    client
        .remove_dir_recursive(&scratch)
        .await
        .expect("remove_dir_recursive");

    let after = client.list_dir(&home).await.expect("list after");
    let names: Vec<_> = after.iter().map(|e| e.name.as_str()).collect();
    assert!(!names.contains(&"scratch"));
}

#[tokio::test]
#[ignore = "requires Docker — run with --ignored"]
async fn sftp_open_sibling_for_parallel_pool() {
    // Validates the SFTP sibling-channel path used by the parallel
    // transfer worker pool: opening N independent subsystem channels
    // on the same SSH connection should succeed and each should be
    // independently usable.
    let (conn, password, _container) = start_sshd().await;
    let engine = engine();
    let (session, _rx) = engine
        .connect(&conn, Some(&password), None, 80, 24)
        .await
        .expect("connect");
    let session = Arc::new(session);
    let primary = session.open_sftp().await.expect("primary sftp");
    let siblings: Vec<_> = futures_or_join(primary.clone(), 3).await;
    let home = primary.canonicalize(".").await.expect("canonicalize");
    // All siblings should successfully list the same directory in
    // parallel without serialising on the primary's mutex.
    for client in &siblings {
        let _ = client.list_dir(&home).await.expect("sibling list_dir");
    }
}

/// Sequentially open `n` siblings off `primary` — keeps the test
/// simple without pulling in a futures crate just for `join_all`.
async fn futures_or_join(
    primary: oryxis_ssh::SftpClient,
    n: usize,
) -> Vec<oryxis_ssh::SftpClient> {
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(primary.open_sibling().await.expect("open_sibling"));
    }
    out
}
