//! Overlay menu builders for tab / sidebar-files / toolbar / picker /
//! sort / terminal context menus. Split out of `render_overlay_menu` in
//! views/layout/menus.rs; each method returns the inner menu `items`
//! Element that `render_overlay_menu` wraps in the shared popover
//! container. Pure relocation, no behavior change.

use super::*;
use crate::messages::MonitorMessage;
use iced::widget::column;

impl Oryxis {
    pub(crate) fn build_menu_sidebar_files_row(
        &self,
        path: String,
        is_dir: bool,
    ) -> Element<'_, Message> {
        let secondary = OryxisColors::t().text_secondary;
        // A multi-selection (SFTP pane's rule): the menu collapses to
        // the bulk actions that make sense over a whole selection. The
        // selection is whatever the right-click kept; the builder runs
        // at render time, right after `ShowSidebarFilesRowMenu` decided
        // it, so it is in sync with the highlighted rows.
        let multi = self.sidebar_files_multi(&path);
        if multi {
            let selection = self
                .active_tab
                .and_then(|i| self.tabs.get(i))
                .map(|t| t.active())
                .map(|p| crate::dispatch_sidebar_files::selected_items(&p.files))
                .unwrap_or_default();
            let n = selection.len();
            let mut items = iced::widget::Column::<iced::Element<'_, _>>::new();
            let copy_label =
                crate::i18n::t("copy_n_paths").replacen("{n}", &n.to_string(), 1);
            items = items.push(self.menu_item_owned(
                iced_fonts::lucide::clipboard_copy(),
                copy_label,
                Message::SidebarFiles(SidebarFilesMessage::SidebarFilesCopySelectionPaths),
                secondary,
            ));
            // A local browser has nothing to download (issue #145), the
            // same gate the single-file download applies.
            if !self.sidebar_files_is_local() {
                let download_label =
                    crate::i18n::t("download_n_items").replacen("{n}", &n.to_string(), 1);
                items = items.push(self.menu_item_owned(
                    iced_fonts::lucide::download(),
                    download_label,
                    Message::SidebarFiles(SidebarFilesMessage::SidebarFilesDownloadSelection),
                    OryxisColors::t().accent,
                ));
            }
            let delete_label =
                crate::i18n::t("delete_n_items").replacen("{n}", &n.to_string(), 1);
            items = items.push(self.menu_item_owned(
                iced_fonts::lucide::trash(),
                delete_label,
                Message::SidebarFiles(SidebarFilesMessage::SidebarFilesDeleteSelection(selection)),
                OryxisColors::t().error,
            ));
            return items.boxed();
        }
        // Directory the "Open SFTP session here" lands on: the
        // folder itself, or a file's containing folder.
        let sftp_dir = if is_dir {
            path.clone()
        } else {
            crate::dispatch_sidebar_files::files_parent_dir(&path)
                .unwrap_or_else(|| "/".to_string())
        };
        let name = crate::dispatch_sidebar_files::files_basename(&path);
        // A local browser (issue #145): transfers, edit-in-place, the
        // dual-pane promote and chmod-properties have no local meaning;
        // files open with the OS instead.
        let local = self.sidebar_files_is_local();
        let mut items = iced::widget::Column::<iced::Element<'_, _>>::new();
        if is_dir {
            items = items.push(self.menu_item(
                iced_fonts::lucide::folder_open(),
                crate::i18n::t("open"),
                Message::SidebarFiles(SidebarFilesMessage::SidebarFilesNavigate(path.clone())),
                secondary,
            ));
        } else if local {
            items = items.push(self.menu_item(
                iced_fonts::lucide::external_link(),
                crate::i18n::t("open"),
                Message::Sftp(SftpMessage::SftpOpenLocal(std::path::PathBuf::from(&path))),
                secondary,
            ));
        } else {
            // Edit-in-place (temp download + OS editor + upload
            // on save) and a one-shot download, both on the
            // sidebar's own channel.
            items = items.push(self.menu_item(
                iced_fonts::lucide::pencil(),
                crate::i18n::t("sftp_open_edit"),
                Message::SidebarFiles(SidebarFilesMessage::SidebarFilesEdit(path.clone())),
                secondary,
            ));
            items = items.push(self.menu_item(
                iced_fonts::lucide::download(),
                crate::i18n::t("download"),
                Message::SidebarFiles(SidebarFilesMessage::SidebarFilesDownload(path.clone())),
                OryxisColors::t().accent,
            ));
        }
        if local {
            items = items.push(self.menu_item(
                iced_fonts::lucide::folder_search(),
                crate::i18n::open_in_file_manager_label(),
                Message::Sftp(SftpMessage::SftpRevealInExplorer(
                    std::path::PathBuf::from(&path),
                    is_dir,
                )),
                secondary,
            ));
        } else {
            items = items.push(self.menu_item(
                iced_fonts::lucide::folder_tree(),
                crate::i18n::t("open_sftp_session_here"),
                Message::SidebarFiles(SidebarFilesMessage::SidebarFilesOpenSftpAt(sftp_dir)),
                OryxisColors::t().accent,
            ));
        }
        if is_dir && !local {
            items = items.push(self.menu_item(
                iced_fonts::lucide::upload(),
                crate::i18n::t("upload_here"),
                Message::SidebarFiles(SidebarFilesMessage::SidebarFilesUploadInto(path.clone())),
                secondary,
            ));
        }
        items = items.push(self.menu_item(
            iced_fonts::lucide::pen_line(),
            crate::i18n::t("rename"),
            Message::SidebarFiles(SidebarFilesMessage::SidebarFilesStartRename(path.clone())),
            secondary,
        ));
        if !local {
            items = items.push(self.menu_item(
                iced_fonts::lucide::cog(),
                crate::i18n::t("properties"),
                Message::SidebarFiles(SidebarFilesMessage::SidebarFilesShowProperties(path.clone(), is_dir)),
                secondary,
            ));
        }
        items = items.push(self.menu_item(
            iced_fonts::lucide::clipboard_copy(),
            crate::i18n::t("copy_path"),
            Message::Sftp(SftpMessage::SftpCopyPath(path.clone())),
            secondary,
        ));
        if !is_dir {
            // Routed through SftpCopyPath (not CopyToClipboard)
            // for the menu-dismiss it carries.
            items = items.push(self.menu_item(
                iced_fonts::lucide::text_cursor_input(),
                crate::i18n::t("copy_name"),
                Message::Sftp(SftpMessage::SftpCopyPath(name)),
                secondary,
            ));
        }
        items = items.push(self.menu_item(
            iced_fonts::lucide::trash(),
            crate::i18n::t("delete"),
            Message::SidebarFiles(SidebarFilesMessage::SidebarFilesDelete(path.clone(), is_dir)),
            OryxisColors::t().error,
        ));
        items.boxed()
    }

    pub(crate) fn build_menu_sidebar_files_background(&self, dir: String) -> Element<'_, Message> {
        let secondary = OryxisColors::t().text_secondary;
        let mut items = column![
            self.menu_item(
                iced_fonts::lucide::folder_plus(),
                crate::i18n::t("new_folder"),
                Message::SidebarFiles(SidebarFilesMessage::SidebarFilesStartNewEntry(crate::state::SftpEntryKind::Folder)),
                secondary,
            ),
            self.menu_item(
                iced_fonts::lucide::file_plus(),
                crate::i18n::t("new_file"),
                Message::SidebarFiles(SidebarFilesMessage::SidebarFilesStartNewEntry(crate::state::SftpEntryKind::File)),
                secondary,
            ),
        ];
        // Uploading into a local folder has no meaning (issue #145);
        // dropping/copying files there is the OS's own job.
        if !self.sidebar_files_is_local() {
            items = items.push(self.menu_item(
                iced_fonts::lucide::upload(),
                crate::i18n::t("upload_here"),
                Message::SidebarFiles(SidebarFilesMessage::SidebarFilesUploadInto(dir.clone())),
                OryxisColors::t().accent,
            ));
        }
        items = items.push(self.menu_item(
            iced_fonts::lucide::rotate_cw(),
            crate::i18n::t("refresh"),
            Message::SidebarFiles(SidebarFilesMessage::SidebarFilesRefresh),
            secondary,
        ));
        items = items.push(self.menu_item(
            iced_fonts::lucide::clipboard_copy(),
            crate::i18n::t("copy_path"),
            Message::Sftp(SftpMessage::SftpCopyPath(dir)),
            secondary,
        ));
        items.boxed()
    }

    pub(crate) fn build_menu_tab_actions(&self, idx: usize) -> Element<'_, Message> {
        iced::widget::Column::with_children(self.tab_actions_menu_items(idx)).boxed()
    }

    /// How many rows the tab menu has right now, for `overlay_menu_height`.
    /// Built and counted rather than tallied from the conditions a second
    /// time: the menu runs from 13 rows to over 20 depending on the tab
    /// (split, session group, quick host, closed-tab stack, right-click
    /// scheme), and the flat guess it used to carry drew its last rows
    /// past the bottom edge of a bottom-docked strip.
    pub(crate) fn tab_actions_menu_rows(&self, idx: usize) -> f32 {
        self.tab_actions_menu_items(idx).len() as f32
    }

    /// The tab menu's rows, in order. One owner for the rows and their
    /// count (see `tab_actions_menu_rows`).
    fn tab_actions_menu_items(&self, idx: usize) -> Vec<Element<'_, Message>> {
        let mut items: Vec<Element<'_, Message>> = vec![
            self.menu_item(iced_fonts::lucide::pen_line(), crate::i18n::t("rename_tab"), Message::Tabs(TabsMessage::StartRenameTab(idx)), OryxisColors::t().text_secondary),
            self.menu_item(iced_fonts::lucide::columns_two(), crate::i18n::t("split_side_by_side"), Message::Terminal(TerminalMessage::SplitTabPane(idx, iced::widget::pane_grid::Axis::Vertical)), OryxisColors::t().text_secondary),
            self.menu_item(iced_fonts::lucide::rows_two(), crate::i18n::t("split_stacked"), Message::Terminal(TerminalMessage::SplitTabPane(idx, iced::widget::pane_grid::Axis::Horizontal)), OryxisColors::t().text_secondary),
            self.menu_item(iced_fonts::lucide::copy(), crate::i18n::t("duplicate_tab"), Message::Tabs(TabsMessage::DuplicateTab(idx)), OryxisColors::t().text_secondary),
        ];
        // Zoom the focused pane to the whole tab (#113). Only on split
        // tabs: a lone pane already fills it. The label flips to Restore
        // while zoomed, which is also the only on-screen affordance
        // telling you the tab is in that state.
        if self.tabs.get(idx).is_some_and(|t| t.pane_grid.panes.len() >= 2) {
            let zoomed = self
                .tabs
                .get(idx)
                .is_some_and(|t| t.pane_grid.maximized().is_some());
            let (glyph, key) = if zoomed {
                (iced_fonts::lucide::minimize(), "restore_panes")
            } else {
                (iced_fonts::lucide::maximize(), "maximize_pane")
            };
            items.push(self.menu_item(
                glyph,
                crate::i18n::t(key),
                Message::Terminal(TerminalMessage::ToggleMaximizePane(Some(idx))),
                OryxisColors::t().text_secondary,
            ));
            // Break the focused pane out into a tab of its own (issue
            // #208 item 4). Offered here as well as on the pane itself
            // because the pane's own menu only exists under the Menu
            // right-click scheme, and this action has nothing to do with
            // what the terminal does with a right-click: a user who
            // keeps right-click on Paste would otherwise have no way to
            // reach it. Focused pane, like the zoom above it.
            if let Some(pane_id) = self.tabs.get(idx).map(|t| t.active().id) {
                items.push(self.menu_item(
                    iced_fonts::lucide::external_link(),
                    crate::i18n::t("pane_to_new_tab"),
                    Message::Terminal(TerminalMessage::MovePaneToNewTab(pane_id)),
                    OryxisColors::t().text_secondary,
                ));
            }
        }
        // Broadcast input across the tab's panes (C2): a check glyph +
        // warning tint mark the armed state, matching the pane borders and
        // status segment. Only offered where there are two panes that take
        // the fan-out (broadcast is inert otherwise; arming is refused
        // there anyway, and an SFTP console never takes it).
        if self.tabs.get(idx).is_some_and(|t| t.broadcast_capable()) {
            let broadcasting = self.tabs.get(idx).map(|t| t.broadcast).unwrap_or(false);
            let (bc_glyph, bc_color) = if broadcasting {
                (iced_fonts::lucide::check(), OryxisColors::t().warning)
            } else {
                (iced_fonts::lucide::radio(), OryxisColors::t().text_secondary)
            };
            items.push(self.menu_item(bc_glyph, crate::i18n::t("broadcast_input"), Message::Terminal(TerminalMessage::ToggleTabBroadcast(idx)), bc_color));
        }
        // Open an SFTP tab for this host: offered when the SFTP
        // feature is on AND the tab has a live SSH session to reuse
        // or matches a saved connection (so it isn't shown on
        // local-shell tabs where it would no-op).
        let can_sftp = self.sftp_enabled
            && self
            .tabs
            .get(idx)
            .map(|t| {
                let base = t.label.trim_end_matches(" (disconnected)");
                // Telnet transports (and Telnet-protocol saved
                // hosts) carry no SSH handle to mount SFTP on. A
                // session that survives roaming has none either, and
                // for the opposite reason: it let its SSH go. That one
                // still gets the entry, because it opens a TAB of its
                // own rather than a surface inside this one.
                t.active().session.as_ref().is_some_and(|s| {
                    s.ssh().is_some() || s.survives_roaming()
                })
                    || self.connections.iter().any(|c| {
                        c.label == base
                            && c.protocol
                                == oryxis_core::models::connection::ConnectionProtocol::Ssh
                    })
            })
            .unwrap_or(false);
        // Hybrid SFTP session (issue #61, owner QA 2026-07-05):
        // SFTP is the tab's own session, not a separate tab. No
        // session yet = "Open SFTP session" (creates + shows it,
        // and the tab's toggle glyph appears); with one, the
        // entry flips Show SFTP / Show terminal. An in-Files-mode
        // tab keeps its entry even after a disconnect so the way
        // back never disappears. The old "Open SFTP tab" entry is
        // gone: detaching the session to its own tab (below) is
        // the standalone path now.
        let in_files = self.tabs.get(idx).map(|t| t.files_mode).unwrap_or(false);
        let has_session = self
            .tabs
            .get(idx)
            .map(|t| self.tab_has_sftp_session(t))
            .unwrap_or(false);
        if can_sftp || in_files {
            let (glyph, label) = if in_files {
                (iced_fonts::lucide::terminal(), crate::i18n::t("tab_show_terminal"))
            } else if has_session {
                (iced_fonts::lucide::folder_tree(), crate::i18n::t("tab_show_files"))
            } else if self
                .tabs
                .get(idx)
                .and_then(|t| t.active().session.as_ref())
                .is_some_and(|s| s.survives_roaming())
            {
                // Says what it does: this one opens a separate tab,
                // and the wording is the difference between a surface
                // appearing here and a new tab appearing there.
                (iced_fonts::lucide::folder_tree(), crate::i18n::t("open_sftp_tab"))
            } else {
                (iced_fonts::lucide::folder_tree(), crate::i18n::t("tab_open_sftp_session"))
            };
            items.push(self.menu_item(glyph, label, Message::Tabs(TabsMessage::ToggleTabFilesMode(idx)), OryxisColors::t().text_secondary));
        }
        // The console, offered whenever this tab names a host it could
        // open one on. Deliberately NOT gated on `has_session`: the
        // console dials for itself through the ordinary connect path
        // (with the reuse pool lending a live link when there is one),
        // so a tab whose session dropped can still open one, which is
        // exactly when someone reaches for it.
        if self.sftp_enabled && self.tab_console_target(idx).is_some() {
            items.push(self.menu_item(
                iced_fonts::lucide::square_terminal(),
                crate::i18n::t("open_sftp_console"),
                Message::Sftp(SftpMessage::OpenSftpConsoleForTab(idx)),
                OryxisColors::t().text_secondary,
            ));
        }
        if has_session && self.sftp_enabled {
            // Promote the tab's SFTP session to a standalone tab
            // (the server-to-server dual-remote surface).
            items.push(self.menu_item(iced_fonts::lucide::external_link(), crate::i18n::t("tab_detach_sftp"), Message::Tabs(TabsMessage::DetachTabSftp(idx)), OryxisColors::t().text_secondary));
            // Close just the SFTP session, back to a plain
            // terminal tab (the terminal keeps running).
            items.push(self.menu_item(iced_fonts::lucide::x(), crate::i18n::t("tab_close_sftp_session"), Message::Tabs(TabsMessage::CloseTabSftpSession(idx)), OryxisColors::t().text_secondary));
        }
        // Quick-connect tab: offer to persist the ad-hoc host into
        // the vault (opens the editor prefilled as a new host).
        if let Some(crate::state::PaneOrigin::QuickHost(qid)) =
            self.tabs.get(idx).map(|t| &t.active().origin)
        {
            items.push(self.menu_item(iced_fonts::lucide::save(), crate::i18n::t("quick_connect_save_host"), Message::Editor(EditorMessage::SaveQuickHost(*qid)), OryxisColors::t().accent));
        }
        // Save the whole arrangement (panes + splits + per-pane
        // scripts) as a reusable session group, or edit it if this
        // tab already came from one. Only meaningful for a split tab
        // (>1 pane); a single-pane tab is just a host, not a group.
        // Already-saved groups keep the "Edit" entry so they stay
        // editable even if pruned down to one pane.
        let tab_ref = self.tabs.get(idx);
        let is_group = tab_ref.map(|t| t.session_group_id.is_some()).unwrap_or(false);
        let is_split = tab_ref.map(|t| t.pane_count() > 1).unwrap_or(false);
        if is_split || is_group {
            let sg_label = if is_group {
                crate::i18n::t("edit_session_group")
            } else {
                crate::i18n::t("save_session_group")
            };
            items.push(self.menu_item(iced_fonts::lucide::boxes(), sg_label, Message::SessionGroup(SessionGroupMessage::ShowSaveSessionGroup(idx)), OryxisColors::t().text_secondary));
        }
        // Pin / unpin: pinned tabs render first and restore on launch.
        // The restore spec captures only a single pane's origin, so
        // pinning is offered only on single-pane, non-group tabs (a
        // split / session-group tab would silently restore just its
        // focused pane). An already-pinned tab always shows "unpin".
        let is_pinned = tab_ref.map(|t| t.pinned).unwrap_or(false);
        if is_pinned || (!is_split && !is_group) {
            let (pin_icon, pin_label) = if is_pinned {
                (iced_fonts::lucide::pin_off(), crate::i18n::t("unpin_tab"))
            } else {
                (iced_fonts::lucide::pin(), crate::i18n::t("pin_tab"))
            };
            items.push(self.menu_item(pin_icon, pin_label, Message::Tabs(TabsMessage::ToggleTabPin(idx)), OryxisColors::t().text_secondary));
        }
        // The window verbs. A tab moves between windows with its
        // session untouched (every tab lives in the app, a window only
        // shows it): into a window of its own while this one holds
        // anything else, and into each other window that is open.
        if let Some(tab_id) = self.tabs.get(idx).map(|t| t._id) {
            items.extend(self.window_move_items(tab_id));
        }
        {
            items.push(self.menu_item(iced_fonts::lucide::external_link(), crate::i18n::t("duplicate_new_window"), Message::Tabs(TabsMessage::DuplicateInNewWindow(idx)), OryxisColors::t().text_secondary));
        }
        // Copy the focused pane's host address. Only offered when the pane's
        // origin still resolves to a connection: a local shell, an SSM / ECS
        // pane or a deleted host has no address to hand over.
        if self
            .tabs
            .get(idx)
            .map(|t| t.active().id)
            .and_then(|pane_id| self.pane_origin_connection(pane_id))
            .is_some()
        {
            items.push(self.menu_item(iced_fonts::lucide::clipboard_copy(), crate::i18n::t("copy_host_address"), Message::Tabs(TabsMessage::CopyTabAddress(idx)), OryxisColors::t().text_secondary));
        }
        // "Copy Screen" belongs to the terminal's own context menu, which
        // only exists under the Menu right-click scheme: on the other two
        // there is no other door to it, so the tab menu becomes it.
        // Offered only for the tab ON SCREEN, and never in Files mode: the
        // viewport it copies is the one the widget last drew, and a
        // background pane keeps taking output without redrawing, so its
        // stored position would no longer be what anyone is looking at.
        if self.prefs.terminal_right_click != crate::util::RightClickMode::Menu
            && Some(idx) == self.cur_active_tab()
            && self.terminal_surface_visible()
            && let Some(pane_id) =
                self.tabs.get(idx).filter(|t| !t.files_mode).map(|t| t.active().id)
        {
            items.push(self.menu_item(
                iced_fonts::lucide::clipboard_copy(),
                crate::i18n::t("terminal_copy_screen"),
                Message::Terminal(TerminalMessage::TerminalCopyScreen(pane_id)),
                OryxisColors::t().text_secondary,
            ));
        }
        items.push(self.menu_item(iced_fonts::lucide::rotate_cw(), crate::i18n::t("reconnect"), Message::Tabs(TabsMessage::ReconnectTab(idx)), OryxisColors::t().accent));
        items.push(self.menu_item(iced_fonts::lucide::x(), crate::i18n::t("close_tab"), Message::Tabs(TabsMessage::CloseTab(idx)), OryxisColors::t().text_secondary));
        // Right under the closes, and only with something to bring back:
        // an entry that is always there and usually does nothing reads as
        // broken the first time it is tried (issue #186).
        if !self.closed_tabs.is_empty() {
            items.push(self.menu_item(iced_fonts::lucide::rotate_ccw(), crate::i18n::t("reopen_closed_tab"), Message::Tabs(TabsMessage::ReopenClosedTab), OryxisColors::t().text_secondary));
        }
        items.push(self.menu_item(iced_fonts::lucide::x(), crate::i18n::t("close_other_tabs"), Message::Tabs(TabsMessage::CloseOtherTabs(idx)), OryxisColors::t().text_secondary));
        items.push(self.menu_item(iced_fonts::lucide::x(), crate::i18n::t("close_all_tabs"), Message::Tabs(TabsMessage::CloseAllTabs), OryxisColors::t().error));
        items
    }

    /// Whether "Move to New Window" is offered in the current window:
    /// its strip holds more than one entry, or it is the only window
    /// (the same rule the handler applies, `can_detach`).
    fn can_detach_here(&self) -> bool {
        self.cur_tab_order().len() > 1 || self.other_windows().is_empty()
    }

    /// How many rows [`Self::window_move_items`] yields for a tab of the
    /// current window, for the menus that are sized by a row count.
    pub(crate) fn window_move_rows(&self) -> f32 {
        let own = usize::from(self.can_detach_here());
        (own + self.other_windows().len()) as f32
    }

    /// The "move this tab to another window" rows for the tab with strip
    /// id `tab_id`: a new window, then one row per other open window,
    /// named by what that window is showing.
    fn window_move_items(&self, tab_id: uuid::Uuid) -> Vec<Element<'_, Message>> {
        let mut items = Vec::new();
        if self.can_detach_here() {
            items.push(self.menu_item(
                iced_fonts::lucide::app_window(),
                crate::i18n::t("move_tab_new_window"),
                Message::Tabs(TabsMessage::MoveTabToNewWindow(tab_id)),
                OryxisColors::t().text_secondary,
            ));
        }
        for target in self.other_windows() {
            items.push(self.menu_item_owned(
                iced_fonts::lucide::log_in(),
                crate::i18n::t("move_tab_to_window").replace("{name}", &self.window_label(target)),
                Message::Tabs(TabsMessage::MoveTabToWindow(tab_id, target)),
                OryxisColors::t().text_secondary,
            ));
        }
        items
    }

    pub(crate) fn build_menu_sftp_tab_actions(&self, idx: usize) -> Element<'_, Message> {
        let is_pinned = self.sftp_tabs.get(idx).map(|t| t.pinned).unwrap_or(false);
        let (pin_icon, pin_label) = if is_pinned {
            (iced_fonts::lucide::pin_off(), crate::i18n::t("unpin_tab"))
        } else {
            (iced_fonts::lucide::pin(), crate::i18n::t("pin_tab"))
        };
        // "Terminal for the MOUNTED host" only means something once one
        // is mounted. A tab still on the host picker has no host to open
        // a shell on, and the entry used to sit there doing nothing when
        // clicked, which reads as broken rather than as not applicable.
        // Through the one authority, which also answers for a dormant
        // pinned tab (its panes carry no label until the first focus
        // re-mounts them, but its spec names the connection).
        let has_host = self.sftp_tab_terminal_host(idx).is_some();
        let mut items = column![self.menu_item(
            iced_fonts::lucide::plus(),
            crate::i18n::t("new_tab"),
            Message::Sftp(SftpMessage::NewSftpTab),
            OryxisColors::t().text_secondary,
        )];
        if has_host {
            // Second, where it has always been. Owner QA 2026-07-05: the
            // SFTP tab had no path back to a shell. Focuses a live
            // terminal on the mounted host, else connects one.
            items = items.push(self.menu_item(
                iced_fonts::lucide::terminal(),
                crate::i18n::t("open_terminal"),
                Message::Tabs(TabsMessage::OpenTerminalForSftpTab(idx)),
                OryxisColors::t().text_secondary,
            ));
        }
        items = items.push(self.menu_item(iced_fonts::lucide::pen_line(), crate::i18n::t("rename_tab"), Message::Tabs(TabsMessage::StartRenameSftpTab(idx)), OryxisColors::t().text_secondary));
        items = items.push(self.menu_item(pin_icon, pin_label, Message::Sftp(SftpMessage::ToggleSftpTabPin(idx)), OryxisColors::t().text_secondary));
        // The window verbs, the same ones the terminal tab's menu has.
        if let Some(tab_id) = self.sftp_tabs.get(idx).map(|t| t.id) {
            for item in self.window_move_items(tab_id) {
                items = items.push(item);
            }
        }
        items = items.push(self.menu_item(iced_fonts::lucide::x(), crate::i18n::t("close_tab"), Message::Sftp(SftpMessage::CloseSftpTab(idx)), OryxisColors::t().text_secondary));
        // One stack for both tab kinds, so this reaches a closed terminal
        // tab too: what the user asks back is the last chip that left the
        // strip, not the last one of this kind.
        if !self.closed_tabs.is_empty() {
            items = items.push(self.menu_item(iced_fonts::lucide::rotate_ccw(), crate::i18n::t("reopen_closed_tab"), Message::Tabs(TabsMessage::ReopenClosedTab), OryxisColors::t().text_secondary));
        }
        if self.has_other_sftp_tabs(idx) {
            items = items.push(self.menu_item(iced_fonts::lucide::x(), crate::i18n::t("close_other_tabs"), Message::Sftp(SftpMessage::CloseOtherSftpTabs(idx)), OryxisColors::t().text_secondary));
        }
        items.boxed()
    }

    /// Recent-host rows for the two tab popovers (issue #206), newest
    /// first, under a hairline.
    ///
    /// The list is [`Self::recent_connections`], the same one the tray
    /// submenu and the JumpList read, so "recent" has one meaning and no
    /// store of its own. Empty until a host has actually been connected,
    /// which keeps rows that do nothing off a popover opened for the
    /// first time, the rule the reopen row already follows here.
    ///
    /// `nav` picks the row builder rather than leaving it to the caller,
    /// because the two popovers disagree on purpose: the strip menu is a
    /// keynav surface and records its rows, while the `+` popover is a
    /// hover popover `modal_nav_surface` deliberately declines.
    fn recent_host_rows(&self, nav: bool) -> Vec<Element<'static, Message>> {
        let recent = self.recent_connections(RECENT_MENU_MAX);
        if recent.is_empty() {
            return Vec::new();
        }
        let privacy_terms = self.privacy_terms();
        let secondary = OryxisColors::t().text_secondary;
        let mut rows: Vec<Element<'static, Message>> = vec![menu_divider()];
        for c in recent {
            // `ConnectSsh` takes a position in `connections`, resolved in
            // the same frame the row is drawn, which is how every other
            // host row in the app reaches it (the picker's included).
            let Some(idx) = self.connections.iter().position(|x| x.id == c.id) else {
                continue;
            };
            let label =
                elide_menu_label(&self.privacy_display_label(&c.label, &c.label, &privacy_terms));
            let msg = Message::Ssh(SshMessage::ConnectSsh(idx));
            // A terminal glyph, not a clock: the row's subject is the
            // host and its verb is "open a session on it". Recency is
            // what ORDERS the list, not what each row is.
            rows.push(if nav {
                self.menu_item_owned(iced_fonts::lucide::terminal(), label, msg, secondary)
            } else {
                crate::widgets::context_menu_item_owned(
                    iced_fonts::lucide::terminal(),
                    label,
                    msg,
                    secondary,
                )
            });
        }
        rows
    }

    /// Height of the recent-host block in `overlay_menu_height`'s row
    /// units, next to its builder for the reason `split_menu_rows` is.
    fn recent_host_menu_rows(&self) -> f32 {
        let n = self.recent_connections(RECENT_MENU_MAX).len();
        if n == 0 { 0.0 } else { n as f32 + DIVIDER_ROWS }
    }

    pub(crate) fn build_menu_split(&self) -> Element<'_, Message> {
        let mut items = column![
            context_menu_item(iced_fonts::lucide::plus(), crate::i18n::t("new_tab"), Message::Tabs(TabsMessage::ShowNewTabPicker), OryxisColors::t().text_secondary),
        ];
        // The splits only exist for a tab to split. With none open the
        // popover is here for the reopen alone, and offering to halve a
        // pane that isn't there is the "reads as broken" case again.
        if self.cur_active_tab().is_some() {
            items = items.push(context_menu_item(iced_fonts::lucide::columns_two(), crate::i18n::t("split_side_by_side"), Message::Terminal(TerminalMessage::SplitPane(iced::widget::pane_grid::Axis::Vertical)), OryxisColors::t().text_secondary));
            items = items.push(context_menu_item(iced_fonts::lucide::rows_two(), crate::i18n::t("split_stacked"), Message::Terminal(TerminalMessage::SplitPane(iced::widget::pane_grid::Axis::Horizontal)), OryxisColors::t().text_secondary));
        }
        // Last, so the rows the popover has always had keep their
        // places. Reaching the reopen by mouse is what issue #186 asked
        // for after the hotkey: the `+` is where a new tab comes from,
        // so it is where a user looks for one back.
        if !self.closed_tabs.is_empty() {
            items = items.push(context_menu_item(iced_fonts::lucide::rotate_ccw(), crate::i18n::t("reopen_closed_tab"), Message::Tabs(TabsMessage::ReopenClosedTab), OryxisColors::t().text_secondary));
        }
        // Below the hairline, because they answer a different question
        // than the rows above: those act on the tab that exists, these
        // name a host to open. "New tab" still leads the popover, so the
        // recents are a shortcut past the picker, never a replacement
        // for it (issue #206).
        for row in self.recent_host_rows(false) {
            items = items.push(row);
        }
        // Keep the popover open while the cursor is over it (hover
        // bridge from the `+` button into the menu).
        MouseArea::new(items)
            .on_enter(Message::Tabs(TabsMessage::SplitMenuEnter))
            .on_exit(Message::Tabs(TabsMessage::SplitMenuLeave))
            .boxed()
    }

    /// Row count of the `+` popover, next to the builder so the height
    /// estimate follows its conditional rows instead of drifting from
    /// them (`overlay_menu_height`).
    pub(crate) fn split_menu_rows(&self) -> f32 {
        let mut rows: f32 = 1.0;
        if self.cur_active_tab().is_some() {
            rows += 2.0;
        }
        if !self.closed_tabs.is_empty() {
            rows += 1.0;
        }
        rows + self.recent_host_menu_rows()
    }

    /// Right-click on the tab strip's empty area (issue #186). Nothing
    /// destructive lives here: the same pixels drag the window, and a
    /// close one flick away from a drag is the misclick the chip's own
    /// dwell was added to prevent.
    pub(crate) fn build_menu_tab_bar_actions(&self) -> Element<'_, Message> {
        let mut items = column![self.menu_item(
            iced_fonts::lucide::plus(),
            crate::i18n::t("new_tab"),
            Message::Tabs(TabsMessage::ShowNewTabPicker),
            OryxisColors::t().text_secondary,
        )];
        if !self.closed_tabs.is_empty() {
            items = items.push(self.menu_item(iced_fonts::lucide::rotate_ccw(), crate::i18n::t("reopen_closed_tab"), Message::Tabs(TabsMessage::ReopenClosedTab), OryxisColors::t().text_secondary));
        }
        // Same block as the `+` popover's, recorded for keynav here
        // because this surface is one and that one is not.
        for row in self.recent_host_rows(true) {
            items = items.push(row);
        }
        items.boxed()
    }

    /// Row count of the strip menu, next to its builder for the reason
    /// `split_menu_rows` is.
    pub(crate) fn tab_bar_menu_rows(&self) -> f32 {
        let base: f32 = if self.closed_tabs.is_empty() { 1.0 } else { 2.0 };
        base + self.recent_host_menu_rows()
    }

    pub(crate) fn build_menu_sort(&self, kind: crate::state::SortMenuKind) -> Element<'_, Message> {
        let current = match kind {
            crate::state::SortMenuKind::Hosts => self.hosts_sort,
            crate::state::SortMenuKind::Keys => self.keys_sort,
            crate::state::SortMenuKind::Snippets => self.snippets_sort,
        };
        use crate::state::ListSort;
        // Each row: leading lucide icon, label, trailing
        // checkmark when the row matches the active sort.
        // Inlined as four explicit calls so the icon widget's
        // lifetime stays 'static (a closure would force the
        // icon to outlive the returned Element borrow).
        let divider = menu_divider();
        column![
            self.sort_row(
                kind,
                ListSort::LabelAsc,
                iced_fonts::lucide::arrow_down_a_z::<iced::Theme>(),
                "sort_label_asc",
                current == ListSort::LabelAsc,
            ),
            self.sort_row(
                kind,
                ListSort::LabelDesc,
                iced_fonts::lucide::arrow_down_z_a::<iced::Theme>(),
                "sort_label_desc",
                current == ListSort::LabelDesc,
            ),
            divider,
            self.sort_row(
                kind,
                ListSort::NewestFirst,
                iced_fonts::lucide::calendar_arrow_down::<iced::Theme>(),
                "sort_newest_first",
                current == ListSort::NewestFirst,
            ),
            self.sort_row(
                kind,
                ListSort::OldestFirst,
                iced_fonts::lucide::calendar_arrow_up::<iced::Theme>(),
                "sort_oldest_first",
                current == ListSort::OldestFirst,
            ),
        ].boxed()
    }

    pub(crate) fn build_menu_cloud_discover_group_picker(
        &self,
        overlay: &OverlayState,
    ) -> Element<'_, Message> {
        // Search input + filtered list. The search field is
        // the menu's own filter (independent of the modal's
        // "Import into" input). Picking a row fills the
        // input and closes the menu.
        let picker_needle = self
            .cloud_discover.default_group_picker_search
            .trim()
            .to_lowercase();
        // Rows are full breadcrumb paths so a subgroup is a pickable
        // import target; the import side resolves paths first.
        let mut all_groups: Vec<String> = self
            .groups
            .iter()
            .filter(|g| g.cloud_query.is_none())
            .map(|g| oryxis_core::models::Group::path_of(&self.groups, g.id))
            .filter(|path| {
                picker_needle.is_empty()
                    || path.to_lowercase().contains(&picker_needle)
            })
            .collect();
        all_groups.sort_by_key(|s| s.to_lowercase());
        all_groups.dedup();
        // Width chases the combo bounds via the outer
        // wrapper in `view_main` + `overlay_menu_width`; the
        // inner content fills whatever space that outer
        // container hands down. Padding 4+4 on the outer
        // wrapper means content fills (combo_width - 8).
        let menu_outer_width = self.overlay_menu_width(overlay);
        let menu_content_width = (menu_outer_width - 8.0).max(80.0);
        // Search input uses a distinct surface tint so the
        // user reads it as the popover's own filter (not a
        // second copy of the modal's "Import into" field).
        // Mirrors what most context-menus do with their
        // header row: tinted bg + tighter border than the
        // form inputs underneath.
        let search_input = iced::widget::text_input(
            crate::i18n::t("search_groups"),
            &self.cloud_discover.default_group_picker_search,
        )
        .on_input(
            |v| Message::Cloud(CloudMessage::CloudDiscoverDefaultGroupPickerSearchChanged(v)),
        )
        .padding(8)
        .width(Length::Fixed(menu_content_width))
        .style(|_theme: &iced::Theme, status| {
            let palette = OryxisColors::t();
            let bg = match status {
                iced::widget::text_input::Status::Focused { .. }
                | iced::widget::text_input::Status::Hovered => palette.bg_hover,
                _ => palette.bg_selected,
            };
            let border_color = match status {
                iced::widget::text_input::Status::Focused { .. } => palette.accent,
                _ => palette.border,
            };
            iced::widget::text_input::Style {
                background: Background::Color(bg),
                border: Border {
                    radius: Radius::from(6.0),
                    color: border_color,
                    width: 1.0,
                },
                placeholder: palette.text_muted,
                value: palette.text_primary,
                selection: Color { a: 0.30, ..palette.accent },
            }
        });
        let list_el: Element<'_, Message> = if all_groups.is_empty() {
            container(
                text(crate::i18n::t("cloud_discover_no_matches"))
                    .size(12)
                    .color(OryxisColors::t().text_muted),
            )
            .padding(Padding {
                top: 12.0,
                right: 12.0,
                bottom: 12.0,
                left: 12.0,
            })
            .boxed()
        } else {
            // Plain label rows: dropped the leading folder
            // glyph since every entry is a folder by
            // definition (the picker only lists groups) and
            // the icon was just visual noise.
            let mut items = iced::widget::Column::<iced::Element<'_, _>>::new().spacing(2);
            for label in all_groups {
                let display = label.clone();
                let row = iced::widget::button(
                    container(
                        text(display)
                            .size(12)
                            .color(OryxisColors::t().text_primary),
                    )
                    .padding(Padding {
                        top: 6.0,
                        right: 10.0,
                        bottom: 6.0,
                        left: 10.0,
                    })
                    .width(Length::Fill),
                )
                .on_press(
                    Message::Cloud(CloudMessage::CloudDiscoverDefaultGroupPick(label.clone())),
                )
                .width(Length::Fill)
                .style(|_, status| {
                    let bg = match status {
                        iced::widget::button::Status::Hovered => {
                            OryxisColors::t().bg_hover
                        }
                        _ => Color::TRANSPARENT,
                    };
                    iced::widget::button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border {
                            radius: Radius::from(4.0),
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                });
                items = items.push(self.modal_nav_slot(
                    crate::keynav::RowAction::activate(
                        Message::Cloud(CloudMessage::CloudDiscoverDefaultGroupPick(label)),
                    ),
                    4.0,
                    false,
                    row.boxed(),
                ));
            }
            iced::widget::scrollable(items)
                .height(Length::Fixed(220.0))
                .boxed()
        };
        column![search_input, Space::new().height(8), list_el]
            .width(Length::Fixed(menu_content_width))
            .boxed()
    }

    pub(crate) fn build_menu_group_picker(
        &self,
        overlay: &OverlayState,
        target: crate::state::GroupPickerTarget,
    ) -> Element<'_, Message> {
        // Same shape as the Discover modal's group picker
        // (search input + filtered scrollable list) but
        // wired to the shared `group_picker_search` /
        // `GroupPickerPick(target)` messages. Lives at the
        // top-level render path because the side-panel
        // editors don't short-circuit the way the modal
        // does.
        let menu_outer_width = self.overlay_menu_width(overlay);
        let menu_content_width = (menu_outer_width - 8.0).max(80.0);
        let needle = self.group_picker_search.trim().to_lowercase();
        // Re-parenting a manual group must not offer the group itself
        // or anything below it: nesting a folder inside its own
        // subtree would mint a parent cycle and orphan the whole
        // branch. The Save handler enforces the same rule, this filter
        // just keeps the invalid rows out of sight.
        let excluded: std::collections::HashSet<uuid::Uuid> = match target {
            crate::state::GroupPickerTarget::GroupEditParent => self
                .group_edit
                .id
                .map(|gid| oryxis_core::models::Group::subtree_ids(&self.groups, gid))
                .unwrap_or_default(),
            _ => Default::default(),
        };
        // Rows are full breadcrumb paths ("Prod / Frontend") so nested
        // folders are distinguishable; the search matches anywhere in
        // the path, and picking fills the path into the combo (the
        // save side resolves paths first).
        let mut all_groups: Vec<String> = self
            .groups
            .iter()
            .filter(|g| g.cloud_query.is_none() && !excluded.contains(&g.id))
            .map(|g| oryxis_core::models::Group::path_of(&self.groups, g.id))
            .filter(|path| {
                needle.is_empty() || path.to_lowercase().contains(&needle)
            })
            .collect();
        all_groups.sort_by_key(|s| s.to_lowercase());
        all_groups.dedup();
        // Moving a host can also mean moving it OUT of every folder:
        // the move target and the host editor's field offer it as a row
        // (an empty field already means root on save, but a row is the
        // discoverable door); the group editors never do, since their
        // own empty field is the "no parent" answer. First row, so it
        // is one Down away.
        let offers_root = matches!(
            target,
            crate::state::GroupPickerTarget::MoveHosts
                | crate::state::GroupPickerTarget::HostEditorParent
        );
        if offers_root {
            let root = crate::i18n::t("group_picker_root");
            if needle.is_empty() || root.to_lowercase().contains(&needle) {
                all_groups.insert(0, root.to_string());
            }
        }
        let search_input = iced::widget::text_input(
            crate::i18n::t("search_groups"),
            &self.group_picker_search,
        )
        .on_input(|v| Message::Navigation(NavigationMessage::GroupPickerSearchChanged(v)))
        .padding(8)
        .width(Length::Fixed(menu_content_width))
        .style(|_theme: &iced::Theme, status| {
            let palette = OryxisColors::t();
            let bg = match status {
                iced::widget::text_input::Status::Focused { .. }
                | iced::widget::text_input::Status::Hovered => palette.bg_hover,
                _ => palette.bg_selected,
            };
            let border_color = match status {
                iced::widget::text_input::Status::Focused { .. } => palette.accent,
                _ => palette.border,
            };
            iced::widget::text_input::Style {
                background: Background::Color(bg),
                border: Border {
                    radius: Radius::from(6.0),
                    color: border_color,
                    width: 1.0,
                },
                placeholder: palette.text_muted,
                value: palette.text_primary,
                selection: Color { a: 0.30, ..palette.accent },
            }
        });
        let list_el: Element<'_, Message> = if all_groups.is_empty() {
            container(
                text(crate::i18n::t("cloud_discover_no_matches"))
                    .size(12)
                    .color(OryxisColors::t().text_muted),
            )
            .padding(Padding {
                top: 12.0,
                right: 12.0,
                bottom: 12.0,
                left: 12.0,
            })
            .boxed()
        } else {
            let mut items = iced::widget::Column::<iced::Element<'_, _>>::new().spacing(2);
            for label in all_groups {
                let display = label.clone();
                // The "Top level" row is the one label that is not a
                // path: it travels as an empty string, the same value
                // an empty field means everywhere else.
                let label = if offers_root && label == crate::i18n::t("group_picker_root") {
                    String::new()
                } else {
                    label
                };
                let row = iced::widget::button(
                    container(
                        text(display)
                            .size(12)
                            .color(OryxisColors::t().text_primary),
                    )
                    .padding(Padding {
                        top: 6.0,
                        right: 10.0,
                        bottom: 6.0,
                        left: 10.0,
                    })
                    .width(Length::Fill),
                )
                .on_press(Message::Navigation(NavigationMessage::GroupPickerPick(target, label.clone())))
                .width(Length::Fill)
                .style(|_, status| {
                    let bg = match status {
                        iced::widget::button::Status::Hovered => {
                            OryxisColors::t().bg_hover
                        }
                        _ => Color::TRANSPARENT,
                    };
                    iced::widget::button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border {
                            radius: Radius::from(4.0),
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                });
                items = items.push(self.modal_nav_slot(
                    crate::keynav::RowAction::activate(
                        Message::Navigation(NavigationMessage::GroupPickerPick(target, label)),
                    ),
                    4.0,
                    false,
                    row.boxed(),
                ));
            }
            iced::widget::scrollable(items)
                .height(Length::Fixed(220.0))
                .boxed()
        };
        column![search_input, Space::new().height(8), list_el]
            .width(Length::Fixed(menu_content_width))
            .boxed()
    }

    pub(crate) fn build_menu_toolbar_overflow(&self) -> Element<'_, Message> {
        // The `…` menu holds *every* toolbar action for the view
        // (primary + secondary), so the narrow toolbar shows only
        // the search icon + this one button.
        use crate::state::{SortMenuKind, View};
        let secondary = OryxisColors::t().text_secondary;
        let mut col = iced::widget::Column::<iced::Element<'_, _>>::new().spacing(2);
        match self.cur_view() {
            View::Dashboard => {
                // Primary add action mirrors the toolbar's
                // context-aware button (none in a dynamic group,
                // Discover in a cloud folder, else New host + the
                // import/cloud sub-menu).
                match self.cur_nav().active_group {
                    Some(gid)
                        if self
                            .groups
                            .iter()
                            .find(|g| g.id == gid)
                            .and_then(|g| g.cloud_query.as_ref())
                            .is_some() => {}
                    Some(gid) => {
                        let linked = self
                            .connections
                            .iter()
                            .filter(|c| c.group_id == Some(gid))
                            .find_map(|c| c.cloud_ref.as_ref().map(|r| r.profile_id))
                            .or_else(|| {
                                self.groups
                                    .iter()
                                    .filter(|g| g.parent_id == Some(gid))
                                    .find_map(|g| {
                                        g.cloud_query.as_ref().map(|q| q.profile_id)
                                    })
                            });
                        if let Some(pid) = linked {
                            col = col.push(self.menu_item(
                                iced_fonts::lucide::download(),
                                crate::i18n::t("cloud_discover"),
                                Message::Cloud(CloudMessage::ShowCloudDiscover(pid)),
                                secondary,
                            ));
                        } else {
                            col = col.push(self.menu_item(
                                iced_fonts::lucide::plus(),
                                crate::i18n::t("new_host"),
                                Message::Editor(EditorMessage::ShowNewConnection),
                                secondary,
                            ));
                            col = col.push(self.menu_item(
                                iced_fonts::lucide::ellipsis(),
                                crate::i18n::t("toolbar_more"),
                                Message::Cloud(CloudMessage::ShowCloudProviderPicker),
                                secondary,
                            ));
                        }
                    }
                    None => {
                        col = col.push(self.menu_item(
                            iced_fonts::lucide::plus(),
                            crate::i18n::t("new_host"),
                            Message::Editor(EditorMessage::ShowNewConnection),
                            secondary,
                        ));
                        col = col.push(self.menu_item(
                            iced_fonts::lucide::ellipsis(),
                            crate::i18n::t("toolbar_more"),
                            Message::Cloud(CloudMessage::ShowCloudProviderPicker),
                            secondary,
                        ));
                    }
                }
                // View cycler: the entry names the NEXT mode (grid ->
                // list -> tree -> grid), opposite convention from the
                // toolbar button, which shows the current one.
                let (icon, label) = match self.prefs.host_view_mode.next() {
                    crate::state::HostViewMode::Grid => (
                        iced_fonts::lucide::layout_grid(),
                        crate::i18n::t("toolbar_view_grid"),
                    ),
                    crate::state::HostViewMode::List => {
                        (iced_fonts::lucide::list(), crate::i18n::t("toolbar_view_list"))
                    }
                    crate::state::HostViewMode::Tree => (
                        iced_fonts::lucide::folder_tree(),
                        crate::i18n::t("toolbar_view_tree"),
                    ),
                };
                col = col.push(self.menu_item(
                    icon,
                    label,
                    Message::Settings(SettingsMessage::CycleHostViewMode),
                    secondary,
                ));
                // The multi-select mode (issue #230), named as the mode it
                // turns on, the same reading the toolbar button's tooltip
                // gives it. It matters more here than inline: at this
                // width the grid is one column wide, which is exactly
                // when a user is scrolling a long list hunting for the
                // handful of hosts they mean to connect together. Gated
                // on the same predicate as the toolbar square: inside a
                // dynamic cloud group the cards are resolved tasks, and a
                // door the toolbar closes must not stay open here.
                if !self.active_group_is_dynamic() {
                    col = col.push(self.menu_item(
                        iced_fonts::lucide::list_checks(),
                        crate::i18n::t("multi_select"),
                        Message::Tabs(TabsMessage::ToggleMultiSelect),
                        secondary,
                    ));
                }
                col = col.push(self.menu_item(
                    iced_fonts::lucide::arrow_down_a_z(),
                    crate::i18n::t("toolbar_sort"),
                    Message::Navigation(NavigationMessage::ToggleSortMenu(SortMenuKind::Hosts)),
                    secondary,
                ));
                if self.host_tag_filter_available() {
                    col = col.push(self.menu_item(
                        iced_fonts::lucide::tag(),
                        crate::i18n::t("host_tag_filter"),
                        Message::Navigation(NavigationMessage::ShowHostTagFilterMenu),
                        secondary,
                    ));
                }
            }
            View::Keys => {
                col = col.push(self.menu_item(
                    iced_fonts::lucide::plus(),
                    crate::i18n::t("add_btn"),
                    Message::Keys(KeysMessage::ToggleKeychainAddMenu),
                    secondary,
                ));
                col = col.push(self.menu_item(
                    iced_fonts::lucide::arrow_down_a_z(),
                    crate::i18n::t("toolbar_sort"),
                    Message::Navigation(NavigationMessage::ToggleSortMenu(SortMenuKind::Keys)),
                    secondary,
                ));
            }
            View::Snippets => {
                if !self.distinct_snippet_tags().is_empty()
                    || !self.cur_nav().snippet_filter_tags.is_empty()
                {
                    col = col.push(self.menu_item(
                        iced_fonts::lucide::tag(),
                        crate::i18n::t("host_tag_filter"),
                        Message::Snippet(SnippetMessage::ShowSnippetTagFilterMenu),
                        secondary,
                    ));
                }
                col = col.push(self.menu_item(
                    iced_fonts::lucide::plus(),
                    crate::i18n::t("new_snippet"),
                    Message::Snippet(SnippetMessage::ShowSnippetPanel),
                    secondary,
                ));
                col = col.push(self.menu_item(
                    iced_fonts::lucide::arrow_down_a_z(),
                    crate::i18n::t("toolbar_sort"),
                    Message::Navigation(NavigationMessage::ToggleSortMenu(SortMenuKind::Snippets)),
                    secondary,
                ));
            }
            View::Cloud => {
                col = col.push(self.menu_item(
                    iced_fonts::lucide::plus(),
                    crate::i18n::t("cloud_new_account"),
                    Message::Cloud(CloudMessage::ShowCloudForm(None)),
                    secondary,
                ));
            }
            View::PortForwarding => {
                col = col.push(self.menu_item(
                    iced_fonts::lucide::plus(),
                    crate::i18n::t("new_port_forward"),
                    Message::PortForward(PortForwardMessage::ShowPortForwardPanel),
                    secondary,
                ));
            }
            View::Proxies => {
                col = col.push(self.menu_item(
                    iced_fonts::lucide::plus(),
                    crate::i18n::t("new_proxy_identity"),
                    Message::ProxyIdentity(ProxyIdentityMessage::ShowProxyIdentityForm(None)),
                    secondary,
                ));
            }
            View::History => {
                if self.history_tag_filter_available() {
                    col = col.push(self.menu_item(
                        iced_fonts::lucide::tag(),
                        crate::i18n::t("host_tag_filter"),
                        Message::History(HistoryMessage::ShowHistoryTagFilterMenu),
                        secondary,
                    ));
                }
                col = col.push(self.menu_item(
                    iced_fonts::lucide::chevron_left(),
                    crate::i18n::t("toolbar_prev"),
                    Message::History(HistoryMessage::LogsPagePrev),
                    secondary,
                ));
                col = col.push(self.menu_item(
                    iced_fonts::lucide::chevron_right(),
                    crate::i18n::t("toolbar_next"),
                    Message::History(HistoryMessage::LogsPageNext),
                    secondary,
                ));
                if !self.logs.is_empty() || !self.session_logs.is_empty() {
                    col = col.push(self.menu_item(
                        iced_fonts::lucide::trash(),
                        crate::i18n::t("clear_all"),
                        Message::History(HistoryMessage::RequestClearHistory),
                        OryxisColors::t().error,
                    ));
                }
            }
            _ => {}
        }
        col.boxed()
    }

    pub(crate) fn build_menu_terminal_context(
        &self,
        pane_id: uuid::Uuid,
        selection: &Option<String>,
    ) -> Element<'_, Message> {
        let mut items = iced::widget::Column::<iced::Element<'_, _>>::new();
        // "Copy" acts on the selection captured at right-click
        // (the app can't read the widget's live selection); shown
        // only when something was selected.
        if let Some(text) = selection {
            items = items.push(self.menu_item_hotkey(
                iced_fonts::lucide::copy(),
                crate::i18n::t("terminal_copy"),
                Message::Terminal(TerminalMessage::TerminalCopySelection(text.clone())),
                OryxisColors::t().text_secondary,
                crate::hotkeys::HotkeyAction::TerminalCopy,
            ));
            // Same gesture as the Ctrl+Shift+X chord. Inside the
            // has-a-selection branch: with nothing selected it would just
            // duplicate the plain "Paste" row below it.
            items = items.push(self.menu_item_hotkey(
                iced_fonts::lucide::clipboard_list(),
                crate::i18n::t("hotkey_terminal_paste_selection"),
                Message::Terminal(TerminalMessage::TerminalPasteSelection(pane_id, text.clone().into())),
                OryxisColors::t().text_secondary,
                crate::hotkeys::HotkeyAction::TerminalPasteSelection,
            ));
        }
        items = items
            .push(self.menu_item(
                iced_fonts::lucide::copy_check(),
                crate::i18n::t("terminal_copy_all"),
                Message::Terminal(TerminalMessage::TerminalCopyAll(pane_id)),
                OryxisColors::t().text_secondary,
            ))
            // Next to "Copy All" because it answers the same question with
            // the opposite scope: the screen as drawn, scroll position
            // included, and nothing off it. Here the pane is necessarily
            // the one under the cursor and necessarily visible, which is
            // what makes the drawn viewport the right thing to export.
            .push(self.menu_item(
                iced_fonts::lucide::clipboard_copy(),
                crate::i18n::t("terminal_copy_screen"),
                Message::Terminal(TerminalMessage::TerminalCopyScreen(pane_id)),
                OryxisColors::t().text_secondary,
            ))
            .push(self.menu_item_hotkey(
                iced_fonts::lucide::clipboard_paste(),
                crate::i18n::t("terminal_paste"),
                Message::Terminal(TerminalMessage::TerminalPasteFromClipboard),
                OryxisColors::t().text_secondary,
                crate::hotkeys::HotkeyAction::TerminalPaste,
            ))
            .push(self.menu_item(
                iced_fonts::lucide::eraser(),
                crate::i18n::t("terminal_clear_scrollback"),
                Message::Terminal(TerminalMessage::TerminalClearScrollback(pane_id)),
                OryxisColors::t().text_secondary,
            ));
        // Close this pane: only offered on split tabs (on a single
        // pane it would just be "close tab", which the tab menu owns).
        // The message carries the right-clicked pane's id: focus and
        // the active tab can change via hotkeys while the menu overlay
        // is open, so a focused-pane close could hit the wrong pane.
        let is_split = self
            .pane_tab_index(pane_id)
            .and_then(|i| self.tabs.get(i))
            .is_some_and(|t| t.pane_grid.panes.len() > 1);
        if is_split {
            // Zoom and rearrange, on the pane the user pointed at. The
            // tab menu carries the same zoom for the FOCUSED pane; here
            // the target is explicit, which is what a right-click on a
            // specific pane means.
            let tab = self.pane_tab_index(pane_id).and_then(|i| self.tabs.get(i));
            let zoomed = tab.is_some_and(|t| t.pane_grid.maximized().is_some());
            let (zoom_glyph, zoom_key) = if zoomed {
                (iced_fonts::lucide::minimize(), "restore_panes")
            } else {
                (iced_fonts::lucide::maximize(), "maximize_pane")
            };
            items = items.push(self.menu_item(
                zoom_glyph,
                crate::i18n::t(zoom_key),
                Message::Terminal(TerminalMessage::ToggleMaximizePaneAt(pane_id)),
                OryxisColors::t().text_secondary,
            ));
            // Rearranging is about a divider, and a zoom is exactly the
            // state where no divider is on screen, so the row is not
            // offered there. The label names the arrangement the click
            // PRODUCES, which is the only way a flip reads without
            // trying it.
            let axis = tab
                .and_then(|t| {
                    t.pane_grid
                        .panes
                        .iter()
                        .find(|(_, p)| p.id == pane_id)
                        .map(|(handle, _)| (t, *handle))
                })
                .and_then(|(t, handle)| t.split_axis_at(handle));
            if !zoomed && let Some(axis) = axis {
                let stacked = matches!(axis, iced::widget::pane_grid::Axis::Horizontal);
                let (glyph, key) = if stacked {
                    (iced_fonts::lucide::columns_two(), "pane_rearrange_side_by_side")
                } else {
                    (iced_fonts::lucide::rows_two(), "pane_rearrange_stacked")
                };
                items = items.push(self.menu_item(
                    glyph,
                    crate::i18n::t(key),
                    Message::Terminal(TerminalMessage::FlipPaneSplit(pane_id)),
                    OryxisColors::t().text_secondary,
                ));
            }
            // Break this pane out into a tab of its own (issue #208
            // item 4). Above Close because it is the non-destructive
            // half of the same idea, "this pane should not be here":
            // one keeps the session, the other ends it.
            items = items.push(self.menu_item(
                iced_fonts::lucide::external_link(),
                crate::i18n::t("pane_to_new_tab"),
                Message::Terminal(TerminalMessage::MovePaneToNewTab(pane_id)),
                OryxisColors::t().text_secondary,
            ));
            items = items.push(self.menu_item(
                iced_fonts::lucide::x(),
                crate::i18n::t("close_pane"),
                Message::Terminal(TerminalMessage::ClosePane(Some(pane_id))),
                OryxisColors::t().text_secondary,
            ));
        }
        items.boxed()
    }

    /// Row count of the pane context menu, for the popover's height.
    ///
    /// Kept NEXT TO the builder, like `split_menu_rows` and
    /// `tab_bar_menu_rows`, because every row above is conditional and a
    /// count that lives in another file drifts from them silently: the
    /// popover is sized from this number, so a row too few clips the
    /// last entry off the window rather than failing loudly.
    pub(crate) fn terminal_context_menu_rows(
        &self,
        pane_id: uuid::Uuid,
        selection: &Option<String>,
    ) -> f32 {
        // A selection adds TWO rows (Copy, then Paste selection), not one.
        let mut rows = if selection.is_some() { 5.0 } else { 3.0 };
        let tab = self.pane_tab_index(pane_id).and_then(|i| self.tabs.get(i));
        let Some(tab) = tab.filter(|t| t.pane_grid.panes.len() > 1) else {
            return rows;
        };
        // Zoom + Close pane, plus the rearrange row when a divider is
        // actually on screen to rearrange.
        rows += 2.0;
        let zoomed = tab.pane_grid.maximized().is_some();
        let has_divider = tab
            .pane_grid
            .panes
            .iter()
            .find(|(_, p)| p.id == pane_id)
            .is_some_and(|(handle, _)| tab.split_axis_at(*handle).is_some());
        if !zoomed && has_divider {
            rows += 1.0;
        }
        rows
    }

    /// Read-only context menu for the session-log transcript viewer
    /// (issue #90, right-click scheme = Menu). Only copy actions apply:
    /// Copy (the selection captured at right-click, shown when there was
    /// one) and Copy All. No Paste / Clear, since the transcript has no
    /// PTY and its scrollback is the immutable recording.
    pub(crate) fn build_menu_session_viewer_context(
        &self,
        selection: &Option<String>,
    ) -> Element<'_, Message> {
        let mut items = iced::widget::Column::<iced::Element<'_, _>>::new();
        if let Some(text) = selection {
            items = items.push(self.menu_item(
                iced_fonts::lucide::copy(),
                crate::i18n::t("terminal_copy"),
                Message::Terminal(TerminalMessage::TerminalCopySelection(text.clone())),
                OryxisColors::t().text_secondary,
            ));
        }
        items = items.push(self.menu_item(
            iced_fonts::lucide::copy_check(),
            crate::i18n::t("terminal_copy_all"),
            Message::History(HistoryMessage::SessionViewerCopyAll),
            OryxisColors::t().text_secondary,
        ));
        items.boxed()
    }

    /// Right-click menu on a Monitor-tab listening-port row (issue #96).
    ///
    /// Forwarding is offered for TCP only, because SSH port forwarding
    /// has no UDP mode; the two kill rows apply to any socket. Both
    /// kills only PARK a confirmation, so this menu never touches the
    /// host on its own.
    pub(crate) fn build_menu_monitor_port(
        &self,
        port: &crate::monitor::model::PortStat,
    ) -> Element<'_, Message> {
        use crate::monitor::kill::KillSignal;
        let c = OryxisColors::t();
        let mut items = iced::widget::Column::<iced::Element<'_, _>>::new();
        if port.proto == "tcp"
            && let Some(conn_id) = self.monitor_pane_connection()
        {
            items = items.push(self.menu_item(
                iced_fonts::lucide::arrow_right_left(),
                crate::i18n::t("monitor_forward_port"),
                Message::Monitor(MonitorMessage::ForwardPort(
                    conn_id,
                    port.port,
                    port.bind.clone(),
                )),
                c.accent,
            ));
        }
        let row = Box::new(port.clone());
        items = items
            .push(self.menu_item(
                iced_fonts::lucide::circle_stop(),
                crate::i18n::t("monitor_kill_process"),
                Message::Monitor(MonitorMessage::AskKillPort(row.clone(), KillSignal::Term)),
                c.text_secondary,
            ))
            .push(self.menu_item(
                iced_fonts::lucide::zap(),
                crate::i18n::t("monitor_force_kill"),
                Message::Monitor(MonitorMessage::AskKillPort(row, KillSignal::Force)),
                c.error,
            ));
        items.boxed()
    }
}

/// How many recent hosts the two tab popovers offer.
///
/// Five, not the tray submenu's ten: this is a shortcut past the picker
/// for the handful of hosts a session revolves around, and a popover
/// long enough to need scanning is a picker with no search field.
const RECENT_MENU_MAX: usize = 5;

/// Longest recent-host label a popover row shows before it is cut.
///
/// The popover is a fixed 210 px (`overlay_menu_width`), and a label is
/// whatever the user named a host, so an uncut one wraps to two or three
/// lines and takes the height estimate with it. The tray has the same
/// list and no such limit because a native submenu sizes itself.
const RECENT_LABEL_MAX: usize = 24;

/// The hairline used to separate a popover's sections.
///
/// The coloured fill must sit on the inner 1 px `Space`, not on the
/// padded container around it, or the breathing room inherits the border
/// colour and the line reads about nine pixels tall.
fn menu_divider() -> Element<'static, Message> {
    container(
        container(Space::new().width(Length::Fill).height(1))
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().border)),
                ..Default::default()
            }),
    )
    .padding(Padding { top: 4.0, right: 4.0, bottom: 4.0, left: 4.0 })
    .boxed()
}

/// Cut a label to [`RECENT_LABEL_MAX`] characters, marking the cut.
/// Counts CHARS rather than bytes: a host labelled in Chinese or Arabic
/// would otherwise be sliced mid-codepoint, which panics.
fn elide_menu_label(label: &str) -> String {
    if label.chars().count() <= RECENT_LABEL_MAX {
        return label.to_string();
    }
    let mut out: String = label.chars().take(RECENT_LABEL_MAX - 1).collect();
    out.push('\u{2026}');
    out
}

/// Height of [`menu_divider`] in `overlay_menu_height`'s row units
/// (1 px line plus 4 px of padding either side, against a 30 px row).
const DIVIDER_ROWS: f32 = 0.3;

#[cfg(test)]
mod tests {
    use super::{elide_menu_label, RECENT_LABEL_MAX};

    #[test]
    fn a_short_label_is_left_alone() {
        assert_eq!(elide_menu_label("web01"), "web01");
        let exact: String = "a".repeat(RECENT_LABEL_MAX);
        assert_eq!(elide_menu_label(&exact), exact);
    }

    #[test]
    fn a_long_label_is_cut_and_says_so() {
        let long: String = "a".repeat(RECENT_LABEL_MAX + 10);
        let cut = elide_menu_label(&long);
        assert_eq!(cut.chars().count(), RECENT_LABEL_MAX);
        assert!(cut.ends_with('\u{2026}'));
    }

    #[test]
    fn a_multibyte_label_is_cut_by_character() {
        // The reason this counts chars: every one of these is three
        // bytes, so a byte slice would land inside a codepoint and
        // panic, on a host labelled the way a good share of this app's
        // users label them.
        let long: String = "\u{670d}".repeat(RECENT_LABEL_MAX + 5);
        let cut = elide_menu_label(&long);
        assert_eq!(cut.chars().count(), RECENT_LABEL_MAX);
        assert!(cut.ends_with('\u{2026}'));
    }
}
