//! AI assistant feature state: provider/model/key settings plus the editable
//! system prompt. Grouped off the `Oryxis` god-struct as part of the
//! modules-by-feature direction (field grouping only).

use iced::widget::text_editor;

/// All AI-assistant settings + the editable system-prompt buffer. The
/// scalar settings hydrate from the `settings` table on boot; `api_key_set`
/// mirrors whether an encrypted key exists, and `system_prompt` holds the
/// live `text_editor` buffer (which is why this struct is not `Clone`).
#[derive(Debug)]
pub(crate) struct AiState {
    /// Whether the AI assistant sidebar is enabled.
    pub(crate) enabled: bool,
    /// Provider id, e.g. `"anthropic"` / `"openai"`.
    pub(crate) provider: String,
    /// Model id sent with each request.
    pub(crate) model: String,
    /// In-memory API key while editing the field. The persisted copy is
    /// encrypted per-field in the vault (the `set_user_password` machinery).
    pub(crate) api_key: String,
    /// Mirrors whether an encrypted key is stored, for the masked UI.
    pub(crate) api_key_set: bool,
    /// Optional override base URL for OpenAI-compatible endpoints.
    pub(crate) api_url: String,
    /// Editable system-prompt buffer. `text_editor::Content` is not `Clone`,
    /// so it lives here rather than in a cloneable form struct.
    pub(crate) system_prompt: text_editor::Content,
}

impl Default for AiState {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: "anthropic".into(),
            model: "claude-sonnet-4-20250514".into(),
            api_key: String::new(),
            api_key_set: false,
            api_url: String::new(),
            system_prompt: text_editor::Content::new(),
        }
    }
}
