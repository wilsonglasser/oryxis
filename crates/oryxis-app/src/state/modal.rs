//! Modal stack (proof-of-concept phase of the god-struct refactor).
//!
//! The app's blocking modals (pickers, editors, confirm dialogs) can
//! stack: a new-tab picker opens over the dashboard, an icon picker opens
//! over an editor form, etc. They used to be tracked as ~18 independent
//! `show_*: bool` / `Option<_>` fields on `Oryxis`, with two hand-
//! maintained functions (`any_modal_blocks_input`, `close_topmost_modal`
//! in `shortcuts.rs`) that had to be updated by hand for every new modal,
//! a documented footgun (a forgotten entry leaks keystrokes into the PTY
//! behind the modal).
//!
//! This enum + an ordered `modal_stack: Vec<Modal>` make those two
//! functions exhaustive `match`es the compiler enforces. The heavier
//! modals keep their form data in dedicated `Oryxis` fields (mutated in
//! place by their handlers, which a `Vec<Modal>` payload would turn into
//! an O(n) search + borrow dance); this enum is a lightweight ordering +
//! dispatch key. The invariant "`Modal::X` on the stack <-> its companion
//! field is populated" is held by routing every open through `open_modal`
//! and every close through `close_modal` / `close_topmost_modal`.
//!
//! Migrated so far (PoC): theme editor, folder-delete confirm. The
//! remaining modals still live as flags and are checked alongside the
//! stack until they are moved over too.

/// One open blocking modal. Variants are added as each modal migrates off
/// its standalone flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Modal {
    /// Custom terminal-theme editor (data in `Oryxis::theme_editor`).
    ThemeEditor,
    /// Delete-folder confirm dialog (target id in `Oryxis::folder_delete`).
    FolderDelete,
}

impl Modal {
    /// Whether this modal captures keyboard input, so keystrokes must not
    /// fall through to the terminal behind it. Every current modal does;
    /// the method exists so a future non-capturing overlay is a compiler-
    /// visible decision, not a silent omission.
    pub(crate) fn blocks_input(self) -> bool {
        match self {
            Modal::ThemeEditor | Modal::FolderDelete => true,
        }
    }
}
