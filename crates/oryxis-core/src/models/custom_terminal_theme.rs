//! User-defined terminal color scheme.
//!
//! Built-in terminal themes are a fixed enum in `oryxis-terminal`. A
//! `CustomTerminalTheme` lets the user define their own palette (the 16
//! ANSI colors plus foreground / background / cursor) and have it appear in
//! the theme pickers alongside the presets. Colors are stored as
//! `"#RRGGBB"` hex strings; the app layer parses them into the terminal's
//! `TerminalPalette` (it owns both the model and the terminal crate, which
//! `oryxis-core` does not depend on).
//!
//! The theme is referenced by its `name` everywhere a built-in is (the
//! global `terminal_theme_override` setting and the per-host
//! `Connection.terminal_theme`), so names must be unique across built-ins
//! and custom themes. The CRUD layer enforces that on save.

use uuid::Uuid;

/// A user-defined terminal palette. All color fields are `"#RRGGBB"` hex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomTerminalTheme {
    pub id: Uuid,
    /// Display name, also the key used by the global / per-host selection.
    /// Unique across built-in + custom themes (enforced on save).
    pub name: String,
    pub foreground: String,
    pub background: String,
    pub cursor: String,
    /// ANSI 0-15 (black, red, green, yellow, blue, magenta, cyan, white,
    /// then the bright variants).
    pub ansi: [String; 16],
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl CustomTerminalTheme {
    /// A blank, sensible starting palette (a dark scheme) for a brand new
    /// custom theme before the user edits it.
    pub fn new_default(name: String) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: Uuid::new_v4(),
            name,
            foreground: "#e0e5ed".into(),
            background: "#161a21".into(),
            cursor: "#2bc2d0".into(),
            ansi: [
                "#262c38".into(),
                "#e86262".into(),
                "#5fd365".into(),
                "#e7ab52".into(),
                "#5ba2e8".into(),
                "#b282dc".into(),
                "#2bc2d0".into(),
                "#cfd5de".into(),
                "#464e5c".into(),
                "#ff7979".into(),
                "#78e682".into(),
                "#ffc566".into(),
                "#78b8fa".into(),
                "#cea2f0".into(),
                "#50d6e2".into(),
                "#edf0f5".into(),
            ],
            created_at: now,
            updated_at: now,
        }
    }
}
