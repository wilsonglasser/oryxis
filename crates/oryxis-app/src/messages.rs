//! The full `Message` enum — every event the iced runtime can dispatch
//! to `Oryxis::update`. Pulled out of `app.rs` so the message-loop file
//! is shorter; re-exported via `pub use` at the bottom of `app.rs` so
//! call sites continue to write `crate::app::Message::Foo`.

use std::sync::Arc;

use iced::keyboard;
use iced::widget::text_editor;
use iced::Point;
use uuid::Uuid;

use oryxis_ssh::SshSession;

use crate::state::{ConnectionStep, SettingsSection, View};

#[derive(Debug, Clone)]
pub enum Message {
    // Vault
    VaultPasswordChanged(String),
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
    // Absorb-click sink — used by modal bodies to stop clicks from falling
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
    /// Open a local file in the OS default app — no temp copy, no
    /// mtime watch. Edits land on the file directly.
    SftpOpenLocal(std::path::PathBuf),
    /// Open an arbitrary URL in the user's default browser.
    /// Used by clickable links in the About panel.
    OpenUrl(String),
    SftpEditReady(crate::state::EditSession),
    SftpEditSave,
    SftpEditDiscard,
    SftpEditWatchTick,
    SftpCancelRemoteLoad,
    /// Retry the last failed remote action — either re-list the
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
    /// message — that way pause/resume can spawn fresh chains without
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
    PtyOutput(usize, Vec<u8>),  // (tab_index, bytes)
    KeyboardEvent(keyboard::Event),
    MouseMoved(Point),
    WindowResized(iced::Size),
    WindowDrag,
    WindowResizeDrag(iced::window::Direction),
    /// Double-click on a N/S edge — fill the full monitor height while
    /// keeping horizontal position and width.
    WindowExpandVertical,
    /// Double-click on an E/W edge — fill the full monitor width while
    /// keeping vertical position and height.
    WindowExpandHorizontal,
    WindowMinimize,
    WindowMaximizeToggle,
    WindowClose,

    // Overlay
    HideOverlayMenu,

    // Card interactions
    CardHovered(usize),
    CardUnhovered,
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
    EditorTogglePasswordVisibility,
    EditorSave,
    EditorCancel,
    DeleteConnection(usize),
    DuplicateConnection(usize),

    // SSH
    ConnectSsh(usize),
    SshProgress(ConnectionStep, String),
    SshConnected(usize, Arc<SshSession>),
    SshNewKnownHosts(Vec<oryxis_core::models::known_host::KnownHost>),
    SshDisconnected(usize),
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
    SnippetCommandChanged(String),
    SaveSnippet,
    EditSnippet(usize),
    DeleteSnippet(usize),
    RunSnippet(usize),

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

    // Settings
    LockVault,
    #[allow(dead_code)]
    TerminalThemeChanged(String),
    AppThemeChanged(String),
    TerminalFontSizeIncrease,
    TerminalFontSizeDecrease,
    TerminalFontChanged(String),
    ChangeSettingsSection(SettingsSection),
    ToggleCopyOnSelect,
    ToggleBoldIsBright,
    ToggleKeywordHighlight,
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

    // Local shell
    OpenLocalShell,

    // Keys
    ShowKeyPanel,
    HideKeyPanel,
    KeyImportLabelChanged(String),
    KeyContentAction(text_editor::Action),
    BrowseKeyFile,
    KeyFileLoaded(String, String), // (filename, content)
    KeyFileBrowseError(String),
    ImportKey,
    DeleteKey(usize),
    ShowKeyMenu(usize),
    #[allow(dead_code)]
    HideKeyMenu,
    EditKey(usize),
    KeySearchChanged(String),

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
    /// Terminal sentinel for `ChatStreamChunk` — clears the loading
    /// state and finalises the message (markdown re-parse, scroll snap).
    ChatStreamDone,
    ChatError(String),
    /// Re-send the last user message — used by the Retry button on an
    /// error bubble. Pops the most recent error and replays.
    ChatRetry,
    ChatToolExec(String),
    #[allow(dead_code)]
    ChatToolResult(String),

    // Port forwarding
    EditorAddPortForward,
    EditorRemovePortForward(usize),
    EditorPortFwdLocalPortChanged(usize, String),
    EditorPortFwdRemoteHostChanged(usize, String),
    EditorPortFwdRemotePortChanged(usize, String),

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

    // Sync
    SyncToggleEnabled,
    SyncModeChanged(String),
    SyncDeviceNameChanged(String),
    SyncSignalingUrlChanged(String),
    SyncRelayUrlChanged(String),
    SyncListenPortChanged(String),
    SyncStartPairing,
    SyncUnpairDevice(uuid::Uuid),
    SyncNow,

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
    /// modal yet — batch-imports everything non-wildcard and shows a
    /// status banner.
    ImportSshConfig,
    #[allow(dead_code)]
    ImportFileLoaded(Vec<u8>),
    ImportPasswordChanged(String),
    ImportConfirm,
    #[allow(dead_code)]
    ImportCompleted(Result<String, String>),
    ExportImportDismiss,

    // Share
    ShareConnection(usize),
    #[allow(dead_code)]
    ShareGroup(uuid::Uuid),
    SharePasswordChanged(String),
    ShareToggleKeys,
    ShareConfirm,
    ShareDismiss,
}
