# Changelog

All notable changes to Oryxis are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project uses [SemVer](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.7.4] - 2026-06-01

### Added
- **Graphics renderer picker.** Settings -> Interface gains a renderer
  selector: Automatic (default), OpenGL (GPU) or Software (CPU). Some
  GPU/driver stacks (notably Vulkan on Mesa under GNOME) corrupt the
  wgpu surface, bleeding other windows' pixels into the app chrome while
  a terminal session forces frequent redraws. That corruption lives in
  the driver's swapchain/present path, below iced, so it cannot be
  repainted away from our side; instead the picker lets you change the
  render path. OpenGL stays hardware-accelerated while dodging most
  Vulkan-on-Mesa bugs, and Software (tiny-skia) is the maximally
  compatible fallback (the terminal is a `canvas` widget, so it renders
  identically off the GPU). The choice maps to `WGPU_BACKEND` /
  `ICED_BACKEND` at startup and takes effect after a restart. Addresses
  the GNOME / Debian rendering glitch reported in #25.
- **macOS `.dmg`.** The release pipeline now packages a proper
  `Oryxis.app` bundle (`Info.plist` + `.icns`) into a `.dmg` for Apple
  Silicon, alongside the existing tarball. Developer ID signing and
  notarization engage automatically once the Apple secrets are present;
  until then the app is ad-hoc signed so it still launches locally.

### Changed
- Bumped `russh` 0.60.3 -> 0.61.1 and `astral-tokio-tar` 0.6.1 -> 0.6.2.

## [0.7.3] - 2026-05-28

### Added
- **Mouse reporting (xterm mouse tracking).** When a remote app turns on
  mouse tracking (tmux `set -g mouse on`, vim `set mouse=a`, htop, less,
  lazygit, ...) the terminal now reports clicks, drags and wheel events
  to it, so selecting a pane, resizing a split by dragging, and clicking
  menu items work like they do in any other terminal. Supports the SGR
  (1006) and legacy X10 protocols and the click / drag (1002) / any-motion
  (1003) tracking modes. Holding **Shift** bypasses reporting and falls
  back to local text selection, the universal terminal escape hatch.
  Also fixes wheel-scroll in alt-screen apps (vim / less / htop) over SSH,
  which previously only worked on local-shell tabs.
- **Nightly update channel.** Settings -> Updates gains a channel picker
  (Stable / Nightly). On the nightly channel the in-app updater follows
  the rolling `nightly` release, comparing the running commit against the
  release's target commit (version numbers don't move between nightlies)
  and installing the new build in place, no installer, no UAC prompt.
  Switching back to Stable offers a clean tagged build immediately so you
  never get stranded on a nightly binary. The build's commit + channel are
  baked in at compile time.

### Changed
- App logo is now a vector (`resources/logo.svg`) embedded at compile
  time and rendered via the `svg` widget on the lock / setup screens and
  the tab-bar product mark, so it stays crisp at any DPI.

## [0.7.2] - 2026-05-27

### Added
- **Right-click-to-copy selection mode.** A sub-option of copy-on-select
  (the Windows console "QuickEdit" model): when on, a finished selection
  no longer auto-copies on mouse release; a right-click over a live
  selection copies it, and a right-click with no selection still pastes.
  No-op while copy-on-select is off. Shown as an indented sub-toggle
  under copy-on-select in Settings -> Terminal.
- **Copy/install MCP config into a WSL client (Windows).** The MCP setup
  panel gains a Native / WSL target toggle. With WSL selected, Copy JSON
  and Install express the binary as its `/mnt/c/...` mount path so a
  Claude Code / Cursor instance running inside a WSL distro can reach it;
  Install merges the entry into the distro's `~/.claude/.mcp.json` via
  `wsl.exe`.

### Fixed
- Linux: set `WM_CLASS` / Wayland `app_id` so GNOME resolves the app
  icon instead of falling back to a generic placeholder.

## [0.7.1] - 2026-05-25

### Added
- **Terminal side panel with tabs.** A panel toggle in the tab bar (right
  of `+`) opens a sidebar with **Chat** (when AI is enabled) and
  **Snippets** tabs, replacing the standalone chat toggle and the
  redundant host-search button.
  - Snippets tab: inline New / Edit editor (no context switch to the
    workspace), an expanding search field, a sort popover (A-z / Z-a /
    newest / oldest), and per-row Edit / Paste (no newline) / Run
    (+ Enter). Action icons float over the row and reveal on hover; rows
    show a single ellipsized command line.
  - **Built-in "Apply sudo password"** action: types the active host's
    stored password + Enter (e.g. to answer a `sudo` prompt). Shown only
    for a live SSH session, never written to the session log.
- **Per-host environment variables.** Sent to the remote shell via SSH
  `setenv` before the shell starts (most `sshd` accept only `LC_*` /
  `LANG_*` unless `AcceptEnv` is widened). Editable in the host editor;
  rides along with connection sync and portable export.
- **Per-host terminal encoding.** Transcodes the PTY stream to and from
  UTF-8 for legacy charsets (Big5, GBK, gb18030, Shift_JIS, EUC-JP,
  EUC-KR, ISO-8859-*, windows-125x, KOI8-R) via `encoding_rs`. UTF-8
  hosts are pure passthrough.
- **Theme preview in the host editor.** The Terminal Theme selector
  always shows a palette swatch preview now, including a preview of the
  inherited global theme for the "use global" state.
- **Connect-screen redesign.** Vertical timeline instead of the
  horizontal step bar, a selectable connection log with a Copy logs
  button, the host badge following the configured icon / color, and Edit
  Host moved into the header.
- **Keyboard navigation in the host editor** (Tab between fields, Enter
  to save).

### Fixed
- Windows: embed the Common Controls v6 manifest so native controls are
  themed.
- Windows: suppress plugin console windows; clear stale connect progress.
- Hover bleed-through under modal scrims.
- Terminal tab markers rendered as tofu boxes.

### Changed
- Bumped `russh` to 0.60.3; dropped `tray-icon` default features
  (clears a glib 0.18 advisory); skip the empty-password KDF on boot.

## [0.7.0] - 2026-05-19

### Added
- **Windows system tray** (closes the last item from issue #18).
  Tray icon registers on app start with a menu that grows as state
  changes:
  - Static actions: Show Oryxis / Hide to tray / Quit.
  - "Active sessions" submenu: one item per open terminal tab,
    click activates the tab + pops the window.
  - "Recent hosts" submenu: top 10 saved connections by last_used
    desc (connections never connected to are filtered out), click
    opens a new tab against that host.
  - Settings -> Interface -> System tray panel: opt-in close-to-
    tray (custom title bar X + Alt+F4 hide instead of close) and
    minimize-to-tray (title bar minimize hides instead of taskbar-
    minimize). Defaults off.
  - Single-instance guard via named mutex so duplicate launches
    don't spawn a second tray icon. JumpList + IPC for routing
    `--connect <uuid>` into an existing instance ship in v0.7.1.
  - macOS / Linux: tray module is a no-op stub, settings panel is
    suppressed, app behaves exactly like v0.6.
- **Cloud providers UX redesign (Phase 1-5).** Replaces the rigid
  v0.6 "everything goes into a provider folder, never editable"
  model with a decoupled origin-as-metadata pattern (cloud_ref
  stays as backpointer; group_id, label, color, icon all
  user-owned post-import).
  - **Multi-region per AWS profile.** Wizard accepts a chip list
    of regions; backend already supported fan-out, now exposed.
    New profiles prefill the chip with `AWS_REGION` env var or the
    `[default]` profile's `region` in `~/.aws/config` when
    available, so single-region devs don't see an empty form.
  - **Import-into picker** in the Discover modal. Floating
    autocomplete combo with a search field opens above the input;
    typing a brand-new name creates the folder on the spot. No
    more being trapped in the auto provider folder.
  - **Filter chip** at the top of the dashboard: click "Filter by
    cloud profile" on any host kebab and the grid dims down to
    only that profile's items (lens model, not a separate sidebar
    section). Brand badge on every cloud-sourced host card.
  - **Sticky reimport.** `customized_fields` column on
    `connections` tracks per-field user edits. The new "Sync now"
    action in the cloud profile kebab refreshes every imported
    host of that profile against AWS, preserving any field the
    user has touched. Hosts that vanished upstream get an "Orphan"
    pill + greyed badge; a "Forget" item in the kebab makes the
    intent explicit.
  - **Auto-refresh + auto-archive settings** (`Cloud Sync`
    section): opt-in periodic refresh via an iced subscription,
    opt-in auto-archive of orphans older than N days on boot.
  - **Dynamic group (ECS) is a first-class group now.** Renamable,
    re-parentable, color/icon via the same shared picker as the
    host editor. Cloud-source query (cluster/service/container)
    became editable in-place.
  - **Container view enrichment.** ECS task rows show container
    name + task definition revision + status pill (RUNNING green,
    PENDING amber, STOPPED red) + private IP + AZ + started-at
    relative (`5m ago`). Data was already in `DescribeTasks`,
    just not surfaced.
  - **Multi-container ECS tasks expand Lens-style.** Leave the
    Container field empty in the dynamic group editor and the
    resolver emits one row per container in every matching task
    (was: one row per task, filtered to a single named
    container). Connect + Copy CLI both target the specific
    container the user clicked. Backwards-compatible: existing
    single-container imports keep their original behaviour
    because their `container` field is non-empty.
  - **Copy `aws ecs execute-command`** action on every ECS task
    row. Small clipboard icon overlay on the trailing edge that
    copies the full CLI invocation (region + cluster + task id +
    container) so power-users can paste into a terminal with the
    AWS CLI installed. Region is plumbed via a new field on
    `DiscoveredHost`.
- **Shared group picker combo** (input + chevron + floating
  search popover) on the Parent Group fields of both the host
  editor and the dynamic group editor. Backed by a small
  reusable `bounds_reporter` widget so the popover anchors at
  the actual on-screen rect of the input (no hardcoded layout
  math). Typing a brand-new name still creates the group on
  Save.
- **Update check feedback as a toast.** "Check for updates now"
  from the burger menu now surfaces a transient toast
  ("Checking…" → "You're on the latest version" or "Update
  available: vX.Y") so the action doesn't look like a no-op when
  fired from outside Settings.
- **Tab badge always renders as a rounded square** regardless of
  the global `default_host_icon` style. Circular badges read as
  pills inside the narrow tab strip; locking the tab shape keeps
  the strip uniform while leaving dashboard cards free to honour
  the user's preference.
- **`Tint tab underline with host accent`** toggle in Settings →
  Interface. Off collapses the 2 px tinted hairline under the
  tab strip to a flat 1 px neutral border across all screens.
- **Workspace layout mode** (new default). Hides the sidebar entirely
  and promotes navigation to the top tab bar: Hosts and SFTP sit as
  area tabs before the connection tabs, the burger menu (top-left)
  covers the remaining vault surfaces (Keychain, Snippets, Known
  Hosts, History, Settings, Local Shell, Updates). Terminal sessions
  get the full canvas width. Classic mode stays available as a
  one-click switch in Settings -> Interface for anyone who prefers
  the old sidebar.
- **Settings -> Interface section** absorbing the old Theme section
  and adding: status bar toggle, tab close button position
  (Left|Right), connection status dot on tabs (green/orange/red),
  Enable SFTP toggle (hides the entry from sidebar + burger), layout
  mode picker (Workspace/Classic), default host icon style picker.
- **Customizable host icons.** Per-host shape override (Circular /
  Square / Outline / Initials) with a global default in Interface
  settings. Rendered consistently on dashboard cards and tab badges.
  Migration: `connections.icon_style TEXT` added.
- **Dynamic accent on the chrome.** When a tab pointing at a saved
  connection is active, the active-tab fill, label, close-X color
  and the 2 px hairline under the tab strip all adopt the host's
  per-host `color`. JetBrains-style "respiração" so you can tell
  prod-vs-dev tabs apart at a glance without reading labels.
- **Burger menu** (`☰`) at the leading edge of the tab bar with full
  navigation list + Settings / Updates / Local Shell entries.
- **Solarized Dark theme** as an `AppTheme` choice. Terminal palette
  already existed; UI palette mirrors `Solarized Light`.
- **System monospace font enumeration** via `fontdb`. The Terminal
  font picker now lists every monospace family installed on the
  host instead of the hardcoded 20-name array, with a static
  fallback when the scan returns nothing.
- **MCP server is now a plugin.** `oryxis-mcp` no longer ships inside
  the OS installers (`.deb`, AppImage, tarballs, NSIS); the app
  downloads it on demand into `~/.oryxis/bin/oryxis-mcp[.exe]` when
  the user enables MCP for the first time, via the same Ed25519-signed
  manifest pipeline cloud plugins use (`mcp-v*` release tags publish
  `mcp.json` + signed per-platform binaries). v0.6 users with the
  toggle already on get a silent migration on first boot. External
  MCP clients (Claude Desktop, Code, Cursor) spawn the stable
  launcher path the install layer maintains, so their existing config
  keeps working across plugin updates.

### Changed
- **P2P sync protocol version 4 (breaking).** `PairingRequest` and
  `PairingAccepted` now carry the sender's `device_id`,
  `PairingRequest` also carries the joiner's `listen_port`, a new
  `PairingChallenge` / `PairingResponse` round proves the joiner
  holds the private key for the public key it sent (pairing runs
  before any peer pubkey is persisted, so the Hello channel-binding
  can't be reused here), and both pairing messages exchange ephemeral
  X25519 public keys to derive a per-pair shared secret. From then on
  every `SyncRecord.payload` is sealed with ChaCha20-Poly1305 under
  that secret. Older devices cannot pair or sync with v4 devices;
  both ends must be on Oryxis 0.7+ for sync to work.

### Added
- **P2P sync is now actually operational.** Previous releases shipped
  the UI over an orphaned engine; this release wires the engine into
  the app lifecycle and covers both LAN and cross-network paths:
  - Engine spawns when sync is toggled on and stops cleanly on toggle
    off. A dedicated `SyncRuntime` opens its own `VaultStore` handle
    on the same SQLite file; concurrent access is safe under WAL +
    `busy_timeout`.
  - Deletes propagate: every syncable `delete_*` records a tombstone
    in `sync_metadata`; the manifest surfaces tombstones; the
    receiver applies the delete and records a fresh local tombstone
    so the deletion keeps travelling onward.
  - Two-sided pairing handshake: host shows a 6-digit code (single
    shot, 5-minute TTL), joiner provides the code + the host's
    address, and both sides persist each other on success. The host
    address can be typed (`ip:port`), pasted as an `oryxis://pair/...`
    link (signaling-resolved), or one-clicked from the live discovered
    devices list.
  - Cross-network sync via a self-hostable signaling server:
    when `signaling_url` is configured (settable in Settings > Sync
    > Advanced), the engine STUNs for its public address once a
    minute and re-registers on the signaling server whenever the IP
    changes; the joiner's link flow looks the device id up there to
    get the host's current `ip:port`.
  - HTTP relay fallback for NAT-blocked peers. The same server that
    handles signaling (Cloudflare Worker or `oryxis-relay` binary)
    exposes a `/relay/:id/inbox` long-poll API; when QUIC direct
    can't reach a peer (typical for symmetric / carrier-grade /
    double NAT), both the pairing handshake and the sync session
    automatically fall back to the relay. The relay carries
    ciphertext only — the X25519-derived ChaCha20-Poly1305 seal
    travels with the payload, so a compromised relay learns timing
    but not content. See `SELF_HOSTING.md` for deployment options
    (Worker, Docker image at `ghcr.io/wilsonglasser/oryxis-relay`,
    or `cargo install --path crates/oryxis-relay`).
  - `oryxis-relay` crate: standalone axum HTTP server providing
    signaling + relay endpoints with in-memory per-recipient FIFO
    queues (TTL 300s, 256-frame depth cap), bearer-token auth, and
    a Dockerfile targeting distroless musl. Workflow on `relay-v*`
    tag publishes multi-arch image to GHCR and native binaries to
    the GitHub release.
  - Live mDNS-discovered devices list in the pairing panel, deduped
    by device id, with a Pair button per row that pre-fills the join
    form's address.
  - `Sync Now` actually syncs (was a literal status-string stub).
  - Engine events (peer discovered, sync completed, pairing progress)
    flow into the UI via `Task::stream`; the Settings panel shows a
    live engine-running indicator.

  `SyncConfig.signaling_url` / `signaling_token` are `Option<String>`;
  the build no longer panics when `ORYXIS_SIGNALING_URL` /
  `ORYXIS_SIGNALING_TOKEN` are unset, it just starts LAN-only and the
  user can fill both at runtime (the token has its own input under
  Settings > Sync > Advanced).

  Every `SyncRecord.payload` is now E2E-sealed with the
  pairing-derived shared secret; a compromised signaling relay or a
  TLS bug would no longer expose payloads.

  Tombstones in `sync_metadata` are garbage-collected at engine boot
  (30-day TTL), and re-creating an entity drops any stale tombstone
  for the same id automatically, so the manifest never ships both a
  live entry and a deletion marker for the same row.

  All sync UI strings are translated to all 11 supported locales
  (was previously en / pt-BR / fa / ar only).

### Removed
- Sentry crash/error reporting. Dropped the `sentry` and
  `sentry-tracing` dependencies, the `init_sentry()` boot hook, the
  `SENTRY_DSN` build-time env var, and the matching CI secret in the
  release workflow.

### Fixed
- **Right-click paste in SSH sessions.** The terminal widget's
  right-click handler wrote the clipboard text straight to the local
  PTY, which never reached the SSH session. Fixed by routing the
  paste through the app dispatcher (`TerminalPasteFromClipboard`)
  so it follows the same SSH-first / PTY-fallback path Ctrl+Shift+V
  already used.
- **AI Chat toggle button** no longer renders over the terminal
  canvas when AI is disabled in Settings.
- **Lock Vault button** is hidden when no master password is set
  (locking has nothing to protect in that mode and the unlock screen
  has no way to re-enter), replaced by a muted hint pointing at the
  password toggle.
- **Relay poll loop** stops retrying on permanent HTTP conditions
  (404, 410, 501) instead of looping every 2 s burning network +
  battery. Logs a single warning with the detail. Transient errors
  (5xx, 429, network blips) keep retrying as before.
- Importing an OpenSSH key from PuTTYgen's "Export OpenSSH key (force
  new file format)" no longer fails with "invalid Base64 encoding".
  PuTTYgen wraps the body at 76 chars; `ssh-encoding` requires exactly
  70. On a Base64 error the importer now retries after re-wrapping.

### Security
- **Signaling register / unregister now signed (Ed25519).** Bearer
  token alone is no longer sufficient to write a `device_id` row.
  Every request carries an Ed25519 signature over the canonical
  payload (`oryxis-register-v1` / `oryxis-unregister-v1` domain
  separated), a `signed_at` timestamp checked against a 60 s server
  skew window (replay defence), and the raw 32-byte verifying key.
  The signaling worker, the standalone `oryxis-relay`, and the
  client all build the same canonical bytes and use `verify_strict`
  (RFC 8032 canonical R) so the trust decision is identical across
  Rust and Worker.
- **TOFU pubkey pinning on signaling.** The first register for a
  given `device_id` pins its public key. Later registers from a
  different signer (e.g. another bearer-token holder trying to
  hijack the entry) return 403. Unregister enforces the same:
  only the original key can remove its entry. Implemented in
  `oryxis_relay::discovery::DeviceTable` (in-memory Mutex,
  race-free) and a `DeviceRegistry` Cloudflare Durable Object
  in `signaling-worker/worker.js` (one DO per device_id =
  single-writer, so check-then-pin can't race even under
  concurrent registers from the same bearer-token holder). KV
  for discovery (`device:*` keys) was retired; the relay queue
  (`relay:*`) stays on KV since its append-only profile has no
  TOFU race. Self-hosters get the DO provisioned automatically
  via wrangler migration `v1` on the first `wrangler deploy`.
- **Per-source cap on pairing attempts.** Replaces the old global
  "3 bad codes invalidates the hosted code" with a `HashMap` keyed
  by joiner network identity (`quic:<ip>` or `relay:<device_id>`).
  An attacker grinding the 10^6 code space from one IP can only
  lock themselves out; the legitimate user paired from elsewhere
  keeps a live code. Bounded at 1024 distinct sources to keep the
  map small under sender_id flood.
- **Bounded relay session map (64 entries, FIFO eviction).** The
  inbox demux on the relay client used to spawn an unbounded mpsc
  per fresh `X-Sender-Id`, which a token holder cycling UUIDs could
  exhaust. New entries past the cap evict the oldest session.
- **Pre-auth frame allocation cap (64 KiB).** The QUIC server used
  to honour the declared length on the very first frame, so an
  unauthenticated dialer could force a 16 MiB allocation per stream
  before any signature check. Hello / HelloAck reads now reject
  frames larger than 64 KiB; post-auth reads keep the 16 MiB cap.
- **Tombstone GC waits for every active peer to catch up.**
  `vacuum_tombstones` now requires `last_synced_at >= deleted_at`
  on every active `sync_peer` before reclaiming the row, closing
  the silent-resurrection bug class (a tombstone could be vacuumed
  while an offline peer was still behind it, then the peer would
  re-sync the entity back into existence).
- **Mutex-poison recovery on relay routing maps.** A panicked
  session task no longer poisons the shared session map and kills
  the whole relay demux; the routing table is recovered via
  `into_inner()` and the offending peer is just dropped.
- **Plugin install errors translate.** Install failures surface
  through stable `plugin_err_*` i18n keys (translated across all
  11 languages) instead of raw `Display` text. Raw detail still
  goes to the log file for debugging without polluting the UI or
  leaking file paths / HTTP codes.

## [0.6.1] - 2026-05-11

### Added
- **PuTTY `.ppk` import** (v2 and v3, RSA / Ed25519 / ECDSA P-256 /
  ECDSA P-384, encrypted or not). Hand-rolled parser: v2 uses SHA-1
  KDF + AES-256-CBC + HMAC-SHA-1, v3 uses Argon2id/i/d + AES-256-CBC +
  HMAC-SHA-256. Verified byte-for-byte against fixtures emitted by
  the real `puttygen` binary (`crates/oryxis-vault/tests/fixtures/ppk`).
- **Encrypted PKCS#8 import** (`BEGIN ENCRYPTED PRIVATE KEY`, RFC 5958
  PBES2). Passphrase prompt fires on file pick, same flow as
  encrypted OpenSSH keys.
- **Ed25519 in PKCS#8** (OID `1.3.101.112`, RFC 8410). Previously
  only Ed25519 inside OpenSSH wrappers loaded.

### Fixed
- DSA and ECDSA P-521 keys no longer silently mislabel as Ed25519 /
  P-256 when imported via OpenSSH. They return an actionable
  `UnsupportedKeyKind` error so the UI can show the right message.
- Legacy OpenSSL-encrypted PEM (`Proc-Type:4,ENCRYPTED` + `DEK-Info:`)
  now surfaces a dedicated error pointing the user at the new `.ppk`
  path or `ssh-keygen -p`, instead of a generic crate-internal string.

### i18n
- Two new keys (`key_encrypted_legacy_pem`, `key_unsupported_kind`)
  translated across all 11 languages.

## [0.6.0] - 2026-05-10

### Added
- **AWS Cloud Accounts** — first-class cloud provider integration. New
  `Settings → Cloud` panel manages encrypted `CloudProfile` rows; three
  AWS auth flavors are supported (named profile from `~/.aws/config`,
  static access key + secret + optional session token, IAM Identity
  Center / SSO via `aws_config::SsoCredentialsProvider`). Each profile
  carries a "Test credentials" button that hits `sts:GetCallerIdentity`
  in-line so misconfigurations surface before discovery. Secrets live
  in the same per-field encrypted column model as identity passwords.
- **Discovery & Import** — from the Hosts toolbar, "+ Host [▾] →
  Discover" opens a side panel that lists every EC2 instance and ECS
  service the profile can see, grouped by region (EC2) and by region /
  cluster (ECS). The panel filters live, hides empty sections, greys
  out already-imported entries, and exposes per-row checkboxes. The
  import action confirms via a transport-pick modal when at least one
  EC2 row is selected (SSH / EC2 Instance Connect / SSM Session); pure
  ECS imports skip the modal since dynamic groups always use ECS Exec.
- **Provider folder layout** — every imported entity nests under a
  single top-level folder named after the cloud profile (`prod-aws`,
  `staging`, …). EC2 hosts get the folder as their `group_id`; ECS
  services materialize as **dynamic groups** (`Group` rows with
  `cloud_query`) parented under it. Renaming the cloud profile renames
  the matching provider folder automatically.
- **EC2 Instance Connect transport** — the connect flow detects an
  imported EC2 host with `transport_pref = InstanceConnect`, pushes a
  one-shot SSH public key through `ec2-instance-connect:SendSSHPublicKey`,
  then completes the handshake with the linked SSH key. AMI-aware OS
  user inference (Amazon Linux → `ec2-user`, Ubuntu → `ubuntu`,
  Debian → `admin`, etc.) keeps connections one-click after import.
- **SSM Session for EC2** — `transport_pref = Ssm` opens an SSM
  Session through the bundled `session-manager-plugin`. No public IP
  or open port required; private subnets work out of the box once the
  instance has the SSM agent + IAM permissions.
- **ECS Exec into a live container** — dynamic groups expand on click
  to list the running tasks; selecting a task starts an interactive
  `aws ecs execute-command` session into the configured container,
  streaming through the Session Manager plugin. The dynamic-group
  editor lets you pin transport, OS user, initial command, key and
  identity per (service, container) tuple.
- **Brand SVG icons** — `resources/icons/brand/` ships native SVGs for
  AWS, ECS, Kubernetes, Docker, Linux distros, BSDs, macOS, Windows,
  Proxmox, OPNsense, OpenWrt, Raspberry Pi and friends. Provider
  folders and dynamic groups render the corresponding glyph in the
  card and breadcrumb. The previous SimpleIcons font subset
  (1.5 MB `.ttf`) was retired.
- **Per-host initial command** — host editor exposes an "Initial
  Command" field at the bottom of the SSH section. After auth, the
  command is sent to the remote shell as `\n`-terminated keystrokes.
  Useful for hosts that drop into `/bin/sh` when you really want
  `bash`, or for `cd /path` on a shared server.
- **Encrypted SSH key import** — The keychain importer now detects
  passphrase-protected OpenSSH private keys and prompts for the
  passphrase inline, the way Termius / 1Password handle it. The key
  is decrypted once at import time and stored unencrypted inside the
  vault, where the master password's Argon2id + ChaCha20Poly1305
  layer takes over for at-rest protection — there is no per-key
  passphrase prompt at connect time. The form auto-detects encryption
  on file pick (no need to click Save first), shows a "Wrong
  passphrase. Please try again." error on bad input, and refuses to
  save with an empty passphrase ("Enter the key passphrase to
  continue."). PKCS#1/PKCS#8 traditional PEMs that are themselves
  passphrase-protected aren't supported yet — users get a clear
  error instructing them to drop the passphrase first
  (`ssh-keygen -p -f <file> -N ''`).
- **Windows per-user installer** — `oryxis-user-setup-x86_64.exe` and
  `oryxis-user-setup-aarch64.exe` install Oryxis under
  `%LOCALAPPDATA%\Programs\Oryxis` with `HKCU` registry entries and no
  UAC prompt, mirroring VSCode's user-installer pattern. Useful on
  locked-down corporate machines and for unattended auto-updates. The
  per-user setup detects an existing system install side-by-side and
  warns (does not auto-uninstall). The system installer
  (`oryxis-setup-*.exe`) keeps its previous behavior; `winget install`
  continues to target it.
- **Windows ARM64 installers** — `oryxis-setup-aarch64.exe` (system)
  and `oryxis-user-setup-aarch64.exe` (per-user) ship alongside the
  existing portable `.zip`. The installer stub is x86 (NSIS upstream
  ships no native ARM64 makensis), but the binaries laid down are
  native ARM64, so the emulation cost applies only during install.
- **`PATH` registration** — both installer flavors add `INSTDIR` to
  `HKLM\Environment\Path` (system) or `HKCU\Environment\Path`
  (per-user) via the EnVar plugin, so `oryxis` and `oryxis-mcp` now
  resolve from any shell — relevant for the MCP server, which
  external clients (Claude Desktop, etc.) typically wire by name.

### Changed
- **Responsive card grid across all list screens** — Hosts, keys,
  identities, snippets and cloud accounts swapped their hard-coded
  3-column tiling for a shared helper that recomputes the column
  count from the current available width on every render. Cards
  flex to fill the row (`Length::Fill`) and rewrap when the user
  resizes the window or opens a side panel — previously the third
  card just clipped off-screen. Long labels truncate cleanly via
  `Wrapping::None` + a `clip(true)` container instead of breaking
  the card geometry.
- **Standardised card row-actions** — Snippets, keys and identities
  switched their "edit" / "more" affordance to the same vertical
  ellipsis (⋮) glyph, 22 px reserved slot, hover-only visibility
  used by hosts and cloud profile cards. The four card families now
  read identically.
- **Split-button dropdowns anchor to the button** — "+ ADD ▼"
  (keychain) and "+ Host [▾]" (cloud provider picker) now drop
  below the chevron at a fixed screen position derived from the
  toolbar geometry, instead of following the cursor. Both menus
  open in the same spot regardless of where the user clicked.
- **Overlay menu minimum height** — Single-item dropdowns no longer
  render shorter than the button they dropped from. A 32 px floor
  is enforced via a Stack-backed spacer (iced 0.13 has no
  `min_height` on container).
- **Settings → Terminal section reordered** — Visual customisations
  (font size, font, theme) moved to the bottom of the section, with
  theme last. Behaviour toggles, keepalive, scrollback, reconnect,
  OS detection and updates come first. The theme picker switched
  from a single tall column to a 2-column responsive grid; cards
  keep the swatch-+-name design, just paired side-by-side.
- **`winget` submission covers both architectures** — the winget
  manifest now lists both `x86_64` and `aarch64` system installers in
  a single submission via Komac's PE-header detection.

### Fixed
- **Renaming a cloud profile didn't rename its provider folder** — the
  link between `CloudProfile` and the provider folder was by label
  only. Editing the profile name in the wizard now propagates the new
  label to the matching `Group` (filtered by `cloud_query.is_none()`
  so dynamic groups with the same name aren't touched). A stable
  `cloud_profile_id` column on `Group` is on the v0.7 list.
- **Missing `session-manager-plugin` failed silently** — clicking an
  ECS task or starting an SSM Session without the AWS CLI plugin
  installed used to log to stderr and do nothing visible. A blocking
  modal now surfaces the missing dependency with a direct link to the
  AWS docs install page (per-OS instructions). Same dialog covers ECS
  Exec / SSM start failures coming back from the AWS SDK so the user
  can read the SDK message verbatim and fix the IAM / config gap.
- **Auto-update on Windows failed with "os error 740"** — the updater
  used `CreateProcess` to launch the downloaded NSIS installer, which
  ignores the executable's manifest and refused to launch the
  elevated system installer with `ERROR_ELEVATION_REQUIRED`. Updater
  now uses `ShellExecuteW`, letting the manifest control elevation
  (UAC for the system installer, no prompt for the per-user one).
- **Window resize event flood** — `Message::WindowResized` quantises
  the incoming size to an 8 px grid before storing. Drag-resize
  emits ~1 event per pixel; rounding collapses ~7 of every 8 events
  into the same `window_size` so view()s that depend on it (the new
  responsive grids) don't reflow on every frame. Reduces pressure
  on iced's subscription channel and the
  `TrySendError { kind: Full }` warnings during sustained drag.

## [0.5.7] - 2026-05-08

### Added
- **Per-host + global terminal theme override** — `Settings → Terminal`
  exposes a "Terminal Theme" picker that overrides the app-theme
  derived palette; the host editor has its own "Terminal Theme" tile
  that pins a specific host to a palette regardless of the global
  pick. Resolution order at runtime is per-host > global > app
  theme. The host's tile renders the active palette inline (bg fill,
  fg-coloured name, ANSI dots) so the choice is visible without
  opening the picker.
- **Visual swatch picker** — both pickers replace the previous
  dropdown with a column of cards. Each card paints the theme's
  background, the theme name in the foreground colour, and a strip
  of six ANSI dots — palettes are now compared at a glance.
- **7 new terminal palettes** — Oryxis Light, Termius, Darcula
  (JetBrains palette, distinct from Dracula), Islands Dark, Nord
  Light, Solarized Light, Paper Light. Every app theme now has a
  matching terminal palette; previously half the app themes silently
  fell back to a non-matching palette.
- **`Ctrl + (= | + | - | 0)` font zoom** — increase / decrease /
  reset terminal font size from anywhere in the app, captured before
  the PTY routing so the bytes don't reach the shell. Matches the
  alacritty / kitty / gnome-terminal convention. Closes #5 part 2.
- **`Ctrl + mouse wheel` font zoom** — wheel over the terminal
  canvas with Ctrl held adjusts font size; mouse-mode TUIs (vim,
  htop, less) keep their wheel behaviour intact since the event is
  consumed before reaching the PTY. Closes #5 part 3.

### Changed
- **`AppThemeChanged` no longer overwrites per-host palette
  overrides** — switching the app theme used to repaint every open
  tab unconditionally, blowing away per-host picks. The repaint
  loop now resolves through `resolve_terminal_theme_for_label` so
  per-host overrides survive an app theme switch.
- **Icon picker modal** — dimmed scrim + click-absorption pattern
  borrowed from `tab_jump`. Previously a click anywhere on the
  dialog bubbled out to the backdrop's `HideIconPicker` handler.
  Also: the per-host theme picker that briefly lived inside this
  modal was moved out into the host editor as a visible tile so it
  isn't hidden below the fold.

### Fixed
- **Terminal font size reverted to default on every restart** — the
  font size was kept in memory only. Now persists in the vault
  settings table and rehydrates on boot. Closes #5 part 1.
- **Terminal font name reverted to default on every restart** —
  same bug class as the font size fix; the `terminal_font_name`
  setting is now loaded on boot and persisted on every change.
- **Single tab disappeared when focus moved off the terminal** —
  `allocate_tab_widths` returned `inactive_width = 0` for `n == 1`,
  which kicked in whenever the active tab lost focus (sidebar
  click, AI chat sidebar, etc.). Mirrors the active width for the
  solo case so the tab stays visible regardless of focus state.
  Thanks @UltraMurlock (PR #6).

### Security
- **Bumped `astral-tokio-tar` 0.6.0 → 0.6.1** — addresses
  `GHSA-fp55-jw48-c537` (PAX header smuggling) and
  `GHSA-xx64-wwv2-hcqq` (symlink permission change during unpack).
  Pulled in only as a dev-dependency via `testcontainers`, but
  patching dev tooling anyway. (PR #7)

## [0.5.6] - 2026-05-05

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
