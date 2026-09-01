//! `Oryxis::handle_navigation`: settings-panel-independent dispatch arms for the
//! navigation area, split out of dispatch.rs. Returns `Err(message)` for anything
//! it doesn't claim so the try_handler! chain falls through.
#![allow(clippy::result_large_err)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::too_many_lines)]

use iced::Task;

use crate::app::{CloudMessage, NavigationMessage, Message, Oryxis};
use crate::state::View;

impl Oryxis {
    pub(crate) fn handle_navigation(
        &mut self,
        message: NavigationMessage,
    ) -> Task<Message> {
        match message {
            NavigationMessage::ModalNavHover(idx) => {
                // Pointer hover converges the modal-layer SELECTION
                // with the mouse (Enter activates the hovered row,
                // arrows continue from it), tagged with the live
                // surface so a stale hover from a closing menu is
                // inert. It does NOT show the ring: `modal.kbd` goes
                // false, so the hover-driven selection stays invisible
                // until the keyboard takes over (focus-visible).
                if let Some((surface, _)) = self.modal_nav_surface() {
                    self.keynav.modal.selected = Some((surface, idx));
                    self.keynav.modal.kbd.set(false);
                }
            }
            NavigationMessage::PickOpenChanged(open) => {
                self.keynav.pick_open = open;
            }
            NavigationMessage::PanelNavTabResolved { forward, focused } => {
                return self.panel_nav_tab_resolved(forward, focused);
            }
            NavigationMessage::SettingsKeyResolved {
                named,
                shift,
                focused,
            } => {
                return self.settings_key_resolved(named, shift, focused);
            }
            NavigationMessage::VaultNavKeyResolved { named, focused } => {
                return self.vault_nav_key_resolved(named, focused);
            }
            // -- Navigation --
            NavigationMessage::GoHome => {
                // The Home tab lands on the vault surface the user left,
                // folder included. It goes THROUGH `ChangeView` rather
                // than setting `active_view` itself: leaving a view runs
                // a pile of teardown (Monitoring's idle TTL, the keynav
                // item lists, open overlays, the Shortcuts capture) that
                // a second copy here would drift from silently. Then
                // re-open the group `ChangeView` just reset to root.
                let group = self.active_group;
                let leave = self.handle_navigation(
                    NavigationMessage::ChangeView(View::Dashboard),
                );
                let Some(gid) = group else {
                    return leave;
                };
                let enter = self.handle_navigation(
                    NavigationMessage::OpenGroup(gid),
                );
                return Task::batch([leave, enter]);
            }
            NavigationMessage::ChangeView(view) => {
                // A gated surface is not reachable while its feature is
                // off, whatever asked. The menu entry is already hidden;
                // this is the guard for everything else that can send a
                // `ChangeView` (a restored route, a hotkey, a deep link
                // added later).
                if view == View::NetworkTools && !self.prefs.network_tools {
                    return Task::none();
                }
                // Navigating away from the Shortcuts editor cancels
                // any pending capture so the next keystroke doesn't
                // silently rebind an action from another screen.
                self.editing_hotkey = None;
                // A dropdown can't survive its view: drop the pick-open
                // key guard in case the widget unmounted while open and
                // never got to publish on_close.
                self.keynav.pick_open = false;
                // Leaving the Logs view re-arms Privacy Mode masking so a
                // revealed timeline doesn't stay exposed on the next visit.
                self.privacy.revealed = false;
                // The Settings sidebar search is scoped to the visit:
                // coming back later starts from the plain section list,
                // not a stale filtered rail.
                if view != View::Settings {
                    self.settings_search.clear();
                }
                // Monitor dashboard lifecycle (issue #95): leaving the
                // view arms the idle TTL on the dialed connections;
                // entering it (re-)establishes every link right away.
                // Captured before `active_view` moves.
                let mut dash_tasks: Vec<Task<Message>> = Vec::new();
                if self.active_view == View::Monitoring && view != View::Monitoring {
                    dash_tasks.push(self.dash_leave());
                }
                self.active_view = view;
                self.active_tab = None;
                // Give a panel surface its strip entry (issue #120).
                // Every door into one goes through here, which is what
                // keeps it to one tab per kind; and it has to happen
                // BEFORE the early returns further down (the
                // search-focus one fires for Settings, so a call at the
                // end never ran).
                if let Some(kind) = crate::state::PanelKind::for_view(view) {
                    self.ensure_panel_tab(kind);
                }
                // Drop any keyboard selection when leaving / changing the
                // surface so a stale highlight doesn't linger. Keynav's
                // own dispatches (SubNav Enter, the section-cycle hotkey)
                // set the keep flag so the pill highlight survives the
                // switch and repeated arrows / Enter keep working.
                let keep_keynav = self.keynav.keep_focus_through_change_view;
                self.keynav.keep_focus_through_change_view = false;
                if !keep_keynav {
                    self.keynav.focus = None;
                }
                // The toolbar / content / sub-nav item lists belong to
                // the view being left; the target view re-records them
                // on its next render. Clear now so the router can never
                // move across another view's stale items in between
                // (the sub-nav list swaps between vault pills and the
                // Settings sections sidebar).
                self.keynav.toolbar_items.borrow_mut().clear();
                self.keynav.subnav_items.borrow_mut().clear();
                self.keynav_clear_content();
                self.keynav.settings_row_actions.borrow_mut().clear();
                // Navigating to the host list (Hosts pill, its burger
                // entry, Ctrl+Shift+1) returns to the root, not whichever
                // group was last open, and stays the one-click way back
                // out of a nested folder. The Home tab is the opposite
                // door: `GoHome` re-opens the folder right after this.
                if view == View::Dashboard {
                    self.active_group = None;
                }
                // Same rule for the Snippets pill: land at the root,
                // not inside whichever snippet group was last open.
                if view == View::Snippets {
                    self.active_snippet_group = None;
                }
                // Burger menu auto-dismisses on navigation: the user
                // picked a destination, leaving the overlay open is
                // visual noise.
                self.panels.burger_menu = false;
                self.panels.subnav_overflow = false;
                // Any floating overlay menu (kebab, sort, the
                // multi-select tag filters, which STAY open by design)
                // dies with its view: a stale `overlay` keeps the modal
                // keyboard router alive, which silently eats
                // Enter/arrows on the next surface (live QA: the
                // terminal 'stopped accepting commands' after
                // navigating away from an open tag dropdown).
                self.overlay = None;
                // Lazy-load the local SFTP pane when the user first lands
                // on the view (or returns to it after the underlying dir
                // changed). Cheap enough to redo unconditionally.
                if view == View::Sftp {
                    // Back the SFTP surface with a tab entry (adopts the
                    // existing top-level `self.sftp` as the first tab). The
                    // single-tab case behaves exactly as before.
                    self.ensure_sftp_tab();
                    // A hybrid terminal tab may own the live buffer; the
                    // standalone surface must never render (or start
                    // transfers against) another tab's Files state. Park
                    // it and hoist a standalone tab in its place.
                    self.park_hybrid_sftp();
                    if self.active_sftp.is_none() && !self.sftp_tabs.is_empty() {
                        self.focus_sftp_tab(0);
                    }
                    // Refresh whichever pane(s) are Local; remote panes
                    // ignore this (refresh_sftp_local early-returns).
                    self.refresh_sftp_local(crate::state::SftpPaneSide::Left);
                    self.refresh_sftp_local(crate::state::SftpPaneSide::Right);
                }
                if view == View::Monitoring {
                    dash_tasks.push(self.dash_enter());
                }
                // Entering Logs re-reads the timeline from the vault:
                // rows created since boot (a session that just started
                // recording, fresh connection events) only exist in the
                // tables, not in the cached page state.
                if view == View::History
                    && let Some(vault) = &self.vault
                {
                    self.logs_total = vault.count_logs().unwrap_or(0);
                    self.logs = vault
                        .list_logs_page(self.logs_page * 50, 50)
                        .unwrap_or_default();
                    self.session_logs_total = vault.count_session_logs().unwrap_or(0);
                    self.session_logs = vault
                        .list_session_logs_page(self.session_logs_page * 50, 50)
                        .unwrap_or_default();
                }
                // Land on the view with its search field focused so the
                // user can start typing immediately (same ids as Ctrl+F).
                // Not when keynav drove the switch: the user is walking
                // the pills, stealing focus back to the search would
                // fight the roving highlight.
                // Opening Settings directly on the (default) Interface
                // section never goes through ChangeSettingsSection, so
                // fetch the renderer readout here too, and put the
                // section back where it was left (issue #120). Built
                // BEFORE the search-focus return below and batched with
                // it: Settings has a search id, so an early return there
                // would shadow both of these every time.
                let settings_tasks: Vec<Task<Message>> = if view == View::Settings {
                    let mut tasks = vec![self.renderer_info_task(), self.settings_restore_scroll()];
                    // Landing straight on the remembered Sync section
                    // (issue #120) also skips ChangeSettingsSection, so
                    // the git card's availability re-probe fires here
                    // too; "install it and reopen this screen" must
                    // stay true for this door as well.
                    if self.settings_section == crate::state::SettingsSection::Sync
                        && self.sync.transport == "git"
                    {
                        tasks.push(crate::dispatch_git_sync::git_availability_task());
                    }
                    tasks
                } else {
                    Vec::new()
                };
                if !keep_keynav && let Some(id) = self.active_view_search_id() {
                    let mut tasks = vec![crate::widgets::focus_input(id)];
                    tasks.extend(settings_tasks);
                    tasks.extend(dash_tasks);
                    return Task::batch(tasks);
                }
                if !settings_tasks.is_empty() || !dash_tasks.is_empty() {
                    let mut tasks = settings_tasks;
                    tasks.extend(dash_tasks);
                    return Task::batch(tasks);
                }
            }
            NavigationMessage::QuickHostInput(v) => {
                self.quick_host_input = v;
            }
            NavigationMessage::OpenGroup(gid) => {
                self.active_group = Some(gid);
                self.host_search.clear();
                // Auto-trigger resolve when the user opens a dynamic
                // group, saves an extra click. Re-resolve when there's
                // no cache yet, or when the cached list has gone stale
                // (older than the TTL): cloud resources like ECS tasks
                // recycle, and a stale list means clicking a dead task
                // fails until a manual Refresh. A still-`Loading` or
                // `Failed` cache is left alone (don't restart in-flight
                // resolves; let the user retry a failure explicitly).
                if self.dynamic_group_needs_resolve(gid) {
                    return self.handle_cloud(CloudMessage::DynamicGroupResolve(gid));
                }
            }
            NavigationMessage::HostSearchChanged(v) => {
                // An emptied box ends the ad-hoc target it described, so
                // the protocol badge goes back to SSH: a Telnet pick
                // made for one switch must not silently follow whatever
                // host is typed next.
                if v.trim().is_empty() {
                    self.quick_connect_protocol =
                        oryxis_core::models::connection::ConnectionProtocol::Ssh;
                }
                self.host_search = v;
                // The filtered set just changed; drop the keyboard
                // selection so it can't point at a now-hidden host. Enter
                // still connects the top result while a search is active.
                self.keynav.focus = None;
            }
            NavigationMessage::HostFilterByCloudProfile(maybe_pid) => {
                self.host_filter_cloud_profile = maybe_pid;
                // Filter changed the visible set; drop the keyboard
                // selection so Enter can't connect a now-hidden host.
                self.keynav.focus = None;
            }
            // (helpers for the tag filter live below the handler impl)
            NavigationMessage::ShowHostTagFilterMenu => {
                use crate::state::{OverlayContent, OverlayState};
                let already_open = matches!(
                    self.overlay.as_ref().map(|o| &o.content),
                    Some(OverlayContent::HostTagFilter)
                );
                if already_open {
                    self.overlay = None;
                } else {
                    // Anchor under the tag-filter button (its bounds are
                    // reported every draw by a `bounds_reporter`), matching
                    // the "+ Host" split menu rather than dropping at the
                    // cursor. The render treats `x` as the leading edge
                    // (right edge under RTL, where it subtracts the menu
                    // width), so hand it the button's leading edge. Falls
                    // back to the cursor before the first draw populates
                    // the cell.
                    let b = self.host_tag_filter_btn_bounds.get();
                    let (x, y) = if b.width > 0.0 {
                        let lead = if crate::i18n::is_rtl_layout() {
                            b.x + b.width
                        } else {
                            b.x
                        };
                        (lead, b.y + b.height + 6.0)
                    } else {
                        (self.mouse_position.x, self.mouse_position.y + 26.0)
                    };
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::HostTagFilter,
                        x,
                        y,
                    });
                }
            }
            NavigationMessage::ToggleHostTagFilterTag(tag) => {
                // Multi-select: the dropdown stays open so several tags
                // can be picked in one visit; the backdrop closes it.
                match self
                    .host_filter_tags
                    .iter()
                    .position(|t| t.eq_ignore_ascii_case(&tag))
                {
                    Some(i) => {
                        self.host_filter_tags.remove(i);
                    }
                    None => self.host_filter_tags.push(tag),
                }
                // Same reasoning as the cloud-profile filter above.
                self.keynav.focus = None;
            }
            NavigationMessage::ClearHostTagFilter => {
                self.host_filter_tags.clear();
                self.overlay = None;
                self.keynav.focus = None;
            }
            NavigationMessage::ToggleGroupPicker(target) => {
                use crate::state::{GroupPickerTarget, OverlayContent, OverlayState};
                let already_open = matches!(
                    self.overlay.as_ref().map(|o| &o.content),
                    Some(OverlayContent::GroupPicker(t)) if *t == target
                );
                if already_open {
                    self.overlay = None;
                } else {
                    let bounds = match target {
                        GroupPickerTarget::DynamicFormParent => {
                            self.dynamic_form_parent_combo_bounds.get()
                        }
                        GroupPickerTarget::SessionGroupFolder => {
                            self.session_group_folder_combo_bounds.get()
                        }
                        GroupPickerTarget::GroupEditParent => {
                            self.group_edit_parent_combo_bounds.get()
                        }
                    };
                    self.group_picker_search.clear();
                    // 6 px gap below the combo. Falls back to mouse
                    // coords if the cell hasn't been populated yet
                    // (first ever open before any draw pass).
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::GroupPicker(target),
                        x: if bounds.width > 0.0 {
                            bounds.x
                        } else {
                            self.mouse_position.x
                        },
                        y: if bounds.height > 0.0 {
                            bounds.y + bounds.height + 6.0
                        } else {
                            self.mouse_position.y + 26.0
                        },
                    });
                }
            }
            NavigationMessage::GroupPickerSearchChanged(v) => {
                self.group_picker_search = v;
            }
            NavigationMessage::GroupPickerPick(target, label) => {
                use crate::state::{GroupPickerTarget, OverlayContent};
                match target {
                    GroupPickerTarget::DynamicFormParent => {
                        self.cloud_dynamic_form.parent_label = label;
                    }
                    GroupPickerTarget::SessionGroupFolder => {
                        self.editor_session_group.group_name = label;
                    }
                    GroupPickerTarget::GroupEditParent => {
                        self.group_edit.parent_label = label;
                    }
                }
                if matches!(
                    self.overlay.as_ref().map(|o| &o.content),
                    Some(OverlayContent::GroupPicker(_))
                ) {
                    self.overlay = None;
                }
            }
            NavigationMessage::ToggleSortMenu(kind) => {
                use crate::state::{OverlayContent, OverlayState, SortMenuKind};
                let already_open = matches!(
                    self.overlay.as_ref().map(|o| &o.content),
                    Some(OverlayContent::SortMenu(k)) if *k == kind
                );
                if already_open {
                    self.overlay = None;
                } else {
                    // Anchor the dropdown to the trailing edge of the
                    // toolbar, just under the button row, matching the
                    // keychain "+ ADD" menu geometry. Pre-compensate for
                    // the panel-on-the-right footprint per view so the
                    // menu's right edge always lands at the visible
                    // content's right edge.
                    let panel_width = match kind {
                        SortMenuKind::Hosts => {
                            if self.panels.host_panel { self.panel_width } else { 0.0 }
                        }
                        SortMenuKind::Keys => {
                            if self.panels.key_panel
                                || self.panels.identity_panel
                                || self.panels.key_generate_panel
                            {
                                self.panel_width
                            } else {
                                0.0
                            }
                        }
                        SortMenuKind::Snippets => {
                            if self.panels.snippet_panel { self.panel_width } else { 0.0 }
                        }
                    };
                    // Must match the `OverlayContent::SortMenu` width in
                    // `overlay_menu_width`. Anchored on the sort button's
                    // real drawn bounds (2 px gap, trailing edges
                    // aligned); the panel width only feeds the fallback.
                    let menu_width = 220.0_f32;
                    let (x, y) = self.toolbar_menu_anchor(
                        &self.toolbar_sort_btn_bounds,
                        menu_width,
                        panel_width,
                    );
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::SortMenu(kind),
                        x,
                        y,
                    });
                }
            }
            NavigationMessage::SetListSort(kind, sort) => {
                use crate::state::SortMenuKind;
                // Selecting from the sidebar's own sort popover dismisses it
                // (harmless for the workspace overlay, which closes itself).
                self.sidebar_sort_open = false;
                let key = match kind {
                    SortMenuKind::Hosts => {
                        self.hosts_sort = sort;
                        "hosts_sort"
                    }
                    SortMenuKind::Keys => {
                        self.keys_sort = sort;
                        "keys_sort"
                    }
                    SortMenuKind::Snippets => {
                        self.snippets_sort = sort;
                        "snippets_sort"
                    }
                };
                if let Some(v) = &self.vault {
                    if let Err(e) = v.set_setting(key, sort.as_storage_str()) {
                        tracing::warn!("failed to persist sort setting {key}: {e}");
                    }
                }
                self.overlay = None;
            }
            NavigationMessage::ToggleToolbarSearch => {
                use crate::state::{OverlayContent, OverlayState};
                let already_open = matches!(
                    self.overlay.as_ref().map(|o| &o.content),
                    Some(OverlayContent::ToolbarSearch)
                );
                if already_open {
                    self.overlay = None;
                } else {
                    // Anchor the floating field over the toolbar's search
                    // zone: at the leading edge under LTR, by its trailing
                    // edge under RTL (the render path subtracts the width).
                    let menu_w = self.toolbar_search_width();
                    let pad = 24.0_f32;
                    let panel = if self.vault_panel_open() {
                        self.panel_width
                    } else {
                        0.0
                    };
                    // Window-space anchor: a left-docked tab strip shifts
                    // the toolbar right; a right dock pulls the RTL
                    // trailing edge in.
                    let strip_left = self.side_strip_left_offset();
                    let strip_right = self.side_strip_reserve() - strip_left;
                    let x = if crate::i18n::is_rtl_layout() {
                        (self.window_size.width - panel - pad - strip_right).max(menu_w)
                    } else {
                        self.vault_rail_width() + strip_left + pad
                    };
                    // Sit over the toolbar row itself (the shared anchor is
                    // tuned for dropdowns *below* the button row; back out
                    // the button height + gap to land on the row).
                    let y = (self.dashboard_dropdown_anchor_y() - 42.0).max(0.0);
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::ToolbarSearch,
                        x,
                        y,
                    });
                    if let Some(id) = self.active_view_search_id() {
                        return crate::widgets::focus_input(id);
                    }
                }
            }
            NavigationMessage::ToggleToolbarOverflow => {
                use crate::state::{OverlayContent, OverlayState};
                let already_open = matches!(
                    self.overlay.as_ref().map(|o| &o.content),
                    Some(OverlayContent::ToolbarOverflow)
                );
                if already_open {
                    self.overlay = None;
                } else {
                    // Anchored on the `…` button's real drawn bounds
                    // (2 px gap, trailing edges aligned), mirroring the
                    // sort menu; the panel width only feeds the fallback.
                    let menu_width = self.overlay_menu_width(&OverlayState {
                        content: OverlayContent::ToolbarOverflow,
                        x: 0.0,
                        y: 0.0,
                    });
                    let panel = if self.vault_panel_open() {
                        self.panel_width
                    } else {
                        0.0
                    };
                    let (x, y) = self.toolbar_menu_anchor(
                        &self.toolbar_overflow_btn_bounds,
                        menu_width,
                        panel,
                    );
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::ToolbarOverflow,
                        x,
                        y,
                    });
                }
            }
            NavigationMessage::QuickHostContinue => {
                // The field this arrives from carries `on_submit`, and the
                // fork's `text_input` runs an on_submit binding on ANY
                // Enter, focused or not (`from_key_press` gates on focus,
                // the on_submit shortcut in front of it does not). The
                // empty state stays mounted BEHIND whatever opens over it,
                // so an Enter meant for a modal on top also arrived here
                // and rebuilt the host editor's form from scratch, which
                // discarded everything the modal had just written into it
                // (a highlight rule added on a new host vanished on Save).
                // A modal owns the keyboard by contract; the button itself
                // is unreachable under the scrim.
                if self.any_modal_blocks_input() {
                    return Task::none();
                }
                // Explicit connect targets (a username, a port, an IP
                // literal) quick-connect DIRECTLY, no editor stop: the
                // empty-state rebuild dropped the toolbar search that
                // used to be the first-run quick-connect entry point
                // (issue #97 regression), and this restores it. A bare
                // name falls through to the editor below, keeping the
                // add-your-first-host onboarding intent.
                let input = self.quick_host_input.trim().to_string();
                if oryxis_core::ssh_target::SshTarget::parse(&input)
                    .is_some_and(|t| t.is_explicit())
                    && let Some(conn) = self.quick_connect_target(&input)
                {
                    // The target now lives in its tab; unlike the
                    // editor path below (where the value survives a
                    // cancel), leaving it here would prepend itself to
                    // the next thing typed on the empty state.
                    self.quick_host_input.clear();
                    return self.update(Message::Ssh(crate::messages::SshMessage::QuickConnect(
                        Box::new(crate::state::QuickConnectEntry::bare(conn)),
                    )));
                }
                // Same editor the toolbar's "+ Host" opens (shared
                // setup: default port, editor combos, group
                // pre-fill). An empty field still opens it: on the
                // first-run screen there is no toolbar, so Continue is
                // the only way in and must never dead-end. The typed
                // value survives the panel in case the user cancels.
                let task = self.open_new_host_editor();
                if input.is_empty() {
                    return task;
                }
                // The TRIMMED value: the raw field kept its whitespace,
                // which then rode into the Host box and out to the
                // resolver (issue #171). What the parse REJECTS still
                // lands here on purpose, so the user sees what they
                // typed; the editor's own split is what cleans it on
                // the way to the vault.
                self.editor_form.hostname = input;
                // Hostname came in pre-filled, so the cursor belongs on
                // the one field still required: the label.
                return crate::widgets::focus_input(iced::widget::Id::new(
                    "editor-label",
                ));
            }
        }
        Task::none()
    }
}

impl Oryxis {
    /// Every distinct host tag, case-insensitive dedup keeping the
    /// first spelling, sorted for the filter dropdown.
    pub(crate) fn distinct_host_tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = Vec::new();
        for conn in &self.connections {
            for tg in &conn.tags {
                if !tags.iter().any(|x| x.eq_ignore_ascii_case(tg)) {
                    tags.push(tg.clone());
                }
            }
        }
        tags.sort_by_key(|t| t.to_lowercase());
        tags
    }

    /// Whether the dashboard tag-filter affordance should render: at
    /// least one host is tagged, or a (now possibly dangling) filter
    /// is active and needs a way to be cleared.
    pub(crate) fn host_tag_filter_available(&self) -> bool {
        !self.host_filter_tags.is_empty()
            || self.connections.iter().any(|c| !c.tags.is_empty())
    }
}
