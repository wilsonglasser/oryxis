//! Custom terminal theme editor (split out of `state.rs`).

/// One editable color slot in the custom terminal theme editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThemeColorSlot {
    Foreground,
    Background,
    Cursor,
    Ansi(u8),
}

/// In-progress edit of a custom terminal theme. `None` for `editing_id`
/// means a brand new theme; `Some(id)` edits an existing one. Colors are
/// `"#RRGGBB"` hex strings being typed.
#[derive(Debug, Clone)]
pub(crate) struct ThemeEditorForm {
    pub editing_id: Option<uuid::Uuid>,
    pub name: String,
    pub foreground: String,
    pub background: String,
    pub cursor: String,
    pub ansi: [String; 16],
    pub error: Option<String>,
}

impl ThemeEditorForm {
    pub fn from_theme(
        t: &oryxis_core::models::custom_terminal_theme::CustomTerminalTheme,
    ) -> Self {
        Self {
            editing_id: Some(t.id),
            name: t.name.clone(),
            foreground: t.foreground.clone(),
            background: t.background.clone(),
            cursor: t.cursor.clone(),
            ansi: t.ansi.clone(),
            error: None,
        }
    }


    /// Write the color string for a slot.
    pub fn set_slot(&mut self, slot: ThemeColorSlot, value: String) {
        match slot {
            ThemeColorSlot::Foreground => self.foreground = value,
            ThemeColorSlot::Background => self.background = value,
            ThemeColorSlot::Cursor => self.cursor = value,
            ThemeColorSlot::Ansi(i) => self.ansi[i as usize] = value,
        }
    }
}

/// In-progress edit of a custom UI (chrome) theme. `colors` holds the 21
/// `"#rrggbb"` strings indexed by `theme::UI_COLOR_FIELDS`.
#[derive(Debug, Clone)]
pub(crate) struct UiThemeEditorForm {
    pub editing_id: Option<uuid::Uuid>,
    pub name: String,
    pub colors: [String; 21],
    pub error: Option<String>,
}

impl UiThemeEditorForm {
    pub fn from_theme(
        t: &oryxis_core::models::custom_ui_theme::CustomUiTheme,
    ) -> Self {
        Self {
            editing_id: Some(t.id),
            name: t.name.clone(),
            colors: t.colors.clone(),
            error: None,
        }
    }

    /// New theme seeded from a base palette (the 21 hex of an existing
    /// theme), so the user starts from something that already works.
    pub fn new_from_colors(colors: [String; 21]) -> Self {
        Self { editing_id: None, name: String::new(), colors, error: None }
    }
}
