//! The full `Message` enum, every event the iced runtime can dispatch
//! to `Oryxis::update`. Pulled out of `app.rs` so the message-loop file
//! is shorter; re-exported via `pub use` at the bottom of `app.rs` so
//! call sites continue to write `crate::app::Message::Foo`.

use std::sync::Arc;

use iced::keyboard;
use iced::widget::text_editor;
use iced::Point;
use uuid::Uuid;

use oryxis_ssh::{ForwardSession, SshSession};
use oryxis_core::models::port_forward_rule::ForwardKind;

use crate::state::{ConnectionStep, SettingsSection, View};

#[derive(Debug, Clone)]
pub enum Message {
    // Vault
    VaultPasswordChanged(String),
    VaultTogglePasswordVisibility,
    VaultUnlock,
    VaultSetup,
    VaultSkipPassword,
    VaultDestroyConfirm,
    VaultDestroy,

    // Navigation
    ChangeView(View),
    QuickHostInput(String),
    QuickHostContinue,
    OpenGroup(Uuid),
    BackToRoot,
    HostSearchChanged(String),
    ToggleSidebar,

    // Tabs
    SelectTab(usize),
    CloseTab(usize),
    TabHovered(usize),
    TabUnhovered,
    ShowNewTabPicker,
    HideNewTabPicker,
    NewTabPickerSearchChanged(String),
    /// Drill into a group in the new-tab picker. For a cloud-query group
    /// this also kicks off (or refreshes) the resolve so the ECS tasks /
    /// K8s pods load. `Uuid` is the group id.
    NewTabPickerOpenGroup(Uuid),
    /// Step back out of a drilled-into group to the top-level picker list.
    NewTabPickerBack,
    ShowTabJump,
    HideTabJump,
    TabJumpSearchChanged(String),
    /// Translate a vertical mouse-wheel delta over the tab bar into a
    /// horizontal scroll on the tab strip. Carries the y-pixel delta;
    /// sign flips for natural-feeling navigation (wheel-down moves
    /// later tabs into view).
    TabBarWheel(f32),
    /// Two-step dispatch: close the modal first, then fire the inner
    /// message (SelectTab, OpenLocalShell, etc). Boxed to keep the enum
    /// variant size from blowing up.
    TabJumpSelect(Box<Message>),
    // Absorb-click sink, used by modal bodies to stop clicks from falling
    // through to the backdrop underneath. Handler is a no-op.
    NoOp,

    // Icon picker (custom host icon/color)
    ShowIconPicker(Uuid),
    HideIconPicker,
    IconPickerSelectIcon(String),
    IconPickerSelectColor(String),
    IconPickerHexInputChanged(String),
    IconPickerSave,
    IconPickerResetAuto,
    // Per-host terminal theme picker (modal opened from the host
    // editor). The form field updates immediately on select; the
    // change is committed on EditorSave like every other form field.
    EditorOpenThemePicker,
    EditorCloseThemePicker,
    /// Empty string == "inherit the global theme".
    EditorTerminalThemeChanged(String),
    /// Cloud transport pick (only meaningful when editing a cloud-imported host).
    EditorCloudTransportChanged(oryxis_core::models::cloud::TransportKind),
    /// Per-host initial command, sent as keystrokes after the shell
    /// opens. Empty = none. Useful for hosts that drop into `/bin/sh`
    /// when you really want `bash`.
    EditorInitialCommandChanged(String),
    /// Set the per-host icon shape override. Empty string clears the
    /// override (falls back to the global `default_host_icon`).
    EditorIconStyleChanged(String),
    EditorEncodingChanged(String),
    /// Empty string == "inherit the global keepalive setting".
    /// "0" == explicitly disabled on this host; any positive integer
    /// is the per-host override in seconds. Sanitized to digits-only.
    EditorKeepaliveChanged(String),
    ShowTabMenu(usize),
    ReconnectTab(usize),
    DuplicateTab(usize),
    DuplicateInNewWindow(usize),

    // SFTP browser
    SftpPickHost(usize),
    SftpHostMounted(String, Arc<SshSession>, oryxis_ssh::SftpClient, String, Vec<oryxis_ssh::SftpEntry>),
    SftpRemoteLoaded(String, Vec<oryxis_ssh::SftpEntry>),
    SftpRemoteError(String),
    SftpNavigateRemote(String),
    SftpRemoteUp,
    SftpNavigateLocal(std::path::PathBuf),
    SftpLocalUp,
    #[allow(dead_code)] // wired by next iteration's Actions menu
    SftpRefreshLocal,
    SftpOpenPicker,
    SftpClosePicker,
    SftpPickerSearch(String),
    SftpToggleLocalHidden,
    SftpToggleRemoteHidden,
    SftpLocalFilter(String),
    SftpRemoteFilter(String),
    SftpToggleLocalActions,
    SftpToggleRemoteActions,
    SftpToggleLocalDrives,
    SftpCloseMenus,
    SftpStartEditLocalPath,
    SftpStartEditRemotePath,
    SftpEditLocalPath(String),
    SftpEditRemotePath(String),
    SftpCommitLocalPath,
    SftpCommitRemotePath,
    #[allow(dead_code)] // wired by upcoming Esc handler
    SftpCancelEditPath,
    SftpSortLocal(crate::state::SftpSortColumn),
    SftpSortRemote(crate::state::SftpSortColumn),

    // Row interactions
    SftpRowRightClick(crate::state::SftpPaneSide, String, bool),
    SftpRowMenuClose,
    SftpStartRename(crate::state::SftpPaneSide, String),
    SftpRenameInput(String),
    SftpRenameCommit,
    SftpAskDelete(crate::state::SftpPaneSide, String, bool),
    SftpAskDeleteSelection,
    SftpConfirmDelete,
    SftpCancelDelete,
    SftpStartNewEntry(crate::state::SftpPaneSide, crate::state::SftpEntryKind),
    SftpNewEntryInput(String),
    SftpNewEntryCommit,
    SftpNewEntryCancel,
    SftpUpload(std::path::PathBuf),
    SftpDownload(String),
    SftpDuplicate(crate::state::SftpPaneSide, String),
    SftpFileHovered,
    SftpFilesHoveredLeft,
    SftpFileDropped(std::path::PathBuf),
    SftpRowEnter(crate::state::SftpPaneSide, String, bool),
    SftpRowExit,
    SftpMouseLeftPressed,
    SftpUploadFolder(std::path::PathBuf),
    SftpDownloadFolder(String),
    SftpDuplicateFolder(crate::state::SftpPaneSide, String),
    SftpSelectRow(crate::state::SftpPaneSide, String, bool),
    SftpStartEdit(String),
    /// Open a local file in the OS default app, no temp copy, no
    /// mtime watch. Edits land on the file directly.
    SftpOpenLocal(std::path::PathBuf),
    /// Open an arbitrary URL in the user's default browser.
    /// Used by clickable links in the About panel.
    OpenUrl(String),
    /// Copy a string to the system clipboard. Used by the Copy
    /// affordance on chat bubbles and code blocks (text-selection
    /// isn't supported by iced's `text` / markdown widgets in 0.14).
    CopyToClipboard(String),
    /// Dismiss the transient toast chip (`Oryxis.toast`). Fired by a
    /// `Task::perform` sleep scheduled when a toast is shown.
    ToastClear,
    /// Dismiss the blocking error dialog (`Oryxis.error_dialog`). Fired
    /// by the OK button or by clicking the scrim.
    ErrorDialogDismiss,
    SftpEditReady(crate::state::EditSession),
    SftpEditSave,
    SftpEditDiscard,
    SftpEditWatchTick,
    SftpCancelRemoteLoad,
    /// Retry the last failed remote action, either re-list the
    /// current path (if a session is still mounted) or re-run the
    /// full host-pick flow (if the connect itself failed).
    SftpRetryRemote,
    SftpShowProperties(crate::state::SftpPaneSide, String, bool),
    SftpPropertiesLoaded(crate::state::PropertiesView),
    SftpPropertiesToggleBit(crate::state::PermBit),
    SftpPropertiesApply,
    SftpPropertiesDone(Result<(), String>),
    SftpPropertiesClose,
    SftpAskOverwrite(crate::state::OverwritePrompt),
    SftpResolveOverwrite(crate::state::OverwriteAction),
    SftpToggleApplyToAll,
    SftpUploadBatch(Vec<std::path::PathBuf>),
    SftpUploadSelection,
    SftpDownloadSelection,
    SftpDuplicateSelection,
    SftpTransferConflict(crate::state::OverwritePrompt, crate::state::TransferItem, u8),
    SftpTransferQueueReady(crate::state::TransferState),
    /// Pop one item and dispatch to whichever slot is free. The Next
    /// handler picks the slot itself instead of carrying it in the
    /// message, that way pause/resume can spawn fresh chains without
    /// having to remember which slot was on which client.
    SftpTransferNext,
    /// Slot freed up after a queue item completed successfully.
    SftpTransferItemDone(u8),
    SftpTransferError(String, u8),
    SftpCancelTransfer,
    SftpOpResult(String, bool),

    // Folder (group) actions
    ShowFolderActions(Uuid),
    StartRenameFolder(Uuid),
    FolderRenameInput(String),
    ConfirmRenameFolder,
    CancelFolderModal,
    StartDeleteFolder(Uuid),
    DeleteFolderKeepHosts,
    DeleteFolderWithHosts,
    CloseOtherTabs(usize),
    CloseAllTabs,

    // Terminal I/O
    PtyOutput(Uuid, Vec<u8>),  // (pane_id, bytes)
    KeyboardEvent(keyboard::Event),
    MouseMoved(Point),
    WindowResized(iced::Size),
    /// OS window gained (`true`) or lost (`false`) focus. Gates the
    /// cloud SSM/ECS keepalive ticker: it only runs while unfocused.
    WindowFocusChanged(bool),
    /// Periodic tick (mounted only while the window is unfocused and at
    /// least one SSM/ECS tab is open) that nudges those tabs' terminal
    /// size so the SSM idle timer resets and a long alt-tab away doesn't
    /// drop the session.
    SsmKeepaliveTick,
    WindowDrag,
    WindowResizeDrag(iced::window::Direction),
    /// Double-click on a N/S edge, fill the full monitor height while
    /// keeping horizontal position and width.
    WindowExpandVertical,
    WindowMinimize,
    WindowMaximizeToggle,
    WindowFullscreenToggle,
    /// Clears the "Press F11 to exit fullscreen" banner. Fired by a
    /// timed `Task::perform` 3 s after entering fullscreen.
    FullscreenHintHide,
    /// Settings → Shortcuts: enter capture mode for an action. The
    /// next non-Esc, non-pure-modifier `KeyPressed` becomes the new
    /// binding (see `shortcuts::handle_hotkey_capture`).
    StartEditingHotkey(crate::hotkeys::HotkeyAction),
    /// Settings → Shortcuts: drop a single action's user override and
    /// fall back to the factory default.
    ResetHotkey(crate::hotkeys::HotkeyAction),
    /// Settings → Shortcuts: drop every user override.
    ResetAllHotkeys,
    WindowClose,
    /// Spawn a fresh top-level Oryxis window without binding to any
    /// existing tab. Triggered by Ctrl+Shift+N and the burger menu's
    /// "New Window" entry. Inherits the vault master password the
    /// same way `DuplicateInNewWindow` does.
    SpawnNewWindow,
    /// Focus the current view's primary search/filter input. Triggered
    /// by Ctrl+F outside the terminal. No-op when the active view has
    /// no search field (Snippets, Settings, History).
    FocusViewSearch,
    /// Activate the Nth slot of the visual tab strip (0-indexed). In
    /// Workspace mode slot 0 is Hosts, slot 1 is SFTP (when enabled),
    /// followed by terminal tabs. In Classic mode the strip only
    /// holds terminal tabs. Out-of-range slots are no-ops.
    ActivateStripSlot(usize),

    // Overlay
    HideOverlayMenu,

    // Card interactions
    CardHovered(usize),
    CardUnhovered,
    FolderCardHovered(Uuid),
    FolderCardUnhovered,
    KeyCardHovered(usize),
    KeyCardUnhovered,
    IdentityCardHovered(usize),
    SnippetCardHovered(usize),
    SnippetCardUnhovered,
    IdentityCardUnhovered,
    ShowCardMenu(usize),
    #[allow(dead_code)]
    HideCardMenu,

    // Connection editor
    ShowNewConnection,
    EditConnection(usize),
    EditorLabelChanged(String),
    EditorHostnameChanged(String),
    EditorPortChanged(String),
    EditorUsernameChanged(String),
    EditorPasswordChanged(String),
    EditorAuthMethodChanged(String),
    EditorGroupChanged(String),
    EditorKeyChanged(String),
    EditorJumpHostChanged(String),
    // Jump-host picker modal variants, wired in a follow-up PR. Mark
    // the trio as allowed-dead-code so the workspace clippy gate
    // doesn't fail while the dispatch path is still being built out.
    #[allow(dead_code)]
    OpenJumpHostPicker,
    #[allow(dead_code)]
    HideJumpHostPicker,
    #[allow(dead_code)]
    JumpHostSearchChanged(String),
    EditorProxyKindChanged(crate::state::ProxyKind),
    EditorProxyHostChanged(String),
    EditorProxyPortChanged(String),
    EditorProxyUsernameChanged(String),
    EditorProxyPasswordChanged(String),
    EditorProxyCommandChanged(String),
    EditorTogglePasswordVisibility,
    EditorSave,
    EditorCancel,
    DeleteConnection(usize),
    DuplicateConnection(usize),

    // SSH
    ConnectSsh(usize),
    SshProgress(ConnectionStep, String),
    SshConnected(Uuid, Arc<SshSession>),  // (pane_id, session)
    SshNewKnownHosts(Vec<oryxis_core::models::known_host::KnownHost>),
    SshDisconnected(Uuid),  // (pane_id)
    SshError(String),
    SshHostKeyVerify(oryxis_ssh::HostKeyQuery),
    SshHostKeyReject,
    SshHostKeyContinue,
    SshHostKeyAcceptAndSave,
    SshCloseProgress,
    SshEditFromProgress,
    SshRetry,

    // Snippets
    ShowSnippetPanel,
    HideSnippetPanel,
    SnippetLabelChanged(String),
    SnippetCommandAction(text_editor::Action),
    SaveSnippet,
    EditSnippet(usize),
    DeleteSnippet(usize),
    RunSnippet(usize),
    /// Inject a snippet's command into the active terminal WITHOUT a
    /// trailing newline (the user presses Enter), unlike `RunSnippet`.
    PasteSnippet(usize),
    /// Built-in "global snippet": type the active host's stored password
    /// then Enter into the terminal (e.g. to answer a sudo prompt). No-op
    /// with a toast when the host has no stored password.
    ApplySudoPassword,

    // Split panes
    /// Focus a pane (click). Routes keyboard / snippets / paste to it.
    FocusPane(iced::widget::pane_grid::Pane),
    /// Drag a pane divider to resize.
    ResizePane(iced::widget::pane_grid::ResizeEvent),
    /// Split the focused pane of the active tab along an axis, opening the
    /// connection picker to fill the new pane.
    SplitPane(iced::widget::pane_grid::Axis),
    /// Like `SplitPane` but targets a specific tab (from its right-click
    /// menu), so it works even when that tab isn't the active one.
    SplitTabPane(usize, iced::widget::pane_grid::Axis),
    /// Hover entered the `+` button: reveal the New-Tab / Split popover.
    /// No-op unless a terminal tab is open.
    ShowSplitMenu,
    /// Cursor entered the popover itself (keeps it open across the bridge).
    SplitMenuEnter,
    /// Cursor left the `+` button or the popover: schedule a close.
    SplitMenuLeave,
    /// Delayed close: hide the popover unless the cursor came back to it.
    SplitMenuCloseIfIdle,
    /// Close the focused pane (closes the tab if it was the last one).
    ClosePane,
    /// Move focus to the adjacent pane in a direction (keyboard nav).
    FocusPaneDir(iced::widget::pane_grid::Direction),
    /// Picker "Local Shell" entry. Opens a local shell, into a split pane
    /// when `pending_pane_split` is set, otherwise a new tab.
    PickLocalShell,
    /// A pane's SSH connect failed; surface the error inside the pane.
    PaneConnectError(Uuid, String),

    // Custom terminal themes (Settings -> Themes)
    /// Open the editor for a brand new custom theme.
    ThemeEditorNew,
    /// Open the editor for the custom theme at this index.
    ThemeEditorEdit(usize),
    /// Close the editor without saving.
    ThemeEditorClose,
    ThemeEditorNameChanged(String),
    /// A color slot's hex value changed (live).
    ThemeEditorColorChanged(crate::state::ThemeColorSlot, String),
    /// Save the in-progress theme (insert or update) + repaint.
    ThemeEditorSave,
    /// Delete the custom theme at this index.
    ThemeDelete(usize),
    /// Import-theme modal (paste an iTerm / Windows Terminal / base16 scheme).
    ThemeImportOpen,
    ThemeImportClose,
    ThemeImportContentAction(text_editor::Action),
    ThemeImportNameChanged(String),
    /// Parse the pasted scheme; on success open it in the editor for review.
    ThemeImportApply,
    /// Hover tracking for the floating edit / delete icons on a custom
    /// theme card.
    ThemeCardHovered(usize),
    ThemeCardUnhovered,
    /// Open the compact color-picker popover for a slot (anchored at the
    /// cursor).
    ThemeEditorOpenPicker(crate::state::ThemeColorSlot),
    /// Close the color-picker popover.
    ThemeEditorClosePicker,

    // Port forwards (standalone entity)
    ShowPortForwardPanel,
    HidePortForwardPanel,
    PfLabelChanged(String),
    PfKindChanged(ForwardKind),
    PfHostChanged(Uuid),
    PfListenHostChanged(String),
    PfListenPortChanged(String),
    PfTargetHostChanged(String),
    PfTargetPortChanged(String),
    PfAutoStartToggled(bool),
    SavePortForwardRule,
    EditPortForwardRule(usize),
    DeletePortForwardRule(usize),
    /// Toggle a rule on: opens a dedicated PTY-less SSH session.
    StartPortForward(Uuid),
    /// Toggle a rule off: drops its `ForwardSession` (cancels the tunnel).
    StopPortForward(Uuid),
    /// Result of a `StartPortForward` connect attempt.
    PortForwardStarted(Uuid, Result<Arc<ForwardSession>, String>),
    /// Periodic liveness sweep; drops forwards whose connection died.
    PortForwardLivenessTick,
    PortForwardCardHovered(usize),
    PortForwardCardUnhovered,
    PortForwardSearchChanged(String),

    // Terminal side panel (Chat / Snippets / History tabs)
    SelectTerminalSidebarTab(crate::state::TerminalSidebarTab),
    SidebarSnippetSearchChanged(String),
    /// Toggle the Snippets-tab sort popover.
    ToggleSidebarSort,
    /// Toggle the Snippets-tab search field (autofocuses on open, clears
    /// the needle on close).
    ToggleSidebarSearch,

    // Known hosts
    DeleteKnownHost(usize),
    ClearAllKnownHosts,

    // History
    ClearLogs,
    LogsPageNext,
    LogsPagePrev,

    // Session logs
    ViewSessionLog(Uuid),
    CloseSessionLogView,
    DeleteSessionLog(usize),
    // History was split in v0.6 (logs + session logs in two panes
    // with independent pagination); v0.7 merges both into one timeline
    // so the per-section Clear / Next / Prev controls don't render
    // anymore. Handlers stay wired so we can resurrect a dedicated
    // session-logs surface without re-introducing the messages.
    #[allow(dead_code)]
    ClearSessionLogs,
    #[allow(dead_code)]
    SessionLogsPageNext,
    #[allow(dead_code)]
    SessionLogsPagePrev,

    // Settings
    LockVault,
    #[allow(dead_code)]
    TerminalThemeChanged(String),
    AppThemeChanged(String),
    TerminalFontSizeIncrease,
    TerminalFontSizeDecrease,
    TerminalFontChanged(String),
    /// Emitted by the terminal widget when the user right-clicks. The
    /// dispatcher reads the clipboard and routes the text to the SSH
    /// session (if active) or the local PTY, mirroring Ctrl+Shift+V.
    TerminalPasteFromClipboard,
    /// Raw input bytes synthesized by the terminal widget (mouse-tracking
    /// reports, wheel-to-arrow translation). Routed to the active SSH
    /// session, falling back to the local PTY.
    TerminalInput(Vec<u8>),
    /// Settings: switch the auto-update release channel (stable/nightly).
    SettingUpdateChannelChanged(crate::update::UpdateChannel),
    ChangeSettingsSection(SettingsSection),
    /// Pick the renderer backend ("auto" / "opengl" / "software").
    /// Persisted to the vault; takes effect on the next launch (the
    /// backend is fixed at startup via WGPU_BACKEND / ICED_BACKEND).
    SettingRendererBackendChanged(String),
    ToggleCopyOnSelect,
    ToggleRightClickCopy,
    ToggleBoldIsBright,
    ToggleKeywordHighlight,
    ToggleSmartContrast,
    SettingToggleShowStatusBar,
    SettingToggleCloseToTray,
    SettingToggleMinimizeToTray,
    SettingToggleTabAccentLine,
    SettingTabCloseButtonSideChanged(String),
    SettingToggleShowTabStatusDot,
    /// Show/hide the top-left burger menu (Settings / Updates / About /
    /// Exit). Mirrors Termius's `☰` strip at the start of the tab bar.
    ToggleBurgerMenu,
    SettingToggleSftpEnabled,
    SettingLayoutModeChanged(String),
    SettingDefaultHostIconChanged(String),
    SettingKeepaliveChanged(String),
    SettingScrollbackChanged(String),
    SettingSftpConcurrencyChanged(String),
    SettingSftpConnectTimeoutChanged(String),
    SettingSftpAuthTimeoutChanged(String),
    SettingSftpSessionTimeoutChanged(String),
    SettingSftpOpTimeoutChanged(String),
    SettingToggleAutoReconnect,
    SettingMaxReconnectChanged(String),
    SettingToggleOsDetection,
    OsDetected(Uuid, Option<String>),
    SettingToggleAutoCheckUpdates,

    // Auto-update
    CheckForUpdate,
    CheckForUpdateManual,
    UpdateCheckResult(Option<crate::update::UpdateInfo>),
    UpdateSkipVersion,
    UpdateLater,
    UpdateStartDownload,
    #[allow(dead_code)]
    UpdateDownloadProgress(f32),
    UpdateDownloadComplete(Result<std::path::PathBuf, String>),
    UpdateOpenRelease,
    AutoReconnectTick,
    ConnectAnimTick,

    // Language
    LanguageChanged(String),
    /// User picked a layout-direction option (Auto / LTR / RTL).
    /// The string is the localized label shown in the picker; the
    /// dispatch handler maps it back to a `LayoutDirection` value.
    LayoutDirectionChanged(String),
    FlattenHostsToggle,

    // Local shell
    OpenLocalShell,
    /// Show the Local Shell picker overlay (Windows: cmd / PowerShell
    /// / WSL distros). On non-Windows platforms `OpenLocalShell` skips
    /// this and spawns the default directly.
    ShowLocalShellPicker,
    /// Result of the async shell-detection probe, `where.exe pwsh` +
    /// `wsl --list --quiet`. Lands in the message loop so we don't
    /// stall the UI thread on a cold WSL host.
    LocalShellsDetected(Vec<crate::state::LocalShellSpec>),
    /// Dismiss the picker overlay (clicking outside or Escape).
    HideLocalShellPicker,
    /// Spawn a specific local shell, `(program, args, label)`
    /// produced by clicking a row in the picker.
    OpenLocalShellWith {
        program: String,
        args: Vec<String>,
        label: String,
    },

    // Keys
    ShowKeyPanel,
    HideKeyPanel,
    KeyImportLabelChanged(String),
    KeyContentAction(text_editor::Action),
    BrowseKeyFile,
    KeyFileLoaded(String, String), // (filename, content)
    KeyFileBrowseError(String),
    KeyImportPassphraseChanged(String),
    KeyImportPassphraseToggleVisibility,
    ImportKey,
    DeleteKey(usize),
    ShowKeyMenu(usize),
    #[allow(dead_code)]
    HideKeyMenu,
    EditKey(usize),
    KeySearchChanged(String),
    /// Workspace sub-nav search input wired to Snippets view.
    SnippetSearchChanged(String),
    /// Workspace sub-nav search input wired to History view.
    HistorySearchChanged(String),

    // Identities
    ShowIdentityPanel,
    HideIdentityPanel,
    IdentityLabelChanged(String),
    IdentityUsernameChanged(String),
    IdentityPasswordChanged(String),
    IdentityKeyChanged(String),
    IdentityTogglePasswordVisibility,
    SaveIdentity,
    EditIdentity(usize),
    DeleteIdentity(usize),
    ShowIdentityMenu(usize),
    ToggleKeychainAddMenu,

    // Per-list sort menus (Hosts / Keychain / Snippets toolbars).
    // The Toggle* messages open/close the dropdown anchored to the
    // toolbar sort button; the Set* messages pick a sort mode and
    // persist it via the matching `*_sort` settings key.
    ToggleSortMenu(crate::state::SortMenuKind),
    SetListSort(crate::state::SortMenuKind, crate::state::ListSort),

    // Proxy Identities (Settings → Proxies)
    ShowProxyIdentityForm(Option<Uuid>),
    HideProxyIdentityForm,
    ProxyIdentityFormLabelChanged(String),
    ProxyIdentityFormKindChanged(crate::state::ProxyKind),
    ProxyIdentityFormHostChanged(String),
    ProxyIdentityFormPortChanged(String),
    ProxyIdentityFormUsernameChanged(String),
    ProxyIdentityFormPasswordChanged(String),
    SaveProxyIdentity,
    DeleteProxyIdentity(Uuid),

    // Cloud Accounts
    ShowCloudForm(Option<Uuid>),
    HideCloudForm,
    CloudFormLabelChanged(String),
    CloudFormProviderChanged(crate::state::CloudProviderChoice),
    CloudFormAuthKindChanged(crate::state::CloudAuthChoice),
    CloudFormAwsProfileNameChanged(String),
    CloudFormAwsRegionDraftChanged(String),
    /// Commit the current draft to the regions chip list. Supports
    /// comma or whitespace separated input so paste-multiple works.
    CloudFormAwsRegionAdd,
    CloudFormAwsRegionRemove(usize),
    CloudFormAwsAccessKeyIdChanged(String),
    CloudFormAwsAccessKeySecretChanged(String),
    CloudFormAwsAccessKeySessionTokenChanged(String),
    // Wired to a future "show password" eye icon next to the secret
    // input, `text_input.secure(false)` flips when this fires.
    #[allow(dead_code)]
    CloudFormAwsAccessKeySecretToggleVisibility,
    CloudFormAwsSsoStartUrlChanged(String),
    CloudFormAwsSsoRegionChanged(String),
    CloudFormAwsSsoAccountIdChanged(String),
    CloudFormAwsSsoRoleNameChanged(String),
    /// Kubernetes (Kubeconfig) auth fields.
    CloudFormKubeconfigPathChanged(String),
    CloudFormContextChanged(String),
    /// Kicks off a `test_credentials` round-trip via the registered
    /// provider. The result lands as `CloudFormTestResult`.
    CloudFormTestCredentials,
    CloudFormTestResult(Result<(), String>),
    SaveCloudProfile,
    DeleteCloudProfile(Uuid),
    /// Open the kebab context menu on a cloud account card. Anchored
    /// to the cursor like the host-card menu.
    ShowCloudCardMenu(Uuid),
    CloudCardHovered(Uuid),
    CloudCardUnhovered,
    /// Open the cloud-provider picker dropdown next to the "+ Host"
    /// button (only when at least one cloud profile is configured).
    ShowCloudProviderPicker,

    // Cloud Discovery & Import
    ShowCloudDiscover(Uuid),
    HideCloudDiscover,
    CloudDiscoverRefresh,
    /// Result of `provider.discover()`, payload boxed because
    /// `DiscoveryResult` carries collections per resource family and
    /// clippy yells about the variant size otherwise.
    CloudDiscoverResult(Result<Box<oryxis_cloud::DiscoveryResult>, String>),
    CloudDiscoverToggleEc2(String),
    /// Toggle an ECS service entry in the discovery panel. Carries
    /// the `cluster/service/container` key.
    CloudDiscoverToggleEcs(String),
    /// Toggle a discovered K8s workload (`namespace/kind/name`).
    CloudDiscoverToggleK8s(String),
    CloudDiscoverImport,
    /// Triggered from the transport-confirmation modal: actually run
    /// the import using the picked default transport.
    CloudDiscoverImportConfirmed,
    /// Close the transport-confirmation modal without importing.
    CloudDiscoverImportCancelled,
    CloudDiscoverFilterChanged(String),
    /// Toggle expanded/collapsed state of a section header in the
    /// discovery panel. Carries the section key (e.g. `"ec2"`).
    CloudDiscoverToggleSection(String),
    CloudDiscoverDefaultTransportChanged(oryxis_core::models::cloud::TransportKind),
    CloudDiscoverDefaultGroupNameChanged(String),
    CloudDiscoverDefaultGroupPick(String),
    /// Open / close the shared group picker for a side-panel parent
    /// group input. Anchors the popover at the matching combo's
    /// measured bounds (`editor_parent_combo_bounds` or
    /// `dynamic_form_parent_combo_bounds`).
    ToggleGroupPicker(crate::state::GroupPickerTarget),
    /// Live filter for the shared group-picker popover.
    GroupPickerSearchChanged(String),
    /// Route a pick into the matching form field and close the
    /// popover. Existing field-change messages (`EditorGroupChanged`,
    /// `DynamicGroupFormParentChanged`) still drive the write.
    GroupPickerPick(crate::state::GroupPickerTarget, String),
    /// Toggle the floating group-picker overlay rendered at the top
    /// of the Discover import modal. Independent of the global
    /// OverlayState so it can sit on top of the modal scrim.
    ToggleCloudDiscoverGroupPicker,
    /// Live filter typed inside the group-picker overlay's own
    /// search field. Doesn't affect the main "Import into" input.
    CloudDiscoverDefaultGroupPickerSearchChanged(String),
    /// Apply / clear the dashboard cloud-profile filter. Passing None
    /// clears it; passing Some(pid) restricts the grid to items whose
    /// cloud origin matches that profile.
    HostFilterByCloudProfile(Option<Uuid>),
    /// Manual sync of a cloud profile, re-runs discovery and updates
    /// every already-imported host whose `cloud_ref.profile_id` matches.
    /// Fields the user has flagged in `customized_fields` are preserved.
    /// Hosts not in the upstream result get their `cloud_ref.orphaned_at`
    /// set; hosts that come back get it cleared.
    CloudProfileSync(Uuid),
    CloudProfileSyncResult(Uuid, Result<Box<oryxis_cloud::DiscoveryResult>, String>),
    SettingCloudAutoRefreshToggle,
    SettingCloudAutoRefreshIntervalChanged(String),
    SettingCloudAutoArchiveToggle,
    SettingCloudOrphanArchiveDaysChanged(String),
    /// Fired by the iced subscription when the auto-refresh interval
    /// elapses. Iterates every cloud profile and dispatches a
    /// `CloudProfileSync(pid)` for each.
    CloudAutoRefreshTick,
    DynamicGroupFormLabelChanged(String),
    DynamicGroupFormParentChanged(String),
    DynamicGroupFormClusterChanged(String),
    DynamicGroupFormServiceChanged(String),
    DynamicGroupFormContainerChanged(String),
    /// K8s dynamic-group source fields (context / namespace / selector
    /// kind + value).
    DynamicGroupFormK8sContextChanged(String),
    DynamicGroupFormNamespaceChanged(String),
    DynamicGroupFormK8sSelectorKindChanged(crate::state::K8sSelectorKind),
    DynamicGroupFormK8sSelectorValueChanged(String),
    /// Open the shared icon + color picker pre-filled with the current
    /// dynamic-group form values. On Save the picker writes back to the
    /// form (not directly to the vault) so the deferred Save button on
    /// the form panel still controls when the group is persisted.
    ShowIconPickerForDynamicGroupForm,
    /// Kick off `provider.resolve_query()` for a dynamic group. The
    /// async result lands as `DynamicGroupResolved`. Idempotent
    /// safe to dispatch even if a resolve is already running for the
    /// same group; the dashboard handler dedupes.
    DynamicGroupResolve(Uuid),
    /// User clicked a task row inside an open dynamic group. Carries
    /// the group id (so we can find the cloud_query) and the task's
    /// `resource_id` (the task ARN suffix). Triggers ECS Exec.
    ConnectEcsExecTask {
        group_id: Uuid,
        task_id: String,
        task_label: String,
        /// Specific container to exec into. Required because under
        /// wildcard queries (empty `container` in `cloud_query`) the
        /// row knows which container the user actually clicked while
        /// the query itself doesn't pin one. Always populated from
        /// the row's `DiscoveredHost.container_name`.
        container: String,
    },
    /// Open an interactive shell in a Kubernetes pod by spawning
    /// `kubectl exec -it` in a local PTY. No provider round-trip; the
    /// dispatch builds the kubectl args from the group's profile + query.
    ConnectKubectlExecPod {
        group_id: Uuid,
        namespace: String,
        pod: String,
        /// Container to exec into, empty = the pod's default (kubectl
        /// picks the first container).
        container: String,
    },
    /// Result of `ecs:ExecuteCommand` + plugin invocation prep. On
    /// success the dispatch spawns the plugin and opens a tab; on
    /// error it's surfaced in the UI.
    EcsExecSessionReady {
        /// Group the task belongs to. Carried so the error arm can
        /// re-resolve the dynamic group's list: a failed connect on a
        /// recycled task means the cached list is stale, refreshing it
        /// surfaces the live task without a manual Refresh click.
        group_id: Uuid,
        task_label: String,
        /// Task id + container the session targets. Carried so the
        /// spawn handler can rebuild a `ConnectEcsExecTask` and stash
        /// it on the tab as its relaunch message (used by Duplicate Tab,
        /// ECS tabs have no saved `Connection` to look up by label).
        task_id: String,
        container: String,
        result: Result<Box<oryxis_cloud::SessionPayload>, String>,
    },
    /// SSM Session result, same plugin payload shape as ECS Exec, so
    /// we reuse the spawn path. Carries the host's display label so
    /// the spawned tab gets a useful title.
    SsmSessionReady {
        host_label: String,
        result: Result<Box<oryxis_cloud::SessionPayload>, String>,
    },
    DynamicGroupResolved(Uuid, Result<Vec<oryxis_cloud::DiscoveredHost>, String>),

    // Plugins panel, cloud-provider plugin install / update lifecycle.
    /// Global auto-update toggle (applies to every plugin without an
    /// explicit per-plugin override).
    PluginToggleGlobalAutoUpdate(bool),
    /// Per-plugin auto-update override.
    PluginToggleAutoUpdate(String, bool),
    /// Fetch the hosted manifest for a provider and compare against
    /// the installed version.
    PluginCheckUpdates(String),
    /// Manifest fetch finished, `Ok` carries the parsed manifest.
    PluginManifestFetched(String, Result<Box<crate::plugins::PluginManifest>, String>),
    /// Open / close the first-use install opt-in modal for a provider.
    ShowPluginInstallModal(String),
    HidePluginInstallModal,
    /// Begin downloading + installing the best compatible version.
    PluginInstall(String),
    /// Install finished, `Ok` carries the installed version string.
    PluginInstallDone(String, Result<String, String>),
    /// Remove a provider's cached binaries.
    PluginUninstall(String),

    // Edit dynamic group panel, sets template fields (key, identity,
    // transport, initial command) on a `Group.cloud_query`.
    EditDynamicGroup(Uuid),
    HideDynamicGroupForm,
    DynamicGroupFormUsernameChanged(String),
    DynamicGroupFormInitialCommandChanged(String),
    DynamicGroupFormTransportChanged(oryxis_core::models::cloud::TransportKind),
    DynamicGroupFormKeyChanged(String),
    DynamicGroupFormIdentityChanged(String),
    SaveDynamicGroup,
    DeleteDynamicGroup(Uuid),
    /// ⋮ menu on a dynamic-group card.
    ShowDynamicGroupCardMenu(Uuid),
    DynamicGroupCardHovered(Uuid),
    DynamicGroupCardUnhovered,

    // Connection identity
    EditorIdentityChanged(String),

    // AI settings
    ToggleAiEnabled,
    AiProviderChanged(String),
    AiModelChanged(String),
    AiApiKeyChanged(String),
    AiApiUrlChanged(String),
    AiSystemPromptAction(text_editor::Action),
    SaveAiApiKey,

    // Vault password management
    ToggleVaultPassword,
    VaultNewPasswordChanged(String),
    SetVaultPassword,

    // AI chat sidebar
    ToggleChatSidebar,
    ChatInputAction(text_editor::Action),
    ChatScrolled(f32),
    ChatResetConversation,
    ChatSidebarResizeStart,
    ChatSidebarResizeStop,
    SendChat,
    /// Incremental text delta from the streaming AI response. Appended
    /// to the active assistant bubble so the user sees tokens land as
    /// they're generated.
    ChatStreamChunk(String),
    /// Terminal sentinel for `ChatStreamChunk`, clears the loading
    /// state and finalises the message (markdown re-parse, scroll snap).
    ChatStreamDone,
    ChatError(String),
    /// Re-send the last user message, used by the Retry button on an
    /// error bubble. Pops the most recent error and replays.
    ChatRetry,
    ChatToolExec(String),
    /// AI proposed a tool call. Carries the command + `risk` it
    /// self-classified ("safe" / "risky"). Safe commands still have to
    /// clear the independent auto-exec judge before running unattended;
    /// risky ones (and ones the model failed to classify) are queued as
    /// a `PendingTool` bubble with RUN / ALWAYS RUN / DENY buttons.
    ChatToolProposed { command: String, risk: String },
    /// The independent safety judge declined to auto-run a model-claimed
    /// `safe` command. Surface it for explicit approval like a risky one.
    ChatToolGuardBlocked { command: String },
    /// User clicked RUN on a pending tool prompt, execute once.
    ChatToolApprove(String),
    /// User clicked ALWAYS RUN, add this command's first token to the
    /// tab's allow-list and execute now.
    ChatToolApproveAlways(String),
    /// User clicked DENY on a pending tool prompt, drop the bubble,
    /// don't run anything, don't notify the model.
    ChatToolDeny(String),
    #[allow(dead_code)]
    ChatToolResult(String),

    // Port forwarding
    EditorAddPortForward,
    EditorRemovePortForward(usize),
    EditorPortFwdLocalPortChanged(usize, String),
    EditorPortFwdRemoteHostChanged(usize, String),
    EditorPortFwdRemotePortChanged(usize, String),
    EditorAddEnvVar,
    EditorRemoveEnvVar(usize),
    EditorEnvVarKeyChanged(usize, String),
    EditorEnvVarValueChanged(usize, String),

    // SSH agent forwarding (per-host opt-in)
    EditorToggleAgentForwarding,

    // MCP
    EditorToggleMcpEnabled,
    ToggleMcpServer,
    ShowMcpInfo,
    HideMcpInfo,
    CopyMcpConfig,
    InstallMcpConfig,
    InstallMcpConfigResult(Result<String, String>),
    /// Pick which client the snippet, Copy, and Install target: the
    /// native client (`false`) or one running inside WSL (`true`).
    /// Only the Windows build renders the toggle that emits this, so
    /// elsewhere the variant is constructed nowhere.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    SetMcpTarget(bool),
    /// Generate a fresh random MCP server token and persist it. Wipes
    /// the previous value, every existing MCP config will need to be
    /// reissued (re-copy / re-install) with the new token.
    RegenerateMcpToken,
    /// Show / hide the MCP token in plain text on the settings panel.
    /// Default masked.
    ToggleMcpTokenVisibility,
    /// Put the active MCP token on the clipboard. Logs nothing, the
    /// toast tells the user it was copied.
    CopyMcpToken,

    // Sync
    SyncToggleEnabled,
    SyncTogglePasswords,
    SyncModeChanged(String),
    SyncDeviceNameChanged(String),
    SyncSignalingUrlChanged(String),
    /// Bearer token text-input change. Persisted to the vault settings
    /// table; an empty string leaves the request without an
    /// `Authorization` header (fine for unauthenticated signaling).
    SyncSignalingTokenChanged(String),
    SyncRelayUrlChanged(String),
    SyncListenPortChanged(String),
    SyncStartPairing,
    SyncUnpairDevice(uuid::Uuid),
    SyncNow,
    /// Top-level result of a manual `SyncNow`. Per-peer outcomes arrive
    /// separately as `SyncEngineEvent`s; this only carries a vault-level
    /// failure (e.g. the lock could not be taken).
    SyncNowFinished(Result<(), String>),
    /// An event emitted by the running `SyncEngine` (peer discovered,
    /// sync completed, pairing progress, ...), pumped in from the
    /// engine's event channel via `Task::stream`.
    SyncEngineEvent(oryxis_sync::SyncEvent),
    /// Stop hosting the pairing code and return to the idle pairing view.
    SyncCancelHostingPairing,
    /// Switch the pairing panel into "join with a code" mode.
    SyncJoinPairingRequested,
    /// Text-input change for the joiner's 6-digit code field.
    SyncJoinCodeChanged(String),
    /// Text-input change for the joiner's `ip:port` host-address field.
    SyncJoinTargetChanged(String),
    /// Joiner pressed Connect: dial the entered address with the code.
    SyncJoinPairingConnect,
    /// Joiner backed out of the join form, return to the idle view.
    SyncJoinPairingCancel,
    /// Text-input change for the joiner's `oryxis://pair/...` link
    /// field (the cross-network alternative to code + address).
    SyncJoinLinkChanged(String),
    /// Joiner pressed Connect with link: parse the link, look the
    /// device id up on the signaling server, run the handshake.
    SyncJoinPairingByLink,
    /// User clicked Pair on a row in the live discovered-devices
    /// list. Switches to the Joining sub-view and pre-fills the
    /// host-address field with the discovered peer's `ip:port`.
    SyncPairWithDiscovered(uuid::Uuid),
    /// Abort the in-flight `Sync Now` Task. Fires the oneshot the
    /// task is racing against; the task lands back as
    /// `SyncNowFinished(Err("Cancelled"))` and clears the flags.
    SyncCancelInProgress,

    // Export / Import
    ExportVault,
    ExportPasswordChanged(String),
    ExportToggleKeys,
    ExportConfirm,
    #[allow(dead_code)]
    ExportCompleted(Result<String, String>),
    ImportVault,
    /// Pick `~/.ssh/config` (or any file the user chooses), parse Host
    /// blocks, and add each as a new connection record. No preview
    /// modal yet, batch-imports everything non-wildcard and shows a
    /// status banner.
    ImportSshConfig,
    #[allow(dead_code)]
    ImportFileLoaded(Vec<u8>),
    ImportPasswordChanged(String),
    ImportConfirm,
    #[allow(dead_code)]
    ImportCompleted(Result<String, String>),
    ExportImportDismiss,

    // System tray (Windows only at runtime; messages compile on
    // every platform so dispatch.rs and subscription.rs stay cfg-
    // free).
    /// 100 ms ticker emitted by the iced subscription. The handler
    /// drains the tray-icon crate's crossbeam event channels and
    /// re-emits real `TrayShow / TrayHide / TrayQuit` messages.
    /// Polling here is acceptable noise (~10 ticks/sec, each a
    /// non-blocking `try_recv`) and avoids wiring a custom
    /// Subscription stream that bridges crossbeam into iced.
    TrayPoll,
    /// User clicked "Show Oryxis" in the tray menu, or left-clicked
    /// the tray icon. Bring the main window back from hidden state
    /// and pull it to the foreground.
    TrayShow,
    /// User clicked "Hide to tray". Hide the main window (true
    /// hide via Win32 ShowWindow, not just minimize) and leave
    /// only the tray icon present.
    TrayHide,
    /// User clicked "Quit" in the tray menu. Tear down the tray
    /// icon and exit the process.
    TrayQuit,
    /// User clicked an entry in the tray menu's "Active sessions"
    /// section. Payload is the tab index from `Oryxis::tabs`. The
    /// handler shows the window (in case it was hidden) and selects
    /// the tab.
    TrayActivateSession(usize),
    /// User clicked an entry in the tray menu's "Recent hosts"
    /// section. Payload is the connection UUID. The handler shows
    /// the window and opens a new tab against that connection.
    TrayOpenHost(uuid::Uuid),

    // Share
    ShareConnection(usize),
    #[allow(dead_code)]
    ShareGroup(uuid::Uuid),
    SharePasswordChanged(String),
    ShareToggleKeys,
    ShareConfirm,
    ShareDismiss,
}
