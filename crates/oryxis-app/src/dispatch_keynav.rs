//! Vault-area focus-zone keyboard router.
//!
//! Helper impl called from the `|v| Message::Terminal(TerminalMessage::KeyboardEvent(v))` arm in
//! `dispatch_terminal.rs` (same pattern as `shortcuts.rs`, not a
//! `try_handler!` link: `KeyboardEvent` stays owned by
//! `handle_terminal`). Returns `Some(task)` when the event was
//! consumed; everything it declines falls through to the hotkey
//! table right below the call site.
//!
//! Model (see `keynav/mod.rs` for the types): Tab / Shift+Tab cycle
//! the zones Search -> Toolbar -> Content -> SubNav, arrows move
//! within the active zone, Enter activates, Esc returns to idle.
//! Search is "zone zero" (`focus == None`), owned by iced's real
//! text_input focus.

use iced::keyboard;
use iced::Task;

use crate::app::{SettingsMessage, TabsMessage, EditorMessage, KeysMessage, SshMessage, CloudMessage, HistoryMessage, NavigationMessage, ProxyIdentityMessage, KnownHostMessage, SessionGroupMessage, PortForwardMessage, SnippetMessage, DashNavItem, Message, Oryxis};
use crate::keynav::movement::{cycle_zone, grid_move, index_move, linear_move, MoveKey};
use crate::keynav::{FocusZone, NavItem, ToolbarItem};
use crate::state::View;

/// Focusing a nonexistent id blurs every focusable widget (the
/// `focus` operation unfocuses all non-matching ones). Used when the
/// selection leaves the search zone so we're clearly in keynav mode.
fn blur_task() -> Task<Message> {
    iced::widget::operation::focus(iced::widget::Id::new("__keynav_blur__"))
}

impl Oryxis {
    /// Entry point. Consumes plain (unmodified) Tab / Shift+Tab /
    /// arrows / Enter / Esc / Home / End while the vault area is
    /// active with no side panel or blocking modal open.
    pub(crate) fn handle_keynav_key(
        &mut self,
        event: &keyboard::Event,
    ) -> Option<Task<Message>> {
        if self.any_modal_blocks_input() {
            return None;
        }
        // An open side panel owns the keyboard through the row-mode
        // router (dispatch_keynav_panel.rs). Panels that don't record
        // rows yet leave `panel_items` empty, which the router treats
        // as "decline everything": same as before, zero risk.
        if self.side_panel_open() {
            if let Some(task) = self.handle_panel_nav_key(event) {
                return Some(task);
            }
            // The panel declined. ONE key can still belong to the vault
            // behind it: a bare Enter in the search box, which is the
            // only surface whose focus iced cannot report, so nothing
            // else can tell us the user is typing there (issue #175:
            // opening the editor killed quick connect outright).
            //
            // The key check is not a formality. The panel router
            // declines every ordinary character too, so falling through
            // on anything else connects mid-word: typing `root@10.0.0.1`
            // dials `roo` on the third keystroke, because by then the
            // search box holds a bare hostname that matches no saved
            // host and quick connect offers it.
            //
            // Only a NAMED target goes through, never the "top filtered
            // result" step: with a panel open the Enter may belong to a
            // field inside it, and `user@host` is an unambiguous ask
            // where "whatever sorts first" is not.
            let keyboard::Event::KeyPressed { key, modifiers, .. } = event else {
                return None;
            };
            if !modifiers.is_empty()
                || !matches!(key, keyboard::Key::Named(keyboard::key::Named::Enter))
            {
                return None;
            }
            return self.search_named_connect();
        }
        let in_settings = self.active_tab.is_none() && self.active_view == View::Settings;
        // The network tools panel records its rows on the same ring
        // Settings uses (`settings_nav_slot` -> `NavItem::SettingsRow`),
        // so opening the gate is the whole wiring: movement, Enter and
        // the picker's Left/Right already act on whatever recorded.
        let in_net_tools =
            self.active_tab.is_none() && self.active_view == View::NetworkTools;
        if !self.in_vault_area() && !in_settings && !in_net_tools {
            return None;
        }
        // Settings-only declines: a pending hotkey capture must see
        // raw keys (its interceptor lives below this router), and the
        // Security password forms keep their dedicated Tab-walk plus
        // full arrow/Enter ownership by the focused fields.
        if in_settings
            && (self.editing_hotkey.is_some()
                || (self.settings_section == crate::state::SettingsSection::Security
                    && (self.vault_ui.show_password_form || self.vault_ui.change_password_open)))
        {
            return None;
        }
        let keyboard::Event::KeyPressed { key, modifiers, .. } = event else {
            return None;
        };
        // Ctrl/Alt/Logo combos belong to the hotkey table (Ctrl+F,
        // Ctrl+PageUp/Down, ...); Shift is only meaningful for Tab.
        if modifiers.control() || modifiers.alt() || modifiers.logo() {
            return None;
        }
        // Space presses the selected control, matching the desktop
        // convention (buttons respond to Space AND Enter). Only while
        // a zone selection is active: idle Space belongs to the search
        // input (it types a space).
        if self.keynav.focus.is_some()
            && let keyboard::Key::Character(c) = key
            && c.as_str() == " "
        {
            return self.keynav_activate();
        }
        let keyboard::Key::Named(named) = key else {
            // Typing falls through so it reaches the search input via
            // iced's own focus.
            return None;
        };
        use keyboard::key::Named;
        // Settings search find-next: Enter / Shift+Enter cycle the
        // matches (find-in-page), but only while the search box owns the
        // keyboard (focus idle) and there is a query. When the user has
        // Tabbed into the content (focus == Content), Enter activates the
        // selected row instead, so this can't hijack it.
        if in_settings
            && self.keynav.focus.is_none()
            && !self.settings_search.trim().is_empty()
            && matches!(named, Named::Enter)
        {
            return Some(self.update(Message::Settings(SettingsMessage::SettingsSearchStep(
                !modifiers.shift(),
            ))));
        }
        match named {
            Named::Tab => {
                // Settings content, ring idle: a mouse-focused text input
                // (the export / import password, the sync passphrase, the
                // auto-lock field) owns its keys. Resolve what iced
                // actually has focused, then walk the recorded rows from
                // that field (the panel contract: inputs get real focus,
                // the rest ring) instead of parking the ring on the FIRST
                // content row — the Security section's "Vault Password"
                // toggle — and scrolling the page away mid-typing. The
                // fall-through keeps the zone cycle (Tab from the search
                // field enters Content at its first row).
                // Leaving the sync passphrase field by Tab is a deliberate
                // walk-away: an open edit is abandoned, empty or not.
                if self.sync.passphrase_editing {
                    self.exit_passphrase_edit();
                }
                if let Some(task) = self.settings_ring_idle_resolve(*named, modifiers.shift()) {
                    return Some(task);
                }
                Some(self.keynav_cycle_zones(!modifiers.shift()))
            }
            // Desktop convention: the Menu key (and Shift+F10) opens
            // the selected item's context menu.
            Named::ContextMenu => self.keynav_open_context_menu(),
            Named::F10 if modifiers.shift() => self.keynav_open_context_menu(),
            Named::Enter => self.keynav_activate(),
            // Delete removes the ringed item, always through the same
            // confirmation its menu entry uses. Before this the vault had
            // no keyboard delete at ALL (the key was only bound in the
            // terminal sidebar), so the only path was Enter into an editor
            // and out through its Delete row.
            Named::Delete => self.keynav_delete(),
            // Space is delivered as a named key by winit (see
            // `util.rs` PTY routing); same rule as the Character
            // guard above.
            Named::Space if self.keynav.focus.is_some() => self.keynav_activate(),
            Named::Escape => {
                // An open sync passphrase edit is cancelled by Esc like
                // any other cancel: back to the read-only mask.
                if self.sync.passphrase_editing {
                    self.exit_passphrase_edit();
                }
                self.keynav_escape()
            }
            Named::ArrowUp
            | Named::ArrowDown
            | Named::ArrowLeft
            | Named::ArrowRight
            | Named::Home
            | Named::End => {
                if modifiers.shift() {
                    // Shift+arrow is text selection in the search
                    // field; never ours.
                    return None;
                }
                // Same mouse-focused-field rule as Tab above: while the
                // ring is idle in Settings the arrow belongs to the
                // input's own caret, not to Content-entry at row 0.
                if let Some(task) = self.settings_ring_idle_resolve(*named, modifiers.shift()) {
                    return Some(task);
                }
                // Ring idle in the other vault views: Up/Down only enter
                // the content zone once iced confirms no non-search text
                // input holds focus (issue #168, the empty dashboard's
                // quick-host field under numpad NumLock-off arrows).
                if let Some(task) = self.vault_ring_idle_resolve(*named) {
                    return Some(task);
                }
                self.keynav_move(*named)
            }
            _ => None,
        }
    }

    /// Tab / Shift+Tab: step to the next / previous content SECTION
    /// while inside the content zone (dashboard Groups -> Hosts,
    /// keychain Keys -> Identities), then move between zones in
    /// visual order (Search -> Toolbar -> Content -> SubNav, wrap).
    fn keynav_cycle_zones(&mut self, forward: bool) -> Task<Message> {
        if let Some((FocusZone::Content, cur)) = self.keynav.focus
            && let Some(section) = self.keynav_content_section_of(cur)
        {
            let target = if forward {
                Some(section + 1)
            } else {
                section.checked_sub(1)
            };
            if let Some(t) = target
                && let Some(item) = self.keynav_content_section_entry(t)
            {
                self.keynav.focus = Some((FocusZone::Content, item));
                return self.keynav_after_move(FocusZone::Content, item);
            }
            // No section left in this direction: fall through to the
            // zone change below.
        }
        let cur = self.keynav.focus.map(|(z, _)| z);
        let next = cycle_zone(
            cur,
            forward,
            self.active_view_search_id().is_some(),
            self.keynav.subnav_items.borrow().is_empty(),
            self.keynav.toolbar_items.borrow().is_empty(),
            self.keynav.content_rows.borrow().iter().all(|r| r.is_empty()),
        );
        // Consume the Tab even when there's nowhere else to go, so a
        // literal \t never leaks anywhere from the vault surface.
        if next == cur {
            return Task::none();
        }
        self.keynav_enter_zone(next, forward)
    }

    /// Content-section boundaries recorded at render time; a missing
    /// record (older single-recording views) counts as one section.
    fn keynav_content_section_starts(&self) -> Vec<usize> {
        let starts = self.keynav.content_section_starts.borrow().clone();
        if starts.is_empty() {
            vec![0]
        } else {
            starts
        }
    }

    /// Which section the item's row belongs to.
    fn keynav_content_section_of(&self, item: NavItem) -> Option<usize> {
        let rows = self.keynav.content_rows.borrow();
        let row = rows.iter().position(|r| r.contains(&item))?;
        let starts = self.keynav_content_section_starts();
        Some(starts.iter().rposition(|&s| s <= row).unwrap_or(0))
    }

    /// First item of the given section, `None` when it doesn't exist.
    fn keynav_content_section_entry(&self, section: usize) -> Option<NavItem> {
        let rows = self.keynav.content_rows.borrow();
        let starts = self.keynav_content_section_starts();
        let start = *starts.get(section)?;
        let end = starts.get(section + 1).copied().unwrap_or(rows.len());
        rows.get(start..end)?.iter().flatten().copied().next()
    }

    /// Land on `zone` (`None` = the search field) picking a sensible
    /// entry item, and keep iced's text_input focus in sync (focus
    /// the search input when entering Search, blur it when leaving).
    fn keynav_enter_zone(&mut self, zone: Option<FocusZone>, forward: bool) -> Task<Message> {
        let was_idle = self.keynav.focus.is_none();
        let Some(zone) = zone else {
            self.keynav.focus = None;
            self.panels.subnav_overflow = false;
            let Some(id) = self.active_view_search_id() else {
                return Task::none();
            };
            // Narrow window: the search field is folded to an icon, so
            // focusing its id would no-op. Open the floating field
            // instead (it focuses itself); when it is already open,
            // fall through to a plain focus. The tier math describes
            // the VAULT toolbar; Settings has no toolbar and its
            // sidebar search never folds, so it always plain-focuses.
            let (search_collapsed, _) = if self.active_view == View::Settings {
                (false, false)
            } else {
                self.toolbar_tiers()
            };
            let floating_open = matches!(
                self.overlay.as_ref().map(|o| &o.content),
                Some(crate::state::OverlayContent::ToolbarSearch)
            );
            if search_collapsed && !floating_open {
                return self.update(Message::Navigation(NavigationMessage::ToggleToolbarSearch));
            }
            return crate::widgets::focus_input(id);
        };
        let Some(item) = self.keynav_zone_entry_item(zone, forward) else {
            // Zone unexpectedly empty (cycle_zone said otherwise only
            // if the lists changed under us); treat as a no-op.
            return Task::none();
        };
        self.keynav.focus = Some((zone, item));
        let mut tasks: Vec<Task<Message>> = Vec::new();
        if was_idle {
            tasks.push(blur_task());
        }
        tasks.push(self.keynav_after_move(zone, item));
        Task::batch(tasks)
    }

    /// The item the selection lands on when entering a zone: the
    /// active view's own pill for SubNav (so Tab highlights where you
    /// are), the first toolbar button, and for Content the first item
    /// of the first section going forward / of the LAST section going
    /// backward (Shift+Tab from the section nav lands on Hosts, one
    /// more Shift+Tab climbs to Groups).
    fn keynav_zone_entry_item(&self, zone: FocusZone, forward: bool) -> Option<NavItem> {
        match zone {
            FocusZone::SubNav => {
                let items = self.keynav.subnav_items.borrow();
                items
                    .iter()
                    .copied()
                    .find(|i| match i {
                        // Land on where the user already is: the
                        // active view's pill, or the active Settings
                        // section's sidebar entry.
                        NavItem::SubNav(v) => *v == self.active_view,
                        NavItem::SettingsSection(s) => *s == self.settings_section,
                        _ => false,
                    })
                    .or_else(|| {
                        if forward {
                            items.first().copied()
                        } else {
                            items.last().copied()
                        }
                    })
            }
            FocusZone::Toolbar => {
                let items = self.keynav.toolbar_items.borrow();
                if forward {
                    items.first().copied()
                } else {
                    items.last().copied()
                }
            }
            FocusZone::Content => {
                let section = if forward {
                    0
                } else {
                    self.keynav_content_section_starts().len().saturating_sub(1)
                };
                self.keynav_content_section_entry(section)
            }
        }
    }

    /// Arrows / Home / End: move within the active zone. From idle,
    /// Down / Up enter the content zone at its first / last item
    /// (the pre-existing dashboard muscle memory, now on every view);
    /// Left / Right stay with the search caret.
    fn keynav_move(&mut self, named: keyboard::key::Named) -> Option<Task<Message>> {
        use keyboard::key::Named;
        match self.keynav.focus {
            None => match named {
                Named::ArrowDown => Some(self.keynav_enter_zone(Some(FocusZone::Content), true)),
                Named::ArrowUp => Some(self.keynav_enter_zone(Some(FocusZone::Content), false)),
                _ => None,
            },
            Some((FocusZone::SubNav, cur)) => self.keynav_move_subnav(named, cur),
            Some((FocusZone::Toolbar, cur)) => self.keynav_move_toolbar(named, cur),
            Some((FocusZone::Content, cur)) => self.keynav_move_content(named, cur),
        }
    }

    /// SubNav zone movement: Left/Right across the horizontal pills
    /// (visually mirrored under RTL), Up/Down along the vertical
    /// rail. The cross-axis keys are not consumed.
    fn keynav_move_subnav(
        &mut self,
        named: keyboard::key::Named,
        cur: NavItem,
    ) -> Option<Task<Message>> {
        use keyboard::key::Named;
        let items = self.keynav.subnav_items.borrow().clone();
        // The Settings sections sidebar is always a vertical list.
        let vertical = self.prefs.nav_orientation == "vertical"
            || self.active_view == View::Settings;
        let rtl = crate::i18n::is_rtl_layout();
        let new = match (named, vertical) {
            (Named::ArrowRight, false) => linear_move(&items, Some(cur), !rtl),
            (Named::ArrowLeft, false) => linear_move(&items, Some(cur), rtl),
            (Named::ArrowDown, true) => linear_move(&items, Some(cur), true),
            (Named::ArrowUp, true) => linear_move(&items, Some(cur), false),
            (Named::Home, _) => items.first().copied(),
            (Named::End, _) => items.last().copied(),
            _ => return None,
        }?;
        self.keynav.focus = Some((FocusZone::SubNav, new));
        Some(self.keynav_after_move(FocusZone::SubNav, new))
    }

    /// Toolbar zone movement: Left/Right along the action cluster
    /// (visually mirrored under RTL, the cluster is a `dir_row`).
    fn keynav_move_toolbar(
        &mut self,
        named: keyboard::key::Named,
        cur: NavItem,
    ) -> Option<Task<Message>> {
        use keyboard::key::Named;
        let items = self.keynav.toolbar_items.borrow().clone();
        let rtl = crate::i18n::is_rtl_layout();
        let new = match named {
            Named::ArrowRight => linear_move(&items, Some(cur), !rtl),
            Named::ArrowLeft => linear_move(&items, Some(cur), rtl),
            Named::Home => items.first().copied(),
            Named::End => items.last().copied(),
            _ => return None,
        }?;
        self.keynav.focus = Some((FocusZone::Toolbar, new));
        Some(Task::none())
    }

    /// Content zone movement: 2-D over the recorded visual rows.
    /// Settings picker rows own Left/Right (option cycling without
    /// opening the dropdown) before positional movement applies.
    fn keynav_move_content(
        &mut self,
        named: keyboard::key::Named,
        cur: NavItem,
    ) -> Option<Task<Message>> {
        use keyboard::key::Named;
        // Tree view mode (issue #102): Left/Right on a MANUAL folder
        // row fold/unfold it in place, the universal tree-widget
        // convention, intercepted before `grid_move` consumes the keys
        // as linear prev/next (same shape as the SettingsRow picker
        // interception below). Dynamic groups are drill-in leaves
        // here, and an already-collapsed Left / expanded Right falls
        // through to plain movement so the keys never go dead. An
        // active SEARCH force-expands every fold, so the interception
        // steps aside too: toggling the remembered set under a search
        // changes nothing on screen (Left looked dead) and re-surfaces
        // as a surprise fold state once the needle clears.
        if matches!(named, Named::ArrowLeft | Named::ArrowRight)
            && self.active_view == View::Dashboard
            && self.prefs.host_view_mode == crate::state::HostViewMode::Tree
            && self.host_search.trim().is_empty()
            && let NavItem::Dash(DashNavItem::Group(gid)) = cur
            && self
                .groups
                .iter()
                .any(|g| g.id == gid && g.cloud_query.is_none())
        {
            let rtl = crate::i18n::is_rtl_layout();
            let expand = matches!(named, Named::ArrowRight) != rtl;
            let expanded = self.hosts_tree_expanded.contains(&gid);
            if expand != expanded {
                return Some(
                    self.update(Message::Ai(crate::app::AiMessage::HostsTreeToggleGroup(gid))),
                );
            }
        }
        if matches!(named, Named::ArrowLeft | Named::ArrowRight)
            && let NavItem::SettingsRow(idx) = cur
        {
            let action = self.keynav.settings_row_actions.borrow().get(idx).cloned();
            if let Some(a) = action {
                let rtl = crate::i18n::is_rtl_layout();
                let forward = matches!(named, Named::ArrowRight) != rtl;
                let msg = if forward { a.next } else { a.prev };
                if let Some(msg) = msg {
                    return Some(self.update(msg));
                }
                // A non-picker row: Left/Right have nothing positional
                // to do in a single-column list; consume as a no-op.
                return Some(Task::none());
            }
        }
        let key = match named {
            Named::ArrowUp => MoveKey::Up,
            Named::ArrowDown => MoveKey::Down,
            Named::ArrowLeft => MoveKey::Left,
            Named::ArrowRight => MoveKey::Right,
            Named::Home => MoveKey::Home,
            Named::End => MoveKey::End,
            _ => return None,
        };
        let rows = self.keynav.content_rows.borrow().clone();
        let new = grid_move(
            &rows,
            Some(cur),
            key,
            crate::i18n::is_rtl_layout(),
            self.content_list_mode(),
        )?;
        self.keynav.focus = Some((FocusZone::Content, new));
        Some(self.keynav_after_move(FocusZone::Content, new))
    }

    /// Post-movement bookkeeping per zone: keep the content selection
    /// scrolled into view; auto-open / close the sub-nav overflow
    /// menu and keep the vertical rail scrolled.
    fn keynav_after_move(&mut self, zone: FocusZone, item: NavItem) -> Task<Message> {
        match zone {
            FocusZone::Content => self.keynav_scroll_content_to(item),
            FocusZone::SubNav => {
                let vertical = self.prefs.nav_orientation == "vertical"
                    || self.active_view == View::Settings;
                if vertical {
                    self.panels.subnav_overflow = false;
                    let items = self.keynav.subnav_items.borrow();
                    let pos = items.iter().position(|&i| i == item).unwrap_or(0);
                    let denom = items.len().saturating_sub(1).max(1);
                    let rail_id = if self.active_view == View::Settings {
                        "settings-sidebar-scroll"
                    } else {
                        "vault-nav-rail-scroll"
                    };
                    return iced::widget::operation::snap_to(
                        iced::widget::Id::new(rail_id),
                        iced::widget::operation::RelativeOffset {
                            x: None,
                            y: Some(pos as f32 / denom as f32),
                        },
                    );
                }
                // Highlight moved into an overflowed pill: pop the
                // "⋯" menu so the selection stays visible; back to an
                // inline pill closes it again.
                let (_, overflow) = self.subnav_pill_split();
                self.panels.subnav_overflow = matches!(
                    item,
                    NavItem::SubNav(v) if overflow.iter().any(|(_, ov)| *ov == v)
                );
                Task::none()
            }
            FocusZone::Toolbar => Task::none(),
        }
    }

    /// What the dashboard search box connects when it NAMES a target:
    /// a saved host by label (or by its canonical `user@host`), else the
    /// ad-hoc quick-connect target the "Enter to connect" chip
    /// advertises. `None` when the text names neither.
    ///
    /// Split out of [`keynav_activate`](Self::keynav_activate) because
    /// the side-panel path (issue #175) may fall back to it and the
    /// "top filtered result" step must NOT come along there: with a
    /// panel open the Enter may well belong to a field inside it, and
    /// connecting whatever host happens to sort first is not something
    /// the user asked for. A named target is unambiguous either way.
    fn search_named_connect(&mut self) -> Option<Task<Message>> {
        if self.active_view != View::Dashboard || self.host_search.trim().is_empty() {
            return None;
        }
        let input = self.host_search.trim().to_string();
        // Precedence: an exact saved-host match (label or its
        // canonical user@host form) always beats quick connect,
        // so typing something that names a saved host connects
        // the saved one, credentials included.
        let exact = self.connections.iter().position(|c| {
            c.label.eq_ignore_ascii_case(&input)
                || c.username.as_deref().is_some_and(|u| {
                    format!("{}@{}", u, c.hostname).eq_ignore_ascii_case(&input)
                })
        });
        if let Some(idx) = exact {
            return Some(self.update(Message::Ssh(SshMessage::ConnectSsh(idx))));
        }
        // Then the ad-hoc target (matches the toolbar's
        // "Enter to connect" hint chip).
        let conn = self.dashboard_quick_connect_target(&input)?;
        Some(self.update(Message::Ssh(SshMessage::QuickConnect(Box::new(
            crate::state::QuickConnectEntry::bare(conn),
        )))))
    }

    /// Enter: activate the selected item, or (Dashboard idle with a
    /// non-empty search) connect the top search result.
    fn keynav_activate(&mut self) -> Option<Task<Message>> {
        match self.keynav.focus {
            None => {
                if self.active_view == View::Dashboard && !self.host_search.is_empty() {
                    if let Some(task) = self.search_named_connect() {
                        return Some(task);
                    }
                    // Finally the top filtered result.
                    let first = self.keynav.content_rows.borrow().iter().flatten().copied().next();
                    if let Some(item) = first {
                        return Some(self.keynav_activate_content(item));
                    }
                }
                None
            }
            Some((FocusZone::SubNav, NavItem::SubNav(view))) => {
                self.panels.subnav_overflow = false;
                // ChangeView clears keynav focus on every other path
                // (mouse, hotkeys); this flag keeps the pill focused
                // so repeated Enter / arrows keep working.
                self.keynav.keep_focus_through_change_view = true;
                Some(self.update(Message::Navigation(NavigationMessage::ChangeView(view))))
            }
            Some((FocusZone::SubNav, NavItem::SettingsSection(section))) => {
                // Same keep-focus contract for the Settings sidebar.
                self.keynav.keep_focus_through_change_view = true;
                Some(self.update(Message::Settings(SettingsMessage::ChangeSettingsSection(section))))
            }
            Some((FocusZone::Toolbar, NavItem::Toolbar(item))) => {
                let msg = self.toolbar_item_message(item)?;
                Some(self.update(msg))
            }
            Some((FocusZone::Content, item)) => Some(self.keynav_activate_content(item)),
            // A zone/item mismatch can't be built by the router.
            Some(_) => None,
        }
    }

    /// Menu key / Shift+F10: open the selected content item's context
    /// menu (the same kebab the mouse reveals on hover). The overlay
    /// anchors at the last mouse position, the only anchor iced gives
    /// us without widget bounds; the menu itself is fully keyboard-
    /// navigable once open.
    /// Delete the ringed content item, via the same confirmation its
    /// context menu uses.
    ///
    /// Content zone only: the sub-nav, toolbar and settings rows have
    /// nothing to delete, and returning `None` there leaves the key free
    /// rather than silently doing nothing.
    fn keynav_delete(&mut self) -> Option<Task<Message>> {
        let (zone, item) = self.keynav.focus?;
        if zone != FocusZone::Content {
            return None;
        }
        let msg = match item {
            NavItem::Dash(DashNavItem::Host(i)) => {
                Message::Editor(EditorMessage::RequestDeleteConnection(i))
            }
            // Folders ask their own richer question (keep the hosts, or
            // delete them too), so they route to that dialog instead of
            // the generic confirm.
            NavItem::Dash(DashNavItem::Group(gid)) => {
                Message::Tabs(TabsMessage::StartDeleteFolder(gid))
            }
            NavItem::Dash(DashNavItem::SessionGroup(i)) => {
                Message::SessionGroup(SessionGroupMessage::RequestDeleteSessionGroup(i))
            }
            NavItem::Key(i) => Message::Keys(KeysMessage::RequestDeleteKey(i)),
            NavItem::Identity(i) => Message::Keys(KeysMessage::RequestDeleteIdentity(i)),
            NavItem::Snippet(i) => Message::Snippet(SnippetMessage::RequestDeleteSnippet(i)),
            NavItem::PortForward(i) => {
                Message::PortForward(PortForwardMessage::RequestDeletePortForwardRule(i))
            }
            NavItem::KnownHost(i) => {
                Message::KnownHost(KnownHostMessage::RequestDeleteKnownHost(i))
            }
            NavItem::HistoryLog(id) => {
                // The ring carries the log's id (stable across paging);
                // the delete message wants its index in the loaded list.
                let i = self.session_logs.iter().position(|l| l.id == id)?;
                Message::History(HistoryMessage::RequestDeleteSessionLog(i))
            }
            // Cloud accounts and proxy identities delete OUTRIGHT today,
            // with no confirmation anywhere: their menu row goes straight
            // to the vault. Binding a key to that would hand the user a
            // one-keystroke unrecoverable delete, so they get the same
            // confirm as everything else, which also closes the gap for
            // the mouse.
            NavItem::CloudAccount(id) => {
                let name = self
                    .cloud_profiles
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| p.label.clone())
                    .unwrap_or_default();
                self.confirm_remove(name, Message::Cloud(CloudMessage::DeleteCloudProfile(id)));
                return Some(Task::none());
            }
            NavItem::Proxy(id) => {
                let name = self
                    .proxy_identities
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| p.label.clone())
                    .unwrap_or_default();
                self.confirm_remove(
                    name,
                    Message::ProxyIdentity(ProxyIdentityMessage::DeleteProxyIdentity(id)),
                );
                return Some(Task::none());
            }
            // Nothing to delete: snippet folders are derived from tags,
            // and the rest are navigation or generic action rows.
            NavItem::SnippetGroup(_)
            | NavItem::ContentAction(_)
            | NavItem::SubNav(_)
            | NavItem::Toolbar(_)
            | NavItem::SettingsSection(_)
            | NavItem::SettingsRow(_) => return None,
        };
        Some(self.update(msg))
    }

    fn keynav_open_context_menu(&mut self) -> Option<Task<Message>> {
        let (zone, item) = self.keynav.focus?;
        if zone != FocusZone::Content {
            return None;
        }
        // Keyboard-opened menu: arm the modality gate so the default
        // row shows its ring right away (a mouse-opened kebab starts
        // ringless until the first arrow key, focus-visible).
        self.keynav.modal.kbd.set(true);
        // Anchor the menu at the ringed card's kebab corner (trailing
        // edge, vertical center), reported by `keynav_ring_content`;
        // zero-width means no ring was drawn yet, fall back to the
        // handler's mouse anchor.
        let rect = self.keynav.ring_bounds.get();
        if rect.width > 0.0 {
            let x = if crate::i18n::is_rtl_layout() {
                rect.x
            } else {
                rect.x + rect.width
            };
            self.keynav.menu_anchor = Some((x, rect.y + rect.height / 2.0));
        }
        let msg = match item {
            NavItem::Dash(DashNavItem::Host(i)) => Message::Tabs(TabsMessage::ShowCardMenu(i)),
            NavItem::Dash(DashNavItem::Group(gid)) => {
                // Dynamic (cloud-query) folders carry their own menu.
                let dynamic = self
                    .groups
                    .iter()
                    .any(|g| g.id == gid && g.cloud_query.is_some());
                if dynamic {
                    Message::Cloud(CloudMessage::ShowDynamicGroupCardMenu(gid))
                } else {
                    Message::Tabs(TabsMessage::ShowFolderActions(gid))
                }
            }
            NavItem::Dash(DashNavItem::SessionGroup(i)) => Message::SessionGroup(SessionGroupMessage::ShowSessionGroupMenu(i)),
            NavItem::Key(i) => Message::Keys(KeysMessage::ShowKeyMenu(i)),
            NavItem::Identity(i) => Message::Keys(KeysMessage::ShowIdentityMenu(i)),
            NavItem::Snippet(i) => Message::Snippet(SnippetMessage::ShowSnippetMenu(i)),
            NavItem::CloudAccount(id) => Message::Cloud(CloudMessage::ShowCloudCardMenu(id)),
            // Session-log rows carry a kebab menu (Export .cast /
            // transcript / commands / Delete). The row is keyed by uuid;
            // the menu is keyed by the index into `session_logs`, so map
            // one to the other (the same index the view and the menu use).
            NavItem::HistoryLog(id) => {
                let idx = self.session_logs.iter().position(|e| e.id == id)?;
                Message::History(HistoryMessage::ShowSessionLogMenu(idx))
            }
            // Rows without a context menu (proxies, known hosts, port
            // forwards, settings): nothing to open.
            _ => return None,
        };
        Some(self.update(msg))
    }

    /// Esc: clear the zone focus back to idle (first press); when
    /// already idle it is not consumed, so the hotkey table keeps
    /// its close-topmost-modal behavior.
    fn keynav_escape(&mut self) -> Option<Task<Message>> {
        if self.keynav.focus.is_some() {
            self.keynav.focus = None;
            self.panels.subnav_overflow = false;
            return Some(Task::none());
        }
        None
    }

    /// Primary action of a content item, matching what a mouse click
    /// on the card/row does (see the plan table: Known Hosts' only
    /// row action is delete, which is confirm-gated; Cloud cards have
    /// no click action so Enter opens the edit form).
    fn keynav_activate_content(&mut self, item: NavItem) -> Task<Message> {
        // Settings rows carry their own recorded action: buttons and
        // toggles dispatch it and KEEP the selection (repeat toggling
        // works); input rows hand the keyboard to the real text input
        // (focus + clear, the search-zone invariant); pure picker rows
        // consume Enter as a no-op (Left/Right are their verbs).
        if let NavItem::SettingsRow(idx) = item {
            let action = self.keynav.settings_row_actions.borrow().get(idx).cloned();
            let Some(a) = action else {
                return Task::none();
            };
            if let Some(id) = a.focus {
                self.keynav.focus = None;
                return crate::widgets::focus_input(id);
            }
            if let Some(msg) = a.activate {
                return self.update(msg);
            }
            return Task::none();
        }
        let msg = match item {
            // Tree view mode: Enter folds/unfolds a MANUAL folder in
            // place (there is no drill-down to open); dynamic groups
            // keep the drill into their cloud screen.
            NavItem::Dash(DashNavItem::Group(gid))
                if self.prefs.host_view_mode == crate::state::HostViewMode::Tree
                    && self.groups.iter().any(|g| g.id == gid && g.cloud_query.is_none()) =>
            {
                Message::Ai(crate::app::AiMessage::HostsTreeToggleGroup(gid))
            }
            NavItem::Dash(DashNavItem::Group(gid)) => Message::Navigation(NavigationMessage::OpenGroup(gid)),
            NavItem::Dash(DashNavItem::SessionGroup(i)) => Message::SessionGroup(SessionGroupMessage::OpenSessionGroup(i)),
            NavItem::Dash(DashNavItem::Host(i)) => Message::Ssh(SshMessage::ConnectSsh(i)),
            NavItem::Key(i) => Message::Keys(KeysMessage::EditKey(i)),
            NavItem::Identity(i) => Message::Keys(KeysMessage::EditIdentity(i)),
            NavItem::Snippet(i) => Message::Snippet(SnippetMessage::RunSnippet(i)),
            NavItem::SnippetGroup(i) => {
                let Some(name) = self.snippet_group_names().get(i).cloned() else {
                    return Task::none();
                };
                Message::Snippet(SnippetMessage::OpenSnippetGroup(name))
            }
            NavItem::PortForward(i) => Message::PortForward(PortForwardMessage::EditPortForwardRule(i)),
            NavItem::HistoryLog(id) => Message::History(HistoryMessage::ViewSessionLog(id)),
            NavItem::CloudAccount(id) => Message::Cloud(CloudMessage::ShowCloudForm(Some(id))),
            NavItem::Proxy(id) => Message::ProxyIdentity(ProxyIdentityMessage::ShowProxyIdentityForm(Some(id))),
            NavItem::KnownHost(i) => Message::KnownHost(KnownHostMessage::RequestDeleteKnownHost(i)),
            NavItem::ContentAction(i) => {
                // Generic action rows (dynamic cloud-group task list,
                // the empty dashboard's create/import block) carry
                // their own recorded `RowAction`. Same verbs as the
                // Settings rows above: an input row hands the keyboard
                // to the real text input, everything else dispatches.
                let action = self.keynav.content_actions.borrow().get(i).cloned();
                let Some(a) = action else {
                    return Task::none();
                };
                if let Some(id) = a.focus {
                    self.keynav.focus = None;
                    return crate::widgets::focus_input(id);
                }
                let Some(msg) = a.activate else {
                    return Task::none();
                };
                self.keynav.focus = None;
                return self.update(msg);
            }
            NavItem::SubNav(_)
            | NavItem::Toolbar(_)
            | NavItem::SettingsSection(_)
            | NavItem::SettingsRow(_) => return Task::none(),
        };
        // Activation clears the selection, matching the old dashboard
        // behavior: the action changes the surface underneath it.
        self.keynav.focus = None;
        self.update(msg)
    }

    /// Message behind each toolbar item on the current view. The
    /// items are recorded at the build site, so this table only ever
    /// sees combinations that were actually rendered.
    fn toolbar_item_message(&self, item: ToolbarItem) -> Option<Message> {
        use crate::state::SortMenuKind;
        Some(match (self.active_view, item) {
            (View::Dashboard, ToolbarItem::ViewToggle) => Message::Settings(SettingsMessage::CycleHostViewMode),
            // Monitoring toolbar (issue #95): shared tag filter, plus
            // its own grid/list toggle.
            (View::Monitoring, ToolbarItem::TagFilter) => Message::Navigation(NavigationMessage::ShowHostTagFilterMenu),
            (View::Monitoring, ToolbarItem::ViewToggle) => Message::Monitor(crate::app::MonitorMessage::DashToggleListView),
            (View::Monitoring, ToolbarItem::MonitorPause) => Message::Monitor(crate::app::MonitorMessage::DashTogglePause),
            (View::Monitoring, ToolbarItem::MonitorRefresh) => Message::Monitor(crate::app::MonitorMessage::DashRefreshNow),
            (View::Dashboard, ToolbarItem::TagFilter) => Message::Navigation(NavigationMessage::ShowHostTagFilterMenu),
            (View::Snippets, ToolbarItem::TagFilter) => Message::Snippet(SnippetMessage::ShowSnippetTagFilterMenu),
            (View::History, ToolbarItem::TagFilter) => Message::History(HistoryMessage::ShowHistoryTagFilterMenu),
            (View::History, ToolbarItem::SearchContent) => Message::History(HistoryMessage::SearchContentToggled),
            (View::Dashboard, ToolbarItem::Sort) => Message::Navigation(NavigationMessage::ToggleSortMenu(SortMenuKind::Hosts)),
            (View::Dashboard, ToolbarItem::Primary) => Message::Editor(EditorMessage::ShowNewConnection),
            (View::Dashboard, ToolbarItem::PrimaryChevron) => Message::Cloud(CloudMessage::ShowCloudProviderPicker),
            (View::Dashboard, ToolbarItem::CloudDiscover(pid)) => Message::Cloud(CloudMessage::ShowCloudDiscover(pid)),
            (View::Keys, ToolbarItem::Sort) => Message::Navigation(NavigationMessage::ToggleSortMenu(SortMenuKind::Keys)),
            (View::Keys, ToolbarItem::Primary) => Message::Keys(KeysMessage::ToggleKeychainAddMenu),
            (View::Snippets, ToolbarItem::Sort) => Message::Navigation(NavigationMessage::ToggleSortMenu(SortMenuKind::Snippets)),
            (View::Snippets, ToolbarItem::Primary) => Message::Snippet(SnippetMessage::ShowSnippetPanel),
            (View::PortForwarding, ToolbarItem::Primary) => Message::PortForward(PortForwardMessage::ShowPortForwardPanel),
            (View::History, ToolbarItem::Primary) => Message::History(HistoryMessage::RequestClearHistory),
            (View::History, ToolbarItem::PagerPrev) => Message::History(HistoryMessage::LogsPagePrev),
            (View::History, ToolbarItem::PagerNext) => Message::History(HistoryMessage::LogsPageNext),
            (View::Cloud, ToolbarItem::Primary) => Message::Cloud(CloudMessage::ShowCloudForm(None)),
            (View::Proxies, ToolbarItem::Primary) => Message::ProxyIdentity(ProxyIdentityMessage::ShowProxyIdentityForm(None)),
            (View::KnownHosts, ToolbarItem::Primary) => Message::KnownHost(KnownHostMessage::RequestClearAllKnownHosts),
            (_, ToolbarItem::PrivacyReveal) => Message::TogglePrivacyReveal,
            (_, ToolbarItem::Overflow) => Message::Navigation(NavigationMessage::ToggleToolbarOverflow),
            (_, ToolbarItem::SearchIcon) => Message::Navigation(NavigationMessage::ToggleToolbarSearch),
            _ => return None,
        })
    }

    /// Whether the content zone moves linearly (single-column rows).
    /// The dashboard follows its grid/list toggle; the card grids
    /// (keychain, snippets, port forwards, cloud) are 2-D; History,
    /// Proxies and Known Hosts are true 1-D lists.
    fn content_list_mode(&self) -> bool {
        match self.active_view {
            // List AND tree are one item per row; only the card grid
            // is 2-D.
            View::Dashboard => {
                self.prefs.host_view_mode != crate::state::HostViewMode::Grid
            }
            View::Keys | View::Snippets | View::Cloud | View::PortForwarding => false,
            // Settings rows and the remaining vault views are
            // single-column lists.
            _ => true,
        }
    }

    /// Scrollable id + estimated row height for the active view's
    /// content list, feeding the keep-in-view snap math. iced exposes
    /// no item bounds, so these are estimates tuned to the card
    /// metrics (the dashboard numbers are the pre-existing ones).
    fn content_scroll_meta(&self) -> Option<(iced::widget::Id, f32)> {
        let (id, row_h) = match self.active_view {
            View::Dashboard => (
                "dashboard-grid-scroll",
                match self.prefs.host_view_mode {
                    crate::state::HostViewMode::Grid => 60.0,
                    crate::state::HostViewMode::List => 56.0,
                    // Dense rows: 26 px content + 2x4 padding + list
                    // spacing. The list-mode 56 overshot the snap by
                    // half a screen on long trees.
                    crate::state::HostViewMode::Tree => 36.0,
                },
            ),
            View::Keys => ("keys-grid-scroll", 60.0),
            View::Snippets => ("snippets-grid-scroll", 60.0),
            View::PortForwarding => ("port-forwards-scroll", 64.0),
            View::History => ("history-list-scroll", 52.0),
            View::Cloud => ("cloud-accounts-scroll", 60.0),
            View::Proxies => ("proxies-list-scroll", 56.0),
            View::KnownHosts => ("known-hosts-scroll", 48.0),
            View::Settings => (self.settings_section.scroll_id(), 52.0),
            // Result cards vary in height with their line count; 72 is
            // the short ones (a DNS record type), which is the size that
            // decides when scrolling has to start.
            View::NetworkTools => ("net-tools-scroll", 72.0),
            // Table rows ~34 px, cards ~150: the walk is row-per-item
            // either way, so the height only tunes when scrolling
            // starts.
            View::Monitoring => (
                "monitor-dash-scroll",
                if self.prefs.monitor_dash_list_view { 34.0 } else { 150.0 },
            ),
            _ => return None,
        };
        Some((iced::widget::Id::new(id), row_h))
    }

    /// Scroll only enough to keep the selected row in view: rows that
    /// already fit on the first screen don't scroll; later rows
    /// scroll so the selected one sits at the bottom edge. Ported
    /// verbatim from the old dashboard-only handler.
    fn keynav_scroll_content_to(&self, item: NavItem) -> Task<Message> {
        let Some((scroll_id, row_h)) = self.content_scroll_meta() else {
            return Task::none();
        };
        let rows = self.keynav.content_rows.borrow();
        let sel_row = rows.iter().position(|row| row.contains(&item)).unwrap_or(0) as f32;
        let viewport = (self.window_size.height - 115.0).max(row_h);
        let visible_rows = (viewport / row_h).floor().max(1.0);
        let max_scroll_rows = (rows.len() as f32 - visible_rows).max(1.0);
        let offset_rows = (sel_row - visible_rows + 1.0).max(0.0);
        let y = (offset_rows / max_scroll_rows).clamp(0.0, 1.0);
        iced::widget::operation::snap_to(
            scroll_id,
            iced::widget::operation::RelativeOffset { x: None, y: Some(y) },
        )
    }

    /// Settings content, ring idle: hand a Tab / movement key to
    /// [`Self::settings_key_resolved`] once iced reports what it
    /// actually has focused (resolved via `find_focused`). Returns
    /// `Some(task)` when the Settings-keeps-its-keys rule applies (a
    /// mouse-focused input row walks the recorded rows on Tab and keeps
    /// its own caret on arrows); `None` leaves the normal vault-area
    /// handling to run. Called from the router's Tab and movement arms,
    /// whose surrounding gates (`side_panel_open`, modals, the Security
    /// password forms, modifiers) have already run.
    fn settings_ring_idle_resolve(
        &self,
        named: keyboard::key::Named,
        shift: bool,
    ) -> Option<Task<Message>> {
        if !(self.active_tab.is_none()
            && self.active_view == View::Settings
            && self.keynav.focus.is_none())
        {
            return None;
        }
        Some(iced::widget::operation::find_focused().map(move |focused| {
            Message::Navigation(NavigationMessage::SettingsKeyResolved {
                named,
                shift,
                focused,
            })
        }))
    }

    /// Vault-area ring idle: hand an Up/Down press to
    /// [`Self::vault_nav_key_resolved`] once iced reports what it
    /// actually has focused (resolved via `find_focused`). Only Up and
    /// Down need the resolution: they are the only keys
    /// [`Self::keynav_move`] claims from idle, so everything else keeps
    /// falling through to the hotkey table untouched. Settings never
    /// reaches here (`settings_ring_idle_resolve` claims its keys
    /// first).
    fn vault_ring_idle_resolve(&self, named: keyboard::key::Named) -> Option<Task<Message>> {
        use keyboard::key::Named as N;
        if self.keynav.focus.is_some() || !matches!(named, N::ArrowUp | N::ArrowDown) {
            return None;
        }
        Some(iced::widget::operation::find_focused().map(move |focused| {
            Message::Navigation(NavigationMessage::VaultNavKeyResolved { named, focused })
        }))
    }

    /// Continuation of a vault-area Up/Down press from ring idle once
    /// iced has reported which widget is actually focused. The view's
    /// search field keeps the deliberate enter-the-content-zone
    /// behavior (type a filter, Down picks a result); any OTHER
    /// focused text input owns the key for its own caret — the empty
    /// dashboard's quick-host field was being blurred mid-typing by
    /// numpad arrows (NumLock off delivers Up/Down for 8/2, issue
    /// #168), which scrolled the list and silently dropped every
    /// keystroke after it.
    pub(crate) fn vault_nav_key_resolved(
        &mut self,
        named: keyboard::key::Named,
        focused: Option<iced::widget::Id>,
    ) -> Task<Message> {
        if let Some(id) = focused
            && self.active_view_search_id() != Some(id)
        {
            return Task::none();
        }
        self.keynav_move(named).unwrap_or(Task::none())
    }

    /// Continuation of a Settings-content Tab / arrow press once iced
    /// has reported which widget is actually focused (the gate in
    /// `handle_keynav_key` resolves it via `find_focused`). A
    /// mouse-focused input row owns the walk: Tab / Shift+Tab step
    /// through the recorded rows from that field (the panel contract),
    /// arrows / Home / End stay with the field's own caret. Anything
    /// else gets the key the vault-area router would have given it:
    /// Tab cycles the zones, arrows enter the content zone.
    pub(crate) fn settings_key_resolved(
        &mut self,
        named: keyboard::key::Named,
        shift: bool,
        focused: Option<iced::widget::Id>,
    ) -> Task<Message> {
        use keyboard::key::Named as N;
        let row = focused.and_then(|id| {
            self.keynav
                .settings_row_actions
                .borrow()
                .iter()
                .position(|a| a.focus.as_ref() == Some(&id))
        });
        match (named, row) {
            (N::Tab, Some(from)) => self.settings_nav_tab(!shift, from),
            (N::Tab, None) => self.keynav_cycle_zones(!shift),
            // A focused input row: the field's own caret / selection
            // handling owns every non-Tab key (the panel contract).
            (_, Some(_)) => Task::none(),
            _ => self.settings_key_unclaimed(named),
        }
    }

    /// Tab / Shift+Tab over the recorded Settings rows, entering at the
    /// row whose input iced currently has focused (`from`). Input rows
    /// receive real iced focus; the rest get the ring and iced focus is
    /// blurred — the panel contract, applied to the Settings content so
    /// a mouse-focused field (the export / import password, the sync
    /// passphrase) Tabs onward instead of the vault-area router parking
    /// the ring on the first content row and scrolling the page away.
    fn settings_nav_tab(&mut self, forward: bool, from: usize) -> Task<Message> {
        let len = self.keynav.settings_row_actions.borrow().len();
        let Some(next) = index_move(len, Some(from), forward) else {
            return Task::none();
        };
        let action = self.keynav.settings_row_actions.borrow().get(next).cloned();
        let Some(action) = action else {
            return Task::none();
        };
        // The search-zone invariant (`keynav_activate_content`): a
        // focused input row means ring IDLE, so the field owns every
        // non-Tab key and the next Tab re-enters through the
        // `find_focused` resolution. Ringing the row on top of the
        // focus would hand arrows back to `keynav_move` (the ring
        // walks away and scrolls mid-typing, the exact symptom this
        // fix removes) and Space / Enter to `keynav_activate`.
        let step = match action.focus {
            Some(id) => {
                self.keynav.focus = None;
                crate::widgets::focus_input(id)
            }
            None => {
                self.keynav.focus = Some((FocusZone::Content, NavItem::SettingsRow(next)));
                blur_task()
            }
        };
        Task::batch([step, self.keynav_scroll_content_to(NavItem::SettingsRow(next))])
    }

    /// The vault-area handling a Settings-content key would have had
    /// when no mouse-focused input row is in play: arrows / Home / End
    /// move within the content zone from idle (Tab is handled inline by
    /// `settings_key_resolved`). Mirrors the arms of `handle_keynav_key`
    /// the resolution in `settings_ring_idle_resolve` intercepted.
    fn settings_key_unclaimed(&mut self, named: keyboard::key::Named) -> Task<Message> {
        use keyboard::key::Named as N;
        match named {
            N::ArrowUp
            | N::ArrowDown
            | N::ArrowLeft
            | N::ArrowRight
            | N::Home
            | N::End => self.keynav_move(named).unwrap_or(Task::none()),
            _ => Task::none(),
        }
    }
}
