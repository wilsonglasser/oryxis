//! User-defined application (chrome) theme.
//!
//! Built-in app themes are a fixed enum in `oryxis-app::theme`. A
//! `CustomUiTheme` lets the user define their own chrome palette: the 21
//! `ThemeColors` fields, stored as `"#rrggbb"` hex strings in the stable
//! order of `theme::UI_COLOR_FIELDS`. The app layer parses them into a
//! `ThemeColors` (it owns both the model and the theme module, which
//! `oryxis-core` does not depend on).
//!
//! Referenced by `name` everywhere a built-in app theme is (the `app_theme`
//! setting), so names must be unique across built-ins and custom themes;
//! the CRUD layer enforces that on save.

use uuid::Uuid;

/// A user-defined chrome theme. `colors` holds 21 `"#rrggbb"` hex strings,
/// indexed by `theme::UI_COLOR_FIELDS`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomUiTheme {
    pub id: Uuid,
    /// Display name, also the key used by the `app_theme` selection. Unique
    /// across built-in + custom themes (enforced on save).
    pub name: String,
    pub colors: [String; 21],
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl CustomUiTheme {
    /// Build a theme from a name and the 21 colors (e.g. seeded from an
    /// existing built-in theme by the app layer).
    pub fn new(name: String, colors: [String; 21]) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: Uuid::new_v4(),
            name,
            colors,
            created_at: now,
            updated_at: now,
        }
    }
}
