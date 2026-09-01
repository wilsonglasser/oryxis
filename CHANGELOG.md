# Changelog

All notable changes to Oryxis are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project uses [SemVer](https://semver.org/spec/v2.0.0.html).

## [0.16.0] - 2026-08-31

An interactive SFTP console for people who would rather type than drag, opening as a pane of the session already in front of them, and an optional network tools panel for the questions asked while a host will not connect. Alongside them, East Asian ambiguous width becomes a per-host answer, the CJK font fix stops Chinese, Japanese and Korean labels from looking cut off, and a command proxy connects on Windows for the first time.

### Added
- Interactive SFTP console: `sftp(1)` commands, globs, Tab completion on remote paths, a command history and byte-level progress inline (#188).
- The console opens as a pane of the live session, stacked under the shell, beside it or zoomed over it (Settings > SFTP). Asked for from a host card it still gets a tab of its own.
- One switch over Terminal / Console / SFTP: the tab's mode chip cycles, the status bar goes straight to one, and Ctrl+Shift+S (Cmd+Shift+S on macOS) is the round trip.
- The console is offered on SSH hosts only, and not on a host carrying mosh. Its commands stay out of the per-host command history.
- Network tools panel in its own tab: DNS, ping, traceroute, port test, HTTP, TLS certificate, WHOIS and eight public blocklists. Off by default; Settings > Features turns it on.
- Ping and traceroute need no `ping` or `traceroute` installed on Linux, or on Windows for IPv4.
- Port test takes up to 64 ports and reports open, refused and filtered as three different answers.
- Per-host East Asian ambiguous width (Auto / Narrow / Wide), in the host editor and the sidebar's Host config tab. Auto reads the host's encoding.
- A pane's right-click menu zooms and rearranges the pane it opened on, rather than whichever pane holds focus.
- The tab strip answers a right-click and the `+` popover offers the reopen, so a closed tab comes back without the hotkey (#186).
- Per-host opt-in that carries dropped files over ZMODEM instead of SFTP, for a shell running inside a container (#192, by @shideqin).

### Security
- The SFTP console refuses a downloaded file name that is not a single plain path component. A hostile server could answer a glob with `..\..\evil.exe` or `C:evil` and steer a `get` outside the local working directory on Windows.
- The SFTP console prints remote file names with control characters replaced by `?`. A name could otherwise clear the screen, forge the console's own prompt marks, or reach the clipboard through OSC 52.
- A value substituted into a ProxyCommand token may only be the shape of a host or a login name. The line is approved once, while the host filling `%h` arrives per dial and a sync peer writes it verbatim.
- Ping and traceroute refuse a target starting with a dash, which the system binary would have read as one of its own flags.

### Fixed
- A command proxy connects on Windows: the spawn was `sh -c`, and a stock Windows box has no `sh` (#194, by @kblock1).
- `%h`, `%n`, `%p` and `%r` resolve in a ProxyCommand, so an imported `~/.ssh/config` host stops asking its proxy for a host literally named `%h` (#194, by @kblock1).
- A command proxy that fails says why: its last lines ride the dial error instead of surfacing as an unexplained disconnect, and its output is drained for the life of the session.
- Chinese, Japanese and Korean labels no longer look cut off: the downloaded CJK font never reached the fallback chain (#189).
- Open Plugins opens the plugin settings from outside Settings (#193).
- Sidebar Files rows answer Delete and the context menu without the keyboard ring (#191, by @shideqin).
- A serial host no longer panics on an unusual baud rate (serialport 4.10.1).
- Zero-width characters past the cell limit no longer grow a cell without bound (alacritty).

## [0.15.0] - 2026-08-24

The mosh release, and the first since 0.10 to carry a security section.
A session
can now be carried over mosh: an SSH host with mosh switched on dials
exactly as it always did, prompts and host keys and proxy consent and
all, and at the last moment hands the session to `mosh-server` and lets
the SSH connection go, so the shell survives sleep and a change of
address. Alongside it, a batch of fixes that mostly share one shape, a
value somebody else authored being trusted as if the user had typed it:
a URL printed by a remote host could run a program on Windows, a paired
sync device could plant a proxy command that ran at boot or replace a
host-key pin, and an imported `.oryxis` file could re-point the update
mirror or arm a port forward that dials on next launch. Three more stop
a remote peer from deciding how many bytes land on the local disk.
Also: the last closed tab comes back, the
built-in terminal themes reach 31, and a key sitting in `~/.ssh` is
finally a key Oryxis will offer.

### Added
- **mosh, as an option on an SSH host** rather than a seventh protocol.
  It has to be: `mosh-server` does not exist until an SSH session starts
  it, and the port and key it answers with come back over that same
  channel, so a mosh host needs the username, the key, the jump chain,
  the proxy and the host-key policy the SSH side already resolved. Four
  fields in the host editor under Credentials (on, server path, port
  range, and a command that replaces the login shell), all of them kept
  when the toggle goes off, because a server path somebody had to look
  up should not have to be found again. The handover lives at the one
  point every dial path converges, so a dial site added later inherits
  it. New crate: `oryxis-mosh`.
- **The link says how long it has been out of touch.** A mosh session
  stays alive while its network does not, which is the whole point of
  the protocol and the one thing "connected" cannot express. Two clocks,
  mosh's own, because the link fails in two ways: nothing arriving at
  all is no contact, things arriving with nothing acknowledged is no
  reply, which is what a one-way path looks like. Surfaced amber on the
  tab strip and in the connection segment, with the direction and the
  duration in the latency slot.
- **Bring back the last closed tab** (#186): Ctrl+Shift+Y, the tab
  context menu and the command palette reopen the last tab that left the
  strip, terminal or SFTP, ten deep and session-only. It resolves
  through the same machinery a dormant pin reopens through, so a saved
  host comes back by id rather than by name. Quick-connect tabs are
  deliberately not remembered, for the reason they are not pinnable:
  their credentials would have to outlive the session that typed them.
- **Fourteen more built-in terminal themes**, taking the curated set to
  31 (Ayu Dark/Light, Catppuccin Mocha/Latte, Everforest Dark, GitHub
  Dark/Light, Gruvbox Light, Horizon, Kanagawa, One Light, Rosé Pine,
  Tokyo Night, Zenburn), each faithful to its upstream terminal export
  with the divergences documented. At 31 the lists needed a filter, so
  the Settings gallery and the per-host picker each got one.
- **A key in `~/.ssh` is now a key Oryxis can offer** (the third key
  source, next to the vault and the agent). Opt-in per host, never a
  global setting, because offering a credential the user never named
  changes who they are to that server. The vault always wins: the disk
  only fills a slot left empty. A passphrase-protected file is reported
  as exactly that in the host editor rather than silently skipped, which
  is what used to leave a user watching `ssh` authenticate while Oryxis
  fell through to a password prompt with nothing on screen saying why.
- **One protocol picker** (#174). Remote desktop used to be a separate
  entry in the add menu, which is how a feature becomes invisible; every
  protocol now lives in one list, joined by Raw (a bare TCP line for
  console servers, which opens in silence because an unasked-for option
  burst lands on the attached device as garbage), Local (a curated
  terminal on this machine) and Telnet over TLS. Quick connect speaks
  the same list through `scheme://`, and a bare `/dev/tty*` or `COM3` is
  Serial because it is a host under no protocol.
- **Inline IME preedit at the caret**, by @shideqin (#178, for #176), so
  composing in Japanese,
  Chinese or Korean shows what is being composed instead of nothing.
- **Overwrite prompts on drop uploads**, by @shideqin (#185). Dropping onto the
  terminal or the sidebar now routes through the same conflict flow the
  SFTP panel uses (Replace / Replace if different / Duplicate / Cancel)
  instead of silently renaming or clobbering.
- **Drag files out of the file browser onto the desktop**, phase two:
  the ghost is drawn by Oryxis and the OS only takes over when the
  cursor leaves the window, because the Windows drag API blocks the UI
  thread for the whole gesture and escalating early froze the window
  before a ghost could paint.
- **Secrets-free CSV export of hosts** (Settings > Security), which
  round-trips through the importer. There is structurally no password
  column: the function never receives secret material at all, and the
  encrypted portable export stays the only secrets-bearing path.
- **A session-recording size cap** (Settings > Security), which drops
  the oldest finished recordings the way any log rotation does. It fixes
  an asymmetry the age rule has alone, where "1 day" deletes a 10 KB
  recording from yesterday and keeps a 40 GB one from today.
- **The dial prompts answer the keyboard.** The host-key and
  command-proxy approvals record keynav rows, with the REFUSING button
  as the default, so a stray Enter can never trust a host key or spawn a
  command proxy. Before this the keyboard could only ever refuse:
  reaching "continue" needed the mouse, at exactly the moment the hands
  are not on it.
- **A scheme pasted into the wrong import panel is carried over**
  ([discussion #68](https://github.com/wilsonglasser/oryxis/discussions/68))
  instead of dead-ending on an error, with a toast saying what
  happened. The redirect needs positive evidence of the other kind,
  never mere absence of the marker, so a typo cannot ping-pong between
  the two panels.

### Changed
- **Portable Windows copies and Linux AppImages update in place**
  (#180). A portable copy used to be handed the setup installer, which
  lays down a second, installed copy instead of updating the one the
  user runs; an AppImage now replaces the image file `$APPIMAGE` points
  at rather than `current_exe`, which is the read-only mounted
  squashfs. Both keep the Ed25519 signature gate.
- **A single-file SFTP download gets the progress bar the upload
  already had.** "Download to local" on one file ran with no bar, no
  byte count and no cancel, which on a 900 MB file is a UI that looks
  frozen. Both directions now share one batch runner.
- **The transfer toasts lost their protocol name.** They say the same
  thing for a ZMODEM transfer, an SFTP queue item and a drag-out, and
  naming one of the three made the other two read as borrowed strings.
- **Every transport reads as dead before its output stream ends.** The
  end of that stream is what tells the app a session disconnected, and
  the app now discards such a notice while the pane's transport is still
  alive, because the mosh handover makes a superseded session an
  ordinary event rather than an exotic one. That test is only safe if a
  session that really died can never still answer "alive" at that
  instant, so each reader now publishes its own death before dropping
  the output sender, in the same task. SSH previously leaned on the
  reader task's join handle and Telnet / Serial on a channel the WRITER
  task closes, both of which settle a scheduling decision later than the
  notice travels. Nobody reported it and no test could reproduce it, but
  the guard's correctness rested on winning that race rather than on not
  having one.

### Fixed
- **A jump host that itself sits behind a jump chain is now reached**
  (#184). The route was built from the final host's chain alone, so C
  via B, with B behind A, never got to B. OpenSSH follows a hop's own
  `ProxyJump` recursively and so does this now, under a visited guard so
  a cycle degrades to a direct dial rather than looping.
- **Pasting on Wayland and WSLg** (#179). A fallback added for
  compositors without the clipboard-control protocol fixed writing and
  took reading with it: WSLg answers with an empty string for content
  that reads correctly over X11, and an empty string is not an error, so
  it arrived as a legitimately empty clipboard and paste did nothing.
- **The classic Alt+Tab switcher shows the app icon** on Windows
  (#182) instead of a generic placeholder.
- **Keystrokes no longer land in the wrong tab while another connects**
  by @shideqin (#177): the gate that holds input during a dial is scoped
  per tab.
- **An open side panel stopped eating the search box's Enter** (#175).
- **An identity nothing links to said `???`** instead of saying so, in
  every one of the 23 languages: the string reached the lookup with no
  table entry behind it.
- **The AI assistant knows when a command has actually finished.** Two
  fixes with one root: the prompt was detected by finding a `$`, `#` or
  `%` anywhere in a line, so `cat`ing a script ending in `# end of file`
  read as "the shell is back", and a `df` row ending in `41% /run/user`
  was the same trap one column over. Where the CURSOR sits is what
  discriminates a prompt from a line that merely contains a marker, and
  that is the signal now. Behind that gate the marker test can afford to
  be loose enough for fish and oh-my-zsh prompts, which never ended a
  capture at all before.
- **Two mosh defects found the first time a real host was on the other
  end**: the pane read as disconnected while the shell worked (the SSH
  stream that dialled it reported the death of a connection the pane had
  already replaced), and the pane carried two login banners painted over
  each other, one per shell, because mosh's model of the screen starts
  blank and never mentions anything SSH left behind.
- **A file browser opened on a mosh pane now opens an SFTP tab of its
  own** instead of silently dropping the request. A session that
  survives roaming has no SSH connection to multiplex on, and two tabs
  with two visible lifetimes is the honest shape.
- **Drag and drop of files into the window**, via synchronized fixes in
  the winit and iced forks, by @shideqin (#183).
- The two lints `clippy` 1.98 started raising, plus one only visible on
  a Windows target.

### Security
- **Opening a URL on Windows went through `cmd.exe`, which parsed it.**
  `cmd /C start "" <url>` put a shell between Oryxis and the browser,
  and Rust quotes an argument only when it holds a space or a tab. A URL
  holds neither, so `&`, `|` and `^` reached `cmd.exe` as operators and
  `%VAR%` expanded: a link reading `https://host/?a=1&calc` ran `calc`
  on the click. Both callers reach that sink with a string somebody else
  authored, one of them being whatever the REMOTE HOST printed into the
  terminal. Now `ShellExecuteW` with no shell in between, plus an
  RFC 3986 scheme allowlist at each sink, because the Windows handler
  resolves a bare path or a UNC name to a program and runs it.
- **A command proxy now asks this machine before it runs on this
  machine.** `ProxyType::Command` is the one connection field that
  becomes a local process, handed to `sh -c` BEFORE the handshake, so it
  runs regardless of host-key policy, reachability or a failed auth. And
  that data does not always come from the person at the keyboard: a
  paired sync device could point an existing group at a planted command
  and get `sh -c <attacker>` on the next dial of any host in it,
  including the auto-start forward that fires at boot with nobody
  present. The records still replicate; what changed is that the SPAWN
  asks. The gate is in the engine rather than its ten callers, so a dial
  site that never heard of it fails closed; approval is keyed by the
  command's SHA-256 and never leaves the device; unattended dials may
  only look up an existing approval, never raise a dialog nobody
  expected; and only a line typed into the host editor is pre-approved,
  because a picked file is not a read file.
- **A sync peer may add a host-key pin, not swap one.** A pin is a
  decision a human made at a fingerprint prompt on this device, and
  storage keeps one row per (host, port, key type), deleting the others
  first, so a peer's record carrying a fresh id was never an insert: it
  replaced the local fingerprint, and every later dial to that host
  trusted the peer's key with no prompt, headless strict dials included.
  What replaces automatic propagation of a key rotation is the ordinary
  "changed" prompt on the next connect, which is the one moment a human
  should be looking at a fingerprint anyway.
- **An import file is not a source of trust.** The import dialog shows a
  per-category count and never a list, so nothing on that path lets the
  user see what a file changes. Settings whose value is itself a trust
  decision now stay behind (`download_mirror` re-points every
  GitHub-bound request including the updater's, a clipboard access level
  of `readwrite` hands every connected server an OSC 52 clipboard READ,
  and the agent-server four activate the local signing service and strip
  its per-signature confirmation); host-key pins dedup by the semantic
  key so an import cannot replace one; and port-forward rules land
  disarmed, because `auto_start` is what turns a stored rule into a dial
  at the next launch with nobody present.
- **A downloaded folder owes the same name rule as its children.** The
  recursive SFTP walk already refused unsafe server-supplied names per
  entry, which left the two places that name the destination themselves.
  Both build a local path from a name the SERVER chose, and splitting on
  `/` only neutralizes `/`, so a backslash, a `..` or a `C:` prefix
  survived it and re-rooted the join, writing outside the directory the
  user picked.
- **A ZMODEM batch is bounded, not just each file in it.** The only
  ceiling reset at every file header, so a peer looping
  `ZFILE -> ZDATA -> ZEOF` stayed inside it forever while the disk
  filled. There is a session budget now, computed from free space.
- **A download asks whether it fits before it starts.** Three paths let
  a remote peer decide how many bytes land on the user's disk and none
  of them asked. SFTP answers before the first byte moves rather than
  failing at 90% with a stranded part file. Best-effort by construction:
  a platform that will not answer returns nothing and the caller
  proceeds, because a check that cannot run must never be the reason a
  transfer refuses.
- **A recording that fills the disk stops, and says so.** Session
  recording wrote for as long as the peer kept printing, with no size in
  hand at all; a full volume ended in a log line and nothing else, so
  recording kept being attempted, every later byte was dropped, and the
  user was never told. There is an unconditional free-space floor now
  (not a setting: nobody switches on "do not fill my disk"), and a stop
  both toasts and flags the row truncated, because a partial stream that
  presents itself as a whole session is a worse failure than stopping.

## [0.14.0] - 2026-08-17

The host editor and terminal typography release. The editor that every
saved host goes through was rebuilt as two tiers, with the rarely-used
fields folded behind headers that summarize what is inside them, an
edge you can drag, starting-point chips on creation, and no Save button
at all: closing the drawer is the write. The terminal font gained a
weight and a stroke-thickness control, which is the honest answer to
text that reads lighter here than in terminals that rasterize through
the OS. The sidebar file browser serves local shells, drags files out
to the desktop, and asks where a download should land. Plugin manifests
now come from a file tracked in this repo instead of a 1 MB release
listing that had grown past its own read ceiling and broken every
install.

### Added
- **A two-tier host editor.** The form starts with what every host
  needs and folds the rest behind section headers that summarize their
  own contents, so a jump chain or a proxy is one line of text until
  you open it. Creation offers starting-point chips (Basic SSH, via
  bastion, cloud) that prepare the form instead of making you find the
  fields. The drawer's edge is a drag handle and the width persists.
- **Edits save when the editor closes.** An existing host has no Save
  button: closing the drawer is the write, on every path that takes it
  off screen, including the ones no handler mentions (navigating away,
  focusing a tab, another panel taking the slot). A new host keeps the
  explicit Save/Connect pair, because a half-typed host must never
  enter the vault by itself. An earlier revision of this persisted on a
  700 ms debounce while you typed; one write per editing session
  removes a whole class of bug instead of guarding each of its sites,
  since every mid-typing save re-sorted the host list under whatever
  was holding a position into it.
- **Terminal font weight, and a stroke thickness control.** The font
  picker gained a weight, and it says so when the family you picked has
  no face at that weight rather than silently drawing the nearest one.
  Text Thickness (Settings > Terminal) widens every stroke a fraction
  of a pixel, which is what macOS and Windows do before drawing text:
  Oryxis rasterizes the raw glyph, which is why the same font at the
  same size read lighter here than in other terminals (#155).
- **The sidebar file browser serves local shells** (#145). The same
  browser, over the app's own filesystem when the pane is a local
  shell, with listing, navigation, rename, create and delete unchanged;
  the transfer-shaped actions stay SSH-only and hide themselves.
- **Drag files out of the sidebar browser** (#167), onto the desktop or
  any application that takes a file drop. Windows first.
- **Downloads pick a destination.** The sidebar's download action opens
  a save dialog seeded with the last folder used, and the OS dialog is
  warmed at boot so the first one does not stall on the shell's COM
  server (Windows).
- **A running-command indicator on the tab strip** (#146), so a tab
  still working is visible without switching to it.
- **Snippets can install scripts, with shipped presets** (#147).
- **The fleet dashboard can be paused**, and a paused dashboard holds
  no connection open.
- **Ubuntu, the GNOME Terminal classic**, as a built-in terminal theme,
  asked for in #118.
- **A confirm before the manual lock**, by @shideqin (#152). It grew a
  Sleep option and a "remember this" choice on top (#169).
- **The download mirror can be picked outright.** "Project mirror"
  reverses the order instead of only serving as a fallback, for a
  network where GitHub never answers at all.

### Fixed
- **Plugin installs, which were broken outright** (#163). The manifest
  came from listing the repository's releases and filtering them, which
  cost ~1 MB per lookup to read three fields and had grown past its own
  read ceiling. It now comes from a file tracked in this repo, with the
  asset host as the second leg and the old path surviving only as a
  last resort.
- **A host deleted from the open editor came back**, and survived a
  restart, because it really was written back. Any host in a group hit
  this on every delete.
- **A single-message edit made after the editor drawer reappeared was
  silently dropped** (one toggle, one keystroke).
- **A password pasted into the Host field landed in a plaintext
  column.** `user:secret@host` is how much of the world's documentation
  writes a connect string; the secret now goes to the encrypted
  password field or is dropped, and is never printed.
- **The Host field stopped carrying the whole connect string** (#171).
  A pasted `user@host:port` is split across the three fields it
  belongs in, and a stored value that cannot resolve says what is wrong
  with it instead of failing with a bare resolver error.
- **A high-resolution wheel scrolls again** (#150), including inside a
  TUI holding mouse tracking, where the fragments were being counted
  twice.
- **The numpad Enter types a newline**, not an interrupt (#162), and
  idle arrow keys no longer get eaten by the keyboard navigation ring
  (#168).
- **Four field reports on the 0.13 tmux manager** (#157, #158, #159,
  #160), including switching sessions while the shell is busy and a
  listing that raced the attach.
- **A lock could leave the burger menu armed behind the lock screen**,
  and now sweeps the paste confirms with it (#169).
- **The AI tool result is the command's output**, not whatever the
  screen showed when it ran.
- **The self-replacing updater stops failing in ways that look like
  success**, refuses while another window is live, and Windows toasts
  say Oryxis rather than Windows PowerShell.
- **The window's maximized state follows the OS**, by @shideqin (#142).
  Windowed geometry is judged against the OS truth rather than the
  app's own memory.
- **A dropped port forward keeps retrying under auto-reconnect**, by
  @latent-9 (#149), and pending retries are kicked when another
  connection proves the network is back.
- **The debug log clears on Windows**, by @shideqin (#154), reworked to
  truncate through a second handle since delete-then-recreate hits the
  delete-pending trap.
- **The import hub's `.oryxis` handoff reaches a visible dialog**, and
  export/import decline loudly on a vault that cannot decrypt rather
  than failing silently.
- **Monitoring prefers a live tab's session** on every path that
  establishes one, instead of redialing a stored credential that is
  already known to fail, and the sample window belongs to the machine
  rather than to the row (#156).
- **The sync passphrase field drifted, in two ways that could leave a
  snapshot nobody can open**, by @shideqin (#170). The field never
  pre-fills the stored value (a masked pre-fill turned every later
  keystroke into an append that silently swapped the group key) and
  typing never writes through: the key changes only when a round
  succeeds with the typed value. On top of that, only a MANUAL round is
  allowed to read the typed buffer at all, so the 5-minute tick can no
  longer seal a snapshot with a half-typed passphrase, and a round
  carries its key by value so typing while one is in flight cannot
  store a value the snapshot was not sealed with. A failed round now
  shows the recovery path under the error.
- **Git sync hung, and a restart could fail to decrypt**, by @shideqin,
  who also moved the blocking sync work off the UI thread.
- **Settings fields keep Tab and the arrow keys**, by @shideqin: the
  keyboard-navigation ring was claiming keys a focused input needed.
- **The harness sandboxes the vault on Windows**, by @shideqin (#153),
  so a test run can never touch the real `~/.oryxis`.
- **Four committed E2E tests had gone stale**, each hiding the next,
  and the suite now runs in CI so a UI change that invalidates one is
  visible immediately.

### Changed
- **One crypto backend in the binary, and it is aws-lc-rs.** `ring` is
  gone from the app's dependency graph; the whole `windows` crate
  family resolves to a single version on every target.
- **keyring 4** for the OS keychain integration.
- The `~/.oryxis` tree resolves through one `ORYXIS_HOME`-aware path
  resolver instead of each caller rebuilding it, following the harness
  vault sandbox by @shideqin (#153).

## [0.13.0] - 2026-08-10

The sync-anywhere and organization release. The encrypted vault
snapshot now travels over a folder (which means any cloud you already
use), a Git remote with history, or a WebDAV server; groups hand
their defaults down to the hosts inside them; the terminal sidebar
splits into two dockable regions and gains an mRemoteNG-style hosts
tree, with the same tree joining the dashboard as a third view mode;
a second tab to a connected host rides the connection it already
has; and MobaXterm, Xshell, SecureCRT, FinalShell, Termius, mRemoteNG
and plain CSV all import their sessions.

### Added
- **Sync through a WebDAV server.** A fifth transport, for the
  Nextcloud, ownCloud or Synology you already run: a collection URL, an
  account and an app password, with no desktop client to install and no
  OAuth application to register anywhere. It reaches those servers
  directly, which is the difference from pointing the folder transport
  at their sync client, and it is the only file transport that DETECTS
  a conflict rather than healing one afterwards: the write carries the
  tag the server handed out on the read, so a server that changed in
  between refuses it and the round starts again on top of what landed.
  A URL ending in `/` is a folder and gets the shared snapshot name;
  one naming a file is used as typed, so two sync groups can share an
  account. The folder is created on the first round if it is not there.
  It is also the first transport where a credential crosses the wire at
  all (SFTP is encrypted, Git delegates to ssh, a folder never leaves
  the machine), so the card warns when a `http://` URL would send the
  account password in the clear.
- **Sync through a Git remote, with history.** A fourth transport:
  the same encrypted snapshot, committed to any Git remote you can
  clone, whether that is a forge or a bare repository on your own box.
  It is the only transport that keeps past versions, so a vault wrecked
  by a bad import or by a deletion that synced from the wrong machine
  can be read back from an earlier commit. It is also the strictest
  about conflicts: a push rejected as non-fast-forward means another
  device wrote first, so the round is redone on top of theirs, never
  forced over it. A round only commits when the vault actually changed
  (the snapshot re-seals with a fresh nonce every time, so the app
  compares a fingerprint of the contents rather than the bytes) and the
  history stays readable instead of filling with empty commits. Oryxis
  drives the `git` you already have rather than bundling one, and says
  so in the setting when it is missing; authentication is git's own, and
  a remote that would have asked for credentials fails fast instead of
  hanging.
- **Sync through a folder, which means sync through any cloud you
  already use.** A third transport next to P2P and SFTP: one encrypted
  snapshot in a directory your system already mounts. Point it at a
  cloud client's folder (OneDrive, Google Drive, Dropbox, iCloud), a
  network share, a Syncthing directory or an external disk, and that
  destination works, because Oryxis never talks to the provider: it
  writes a file, and whatever owns the folder carries it. No account to
  connect, no OAuth screen, no token to renew, and nothing about the
  provider baked into the app. It is the same encrypted blob the SFTP
  transport uses and the same group passphrase, so a device can move
  between the two without leaving its sync group, and the snapshot is
  installed with an atomic rename from a temp file in the same
  directory, so a reader never sees half of one. Two machines writing
  one mirrored folder can still race, exactly as they can over SFTP,
  and the setting says so where you choose it.
- **A second tab to a host reuses the connection it already has.**
  Opening another tab (or duplicating one) to a host that is connected
  now opens a channel on the live connection instead of paying for a
  TCP handshake, a key exchange and an authentication again, and on a
  jump chain, all of that per hop. It also cannot ask for a host key or
  a second factor, because the connection it rides was already verified
  and authenticated. The connection closes when its LAST tab does, so
  closing one of two leaves the other working. Where a link is shared,
  the status bar's latency tooltip says so: when a shared connection
  drops, every tab on it drops at the same instant, and that reads as
  several tabs breaking for no reason unless the app says otherwise.
  Reuse is silent about failure by design: a pooled connection that
  turns out to be unusable (a server at its session cap, a link that
  died between checks) just dials fresh. Settings > Connection has the
  switch, on by default.
- **The terminal sidebar splits into two dockable regions (#102).**
  Every sidebar tab (Chat, Snippets, History, Files, Monitor, Tmux,
  Host config and the new Hosts tree) picks its side in Settings >
  Terminal: left, right, or hidden entirely for the tabs you never
  use. Both regions can be open at once, each with its own tab strip,
  active tab and width, so the AI chat can live on the left while the
  file browser stays on the right. The toggle hotkey follows the tabs
  (a lone populated region is always its target, and with both
  populated it prefers whichever is open, so the keyboard can always
  close what is on screen); a second binding, Ctrl+Alt+B, drives the
  counterpart region, mirroring how VS Code pairs its two side bars.
- **A hosts tree in the terminal sidebar, and a tree view for the
  dashboard (#102).** The new Hosts sidebar tab shows the vault as an
  mRemoteNG-style tree: folders nest to any depth and fold in place,
  a click on a host opens a session next to the one you are in, saved
  split-pane arrangements sit alongside their hosts, and dynamic
  (ECS / Kubernetes) groups resolve their tasks and pods inline on
  expand. The dashboard gains the same shape as a third view mode
  next to grid and list: dense rows, indent guides, the fold chevron
  on the leading edge, with search force-expanding every match and a
  matching folder showing its whole subtree. Both trees share the
  vault's sort preference, one search predicate (label, hostname,
  tags and username) and the expansion state, so what unfolds in one
  is unfolded in the other.
- **Groups hand their settings down to the hosts inside them.** A group
  now carries defaults: login user, identity, proxy identity, terminal
  theme, startup snippet, environment variables and the port new hosts
  are created with. A host that leaves a field empty takes it from the
  nearest group that sets it, walking up through nested subgroups, so
  forty hosts behind one bastion get the proxy and the identity set
  once instead of forty times. Resolution is per parameter: a group
  that only sets the proxy leaves everything else to be answered
  further up or by the host itself, and a subgroup overrides just what
  it names. The host editor says where an inherited value comes from
  ("Inherits deploy from prod"), and typing your own still wins.
  Environment variables merge by name rather than replacing, so a host
  adds to what its groups provide and only overrides the variables it
  names. Group credentials are a reference to an identity, never a copy
  of a password: the vault keeps one place where a credential can live.
  The port is the exception to inheritance and deliberately so, it
  prefills a host created inside the group and never touches one that
  already exists, because a host that connects today must not change
  destination because a folder gained a default. Group defaults ride
  sync and portable export like any other field, and older peers ignore
  what they do not understand.
- **tmux sessions from the sidebar (#116).** A new terminal-sidebar tab
  lists the tmux sessions running on the focused pane's host, with their
  window count and whether a client is already attached, and creates,
  attaches to and kills them. The listing, the create and the kill run
  `tmux` itself on an exec channel multiplexed onto the session you
  already have, so nothing is installed on the server, no rc file is
  written and nothing is injected into your shell; attaching is the one
  action that reaches your shell, because it is the command you would
  have typed, sent into the pane the tab sits beside when you click. New
  sessions are created detached so they never fight the pane you are in,
  and a kill asks first, naming the session. A host without tmux says so
  instead of offering buttons that could only fail, and a host whose tmux
  server is not running gets the empty state that invites a first
  session. Session names are text the remote host printed, so they are
  quoted wherever they are used and a name carrying a line break is
  refused rather than quoted. Off by default, behind a Features & Plugins
  toggle like host monitoring, and it never polls: the list is read when
  the tab opens, after your own actions, and when you ask.
- **Pick which disks a host reports.** SSH > Integration takes a
  Monitoring disks choice next to the monitor toggle: Auto keeps the
  automatic behaviour (one row per storage device), Custom reports only
  the mount points you list, with `*` matching any text (`/mnt/*`). The
  choice reaches every surface at once, the Monitor tab, the dashboard,
  the status bar and the threshold alerts, so a mount you chose not to
  watch cannot raise a toast either. It rides sync and portable export
  like any other per-host setting.
- **Terminal background image, per host.** Settings > Terminal takes a
  picture to lay behind the grid, with a fit (cover / fit / stretch /
  centre / tile) and a fade that dissolves it into the theme's
  background colour so text stays readable. Every host can override the
  picture, the fit, the fade and the opacity, or opt out of the global
  picture entirely: each field inherits on its own, so a host that only
  changes the fade still follows the global picture. The image is drawn
  per pane, like Windows Terminal and iTerm2, and blocks a TUI paints
  itself stay solid. Only the path is stored, never the pixels, so the
  vault stays small and sync ships bytes instead of megabytes.
- **Translucent terminal background.** Settings > Terminal >
  Background opacity fades the terminal down to 30% so the desktop
  shows through, while the tab strip, panels, sidebars and the status
  bar stay opaque: the effect lands on the surface you stare at, not on
  the text you have to read. Only the terminal's own backdrop carries
  the alpha, so split gutters and the empty area fade with it instead
  of floating on an opaque plate, and a coloured block from a TUI stays
  solid. A see-through window is set up when Oryxis starts, so the
  first step away from 100% offers a restart; every later change,
  including going back to opaque, applies live. Linux (Wayland, or X11
  with a compositor) and macOS; on Windows the graphics stack usually
  offers no transparent surface, where the setting has no effect
  rather than a broken window.
- **First-run setup gained two steps.** "Make it yours" lists the
  optional features (AI, SFTP, Sync, RDP/VNC, host monitoring, SSH
  agent) with one line each and a live toggle, so the app can be
  shaped before it is first used and nothing niche is on by surprise;
  "Bring your hosts along" offers to read the sessions of the SSH
  client you already use, and opens the importer right after your
  vault is created. Skip still jumps straight to the password step.
- **MobaXterm, Xshell, SecureCRT, FinalShell, Termius and CSV
  import.** The Import hub grows a "Choose folder" button for the
  clients that keep one file per session (Xshell `.xsh`, SecureCRT
  session `.ini`, FinalShell connection JSON): point it at the
  sessions directory and the whole tree comes in as one batch.
  MobaXterm reads its `MobaXterm.ini` bookmarks (SSH sessions, with
  their folder recorded per host). Any CSV of hosts imports through
  header matching, so a Termius export, a spreadsheet you keep by
  hand or a column of hostnames all work; a `password` column is
  honored and lands encrypted. Clients that encrypt their stored
  passwords with a per-install key (Xshell, SecureCRT, FinalShell,
  MobaXterm) import the host and say so in its notes instead of
  guessing.
- **mRemoteNG import (confCons.xml).** SSH, Telnet, RDP and VNC nodes
  map (RDP/VNC onto the remote-desktop hosts), the container
  hierarchy is recorded in each host's notes, and HTTP/rlogin nodes
  are named as not importable. Passwords decrypt with mRemoteNG's own
  scheme (AES-GCM, PBKDF2 key) including fully-encrypted files: a
  file with a real password makes the Import hub ask for it and
  retry, and passwords land straight in the vault's encrypted
  column. KiTTY sessions also import now (same format as PuTTY,
  detected by its own registry hive).
- **One Import entry with format auto-detection.** The per-source
  buttons collapse into a single "Import" that opens a small hub
  naming every supported source (Oryxis export, OpenSSH config,
  PuTTY .reg, WinSCP .ini/.reg); pick any file and its format is
  detected from the content, including a full-registry export
  carrying PuTTY and WinSCP sessions at once, which imports as one
  combined batch. An unrecognized file says so inline instead of
  guessing.
- **PuTTY session import.** "+ Host ▾" (and the first-run empty
  state) gains "Import PuTTY sessions (.reg)": pick a regedit export
  of the PuTTY hive and every session lands in the same tick-per-host
  preview the `~/.ssh/config` import uses, deduplicated against
  existing labels. SSH, Telnet and serial sessions map with their
  ports, users, agent/X11 forwarding, SOCKS4/SOCKS5/HTTP/command
  proxies and serial line parameters; a session's `.ppk` path is kept
  in the notes with the auth method set to Key (the Keychain's Import
  Key reads .ppk when you're ready). Sessions the parser can't map
  (raw/rlogin, or no host) are listed by name instead of vanishing.
  UTF-16LE exports decode transparently. First of the importer set;
  mRemoteNG, Termius and generic CSV are next.
- **WinSCP site import.** Reads a portable `WinSCP.ini` or a registry
  export, same preview flow. SFTP/SCP sites map with host, port,
  user, agent forwarding and proxies; FTP/WebDAV/S3 sites are listed
  as not importable. Stored passwords come along when WinSCP's
  reversible scheme protects them (they land straight in the vault's
  encrypted column); sites guarded by a WinSCP master password import
  without the password and say so in the notes.
- **Multi-host monitoring dashboard (#95).** A Monitoring pill next to
  Hosts (once the host-monitoring feature toggle is on) shows live
  vitals cards for every opted-in host at once. Hosts with an open
  terminal tab are read over that session; the rest get a headless,
  probe-only SSH connection dialed with the stored credentials
  (strict host key, TOTP autofill, jump chains and proxies included)
  and pooled with an idle TTL, so leaving the view and coming right
  back doesn't redial the fleet. Cards show CPU and memory gauges
  plus network, fullest-disk and GPU at a glance; clicking one opens
  a detail panel rendered by the same code as the per-session Monitor
  sidebar (CPU sparkline, swap, load, GPUs, collapsible disk and
  listening-port sections), plus the explicit "Open terminal" /
  "Retry" actions (a card click never opens a session by itself).
  Search and the shared host-tag filter narrow the fleet, a toggle
  switches between the card grid and a table sortable by any column
  (label, CPU, memory, network, disk, uptime), polling only runs
  while the view is visible, and the same sample rings feed this
  view, the sidebar Monitor tab and the status bar, so they can
  never disagree.
- **Downloadable terminal font pack (#109).** The terminal font picker
  now carries a curated catalog of popular Nerd Font builds: JetBrains
  Mono, CaskaydiaCove (Cascadia Code), Fira Code, Hack, MesloLGS (the
  powerlevel10k standard), Roboto Mono, Ubuntu Mono and Iosevka. Each
  downloads individually the first time it is picked (2-3 MB, Iosevka
  13 MB), verified against a pinned SHA-256, cached under
  `~/.oryxis/fonts/`, applied to live sessions without a restart, and
  routed through the China download mirror like the CJK fonts. Nothing
  is bundled into the installers and the built-in SauceCodePro Nerd
  Font remains the default.
- **Wake-on-LAN.** Store a MAC address on any host (any common
  notation) and wake the machine from the host card's menu with a
  magic-packet broadcast, before SSH, RDP or anything else can reach
  it.
- **GPU gauges in the Monitor tab.** The agentless host monitor probes
  `nvidia-smi` on the same exec channel it already uses, and renders a
  GPU section only when the probe answers.
- **Quick connect from outside the app.** Oryxis registers as the OS
  `ssh://` URL handler and accepts `oryxis user@host` on the command
  line; a running instance lands the session in a new tab instead of
  starting a second window.
- **Bastion login scripts (#122).** Some jump boxes authenticate INSIDE
  the terminal after SSH is already up: JumpServer / KoKo and friends
  drop you in a menu that asks for an asset, a user and a password. A
  login script is a reusable expect/send sequence attached to a host,
  with the answers that are secrets read from the vault at send time
  rather than stored in the script. Ships with a JumpServer preset and
  a generic interactive-bastion one, both editable; `{placeholders}`
  let one script serve many hosts, each with its own asset and target
  user. The run is bounded on every side: steps fire strictly in order,
  each has its own deadline, the whole run expires, any keystroke of
  yours aborts it, and the host's startup command is held back until
  the script actually lands you on the asset. Managed in Settings >
  Connection, created inline from the host editor, and it rides sync
  and portable export like any other entity.
- **Stored passwords at password prompts (#117).** When a session
  blocks on `[sudo] password for you:` (or `su`, `ssh`, a key
  passphrase), a popup at the cursor lists the passwords the vault
  holds for that host and your identities. Down engages it, Enter
  sends, Esc hides. Nothing is ever sent without picking a row, so
  typing your own password is never interrupted, and prompts that ask
  you to CHOOSE a password (`passwd`, key generation) are recognized
  and never offered one. The credential is decrypted at the moment you
  pick it, written straight to that one pane (never broadcast, never
  mirrored into command history) and scrubbed from memory after the
  write. On by default; one toggle in Settings > Terminal.

### Removed
- **"Force exact directory following (OSC 7)" is gone**, and the app no
  longer types anything into your shell. That Settings > SFTP toggle
  wrote a `PROMPT_COMMAND` setup line into the session on connect and
  tried to erase its own echo afterwards, but nothing guaranteed the
  shell had reached a prompt when the bytes arrived: on a host with a
  long MOTD or a slow `/etc/profile.d` they landed first, the terminal
  echoed them raw, and the self-erasing trailer wiped a region
  calibrated against a cursor position that never existed, leaving the
  setup block sitting on screen for the rest of the session. No other
  terminal installs shell integration this way (kitty ships a bootstrap and
  `exec`s the login shell, VS Code injects only into shells it launches
  itself and documents that plain SSH is out of scope, WezTerm and
  iTerm2 hand you a snippet), so the snippet is now the supported path
  and it lives in your own dotfiles: see the new **[cwd
  guide](docs/CWD.md)**. The Files sidebar keeps following your
  directory through the window title without it. A stale
  `sftp_force_osc7` row in an old vault is inert.

### Fixed
- **The folder and Git transports were still running the peer-to-peer
  engine.** Picking either of them left QUIC listening and mDNS
  advertising on the local network, and the engine's own timer kept
  syncing peer to peer behind the file the user believed they had
  chosen. The same mistake put the pairing code, the LAN device count
  and the signaling / relay / port fields in front of them, along with
  a "Sync now" button that fired the peer-to-peer path and did nothing
  at all. Every one of those now asks whether the transport IS
  peer-to-peer, rather than whether it is not SFTP.
- **Auto mode did nothing on the folder and Git transports.** The
  cadence timer only ever mounted for SFTP, so a user who picked Auto
  got a transport that moved only when clicked. One timer now serves
  every snapshot transport, and none of them fires while the vault is
  locked: a soft auto-lock keeps the app running, and a round would
  otherwise reach a server with stored credentials on behalf of a vault
  the user believes is closed.
- **Switching transports showed the previous one's result.** The status
  line was cleared for SFTP only, written when there were two of them,
  so moving away and back read as if the new transport had just synced.
- **A folder or Git round left the screen showing stale data.** The
  round merges on its own vault handle, so records it pulled stayed
  invisible until the next restart. Both reload now, as the SFTP round
  already did.
- **A portable export carried the sync passphrase as unreadable
  bytes.** It is encrypted under the exporting vault's master key, so
  it arrived in the target vault undecryptable; inert until that vault
  changed its master password, a pass that walks every encrypted
  setting and aborts on the first one it cannot read.
- **The latency segment stops claiming a dead link is fast.** The status
  bar's RTT read the last successful round trip, which keeps its value
  after the server goes quiet, so a connection that had stopped
  answering still showed a healthy number. Silence now replaces the
  measurement ("no reply 21s") instead of colouring it, and the figure
  itself is colour-banded: green under 80 ms, amber under 250 ms, red
  above, so the bar is glanceable rather than something to read. Hovering
  gives the average, the peak, the jitter and how many probes timed out.
- **The Monitor tab lists one disk per device, not one per mount.**
  `df` reports mounts rather than storage, so a rooted Android phone
  answered with 176 disk rows, nearly all of them `/system/bin/*` bind
  mounts repeating the system partition's figures, and the status bar
  picked one of those as the busiest disk. Pseudo filesystems are now
  filtered on a wider list, one row survives per source device (its
  shallowest mount point), and a bind mount whose numbers repeat a real
  device is dropped. A CIFS share, a mergerfs pool and ZFS datasets are
  none of those and stay.
- **A master password change no longer orphaned TOTP secrets.** The
  re-encryption pass did not list the connections table's
  `totp_secret` column, so changing the master password left every
  stored 2FA secret undecryptable. The column list is now covered by a
  rotation test.
- **Duplicating a host kept its whole configuration.** The duplicate
  was built field by field from a list that had drifted behind the
  model, silently dropping quirks, algorithms, port forwards, env vars
  and more. It clones the host now, so a new field is carried by
  construction.
- **The sudo-password snippet action works on split tabs.** It resolved
  the host by tab LABEL, so a tab holding two hosts sent the wrong
  host's password or none at all; it resolves from the pane now, and
  scrubs the credential after the write like every other secret path.

## [0.12.0] - 2026-08-04

The workspace and community release. Settings opens as a tab beside
your sessions, a dragged tab becomes a split pane right where you drop
it, community themes have a home you can contribute to on GitHub, SFTP
downloads ask before overwriting, port forwards to one host share a
single SSH connection, and servers that demand a second factor
authenticate properly. Under it all, the app state and the largest
dispatch handlers were rebuilt into small routers, and the debug log
can no longer record a secret.

### Added
- **Settings as a tab (#120).** Settings gets a chip in the strip the
  first time you visit it, so tuning something against live content
  (tab bar options, terminal theme, status bar segments) is one click
  back and forth instead of a round trip. The chip drags, reorders and
  closes like any other tab, each section remembers its scroll
  position, and closing it returns to the tab you were on. Never
  pinned, never persisted across restarts.
- **Community themes (#118).** The repo gains a `themes/` directory
  anyone can contribute to with a browser-only pull request; a
  generator builds the gallery index, validates palettes the way the
  importer does, and labels (never rejects) low-contrast entries. Both
  in-app theme galleries carry a card that opens the site gallery at
  <https://oryxis.app/themes>, whose Copy button pastes straight into
  Import. First contribution: True Black OLED by @AC-Lover, a
  true-black UI theme for OLED panels with a matching terminal
  palette. Night Owl and Night Owl Light join the built-in set, and a
  new contrast test suite holds every shipped palette to a measured
  floor.
- **Theme galleries.** The terminal and app theme grids move out of
  the Settings page into scrollable gallery modals; Settings shows one
  card, the theme actually in force, and clicking it opens the
  gallery. Deleting a custom theme now asks first.
- **Drag a tab into the terminal (#112).** Release a dragged tab over
  the content area and its sessions merge into the tab showing there;
  where you release picks the split (a pane's side splits that pane,
  the grid's outer band lays a full-width pane). The preview shows the
  half the pane will occupy, a grouped tab moves all its panes, and
  the source is detached rather than closed so live sessions survive
  the move.
- **Tab numbers, duplicate placement and host address (#110).**
  Optional tab numbers (before the label, or in place of the host
  icon) with Ctrl+digit counting tabs the way the numbers read;
  Duplicate Tab lands next to the original (or end / start, your
  pick) and works on configured local shells; an optional second line
  shows the host address, formatted like the host cards and masked
  under Privacy Mode. Labels are now measured and truncated in
  pixels, so mixed Latin/CJK labels stop overflowing their chip.
  Based on #110 by @shideqin.
- **Uniform tab width (#112).** A Tab width mode that sizes every tab
  to the widest label so the strip stops reflowing when you switch
  tabs, with a small / medium / large ceiling so one long name cannot
  decide the width for everyone.
- **Split-pane pack (#113).** The focused pane gets a visible 2 px
  accent outline (it was painted under the canvas before); a setting
  drops the outline on inactive panes; Ctrl+Shift+Z zooms the focused
  pane to the whole tab, tmux-style, and the zoom follows focus; the
  seam between panes is the resize handle, with the panes flush and
  the resize cursor showing over it; an optional pane gap (4 / 8 /
  12 px) pads the window edge to match.
- **PRIMARY selection (#106).** Selecting text sets an X11-style
  PRIMARY buffer separate from the clipboard; middle-click pastes it
  (falling back to the clipboard when nothing was selected), the new
  Paste selection action ships on Shift+Insert, and a faint ghost
  band shows what a PRIMARY paste will insert after the highlight
  clears. Under copy-on-select the gestures keep reading the
  clipboard, since there selecting is the copy.
- **Drag-and-drop upload onto the terminal (#106).** Drop files or
  folders on a terminal pane to upload them to that pane's host: SFTP
  on the live connection when the shell's cwd is known (nothing typed
  into the PTY), ZMODEM as the fallback, quoted paths pasted into a
  local shell. One progress overlay with cancel covers both
  transports, and the sidebar Files browser is a drop target too,
  with its visible directory as the destination.
- **Mouse buttons as shortcuts.** Middle, Back and Forward buttons
  bind to actions in Settings > Shortcuts like keyboard chords, with
  the same capture and conflict flow; middle-click paste is now just
  a default binding. Back / Forward walk the visited directories in
  the SFTP panes and the Files sidebar first (#114), and bind freely
  everywhere else.
- **Pick the editor where you open the file (#114).** The SFTP row
  menu grows an "Open with" family: the OS association, the default
  editor, "Other application..." for this open only (also the first
  Open with that works on Linux), and "Set default editor..." right
  there instead of in Settings. The edited copy still confirms before
  going back up, and reopening lands in the application that already
  holds the file.
- **Downloads ask (#114).** A download that finds its name taken
  raises the same overwrite prompt uploads have always had, with
  apply-to-remaining; "Download to..." picks a destination for one
  transfer, and an off-by-default setting makes the plain download
  action ask every time.
- **SFTP move (#115).** Move files and folders between hosts: the
  copy is verified before anything is removed, a move within one host
  is an instant rename that keeps ownership and timestamps, and a
  relay onto the same file or into its own subtree is refused.
- **Sidebar Files interaction (#123, #114).** Click selects with an
  accent highlight, double-click enters a directory, Copy path keeps
  the scroll position, and the selection survives a refresh of the
  same directory. The recent-folders dropdown is now persisted per
  host, so the history survives closing the host.
- **Kill the process behind a port (#96).** The Monitor tab's
  listening-port rows offer Kill process (SIGTERM) and Force kill
  (SIGKILL) behind a confirmation naming host, port, process, PID and
  signal. The PID is re-resolved on the host before anything is
  signalled, sudo escalation is offered only when the user can
  actually escalate, and the password travels on stdin, never in the
  command line.
- **Port forwards, explained on the host.** The host editor's inline
  forwards section now says what it is (session-scoped, -L only) and
  lists the standalone rules that travel through this host, live
  status included, with a button to the Port Forwarding screen. The
  rule cards get the standard kebab menu and a confirmation before
  delete (#119).
- **Command history that survives tmux (#92).** The shell can report
  its own command lines (the VS Code OSC 633 sequence), which is the
  only capture path tmux cannot hide. Reported lines are accepted
  only when they carry a per-vault key: Settings > Terminal has Copy
  (the snippet with the key baked in) and Rotate, `docs/TMUX.md`
  documents the setup, and Oryxis never writes to a host's dotfiles.
- **Linear transcript mode (#92).** A recording that lived on the
  alternate screen (tmux, vim, pagers) opens in a linear rendering
  where every repaint is appended in order, instead of a faithful
  replay that shows one final frame. A header button flips between
  Linear and Rendered, and both keep selection, copy and search.
- **Saved AI conversations (#105).** Chat conversations are saved to
  the vault, encrypted like session recordings, and read back from
  the History timeline next to the recordings, host-filtered and
  read-only. Saving is a choice: Settings > AI grows "Save
  conversations" (on by default), and turning it off stops recording
  without deleting what is already stored.
- **AI Reasoning toggle.** Chain-of-thought is billed and never
  displayed, so it is now off by default where the provider can be
  told (DeepSeek, Gemini); turning it on restores each provider's own
  default.
- **Copy works over a recording.** The player answers the copy,
  select-all and scrollback-paging chords with your own bindings, so
  text selected in a replay can leave it.
- **Keyboard Delete across the vault.** Delete removes the ringed
  host, group, key, identity, snippet, forward rule, known host or
  session log through the same confirmation its context menu uses.
  Cloud accounts and proxy identities, which deleted outright before,
  gain the confirm for the mouse too.
- **Microsoft Store.** Oryxis ships as an MSIX package on the
  Microsoft Store (x64 and arm64). A Store install knows it is one:
  the self-updater stands down and updates arrive through the Store.

### Changed
- **Typing snaps back to the live edge (#111).** Scroll up, type, and
  the view returns to the prompt, matching every modern terminal. Now
  on by default (vaults that stored "off" stay off), and triggered by
  bytes actually reaching the PTY, so typing into the sidebar or the
  AI chat never yanks the terminal out of its scrollback.
- **Settings > Terminal regrouped.** Appearance kept what is actually
  appearance; Notifications, Integration, Sidebar and Split panes got
  their own headers. "ZMODEM download folder" is now "Default
  download folder" and lives under Behavior.
- **Forwards to one host share one SSH connection (#126).** Three
  rules against one host used to cost three transports and three
  auths on top of the interactive tab; now the first rule dials, the
  rest attach as channels on the shared handle, and the connection
  closes when the last forward drops. One dial also means one
  host-key prompt and one error instead of a storm. Deleting a -R
  rule releases its server-side bind immediately, the
  legacy-algorithm dialog's retry restarts every rule it aborted, and
  a rule stopped while its attach was in flight stays stopped.
- **Dependency security updates.** russh 0.62.4 / 0.62.5 (GHSA-g9hv,
  GHSA-5xvq, GHSA-cqjc, GHSA-m65r-rprj-r5rg: pre-auth panics a
  malicious server could trigger, and channel-data backpressure) and
  quinn-proto 0.11.15 (GHSA-4w2j-m93h-cj5j, memory exhaustion in the
  sync transport).

### Fixed
- **2FA and multi-step auth (#125).** A server that accepts the first
  factor and requires more (RFC 4252 partial success: sshd
  `AuthenticationMethods`, Bitvise compound auth) was treated as a
  rejection. Every auth path now continues the chain, offering
  whichever remaining method still has an answer; a stored TOTP
  secret completes the second factor silently, the 2FA modal prompts
  otherwise. Follow-ups: the step-1 key is re-offered when the server
  wanted the methods in its own order, chained keyboard-interactive
  factors work, and a failed connect no longer strands the 2FA prompt
  on top of the error.
- **The debug log stops recording secrets.** The stall-diagnostics
  ring formatted every message, including each keystroke of a
  password field and pasted clipboard text, into the file users are
  asked to attach to issues. Secret-bearing payloads now print
  `<redacted>` by construction, and a structural test keeps them that
  way.
- **The Files folder history is encrypted.** The per-host
  recent-folders list was plain JSON in a table readable without
  unlocking; it is now encrypted like the rest of the trail, and a
  plaintext row from an older build is deleted rather than preserved.
- **Command history cannot be planted.** A reported command line
  without the vault's key is refused, fail-closed, so nothing a host
  prints can put a command in your history that you never typed, and
  a spoof attempt no longer silences real capture.
- **Closing many tabs tears them down like closing one.** "Close
  other tabs" and "Close all tabs" skipped the whole teardown, leaving
  connections, port forwards and AI streams running for tabs nobody
  could see.
- **Pending actions follow their tab.** An in-flight paste, a pending
  pane split and a parked snippet-variables form all resolved their
  target by position, so a tab closed at the wrong moment could land
  text, a pane or a running command in another host's session. All
  three now name the tab by id and drop rather than guess when it is
  gone.
- **Tab switching stalls (#104).** One in-flight connect painted its
  progress screen over every tab, which read as a seconds-long stall;
  the connect screen is now scoped to its own tab. A mark flood no
  longer costs O(n) per mark on the UI thread, and Debug logging
  gains an event-loop stall watchdog that names which layer stopped.
- **Split-tab repairs (#108).** Closing the first pane of a split no
  longer leaves the tab wearing the closed pane's name, splitting a
  pane honors the configured local shells instead of hard-coding the
  OS default, and a grouped tab's label stops spilling past its chip.
- **Grabbing an SFTP row to drag it.** The press now hit-tests the
  drawn row rectangles instead of trusting hover state, which
  tooltips and event ordering could drop; grabbing a row by its name
  works as often as grabbing it by its icon.
- **SFTP guards.** The relay containment check resolves symlinks, a
  move into the folder the item already sits in is refused instead of
  silently renaming, and two error lines that were English literals
  went through i18n in all 23 languages.
- **A dropped forward climbs back (#101).** A running forward that
  loses its connection now retries under the same auto-reconnect
  setting that governs hosts, instead of only auto-start rules ever
  retrying; and when an ssh-agent gains keys (KeePassXC unlocking),
  every pending rule retries immediately instead of sitting out a
  two-minute backoff.
- **No "New output" toast for the tab on screen.** Alt-tabbing back
  no longer raises a notification about the very output you are
  reading.
- **The find bar fits a narrow pane** instead of clipping its own
  close button; it sheds the counter and arrows first, and Close is
  never dropped.
- **Selection and focus.** A pane drops its highlight when focus
  leaves it, so a three-way split no longer shows three highlighted
  blocks; the session player, which is never focused, keeps its
  selection and honors the right-click scheme; the PRIMARY ghost
  stands down when the grid scrolls under it.
- **Progress bars at the extremes (#107).** A weightless bar segment
  rendered full instead of empty, so a 4.4 GB transfer showed a full
  bar after 358 KB, the update modal inverted at 0% and 100%, and the
  monitor gauges read saturated on an idle host. All three share one
  track widget now, RTL-aware.
- **screen's title sequence (#88).** RHEL/CentOS prompts under a
  `screen*` TERM printed `ESC k` payloads into the grid, doubling the
  prompt and breaking Ctrl+R redraws; the sequence is stripped and
  surfaced as the window title.
- **Thinking-mode AI providers (#105).** DeepSeek v4's
  `reasoning_content` and Gemini's `thoughtSignature` are captured
  and replayed as their APIs require, so thinking models stop failing
  with a 400 on the second message and after tool calls.
- **A retried AI answer replaces the saved one.** Retrying an errored
  reply no longer leaves the stored conversation ending on the error
  forever.
- **A destructive confirm says Cancel (#112),** not "Close" sitting
  beside "Close group"; closing a grouped tab asks first and names
  how many sessions are at stake.
- **Vertical dock polish (#87).** The Underline tab style no longer
  squeezes inactive chips or draws stray vertical ticks on side
  docks, and the active-tab gradient paints across the chip instead
  of fading along it.
- **Smaller fixes.** The pane-gap setting pads the window edge too;
  the AI error card fills the sidebar and wraps long payloads; the
  shortcut-alignment row joins the settings search index; the
  mouse-gesture placeholder in Shortcuts is readable on accent
  themes.

## [0.11.0] - 2026-07-25

The remote-desktop and observability release. Remote GUI applications
draw on your local display over a cookie-spoofed X11 channel, every SSH
tab can show live host vitals without installing anything on the server,
host folders nest to any depth, and the tab strip and terminal sidebar
dock wherever you want them.

### Added
- **X11 forwarding.** Per-host opt-in (also imported from `ForwardX11`
  in `~/.ssh/config`) so remote GUI applications draw on your local X
  display. The remote never learns your real cookie: a fake
  `MIT-MAGIC-COOKIE-1` is minted per session and announced in
  `x11-req`, then verified in constant time on every X11 channel the
  server opens and swapped for the real one before a single byte
  reaches the X server, the way OpenSSH does it. `$DISPLAY` resolution
  covers Linux/BSD unix sockets, XQuartz launchd paths on macOS, the
  TCP endpoint VcXsrv / Xming serve on Windows (including the no-
  `DISPLAY` fallback), bracketed and bare IPv6 literals, and the legacy
  `hostname/unix:N` transport form. Displays running with access
  control off (WSLg, VcXsrv `-ac`) work too: the auth is stripped on
  the way in rather than substituted. Trusted forwarding only
  (OpenSSH's `-Y`); the X SECURITY extension that `-X` relies on denies
  the pointer and keyboard grabs Java and Swing toolkits need.
- **Host monitoring (#83).** A sidebar Monitor tab reads CPU, memory,
  swap, disk and network off the SSH connection you already have, on an
  exec channel multiplexed onto the live session, with nothing
  installed on the server. Linux `/proc` is the primary source, with
  BSD and macOS probe fallbacks. The feature hides behind a Features &
  Plugins toggle and is then opt-in per host, with an "Enable for all
  hosts" switch and a configurable probe interval. The panel lists the
  host's listening ports and turns any row into a local port forward in
  one click, honoring the listener's bind address; thresholds raise
  alerts; a long mount list collapses; an optional status-bar segment
  keeps the headline numbers visible with the panel closed. Every
  number crossing the wire is treated as untrusted: the probe
  arithmetic saturates rather than wrapping, and truncated or forged
  payloads degrade to "unknown".
- **Nested host folders (discussion #67).** Groups now hold groups, to
  any depth. Pickers render the full breadcrumb path so two folders
  named `prod` under different parents stay distinguishable, typing a
  path creates the whole chain at once, deleting a folder promotes its
  children to the grandparent instead of orphaning them, and a cycle
  guard keeps a re-parent from swallowing its own subtree. A dedicated
  "New group" button creates top-level folders from the dropdown and
  the empty state.
- **Tab strip on any edge (#87).** Dock the tabs top, bottom, left or
  right. The side docks turn the strip vertical, can absorb the window
  chrome (burger, Home, window buttons) to reclaim the top bar
  entirely, and can run full height. Inactive tabs take a separation
  style of your choice: none, border or underline.
- **Terminal sidebar placement (#85).** Dock the sidebar on the left as
  well as the right, choose which tab it opens on, and have it open
  itself on connect, globally or overridden per host.
- **Configurable status bar (#83).** Every segment is individually
  toggleable, and latency, transfer size and the shell's working
  directory join the existing ones.
- **Settings search.** Type in the Settings sidebar and matching rows
  highlight in place with a per-section hit-count badge; Enter and
  Shift+Enter step through matches with a position counter. Matching
  runs against the English labels as well as the active language, so
  English terms work in any UI language.
- **Search inside session recordings (discussion #67).** The History
  screen searches the recorded content itself, not just titles,
  decrypting on demand under a bounded scan, and can filter to the
  hosts a given command ever ran on.
- **Reconnect hotkey and Close pane (discussion #67).** Ctrl+Shift+R
  reconnects the active tab; the terminal context menu gains a Close
  pane entry on split tabs.
- **Theme portability (#82).** Terminal and UI themes export to a file,
  import from one, and clone a built-in as a starting point. Two new
  built-in terminal palettes: One Dark and Gruvbox Dark.
- **SFTP Open with (#84).** Hand a remote file to a specific local
  application, with a MobaXterm-style confirmation before the edited
  copy goes back up, plus a path-history dropdown in the file explorer.
- **Self-healing port forwards.** An auto-start forward that fails to
  bind now retries with capped backoff (15 / 30 / 60 / 120 s) and shows
  a "Retrying" chip instead of silently staying down.
- **SGR 58 underline color.** The terminal paints underlines in the
  color the escape sequence asks for, live and in exported
  transcripts, instead of reusing the glyph foreground.

### Changed
- **Terminal-safe hotkey defaults (#99, #100).** Actions that used to
  sit on bare chords a shell application could legitimately want moved
  onto `Ctrl+Shift`: the new-tab picker is now `Ctrl+Shift+T`, quick
  connect `Ctrl+Shift+G`, jump-to-tab `Ctrl+Shift+J` and the local
  shell `Ctrl+Shift+L`. A bare `Ctrl+K`, `Ctrl+L` or `Ctrl+J` reaches
  the shell again. Existing custom bindings are untouched.
- **Quick connect from the first-run empty state (#97).** Typing a
  target that names itself explicitly (a username, a port or an IP
  literal) connects directly and the button reads "Connect"; a bare
  hostname still opens the pre-filled editor, preserving the
  add-your-first-host flow.
- **SSH auth timeout raised to 120 s**, matching sshd's default
  `LoginGraceTime`, so confirm-gated agents, hardware-key touches and
  2FA prompts are not cut off by a client that gives up before the
  server would. The 15 s connect timeout is unchanged, so an
  unreachable host still fails fast.
- **TOTP secret behind a disclosure toggle.** The host editor only
  shows the field once "Use TOTP" is on, so the form stays short for
  the hosts that do not use it.
- **Dashboard folder header.** A compact header with a back arrow
  replaces the breadcrumb inside a folder.

### Fixed
- **SSH agent discovery (#98).** Oryxis tries every agent it finds
  instead of stopping at the first pipe, sweeps all Pageant-style
  named pipes, dedupes offers across agents so a shared key is not
  presented twice, tries a host's pinned key first across every agent
  before the full sweep, and bounds each candidate dial so a wedged
  agent cannot stall the connect. A server that hangs up mid-sweep is
  now reported as "too many authentication attempts" instead of a
  generic failure.
- **No self-confirm on connect.** Oryxis no longer dials its own
  ssh-agent while connecting, which could pop a confirmation prompt
  for its own outgoing session.
- **Missing terminfo on the remote (#88).** The terminal type is
  probed before the PTY request and falls back when the host has no
  entry for it, with a toast explaining the substitution, instead of
  leaving a session with a broken display.
- **Session transcripts (#90, #91).** The History transcript renders
  through the real terminal widget, scrolls with sub-cell pixel
  wheels, and stays scrollable when the recording ends inside a
  full-screen application.
- **Replay sizing (#89).** The player fits the recording's font to the
  stage instead of letting a wide recording scroll, and stops
  rescaling on window resize.
- **Privacy Mode redaction (#78 follow-up).** Overlapping mask spans
  merge into a single bar instead of drawing two eye-slashes, the
  eye-slash draws in foreground ink, masks resolve by id, and the
  Monitor panel's mount list masks along with everything else.
- **Group cycles from sync.** A parent cycle created by two devices
  editing concurrently degrades to root so the affected folders stay
  visible and editable instead of vanishing. The cycle is now also
  repaired **on disk** rather than only worked around at render time:
  both the sync apply path and portable import detach the folder that
  closed the loop, picking it deterministically (newest edit wins,
  ties broken by id) so every device repairs identically instead of
  fighting over which link to cut. Nothing is lost, the detached
  folder and its hosts move to the top level. A dangling parent is
  deliberately left alone: during a transfer the parent's own record
  may simply not have arrived yet.
- **Tab lifecycle.** Reconnect and close now share one teardown path,
  Close pane targets the pane you clicked, and a tab closed mid-dial
  no longer leaves an orphan connection running.
- **Side-dock layout (#87).** Reorder drags arm on every dock edge,
  not just the top, and the width budgets, status-bar card and
  layout-toggle scroll reset behave on the vertical strips.
- **Sidebar keystrokes leaking into the PTY (#85, #87)** on non-default
  docks.
- **X11 channel drain.** Both directions drain on EOF so a reply in
  flight when one side hangs up is delivered rather than dropped.
- **RTL and Privacy in the Monitor panel**, the tab-jump search focus,
  snippet chords surviving an upgrade, zoned IPv6 in quick connect,
  nested folder creation from typed paths, and theme contrast coverage
  with interop-safe export.

## [0.10.0] - 2026-07-19

The advanced-authentication and terminal-power release. Oryxis serves
a standard ssh-agent to the rest of your system, connects with OpenSSH
certificates and FIDO2 security keys, generates keys in-app, searches
scrollback and broadcasts input across split panes, plays your
recordings back and exports them as GIFs, and hardens Privacy Mode into
a system you can tune per class. Under it all, the message and dispatch
layers were rebuilt into a fully type-safe, compiler-checked router.

### Added
- **SSH agent server** (#54). Oryxis now speaks the standard ssh-agent
  protocol, so external tools (`git`, VS Code, WSL) authenticate with
  your vault keys with no extra config. Vault keys are read-only over
  the wire and decrypted per signature, never held in memory for the
  unlocked window; a per-key "Expose via agent" flag filters the
  roster. Opt-in, it also accepts keys **pushed in** by tools like
  KeePassXC (ADD / REMOVE over the protocol): added keys live in memory
  only, are never written to the vault, and are swept on vault lock,
  toggle-off or exit, with lifetime and confirm constraints honored. A
  per-signature confirm prompt is available, and CONFIRM-constrained
  keys always prompt. Transports: a unix socket
  (`~/.oryxis/agent.sock`, 0600) and a Windows named pipe
  (`\\.\pipe\oryxis-ssh-agent`) with a per-user DACL; an opt-in setting
  additionally serves the standard `\\.\pipe\openssh-ssh-agent` name so
  tools with a hardcoded target need zero config. Configured in its own
  Settings section.
- **Certificate authentication.** Oryxis offers OpenSSH user
  certificates during publickey auth and adds a certificate-only auth
  method that never silently falls back to the bare key. The keychain
  gains a certificate editor and viewer, a "Certificate" badge on
  keys that carry one, and an optional principal / host hint. Rides
  sync and portable export like any key.
- **FIDO2 security keys and PKCS#11 (via the agent).** Import
  `sk-ssh-ed25519` and `sk-ecdsa-sk` security-key public keys and
  smartcard / PKCS#11 identities exposed by your system agent; they
  carry a "Security key" badge and can be pinned to a specific agent
  identity and host. The touch / PIN is handled by the platform's own
  agent, so hardware-backed keys authenticate without leaving the
  private material anywhere Oryxis can see it.
- **In-app SSH key generation.** Generate Ed25519, RSA (2048 / 3072 /
  4096) or ECDSA (P-256 / P-384 / P-521) keys straight from the
  keychain, with an optional passphrase, alongside the existing import
  flow. The passphrase also rides the portable export.
- **Argon2id key derivation auto-tuning.** At vault creation (and on a
  master-password change) the KDF calibrates to your machine, targeting
  roughly one second to unlock, and persists the chosen parameters next
  to the salt. Existing vaults keep their parameters until a rotation
  re-derives; the sync secret and portable-export KDFs are deliberately
  frozen so cross-device and cross-machine unlock never change.
- **Scrollback search** (Ctrl+F). Find-in-buffer for the terminal:
  amber match highlight, an N / M counter, step forward / back, Esc to
  close. It yields the shortcut to full-screen apps (vim / less / htop
  keep their own page-forward binding) so it only ever grabs Ctrl+F on
  the normal screen.
- **Broadcast input across split panes.** Arm a tab (Ctrl+Shift+U, a
  status-bar indicator, or the tab menu) and every keystroke and paste
  fans out to all of its split panes at once, for driving a fleet from
  one prompt. Panes can be muted individually; a broadcast border marks
  an armed tab and stays inert on a lone pane; secret-bearing sends stay
  single-pane.
- **OSC 8 hyperlinks.** Terminal escape-sequence hyperlinks are now
  clickable, with a scheme allowlist (http / https / mailto / ftp;
  `javascript:` and `file:` are refused), a target-reveal chip so a
  link's real destination can't be spoofed by its label, and an
  underline that follows a link wrapped across rows.
- **Command palette** (Ctrl+Shift+P). A fuzzy action search over every
  hotkey action and every Settings section: type, arrow, Enter.
- **Legacy keyboard modes and per-host toggles.** For the
  network-appliance audience, per-host quirks: backspace (^H vs ^?),
  rxvt Home / End, function-key styles, and toggles to disable mouse
  reporting, remote resize, title changes and OSC 52 clipboard access,
  plus an SSH rekey-limit field. All in an "Advanced terminal" section
  of the host editor; defaults are byte-identical to the old behavior.
- **GIF export of session recordings** (#71). Render any `.cast`
  recording into a shareable GIF from the History screen, via an
  optional `agg` plugin downloaded on first use through the same signed
  distribution pipeline as the MCP server (the encoder and its fonts
  stay out of the core binary). Colors come from the theme embedded in
  the recording, so no extra flags.
- **In-app session player** (#71). Play a recorded session back inside
  the History view on the real terminal engine (no second emulator):
  play / pause (Space), restart, seek and speed control, with the
  timing captured in the recording. Read-only by construction, there is
  no path for a replay to type anything, and it stops and sweeps
  recordings on vault lock. The viewer actions (View log, export menu)
  are mirrored in the player header.
- **Privacy Mode v2** (#78). Masking graduates into a system: labels
  redact everywhere they render (host cards, tab strip and drag ghosts,
  the tab-jump modal, the Ctrl+K picker, the status bar, tray menu and
  Windows JumpList), a session-scoped override (Ctrl+Shift+M, with a
  chip) reveals for the current session only, vault-derived terms and
  explicit always / never mask lists (multi-line editors in Settings)
  drive what is masked, per-class gates turn each masking category on or
  off, and the visual is a span-level eye-slash with a one-time hint the
  first time something masks.
- **Rebindable terminal clipboard hotkeys** (#75). The terminal's copy,
  paste, select-all and scrollback chords join the rebindable-hotkey
  family, scoped to the focused pane.
- **SFTP archive operations.** Compress and extract zip and tar.gz from
  the SFTP context menu: the remote side runs `tar` / `unzip` / `zip`
  on an exec channel multiplexed over the live connection (POSIX and
  Windows shells, with safe quoting), the local side uses native Rust
  codecs, and a zip's contents can be browsed in place over ranged
  reads without a full download.
- **International keyboard handling** (#80). Composed input from AltGr
  (Windows / Linux) and Option (macOS) now produces the character the
  layout intends instead of being eaten as a Meta chord, fixing dead
  keys and symbols on bépo, German and other non-US layouts. See the
  Changed section for the macOS Option default.
- **Redesigned connection screen.** Connecting now shows a living
  vertical timeline: one animated disc per step (resolve, dial, proxy,
  jump, auth, channel), a centered title, and Termius-style logs, with
  a clean single error line on failure instead of a dumped stack.
- **China download mirror.** A Settings > Advanced option routes every
  GitHub-bound download (CJK fonts, plugin manifests and binaries,
  update checks and installers) through a mirror for networks where
  GitHub is slow or blocked. Auto (the default) tries GitHub first and
  falls back per-request to the project asset host; a custom prefix
  proxy is also supported. Mirrors are untrusted by design: fonts are
  sha256-pinned, plugins and updates sha256 + Ed25519-pinned, so a
  hostile mirror can only withhold or replay metadata, never run
  unsigned code.
- **Self-hosted relay setup wizard.** Settings > Sync gains a wizard
  that generates the compose / systemd / Caddy files for your own sync
  relay and adopts the endpoint after a health probe, plus a persistent
  P2P health readout.
- **MCP: opt-in vault password in the Claude Code config** (#72). When
  installing the MCP server config for Claude Code, an opt-in setting
  embeds the vault password so the server can unlock unattended;
  protected on disk.
- **SFTP keyboard and navigation.** The row context menu opens with the
  Menu key and is fully keyboard-navigable (#52), and type-ahead does
  Windows-style cycling (repeat a key to advance through matches, firing
  immediately).
- **Rebuilt first-run empty state**, an **Auto (OS)** language option
  that follows the operating system's language by default, a
  **restructured README** landing page with SECURITY / CONTRIBUTING and
  a `docs/` set, a **Chinese landing page** and FAQ, and **five more
  translated READMEs** (Traditional Chinese, Japanese, Korean, Persian,
  Portuguese).

### Changed
- **Sync protocol bumped to v7 (re-pair required).** The certificate
  and security-key work added enum variants a v6 peer cannot
  deserialize, which serde rejects hard (unlike unknown fields). The
  bump turns that into the same loud, non-destructive version reject as
  every prior break: **both devices must run 0.10.0 to sync, and paired
  devices need to re-pair.** The SFTP-snapshot format moves in lockstep
  but asymmetrically, a v7 device still reads old snapshots (so existing
  snapshot blobs keep working) and writes the new format; nothing is
  lost either way.
- **macOS: the Option key now composes characters by default.** Option
  produces the accented / special character the layout intends
  (`Option+n` then `n` gives `ñ`) instead of acting as Meta. If you
  relied on Option as Meta, a per-host "Option as Meta" quirk in the
  Advanced terminal editor restores it (four modes: both, left only,
  right only, off).
- **Version-shaped numbers are masked like addresses under Privacy
  Mode.** A dotted quad such as `1.2.3.4` in a version string is now
  redacted with the same rules as an IP, closing a leak where a
  build / version banner could carry an address past the mask.
- **The hosted sync relay is no longer a default.** Fresh installs are
  LAN-only until you pick an internet backend (an SFTP snapshot or your
  own relay); existing users who were actually syncing through the old
  hosted URL are grandfathered onto it automatically at boot.
- **ZMODEM transfers stream** instead of stop-and-wait windows (much
  faster on high-latency links), with download resume, multi-file
  upload and `sz -e` support, and per-tab progress.
- **Settings reorganized.** Cards are grouped by theme rather than one
  box per row, the SSH-agent configuration moved into its own section,
  Top Bar and Tabs split into separate sections, and a setting sources
  the host-coloured tab text from the host or the app (#79).

### Fixed
- **Closing the window no longer quits the app** on Windows when
  close-to-tray is enabled (#74): every close verb (the X, Alt+F4, the
  taskbar close) routes through the close-to-tray handler, the tray icon
  appears when the primary window hides itself, and the OS minimize
  verbs honour minimize-to-tray. Also sweeps the stray background
  processes that could linger after exit.
- **`sz` returns to the shell after a successful send** (#77): the final
  ZMODEM handshake is flushed so `sz` / `rz` exit promptly, and a shell
  prompt coalesced right behind the transfer's "OO" sign-off is no
  longer swallowed.
- **The hybrid Files surface remounts after a terminal reconnect**
  (#63): a dropped-then-restored SSH session no longer leaves the tab's
  SFTP surface pointing at the dead channel (the "session closed"
  retry loop), local directory listing is async (no cold-path UI
  freeze), OS drag-and-drop drops land reliably, and out-of-range file
  mtimes no longer break a listing.
- **Host-coloured tab text is contrast-validated** (#79): the accent
  used as tab text is checked for 4.5:1 contrast against the tab
  background and falls back when it would be unreadable.
- **Privacy Mode redaction gaps closed** (#78): an IP embedded in a URL
  host, an IP after `@`, the OSC 9 notification body, tray and JumpList
  labels, IPv6 local-range classification and highlight spans remapped
  from byte offsets to columns.
- The command palette and its sibling pickers close on vault lock, the
  find-bar stays top-right, and a broadcast border only shows on split
  tabs.

### Security
- **Archive extraction hardened:** setuid / setgid bits are dropped on
  extract, two remote / local archive path-injection holes are closed,
  and a symlink archive member is refused.
- **SSH agent hardening:** the per-signature key oracle is darkened for
  connections that outlive the runtime, an add-during-lock race is
  tightened, and the signed PEM is zeroized after use.
- **ZMODEM hardening:** a received file name with a Windows drive letter
  or alternate data stream is neutralized, an unannounced download is
  capped, a download is held to its announced size, and a peer CAN
  cancel is detected.
- **MCP** protects the embedded vault password on disk, **update and
  plugin** code confines remote-derived file names to the temp / cache
  directory, and font downloads are HTTPS-only with atomic + fsync
  installs.

### Internal
- **The `Message` enum was split into ~30 per-domain sub-enums** and the
  central dispatcher rebuilt into a fully exhaustive, catch-all-free
  match with roughly seventy type-safe per-variant handlers, so a
  message that isn't routed is now a compile error.
- **Large files were split** into sibling modules (the terminal widget,
  the SSH / SFTP / terminal / settings / keys dispatchers, the keys
  views) to keep each file navigable.
- **A headless end-to-end test harness** (dev-only, behind a cargo
  feature) drives the real app with no window and renders screenshots,
  for reproducible UI QA and committed `.ice` flows.

## [0.9.0] - 2026-07-09

### Added
- **Hybrid SSH/SFTP tab + Files sidebar** (#61). Every SSH tab can
  browse files without leaving the terminal, multiplexed over the
  same live connection (no second login, jump chains and proxies
  included). A new **Files** tab in the terminal sidebar is a
  per-pane browser that follows the shell's working directory as you
  `cd` (via shell-integration cwd reporting, with a window-title
  fallback for stock bash; manual navigation unpins, one click
  re-follows) and carries the full operation set: upload, download,
  rename, delete, new file / folder, and Copy path in every context
  menu. From there, "Open SFTP session" (or the expand action)
  promotes the tab itself into the dual-pane SFTP manager at that
  directory; the tab then owns both surfaces, and a chip on the tab,
  a status-bar segment, the tab context menu or Ctrl+Shift+F flip
  between Terminal and Files while the PTY keeps running underneath.
  The SFTP surface can also detach into its own tab (blocked
  mid-transfer, never silently). An opt-in "Force OSC 7" setting
  injects cwd reporting into bash and zsh sessions that lack shell
  integration, hiding its own echo. Standalone SFTP tabs are
  unchanged and remain the server-to-server surface.
- **Google Cloud provider (Compute Engine + GKE).** Cloud Accounts
  gains GCP alongside AWS and Kubernetes, as an on-demand subprocess
  plugin driving the `gcloud` CLI you already authenticate with.
  Compute Engine instances are discovered across zones and import as
  dynamic groups that resolve live on expand; GKE clusters are
  discovered too, and adding one runs `get-credentials` and creates a
  Kubernetes account pointed at the resulting context, reusing the
  entire kubectl pipeline. Discovery is best-effort per service, so
  an API you never enabled doesn't sink the rest.
- **Azure provider (VMs + AKS).** The same shape over the `az` CLI:
  Virtual Machines import as dynamic groups, AKS clusters add as
  one-click Kubernetes accounts. Cloud Accounts now spans AWS,
  Google Cloud, Azure and bare Kubernetes, every one an on-demand
  plugin that stays out of the binary until you use it.
- **Biometric app unlock.** Unlock the vault with Windows Hello,
  Touch ID, or the Linux system keyring instead of typing the master
  password. Opt-in: offered when you set a master password (and in
  onboarding), toggleable in Settings. The lock screen leads with the
  biometric prompt, labelled with the platform's own name, and the
  master password stays one click away as the fallback; a failed or
  cancelled prompt falls back cleanly. Locking still zeroizes the
  key. On Linux, where the keyring hands the secret back without a
  presence check, the UI says so honestly instead of pretending.
- **Windows JumpList.** Right-click the taskbar icon for your recent
  hosts: picking one connects in the running window (routed through
  the single-instance IPC) or launches the app connecting. The list
  updates as you connect.
- **Performance HUD** (#69). An opt-in terminal overlay that tells
  the truth about rendering: frame time against the 16.7 ms budget
  (sparkline auto-scaled to the window peak), busy and slow-frame
  percentages with severity tinting, and a network line with
  round-trip time and jitter measured over the live SSH connection.
- **Typed-command capture in session recordings.** Recordings store
  the commands typed at a prompt as their own chunk kind, captured by
  the same pipeline as the per-host command history (quick-connect,
  local and cloud panes included) and passed through the same
  secret-redaction before touching the vault. A new "Export typed
  commands (.txt)" action on a session lists them with timestamps;
  the `.cast` and transcript exports stay output-only.
- **Creation hotkeys.** New host, new SSH key and new identity get
  their own rebindable chords, listed in the burger menu.
- **Six new languages.** Hebrew (right-to-left, joining Persian and
  Arabic), Traditional Chinese (Taiwan vocabulary, not a script
  conversion), Thai, Hindi, Czech and Greek bring the UI to 23
  languages, with the Hebrew / Thai / Devanagari fonts bundled and
  Traditional Chinese as an on-demand font download like the other
  CJK faces.
- **Reset scrollback to the live edge (PuTTY's two behaviors).** Two
  Settings > Terminal toggles (both off by default) bring you back to
  the bottom of the buffer without reaching for the wheel or scrollbar:
  **on keypress** (any key sent to the terminal jumps to the bottom)
  and **on display activity** (new terminal output jumps to the
  bottom). Independent, so you can run either, both, or neither.
- **Configurable right-click in the terminal (PuTTY's three schemes).**
  A Settings > Terminal picker chooses what right-click does: **Paste**
  (the default, unchanged, honouring the copy-on-select "copy on
  right-click" sub-option); **Context menu** (Copy All / Paste / Clear
  Scrollback, anchored at the click and keyboard-navigable); or
  **Extend selection** (xterm-style, moving the selection's nearer
  boundary to the click and copying). The right-click gesture now has a
  single authority; the "copy on right-click" sub-option is shown only
  under Paste. Completes the PuTTY parity pack.
- **Edit an ad-hoc host mid-connect.** The connect progress screen for
  a quick connect (`user@host` without saving) now offers "Edit host"
  in every state, not only after a failure. It edits the temporary
  host in place: the editor opens with Connect (without saving) as the
  primary action and Save demoted to the explicit "persist to vault"
  opt-in, so fixing a typo'd user or port and re-dialing never writes
  to the vault by surprise. Any in-flight prompt or dial is cancelled
  cleanly first.
- **PuTTY parity pack.** The small things every PuTTY hand expects.
  TCP_NODELAY is now set on every socket the app opens (SSH session,
  proxy dial, and the local ends of `-L` / `-R` / SOCKS forwards), so
  interactive traffic stops paying Nagle latency. A per-host IP
  version preference (Auto / IPv4 / IPv6, host editor > Network, and
  on the reduced Telnet form, PuTTY applies it to Telnet too) filters
  resolved addresses on the direct dial, the proxy dial and a jump
  chain's first hop (the bastion's own preference governs that one),
  failing honestly when a name has no address in the chosen family;
  `~/.ssh/config` import maps the `AddressFamily inet` / `inet6`
  directive onto it, and bare IPv6 literals typed as the host address
  now dial correctly (they are bracketed for the resolver). The SSH pre-authentication banner (legal notices, MFA
  instructions) is shown on the connect card and written to the
  terminal scrollback instead of silently dropped; Privacy Mode
  redacts it like the rest of the connect screen. And X11-style
  middle-click paste in the terminal (own toggle in Settings >
  Terminal, on by default), riding the same careful-paste and
  paste-guard checks as every other paste path.
- **Smart tabs.** Background tabs now tell you when they need you,
  built on the same OSC 133 shell-integration marks as the command
  history capture. A command that ran past a configurable threshold
  (Settings > Terminal, default 10 s) and finished while you were
  looking elsewhere earns an attention dot on the tab's badge (green
  for success, red for a nonzero exit code) plus a notification with
  the command line and its duration, delivered through the existing
  notification policy (in-app toast while the window is focused,
  native OS notification while it is not). Hosts without shell
  integration are covered by a quiet-period heuristic: output arriving
  after 30 s of silence on an unwatched tab marks it with an activity
  dot (the `tail -f` / long-build resuming case), notifying once per
  silence instead of per line. Viewing the tab clears its dot; sitting
  in the Dashboard or Settings counts as not watching, so the terminal
  keeps collecting attention behind those views. Under Privacy Mode
  the notification drops the command line and the host identity (the
  OS notification center persists plaintext, and command arguments can
  carry secrets). The whole feature is a Settings > Terminal toggle
  (on by default).
- **Remote desktop (RDP / VNC), optionally through an SSH gateway.** A
  first-class remote-desktop host: its address/port are the desktop
  endpoint and its username/password the desktop login. It reaches the
  machine directly or tunnels through an SSH host you pick as a gateway
  (an ephemeral `-L` forward), then launches the OS-native client (mstsc
  / Microsoft Remote Desktop / FreeRDP / Remmina / a VNC viewer).
  Clicking the card opens the desktop; no separate menu. The gateway
  tunnel is managed (Stop from the card menu; cleared on vault lock),
  independent of the client process so it survives clients that return
  immediately, and it self-closes once the desktop client disconnects
  and goes idle (uniform across blocking viewers and handoff launchers).
  A first-time gateway prompts for host-key verification like any
  connect. Created via "Add remote desktop" in the + Host menu. Opt-in:
  off by default (Settings > Advanced), so the feature stays hidden until
  enabled. If no client is installed, a message names what to get.
- **ZMODEM file transfer in the terminal.** Run `sz file` or `rz` on
  the remote and Oryxis auto-detects the transfer, takes over the byte
  stream, and moves the file: downloads land in a configurable folder
  (Settings > Terminal, default the OS Downloads dir), uploads prompt
  for the local file to send. A progress overlay shows direction, name
  and bytes with a Cancel button. Built on a native Rust engine
  (`oryxis-zmodem` over `zmodem2`) bundled in the core binary; works
  over SSH, Telnet and serial. A disconnect mid-transfer resumes the
  terminal cleanly rather than freezing it.
- **Serial port protocol.** The per-host protocol selector adds Serial
  (alongside SSH and Telnet). Serial hosts open a local COM /
  `/dev/tty*` line over a new native engine (`oryxis-serial`, built on
  `tokio-serial`) with configurable baud, data bits, parity, stop
  bits, flow control, a line-ending choice (CR / LF / CR LF for Enter)
  and an optional local-echo toggle (raw serial has no ECHO
  negotiation, so a non-echoing device shows nothing typed until it is
  on). The editor swaps to a further-reduced form (port path + line
  parameters, no auth, no numeric port). Serial hosts ride sync and
  portable export like any host.
- **Telnet protocol.** A per-host protocol selector (SSH / Telnet) in
  the host editor. Telnet hosts connect over a new native Rust engine
  (`oryxis-telnet`): RFC 854/855 option negotiation with the full RFC
  1143 loop-proof state machine, RFC 1073 NAWS window size, RFC 1091
  TERMINAL-TYPE, RFC 1572 NEW-ENVIRON, per-host charset transcoding,
  and prompt-driven credential autofill (once per session, time-boxed,
  never a retry loop). The editor swaps to a reduced form for Telnet
  (host / port / username / password / encoding / terminal theme, plus
  an honest cleartext-credentials note) and hides every SSH-only field.
  Telnet hosts ride sync and portable export like any host; SFTP and OS
  detection stay SSH-only. Password sent in cleartext by design, the
  protocol has no secure option.
- **Ad-hoc quick connect.** Type `user@host` in the new-tab picker
  (Ctrl+K), the toolbar search or the tab-jump (Ctrl+J) and connect
  without saving a host; the host editor gains a "Connect without
  saving" action. If the first auth attempt fails, the prompt and the
  failure screen offer switching to any saved identity or key and
  reconnect in place.
- **Per-host command history.** Commands executed on saved hosts are
  captured into the vault, encrypted at rest and run through a
  secret-redaction pass before storage (shell-integration OSC 133
  marks with a raw-input heuristic fallback; prompts in password
  state are never recorded, and a leading space skips capture) and
  surfaced in a new
  History tab in the terminal sidebar: a most-frequent shortlist over
  a recent list, with search, run, paste and delete (confirmed).
  Local-only by design: never synced, never portable-exported, wiped
  with the host. Toggleable in Settings -> Terminal. Two explicit
  plain-text escape hatches for offline reference and support
  sharing: an Export button on the History tab writes the host's
  commands to a `.txt` of your choosing, and an optional setting
  live-appends every captured command to a per-host log file under a
  configurable folder (default `~/.oryxis/command-history/`).
- **Snippet groups, snippet tags, host tags, and tag filters.**
  Snippets can carry a free-form group and comma-separated tags; both
  editors (vault panel and terminal sidebar) expose the fields, the
  lists render grouped sections (ungrouped first, then each group),
  cards show the tags, and search matches label, command, tags and
  group. Hosts gained a Tags field in the editor (the model always
  had them, now they are editable), the dashboard search matches
  them, and a new tag-filter dropdown next to the sort button narrows
  the host grid to one tag. In the terminal sidebar's Snippets tab a
  toggle shows only snippets sharing a tag with the focused host, so
  a `db`-tagged host surfaces the `db` runbook. Local terminals carry
  tags too (Settings > Local terminals), so a local pane surfaces its
  own runbook the same way. Snippet groups render as dashboard-style
  folder cards (click or Enter to drill in, breadcrumb back), the
  Snippets toolbar has its own multi-select tag filter, and both tag
  dropdowns are multi-select with a selection count on the toolbar
  button; the host filter narrows the Groups section by subtree.
  Groups and tags ride sync and portable export as plain snippet data.
- **Paste guard content heuristics.** The careful-paste confirmation
  now also fires on a SINGLE-line paste when its content looks
  dangerous, with one explicit warning line per class: invisible or
  bidirectional characters, raw terminal control sequences, `curl |
  sh` style fetch-and-execute one-liners (detected even when hidden
  behind invisible characters), and words mixing look-alike letters
  from different alphabets. Its own toggle in Settings > Terminal,
  independent of the multi-line careful-paste switch.
- **Per-snippet shortcuts.** A snippet can carry its own key combo
  (recorded right in the editor, with conflict and shell-key
  protection): with a terminal focused, the chord runs the snippet,
  variables prompt included. The shortcut is stored on the snippet
  itself, so deleting the snippet removes it with no leftovers.
- **Snippet variables.** `{name}` and `{name:default}` placeholders
  in a snippet body prompt for values in a small dialog before the
  send (run and paste alike), with defaults pre-filled and the first
  field focused. The matcher is deliberately narrow so shell text
  never trips it: `${VAR}`, `{}` and `{print $1}` pass through
  untouched. No major competitor ships this.
- **Fixed: hotkey chords no longer type stray characters.** On some
  platforms a chorded key event still carries its base character
  (Ctrl+Shift+3 arrives with `#`), and focused text fields inserted
  it, so the new section-jump chords sprayed `#$!@` into search
  boxes. Text inputs and editors now ignore event text while Ctrl
  (without AltGr) or the logo key is held.
- **Fixed: floating menus die with their surface.** An overlay menu
  left open across a navigation or a tab switch (easy with the
  stay-open tag filters) kept the modal keyboard router alive and
  invisibly swallowed Enter and the arrows, reading as "the terminal
  stopped accepting commands". Navigating or activating a tab now
  closes any floating menu.
- **Session recording export (asciinema-compatible).** The encrypted
  session logs the vault already keeps now record real timing (chunk
  offsets stamped at capture, per-line replay steps, terminal resizes)
  and any session exports from the History screen as a standard
  asciicast v3 `.cast` file with the effective terminal theme
  embedded (the asciinema player and GIF renderers reproduce your
  colors with no extra flags), or as a plain-text transcript (ANSI
  resolved by the same renderer the in-app viewer uses). Recording
  detail and on-disk compression are configurable in Settings.
  Output-only by design: keystrokes are never recorded, so the
  input-leak class doesn't exist. Sessions recorded
  before this release still export, replayed with a small fixed delta
  instead of real pacing. Note: exports carry the raw recording;
  Privacy Mode masking is display-only.
- **TOTP 2FA in the vault.** Store a per-host TOTP secret (bare base32
  or a full `otpauth://` URI) encrypted like every other credential;
  keyboard-interactive verification-code prompts are answered
  automatically, once per auth attempt, with a manual fallback if the
  server rejects the code.
- **Vault auto-lock.** An optional idle timer soft-locks the vault:
  the master key is zeroized and the lock screen shown, while live SSH
  sessions and tabs survive and are back after unlock. The manual Lock
  Vault button remains a full teardown. The lock screen auto-focuses
  its password field on every arrival (boot, manual lock, idle lock).
- **Unified keyboard navigation** (#52). Focus zones across the vault
  area (Tab cycles search / toolbar / content / sub-nav, arrows move,
  Enter activates), the modals / menus / Settings / side panels
  (per-frame recorded rows; selects are Tab-focusable with real
  keyboard handling in the pick_list widget), and the entire terminal
  sidebar: Ctrl+Shift+H opens it and cycles Chat / Snippets / History /
  Host config, Tab walks every control (header buttons, searches,
  selects, toggles, theme cards, chat mode chips), Enter runs the
  selected snippet or history command, Shift+Enter pastes it without
  the newline, Delete removes it, Ctrl+F opens the tab's search,
  Ctrl+Shift+B toggles the sidebar and Esc hands the keyboard back to
  the terminal. Entering History focuses its search field.
- **Vault section shortcuts.** Ctrl+Shift+1..8 (Cmd+Shift on macOS)
  jump straight to Hosts, Keychain, Snippets, Port Forwarding, Logs,
  Cloud Accounts, Proxies or Known Hosts from anywhere, including a
  terminal tab; the burger menu shows every hint, with Ctrl+1 still
  opening the vault area itself. Rebindable like the tab-slot family.
- **Careful paste.** Multi-line pastes show a confirmation with a
  line-count preview so a hidden trailing newline can't auto-execute;
  bracketed paste honours the remote app's mode; opt-out available.
- **Tab rename and bottom tab bar.** Transient per-tab rename (not
  persisted to the host) and an opt-in setting to dock the tab strip
  at the bottom of the window.
- **Ctrl+Tab tab switching by recency.** A single press toggles the two
  most recent tabs; holding Ctrl and pressing Tab again walks deeper
  through the most-recently-used stack, like the OS Alt+Tab.
- **Never-stored password auth + legacy RSA.** A new "Password prompt"
  auth method asks for the password at every connection and never
  writes it to the vault; RSA keys negotiate rsa-sha2-512/256 with a
  SHA-1 fallback so old servers keep working.
- **Window geometry persistence.** Size, position (and monitor) and
  the maximized / fullscreen state are remembered across launches.
- **Debug logging + environment info.** Settings -> Advanced gains an
  opt-in debug log written to `~/.oryxis/oryxis-debug.log` and a
  "Copy environment info" button for bug reports.
- **Renderer auto-probe with software fallback.** At boot the GPU
  stack is probed; when the advertised backend is actually a software
  rasterizer (the WSL / llvmpipe class that misrenders), the app drops
  to the built-in software renderer instead. The explicit OpenGL
  choice does the same check, the Settings labels now describe the
  real backend ladder, and changing the renderer offers an in-app
  restart to apply.
- **Performance mode** plus terminal render polish: terminal geometry
  cache, event-driven tray updates, and cursor-move forwarding gated
  behind an interest flag so idle mouse movement stops burning CPU.

### Changed
- **Sync wire format: XChaCha20-Poly1305 on protocol v6.** The P2P
  sync payload cipher moves from ChaCha20-Poly1305 to XChaCha20 with
  192-bit random nonces, on a protocol bump (v5 to v6) and a snapshot
  format bump. Both peers must run 0.9; an older peer or snapshot is
  rejected cleanly with a clear message, nothing is overwritten or
  lost.
- The update dialog identifies nightly builds and labels the download
  button with the exact artifact it picked for your platform.
- The new-tab picker focuses its search box on open.
- **AI chat harness.** Replies and tool calls now route to the tab
  that asked (a stream could previously land commands on another
  host's tab), each tab carries a Plan / Ask / Auto mode, a floating
  Stop button aborts a runaway tool loop, privacy-mode redaction is
  applied to captured terminal context before it reaches the model,
  and provider requests carry timeouts and retry handling.
- **Privacy mode expanded.** Terminal masking now covers IPv6
  addresses, `user@host` pairs, home-directory usernames and vault
  hostnames; the mask renders in neutral grey; the connection-progress
  panel no longer leaks the address and gains an in-place reveal eye.
- **Split-pane shortcuts are rebindable** like every other binding,
  and Ctrl+Shift+E was freed up for SFTP.
- The lock screen adopts the onboarding's accent-gradient look.

### Fixed
- SFTP dialogs no longer render, still functional, on top of the lock
  screen; the idle auto-lock also sweeps every secret-bearing piece
  of UI state (editor password fields, pending host-key prompts, open
  SFTP dialogs).
- The host-key verification prompt is a real modal: keystrokes,
  Enter included, no longer leak into the terminal underneath while
  it is open, and Esc rejects.
- App hotkeys no longer fire behind a blocking modal (closing a pane
  behind a 2FA prompt, for example).
- Toast notifications auto-dismiss on a timer everywhere and can be
  clicked away; several code paths could previously strand one on
  screen indefinitely.
- Inline renames in SFTP that don't change the name no longer send a
  rename to the server (clicking away from the field could touch
  every visited file's mtime), and Ctrl+A selects all in SFTP text
  fields (#63).
- The keychain screen shows identities again when the vault has no
  SSH keys (#70).
- Tag-filter dropdowns anchor under their toolbar button instead of
  wherever the mouse happened to be, keyboard activation included.
- Ctrl+Tab (most-recently-used tab cycling) no longer walks into
  dormant pinned tabs restored at boot, which silently reconnected
  their hosts just by cycling past them. MRU cycling covers open tabs
  only; dormant pins still open deliberately via click, Alt+arrow or
  Ctrl+1..9.
- Toast notifications now show on every view: the chip moved from the
  terminal area to the window root, so OSC 9 / smart-tab / copy
  feedback raised while you sit in the Dashboard or Settings is no
  longer silently dropped. The lock screen still suppresses it so a
  background session's notification can't leak onto a locked UI.
- Pasted text with CRLF line endings no longer doubles newlines; all
  line endings are normalized to CR on paste (#60).
- Full-screen and raw-mode prompts over SSH no longer freeze: in-band
  terminal queries (device attributes, cursor position) are answered
  (#48).
- A renamed ECS dynamic group could vanish yet still block its own
  re-import.
- A `pick_list` dropdown unmounted while open (section switch, tab
  switch, sidebar close) could permanently swallow Enter / Space /
  Esc / arrows app-wide.
- Focus rings no longer accumulate across the host editor's inputs,
  and context-menu / picker rows click reliably again.

## [0.8.3] - 2026-06-30

### Added
- **Vertical navigation rail.** Settings -> Interface now has a
  **Navigation** orientation (Horizontal pills, the default, or
  Vertical rail). The vertical rail is an icon column on the leading
  edge of the vault content with the section icons + a pinned Settings
  gear; it scrolls (thin, hover-revealed scrollbar) and toggles between
  icon-only (with hover tooltips) and a wider labelled form.
- **Keyboard navigation on the dashboard.** From the host search, Tab /
  arrow keys move a selection across the cards: groups first, then
  hosts. In grid mode the up/down arrows move by row and left/right by
  column; in list mode every direction moves by record. Movement is
  cyclic (last wraps to first). Enter opens the selected group or
  connects the selected host (or the top result while searching), and
  Escape clears. The search auto-focuses when Home opens, the selection
  blurs the input and clears on any click, and the selected card
  scrolls into view.
- **Host-group icon / colour editor.** Folders are now editable from a
  sidebar panel (name + icon/colour via the shared picker), not just a
  rename dialog; group cards reflect the chosen colour.
- **Search on Cloud Accounts and Proxies**, matching the other vault
  screens (hidden when the list is empty).
- **Per-card accent wash.** Each dashboard card carries a soft wash in
  its own colour (toned toward the surface), behind a new "Accent glass
  cards" toggle. The top bar carries a matching gradient accent wash
  behind its own "Wash top bar" toggle, separate from the underline.
- **Shared empty-state pattern** (icon + title + description + call to
  action) extended to Proxies, Known Hosts and History.
- **Connection settings section.** Keepalive interval, auto-reconnect
  and OS detection moved out of the Terminal section into their own
  Connection section.
- **Font + theme preview.** The Terminal font picker now renders a live
  sample (sentence, a coloured prompt and a row of Nerd Font glyphs) in
  the selected font, size and terminal palette, so you can confirm the
  font exists and preview the theme at a glance.
- **"Show host address" toggle** (Settings -> Interface -> Dashboard,
  off by default). When off, host cards show only the auth method;
  when on they show `user@host` (port 22 is always omitted).
- **Settings group sub-headers.** The larger Interface and Terminal
  sections are split into labelled groups (General / Dashboard / Tabs &
  top bar / App theme / Advanced; Behavior / Appearance).
- **Provider brand logos** on plugin cards (AWS, Kubernetes) instead of
  a generic package icon; descriptions under each Plugins feature
  toggle explaining what it does.
- **Startup command from a snippet.** The host editor's initial command
  is now a picker: None, any saved snippet (seeds the command from its
  body), or Custom command (the free-text editor). The choice is
  recovered on reopen by matching the stored command against snippets.
- **Master-password confirmation field.** Setting a vault master
  password now asks for it twice and rejects a mismatch, so a typo in a
  hidden field can't silently lock you out (recoverable only by
  destroying the vault).
- **Remove-password button.** The Security section gained an explicit
  "Remove password" button instead of relying only on the non-obvious
  toggle-off gesture.
- **Active renderer readout.** Settings -> Interface shows the graphics
  backend and adapter the compositor actually selected (e.g. "Vulkan
  (NVIDIA GeForce RTX 3080)"), so "Automatic" is no longer opaque and a
  backend fallback is diagnosable. Backed by a new
  `system::graphics_information()` exposed in the iced fork.
- **OSC escape-sequence support in the terminal.** A resumable OSC
  scanner reads the same byte stream fed to the emulator and surfaces
  the sequences alacritty doesn't expose: **OSC 8** explicit hyperlinks
  (Ctrl+Click opens the real target even when the visible label isn't a
  URL), **OSC 7** working-directory reports (a new local shell opens in
  the focused pane's directory), **OSC 133** shell-integration marks
  (captured per pane), and **OSC 52** clipboard access behind a new
  "Clipboard access" setting (off / write-only / read-write, default
  write-only, so a remote may set but not read your local clipboard).
- **Desktop notifications from the terminal.** OSC 9 notifications
  surface through a new "Notifications" setting (off / in-app toast / OS
  notification, default OS) and fire only while the window is unfocused;
  OS mode uses the native toast / libnotify and falls back to an in-app
  toast. OSC 9;4 progress is drawn as a coloured border that grows
  clockwise around the tab (accent / amber / red by reported state).
- **Terminal conformance pass.** Application-cursor-keys mode (DECCKM),
  modified navigation keys in the xterm `CSI 1;<mod>` form (Ctrl / Shift
  / Alt + arrows / Home / End / PageUp / Del / F-keys), Shift+Tab
  back-tab and Alt-as-ESC (Meta), so `mc` / `vim` / `less` / readline /
  tmux key bindings work over SSH. Cell advance is measured per font, so
  Fira Code and other off-ratio fonts no longer overlap glyphs.
- **Tab title from the shell (OSC 0/2).** Off by default (the curated
  host label stays); a global toggle plus a per-host `auto-title`
  override shows the shell-reported title in the tab strip.
- **Per-host terminal type (TERM).** A host whose terminfo trips on the
  default can send `xterm` / `linux` / `vt100` / `screen-256color` etc.
  instead of `xterm-256color`; local shells keep the default.
- **Host config sidebar tab.** A cog tab beside Chat and Snippets edits
  the focused pane's settings live with the terminal in view (per-host
  theme, encoding, terminal type, auto-title), plus the global
  appearance controls. A local pane gets a session-only theme with
  one-click promotion to the global default.
- **Persistent local-terminal list.** Detected and manual local shells
  are cached and managed from Settings -> Terminal as cards (icon +
  colour via the shared picker; add / edit / remove / re-scan, and an
  "always open X" default that skips the picker). Machine-local: kept
  out of sync and portable export.
- **Terminal hints.** A "hold Shift to select" toast appears when a
  mouse-capturing application is running in the pane.
- **Per-host legacy algorithm overrides.** Reach old servers that only
  offer cbc / 3des / sha1 / dh-group1 by pinning ciphers, key exchange,
  MACs or host-key algorithms per host. Each category defaults to Auto
  (russh's safe set) and switches to a checklist in the host editor;
  rides sync + portable export.
- **Automatic legacy-algorithm fallback.** When a handshake fails with
  "no common algorithm", a dialog offers to reconnect with legacy
  algorithms (Cancel / Connect once / Always allow) instead of
  dead-ending. The expansion is secure-first, so a modern server still
  negotiates a strong cipher. Offered on every interactive connect
  (terminal, SFTP, port-forward, SFTP backup); MCP honors pins
  headlessly.
- **More key formats.** ECDSA **P-521** keys (generate / import / store)
  and OpenSSL traditional (legacy) **PEM** private keys with `DEK-Info`
  encryption (3DES / DES / AES-CBC), which were previously rejected.
  Backed by a migration onto the ssh-key 0.7 crypto stack russh already
  pulls.
- **Privacy Mode.** A global toggle plus a per-host override masks host
  / IP / user / port / proxy behind muted block glyphs in cards, the
  History view and the terminal pane, revealed on hover or an eye
  toggle. Detection runs in the terminal highlight pass, so it works
  with keyword tinting off.
- **SFTP sync transport.** A file-based alternative to P2P sync: a group
  reconciles against one sealed snapshot on a backup host (download, LWW
  merge into the vault, rebuild, atomic upload via
  `posix-rename@openssh.com`), with host / path / passphrase settings
  and an auto-cadence tick.
- **Vault export / import over SFTP.** The export dialog gains "To SFTP"
  and an "Import from SFTP" entry that write / read the encrypted blob
  to a saved host plus a remote path, reusing a live session or opening
  a fresh SFTP connection through the same host-key verification as the
  terminal mount. A plain-language "How sync works" panel was added to
  the Sync settings.
- **Granular settings export / import.** Vault export now covers
  application settings and lets you tick exactly which entity families
  to include (11 categories), with a content-aware selection on import
  that shows what the file holds before applying. Device-local and
  security keys are withheld from the file.
- **Selective SSH config import.** A preview modal lets you tick which
  parsed `Host` blocks to import (flagging label collisions) instead of
  importing all of `~/.ssh/config` at once. A unified host-export dialog
  with a per-folder checklist (plus an ungrouped toggle) replaces the
  per-host Share actions.
- **New connection defaults.** Settings -> Connection gains a "New
  connection defaults" card (agent forwarding, default port, keepalive
  and TERM) that pre-fills every new host form.
- **SFTP file-manager overhaul.** A resizable centre divider; show /
  hide, resize and reorder columns (new Type / Permissions / Owner, with
  Type reading the file's MIME type from an embedded ~1246-entry table);
  horizontal scroll with a sticky header; keyboard navigation (arrows /
  Enter / Tab, with ".." as a virtual first row); a resizable
  message-log panel; inline slow-click rename and double-click-to-fit
  columns; FileZilla-style context menus (New folder / New file /
  Refresh / Show hidden / Open in File Manager, with cross-platform
  reveal); an SFTP entry in the new-tab picker; reliable cross-pane
  drag-and-drop; and a numeric (octal) mode input in Properties.
- **Tab fill style.** A gradient (default) or flat accent-tint picker
  routed through one shared active-tab helper, with live previews under
  Settings -> Interface (a mock tab strip and host card that reuse the
  real render helpers, so they can't drift from the UI).
- **First-run onboarding carousel.** A full-page 5-slide welcome
  (welcome, encrypted vault, connect, sync, AI) ending in the
  master-password setup slide, replacing the dry vault-setup screen.
- **Confirm-before-remove dialogs** for every keychain delete (host,
  session group, snippet, key, identity), so a stray click can't
  silently drop an item.

### Changed
- **One layout, two nav orientations.** The Classic sidebar and the
  `Layout mode` (Classic / Workspace) setting are retired in favour of a
  single top-bar layout plus the Navigation orientation above; existing
  Classic users migrate to the vertical rail automatically.
- **Dashboard list mode** renders History-style rows: full-width
  independently-rounded cards with a small gap, applied uniformly to
  groups and hosts (replacing the connected divider list).
- **Side editor panels** (host, proxy, group, ...) rise full-height and
  cover the contextual sub-nav on their side instead of starting below
  it. The Proxies editor moved from an inline block to a right-hand
  sidebar panel.
- **Empty views** drop their toolbar search and "New" action; the empty
  state's button is the single create path.
- The **vault switcher** chip / badge is hidden while there is only one
  vault.
- **Plugin auto-update now actually runs.** The "Auto-update" toggles
  (global and per-plugin) were stored but never acted on. On launch the
  app now checks each installed, unpinned plugin whose auto-update is on
  and silently installs a newer compatible version (`min_app` /
  protocol gated), refreshing the MCP launcher copy and rebinding cloud
  providers like a manual update would. Pinned or auto-update-off
  plugins are left alone, and a failed check keeps the current version.
- **Features are managed from the Plugins screen.** AI Assistant, SFTP
  and Sync are enabled / disabled from a "Features" section on the
  Plugins screen (alongside the downloadable provider plugins), not from
  their own Settings sections. Each feature's Settings section appears in
  the sidebar only once it is enabled, and Cloud Sync appears only once a
  cloud provider plugin is installed.
- **MCP is managed as a plugin, not a feature toggle.** It's a real
  plugin binary, so it's activated / updated from the "Oryxis MCP Server"
  plugin card; its server on/off lives in the MCP settings section, which
  appears once the plugin is present (no longer a Features toggle).
- **Security section renamed "Security & Privacy"**; session logging,
  connection history and the retention window moved there from Terminal
  (recordings are scrubbed + sealed, so they belong with the vault). The
  Terminal section is now display-only.
- **Settings sidebar reorder.** Interface is the default landing
  section, followed by Terminal, Connection, Shortcuts, Security &
  Privacy and Plugins, then the enabled feature sections, then About.
- **Settings sections drop their redundant in-page title** (the sidebar
  already names the section) and use a consistent 24 px gutter on all
  four edges.
- The Plugins "Auto-update all" toggle now sits on the same line as the
  downloaded-plugins subtitle.
- **Plugins settings section renamed "Features & Plugins"** to reflect
  that it hosts both the feature toggles and the downloadable plugins.
- **Unified on/off toggle.** Every toggle (settings rows and the plugin
  auto-update controls) now renders the same switch; the plugin toggles
  dropped their one-off ON/OFF pill style.
- **Ctrl+Tab cycles positionally.** Ctrl+Tab / Ctrl+Shift+Tab walk the
  unified strip ([Home, pinned, tabs]) with wrap-around, and Alt+arrow
  cycling traverses that same order across terminal and SFTP tabs
  (it used to skip SFTP tabs and ignore pinning).
- **Responsive vault toolbar.** The search field collapses to an icon
  and secondary actions move into an overflow menu when the window is
  too narrow, with RTL-aware anchoring.
- **Windows release binaries are Authenticode code-signed** (via
  SignPath), so SmartScreen / UAC show a verified publisher.

### Fixed
- Cloud Accounts cards now show the accent border on hover, like the
  host and keychain cards.
- The Logs "Clear all" button is disabled when there is nothing to
  clear.
- The update dialog's download progress bar now fills proportionally
  instead of always showing full, and non-stable-channel users see a
  plain "Downloading ..." label instead of the installer-specific text.
- The Known Hosts empty state had a sentence-long title and a wrong
  "remove an entry" hint; it's now a short title with a description that
  explains where entries come from.
- The History view hides its toolbar (entry count, pagination, Clear
  all) when there's no activity, matching the other empty views.
- The empty-state icon box is now a fixed square (it tracked the glyph's
  own width/height before, so it came out oblong).
- Cloud Accounts search auto-focuses on entry, and cloud cards use the
  shared host avatar (filled brand colour) instead of a one-off box.
- The vault sub-nav "…" overflow no longer collapses a pill or two too
  early, and its dropdown menu now lands under the "…" instead of
  clipping off the right edge.
- Dev-build plugin cards drop the no-op "Check for updates" button and
  shorten the repeated "locally built" line.
- Side-panel editor headers (Host, Group, Session Group) align the title
  with the left gutter (the tall close button was pushing it down).
- The About section shows the app logo beside the name and tagline.
- The host editor's group-picker dropdown now anchors under the chevron
  when the form is scrolled (its anchor ignored the scroll offset before,
  so the popover opened too low).
- **Nightly self-update on Windows** no longer dies with "rename running
  exe: Access is denied (os error 5)". It hands off to a detached helper
  that waits for the app to exit, swaps the binary, and relaunches;
  staging uses a unique name (so a stale leftover can't block it) and
  elevates only when the install directory needs it, surfacing a clear
  message with the release link if it still can't replace the binary.
- The "Reset hints" button is disabled when no one-time hints have been
  dismissed (nothing to reset).
- The main menu (burger) now closes when opening SFTP from it; it used
  to linger over the new SFTP tab and host picker until an extra click.
- "Open in File Manager" now uses the OS-native name: **File Explorer**
  on Windows and **Finder** on macOS (other platforms keep the generic
  label).
- **Terminal scrollback survives reconnect.** Manual and silent 30 s
  auto-reconnect on a single-pane SSH tab now re-attach in place (a dim
  "[reconnecting...]" marker) instead of rebuilding the alacritty grid
  and wiping the screen plus scrollback the user was looking at.
- **A synchronized-update (DEC ?2026) timeout is now driven**, so an app
  that opens a sync update and then blocks on input (e.g. `docker
  compose`'s "(y/N)" prompt) no longer freezes the pane on the
  pre-update frame.
- **AI chat tool loops can't run away.** A safe read-only command already
  auto-run this turn is no longer re-run unconditionally (it is surfaced
  for explicit approval), and the stream is cancellable, so closing the
  sidebar or starting a new conversation actually stops it.
- **macOS clipboard shortcuts.** Cmd+C / Cmd+V now drive copy / paste
  (Cmd is reported as the `logo` modifier on macOS); Ctrl keeps its Unix
  meaning, and bare Cmd combos no longer leak a stray character into the
  PTY.
- **Right-click paste** is gated on the copy-on-select setting (which
  bundles select-to-copy with right-click-paste); it used to fire even
  when the setting was off.
- **Local-terminal tabs tint the top bar** by detected OS (PowerShell /
  cmd / WSL), matching the tab icon colour instead of falling back to
  the default accent.
- **Terminal selection no longer clears on a bare modifier press**
  (Ctrl / Shift / Alt / Super), so select-then-copy works when
  copy-on-select is off.
- **Wide-character spacers are skipped when copying a selection**, so
  CJK / emoji text copies without embedded gaps.
- **Pageant agent-pipe discovery** enumerates the named-pipe namespace
  and matches the live `pageant.<user>.<guid>` pipe instead of
  hard-coding the `pageant.conf` path, so it works wherever pageant
  wrote its config (e.g. Scoop installs).
- The Windows `MessageBeep` module path and feature gate were corrected.
- **MCP plugin unlocks v0.8.2-migrated vaults again.** The published
  `oryxis-mcp` binary (`mcp-v0.1.0`) predated the v0.8.2 vault sealing
  change and rejected the new field format with "Crypto error: Data too
  short" on unlock, leaving the standalone MCP server unusable on any
  password-protected vault opened by v0.8.2+. Rebuilt and republished as
  `mcp-v0.1.1` against the current vault crate (which reads both the
  legacy per-field and the new derived-key formats); the manifest's
  `min_app` is raised to 0.8.2 so only matching apps pick it up. With
  auto-update on (the default) the fixed binary installs on the next
  launch; otherwise pull it from Settings -> Plugins -> "Oryxis MCP
  Server" -> "Check for updates".

## [0.8.2] - 2026-06-12

### Performance
- **Vault operations no longer freeze the UI.** The master key is
  derived once at unlock instead of running a full Argon2id pass per
  encrypted field, making connects (especially through jump chains),
  AI chat sends, cloud refreshes and port-forward starts effectively
  instant on the crypto side. Existing vaults migrate automatically on
  the first unlock.
- **Smoother terminal under heavy output.** SSH/PTY output is coalesced
  into larger batches instead of one redraw per 8 KB chunk, and the
  renderer batches same-style glyph runs, skips blank cells and stops
  holding the terminal lock while building geometry.
- **Closing a tab now really closes the session.** Live SSH sessions,
  their background tasks and per-connection forward listeners are torn
  down on tab/pane close and on vault lock (they used to keep running
  invisibly).
- **Faster sync ticks**: manifest building reads lean id/timestamp
  rows, record collection loads each table once, applies run in a
  single transaction, and peers sync concurrently (one offline peer no
  longer stalls the others).
- **Faster AWS discovery**: regions, clusters and services are queried
  concurrently with one shared credential load, and task definitions
  are cached within a pass.
- Many per-frame allocations removed from the dashboard, history, SFTP
  and chat views; system font enumeration is cached; file dialogs no
  longer block the event loop; the updater streams its download with a
  live progress bar.

### Security
- **Session recordings now scrub secrets and PII before persisting.**
  Private key blocks, cloud/API token shapes (AWS, GitHub, Slack,
  OpenAI/Anthropic, JWT, Bearer/Basic credentials), `password=`-style
  assignments, credentials embedded in connection-string URLs,
  formatted CPF/CNPJ numbers, Luhn-valid payment card numbers and
  email addresses are masked as `[REDACTED]` when a recording buffer
  is flushed to the vault. Recordings are also sealed
  at rest with a dedicated content key wrapped by the master password.
- **Signed app updates.** Every release and nightly asset now ships a
  detached Ed25519 signature, and the auto-updater verifies it against
  the baked-in production key before launching an installer or
  swapping the nightly binary. Updater HTTP clients are HTTPS-only.
- **SFTP recursive downloads validate server-supplied names.** A
  hostile server can no longer steer files outside the chosen
  destination folder via crafted directory-entry names.
- **Destroy Vault now drops every table** (including ones added in
  recent releases) and VACUUMs the database file so wiped data doesn't
  linger.
- **Master password changes re-encrypt every secret.** Proxy passwords
  (inline and proxy-identity), cloud profile secrets and sync peer
  shared secrets were missing from the re-encryption pass, so changing
  the master password made them undecryptable. A structural test now
  pins every encrypted column.
- **Known hosts are tracked per host, port and key type.** Accepting a
  changed host key replaces the stale entry instead of stacking a
  duplicate row (which kept the warning coming back), and a server
  offering a different key algorithm prompts as an unknown key instead
  of a false "key changed" MITM warning.
- **Hardening:** cached cloud-provider plugins are re-verified against
  their install-time signature at spawn; the in-memory master password
  buffer is zeroized on lock/drop; proxy configurations redact the
  password from debug formatting.

### Added
- **Colorized session log viewer.** Recordings render with the terminal
  theme's palette (ANSI colors parsed, carriage-return redraws and
  escape sequences handled properly instead of leaking broken
  characters over a plain dump). Log rows are now clickable to open the
  recording (the View button is gone), the Delete action moved to the
  last column and asks for confirmation, and the timestamp sits where
  the buttons used to be.
- **Plugin uninstall confirmation.** Removing a plugin asks first, and
  removing the MCP plugin also deletes the stable launcher copy and
  flips the MCP Server toggle off. Dev builds offer "Remove downloaded
  files" when cached plugin downloads exist alongside the local binary.
- **Log retention setting.** Settings can auto-delete connection events
  and finished session recordings older than 1/3/7 days, 2 weeks, or
  1/3 months (default: never). Applied at boot and immediately when
  the option changes; in-progress recordings are never pruned.
- **One-time terminal link hint.** The "Ctrl + Click to open the link"
  hover hint retires itself permanently after the first successful
  ctrl-click, and is now localized (it was hardcoded English). A new
  **Reset hints** action in Settings → Interface brings every one-time
  tip back. (#38)
- **Reveal (eye) toggles on hidden fields.** The host editor's proxy
  password, the Share dialog, the AI API key, the master password and
  export/import passwords, and the sync signaling token can now be
  shown while typing, the same affordance the unlock screen already
  had. (#38)
- **Clickable vault statistics.** About → Vault Statistics gained a
  Logs count, and every stat row navigates to its section on click. (#38)
- **Cloud session end notice + reconnect.** When an ECS Exec / kubectl
  session's process exits (recycled task, idle timeout), the tab marks
  itself disconnected, prints a notice in the pane and reconnects when
  the tab is selected again; previously the pane just went silently
  dead. If the backing dynamic group no longer exists, an error dialog
  says so instead of failing silently.

### Changed
- **"Hosts" area tab is now "Vault"; "History" is now "Logs".** The
  top-strip tab covers the whole vault surface (hosts, keychain,
  snippets, port forwarding, logs), so it carries the vault name and a
  vault icon; the History pill/view was renamed Logs. The burger menu
  groups the vault surfaces under a "VAULT" section header. (#38)
- **[+] new-tab button sits next to the last tab** (browser-style).
  When the strip truly overflows (tabs at minimum width still don't
  fit) it docks at the strip's trailing edge so it never scrolls out of
  reach. (#38)
- **One visual language for the active tab.** Active nav tabs (Vault,
  SFTP) and the active compact pinned chip paint the same vertical
  gradient as session tabs; the pinned chip's accent outline was
  removed. Full-style pins keep their border. (#38)
- **Honest update checks.** Network failures (DNS, timeout, firewall)
  are no longer reported as "you're on the latest version": the real
  cause shows in Settings → About with an inline Retry button, and a
  manual check from the menu navigates there so the result is always
  visible. Menu item reworded to "Check for updates". (#38)
- **"Clear all" in Logs asks for confirmation** and states how many
  entries are deleted; the button was relabeled and restyled as a
  destructive action. (#38)
- **ECS tab titles** prefer the service/container name over the raw
  task id: "ECS · web (d9808c7b)" instead of a truncated hex string. (#38)
- Group cards show a trailing chevron so folders read as "openable" at
  a glance; the theme pickers' "use global theme" row is now a real
  palette card previewing the effective global palette; "1 hosts"
  pluralization fixed everywhere; the destroy-vault warning now
  enumerates exactly what gets deleted; several remaining hardcoded
  English strings localized across all 17 languages. (#38)
- **Recording is now opt-in.** Session logging (terminal output capture)
  now defaults to off instead of on, so a fresh install records nothing
  until you ask it to. The new **Connection history** toggle (Settings →
  Terminal) likewise defaults to off and gates whether connection events
  (connects, disconnects, auth failures, errors) are written to the vault.
  The History nav entry hides itself entirely while both toggles are off
  and no recorded data exists, so the feature stays out of the way until
  it's wanted.

### Fixed
- **Pinned ECS tabs survive task recycling.** Reopening a pinned ECS
  Exec tab resolves the dynamic group and connects to the task
  currently running (the saved task id is ephemeral by design). When
  the exec still fails, the error dialog offers a "Connect to current
  task" recovery button and the app lands on the group's task listing;
  the dormant placeholder re-arms so selecting the tab again retries
  instead of staying a dead pane. Reopening a cloud pin also stays on
  its placeholder with a connecting hint instead of flashing the Hosts
  view during the spawn.
- **Pinned tabs no longer duplicate.** Pins de-duplicate by identity
  (host id / cloud group + container, ignoring recycled task and pod
  ids) when persisting and when restoring at boot, healing strips that
  had already accumulated duplicate chips.
- **Logs view shows new activity.** Entering Logs re-reads the
  timeline from the vault; sessions recorded after boot only existed
  in the database and were invisible until an unrelated full reload.
- **Consistent confirmation dialogs.** Destructive confirmations
  (delete recording, uninstall plugin, clear all) use the error red
  for the primary action and the same button order (Cancel leading,
  action trailing).
- **IME / CJK input was blocked in the terminal.** With a terminal open, the
  OS input method (IME) stayed locked in direct (English) mode and could not
  be switched to Korean / Chinese / Japanese composition. The terminal is an
  `iced` canvas rather than a `text_input`, so nothing in its widget tree ever
  asked the runtime for an input method, and winit defaults `set_ime_allowed`
  to off, which is exactly the "stuck in EN" state. The focused terminal pane
  now requests the input method on every redraw, so the IME can be switched to
  any Asian script just like in the app's text fields, and the composed text
  (delivered as a separate IME commit event) is forwarded to the active local
  or SSH session, behind the same focus guards as keystrokes so it never leaks
  into a focused text field or modal. The candidate popup follows the terminal
  caret.

## [0.8.1] - 2026-06-08

### Fixed
- **Terminal input was dead after connecting.** Since v0.8.0 the terminal
  accepted no keystrokes at all (characters or Enter) on every launch and
  every platform: the SFTP host picker's open-state flag defaulted to `true`
  at boot, and v0.8.0 had started treating that flag as a focus-owning modal
  in the global keyboard gate, so it silently swallowed every key before it
  reached the session, with no SFTP UI ever shown. The flag now defaults to
  off, and all SFTP dialogs (host picker, rename, new, properties, overwrite,
  delete) are layered at the app root as full-window blocking overlays like
  every other modal, so a set modal flag always corresponds to a visible
  modal and can never freeze a terminal behind it. The empty SFTP remote pane
  also gained a centered prompt with a "Pick a host" button, and Esc closes
  the host picker.
- **Renderer crash self-heal on incompatible GPUs.** On GPU/driver stacks
  that can't satisfy `iced_wgpu`'s shader requirements (VMs, old drivers,
  software Vulkan), the app panicked during shader validation after the
  device was created, past the point where iced falls back to its tiny-skia
  software renderer. A panic hook now catches that, escalates the backend
  (auto -> GL -> software), persists the choice, and relaunches, bounded to
  two escalations so an unrenderable setup can't loop. Working GPUs keep
  hardware acceleration since it only triggers on an actual crash.
- **Terminal scrollback size now applies.** The scrollback-lines setting was
  saved to the vault and read on boot, but the terminal backend hard-coded a
  10,000-line history and never received the configured value, so changing it
  did nothing. The setting is now passed through to the backend.
- **Three untranslated UI strings.** The identity editor's Save / Update
  button, the AI settings Save button, and the AI settings "API URL" label
  were hard-coded in English instead of going through `i18n::t`. They are now
  translated across all 17 languages.

## [0.8.0] - 2026-06-06

### Added
- **AI assistant that runs commands.** The terminal-side AI chat now drives
  the session directly through an `execute_command` tool instead of printing
  commands for you to copy: ask it to check, fix, or inspect something and it
  runs the command in the focused pane and reads the output back. Auto-exec is
  gated by three independent safety layers so a destructive command can never
  run unattended: a deterministic floor that always forces a confirmation for
  catastrophic host-level commands (`rm -rf`, `mkfs`, `dd` to a raw device,
  `reboot`, fork bombs, `DROP DATABASE`, ...) no matter how the model
  classified it; an independent LLM judge that vets the nuanced rest and fails
  safe (any error or ambiguity blocks); and a per-session "always run X"
  allow-list that is keyed on a single simple command and refuses to shortcut
  anything containing shell chaining, pipes, redirection, or substitution
  (`ls; rm -rf ~` can't ride a trusted `ls`). The chat also warns up front that
  the assistant executes commands on your live servers.
- **Kubernetes cloud provider.** A new "Kubernetes" option in Cloud
  Accounts, authenticated by a kubeconfig (optional path + context). It
  discovers workloads (Deployments / StatefulSets / DaemonSets) across
  namespaces, imports the selected ones as dynamic groups that resolve to
  their live pods on expand, and opens an interactive shell in a pod. The
  provider ships as a subprocess plugin like AWS, but is a thin wrapper that
  drives the `kubectl` CLI (no heavy SDK): discovery / resolve run
  `kubectl get ... -o json`, and the pod shell spawns `kubectl exec -it` in a
  local PTY. `kubectl` must be on PATH; a missing binary surfaces a clear
  dialog. The dynamic-group editor lets you change the context, namespace and
  label selector of an imported group. A workload whose selector can't be
  resolved to concrete labels is reported rather than silently resolving to
  every pod in the namespace.
- **Port forwarding as a standalone entity.** Port forwards are no longer
  tied to a terminal session. A new "Port Forwarding" area in the sidebar
  manages `PortForwardRule` entities, each with a per-row on/off toggle that
  opens a dedicated PTY-less SSH connection holding the tunnel until turned
  off, plus an "auto-start on launch" option. All three directions are
  supported: Local (`-L`), Remote (`-R`, via `tcpip-forward`, with a
  `GatewayPorts yes` hint when binding `0.0.0.0`), and Dynamic SOCKS5 (`-D`,
  a local SOCKS5 proxy that opens a `direct-tcpip` channel per request). A
  dynamic forward bound to a non-loopback address warns that it exposes an
  unauthenticated open proxy into the remote network. Toggling a rule on for
  an untrusted host surfaces the same host-key verification modal the terminal
  uses; boot auto-start stays known-only and silent. A dropped connection
  flips the row back to off. Rules sync over P2P and travel in portable
  export/import; legacy inline `Connection.port_forwards` are migrated into
  `Local` rules (`auto_start = false`) on first launch, with the legacy field
  kept as the "raise with the terminal" shortcut.
- **Split panes.** A terminal tab can now be split into an arbitrary grid
  of panes (tmux / iTerm style), built on iced's `pane_grid`. Ctrl+Shift+E
  splits the focused pane side-by-side, Ctrl+Shift+O stacks it. You can also
  split from the popover that appears on hovering the `+` tab button, or from
  a tab's right-click menu. Each split opens the connection picker so the new
  pane can be a saved host (it connects inside the pane, with the shared
  host-key prompt for untrusted hosts) or a local shell. Drag the dividers to
  resize, click or Ctrl+Shift+arrow to move focus, Ctrl+Shift+W to close a
  pane (closing the last one closes the tab). Each pane keeps its own session,
  output and scrollback; keyboard, paste, snippets and the AI assistant target
  the focused pane. A split tab shows the focused pane's name + icon plus a
  pane-count badge, so a tab split across two hosts reads as whichever pane
  you're in.
- **Session groups.** Save a split-panel arrangement as a reusable entity:
  right-click a tab and pick "Save as group" (or "Edit group" once it came
  from one). A session group carries no connection data of its own, just a
  reference to each pane (a saved host by id, or a local shell) plus the
  exact split tree (axes and ratios), and it lives in a folder with its own
  name, color and icon like a host. Each pane can carry its own startup
  script, which overrides the host's `initial_command` for that pane (empty
  falls back to it; local shells just run the script), so you can open five
  local terminals each running a different command. Opening a group rebuilds
  a single splitted tab and connects every pane; a host that was deleted in
  the meantime is dropped with a warning rather than failing the whole open.
  Groups appear on the dashboard alongside hosts, sync as a credential-free
  entity, and travel in a full portable export.
- **Server-to-server file copy in the SFTP tab.** Transfer files directly
  between two remote hosts in the dual-pane SFTP browser, with the bytes
  streamed host-to-host through the app (no full local round-trip to disk) and
  a live byte-level progress bar. A failed transfer removes the partial file
  on the destination rather than leaving a truncated one behind.
- **SFTP dual-pane UX pass.** A reworked two-pane browser with type-ahead row
  selection, drag-and-drop (including from the Windows / WSL host), modal
  operations that block interaction with the panes underneath, and live
  byte-level progress on every transfer (upload, download, and server-to-server
  relay).
- **Custom themes.** Create your own terminal color schemes (the 16 ANSI
  colors plus foreground / background / cursor) and your own UI / chrome
  themes (the 21 app colors), each with a built-in graphical color picker
  (saturation/value square + hue bar, no third-party crate) and a live
  preview. Custom terminal themes appear in the Settings -> Terminal grid
  (and the per-host theme picker) alongside the presets; custom UI themes
  appear in Settings -> Interface, seeded from the active theme so you start
  from something that works. Terminal schemes can also be imported by pasting
  an iTerm `.itermcolors`, Windows Terminal JSON, or base16 YAML.
- **Custom host icon picker overhaul.** The per-host icon/color dialog now
  uses the same graphical color picker as the custom-theme editor (the
  saturation/value square + hue bar) instead of a fixed swatch palette, and
  the icon section gained a search box that filters the entire Lucide library
  (~1500 glyphs) on top of the curated presets. The whole icon font already
  ships in the binary, so searching every glyph adds no extra weight. The
  modal's backdrop is now opaque too, so hover / scroll / clicks no longer
  bleed through to the host list underneath it.
- **Graceful plugin shutdown.** Cloud-provider plugin subprocesses (AWS,
  Kubernetes) are now drained before they are reaped: on idle teardown,
  rebind, and app exit the host lets in-flight requests finish, sends a
  `shutdown` notification, and closes stdin so the plugin exits on its own
  (flushing logs / closing SDK clients) instead of being hard-killed. The
  hard kill stays only as a time-bounded fallback for a wedged plugin, so
  app close can't hang. The `shutdown` notification is additive (no protocol
  bump; plugins that predate it still exit cleanly on the stdin EOF).
- **Multi-hop host chaining.** The host editor's "Host Chaining" row now
  opens a dedicated chain editor (Termius style) instead of a single-host
  picker: build an ordered chain of jump hosts, reorder them, and remove
  them, with the host being edited shown as the final destination. The
  session tunnels through each hop in order before reaching the host. The
  data model and SSH engine already supported arbitrary-length chains; this
  exposes them in the UI. The old read-only "Host Chaining" display row and
  the separate single-host "Jump Host" picker (which both edited the same
  field) are collapsed into this one entry point.
- **Pinned tabs.** Pin a tab from its context menu and it renders first in
  the strip, survives "close other tabs" / "close all tabs" (like a browser),
  and reappears on the next launch. Two styles, chosen in Settings -> Interface:
  a compact Chrome-style icon chip, or the full tab with a distinct accent
  border. Restore is lazy: a pinned tab comes back dormant (a placeholder in
  the strip) and only reconnects the host (or respawns the local shell) the
  first time you select it, so launch stays fast. Works for saved hosts, local
  shells, and ECS Exec / kubectl pods (the latter reopen via the same reconnect
  path, re-resolving the group if the task recycled). Pinning is offered on
  single-pane tabs (a split or session-group tab is saved as a session group
  instead). SSM sessions can be pinned for the session but are not yet restored
  across restarts.
- **Drag to reorder tabs.** Drag a tab in the strip to reposition it: it lifts
  into a floating ghost that follows the cursor while the other tabs slide out
  of the way live to open the drop slot. Reordering is scoped to within a group
  (pinned among pinned, normal among normal), so the pinned-first layout stays
  consistent. The pinned order persists across restarts.
- **Multi-line snippets.** The snippet command field auto-grows into a
  multi-line editor, so a snippet can hold a small script instead of a single
  line.
- **Import a shared host from the "+ Host" menu.** The share / import flow is
  reachable directly from the "+ Host" menu, with a smoother end-to-end import.
- **Six new languages.** Korean, Polish, Turkish, Indonesian, Vietnamese and
  Ukrainian bring the UI to 17 languages. The i18n tables were split from one
  monolithic file into a module per language (`i18n/<code>.rs`), and the UI
  font switched to Noto Sans (with Noto Sans Arabic and a CJK menu fallback)
  for full coverage of the new scripts.
- **Full AGPL-3.0 license text.** The complete license is now shipped in the
  repository.

### Fixed
- **Modal overlays.** Picker and editor modals (the chain editor, host editor,
  icon picker, theme editors) no longer leak hover and scroll events to the
  list and editor behind them: every modal now routes through one shared
  overlay whose backdrop captures every mouse event, not just clicks, and
  opening one no longer resets the scroll position of the content underneath.
- **Vault sub-navigation.** The "Hosts" top tab stays selected across all
  vault sub-sections (Keys / Snippets / Port Forwarding / History) instead of
  losing the highlight.

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

## [0.2.1] - 2026-04-10

### Fixed
- Keep the PTY slave alive on Windows to stop ConPTY from terminating the
  local shell session.

## [0.2.0] - 2026-04-10

### Added
- **Vault export / import** (portable, encrypted).
- **MCP server** to expose SSH hosts to external clients (initial version).
- **P2P sync** and host sharing between devices (initial version).
- **Local port forwarding** (`-L` style).
- Traditional private-key PEM formats on import: PKCS#1, PKCS#8, SEC1.
- Ko-fi / Buy Me a Coffee funding links.

### Fixed
- UI icon rendering and dropdown clipping.

## [0.1.1] - 2026-04-08

### Fixed
- Normalize CRLF line endings on SSH key import.

## [0.1.0] - 2026-04-07

First public release. MVP SSH client.

### Added
- **SSH client** with an embedded alacritty-based terminal, local shell, and
  snippets.
- **Encrypted vault** (Argon2id + ChaCha20Poly1305) with boot flow, existing
  password detection, and a guarded vault reset.
- **Keys & identities**: private-key import with username autocomplete and a
  reusable identity system.
- **AI chat** sidebar with stabilization-based command-output polling.
- **Session recording**.
- **i18n**: 9 languages (en, pt-BR, es, fr, de, it, zh, ja, ru) with an
  in-settings language picker.
- **Themes** and floating overlay menus (context menus / dropdowns float over
  content with backdrop dismiss).
- Windows packaging: NSIS installer, multi-size `.ico`, and App Paths
  registration for Windows Search.
