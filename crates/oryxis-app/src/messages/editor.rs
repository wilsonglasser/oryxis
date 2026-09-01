//! Host editor form: fields, quirks, serial, proxy, chain editor, env vars, port forwards.

use iced::widget::text_editor;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum EditorMessage {
    EditorOpenThemePicker,
    /// Host editor: per-host background opacity, as the picker's label
    /// (an "Inherit" sentinel clears the override).
    EditorOpacityChanged(String),
    /// Host editor: pick / clear / lay out / fade this host's own
    /// background picture. `Picked` carries the dialog result, where
    /// `Err` is a cancel and stays silent.
    EditorBgImageBrowse,
    EditorBgImagePicked(Result<String, String>),
    /// Cycles the picture override: global picture -> none on this host
    /// -> back to inheriting. Three states, so the row can express "no
    /// picture HERE" against a global one.
    EditorBgImageModeChanged(String),
    EditorBgFitChanged(String),
    EditorBgDimChanged(String),
    EditorCloseThemePicker,
    /// Filter line typed in the per-host theme picker (matches card
    /// labels, case-insensitive).
    EditorThemePickerFilterChanged(String),
    /// Empty string == "inherit the global theme".
    EditorTerminalThemeChanged(String),
    /// Cloud transport pick (only meaningful when editing a cloud-imported host).
    EditorCloudTransportChanged(oryxis_core::models::cloud::TransportKind),
    /// Per-host initial command, sent as keystrokes after the shell
    /// opens. Empty = none. Useful for hosts that drop into `/bin/sh`
    /// when you really want `bash`.
    EditorInitialCommandChanged(text_editor::Action),
    /// Set the per-host icon shape override. Empty string clears the
    /// override (falls back to the global `default_host_icon`).
    EditorIconStyleChanged(String),
    EditorEncodingChanged(String),
    /// How this host measures Unicode "Ambiguous" width characters,
    /// picked in the host editor's Terminal card.
    EditorAmbiguousWidthChanged(oryxis_core::models::connection::AmbiguousWidth),
    /// Per-host TERM name picked in the host editor.
    EditorTerminalTypeChanged(String),
    /// Empty string == "inherit the global keepalive setting".
    /// "0" == explicitly disabled on this host; any positive integer
    /// is the per-host override in seconds. Sanitized to digits-only.
    EditorKeepaliveChanged(String),
    /// Wake-on-LAN MAC address as typed. Empty == no MAC (hides the
    /// card action); validated on save, not per keystroke.
    EditorMacAddressChanged(String),
    /// Directory a fresh SFTP mount of this host lands in, as typed.
    /// Empty == the login directory (the default).
    EditorSftpInitialPathChanged(String),
    /// SSH > Integration: flip the per-host "drag-and-drop uploads ride
    /// ZMODEM (`rz`) instead of SFTP" flag, for shells that run inside
    /// a container (SFTP reaches the host filesystem, `rz` lands where
    /// the shell runs).
    EditorToggleZmodemDrops,
    /// Per-host auto-title (OSC 0/2) selection from the host editor pick:
    /// the localized "Default / Show / Hide" label.
    EditorAutoTitleChanged(String),
    /// Per-host Privacy Mode selection from the host editor pick: the
    /// localized "Default / On / Off" label.
    EditorPrivacyModeChanged(String),
    EditorSidebarAutoOpenChanged(String),
    /// Backspace mode pick (localized "Control-? (127)" / "Control-H (8)").
    EditorQuirkBackspaceChanged(String),
    /// Home/End mode pick (localized "Standard" / "rxvt").
    EditorQuirkHomeEndChanged(String),
    /// Function-key mode pick (localized Xterm / Linux / VT400 / rxvt).
    EditorQuirkFnKeysChanged(String),
    /// "Report mouse to remote" toggle (off = `disable_mouse_reporting`).
    EditorQuirkMouseReportingChanged(bool),
    /// "Allow remote title changes" toggle (off = `disable_title_change`).
    EditorQuirkTitleChangeChanged(bool),
    /// OSC 52 clipboard-write override pick (localized Default / On / Off).
    EditorQuirkOsc52Changed(String),
    /// macOS Option-as-Meta pick (localized Off / Left / Right / Both;
    /// issue #80: the default composes characters like every macOS
    /// terminal, Meta is the readline/emacs opt-in).
    EditorQuirkOptionAsMetaChanged(String),
    /// Per-host SSH rekey limit (MB) text input.
    EditorQuirkRekeyChanged(String),
    /// Toggle a per-host SSH algorithm category between Auto (None) and a
    /// custom pinned list (seeded from the safe defaults).
    EditorAlgoSetAuto(crate::state::AlgoCategory, bool),
    /// Add/remove one algorithm name in a category's pinned list.
    EditorAlgoToggle(crate::state::AlgoCategory, String),
    ShowNewConnection,
    /// Open the host editor on a vault host, by ID: the click's index
    /// can go stale before the handler runs (its own flush of a
    /// pending auto-save rename re-sorts the list), same rationale as
    /// `DeleteConnection`.
    EditConnection(uuid::Uuid),
    EditorLabelChanged(String),
    /// Host editor: comma-separated tags field.
    EditorTagsChanged(String),
    EditorHostnameChanged(String),
    /// Host editor: the wire-protocol picker (SSH / Telnet). Switching
    /// swaps the reduced form and, when the port still holds the old
    /// protocol's default, retargets it (22 <-> 23).
    EditorProtocolChanged(oryxis_core::models::connection::ConnectionProtocol),
    EditorSerialBaudChanged(u32),
    EditorSerialDataBitsChanged(u8),
    EditorSerialParityChanged(oryxis_core::models::serial::SerialParity),
    EditorSerialStopBitsChanged(oryxis_core::models::serial::SerialStopBits),
    EditorSerialFlowChanged(oryxis_core::models::serial::SerialFlowControl),
    EditorSerialLineEndingChanged(oryxis_core::models::serial::SerialLineEnding),
    EditorSerialLocalEchoToggled,
    EditorRdKindChanged(oryxis_core::models::remote_desktop::RemoteDesktopKind),
    EditorRdGatewayChanged(Option<uuid::Uuid>),
    /// Host editor: Telnet over TLS (`telnets`). Turning it off also
    /// hides (and, on save, clears) the verification escape below it.
    EditorToggleTelnetTls,
    /// Carry this SSH host over mosh, and the three settings that only
    /// mean anything while it is on.
    EditorToggleMosh,
    EditorMoshServerPathChanged(String),
    EditorMoshPortRangeChanged(String),
    EditorMoshCommandChanged(String),
    /// Host editor: accept a server certificate the trust store
    /// rejects. Per host, and only reachable while TLS is on.
    EditorToggleTelnetTlsInsecure,
    /// Host editor: which curated local terminal a Local host spawns
    /// (`None` = the machine's default shell).
    EditorLocalTerminalChanged(Option<uuid::Uuid>),
    /// Host editor: the folder a Local host starts in.
    EditorLocalCwdChanged(String),
    /// Address-family preference picked in the host editor (SSH > Network).
    EditorAddressFamilyChanged(oryxis_core::models::connection::AddressFamily),
    EditorPortChanged(String),
    EditorUsernameChanged(String),
    EditorPasswordChanged(super::Redacted),
    EditorAuthMethodChanged(String),
    EditorGroupChanged(String),
    EditorKeyChanged(String),
    OpenChainEditor,
    CloseChainEditor,
    /// Switch the chain editor into "add a hop" mode (host picker).
    ChainEditorStartAdd,
    /// Back out of "add a hop" mode to the chain list.
    ChainEditorCancelAdd,
    ChainEditorSearchChanged(String),
    /// Append the selected connection as the next hop.
    ChainEditorAddHop(Uuid),
    ChainEditorRemoveHop(usize),
    ChainEditorMoveHopUp(usize),
    ChainEditorMoveHopDown(usize),
    EditorProxyKindChanged(crate::state::ProxyKind),
    EditorProxyHostChanged(String),
    EditorProxyPortChanged(String),
    EditorProxyUsernameChanged(String),
    EditorProxyPasswordChanged(super::Redacted),
    /// Eye toggle for the inline proxy password. Was routed through the
    /// shared `SettingsMessage::ToggleSecretVisibility` / `revealed_secrets`
    /// set, which outlives the form it was describing; it is a form flag
    /// like the other three eyes now.
    EditorToggleProxyPasswordVisibility,
    EditorProxyCommandChanged(String),
    EditorTogglePasswordVisibility,
    /// TOTP secret (2FA) field: value edit + eye toggle. Tri-state save
    /// mirrors the password field (untouched preserves the stored secret).
    EditorTotpChanged(super::Redacted),
    EditorToggleTotpVisibility,
    EditorUseTotpToggled,
    /// Disk key source: the opt-in toggle, the `IdentityFile` path, and
    /// the file picker that fills it. Not `Redacted`: the value is a
    /// PATH, and the key itself never enters the form.
    EditorUseDiskKeyToggled,
    EditorIdentityFileChanged(String),
    EditorBrowseIdentityFile,
    /// Login automation picker: the combo's display string (the "off"
    /// sentinel, a saved script's name, or the "new script" sentinel).
    EditorLoginScriptChanged(String),
    EditorLoginScriptComboOpened,
    /// One `{placeholder}` value for this host, by variable name.
    EditorLoginScriptVarChanged(String, String),
    /// The credential the script types at the asset's own prompt.
    /// Redacted like every other secret-bearing variant.
    EditorTargetPasswordChanged(super::Redacted),
    EditorToggleTargetPasswordVisibility,
    /// Inline "new script" sub-form: template choice, the three prompt
    /// fields, then create (which saves the entity and selects it) or
    /// cancel.
    EditorScriptDraftTemplateChanged(String),
    EditorScriptDraftNameChanged(String),
    /// One of the draft's three prompt patterns. These carry the text
    /// the bastion PRINTS (`Opt>`, `password:`), never a credential, so
    /// a plain `String` is right here.
    EditorScriptDraftPromptChanged(crate::state::ScriptPromptField, String),
    EditorScriptDraftCreate,
    EditorScriptDraftCancel,
    EditorSave,
    /// Connect using the current editor form WITHOUT persisting anything:
    /// builds an ephemeral quick-connect entry (typed credentials ride in
    /// memory) and dispatches `QuickConnect`. New-host flow only.
    EditorConnectWithoutSaving,
    EditorCancel,
    /// Ask for confirmation before removing a host. Confirming dispatches
    /// `DeleteConnection`. Destructive removals are routed through a confirm
    /// dialog so a stray click can't silently drop a host.
    RequestDeleteConnection(usize),
    /// By id, not index: the action sits behind a confirm dialog, and
    /// the list can re-sort while it is up (an auto-saved rename, a
    /// sync apply), so an index captured at request time could point
    /// at a different host by the time the user confirms.
    DeleteConnection(Uuid),
    DuplicateConnection(usize),
    /// Open the host editor prefilled from the quick-connect entry so the
    /// user can persist it as a regular host.
    SaveQuickHost(Uuid),
    /// Same prefill, but as the temporary-host edit flow (from the
    /// connect progress screen): Connect (without saving) is the primary
    /// footer action, Save the secondary.
    EditQuickHost(Uuid),
    /// Live per-host edits from the Host config sidebar tab. Each mutates
    /// the focused pane's connection, persists immediately, and (for the
    /// theme) repaints the running terminal for instant preview.
    HostConfigThemeChanged(String),
    HostConfigEncodingChanged(String),
    HostConfigAmbiguousWidthChanged(oryxis_core::models::connection::AmbiguousWidth),
    HostConfigTerminalTypeChanged(String),
    HostConfigAutoTitleChanged(String),
    /// Host editor startup-command source changed (the picker label:
    /// the None sentinel, the Custom sentinel, or a snippet label).
    EditorStartupChoiceChanged(String),
    /// The Initial Command / Snippet combo gained focus; clears its
    /// typed value so the dropdown opens on the full list.
    EditorStartupComboOpened,
    /// The SSH Key combo gained focus; clears its typed value so the
    /// dropdown opens on the full list.
    EditorKeyComboOpened,
    EditorIdentityChanged(String),
    EditorAddPortForward,
    EditorRemovePortForward(usize),
    EditorPortFwdLocalPortChanged(usize, String),
    EditorPortFwdRemoteHostChanged(usize, String),
    EditorPortFwdRemotePortChanged(usize, String),
    EditorAddEnvVar,
    EditorRemoveEnvVar(usize),
    EditorEnvVarKeyChanged(usize, String),
    EditorEnvVarValueChanged(usize, String),
    EditorToggleAgentForwarding,
    EditorToggleX11Forwarding,
    EditorToggleMcpEnabled,
    /// SSH > Integration: flip the per-host agentless monitoring opt-in
    /// (issue #83).
    EditorToggleMonitorEnabled,
    /// Switch this host's disk reporting between Auto and Custom
    /// (issue #135). `true` picks Custom.
    EditorMonitorDisksCustom(bool),
    EditorAddMonitorDisk,
    EditorRemoveMonitorDisk(usize),
    EditorMonitorDiskChanged(usize, String),
    /// Cycle the per-host session-recording override: Default -> On -> Off.
    EditorCycleSessionLogging,
    /// Open / close one of the host editor's collapsible sections
    /// (two-tier form). Session-scoped UI state, never persisted.
    EditorSectionToggled(crate::state::HostEditorSection),
    /// A create-flow starting-point chip was clicked (new-host editor
    /// only). One-shot form preparation, see `HostEditorPreset`.
    EditorPresetPicked(crate::state::HostEditorPreset),
}
