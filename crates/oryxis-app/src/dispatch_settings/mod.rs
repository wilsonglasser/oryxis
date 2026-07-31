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
mod appearance;
mod defaults;
mod local_terminals;
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
            &self.setting_keepalive_interval,
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
            | SettingsMessage::UiThemeBuiltinCardUnhovered
            | SettingsMessage::UiThemeCardHovered(..)
            | SettingsMessage::UiThemeCardUnhovered
            | SettingsMessage::ThemeClone(..)
            | SettingsMessage::ThemeCloneBuiltin(..)
            | SettingsMessage::ThemeExport(..)
            | SettingsMessage::ThemeExportBuiltin(..)
            | SettingsMessage::ThemeExportFinished(..)
            | SettingsMessage::ThemeImportBrowse
            | SettingsMessage::ThemeImportFileLoaded(..)
            | SettingsMessage::ThemeBuiltinCardHovered(..)
            | SettingsMessage::ThemeBuiltinCardUnhovered
            | SettingsMessage::ThemeCardHovered(..)
            | SettingsMessage::ThemeCardUnhovered
            | SettingsMessage::ThemeEditorOpenPicker(..)
            | SettingsMessage::ThemeEditorClosePicker
            | SettingsMessage::LocalConfigThemeChanged(..)
            | SettingsMessage::LocalConfigSaveGlobal
            | SettingsMessage::TerminalThemeChanged(..)
            | SettingsMessage::AppThemeChanged(..)) => {
                return self.handle_settings_themes(m).unwrap_or_else(crate::dispatch::unrouted);
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
            | SettingsMessage::LocalTerminalCardUnhovered) => {
                return self.handle_settings_local_terminals(m).unwrap_or_else(crate::dispatch::unrouted);
            }
            m @ (SettingsMessage::SettingToggleShowStatusBar
            | SettingsMessage::SettingToggleStatusVersion
            | SettingsMessage::SettingToggleStatusConnection
            | SettingsMessage::SettingToggleStatusLatency
            | SettingsMessage::SettingToggleStatusDimensions
            | SettingsMessage::SettingToggleStatusCwd
            | SettingsMessage::SettingToggleStatusAlignLeft
            | SettingsMessage::SettingToggleTerminalSidebarLeft
            | SettingsMessage::SettingToggleSidebarAutoOpen
            | SettingsMessage::SettingToggleHostMonitoring
            | SettingsMessage::SettingToggleMonitorAllHosts
            | SettingsMessage::SettingToggleMonitorStatusBar
            | SettingsMessage::SettingMonitorIntervalChanged(..)
            | SettingsMessage::ToggleHostListView
            | SettingsMessage::ToggleCardAccentGlass
            | SettingsMessage::ToggleShowHostAddress
            | SettingsMessage::ToggleShowTabHostAddress
            | SettingsMessage::SettingToggleTabAccentLine
            | SettingsMessage::SettingToggleTabAccentWash
            | SettingsMessage::SettingToggleTabAccentText
            | SettingsMessage::SettingTabCloseButtonSideChanged(..)
            | SettingsMessage::SettingPinnedTabStyleChanged(..)
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
                return self.handle_settings_appearance(m).unwrap_or_else(crate::dispatch::unrouted);
            }
            m @ (SettingsMessage::TogglePasteGuard
            | SettingsMessage::ToggleCommandHistory
            | SettingsMessage::TerminalFontSizeIncrease
            | SettingsMessage::TerminalFontSizeDecrease
            | SettingsMessage::TerminalFontChanged(..)
            | SettingsMessage::TerminalLinkOpened
            | SettingsMessage::HintModeChanged(..)
            | SettingsMessage::ToggleCopyOnSelect
            | SettingsMessage::ToggleRightClickCopy
            | SettingsMessage::ToggleMiddleClickPaste
            | SettingsMessage::ToggleSftpForceOsc7
            | SettingsMessage::ToggleSftpAskDownloadDir
            | SettingsMessage::SettingSftpDefaultEditorChanged(..)
            | SettingsMessage::SettingSftpDefaultEditorBrowse
            | SettingsMessage::SettingSftpDefaultEditorPicked(..)
            | SettingsMessage::ToggleSftpEditAutosave
            | SettingsMessage::ToggleScrollbackResetKeypress
            | SettingsMessage::ToggleScrollbackResetOutput
            | SettingsMessage::TerminalRightClickChanged(..)
            | SettingsMessage::SidebarDefaultTabChanged(..)
            | SettingsMessage::ToggleCarefulPaste
            | SettingsMessage::ToggleBoldIsBright
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
                return self.handle_settings_terminal_prefs(m).unwrap_or_else(crate::dispatch::unrouted);
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
                return self.handle_settings_defaults(m).unwrap_or_else(crate::dispatch::unrouted);
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
                return self.handle_settings_advanced(m).unwrap_or_else(crate::dispatch::unrouted);
            }
            m @ (SettingsMessage::TogglePrivacyMode
            | SettingsMessage::TogglePrivacySessionOverride
            | SettingsMessage::SettingPrivacyAlwaysMaskAction(..)
            | SettingsMessage::SettingPrivacyNeverMaskAction(..)
            | SettingsMessage::TogglePrivacyMaskClass(..)) => {
                return self.handle_settings_privacy(m).unwrap_or_else(crate::dispatch::unrouted);
            }
            // Session-logging / OS-detect toggles (handled here; the
            // recording + probe logic lives in dispatch_ssh).
            SettingsMessage::SettingToggleSessionLogging => {
                self.setting_session_logging = !self.setting_session_logging;
                self.persist_setting(
                    "session_logging",
                    if self.setting_session_logging { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingToggleSessionLogFull => {
                self.setting_session_log_full = !self.setting_session_log_full;
                self.persist_setting(
                    "session_log_full",
                    if self.setting_session_log_full { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingToggleSessionLogCompress => {
                self.setting_session_log_compress = !self.setting_session_log_compress;
                self.persist_setting(
                    "session_log_compress",
                    if self.setting_session_log_compress { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingToggleConnectionHistory => {
                self.setting_connection_history = !self.setting_connection_history;
                self.persist_setting(
                    "connection_history",
                    if self.setting_connection_history { "true" } else { "false" },
                );
            }
            SettingsMessage::LogsRetentionChanged(code) => {
                self.setting_logs_retention = code.to_string();
                self.persist_setting("logs_retention", code);
                // Apply right away so picking a shorter window has a
                // visible effect, then refresh the cached Logs state.
                if let Some(days) = Self::retention_days(code)
                    && let Some(vault) = &self.vault
                {
                    let cutoff = chrono::Utc::now() - chrono::Duration::days(days);
                    match vault.prune_logs_older_than(cutoff) {
                        Ok(0) => {}
                        Ok(n) => tracing::info!("logs retention pruned {n} rows"),
                        Err(e) => tracing::warn!("logs retention prune failed: {e}"),
                    }
                    self.logs_page = 0;
                    self.session_logs_page = 0;
                    self.logs_total = vault.count_logs().unwrap_or(0);
                    self.logs = vault.list_logs_page(0, 50).unwrap_or_default();
                    self.session_logs_total = vault.count_session_logs().unwrap_or(0);
                    self.session_logs =
                        vault.list_session_logs_page(0, 50).unwrap_or_default();
                }
            }
            SettingsMessage::SettingToggleOsDetection => {
                self.setting_os_detection = !self.setting_os_detection;
                self.persist_setting(
                    "os_detection",
                    if self.setting_os_detection { "true" } else { "false" },
                );
            }
            // -- Settings --
            SettingsMessage::LanguageChanged(token) => {
                use crate::i18n::Language;
                // Token-as-value from the picker: "auto" follows the
                // OS locale, anything else is a concrete language code.
                let lang = if token == "auto" {
                    crate::i18n::detect_os_language()
                } else {
                    Language::from_code(&token)
                };
                self.setting_language_choice = if token == "auto" {
                    token
                } else {
                    // Persist the canonical code (`from_code` may have
                    // normalized an unknown token to English).
                    lang.code().to_string()
                };
                Language::set_active(lang);
                if let Some(vault) = &self.vault {
                    let _ = vault
                        .set_setting("language", &self.setting_language_choice);
                }
                // Switching to a CJK language pulls its font on
                // demand (once per session). Show a hint while it
                // downloads; a cached font loads silently.
                if let Some(code) = crate::fonts::asset_code(lang)
                    && !self.loaded_cjk_fonts.contains(code)
                {
                    self.loaded_cjk_fonts.insert(code.to_string());
                    if !crate::fonts::is_language_cached(lang) {
                        self.set_toast(
                            crate::i18n::t("cjk_font_downloading").to_string(),
                        );
                    }
                    return crate::fonts::ensure_task(lang);
                }
            }
            SettingsMessage::CjkFontReady(code, result) => match result {
                Ok(bytes) => {
                    // Clear the "downloading" hint and register the font
                    // with the iced font system so cosmic-text can fall
                    // back to it. `iced::font::Error` is uninhabited, so
                    // the load result is discarded.
                    self.toast = None;
                    return iced::font::load(bytes).discard();
                }
                Err(e) => {
                    tracing::warn!(
                        target = "oryxis::fonts",
                        lang = %code,
                        error = %e,
                        "CJK font download failed; using system fallback"
                    );
                    // Drop the guard so a later switch can retry.
                    self.loaded_cjk_fonts.remove(&code);
                    self.set_toast(crate::i18n::t("cjk_font_failed").to_string());
                    return Task::perform(
                        async {
                            tokio::time::sleep(
                                std::time::Duration::from_millis(2600),
                            )
                            .await;
                        },
                        |_| Message::ToastClear,
                    );
                }
            },
            SettingsMessage::LayoutDirectionChanged(name) => {
                use crate::i18n::{t, LayoutDirection};
                // Match against the *localized* label since that's what
                // the pick_list emits; keys live on the enum so the
                // mapping survives language switches.
                if let Some(dir) = LayoutDirection::ALL
                    .iter()
                    .find(|d| t(d.label_key()) == name)
                {
                    LayoutDirection::set_active(*dir);
                    self.persist_setting("layout_direction", dir.code());
                }
            }
            SettingsMessage::SettingsSearchChanged(v) => {
                self.settings_search = v;
                self.settings_active_match = 0;
                if self.settings_search.trim().is_empty() {
                    return Task::none();
                }
                let ordered = self.settings_ordered_matches(&self.settings_search);
                if ordered.is_empty() {
                    return Task::none();
                }
                // Land the cursor on the first match in the OPEN section
                // if it has one (don't yank the user's section); else
                // open the document-first matching section.
                match ordered.iter().position(|(s, _)| *s == self.settings_section) {
                    Some(idx) => self.settings_active_match = idx,
                    None => {
                        self.settings_active_match = 0;
                        self.switch_settings_section_for_search(ordered[0].0);
                    }
                }
                // Keep the active match in view as the query narrows
                // (JetBrains-style). Scrolling the content pane doesn't
                // touch the search input's caret, so this is safe on
                // every change.
                return self.schedule_settings_scroll();
            }
            SettingsMessage::SettingsSearchStep(forward) => {
                let ordered = self.settings_ordered_matches(&self.settings_search);
                if ordered.is_empty() {
                    return Task::none();
                }
                let n = ordered.len();
                self.settings_active_match = if forward {
                    (self.settings_active_match + 1) % n
                } else {
                    (self.settings_active_match + n - 1) % n
                };
                let section = ordered[self.settings_active_match].0;
                if section != self.settings_section {
                    self.switch_settings_section_for_search(section);
                }
                return self.schedule_settings_scroll();
            }
            SettingsMessage::RevealSetting(section, label_key) => {
                // Palette entry point: put the setting's label in the
                // search box and open its section, so it lands on the
                // exact same highlight + scroll path as typing the query.
                self.settings_search = crate::i18n::t(label_key).to_string();
                self.keynav.pick_open = false;
                let t1 = self.update(Message::Navigation(
                    crate::app::NavigationMessage::ChangeView(View::Settings),
                ));
                let t2 = self.update(Message::Settings(
                    SettingsMessage::ChangeSettingsSection(section),
                ));
                let t3 = self.schedule_settings_scroll();
                return Task::batch([t1, t2, t3]);
            }
            SettingsMessage::RevealSettingScroll => {
                // Scroll the top matched row (tagged with
                // SETTINGS_SCROLL_TARGET_ID by the render) into view.
                // The operation reads real layout positions during
                // `operate`, so it works for rows scrolled far off the
                // bottom (which `draw` culls) - the whole reason the
                // old fixed-height / bounds-cell estimate mis-fired.
                if !self.settings_search.trim().is_empty() {
                    return crate::widgets::scroll_into_view_task(
                        self.settings_section.scroll_id(),
                        crate::keynav::SETTINGS_SCROLL_TARGET_ID,
                        16.0,
                    );
                }
            }
            SettingsMessage::ChangeSettingsSection(section) => {
                // Leaving the Shortcuts editor cancels any pending
                // capture; otherwise the next keystroke on the new
                // section would silently rebind the action.
                if self.settings_section == crate::state::SettingsSection::Shortcuts
                    && section != crate::state::SettingsSection::Shortcuts
                {
                    self.editing_hotkey = None;
                }
                self.settings_section = section;
                // A pick_list dropdown open on the old section unmounts
                // WITHOUT firing on_close when the section swaps, and a
                // stuck `pick_open` swallows Enter/Space/Esc/arrows
                // process-wide (live-QA bug: Enter dead in every
                // terminal after fiddling with the renderer dropdown).
                self.keynav.pick_open = false;
                // Keyboard navigation: the old section's rows are gone;
                // keep a sidebar (SubNav) selection alive through the
                // switch (keynav's own Enter path sets the flag) so
                // repeated Up/Down + Enter keep walking sections.
                let keep = self.keynav.keep_focus_through_change_view;
                self.keynav.keep_focus_through_change_view = false;
                if !keep {
                    self.keynav.focus = None;
                }
                self.keynav_clear_content();
                self.keynav.settings_row_actions.borrow_mut().clear();
                // Clicking another matching section while a search is
                // active scrolls that section's first match into view.
                if !self.settings_search.trim().is_empty() {
                    return Task::batch([
                        self.renderer_info_task(),
                        self.schedule_settings_scroll(),
                    ]);
                }
                // Sections remember where you left them (issue #120), so
                // hopping out to check a change and back lands on the same
                // row instead of at the top.
                return Task::batch([self.renderer_info_task(), self.settings_restore_scroll()]);
            }
            SettingsMessage::SectionScrolled(offset) => {
                self.settings_scroll.insert(self.settings_section, offset);
            }
            SettingsMessage::SectionScrollTo(id, y) => {
                return iced::widget::operation::snap_to(
                    id,
                    iced::widget::operation::RelativeOffset { x: None, y: Some(y) },
                );
            }
            SettingsMessage::StartEditingHotkey(action, slot) => {
                self.editing_hotkey = Some((action, slot));
            }
            SettingsMessage::MouseButtonPressed(button) => {
                return self.handle_mouse_button_press(button);
            }
            SettingsMessage::ResetHotkey(action) => {
                let mut defaults = crate::hotkeys::default_bindings();
                match defaults.remove(&action) {
                    Some(d) => self.hotkey_bindings.insert(action, d),
                    None => self.hotkey_bindings.remove(&action),
                };
                // Empty value persists the absence of an override, so
                // future boots rehydrate to the default. Same
                // semantics as deleting the row, and distinct from the
                // UNBOUND token a deliberate unbind writes.
                self.persist_setting(&format!("hotkey_{}", action.id()), "");
            }
            SettingsMessage::ResetAllHotkeys => {
                self.hotkey_bindings = crate::hotkeys::default_bindings();
                for action in crate::hotkeys::HotkeyAction::all() {
                    self.persist_setting(&format!("hotkey_{}", action.id()), "");
                }
            }
            SettingsMessage::SettingTogglePerformanceMode => {
                self.setting_performance_mode = !self.setting_performance_mode;
                self.persist_setting(
                    "performance_mode",
                    if self.setting_performance_mode { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingTogglePerfOverlay => {
                self.setting_perf_overlay = !self.setting_perf_overlay;
                self.persist_setting(
                    "perf_overlay",
                    if self.setting_perf_overlay { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingToggleRemoteDesktop => {
                self.remote_desktop_enabled = !self.remote_desktop_enabled;
                self.persist_setting(
                    "remote_desktop_enabled",
                    if self.remote_desktop_enabled { "true" } else { "false" },
                );
            }
            SettingsMessage::ToggleSecretVisibility(field) => {
                if !self.revealed_secrets.remove(&field) {
                    self.revealed_secrets.insert(field);
                }
            }
            SettingsMessage::SettingToggleCloseToTray => {
                self.setting_close_to_tray = !self.setting_close_to_tray;
                self.persist_setting(
                    "close_to_tray",
                    if self.setting_close_to_tray { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingToggleMinimizeToTray => {
                self.setting_minimize_to_tray = !self.setting_minimize_to_tray;
                // The Win32 subclass that intercepts the OS minimize
                // verbs can't read app state, so the toggle has to be
                // mirrored down to it or it keeps acting on the value
                // this process booted with.
                crate::tray::set_minimize_to_tray(self.setting_minimize_to_tray);
                self.persist_setting(
                    "minimize_to_tray",
                    if self.setting_minimize_to_tray { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingToggleSftpEnabled => {
                self.sftp_enabled = !self.sftp_enabled;
                self.persist_setting(
                    "sftp_enabled",
                    if self.sftp_enabled { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingKeepaliveChanged(val) => {
                // Accept only digits; cap at 86_400 (1 day) so users can't
                // accidentally type a runaway value.
                self.setting_keepalive_interval = sanitize_uint(&val, 86_400);
                self.persist_setting("keepalive_interval", &self.setting_keepalive_interval);
            }
            SettingsMessage::SettingCloudAutoRefreshToggle => {
                self.setting_cloud_auto_refresh_enabled =
                    !self.setting_cloud_auto_refresh_enabled;
                self.persist_setting(
                    "cloud_auto_refresh_enabled",
                    if self.setting_cloud_auto_refresh_enabled { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingCloudAutoRefreshIntervalChanged(val) => {
                // Floor of 1 minute, ceiling of 1 day. AWS rate limits
                // are well above a per-minute pace for the discovery
                // calls we make, but the ceiling is just a sanity cap.
                self.setting_cloud_auto_refresh_interval_minutes =
                    sanitize_uint(&val, 1_440);
                if self.setting_cloud_auto_refresh_interval_minutes == "0" {
                    self.setting_cloud_auto_refresh_interval_minutes = "1".into();
                }
                self.persist_setting(
                    "cloud_auto_refresh_interval_minutes",
                    &self.setting_cloud_auto_refresh_interval_minutes,
                );
            }
            SettingsMessage::SettingCloudAutoArchiveToggle => {
                self.setting_cloud_auto_archive_orphans =
                    !self.setting_cloud_auto_archive_orphans;
                self.persist_setting(
                    "cloud_auto_archive_orphans",
                    if self.setting_cloud_auto_archive_orphans { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingCloudOrphanArchiveDaysChanged(val) => {
                // Floor of 1 day (an orphan needs at least one full day
                // to "settle" so a transient AWS API hiccup doesn't
                // wipe legitimate hosts). Ceiling of one year.
                self.setting_cloud_orphan_archive_days = sanitize_uint(&val, 365);
                if self.setting_cloud_orphan_archive_days == "0" {
                    self.setting_cloud_orphan_archive_days = "1".into();
                }
                self.persist_setting(
                    "cloud_orphan_archive_days",
                    &self.setting_cloud_orphan_archive_days,
                );
            }
            SettingsMessage::SettingSftpConcurrencyChanged(val) => {
                // Cap at 8, beyond that the SSH channel multiplexer
                // overhead outweighs the throughput gain on most links.
                self.setting_sftp_concurrency = sanitize_uint(&val, 8);
                if self.setting_sftp_concurrency == "0" {
                    self.setting_sftp_concurrency = "1".into();
                }
                self.persist_setting("sftp_concurrency", &self.setting_sftp_concurrency);
            }
            SettingsMessage::SettingSftpConnectTimeoutChanged(val) => {
                self.setting_sftp_connect_timeout = sanitize_uint(&val, 600);
                if self.setting_sftp_connect_timeout == "0" {
                    self.setting_sftp_connect_timeout = "1".into();
                }
                self.persist_setting(
                    "sftp_connect_timeout",
                    &self.setting_sftp_connect_timeout,
                );
            }
            SettingsMessage::SettingSftpAuthTimeoutChanged(val) => {
                self.setting_sftp_auth_timeout = sanitize_uint(&val, 600);
                if self.setting_sftp_auth_timeout == "0" {
                    self.setting_sftp_auth_timeout = "1".into();
                }
                self.persist_setting("sftp_auth_timeout", &self.setting_sftp_auth_timeout);
            }
            SettingsMessage::SettingSftpSessionTimeoutChanged(val) => {
                self.setting_sftp_session_timeout = sanitize_uint(&val, 600);
                if self.setting_sftp_session_timeout == "0" {
                    self.setting_sftp_session_timeout = "1".into();
                }
                self.persist_setting(
                    "sftp_session_timeout",
                    &self.setting_sftp_session_timeout,
                );
            }
            SettingsMessage::SettingSftpOpTimeoutChanged(val) => {
                self.setting_sftp_op_timeout = sanitize_uint(&val, 600);
                if self.setting_sftp_op_timeout == "0" {
                    self.setting_sftp_op_timeout = "1".into();
                }
                // Apply live to both panes' active SFTP clients so the
                // user doesn't have to reconnect to feel the change.
                let to = self.sftp_op_timeout();
                if let Some(client) = &self.sftp.left.client {
                    client.set_op_timeout(to);
                }
                if let Some(client) = &self.sftp.right.client {
                    client.set_op_timeout(to);
                }
                self.persist_setting("sftp_op_timeout", &self.setting_sftp_op_timeout);
            }
            SettingsMessage::SettingToggleAutoReconnect => {
                self.setting_auto_reconnect = !self.setting_auto_reconnect;
                self.persist_setting(
                    "auto_reconnect",
                    if self.setting_auto_reconnect { "true" } else { "false" },
                );
            }
            SettingsMessage::SettingMaxReconnectChanged(val) => {
                self.setting_max_reconnect_attempts = sanitize_uint(&val, 100);
                self.persist_setting(
                    "max_reconnect_attempts",
                    &self.setting_max_reconnect_attempts,
                );
            }
            SettingsMessage::SettingAutoLockChanged(val) => {
                self.setting_auto_lock_minutes = sanitize_uint(&val, 1440);
                self.persist_setting("auto_lock_minutes", &self.setting_auto_lock_minutes);
            }
            SettingsMessage::AutoLockTick => {
                // Idle check. Guarded on Unlocked so a tick racing the
                // lock is a no-op, and on a parseable non-zero threshold
                // (the subscription only mounts then, but the setting can
                // change between mount and fire).
                let minutes = self
                    .setting_auto_lock_minutes
                    .parse::<u64>()
                    .ok()
                    .filter(|m| *m > 0);
                if let Some(minutes) = minutes
                    && self.vault_ui.state == crate::state::VaultState::Unlocked
                    // Without a master password, locking reopens
                    // immediately; auto-locking would just churn.
                    && self.vault_ui.has_user_password
                    && self.last_user_activity.elapsed().as_secs() >= minutes * 60
                {
                    tracing::info!("vault auto-lock after {minutes} min idle");
                    return Task::done(Message::Vault(VaultMessage::AutoLockVault));
                }
            }
            SettingsMessage::ConnectAnimTick => {
                self.connect_anim_tick = self.connect_anim_tick.wrapping_add(1);
            }
            SettingsMessage::AutoReconnectTick => {
                // Liveness sweep, independent of the auto-reconnect setting.
                // A pane whose SSH writer task has died reports
                // `is_alive() == false` while its reader may still be
                // draining output: the tab looks "connected" but silently
                // swallows every keystroke (the writer's `send` errors and
                // the input sites discard it). Nothing else checks
                // `is_alive`, so without this such a pane stays a dead
                // input sink forever. Surface it as a real disconnect so the
                // UI updates and, when enabled, reconnect kicks in. Panes
                // already torn down have `session == None` and are
                // skipped, so this can't loop.
                let dead: Vec<_> = self
                    .tabs
                    .iter()
                    .flat_map(|t| t.pane_grid.panes.values())
                    .filter(|p| p.session.as_ref().is_some_and(|s| !s.is_alive()))
                    .map(|p| p.id)
                    .collect();
                if !dead.is_empty() {
                    return Task::batch(
                        dead.into_iter()
                            .map(|id| Task::done(Message::Ssh(SshMessage::SshDisconnected(id)))),
                    );
                }
                if !self.setting_auto_reconnect {
                    // fall through, nothing to do
                } else {
                    let max_attempts: u32 =
                        self.setting_max_reconnect_attempts.parse().unwrap_or(5);
                    // Find the first disconnected SSH tab whose counter is under the limit.
                    // Only reconnect one per tick to avoid thrashing; next tick picks up
                    // the next candidate.
                    let candidate: Option<usize> = (0..self.tabs.len()).find(|&i| {
                        let tab = &self.tabs[i];
                        if !tab.label.ends_with(" (disconnected)") {
                            return false;
                        }
                        // Never auto-reconnect a split tab: `ReconnectTab`
                        // removes + rebuilds the whole tab, which would kill
                        // the live sibling panes. (Belt + suspenders: a
                        // multi-pane tab isn't relabeled "(disconnected)" in
                        // the first place, see `SshDisconnected`.)
                        if tab.pane_grid.panes.len() > 1 {
                            return false;
                        }
                        let base = tab.label.trim_end_matches(" (disconnected)");
                        // Quick-connect hosts resolve via the same label
                        // lookup; their counters key on the ephemeral id,
                        // which is stable for the life of the entry.
                        let Some(conn) = self.any_connection_by_label(base) else {
                            return false;
                        };
                        let attempts = self.reconnect_counters.get(&conn.id).copied().unwrap_or(0);
                        attempts < max_attempts
                    });
                    if let Some(tab_idx) = candidate {
                        let base = self.tabs[tab_idx]
                            .label
                            .trim_end_matches(" (disconnected)")
                            .to_string();
                        if let Some(cid) = self.any_connection_by_label(&base).map(|c| c.id) {
                            let entry = self.reconnect_counters.entry(cid).or_insert(0);
                            *entry += 1;
                        }
                        return Task::done(Message::Tabs(TabsMessage::ReconnectTab(tab_idx)));
                    }
                }
            }
        }
        Task::none()
    }
}
