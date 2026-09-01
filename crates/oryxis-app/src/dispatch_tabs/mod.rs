//! `Oryxis::handle_tabs`, match arms for the tab strip + tab modals
//! (new-tab picker, tab-jump, icon picker), card hover/menu, folder
//! actions, window chrome (drag/resize/min/max/close).

#![allow(clippy::result_large_err)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]

mod hybrid;
mod icon_picker;
mod lifecycle;
mod merge;
mod ordering;
mod window;

use iced::Task;

use crate::app::{SettingsMessage, TabsMessage, TerminalMessage, SshMessage, CloudMessage, NavigationMessage, Message, Oryxis};
use crate::state::{OverlayContent, OverlayState, View};

/// Smallest gap between two `WindowDrag` / `WindowResizeDrag`
/// presses we'll accept. iced's `MouseArea` re-fires `on_press` on
/// the second click of a double-click before `on_double_click` lands;
/// honouring that second drag races our `toggle_maximize` /
/// `WindowExpand*` follow-up. `300ms` is wider than any realistic
/// double-click and short enough that a deliberate two-quick-clicks-
/// to-drag still feels responsive.
const WINDOW_PRESS_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(300);

impl Oryxis {
    /// Index of the tab with this id, or `None` if it is gone.
    ///
    /// Anything that survives across updates (a pending split, a routed
    /// continuation) must hold an id and resolve it here, never cache a
    /// position: `Oryxis::tabs` shifts on every close and on a tab merge,
    /// and a stale index silently addresses a different tab.
    pub(crate) fn tab_index_by_id(&self, id: uuid::Uuid) -> Option<usize> {
        self.tabs.iter().position(|t| t._id == id)
    }

    /// Returns `true` when this press should be forwarded to the OS.
    /// Returns `false` when the previous press was within
    /// [`WINDOW_PRESS_DEBOUNCE`], swallowing the spurious second
    /// `on_press` that a double-click emits.
    pub(crate) fn consume_window_press(&mut self) -> bool {
        let now = std::time::Instant::now();
        let allow = self
            .last_window_press_at
            .is_none_or(|prev| now.duration_since(prev) >= WINDOW_PRESS_DEBOUNCE);
        if allow {
            self.last_window_press_at = Some(now);
        }
        allow
    }

    /// Route one tabs-domain message to the surface that owns it.
    ///
    /// Was a 1081-line `match` with 106 arms, the largest function in the
    /// crate. The groups are surfaces the user can point at: the window,
    /// the strip, a picker, a menu, an inline rename, a group.
    pub(crate) fn handle_tabs(&mut self, message: TabsMessage) -> Task<Message> {
        match message {
            m @ (
                TabsMessage::MouseMoved(..)
                | TabsMessage::DragOutReady(..)
                | TabsMessage::WindowResized(..)
                | TabsMessage::WindowMoved(..)
                | TabsMessage::WindowEnsureOnScreen
                | TabsMessage::WindowFocusChanged(..)
                | TabsMessage::WindowDrag
                | TabsMessage::WindowResizeDrag(..)
                | TabsMessage::SidePanelResizeStart
                | TabsMessage::WindowExpandVertical
                | TabsMessage::WindowMinimize
                | TabsMessage::WindowMaximizeToggle
                | TabsMessage::WindowMaximizedSynced(..)
                | TabsMessage::WindowClose
                | TabsMessage::WindowFullscreenToggle
                | TabsMessage::FullscreenHintHide
                | TabsMessage::SpawnNewWindow
            ) => self.handle_tabs_window(m),
            m @ (
                TabsMessage::CardHovered(..)
                | TabsMessage::CardUnhovered(..)
                | TabsMessage::FolderCardHovered(..)
                | TabsMessage::FolderCardUnhovered(..)
                | TabsMessage::KeyCardHovered(..)
                | TabsMessage::KeyCardUnhovered(..)
                | TabsMessage::IdentityCardHovered(..)
                | TabsMessage::IdentityCardUnhovered(..)
                | TabsMessage::SnippetCardHovered(..)
                | TabsMessage::SnippetCardUnhovered(..)
                | TabsMessage::PanelTabHovered(..)
                | TabsMessage::PanelTabUnhovered(..)
                | TabsMessage::TabHovered(..)
                | TabsMessage::TabUnhovered(..)
                | TabsMessage::TabCloseDwell(..)
            ) => self.handle_tabs_hover(m),
            m @ (
                TabsMessage::SelectTab(..)
                | TabsMessage::CloseTab(..)
                | TabsMessage::CloseTabFromStrip(..)
                | TabsMessage::ConfirmCloseGroupedTab(..)
                | TabsMessage::ReopenClosedTab
                | TabsMessage::CloseOtherTabs(..)
                | TabsMessage::CloseAllTabs
                | TabsMessage::ClosePanelTab(..)
                | TabsMessage::ToggleTabPin(..)
                | TabsMessage::ReconnectTab(..)
                | TabsMessage::DuplicateTab(..)
                | TabsMessage::DuplicateInNewWindow(..)
                | TabsMessage::TabDragToEnd
                | TabsMessage::TabBarWheel(..)
                | TabsMessage::ShowTabMenu(..)
                | TabsMessage::ShowTabBarMenu
                | TabsMessage::ShowSplitMenu
                | TabsMessage::SplitMenuEnter
                | TabsMessage::SplitMenuLeave
                | TabsMessage::SplitMenuCloseIfIdle
                | TabsMessage::ActivateStripSlot(..)
                | TabsMessage::CopyTabAddress(..)
                | TabsMessage::ToggleTabFilesMode(..)
                | TabsMessage::ShowTabSurface(..)
                | TabsMessage::DetachTabSftp(..)
                | TabsMessage::CloseTabSftpSession(..)
                | TabsMessage::OpenTerminalForSftpTab(..)
                | TabsMessage::SsmKeepaliveTick
                | TabsMessage::BusyAnimTick
            ) => self.handle_tabs_strip(m),
            m @ (
                TabsMessage::ShowNewTabPicker
                | TabsMessage::HideNewTabPicker
                | TabsMessage::NewTabPickerOpenGroup(..)
                | TabsMessage::NewTabPickerBack
                | TabsMessage::NewTabPickerSearchChanged(..)
                | TabsMessage::NewTabPickerSubmit
                | TabsMessage::PickLocalShell
                | TabsMessage::ShowTabJump
                | TabsMessage::HideTabJump
                | TabsMessage::TabJumpSearchChanged(..)
                | TabsMessage::TabJumpSelect(..)
                | TabsMessage::ShowCommandPalette
                | TabsMessage::HideCommandPalette
                | TabsMessage::PaletteQueryChanged(..)
                | TabsMessage::PaletteActivate(..)
                | TabsMessage::ShowIconPicker(..)
                | TabsMessage::HideIconPicker
                | TabsMessage::IconPickerSelectIcon(..)
                | TabsMessage::IconPickerIconSearchChanged(..)
                | TabsMessage::IconPickerOpenColorPopover
                | TabsMessage::IconPickerCloseColorPopover
                | TabsMessage::IconPickerSelectColor(..)
                | TabsMessage::IconPickerHexInputChanged(..)
                | TabsMessage::IconPickerSave
                | TabsMessage::IconPickerResetAuto
            ) => self.handle_tabs_pickers(m),
            m @ (
                TabsMessage::ToggleBurgerMenu
                | TabsMessage::ToggleSubnavOverflow
                | TabsMessage::HideOverlayMenu
                | TabsMessage::ShowCardMenu(..)
                | TabsMessage::ShowTreeHostMenu(..)
                | TabsMessage::HideCardMenu
                | TabsMessage::RunHotkeyAction(..)
                | TabsMessage::OpenSettingsSection(..)
                | TabsMessage::FocusViewSearch
            ) => self.handle_tabs_menus(m),
            m @ (
                TabsMessage::StartRenameTab(..)
                | TabsMessage::StartRenameSftpTab(..)
                | TabsMessage::TabRenameInput(..)
                | TabsMessage::ConfirmTabRename
                | TabsMessage::CancelTabRename
                | TabsMessage::ShowFolderActions(..)
                | TabsMessage::StartRenameFolder(..)
                | TabsMessage::FolderRenameInput(..)
                | TabsMessage::ConfirmRenameFolder
                | TabsMessage::CancelFolderModal
            ) => self.handle_tabs_rename(m),
            m @ (
                TabsMessage::EditGroup(..)
                | TabsMessage::NewSubgroup(..)
                | TabsMessage::NewGroup
                | TabsMessage::GroupEditLabelChanged(..)
                | TabsMessage::GroupEditParentChanged(..)
                | TabsMessage::GroupEditToggleDefaults
                | TabsMessage::GroupEditDefaultUsername(..)
                | TabsMessage::GroupEditDefaultPort(..)
                | TabsMessage::GroupEditDefaultIdentity(..)
                | TabsMessage::GroupEditDefaultProxyIdentity(..)
                | TabsMessage::GroupEditDefaultTheme(..)
                | TabsMessage::GroupEditDefaultSnippet(..)
                | TabsMessage::GroupEditEnvAdd
                | TabsMessage::GroupEditEnvRemove(..)
                | TabsMessage::GroupEditEnvKey(..)
                | TabsMessage::GroupEditEnvValue(..)
                | TabsMessage::ShowGroupEditIconPicker
                | TabsMessage::SaveGroupEdit
                | TabsMessage::CancelGroupEdit
                | TabsMessage::StartDeleteFolder(..)
                | TabsMessage::DeleteFolderKeepHosts
                | TabsMessage::DeleteFolderWithHosts
            ) => self.handle_tabs_groups(m),
        }
    }
}

mod group_defaults;
mod groups;
mod hover;
mod menus;
mod pickers;
mod rename;
mod reopen;
mod strip;
