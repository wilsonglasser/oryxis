//! Settings: appearance, terminal prefs, defaults, privacy, themes, local terminals, advanced, hotkeys.

use iced::widget::text_editor;
use crate::state::SettingsSection;
use super::PrivacyMaskClass;

#[derive(Debug, Clone)]
pub enum SettingsMessage {
    /// Settings → Shortcuts: enter capture mode for an action. The
    /// next non-Esc, non-pure-modifier `KeyPressed` becomes the new
    /// binding (see `shortcuts::handle_hotkey_capture`).
    StartEditingHotkey(crate::hotkeys::HotkeyAction, crate::hotkeys::HotkeySlot),
    /// A bindable mouse button (middle / back / forward / extra) was
    /// pressed anywhere in the window. Fired unconditionally by the
    /// event subscription; `shortcuts::handle_mouse_button_press`
    /// decides whether it records a binding or fires one.
    MouseButtonPressed(iced::mouse::Button),
    /// Settings → Shortcuts: drop a single action's user override and
    /// fall back to the factory default.
    ResetHotkey(crate::hotkeys::HotkeyAction),
    /// Settings → Shortcuts: drop every user override.
    ResetAllHotkeys,
    /// Settings > Terminal: toggle the paste content heuristics.
    TogglePasteGuard,
    /// Settings > Terminal: enable/disable command-history capture.
    ToggleCommandHistory,
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
    /// Open the editor with a copy of the custom theme at this index
    /// (new theme, deduped "(copy)" name seed).
    ThemeClone(usize),
    /// Same, seeded from the built-in terminal theme at this index
    /// (into `TerminalTheme::ALL`).
    ThemeCloneBuiltin(usize),
    /// Export the custom theme at this index as a Windows Terminal
    /// scheme JSON file (save dialog).
    ThemeExport(usize),
    /// Export the BUILT-IN terminal theme at this index (into
    /// `TerminalTheme::ALL`) the same way, so presets double as format
    /// templates for hand-made theme files.
    ThemeExportBuiltin(usize),
    /// A theme export finished: `Ok` toasts success, `Err("cancelled")`
    /// stays silent, any other `Err` toasts the failure. Shared by the
    /// terminal and UI theme exports.
    ThemeExportFinished(Result<(), String>),
    /// Import-theme modal (paste an iTerm / Windows Terminal / base16 scheme).
    ThemeImportOpen,
    ThemeImportClose,
    ThemeImportContentAction(text_editor::Action),
    ThemeImportNameChanged(String),
    /// Parse the pasted scheme; on success open it in the editor for review.
    ThemeImportApply,
    /// Pick a scheme file from disk and load it into the import modal.
    ThemeImportBrowse,
    /// File contents arrived from the browse dialog (or an error;
    /// "cancelled" is silent).
    ThemeImportFileLoaded(Result<String, String>),
    /// Hover tracking for the floating clone icon on a BUILT-IN terminal
    /// theme card in the settings grid.
    ThemeBuiltinCardHovered(usize),
    ThemeBuiltinCardUnhovered,
    UiThemeEditorNew,
    UiThemeEditorEdit(usize),
    UiThemeEditorClose,
    UiThemeEditorNameChanged(String),
    UiThemeColorChanged(usize, String),
    UiThemeEditorOpenPicker(usize),
    UiThemeEditorClosePicker,
    UiThemeEditorSave,
    UiThemeDelete(usize),
    /// Open the UI theme editor with a copy of the custom UI theme at
    /// this index (new theme, deduped "(copy)" name seed).
    UiThemeClone(usize),
    /// Same, seeded from the built-in app theme at this index (into
    /// `AppTheme::ALL`).
    UiThemeCloneBuiltin(usize),
    /// Export the custom UI theme at this index as an Oryxis UI theme
    /// JSON file (save dialog). Completion rides `ThemeExportFinished`.
    UiThemeExport(usize),
    /// Export the BUILT-IN app theme at this index (into `AppTheme::ALL`)
    /// the same way.
    UiThemeExportBuiltin(usize),
    /// Import-UI-theme modal (paste the Oryxis UI theme JSON), mirroring
    /// the terminal scheme import modal.
    UiThemeImportOpen,
    UiThemeImportClose,
    UiThemeImportContentAction(text_editor::Action),
    UiThemeImportNameChanged(String),
    /// Parse the pasted UI theme; on success open it in the editor for
    /// review (a typed name overrides the file's own).
    UiThemeImportApply,
    /// Pick an Oryxis UI theme JSON from disk and load it into the modal.
    UiThemeImportBrowse,
    /// File contents arrived from the UI-theme browse dialog.
    UiThemeImportFileLoaded(Result<String, String>),
    /// Hover tracking for the floating clone icon on a BUILT-IN app
    /// theme card in the Interface grid.
    UiThemeBuiltinCardHovered(usize),
    UiThemeBuiltinCardUnhovered,
    UiThemeCardHovered(usize),
    UiThemeCardUnhovered,
    /// Hover tracking for the floating edit / delete icons on a custom
    /// theme card.
    ThemeCardHovered(usize),
    ThemeCardUnhovered,
    /// Open the compact color-picker popover for a slot (anchored at the
    /// cursor).
    ThemeEditorOpenPicker(crate::state::ThemeColorSlot),
    /// Close the color-picker popover.
    ThemeEditorClosePicker,
    /// Local/ephemeral panes have no saved host: pick a session-only theme
    /// for the open local terminals, or promote it to the global default.
    LocalConfigThemeChanged(String),
    LocalConfigSaveGlobal,
    TerminalThemeChanged(String),
    AppThemeChanged(String),
    TerminalFontSizeIncrease,
    TerminalFontSizeDecrease,
    TerminalFontChanged(String),
    /// The user ctrl-clicked a link in the terminal: the gesture landed,
    /// so under `HintMode::Once` retire the link toast for the focused pane.
    TerminalLinkOpened,
    /// Settings: terminal hint mode picker changed. Carries the localized
    /// option label; the dispatch handler maps it back to a `HintMode`.
    HintModeChanged(String),
    /// Flip the reveal/eye state of a secret input field.
    ToggleSecretVisibility(crate::state::SecretField),
    /// Trash on a custom theme card: RAISES THE CONFIRM, it does not
    /// delete. A theme can be a long edit or an import that no longer
    /// exists anywhere else, and the trash sits in the same hover cluster
    /// as clone and edit. `ThemeDelete` / `UiThemeDelete` are only ever
    /// reached by confirming.
    /// Open / close the app-theme gallery (Settings > Interface), the
    /// sibling of the terminal one.
    OpenUiThemeGallery,
    CloseUiThemeGallery,
    ThemeDeleteRequested(usize),
    UiThemeDeleteRequested(usize),
    /// Uniform-mode width ceiling (small / medium / large). Ignored
    /// while the adaptive mode is selected.
    SettingTabUniformSizeChanged(String),
    ChangeSettingsSection(SettingsSection),
    /// The open section's scrollable moved; carries the relative y offset
    /// (0.0..=1.0). Recorded per section so returning to Settings lands
    /// where you left it (issue #120).
    SectionScrolled(f32),
    /// Deferred half of the restore: runs once the target section's view
    /// has rebuilt, so the scrollable it addresses actually exists.
    SectionScrollTo(iced::widget::Id, f32),
    /// Settings sidebar search: the query text changed (live filter).
    SettingsSearchChanged(String),
    /// Enter / Shift+Enter in the search: step the find-next cursor to
    /// the next (`true`) / previous (`false`) match, crossing sections,
    /// and scroll it into view. Find-in-page navigation.
    SettingsSearchStep(bool),
    /// Activate a settings-search / palette result: open the section
    /// and reveal (ring + scroll to) the row whose label key this is.
    RevealSetting(SettingsSection, &'static str),
    /// Internal follow-up to a search that changed the open section:
    /// scrolls the top match into view via a layout-reading widget
    /// operation (draw-independent, so no retry needed).
    RevealSettingScroll,
    /// Pick the renderer backend ("auto" / "opengl" / "software").
    /// Persisted to the vault; takes effect on the next launch (the
    /// backend is fixed at startup via WGPU_BACKEND / ICED_BACKEND).
    SettingRendererBackendChanged(String),
    /// Resolved graphics backend + adapter from the compositor, queried
    /// when the Interface settings section opens. `(backend, adapter)`.
    RendererInfoLoaded(String, String),
    ToggleCopyOnSelect,
    ToggleRightClickCopy,
    ToggleMiddleClickPaste,
    /// Settings > Interface: show the monitored host's vitals in the
    /// status bar (issue #83).
    SettingToggleMonitorStatusBar,
    /// Settings > Connection: seconds between host-monitor probes.
    SettingMonitorIntervalChanged(String),
    ToggleSftpForceOsc7,
    ToggleSftpAskDownloadDir,
    /// Settings > SFTP: the single external editor used by the remote
    /// "Open with default text editor" action (issue #84).
    SettingSftpDefaultEditorChanged(String),
    /// Pick the editor executable via a file dialog.
    SettingSftpDefaultEditorBrowse,
    /// The browse dialog returned ("cancelled" errors stay silent).
    SettingSftpDefaultEditorPicked(Result<String, String>),
    /// Settings > SFTP: toggle the persisted auto-upload grant the
    /// save-confirmation dialog's "Autosave" button turns on.
    ToggleSftpEditAutosave,
    /// PuTTY "reset scrollback on keypress" toggled in Settings > Terminal.
    ToggleScrollbackResetKeypress,
    /// PuTTY "reset scrollback on display activity" toggled in Settings.
    ToggleScrollbackResetOutput,
    /// Right-click scheme changed from the settings pick (localized
    /// "Context menu / Paste / Extend selection" label).
    TerminalRightClickChanged(String),
    /// Flip the careful-paste guard (warn before multi-line paste).
    ToggleCarefulPaste,
    ToggleBoldIsBright,
    TogglePaneBorderInactive,
    PaneGapChanged(String),
    OpenTerminalThemeGallery,
    CloseTerminalThemeGallery,
    /// Toggle showing the shell-set window title (OSC 0/2) in the tab strip.
    ToggleTerminalAutoTitle,
    /// Terminal bell behavior changed from the settings pick (localized
    /// "Off / Flash / Beep" label).
    BellModeChanged(String),
    /// OSC 52 clipboard access policy changed from the settings pick
    /// (localized "Off / Write only / Read & write" label).
    ClipboardAccessChanged(String),
    /// OSC 9 notification surfacing changed from the settings pick
    /// (localized "Off / Toast / OS" label).
    NotificationModeChanged(String),
    /// Smart tabs (attention dots + long-command / activity
    /// notifications) toggled in Settings > Terminal.
    SettingToggleSmartTabs,
    /// Smart-tabs long-command threshold changed from the settings pick
    /// (display label; resolved via `smart_tabs::threshold_options`).
    SmartTabsThresholdChanged(String),
    ToggleKeywordHighlight,
    ToggleSmartContrast,
    SettingToggleShowStatusBar,
    /// Status-bar element visibility toggles (issue #83 follow-up).
    SettingToggleStatusVersion,
    SettingToggleStatusConnection,
    SettingToggleStatusLatency,
    SettingToggleStatusDimensions,
    SettingToggleStatusCwd,
    /// Align the status-bar content on the leading edge (issue #83).
    SettingToggleStatusAlignLeft,
    /// Settings > Terminal: flip the terminal sidebar to the left edge
    /// (issue #85).
    SettingToggleTerminalSidebarLeft,
    SettingToggleSidebarAutoOpen,
    /// Settings > Terminal: which tab the sidebar opens onto (issue #85).
    /// Carries the translated picker label; the handler maps it back to
    /// a tab (or the "last opened" sentinel).
    SidebarDefaultTabChanged(String),
    /// Features & Plugins: master toggle for the host-monitoring feature
    /// (issue #83). Gates ALL monitoring UI.
    SettingToggleHostMonitoring,
    /// Monitoring section: "Enable for all hosts" (issue #83).
    SettingToggleMonitorAllHosts,
    /// Flip the host dashboard between the responsive card grid and a
    /// single-column list.
    ToggleHostListView,
    /// Flip the per-colour accent wash on dashboard cards (glass vs pure).
    ToggleCardAccentGlass,
    /// Flip showing of the `user@host:port` address on host cards.
    ToggleShowHostAddress,
    /// Flip showing of the `host:port` address on tab labels.
    ToggleShowTabHostAddress,
    /// Flip the global Privacy Mode default (auto-hide sensitive data).
    TogglePrivacyMode,
    /// Privacy Mode session override (issue #78): press once to force
    /// the opposite of the configured global state (above per-host
    /// overrides too), press again to fall back to the settings.
    /// Volatile, never persisted. Driven by the Ctrl+Shift+M hotkey
    /// and the status-bar chip.
    TogglePrivacySessionOverride,
    /// Privacy Mode always-mask textarea action (issue #78): literals
    /// masked wherever they appear, on top of the derived terms.
    SettingPrivacyAlwaysMaskAction(text_editor::Action),
    /// Privacy Mode never-mask textarea action (issue #78): words the
    /// derived terms must not include (generic usernames).
    SettingPrivacyNeverMaskAction(text_editor::Action),
    /// Flip one per-class Privacy Mode gate (issue #78 block 1).
    TogglePrivacyMaskClass(PrivacyMaskClass),
    /// Flip the Settings > Advanced debug logging (tracing events also
    /// written to the exportable `~/.oryxis/oryxis-debug.log`).
    SettingToggleDebugLogging,
    /// Settings > Advanced: download-mirror picker changed
    /// ("auto" / "github" / "custom").
    DownloadMirrorPicked(String),
    /// Custom mirror URL field edited (live value).
    DownloadMirrorUrlEdited(String),
    /// Custom mirror URL committed (Enter / Save): validate + persist.
    DownloadMirrorUrlCommitted,
    /// Run the mirror reachability probe against the entered URL.
    DownloadMirrorTest,
    /// Probe outcome: latency in ms, or the failure cause.
    DownloadMirrorTestResult(Result<u64, String>),
    /// Reveal the debug log file in the OS file manager (falls back to
    /// the `~/.oryxis` folder while no log file exists yet).
    RevealDebugLog,
    /// Wipe the debug log file (truncated in place while logging is on,
    /// deleted otherwise).
    ClearDebugLog,
    SettingToggleCloseToTray,
    SettingToggleMinimizeToTray,
    SettingToggleTabAccentLine,
    SettingToggleTabAccentWash,
    SettingToggleTabAccentText,
    SettingTogglePerformanceMode,
    SettingTogglePerfOverlay,
    /// Toggle the opt-in "remote desktop" feature (`remote_desktop_enabled`).
    SettingToggleRemoteDesktop,
    /// Relaunch the app in place to apply a start-time-only setting (the
    /// graphics renderer). Fired from the renderer-change restart modal.
    RelaunchApp,
    SettingTabCloseButtonSideChanged(String),
    SettingPinnedTabStyleChanged(String),
    SettingTabFillStyleChanged(String),
    SettingTabAccentColorChanged(String),
    /// Dock the tab strip at the top (default) or the bottom of the
    /// window ("top" / "bottom"). The window chrome (burger, drag area,
    /// minimize / maximize / close) stays in a slim top bar either way.
    SettingTabBarPositionChanged(String),
    /// Inactive-tab separation style: "none" / "border" / "underline".
    SettingInactiveTabStyleChanged(String),
    /// Tab sizing in the horizontal strip: `adaptive` or `uniform`.
    SettingTabWidthModeChanged(String),
    SettingTogglePinnedTabsTopBar,
    SettingToggleSideHideTopBar,
    SettingToggleSideFullHeight,
    SettingToggleShowTabStatusDot,
    SettingToggleSftpEnabled,
    SettingNavOrientationChanged(String),
    /// Expand/collapse the vertical nav rail (labels vs icon-only).
    ToggleNavRailExpanded,
    SettingDefaultHostIconChanged(String),
    SettingKeepaliveChanged(String),
    /// New-connection defaults (pre-filled into a fresh host form).
    ToggleDefaultAgentForwarding,
    DefaultPortChanged(String),
    DefaultKeepaliveChanged(String),
    DefaultTerminalTypeChanged(String),
    /// Extended new-connection defaults (the default host profile).
    DefaultUsernameChanged(String),
    DefaultAuthMethodChanged(String),
    DefaultIdentityChanged(String),
    DefaultKeyChanged(String),
    DefaultGroupChanged(String),
    DefaultProxyChanged(String),
    ToggleDefaultMcpEnabled,
    DefaultEncodingChanged(String),
    DefaultAddEnvVar,
    DefaultRemoveEnvVar(usize),
    DefaultEnvVarKeyChanged(usize, String),
    DefaultEnvVarValueChanged(usize, String),
    /// Collapse / expand the "New connection defaults" card.
    ToggleDefaultsCollapsed,
    SettingScrollbackChanged(String),
    SettingWordDelimitersChanged(String),
    SettingResetWordDelimiters,
    SettingSftpConcurrencyChanged(String),
    SettingSftpConnectTimeoutChanged(String),
    SettingSftpAuthTimeoutChanged(String),
    SettingSftpSessionTimeoutChanged(String),
    SettingSftpOpTimeoutChanged(String),
    SettingToggleAutoReconnect,
    SettingMaxReconnectChanged(String),
    /// Vault auto-lock idle threshold, minutes as typed ("0" = off).
    SettingAutoLockChanged(String),
    /// Periodic idle check while the vault is unlocked and auto-lock is
    /// enabled; locks when the idle threshold is crossed.
    AutoLockTick,
    AutoReconnectTick,
    ConnectAnimTick,
    LanguageChanged(String),
    /// User picked a layout-direction option (Auto / LTR / RTL).
    /// The string is the localized label shown in the picker; the
    /// dispatch handler maps it back to a `LayoutDirection` value.
    LayoutDirectionChanged(String),
    FlattenHostsToggle,
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
    /// Navigate from the picker's "+ terminal" footer to the management
    /// card; closes the picker overlay.
    OpenLocalTerminalsSettings,
    /// Re-run the auto-scan and merge new findings into the curated list
    /// (keeps everything already there; re-adds detected entries removed
    /// earlier, since it's an explicit user action).
    RescanLocalTerminals,
    /// Result of the async re-scan probe; merged + persisted on arrival.
    LocalTerminalsRescanned(Vec<crate::state::LocalShellSpec>),
    /// Remove one curated entry by its id.
    RemoveLocalTerminal(uuid::Uuid),
    /// Set the "always open X" default (the entry id), or `None` to
    /// restore "always ask (picker)".
    SetDefaultLocalTerminal(Option<uuid::Uuid>),
    /// Open the "add local terminal" modal (blank form).
    OpenLocalTerminalAddModal,
    /// Open the modal to edit an existing entry by id.
    OpenLocalTerminalEditModal(uuid::Uuid),
    CloseLocalTerminalAddModal,
    /// Open the host icon / color picker targeting the add-edit form.
    OpenLocalTerminalIconPicker,
    /// Add / edit form field edits.
    LocalTerminalFormLabelChanged(String),
    LocalTerminalFormProgramChanged(String),
    LocalTerminalFormArgsChanged(String),
    LocalTerminalFormTagsChanged(String),
    /// Commit the add / edit form into the curated list.
    AddLocalTerminalSubmit,
    /// Hover tracking for the per-card remove action.
    LocalTerminalCardHovered(usize),
    LocalTerminalCardUnhovered,
    SettingCloudAutoRefreshToggle,
    SettingCloudAutoRefreshIntervalChanged(String),
    SettingCloudAutoArchiveToggle,
    SettingCloudOrphanArchiveDaysChanged(String),
    /// A CJK font (Korean / Chinese / Japanese) finished downloading or
    /// was read from cache; `Ok` carries the font bytes to hand to
    /// `iced::font::load`. Carries the language code so the in-memory
    /// "already loaded" guard can be cleared on failure for a retry.
    CjkFontReady(String, Result<Vec<u8>, String>),
    /// Retention code picked in Settings ("off" / "1d" / ... / "90d");
    /// persists and prunes immediately.
    LogsRetentionChanged(&'static str),
    SettingToggleOsDetection,
    /// Toggle the global "record terminal sessions" setting.
    SettingToggleSessionLogging,
    /// Toggle full-detail recording (timing + resizes, feeds the .cast
    /// export) vs the plain output log.
    SettingToggleSessionLogFull,
    /// Toggle deflate compression of recorded chunks at flush time.
    SettingToggleSessionLogCompress,
    /// Toggle the global "record connection events" (history) setting.
    SettingToggleConnectionHistory,
}
