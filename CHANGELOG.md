# Changelog

All notable changes to Oryxis are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project uses [SemVer](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0] - 2026-04-26

### Added
- **Streaming AI responses** — assistant text now arrives token-by-token
  as the provider emits it, with a generic SSE parser plus per-provider
  decoders for Anthropic (`content_block_delta` + `partial_json` for
  tool calls), OpenAI-compat (`delta.content` + accumulated tool_call
  arguments), and Gemini (`streamGenerateContent?alt=sse`). Tool-call
  follow-ups stream too — the terminal poll and the next chat round
  pump through a single `Task::stream`.
- **SSH agent forwarding** — opt-in per host (`Connection.agent_forwarding`,
  toggle in the host editor; mapped from `ForwardAgent yes` on
  `ssh_config` import). Issues `auth-agent-req@openssh.com` before
  `request_shell` and bridges inbound forward channels to the local
  ssh-agent (Unix socket on Linux/macOS, named pipe on Windows). Only
  channels we explicitly asked for are accepted.
- **SSH integration tests** (`crates/oryxis-ssh/tests/ssh_integration.rs`)
  via testcontainers — password auth, ed25519 pubkey auth, exit-code
  propagation, stdout/stderr split, wrong password, PTY round-trip,
  resize, `detect_os`, and forward-on/off coverage. Same `#[ignore]`
  gating as `sftp_integration.rs`.
- **Theme contrast helpers** — new `relative_luminance`,
  `contrast_ratio`, `contrast_text_for` in `theme.rs`; new
  `button_bg / button_bg_hover / button_text` triple per theme so
  every primary CTA shares a hand-tuned foreground (no more dark
  text on dark accent in light themes / Darcula).
- **`widgets::cta_button`** — wide accent CTA used by Snippets / Keys
  empty states. Pulls `button_text` so labels stay legible across
  every theme.
- **`widgets::settings_row_link`** + `Message::OpenUrl(String)` —
  About panel's GitHub line is now a clickable link that opens in the
  OS default browser.
- **Edit local file** in the SFTP local pane — context-menu item
  hands the path to `open::that()` (no temp copy / no mtime watch,
  unlike the remote Edit-in-place flow).
- **Lock screen and unlock screen now respect the saved theme** —
  `app_theme` and `language` live in the plaintext settings table, so
  boot reads them before the vault unlock instead of falling back to
  defaults until the password is typed.

### Changed
- **`app.rs` split** — was 6715 lines, now 358. Extracted `boot.rs`,
  `messages.rs`, `subscription.rs`, `root_view.rs`, `connect_methods.rs`,
  `sftp_methods.rs`, `sftp_helpers.rs`, plus the per-domain dispatch
  modules (`dispatch_*.rs`) below.
- **`dispatch.rs` (the `update` match) split** — was 5114 lines after
  the initial extraction, now 489. The master `update` chains
  `try_handler!` calls into 10 domain handlers
  (`dispatch_sftp / sftp_files / sftp_transfers / ssh / settings /
  keys / ai / editor / tabs / terminal / share`); each returns
  `Result<Task<Message>, Message>` to pass unclaimed messages back up
  the chain. Test count: 145 unit + 14 integration.
- **Tab-bar right cluster uniform** — `+` and `⋯` (jump-to) buttons
  now share the chrome-button width (46) and full bar height (40),
  zero radius, same hover tint as `−` `□` `✕`. Sidebar collapse uses
  `lucide::panel_left_close / open` instead of `«` / `»` chevrons.
- **Theme persistence** — `Message::AppThemeChanged` writes
  `app_theme` to the settings table; `boot.rs` rehydrates via
  `AppTheme::from_name(...)`.
- **`SolarizedLight.text_primary`** moved from base01 (#586E75) to
  base02 (#073642) — caught by the new theme contrast tests; the
  original was 4.39 : 1 against the base2 sidebar (below WCAG AA).
- **i18n keys added** — `forward_ssh_agent`, `github`, `select_file`,
  `start_over`. New strings introduced this release go through
  `crate::i18n::t(...)` instead of being hardcoded.

### Fixed
- **SFTP modals dismissing on body click** — every dialog (host
  picker, properties, overwrite, edit prompts, etc.) wrapped its
  scrim in a Stack where clicks fell through to the close target. All
  six now wrap the dialog body in `MouseArea::on_press(NoOp)` to
  swallow clicks inside it.
- **AI streaming placeholders** — empty assistant bubbles (created
  before the first token arrives or when the model goes straight to
  a tool call) are now filtered at the view layer and out of the
  message-builder, so they don't render as glitch boxes or get sent
  back to the model on the next turn.
- **`+ HOST` / `+ ADD` / `New Snippet` buttons in light themes** —
  used `text_primary` (which is dark in light themes) on the accent
  background. Now use the per-theme `button_text` (white) so they
  stay readable in every theme.
- **`SshEngine` agent_forward request order** — now sent *before*
  `request_shell`, otherwise sshd doesn't set `SSH_AUTH_SOCK` for the
  spawned shell (caught by the new integration test).
- **`linuxserver/openssh-server` wait condition** — `WaitFor::message_on_stderr("sshd is listening on port 2222")`
  no longer matches the current image (the line moved to stdout and
  now fires *before* the socket accepts connections). Both
  `ssh_integration.rs` and `sftp_integration.rs` now wait for
  `[ls.io-init] done.` instead.

---

The 0.4.0 release also ships everything that had accumulated in the
"Unreleased" section since 0.3.3 — the SFTP browser baseline, drag &
drop, multi-select, transfer queue, etc. — listed below for the
record:

### Added (SFTP browser baseline)
- **SFTP file browser** (left-nav: SFTP). Dual-pane local/remote view
  with sort, filter, breadcrumb navigation, hidden-file toggle.
- **OS-level drag-and-drop uploads** — drop files from Finder /
  Explorer / Files onto the remote pane (or onto a folder row to
  upload there).
- **Internal cross-pane drag** — drag any row across panes to
  upload/download; floating ghost shows the dragged label or count.
- **Multi-select** with Ctrl-toggle and Shift-range; right-click on a
  selected row dispatches Delete / Download / Duplicate / Upload as a
  batch with a single confirm modal.
- **Edit-in-place** — download a remote file to a tagged temp,
  open in the OS default editor, watch via 2-second mtime poll, prompt
  to upload back when the user saves.
- **Properties dialog** with chmod (R/W/X grid for owner/group/others),
  file size, mtime, owner uid/gid; preserves setuid/setgid/sticky.
- **Overwrite handling** — Replace / Replace if different size /
  Duplicate / Cancel modal on name collision; "Apply to remaining"
  checkbox for multi-file transfers.
- **Configurable transfer parallelism** (1–8 SFTP channels per
  session) via a new Settings → SFTP panel.
- **Configurable timeouts** for TCP connect, SSH auth, channel open,
  and per-operation requests — all live-applicable.
- **Recursive remote delete via `rm -rf`** — much faster than per-file
  SFTP, single exec channel round-trip.
- **`cp -r --` for remote folder duplicate** via the same exec
  multiplexing.
- **Bulk transfer queue** with per-item progress bar, cancel button,
  and apply-to-all sticky decision for repeated conflicts.
- **Settings persistence** — all user preferences (theme, font,
  keepalive, scrollback, SFTP parallelism, SFTP timeouts, AI provider,
  etc.) now persist to the encrypted vault and restore on launch.
- **Tab bar overflow handling** — tabs compact to a min width as the
  bar fills; active tab keeps natural width; beyond that the strip
  becomes invisibly scrollable (mouse wheel scrolls horizontally), and
  a `⋯` button surfaces a Termius-style "Jump to" modal listing all
  open tabs + Quick connect entries (`Ctrl+J`).
- **AI chat error treatment** — provider/network failures render as a
  red bubble with a Retry button instead of a fake assistant message;
  errors are filtered out of the history sent to the model on retry.
- **Linux packaging**: `.deb` (cargo-deb) and `.AppImage` (linuxdeploy)
  added to the release pipeline alongside the existing `.tar.gz`.
- **SFTP integration tests** (`tests/sftp_integration.rs`) using
  testcontainers and `linuxserver/openssh-server` — gated behind
  `#[ignore]`; run with `cargo test -- --ignored`.
- **Property-based tests** for path / name helpers (`unique_entry_name`,
  `parent_path`, `remote_join`) using `proptest`.

### Fixed
- **Connect timeouts now actually fire on the SFTP picker path** —
  `connect_with_resolver` was bypassing the per-phase timeout wrappers
  and falling through to the kernel's ~127s SYN-retransmit ceiling
  (OS error 110). Auth and session-open phases also timed out on
  misbehaving servers.
- **`cp` exit-status read race** — the exec channel's `Eof` was
  arriving before `ExitStatus`, the loop was breaking on Eof and
  defaulting to exit 255 even on success. Now reads until channel
  close.
- **Retry button after a failed connect** was a silent no-op (the
  SFTP nav handler bailed when there was no client). Now `Retry`
  re-runs the full pick flow.
- **AI chat retry pop-stacking** — retry now pops the trailing error
  + the user message that triggered it before re-dispatching, so
  history doesn't grow duplicate user messages on each retry.
- **Vault DB / export file / edit-in-place temp files** all chmod
  0600 on Unix at write time (defense in depth — the export is
  already age-encrypted, the vault is at-rest encrypted, but tightening
  the perms keeps casual local-user reads at bay).
- Removed a debug-session test from `crates/oryxis-ssh/src/engine.rs`
  that contained a hardcoded production password and IP. History was
  rewritten via `git filter-repo` and force-pushed; affected
  credentials were rotated. (Lesson noted in the project memory:
  scripts that touch real infrastructure must live outside the repo
  tree, `#[ignore]` does not protect the source bytes from `git log`.)
- Various pre-existing test warnings cleaned up so
  `cargo clippy --workspace --all-targets -- -D warnings` passes.

### Changed
- `SshEngine` gained `with_connect_timeout` / `with_auth_timeout` /
  `with_session_timeout` builder methods; `SftpClient` carries a
  shared atomic op-timeout that the settings panel mutates live.
- `SftpClient::open_sibling()` opens an independent SFTP subsystem
  channel on the same SSH connection — backbone of the parallel
  transfer pool.
- `Vault::open` now applies `chmod 0600` to the SQLite DB and its WAL
  / SHM sidecars on Unix.

## [0.3.3] - 2026-04-23
- CI: NSIS install / packaging fixes for Windows.

## [0.3.2] - 2026-04-22
- CI / packaging adjustments.

## [0.3.1] - 2026-04-21
- CI / packaging adjustments.

## [0.3.0] - 2026-04-20
- Initial 0.3 baseline (pre-SFTP).
