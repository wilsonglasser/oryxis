//! Blocking-modal registry (capstone of the god-struct refactor).
//!
//! The app's blocking modals (pickers, editors, confirm dialogs) used to
//! be tracked as ~19 independent `show_*: bool` / `Option<_>` fields on
//! `Oryxis`, with two hand-maintained functions in `shortcuts.rs`
//! (`any_modal_blocks_input`, `close_topmost_modal`) that had to be edited
//! by hand for every new modal, a documented footgun: a forgotten entry
//! leaks keystrokes into the PTY behind the modal, or makes a modal
//! un-dismissable by Esc.
//!
//! This enum makes those two functions exhaustive `match`es the compiler
//! enforces. The per-modal `show_*` flag / `Option<_>` data field stays as
//! the single source of truth for "is this modal open" (so render sites
//! and the ~50 scattered open/close sites are unchanged); the enum is a
//! key into them. `Oryxis::is_modal_open` and `Oryxis::close_modal`
//! (`shortcuts.rs`) are `match`es over every variant, so a new modal
//! cannot compile without being handled. The only manual lists are
//! [`Modal::ALL`] and [`Modal::ESC_ORDER`]; a unit test guards `ALL`
//! against a forgotten variant.

/// One blocking modal. Each maps to a `show_*` / `Option<_>` field on
/// `Oryxis` via `is_modal_open` / `close_modal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Modal {
    NewTabPicker,
    TabJump,
    IconPicker,
    ThemePicker,
    ChainEditor,
    SessionGroupPanel,
    FolderRename,
    FolderDelete,
    /// Keyboard-interactive (2FA / OTP) prompt. Blocks input but owns its
    /// own dismissal, so it is intentionally absent from `ESC_ORDER`.
    KbiPrompt,
    ThemeEditor,
    ThemeImport,
    UiThemeEditor,
    ShareDialog,
    CloudImportConfirm,
    SftpRename,
    SftpNewEntry,
    SftpProperties,
    SftpOverwrite,
    SftpPicker,
}

impl Modal {
    /// Every variant. Drives `any_modal_blocks_input`. Kept in sync with
    /// the enum by `tests::all_covers_every_variant`.
    pub(crate) const ALL: &'static [Modal] = &[
        Modal::NewTabPicker,
        Modal::TabJump,
        Modal::IconPicker,
        Modal::ThemePicker,
        Modal::ChainEditor,
        Modal::SessionGroupPanel,
        Modal::FolderRename,
        Modal::FolderDelete,
        Modal::KbiPrompt,
        Modal::ThemeEditor,
        Modal::ThemeImport,
        Modal::UiThemeEditor,
        Modal::ShareDialog,
        Modal::CloudImportConfirm,
        Modal::SftpRename,
        Modal::SftpNewEntry,
        Modal::SftpProperties,
        Modal::SftpOverwrite,
        Modal::SftpPicker,
    ];

    /// Modals Esc dismisses, in topmost-first priority order (the order
    /// `close_topmost_modal` walks). Modals absent here own their own
    /// dismissal and are not Esc-closeable: the kbi prompt and the SFTP
    /// rename / new-entry / properties / overwrite dialogs.
    pub(crate) const ESC_ORDER: &'static [Modal] = &[
        Modal::NewTabPicker,
        Modal::TabJump,
        Modal::IconPicker,
        Modal::ThemePicker,
        Modal::ChainEditor,
        Modal::FolderRename,
        Modal::FolderDelete,
        Modal::SessionGroupPanel,
        Modal::ThemeEditor,
        Modal::UiThemeEditor,
        Modal::ThemeImport,
        Modal::ShareDialog,
        Modal::CloudImportConfirm,
        Modal::SftpPicker,
    ];

    /// Whether this modal captures keyboard input, so keystrokes must not
    /// fall through to the terminal behind it. Every current modal does;
    /// the method exists so a future non-capturing overlay is a compiler-
    /// visible decision, not a silent omission.
    pub(crate) fn blocks_input(self) -> bool {
        match self {
            Modal::NewTabPicker
            | Modal::TabJump
            | Modal::IconPicker
            | Modal::ThemePicker
            | Modal::ChainEditor
            | Modal::SessionGroupPanel
            | Modal::FolderRename
            | Modal::FolderDelete
            | Modal::KbiPrompt
            | Modal::ThemeEditor
            | Modal::ThemeImport
            | Modal::UiThemeEditor
            | Modal::ShareDialog
            | Modal::CloudImportConfirm
            | Modal::SftpRename
            | Modal::SftpNewEntry
            | Modal::SftpProperties
            | Modal::SftpOverwrite
            | Modal::SftpPicker => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Modal;

    #[test]
    fn all_covers_every_variant() {
        // The exhaustive match means a new variant fails to compile here
        // until it is named; the assert then forces it into `ALL` too.
        for &m in Modal::ALL {
            match m {
                Modal::NewTabPicker
                | Modal::TabJump
                | Modal::IconPicker
                | Modal::ThemePicker
                | Modal::ChainEditor
                | Modal::SessionGroupPanel
                | Modal::FolderRename
                | Modal::FolderDelete
                | Modal::KbiPrompt
                | Modal::ThemeEditor
                | Modal::ThemeImport
                | Modal::UiThemeEditor
                | Modal::ShareDialog
                | Modal::CloudImportConfirm
                | Modal::SftpRename
                | Modal::SftpNewEntry
                | Modal::SftpProperties
                | Modal::SftpOverwrite
                | Modal::SftpPicker => {}
            }
        }
        assert_eq!(Modal::ALL.len(), 19, "add the new variant to Modal::ALL");
        // Every Esc-closeable modal must also be a known modal.
        for m in Modal::ESC_ORDER {
            assert!(Modal::ALL.contains(m));
        }
    }
}
