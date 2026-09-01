//! Keyboard router for the modal / overlay-menu layer.
//!
//! Runs from the `|v| Message::Terminal(TerminalMessage::KeyboardEvent(v))` arm in `dispatch_terminal.rs`
//! BEFORE the vault keynav router: while a navigable modal, dropdown
//! menu or the burger menu is open, this layer owns the movement and
//! activation keys. It never consumes Esc (that stays with
//! `close_topmost_modal` in `shortcuts.rs`) and never consumes
//! printable characters (picker search fields keep receiving them
//! through iced's real focus).
//!
//! Selection is INDEX-based over `RowAction`s recorded during view()
//! (see `keynav/slots.rs`), tagged with a `ModalSurface` so a stale
//! selection from a previous surface is inert. Three families:
//!
//! - Confirm dialogs: open with the ring on their default (action)
//!   button, Enter/Space activate, Tab and arrows cycle the buttons.
//! - Search pickers (new-tab, tab-jump, group pickers): typing stays
//!   in the input, Up/Down move the row selection, Enter activates
//!   the selection or the top match.
//! - Row menus (kebabs, sort, overflow, burger): open with the first
//!   row ringed, Up/Down move, Enter/Space fire the row's click
//!   message (mouse parity, including how the menu closes).

use iced::keyboard;
use iced::Task;

use crate::app::{Message, Oryxis};
use crate::keynav::movement::index_move;
use crate::keynav::{ModalSurface, RowAction};
use crate::state::Modal;

/// How a surface treats keys beyond the shared movement set.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfaceFamily {
    /// Buttons only (or buttons + inputs, see `has_input`).
    Confirm,
    /// Filtered list under a live search input.
    Picker,
    /// Plain row menu.
    Menu,
}

impl Oryxis {
    /// The topmost keyboard-navigable modal surface, resolved in the
    /// same priority order as `close_topmost_modal` (overlay first,
    /// then `ESC_ORDER`, then the burger menu). Returns `None` for
    /// surfaces that own their own keys (KBI prompt, SFTP dialogs,
    /// rename inputs, theme/icon editors, hover popovers) so those
    /// keep today's behavior untouched, and while a hotkey capture
    /// is pending (the capture must see raw keys).
    pub(crate) fn modal_nav_surface(&self) -> Option<(ModalSurface, SurfaceFamily)> {
        use crate::state::OverlayContent as OC;
        if self.editing_hotkey.is_some() {
            return None;
        }
        if let Some(ov) = &self.overlay {
            let family = match &ov.content {
                // Hover popover and the floating search field have no
                // row rows to navigate. The password-suggest popup
                // (#117) has rows but is deliberately NOT a modal
                // surface: it sits over a live PTY and owns keys only
                // once the user engages it, which
                // `handle_password_suggest_key` decides earlier in the
                // router. Letting the generic layer claim it would eat
                // Up/Down/Enter from the shell underneath.
                OC::SplitMenu | OC::ToolbarSearch | OC::PasswordSuggest { .. } => return None,
                OC::GroupPicker(_) | OC::CloudDiscoverGroupPicker => SurfaceFamily::Picker,
                _ => SurfaceFamily::Menu,
            };
            return Some((
                ModalSurface::Overlay(std::mem::discriminant(&ov.content)),
                family,
            ));
        }
        for &m in Modal::ESC_ORDER {
            if self.is_modal_open(m) {
                let family = match m {
                    // Command palette input carries no on_submit (unlike
                    // NewTabPicker), so the router's Enter path activates
                    // the selection-or-top-match directly.
                    Modal::NewTabPicker | Modal::TabJump | Modal::CommandPalette => {
                        SurfaceFamily::Picker
                    }
                    // Chain editor: the add-a-hop sub-view is a search
                    // picker; the chain list navigates as a row menu.
                    Modal::ChainEditor => {
                        if self.chain_editor_adding {
                            SurfaceFamily::Picker
                        } else {
                            SurfaceFamily::Menu
                        }
                    }
                    Modal::FolderDelete
                    | Modal::CarefulPaste
                    | Modal::SnippetVars
                    | Modal::ErrorDialog
                    | Modal::ClearHistoryConfirm
                    | Modal::SshImport
                    | Modal::ShareDialog
                    | Modal::CloudImportConfirm
                    // Dial security prompts: the REFUSING button is the
                    // default-ringed action on both, so a stray Enter can
                    // never trust a host key or spawn a command proxy
                    // (Esc refuses either way, via ESC_ORDER). Their rows
                    // are recorded by the shared button builders
                    // (`host_key_buttons` / `proxy_command_buttons`), so
                    // the inline connect-progress prompts navigate
                    // identically to the standalone cards.
                    | Modal::HostKey
                    | Modal::ProxyCommand
                    // Security prompt: Deny is the default-ringed action.
                    | Modal::AgentConfirm
                    // Save-confirmation for an edit watch: Yes is the
                    // default-ringed action, Esc skips the save.
                    | Modal::SftpEditPrompt
                    // Reopen-or-redownload: "Reopen the local copy" is the
                    // default-ringed action (it never loses work).
                    | Modal::SftpEditReopen
                    // Read-only cert viewer: Close is the default action;
                    // Remove (when present) is the other recorded row.
                    | Modal::CertificateViewer
                    // Kill-the-process confirm (issue #96): Cancel is
                    // the default-ringed action, so a stray Enter can
                    // never take a remote service down.
                    | Modal::MonitorKill
                    // Manual-lock confirm: Cancel is the default-ringed
                    // action too, so a stray Enter can never sever every
                    // live connection.
                    | Modal::LockVaultConfirm
                    // "Let this rule run a snippet" (C6): Don't send is
                    // the default-ringed action, so a stray Enter can
                    // never hand the session to remote output.
                    | Modal::TriggerConfirm
                    // "Open this link?": Cancel is the default-ringed
                    // action, so a stray Enter never hands a remote
                    // host's URL to the browser.
                    | Modal::TerminalLinkConfirm
                    // The highlight-rule editor is a form, but it walks
                    // like a confirm: Tab / arrows step its rows, Enter
                    // fires the default (Save). Its text fields keep the
                    // caret because it declares `has_input`.
                    | Modal::HighlightRuleEditor => SurfaceFamily::Confirm,
                    // Rename inputs (on_submit), editors, pickers with
                    // their own model: out of this layer. That includes
                    // the theme IMPORT modals: their multiline paste
                    // text_editor makes an Enter-confirms default row
                    // actively harmful (Enter must insert a newline),
                    // and iced can't report editor focus to gate on.
                    // Esc still closes them via ESC_ORDER.
                    _ => return None,
                };
                return Some((ModalSurface::Modal(m), family));
            }
        }
        if self.panels.burger_menu {
            return Some((ModalSurface::Burger, SurfaceFamily::Menu));
        }
        // The SFTP right-click menu is a plain context menu: arrows move,
        // Enter activates, Esc closes (via close_topmost_modal). Lowest
        // precedence, but it never coexists with the surfaces above.
        if self.sftp.row_menu.is_some() {
            return Some((ModalSurface::SftpRowMenu, SurfaceFamily::Menu));
        }
        None
    }

    /// Whether the surface carries a text input, in which case the
    /// caret keeps Left/Right and Space (only Tab/Up/Down cycle).
    fn modal_surface_has_input(surface: ModalSurface) -> bool {
        matches!(
            surface,
            ModalSurface::Modal(
                Modal::ShareDialog
                    | Modal::SshImport
                    | Modal::CloudImportConfirm
                    // The snippet-variables prompt is a column of value
                    // text_inputs; the caret must keep Space (and Left/Right)
                    // so typing a value never fires the default Confirm and
                    // submits the snippet with partial values.
                    | Modal::SnippetVars
                    // Name / pattern / hex fields: the caret keeps Space
                    // (a rule named "Disk full" needs one) and Left /
                    // Right, except on a picker row the user stepped onto.
                    | Modal::HighlightRuleEditor
            )
        )
    }

    /// Entry point, called before `handle_keynav_key`. Returns
    /// `Some(task)` when the key was consumed by the modal layer.
    pub(crate) fn handle_modal_nav_key(
        &mut self,
        event: &keyboard::Event,
    ) -> Option<Task<Message>> {
        let (surface, family) = self.modal_nav_surface()?;
        // A selection left over from a different surface is dead
        // weight; drop it so this surface starts at its default.
        if let Some((tag, _)) = self.keynav.modal.selected
            && tag != surface
        {
            self.keynav.modal.selected = None;
        }
        let keyboard::Event::KeyPressed { key, modifiers, .. } = event else {
            return None;
        };
        if modifiers.control() || modifiers.alt() || modifiers.logo() {
            return None;
        }
        // Any key reaching the modal router flips the modality gate:
        // from here on the (possibly hover-made) selection shows its
        // ring, until the next hover hides it again (focus-visible).
        self.keynav.modal.kbd.set(true);
        let has_input = Self::modal_surface_has_input(surface);
        let len = self.keynav.modal.items.borrow().len();

        // Space activates like Enter on input-less surfaces; with an
        // input present it must keep typing spaces.
        let is_space = matches!(key, keyboard::Key::Named(keyboard::key::Named::Space))
            || matches!(key, keyboard::Key::Character(c) if c.as_str() == " ");
        if is_space {
            if family == SurfaceFamily::Picker || has_input {
                return None;
            }
            return self.modal_nav_activate(surface);
        }
        let keyboard::Key::Named(named) = key else {
            // Printable characters always reach the real input.
            return None;
        };
        use keyboard::key::Named;
        match named {
            Named::Escape => None,
            Named::Enter => {
                // The new-tab picker's search input carries an
                // on_submit that fires on the same press through the
                // widget tree; that handler is selection-aware, so
                // this router must decline Enter there or the action
                // would dispatch twice.
                if surface == ModalSurface::Modal(Modal::NewTabPicker) {
                    return None;
                }
                self.modal_nav_activate(surface)
            }
            Named::Tab => {
                // Cycle rows in confirms and pickers (Tab walks out of the
                // search field into the list, Shift+Tab walks back);
                // consumed no-op in plain menus so a literal \t never
                // lands anywhere and arrows stay the movement keys there.
                if matches!(family, SurfaceFamily::Confirm | SurfaceFamily::Picker)
                    && len > 0
                {
                    self.modal_nav_step(surface, !modifiers.shift());
                }
                Some(Task::none())
            }
            Named::ArrowUp | Named::ArrowDown => {
                if len == 0 {
                    return Some(Task::none());
                }
                self.modal_nav_step(surface, matches!(named, Named::ArrowDown));
                Some(Task::none())
            }
            Named::ArrowLeft | Named::ArrowRight => {
                let rtl = crate::i18n::is_rtl_layout();
                let forward = matches!(named, Named::ArrowRight) != rtl;
                // A picker row the user EXPLICITLY stepped onto cycles
                // its options even on a surface that also carries text
                // fields: the ring says which control the arrows act on,
                // and a picker is never a caret. Explicit only, so a
                // surface whose default row happens to be a picker keeps
                // handing Left/Right to whatever field has focus.
                if matches!(self.keynav.modal.selected, Some((tag, _)) if tag == surface)
                    && let Some(task) = self.modal_nav_cycle_option(surface, forward)
                {
                    return Some(task);
                }
                if has_input || family == SurfaceFamily::Picker {
                    // Caret movement in the surface's input.
                    return None;
                }
                // Picker rows cycle their options; plain rows treat
                // Left/Right as movement (confirm buttons sit side by
                // side).
                if let Some(task) = self.modal_nav_cycle_option(surface, forward) {
                    return Some(task);
                }
                if len > 0 {
                    self.modal_nav_step(surface, forward);
                }
                Some(Task::none())
            }
            Named::Home | Named::End => {
                if len == 0 {
                    return Some(Task::none());
                }
                let idx = if matches!(named, Named::Home) { 0 } else { len - 1 };
                self.keynav.modal.selected = Some((surface, idx));
                Some(Task::none())
            }
            _ => None,
        }
    }

    /// Move the selection one step (wrapping), starting from the
    /// effective position (explicit selection or surface default).
    fn modal_nav_step(&mut self, surface: ModalSurface, forward: bool) {
        let len = self.keynav.modal.items.borrow().len();
        let cur = self.modal_nav_effective(surface);
        if let Some(next) = index_move(len, cur, forward) {
            self.keynav.modal.selected = Some((surface, next));
        }
    }

    /// Enter/Space on the effective row: dispatch its activate
    /// message, focus its input, or (picker family with no explicit
    /// selection) activate the top match.
    fn modal_nav_activate(&mut self, surface: ModalSurface) -> Option<Task<Message>> {
        let idx = self.modal_nav_effective(surface).or({
            // Picker Enter with no selection: the first (top) row.
            if self.keynav.modal.items.borrow().is_empty() {
                None
            } else {
                Some(0)
            }
        })?;
        let action: RowAction = self.keynav.modal.items.borrow().get(idx)?.clone();
        if let Some(id) = action.focus {
            self.keynav.modal.selected = None;
            return Some(crate::widgets::focus_input(id));
        }
        let msg = action.activate?;
        Some(self.update(msg))
    }

    /// Left/Right on a picker row: fire the prepared neighbor
    /// on_select message. `None` when the effective row is not a
    /// picker (caller falls back to movement).
    fn modal_nav_cycle_option(
        &mut self,
        surface: ModalSurface,
        forward: bool,
    ) -> Option<Task<Message>> {
        let idx = self.modal_nav_effective(surface)?;
        let action: RowAction = self.keynav.modal.items.borrow().get(idx)?.clone();
        let msg = if forward { action.next } else { action.prev }?;
        // Keep the selection on the row so repeated arrows keep
        // cycling.
        self.keynav.modal.selected = Some((surface, idx));
        Some(self.update(msg))
    }
}
