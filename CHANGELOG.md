# Changelog

All notable changes to Oryxis are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project uses [SemVer](https://semver.org/spec/v2.0.0.html).

## [0.8.3] - Unreleased

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
