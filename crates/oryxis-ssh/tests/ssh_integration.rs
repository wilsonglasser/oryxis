//! End-to-end integration tests for the plain SSH path (auth, exec,
//! PTY shell, detect_os) against a real OpenSSH server in a throwaway
//! container.
//!
//! Same Docker / `--ignored` rules as `sftp_integration.rs`. Each test
//! spins its own container so they parallelise cleanly.

use std::sync::Arc;
use std::time::Duration;

use oryxis_core::models::connection::{AuthMethod, Connection};
use oryxis_ssh::{HostKeyStatus, SshEngine};
use testcontainers::{
    core::{ContainerPort, IntoContainerPort, WaitFor},
    runners::AsyncRunner,
    GenericImage, ImageExt,
};

const TEST_USER: &str = "tester";
const TEST_PASS: &str = "testpass123";

/// Ephemeral ed25519 keypair used only by the pubkey-auth tests below.
/// Public half is fed to the linuxserver/openssh-server container via
/// the `PUBLIC_KEY` env var; the private half is handed to russh as PEM.
/// Generated with `ssh-keygen -t ed25519 -N "" -C oryxis-test`. Has no
/// authority on any real machine — committing it here is fine.
const TEST_PUBKEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHqXz+0CmwH1pGs+5hWVBcqRQmED5a1tJ5Umb1vp0cW8 oryxis-test";
const TEST_PRIVKEY: &str = "-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW
QyNTUxOQAAACB6l8/tApsB9aRrPuYVlQXKkUJhA+WtbSeVJm9b6dHFvAAAAJChC7l8oQu5
fAAAAAtzc2gtZWQyNTUxOQAAACB6l8/tApsB9aRrPuYVlQXKkUJhA+WtbSeVJm9b6dHFvA
AAAED+kh0/9HXyIxhyVOboYST/QHB9Uswr4KfyjtmwkwUOHXqXz+0CmwH1pGs+5hWVBcqR
QmED5a1tJ5Umb1vp0cW8AAAAC29yeXhpcy10ZXN0AQI=
-----END OPENSSH PRIVATE KEY-----
";

/// Spin up sshd in `linuxserver/openssh-server`. Mirrors the helper in
/// `sftp_integration.rs`; if `pubkey` is set, the container also accepts
/// the embedded test public key.
async fn start_sshd(
    pubkey: bool,
) -> (
    Connection,
    String,
    testcontainers::ContainerAsync<GenericImage>,
) {
    let mut image = GenericImage::new("linuxserver/openssh-server", "latest")
        .with_exposed_port(ContainerPort::Tcp(2222))
        // The image prints "sshd is listening on port 2222" *before*
        // it's actually bound and ready, so we wait for the very last
        // init line which fires only after sshd is reachable.
        .with_wait_for(WaitFor::message_on_stdout("[ls.io-init] done."))
        .with_env_var("PUID", "1000")
        .with_env_var("PGID", "1000")
        .with_env_var("PASSWORD_ACCESS", "true")
        .with_env_var("USER_NAME", TEST_USER)
        .with_env_var("USER_PASSWORD", TEST_PASS)
        .with_env_var("SUDO_ACCESS", "false");
    if pubkey {
        image = image.with_env_var("PUBLIC_KEY", TEST_PUBKEY);
    }
    let container = image
        .start()
        .await
        .expect("docker daemon must be running");
    let port = container
        .get_host_port_ipv4(2222.tcp())
        .await
        .expect("port mapping");
    let host = container.get_host().await.expect("host").to_string();
    let mut conn = Connection::new("test", host);
    conn.port = port;
    conn.username = Some(TEST_USER.to_string());
    conn.auth_method = AuthMethod::Password;
    (conn, TEST_PASS.to_string(), container)
}

fn engine() -> SshEngine {
    SshEngine::new()
        .with_host_key_check(Arc::new(|_, _, _, _| HostKeyStatus::Known))
        .with_connect_timeout(Duration::from_secs(20))
        .with_auth_timeout(Duration::from_secs(20))
        .with_session_timeout(Duration::from_secs(20))
}

/// Drive the 3-stage connect (transport / auth / exec) so we can call
/// `exec_command` instead of opening a shell. The convenience wrapper
/// `engine.connect` always opens a PTY; for exec we go the long way.
async fn exec_with_password(
    conn: &Connection,
    password: &str,
    command: &str,
) -> oryxis_ssh::ExecResult {
    let engine = engine();
    let mut handle = engine
        .establish_transport(conn, None)
        .await
        .expect("transport");
    engine
        .do_authenticate(&mut handle, conn, Some(password), None)
        .await
        .expect("authenticate");
    engine
        .exec_command(handle, command, Duration::from_secs(20))
        .await
        .expect("exec_command")
}

#[tokio::test]
#[ignore = "requires Docker — run with --ignored"]
async fn password_auth_runs_exec_command() {
    let (conn, password, _container) = start_sshd(false).await;
    let result = exec_with_password(&conn, &password, "echo hello-from-oryxis").await;
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "hello-from-oryxis");
    assert!(result.stderr.is_empty());
}

#[tokio::test]
#[ignore = "requires Docker — run with --ignored"]
async fn pubkey_auth_runs_exec_command() {
    let (mut conn, _password, _container) = start_sshd(true).await;
    conn.auth_method = AuthMethod::Key;
    let engine = engine();
    let mut handle = engine
        .establish_transport(&conn, None)
        .await
        .expect("transport");
    engine
        .do_authenticate(&mut handle, &conn, None, Some(TEST_PRIVKEY))
        .await
        .expect("authenticate via pubkey");
    let result = engine
        .exec_command(handle, "id -un", Duration::from_secs(20))
        .await
        .expect("exec_command");
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), TEST_USER);
}

#[tokio::test]
#[ignore = "requires Docker — run with --ignored"]
async fn exec_command_propagates_nonzero_exit() {
    let (conn, password, _container) = start_sshd(false).await;
    let result = exec_with_password(&conn, &password, "exit 42").await;
    assert_eq!(result.exit_code, 42);
}

#[tokio::test]
#[ignore = "requires Docker — run with --ignored"]
async fn exec_command_separates_stdout_and_stderr() {
    let (conn, password, _container) = start_sshd(false).await;
    let result = exec_with_password(
        &conn,
        &password,
        "echo on-stdout; echo on-stderr 1>&2",
    )
    .await;
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("on-stdout"));
    assert!(result.stderr.contains("on-stderr"));
    // Cross-check: stdout should NOT carry the stderr line and vice
    // versa — confirms the ExtendedData (ext=1) split worked.
    assert!(!result.stdout.contains("on-stderr"));
    assert!(!result.stderr.contains("on-stdout"));
}

#[tokio::test]
#[ignore = "requires Docker — run with --ignored"]
async fn wrong_password_yields_error() {
    let (conn, _password, _container) = start_sshd(false).await;
    let engine = engine();
    let mut handle = engine
        .establish_transport(&conn, None)
        .await
        .expect("transport");
    let err = engine
        .do_authenticate(&mut handle, &conn, Some("definitely-not-the-password"), None)
        .await
        .expect_err("auth must fail");
    // The exact message is provider-dependent; we only assert that the
    // call surfaced an error rather than silently succeeding.
    let _ = err;
}

#[tokio::test]
#[ignore = "requires Docker — run with --ignored"]
async fn pty_session_round_trips_input_to_output() {
    // Sanity check on the interactive shell path: open a PTY, write a
    // command terminated by newline, and confirm the prompt echoes
    // both the command we typed and its output back to us.
    let (conn, password, _container) = start_sshd(false).await;
    let engine = engine();
    let (session, mut rx) = engine
        .connect(&conn, Some(&password), None, 80, 24)
        .await
        .expect("connect");
    session.write(b"echo pty-marker-xyz\n").expect("write");

    // Drain output until we see our marker or hit a generous timeout.
    // The PTY echoes both the typed command and its output, so we check
    // for the literal output token (without "echo " prefix) appearing
    // on its own line.
    let mut buf = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let saw_marker = loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break false;
        }
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Some(chunk)) => {
                buf.extend_from_slice(&chunk);
                let text = String::from_utf8_lossy(&buf);
                // Look for the marker on a line that isn't the echoed
                // command itself (which always carries `echo `).
                if text
                    .lines()
                    .any(|l| l.contains("pty-marker-xyz") && !l.contains("echo "))
                {
                    break true;
                }
            }
            Ok(None) => break false,
            Err(_) => break false,
        }
    };
    assert!(
        saw_marker,
        "expected pty output to include the marker, got: {:?}",
        String::from_utf8_lossy(&buf)
    );
}

#[tokio::test]
#[ignore = "requires Docker — run with --ignored"]
async fn pty_session_resize_is_not_fatal() {
    // resize() is fire-and-forget — we just want to confirm it doesn't
    // panic and the session stays alive afterwards.
    let (conn, password, _container) = start_sshd(false).await;
    let engine = engine();
    let (session, _rx) = engine
        .connect(&conn, Some(&password), None, 80, 24)
        .await
        .expect("connect");
    session.resize(120, 40);
    session.resize(200, 60);
    // Tiny grace period so the window-change request can hit the wire
    // before we tear down — an immediate drop sometimes truncates the
    // last channel message and produces a misleading "channel closed"
    // log line.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(session.is_alive());
}

#[tokio::test]
#[ignore = "requires Docker — run with --ignored"]
async fn agent_forwarding_sets_remote_ssh_auth_sock() {
    // When the engine is configured with `with_agent_forwarding(true)`,
    // sshd inside the container should create a unix socket and export
    // its path as `SSH_AUTH_SOCK` to the user's shell. We don't need a
    // real local agent for this assertion — the env var is set on the
    // remote side as soon as the channel-level request is accepted.
    let (conn, password, _container) = start_sshd(false).await;
    let engine = engine().with_agent_forwarding(true);
    let (session, mut rx) = engine
        .connect(&conn, Some(&password), None, 80, 24)
        .await
        .expect("connect");
    // Sleep briefly so the shell prompt is fully drawn before we type
    // — otherwise the marker can interleave with motd / prompt output.
    tokio::time::sleep(Duration::from_millis(500)).await;
    session
        .write(b"echo SOCK=[$SSH_AUTH_SOCK]\n")
        .expect("write");

    let mut buf = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let saw_socket = loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break false;
        }
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Some(chunk)) => {
                buf.extend_from_slice(&chunk);
                let text = String::from_utf8_lossy(&buf);
                // Look for `SOCK=[/...` on a line that isn't the
                // echoed command (the typed line carries the literal
                // `$SSH_AUTH_SOCK`, not its expansion).
                if text.lines().any(|l| {
                    l.contains("SOCK=[/") && !l.contains("$SSH_AUTH_SOCK")
                }) {
                    break true;
                }
            }
            Ok(None) | Err(_) => break false,
        }
    };
    assert!(
        saw_socket,
        "expected SSH_AUTH_SOCK to be set on the remote shell when forwarding is on, got: {:?}",
        String::from_utf8_lossy(&buf)
    );
}

#[tokio::test]
#[ignore = "requires Docker — run with --ignored"]
async fn agent_forwarding_off_leaves_remote_socket_unset() {
    // Mirror of the previous test — without forwarding, the remote
    // shell shouldn't have `SSH_AUTH_SOCK` set (the whole point of
    // OpenSSH's default `ForwardAgent no` is that opting in is explicit).
    let (conn, password, _container) = start_sshd(false).await;
    let engine = engine(); // forwarding off (default)
    let (session, mut rx) = engine
        .connect(&conn, Some(&password), None, 80, 24)
        .await
        .expect("connect");
    tokio::time::sleep(Duration::from_millis(500)).await;
    session
        .write(b"echo SOCK=[$SSH_AUTH_SOCK]\n")
        .expect("write");

    let mut buf = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let saw_empty = loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break false;
        }
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Some(chunk)) => {
                buf.extend_from_slice(&chunk);
                let text = String::from_utf8_lossy(&buf);
                if text.lines().any(|l| {
                    l.contains("SOCK=[]") && !l.contains("$SSH_AUTH_SOCK")
                }) {
                    break true;
                }
            }
            Ok(None) | Err(_) => break false,
        }
    };
    assert!(
        saw_empty,
        "expected SSH_AUTH_SOCK to be unset without forwarding, got: {:?}",
        String::from_utf8_lossy(&buf)
    );
}

#[tokio::test]
#[ignore = "requires Docker — run with --ignored"]
async fn detect_os_returns_a_value() {
    // The image is Alpine-based; we don't pin the exact string because
    // it depends on which uname/lsb-release path detect_os hits inside
    // the container. We only assert the call resolves to *something*
    // non-empty within the timeout.
    let (conn, password, _container) = start_sshd(false).await;
    let engine = engine();
    let (session, _rx) = engine
        .connect(&conn, Some(&password), None, 80, 24)
        .await
        .expect("connect");
    let os = session.detect_os().await;
    assert!(os.is_some(), "expected detect_os to return Some(_)");
    let os = os.unwrap();
    assert!(!os.is_empty(), "detect_os returned an empty string");
}
