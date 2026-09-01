//! Root layout: menus. Split out of views/layout/mod.rs.

use super::*;
use iced::widget::column;

/// Min-height compromise for the popover chrome (see
/// `render_overlay_menu`), at module scope because the password
/// popup's exact height has to add the same padding it is drawn with.
pub(super) const MENU_CHROME_PAD_V: f32 = 6.0;

/// Top edge of a popover of `menu_height` hanging from `anchor_y`, in a
/// window `window_h` tall.
///
/// It stays below the anchor whenever it fits there. When it does not
/// and the caller supplied `flip_pivot` (the TOP edge of the thing the
/// popover hangs from), the box flips over that edge instead. Sliding
/// it up until its bottom meets the window's, which is all a clamp can
/// do, would park it over what it points at: the password popup hangs
/// from the terminal caret, and a shell prompt sits on the LAST row, so
/// "no room below" is the ordinary case there, not an edge one.
///
/// When neither side has room the clamp still wins: a popover that fits
/// nowhere is better half-covered than half off-screen.
fn popover_y(anchor_y: f32, menu_height: f32, window_h: f32, flip_pivot: Option<f32>) -> f32 {
    let max_y = window_h - menu_height;
    if anchor_y > max_y
        && let Some(pivot) = flip_pivot
    {
        let above = pivot - menu_height - crate::dispatch_password_suggest::CARET_GAP;
        if above >= 0.0 {
            return above;
        }
    }
    anchor_y.min(max_y).max(0.0)
}

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
            OverlayContent::SplitMenu
            | OverlayContent::TabActions(_)
            | OverlayContent::TabBarActions => 210.0,
            // Fits "Import ~/.ssh/config" / "Export all hosts" and the
            // longer translations of both on one line.
            OverlayContent::CloudProviderPicker => 210.0,
            // "Export transcript (.txt)" + translations on one line,
            // with room for the privacy footer to wrap sanely.
            OverlayContent::SessionLogActions(_) => 240.0,
            OverlayContent::ChatConversationActions(_) => 200.0,
            OverlayContent::SessionLogViewerActions(_) => 240.0,
            // "Open SFTP session here" + translations on one line.
            OverlayContent::SidebarFilesRow { .. }
            | OverlayContent::SidebarFilesBackground { .. } => 220.0,
            OverlayContent::HostTagFilter
            | OverlayContent::SnippetTagFilter
            | OverlayContent::HistoryTagFilter => 200.0,
            // "Forward this port locally" + translations on one line.
            OverlayContent::MonitorPortActions(_) => 220.0,
            // "Exposed to agent" / "Remove certificate" must not wrap.
            OverlayContent::KeyActions(_) => 210.0,
            // "Check for updates" / "Remove downloaded files" +
            // translations on one line.
            OverlayContent::PluginActions(_) => 230.0,
            OverlayContent::CloudDiscoverGroupPicker => {
                let b = self.cloud_discover.default_group_combo_bounds.get();
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
                    crate::state::GroupPickerTarget::GroupEditParent => {
                        self.group_edit_parent_combo_bounds.get()
                    }
                };
                if b.width > 0.0 { b.width } else { 308.0 }
            }
            OverlayContent::ToolbarSearch => self.toolbar_search_width(),
            OverlayContent::ToolbarOverflow => 210.0,
            // Host labels and identity names on one line, plus the
            // "Enter to send" hint under them.
            OverlayContent::PasswordSuggest { .. } => 260.0,
            _ => 150.0,
        }
    }

    /// Rough on-screen height of an overlay popover, used by `view_main`
    /// to clamp the anchor so the menu never clips past the bottom edge.
    /// Item counts are safe upper bounds per variant (over-estimating
    /// only nudges the menu a little higher); the old flat 80 px guess
    /// was fine while every menu opened from the top bar, but the
    /// bottom-docked tab strip anchors its (tall) context menu near the
    /// window's bottom edge where the real height matters.
    pub(crate) fn overlay_menu_height(&self, overlay: &OverlayState) -> f32 {
        const ITEM_H: f32 = 30.0;
        // The one variant that is measured rather than estimated: its
        // rows are two lines tall only when the credential carries a
        // username, the list is capped at a share of the window, and
        // the number decides a flip, not just a nudge.
        if let OverlayContent::PasswordSuggest { entries, .. } = &overlay.content {
            return super::menu_password_suggest::password_suggest_layout(
                entries,
                self.window_size.height,
            )
            .total;
        }
        let items: f32 = match &overlay.content {
            // A flat count, unlike the host menus below, because this
            // one's conditional entries have always been budgeted for
            // rather than counted. The console row (issue #188) fits in
            // the slack that leaves; when the next entry does not, this
            // wants the `*_menu_rows` treatment its neighbours got.
            OverlayContent::TabActions(_) => 13.0,
            OverlayContent::HostTagFilter | OverlayContent::HistoryTagFilter => {
                (self.distinct_host_tags().len() + 1) as f32
            }
            OverlayContent::SnippetTagFilter => (self.distinct_snippet_tags().len() + 1) as f32,
            OverlayContent::SessionLogActions(_) => 4.0,
            OverlayContent::ChatConversationActions(_) => 4.0,
            OverlayContent::SessionLogViewerActions(_) => 4.0,
            OverlayContent::SftpTabActions(_) => 6.0,
            OverlayContent::SidebarFilesRow { is_dir, .. } => {
                // The local browser's menu (issue #145) swaps the
                // transfer-shaped items for OS ones; counted next to
                // the builder (`build_menu_sidebar_files_row`).
                match (self.sidebar_files_is_local(), *is_dir) {
                    (true, true) => 5.0,
                    (true, false) => 6.0,
                    (false, true) => 7.0,
                    (false, false) => 8.0,
                }
            }
            OverlayContent::SidebarFilesBackground { .. } => {
                if self.sidebar_files_is_local() { 4.0 } else { 5.0 }
            }
            // Kill + Force kill, plus Forward on a TCP row.
            OverlayContent::MonitorPortActions(p) => {
                if p.proto == "tcp" { 3.0 } else { 2.0 }
            }
            // Counted next to the builder (`host_actions_menu_rows`)
            // so the height follows the conditional entries exactly.
            OverlayContent::HostActions(id) => self.host_actions_menu_rows(*id, true),
            // The card menu minus Remove / filter-by-profile.
            OverlayContent::TreeHostActions(id) => self.host_actions_menu_rows(*id, false),
            OverlayContent::PluginActions(_) => 3.0,
            OverlayContent::SessionGroupActions(_) => 4.0,
            OverlayContent::FolderActions(_) => 4.0,
            // Both counted next to their builders, whose rows are
            // conditional (`split_menu_rows` / `tab_bar_menu_rows`).
            OverlayContent::SplitMenu => self.split_menu_rows(),
            OverlayContent::TabBarActions => self.tab_bar_menu_rows(),
            // Counted next to its builder too, for the same reason as
            // the two above: its rows are conditional.
            OverlayContent::TerminalContextMenu(pane_id, sel) => {
                self.terminal_context_menu_rows(*pane_id, sel)
            }
            OverlayContent::SessionLogViewerContext(sel) => {
                // Copy (only with a selection) + Copy All.
                if sel.is_some() { 2.0 } else { 1.0 }
            }
            _ => 2.5,
        };
        items * ITEM_H + 10.0
    }

    /// Where the top edge of an overlay popover goes: [`popover_y`]
    /// fed with the anchor's flip pivot, which only the caret-anchored
    /// password popup publishes (every other menu hangs off a widget
    /// whose height this layer never learns).
    pub(crate) fn overlay_menu_y(&self, overlay: &OverlayState, menu_height: f32) -> f32 {
        let pivot = match &overlay.content {
            OverlayContent::PasswordSuggest { caret_top, .. } => Some(*caret_top),
            _ => None,
        };
        popover_y(
            overlay.y,
            menu_height,
            self.window_size.height,
            pivot,
        )
    }

    /// Multi-select tag-filter dropdown shared by the Hosts and
    /// Snippets toolbars: "All tags" (clears, closes) then one
    /// toggleable row per distinct tag; the menu stays open on a
    /// toggle so several tags land in one visit (the backdrop closes
    /// it). Active rows read in accent with a check glyph. Labels are
    /// user data (owned Strings), so rows are built inline instead of
    /// via `menu_item`; each is still recorded into the modal keyboard
    /// layer.
    fn tag_filter_menu(
        &self,
        selected: Vec<String>,
        all_tags: Vec<String>,
        mk_toggle: fn(String) -> Message,
        clear: Message,
    ) -> Element<'_, Message> {
        let tag_row = |label: String, active: bool, msg: Message| -> Element<'_, Message> {
            let row = button(
                container(
                    dir_row(vec![
                        if active {
                            iced_fonts::lucide::check()
                                .size(14)
                                .color(OryxisColors::t().accent)
                                .into()
                        } else {
                            iced_fonts::lucide::tag()
                                .size(14)
                                .color(OryxisColors::t().text_secondary)
                                .into()
                        },
                        Space::new().width(8).into(),
                        text(label)
                            .size(12)
                            .color(if active {
                                OryxisColors::t().accent
                            } else {
                                OryxisColors::t().text_primary
                            })
                            .into(),
                    ])
                    .align_y(iced::Alignment::Center),
                )
                .width(Length::Fill)
                .align_x(dir_align_x()),
            )
            .on_press(msg.clone())
            .width(Length::Fill)
            .padding(Padding { top: 6.0, right: 12.0, bottom: 6.0, left: 12.0 })
            .style(|_, status| {
                let bg = match status {
                    iced::widget::button::Status::Hovered => OryxisColors::t().bg_hover,
                    _ => Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(4.0), ..Default::default() },
                    ..Default::default()
                }
            })
            .into();
            self.modal_nav_slot(crate::keynav::RowAction::activate(msg), 4.0, false, row)
        };
        let mut col = column![tag_row(
            crate::i18n::t("all_tags").to_string(),
            selected.is_empty(),
            clear,
        )]
        .spacing(2);
        for tg in all_tags {
            let active = selected.iter().any(|f| f.eq_ignore_ascii_case(&tg));
            col = col.push(tag_row(tg.clone(), active, mk_toggle(tg)));
        }
        col.into()
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
        // Keyboard rows are recorded by `menu_item` / `sort_row` / the
        // picker rows below, in render order. Plain row menus open
        // with their first row selected (one Enter fires it); the
        // search-driven group pickers start with no selection so
        // typing stays primary; the split popover is hover-only.
        self.modal_nav_reset();
        if !matches!(
            overlay.content,
            OverlayContent::GroupPicker(_)
                | OverlayContent::CloudDiscoverGroupPicker
                | OverlayContent::SplitMenu
        ) {
            self.keynav.modal.default.set(Some(0));
        }
        // Per-variant width. Group pickers track the live combo width
        // measured by their `bounds_reporter` so the popover always
        // matches the input it dropdowns from; sort menu gets a wider
        // fixed slot so long translations fit; everything else falls
        // back to the default kebab width.
        let menu_width = self.overlay_menu_width(overlay);
        // Each variant delegates to a `build_menu_*` builder (moved
        // verbatim into menu_card.rs / menu_vault.rs to keep this file
        // and the builders under the size limit). One arm runs per
        // call, so keynav slot-recording order is preserved by
        // construction. The tag-filter arms stay inline delegating to
        // the still-private `tag_filter_menu`.
        let items: Element<'_, Message> = match &overlay.content {
            OverlayContent::HostTagFilter => self.tag_filter_menu(
                self.host_filter_tags.clone(),
                self.distinct_host_tags(),
                |v| Message::Navigation(NavigationMessage::ToggleHostTagFilterTag(v)),
                Message::Navigation(NavigationMessage::ClearHostTagFilter),
            ),
            OverlayContent::SnippetTagFilter => self.tag_filter_menu(
                self.snippet_filter_tags.clone(),
                self.distinct_snippet_tags(),
                |v| Message::Snippet(SnippetMessage::ToggleSnippetTagFilterTag(v)),
                Message::Snippet(SnippetMessage::ClearSnippetTagFilter),
            ),
            // The History filter reuses the host tags: timeline rows
            // resolve to connections, so the tag universe is the same.
            OverlayContent::HistoryTagFilter => self.tag_filter_menu(
                self.history_filter_tags.clone(),
                self.distinct_host_tags(),
                |v| Message::History(HistoryMessage::ToggleHistoryTagFilterTag(v)),
                Message::History(HistoryMessage::ClearHistoryTagFilter),
            ),
            OverlayContent::SessionLogActions(idx) => self.build_menu_session_log_actions(*idx),
            OverlayContent::ChatConversationActions(idx) => {
                self.build_menu_chat_conversation_actions(*idx)
            }
            OverlayContent::SessionLogViewerActions(idx) => {
                self.build_menu_session_log_viewer_actions(*idx)
            }
            OverlayContent::HostActions(id) => self.build_menu_host_actions(*id),
            OverlayContent::TreeHostActions(id) => self.build_menu_tree_host_actions(*id),
            OverlayContent::SessionGroupActions(idx) => self.build_menu_session_group_actions(*idx),
            OverlayContent::KeyActions(idx) => self.build_menu_key_actions(*idx),
            OverlayContent::PluginActions(id) => self.build_menu_plugin_actions(id),
            OverlayContent::IdentityActions(idx) => self.build_menu_identity_actions(*idx),
            OverlayContent::SnippetActions(idx) => self.build_menu_snippet_actions(*idx),
            OverlayContent::PortForwardActions(idx) => self.build_menu_port_forward_actions(*idx),
            OverlayContent::KeychainAdd => self.build_menu_keychain_add(),
            OverlayContent::FolderActions(gid) => self.build_menu_folder_actions(*gid),
            OverlayContent::DynamicGroupActions(id) => self.build_menu_dynamic_group_actions(*id),
            OverlayContent::CloudProfileActions(id) => self.build_menu_cloud_profile_actions(*id),
            OverlayContent::CloudProviderPicker => self.build_menu_cloud_provider_picker(),
            OverlayContent::SidebarFilesRow { path, is_dir } => {
                self.build_menu_sidebar_files_row(path.clone(), *is_dir)
            }
            OverlayContent::SidebarFilesBackground { dir } => {
                self.build_menu_sidebar_files_background(dir.clone())
            }
            OverlayContent::TabActions(idx) => self.build_menu_tab_actions(*idx),
            OverlayContent::SftpTabActions(idx) => self.build_menu_sftp_tab_actions(*idx),
            OverlayContent::SplitMenu => self.build_menu_split(),
            OverlayContent::TabBarActions => self.build_menu_tab_bar_actions(),
            OverlayContent::SortMenu(kind) => self.build_menu_sort(*kind),
            OverlayContent::CloudDiscoverGroupPicker => {
                self.build_menu_cloud_discover_group_picker(overlay)
            }
            OverlayContent::GroupPicker(target) => self.build_menu_group_picker(overlay, *target),
            // Rendered above via early return (no popover chrome).
            OverlayContent::ToolbarSearch => Space::new().into(),
            OverlayContent::ToolbarOverflow => self.build_menu_toolbar_overflow(),
            OverlayContent::TerminalContextMenu(pane_id, selection) => {
                self.build_menu_terminal_context(*pane_id, selection)
            }
            OverlayContent::SessionLogViewerContext(selection) => {
                self.build_menu_session_viewer_context(selection)
            }
            OverlayContent::MonitorPortActions(port) => self.build_menu_monitor_port(port),
            OverlayContent::PasswordSuggest {
                entries, selected, ..
            } => self.build_menu_password_suggest(entries, *selected),
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
        // The keyboard router auto-opens this menu when the sub-nav
        // highlight walks into an overflowed destination; render that
        // row with the hover background so the selection stays visible.
        let kb_sel = match self.keynav.selected_in(crate::keynav::FocusZone::SubNav) {
            Some(crate::keynav::NavItem::SubNav(v)) => Some(v),
            _ => None,
        };
        let mut col = iced::widget::Column::new().width(Length::Fill).spacing(1);
        for (k, v) in overflow {
            let active = self.active_view == v;
            let kb = kb_sel == Some(v);
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
            .on_press(Message::Navigation(NavigationMessage::ChangeView(v)))
            .style(move |_, status| {
                let bg = if kb || matches!(status, iced::widget::button::Status::Hovered) {
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
        // Window-space anchor: a left-docked tab strip shifts the whole
        // sub-nav right; a right dock pulls the clamp edge in. With the
        // side dock's hidden top bar the row also sits 41 px higher.
        let strip_left = self.side_strip_left_offset();
        let strip_right = self.side_strip_reserve() - strip_left;
        // Clamp so the 200 px panel never runs past the right edge.
        let dots_x = (8.0 + strip_left + chip + inline_w)
            .min((self.window_size.width - strip_right - 206.0).max(0.0));
        let side_hidden_bar = crate::views::tab_bar::tab_bar_pos().is_side()
            && self.prefs.side_hide_top_bar;
        let pinned = container(panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Left)
            .align_y(iced::alignment::Vertical::Top)
            .padding(Padding {
                top: if side_hidden_bar { 37.0 } else { 78.0 },
                right: 0.0,
                bottom: 0.0,
                left: dots_x,
            });
        let backdrop: Element<'_, Message> = MouseArea::new(
            container(Space::new().width(Length::Fill).height(Length::Fill))
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(Message::Tabs(TabsMessage::ToggleSubnavOverflow))
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
        // Keyboard rows recorded in render order; the menu opens with
        // its first row selected (Up/Down move, Enter/Space fire).
        self.modal_nav_reset();
        self.keynav.modal.default.set(Some(0));
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
            let btn: Element<'_, Message> = button(
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
            // The inner container carries the row's real padding; the
            // button's own default (10 px lateral) would push every
            // item's hotkey hint 10 px off the section header's, which
            // is a plain container (owner QA: the badge column must
            // align).
            .padding(0)
            .on_press(msg.clone())
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
            .into();
            self.modal_nav_slot(crate::keynav::RowAction::activate(msg), 6.0, false, btn)
        };
        // Resolve hotkey hints from the live bindings so user
        // overrides flow through to the menu without rebuilds.
        let hk_settings = self.hotkey_label_for_action(crate::hotkeys::HotkeyAction::OpenSettings);
        let hk_local_shell = self.hotkey_label_for_action(crate::hotkeys::HotkeyAction::OpenLocalShell);
        let hk_new_window = self.hotkey_label_for_action(crate::hotkeys::HotkeyAction::NewWindow);
        // The VAULT section header carries the strip-slot hint (Ctrl+1
        // opens the vault area, which the strip always renders as its
        // first tab); each entry shows its own Ctrl+Shift+digit jump.
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
        // "VAULT" section header + indented children: the flat list
        // read as if Hosts/Keychain/... sat outside the Vault (issue
        // #38 review feedback); mirroring the top strip's Vault tab
        // here keeps one mental model. Indentation goes through
        // dir_row so it flips under RTL. The header carries the
        // strip-slot hint (Ctrl+1 opens the vault area itself; the
        // per-section Ctrl+Shift+digit hints live on the entries).
        let section = |label: &'static str, hint: Option<String>| -> Element<'_, Message> {
            let label_el: Element<'_, Message> = text(crate::i18n::t(label).to_uppercase())
                .size(10)
                .font(iced::Font {
                    weight: iced::font::Weight::Semibold,
                    ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                })
                .color(OryxisColors::t().text_muted)
                .into();
            let inner: Element<'_, Message> = if let Some(hint) = hint {
                dir_row(vec![
                    label_el,
                    Space::new().width(Length::Fill).into(),
                    // Same size as the item hints so the badge column
                    // reads as one aligned rail.
                    text(hint).size(11).color(OryxisColors::t().text_muted).into(),
                ])
                .align_y(iced::Alignment::Center)
                .into()
            } else {
                label_el
            };
            container(inner)
                .padding(Padding { top: 8.0, right: 16.0, bottom: 2.0, left: 16.0 })
                .width(Length::Fill)
                .align_x(dir_align_x())
                .into()
        };
        pub(crate) fn indent(inner: Element<'_, Message>) -> Element<'_, Message> {
            dir_row(vec![Space::new().width(10).into(), inner]).into()
        }
        let menu_col = column![
            section("vault", hk_hosts),
            indent(item(
                "hosts",
                // Mirrors the Hosts sub-nav pill (same list, same
                // destination) and the shortcut this row renders next to
                // itself: the vault section, at its root. The Home tab is
                // the door that keeps the folder (`GoHome`).
                Message::Navigation(NavigationMessage::ChangeView(View::Dashboard)),
                self.hotkey_label_for_vault_slot(1)
            )),
            indent(item(
                "keychain",
                Message::Navigation(NavigationMessage::ChangeView(View::Keys)),
                self.hotkey_label_for_vault_slot(2)
            )),
            indent(item(
                "snippets",
                Message::Navigation(NavigationMessage::ChangeView(View::Snippets)),
                self.hotkey_label_for_vault_slot(3)
            )),
            indent(item(
                "port_forwards",
                Message::Navigation(NavigationMessage::ChangeView(View::PortForwarding)),
                self.hotkey_label_for_vault_slot(4)
            )),
            if self.logs_surface_visible() {
                indent(item(
                    "logs",
                    Message::Navigation(NavigationMessage::ChangeView(View::History)),
                    self.hotkey_label_for_vault_slot(5)
                ))
            } else {
                Space::new().into()
            },
            indent(item(
                "cloud_accounts",
                Message::Navigation(NavigationMessage::ChangeView(View::Cloud)),
                self.hotkey_label_for_vault_slot(6)
            )),
            indent(item(
                "proxies",
                Message::Navigation(NavigationMessage::ChangeView(View::Proxies)),
                self.hotkey_label_for_vault_slot(7)
            )),
            indent(item(
                "known_hosts",
                Message::Navigation(NavigationMessage::ChangeView(View::KnownHosts)),
                self.hotkey_label_for_vault_slot(8)
            )),
            if self.prefs.host_monitoring {
                indent(item(
                    "monitor_dash_pill",
                    Message::Navigation(NavigationMessage::ChangeView(View::Monitoring)),
                    self.hotkey_label_for_vault_slot(9)
                ))
            } else {
                Space::new().into()
            },
            Space::new().height(4),
            // Mirror every sidebar nav entry here so Workspace mode
            // (where the sidebar is gone) still exposes the full set of
            // vault surfaces. The SFTP entry is gated on `sftp_enabled`,
            // same rule the sidebar applies. Built IN PLACE (not in a
            // variable above the column) on purpose: `item` records its
            // keynav slot at construction time, so build order IS the
            // Up/Down walk order and the menu's Enter default (slot 0).
            // Hoisted ahead of the column, this row recorded slot 0 and
            // a stray Enter through the modal router opened an SFTP tab
            // instead of activating the first visible row (issue #169).
            if self.sftp_enabled {
                // SFTP is a tab now: the menu opens a fresh SFTP browser tab.
                item("sftp", Message::Sftp(SftpMessage::NewSftpTab), hk_sftp)
            } else {
                Space::new().into()
            },
            item("settings", Message::Navigation(NavigationMessage::ChangeView(View::Settings)), hk_settings),
            // The network tools panel's only door, and it exists only
            // while the feature is on (the optional-features rule: off
            // means no UI at all, not a disabled row).
            if self.prefs.network_tools {
                item(
                    "network_tools",
                    Message::Navigation(NavigationMessage::ChangeView(View::NetworkTools)),
                    None,
                )
            } else {
                Space::new().into()
            },
            sep,
            item("local_shell", Message::Settings(SettingsMessage::OpenLocalShell), hk_local_shell),
            item("new_window", Message::Tabs(TabsMessage::SpawnNewWindow), hk_new_window),
            item("check_for_updates_now", Message::Update(UpdateMessage::CheckForUpdateManual), None),
            // Lock Vault only when a master password is set; without one,
            // locking has nothing to protect and the unlock screen has no
            // way to re-enter (mirrors the Settings -> Security gating).
            if self.vault_ui.has_user_password {
                // Asks first: Lock Vault tears every live session down, so
                // the item opens the confirm dialog (`LockVaultConfirm`)
                // rather than committing directly.
                item("lock_vault", Message::Vault(VaultMessage::LockVaultConfirm), None)
            } else {
                Space::new().into()
            },
        ]
        .width(Length::Fill);
        // 300 px: wide enough that the longest label + the longest
        // hotkey hint ("Port Forwarding" + "Ctrl + Shift + 4") share
        // a row without the hint wrapping onto a second line.
        let menu_panel = container(menu_col)
            .width(Length::Fixed(300.0))
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
        // Pin the panel just below the tab bar (40 px tall), under its
        // trigger. dir_align_x flips the anchor side under RTL. With the
        // top bar hidden (issue #87) the burger lives in the SIDE
        // strip's header instead, so the anchor follows the strip: a
        // right dock opens the menu from the right edge, a left dock
        // shifts it past the strip band (otherwise the panel opens in
        // the opposite corner of the window, detached from its button).
        let strip_pos = crate::views::tab_bar::tab_bar_pos();
        let in_strip_header = self.prefs.side_hide_top_bar && strip_pos.is_side();
        let (align, pad) = if in_strip_header {
            let inset = crate::views::tab_bar::SIDE_STRIP_WIDTH + 8.0;
            if strip_pos == crate::views::tab_bar::TabBarPos::Right {
                (
                    iced::alignment::Horizontal::Right,
                    Padding { top: 44.0, right: inset, bottom: 0.0, left: 0.0 },
                )
            } else {
                (
                    iced::alignment::Horizontal::Left,
                    Padding { top: 44.0, right: 0.0, bottom: 0.0, left: inset },
                )
            }
        } else {
            (
                dir_align_x(),
                Padding { top: 44.0, right: 6.0, bottom: 0.0, left: 6.0 },
            )
        };
        let pinned = container(menu_panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(align)
            .align_y(iced::alignment::Vertical::Top)
            .padding(pad);
        // Backdrop catches outside clicks. Z-stack: backdrop on the
        // bottom, panel on top so the panel's buttons still receive
        // their own clicks.
        let backdrop: Element<'_, Message> = MouseArea::new(
            container(Space::new().width(Length::Fill).height(Length::Fill))
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(Message::Tabs(TabsMessage::ToggleBurgerMenu))
        .into();
        Stack::new()
            .push(backdrop)
            .push(pinned)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A 750 px window, a popup 96 px tall (one credential with a
    // username, measured through the harness) and a caret 20 px tall:
    // the shape the bug was reported in.
    const WINDOW_H: f32 = 750.0;
    const POPUP_H: f32 = 96.0;

    #[test]
    fn a_popover_with_room_below_stays_below() {
        assert_eq!(popover_y(300.0, POPUP_H, WINDOW_H, Some(280.0)), 300.0);
    }

    #[test]
    fn a_caret_at_the_last_row_flips_the_popover_above_it() {
        // The failure this guards: the clamp alone slid the box up to
        // 654 (window minus height), covering the prompt line the user
        // is being asked to answer.
        let caret_top = 700.0;
        let below = caret_top + 20.0 + crate::dispatch_password_suggest::CARET_GAP;
        let y = popover_y(below, POPUP_H, WINDOW_H, Some(caret_top));
        assert_eq!(y, caret_top - POPUP_H - crate::dispatch_password_suggest::CARET_GAP);
        assert!(y + POPUP_H < caret_top, "the caret line must stay visible");
    }

    #[test]
    fn an_anchor_without_a_pivot_still_clamps() {
        // Every menu but the password popup hangs off a widget whose
        // height this layer never learns, so it has nothing to flip
        // over and must keep the old behaviour.
        assert_eq!(popover_y(740.0, POPUP_H, WINDOW_H, None), WINDOW_H - POPUP_H);
    }

    #[test]
    fn a_popover_that_fits_neither_side_clamps_instead_of_hanging_off() {
        // A vault with many identities: taller than the room above the
        // caret AND below it. Half-covered beats half off-screen.
        let tall = 700.0;
        assert_eq!(popover_y(600.0, tall, WINDOW_H, Some(580.0)), WINDOW_H - tall);
        // ... and never above the window's own top edge.
        assert_eq!(popover_y(600.0, 800.0, WINDOW_H, Some(580.0)), 0.0);
    }
}
