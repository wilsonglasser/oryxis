# Feature tour

The complete inventory of what Oryxis does today. For the short version,
see the [Highlights](../README.md#highlights) in the README; for what's
coming next, see the [Roadmap](../README.md#roadmap).

## SSH & connectivity

- **Auto-authentication.** Tries key, agent, password, and
  keyboard-interactive in order. A "Password prompt" method asks at every
  connection and never writes anything to the vault.
- **Multi-factor servers.** RFC 4252 partial success is a first-class
  step: sshd `AuthenticationMethods` chains and Bitvise-style compound
  auth (password + TOTP, key + password, chained one-time codes)
  continue through every remaining factor instead of dying on the
  first. A stored TOTP secret answers the verification-code round
  silently; without one, the prompt surfaces the way OpenSSH would.
- **Full SSH pipeline.** Direct, SOCKS4/5, HTTP CONNECT, ProxyCommand,
  multi-hop jump host chaining, and port forwarding via
  [russh](https://github.com/warp-tech/russh).
- **Connection reuse.** A second tab to a connected host opens a channel
  on the live connection instead of dialing, authenticating and second-
  factoring again (per hop, on a jump chain). The connection closes with
  its last tab, and a pooled link that turns out to be dead just dials
  fresh. On by default, Settings > Connection has the switch.
- **Wake-on-LAN.** Store a MAC address on a host and wake the machine
  from its card menu with a magic-packet broadcast, before SSH can
  reach it.
- **Standalone port forwarding.** Local (`-L`), Remote (`-R`) and Dynamic
  SOCKS5 (`-D`) forwards live as their own entities with per-row on/off
  toggles, auto-start at boot, and no terminal required. Every forward
  to the same host shares one SSH connection (one transport, one auth),
  a dropped forward climbs back with the same backoff a host does, and
  the host's card says which forwards target it.
- **Authenticated proxies.** SOCKS5 and HTTP CONNECT Basic auth, with proxy
  passwords in their own encrypted column.
- **Proxy + jump host stacking.** A jump host behind a proxy dials through
  it on the first hop.
- **Reusable Proxy Identities.** Save SOCKS5 / HTTP / SOCKS4 configs once
  and link them from any host.
- **SSH agent forwarding.** Per-host opt-in; bridges the local ssh-agent
  socket through the channel.
- **X11 forwarding.** Per-host opt-in (and imported from `ForwardX11` in
  `~/.ssh/config`) so remote GUI apps draw on your local display. The
  remote never learns your real cookie: a fake `MIT-MAGIC-COOKIE-1` is
  minted per session, verified in constant time on every X11 channel and
  swapped for the real one before any byte reaches the X server.
  Resolves `$DISPLAY` across Linux/BSD unix sockets, XQuartz launchd
  paths on macOS, and the TCP endpoint VcXsrv / Xming serve on Windows,
  including displays that run with access control off.
- **Bastion login scripts.** Some jump boxes authenticate INSIDE the
  terminal after SSH is already up: JumpServer / KoKo and friends drop
  you in a menu that asks for an asset, a user and a password. A login
  script is a reusable expect/send sequence (wait for a prompt, send an
  answer) attached to a host, with the answers that are secrets read
  from the vault at send time rather than stored in the script. Ships
  with a JumpServer preset and a generic interactive-bastion one, both
  editable; `{placeholders}` let one script serve many hosts, each with
  its own asset and target user. The run is bounded on every side: steps
  fire strictly in order, each has its own deadline, the whole run
  expires, any keystroke of yours aborts it, and the host's startup
  command is held back until the script actually lands you on the asset.
  Managed in Settings > Connection, created inline from the host editor.
- **Import from the client you already use.** One "Import" entry reads
  `~/.ssh/config` (with `ProxyCommand` and `ProxyJump` resolved
  automatically), PuTTY and KiTTY registry exports, WinSCP sites,
  mRemoteNG's `confCons.xml`, MobaXterm bookmarks, Xshell / SecureCRT /
  FinalShell session folders, and any CSV of hosts (a Termius export
  included). The format is detected from the file's content, not its
  name, and a registry export holding two clients' sessions imports as
  one batch. Every host lands in a tick-per-host preview,
  deduplicated against what you already have. Passwords come along
  where the source's own scheme allows it (WinSCP, mRemoteNG, a CSV
  password column) and go straight into the encrypted vault; clients
  that encrypt with a per-install key import the host and say so in
  its notes rather than guessing.
- **PuTTY-grade details.** TCP_NODELAY on every socket, per-host IPv4/IPv6
  preference honored on direct dials, proxy dials and jump chains, and the
  SSH pre-authentication banner shown instead of silently dropped.
- **RSA SHA-2 support** (`rsa-sha2-512/256` with SHA-1 fallback),
  step-by-step connection progress, TOFU host key verification, and
  integration tests against real OpenSSH containers.
- **Wake-on-LAN.** Store a MAC address on any host (any common notation)
  and wake the machine from the host card's menu with a magic-packet
  broadcast, before SSH, RDP or anything else can reach it.

## mosh

- **A session that survives the network.** Switch mosh on for a host and
  the shell rides out sleep, a change of Wi-Fi and a change of address.
  Native Rust client speaking the stock `mosh-server`'s protocol, so
  nothing extra is installed on your machine; the host needs `mosh` and
  a reachable UDP port.
- **An option on the SSH host, not a separate protocol.** `mosh-server`
  does not exist until an SSH session starts it, and the port and key it
  answers with come back over that same channel, so a mosh host reuses
  the username, key, jump chain, proxy and host-key policy the SSH side
  already resolves. Four fields under Credentials: the toggle, the
  server path, a UDP port range for hosts behind a restrictive firewall,
  and a command that replaces the login shell. They are kept when the
  toggle goes off.
- **The link says how long it has been out of touch.** "Connected"
  cannot express a session that is alive while its network is not, so
  two clocks report it: no contact when nothing is arriving, no reply
  when things arrive but nothing sent is acknowledged, which is what a
  one-way path looks like. Amber on the tab strip and in the connection
  segment, with the direction and duration in the latency slot.
- **Files open in a tab of their own.** A roaming session holds no SSH
  connection to multiplex SFTP on, so the request becomes a standalone
  SFTP tab against the same host, with its own visible lifetime.
- Not supported through a jump chain: SSH reaches the final host from
  the last hop, so the server binds an address facing the bastion, and
  UDP does not travel down an SSH tunnel. Upstream mosh has the same
  limitation.
- **The login banner (MOTD) usually does not appear, and that is the
  server's rule rather than ours.** Ubuntu prints it twice over, from
  two places with different rules. `pam_motd` runs in sshd's own PAM
  stack and fires on EVERY SSH login, which is why the banner is always
  there on an SSH tab. mosh's shell is started by `mosh-server`, not by
  sshd, so that path never runs for it; the only one left is
  `/etc/profile.d/update-motd.sh`, which a login shell runs and which
  stamps `~/.motd_shown` and then stays quiet for the REST OF THE DAY.
  So the first mosh session of the day shows a banner and the rest show
  none. Deleting that stamp brings it back once. Upstream mosh behaves
  identically, for the same reason.

## Telnet, serial & ZMODEM

- **Per-host protocol selector.** One picker per host: SSH, Telnet, raw
  TCP, serial, a local shell or a remote desktop. The editor swaps to a
  reduced form per protocol, hiding the rows that protocol cannot use
  rather than showing them and ignoring them.
- **Raw TCP lines.** A bare socket for console servers and appliances
  that speak no protocol at all. It opens in silence, with no client
  greeting: a console server forwards what it receives down the serial
  line, and an unasked-for option burst lands on the attached device as
  garbage.
- **Telnet over TLS.** A toggle on the Telnet form rather than a
  protocol of its own, since there is no in-band upgrade to negotiate.
  Certificate verification is on by default against the webpki roots,
  with a per-host escape for appliances carrying self-signed
  certificates.
- **Native Telnet engine.** RFC 854/855 option negotiation with the
  loop-proof RFC 1143 state machine, NAWS window sizing, terminal-type,
  charset transcoding, and prompt-driven credential autofill. The editor
  carries an honest note that the protocol is cleartext by design.
- **Serial consoles.** Local COM / `/dev/tty*` lines with configurable
  baud, framing, flow control, line endings and local echo.
- **ZMODEM transfers.** Run `sz` / `rz` on the remote and Oryxis
  auto-detects the transfer, takes over the byte stream and moves the file
  with a progress overlay. Works over SSH, Telnet and serial alike; a
  disconnect mid-transfer resumes the terminal cleanly.

## Quick connect

- **`user@host`, no ceremony.** Type it into the new-tab picker (Ctrl+K),
  the toolbar search or the tab jump and connect without saving a host.
- **Auth switch on failure.** If the first attempt fails, the prompt offers
  every saved identity and key and reconnects in place.
- **Edit mid-connect.** "Edit host" is available in every connection state,
  editing the temporary host without writing to the vault.
- Quick connections behave like real tabs (splits, SFTP, port forwards);
  typed credentials are swept on vault lock.
- **From your shell.** `oryxis user@host[:port]` connects, landing a tab
  in the window you already have open when one is running.
- **From a link.** Oryxis registers as the `ssh://` handler, and a
  clicked link opens the ad-hoc host editor with the target filled in.
  It deliberately stops there rather than dialing: a web page chooses
  that payload, so the connection stays your click. (macOS is the
  exception for now, along with `oryxis://`: LaunchServices delivers
  URLs as Apple Events rather than arguments.)

## Remote desktop (RDP / VNC)

- **One click to a desktop.** An RDP or VNC host is a first-class card that
  launches the OS-native client (`mstsc`, Microsoft Remote Desktop,
  FreeRDP, Remmina, or your VNC viewer).
- **Through an SSH tunnel.** The card can reach the machine directly or
  tunnel through an SSH host you pick as a gateway, an ephemeral `-L`
  forward provisioned on demand, managed, and closed once the client
  disconnects.
- Opt-in and hidden until enabled in Settings.

## Terminal

- **Embedded emulator.**
  [alacritty_terminal](https://github.com/alacritty/alacritty) with
  256-color, truecolor, mouse selection, scrollback.
- **Split panes.** Split a tab into a tmux/iTerm-style grid; each pane is
  its own session (saved host or local shell), with keyboard / paste /
  snippets / AI targeting the focused pane.
- **Session groups.** Save a split arrangement (panes + split tree +
  per-pane startup scripts) as a reusable, credential-free entity.
- **Smart tabs.** A command that ran past a threshold and finished while
  you were looking elsewhere earns an attention dot (green success, red
  nonzero exit) plus a notification with the command and duration. Hosts
  without shell integration get a quiet-period heuristic.
- **Per-host command history.** Captured via shell-integration marks
  (OSC 133) with a raw-input fallback, encrypted at rest, redacted before
  storage, surfaced in a sidebar History tab: most-frequent shortlist,
  recent list, search, re-run, paste-without-execute, delete. Local-only
  by design (never synced, never exported); optional plain-text export and
  per-host live-append log. Inside tmux the screen belongs to tmux, so
  capture needs the shell's own report: see the
  [tmux guide](TMUX.md).
- **Session recording.** The encrypted session logs capture timing and
  terminal resizes; any session exports as an asciicast v3 `.cast` file
  with your terminal theme embedded, or as a plain-text transcript.
  Output-only by design, so keystrokes never leak into a recording.
- **Search inside recordings.** The History screen searches the session
  content itself, not just titles, decrypting on demand with a bounded
  scan, and can filter to the hosts a given command ever ran on.
- **Pinned & reorderable tabs.** Pin tabs (restored on next launch, lazy
  reconnect), drag to reorder, rename, MRU switching with Ctrl+Tab, and an
  optional bottom tab bar.
- **Syntax highlighting.** IPs, URLs, and file paths auto-detected and
  colored.
- **17 terminal palettes plus custom schemes.** Picker with inline swatch
  previews, global or per-host; build your own, clone a built-in as a
  starting point, or import iTerm / Windows Terminal / base16 from a
  pasted blob or a file. Terminal and UI themes both export back out, so
  a scheme moves between machines without the vault.
- **Bundled Nerd Fonts.** SauceCodePro plus a Symbols Nerd Font fallback so
  Powerline and icon glyphs always render.
- **Paste done right.** X11-style middle-click paste, configurable
  right-click (paste / context menu / xterm-style extend selection), CRLF
  normalization, and a paste guard (see Security below).
- **International keyboards done right.** AltGr-composed characters (the
  bepo `_`, the German `@`, `{`, `[`) reach the shell as text on every
  platform instead of being eaten as control chords, and Ctrl+Space sends
  NUL. Ctrl+Shift with a cursor, editing or function key sends the xterm
  modified sequence (`ESC[1;6D` for Ctrl+Shift+Left) like a real terminal,
  so modifier-aware TUIs see the combo. On macOS the Option key composes
  characters like every native terminal, with a per-host "Option as Meta"
  override (off / left / right / both) for readline and emacs users.
- **System mono font enumeration**, configurable font size (10-24px,
  `Ctrl + = / - / 0`), bold-to-bright colors, scrollback reset on keypress
  (on by default: typing jumps back to the live edge, so a scrolled-up
  viewport never hides what you type) and/or on output, and an opt-in
  performance HUD (frame time vs budget, RTT and jitter on the SSH
  connection).
- **Downloadable font pack.** A curated catalog of popular Nerd Font
  builds sits in the font picker next to the system fonts: JetBrains
  Mono, CaskaydiaCove (Cascadia Code), Fira Code, Hack, MesloLGS (the
  powerlevel10k standard), Roboto Mono, Ubuntu Mono and Iosevka. Each
  downloads individually on first selection (2-3 MB, Iosevka 13 MB),
  SHA-256 pinned, cached locally, applied without a restart, and
  mirror-aware like the CJK fonts; nothing is bundled into the
  installers and the default SauceCodePro Nerd Font stays built in.

## Host monitoring

- **Agentless vitals.** A sidebar Monitor tab reads CPU, memory, swap,
  disk and network off the SSH connection you already have, on an exec
  channel multiplexed onto the live session. Nothing is installed on the
  server; Linux `/proc` is the primary source with BSD and macOS probe
  fallbacks.
- **Multi-host dashboard.** A Monitoring pill next to Hosts (visible
  once the feature toggle is on) shows live vitals cards for every
  opted-in host at once, FinalShell-style: hosts with an open tab are
  read over that session, the rest get a headless probe-only
  connection dialed with the stored credentials (strict host key,
  TOTP autofill) and pooled with an idle TTL. Cards carry CPU /
  memory gauges plus net, disk and GPU at a glance; clicking one
  opens a detail panel with the exact presentation of the per-session
  Monitor sidebar (sparkline, swap, load, GPUs, collapsible disk and
  listening-port sections) plus explicit open-terminal and retry
  actions. Search and the host-tag filter narrow the fleet, a toggle
  switches between the card grid and a table sortable by any column,
  and polling only runs while the view is visible.
- **GPU gauges.** Utilization, VRAM and temperature per device, from
  `nvidia-smi` where it exists and the amdgpu sysfs counters otherwise.
  The section renders only when the host answers, so machines without a
  GPU show nothing rather than an empty panel.
- **Opt-in, twice over.** The whole feature hides behind a Features &
  Plugins toggle, and then per host, so no connection starts probing
  behind your back. "Enable for all hosts" flips it wholesale, and the
  probe interval is configurable.
- **Listening ports with click-to-forward.** The panel lists what the
  host is listening on and turns any row into a local port forward in
  one click, honoring the listener's own bind address.
- **Kill the process on a port.** Right-click a port row for a graceful
  stop or a forced kill, behind a confirmation naming the port, process,
  PID and signal. The signal goes out on an exec channel, never into
  your shell, and Oryxis re-asks the host who owns the port first, so a
  service that restarted while the dialog was open is reported instead
  of signalled by mistake. Sockets whose PID the login user cannot see
  (the usual case for root-owned services) escalate through sudo, using
  the host's stored password only if sudo actually asks for one.
- **Threshold alerts** when a gauge crosses a line you set, a collapsible
  disk list for hosts with many mounts, and an optional status-bar
  segment that keeps the headline numbers visible with the panel closed.
- **Untrusted by construction.** Every number crossing the wire comes
  from a machine you may not control, so all probe arithmetic saturates
  rather than wrapping, and truncated or forged payloads degrade to
  "unknown" instead of nonsense.

## tmux session manager

- **The host's tmux sessions in the sidebar.** A tmux tab lists what is
  running on the focused pane's host, with the window count and which
  sessions already have a client attached. The listing runs `tmux` itself
  on an exec channel multiplexed onto the live session, so nothing is
  installed on the server, no rc file is written and nothing is injected
  into your shell.
- **Create, attach, kill.** New sessions are created detached, so they
  never fight the pane you are in; attaching types the command you would
  have typed into that pane, on your click; killing asks first and names
  the session, because it stops everything running inside it.
- **Reads what the host says.** A host without tmux says so rather than
  offering buttons that could only fail, a tmux with no server running is
  the empty state that invites a first session, and a failed command
  surfaces the host's own wording instead of a generic error.
- **Session names are untrusted text.** Every name comes off the remote
  host, so it is quoted at each boundary, whether it is heading for an
  exec channel or for your shell, and a name carrying a line break is
  refused rather than quoted.
- **Off by default,** behind a Features & Plugins toggle like host
  monitoring, and it never polls: the list is read when the tab opens,
  after your own actions, and when you ask.

## SFTP & files

- **Dual-pane layout.** Local and remote side by side, with sortable
  columns.
- **Open / Edit, in the background.** Hands a remote file to your OS
  default application, the editor you configured, or the OS "open with"
  picker, chosen right where you open it, then watches the local copy
  while you keep browsing: nothing blocks, and each save you make asks
  whether to send the file back (yes, yes to all, autosave, skip, or
  stop editing). Reopening a file that is still being edited offers the
  local copy instead of silently re-downloading over it. A path-history
  dropdown jumps back to directories you have already visited.
- **Downloads that ask first.** A download about to overwrite says so
  before a byte moves, and you pick where it lands instead of hunting
  for a default folder afterwards.
- **Per-host start folder.** A host can remember where its SFTP mounts
  open, set from the host editor or from any remote folder's context
  menu. A path that stops resolving falls back to the login directory
  instead of failing the mount.
- **Files in every SSH tab.** A sidebar Files tab browses over the tab's
  existing connection and follows your shell's working directory as you
  `cd` (shell-integration cwd reporting with a window-title fallback;
  manual navigation unpins, one click follows again). The title fallback
  is a heuristic, so exact following on any prompt takes a one-time
  snippet in your rc: see the [cwd guide](CWD.md). Rows click-select
  and double-click to enter, matching the SFTP panes; the recent-folder
  history is remembered per host across sessions (encrypted like the
  rest of the trail), and the mouse thumb buttons walk it back and
  forward on any visible file surface.
- **Hybrid Files mode.** "Open SFTP session" flips the whole tab into the
  dual-pane manager at the directory you were in, while the PTY keeps
  running underneath; a chip on the tab (or Ctrl+Shift+F) flips back, and
  the surface can detach into its own tab.
- **Drag-and-drop uploads.** Drop files from any OS file manager onto the
  remote pane; drag rows between panes to upload or download.
- **Drops into a container.** A host whose shell runs inside a container
  can carry dropped files over ZMODEM (`rz`) instead of SFTP, per host,
  so the upload lands in the container's own working directory rather
  than on the host filesystem SFTP reaches.
- **Multi-select.** Ctrl/Cmd-click and Shift-range; batch Delete /
  Download / Duplicate / Upload.
- **Properties dialog.** Per-row chmod grid, size, mtime, owner.
- **Server-to-server copy.** Transfer files directly between two remote
  hosts, streamed host-to-host with no local round-trip and a live
  byte-level progress bar.
- **Overwrite handling**, configurable parallelism (1-8 channels),
  recursive delete over exec, and tunable timeouts.

## AI chat assistant

- **Integrated AI sidebar.** Collapsible chat panel per terminal session;
  replies route to the tab that asked.
- **Streaming responses.** Tokens land as the model emits them; markdown
  re-renders progressively.
- **Runs commands for you.** The assistant drives the focused pane through
  an `execute_command` tool and reads the output back, instead of printing
  commands to copy.
- **Plan / Ask / Auto modes** with a floating Stop control.
- **Three-layer auto-exec safety.** A deterministic floor force-prompts
  catastrophic commands, an independent fail-safe LLM judge vets the rest,
  and the "always run" allow-list refuses chained / piped / substituted
  commands so a trusted name can't smuggle a destructive payload.
- **Multiple providers.** Anthropic, OpenAI, Google Gemini, or any
  OpenAI-compatible endpoint; bring your own key, stored encrypted.
- **Privacy-aware.** Privacy Mode redaction is applied before terminal
  context reaches the model. Terminal context, smart output capture, and a
  custom system prompt option.
- **Saving is a choice.** Conversations persist (encrypted, in the
  vault) only when you opt in; a retried answer replaces the one it
  corrects instead of stacking a duplicate.

## MCP server

- **AI integration.** Expose your SSH hosts to AI assistants via the
  [Model Context Protocol](https://modelcontextprotocol.io/).
- **5 tools.** `list_hosts`, `get_host`, `ssh_execute`, `list_groups`,
  `list_keys`.
- **Per-host control.** Toggle MCP exposure per connection.
- **Disabled by default.** Enable in Settings > Security.
- **Distributed as a plugin.** Downloaded on demand, with a stable launcher
  path for external clients.

Setup for Claude Code (`~/.claude.json`):

```json
{
  "mcpServers": {
    "oryxis": {
      "command": "oryxis-mcp",
      "env": {
        "ORYXIS_VAULT_PASSWORD": "your-vault-password"
      }
    }
  }
}
```

If your vault has no password, omit the `env` field.


## Identity system

- **Reusable credentials.** Identities (username + password + key) linked
  to many hosts.
- **Autocomplete.** Type a username to find and link matching identities.
- **Keychain view.** Keys and Identities side by side with search and
  context menus.
- **Proxy Identities.** Same shape for proxy configs, password stored
  encrypted.
- **Encrypted SSH key import.** Passphrase-protected keys are decrypted on
  import; the vault master password protects them at rest.
- **Group settings inheritance.** A group carries defaults (login user,
  identity, proxy identity, terminal theme, startup snippet,
  environment variables, and the port new hosts start with) and every
  host inside inherits what it does not set itself, resolved up through
  nested subgroups. Per parameter, not all-or-nothing: a group that only
  sets the proxy leaves the rest alone, and a subgroup overrides just
  what it names. The host editor shows where an inherited value comes
  from and typing your own overrides it. Environment variables merge by
  name instead of replacing, so a host adds to its groups' set and
  overrides only the names it repeats. Credentials are an identity
  REFERENCE, never a copy, so no second place holds a password. The port
  is the deliberate exception: it prefills a host created in the group
  and never changes one that already exists.

## Vault & security

- **No password by default.** Opens instantly; enable a master password in
  Settings.
- **Argon2id + ChaCha20-Poly1305** with a per-field salt and nonce;
  re-encryption of all secrets when the password changes; vault reset from
  the lock screen.
- **Biometric unlock.** Windows Hello, Touch ID, or the Linux system
  keyring; opt-in, with the password always one click away.
- **Idle auto-lock.** A timer zeroizes the master key and shows the lock
  screen while live SSH sessions and tabs survive and greet you after
  unlock; secret-bearing UI is swept on lock.
- **TOTP 2FA autofill.** Store a per-host TOTP secret (bare base32 or an
  `otpauth://` URI), encrypted like every credential;
  keyboard-interactive verification-code prompts are answered
  automatically, once per auth attempt, with a manual fallback.
- **Stored passwords at password prompts.** When a session blocks on
  `[sudo] password for you:` (or `su`, `ssh`, a key passphrase), a popup
  at the cursor lists the passwords the vault holds for that host and
  your identities. Down engages it, Enter sends, Esc hides. Nothing is
  ever sent without picking a row, so typing your own password is never
  interrupted, and prompts that ask you to CHOOSE a password (`passwd`,
  key generation) are recognized and never offered one. The credential
  is decrypted at the moment you pick it, written straight to that one
  pane (never broadcast, never mirrored into command history) and
  scrubbed from memory after the write. Off with one toggle in
  Settings > Terminal.
- **Privacy Mode.** Masks IPv4/IPv6 addresses, `user@host` pairs, home
  directories and vault hostnames on screen, in notifications, and before
  AI context leaves the app; click a mask to pin-reveal it.
- **Paste guard.** Multi-line pastes and single-line pastes with invisible
  or bidirectional characters, raw control sequences, `curl | sh`
  fetch-and-execute patterns, or mixed-alphabet look-alikes hit a
  confirmation with one plain warning line per finding.
- **No telemetry.** No data leaves your machine.

See [SECURITY.md](../SECURITY.md) for the full security model and the
vulnerability disclosure policy.

## Snippets

- **Snippets with variables.** `{name}` and `{name:default}` placeholders
  prompt in a small dialog before the send; shell text like `${VAR}` and
  `{print $1}` is deliberately left alone.
- **Groups, tags and shortcuts.** Grouped folder cards in the vault
  (nestable to any depth, see below), a tag filter on hosts and snippets,
  a sidebar toggle that surfaces only snippets tagged like the focused
  host, and a recordable key combo that runs a snippet straight into the
  focused terminal.
- **Run or paste.** Every snippet can run (with Enter) or paste (without)
  into the focused pane, from the vault or the terminal sidebar.

## Themes & internationalization

- **13 global themes plus custom UI schemes.** Switch the entire UI
  instantly, or build your own (21 colors) with a built-in graphical color
  picker and live preview. Both the UI and the terminal pickers open
  into full gallery modals with live previews instead of cramped grids.
- **Community themes.** A directory in the repo takes contributed
  themes (UI, terminal, or a matching pair) by pull request, and a card
  in both galleries links straight to it; True Black OLED, the first
  contributed pair, ships with the app.
- **Per-theme button colors** with WCAG contrast guards enforced in CI.
- **Terminal background image.** `Settings → Terminal → Background
  image` takes any PNG / JPEG / WebP / BMP / GIF / TIFF, with a fit
  (cover, fit, stretch, centre, tile) and a fade that dissolves it into
  the theme's background colour so text stays readable. Drawn per pane;
  colored blocks a TUI paints stay solid. Every host can override the
  picture, its fit, its fade and the opacity independently, or opt out
  of the global picture, in `Host editor → Terminal`. Only the path is
  stored, so the vault stays small and a moved file falls back to the
  plain background.
- **Translucent terminal background.** `Settings → Terminal → Background
  opacity` lets the desktop show through the terminal, down to 30%.
  Panels, tabs and the status bar stay opaque, so nothing you read over
  a busy wallpaper loses contrast. The window's transparency is set up
  at startup, so the first step away from 100% offers a restart; every
  change after that is live. Linux (Wayland, or X11 with a compositor)
  and macOS; on Windows the graphics stack usually reports no
  transparent surface, where the setting simply has no effect.
- **Highlight rules.** `Settings → Terminal → Highlight rules` colours
  the text you care about: a pattern (plain text or a regular
  expression, case-insensitive by default), a colour, and an optional
  action when it shows up. Your rules beat the automatic URL / IP / path
  detection, and the first matching rule wins, so the list is ordered.
  The action can be a desktop notification, a sound, or typing one of
  your snippets into that session, with a per-rule cooldown so a log
  full of the same word is one notification and not a hundred. Sending a
  snippet asks first, once per rule per session, showing the line that
  matched and the snippet's text: the trigger is decided by output the
  HOST printed, and that must not be a way for a server to choose what
  runs on it. Actions do not fire inside full-screen applications
  (tmux, vim, htop), which repaint the screen instead of printing lines;
  the colouring still works there. Every host can carry rules of its own
  in `Host editor → Terminal`, and choose whether they add to the global
  ones or are the only ones that apply; replacing with an empty list is
  how a noisy host turns highlighting off entirely. Per-host rules ride
  the portable export with the rest of the connection.
- **23 languages.** English, Português, Español, Français, Deutsch,
  Italiano, 简体中文, 繁體中文, 日本語, Русский, فارسی, العربية, עברית,
  한국어, Polski, Türkçe, Bahasa Indonesia, Tiếng Việt, Українська, ไทย,
  हिन्दी, Čeština, Ελληνικά.
- **RTL layout support.** Persian, Arabic and Hebrew flip the chrome;
  `Settings → Theme → Layout direction` overrides with Auto / LTR / RTL.
- **Theme + language on the lock screen**, plus floating overlay context
  menus.

## Export / import

- **Single encrypted file.** Export your whole vault as a
  password-protected `.oryxis` file.
- **Selective export.** Include SSH private keys or only host configs.
- **Smart merge.** Import merges by UUID, keeping the newer record (LWW).
- **Round-trips proxy data** so a fresh device gets working proxy auth.


## Plugin subsystem

- **Local-only.** Nothing is downloaded anymore: whatever sits in the
  local plugin cache (or next to the app executable as a dev build) is
  what runs.
- **Signed binaries.** A plugin binary must carry a valid Ed25519
  signature against the baked-in key before it is copied to the stable
  launcher path external clients spawn; release builds fail closed on
  unsigned binaries.
- **Manifest + cache.** The app reads the cached `manifest.json` to
  resolve the active version and its signature.

## OS integration

- **Windows system tray.** Show / Hide to tray / Quit with dynamic
  submenus, an active-sessions submenu, a recent-hosts submenu, opt-in
  close-to-tray and minimize-to-tray, and a single-instance guard with
  primary/child IPC.
- **Windows JumpList.** Recent hosts on the taskbar icon; right-click,
  pick, connected.
- **Window memory.** Size, position, monitor, maximized and fullscreen
  state restored across launches.
- **Truthful rendering.** A boot probe detects software rasterizers
  masquerading as GPU backends (the WSL / llvmpipe class) and drops to the
  built-in software renderer, with honest backend labels and an in-app
  restart.

## UI / UX

- **Native GPU-accelerated UI.** [Iced](https://iced.rs) on the wgpu
  backend.
- **Keyboard everything.** Focus zones across the vault, keyboard walks
  through modals, menus, Settings and the terminal sidebar, Ctrl+Shift+1..8
  section jumps, MRU tab switching, and creation chords. Every binding is
  rebindable with a live capture mode.
- **Workspace layout mode.** Sidebar hides when a tab is open so the
  terminal fills the canvas; Classic mode stays a one-click opt-out.
- **Nested host folders.** Groups hold groups, to any depth. Pickers
  show the full breadcrumb path so two folders named `prod` under
  different parents stay distinguishable, typing a path creates the whole
  chain at once, and deleting a folder promotes its children to the
  grandparent instead of orphaning them.
- **Customizable host icons.** Circular / Square / Outline / Initials,
  global or per-host, with dynamic accent on the chrome from the host's
  color.
- **Responsive card grid.** Column count reflows to the available width;
  long labels truncate cleanly.
- **Multi-tab sessions** with tab overflow, a scrollable strip, and a
  jump-to modal. Duplicating a tab lands it right next to the original,
  tabs can show the host's address under the label, and uniform tab
  width takes a small / medium / large ceiling.
- **Tab strip on any edge.** Dock the tabs top, bottom, left or right;
  the side docks turn the strip vertical, can absorb the window chrome
  (burger, Home, window buttons) to reclaim the top bar entirely, and run
  full height. Inactive tabs take a separation style of your choice
  (none, border or underline).
- **Terminal sidebar in two dockable regions.** Every sidebar tab
  (Chat, Snippets, History, Files, Monitor, Tmux, Host config, Hosts
  tree) picks its side, left or right, or hides entirely; both regions
  can be open at once, each with its own strip and width. Pick which
  tab a region opens on, have it open itself on connect (globally or
  per host), and toggle either region from the keyboard: the main
  binding follows the tabs, and Ctrl+Alt+B reaches the other region.
- **Hosts tree.** An mRemoteNG-style tree of the vault as a sidebar tab
  beside your session (folders fold in place, hosts connect on click,
  saved arrangements included), and the same
  tree as a third dashboard view mode next to grid and list, with
  search force-expanding matches to their whole subtree.
- **Configurable status bar.** Every segment is individually
  toggleable, including latency, transfer size, the working directory
  and the host vitals.
- **Settings as a tab.** Settings opens in the tab strip like any other
  tab: it keeps its place, survives switching away, and closes like a
  tab does.
- **Settings search.** Type in the Settings sidebar and matching rows
  highlight in place with a hit-count badge per section; Enter and
  Shift+Enter step through matches. English terms work in any UI
  language.
- **Debug affordances.** Settings > Advanced has an opt-in debug log and a
  "Copy environment info" button for bug reports.

## Default keyboard shortcuts

Every binding is rebindable in Settings; this is the out-of-the-box set
for the most common actions. The in-app burger menu shows the full,
current list.

Chords that a terminal application could legitimately want (a bare
`Ctrl+K`, `Ctrl+L` or `Ctrl+J`) are deliberately left to the shell, so
the app's own actions sit on `Ctrl+Shift`.

| Shortcut | Action |
|----------|--------|
| `Ctrl+Shift+T` | New-tab picker |
| `Ctrl+Shift+G` | Quick connect |
| `Ctrl+Shift+J` | Jump to tab |
| `Alt+Left` / `Alt+Right` | Cycle tabs |
| `Ctrl+1...9` | Switch to tab 1-9 |
| `Ctrl+Shift+C` / `Ctrl+Shift+V` | Copy / paste in the terminal |
| `Ctrl+Shift+W` | Close tab |
| `Ctrl+Shift+R` | Reconnect the active tab |
| `Ctrl+Shift+F` | Toggle Files mode on an SSH tab |
| `Ctrl+Shift+H` | Focus the terminal sidebar |
| `Ctrl+Shift+P` | Command palette |
| `Ctrl+Shift+L` | Open local terminal |
| `Ctrl+N` | New host |
| `Ctrl+F` | Search the current view / terminal scrollback |
| `Ctrl+,` | Settings |
| `Ctrl+= / Ctrl+- / Ctrl+0` | Terminal font size (also `Ctrl+Wheel`) |
