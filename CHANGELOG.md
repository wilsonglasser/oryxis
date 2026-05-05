# Changelog

All notable changes to Oryxis are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project uses [SemVer](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Proxy Identities** — reusable SOCKS5 / SOCKS4 / HTTP CONNECT proxy
  configurations editable under `Settings → Proxies`, linkable from any
  host via the host editor's integrated proxy picker. Password stored
  in its own encrypted column.
- **Authenticated proxies** — SOCKS5 username/password (RFC 1929) and
  HTTP CONNECT Basic auth (RFC 7617). Proxy credentials live in the
  encrypted `proxy_password` column, never in the plaintext `proxy`
  JSON.
- **Jump host + proxy stacking** — a jump host that itself sits behind
  a proxy now dials through that proxy on the first hop; subsequent
  hops keep using the SSH tunnel.
- **`~/.ssh/config` import** — `ProxyCommand` is mapped to a typed
  `Command` proxy; `ProxyJump alias` is auto-resolved against other
  imported aliases (unresolved aliases land in `Connection.notes` for
  manual fix).
- **Opt-in password sync** — new toggle in `Settings → Sync` mirrors
  connection / identity / proxy passwords across paired devices when
  on (off by default). Wire format is forward + backward compatible
  with older peers.
- **Portable export round-trips proxy data** — `.oryxis` files now
  carry `ExportProxyIdentity` rows and `ExportConnection.proxy_password`,
  so a fresh device imports working proxy auth out of the box.
- **Persian (فارسی) and Arabic (العربية) UI translations** — both
  fully translated. `Language::is_rtl()` covers both.
- **Layout direction setting** — `Settings → Theme` exposes Auto /
  Left-to-Right / Right-to-Left. Auto follows the active language;
  explicit values override regardless.
- **Workspace-wide RTL layout pass** — `widgets::dir_row` and
  `dir_align_x()` mirror sidebars, tab bar, host / key / identity /
  folder cards, history rows, settings sidebar, keychain split button
  corners, and window controls under RTL. `panel_right_*` icons swap
  in for the sidebar collapse toggle.

### Changed
- **Folder, key and identity cards now hide the `⋮` menu until the
  card is hovered**, matching the existing host-card behaviour. Keeps
  the cards clean at rest and stops the button from competing with
  trailing-edge text under RTL.
- **Keychain scrollable padding** trimmed so the scrollbar reads as
  flush against the panel edge instead of floating in dead space.
- **Sidebar nav** is now wrapped in a `scrollable` so the bottom
  entries stay reachable when the window is short enough to clip the
  list.

## [0.5.5] - 2026-04-28

### Fixed
- **winget validation failed with `STATUS_DLL_NOT_FOUND` (0xC0000135)**
  on `oryxis.exe` and `oryxis-mcp.exe` — the MSVC toolchain dynamically
  linked the binaries against `vcruntime140.dll` / `msvcp140.dll`, which
  the winget validation sandbox doesn't ship. Switched Windows builds to
  static-link the C runtime via `.cargo/config.toml`
  (`-C target-feature=+crt-static` for `cfg(target_env = "msvc")`), so
  the binaries no longer depend on VC++ Redistributable being installed.

## [0.5.4] - 2026-04-28

### Fixed
- **Auto-updater "No installer asset for this platform"** — the asset
  matcher demanded the substring `windows` in the filename, but the
  release pipeline ships the installer as `oryxis-setup-x86_64.exe`
  (no `windows` in the name). Match now keys on the actual filename
  shape per `(os, arch)` pair: `setup`+`x86_64`+`.exe` on Windows
  x64, the portable `.zip` on Windows arm64, the AppImage on Linux,
  and the macOS arm64 tarball. Existing v0.5.3 installs still need
  one manual update to land this fix; future updates auto-detect.

## [0.5.3] - 2026-04-28

### Fixed
- **Windows installer reported the wrong version in Add/Remove
  Programs** — `DisplayVersion` was hardcoded to `0.3.3` since that
  release and never bumped, so every Oryxis install since then
  showed up as 0.3.3 in Windows' programs list. Now driven by a
  `/DVERSION=…` define from the release workflow (`github.ref_name`
  with the leading `v` stripped), and the same value populates
  `VIProductVersion` / `FileVersion` / `ProductVersion`.

### Changed
- **NSIS uninstall registry key gained `QuietUninstallString`,
  `InstallLocation`, `URLInfoAbout`, `HelpLink`, `NoModify`,
  `NoRepair`** — required / recommended fields for winget to
  detect and validate the install.

## [0.5.2] - 2026-04-27

### Added
- **Rounded window corners on Windows 11** — undecorated chrome now
  opts into the DWM corner-preference API (`CornerPreference::Round`)
  with the matching `undecorated_shadow`, so the window edge is
  rounded the same way every native Win11 app is. Win10 and other
  platforms unchanged.
- **Double-click on the title bar toggles maximize** — Aero-snap
  convention; matches the maximize chrome button. Also added on the
  top/bottom edge resize handles to fill the **current** monitor's
  height (multi-monitor setups no longer jump to the primary). E/W
  edges stay drag-only — Windows itself has no horizontal-fill
  gesture.
- **Async Local Shell detection** — `where pwsh.exe` and
  `wsl --list --quiet` run on a blocking thread instead of stalling
  the UI. The picker now opens instantly with a "Detecting shells…"
  hint while the probe finishes (i18n in all 9 languages).
- **Distro / shell icons in the tab chip** — Local Shell tabs now
  show the brand glyph for the underlying shell: Ubuntu / Debian /
  Alpine / Kali / Arch / openSUSE / NixOS / etc. for WSL distros,
  the Lucide terminal in Windows blue for PowerShell / cmd, and a
  Docker container icon for `docker-desktop`. Driven by a label
  parser, no extra config.
- **Smart contrast** — when an app picks a foreground / background
  pair that renders too close to vanish (PowerShell's
  `$PSStyle.FileInfo.Directory` blue-on-blue, LS_COLORS' `ow`
  green-on-green over a green-tinted palette), the renderer flips
  the foreground to white or near-black depending on background
  luminance so the text stays legible. Settings → Terminal toggle
  + i18n in all 9 languages; opt-out for colour-precise tools.
- **Website link in About** → [oryxis.app](https://oryxis.app/).
- **PTY spawn tracing** — `Spawned local shell …` / `PTY first
  output …` / `PTY EOF …` logs at `info` so a blank-terminal symptom
  can be triaged from a console run without breakpoints.

### Changed
- **iced fork bump** to `oryxis` branch (= `text-selection +
  monitor-position` merged). Adds `iced::window::monitor_position`
  alongside `monitor_size` so the new vertical-fill gesture lands on
  the right monitor, and pulls in the upstream-bound text-selection
  PR's refactored `Selectable` trait + cross-widget grouping.
- **Window drag / resize-drag press debounce (300ms)** — iced's
  `MouseArea` re-fires `on_press` on the second click of a
  double-click, and forwarding two `iced::window::drag(...)` calls
  raced our follow-up `toggle_maximize` / vertical-fill resize
  (window snapped right back). The debounce swallows the spurious
  second press cleanly.

### Fixed
- **Local Shell terminal stayed blank on Windows** — the alacritty
  emulator emits `Event::PtyWrite` for replies it owes the host
  (e.g. ConPTY's `\x1b[6n` cursor-position request). Our
  `EventProxy` was dropping that event, so ConPTY blocked after the
  first 4 bytes and cmd.exe / wsl.exe never painted a banner. PTY
  writes are now centralised on a dedicated writer thread driven by
  one mpsc channel — both user keystrokes and emulator replies
  flow through the same path, no races on the underlying handle.
- **Local Shell picker subprocess flicker** — `where.exe` and
  `wsl --list --quiet` now spawn with `CREATE_NO_WINDOW`, so the
  detection probe doesn't briefly flash a console window behind
  oryxis on each open.

## [0.5.1] - 2026-04-27

### Added
- **WSL `\\wsl$` SFTP listing via `wsl.exe -l -q`** — the Local pane
  used to fall over with `os error 3` (UNC server-only paths can't
  be enumerated by `read_dir`). Now the WSL UNC root synthesizes
  distro entries from the WSL CLI; clicking a distro descends into
  it the normal way.
- **`ORYXIS_TERM_PERF=1` perf overlay** — opt-in HUD top-right of
  every terminal showing FPS + per-phase timings (lock acquire,
  cell pass, syntax highlight, total) plus the rolling max over the
  last ~120 frames. Lets you spot draw-time spikes that read as
  typing lag without instrumenting from outside.

### Changed
- **SFTP breadcrumb separator picks per path flavor** — `\` for
  real Windows volumes (`C:\`, `D:\`), `/` for Unix paths and WSL
  UNC (which is Linux underneath). No more `C: / Users / wilso`.
- **SFTP path bar covers full width** — the breadcrumb's MouseArea
  was shrinking to the visible crumbs; clicks on the gutter were
  hitting nothing. Wrapped in a `Fill` container so the whole bar
  acts as "click to edit", matching Finder / Explorer.
- **Drives dropdown closes on selection** — `SftpNavigateLocal`
  now clears `local_drives_open` (and the action menus) so the
  overlay doesn't linger after the click.

### Fixed
- **SSH key import failing on Windows-saved PEM files** — Notepad
  and some PowerShell redirects write a UTF-8 BOM at the start of
  the file. The PEM parser saw bytes before `-----BEGIN…` and
  failed with `PEM Base64 error: invalid Base64 encoding`. Strip
  the BOM (and the existing CRLF normalization stays). New tests
  cover both BOM and CRLF.
- **Terminal typing lag on hover** — URL hover detection ran
  `url_at_cell` on every mouse pixel (locking the terminal mutex on
  each pass). Under typing + cursor over the canvas, that contended
  with the SSH-echo `state.process` and showed up as input delay.
  Now caches the last `(col, row)` and only re-runs the scan when
  the cursor crosses a cell boundary.
- **Terminal URL tooltip transparent** — `Color { a: 0.92, ..bg }`
  let the underlying URL text bleed through; switched to solid
  `palette.background` and added 8 px right padding so the label
  reads cleanly.

## [0.5.0] - 2026-04-26

### Added
- **Local Shell picker on Windows** — Ctrl+T (or the `+` button)
  surfaces a Termius-style menu listing PowerShell (prefers `pwsh`),
  Command Prompt, and every installed WSL distro. Each entry spawns
  the shell directly via portable-pty's `CommandBuilder`. Non-Windows
  platforms still get the OS default shell with no menu.
- **Ctrl+Click to open URLs** in the terminal — plain clicks now
  start a selection like any other cell (matches Termius); the
  Ctrl-modifier gates link-follow. Hovering a URL switches the
  cursor to `Pointer`, underlines only that URL, and renders a
  "Ctrl + Click to open the link" tooltip near the cursor.
- **Risk-aware AI tool gate** — `bash` tool calls are classified as
  read-only / mutating / destructive, with a per-message Run /
  Always run / Deny prompt before execution. "Always run" persists
  per-tab so you don't re-confirm the same `ls` / `cat` runs.

### Changed
- **AI chat layout polish** — assistant bubbles span full width,
  hover-revealed Copy button, code blocks have inline Copy / Play
  affordances (Play skips the risk gate when manually triggered),
  toast floats over the panel instead of pushing content. Tool-call
  responses no longer eagerly produce empty bubbles.
- **UI consistency pass** — tab-bar `+` and `⋯` buttons now use the
  Lucide glyph at the same chrome dimensions; SFTP context-menu
  icons take the same accent tint as host-card menus; Local Shell
  dropdown matches the Drives dropdown. WSL drive detection added
  to the SFTP local-path picker.
- **Kali Linux brand color** in `os_icon.rs` bumped to a
  recognizable blue (was a washed-out tone).

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
