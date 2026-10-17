//! Snippet CRUD, tag filters, variable prompts and the built-in
//! sudo-password snippet, wrapped by [`crate::messages::Message::Snippet`].

use iced::widget::text_editor;

#[derive(Debug, Clone)]
pub enum SnippetMessage {
    ShowSnippetPanel,
    HideSnippetPanel,
    SnippetLabelChanged(String),
    /// Snippet editor: free-form group name field.
    SnippetGroupChanged(String),
    /// Snippet editor: comma-separated tags field.
    SnippetTagsChanged(String),
    /// Snippet editor: flip the install-script category (issue #147).
    ToggleSnippetInstall,
    /// Snippets sidebar: toggle "only snippets tagged like this host".
    ToggleSnippetTagFilter,
    /// Vault Snippets: open/close the tag-filter dropdown.
    ShowSnippetTagFilterMenu,
    /// Vault Snippets: toggle one tag in the multi-select filter.
    ToggleSnippetTagFilterTag(String),
    /// Vault Snippets: clear the tag filter.
    ClearSnippetTagFilter,
    /// Vault Snippets: open a snippet group as a folder.
    OpenSnippetGroup(String),
    /// Vault Snippets: back to the root (group cards + ungrouped).
    CloseSnippetGroup,
    /// Sidebar Snippets tab: open a group folder row.
    OpenSidebarSnippetGroup(String),
    /// Sidebar Snippets tab: back to the folder list.
    CloseSidebarSnippetGroup,
    /// Snippet-variables modal: edit the Nth placeholder's value.
    SnippetVarChanged(usize, String),
    /// Substitute the filled values and send the parked snippet.
    ConfirmSnippetVars,
    /// Drop the parked snippet without sending.
    CancelSnippetVars,
    /// A snippet run's timer (`run id`, `generation`): a time wait is
    /// over, or a wait with a timeout ran out. An older generation is a
    /// wait that already ended, dropped.
    SnippetRunTimer(u64, u64),
    /// A save step's write finished (`run id`, `generation`, the path
    /// written or the error).
    SnippetRunFileWritten(u64, u64, Result<String, String>),
    /// Clipboard text handed back for a paste action in a running snippet.
    SnippetClipboardResolved(u64, u64, Option<super::Redacted>),
    /// Stop a snippet run (its status line's Cancel).
    CancelSnippetRun(u64),
    /// Snippet editor: arm the shortcut recorder (next chord binds).
    SnippetHotkeyCaptureStart,
    /// Snippet editor: remove the custom shortcut.
    SnippetHotkeyClear,
    /// Snippet editor: an edit inside a command row's text.
    SnippetStepCommandAction(usize, text_editor::Action),
    /// Snippet editor: add an action at the end.
    SnippetStepAdd,
    /// Snippet editor: remove a row.
    SnippetStepRemove(usize),
    /// Snippet editor: move a row one place up / down.
    SnippetStepMoveUp(usize),
    SnippetStepMoveDown(usize),
    /// Snippet editor: switch a row's kind.
    SnippetStepKindPicked(usize, crate::state::StepKind),
    /// Snippet editor: the app action an action row runs.
    SnippetStepActionPicked(usize, crate::snippet_run::SnippetAction),
    /// Snippet editor: the snippet a "run snippet" row runs.
    SnippetStepSnippetPicked(usize, uuid::Uuid),
    SnippetStepPatternChanged(usize, String),
    SnippetStepTimeoutChanged(usize, String),
    SnippetStepSecondsChanged(usize, String),
    SnippetStepFileNameChanged(usize, String),
    SnippetStepToggleRegex(usize),
    SnippetStepToggleContinue(usize),
    SnippetStepSaveModePicked(usize, oryxis_core::models::SaveFileMode),
    SnippetStepSaveContentPicked(usize, oryxis_core::models::SaveFileContent),
    SaveSnippet,
    /// Open the kebab (⋮) context menu on a snippet card (Edit / Delete).
    ShowSnippetMenu(usize),
    EditSnippet(usize),
    /// Ask for confirmation before removing a snippet.
    RequestDeleteSnippet(usize),
    DeleteSnippet(usize),
    RunSnippet(usize),
    /// Inject a snippet's command into the active terminal WITHOUT a
    /// trailing newline (the user presses Enter), unlike `RunSnippet`.
    PasteSnippet(usize),
    /// Built-in "global snippet": type the active host's stored password
    /// then Enter into the terminal (e.g. to answer a sudo prompt). No-op
    /// with a toast when the host has no stored password.
    ApplySudoPassword,
}
