//! Root layout: menus. Split out of views/layout/mod.rs.

use super::*;
use iced::widget::column;
impl Oryxis {
    /// Resolve the on-screen width of an overlay popover. Group
    /// pickers track their associated combo's measured bounds (so
    /// the popover stays the same width as the input it dropdowns
    /// from). Sort menus get a wider fixed slot so long-translated
    /// labels fit. Everything else uses the default kebab width.
    /// Falls back to the kebab width when a combo's bounds cell
    /// hasn't been populated yet (extremely brief, before the first
    /// draw pass on a freshly opened panel).
    pub(crate) fn overlay_menu_width(&self, overlay: &OverlayState) -> f32 {
        match &overlay.content {
            OverlayContent::SortMenu(_) => 220.0,
            // Wide enough for "Split side by side" / "Duplicate in New
            // Window" / "Close Other Tabs" to sit on one line.
            OverlayContent::SplitMenu | OverlayContent::TabActions(_) => 210.0,
            // Fits "Import ~/.ssh/config" / "Export all hosts" and the
            // longer translations of both on one line.
            OverlayContent::CloudProviderPicker => 210.0,
            OverlayContent::CloudDiscoverGroupPicker => {
                let b = self.cloud_discover_default_group_combo_bounds.get();
                if b.width > 0.0 { b.width } else { 308.0 }
            }
            OverlayContent::GroupPicker(target) => {
                let b = match target {
                    crate::state::GroupPickerTarget::DynamicFormParent => {
                        self.dynamic_form_parent_combo_bounds.get()
                    }
                    crate::state::GroupPickerTarget::SessionGroupFolder => {
                        self.session_group_folder_combo_bounds.get()
                    }
                };
                if b.width > 0.0 { b.width } else { 308.0 }
            }
            OverlayContent::ToolbarSearch => self.toolbar_search_width(),
            OverlayContent::ToolbarOverflow => 210.0,
            _ => 150.0,
        }
    }

    pub(crate) fn render_overlay_menu(&self, overlay: &OverlayState) -> Element<'_, Message> {
        // Floating toolbar search: just the live search input at full
        // width, no popover chrome (it reads as the inline field having
        // floated into the bar, not as a dropdown box).
        if matches!(overlay.content, OverlayContent::ToolbarSearch) {
            let w = self.overlay_menu_width(overlay);
            return container(self.vault_search_field())
                .width(Length::Fixed(w))
                .into();
        }
        // Per-variant width. Group pickers track the live combo width
        // measured by their `bounds_reporter` so the popover always
        // matches the input it dropdowns from; sort menu gets a wider
        // fixed slot so long translations fit; everything else falls
        // back to the default kebab width.
        let menu_width = self.overlay_menu_width(overlay);
        let items: Element<'_, Message> = match &overlay.content {
            OverlayContent::HostActions(idx) => {
                let idx = *idx;
                let conn = self.connections.get(idx);
                let cloud_profile_id = conn
                    .and_then(|c| c.cloud_ref.as_ref())
                    .map(|r| r.profile_id);
                let is_orphan = conn
                    .and_then(|c| c.cloud_ref.as_ref())
                    .and_then(|r| r.orphaned_at)
                    .is_some();
                let mut items = column![
                    context_menu_item(iced_fonts::lucide::play(), crate::i18n::t("connect"), Message::ConnectSsh(idx), OryxisColors::t().success),
                    context_menu_item(iced_fonts::lucide::pencil(), crate::i18n::t("edit"), Message::EditConnection(idx), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::copy(), crate::i18n::t("duplicate"), Message::DuplicateConnection(idx), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::share(), crate::i18n::t("share"), Message::ShareConnection(idx), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::folder_tree(), crate::i18n::t("open_sftp_tab"), Message::OpenSftpForConnection(idx), OryxisColors::t().text_secondary),
                ];
                if let Some(pid) = cloud_profile_id {
                    items = items.push(context_menu_item(
                        iced_fonts::lucide::funnel(),
                        crate::i18n::t("host_filter_by_profile"),
                        Message::HostFilterByCloudProfile(Some(pid)),
                        OryxisColors::t().text_secondary,
                    ));
                }
                // Orphan hosts get a "Forget" label (semantically
                // closer to "this resource is gone upstream, drop my
                // local record") instead of the generic "Remove".
                // Same `DeleteConnection` action under the hood.
                let (remove_label, remove_icon) = if is_orphan {
                    (crate::i18n::t("host_orphan_forget"), iced_fonts::lucide::eraser())
                } else {
                    (crate::i18n::t("remove"), iced_fonts::lucide::trash())
                };
                items
                    .push(context_menu_item(remove_icon, remove_label, Message::RequestDeleteConnection(idx), OryxisColors::t().error))
                    .into()
            }
            OverlayContent::SessionGroupActions(idx) => {
                let idx = *idx;
                column![
                    context_menu_item(iced_fonts::lucide::play(), crate::i18n::t("open_session_group"), Message::OpenSessionGroup(idx), OryxisColors::t().success),
                    context_menu_item(iced_fonts::lucide::pencil(), crate::i18n::t("edit"), Message::EditSessionGroup(idx), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::copy(), crate::i18n::t("duplicate"), Message::DuplicateSessionGroup(idx), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::trash(), crate::i18n::t("remove"), Message::RequestDeleteSessionGroup(idx), OryxisColors::t().error),
                ]
                .into()
            }
            OverlayContent::KeyActions(idx) => {
                let idx = *idx;
                column![
                    context_menu_item(iced_fonts::lucide::pencil(), crate::i18n::t("edit"), Message::EditKey(idx), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::trash(), crate::i18n::t("remove"), Message::RequestDeleteKey(idx), OryxisColors::t().error),
                ].into()
            }
            OverlayContent::IdentityActions(idx) => {
                let idx = *idx;
                column![
                    context_menu_item(iced_fonts::lucide::pencil(), crate::i18n::t("edit"), Message::EditIdentity(idx), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::trash(), crate::i18n::t("remove"), Message::RequestDeleteIdentity(idx), OryxisColors::t().error),
                ].into()
            }
            OverlayContent::SnippetActions(idx) => {
                let idx = *idx;
                column![
                    context_menu_item(iced_fonts::lucide::pencil(), crate::i18n::t("edit"), Message::EditSnippet(idx), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::trash(), crate::i18n::t("delete"), Message::RequestDeleteSnippet(idx), OryxisColors::t().error),
                ].into()
            }
            OverlayContent::KeychainAdd => {
                column![
                    context_menu_item(iced_fonts::lucide::key_round(), crate::i18n::t("import_key"), Message::ShowKeyPanel, OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::user(), crate::i18n::t("new_identity"), Message::ShowIdentityPanel, OryxisColors::t().text_secondary),
                ].into()
            }
            OverlayContent::FolderActions(gid) => {
                let gid = *gid;
                // Folders that hold cloud-imported hosts used to hide
                // their rename / delete actions to protect the
                // import-by-label dedupe. The decoupling work in v0.7
                // moved import targets to an explicit picker, so
                // renaming or moving the auto folder no longer breaks
                // anything (worst case the next Auto import creates a
                // sibling). Surface the standard actions instead.
                column![
                    context_menu_item(iced_fonts::lucide::pencil(), crate::i18n::t("edit"), Message::EditGroup(gid), OryxisColors::t().accent),
                    context_menu_item(iced_fonts::lucide::trash(), crate::i18n::t("delete"), Message::StartDeleteFolder(gid), OryxisColors::t().error),
                ].into()
            }
            OverlayContent::DynamicGroupActions(id) => {
                let id = *id;
                column![
                    context_menu_item(iced_fonts::lucide::pencil(), crate::i18n::t("edit"), Message::EditDynamicGroup(id), OryxisColors::t().accent),
                    // Rename = friendly display label only. The
                    // cloud_query (cluster/service/container) and the
                    // import-dedupe key never look at it, so renaming
                    // is safe and the subtitle keeps surfacing the
                    // original ECS path.
                    context_menu_item(iced_fonts::lucide::text_cursor_input(), crate::i18n::t("rename"), Message::StartRenameFolder(id), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::trash(), crate::i18n::t("delete"), Message::DeleteDynamicGroup(id), OryxisColors::t().error),
                ].into()
            }
            OverlayContent::CloudProfileActions(id) => {
                let id = *id;
                column![
                    context_menu_item(iced_fonts::lucide::pencil(), crate::i18n::t("edit"), Message::ShowCloudForm(Some(id)), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::refresh_cw(), crate::i18n::t("cloud_profile_sync"), Message::CloudProfileSync(id), OryxisColors::t().accent),
                    context_menu_item(iced_fonts::lucide::trash(), crate::i18n::t("delete"), Message::DeleteCloudProfile(id), OryxisColors::t().error),
                ].into()
            }
            OverlayContent::CloudProviderPicker => {
                // The "+ Host ▾" add menu. Offers importing a `.oryxis`
                // file (a full vault export or a single shared host),
                // importing an OpenSSH `~/.ssh/config`, exporting the
                // current view, then one entry per configured cloud
                // profile for discovery. Import / export live here so
                // they're reachable from where hosts are managed
                // instead of being buried in Settings.
                let mut items = column![
                    context_menu_item(
                        iced_fonts::lucide::download(),
                        crate::i18n::t("import_from_file"),
                        Message::ImportVault,
                        OryxisColors::t().text_secondary,
                    ),
                    context_menu_item(
                        iced_fonts::lucide::file_code(),
                        crate::i18n::t("import_ssh_config_btn"),
                        Message::ImportSshConfig,
                        OryxisColors::t().text_secondary,
                    ),
                ];
                // Export hosts: opens the share dialog with a per-folder
                // include/exclude checklist (keys-off by default), unlike
                // the full-vault export in Settings. Pre-scoped to the
                // active folder when one is open.
                if !self.connections.is_empty() {
                    items = items.push(context_menu_item(
                        iced_fonts::lucide::upload(),
                        crate::i18n::t("export_hosts"),
                        Message::ShowExportHosts(self.active_group),
                        OryxisColors::t().text_secondary,
                    ));
                }
                // Only profiles whose provider plugin is installed can
                // run discovery; hide the rest (they'd fail with a
                // "binary not found" wall) until the plugin is back.
                for cp in self
                    .cloud_profiles
                    .iter()
                    .filter(|p| self.cloud_provider_installed(&p.provider))
                {
                    let (glyph, brand) = crate::os_icon::provider_icon(
                        &cp.provider,
                        OryxisColors::t().accent,
                    );
                    items = items.push(context_menu_item(
                        glyph,
                        cp.label.as_str(),
                        Message::ShowCloudDiscover(cp.id),
                        brand,
                    ));
                }
                items.into()
            }
            OverlayContent::TabActions(idx) => {
                let idx = *idx;
                let mut items = column![
                    context_menu_item(iced_fonts::lucide::columns_two(), crate::i18n::t("split_side_by_side"), Message::SplitTabPane(idx, iced::widget::pane_grid::Axis::Vertical), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::rows_two(), crate::i18n::t("split_stacked"), Message::SplitTabPane(idx, iced::widget::pane_grid::Axis::Horizontal), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::copy(), crate::i18n::t("duplicate_tab"), Message::DuplicateTab(idx), OryxisColors::t().text_secondary),
                ];
                // Open an SFTP tab for this host: offered when the tab has a
                // live SSH session to reuse or matches a saved connection (so
                // it isn't shown on local-shell tabs where it would no-op).
                let can_sftp = self
                    .tabs
                    .get(idx)
                    .map(|t| {
                        let base = t.label.trim_end_matches(" (disconnected)");
                        t.active().ssh_session.is_some()
                            || self.connections.iter().any(|c| c.label == base)
                    })
                    .unwrap_or(false);
                if can_sftp {
                    items = items.push(context_menu_item(iced_fonts::lucide::folder_tree(), crate::i18n::t("open_sftp_tab"), Message::OpenSftpForTab(idx), OryxisColors::t().text_secondary));
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
                    items = items.push(context_menu_item(iced_fonts::lucide::boxes(), sg_label, Message::ShowSaveSessionGroup(idx), OryxisColors::t().text_secondary));
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
                    items = items.push(context_menu_item(pin_icon, pin_label, Message::ToggleTabPin(idx), OryxisColors::t().text_secondary));
                }
                // "Duplicate in New Window" spawns a fresh process that
                // can only re-open hosts saved in the vault. ECS Exec /
                // kubectl tabs are ephemeral dynamic-group sessions (no
                // saved connection, no uuid to hand the child), flagged
                // by a `relaunch` message, so hide the item there rather
                // than open an empty window.
                let new_window_ok = self
                    .tabs
                    .get(idx)
                    .map(|t| t.relaunch.is_none())
                    .unwrap_or(true);
                if new_window_ok {
                    items = items.push(context_menu_item(iced_fonts::lucide::external_link(), crate::i18n::t("duplicate_new_window"), Message::DuplicateInNewWindow(idx), OryxisColors::t().text_secondary));
                }
                items = items.push(context_menu_item(iced_fonts::lucide::rotate_cw(), crate::i18n::t("reconnect"), Message::ReconnectTab(idx), OryxisColors::t().accent));
                items = items.push(context_menu_item(iced_fonts::lucide::x(), crate::i18n::t("close_tab"), Message::CloseTab(idx), OryxisColors::t().text_secondary));
                items = items.push(context_menu_item(iced_fonts::lucide::x(), crate::i18n::t("close_other_tabs"), Message::CloseOtherTabs(idx), OryxisColors::t().text_secondary));
                items = items.push(context_menu_item(iced_fonts::lucide::x(), crate::i18n::t("close_all_tabs"), Message::CloseAllTabs, OryxisColors::t().error));
                items.into()
            }
            OverlayContent::SftpTabActions(idx) => {
                let idx = *idx;
                let is_pinned = self.sftp_tabs.get(idx).map(|t| t.pinned).unwrap_or(false);
                let (pin_icon, pin_label) = if is_pinned {
                    (iced_fonts::lucide::pin_off(), crate::i18n::t("unpin_tab"))
                } else {
                    (iced_fonts::lucide::pin(), crate::i18n::t("pin_tab"))
                };
                let mut items = column![
                    context_menu_item(iced_fonts::lucide::plus(), crate::i18n::t("new_tab"), Message::NewSftpTab, OryxisColors::t().text_secondary),
                    context_menu_item(pin_icon, pin_label, Message::ToggleSftpTabPin(idx), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::x(), crate::i18n::t("close_tab"), Message::CloseSftpTab(idx), OryxisColors::t().text_secondary),
                ];
                if self.sftp_tabs.len() > 1 {
                    items = items.push(context_menu_item(iced_fonts::lucide::x(), crate::i18n::t("close_other_tabs"), Message::CloseOtherSftpTabs(idx), OryxisColors::t().text_secondary));
                }
                items.into()
            }
            OverlayContent::SplitMenu => {
                let items = column![
                    context_menu_item(iced_fonts::lucide::plus(), crate::i18n::t("new_tab"), Message::ShowNewTabPicker, OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::columns_two(), crate::i18n::t("split_side_by_side"), Message::SplitPane(iced::widget::pane_grid::Axis::Vertical), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::rows_two(), crate::i18n::t("split_stacked"), Message::SplitPane(iced::widget::pane_grid::Axis::Horizontal), OryxisColors::t().text_secondary),
                ];
                // Keep the popover open while the cursor is over it (hover
                // bridge from the `+` button into the menu).
                MouseArea::new(items)
                    .on_enter(Message::SplitMenuEnter)
                    .on_exit(Message::SplitMenuLeave)
                    .into()
            }
            OverlayContent::SortMenu(kind) => {
                let kind = *kind;
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
                // Hairline divider: the colored fill must sit on the
                // inner 1 px Space, not the outer padded container,
                // otherwise the breathing-room padding inherits the
                // border colour and the line reads ~9 px tall.
                let divider: Element<'_, Message> = container(
                    container(Space::new().width(Length::Fill).height(1))
                        .width(Length::Fill)
                        .style(|_| container::Style {
                            background: Some(Background::Color(
                                OryxisColors::t().border,
                            )),
                            ..Default::default()
                        }),
                )
                .padding(Padding {
                    top: 4.0,
                    right: 4.0,
                    bottom: 4.0,
                    left: 4.0,
                })
                .into();
                column![
                    crate::widgets::sort_menu_row(
                        kind,
                        ListSort::LabelAsc,
                        iced_fonts::lucide::arrow_down_a_z::<iced::Theme, iced::Renderer>(),
                        "sort_label_asc",
                        current == ListSort::LabelAsc,
                    ),
                    crate::widgets::sort_menu_row(
                        kind,
                        ListSort::LabelDesc,
                        iced_fonts::lucide::arrow_down_z_a::<iced::Theme, iced::Renderer>(),
                        "sort_label_desc",
                        current == ListSort::LabelDesc,
                    ),
                    divider,
                    crate::widgets::sort_menu_row(
                        kind,
                        ListSort::NewestFirst,
                        iced_fonts::lucide::calendar_arrow_down::<iced::Theme, iced::Renderer>(),
                        "sort_newest_first",
                        current == ListSort::NewestFirst,
                    ),
                    crate::widgets::sort_menu_row(
                        kind,
                        ListSort::OldestFirst,
                        iced_fonts::lucide::calendar_arrow_up::<iced::Theme, iced::Renderer>(),
                        "sort_oldest_first",
                        current == ListSort::OldestFirst,
                    ),
                ]
                .into()
            }
            OverlayContent::CloudDiscoverGroupPicker => {
                // Search input + filtered list. The search field is
                // the menu's own filter (independent of the modal's
                // "Import into" input). Picking a row fills the
                // input and closes the menu.
                let picker_needle = self
                    .cloud_discover_default_group_picker_search
                    .trim()
                    .to_lowercase();
                let mut all_groups: Vec<String> = self
                    .groups
                    .iter()
                    .filter(|g| g.cloud_query.is_none())
                    .filter(|g| {
                        picker_needle.is_empty()
                            || g.label.to_lowercase().contains(&picker_needle)
                    })
                    .map(|g| g.label.clone())
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
                    &self.cloud_discover_default_group_picker_search,
                )
                .on_input(
                    Message::CloudDiscoverDefaultGroupPickerSearchChanged,
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
                        icon: palette.text_muted,
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
                    .into()
                } else {
                    // Plain label rows: dropped the leading folder
                    // glyph since every entry is a folder by
                    // definition (the picker only lists groups) and
                    // the icon was just visual noise.
                    let mut items = column![].spacing(2);
                    for label in all_groups {
                        let display = label.clone();
                        items = items.push(
                            iced::widget::button(
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
                                Message::CloudDiscoverDefaultGroupPick(label),
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
                            }),
                        );
                    }
                    iced::widget::scrollable(items)
                        .height(Length::Fixed(220.0))
                        .into()
                };
                column![search_input, Space::new().height(8), list_el]
                    .width(Length::Fixed(menu_content_width))
                    .into()
            }
            OverlayContent::GroupPicker(target) => {
                // Same shape as the Discover modal's group picker
                // (search input + filtered scrollable list) but
                // wired to the shared `group_picker_search` /
                // `GroupPickerPick(target)` messages. Lives at the
                // top-level render path because the side-panel
                // editors don't short-circuit the way the modal
                // does.
                let target = *target;
                let menu_outer_width = self.overlay_menu_width(overlay);
                let menu_content_width = (menu_outer_width - 8.0).max(80.0);
                let needle = self.group_picker_search.trim().to_lowercase();
                let mut all_groups: Vec<String> = self
                    .groups
                    .iter()
                    .filter(|g| g.cloud_query.is_none())
                    .filter(|g| {
                        needle.is_empty()
                            || g.label.to_lowercase().contains(&needle)
                    })
                    .map(|g| g.label.clone())
                    .collect();
                all_groups.sort_by_key(|s| s.to_lowercase());
                all_groups.dedup();
                let search_input = iced::widget::text_input(
                    crate::i18n::t("search_groups"),
                    &self.group_picker_search,
                )
                .on_input(Message::GroupPickerSearchChanged)
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
                        icon: palette.text_muted,
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
                    .into()
                } else {
                    let mut items = column![].spacing(2);
                    for label in all_groups {
                        let display = label.clone();
                        items = items.push(
                            iced::widget::button(
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
                            .on_press(Message::GroupPickerPick(target, label))
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
                            }),
                        );
                    }
                    iced::widget::scrollable(items)
                        .height(Length::Fixed(220.0))
                        .into()
                };
                column![search_input, Space::new().height(8), list_el]
                    .width(Length::Fixed(menu_content_width))
                    .into()
            }
            // Rendered above via early return (no popover chrome).
            OverlayContent::ToolbarSearch => Space::new().into(),
            OverlayContent::ToolbarOverflow => {
                // The `…` menu holds *every* toolbar action for the view
                // (primary + secondary), so the narrow toolbar shows only
                // the search icon + this one button.
                use crate::state::{SortMenuKind, View};
                let secondary = OryxisColors::t().text_secondary;
                let mut col = column![].spacing(2);
                match self.active_view {
                    View::Dashboard => {
                        // Primary add action mirrors the toolbar's
                        // context-aware button (none in a dynamic group,
                        // Discover in a cloud folder, else New host + the
                        // import/cloud sub-menu).
                        match self.active_group {
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
                                    col = col.push(context_menu_item(
                                        iced_fonts::lucide::download(),
                                        crate::i18n::t("cloud_discover"),
                                        Message::ShowCloudDiscover(pid),
                                        secondary,
                                    ));
                                } else {
                                    col = col.push(context_menu_item(
                                        iced_fonts::lucide::plus(),
                                        crate::i18n::t("new_host"),
                                        Message::ShowNewConnection,
                                        secondary,
                                    ));
                                    col = col.push(context_menu_item(
                                        iced_fonts::lucide::ellipsis(),
                                        crate::i18n::t("toolbar_more"),
                                        Message::ShowCloudProviderPicker,
                                        secondary,
                                    ));
                                }
                            }
                            None => {
                                col = col.push(context_menu_item(
                                    iced_fonts::lucide::plus(),
                                    crate::i18n::t("new_host"),
                                    Message::ShowNewConnection,
                                    secondary,
                                ));
                                col = col.push(context_menu_item(
                                    iced_fonts::lucide::ellipsis(),
                                    crate::i18n::t("toolbar_more"),
                                    Message::ShowCloudProviderPicker,
                                    secondary,
                                ));
                            }
                        }
                        // View toggle (grid <-> list) only when the grid
                        // shows more than one column.
                        let (icon, label) = if self.setting_host_list_view {
                            (
                                iced_fonts::lucide::layout_grid(),
                                crate::i18n::t("toolbar_view_grid"),
                            )
                        } else {
                            (iced_fonts::lucide::list(), crate::i18n::t("toolbar_view_list"))
                        };
                        col = col.push(context_menu_item(
                            icon,
                            label,
                            Message::ToggleHostListView,
                            secondary,
                        ));
                        col = col.push(context_menu_item(
                            iced_fonts::lucide::arrow_down_a_z(),
                            crate::i18n::t("toolbar_sort"),
                            Message::ToggleSortMenu(SortMenuKind::Hosts),
                            secondary,
                        ));
                    }
                    View::Keys => {
                        col = col.push(context_menu_item(
                            iced_fonts::lucide::plus(),
                            crate::i18n::t("add_btn"),
                            Message::ToggleKeychainAddMenu,
                            secondary,
                        ));
                        col = col.push(context_menu_item(
                            iced_fonts::lucide::arrow_down_a_z(),
                            crate::i18n::t("toolbar_sort"),
                            Message::ToggleSortMenu(SortMenuKind::Keys),
                            secondary,
                        ));
                    }
                    View::Snippets => {
                        col = col.push(context_menu_item(
                            iced_fonts::lucide::plus(),
                            crate::i18n::t("new_snippet"),
                            Message::ShowSnippetPanel,
                            secondary,
                        ));
                        col = col.push(context_menu_item(
                            iced_fonts::lucide::arrow_down_a_z(),
                            crate::i18n::t("toolbar_sort"),
                            Message::ToggleSortMenu(SortMenuKind::Snippets),
                            secondary,
                        ));
                    }
                    View::Cloud => {
                        col = col.push(context_menu_item(
                            iced_fonts::lucide::plus(),
                            crate::i18n::t("cloud_new_account"),
                            Message::ShowCloudForm(None),
                            secondary,
                        ));
                    }
                    View::PortForwarding => {
                        col = col.push(context_menu_item(
                            iced_fonts::lucide::plus(),
                            crate::i18n::t("new_port_forward"),
                            Message::ShowPortForwardPanel,
                            secondary,
                        ));
                    }
                    View::Proxies => {
                        col = col.push(context_menu_item(
                            iced_fonts::lucide::plus(),
                            crate::i18n::t("new_proxy_identity"),
                            Message::ShowProxyIdentityForm(None),
                            secondary,
                        ));
                    }
                    View::History => {
                        col = col.push(context_menu_item(
                            iced_fonts::lucide::chevron_left(),
                            crate::i18n::t("toolbar_prev"),
                            Message::LogsPagePrev,
                            secondary,
                        ));
                        col = col.push(context_menu_item(
                            iced_fonts::lucide::chevron_right(),
                            crate::i18n::t("toolbar_next"),
                            Message::LogsPageNext,
                            secondary,
                        ));
                        if !self.logs.is_empty() || !self.session_logs.is_empty() {
                            col = col.push(context_menu_item(
                                iced_fonts::lucide::trash(),
                                crate::i18n::t("clear_all"),
                                Message::RequestClearHistory,
                                OryxisColors::t().error,
                            ));
                        }
                    }
                    _ => {}
                }
                col.into()
            }
        };

        // Min-height (so a single-item menu reads as a real button-
        // height drop-down, not a sliver). Iced 0.13 has no
        // `min_height`, the previous Stack-based workaround
        // collapsed items to zero width in this fork, and stuffing a
        // fixed-height Space inside the column inflates multi-item
        // menus by the spacer height. Compromise: render items in
        // an outer container with a tight vertical padding that
        // approximates the spilt-button height for small menus
        // while letting tall menus grow naturally.
        const SINGLE_ROW_MIN_PAD: f32 = 6.0;
        container(items)
            .width(menu_width)
            .padding(Padding {
                top: SINGLE_ROW_MIN_PAD,
                right: 4.0,
                bottom: SINGLE_ROW_MIN_PAD,
                left: 4.0,
            })
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    radius: Radius::from(8.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                shadow: iced::Shadow {
                    color: Color::from_rgba(0.0, 0.0, 0.0, 0.3),
                    offset: iced::Vector::new(0.0, 4.0),
                    blur_radius: 12.0,
                },
                ..Default::default()
            })
            .into()
    }

    /// Overflow ("…") dropdown for the vault sub-nav: the destinations
    /// that didn't fit inline. Backdrop + pinned panel, like the burger
    /// menu; anchored under the "…" trigger via an estimated x offset.
    pub(crate) fn view_subnav_overflow_menu(&self) -> Element<'_, Message> {
        let (inline, overflow) = self.subnav_pill_split();
        let mut col = iced::widget::Column::new().width(Length::Fill).spacing(1);
        for (k, v) in overflow {
            let active = self.active_view == v;
            let fg = if active {
                OryxisColors::t().accent
            } else {
                OryxisColors::t().text_primary
            };
            let item = button(
                container(text(crate::i18n::t(k)).size(13).color(fg))
                    .width(Length::Fill)
                    .align_x(dir_align_x())
                    .padding(Padding { top: 7.0, right: 12.0, bottom: 7.0, left: 12.0 }),
            )
            .width(Length::Fill)
            .on_press(Message::ChangeView(v))
            .style(move |_, status| {
                let bg = if matches!(status, iced::widget::button::Status::Hovered) {
                    OryxisColors::t().bg_hover
                } else if active {
                    Color { a: 0.12, ..OryxisColors::t().accent }
                } else {
                    Color::TRANSPARENT
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                }
            });
            col = col.push(item);
        }
        let panel = container(col)
            .width(Length::Fixed(200.0))
            .padding(Padding { top: 6.0, right: 6.0, bottom: 6.0, left: 6.0 })
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                border: Border {
                    radius: Radius::from(8.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            });
        // Estimated x of the "…" trigger: row left padding + chip + gap
        // + the inline pills. Lands the dropdown just under the cue. The
        // chip is only present when the vault switcher shows (must match
        // `subnav_pill_split`), otherwise the menu lands ~115 px too far
        // right and clips off the window edge.
        let chip = if self.show_vault_switcher() { 115.0 + 8.0 } else { 0.0 };
        let inline_w: f32 = inline
            .iter()
            .map(|(k, _)| Self::subnav_pill_width(k))
            .sum();
        // Clamp so the 200 px panel never runs past the right edge.
        let dots_x = (8.0 + chip + inline_w).min((self.window_size.width - 206.0).max(0.0));
        let pinned = container(panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Left)
            .align_y(iced::alignment::Vertical::Top)
            .padding(Padding {
                top: 78.0,
                right: 0.0,
                bottom: 0.0,
                left: dots_x,
            });
        let backdrop: Element<'_, Message> = MouseArea::new(
            container(Space::new().width(Length::Fill).height(Length::Fill))
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(Message::ToggleSubnavOverflow)
        .into();
        Stack::new()
            .push(backdrop)
            .push(pinned)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Burger menu overlay anchored to the top-left of the window.
    /// Pairs with the `☰` trigger in the tab bar. A transparent
    /// MouseArea backdrop catches outside clicks to dismiss; the
    /// menu items themselves stop propagation by living inside their
    /// own button widgets.
    pub(crate) fn view_burger_menu(&self) -> Element<'_, Message> {
        // Menu row: label on the leading edge, optional muted hotkey
        // hint on the trailing edge (Termius-style "Ctrl+1" tail).
        // Items dispatch the same Messages the existing sidebar /
        // status bar use, so we don't have to introduce new flows.
        let item = |label: &'static str, msg: Message, shortcut: Option<String>| -> Element<'_, Message> {
            let label_el: Element<'_, Message> = text(crate::i18n::t(label))
                .size(13)
                .color(OryxisColors::t().text_primary)
                .into();
            let inner: Element<'_, Message> = if let Some(s) = shortcut {
                let shortcut_el: Element<'_, Message> = text(s)
                    .size(11)
                    .color(OryxisColors::t().text_muted)
                    .into();
                dir_row(vec![
                    label_el,
                    Space::new().width(Length::Fill).into(),
                    shortcut_el,
                ])
                .align_y(iced::Alignment::Center)
                .into()
            } else {
                label_el
            };
            button(
                container(inner)
                    .padding(Padding {
                        top: 8.0,
                        right: 16.0,
                        bottom: 8.0,
                        left: 16.0,
                    })
                    .width(Length::Fill)
                    .align_x(dir_align_x()),
            )
            .on_press(msg)
            .width(Length::Fill)
            .style(|_, status| {
                let bg = match status {
                    iced::widget::button::Status::Hovered => OryxisColors::t().bg_hover,
                    iced::widget::button::Status::Pressed => OryxisColors::t().bg_selected,
                    _ => Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                }
            })
            .into()
        };
        // Resolve hotkey hints from the live bindings so user
        // overrides flow through to the menu without rebuilds.
        let hk_settings = self.hotkey_label_for_action(crate::hotkeys::HotkeyAction::OpenSettings);
        let hk_local_shell = self.hotkey_label_for_action(crate::hotkeys::HotkeyAction::OpenLocalShell);
        let hk_new_window = self.hotkey_label_for_action(crate::hotkeys::HotkeyAction::NewWindow);
        // Hosts / SFTP carry the Ctrl+1 / Ctrl+2 hints since the strip
        // always renders them as area tabs.
        let hk_hosts = self.hotkey_label_for_strip_slot(0);
        // SFTP is no longer a fixed strip slot; the menu item opens a new SFTP
        // tab, so show the dedicated OpenSftp shortcut instead.
        let hk_sftp = if self.sftp_enabled {
            self.hotkey_label_for_action(crate::hotkeys::HotkeyAction::OpenSftp)
        } else {
            None
        };
        // Visual separator between item groups: a 1 px hairline with
        // some breathing room above and below. The previous version
        // applied the border color to the outer container *and* its
        // padding, which rendered as a chunky colored bar instead of
        // a thin divider. Wrap the colored hairline in a transparent
        // outer container so only the inner 1 px takes the color.
        let sep: Element<'_, Message> = iced::widget::column![
            Space::new().height(6),
            container(Space::new().width(Length::Fill).height(1))
                .width(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().border)),
                    ..Default::default()
                }),
            Space::new().height(6),
        ]
        .width(Length::Fill)
        .into();
        // Mirror every sidebar nav entry here so Workspace mode
        // (where the sidebar is gone) still exposes the full set of
        // vault surfaces. The SFTP entry is gated on `sftp_enabled`,
        // same rule the sidebar applies.
        let sftp_item: Element<'_, Message> = if self.sftp_enabled {
            // SFTP is a tab now: the menu opens a fresh SFTP browser tab.
            item("sftp", Message::NewSftpTab, hk_sftp)
        } else {
            Space::new().height(0).into()
        };
        // Lock Vault only when a master password is set; without one,
        // locking has nothing to protect and the unlock screen has no
        // way to re-enter (mirrors the Settings -> Security gating).
        let lock_item: Element<'_, Message> = if self.vault_ui.has_user_password {
            item("lock_vault", Message::LockVault, None)
        } else {
            Space::new().height(0).into()
        };
        // "VAULT" section header + indented children: the flat list
        // read as if Hosts/Keychain/... sat outside the Vault (issue
        // #38 review feedback); mirroring the top strip's Vault tab
        // here keeps one mental model. Indentation goes through
        // dir_row so it flips under RTL.
        let section = |label: &'static str| -> Element<'_, Message> {
            container(
                text(crate::i18n::t(label).to_uppercase())
                    .size(10)
                    .font(iced::Font {
                        weight: iced::font::Weight::Semibold,
                        ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                    })
                    .color(OryxisColors::t().text_muted),
            )
            .padding(Padding { top: 8.0, right: 16.0, bottom: 2.0, left: 16.0 })
            .width(Length::Fill)
            .align_x(dir_align_x())
            .into()
        };
        pub(crate) fn indent(inner: Element<'_, Message>) -> Element<'_, Message> {
            dir_row(vec![Space::new().width(10).into(), inner]).into()
        }
        let menu_col = column![
            section("vault"),
            indent(item("hosts", Message::ChangeView(View::Dashboard), hk_hosts)),
            indent(item("keychain", Message::ChangeView(View::Keys), None)),
            indent(item("snippets", Message::ChangeView(View::Snippets), None)),
            indent(item(
                "port_forwards",
                Message::ChangeView(View::PortForwarding),
                None
            )),
            if self.logs_surface_visible() {
                indent(item("logs", Message::ChangeView(View::History), None))
            } else {
                Space::new().height(0).into()
            },
            indent(item("cloud_accounts", Message::ChangeView(View::Cloud), None)),
            indent(item("proxies", Message::ChangeView(View::Proxies), None)),
            indent(item("known_hosts", Message::ChangeView(View::KnownHosts), None)),
            Space::new().height(4),
            sftp_item,
            item("settings", Message::ChangeView(View::Settings), hk_settings),
            sep,
            item("local_shell", Message::OpenLocalShell, hk_local_shell),
            item("new_window", Message::SpawnNewWindow, hk_new_window),
            item("check_for_updates_now", Message::CheckForUpdateManual, None),
            lock_item,
        ]
        .width(Length::Fill);
        let menu_panel = container(menu_col)
            .width(Length::Fixed(240.0))
            .padding(Padding { top: 6.0, right: 6.0, bottom: 6.0, left: 6.0 })
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                border: Border {
                    radius: Radius::from(8.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            });
        // Pin the panel to the top-left, just below the tab bar
        // (40 px tall). dir_align_x flips the anchor side under RTL
        // so the dropdown lands under its trigger.
        let pinned = container(menu_panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(dir_align_x())
            .align_y(iced::alignment::Vertical::Top)
            .padding(Padding {
                top: 44.0,
                right: 0.0,
                bottom: 0.0,
                left: 6.0,
            });
        // Backdrop catches outside clicks. Z-stack: backdrop on the
        // bottom, panel on top so the panel's buttons still receive
        // their own clicks.
        let backdrop: Element<'_, Message> = MouseArea::new(
            container(Space::new().width(Length::Fill).height(Length::Fill))
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(Message::ToggleBurgerMenu)
        .into();
        Stack::new()
            .push(backdrop)
            .push(pinned)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}
