//! `Oryxis::handle_settings`, match arms for the Settings panel:
//! terminal/SFTP/SSH knobs, app theme, language, auto-reconnect tick,
//! OS-detection toggles, vault lock, font size adjustments.

#![allow(clippy::result_large_err)]

use iced::Task;

use std::sync::{Arc, Mutex};
use tokio_stream::wrappers::UnboundedReceiverStream;

use oryxis_terminal::widget::TerminalState;

use crate::app::{Message, Oryxis, DEFAULT_TERM_COLS, DEFAULT_TERM_ROWS};
use crate::state::{TerminalTab, VaultState, View};
use crate::theme::AppTheme;
use crate::util::sanitize_uint;

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

impl Oryxis {
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
    /// Validate + persist the in-progress custom theme. Returns
    /// `Some(error_message)` on failure (shown in the editor), `None` on
    /// success (after reloading the list + repainting).
    fn save_theme_editor(&mut self) -> Option<String> {
        use oryxis_core::models::custom_terminal_theme::CustomTerminalTheme;
        let form = self.theme_editor.clone()?;
        let name = form.name.trim().to_string();
        if name.is_empty() {
            return Some(crate::i18n::t("theme_error_name_required").to_string());
        }
        if oryxis_terminal::TerminalTheme::ALL.iter().any(|t| t.name() == name) {
            return Some(crate::i18n::t("theme_error_name_builtin").to_string());
        }
        if self
            .custom_terminal_themes
            .iter()
            .any(|t| t.name == name && Some(t.id) != form.editing_id)
        {
            return Some(crate::i18n::t("theme_error_name_taken").to_string());
        }
        let valid = |h: &str| crate::widgets::parse_hex_color(h).is_some();
        if !valid(&form.foreground)
            || !valid(&form.background)
            || !valid(&form.cursor)
            || form.ansi.iter().any(|h| !valid(h))
        {
            return Some(crate::i18n::t("theme_error_color_invalid").to_string());
        }

        let existing = form
            .editing_id
            .and_then(|id| self.custom_terminal_themes.iter().find(|t| t.id == id).cloned());
        let old_name = existing.as_ref().map(|e| e.name.clone());
        let created_at = existing
            .as_ref()
            .map(|e| e.created_at)
            .unwrap_or_else(chrono::Utc::now);
        let theme = CustomTerminalTheme {
            id: form.editing_id.unwrap_or_else(uuid::Uuid::new_v4),
            name: name.clone(),
            foreground: form.foreground,
            background: form.background,
            cursor: form.cursor,
            ansi: form.ansi,
            created_at,
            updated_at: chrono::Utc::now(),
        };

        {
            let Some(vault) = &self.vault else {
                return Some(crate::i18n::t("theme_error_save_failed").to_string());
            };
            if vault.save_custom_terminal_theme(&theme).is_err() {
                return Some(crate::i18n::t("theme_error_save_failed").to_string());
            }
        }

        // On rename, keep the global override pointed at the same theme.
        if let Some(old) = old_name
            && old != name
            && self.terminal_theme_override.as_deref() == Some(old.as_str())
        {
            self.terminal_theme_override = Some(name.clone());
            self.persist_setting("terminal_theme_override", &name);
        }

        self.custom_terminal_themes = self
            .vault
            .as_ref()
            .and_then(|v| v.list_custom_terminal_themes().ok())
            .unwrap_or_default();
        self.terminal_palette = self.resolve_global_terminal_palette();
        self.repaint_all_terminal_palettes();
        None
    }
}

impl Oryxis {
    /// Apply an app-theme name (built-in or a custom UI theme) to the global
    /// `OryxisColors`, tracking it in `active_app_theme_name`. Returns false
    /// if the name matches neither. Does not persist; callers that handle a
    /// user action persist + repaint.
    /// Caller sets `active_app_theme_name` on a `true` result (kept `&self`
    /// so it can be called while `self.vault` is borrowed during boot).
    pub(crate) fn apply_app_theme_name(&self, name: &str) -> bool {
        if let Some(theme) = AppTheme::ALL.iter().find(|t| t.name() == name).copied() {
            AppTheme::set_active(theme); // also clears any active custom UI theme
            true
        } else if let Some(colors) = self
            .custom_ui_themes
            .iter()
            .find(|t| t.name == name)
            .map(|t| t.colors.clone())
        {
            crate::theme::set_active_custom_ui(crate::theme::theme_colors_from_hex(&colors));
            true
        } else {
            false
        }
    }

    /// Validate + persist the in-progress custom UI theme. Returns
    /// `Some(error)` on failure. On success, if the saved theme is the
    /// active one, re-applies it live.
    fn save_ui_theme_editor(&mut self) -> Option<String> {
        use oryxis_core::models::custom_ui_theme::CustomUiTheme;
        let form = self.ui_theme_editor.clone()?;
        let name = form.name.trim().to_string();
        if name.is_empty() {
            return Some(crate::i18n::t("theme_error_name_required").to_string());
        }
        if AppTheme::ALL.iter().any(|t| t.name() == name) {
            return Some(crate::i18n::t("theme_error_name_builtin").to_string());
        }
        if self
            .custom_ui_themes
            .iter()
            .any(|t| t.name == name && Some(t.id) != form.editing_id)
        {
            return Some(crate::i18n::t("theme_error_name_taken").to_string());
        }
        if form
            .colors
            .iter()
            .any(|h| crate::widgets::parse_hex_color(h).is_none())
        {
            return Some(crate::i18n::t("theme_error_color_invalid").to_string());
        }

        let existing = form
            .editing_id
            .and_then(|id| self.custom_ui_themes.iter().find(|t| t.id == id).cloned());
        let old_name = existing.as_ref().map(|e| e.name.clone());
        let created_at = existing
            .as_ref()
            .map(|e| e.created_at)
            .unwrap_or_else(chrono::Utc::now);
        let theme = CustomUiTheme {
            id: form.editing_id.unwrap_or_else(uuid::Uuid::new_v4),
            name: name.clone(),
            colors: form.colors,
            created_at,
            updated_at: chrono::Utc::now(),
        };

        {
            let Some(vault) = &self.vault else {
                return Some(crate::i18n::t("theme_error_save_failed").to_string());
            };
            if vault.save_custom_ui_theme(&theme).is_err() {
                return Some(crate::i18n::t("theme_error_save_failed").to_string());
            }
        }
        self.custom_ui_themes = self
            .vault
            .as_ref()
            .and_then(|v| v.list_custom_ui_themes().ok())
            .unwrap_or_default();

        // If editing the active theme (by old or new name), re-apply live.
        let was_active = old_name.as_deref() == Some(self.active_app_theme_name.as_str())
            || self.active_app_theme_name == name;
        if was_active {
            crate::theme::set_active_custom_ui(crate::theme::theme_colors_from_hex(
                &theme.colors,
            ));
            self.active_app_theme_name = name.clone();
            self.persist_setting("app_theme", &name);
            self.terminal_palette = self.resolve_global_terminal_palette();
            self.repaint_all_terminal_palettes();
        }
        None
    }
}

/// Build a `TerminalPalette` from a user-defined theme's hex strings.
/// Unparseable entries fall back to white/black so a malformed color never
/// crashes the render.
fn custom_theme_palette(
    t: &oryxis_core::models::custom_terminal_theme::CustomTerminalTheme,
) -> oryxis_terminal::TerminalPalette {
    let c = |hex: &str, fallback: iced::Color| {
        crate::widgets::parse_hex_color(hex).unwrap_or(fallback)
    };
    oryxis_terminal::TerminalPalette {
        foreground: c(&t.foreground, iced::Color::WHITE),
        background: c(&t.background, iced::Color::BLACK),
        cursor: c(&t.cursor, iced::Color::WHITE),
        ansi: std::array::from_fn(|i| c(&t.ansi[i], iced::Color::WHITE)),
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
                self.theme_editor = None;
                self.theme_color_popover = None;
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
                    self.theme_editor = None;
                    self.theme_color_popover = None;
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
                // First successful ctrl-click on a link: the hint did
                // its job, retire it permanently (until "Reset hints").
                if !self.hint_link_click_used {
                    self.hint_link_click_used = true;
                    self.persist_setting("hint_link_click_used", "true");
                }
            }
            Message::ToggleSecretVisibility(field) => {
                if !self.revealed_secrets.remove(&field) {
                    self.revealed_secrets.insert(field);
                }
            }
            Message::ResetHints => {
                if let Some(vault) = &self.vault
                    && let Err(e) = vault.delete_settings_with_prefix("hint_")
                {
                    tracing::warn!("failed to reset hints: {e}");
                }
                self.hint_link_click_used = false;
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
                    if self.vault_has_user_password {
                        self.vault_state = VaultState::Locked;
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
                // Windows always has at least cmd + PowerShell (plus
                // possibly Git Bash / Nushell / Cygwin / MSYS2 / WSL),
                // so the picker is always worth showing; detection there
                // touches subprocesses and runs async via the picker.
                if cfg!(target_os = "windows") {
                    return Ok(Task::done(Message::ShowLocalShellPicker));
                }
                // On Unix detection is just a few file reads, so decide
                // inline: only interrupt with a picker when the user
                // actually has more than one shell to choose from.
                let shells = detect_local_shells();
                if shells.len() > 1 {
                    self.local_shells = Some(shells);
                    return Ok(Task::done(Message::ShowLocalShellPicker));
                }
                return Ok(spawn_local_shell(self, None));
            }
            Message::ShowLocalShellPicker => {
                self.local_shell_picker_open = true;
                if self.local_shells.is_none() {
                    // Detection touches `where.exe` and `wsl --list`,
                    // both of which can take seconds on a cold WSL
                    // host. Run on a blocking thread so the picker
                    // can paint immediately and we fill it in when
                    // the result lands.
                    return Ok(Task::perform(
                        tokio::task::spawn_blocking(detect_local_shells),
                        |result| match result {
                            Ok(shells) => Message::LocalShellsDetected(shells),
                            Err(_) => Message::LocalShellsDetected(Vec::new()),
                        },
                    ));
                }
            }
            Message::LocalShellsDetected(shells) => {
                self.local_shells = Some(shells);
            }
            Message::HideLocalShellPicker => {
                self.local_shell_picker_open = false;
            }
            Message::OpenLocalShellWith { program, args, label } => {
                self.local_shell_picker_open = false;
                return Ok(spawn_local_shell(self, Some((program, args, label))));
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }
}

/// Spawn either the default shell (`pick = None`) or a specific
/// program (`pick = Some((program, args, label))`) and wire it up
/// as a new terminal tab.
fn spawn_local_shell(
    app: &mut Oryxis,
    pick: Option<(String, Vec<String>, String)>,
) -> Task<Message> {
    app.connecting = None; // Clear any pending SSH connection progress
    let (program_label, args_label) = match &pick {
        Some((p, a, _)) => (p.clone(), a.clone()),
        None => ("<default-shell>".into(), Vec::new()),
    };
    // Open in the focused pane's directory when it's a local shell that
    // reported one via OSC 7 (a remote SSH cwd wouldn't exist locally).
    let inherit_cwd = app
        .active_tab
        .and_then(|i| app.tabs.get(i))
        .map(|t| t.active())
        .filter(|p| matches!(p.origin, crate::state::PaneOrigin::Local(_)))
        .and_then(|p| p.cwd.clone());
    let result = match &pick {
        Some((program, args, _)) => TerminalState::new_with_command(
            DEFAULT_TERM_COLS as u16,
            DEFAULT_TERM_ROWS as u16,
            program,
            args,
            inherit_cwd.as_deref(),
        ),
        None => TerminalState::new(
            DEFAULT_TERM_COLS as u16,
            DEFAULT_TERM_ROWS as u16,
            inherit_cwd.as_deref(),
        ),
    };
    match result {
        Ok((mut state, rx)) => {
            tracing::info!(
                "Spawned local shell: program={} args={:?}",
                program_label, args_label
            );
            state.palette = app.terminal_palette.clone();
            let tab_idx = app.tabs.len();
            let label = pick
                .as_ref()
                .map(|(_, _, l)| l.clone())
                .unwrap_or_else(|| "Local Shell".to_string());
            // Capture the exact shell so a saved session group restores it.
            // No pick = default OS shell (empty program).
            let origin = crate::state::PaneOrigin::Local(crate::state::LocalShellSpec {
                label: label.clone(),
                program: pick.as_ref().map(|(p, _, _)| p.clone()).unwrap_or_default(),
                args: pick.as_ref().map(|(_, a, _)| a.clone()).unwrap_or_default(),
            });
            app.tabs.push(TerminalTab::new_single(
                label,
                Arc::new(Mutex::new(state)),
            ));
            app.tabs[tab_idx].active_mut().origin = origin;
            let pane_id = app.tabs[tab_idx].active().id;
            app.active_tab = Some(tab_idx);
            app.remember_terminal_tab_focus(tab_idx);
            app.active_view = View::Terminal;
            let stream = UnboundedReceiverStream::new(rx);
            Task::batch(vec![
                app.tab_scroll_to_active(),
                Task::stream(stream).map(move |bytes| Message::PtyOutput(pane_id, bytes)),
            ])
        }
        Err(e) => {
            tracing::error!(
                "Failed to spawn local shell program={} args={:?}: {}",
                program_label, args_label, e
            );
            Task::none()
        }
    }
}

/// Build the menu of available local shells: cmd / PowerShell /
/// Git Bash / Nushell / Cygwin / MSYS2 / WSL on Windows, or the
/// login shell plus any other common shells on `PATH` on Unix.
fn detect_local_shells() -> Vec<crate::state::LocalShellSpec> {
    #[cfg(unix)]
    {
        detect_unix_shells()
    }
    #[cfg(target_os = "windows")]
    {
        use crate::state::LocalShellSpec;
        let mut out: Vec<LocalShellSpec> = Vec::new();
        // PowerShell, prefer pwsh.exe (PS7+) over the bundled
        // powershell.exe; both detect via `where.exe` to cope with
        // the fact that PS7 isn't on every machine.
        if which("pwsh.exe").is_some() {
            out.push(LocalShellSpec {
                label: "PowerShell".into(),
                program: "pwsh.exe".into(),
                args: vec![],
            });
        } else {
            out.push(LocalShellSpec {
                label: "Windows PowerShell".into(),
                program: "powershell.exe".into(),
                args: vec![],
            });
        }
        out.push(LocalShellSpec {
            label: "Command Prompt".into(),
            program: "cmd.exe".into(),
            args: vec![],
        });
        // Git Bash, the MSYS2 bash that ships with Git for Windows.
        // `where bash.exe` is unreliable (it usually resolves to the
        // WSL bash shim), so probe the canonical install locations.
        // `--login` sources `/etc/profile` so the MSYS `/usr/bin` PATH
        // is set up and `git`/`ls`/... resolve.
        if let Some(path) = find_git_bash() {
            out.push(LocalShellSpec {
                label: "Git Bash".into(),
                program: path,
                args: vec!["--login".into(), "-i".into()],
            });
        }
        // Nushell, cross-platform and normally on PATH.
        if which("nu.exe").is_some() {
            out.push(LocalShellSpec {
                label: "Nushell".into(),
                program: "nu.exe".into(),
                args: vec![],
            });
        }
        // Cygwin / MSYS2 bash, niche but still alive on dev boxes.
        // Same `where` ambiguity as Git Bash, so fixed roots only.
        for (label, path) in [
            ("MSYS2", r"C:\msys64\usr\bin\bash.exe"),
            ("Cygwin", r"C:\cygwin64\bin\bash.exe"),
        ] {
            if std::path::Path::new(path).is_file() {
                out.push(LocalShellSpec {
                    label: label.into(),
                    program: path.into(),
                    args: vec!["--login".into(), "-i".into()],
                });
            }
        }
        // WSL distros, `wsl --list --quiet` outputs UTF-16 LE BOM
        // by default. Decode and split on lines to get distro names.
        for distro in list_wsl_distros() {
            out.push(LocalShellSpec {
                label: format!("{distro} (WSL)"),
                program: "wsl.exe".into(),
                args: vec!["-d".into(), distro],
            });
        }
        out
    }
    #[cfg(not(any(unix, target_os = "windows")))]
    {
        Vec::new()
    }
}

/// Resolve the bash that ships with Git for Windows by probing the
/// canonical install roots (system 64/32-bit and per-user). Returns
/// the first `bin\bash.exe` that exists.
#[cfg(target_os = "windows")]
fn find_git_bash() -> Option<String> {
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    for var in ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"] {
        if let Ok(base) = std::env::var(var) {
            candidates.push(std::path::PathBuf::from(base).join(r"Git\bin\bash.exe"));
        }
    }
    if let Ok(base) = std::env::var("LOCALAPPDATA") {
        candidates.push(std::path::PathBuf::from(base).join(r"Programs\Git\bin\bash.exe"));
    }
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
}

/// Build the Unix local-shell menu: the user's login `$SHELL` first
/// (marked as the default), then any other common interactive shells
/// found on `PATH`. Deduplicated by resolved path.
#[cfg(unix)]
fn detect_unix_shells() -> Vec<crate::state::LocalShellSpec> {
    use crate::state::LocalShellSpec;
    let mut out: Vec<LocalShellSpec> = Vec::new();
    // Dedup by canonical path so `/bin/bash` and `/usr/bin/bash` (same
    // binary via a symlinked `/bin`) don't show up as two entries.
    let mut seen: std::collections::HashSet<std::path::PathBuf> = std::collections::HashSet::new();
    let canonical = |path: &std::path::Path| -> std::path::PathBuf {
        std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    };
    let basename = |path: &str| -> String {
        std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string())
    };
    // Login shell goes first, flagged so the user knows which is theirs.
    if let Ok(shell) = std::env::var("SHELL")
        && !shell.is_empty()
        && std::path::Path::new(&shell).is_file()
        && seen.insert(canonical(std::path::Path::new(&shell)))
    {
        out.push(LocalShellSpec {
            label: format!("{} ({})", basename(&shell), crate::i18n::t("shell_default")),
            program: shell,
            args: vec![],
        });
    }
    for name in ["bash", "zsh", "fish", "nu"] {
        if let Some(path) = unix_which(name)
            && seen.insert(canonical(&path))
        {
            out.push(LocalShellSpec {
                label: name.into(),
                program: path.to_string_lossy().into_owned(),
                args: vec![],
            });
        }
    }
    out
}

/// Minimal `which`: first `PATH` entry that holds the named program.
#[cfg(unix)]
fn unix_which(program: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|cand| cand.is_file())
}

#[cfg(target_os = "windows")]
fn which(program: &str) -> Option<std::path::PathBuf> {
    use std::os::windows::process::CommandExt;
    // CREATE_NO_WINDOW (0x0800_0000), without this each `where.exe`
    // call briefly flashes a cmd console behind oryxis.
    let out = std::process::Command::new("where")
        .arg(program)
        .creation_flags(0x0800_0000)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    s.lines().next().map(|l| std::path::PathBuf::from(l.trim()))
}

#[cfg(target_os = "windows")]
fn list_wsl_distros() -> Vec<String> {
    use std::os::windows::process::CommandExt;
    let out = match std::process::Command::new("wsl")
        .args(["--list", "--quiet"])
        .creation_flags(0x0800_0000)
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };
    // wsl.exe emits UTF-16 LE with a BOM. Decode by reading
    // u16 pairs.
    let bytes = out.stdout;
    let utf16: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&utf16)
        .lines()
        .map(|l| l.trim().trim_start_matches('\u{feff}').to_string())
        .filter(|l| !l.is_empty())
        .collect()
}
