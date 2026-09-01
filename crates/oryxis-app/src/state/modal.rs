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
    /// Command palette (C4): fuzzy search over every action. A search
    /// picker like `TabJump` (single input + a filtered row list); Esc
    /// closes it, and it blocks input like every other modal.
    CommandPalette,
    IconPicker,
    ThemePicker,
    ChainEditor,
    SessionGroupPanel,
    FolderRename,
    FolderDelete,
    /// Transient tab rename (terminal or SFTP tab, addressed by `TabRef`).
    TabRename,
    /// Careful-paste confirmation (multi-line clipboard paste parked in
    /// `pending_paste`, waiting for the user to confirm or cancel).
    CarefulPaste,
    /// Snippet-variables prompt (`{name}` placeholders parked in
    /// `pending_snippet_vars`, filled before the send).
    SnippetVars,
    /// Keyboard-interactive (2FA / OTP) prompt. Blocks input but owns its
    /// own dismissal, so it is intentionally absent from `ESC_ORDER`.
    KbiPrompt,
    /// Host-key verification prompt (`pending_host_key`) for a backgrounded
    /// connect (split pane / manual port forward / RDP launcher) with no
    /// connect-progress screen. A security prompt, so it MUST block input
    /// (Enter must never fall through to the PTY behind it) and Esc rejects
    /// the key (the safe default: never accept an unknown / changed key by
    /// a stray keystroke).
    HostKey,
    /// Command-proxy approval prompt (`pending_proxy_command`): the dial
    /// is about to run a line from the vault as a local process. A
    /// security prompt of the same class as `HostKey`, and the more
    /// consequential of the two, so it MUST block input and Esc refuses
    /// (the safe default is not to run it).
    ProxyCommand,
    /// Global terminal-theme gallery, opened from the Settings row.
    /// The per-host picker is `ThemePicker`; this one writes the global
    /// override and carries the create / import / clone affordances.
    TerminalThemeGallery,
    UiThemeGallery,
    ThemeEditor,
    ThemeImport,
    UiThemeEditor,
    UiThemeImport,
    ShareDialog,
    CloudImportConfirm,
    /// Shared error / single-action confirm dialog (`error_dialog`),
    /// also the confirm step for known-host and session-log deletes.
    ErrorDialog,
    /// "Clear all history" confirmation.
    ClearHistoryConfirm,
    /// SSH-config import host-selection dialog.
    SshImport,
    SftpRename,
    SftpNewEntry,
    SftpProperties,
    SftpOverwrite,
    /// Save-confirmation for an "Open with" edit watch (issue #84): a
    /// data-bearing decision like `SftpOverwrite`, so it blocks input.
    /// In `ESC_ORDER`: Esc means "skip this save" (the safe default,
    /// `close_modal` re-arms the watch so the next save prompts again),
    /// never an accidental upload. Its buttons are keynav Confirm rows,
    /// Enter fires the ringed choice (default Yes).
    SftpEditPrompt,
    /// Reopen-or-redownload dialog for a file that is already being
    /// edited. Data-bearing like `SftpEditPrompt` (one branch deletes the
    /// local copy), so it blocks input; in `ESC_ORDER` where Esc means
    /// "do nothing", neither reopen nor discard.
    SftpEditReopen,
    SftpPicker,
    /// The ssh-agent per-signature confirm prompt (`agent.pending_confirm`,
    /// B1). A blocking security prompt like `HostKey`: it MUST block input
    /// (Enter must never fall through to the PTY behind it) and Esc denies
    /// the signature (the safe default). In `ESC_ORDER` next to `HostKey`
    /// so the Esc router and the modal-keynav router both reach it.
    AgentConfirm,
    /// Read-only viewer for a key's attached OpenSSH certificate
    /// (`cert_viewer`, B2). Carries no secret (public cert material), so
    /// Esc simply closes it; it is in `ESC_ORDER` in the lightweight
    /// group next to the other dismissible info dialogs.
    CertificateViewer,
    /// "Kill the process on this listening port" confirmation
    /// (`monitor.kill`, issue #96). Destructive, remote and
    /// irreversible, so it blocks input and Esc cancels; unlike its
    /// sibling confirms, the SAFE button is the default row (see
    /// `build_monitor_kill_dialog`).
    MonitorKill,
    /// The highlight-rule editor (C6), opened from either rule list
    /// (Settings' global one or a host's own). A form modal like the
    /// theme editor: Esc cancels the edit, which discards only the
    /// working copy since a rule reaches its list on Save.
    HighlightRuleEditor,
    /// "A highlight rule wants to run a snippet on this session" (C6).
    /// A security prompt like `AgentConfirm`: what asked for it is
    /// REMOTE output, so it must block input (Enter must never fall
    /// through to the PTY behind it) and Esc refuses, which is also
    /// remembered for the session.
    TriggerConfirm,
    /// "Open this link?" for a Ctrl+click in a REMOTE pane
    /// (`link_confirm`). Same class as `TriggerConfirm`: what raised it
    /// is REMOTE output, so it blocks input (Enter must never fall
    /// through to the PTY behind it) and Esc opens nothing.
    TerminalLinkConfirm,
    /// Manual-lock confirmation (`vault_ui.lock_confirm`). Lock Vault
    /// tears down every live SSH session and tab, so the button asks
    /// first; Esc / backdrop / the Cancel button all decline (the safe
    /// default), and only the Lock button commits.
    LockVaultConfirm,
}

impl Modal {
    /// Every variant. Drives `any_modal_blocks_input`. Kept in sync with
    /// the enum by `tests::all_covers_every_variant`.
    pub(crate) const ALL: &'static [Modal] = &[
        Modal::NewTabPicker,
        Modal::TabJump,
        Modal::CommandPalette,
        Modal::IconPicker,
        Modal::ThemePicker,
        Modal::ChainEditor,
        Modal::SessionGroupPanel,
        Modal::FolderRename,
        Modal::FolderDelete,
        Modal::TabRename,
        Modal::CarefulPaste,
        Modal::SnippetVars,
        Modal::KbiPrompt,
        Modal::HostKey,
        Modal::ProxyCommand,
        Modal::ThemeEditor,
        Modal::TerminalThemeGallery,
        Modal::UiThemeGallery,
        Modal::ThemeImport,
        Modal::UiThemeEditor,
        Modal::UiThemeImport,
        Modal::ShareDialog,
        Modal::CloudImportConfirm,
        Modal::ErrorDialog,
        Modal::ClearHistoryConfirm,
        Modal::SshImport,
        Modal::SftpRename,
        Modal::SftpNewEntry,
        Modal::SftpProperties,
        Modal::SftpOverwrite,
        Modal::SftpEditPrompt,
        Modal::SftpEditReopen,
        Modal::SftpPicker,
        Modal::AgentConfirm,
        Modal::CertificateViewer,
        Modal::MonitorKill,
        Modal::HighlightRuleEditor,
        Modal::TriggerConfirm,
        Modal::TerminalLinkConfirm,
        Modal::LockVaultConfirm,
    ];

    /// Modals Esc dismisses, in topmost-first priority order (the order
    /// `close_topmost_modal` walks). Modals absent here own their own
    /// dismissal and are not Esc-closeable: the kbi prompt and the SFTP
    /// rename / new-entry / properties / overwrite dialogs.
    pub(crate) const ESC_ORDER: &'static [Modal] = &[
        Modal::NewTabPicker,
        Modal::TabJump,
        Modal::CommandPalette,
        Modal::IconPicker,
        Modal::ThemePicker,
        Modal::ChainEditor,
        Modal::FolderRename,
        Modal::FolderDelete,
        Modal::TabRename,
        Modal::CarefulPaste,
        Modal::SnippetVars,
        // A security prompt: Esc rejects the host key (safe default).
        Modal::HostKey,
        // Sibling security prompt: Esc refuses to run the command proxy
        // (safe default), and the refusal must reach the parked dial.
        Modal::ProxyCommand,
        // Sibling security prompt: Esc denies the signature (safe default).
        Modal::AgentConfirm,
        // Same class: Esc refuses to let remote output type into the
        // session, and the refusal sticks for the session.
        Modal::TriggerConfirm,
        // Same class again: Esc opens nothing, which is the safe answer
        // to a link whose text a remote host chose.
        Modal::TerminalLinkConfirm,
        // The error dialog can pop over another flow, so it dismisses
        // before the heavier editors below; the two confirm dialogs
        // follow in the same lightweight-confirm group.
        Modal::ErrorDialog,
        Modal::ClearHistoryConfirm,
        // Esc = don't signal anything (the safe default for a remote,
        // irreversible action); same lightweight-confirm group.
        Modal::MonitorKill,
        // Esc = don't lock (the safe default for a teardown that severs
        // every live connection). After MonitorKill to mirror the
        // `main_layout` chain, where every dialog above renders on top
        // of this one: Esc must answer the dialog the user can see.
        Modal::LockVaultConfirm,
        // Esc = neither reopen nor discard the local copy. Ahead of the
        // save prompt because `layer_sftp_modals` renders it on top: Esc
        // must always answer the dialog the user can actually see.
        Modal::SftpEditReopen,
        // Esc = skip this save (never upload by accident); the watch
        // re-arms so the next save prompts again.
        Modal::SftpEditPrompt,
        Modal::SshImport,
        Modal::SessionGroupPanel,
        // Same class of form modal: Esc abandons the working copy, and
        // the rule list behind it is untouched until a Save. Ahead of
        // the theme editors because `layer_modals` renders it ahead of
        // them: Esc has to answer whatever is actually on screen.
        Modal::HighlightRuleEditor,
        Modal::ThemeEditor,
        Modal::UiThemeEditor,
        // Import sits ON TOP of the gallery it is opened from (`layer_modals`),
        // so Esc has to answer it first.
        Modal::ThemeImport,
        Modal::TerminalThemeGallery,
        Modal::UiThemeImport,
        Modal::UiThemeGallery,
        Modal::ShareDialog,
        Modal::CloudImportConfirm,
        Modal::SftpPicker,
        // Read-only info dialog; Esc just closes it.
        Modal::CertificateViewer,
    ];

    /// Whether this modal captures keyboard input, so keystrokes must not
    /// fall through to the terminal behind it. Every current modal does;
    /// the method exists so a future non-capturing overlay is a compiler-
    /// visible decision, not a silent omission.
    pub(crate) fn blocks_input(self) -> bool {
        match self {
            Modal::NewTabPicker
            | Modal::TabJump
            | Modal::CommandPalette
            | Modal::IconPicker
            | Modal::ThemePicker
            | Modal::ChainEditor
            | Modal::SessionGroupPanel
            | Modal::FolderRename
            | Modal::FolderDelete
            | Modal::TabRename
            | Modal::CarefulPaste
            | Modal::SnippetVars
            | Modal::KbiPrompt
            | Modal::HostKey
            | Modal::ProxyCommand
            | Modal::ThemeEditor
            | Modal::TerminalThemeGallery
            | Modal::UiThemeGallery
            | Modal::ThemeImport
            | Modal::UiThemeEditor
            | Modal::UiThemeImport
            | Modal::ShareDialog
            | Modal::CloudImportConfirm
            | Modal::ErrorDialog
            | Modal::ClearHistoryConfirm
            | Modal::SshImport
            | Modal::SftpRename
            | Modal::SftpNewEntry
            | Modal::SftpProperties
            | Modal::SftpOverwrite
            | Modal::SftpEditPrompt
            | Modal::SftpEditReopen
            | Modal::SftpPicker
            | Modal::AgentConfirm
            | Modal::CertificateViewer
            | Modal::MonitorKill
            | Modal::HighlightRuleEditor
            | Modal::TriggerConfirm
            | Modal::TerminalLinkConfirm
            | Modal::LockVaultConfirm => true,
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
                | Modal::CommandPalette
                | Modal::IconPicker
                | Modal::ThemePicker
                | Modal::ChainEditor
                | Modal::SessionGroupPanel
                | Modal::FolderRename
                | Modal::FolderDelete
                | Modal::TabRename
                | Modal::CarefulPaste
                | Modal::SnippetVars
                | Modal::KbiPrompt
                | Modal::HostKey
                | Modal::ProxyCommand
                | Modal::ThemeEditor
                | Modal::TerminalThemeGallery
                | Modal::UiThemeGallery
                | Modal::ThemeImport
                | Modal::UiThemeEditor
                | Modal::UiThemeImport
                | Modal::ShareDialog
                | Modal::CloudImportConfirm
                | Modal::ErrorDialog
                | Modal::ClearHistoryConfirm
                | Modal::SshImport
                | Modal::SftpRename
                | Modal::SftpNewEntry
                | Modal::SftpProperties
                | Modal::SftpOverwrite
                | Modal::SftpEditPrompt
                | Modal::SftpEditReopen
                | Modal::SftpPicker
                | Modal::AgentConfirm
                | Modal::CertificateViewer
                | Modal::MonitorKill
                | Modal::HighlightRuleEditor
                | Modal::TriggerConfirm
                | Modal::TerminalLinkConfirm
                | Modal::LockVaultConfirm => {}
            }
        }
        assert_eq!(Modal::ALL.len(), 40, "add the new variant to Modal::ALL");
        // Every Esc-closeable modal must also be a known modal.
        for m in Modal::ESC_ORDER {
            assert!(Modal::ALL.contains(m));
        }
    }
}
