//! `Oryxis::handle_settings`, match arms for the Settings panel:
//! terminal/SFTP/SSH knobs, app theme, language, auto-reconnect tick,
//! OS-detection toggles, vault lock, font size adjustments.

#![allow(clippy::result_large_err)]

pub(crate) use iced::Task;

pub(crate) use std::sync::{Arc, Mutex};
pub(crate) use tokio_stream::wrappers::UnboundedReceiverStream;

pub(crate) use oryxis_terminal::widget::TerminalState;

pub(crate) use crate::app::{Message, Oryxis, DEFAULT_TERM_COLS, DEFAULT_TERM_ROWS};
pub(crate) use crate::state::{TerminalTab, VaultState, View};
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

/// Map the active app theme to its companion terminal palette. Used
/// as the bottom-of-the-stack fallback in
/// `resolve_global_terminal_theme` when neither a global override nor a
/// per-host override is set. Every app theme has a matching palette
/// of the same name.
fn app_theme_to_terminal(theme: AppTheme) -> oryxis_terminal::TerminalTheme {
    match theme {
        AppTheme::OryxisDark => oryxis_terminal::TerminalTheme::OryxisDark,
        AppTheme::OryxisLight => oryxis_terminal::TerminalTheme::OryxisLight,
        AppTheme::Termius => oryxis_terminal::TerminalTheme::Termius,
        AppTheme::Darcula => oryxis_terminal::TerminalTheme::Darcula,
        AppTheme::IslandsDark => oryxis_terminal::TerminalTheme::IslandsDark,
        AppTheme::Dracula => oryxis_terminal::TerminalTheme::Dracula,
        AppTheme::Monokai => oryxis_terminal::TerminalTheme::Monokai,
        AppTheme::HackerGreen => oryxis_terminal::TerminalTheme::HackerGreen,
        AppTheme::Nord => oryxis_terminal::TerminalTheme::Nord,
        AppTheme::NordLight => oryxis_terminal::TerminalTheme::NordLight,
        AppTheme::SolarizedDark => oryxis_terminal::TerminalTheme::SolarizedDark,
        AppTheme::SolarizedLight => oryxis_terminal::TerminalTheme::SolarizedLight,
        AppTheme::PaperLight => oryxis_terminal::TerminalTheme::PaperLight,
    }
}

// Settings-dispatch helpers split into sibling files.
mod shell;
mod themes;
use shell::*;
use themes::*;

impl Oryxis {
    /// The curated local terminals as launch payloads, in list order.
    /// Empty when never scanned or genuinely empty (the caller decides
    /// what to do with an empty list).
    pub(crate) fn local_terminal_specs(&self) -> Vec<crate::state::LocalShellSpec> {
        self.local_terminals
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|e| e.to_spec())
            .collect()
    }

    /// Persist the curated local-terminal list to the `local_terminals`
    /// setting as JSON. Machine-local config, never synced or exported.
    pub(crate) fn persist_local_terminals(&self) {
        let json = serde_json::to_string(self.local_terminals.as_deref().unwrap_or(&[]))
            .unwrap_or_else(|_| "[]".to_string());
        if let Some(vault) = self.vault.as_ref()
            && let Err(e) = vault.set_setting("local_terminals", &json)
        {
            tracing::warn!("Failed to persist local_terminals: {e}");
        }
    }

    /// Persist the "always open X" preference (the entry key, or empty
    /// for "always ask").
    pub(crate) fn persist_local_terminal_default(&self) {
        let value = self
            .local_terminal_default
            .map(|id| id.to_string())
            .unwrap_or_default();
        if let Some(vault) = self.vault.as_ref()
            && let Err(e) = vault.set_setting("local_terminal_default", &value)
        {
            tracing::warn!("Failed to persist local_terminal_default: {e}");
        }
    }

    /// Decide how to satisfy an "open local terminal" intent against the
    /// already-scanned list: honor a valid "always open X" default, else
    /// spawn directly when there's nothing to choose (0 or 1 entry), else
    /// show the picker. Assumes `local_terminals` is `Some`.
    fn decide_open_local_terminal(&mut self) -> Task<Message> {
        // "Always open X": a default id still present in the list spawns
        // straight away. A dangling id falls through to the count logic.
        if let Some(id) = self.local_terminal_default
            && let Some(spec) = self
                .local_terminals
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .find(|e| e.id == id)
                .map(|e| e.to_spec())
        {
            return spawn_local_shell(self, Some((spec.program, spec.args, spec.label)));
        }
        let specs = self.local_terminal_specs();
        match specs.as_slice() {
            // Nothing curated: fall back to the OS default shell.
            [] => spawn_local_shell(self, None),
            [only] => spawn_local_shell(
                self,
                Some((only.program.clone(), only.args.clone(), only.label.clone())),
            ),
            _ => Task::done(Message::ShowLocalShellPicker),
        }
    }

    /// Effective terminal palette for callers that don't have a
    /// specific connection in mind: settings preview, local-shell tabs,
    /// new-tab spawn defaults. Order: explicit user override → app
    /// theme mapping.
    /// Resolve a theme NAME (built-in or user-defined) to its palette.
    /// `None` when the name matches neither (e.g. a custom theme the user
    /// deleted), so callers fall through to their default.
    pub(crate) fn terminal_palette_for_name(
        &self,
        name: &str,
    ) -> Option<oryxis_terminal::TerminalPalette> {
        if let Some(theme) =
            oryxis_terminal::TerminalTheme::ALL.iter().find(|t| t.name() == name)
        {
            return Some(theme.palette());
        }
        self.custom_terminal_themes
            .iter()
            .find(|t| t.name == name)
            .map(custom_theme_palette)
    }

    /// Effective global terminal palette: explicit user override (built-in
    /// or custom) → app theme mapping.
    pub(crate) fn resolve_global_terminal_palette(
        &self,
    ) -> oryxis_terminal::TerminalPalette {
        if let Some(name) = &self.terminal_theme_override
            && let Some(palette) = self.terminal_palette_for_name(name)
        {
            return palette;
        }
        app_theme_to_terminal(AppTheme::active()).palette()
    }

    /// Display name of the effective global terminal theme (for the
    /// "inherit (Global)" label). Keeps a stale override name from showing
    /// once the custom theme behind it is deleted.
    pub(crate) fn resolve_global_terminal_theme_name(&self) -> String {
        if let Some(name) = &self.terminal_theme_override
            && self.terminal_palette_for_name(name).is_some()
        {
            return name.clone();
        }
        app_theme_to_terminal(AppTheme::active()).name().to_string()
    }

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

    /// Whether Privacy Mode is active for a connection. Per-host
    /// override (`Connection.privacy_mode`) wins over the global
    /// `setting_privacy_mode`; `None` inherits the global default.
    pub(crate) fn privacy_active(&self, conn: &oryxis_core::models::Connection) -> bool {
        conn.privacy_mode.unwrap_or(self.setting_privacy_mode)
    }

    /// Privacy Mode for a terminal pane, resolved from its label. Host
    /// panes match a saved connection (so the per-host override applies);
    /// local shells / WSL / PowerShell fall back to the global default.
    pub(crate) fn privacy_active_for_label(&self, label: &str) -> bool {
        let base = label.trim_end_matches(" (disconnected)");
        self.connections
            .iter()
            .find(|c| c.label == base)
            .map(|c| self.privacy_active(c))
            .unwrap_or(self.setting_privacy_mode)
    }

    /// Effective terminal palette for a known `Connection`. Per-host
    /// override wins, then the global override, then the app theme.
    pub(crate) fn resolve_terminal_palette_for_connection(
        &self,
        conn: &oryxis_core::models::Connection,
    ) -> oryxis_terminal::TerminalPalette {
        if let Some(name) = &conn.terminal_theme
            && let Some(palette) = self.terminal_palette_for_name(name)
        {
            return palette;
        }
        self.resolve_global_terminal_palette()
    }

    /// Same resolution but starting from a tab label. Used by repaint
    /// loops where we don't already hold a `Connection` reference.
    /// Falls through to the global theme for tabs without a matching
    /// connection (local shells, WSL, PowerShell, …).
    fn resolve_terminal_palette_for_label(
        &self,
        label: &str,
    ) -> oryxis_terminal::TerminalPalette {
        let base = label.trim_end_matches(" (disconnected)");
        if let Some(conn) = self.connections.iter().find(|c| c.label == base) {
            return self.resolve_terminal_palette_for_connection(conn);
        }
        self.resolve_global_terminal_palette()
    }

    /// Re-paint every open tab's palette. Use after a global theme
    /// change. Tabs whose connection has its own override pick that
    /// override up automatically through `resolve_terminal_palette_for_label`.
    pub(crate) fn repaint_all_terminal_palettes(&self) {
        for tab in &self.tabs {
            let palette = self.resolve_terminal_palette_for_label(&tab.label);
            for pane in tab.pane_grid.panes.values() {
                if let Ok(mut state) = pane.terminal.lock() {
                    state.palette = palette.clone();
                }
            }
        }
    }

    /// Apply the session-only local terminal theme to every open
    /// local/ephemeral pane (panes without a saved host). Host panes keep
    /// their own resolution. `None` falls back to the global palette.
    pub(crate) fn apply_local_terminal_palette(&self) {
        let palette = match &self.local_terminal_theme {
            Some(name) => self
                .terminal_palette_for_name(name)
                .unwrap_or_else(|| self.resolve_global_terminal_palette()),
            None => self.resolve_global_terminal_palette(),
        };
        for tab in &self.tabs {
            for pane in tab.pane_grid.panes.values() {
                if matches!(pane.origin, crate::state::PaneOrigin::Host(_)) {
                    continue;
                }
                if let Ok(mut state) = pane.terminal.lock() {
                    state.palette = palette.clone();
                }
            }
        }
    }

    /// Re-paint only the tabs attached to a single host's label.
    /// Called when the per-host override changes.
    pub(crate) fn repaint_terminal_palettes_for_label(&self, label: &str) {
        let palette = self.resolve_terminal_palette_for_label(label);
        let base = label.trim_end_matches(" (disconnected)");
        for tab in &self.tabs {
            let tab_base = tab.label.trim_end_matches(" (disconnected)");
            if tab_base != base {
                continue;
            }
            for pane in tab.pane_grid.panes.values() {
                if let Ok(mut state) = pane.terminal.lock() {
                    state.palette = palette.clone();
                }
            }
        }
    }
}

impl Oryxis {
    /// Task that asks iced for the graphics backend the compositor
    /// actually selected, but only when the Interface settings section
    /// (which displays it) is showing and it hasn't loaded yet. By then
    /// the compositor exists, so the oneshot resolves instead of being
    /// dropped. Returns [`Task::none`] otherwise. Fired both when
    /// switching into the section and when opening Settings on it.
    pub(crate) fn renderer_info_task(&self) -> Task<Message> {
        if self.settings_section == crate::state::SettingsSection::Interface
            && self.renderer_active.is_none()
        {
            iced::system::graphics_information()
                .map(|info| Message::RendererInfoLoaded(info.backend, info.adapter))
        } else {
            Task::none()
        }
    }

    pub(crate) fn handle_settings(
        &mut self,
        message: Message,
    ) -> Result<Task<Message>, Message> {
        match message {
            // -- Settings --
            Message::TerminalThemeChanged(name) => {
                // Empty string == "follow app theme". Anything else is
                // matched against the known theme names; an unknown
                // string is ignored so a typo'd setting can't lock
                // the user out of the picker.
                if name.is_empty() {
                    self.terminal_theme_override = None;
                    self.persist_setting("terminal_theme_override", "");
                } else if self.terminal_palette_for_name(&name).is_some() {
                    // Built-in or custom theme name.
                    self.terminal_theme_override = Some(name.clone());
                    self.persist_setting("terminal_theme_override", &name);
                } else {
                    return Ok(Task::none());
                }
                self.terminal_palette = self.resolve_global_terminal_palette();
                self.repaint_all_terminal_palettes();
            }
            Message::LocalConfigThemeChanged(name) => {
                // Session-only override for local/ephemeral panes. Empty =
                // follow the global terminal theme. Unknown names ignored.
                if name.is_empty() {
                    self.local_terminal_theme = None;
                } else if self.terminal_palette_for_name(&name).is_some() {
                    self.local_terminal_theme = Some(name);
                } else {
                    return Ok(Task::none());
                }
                self.apply_local_terminal_palette();
            }
            Message::LocalConfigSaveGlobal => {
                // Promote the session override to the persisted global
                // default, then drop it (the panes now follow global).
                if let Some(name) = self.local_terminal_theme.take() {
                    self.terminal_theme_override = Some(name.clone());
                    self.persist_setting("terminal_theme_override", &name);
                    self.terminal_palette = self.resolve_global_terminal_palette();
                    self.repaint_all_terminal_palettes();
                }
            }
            Message::ThemeEditorOpenPicker(slot) => {
                self.theme_color_popover = Some((slot, self.mouse_position));
            }
            Message::ThemeEditorClosePicker => {
                self.theme_color_popover = None;
            }
            Message::ThemeCardHovered(idx) => {
                self.hovered_theme_card = Some(idx);
            }
            Message::ThemeCardUnhovered => {
                self.hovered_theme_card = None;
            }
            Message::ThemeEditorNew => {
                // Seed from the active terminal palette so the user starts
                // from the currently-selected theme.
                let p = self.terminal_palette.clone();
                let hex = |c: iced::Color| {
                    let q = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
                    format!("#{:02x}{:02x}{:02x}", q(c.r), q(c.g), q(c.b))
                };
                self.theme_editor = Some(crate::state::ThemeEditorForm {
                    editing_id: None,
                    name: String::new(),
                    foreground: hex(p.foreground),
                    background: hex(p.background),
                    cursor: hex(p.cursor),
                    ansi: std::array::from_fn(|i| hex(p.ansi[i])),
                    error: None,
                });
            }
            Message::ThemeImportOpen => {
                self.show_theme_import = true;
                self.theme_import_content = iced::widget::text_editor::Content::new();
                self.theme_import_name.clear();
                self.theme_import_error = None;
            }
            Message::ThemeImportClose => {
                self.show_theme_import = false;
            }
            Message::ThemeImportContentAction(action) => {
                self.theme_import_content.perform(action);
                self.theme_import_error = None;
            }
            Message::ThemeImportNameChanged(v) => {
                self.theme_import_name = v;
            }
            Message::ThemeImportApply => {
                let content = self.theme_import_content.text();
                let name = if self.theme_import_name.trim().is_empty() {
                    crate::i18n::t("theme_imported_default").to_string()
                } else {
                    self.theme_import_name.trim().to_string()
                };
                match crate::theme_import::parse_theme(&content, &name) {
                    Ok(theme) => {
                        // Open the parsed colors in the editor (as a new
                        // theme) so the user can review / rename before save.
                        let mut form = crate::state::ThemeEditorForm::from_theme(&theme);
                        form.editing_id = None;
                        self.theme_editor = Some(form);
                        self.show_theme_import = false;
                    }
                    Err(e) => self.theme_import_error = Some(e),
                }
            }
            // -- Custom UI (chrome) themes --
            Message::UiThemeEditorNew => {
                // Seed from the currently active chrome colors so the user
                // starts from a working theme.
                let seed = crate::theme::theme_colors_to_hex(crate::theme::OryxisColors::t());
                self.ui_theme_editor =
                    Some(crate::state::UiThemeEditorForm::new_from_colors(seed));
            }
            Message::UiThemeEditorEdit(idx) => {
                if let Some(theme) = self.custom_ui_themes.get(idx) {
                    self.ui_theme_editor =
                        Some(crate::state::UiThemeEditorForm::from_theme(theme));
                }
            }
            Message::UiThemeEditorClose => {
                self.ui_theme_editor = None;
                self.ui_color_popover = None;
            }
            Message::UiThemeEditorNameChanged(name) => {
                if let Some(form) = &mut self.ui_theme_editor {
                    form.name = name;
                    form.error = None;
                }
            }
            Message::UiThemeColorChanged(idx, value) => {
                if let Some(form) = &mut self.ui_theme_editor
                    && idx < 21
                {
                    let cleaned: String = value
                        .chars()
                        .filter(|c| *c == '#' || c.is_ascii_hexdigit())
                        .take(7)
                        .collect();
                    form.colors[idx] = cleaned;
                }
            }
            Message::UiThemeEditorOpenPicker(idx) => {
                self.ui_color_popover = Some((idx, self.mouse_position));
            }
            Message::UiThemeEditorClosePicker => {
                self.ui_color_popover = None;
            }
            Message::UiThemeEditorSave => {
                if let Some(err) = self.save_ui_theme_editor() {
                    if let Some(form) = &mut self.ui_theme_editor {
                        form.error = Some(err);
                    }
                } else {
                    self.ui_theme_editor = None;
                    self.ui_color_popover = None;
                }
            }
            Message::UiThemeDelete(idx) => {
                if let Some(theme) = self.custom_ui_themes.get(idx)
                    && let Some(vault) = &self.vault
                {
                    let was_active = self.active_app_theme_name == theme.name;
                    let _ = vault.delete_custom_ui_theme(&theme.id);
                    self.custom_ui_themes =
                        vault.list_custom_ui_themes().unwrap_or_default();
                    if was_active {
                        // The active theme is gone; fall back to the default.
                        crate::theme::AppTheme::set_active(
                            crate::theme::AppTheme::OryxisDark,
                        );
                        self.active_app_theme_name = "Oryxis Dark".to_string();
                        self.persist_setting("app_theme", "Oryxis Dark");
                        self.terminal_palette = self.resolve_global_terminal_palette();
                        self.repaint_all_terminal_palettes();
                    }
                }
            }
            Message::UiThemeCardHovered(idx) => {
                self.hovered_ui_theme_card = Some(idx);
            }
            Message::UiThemeCardUnhovered => {
                self.hovered_ui_theme_card = None;
            }
            Message::ThemeEditorEdit(idx) => {
                if let Some(theme) = self.custom_terminal_themes.get(idx) {
                    self.theme_editor =
                        Some(crate::state::ThemeEditorForm::from_theme(theme));
                }
            }
            Message::ThemeEditorClose => {
                self.close_modal(crate::state::Modal::ThemeEditor);
            }
            Message::ThemeEditorNameChanged(name) => {
                if let Some(form) = &mut self.theme_editor {
                    form.name = name;
                    form.error = None;
                }
            }
            Message::ThemeEditorColorChanged(slot, value) => {
                if let Some(form) = &mut self.theme_editor {
                    // Keep only hex-ish characters so the live preview stays
                    // sane while typing; full validation happens on save.
                    let cleaned: String = value
                        .chars()
                        .filter(|c| *c == '#' || c.is_ascii_hexdigit())
                        .take(7)
                        .collect();
                    form.set_slot(slot, cleaned);
                }
            }
            Message::ThemeEditorSave => {
                if let Some(err) = self.save_theme_editor() {
                    if let Some(form) = &mut self.theme_editor {
                        form.error = Some(err);
                    }
                } else {
                    self.close_modal(crate::state::Modal::ThemeEditor);
                }
            }
            Message::ThemeDelete(idx) => {
                if let Some(theme) = self.custom_terminal_themes.get(idx)
                    && let Some(vault) = &self.vault
                {
                    let _ = vault.delete_custom_terminal_theme(&theme.id);
                    self.custom_terminal_themes =
                        vault.list_custom_terminal_themes().unwrap_or_default();
                    // A host / global override pointing at the deleted theme
                    // now resolves to its fallback; repaint reflects that.
                    self.terminal_palette = self.resolve_global_terminal_palette();
                    self.repaint_all_terminal_palettes();
                }
            }
            Message::LanguageChanged(name) => {
                use crate::i18n::Language;
                if let Some(lang) =
                    Language::ALL.iter().find(|l| l.name() == name).copied()
                {
                    Language::set_active(lang);
                    if let Some(vault) = &self.vault {
                        let _ = vault.set_setting("language", lang.code());
                    }
                    // Switching to a CJK language pulls its font on
                    // demand (once per session). Show a hint while it
                    // downloads; a cached font loads silently.
                    if let Some(code) = crate::fonts::asset_code(lang)
                        && !self.loaded_cjk_fonts.contains(code)
                    {
                        self.loaded_cjk_fonts.insert(code.to_string());
                        if !crate::fonts::is_language_cached(lang) {
                            self.toast = Some(
                                crate::i18n::t("cjk_font_downloading").to_string(),
                            );
                        }
                        return Ok(crate::fonts::ensure_task(lang));
                    }
                }
            }
            Message::CjkFontReady(code, result) => match result {
                Ok(bytes) => {
                    // Clear the "downloading" hint and register the font
                    // with the iced font system so cosmic-text can fall
                    // back to it. `iced::font::Error` is uninhabited, so
                    // the load result is discarded.
                    self.toast = None;
                    return Ok(iced::font::load(bytes).discard());
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
                    self.toast =
                        Some(crate::i18n::t("cjk_font_failed").to_string());
                    return Ok(Task::perform(
                        async {
                            tokio::time::sleep(
                                std::time::Duration::from_millis(2600),
                            )
                            .await;
                        },
                        |_| Message::ToastClear,
                    ));
                }
            },
            Message::FlattenHostsToggle => {
                self.flatten_hosts = !self.flatten_hosts;
                self.persist_setting(
                    "flatten_hosts",
                    if self.flatten_hosts { "true" } else { "false" },
                );
            }
            Message::LayoutDirectionChanged(name) => {
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
            Message::BellModeChanged(name) => {
                use crate::util::BellMode;
                if let Some(mode) = BellMode::ALL
                    .iter()
                    .find(|m| crate::i18n::t(m.label_key()) == name)
                {
                    self.setting_bell_mode = *mode;
                    self.persist_setting("terminal_bell_mode", mode.code());
                }
            }
            Message::ClipboardAccessChanged(name) => {
                use crate::util::ClipboardAccess;
                if let Some(mode) = ClipboardAccess::ALL
                    .iter()
                    .find(|m| crate::i18n::t(m.label_key()) == name)
                {
                    self.setting_clipboard_access = *mode;
                    self.persist_setting("terminal_clipboard_access", mode.code());
                    let (cw, cr) = mode.flags();
                    oryxis_terminal::set_clipboard_access(cw, cr);
                }
            }
            Message::NotificationModeChanged(name) => {
                use crate::util::NotificationMode;
                if let Some(mode) = NotificationMode::ALL
                    .iter()
                    .find(|m| crate::i18n::t(m.label_key()) == name)
                {
                    self.setting_notification_mode = *mode;
                    self.persist_setting("terminal_notification", mode.code());
                }
            }
            Message::AppThemeChanged(name) => {
                if self.apply_app_theme_name(&name) {
                    self.active_app_theme_name = name.clone();
                    self.persist_setting("app_theme", &name);
                    // Refresh the global derived palette and re-paint
                    // every tab. Tabs whose connection has its own
                    // terminal_theme override pick that up via
                    // `resolve_terminal_theme_for_label`, so the user's
                    // per-host pick survives an app theme switch.
                    self.terminal_palette = self.resolve_global_terminal_palette();
                    self.repaint_all_terminal_palettes();
                }
            }
            Message::TerminalFontSizeIncrease => {
                self.terminal_font_size = (self.terminal_font_size + 1.0).min(24.0);
                self.persist_setting(
                    "terminal_font_size",
                    &format!("{}", self.terminal_font_size),
                );
            }
            Message::TerminalFontSizeDecrease => {
                self.terminal_font_size = (self.terminal_font_size - 1.0).max(10.0);
                self.persist_setting(
                    "terminal_font_size",
                    &format!("{}", self.terminal_font_size),
                );
            }
            Message::TerminalFontChanged(name) => {
                self.terminal_font_name = name;
                self.persist_setting("terminal_font_name", &self.terminal_font_name);
            }
            Message::ChangeSettingsSection(section) => {
                // Leaving the Shortcuts editor cancels any pending
                // capture; otherwise the next keystroke on the new
                // section would silently rebind the action.
                if self.settings_section == crate::state::SettingsSection::Shortcuts
                    && section != crate::state::SettingsSection::Shortcuts
                {
                    self.editing_hotkey = None;
                }
                self.settings_section = section;
                return Ok(self.renderer_info_task());
            }
            Message::RendererInfoLoaded(backend, adapter) => {
                self.renderer_active = Some((backend, adapter));
            }
            Message::StartEditingHotkey(action) => {
                self.editing_hotkey = Some(action);
            }
            Message::ResetHotkey(action) => {
                let defaults = crate::hotkeys::default_bindings();
                if let Some(default_binding) = defaults.get(&action) {
                    self.hotkey_bindings.insert(action, *default_binding);
                }
                // Empty value persists the absence of an override, so
                // future boots rehydrate to the default. Same
                // semantics as deleting the row.
                self.persist_setting(&format!("hotkey_{}", action.id()), "");
            }
            Message::ResetAllHotkeys => {
                self.hotkey_bindings = crate::hotkeys::default_bindings();
                for action in crate::hotkeys::HotkeyAction::all() {
                    self.persist_setting(&format!("hotkey_{}", action.id()), "");
                }
            }
            Message::SettingRendererBackendChanged(mode) => {
                self.setting_renderer_backend = mode.clone();
                self.persist_setting("renderer_backend", &mode);
            }
            Message::ToggleCopyOnSelect => {
                self.setting_copy_on_select = !self.setting_copy_on_select;
                self.persist_setting(
                    "copy_on_select",
                    if self.setting_copy_on_select { "true" } else { "false" },
                );
            }
            Message::ToggleRightClickCopy => {
                self.setting_right_click_copy = !self.setting_right_click_copy;
                self.persist_setting(
                    "right_click_copy",
                    if self.setting_right_click_copy { "true" } else { "false" },
                );
            }
            Message::ToggleTerminalAutoTitle => {
                let on = !crate::state::auto_title_enabled();
                crate::state::set_auto_title(on);
                self.persist_setting("terminal_auto_title", if on { "true" } else { "false" });
            }
            Message::ToggleBoldIsBright => {
                self.setting_bold_is_bright = !self.setting_bold_is_bright;
                self.persist_setting(
                    "bold_is_bright",
                    if self.setting_bold_is_bright { "true" } else { "false" },
                );
            }
            Message::ToggleKeywordHighlight => {
                self.setting_keyword_highlight = !self.setting_keyword_highlight;
                self.persist_setting(
                    "keyword_highlight",
                    if self.setting_keyword_highlight { "true" } else { "false" },
                );
            }
            Message::TerminalLinkOpened => {
                // First successful ctrl-click on a link in this pane: the
                // hint did its job, retire it for the pane (HintMode::Once).
                // In-memory only, a fresh pane shows it again.
                if let Some(tab_idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(tab_idx)
                {
                    tab.active_mut().link_hint_shown = true;
                }
            }
            Message::ToggleSecretVisibility(field) => {
                if !self.revealed_secrets.remove(&field) {
                    self.revealed_secrets.insert(field);
                }
            }
            Message::HintModeChanged(name) => {
                use crate::i18n::t;
                use crate::util::HintMode;
                if let Some(mode) = HintMode::ALL.iter().find(|m| t(m.label_key()) == name) {
                    self.setting_hint_mode = *mode;
                    self.persist_setting("terminal_hint_mode", mode.code());
                }
            }
            Message::ToggleSmartContrast => {
                self.setting_smart_contrast = !self.setting_smart_contrast;
                self.persist_setting(
                    "smart_contrast",
                    if self.setting_smart_contrast { "true" } else { "false" },
                );
            }
            Message::SettingToggleShowStatusBar => {
                self.setting_show_status_bar = !self.setting_show_status_bar;
                self.persist_setting(
                    "show_status_bar",
                    if self.setting_show_status_bar { "true" } else { "false" },
                );
            }
            Message::ToggleHostListView => {
                // Dismiss the `…` overflow menu when toggled from there
                // (no-op for the inline toolbar button).
                self.overlay = None;
                self.setting_host_list_view = !self.setting_host_list_view;
                self.persist_setting(
                    "host_list_view",
                    if self.setting_host_list_view { "true" } else { "false" },
                );
            }
            Message::ToggleCardAccentGlass => {
                self.setting_card_accent_glass = !self.setting_card_accent_glass;
                self.persist_setting(
                    "card_accent_glass",
                    if self.setting_card_accent_glass { "true" } else { "false" },
                );
            }
            Message::ToggleShowHostAddress => {
                self.setting_show_host_address = !self.setting_show_host_address;
                self.persist_setting(
                    "show_host_address",
                    if self.setting_show_host_address { "true" } else { "false" },
                );
            }
            Message::TogglePrivacyMode => {
                self.setting_privacy_mode = !self.setting_privacy_mode;
                self.persist_setting(
                    "privacy_mode",
                    if self.setting_privacy_mode { "true" } else { "false" },
                );
            }
            Message::SettingToggleCloseToTray => {
                self.setting_close_to_tray = !self.setting_close_to_tray;
                self.persist_setting(
                    "close_to_tray",
                    if self.setting_close_to_tray { "true" } else { "false" },
                );
            }
            Message::SettingToggleMinimizeToTray => {
                self.setting_minimize_to_tray = !self.setting_minimize_to_tray;
                self.persist_setting(
                    "minimize_to_tray",
                    if self.setting_minimize_to_tray { "true" } else { "false" },
                );
            }
            Message::SettingToggleShowTabStatusDot => {
                self.setting_show_tab_status_dot = !self.setting_show_tab_status_dot;
                self.persist_setting(
                    "show_tab_status_dot",
                    if self.setting_show_tab_status_dot { "true" } else { "false" },
                );
            }
            Message::SettingToggleTabAccentLine => {
                self.setting_tab_accent_line = !self.setting_tab_accent_line;
                self.persist_setting(
                    "tab_accent_line",
                    if self.setting_tab_accent_line { "true" } else { "false" },
                );
            }
            Message::SettingToggleTabAccentWash => {
                self.setting_tab_accent_wash = !self.setting_tab_accent_wash;
                self.persist_setting(
                    "tab_accent_wash",
                    if self.setting_tab_accent_wash { "true" } else { "false" },
                );
            }
            Message::SettingToggleSftpEnabled => {
                self.sftp_enabled = !self.sftp_enabled;
                self.persist_setting(
                    "sftp_enabled",
                    if self.sftp_enabled { "true" } else { "false" },
                );
            }
            Message::SettingNavOrientationChanged(val) => {
                let normalized = match val.as_str() {
                    "vertical" => "vertical",
                    _ => "horizontal",
                };
                self.setting_nav_orientation = normalized.into();
                self.persist_setting("nav_orientation", normalized);
            }
            Message::ToggleNavRailExpanded => {
                self.setting_nav_rail_expanded = !self.setting_nav_rail_expanded;
                self.persist_setting(
                    "nav_rail_expanded",
                    if self.setting_nav_rail_expanded { "true" } else { "false" },
                );
            }
            Message::SettingDefaultHostIconChanged(val) => {
                let normalized = match val.as_str() {
                    "square" => "square",
                    "rounded" => "rounded",
                    "outline" => "outline",
                    "initials" => "initials",
                    _ => "circular",
                };
                self.setting_default_host_icon = normalized.into();
                self.persist_setting("default_host_icon", normalized);
            }
            Message::SettingTabCloseButtonSideChanged(val) => {
                // Only accept the two known values; anything else
                // collapses to the default so an unknown pick from a
                // future build can't wedge the tab bar.
                let normalized = match val.as_str() {
                    "right" => "right",
                    _ => "left",
                };
                self.setting_tab_close_button_side = normalized.into();
                self.persist_setting("tab_close_button_side", normalized);
            }
            Message::SettingPinnedTabStyleChanged(val) => {
                let normalized = match val.as_str() {
                    "full" => "full",
                    _ => "compact",
                };
                self.setting_pinned_tab_style = normalized.into();
                self.persist_setting("pinned_tab_style", normalized);
            }
            Message::SettingTabFillStyleChanged(val) => {
                let normalized = match val.as_str() {
                    "solid" => "solid",
                    _ => "gradient",
                };
                self.setting_tab_fill_style = normalized.into();
                self.persist_setting("tab_fill_style", normalized);
            }
            Message::SettingKeepaliveChanged(val) => {
                // Accept only digits; cap at 86_400 (1 day) so users can't
                // accidentally type a runaway value.
                self.setting_keepalive_interval = sanitize_uint(&val, 86_400);
                self.persist_setting("keepalive_interval", &self.setting_keepalive_interval);
            }
            Message::ToggleDefaultAgentForwarding => {
                self.setting_default_agent_forwarding = !self.setting_default_agent_forwarding;
                self.persist_setting(
                    "default_agent_forwarding",
                    if self.setting_default_agent_forwarding { "true" } else { "false" },
                );
            }
            Message::DefaultPortChanged(val) => {
                self.setting_default_port = sanitize_uint(&val, 65_535);
                self.persist_setting("default_port", &self.setting_default_port);
            }
            Message::DefaultKeepaliveChanged(val) => {
                // Empty stays empty (= inherit the global keepalive); otherwise
                // digits capped at 1 day.
                self.setting_default_keepalive = if val.trim().is_empty() {
                    String::new()
                } else {
                    sanitize_uint(&val, 86_400)
                };
                self.persist_setting("default_keepalive", &self.setting_default_keepalive);
            }
            Message::DefaultTerminalTypeChanged(val) => {
                self.setting_default_terminal_type = val;
                self.persist_setting("default_terminal_type", &self.setting_default_terminal_type);
            }
            Message::DefaultUsernameChanged(val) => {
                self.setting_default_username = val;
                self.persist_setting("default_username", &self.setting_default_username);
            }
            Message::DefaultAuthMethodChanged(val) => {
                self.setting_default_auth_method = crate::util::auth_method_from_label(&val);
                self.persist_setting(
                    "default_auth_method",
                    &crate::util::auth_method_to_setting(&self.setting_default_auth_method),
                );
            }
            Message::DefaultIdentityChanged(val) => {
                // The picker emits a saved identity's label or the localized
                // "(none)" sentinel; map back to the stable UUID.
                self.setting_default_identity_id = if val == crate::i18n::t("new_default_none") {
                    None
                } else {
                    self.identities.iter().find(|i| i.label == val).map(|i| i.id)
                };
                self.persist_setting(
                    "default_identity_id",
                    &self.setting_default_identity_id.map(|id| id.to_string()).unwrap_or_default(),
                );
            }
            Message::DefaultKeyChanged(val) => {
                self.setting_default_key_id = if val == crate::i18n::t("new_default_none") {
                    None
                } else {
                    self.keys.iter().find(|k| k.label == val).map(|k| k.id)
                };
                self.persist_setting(
                    "default_key_id",
                    &self.setting_default_key_id.map(|id| id.to_string()).unwrap_or_default(),
                );
            }
            Message::DefaultGroupChanged(val) => {
                self.setting_default_group_id = if val == crate::i18n::t("new_default_none") {
                    None
                } else {
                    self.groups.iter().find(|g| g.label == val).map(|g| g.id)
                };
                self.persist_setting(
                    "default_group_id",
                    &self.setting_default_group_id.map(|id| id.to_string()).unwrap_or_default(),
                );
            }
            Message::DefaultProxyChanged(val) => {
                // Default proxy is a saved Proxy Identity reference; inline
                // proxies are per-host by nature and aren't defaulted.
                self.setting_default_proxy_identity_id =
                    if val == crate::i18n::t("new_default_none") {
                        None
                    } else {
                        self.proxy_identities.iter().find(|p| p.label == val).map(|p| p.id)
                    };
                self.persist_setting(
                    "default_proxy_identity_id",
                    &self
                        .setting_default_proxy_identity_id
                        .map(|id| id.to_string())
                        .unwrap_or_default(),
                );
            }
            Message::ToggleDefaultMcpEnabled => {
                self.setting_default_mcp_enabled = !self.setting_default_mcp_enabled;
                self.persist_setting(
                    "default_mcp_enabled",
                    if self.setting_default_mcp_enabled { "true" } else { "false" },
                );
            }
            Message::DefaultEncodingChanged(val) => {
                // "UTF-8" maps to None (no override), like the host editor.
                self.setting_default_encoding = if val == "UTF-8" { None } else { Some(val) };
                self.persist_setting(
                    "default_encoding",
                    self.setting_default_encoding.as_deref().unwrap_or(""),
                );
            }
            Message::DefaultAddEnvVar => {
                // A fresh blank row, persisted on the first keystroke (a
                // blank-key row is dropped by `env_vars_to_setting`).
                self.setting_default_env_vars.push(crate::state::EnvVarForm::default());
            }
            Message::DefaultRemoveEnvVar(idx) => {
                if idx < self.setting_default_env_vars.len() {
                    self.setting_default_env_vars.remove(idx);
                }
                self.persist_setting(
                    "default_env_vars",
                    &crate::util::env_vars_to_setting(&self.setting_default_env_vars),
                );
            }
            Message::DefaultEnvVarKeyChanged(idx, val) => {
                if let Some(e) = self.setting_default_env_vars.get_mut(idx) {
                    e.key = val;
                }
                self.persist_setting(
                    "default_env_vars",
                    &crate::util::env_vars_to_setting(&self.setting_default_env_vars),
                );
            }
            Message::DefaultEnvVarValueChanged(idx, val) => {
                if let Some(e) = self.setting_default_env_vars.get_mut(idx) {
                    e.value = val;
                }
                self.persist_setting(
                    "default_env_vars",
                    &crate::util::env_vars_to_setting(&self.setting_default_env_vars),
                );
            }
            Message::ToggleDefaultsCollapsed => {
                self.setting_defaults_collapsed = !self.setting_defaults_collapsed;
                self.persist_setting(
                    "defaults_collapsed",
                    if self.setting_defaults_collapsed { "true" } else { "false" },
                );
            }
            Message::SettingScrollbackChanged(val) => {
                // Cap at 1M rows, alacritty allocates lazily but >1M is
                // both unreasonable and a foot-gun for memory pressure.
                self.setting_scrollback_rows = sanitize_uint(&val, 1_000_000);
                self.persist_setting("scrollback_rows", &self.setting_scrollback_rows);
                // Applies to terminals opened after this point; existing
                // sessions keep their current buffer.
                oryxis_terminal::set_default_scrollback(resolve_scrollback_rows(
                    &self.setting_scrollback_rows,
                ));
            }
            Message::SettingWordDelimitersChanged(val) => {
                // Free-text: any character may delimit a word. Stored as
                // typed; the widget syncs it into the terminal backend on
                // the next double-click. Empty is allowed (no delimiters).
                self.setting_word_delimiters = val;
                self.persist_setting("word_delimiters", &self.setting_word_delimiters);
            }
            Message::SettingResetWordDelimiters => {
                self.setting_word_delimiters =
                    oryxis_terminal::DEFAULT_WORD_DELIMITERS.to_string();
                self.persist_setting("word_delimiters", &self.setting_word_delimiters);
            }
            Message::SettingCloudAutoRefreshToggle => {
                self.setting_cloud_auto_refresh_enabled =
                    !self.setting_cloud_auto_refresh_enabled;
                self.persist_setting(
                    "cloud_auto_refresh_enabled",
                    if self.setting_cloud_auto_refresh_enabled { "true" } else { "false" },
                );
            }
            Message::SettingCloudAutoRefreshIntervalChanged(val) => {
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
            Message::SettingCloudAutoArchiveToggle => {
                self.setting_cloud_auto_archive_orphans =
                    !self.setting_cloud_auto_archive_orphans;
                self.persist_setting(
                    "cloud_auto_archive_orphans",
                    if self.setting_cloud_auto_archive_orphans { "true" } else { "false" },
                );
            }
            Message::SettingCloudOrphanArchiveDaysChanged(val) => {
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
            Message::SettingSftpConcurrencyChanged(val) => {
                // Cap at 8, beyond that the SSH channel multiplexer
                // overhead outweighs the throughput gain on most links.
                self.setting_sftp_concurrency = sanitize_uint(&val, 8);
                if self.setting_sftp_concurrency == "0" {
                    self.setting_sftp_concurrency = "1".into();
                }
                self.persist_setting("sftp_concurrency", &self.setting_sftp_concurrency);
            }
            Message::SettingSftpConnectTimeoutChanged(val) => {
                self.setting_sftp_connect_timeout = sanitize_uint(&val, 600);
                if self.setting_sftp_connect_timeout == "0" {
                    self.setting_sftp_connect_timeout = "1".into();
                }
                self.persist_setting(
                    "sftp_connect_timeout",
                    &self.setting_sftp_connect_timeout,
                );
            }
            Message::SettingSftpAuthTimeoutChanged(val) => {
                self.setting_sftp_auth_timeout = sanitize_uint(&val, 600);
                if self.setting_sftp_auth_timeout == "0" {
                    self.setting_sftp_auth_timeout = "1".into();
                }
                self.persist_setting("sftp_auth_timeout", &self.setting_sftp_auth_timeout);
            }
            Message::SettingSftpSessionTimeoutChanged(val) => {
                self.setting_sftp_session_timeout = sanitize_uint(&val, 600);
                if self.setting_sftp_session_timeout == "0" {
                    self.setting_sftp_session_timeout = "1".into();
                }
                self.persist_setting(
                    "sftp_session_timeout",
                    &self.setting_sftp_session_timeout,
                );
            }
            Message::SettingSftpOpTimeoutChanged(val) => {
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
            Message::SettingToggleAutoReconnect => {
                self.setting_auto_reconnect = !self.setting_auto_reconnect;
                self.persist_setting(
                    "auto_reconnect",
                    if self.setting_auto_reconnect { "true" } else { "false" },
                );
            }
            Message::SettingMaxReconnectChanged(val) => {
                self.setting_max_reconnect_attempts = sanitize_uint(&val, 100);
                self.persist_setting(
                    "max_reconnect_attempts",
                    &self.setting_max_reconnect_attempts,
                );
            }
            Message::ConnectAnimTick => {
                self.connect_anim_tick = self.connect_anim_tick.wrapping_add(1);
            }
            Message::AutoReconnectTick => {
                // Liveness sweep, independent of the auto-reconnect setting.
                // A pane whose SSH writer task has died reports
                // `is_alive() == false` while its reader may still be
                // draining output: the tab looks "connected" but silently
                // swallows every keystroke (the writer's `send` errors and
                // the input sites discard it). Nothing else checks
                // `is_alive`, so without this such a pane stays a dead
                // input sink forever. Surface it as a real disconnect so the
                // UI updates and, when enabled, reconnect kicks in. Panes
                // already torn down have `ssh_session == None` and are
                // skipped, so this can't loop.
                let dead: Vec<_> = self
                    .tabs
                    .iter()
                    .flat_map(|t| t.pane_grid.panes.values())
                    .filter(|p| p.ssh_session.as_ref().is_some_and(|s| !s.is_alive()))
                    .map(|p| p.id)
                    .collect();
                if !dead.is_empty() {
                    return Ok(Task::batch(
                        dead.into_iter()
                            .map(|id| Task::done(Message::SshDisconnected(id))),
                    ));
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
                        let Some(conn) = self.connections.iter().find(|c| c.label == base) else {
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
                        if let Some(conn) = self.connections.iter().find(|c| c.label == base) {
                            let entry = self.reconnect_counters.entry(conn.id).or_insert(0);
                            *entry += 1;
                        }
                        return Ok(Task::done(Message::ReconnectTab(tab_idx)));
                    }
                }
            }
            Message::LockVault => {
                if let Some(vault) = &mut self.vault {
                    vault.lock();
                    if self.vault_ui.has_user_password {
                        self.vault_ui.state = VaultState::Locked;
                        self.connections.clear();
                        self.keys.clear();
                        self.snippets.clear();
                        self.groups.clear();
                        // Close live SSH sessions, not just the panes
                        // referencing them, so locking the vault really
                        // severs the remote connections.
                        for tab in &self.tabs {
                            Self::close_tab_ssh_sessions(tab);
                        }
                        self.tabs.clear();
                        self.active_tab = None;
                        self.clear_terminal_tab_memory();
                        self.active_view = View::Dashboard;
                    } else {
                        // No user password: re-open immediately
                        let _ = vault.open_without_password();
                    }
                }
            }

            Message::OpenLocalShell => {
                // Burger menu is the most common entry point for
                // this action; dismiss it so the spawned shell
                // doesn't appear behind the still-open dropdown.
                self.show_burger_menu = false;
                // First open ever: run the one-time scan, then act on the
                // result. Every later open reads the persisted list (the
                // scan never repeats unless the user asks for a re-scan).
                if self.local_terminals.is_none() {
                    return Ok(Task::perform(
                        tokio::task::spawn_blocking(detect_local_shells),
                        |result| Message::LocalShellsDetected(result.unwrap_or_default()),
                    ));
                }
                return Ok(self.decide_open_local_terminal());
            }
            Message::ShowLocalShellPicker => {
                self.local_shell_picker_open = true;
                // The list is already populated by the time we get here
                // (OpenLocalShell scans first). Guard the never-scanned
                // case anyway so a direct dispatch still fills the picker.
                if self.local_terminals.is_none() {
                    return Ok(Task::perform(
                        tokio::task::spawn_blocking(detect_local_shells),
                        |result| Message::LocalShellsDetected(result.unwrap_or_default()),
                    ));
                }
            }
            Message::LocalShellsDetected(shells) => {
                // One-time scan result: seed the curated list and persist.
                let entries: Vec<crate::state::LocalTerminalEntry> =
                    shells.into_iter().map(detected_entry).collect();
                self.local_terminals = Some(entries);
                self.persist_local_terminals();
                // If the picker overlay is open it was opened explicitly,
                // so just leave it showing the freshly filled list. If it
                // isn't, this scan was triggered by an open intent, so
                // continue the open decision now that we have the list.
                if !self.local_shell_picker_open {
                    return Ok(self.decide_open_local_terminal());
                }
            }
            Message::HideLocalShellPicker => {
                self.local_shell_picker_open = false;
            }
            Message::OpenLocalShellWith { program, args, label } => {
                self.local_shell_picker_open = false;
                return Ok(spawn_local_shell(self, Some((program, args, label))));
            }
            Message::OpenLocalTerminalsSettings => {
                self.local_shell_picker_open = false;
                self.active_view = View::Settings;
                self.settings_section = crate::state::SettingsSection::Terminal;
            }
            Message::RescanLocalTerminals => {
                return Ok(Task::perform(
                    tokio::task::spawn_blocking(detect_local_shells),
                    |result| Message::LocalTerminalsRescanned(result.unwrap_or_default()),
                ));
            }
            Message::LocalTerminalsRescanned(shells) => {
                // Merge: keep everything already curated (manual entries and
                // user edits), append only detected entries whose command
                // isn't present yet. A previously-removed-but-still-detected
                // entry reappearing on an explicit re-scan is expected.
                let mut list = self.local_terminals.take().unwrap_or_default();
                let mut seen: std::collections::HashSet<String> =
                    list.iter().map(|e| e.cmd_key()).collect();
                for s in shells {
                    let entry = detected_entry(s);
                    if seen.insert(entry.cmd_key()) {
                        list.push(entry);
                    }
                }
                self.local_terminals = Some(list);
                self.persist_local_terminals();
            }
            Message::RemoveLocalTerminal(id) => {
                if let Some(list) = self.local_terminals.as_mut() {
                    list.retain(|e| e.id != id);
                }
                // Drop a default pointing at the now-removed entry.
                if self.local_terminal_default == Some(id) {
                    self.local_terminal_default = None;
                    self.persist_local_terminal_default();
                }
                self.persist_local_terminals();
            }
            Message::SetDefaultLocalTerminal(id) => {
                self.local_terminal_default = id;
                self.persist_local_terminal_default();
            }
            Message::OpenLocalTerminalAddModal => {
                self.local_terminal_form = crate::state::LocalTerminalForm::default();
                self.local_terminal_add_open = true;
            }
            Message::OpenLocalTerminalEditModal(id) => {
                if let Some(entry) = self
                    .local_terminals
                    .as_deref()
                    .unwrap_or(&[])
                    .iter()
                    .find(|e| e.id == id)
                {
                    self.local_terminal_form = crate::state::LocalTerminalForm {
                        editing_id: Some(id),
                        label: entry.label.clone(),
                        program: entry.program.clone(),
                        args: entry.args.join(" "),
                        color: entry.color.clone(),
                        icon: entry.icon.clone(),
                        error: None,
                    };
                    self.local_terminal_add_open = true;
                }
            }
            Message::CloseLocalTerminalAddModal => {
                self.local_terminal_add_open = false;
            }
            Message::OpenLocalTerminalIconPicker => {
                // Seed the shared host icon picker from the form and target
                // it back at the form (deferred save on IconPickerSave).
                // Fall back to the label's OS hint (then a terminal glyph)
                // so the preview matches the card when there's no override.
                self.icon_picker.icon = self.local_terminal_form.icon.clone().or_else(|| {
                    crate::os_icon::local_shell_os_hint(&self.local_terminal_form.label)
                        .or_else(|| Some("terminal".to_string()))
                });
                self.icon_picker.color = self.local_terminal_form.color.clone();
                self.icon_picker.hex_input =
                    self.local_terminal_form.color.clone().unwrap_or_default();
                self.icon_picker.icon_search = String::new();
                self.icon_color_popover = None;
                self.icon_picker.for_id = None;
                self.icon_picker.for_group_form = false;
                self.icon_picker.for_session_group = false;
                self.icon_picker.for_group_edit = false;
                self.icon_picker.for_local_terminal = true;
                self.show_icon_picker = true;
            }
            Message::LocalTerminalCardHovered(idx) => {
                self.hovered_local_terminal_card = Some(idx);
            }
            Message::LocalTerminalCardUnhovered => {
                self.hovered_local_terminal_card = None;
            }
            Message::LocalTerminalFormLabelChanged(v) => {
                self.local_terminal_form.label = v;
                self.local_terminal_form.error = None;
            }
            Message::LocalTerminalFormProgramChanged(v) => {
                self.local_terminal_form.program = v;
                self.local_terminal_form.error = None;
            }
            Message::LocalTerminalFormArgsChanged(v) => {
                self.local_terminal_form.args = v;
                self.local_terminal_form.error = None;
            }
            Message::AddLocalTerminalSubmit => {
                let label = self.local_terminal_form.label.trim().to_string();
                let program = self.local_terminal_form.program.trim().to_string();
                if label.is_empty() || program.is_empty() {
                    self.local_terminal_form.error = Some("local_terminal_invalid");
                } else {
                    let args: Vec<String> = self
                        .local_terminal_form
                        .args
                        .split_whitespace()
                        .map(|s| s.to_string())
                        .collect();
                    let color = self.local_terminal_form.color.clone();
                    let icon = self.local_terminal_form.icon.clone();
                    let mut list = self.local_terminals.take().unwrap_or_default();
                    match self.local_terminal_form.editing_id {
                        // Edit in place: program/args/label/appearance all
                        // change; the id and manual flag are preserved.
                        Some(id) => {
                            if let Some(e) = list.iter_mut().find(|e| e.id == id) {
                                e.label = label;
                                e.program = program;
                                e.args = args;
                                e.color = color;
                                e.icon = icon;
                            }
                        }
                        // Add a new manual entry, skipping an exact command
                        // duplicate (label-only difference isn't worth a dup).
                        None => {
                            let entry = crate::state::LocalTerminalEntry {
                                id: uuid::Uuid::new_v4(),
                                label,
                                program,
                                args,
                                manual: true,
                                color,
                                icon,
                            };
                            if !list.iter().any(|e| e.cmd_key() == entry.cmd_key()) {
                                list.push(entry);
                            }
                        }
                    }
                    self.local_terminals = Some(list);
                    self.persist_local_terminals();
                    self.local_terminal_form = crate::state::LocalTerminalForm::default();
                    self.local_terminal_add_open = false;
                }
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }
}

