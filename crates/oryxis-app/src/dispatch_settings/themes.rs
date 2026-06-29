//! Settings dispatch helpers: themes. Split out of dispatch_settings/mod.rs.

use super::*;
impl Oryxis {
    /// Validate + persist the in-progress custom theme. Returns
    /// `Some(error_message)` on failure (shown in the editor), `None` on
    /// success (after reloading the list + repainting).
    pub(crate) fn save_theme_editor(&mut self) -> Option<String> {
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
    pub(crate) fn save_ui_theme_editor(&mut self) -> Option<String> {
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
pub(crate) fn custom_theme_palette(
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
