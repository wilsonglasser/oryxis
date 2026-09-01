//! What a shortcut aims at, and what it is called.
//!
//! Resolving a strip slot to the tab that occupies it, finding the
//! active tab's connection, naming the search field of the current
//! view, and rendering a binding's label for the UI. Pure lookups: no
//! key is handled here, but every handler needs them.

use iced::widget;

use crate::app::{SftpMessage, TabsMessage, NavigationMessage, Message, Oryxis};
use crate::hotkeys::HotkeyAction;
use crate::state::View;

impl Oryxis {
    /// Resolves slot N (0-indexed) of the visual tab strip to the
    /// `Message` that activates that slot, mirroring the order
    /// `views/tab_bar.rs` renders. Returns `None` when the slot is
    /// out of range so Ctrl+5 on a window with two tabs is a no-op
    /// instead of bouncing focus around.
    pub(crate) fn strip_slot_target(&self, slot: usize) -> Option<Message> {
        // Slot 0 is the Home (Hosts) area tab; the rest follow the unified
        // strip order (terminal + SFTP tabs, pinned-first), exactly as
        // `views/tab_bar.rs` renders it, so Ctrl+N lands on the Nth visible
        // chip. SFTP is a tab now, not a fixed Ctrl+2 area tab.
        let mut slots: Vec<Message> = Vec::new();
        // Home only takes the first slot on vaults that predate the
        // change (see `setting_tab_slot_includes_home`). Off, Ctrl+N is
        // the Nth chip in the strip, which is the number the tab shows;
        // Home stays reachable on Ctrl+Shift+1 (the vault section slot)
        // and on its house icon.
        if self.prefs.tab_slot_includes_home {
            // `GoHome`, not `ChangeView`: this slot IS the house chip, so
            // the key has to keep the folder the click keeps.
            slots.push(Message::Navigation(NavigationMessage::GoHome));
        }
        slots.extend(self.ordered_tab_refs().iter().filter_map(|r| self.tab_ref_select_msg(r)));
        slots.into_iter().nth(slot)
    }

    /// The unified left-to-right strip order (pinned tabs first), terminal
    /// and SFTP tabs interleaved exactly as `views/tab_bar.rs` renders them.
    /// Shared by Ctrl+N slot resolution and Alt+arrow cycling so both honour
    /// the visible order instead of a storage-vec index (which skips SFTP
    /// tabs and ignores pinning).
    pub(crate) fn ordered_tab_refs(&self) -> Vec<crate::state::TabRef> {
        use crate::state::TabRef;
        let pinned_of = |r: &TabRef| -> bool {
            match r {
                TabRef::Terminal(id) => {
                    self.tabs.iter().find(|t| t._id == *id).map(|t| t.pinned).unwrap_or(false)
                }
                TabRef::Sftp(id) => {
                    self.sftp_tabs.iter().find(|t| t.id == *id).map(|t| t.pinned).unwrap_or(false)
                }
                TabRef::Panel(_) => false,
            }
        };
        let mut refs: Vec<TabRef> =
            self.tab_order.iter().copied().filter(|r| pinned_of(r)).collect();
        refs.extend(self.tab_order.iter().copied().filter(|r| !pinned_of(r)));
        refs
    }

    /// The `Message` that activates a given strip tab, or `None` when it can't
    /// be activated (an SFTP tab while SFTP is disabled, or a dangling id).
    pub(crate) fn tab_ref_select_msg(&self, r: &crate::state::TabRef) -> Option<Message> {
        use crate::state::TabRef;
        match r {
            TabRef::Terminal(id) => {
                self.tabs.iter().position(|t| t._id == *id).map(|v| Message::Tabs(TabsMessage::SelectTab(v)))
            }
            TabRef::Sftp(id) => {
                if !self.sftp_enabled {
                    return None;
                }
                self.sftp_tabs.iter().position(|t| t.id == *id).map(|v| Message::Sftp(SftpMessage::SelectSftpTab(v)))
            }
            // Selecting it IS navigating to the panel's view;
            // `ensure_panel_tab` makes that idempotent, so this never
            // opens a second one.
            TabRef::Panel(kind) => self.panel_tab_open(*kind).then(|| {
                Message::Navigation(crate::app::NavigationMessage::ChangeView(kind.view()))
            }),
        }
    }

    /// The currently focused tab as a `TabRef`. Mirrors the strip's own
    /// active model (`views/tab_bar.rs`): `active_sftp` is NOT cleared when a
    /// terminal tab is selected, so an SFTP tab counts as active only on the
    /// SFTP surface (`active_tab` empty and the SFTP view showing). Otherwise
    /// the selected terminal tab wins. Checking `active_sftp` first here was
    /// the bug that made Alt+arrow / Ctrl+Tab jump from a stale SFTP slot.
    pub(crate) fn active_tab_ref(&self) -> Option<crate::state::TabRef> {
        use crate::state::TabRef;
        if self.active_tab.is_none()
            && self.active_view == View::Sftp
            && let Some(i) = self.active_sftp
        {
            return self.sftp_tabs.get(i).map(|t| TabRef::Sftp(t.id));
        }
        if let Some(i) = self.active_tab {
            return self.tabs.get(i).map(|t| TabRef::Terminal(t._id));
        }
        // Same rule as the SFTP arm above: a panel owns the strip slot
        // only while its own surface is the one showing, so Ctrl+Tab can
        // come back to whatever was open before it.
        if let Some(kind) = crate::state::PanelKind::for_view(self.active_view)
            && self.panel_tab_open(kind)
        {
            return Some(TabRef::Panel(kind));
        }
        None
    }

    /// Resolves the active tab to its position in `self.connections`,
    /// or `None` when no tab is active, the tab is a local shell, or
    /// the saved host has since been deleted. Used by Ctrl+P to open
    /// the host editor for the current connection.
    pub(crate) fn active_tab_connection_idx(&self) -> Option<usize> {
        let tab_idx = self.active_tab?;
        let tab = self.tabs.get(tab_idx)?;
        let base_label = tab.label.trim_end_matches(" (disconnected)");
        self.connections.iter().position(|c| c.label == base_label)
    }

    /// Returns the `widget::Id` of the search/filter input for the
    /// current view, or `None` when the view has no search field.
    /// Consumed by `Message::Tabs(TabsMessage::FocusViewSearch)` (Ctrl+F).
    pub(crate) fn active_view_search_id(&self) -> Option<widget::Id> {
        match self.active_view {
            // First run builds no toolbar, so there is no search field
            // to focus: Ctrl+F no-ops and Tab skips the search zone
            // instead of opening the floating field over an empty
            // screen.
            View::Dashboard => (!self.dashboard_is_empty())
                .then(|| widget::Id::new("search-dashboard")),
            View::Keys => Some(widget::Id::new("search-keys")),
            // The network tools panel has a target field, not a search
            // field: Ctrl+F there would focus the thing the panel runs
            // against, which is not what find means.
            View::NetworkTools => None,
            // Snippets and History only expose their search field on
            // the Workspace-mode sub-nav. In Classic mode there's no
            // search input to focus, so Ctrl+F harmlessly tries to
            // focus an Id that doesn't exist (iced no-ops on a miss).
            View::Snippets => Some(widget::Id::new("search-snippets")),
            View::PortForwarding => Some(widget::Id::new("search-port-forwards")),
            View::History => Some(widget::Id::new("search-history")),
            View::Sftp => {
                // Two filter inputs (local + remote panes); focus
                // the remote one since that's the side that costs an
                // SSH round-trip and is where a typed filter matters
                // most.
                Some(widget::Id::new("search-sftp-remote"))
            }
            View::Cloud => Some(widget::Id::new("search-cloud")),
            View::Proxies => Some(widget::Id::new("search-proxies")),
            // A hybrid tab in Files mode is the SFTP surface: Ctrl+F
            // focuses the remote filter, parity with View::Sftp above.
            View::Terminal => self
                .sftp_surface_visible()
                .then(|| widget::Id::new("search-sftp-remote")),
            // The Settings sidebar search is the view's "zone zero":
            // Ctrl+F focuses it and Tab cycles Search → sections →
            // content like any vault view.
            View::Settings => Some(widget::Id::new("search-settings")),
            View::KnownHosts => None,
            View::Monitoring => (!self.dash_hosts().is_empty())
                .then(|| widget::Id::new("search-monitor")),
        }
    }

    /// Pretty-printed binding for the given action (`"Ctrl + K"`),
    /// or `None` when the action has no binding (conflict-unbound).
    /// Used by the burger menu / context menus to surface the
    /// current shortcut next to its action.
    pub(crate) fn hotkey_label_for_action(
        &self,
        action: HotkeyAction,
    ) -> Option<String> {
        let binding = self.hotkey_bindings.get(&action)?;
        Some(binding.badges()?.join(" + "))
    }

    /// Pretty-printed binding for the Nth strip slot (0-indexed),
    /// e.g. `"Ctrl + 1"` for slot 0 when `SwitchToTabSlot` is
    /// bound to Ctrl + digit. Returns `None` when the family is
    /// unbound. Used by the burger menu to show the per-area
    /// shortcut next to Hosts / SFTP.
    pub(crate) fn hotkey_label_for_strip_slot(
        &self,
        slot: usize,
    ) -> Option<String> {
        let binding = self.hotkey_bindings.get(&HotkeyAction::SwitchToTabSlot)?;
        let mut parts = binding.badges()?;
        // Drop the family suffix ("1...9") and append the concrete
        // slot digit so the hint reads like a real chord.
        parts.pop();
        parts.push((slot + 1).to_string());
        Some(parts.join(" + "))
    }

    /// Pretty-printed binding for the Nth vault section (1-indexed
    /// digit), e.g. `"Ctrl + Shift + 2"` for Keychain. Same concrete
    /// digit treatment as the strip-slot label; `None` when the
    /// `VaultSectionSlot` family is unbound. Used by the burger
    /// menu's VAULT entries.
    pub(crate) fn hotkey_label_for_vault_slot(
        &self,
        digit: usize,
    ) -> Option<String> {
        let binding = self.hotkey_bindings.get(&HotkeyAction::VaultSectionSlot)?;
        let mut parts = binding.badges()?;
        parts.pop();
        parts.push(digit.to_string());
        Some(parts.join(" + "))
    }
}
