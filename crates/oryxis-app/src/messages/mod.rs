//! The full `Message` enum, every event the iced runtime can dispatch
//! to `Oryxis::update`. Pulled out of `app.rs` so the message-loop file
//! is shorter; re-exported via `pub use` at the bottom of `app.rs` so
//! call sites continue to write `crate::app::Message::Foo`.


use uuid::Uuid;


/// Payload wrapper for the secret-bearing fields, so a message that
/// carries a password can be `Debug`-formatted (the stall watchdog and
/// `dispatch::unrouted` both do) without the secret reaching the log.
mod redacted;
pub use redacted::Redacted;

mod ai;
pub use ai::AiMessage;
mod onboarding;
pub use onboarding::OnboardingMessage;
mod sftp;
pub use sftp::SftpMessage;
mod settings;
pub use settings::SettingsMessage;
mod tabs;
pub use tabs::TabsMessage;
mod editor;
pub use editor::EditorMessage;
mod keys;
pub use keys::KeysMessage;
mod monitor;
pub use monitor::MonitorMessage;
mod net_tools;
pub use net_tools::NetToolsMessage;
mod tmux;
pub use tmux::TmuxMessage;
mod sidebar_files;
pub use sidebar_files::SidebarFilesMessage;
mod terminal;
pub use terminal::TerminalMessage;
mod ssh;
pub use ssh::SshMessage;
mod cloud;
pub use cloud::CloudMessage;
mod history;
pub use history::HistoryMessage;
mod mcp;
pub use mcp::McpMessage;
mod navigation;
pub use navigation::NavigationMessage;
mod command_history;
pub use command_history::CommandHistoryMessage;
mod update;
pub use update::UpdateMessage;
mod proxy_identity;
pub use proxy_identity::ProxyIdentityMessage;
mod plugin;
pub use plugin::PluginMessage;
mod agent;
pub use agent::AgentMessage;
mod multi_vault;
pub use multi_vault::MultiVaultMessage;
mod zmodem;
pub use zmodem::ZmodemMessage;
mod known_host;
pub use known_host::KnownHostMessage;
mod remote_desktop;
pub use remote_desktop::RemoteDesktopMessage;
mod tray;
pub use tray::TrayMessage;
mod player;
pub use player::PlayerMessage;
mod vault;
pub use vault::VaultMessage;
mod session_group;
pub use session_group::SessionGroupMessage;
mod port_forward;
pub use port_forward::PortForwardMessage;
mod snippet;
pub use snippet::SnippetMessage;
mod share;
pub use share::ShareMessage;
mod sync;
pub use sync::SyncMessage;

/// The four per-class Privacy Mode gates (issue #78 block 1), each
/// mirroring a `privacy_mask_*` setting. The usernames class covers
/// both the shape heuristics (`user@host`, home dirs) and the
/// saved-connection usernames inside the terms list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivacyMaskClass {
    PublicIps,
    PrivateIps,
    Usernames,
    Hostnames,
}

#[derive(Debug, Clone)]
pub enum Message {
    // Vault
    // Vault lock / password / biometric (handle_vault)
    Vault(VaultMessage),

    // First-run welcome / onboarding carousel (rendered off
    // `VaultState::NeedSetup`).
    Onboarding(OnboardingMessage),

    // Navigation
    // Navigation (handle_navigation)
    Navigation(NavigationMessage),

    // Tabs
    // Tabs (handle_tabs)
    Tabs(TabsMessage),
    // ── Command palette (C4) ────────────────────────────────────────
    // Absorb-click sink, used by modal bodies to stop clicks from falling
    // through to the backdrop underneath. Handler is a no-op.
    NoOp,

    // Icon picker (custom host icon/color)
    // Per-host terminal theme picker (modal opened from the host
    // editor). The form field updates immediately on select; the
    // change is committed on EditorSave like every other form field.
    // Editor (handle_editor)
    Editor(EditorMessage),
    // ── C5 per-host legacy keyboard modes + feature toggles ──────────

    // SFTP browser. Most pane operations are side-addressed: the
    // `SftpPaneSide` says *which* pane (Left / Right), and the handler
    // branches on that pane's `is_remote` flag to pick filesystem vs
    // SFTP behaviour.
    /// Wrapper for the SFTP async-completion sub-enum ([`SftpMessage`]).
    /// The owner-routing envelope (`SftpFor`) carries these, and
    /// `route_sftp_async` re-dispatches unowned ones through here.
    Sftp(SftpMessage),
    /// Owner-routing envelope for SFTP async completions whose payload has no
    /// owner stamp of its own (the mount pipeline: `SftpMessage::HostMounted` /
    /// `SftpMessage::RemoteError`). Carries the id of the tab (standalone SFTP tab or
    /// hybrid terminal tab) that owned the live buffer at kickoff time, so
    /// `route_sftp_async` swaps that owner's state in before the inner
    /// message runs, or drops it when the owner is gone. Without it, a
    /// park/hoist swap between kickoff and completion would land the result
    /// in whichever buffer happens to be live. Built via `Message::sftp_owned`.
    SftpFor(Uuid, Box<SftpMessage>),

    // Row interactions

    // Sidebar Files tab (the per-pane SFTP browser next to Chat /
    // Snippets / History). Navigation targets the ACTIVE pane; async
    // results carry the pane's stable `Uuid` so a pane/tab switch
    // mid-flight can't land a listing on the wrong browser.
    // SidebarFiles (handle_sidebar_files)
    SidebarFiles(SidebarFilesMessage),
    // Sidebar Monitor tab (agentless host vitals over the pane's live
    // session). Samples carry the connection's `Uuid` + a request stamp
    // so a reconnect mid-probe can't land on the wrong series.
    // Monitor (handle_monitor)
    Monitor(MonitorMessage),
    // Sidebar tmux tab (issue #116): list / create / attach / kill the
    // tmux sessions on the pane's host. Every variant carries the
    // pane's stable `Uuid` so a pane switch mid-flight can't land a
    // listing (or an attach) on the wrong shell.
    // Tmux (handle_tmux)
    Tmux(TmuxMessage),
    /// Open an arbitrary URL in the user's default browser.
    /// Used by clickable links in the About panel.
    OpenUrl(String),
    /// Copy a string to the system clipboard. Used by the Copy
    /// affordance on chat bubbles and code blocks (text-selection
    /// isn't supported by iced's `text` / markdown widgets in 0.14).
    CopyToClipboard(String),
    /// Result of the `CopyToClipboard` write, reported by the runtime.
    /// The clipboard belongs to the iced runtime (see
    /// `dispatch_global::write_clipboard_text`), so the "Copied" toast can
    /// only be raised once the write actually landed.
    ClipboardWritten(bool),
    /// Dismiss the transient toast chip (`Oryxis.toast`). Deadline-guarded
    /// clear: drops the chip only if its deadline has passed. Fired by the
    /// `ToastTick`-style subscription and by any legacy scheduled
    /// sleep-timer, so a superseded timer can never wipe a newer toast.
    ToastClear,
    /// Immediate dismissal (clicking the chip), regardless of deadline.
    ToastDismiss,
    /// Raise the toast chip with this text. Fired by work that runs off
    /// the UI thread and only learns late that an in-app notice is owed:
    /// an OS notification the OS refused to carry
    /// (`dispatch_global::take_os_notice_tasks`).
    ToastShow(String),
    /// Dismiss the blocking error dialog (`Oryxis.error_dialog`). Fired
    /// by the OK button or by clicking the scrim.
    ErrorDialogDismiss,
    /// Fire the dialog's optional recovery action: dismisses the
    /// dialog and dispatches the message it carries.
    ErrorDialogRunAction,
    /// A message that came from a window other than the main one (see
    /// `window_ctx`): the extra window's widgets and input events are
    /// wrapped in this, and `update` runs the inner message with that
    /// window's strip, active tab and geometry in place.
    InWindow(iced::window::Id, Box<Message>),
    /// A message about one pane (its dial stream, its ZMODEM transfer):
    /// runs in the window that shows the pane's tab NOW. The stream was
    /// started in one window and every message it yields arrives as that
    /// window's (`InWindow`), but the tab may have moved since; without
    /// this a host-key or PIN prompt raised on a reconnect floated in the
    /// window the dial started in, and in the resident once that window
    /// closed. Resolved by `update_in_window` / `update_for_pane` BEFORE
    /// any window is swapped in; a pane that is gone runs where it was
    /// delivered.
    ForPane(Uuid, Box<Message>),
    /// A message about one vault (a pane's tab of it, its sync, its
    /// monitor): runs with that vault as the one `update` works on,
    /// whatever window it is delivered to (`update_on_vault`). A window
    /// working on another vault keeps the tabs of this one parked, and
    /// their streams still have to reach this vault.
    ForVault(Uuid, Box<Message>),
    // Archive operations (extract / compress / virtual zip browse).
    // Async completions ride the `SftpFor` owner envelope like the
    // transfer queue does.
    // The leading `Uuid` on the transfer-queue continuation messages is the
    // owning SFTP tab. These arrive after async work, by which point the user
    // may have focused another SFTP tab; the dispatcher swaps the owning tab's
    // state into `self.sftp` for the duration so the handler routes to the
    // right tab. See `Message::sftp_async_owner` + `route_sftp_async`.

    // Folder (group) actions

    // Terminal I/O
    // Terminal (handle_terminal)
    Terminal(TerminalMessage),
    // Zmodem (handle_zmodem)
    Zmodem(ZmodemMessage),
    // Cloud (handle_cloud)
    Cloud(CloudMessage),
    // Settings (handle_settings)
    Settings(SettingsMessage),

    // Overlay

    // Card interactions
    // CommandHistory (handle_command_history)
    CommandHistory(CommandHistoryMessage),

    // Connection editor
    // Serial line params (reduced Serial form). Each carries the typed
    // value; the handler materializes `SerialParams` defaults first.
    // Remote desktop (RDP/VNC) editor rows: kind picker + the SSH host
    // to tunnel through (`None` = direct). The desktop endpoint + login
    // reuse the normal hostname/port/username/password fields.
    // Chain editor (Termius-style multi-hop jump-host editor). Opens
    // from the "Host Chaining" row in the host editor; edits the
    // ordered `editor_form.jump_chain`.

    // Session groups (saved split-panel arrangements)
    // Session groups (handle_session_group)
    SessionGroup(SessionGroupMessage),

    // SSH
    // Ssh (handle_ssh)
    Ssh(SshMessage),

    // Snippets
    // Snippets (handle_snippets)
    Snippet(SnippetMessage),

    // Command history (terminal sidebar History tab)

    // Split panes

    // Custom terminal themes (Settings -> Themes)

    // Custom UI (chrome) themes (Settings -> Interface). `usize` is the
    // color-field index into `theme::UI_COLOR_FIELDS`.

    // Port forwards (standalone entity)
    // Port forwards (handle_port_forwards)
    PortForward(PortForwardMessage),
    // ProxyIdentity (handle_proxy_identity)
    ProxyIdentity(ProxyIdentityMessage),

    // Terminal side panel (Chat / Snippets / Host config tabs)
    // AI settings + chat sidebar (handle_ai)
    Ai(AiMessage),

    // Known hosts
    // KnownHost (handle_known_host)
    KnownHost(KnownHostMessage),

    // History
    // History (handle_history)
    History(HistoryMessage),

    // Session logs
    /// In-app session player (issue #71): a read-only playback surface
    /// on the History view.
    Player(PlayerMessage),
    // History was split in v0.6 (logs + session logs in two panes
    // with independent pagination); v0.7 merges both into one timeline
    // so the per-section Clear / Next / Prev controls don't render
    // anymore. Handlers stay wired so we can resurrect a dedicated
    // session-logs surface without re-introducing the messages.

    // Network tools
    /// The optional network tools panel (DNS, ping, traceroute, port
    /// test, HTTP/TLS, WHOIS, DNSBL). Hidden behind
    /// `network_tools_enabled`.
    NetTools(NetToolsMessage),

    // Settings
    // Update (handle_update)
    Update(UpdateMessage),
    /// Toggle the Logs view Privacy Mode reveal (show raw sensitive data
    /// in the timeline + session-log viewer until toggled back).
    TogglePrivacyReveal,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    Tray(TrayMessage),
    // RemoteDesktop (handle_remote_desktop)
    RemoteDesktop(RemoteDesktopMessage),

    // Auto-update

    // Language

    // Local shell

    // Local terminals management (Settings → Terminal card)

    // Keys
    // Keys (handle_keys)
    Keys(KeysMessage),

    // ── SSH-agent server (B1) ──
    // Agent (handle_agent)
    Agent(AgentMessage),
    MultiVault(MultiVaultMessage),
    // (filename, content, auto-probed `<file>.pub` and `<file>-cert.pub`
    // if present and parseable)

    // Identities

    // Per-list sort menus (Hosts / Keychain / Snippets toolbars).
    // The Toggle* messages open/close the dropdown anchored to the
    // toolbar sort button; the Set* messages pick a sort mode and
    // persist it via the matching `*_sort` settings key.

    // Responsive toolbar collapse (narrow windows). `ToggleToolbarSearch`
    // pops/dismisses the floating search field when the inline box has
    // collapsed to an icon; `ToggleToolbarOverflow` pops/dismisses the
    // `…` menu folding the view's secondary toolbar actions.

    // Keyboard navigation, modal layer: pointer hover moved onto a
    // recorded row, so the keyboard ring follows it (index into the
    // per-frame `keynav.modal.items` recording).
    // A pick_list dropdown opened (true) or closed (false); keeps
    // `keynav.pick_open` in sync so app-side key routing yields to
    // the widget while its menu is up.

    // Proxy Identities (Settings → Proxies)

    // Cloud Accounts
    // Wired to a future "show password" eye icon next to the secret
    // input, `text_input.secure(false)` flips when this fires.

    // Cloud Discovery & Import

    // Plugins panel, cloud-provider plugin install / update lifecycle.
    // Plugin (handle_plugin)
    Plugin(PluginMessage),

    // Edit dynamic group panel, sets template fields (key, identity,
    // transport, initial command) on a `Group.cloud_query`.

    // Connection identity

    // AI settings

    // Vault password management

    // AI chat sidebar

    // Port forwarding

    // SSH agent forwarding (per-host opt-in)

    // MCP
    // Mcp (handle_mcp)
    Mcp(McpMessage),

    // Sync
    Sync(SyncMessage),

    // Export / Import
    // Export / import / share (handle_share)
    Share(ShareMessage),

    // System tray (Windows only at runtime; messages compile on
    // every platform so dispatch.rs and subscription.rs stay cfg-
    // free).

    // Share
}

impl Message {
    /// `self` as a follow-up of an update that ran on `vault`: it runs on
    /// that vault again when it arrives (`ForVault`), whatever vault the
    /// window it reaches works on by then. Inside an `InWindow` (the
    /// window swap comes first, then the vault), and left alone when it
    /// already names its own route.
    ///
    /// A reply about a WINDOW (its place, its size, its state, its close)
    /// is not a vault's: it runs on whatever vault the window is on when
    /// it arrives, so it never goes stale with a vault closed meanwhile.
    pub(crate) fn on_vault(self, vault: Uuid) -> Message {
        match self {
            Message::InWindow(window, inner) => Message::InWindow(window, Box::new(inner.on_vault(vault))),
            m @ (Message::ForVault(..) | Message::ForPane(..) | Message::NoOp) => m,
            m if m.is_about_a_window() => m,
            other => Message::ForVault(vault, Box::new(other)),
        }
    }

    /// The replies about a window itself (see [`Self::on_vault`]).
    fn is_about_a_window(&self) -> bool {
        matches!(
            self,
            Message::Tabs(
                TabsMessage::WindowResized(_)
                    | TabsMessage::WindowMoved(_)
                    | TabsMessage::WindowEnsureOnScreen
                    | TabsMessage::WindowFocusChanged(_)
                    | TabsMessage::WindowStateSynced { .. }
                    | TabsMessage::WindowFullscreenSettled(_)
                    | TabsMessage::WindowClose
                    | TabsMessage::ConfirmCloseWindow { .. }
                    | TabsMessage::FullscreenHintHide
                    | TabsMessage::DetachTabAt(..)
                    | TabsMessage::ToplevelDragStarted { .. }
                    | TabsMessage::CarryHovered(_)
                    | TabsMessage::CarryReleased
            )
        )
    }

    /// For an SFTP async-continuation message that targets a specific tab,
    /// returns that tab's id. The dispatcher uses this to swap the owning
    /// tab's state into `self.sftp` for the duration so the handler routes
    /// to the right tab even after the user focused a different SFTP tab.
    pub(crate) fn sftp_async_owner(&self) -> Option<Uuid> {
        match self {
            Message::Sftp(SftpMessage::SftpTransferQueueReady(id, _))
            | Message::Sftp(SftpMessage::SftpTransferNext(id))
            | Message::Sftp(SftpMessage::SftpTransferItemDone(id, _))
            | Message::Sftp(SftpMessage::SftpTransferError(id, _, _))
            | Message::Sftp(SftpMessage::SftpTransferConflict(id, _, _, _))
            | Message::SftpFor(id, _) => Some(*id),
            _ => None,
        }
    }

    /// Wrap an SFTP async completion in the `SftpFor` owner-routing
    /// envelope when a buffer owner existed at kickoff time. `None`
    /// falls back to the unowned message (pre-envelope behavior: applied
    /// to whichever buffer is live on arrival), which only happens when
    /// no SFTP surface owned the buffer at all.
    pub(crate) fn sftp_owned(owner: Option<Uuid>, message: SftpMessage) -> Message {
        match owner {
            Some(id) => Message::SftpFor(id, Box::new(message)),
            None => Message::Sftp(message),
        }
    }
}

#[cfg(test)]
mod tests {
    /// A variant name must be unique across ALL the sub-enums.
    ///
    /// Two sub-enums may legally declare the same simple name, and the
    /// wrappers make either one compile at every send site, so the
    /// wrapper is the only thing telling them apart and nothing checks
    /// it. That is a permanent landmine rather than a bug: it waits for
    /// whoever next reaches for the name.
    ///
    /// The convention was written down and drifted anyway, twice: the
    /// sync prefix strip minted three collisions with `SftpMessage`, and
    /// `NetToolsMessage` arrived with `TabsMessage`'s `CardHovered(usize)`
    /// under a different wrapper. Reading the sources is what makes the
    /// rule hold without anyone remembering it, and it follows the
    /// precedent of `lockfile_guard.rs` reading `Cargo.lock`.
    #[test]
    fn no_variant_name_is_declared_by_two_sub_enums() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/messages");
        let mut owners: std::collections::BTreeMap<String, Vec<String>> = Default::default();
        let mut files = 0usize;
        for entry in std::fs::read_dir(&dir).expect("read src/messages") {
            let path = entry.expect("dir entry").path();
            let stem = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) if path.extension().is_some_and(|e| e == "rs") && s != "mod" => {
                    s.to_owned()
                }
                _ => continue,
            };
            files += 1;
            let src = std::fs::read_to_string(&path).expect("read sub-enum source");
            for raw in src.lines() {
                // A variant is declared at exactly one indent level
                // inside its enum; anything deeper is a field or a match
                // arm, and anything shallower is an item.
                let Some(rest) = raw.strip_prefix("    ") else {
                    continue;
                };
                if rest.starts_with(' ') || rest.starts_with("//") {
                    continue;
                }
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect();
                if name.is_empty() || !name.starts_with(|c: char| c.is_ascii_uppercase()) {
                    continue;
                }
                // Only a declaration, never a use: the character after
                // the name says which.
                let tail = &rest[name.len()..];
                if !(tail.starts_with('(')
                    || tail.starts_with('{')
                    || tail.starts_with(',')
                    || tail.is_empty())
                {
                    continue;
                }
                owners.entry(name).or_default().push(stem.clone());
            }
        }
        assert!(files > 20, "only found {files} sub-enum files; the scan moved");

        let clashes: Vec<String> = owners
            .iter()
            .filter(|(_, wheres)| wheres.len() > 1)
            .map(|(name, wheres)| format!("{name} in {}", wheres.join(" and ")))
            .collect();
        assert!(
            clashes.is_empty(),
            "a variant name is declared by two sub-enums, so the wrapper is \
             the only thing telling them apart:\n  {}",
            clashes.join("\n  ")
        );
    }
}
