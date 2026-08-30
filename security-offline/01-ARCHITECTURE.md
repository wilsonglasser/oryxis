# 01 — Architecture (post-transformation target state)

## Process model

- Single GUI process (`oryxis`, iced + wgpu + tokio). No background services, no launch agents, no scheduled tasks.
- Single-instance guard: Windows named mutex (`src/tray.rs:287-327`); secondary instances forward intents via file IPC and exit.
- Long-lived tasks: ssh-agent accept loop, 500 ms deep-link/connect inbox poll (`subscription.rs`), tray poll, font ensure tasks, AI SSE streams, SSH engines, dev-only harness daemon.
- Subprocess spawns (all sites verified):
  - `gif_export.rs:67` — cached local plugin binary (two file-path args, no shell).
  - `oryxis-ssh/engine/connect.rs:556` — `sh -c <ProxyCommand>` from ssh_config, gated by per-dial consent UI (`:540-554`).
  - `mcp.rs` — `wsl.exe` (Windows) for WSL config install.
  - `dispatch_remote_desktop.rs` — RDP/VNC client from PATH (program+args).
  - `dispatch_sftp_files.rs` — user-chosen "open with" program; `rundll32.exe`.
  - `util.rs:698-895` — sound players, `open`/`xdg-open`/`explorer`, `dbus-send` (args quoted).
  - `dispatch_settings/local_terminals.rs` — `where`, `wsl` probing.
  - `agent_server/listener.rs:107` — `ps -p <pid>` (peer display).
  - `oryxis-terminal/widget/clipboard.rs` — `open`/`xdg-open` for OSC-8 links.
  - `build.rs:86` — `git` version stamp (build time only).
  - test-only: `oryxis-archive/remote.rs`, zmodem tests.
- External clients (Claude Desktop/Code, Cursor) spawn `~/.oryxis/bin/oryxis-mcp` (stdio JSON-RPC, no network).

## Entry points

- CLI argv + `oryxis://` deep links (`deep_link.rs`): `oryxis://theme/<base64url-JSON>` (import panel, strict parse, 128 KiB cap, no path/query/fragment) and `ssh://[user@]host[:port]` (ad-hoc editor, never dials; test-enforced). `oryxis user@host` CLI form dials on explicit provenance.
- Local listeners: ssh-agent Unix socket `~/.oryxis/agent.sock` (0700/0600, stale-probe, per-connection confirm mode); Windows named pipe `\\.\pipe\oryxis-ssh-agent` (user-only DACL, first-instance anti-squat, no remote clients); dev-only harness TCP `127.0.0.1:6799` (feature `harness`).
- File IPC: `~/.oryxis/runtime/{instances,commands,deeplink,connect}`.

## Storage (all under `~/.oryxis/`, `ORYXIS_HOME` override)

| Path | Content | Protection |
|---|---|---|
| `vault.db` | SQLite: hosts, keys, identities, snippets, logs, settings | ChaCha20Poly1305 per-field, Argon2id KDF (calibrated, floored at m=19456 KiB/t=2/p=1), file mode 0600 |
| `fonts/` | downloaded CJK/Nerd-Font cache | pinned len+SHA-256 validated on use |
| `plugins/<provider>/<version>/` | local plugin binaries (mcp, gif) | Ed25519-verified at install (see 11-SECURITY-CHANGES: launcher gate added) |
| `bin/oryxis-mcp` | stable launcher spawned by external clients | 0755, user-private dir |
| `agent.sock` | ssh-agent socket | 0600 |
| `runtime/` | tray/deep-link IPC files | user-private |
| `oryxis-debug.log` | debug log (opt-in) | local only |

Removed with the transformation: sync device identity, signaling tokens, sync peers/metadata tables (schema no longer creates them; `destroy_and_recreate` still drops them for legacy vaults), cloud profile tables/API, plugin download cache semantics.

## Trust boundaries

1. Vault file ↔ process (at-rest crypto).
2. OS keystore ↔ process (biometric unlock stores master password locally).
3. Remote SSH peers ↔ terminal/agent (untrusted server output, agent confirm mode).
4. Imported files ↔ vault (portable export, PuTTY/mRemoteNG imports, theme deep links — deny-listed security settings, untrusted-input parsing).
5. Plugin cache ↔ app (signature-gated at launcher copy now).
6. External MCP clients ↔ oryxis-mcp (bearer token in env/config; optional master-password embedding behind explicit confirm — documented finding).
7. User-configured AI provider ↔ session context (explicit opt-in egress; see 04-EGRESS-MATRIX).
