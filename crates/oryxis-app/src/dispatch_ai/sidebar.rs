//! The terminal sidebar's chrome: which tab is showing, its search and
//! sort affordances, and the drag that resizes it.
//!
//! Not chat-specific, despite living with the AI dispatch: the same
//! panel hosts Snippets, History and Files, and switching between them
//! is the same message.

use iced::Task;
use crate::app::{SftpMessage, AiMessage, Message, Oryxis};


impl Oryxis {
    pub(super) fn handle_ai_sidebar(&mut self, message: AiMessage) -> Task<Message> {
        match message {
            AiMessage::ToggleSidebarRegion(side) => {
                let toggled_to = if let Some(idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(idx)
                {
                    tab.sidebar_open[side.idx()] = !tab.sidebar_open[side.idx()];
                    Some(tab.sidebar_open[side.idx()])
                } else {
                    None
                };
                if toggled_to == Some(true) {
                    // Opening: land on the configured default tab
                    // (issue #85) when it lives in THIS region, resolved
                    // against what the region offers so a gated default
                    // (Files/Monitor with no SSH, Chat with AI off)
                    // never opens an empty panel. "Last opened" (or a
                    // default docked to the other region) keeps the
                    // remembered tab, which `sidebar_region_tab` already
                    // re-resolves against the region's gates per frame.
                    if let Some(default) = self.prefs.sidebar_default_tab
                        && self.prefs.sidebar_tab_side(default) == Some(side)
                        && self.sidebar_tab_available(default)
                    {
                        self.set_sidebar_region_tab(default);
                    }
                    // Opening onto the Files or tmux tab: mount / catch up
                    // to the shell's cwd, or list the host's sessions.
                    // Both are no-ops on every other tab (including every
                    // tab of the other region).
                    if matches!(
                        self.sidebar_region_tab(side),
                        Some(crate::state::TerminalSidebarTab::Files)
                            | Some(crate::state::TerminalSidebarTab::Tmux)
                    ) {
                        return iced::Task::batch([
                            self.sidebar_files_sync(),
                            self.tmux_sync(),
                        ]);
                    }
                }
                if toggled_to == Some(false) {
                    // Closing the chat's region is the user's "stop it"
                    // gesture (the reported bug: a runaway tool loop kept
                    // running after the sidebar was closed). Cancel any
                    // live chat work so it doesn't keep executing commands
                    // in the background. Only when Chat lived here: closing
                    // the other region must not kill a visible chat.
                    if self.prefs.sidebar_tab_side(crate::state::TerminalSidebarTab::Chat)
                        == Some(side)
                    {
                        self.abort_active_chat_task();
                    }
                    // A closed region can't keep a keynav ring: it would
                    // silently swallow Enter/arrows meant for the terminal.
                    // Only this region's ring: the other region keeps its
                    // engagement. Same for the dropdown gate: a HostConfig
                    // pick_list open at close time unmounts without
                    // on_close, but only when HostConfig was what THIS
                    // region showed; clearing unconditionally would drop
                    // the modality gate under a dropdown still open in the
                    // other region, double-dispatching its next Enter/Esc.
                    if self
                        .keynav
                        .sidebar_selected
                        .is_some_and(|(t, _)| self.prefs.sidebar_tab_side(t) == Some(side))
                    {
                        self.keynav.sidebar_selected = None;
                    }
                    if self.sidebar_region_tab(side)
                        == Some(crate::state::TerminalSidebarTab::HostConfig)
                    {
                        self.keynav.pick_open = false;
                    }
                }
            }
            AiMessage::SelectTerminalSidebarTab(tab) => {
                // A HostConfig dropdown open when the sidebar tab swaps
                // unmounts without on_close; drop the gate with it, but
                // only when the swap happens in the region HostConfig was
                // showing in: switching tabs in the OTHER region leaves
                // the dropdown mounted, and clearing would double-dispatch
                // its next Enter/Esc.
                let host_config = crate::state::TerminalSidebarTab::HostConfig;
                if tab != host_config
                    && let Some(region) = self.prefs.sidebar_tab_side(tab)
                    && self.sidebar_region_tab(region) == Some(host_config)
                {
                    self.keynav.pick_open = false;
                }
                // Leaving the Files tab is a blur for its path edit; a
                // stale full-width input waiting behind the tab switch
                // would read as broken on return.
                self.close_files_path_edit();
                self.set_sidebar_region_tab(tab);
                if tab == crate::state::TerminalSidebarTab::History {
                    self.refresh_command_history();
                    // Owner call: entering History lands the keyboard in
                    // its search field. No-op on the empty state, whose
                    // frame renders no such input.
                    return crate::widgets::focus_input(iced::widget::Id::new(
                        "sidebar-history-search",
                    ));
                }
                if tab == crate::state::TerminalSidebarTab::Files {
                    // Mount the pane's SFTP channel (first open) or catch
                    // up to the shell's cwd.
                    return self.sidebar_files_sync();
                }
                if tab == crate::state::TerminalSidebarTab::Tmux {
                    // List the host's sessions on first open. Idempotent:
                    // returning to the tab reuses what is already there,
                    // and a refresh is the user's own action.
                    return self.tmux_sync();
                }
            }
            AiMessage::SidebarSnippetSearchChanged(v) => {
                self.sidebar_snippet_search = v;
            }
            AiMessage::HostsTreeToggleGroup(gid) => {
                if !self.hosts_tree_expanded.remove(&gid) {
                    self.hosts_tree_expanded.insert(gid);
                }
            }
            AiMessage::HostsTreeSearchChanged(v) => {
                self.hosts_tree_search = v;
            }
            AiMessage::ToggleSidebarSort => {
                self.sidebar_sort_open = !self.sidebar_sort_open;
                if self.sidebar_sort_open {
                    self.sidebar_search_open = false;
                }
            }
            AiMessage::ToggleSidebarSearch => {
                self.sidebar_search_open = !self.sidebar_search_open;
                self.sidebar_sort_open = false;
                if self.sidebar_search_open {
                    return crate::widgets::focus_input(iced::widget::Id::new(
                        "sidebar-snippet-search",
                    ));
                }
                // Collapsing clears the needle so the list shows everything.
                self.sidebar_snippet_search.clear();
            }
            AiMessage::ChatSidebarResizeStart(side) => {
                // Capture the region plus cursor x and current width,
                // the MouseMoved handler computes the delta against
                // these.
                self.chat_ui.sidebar_drag =
                    Some((side, self.mouse_position.x, self.chat_ui.sidebar_width[side.idx()]));
            }
            AiMessage::ChatSidebarResizeStop => {
                // Global left-release: a drag-out that never crossed
                // its threshold dies with the press (issue #167).
                self.drag_out_arm = None;
                self.chat_ui.sidebar_drag = None;
                // The same global Left-release ends a side-panel editor
                // drawer resize; persist the final width so it survives
                // a relaunch.
                if self.panel_resize_drag.take().is_some() {
                    self.persist_setting(
                        "side_panel_width",
                        &format!("{:.0}", self.panel_width),
                    );
                }
                // The same global Left-release ends an SFTP divider drag;
                // persist the final ratio so it survives a relaunch.
                if self.sftp_chrome.split_drag.take().is_some() {
                    self.persist_setting(
                        "sftp_split_ratio",
                        &format!("{:.4}", self.sftp_chrome.split_ratio),
                    );
                }
                // Same Left-release ends a log-panel resize; persist the
                // final height so it survives a relaunch.
                if self.sftp_chrome.log_drag.take().is_some() {
                    self.persist_setting(
                        "sftp_log_height",
                        &format!("{:.0}", self.sftp.log_height),
                    );
                }
                // End a column resize: the width was updated live, so just
                // re-seed the template and persist.
                if let Some((side, _, _, _)) = self.sftp_chrome.col_resize.take() {
                    self.sftp_chrome.columns_template = self.sftp.pane(side).columns.clone();
                    self.persist_sftp_columns();
                }
                // A tab released over the content area merges into the
                // tab showing there instead of reordering (issue #112).
                // Runs first and consumes the drag on success, so the
                // reorder path below sees nothing left to do. Nothing is
                // proposed unless the cursor sits on a split anchor, so
                // an ordinary reorder release falls straight through.
                self.merge_dragged_tab_if_proposed();
                // Ends a tab reorder drag. The live-slide already moved
                // the tab into place during the drag (see TabHovered); on
                // drop we just persist the new pinned order (if the dragged
                // tab is pinned) and clear. A plain click (never promoted to
                // `active`) clears with no persist. Runs BEFORE any early
                // return below: a release that also finished a column
                // sort / SFTP drag / armed a rename used to skip this,
                // leaving the ghost chip stuck on screen (field report).
                if let Some(drag) = self.tab_drag.take()
                    && drag.active
                {
                    // Persist when the dragged tab (terminal or SFTP) is pinned,
                    // so the rearranged pinned order survives a relaunch.
                    let pinned = self
                        .tabs
                        .iter()
                        .find(|t| t._id == drag.from_id)
                        .map(|t| t.pinned)
                        .or_else(|| {
                            self.sftp_tabs
                                .iter()
                                .find(|t| t.id == drag.from_id)
                                .map(|t| t.pinned)
                        })
                        .unwrap_or(false);
                    if pinned {
                        self.persist_pinned_tabs();
                    }
                }
                // End a column reorder. If the drag went active, move the
                // dragged column before whichever header the cursor is over;
                // a release without movement is a plain click that sorts.
                if let Some(drag) = self.sftp_chrome.col_drag.take() {
                    let hovered = self.sftp_chrome.hovered_col;
                    self.sftp_chrome.hovered_col = None;
                    if drag.active {
                        // Name is never a drop target: nothing can be dropped
                        // onto/before it (so it shows no drop effect and keeps
                        // its slot). It can still be dragged elsewhere itself.
                        if let Some((hside, hcol)) = hovered
                            && hside == drag.side
                            && hcol != drag.col
                            && hcol != crate::state::SftpColumn::Name
                        {
                            self.sftp.pane_mut(drag.side).columns.reorder(drag.col, hcol);
                            self.sftp_chrome.columns_template =
                                self.sftp.pane(drag.side).columns.clone();
                            self.persist_sftp_columns();
                        }
                    } else if let Some(sort_col) = drag.col.sort_column() {
                        return Task::done(Message::Sftp(SftpMessage::SftpSort(drag.side, sort_col)));
                    }
                }
                // Same global Left-release event also ends an internal
                // SFTP drag. If the drag was active, dispatch the transfer;
                // otherwise it was a plain click, which may have armed a
                // slow-click rename (set on the press in SftpSelectRow).
                if let Some(drag) = self.sftp.drag.take()
                    && drag.active
                {
                    self.sftp.pending_rename = None;
                    return self.handle_internal_drag_drop(drag);
                }
                if self.sftp.pending_rename.is_some() {
                    return self.defer_slow_rename();
                }
            }
            // Routed here by `handle_ai`; anything else is a
            // grouping mistake rather than a runtime case.
            m => return crate::dispatch::unrouted(m),
        }
        Task::none()
    }
}
