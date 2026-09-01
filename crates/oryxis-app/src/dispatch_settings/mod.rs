//! `Oryxis::handle_settings`, the Settings-panel dispatch router.
//! The arm families live in sibling files (`themes`,
//! `local_terminals`, `appearance`, `terminal_prefs`, `defaults`,
//! `advanced`, `privacy`); the match below keeps the arms that fit
//! no family: language + layout, hotkey editing, section switching,
//! SFTP / cloud / reconnect knobs and the auto-lock idle tick.

#![allow(clippy::result_large_err)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]

pub(crate) use iced::Task;

pub(crate) use std::sync::{Arc, Mutex};
pub(crate) use tokio_stream::wrappers::UnboundedReceiverStream;

pub(crate) use oryxis_terminal::widget::TerminalState;

pub(crate) use crate::app::{SettingsMessage, TabsMessage, TerminalMessage, SshMessage, VaultMessage, Message, Oryxis, DEFAULT_TERM_COLS, DEFAULT_TERM_ROWS};
pub(crate) use crate::state::{TerminalTab, View};
pub(crate) use crate::theme::AppTheme;
pub(crate) use crate::util::sanitize_uint;

/// Resolve the persisted `scrollback_rows` string into a concrete line
/// count for the terminal backend. The setting treats "0" as "maximum",
/// which maps to the same 1M ceiling the input field is capped at; an
/// empty or unparseable value falls back to the 10,000 default.
pub(crate) fn resolve_scrollback_rows(rows: &str) -> usize {
    match rows.trim().parse::<usize>() {
        Ok(0) => 1_000_000,
        Ok(n) => n,
        Err(_) => 10_000,
    }
}

// Settings-dispatch sub-handlers, one file per arm family.
mod advanced;
mod search;
mod locale;
mod hotkeys;
mod session_logs;
mod timers;
mod toggles;
mod appearance;
mod defaults;
mod highlight_rules;
pub(crate) use highlight_rules::{action_label, action_options, host_mode_options};
mod local_terminals;
mod login_scripts;
// The reconnect respawn (`dispatch_tabs/lifecycle.rs`) spawns the
// dead tab's exact shell with its captured cwd, bypassing the
// picker / "always open X" decision flow.
pub(crate) use local_terminals::spawn_local_shell_in;
mod privacy;
mod terminal_prefs;
mod themes;

impl Oryxis {
    /// Effective SSH keepalive duration for a connection. Per-host
    /// override (`Connection.keepalive_interval`) wins over the global
    /// `setting_keepalive_interval`. `Some(Duration)` means "send
    /// keepalive every N seconds"; `None` means disabled. A per-host
    /// `Some(0)` explicitly disables on that host even when the global
    /// is non-zero. Used by both the shell and SFTP connect paths.
    pub(crate) fn effective_keepalive(
        &self,
        conn: &oryxis_core::models::Connection,
    ) -> Option<std::time::Duration> {
        crate::util::resolve_keepalive(
            conn.keepalive_interval,
            &self.prefs.keepalive_interval,
        )
    }

    /// Switch the open Settings section during a search (auto-open /
    /// find-next crossing a section boundary): same teardown as a
    /// manual `ChangeSettingsSection` minus the message round-trip.
    fn switch_settings_section_for_search(&mut self, section: crate::state::SettingsSection) {
        self.settings_section = section;
        self.keynav.pick_open = false;
        self.keynav.focus = None;
        self.keynav_clear_content();
        self.keynav.settings_row_actions.borrow_mut().clear();
    }

    /// Fire `RevealSettingScroll` after a short delay so the target
    /// section's view has rebuilt (its scroll-target row exists in the
    /// widget tree) before the scroll-into-view operation runs. The
    /// operation itself is draw-independent, so a small fixed delay is
    /// enough - no retry loop needed.
    fn schedule_settings_scroll(&self) -> Task<Message> {
        Task::perform(
            async {
                tokio::time::sleep(std::time::Duration::from_millis(90)).await;
            },
            |_| Message::Settings(SettingsMessage::RevealSettingScroll),
        )
    }

    /// Put the open section back where the user left it (issue #120). The
    /// offset is recorded by each section's `on_scroll`; scrolling to it
    /// needs the section's view to have rebuilt first, so it rides the
    /// same small delay the reveal scroll uses. A section never scrolled
    /// (or scrolled back to the top) costs nothing.
    pub(crate) fn settings_restore_scroll(&self) -> Task<Message> {
        let Some(&y) = self.settings_scroll.get(&self.settings_section) else {
            return Task::none();
        };
        if y <= 0.0 {
            return Task::none();
        }
        let id = iced::widget::Id::new(self.settings_section.scroll_id());
        Task::perform(
            async {
                tokio::time::sleep(std::time::Duration::from_millis(90)).await;
            },
            move |_| Message::Settings(SettingsMessage::SectionScrollTo(id.clone(), y)),
        )
    }
}

impl Oryxis {
    /// Dispatch a Settings message: family-owned variants route
    /// straight to their sub-handler (themes / local terminals /
    /// appearance / terminal prefs / defaults / advanced / privacy),
    /// the misc arms that fit no family match inline. Exhaustive on
    /// purpose: a new `SettingsMessage` variant fails to compile until
    /// it gets an arm, so it can never be silently dropped.
    pub(crate) fn handle_settings(
        &mut self,
        message: SettingsMessage,
    ) -> Task<Message> {
        match message {
            m @ (SettingsMessage::ThemeEditorNew
            | SettingsMessage::ThemeEditorEdit(..)
            | SettingsMessage::ThemeEditorClose
            | SettingsMessage::ThemeEditorNameChanged(..)
            | SettingsMessage::ThemeEditorColorChanged(..)
            | SettingsMessage::ThemeEditorSave
            | SettingsMessage::ThemeDelete(..)
            | SettingsMessage::ThemeDeleteRequested(..)
            | SettingsMessage::UiThemeDeleteRequested(..)
            | SettingsMessage::ThemeImportOpen
            | SettingsMessage::ThemeImportClose
            | SettingsMessage::ThemeImportContentAction(..)
            | SettingsMessage::ThemeImportNameChanged(..)
            | SettingsMessage::ThemeImportApply
            | SettingsMessage::UiThemeEditorNew
            | SettingsMessage::UiThemeEditorEdit(..)
            | SettingsMessage::UiThemeEditorClose
            | SettingsMessage::UiThemeEditorNameChanged(..)
            | SettingsMessage::UiThemeColorChanged(..)
            | SettingsMessage::UiThemeEditorOpenPicker(..)
            | SettingsMessage::UiThemeEditorClosePicker
            | SettingsMessage::UiThemeEditorSave
            | SettingsMessage::UiThemeDelete(..)
            | SettingsMessage::UiThemeClone(..)
            | SettingsMessage::UiThemeCloneBuiltin(..)
            | SettingsMessage::UiThemeExport(..)
            | SettingsMessage::UiThemeExportBuiltin(..)
            | SettingsMessage::UiThemeImportOpen
            | SettingsMessage::UiThemeImportClose
            | SettingsMessage::UiThemeImportContentAction(..)
            | SettingsMessage::UiThemeImportNameChanged(..)
            | SettingsMessage::UiThemeImportApply
            | SettingsMessage::UiThemeImportBrowse
            | SettingsMessage::UiThemeImportFileLoaded(..)
            | SettingsMessage::UiThemeBuiltinCardHovered(..)
            | SettingsMessage::UiThemeBuiltinCardUnhovered(..)
            | SettingsMessage::UiThemeCardHovered(..)
            | SettingsMessage::UiThemeCardUnhovered(..)
            | SettingsMessage::ThemeClone(..)
            | SettingsMessage::ThemeCloneBuiltin(..)
            | SettingsMessage::ThemeExport(..)
            | SettingsMessage::ThemeExportBuiltin(..)
            | SettingsMessage::ThemeExportFinished(..)
            | SettingsMessage::ThemeImportBrowse
            | SettingsMessage::ThemeGalleryFilterChanged(..)
            | SettingsMessage::ThemeImportFileLoaded(..)
            | SettingsMessage::ThemeBuiltinCardHovered(..)
            | SettingsMessage::ThemeBuiltinCardUnhovered(..)
            | SettingsMessage::ThemeCardHovered(..)
            | SettingsMessage::ThemeCardUnhovered(..)
            | SettingsMessage::ThemeEditorOpenPicker(..)
            | SettingsMessage::ThemeEditorClosePicker
            | SettingsMessage::LocalConfigThemeChanged(..)
            | SettingsMessage::LocalConfigSaveGlobal
            | SettingsMessage::TerminalThemeChanged(..)
            | SettingsMessage::AppThemeChanged(..)) => {
                self.handle_settings_themes(m).unwrap_or_else(crate::dispatch::unrouted)
            }
            m @ (SettingsMessage::OpenLocalShell
            | SettingsMessage::ShowLocalShellPicker
            | SettingsMessage::LocalShellsDetected(..)
            | SettingsMessage::HideLocalShellPicker
            | SettingsMessage::OpenLocalShellWith{ .. }
            | SettingsMessage::OpenLocalTerminalsSettings
            | SettingsMessage::RescanLocalTerminals
            | SettingsMessage::LocalTerminalsRescanned(..)
            | SettingsMessage::RemoveLocalTerminal(..)
            | SettingsMessage::SetDefaultLocalTerminal(..)
            | SettingsMessage::OpenLocalTerminalAddModal
            | SettingsMessage::OpenLocalTerminalEditModal(..)
            | SettingsMessage::CloseLocalTerminalAddModal
            | SettingsMessage::OpenLocalTerminalIconPicker
            | SettingsMessage::LocalTerminalFormLabelChanged(..)
            | SettingsMessage::LocalTerminalFormProgramChanged(..)
            | SettingsMessage::LocalTerminalFormArgsChanged(..)
            | SettingsMessage::LocalTerminalFormTagsChanged(..)
            | SettingsMessage::AddLocalTerminalSubmit
            | SettingsMessage::LocalTerminalCardHovered(..)
            | SettingsMessage::LocalTerminalCardUnhovered(..)) => {
                self.handle_settings_local_terminals(m).unwrap_or_else(crate::dispatch::unrouted)
            }
            m @ (SettingsMessage::SettingToggleShowStatusBar
            | SettingsMessage::SettingToggleStatusVersion
            | SettingsMessage::SettingToggleStatusConnection
            | SettingsMessage::SettingToggleStatusLatency
            | SettingsMessage::SettingToggleStatusDimensions
            | SettingsMessage::SettingToggleStatusCwd
            | SettingsMessage::SettingToggleStatusAlignLeft
            | SettingsMessage::SidebarTabSideChanged(..)
            | SettingsMessage::SettingToggleSidebarAutoOpen
            | SettingsMessage::SettingToggleHostMonitoring
            | SettingsMessage::SettingToggleMonitorAllHosts
            | SettingsMessage::SettingToggleTmuxManager
            | SettingsMessage::SettingToggleConnectionReuse
            | SettingsMessage::SettingToggleMonitorStatusBar
            | SettingsMessage::SettingMonitorIntervalChanged(..)
            | SettingsMessage::CycleHostViewMode
            | SettingsMessage::ToggleCardAccentGlass
            | SettingsMessage::ToggleShowHostAddress
            | SettingsMessage::ToggleShowTabHostAddress
            | SettingsMessage::SettingToggleTabAccentLine
            | SettingsMessage::SettingToggleTabAccentWash
            | SettingsMessage::SettingToggleTabAccentText
            | SettingsMessage::SettingTabCloseButtonSideChanged(..)
            | SettingsMessage::SettingPinnedTabStyleChanged(..)
            | SettingsMessage::SettingDuplicateTabPositionChanged(..)
            | SettingsMessage::SettingTabNumberStyleChanged(..)
            | SettingsMessage::SettingToggleTabSlotIncludesHome
            | SettingsMessage::SettingTabFillStyleChanged(..)
            | SettingsMessage::SettingTabAccentColorChanged(..)
            | SettingsMessage::SettingTabBarPositionChanged(..)
            | SettingsMessage::SettingInactiveTabStyleChanged(..)
            | SettingsMessage::SettingTabWidthModeChanged(..)
            | SettingsMessage::SettingTabUniformSizeChanged(..)
            | SettingsMessage::SettingTogglePinnedTabsTopBar
            | SettingsMessage::SettingToggleSideHideTopBar
            | SettingsMessage::SettingToggleSideFullHeight
            | SettingsMessage::SettingToggleShowTabStatusDot
            | SettingsMessage::SettingNavOrientationChanged(..)
            | SettingsMessage::ToggleNavRailExpanded
            | SettingsMessage::SettingDefaultHostIconChanged(..)
            | SettingsMessage::FlattenHostsToggle) => {
                self.handle_settings_appearance(m).unwrap_or_else(crate::dispatch::unrouted)
            }
            m @ (SettingsMessage::TogglePasteGuard
            | SettingsMessage::ToggleCommandHistory
            | SettingsMessage::CopyShellIntegrationSnippet
            | SettingsMessage::RegenerateShellIntegrationNonce
            | SettingsMessage::TerminalFontSizeIncrease
            | SettingsMessage::TerminalFontSizeDecrease
            | SettingsMessage::TerminalFontChanged(..)
            | SettingsMessage::TerminalFontWeightChanged(..)
            | SettingsMessage::TerminalTextThicknessChanged(..)
            | SettingsMessage::PackFontReady(..)
            | SettingsMessage::TerminalLinkOpened
            | SettingsMessage::HintModeChanged(..)
            | SettingsMessage::ToggleCopyOnSelect
            | SettingsMessage::ToggleRightClickCopy
            | SettingsMessage::ToggleMiddleClickPaste
            | SettingsMessage::ToggleSftpAskDownloadDir
            | SettingsMessage::ToggleSftpUploadTempName
            | SettingsMessage::SftpConsoleLayoutChanged(..)
            | SettingsMessage::SettingSftpDefaultEditorChanged(..)
            | SettingsMessage::SettingSftpDefaultEditorBrowse
            | SettingsMessage::SettingSftpDefaultEditorPicked(..)
            | SettingsMessage::ToggleSftpEditAutosave
            | SettingsMessage::ToggleScrollbackResetKeypress
            | SettingsMessage::ToggleScrollbackResetOutput
            | SettingsMessage::ToggleTerminalPasswordAutofill
            | SettingsMessage::TerminalRightClickChanged(..)
            | SettingsMessage::SidebarDefaultTabChanged(..)
            | SettingsMessage::ToggleCarefulPaste
            | SettingsMessage::ToggleBoldIsBright
            | SettingsMessage::TerminalOpacityChanged(..)
            | SettingsMessage::TerminalBgImageBrowse
            | SettingsMessage::TerminalBgImagePicked(..)
            | SettingsMessage::TerminalBgImageCleared
            | SettingsMessage::TerminalBgFitChanged(..)
            | SettingsMessage::TerminalBgDimChanged(..)
            | SettingsMessage::TogglePaneBorderInactive
            | SettingsMessage::PaneGapChanged(..)
            | SettingsMessage::OpenTerminalThemeGallery
            | SettingsMessage::CloseTerminalThemeGallery
            | SettingsMessage::OpenUiThemeGallery
            | SettingsMessage::CloseUiThemeGallery
            | SettingsMessage::ToggleTerminalAutoTitle
            | SettingsMessage::BellModeChanged(..)
            | SettingsMessage::ClipboardAccessChanged(..)
            | SettingsMessage::NotificationModeChanged(..)
            | SettingsMessage::SettingToggleSmartTabs
            | SettingsMessage::SmartTabsThresholdChanged(..)
            | SettingsMessage::ToggleKeywordHighlight
            | SettingsMessage::ToggleSmartContrast
            | SettingsMessage::SettingScrollbackChanged(..)
            | SettingsMessage::SettingWordDelimitersChanged(..)
            | SettingsMessage::SettingResetWordDelimiters) => {
                self.handle_settings_terminal_prefs(m).unwrap_or_else(crate::dispatch::unrouted)
            }
            m @ (SettingsMessage::ToggleDefaultAgentForwarding
            | SettingsMessage::DefaultPortChanged(..)
            | SettingsMessage::DefaultKeepaliveChanged(..)
            | SettingsMessage::DefaultTerminalTypeChanged(..)
            | SettingsMessage::DefaultUsernameChanged(..)
            | SettingsMessage::DefaultAuthMethodChanged(..)
            | SettingsMessage::DefaultIdentityChanged(..)
            | SettingsMessage::DefaultKeyChanged(..)
            | SettingsMessage::DefaultGroupChanged(..)
            | SettingsMessage::DefaultProxyChanged(..)
            | SettingsMessage::ToggleDefaultMcpEnabled
            | SettingsMessage::DefaultEncodingChanged(..)
            | SettingsMessage::DefaultAddEnvVar
            | SettingsMessage::DefaultRemoveEnvVar(..)
            | SettingsMessage::DefaultEnvVarKeyChanged(..)
            | SettingsMessage::DefaultEnvVarValueChanged(..)
            | SettingsMessage::ToggleDefaultsCollapsed) => {
                self.handle_settings_defaults(m).unwrap_or_else(crate::dispatch::unrouted)
            }
            m @ (SettingsMessage::SettingRendererBackendChanged(..)
            | SettingsMessage::RendererInfoLoaded(..)
            | SettingsMessage::SettingToggleDebugLogging
            | SettingsMessage::DownloadMirrorPicked(..)
            | SettingsMessage::DownloadMirrorUrlEdited(..)
            | SettingsMessage::DownloadMirrorUrlCommitted
            | SettingsMessage::DownloadMirrorTest
            | SettingsMessage::DownloadMirrorTestResult(..)
            | SettingsMessage::RevealDebugLog
            | SettingsMessage::ClearDebugLog
            | SettingsMessage::RelaunchApp) => {
                self.handle_settings_advanced(m).unwrap_or_else(crate::dispatch::unrouted)
            }
            m @ (SettingsMessage::TogglePrivacyMode
            | SettingsMessage::TogglePrivacySessionOverride
            | SettingsMessage::SettingPrivacyAlwaysMaskAction(..)
            | SettingsMessage::SettingPrivacyNeverMaskAction(..)
            | SettingsMessage::TogglePrivacyMaskClass(..)) => {
                self.handle_settings_privacy(m).unwrap_or_else(crate::dispatch::unrouted)
            }
            // Session-logging / OS-detect toggles (handled here; the
            // recording + probe logic lives in dispatch_ssh).
            // -- Settings --
            m @ (
            SettingsMessage::SettingsSearchChanged(..)
            | SettingsMessage::SettingsSearchStep(..)
            | SettingsMessage::RevealSetting(..)
            | SettingsMessage::RevealSettingScroll
            | SettingsMessage::ChangeSettingsSection(..)
            | SettingsMessage::SectionScrolled(..)
            | SettingsMessage::SectionScrollTo(..)
            ) => self.handle_settings_search(m).unwrap_or_else(crate::dispatch::unrouted),
            m @ (
            SettingsMessage::LanguageChanged(..)
            | SettingsMessage::CjkFontReady(..)
            | SettingsMessage::LayoutDirectionChanged(..)
            ) => self.handle_settings_locale(m).unwrap_or_else(crate::dispatch::unrouted),
            m @ (
            SettingsMessage::StartEditingHotkey(..)
            | SettingsMessage::MouseButtonPressed(..)
            | SettingsMessage::ResetHotkey(..)
            | SettingsMessage::ResetAllHotkeys
            | SettingsMessage::ToggleSecretVisibility(..)
            ) => self.handle_settings_hotkeys(m).unwrap_or_else(crate::dispatch::unrouted),
            m @ (
            SettingsMessage::SettingToggleSessionLogging
            | SettingsMessage::SettingToggleSessionLogFull
            | SettingsMessage::SettingToggleSessionLogCompress
            | SettingsMessage::SettingToggleConnectionHistory
            | SettingsMessage::LogsRetentionChanged(..)
            | SettingsMessage::LogsSizeCapChanged(..)
            | SettingsMessage::SettingToggleOsDetection
            ) => self.handle_settings_session_logs(m).unwrap_or_else(crate::dispatch::unrouted),
            m @ (
            SettingsMessage::SettingToggleAutoReconnect
            | SettingsMessage::SettingMaxReconnectChanged(..)
            | SettingsMessage::SettingAutoLockChanged(..)
            | SettingsMessage::SettingManualLockActionChanged(..)
            | SettingsMessage::AutoLockTick
            | SettingsMessage::ConnectAnimTick
            | SettingsMessage::AutoReconnectTick
            ) => self.handle_settings_timers(m).unwrap_or_else(crate::dispatch::unrouted),
            m @ (
            SettingsMessage::SettingTogglePerformanceMode
            | SettingsMessage::SettingTogglePerfOverlay
            | SettingsMessage::SettingToggleNetworkTools
            | SettingsMessage::SettingToggleRemoteDesktop
            | SettingsMessage::SettingToggleCloseToTray
            | SettingsMessage::SettingToggleMinimizeToTray
            | SettingsMessage::SettingToggleSftpEnabled
            | SettingsMessage::SettingKeepaliveChanged(..)
            | SettingsMessage::SettingCloudAutoRefreshToggle
            | SettingsMessage::SettingCloudAutoRefreshIntervalChanged(..)
            | SettingsMessage::SettingCloudAutoArchiveToggle
            | SettingsMessage::SettingCloudOrphanArchiveDaysChanged(..)
            | SettingsMessage::SettingSftpConcurrencyChanged(..)
            | SettingsMessage::SettingSftpConnectTimeoutChanged(..)
            | SettingsMessage::SettingSftpAuthTimeoutChanged(..)
            | SettingsMessage::SettingSftpSessionTimeoutChanged(..)
            | SettingsMessage::SettingSftpOpTimeoutChanged(..)
            ) => self.handle_settings_toggles(m).unwrap_or_else(crate::dispatch::unrouted),
            m @ (
                SettingsMessage::LoginScriptOpenInSettings(..)
                | SettingsMessage::LoginScriptEdit(..)
                | SettingsMessage::LoginScriptCancelEdit
                | SettingsMessage::LoginScriptNameChanged(..)
                | SettingsMessage::LoginScriptAddStep
                | SettingsMessage::LoginScriptRemoveStep(..)
                | SettingsMessage::LoginScriptStepExpect(..)
                | SettingsMessage::LoginScriptStepSendKind(..)
                | SettingsMessage::LoginScriptStepText(..)
                | SettingsMessage::LoginScriptStepOptional(..)
                | SettingsMessage::LoginScriptSave
                | SettingsMessage::LoginScriptRequestDelete(..)
                | SettingsMessage::LoginScriptCancelDelete
                | SettingsMessage::LoginScriptDelete(..)
            ) => self.handle_settings_login_scripts(m),
            m @ (
                SettingsMessage::HighlightRuleAdd(..)
                | SettingsMessage::HighlightRuleEdit(..)
                | SettingsMessage::HighlightRuleCancelEdit
                | SettingsMessage::HighlightRuleSave
                | SettingsMessage::HighlightRuleToggleEnabled(..)
                | SettingsMessage::HighlightRuleMove(..)
                | SettingsMessage::HighlightRuleRequestDelete(..)
                | SettingsMessage::HighlightRuleCancelDelete
                | SettingsMessage::HighlightRuleDelete(..)
                | SettingsMessage::HighlightRuleNameChanged(..)
                | SettingsMessage::HighlightRulePatternChanged(..)
                | SettingsMessage::HighlightRuleToggleRegex
                | SettingsMessage::HighlightRuleToggleCaseSensitive
                | SettingsMessage::HighlightRuleColorChanged(..)
                | SettingsMessage::HighlightRuleActionChanged(..)
                | SettingsMessage::HighlightRuleSnippetChanged(..)
                | SettingsMessage::HighlightRuleHostModeChanged(..)
            ) => self
                .handle_settings_highlight_rules(m)
                .unwrap_or_else(crate::dispatch::unrouted),
        }
    }
}
