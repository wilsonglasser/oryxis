//! SFTP browser view, dual-pane (local | remote) file manager.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, row, scrollable, text, text_input, MouseArea, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis};
use crate::i18n::t;
use crate::state::{SftpEntryKind, SftpPaneSide};
use crate::theme::OryxisColors;
use crate::widgets::dir_align_x;

const ROW_HEIGHT: f32 = 28.0;

/// Which pane-anchored popover is open (layered at the view level so its
/// dismiss scrim covers the whole view).
#[derive(Clone, Copy)]
enum PanePopover {
    Actions,
    Filter,
}

impl Oryxis {
    pub(crate) fn view_sftp(&self) -> Element<'_, Message> {
        // Draggable center divider: clicking starts a resize, the global
        // mouse-move handler follows the cursor and the global mouse-up stops
        // it (shared with the chat-sidebar resize plumbing). The two panes
        // split the width by `sftp_split_ratio`.
        let divider: Element<'_, Message> = MouseArea::new(
            container(Space::new().width(Length::Fixed(5.0)).height(Length::Fill))
                .width(Length::Fixed(5.0))
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().border)),
                    ..Default::default()
                }),
        )
        .on_press(Message::SftpSplitResizeStart)
        .interaction(iced::mouse::Interaction::ResizingHorizontally)
        .into();
        let left_portion = (self.sftp_split_ratio * 1000.0).round().clamp(1.0, 999.0) as u16;
        let right_portion = 1000u16.saturating_sub(left_portion).max(1);
        let panes = row![
            container(self.view_sftp_pane(SftpPaneSide::Left))
                .width(Length::FillPortion(left_portion))
                .height(Length::Fill),
            divider,
            container(self.view_sftp_pane(SftpPaneSide::Right))
                .width(Length::FillPortion(right_portion))
                .height(Length::Fill),
        ]
        .width(Length::Fill)
        .height(Length::Fill);

        // Stack the panes with the optional progress strip below, when a
        // folder transfer is running we surface a thin status bar with
        // counts + a cancel button, otherwise the panes own all the space.
        let panes_area: Element<'_, Message> = if let Some(transfer) = &self.sftp.transfer {
            // Clicking the strip toggles a per-file panel that rises above
            // it. (Clicking the inner Cancel button also cancels, which
            // clears the transfer and hides both, so the extra toggle is
            // harmless.)
            let strip = MouseArea::new(transfer_progress_strip(
                transfer,
                self.sftp
                    .transfer_bytes_done
                    .load(std::sync::atomic::Ordering::Relaxed),
                self.sftp.transfer_bytes_total,
            ))
            .on_press(Message::SftpToggleTransferPanel);
            let mut col = column![panes].width(Length::Fill).height(Length::Fill);
            if self.sftp.transfer_panel_open {
                col = col.push(transfer_file_panel(transfer, &self.sftp.transfer_done_log));
            }
            col.push(strip).into()
        } else {
            panes.into()
        };

        // Footer: the optional message-log panel (FileZilla-style) above a
        // always-visible thin bar carrying the log toggle. The panes own the
        // remaining vertical space.
        let mut body_col = column![container(panes_area).width(Length::Fill).height(Length::Fill)]
            .width(Length::Fill)
            .height(Length::Fill);
        if self.sftp.log_open {
            body_col = body_col.push(sftp_log_panel(&self.sftp.log));
        }
        body_col = body_col.push(sftp_log_bar(self.sftp.log_open, self.sftp.log.len()));
        let body: Element<'_, Message> = body_col.into();

        // Pane-anchored popovers (the `⋮` actions menu and the collapsed
        // filter input) are layered here, at the whole-view level, rather than
        // inside a single pane. That way their dismiss scrim covers the entire
        // view, so clicking the *other* pane (or anywhere) closes them, and the
        // menu can overhang the pane divider without being clipped.
        let body = self.layer_sftp_pane_popover(body);
        // Floating ghost of the column header being reordered, following the
        // cursor (mirrors the host-tab drag ghost up top).
        let body = self.layer_sftp_col_drag_ghost(body);

        // The SFTP modals (picker, rename, new entry, properties,
        // overwrite, delete) are NOT composed here. They're layered at the
        // app root via `layer_sftp_modals` so they behave like every other
        // modal: a full-window blocking overlay. Keeping them view-local
        // used to leave their flags (e.g. `picker_open`) set while the user
        // switched to a terminal tab, where `any_modal_blocks_input` then
        // silently swallowed every keystroke. Layering at the root keeps the
        // invariant "flag set <=> modal visible on screen".
        body
    }

    /// Layer the open pane popover (actions `⋮` menu or collapsed-filter
    /// input) over `base` with a full-view scrim, positioned at the top-right
    /// of its pane. At most one is open at a time (opening one closes the
    /// others). Returns `base` untouched when none is open.
    fn layer_sftp_pane_popover<'a>(
        &'a self,
        base: Element<'a, Message>,
    ) -> Element<'a, Message> {
        // Resolve which popover is open and the pane it belongs to.
        let open: Option<(SftpPaneSide, PanePopover)> = if self.sftp.left.actions_open {
            Some((SftpPaneSide::Left, PanePopover::Actions))
        } else if self.sftp.right.actions_open {
            Some((SftpPaneSide::Right, PanePopover::Actions))
        } else if self.sftp.left.filter_open {
            Some((SftpPaneSide::Left, PanePopover::Filter))
        } else if self.sftp.right.filter_open {
            Some((SftpPaneSide::Right, PanePopover::Filter))
        } else {
            None
        };
        let Some((side, kind)) = open else { return base };
        let pane = self.sftp.pane(side);

        // Pane geometry in view-local coordinates (x = 0 at the view's left
        // edge, i.e. right of the nav rail).
        let content_w = (self.window_size.width - self.vault_rail_width()).max(1.0);
        let left_w = content_w * self.sftp_split_ratio;
        let pane_right = match side {
            SftpPaneSide::Left => left_w,
            SftpPaneSide::Right => content_w,
        };
        // The collapsed-filter popover only applies to a narrow (compact)
        // pane; if the pane is wide the inline input is shown instead, so
        // suppress a stale popover (e.g. left open then the pane widened).
        if matches!(kind, PanePopover::Filter) {
            let pane_w = match side {
                SftpPaneSide::Left => left_w,
                SftpPaneSide::Right => content_w - left_w,
            };
            if (pane_w - 6.0).max(1.0) >= 430.0 {
                return base;
            }
        }

        let (card, card_w, y): (Element<'a, Message>, f32, f32) = match kind {
            PanePopover::Actions => (
                actions_menu_card(
                    side,
                    pane.is_remote,
                    &pane.remote_path,
                    &pane.local_path,
                    pane.show_hidden,
                    pane.columns.visible,
                ),
                228.0,
                48.0,
            ),
            PanePopover::Filter => (filter_card(side, &pane.filter), 232.0, 46.0),
        };
        let x = (pane_right - card_w - 14.0).max(0.0);

        let scrim: Element<'a, Message> = MouseArea::new(
            container(Space::new()).width(Length::Fill).height(Length::Fill),
        )
        .on_press(Message::SftpCloseMenus)
        .into();
        let positioned: Element<'a, Message> = column![
            Space::new().height(Length::Fixed(y)),
            row![Space::new().width(Length::Fixed(x)), card],
        ]
        .into();
        iced::widget::Stack::new()
            .push(base)
            .push(scrim)
            .push(positioned)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Layer a floating ghost of the column header being reordered over
    /// `base`, following the cursor. Non-interactive (a plain container), so
    /// the header MouseAreas underneath still receive the hover events that
    /// pick the drop target. Returns `base` untouched when no reorder is live.
    fn layer_sftp_col_drag_ghost<'a>(
        &'a self,
        base: Element<'a, Message>,
    ) -> Element<'a, Message> {
        let Some(drag) = self.sftp_col_drag.filter(|d| d.active) else {
            return base;
        };
        let ghost = col_drag_ghost(data_col_label(drag.col));
        // Cursor → view-local coordinates: x is offset by the nav rail (0 on
        // the SFTP surface), y by the tab bar + hairline above the content.
        let rail = self.vault_rail_width();
        let view_top = if self.window_fullscreen { 0.0 } else { 41.0 };
        let gx = (self.mouse_position.x - rail + 12.0).max(0.0);
        let gy = (self.mouse_position.y - view_top - 4.0).max(0.0);
        let positioned: Element<'a, Message> = column![
            Space::new().height(Length::Fixed(gy)),
            row![Space::new().width(Length::Fixed(gx)), ghost],
        ]
        .into();
        iced::widget::Stack::new()
            .push(base)
            .push(positioned)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Layer the active SFTP modal over `base` (the whole composed app), so
    /// SFTP dialogs blanket the window like the global pickers instead of
    /// only the SFTP panes. Returns `base` untouched when no modal is open.
    /// Each modal keeps its own scrim / `opaque` wrapper, this only moves
    /// where they compose (root vs. inside `view_sftp`). The right-click row
    /// context menu stays at the layout root (window-coordinate clamping).
    pub(crate) fn layer_sftp_modals<'a>(
        &'a self,
        base: Element<'a, Message>,
    ) -> Element<'a, Message> {
        // Exactly one SFTP modal is open at a time (opening one closes the
        // others), so pick the topmost rather than stacking. Each builder
        // returns its own scrim + centered card wrapped so its `opaque`
        // scrim still swallows scroll/motion over the panes behind it.
        // Note: the SFTP close-guard modal is rendered at the global layer in
        // `view_main` (keyed on `pending_sftp_close`), not here, since the
        // close button lives in the always-visible tab strip and can be
        // clicked from any surface, not just while viewing SFTP.
        let modal: Option<Element<'a, Message>> = if !self.sftp.delete_confirm.is_empty() {
            Some(delete_confirm_modal(&self.sftp.delete_confirm))
        } else if let Some(entry) = &self.sftp.new_entry {
            Some(new_entry_modal(entry))
        } else if let Some(session) = &self.sftp.edit_session {
            Some(edit_in_place_modal(session))
        } else if let Some(prompt) = &self.sftp.overwrite_prompt {
            Some(overwrite_modal(prompt))
        } else if let Some(props) = &self.sftp.properties {
            Some(properties_modal(props))
        } else if self.sftp.picker_open {
            Some(self.view_sftp_picker())
        } else {
            None
        };
        let Some(modal) = modal else { return base };
        // Full-window scrim, NO top reserve: a modal must block everything
        // behind it, including the tab bar. If the top chrome stayed
        // clickable the user could switch to a terminal tab with the modal
        // still open, and `any_modal_blocks_input` would then freeze that
        // terminal's keyboard (the exact bug this whole change fixes).
        // Covering the whole window makes dismissing the modal (Esc / scrim
        // click / Cancel) the only way forward, which is the point of a
        // blocking modal. `opaque` swallows scroll/motion too, not just
        // clicks, so nothing bleeds through to the panes behind.
        iced::widget::Stack::new()
            .push(base)
            .push(iced::widget::opaque(modal))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Render one pane (Left or Right). Branches on the pane's
    /// `is_remote` nature to draw either the Local filesystem browser or
    /// the remote SFTP browser. The header always renders the
    /// host-picker chip; for a Local pane it reads "Local", for a remote
    /// pane it reads the mounted host label.
    fn view_sftp_pane(&self, side: SftpPaneSide) -> Element<'_, Message> {
        let pane = self.sftp.pane(side);
        let is_remote = pane.is_remote;
        // Resolve the column layout from this pane's on-screen width (the
        // content area split by the divider ratio). When the visible columns
        // overflow, the layout switches the rows to a fixed width and the list
        // gets a horizontal scrollbar.
        let pane_avail = {
            let content_w = (self.window_size.width - self.vault_rail_width()).max(1.0);
            let w = match side {
                SftpPaneSide::Left => content_w * self.sftp_split_ratio,
                SftpPaneSide::Right => content_w * (1.0 - self.sftp_split_ratio),
            };
            (w - 6.0).max(1.0)
        };
        // Narrow panes collapse the inline filter to an icon and let the host
        // chip shrink so the kebab is never pushed off the toolbar.
        let compact = pane_avail < 430.0;
        // Per-pane columns: ordered visible set + widths, plus the active
        // reorder drag / hover for header feedback.
        let ordered_cols = pane.columns.ordered_visible();
        let col_widths = pane.columns.width;
        let name_width = pane.columns.name_width;
        let trailing = trailing_cols_width(&ordered_cols, col_widths);
        let layout = ColLayout::resolve(name_width, trailing, pane_avail);
        let col_drag = self
            .sftp_col_drag
            .filter(|d| d.side == side && d.active)
            .map(|d| d.col);
        let col_hovered = self
            .sftp_hovered_col
            .filter(|(s, _)| *s == side)
            .map(|(_, c)| c);
        // Per-pane scroll id keyed by the current directory. Within one
        // directory the id is stable, so the list keeps its scroll offset
        // across re-renders (dragging a row, an in-place reload). Changing
        // directory changes the id, so iced treats it as a fresh
        // scrollable and the new listing starts at the top.
        let cur_path = if is_remote {
            pane.remote_path.clone()
        } else {
            pane.local_path.to_string_lossy().into_owned()
        };
        let side_key = match side {
            SftpPaneSide::Left => "left",
            SftpPaneSide::Right => "right",
        };
        let list_scroll_id = format!("sftp-list-{side_key}-{cur_path}");

        // Header chip: a button that opens the host picker targeting this
        // pane. Local panes show a monitor badge + "Local"; remote panes
        // show the host's OS badge + label + a chevron.
        let chip_icon: Element<'_, Message> = if !is_remote {
            container(
                iced_fonts::lucide::monitor().size(12).color(Color::WHITE),
            )
            .center_x(Length::Fixed(20.0))
            .center_y(Length::Fixed(20.0))
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().accent)),
                border: Border { radius: Radius::from(4.0), ..Default::default() },
                ..Default::default()
            })
            .into()
        } else {
            let mounted_conn = pane.host_label.as_ref().and_then(|label| {
                self.connections.iter().find(|c| &c.label == label)
            });
            if let Some(conn) = mounted_conn {
                let (glyph, badge_color) = crate::os_icon::resolve_icon(
                    conn.detected_os.as_deref(),
                    OryxisColors::t().accent,
                );
                container(glyph.view(14.0, Color::WHITE))
                    .center_x(Length::Fixed(20.0))
                    .center_y(Length::Fixed(20.0))
                    .style(move |_| container::Style {
                        background: Some(Background::Color(badge_color)),
                        border: Border { radius: Radius::from(4.0), ..Default::default() },
                        ..Default::default()
                    })
                    .into()
            } else {
                container(
                    iced_fonts::lucide::server()
                        .size(12)
                        .color(OryxisColors::t().text_muted),
                )
                .center_x(Length::Fixed(20.0))
                .center_y(Length::Fixed(20.0))
                .into()
            }
        };
        let chip_label = if !is_remote {
            t("sftp_local").to_string()
        } else {
            pane.host_label
                .clone()
                .unwrap_or_else(|| t("pick_a_host").to_string())
        };
        // In a narrow pane the chip flexes (Fill + clip) so a long host label
        // shrinks instead of pushing the filter / kebab off the toolbar; in a
        // wide pane it keeps its natural width with a Fill spacer after it.
        let chip_len = if compact { Length::Fill } else { Length::Shrink };
        let mut chip_row = row![
            chip_icon,
            Space::new().width(8),
            text(chip_label)
                .size(14)
                .color(OryxisColors::t().text_primary)
                .width(chip_len)
                .wrapping(iced::widget::text::Wrapping::None),
        ]
        .align_y(iced::Alignment::Center);
        chip_row = chip_row.push(Space::new().width(8));
        chip_row = chip_row.push(
            iced_fonts::lucide::chevron_down()
                .size(10)
                .color(OryxisColors::t().text_muted),
        );
        let header_title: Element<'_, Message> = button(chip_row)
            .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 4.0 })
            .width(chip_len)
            .on_press(Message::SftpOpenPicker(side))
            .style(|_, status| {
                let bg = match status {
                    BtnStatus::Hovered => OryxisColors::t().bg_hover,
                    _ => Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                }
            })
            .into();

        let actions_btn: Element<'_, Message> = pane_actions_btn(Message::SftpToggleActions(side));

        // Narrow panes collapse the inline filter to a search icon (the
        // floating input opens on click), so the kebab is never pushed off
        // the toolbar. The chip (Fill) shrinks to make room.
        let filter_widget: Element<'_, Message> = if compact {
            let has_filter = !pane.filter.is_empty();
            button(
                iced_fonts::lucide::search().size(14).color(if has_filter {
                    OryxisColors::t().accent
                } else {
                    OryxisColors::t().text_muted
                }),
            )
            .on_press(Message::SftpToggleFilterSearch(side))
            .padding(Padding { top: 7.0, right: 8.0, bottom: 7.0, left: 8.0 })
            .style(|_, status| {
                let bg = match status {
                    BtnStatus::Hovered => OryxisColors::t().bg_hover,
                    _ => Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                }
            })
            .into()
        } else {
            // Sized to match the system-standard search field (size 13 +
            // 9/12 padding, see `layout.rs` sub-nav search).
            let mut filter_input = text_input(t("filter_placeholder"), &pane.filter)
                .on_input(move |s| Message::SftpFilter(side, s))
                .padding(Padding { top: 9.0, right: 12.0, bottom: 9.0, left: 12.0 })
                .size(13)
                .width(200)
                .style(crate::widgets::rounded_input_style)
                .align_x(dir_align_x());
            if is_remote {
                filter_input = filter_input.id(iced::widget::Id::new("search-sftp-remote"));
            }
            filter_input.into()
        };

        // Wide pane: natural-width chip + a Fill spacer pushes filter/kebab to
        // the trailing edge. Narrow pane: the chip itself is Fill and shrinks,
        // so a fixed gap is enough. Either way filter + kebab stay fixed and
        // never get clipped.
        let lead_spacer: Element<'_, Message> = if compact {
            Space::new().width(8).into()
        } else {
            Space::new().width(Length::Fill).into()
        };
        let toolbar = row![
            header_title,
            lead_spacer,
            filter_widget,
            Space::new().width(8),
            actions_btn,
        ]
        .align_y(iced::Alignment::Center)
        .padding(Padding { top: 12.0, right: 14.0, bottom: 8.0, left: 14.0 });

        // The path bar swaps between a clickable breadcrumb and a text
        // input, same area, two modes, like Finder / Files / Explorer.
        let path_bar: Element<'_, Message> = if let Some(input) = &pane.path_editing {
            let placeholder = if is_remote {
                pane.remote_path.clone()
            } else {
                pane.local_path.display().to_string()
            };
            text_input(&placeholder, input)
                .on_input(move |s| Message::SftpEditPath(side, s))
                .on_submit(Message::SftpCommitPath(side))
                .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                .size(11)
                .style(crate::widgets::rounded_input_style)
                .align_x(dir_align_x())
                .into()
        } else {
            let crumbs: Element<'_, Message> = if is_remote {
                remote_breadcrumb(side, &pane.remote_path)
            } else {
                local_breadcrumb(side, &pane.local_path)
            };
            MouseArea::new(container(crumbs).width(Length::Fill))
                .on_press(Message::SftpStartEditPath(side))
                .into()
        };

        let needle = pane.filter.to_lowercase();
        // When the columns overflow, the header strip moves into the
        // horizontally-scrollable list (so it pans in sync with the rows);
        // otherwise it stays sticky in the header band as before.
        let mut band_content = column![
            toolbar,
            container(path_bar)
                .padding(Padding { top: 0.0, right: 14.0, bottom: 8.0, left: 14.0 })
                .width(Length::Fill),
        ]
        .width(Length::Fill);
        if !layout.overflow {
            band_content = band_content.push(column_headers(
                side,
                pane.sort,
                &ordered_cols,
                col_widths,
                layout,
                col_drag,
                col_hovered,
            ));
        }
        let header_band = pane_header_band(band_content);

        let body: Element<'_, Message> = if !is_remote {
            if let Some(err) = &pane.error {
                container(text(err.clone()).size(12).color(OryxisColors::t().error))
                    .padding(12)
                    .into()
            } else {
                let mut col = column![].spacing(0);
                if pane.local_path.parent().is_some() {
                    col = col.push(parent_row(side, &ordered_cols, col_widths, layout));
                }
                // Per-pane invariants hoisted out of the entry loop:
                // rename state for this side, the selected-row paths as
                // a set for O(1) membership, and the cross-pane drag
                // flag (it doesn't depend on the entry).
                let rename = self.sftp.rename.as_ref().filter(|r| r.side == side);
                let selected_paths: std::collections::HashSet<&str> = self
                    .sftp
                    .selected_rows
                    .iter()
                    .filter(|(s, _)| *s == side)
                    .map(|(_, p)| p.as_str())
                    .collect();
                // Tint a local folder row that's the drop target while
                // a cross-pane internal drag is in flight.
                let internal_cross_pane = self
                    .sftp
                    .drag
                    .as_ref()
                    .is_some_and(|d| d.active && d.origin_side != side);
                for entry in &pane.local_entries {
                    if !pane.show_hidden && entry.name.starts_with('.') {
                        continue;
                    }
                    if !needle.is_empty() && !entry.name.to_lowercase().contains(&needle) {
                        continue;
                    }
                    let path = pane.local_path.join(&entry.name);
                    let path_str = path.to_string_lossy().into_owned();
                    let rename_input = rename
                        .filter(|r| r.original_path == path_str)
                        .map(|r| r.input.as_str());
                    let is_selected = selected_paths.contains(path_str.as_str());
                    let is_drop_target = internal_cross_pane
                        && entry.is_dir
                        && self
                            .sftp
                            .hovered_row
                            .as_ref()
                            .is_some_and(|(s, p, _)| *s == side && p == &path_str);
                    col = col.push(file_row_local(
                        side,
                        entry.name.clone(),
                        entry.is_dir,
                        if entry.is_dir { String::new() } else { format_size(entry.size) },
                        entry.modified,
                        entry.mode,
                        entry.uid,
                        entry.gid,
                        path,
                        rename_input,
                        is_selected,
                        is_drop_target,
                        &ordered_cols,
                        col_widths,
                        layout,
                    ));
                }
                sftp_list_scrollable(
                    col,
                    &list_scroll_id,
                    side,
                    pane.sort,
                    &ordered_cols,
                    col_widths,
                    layout,
                    col_drag,
                    col_hovered,
                )
            }
        } else if let Some(err) = &pane.error {
            // Retry routes through SftpRetryRemote which knows whether
            // the session is still up (re-list) or whether the connect
            // itself failed (re-run the full pick flow).
            container(
                column![
                    row![
                        iced_fonts::lucide::circle_alert()
                            .size(14)
                            .color(OryxisColors::t().error),
                        Space::new().width(8),
                        text(err.clone())
                            .size(12)
                            .color(OryxisColors::t().error)
                            .width(Length::Fill),
                    ]
                    .align_y(iced::Alignment::Center),
                    Space::new().height(10),
                    row![
                        crate::widgets::styled_button(
                            t("retry"),
                            Message::SftpRetryRemote(side),
                            OryxisColors::t().accent,
                        ),
                        Space::new().width(8),
                        crate::widgets::styled_button(
                            t("pick_another_host"),
                            Message::SftpOpenPicker(side),
                            OryxisColors::t().text_muted,
                        ),
                    ],
                ]
                .padding(16),
            )
            .into()
        } else if pane.remote_loading && pane.remote_entries.is_empty() {
            // Only take over the pane with a loading screen on the first
            // load (nothing to show yet). On navigation/refresh we keep the
            // current listing visible until the new one arrives, like
            // FileZilla, so there's no jarring flash to "Loading...".
            container(
                column![
                    text(t("loading")).size(12).color(OryxisColors::t().text_muted),
                    Space::new().height(10),
                    crate::widgets::styled_button(
                        t("cancel"),
                        Message::SftpCancelRemoteLoad(side),
                        OryxisColors::t().text_muted,
                    ),
                ]
                .padding(12),
            )
            .into()
        } else if pane.host_label.is_none() {
            // Empty remote pane: a centered prompt with a button that opens
            // the host picker, instead of a lone line of muted text in the
            // corner. The picker no longer auto-opens on boot, so this is the
            // user's entry point into mounting a host.
            container(
                column![
                    iced_fonts::lucide::server()
                        .size(44)
                        .color(OryxisColors::t().text_muted),
                    Space::new().height(16),
                    text(t("pick_host_to_start"))
                        .size(15)
                        .color(OryxisColors::t().text_primary),
                    Space::new().height(16),
                    crate::widgets::styled_button(
                        t("pick_a_host"),
                        Message::SftpOpenPicker(side),
                        OryxisColors::t().accent,
                    ),
                ]
                .align_x(iced::Alignment::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
        } else {
            let mut col = column![].spacing(0);
            if pane.remote_path != "/" && !pane.remote_path.is_empty() {
                col = col.push(parent_row(side, &ordered_cols, col_widths, layout));
            }
            // Same per-pane invariants as the local branch, hoisted out
            // of the entry loop: rename state, the selected paths as a
            // set for O(1) membership, parent path, and the drag phase.
            let rename = self.sftp.rename.as_ref().filter(|r| r.side == side);
            let selected_paths: std::collections::HashSet<&str> = self
                .sftp
                .selected_rows
                .iter()
                .filter(|(s, _)| *s == side)
                .map(|(_, p)| p.as_str())
                .collect();
            let parent = pane.remote_path.trim_end_matches('/');
            let internal_cross_pane = self
                .sftp
                .drag
                .as_ref()
                .is_some_and(|d| d.active && d.origin_side != side);
            let drop_phase = self.sftp.drop_active || internal_cross_pane;
            for entry in &pane.remote_entries {
                if !pane.show_hidden && entry.name.starts_with('.') {
                    continue;
                }
                if !needle.is_empty() && !entry.name.to_lowercase().contains(&needle) {
                    continue;
                }
                let full = if parent.is_empty() {
                    format!("/{}", entry.name)
                } else {
                    format!("{}/{}", parent, entry.name)
                };
                let rename_input = rename
                    .filter(|r| r.original_path == full)
                    .map(|r| r.input.as_str());
                let is_drop_target = drop_phase
                    && entry.is_dir
                    && self
                        .sftp
                        .hovered_row
                        .as_ref()
                        .is_some_and(|(s, p, _)| *s == side && p == &full);
                let is_selected = selected_paths.contains(full.as_str());
                col = col.push(file_row_remote(
                    side,
                    entry.name.clone(),
                    entry.is_dir,
                    entry.is_symlink,
                    if entry.is_dir { String::new() } else { format_size(entry.size) },
                    entry.mtime,
                    entry.permissions,
                    entry.uid,
                    entry.gid,
                    full,
                    rename_input,
                    is_drop_target,
                    is_selected,
                    &ordered_cols,
                    col_widths,
                    layout,
                ));
            }
            sftp_list_scrollable(
                col,
                &list_scroll_id,
                side,
                pane.sort,
                &ordered_cols,
                col_widths,
                layout,
                col_drag,
                col_hovered,
            )
        };

        // Right-click on the empty area opens the directory-level context
        // menu. Rows carry their own `on_right_press`, which captures the
        // event first, so this only fires on the blank space below them.
        // Gated to browsable panes (local always; remote once mounted).
        let browsable = !is_remote || pane.host_label.is_some();
        let body: Element<'_, Message> = if browsable {
            MouseArea::new(body)
                .on_right_press(Message::SftpBackgroundRightClick(side))
                .into()
        } else {
            body
        };

        // With no host mounted yet, drop the top bar entirely (host chip,
        // filter, breadcrumb, column headers) and let the centered
        // "Pick a host" empty state own the whole pane.
        let show_header = !(is_remote && pane.host_label.is_none());
        let pane_body: Element<'_, Message> = if show_header {
            column![header_band, body]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            body
        };
        // The actions (`⋮`) menu and the collapsed-filter popover are NOT
        // pushed here: they're layered at the `view_sftp` level with a
        // full-window scrim so a click anywhere (including the other pane)
        // dismisses them. The drives picker (Windows-only) stays pane-local.
        let mut stack = iced::widget::Stack::new()
            .push(pane_body)
            .width(Length::Fill)
            .height(Length::Fill);

        if !is_remote && pane.drives_open {
            stack = stack.push(drives_menu_overlay(side));
        }

        // Drop highlight when a cross-pane internal drag (or, for a
        // remote pane, an OS file drag) targets this pane.
        let internal_drag_in = self
            .sftp
            .drag
            .as_ref()
            .is_some_and(|d| d.active && d.origin_side != side);
        let show_outline = internal_drag_in || (is_remote && self.sftp.drop_active);
        if show_outline {
            let outline = container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_| container::Style {
                    border: Border {
                        radius: Radius::from(0.0),
                        color: OryxisColors::t().accent,
                        width: 2.0,
                    },
                    ..Default::default()
                });
            stack = stack.push(outline);
        }
        stack.into()
    }

    fn view_sftp_picker(&self) -> Element<'_, Message> {
        let needle = self.sftp.picker_search.to_lowercase();
        let matches: Vec<usize> = self
            .connections
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                if needle.is_empty() {
                    true
                } else {
                    c.label.to_lowercase().contains(&needle)
                        || c.hostname.to_lowercase().contains(&needle)
                }
            })
            .map(|(i, _)| i)
            .collect();

        let mut list = column![].spacing(4);
        // The left pane can be Local; the right pane can't. Offer a
        // "Local" entry at the top of the list only when picking for the
        // left pane.
        if self.sftp.picker_target == SftpPaneSide::Left {
            let local_match = needle.is_empty() || t("sftp_local").to_lowercase().contains(&needle);
            if local_match {
                let badge = container(
                    iced_fonts::lucide::monitor().size(14).color(Color::WHITE),
                )
                .center_x(Length::Fixed(24.0))
                .center_y(Length::Fixed(24.0))
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().accent)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                });
                let local_btn = button(
                    crate::widgets::dir_row(vec![
                        badge.into(),
                        Space::new().width(10).into(),
                        column![
                            text(t("sftp_local")).size(13).color(OryxisColors::t().text_primary),
                            text(t("sftp_local_machine")).size(10).color(OryxisColors::t().text_muted),
                        ]
                        .width(Length::Fill)
                        .align_x(dir_align_x())
                        .into(),
                    ])
                    .align_y(iced::Alignment::Center),
                )
                .on_press(Message::SftpPickLocal)
                .padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 })
                .width(Length::Fill)
                .style(|_, status| {
                    let bg = match status {
                        BtnStatus::Hovered => OryxisColors::t().bg_hover,
                        _ => Color::TRANSPARENT,
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border { radius: Radius::from(6.0), ..Default::default() },
                        ..Default::default()
                    }
                });
                list = list.push(local_btn);
            }
        }
        for ci in matches {
            let conn = &self.connections[ci];
            let active = self
                .tabs
                .iter()
                .any(|t| t.label.trim_end_matches(" (disconnected)") == conn.label);
            let status_color = if active {
                OryxisColors::t().success
            } else {
                OryxisColors::t().text_muted
            };
            let status_text = if active { "reuse open session" } else { conn.hostname.as_str() };
            let fallback = if active {
                OryxisColors::t().success
            } else {
                OryxisColors::t().accent
            };
            let (glyph, default_color) =
                crate::os_icon::resolve_icon(conn.detected_os.as_deref(), fallback);
            // Respect the per-host icon shape + accent color so the
            // picker row matches the dashboard card for the same host.
            let badge_style = crate::widgets::resolve_host_icon_style(
                conn.icon_style.as_deref(),
                &self.setting_default_host_icon,
            );
            let badge_color = conn.custom_color.as_deref()
                .or(conn.color.as_deref())
                .and_then(crate::widgets::parse_hex_color)
                .unwrap_or(default_color);
            let glyph_el: Element<'_, Message> = glyph.view(14.0, Color::WHITE);
            let badge = crate::widgets::host_icon(
                badge_style,
                badge_color,
                &conn.label,
                Some(glyph_el),
                24.0,
            );
            let row_btn = button(
                crate::widgets::dir_row(vec![
                    badge,
                    Space::new().width(10).into(),
                    column![
                        text(conn.label.clone()).size(13).color(OryxisColors::t().text_primary),
                        text(status_text).size(10).color(status_color),
                    ]
                    .width(Length::Fill)
                    .align_x(dir_align_x())
                    .into(),
                ])
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::SftpPickHost(ci))
            .padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 })
            .width(Length::Fill)
            .style(|_, status| {
                let bg = match status {
                    BtnStatus::Hovered => OryxisColors::t().bg_hover,
                    _ => Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                }
            });
            list = list.push(row_btn);
        }

        let dialog = container(
            column![
                crate::widgets::dir_row(vec![
                    text(t("select_a_host")).size(15).color(OryxisColors::t().text_primary).into(),
                    Space::new().width(Length::Fill).into(),
                    button(
                        iced_fonts::lucide::x()
                            .size(13)
                            .color(OryxisColors::t().text_muted),
                    )
                    .on_press(Message::SftpClosePicker)
                    .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                    .style(|_, status| {
                        let bg = match status {
                            BtnStatus::Hovered => OryxisColors::t().bg_hover,
                            _ => Color::TRANSPARENT,
                        };
                        button::Style {
                            background: Some(Background::Color(bg)),
                            border: Border { radius: Radius::from(4.0), ..Default::default() },
                            ..Default::default()
                        }
                    })
                    .into(),
                ])
                .align_y(iced::Alignment::Center)
                .width(Length::Fill),
                Space::new().height(8),
                text_input(t("search_hosts"), &self.sftp.picker_search)
                    .on_input(Message::SftpPickerSearch)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
                Space::new().height(8),
                scrollable(list).height(Length::Fixed(360.0)),
            ]
            .padding(20)
            .width(Length::Fixed(440.0))
            .align_x(dir_align_x()),
        )
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border {
                radius: Radius::from(12.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        });

        // `iced::widget::opaque` makes the scrim capture every mouse event
        // (scroll and motion included, not just the click `on_press`
        // handles), so they stop here instead of bleeding through the
        // Stack to the SFTP panes underneath, e.g. scrolling the file list
        // behind the open modal.
        let scrim: Element<'_, Message> = iced::widget::opaque(
            MouseArea::new(
                container(Space::new())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5))),
                        ..Default::default()
                    }),
            )
            .on_press(Message::SftpClosePicker),
        );

        // Wrap the dialog in a MouseArea that swallows clicks via
        // `NoOp`, otherwise events fall through the Stack to the scrim
        // underneath and the picker closes on every click inside it.
        let centered = container(MouseArea::new(dialog).on_press(Message::NoOp))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill);

        iced::widget::Stack::new()
            .push(scrim)
            .push(centered)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn pane_actions_btn<'a>(toggle_msg: Message) -> Element<'a, Message> {
    crate::widgets::card_kebab_button(
        OryxisColors::t().text_secondary,
        true,
        toggle_msg,
    )
    .into()
}

/// The collapsed-filter input card. Positioned + scrimmed by the caller at
/// the `view_sftp` level.
fn filter_card<'a>(side: SftpPaneSide, filter: &str) -> Element<'a, Message> {
    let id = match side {
        SftpPaneSide::Left => "sftp-filter-pop-left",
        SftpPaneSide::Right => "sftp-filter-pop-right",
    };
    let input = text_input(t("filter_placeholder"), filter)
        .id(iced::widget::Id::new(id))
        .on_input(move |s| Message::SftpFilter(side, s))
        .on_submit(Message::SftpToggleFilterSearch(side))
        .padding(Padding { top: 9.0, right: 12.0, bottom: 9.0, left: 12.0 })
        .size(13)
        .width(Length::Fixed(220.0))
        .style(crate::widgets::rounded_input_style)
        .align_x(dir_align_x());
    let card = container(input)
        .padding(6)
        .width(Length::Fixed(232.0))
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border {
                radius: Radius::from(8.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            shadow: iced::Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.25),
                offset: iced::Vector::new(0.0, 4.0),
                blur_radius: 12.0,
            },
            ..Default::default()
        });
    card.into()
}

/// Floating Actions menu for a pane, anchored to the top-right via a
/// container that pushes it to the corner.
/// The actions (`⋮`) menu card. Positioned + scrimmed by the caller at the
/// `view_sftp` level so a click anywhere (including the other pane) closes it.
fn actions_menu_card<'a>(
    side: SftpPaneSide,
    is_remote: bool,
    remote_path: &str,
    local_path: &std::path::Path,
    show_hidden: bool,
    cols: crate::state::SftpColumns,
) -> Element<'a, Message> {
    use crate::state::SftpColumn;
    // Same directory-level actions as the cursor-anchored background menu,
    // shared via `dir_action_items` so the two never drift apart.
    let mut menu_col = column![].spacing(2).padding(4);
    let dir_ctx = DirActionCtx { pane_dir: remote_path, local_dir: local_path, show_hidden };
    for it in dir_action_items(side, is_remote, dir_ctx, true) {
        menu_col = menu_col.push(it);
    }
    // Columns section: toggle each optional column. The menu stays open on
    // click so several can be flipped in one pass.
    menu_col = menu_col.push(menu_separator());
    menu_col = menu_col.push(
        container(
            text(t("columns"))
                .size(10)
                .color(OryxisColors::t().text_muted),
        )
        .padding(Padding { top: 4.0, right: 10.0, bottom: 2.0, left: 10.0 }),
    );
    for (label, col, on) in [
        (t("col_modified"), SftpColumn::Modified, cols.modified),
        (t("col_size"), SftpColumn::Size, cols.size),
        (t("col_type"), SftpColumn::Kind, cols.kind),
        (t("col_permissions"), SftpColumn::Permissions, cols.permissions),
        (t("col_owner"), SftpColumn::Owner, cols.owner),
    ] {
        menu_col = menu_col.push(column_toggle_item(side, label, col, on));
    }
    let menu = container(menu_col)
    // Pin the menu to the same width as the rows inside it. Without
    // this, `menu_separator`'s `Length::Fill` propagates up through
    // `column![]` and the outer container, stretching the dropdown
    // across the entire pane.
    .width(Length::Fixed(228.0))
    .style(|_| container::Style {
        background: Some(Background::Color(OryxisColors::t().bg_surface)),
        border: Border {
            radius: Radius::from(8.0),
            color: OryxisColors::t().border,
            width: 1.0,
        },
        shadow: iced::Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.25),
            offset: iced::Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
        ..Default::default()
    });
    menu.into()
}

fn menu_separator<'a>() -> Element<'a, Message> {
    container(Space::new().height(1))
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().border)),
            ..Default::default()
        })
        .into()
}

/// One row of the Columns section in the actions menu: a check glyph
/// (shown only when the column is visible) plus the column label. Firing
/// `SftpToggleColumn` flips the column without closing the menu.
fn column_toggle_item<'a>(
    side: SftpPaneSide,
    label: &'a str,
    col: crate::state::SftpColumn,
    visible: bool,
) -> Element<'a, Message> {
    let check = iced_fonts::lucide::check().size(12).color(if visible {
        OryxisColors::t().accent
    } else {
        Color::TRANSPARENT
    });
    button(
        row![
            check,
            Space::new().width(10),
            text(label).size(12).color(OryxisColors::t().text_primary),
        ]
        .align_y(iced::Alignment::Center),
    )
    .on_press(Message::SftpToggleColumn(side, col))
    .padding(Padding { top: 6.0, right: 14.0, bottom: 6.0, left: 10.0 })
    .width(Length::Fixed(220.0))
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().bg_hover,
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(4.0), ..Default::default() },
            ..Default::default()
        }
    })
    .into()
}

/// Right-click row context menu, items vary by pane side and entry
/// type. When the clicked row is part of a multi-selection (same pane),
/// the menu switches to bulk variants: count-aware Delete; single-only
/// ops (Rename, Edit) hide.
/// Pane context the directory-level actions need: the current directory
/// (target of New folder / New file / Refresh), the local path for
/// "Open in File Manager", and the hidden-files toggle state.
#[derive(Clone, Copy)]
pub(crate) struct DirActionCtx<'a> {
    pub pane_dir: &'a str,
    pub local_dir: &'a std::path::Path,
    pub show_hidden: bool,
}

pub(crate) fn row_context_menu_box<'a>(
    menu: &crate::state::SftpRowMenu,
    cross_pane_ready: bool,
    source_is_remote: bool,
    other_is_remote: bool,
    other_label: Option<String>,
    selection_count_same_pane: usize,
    dir_ctx: DirActionCtx<'_>,
) -> Element<'a, Message> {
    let multi = selection_count_same_pane > 1;
    let mut items = column![].spacing(2).padding(4);
    let accent = OryxisColors::t().accent;
    let secondary = OryxisColors::t().text_secondary;
    let danger = OryxisColors::t().error;
    // Background right-click (empty area): only directory-level actions,
    // no per-entry target exists. Same items as the pane's `⋮` menu.
    if menu.is_background {
        for it in dir_action_items(menu.side, source_is_remote, dir_ctx, true) {
            items = items.push(it);
        }
        return context_menu_shell(items);
    }
    // Cross-pane action, picked by the source and the opposite pane's
    // natures: Local -> remote uploads, remote -> Local downloads,
    // remote -> remote relays. Only offered when the other pane is a
    // ready destination (connected remote, or a Local pane).
    if !source_is_remote && other_is_remote {
        // Upload to the (remote) other pane.
        if cross_pane_ready {
            if multi {
                items = items.push(menu_item_owned_tinted(
                    iced_fonts::lucide::upload(),
                    t("upload_n_items").replacen("{n}", &selection_count_same_pane.to_string(), 1),
                    Message::SftpUploadSelection,
                    accent,
                ));
            } else {
                let upload_msg = if menu.is_dir {
                    Message::SftpUploadFolder(std::path::PathBuf::from(&menu.path))
                } else {
                    // Route even a single file through the batch queue so the
                    // transfer shows the progress strip + per-file panel
                    // (SftpUpload alone creates no TransferState, hence no
                    // on-screen indicator).
                    Message::SftpUploadBatch(vec![std::path::PathBuf::from(&menu.path)])
                };
                let upload_label = match &other_label {
                    Some(h) => t("upload_to_host").replacen("{host}", h, 1),
                    None => t("upload_to_host").replacen("{host}", t("the_other_host"), 1),
                };
                items = items.push(menu_item_owned_tinted(
                    iced_fonts::lucide::upload(),
                    upload_label,
                    upload_msg,
                    accent,
                ));
            }
        }
        // Open the local file in the OS default editor.
        if !multi && !menu.is_dir {
            items = items.push(menu_item_owned_tinted(
                iced_fonts::lucide::pencil(),
                crate::i18n::t("edit").to_string(),
                Message::SftpOpenLocal(std::path::PathBuf::from(&menu.path)),
                secondary,
            ));
        }
    } else if source_is_remote && !other_is_remote {
        // Download to the (Local) other pane.
        if cross_pane_ready {
            if multi {
                items = items.push(menu_item_owned_tinted(
                    iced_fonts::lucide::download(),
                    t("download_n_items").replacen("{n}", &selection_count_same_pane.to_string(), 1),
                    Message::SftpDownloadSelection,
                    accent,
                ));
            } else {
                let download_msg = if menu.is_dir {
                    Message::SftpDownloadFolder(menu.path.clone())
                } else {
                    Message::SftpDownload(menu.path.clone())
                };
                items = items.push(menu_item_tinted(
                    iced_fonts::lucide::download(),
                    t("download_to_local"),
                    download_msg,
                    accent,
                ));
            }
        }
        // Edit-in-place for a single remote file.
        if !multi && !menu.is_dir {
            items = items.push(menu_item_owned_tinted(
                iced_fonts::lucide::pencil(),
                crate::i18n::t("edit").to_string(),
                Message::SftpStartEdit(menu.path.clone()),
                secondary,
            ));
        }
    } else if source_is_remote && other_is_remote {
        // Relay to the other (remote) host. Single-item only for now,
        // multi falls back to per-item relays via the row each.
        if cross_pane_ready {
            let label = match &other_label {
                Some(h) => t("relay_to_remote").replacen("{host}", h, 1),
                None => t("relay_to_remote").replacen("{host}", t("the_other_host"), 1),
            };
            let relay_msg = if menu.is_dir {
                Message::SftpRelayFolder(menu.side, menu.path.clone())
            } else {
                Message::SftpRelay(menu.side, menu.path.clone())
            };
            items = items.push(menu_item_owned_tinted(
                iced_fonts::lucide::arrow_right_left(),
                label,
                relay_msg,
                accent,
            ));
        }
        // Edit-in-place for a single remote file.
        if !multi && !menu.is_dir {
            items = items.push(menu_item_owned_tinted(
                iced_fonts::lucide::pencil(),
                crate::i18n::t("edit").to_string(),
                Message::SftpStartEdit(menu.path.clone()),
                secondary,
            ));
        }
    }
    // Reveal in the OS file manager, local pane only (no notion of an
    // "explorer" for a remote host). Single selection: a folder opens in
    // place, a file opens its folder with the file selected.
    if !source_is_remote && !multi {
        items = items.push(menu_item_tinted(
            iced_fonts::lucide::folder_open(),
            crate::i18n::open_in_file_manager_label(),
            Message::SftpRevealInExplorer(std::path::PathBuf::from(&menu.path), menu.is_dir),
            secondary,
        ));
    }
    if multi {
        items = items.push(menu_item_owned_tinted(
            iced_fonts::lucide::copy(),
            t("duplicate_n_items").replacen("{n}", &selection_count_same_pane.to_string(), 1),
            Message::SftpDuplicateSelection,
            secondary,
        ));
    } else {
        let duplicate_msg = if menu.is_dir {
            Message::SftpDuplicateFolder(menu.side, menu.path.clone())
        } else {
            Message::SftpDuplicate(menu.side, menu.path.clone())
        };
        items = items.push(menu_item_tinted(
            iced_fonts::lucide::copy(),
            t("duplicate"),
            duplicate_msg,
            secondary,
        ));
        items = items.push(menu_item_tinted(
            iced_fonts::lucide::pencil(),
            t("rename"),
            Message::SftpStartRename(menu.side, menu.path.clone()),
            secondary,
        ));
        items = items.push(menu_item_tinted(
            iced_fonts::lucide::cog(),
            t("properties"),
            Message::SftpShowProperties(menu.side, menu.path.clone(), menu.is_dir),
            secondary,
        ));
    }
    let delete_label = if multi {
        t("delete_n_items").replacen("{n}", &selection_count_same_pane.to_string(), 1)
    } else {
        t("delete").to_string()
    };
    let delete_msg = if multi {
        Message::SftpAskDeleteSelection
    } else {
        Message::SftpAskDelete(menu.side, menu.path.clone(), menu.is_dir)
    };
    items = items.push(menu_item_owned_tinted(
        iced_fonts::lucide::trash(),
        delete_label,
        delete_msg,
        danger,
    ));

    // Directory-level actions appended below the per-entry block, like
    // FileZilla's row menu (create folder/file + refresh act on the
    // pane's current directory, not the clicked entry).
    items = items.push(menu_separator());
    for it in dir_action_items(menu.side, source_is_remote, dir_ctx, false) {
        items = items.push(it);
    }

    context_menu_shell(items)
}

/// Directory-level actions for the current pane: New folder, New file,
/// Refresh, and (when `full`) Show hidden + Open in File Manager. `full`
/// is set for the background / `⋮` menus where these are the whole menu;
/// the row menu appends only the create + refresh trio. Open in File
/// Manager stays local-only (no OS explorer for a remote host); the
/// create/refresh/hidden actions apply to both panes.
fn dir_action_items<'a>(
    side: SftpPaneSide,
    is_remote: bool,
    ctx: DirActionCtx<'_>,
    full: bool,
) -> Vec<Element<'a, Message>> {
    let refresh_msg = if is_remote {
        Message::SftpNavigateRemote(side, ctx.pane_dir.to_string())
    } else {
        Message::SftpRefreshLocal(side)
    };
    let mut items: Vec<Element<'a, Message>> = vec![
        menu_item(
            iced_fonts::lucide::folder_plus(),
            t("new_folder"),
            Message::SftpStartNewEntry(side, SftpEntryKind::Folder),
        ),
        menu_item(
            iced_fonts::lucide::file_plus(),
            t("new_file"),
            Message::SftpStartNewEntry(side, SftpEntryKind::File),
        ),
    ];
    if full {
        items.push(menu_separator());
    }
    items.push(menu_item(iced_fonts::lucide::rotate_cw(), t("refresh"), refresh_msg));
    if full {
        let hidden_label =
            if ctx.show_hidden { t("hide_hidden_files") } else { t("show_hidden_files") };
        items.push(menu_item(
            iced_fonts::lucide::eye(),
            hidden_label,
            Message::SftpToggleHidden(side),
        ));
        if !is_remote {
            items.push(menu_separator());
            items.push(menu_item(
                iced_fonts::lucide::folder_open(),
                crate::i18n::open_in_file_manager_label(),
                Message::SftpRevealInExplorer(ctx.local_dir.to_path_buf(), true),
            ));
        }
    }
    items
}

/// Shared shell for the cursor-anchored SFTP context menus (row +
/// background): fixed width so the `Length::Fill` separators don't
/// stretch the popover, plus the surface/border/shadow styling.
fn context_menu_shell<'a>(
    items: iced::widget::Column<'a, Message>,
) -> Element<'a, Message> {
    container(items)
        .width(Length::Fixed(ROW_CONTEXT_MENU_WIDTH))
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border {
                radius: Radius::from(8.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            shadow: iced::Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.25),
                offset: iced::Vector::new(0.0, 4.0),
                blur_radius: 12.0,
            },
            ..Default::default()
        })
        .into()
}

/// Compute the approximate height of the row context menu given the
/// current target, keeps the layout-level clamp accurate so the menu
/// never spills off the bottom or right edge of the window.
pub(crate) fn row_context_menu_height(
    menu: &crate::state::SftpRowMenu,
    cross_pane_ready: bool,
    source_is_remote: bool,
    other_is_remote: bool,
    selection_count_same_pane: usize,
) -> f32 {
    // Background menu: directory actions only. New folder + New file +
    // Refresh + Show hidden (4), plus Open in File Manager on a local
    // pane (5), plus ~2 thin separators.
    if menu.is_background {
        let items = if source_is_remote { 4.0 } else { 5.0 };
        let separators = if source_is_remote { 1.0 } else { 2.0 };
        return items * 30.0 + separators * 4.0 + 8.0;
    }
    let multi = selection_count_same_pane > 1;
    // Always present: Duplicate + Rename + Properties + Delete (single),
    // or Duplicate + Delete (multi).
    let mut count = if multi { 2.0 } else { 4.0 };
    // Cross-pane action (Upload / Download / Relay) when the other pane
    // is a ready destination.
    if cross_pane_ready {
        count += 1.0;
    }
    // Edit-in-place / open-local for a single remote-source file, or a
    // single local file when uploading.
    if !multi && !menu.is_dir {
        let editable = source_is_remote || other_is_remote;
        if editable {
            count += 1.0;
        }
    }
    // "Open in File Manager" for a single local-pane entry.
    if !source_is_remote && !multi {
        count += 1.0;
    }
    // Appended directory actions (New folder + New file + Refresh) plus
    // their leading separator.
    count += 3.0;
    // Each item ~30px (padding 6+6 + ~12px text + 2px gap) plus 8px
    // container padding, plus one thin separator above the dir actions.
    count * 30.0 + 4.0 + 8.0
}

/// Width is fixed because every item uses the same `menu_item` width.
pub(crate) const ROW_CONTEXT_MENU_WIDTH: f32 = 220.0;

/// Owned-label variant of `menu_item` for cases where the label is
/// computed at runtime (e.g. "Delete N items" with a dynamic count).
/// Owned-label variant that lets the caller pick the icon tint
/// used for destructive (red) and primary (accent / success) actions
/// to match the host-card context menu's color coding.
fn menu_item_owned_tinted<'a>(
    icon: iced::widget::Text<'a>,
    label: String,
    msg: Message,
    tint: Color,
) -> Element<'a, Message> {
    button(
        row![
            icon.size(12).color(tint),
            Space::new().width(10),
            text(label).size(12).color(OryxisColors::t().text_primary),
        ]
        .align_y(iced::Alignment::Center),
    )
    .on_press(msg)
    .padding(Padding { top: 6.0, right: 14.0, bottom: 6.0, left: 10.0 })
    .width(Length::Fixed(220.0))
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().bg_hover,
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(4.0), ..Default::default() },
            ..Default::default()
        }
    })
    .into()
}

fn menu_item<'a>(
    icon: iced::widget::Text<'a>,
    label: &'a str,
    msg: Message,
) -> Element<'a, Message> {
    menu_item_tinted(icon, label, msg, OryxisColors::t().text_secondary)
}

/// Like `menu_item` but with an explicit icon tint (red for delete,
/// accent for primary actions, etc.).
fn menu_item_tinted<'a>(
    icon: iced::widget::Text<'a>,
    label: &'a str,
    msg: Message,
    tint: Color,
) -> Element<'a, Message> {
    button(
        row![
            icon.size(12).color(tint),
            Space::new().width(10),
            text(label).size(12).color(OryxisColors::t().text_primary),
        ]
        .align_y(iced::Alignment::Center),
    )
    .on_press(msg)
    .padding(Padding { top: 6.0, right: 14.0, bottom: 6.0, left: 10.0 })
    .width(Length::Fixed(220.0))
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().bg_hover,
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(4.0), ..Default::default() },
            ..Default::default()
        }
    })
    .into()
}

/// Drive picker dropdown for Windows local pane. Lists `C:`, `D:`, etc.
/// based on what's actually mounted. Closed via the scrim.
fn drives_menu_overlay<'a>(side: SftpPaneSide) -> Element<'a, Message> {
    let drives = list_windows_drives_cached();
    let mut col = column![].spacing(2).padding(4);
    if drives.is_empty() {
        col = col.push(
            container(text(t("no_drives_detected")).size(11).color(OryxisColors::t().text_muted))
                .padding(8),
        );
    } else {
        for drive in drives {
            let drive_path: std::path::PathBuf = format!("{}\\", drive).into();
            col = col.push(
                button(
                    row![
                        iced_fonts::lucide::hard_drive()
                            .size(12)
                            .color(OryxisColors::t().accent),
                        Space::new().width(8),
                        text(drive.clone()).size(12).color(OryxisColors::t().text_primary),
                    ]
                    .align_y(iced::Alignment::Center),
                )
                .on_press(Message::SftpNavigateLocal(side, drive_path))
                .padding(Padding { top: 6.0, right: 16.0, bottom: 6.0, left: 10.0 })
                .width(Length::Fixed(160.0))
                .style(|_, status| {
                    let bg = match status {
                        BtnStatus::Hovered => OryxisColors::t().bg_hover,
                        _ => Color::TRANSPARENT,
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border { radius: Radius::from(4.0), ..Default::default() },
                        ..Default::default()
                    }
                }),
            );
        }
    }
    let menu = container(col).style(|_| container::Style {
        background: Some(Background::Color(OryxisColors::t().bg_surface)),
        border: Border {
            radius: Radius::from(8.0),
            color: OryxisColors::t().border,
            width: 1.0,
        },
        shadow: iced::Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.25),
            offset: iced::Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
        ..Default::default()
    });
    let scrim: Element<'_, Message> = MouseArea::new(
        container(Space::new()).width(Length::Fill).height(Length::Fill),
    )
    .on_press(Message::SftpCloseMenus)
    .into();
    let positioned = container(menu)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Left)
        .align_y(iced::alignment::Vertical::Top)
        .padding(Padding { top: 70.0, right: 0.0, bottom: 0.0, left: 14.0 });
    iced::widget::Stack::new().push(scrim).push(positioned).into()
}

/// True when the path's first component is a real Windows volume
/// (`C:\`, `D:\`, including the `\\?\C:\` verbatim form). UNC paths
/// like `\\server\share` or `\\wsl$\Ubuntu` return false, those are
/// served by Unix-style filesystems where `/` reads more naturally.
fn is_windows_disk_path(path: &std::path::Path) -> bool {
    matches!(
        path.components().next(),
        Some(std::path::Component::Prefix(p))
            if matches!(
                p.kind(),
                std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)
            )
    )
}

/// Cached front for `list_windows_drives`. The raw probe touches the
/// filesystem for every drive letter (A: through Z:) and stats
/// `wsl.exe`, far too heavy to run per frame while the drive popover
/// is open, so the result is reused for a few seconds. Plugging or
/// unplugging a drive shows up on the next refresh window.
fn list_windows_drives_cached() -> Vec<String> {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    const TTL: Duration = Duration::from_secs(5);
    static CACHE: Mutex<Option<(Instant, Vec<String>)>> = Mutex::new(None);
    let mut guard = CACHE.lock().unwrap();
    if let Some((probed_at, drives)) = guard.as_ref()
        && probed_at.elapsed() < TTL
    {
        return drives.clone();
    }
    let drives = list_windows_drives();
    *guard = Some((Instant::now(), drives.clone()));
    drives
}

/// Enumerate available drive letters on Windows. Empty on non-Windows
/// hosts (the dropdown isn't rendered there). When running under WSL,
/// surface `\\wsl.localhost` as a synthetic root so the user can hop
/// between WSL distros without dropping to a terminal.
fn list_windows_drives() -> Vec<String> {
    #[cfg(target_os = "windows")]
    {
        let mut drives = Vec::new();
        for letter in b'A'..=b'Z' {
            let path = format!("{}:\\", letter as char);
            if std::path::Path::new(&path).exists() {
                drives.push(format!("{}:", letter as char));
            }
        }
        // WSL distros live under \\wsl.localhost (or the legacy
        // \\wsl$). `Path::exists()` on a UNC root returns false until
        // the SMB redirector lazily mounts it, so we detect WSL via
        // `wsl.exe` in System32, present iff the user has WSL
        // installed at all. We expose `\\wsl$` as the entry point
        // because it's the alias that always resolves; navigating into
        // it lists distros as folders.
        let wsl_exe = std::env::var_os("SystemRoot")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Windows"))
            .join("System32")
            .join("wsl.exe");
        if wsl_exe.exists() {
            drives.push(r"\\wsl$".to_string());
        }
        drives
    }
    #[cfg(not(target_os = "windows"))]
    {
        Vec::new()
    }
}

/// Build a clickable breadcrumb for a remote POSIX path. The root is
/// the only `/` rendered, subsequent segments are added with separators
/// in between, never *after* the root crumb itself, which avoids the
/// `/ / home` doubling that crept in when separators were emitted at the
/// start of every iteration.
fn remote_breadcrumb<'a>(side: SftpPaneSide, path: &str) -> Element<'a, Message> {
    let mut row = iced::widget::Row::new().align_y(iced::Alignment::Center).spacing(2);
    row = row.push(crumb_remote(side, "/", "/"));
    let mut accumulated = String::new();
    let mut first_segment = true;
    for segment in path.split('/').filter(|s| !s.is_empty()) {
        accumulated.push('/');
        accumulated.push_str(segment);
        if !first_segment {
            row = row.push(text("/").size(11).color(OryxisColors::t().text_muted));
        }
        first_segment = false;
        row = row.push(crumb_remote(side, segment, &accumulated));
    }
    row.into()
}

/// Build a clickable breadcrumb for a local filesystem path. On Windows
/// the first crumb is the drive letter and clicking it opens the drive
/// picker dropdown. The Unix root chip swallows the next separator so
/// the visual reads `/ home / user` instead of `/ / home / user`. On
/// Windows the implicit `RootDir` component after the drive prefix is
/// skipped (its job is taken by the drive chip itself).
fn local_breadcrumb<'a>(side: SftpPaneSide, path: &std::path::Path) -> Element<'a, Message> {
    // Pick the separator from the path's flavor: real Windows drives
    // (`C:\`, `D:\`) get `\`; everything else (Unix paths, WSL UNC like
    // `\\wsl$\Ubuntu\…`, bare network shares) keeps the Unix `/` since
    // either the user is on Linux or they're navigating into a Linux
    // filesystem from Windows.
    let separator = if is_windows_disk_path(path) { "\\" } else { "/" };
    let mut row = iced::widget::Row::new().align_y(iced::Alignment::Center).spacing(2);
    let mut accumulated = std::path::PathBuf::new();
    let mut first = true;
    let mut last_was_root_or_drive = false;
    let mut had_drive = false;
    for component in path.components() {
        let (label, is_drive, is_root) = match component {
            std::path::Component::Prefix(p) => {
                had_drive = true;
                (p.as_os_str().to_string_lossy().into_owned(), true, false)
            }
            std::path::Component::RootDir => {
                // Skip the implicit root component on Windows, the drive
                // chip already represents the volume root.
                if had_drive {
                    accumulated.push(component.as_os_str());
                    last_was_root_or_drive = true;
                    continue;
                }
                ("/".to_string(), false, true)
            }
            std::path::Component::Normal(s) => (s.to_string_lossy().into_owned(), false, false),
            std::path::Component::CurDir | std::path::Component::ParentDir => continue,
        };
        accumulated.push(component.as_os_str());
        if !first && !last_was_root_or_drive {
            row = row.push(text(separator).size(11).color(OryxisColors::t().text_muted));
        }
        first = false;
        last_was_root_or_drive = is_root || is_drive;
        if is_drive {
            // Drive-letter chip toggles the drives dropdown so the user
            // can jump to another mount without typing.
            row = row.push(
                button(
                    row![
                        iced_fonts::lucide::hard_drive()
                            .size(11)
                            .color(OryxisColors::t().accent),
                        Space::new().width(4),
                        text(label).size(11).color(OryxisColors::t().text_secondary),
                        Space::new().width(2),
                        iced_fonts::lucide::chevron_down()
                            .size(9)
                            .color(OryxisColors::t().text_muted),
                    ]
                    .align_y(iced::Alignment::Center),
                )
                .on_press(Message::SftpToggleDrives(side))
                .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
                .style(|_, status| {
                    let bg = match status {
                        BtnStatus::Hovered => OryxisColors::t().bg_hover,
                        _ => Color::TRANSPARENT,
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border { radius: Radius::from(4.0), ..Default::default() },
                        ..Default::default()
                    }
                }),
            );
        } else {
            row = row.push(local_crumb(side, label, accumulated.clone()));
        }
    }
    row.into()
}

fn crumb_remote<'a>(side: SftpPaneSide, label: &str, full: &str) -> Element<'a, Message> {
    let label = label.to_string();
    let full = full.to_string();
    button(text(label).size(11).color(OryxisColors::t().text_secondary))
        .on_press(Message::SftpNavigateRemote(side, full))
        .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
        .style(|_, status| {
            let bg = match status {
                BtnStatus::Hovered => OryxisColors::t().bg_hover,
                _ => Color::TRANSPARENT,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(4.0), ..Default::default() },
                ..Default::default()
            }
        })
        .into()
}

fn local_crumb<'a>(side: SftpPaneSide, label: String, full: std::path::PathBuf) -> Element<'a, Message> {
    button(text(label).size(11).color(OryxisColors::t().text_secondary))
        .on_press(Message::SftpNavigateLocal(side, full))
        .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
        .style(|_, status| {
            let bg = match status {
                BtnStatus::Hovered => OryxisColors::t().bg_hover,
                _ => Color::TRANSPARENT,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(4.0), ..Default::default() },
                ..Default::default()
            }
        })
        .into()
}

fn parent_row<'a>(
    side: SftpPaneSide,
    visible: &[crate::state::SftpColumn],
    widths: crate::state::SftpColWidths,
    layout: ColLayout,
) -> Element<'a, Message> {
    let msg = Message::SftpUp(side);
    let mut children: Vec<Element<'a, Message>> = vec![
        iced_fonts::lucide::folder()
            .size(13)
            .color(OryxisColors::t().text_muted)
            .into(),
        Space::new().width(8).into(),
        text("..")
            .size(12)
            .color(OryxisColors::t().text_muted)
            .width(layout.name_len)
            .into(),
    ];
    // Blank trailing cells keep the ".." row aligned with the columns.
    children.extend(trailing_cells(visible, widths, |_| String::new()));
    let inner = iced::widget::Row::with_children(children).align_y(iced::Alignment::Center);
    button(inner)
        .padding(Padding { top: 4.0, right: 12.0, bottom: 4.0, left: 12.0 })
        .width(layout.row_len)
        .height(Length::Fixed(ROW_HEIGHT))
        .on_press(msg)
        .style(|_, status| {
            let bg = match status {
                BtnStatus::Hovered => OryxisColors::t().bg_hover,
                _ => Color::TRANSPARENT,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border::default(),
                ..Default::default()
            }
        })
        .into()
}

/// Width of the leading icon + gap column, used both as the header's
/// alignment pad and to size the fixed-width row when columns overflow.
const ICON_COL_W: f32 = 21.0;
/// Left + right padding applied to every header / row container.
const ROW_PAD_X: f32 = 24.0;
/// Width of the resize handle (and its visible divider) on the right edge of
/// each column header.
const COL_RESIZE_HANDLE_W: f32 = 7.0;

/// Sum of the visible data columns at their current widths, in order.
fn trailing_cols_width(
    visible: &[crate::state::SftpColumn],
    widths: crate::state::SftpColWidths,
) -> f32 {
    visible.iter().map(|c| widths.get(*c)).sum()
}

/// Resolved horizontal layout for the file list of one pane. Every column
/// (Name included) has an explicit width, so resizing one grows the total
/// instead of squeezing a neighbour. When the total exceeds the pane the row
/// gets a fixed width and a horizontal scrollbar; otherwise it fills the pane
/// (columns left-packed, the trailing slack left empty).
#[derive(Clone, Copy)]
struct ColLayout {
    /// Name-column width in pixels.
    name_w: f32,
    /// Fixed Name-column length (`Fixed(name_w)`).
    name_len: Length,
    /// Whole-row width: `Fill` when everything fits, `Fixed(total)` when it
    /// overflows (so the horizontal scrollable has something to pan).
    row_len: Length,
    /// True when the row width is fixed (columns overflow the pane).
    overflow: bool,
}

impl ColLayout {
    /// Build the layout for a pane `avail` px wide, given the Name width and
    /// the visible data columns' total width.
    fn resolve(name_width: f32, trailing: f32, avail: f32) -> Self {
        let total = ROW_PAD_X + ICON_COL_W + name_width + trailing;
        ColLayout {
            name_w: name_width,
            name_len: Length::Fixed(name_width),
            row_len: if total <= avail {
                Length::Fill
            } else {
                Length::Fixed(total)
            },
            overflow: total > avail,
        }
    }
}

/// POSIX rwx string for a permission/mode value, e.g. "drwxr-xr-x".
/// The leading type char comes from the row's dir/symlink flags; only
/// the low 12 bits of `mode` carry the rwx + setuid/setgid/sticky bits.
fn format_perms(mode: Option<u32>, is_dir: bool, is_symlink: bool) -> String {
    let Some(m) = mode else {
        return "-".to_string();
    };
    let type_char = if is_symlink {
        'l'
    } else if is_dir {
        'd'
    } else {
        '-'
    };
    let rwx = |bits: u32| {
        format!(
            "{}{}{}",
            if bits & 0o4 != 0 { 'r' } else { '-' },
            if bits & 0o2 != 0 { 'w' } else { '-' },
            if bits & 0o1 != 0 { 'x' } else { '-' },
        )
    };
    format!(
        "{}{}{}{}",
        type_char,
        rwx((m >> 6) & 0o7),
        rwx((m >> 3) & 0o7),
        rwx(m & 0o7),
    )
}

/// "uid:gid" owner string, with a dash when neither side is known
/// (Windows local entries, or a server that omits owner attributes).
fn format_owner(uid: Option<u32>, gid: Option<u32>) -> String {
    match (uid, gid) {
        (Some(u), Some(g)) => format!("{u}:{g}"),
        (Some(u), None) => u.to_string(),
        (None, Some(g)) => format!(":{g}"),
        (None, None) => "-".to_string(),
    }
}

/// Value for the Type column: folders / symlinks keep their friendly label;
/// files show the MIME type guessed from the extension (`application/
/// octet-stream` when unknown or extensionless).
fn format_kind(name: &str, is_dir: bool, is_symlink: bool) -> String {
    if is_symlink {
        return t("sftp_type_symlink").to_string();
    }
    if is_dir {
        return t("sftp_type_folder").to_string();
    }
    match name.rsplit_once('.') {
        Some((stem, ext)) if !ext.is_empty() && !stem.is_empty() => {
            mime_for_ext(&ext.to_ascii_lowercase()).to_string()
        }
        _ => "application/octet-stream".to_string(),
    }
}

/// MIME type for a (lowercased) file extension. The comprehensive
/// [`crate::mime_types`] table (embedded from mime-db) covers the long tail;
/// `dev_mime_override` wins first for source-code / dev extensions that
/// mime-db gets wrong (e.g. `.rs`, `.ts`) or doesn't list (`.go`, `.vue`).
/// Anything unknown falls back to `application/octet-stream`.
fn mime_for_ext(ext: &str) -> &'static str {
    dev_mime_override(ext)
        .or_else(|| crate::mime_types::lookup(ext))
        .unwrap_or("application/octet-stream")
}

/// Source-code / dev extensions where mime-db is wrong or missing. Returns
/// `None` for everything else so the embedded mime-db table answers.
fn dev_mime_override(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "rs" => "text/x-rust",
        "go" => "text/x-go",
        "py" | "pyw" | "pyi" => "text/x-python",
        "rb" => "text/x-ruby",
        // mime-db maps .ts to MPEG transport stream; in a code tree it's
        // overwhelmingly TypeScript.
        "ts" | "tsx" | "mts" | "cts" => "application/typescript",
        "jsx" => "text/jsx",
        "mjs" | "cjs" => "text/javascript",
        "kt" | "kts" => "text/x-kotlin",
        "swift" => "text/x-swift",
        "cs" => "text/x-csharp",
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" => "text/x-c++",
        "vue" => "text/x-vue",
        "svelte" => "text/x-svelte",
        "astro" => "text/x-astro",
        "dockerfile" => "text/x-dockerfile",
        "env" => "text/plain",
        "ex" | "exs" => "text/x-elixir",
        "erl" => "text/x-erlang",
        "hs" => "text/x-haskell",
        "clj" | "cljs" | "cljc" => "text/x-clojure",
        "scala" | "sc" => "text/x-scala",
        "dart" => "application/dart",
        "zig" => "text/x-zig",
        "nim" => "text/x-nim",
        "proto" => "text/x-protobuf",
        "tf" | "tfvars" => "text/x-terraform",
        "gradle" => "text/x-gradle",
        _ => return None,
    })
}

/// Build the data cells of a file row in the pane's column order, at the
/// pane's column widths. `value` formats one column's text. Returns the
/// cells to append after the Name label.
fn trailing_cells<'a>(
    visible: &[crate::state::SftpColumn],
    widths: crate::state::SftpColWidths,
    value: impl Fn(crate::state::SftpColumn) -> String,
) -> Vec<Element<'a, Message>> {
    visible
        .iter()
        .map(|c| {
            text(value(*c))
                .size(11)
                .color(OryxisColors::t().text_muted)
                .width(Length::Fixed(widths.get(*c)))
                .into()
        })
        .collect()
}

/// Visually distinct band that wraps the toolbar / breadcrumb / column
/// headers, gives the file list a clean separation from the chrome,
/// matching how Finder / Explorer / Termius split the two regions.
fn pane_header_band<'a>(content: iced::widget::Column<'a, Message>) -> Element<'a, Message> {
    container(content)
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
            border: Border {
                width: 0.0,
                color: OryxisColors::t().border,
                radius: Radius::from(0.0),
            },
            ..Default::default()
        })
        .into()
}

/// Wrap a built file-list column in the right scrollable. With columns
/// that fit, this is the plain vertical scroll used before. When the
/// columns overflow, the header strip is prepended (so it pans with the
/// rows) and the scrollable gains a horizontal scrollbar.
#[allow(clippy::too_many_arguments)]
fn sftp_list_scrollable<'a>(
    col: iced::widget::Column<'a, Message>,
    scroll_id: &str,
    side: SftpPaneSide,
    sort: crate::state::SftpSort,
    visible: &[crate::state::SftpColumn],
    widths: crate::state::SftpColWidths,
    layout: ColLayout,
    drag_col: Option<crate::state::SftpColumn>,
    hovered: Option<crate::state::SftpColumn>,
) -> Element<'a, Message> {
    if layout.overflow {
        // Sticky header + horizontal scroll (FileZilla-style): the rows get
        // their own vertical scrollable, and that plus the header sit inside
        // an outer horizontal scrollable. Panning sideways moves header and
        // rows together (kept aligned), while the header never scrolls away
        // vertically because the vertical scroll lives on the inner list only.
        let inner_list = scrollable(col)
            .id(iced::widget::Id::from(scroll_id.to_string()))
            .width(layout.row_len)
            .height(Length::Fill);
        let header = column_headers(side, sort, visible, widths, layout, drag_col, hovered);
        let stacked = column![header, inner_list]
            .width(layout.row_len)
            .height(Length::Fill);
        scrollable(stacked)
            .width(Length::Fill)
            .height(Length::Fill)
            .direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::new()))
            .into()
    } else {
        scrollable(col)
            .id(iced::widget::Id::from(scroll_id.to_string()))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

/// Label for a data column header.
fn data_col_label(col: crate::state::SftpColumn) -> &'static str {
    use crate::state::SftpColumn;
    match col {
        SftpColumn::Modified => t("col_modified"),
        SftpColumn::Size => t("col_size"),
        SftpColumn::Kind => t("col_type"),
        SftpColumn::Permissions => t("col_permissions"),
        SftpColumn::Owner => t("col_owner"),
    }
}

/// Floating ghost shown while a column header is dragged for reorder:
/// the column label in an accent-bordered, shadowed chip (mirrors the
/// host-tab drag ghost).
fn col_drag_ghost<'a>(label: &str) -> Element<'a, Message> {
    let accent = OryxisColors::t().accent;
    container(
        text(label.to_string())
            .size(11)
            .color(accent)
            .wrapping(iced::widget::text::Wrapping::None),
    )
    .padding(Padding { top: 5.0, right: 12.0, bottom: 5.0, left: 12.0 })
    .style(move |_| container::Style {
        background: Some(Background::Color(Color { a: 0.96, ..OryxisColors::t().bg_hover })),
        border: Border { radius: Radius::from(6.0), color: accent, width: 1.5 },
        shadow: iced::Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
            offset: iced::Vector::new(0.0, 2.0),
            blur_radius: 6.0,
        },
        ..Default::default()
    })
    .into()
}

/// A resize handle: a centered 1px divider line inside a wider grab zone,
/// living on the right edge of a header. `target == None` resizes Name.
fn col_resize_handle<'a>(
    side: SftpPaneSide,
    target: Option<crate::state::SftpColumn>,
) -> Element<'a, Message> {
    MouseArea::new(
        container(
            container(Space::new().width(Length::Fixed(1.0)).height(Length::Fill))
                .width(Length::Fixed(1.0))
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().border)),
                    ..Default::default()
                }),
        )
        .width(Length::Fixed(COL_RESIZE_HANDLE_W))
        .height(Length::Fill)
        .center_x(Length::Fixed(COL_RESIZE_HANDLE_W)),
    )
    .on_press(Message::SftpColResizeStart(side, target))
    .interaction(iced::mouse::Interaction::ResizingHorizontally)
    .into()
}

/// Column header strip. Every column has an explicit width and a right-edge
/// resize handle (a visible divider). Name is sortable + resizable but fixed
/// first; the data columns are also draggable to reorder. Sortable headers
/// flip the sort on a plain click (release without a drag, handled in the
/// mouse-up dispatcher).
fn column_headers<'a>(
    side: SftpPaneSide,
    sort: crate::state::SftpSort,
    visible: &[crate::state::SftpColumn],
    widths: crate::state::SftpColWidths,
    layout: ColLayout,
    drag_col: Option<crate::state::SftpColumn>,
    hovered: Option<crate::state::SftpColumn>,
) -> Element<'a, Message> {
    use crate::state::SftpSortColumn;
    // The fixed, sortable + resizable Name header.
    let name_arrow = if sort.column == SftpSortColumn::Name {
        if sort.ascending { " \u{2191}" } else { " \u{2193}" }
    } else {
        ""
    };
    let name_btn_w = (layout.name_w - COL_RESIZE_HANDLE_W).max(8.0);
    let name_btn: Element<'a, Message> = button(
        text(format!("{}{}", t("col_name"), name_arrow))
            .size(11)
            .color(if sort.column == SftpSortColumn::Name {
                OryxisColors::t().text_primary
            } else {
                OryxisColors::t().text_muted
            }),
    )
    .on_press(Message::SftpSort(side, SftpSortColumn::Name))
    .width(Length::Fixed(name_btn_w))
    .padding(Padding { top: 4.0, right: 6.0, bottom: 4.0, left: 6.0 })
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().bg_hover,
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(4.0), ..Default::default() },
            ..Default::default()
        }
    })
    .into();
    let name_header: Element<'a, Message> = row![name_btn, col_resize_handle(side, None)]
        .width(layout.name_len)
        .align_y(iced::Alignment::Center)
        .into();

    let mut children: Vec<Element<'a, Message>> = vec![
        // Pad-icon column to align with file rows below.
        Space::new().width(Length::Fixed(ICON_COL_W)).into(),
        name_header,
    ];

    let dragging = drag_col.is_some();
    for &col in visible {
        let w = widths.get(col);
        let sortable = col.sort_column();
        let active_sort = sortable.is_some_and(|sc| sort.column == sc);
        let arrow = match sortable {
            Some(sc) if sort.column == sc => {
                if sort.ascending { " \u{2191}" } else { " \u{2193}" }
            }
            _ => "",
        };
        // Highlight a header as the drop target while another column is
        // dragged over it.
        let is_target = dragging && hovered == Some(col) && drag_col != Some(col);
        let is_dragged = drag_col == Some(col);
        let label_w = (w - COL_RESIZE_HANDLE_W).max(8.0);
        let label_color = if active_sort {
            OryxisColors::t().text_primary
        } else {
            OryxisColors::t().text_muted
        };
        let label_area = MouseArea::new(
            container(text(format!("{}{}", data_col_label(col), arrow)).size(11).color(label_color))
                .width(Length::Fixed(label_w))
                .padding(Padding { top: 4.0, right: 6.0, bottom: 4.0, left: 6.0 })
                .style(move |_| {
                    let bg = if is_target {
                        Some(Background::Color(Color { a: 0.20, ..OryxisColors::t().accent }))
                    } else if is_dragged {
                        Some(Background::Color(OryxisColors::t().bg_hover))
                    } else {
                        None
                    };
                    container::Style {
                        background: bg,
                        border: Border { radius: Radius::from(4.0), ..Default::default() },
                        ..Default::default()
                    }
                }),
        )
        .on_press(Message::SftpColDragStart(side, col))
        .on_enter(Message::SftpColHovered(side, col))
        .on_exit(Message::SftpColUnhovered)
        // Grab hint normally; grabbing while a reorder drag is active so the
        // user gets cursor feedback that the drag took.
        .interaction(if dragging {
            iced::mouse::Interaction::Grabbing
        } else {
            iced::mouse::Interaction::Grab
        });
        children.push(
            row![label_area, col_resize_handle(side, Some(col))]
                .width(Length::Fixed(w))
                .align_y(iced::Alignment::Center)
                .into(),
        );
    }

    container(
        iced::widget::Row::with_children(children)
            .height(Length::Fill)
            .align_y(iced::Alignment::Center),
    )
    // Fixed height so the Fill-height resize handles don't balloon the strip
    // (and thus the whole header band) to fill the pane vertically.
    .height(Length::Fixed(28.0))
    .padding(Padding { top: 4.0, right: 12.0, bottom: 4.0, left: 12.0 })
    .width(layout.row_len)
    .style(|_| container::Style {
        background: Some(Background::Color(OryxisColors::t().bg_surface)),
        border: Border {
            width: 0.0,
            color: OryxisColors::t().border,
            radius: Radius::from(0.0),
        },
        ..Default::default()
    })
    .into()
}

fn format_modified_local(modified: Option<std::time::SystemTime>) -> String {
    let Some(t) = modified else { return String::new() };
    let dt: chrono::DateTime<chrono::Local> = t.into();
    dt.format("%Y-%m-%d %H:%M").to_string()
}

fn format_modified_remote(mtime: Option<u32>) -> String {
    let Some(secs) = mtime else { return String::new() };
    match chrono::DateTime::<chrono::Utc>::from_timestamp(secs as i64, 0) {
        Some(dt) => dt
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
        None => String::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn file_row_local<'a>(
    side: SftpPaneSide,
    name: String,
    is_dir: bool,
    size_str: String,
    modified: Option<std::time::SystemTime>,
    mode: Option<u32>,
    uid: Option<u32>,
    gid: Option<u32>,
    path: std::path::PathBuf,
    rename_input: Option<&str>,
    is_selected: bool,
    is_drop_target: bool,
    visible: &[crate::state::SftpColumn],
    widths: crate::state::SftpColWidths,
    layout: ColLayout,
) -> Element<'a, Message> {
    use crate::state::SftpColumn;
    let icon = file_icon(&name, is_dir, false);
    let kind = format_kind(&name, is_dir, false);
    let modified_s = format_modified_local(modified);
    let perms_s = format_perms(mode, is_dir, false);
    let owner_s = format_owner(uid, gid);

    // Inline rename mode swaps the row's label for a text input; the
    // icon + columns stay put so the row geometry doesn't jump.
    let label_widget: Element<'_, Message> = if let Some(input) = rename_input {
        text_input(&name, input)
            .on_input(Message::SftpRenameInput)
            .on_submit(Message::SftpRenameCommit)
            .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
            .size(11)
            .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
            .width(layout.name_len)
            .into()
    } else {
        text(name).size(12).color(OryxisColors::t().text_primary).width(layout.name_len).into()
    };

    let mut children: Vec<Element<'a, Message>> =
        vec![icon.into(), Space::new().width(8).into(), label_widget];
    children.extend(trailing_cells(visible, widths, |c| match c {
        SftpColumn::Modified => modified_s.clone(),
        SftpColumn::Size => size_str.clone(),
        SftpColumn::Kind => kind.clone(),
        SftpColumn::Permissions => perms_s.clone(),
        SftpColumn::Owner => owner_s.clone(),
    }));
    let inner = iced::widget::Row::with_children(children)
        // Fill the button's fixed height so the content centers vertically.
        .height(Length::Fill)
        .align_y(iced::Alignment::Center);

    // Click action priority: while renaming, swallow clicks; folders
    // navigate; files mark themselves selected so the user has visible
    // confirmation that the row is interactive (was previously a disabled
    // button, no hover, no pointer cursor, looked dead).
    let path_str = path.to_string_lossy().into_owned();
    // SftpSelectRow handles plain folder click (navigate), file click
    // (single-select), and modifier clicks (toggle / range). Routing it
    // all through one message means modifier state can be consulted
    // server-side instead of being stored at button-build time.
    let on_click = if rename_input.is_some() {
        None
    } else {
        Some(Message::SftpSelectRow(side, path_str.clone(), is_dir))
    };
    let path_for_enter = path_str.clone();
    let mut btn = button(inner)
        .padding(Padding { top: 4.0, right: 12.0, bottom: 4.0, left: 12.0 })
        .width(layout.row_len)
        .height(Length::Fixed(ROW_HEIGHT))
        .style(move |_, status| {
            // Drop highlight beats selection while a drag is in flight,
            // matches the right-pane logic.
            let bg = if is_drop_target || is_selected {
                Color { a: 0.20, ..OryxisColors::t().accent }
            } else {
                match status {
                    BtnStatus::Hovered => OryxisColors::t().bg_hover,
                    _ => Color::TRANSPARENT,
                }
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border::default(),
                ..Default::default()
            }
        });
    if let Some(msg) = on_click {
        btn = btn.on_press(msg);
    }
    // Hover events feed both the OS drag drop targeting and the new
    // internal drag-drop press handler, needed even on file rows since
    // a file is a valid drag *source* (just not a drop *target*).
    MouseArea::new(btn)
        .on_right_press(Message::SftpRowRightClick(side, path_str, is_dir))
        .on_enter(Message::SftpRowEnter(side, path_for_enter, is_dir))
        .on_exit(Message::SftpRowExit)
        .into()
}

#[allow(clippy::too_many_arguments)]
fn file_row_remote<'a>(
    side: SftpPaneSide,
    name: String,
    is_dir: bool,
    is_symlink: bool,
    size_str: String,
    mtime: Option<u32>,
    permissions: Option<u32>,
    uid: Option<u32>,
    gid: Option<u32>,
    full_path: String,
    rename_input: Option<&str>,
    is_drop_target: bool,
    is_selected: bool,
    visible: &[crate::state::SftpColumn],
    widths: crate::state::SftpColWidths,
    layout: ColLayout,
) -> Element<'a, Message> {
    use crate::state::SftpColumn;
    let icon = file_icon(&name, is_dir, is_symlink);
    let kind = format_kind(&name, is_dir, is_symlink);
    let modified_s = format_modified_remote(mtime);
    let perms_s = format_perms(permissions, is_dir, is_symlink);
    let owner_s = format_owner(uid, gid);

    let label_widget: Element<'_, Message> = if let Some(input) = rename_input {
        text_input(&name, input)
            .on_input(Message::SftpRenameInput)
            .on_submit(Message::SftpRenameCommit)
            .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
            .size(11)
            .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
            .width(layout.name_len)
            .into()
    } else {
        text(name).size(12).color(OryxisColors::t().text_primary).width(layout.name_len).into()
    };

    // Single message routes folder navigation, file single-select, and
    // ctrl/shift modifier selection, see the local row counterpart.
    // Symlinks behave like folders for click (treat as nav target) since
    // we can't tell from the listing whether they point at a file vs dir.
    let nav_target = if rename_input.is_some() {
        None
    } else {
        Some(Message::SftpSelectRow(
            side,
            full_path.clone(),
            is_dir || is_symlink,
        ))
    };
    let mut children: Vec<Element<'a, Message>> =
        vec![icon.into(), Space::new().width(8).into(), label_widget];
    children.extend(trailing_cells(visible, widths, |c| match c {
        SftpColumn::Modified => modified_s.clone(),
        SftpColumn::Size => size_str.clone(),
        SftpColumn::Kind => kind.clone(),
        SftpColumn::Permissions => perms_s.clone(),
        SftpColumn::Owner => owner_s.clone(),
    }));
    let inner = iced::widget::Row::with_children(children)
        // Fill the button's fixed height so `align_y(Center)` actually centers
        // the content vertically inside the row (otherwise a shrink-height row
        // sits at the top of the slot and reads as misaligned).
        .height(Length::Fill)
        .align_y(iced::Alignment::Center);
    let mut btn = button(inner)
        .padding(Padding { top: 4.0, right: 12.0, bottom: 4.0, left: 12.0 })
        .width(layout.row_len)
        .height(Length::Fixed(ROW_HEIGHT))
        .style(move |_, status| {
            // Drop highlight beats selection (transient, communicates
            // imminent action), selection beats default hover.
            let bg = if is_drop_target || is_selected {
                Color { a: 0.20, ..OryxisColors::t().accent }
            } else {
                match status {
                    BtnStatus::Hovered => OryxisColors::t().bg_hover,
                    _ => Color::TRANSPARENT,
                }
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border::default(),
                ..Default::default()
            }
        });
    if let Some(msg) = nav_target {
        btn = btn.on_press(msg);
    }
    // Hover events update the global hovered_row state. That state
    // serves the OS drop target picker, the internal drag-drop press
    // handler, and the cross-pane folder drop highlight.
    MouseArea::new(btn)
        .on_right_press(Message::SftpRowRightClick(side, full_path.clone(), is_dir))
        .on_enter(Message::SftpRowEnter(side, full_path, is_dir))
        .on_exit(Message::SftpRowExit)
        .into()
}

fn file_icon<'a>(name: &str, is_dir: bool, is_symlink: bool) -> iced::widget::Text<'a> {
    if is_dir {
        return iced_fonts::lucide::folder()
            .size(13)
            .color(OryxisColors::t().accent);
    }
    if is_symlink {
        return iced_fonts::lucide::file_symlink()
            .size(13)
            .color(OryxisColors::t().accent);
    }
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    let (glyph, color) = match ext.as_deref() {
        Some("rs") | Some("ts") | Some("js") | Some("py") | Some("go") | Some("c") | Some("cpp")
        | Some("h") | Some("hpp") | Some("java") | Some("kt") | Some("rb") | Some("php")
        | Some("sh") | Some("bash") | Some("zsh") | Some("fish") | Some("vim") | Some("lua") => (
            iced_fonts::lucide::file_code(),
            OryxisColors::t().success,
        ),
        Some("json") | Some("yaml") | Some("yml") | Some("toml") | Some("ini") | Some("env")
        | Some("conf") | Some("cfg") => (
            iced_fonts::lucide::file_cog(),
            OryxisColors::t().warning,
        ),
        Some("md") | Some("txt") | Some("rst") | Some("log") => (
            iced_fonts::lucide::file_text(),
            OryxisColors::t().text_secondary,
        ),
        Some("png") | Some("jpg") | Some("jpeg") | Some("gif") | Some("svg") | Some("webp")
        | Some("bmp") | Some("ico") => (
            iced_fonts::lucide::file_image(),
            OryxisColors::t().accent,
        ),
        Some("mp4") | Some("mkv") | Some("mov") | Some("avi") | Some("webm") => (
            iced_fonts::lucide::file_video(),
            OryxisColors::t().accent,
        ),
        Some("mp3") | Some("wav") | Some("flac") | Some("ogg") | Some("m4a") => (
            iced_fonts::lucide::file_audio(),
            OryxisColors::t().accent,
        ),
        Some("zip") | Some("tar") | Some("gz") | Some("bz2") | Some("xz") | Some("7z")
        | Some("rar") | Some("deb") | Some("rpm") => (
            iced_fonts::lucide::file_archive(),
            OryxisColors::t().warning,
        ),
        Some("pdf") => (
            iced_fonts::lucide::file_text(),
            OryxisColors::t().error,
        ),
        Some("csv") | Some("xlsx") | Some("xls") => (
            iced_fonts::lucide::file_spreadsheet(),
            OryxisColors::t().success,
        ),
        Some("html") | Some("htm") | Some("css") | Some("scss") => (
            iced_fonts::lucide::file_code(),
            OryxisColors::t().accent,
        ),
        Some("key") | Some("pem") | Some("crt") | Some("cer") => (
            iced_fonts::lucide::file_key(),
            OryxisColors::t().warning,
        ),
        _ => (
            iced_fonts::lucide::file(),
            OryxisColors::t().text_muted,
        ),
    };
    glyph.size(13).color(color)
}

/// Confirmation dialog for the "Delete" action. Single targets show
/// the file name and a folder-recursive hint; bulk deletes show a count
/// and folder-vs-file breakdown so the user understands the blast
/// radius.
fn delete_confirm_modal<'a>(
    targets: &[crate::state::SftpDeleteTarget],
) -> Element<'a, Message> {
    let (title, detail) = if targets.len() == 1 {
        let target = &targets[0];
        let basename = target
            .path
            .rsplit(['/', '\\'])
            .find(|s| !s.is_empty())
            .unwrap_or(&target.path)
            .to_string();
        let detail = if target.is_dir {
            format!("\"{}\", {}", basename, t("folder_and_contents"))
        } else {
            format!("\"{}\"", basename)
        };
        (t("delete_item_question").to_string(), detail)
    } else {
        let folder_count = targets.iter().filter(|t| t.is_dir).count();
        let file_count = targets.len() - folder_count;
        let detail = match (folder_count, file_count) {
            (0, n) => format!("{} {}", n, t("files_lower")),
            (n, 0) => format!("{} {}", n, t("folders_recursive_lower")),
            (f, fi) => format!("{} {} {} {} {}", f, t("folders_recursive_lower"), t("and"), fi, t("files_lower")),
        };
        (
            t("delete_n_items_question").replacen("{n}", &targets.len().to_string(), 1),
            detail,
        )
    };
    let dialog = container(
        column![
            text(title).size(16).color(OryxisColors::t().text_primary),
            Space::new().height(6),
            text(detail).size(13).color(OryxisColors::t().text_muted),
            Space::new().height(16),
            row![
                crate::widgets::styled_button(
                    t("delete"),
                    Message::SftpConfirmDelete,
                    OryxisColors::t().error,
                ),
                Space::new().width(8),
                crate::widgets::styled_button(
                    t("cancel"),
                    Message::SftpCancelDelete,
                    OryxisColors::t().text_muted,
                ),
            ],
        ]
        .padding(24)
        .width(420),
    )
    .style(|_| container::Style {
        background: Some(Background::Color(OryxisColors::t().bg_surface)),
        border: Border {
            radius: Radius::from(12.0),
            color: OryxisColors::t().border,
            width: 1.0,
        },
        ..Default::default()
    });

    let scrim: Element<'_, Message> = MouseArea::new(
        container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5))),
                ..Default::default()
            }),
    )
    .on_press(Message::SftpCancelDelete)
    .into();

    // Wrap the dialog in a MouseArea that swallows clicks via `NoOp`,
    // otherwise events fall through the Stack to the scrim underneath
    // and the modal closes on every click inside the dialog body.
    let centered = container(MouseArea::new(dialog).on_press(Message::NoOp))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill);

    iced::widget::Stack::new()
        .push(scrim)
        .push(centered)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Close-guard modal: shown when the user tries to close an SFTP tab that has
/// an in-flight transfer or an unsaved edit-session. Rendered at the global
/// layer (`view_main`) so it shows regardless of the current surface.
pub(crate) fn close_guard_modal<'a>() -> Element<'a, Message> {
    let dialog = container(
        column![
            text(t("sftp_close_guard_title")).size(16).color(OryxisColors::t().text_primary),
            Space::new().height(6),
            text(t("sftp_close_guard_detail")).size(13).color(OryxisColors::t().text_muted),
            Space::new().height(16),
            row![
                crate::widgets::styled_button(
                    t("close_anyway"),
                    Message::ConfirmCloseSftpTab,
                    OryxisColors::t().error,
                ),
                Space::new().width(8),
                crate::widgets::styled_button(
                    t("cancel"),
                    Message::CancelCloseSftpTab,
                    OryxisColors::t().text_muted,
                ),
            ],
        ]
        .padding(24)
        .width(420),
    )
    .style(|_| container::Style {
        background: Some(Background::Color(OryxisColors::t().bg_surface)),
        border: Border {
            radius: Radius::from(12.0),
            color: OryxisColors::t().border,
            width: 1.0,
        },
        ..Default::default()
    });
    let scrim: Element<'_, Message> = MouseArea::new(
        container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5))),
                ..Default::default()
            }),
    )
    .on_press(Message::CancelCloseSftpTab)
    .into();
    let centered = container(MouseArea::new(dialog).on_press(Message::NoOp))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill);
    iced::widget::Stack::new()
        .push(scrim)
        .push(centered)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Modal for "New folder" / "New file", single text input + create/cancel.
/// `Enter` in the input commits, mirroring the inline rename behaviour.
fn new_entry_modal<'a>(entry: &crate::state::SftpNewEntry) -> Element<'a, Message> {
    let title = match entry.kind {
        SftpEntryKind::Folder => t("new_folder"),
        SftpEntryKind::File => t("new_file"),
    };
    let placeholder = match entry.kind {
        SftpEntryKind::Folder => t("folder_name_placeholder"),
        SftpEntryKind::File => t("file_name_placeholder"),
    };
    let dialog = container(
        column![
            text(title).size(16).color(OryxisColors::t().text_primary),
            Space::new().height(12),
            text_input(placeholder, &entry.input)
                .on_input(Message::SftpNewEntryInput)
                .on_submit(Message::SftpNewEntryCommit)
                .padding(10)
                .size(13)
                .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
            Space::new().height(16),
            row![
                crate::widgets::styled_button(
                    t("create"),
                    Message::SftpNewEntryCommit,
                    OryxisColors::t().accent,
                ),
                Space::new().width(8),
                crate::widgets::styled_button(
                    t("cancel"),
                    Message::SftpNewEntryCancel,
                    OryxisColors::t().text_muted,
                ),
            ],
        ]
        .padding(24)
        .width(380),
    )
    .style(|_| container::Style {
        background: Some(Background::Color(OryxisColors::t().bg_surface)),
        border: Border {
            radius: Radius::from(12.0),
            color: OryxisColors::t().border,
            width: 1.0,
        },
        ..Default::default()
    });

    let scrim: Element<'_, Message> = MouseArea::new(
        container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5))),
                ..Default::default()
            }),
    )
    .on_press(Message::SftpNewEntryCancel)
    .into();

    // Wrap the dialog in a MouseArea that swallows clicks via `NoOp`,
    // otherwise events fall through the Stack to the scrim underneath
    // and the modal closes on every click inside the dialog body.
    let centered = container(MouseArea::new(dialog).on_press(Message::NoOp))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill);

    iced::widget::Stack::new()
        .push(scrim)
        .push(centered)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Modal shown while an edit-in-place session is active. The user has
/// the temp file open in their OS editor, when they're done they come
/// back here and either save the changes back to the remote or discard.
/// Backdrop is non-dismissable on click; the user must explicitly choose
/// so a stray click can't drop their edits.
fn edit_in_place_modal<'a>(
    session: &crate::state::EditSession,
) -> Element<'a, Message> {
    let (status_text, status_color) = if session.dirty {
        (
            t("edit_changes_detected").to_string(),
            OryxisColors::t().accent,
        )
    } else {
        (
            t("edit_waiting_changes").to_string(),
            OryxisColors::t().text_muted,
        )
    };
    let title = if session.dirty {
        t("edit_title_modified")
    } else {
        t("edit_title_clean")
    };
    let dialog = container(
        column![
            text(title).size(16).color(OryxisColors::t().text_primary),
            Space::new().height(6),
            text(session.label.clone())
                .size(13)
                .color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            text(format!("{} {}", t("local_copy_label"), session.temp_path.display()))
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(4),
            text(session.remote_path.clone())
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(16),
            text(status_text).size(12).color(status_color),
            Space::new().height(16),
            row![
                crate::widgets::styled_button(
                    t("save_to_remote"),
                    Message::SftpEditSave,
                    OryxisColors::t().accent,
                ),
                Space::new().width(8),
                crate::widgets::styled_button(
                    t("discard"),
                    Message::SftpEditDiscard,
                    OryxisColors::t().text_muted,
                ),
            ],
        ]
        .padding(24)
        .width(440),
    )
    .style(|_| container::Style {
        background: Some(Background::Color(OryxisColors::t().bg_surface)),
        border: Border {
            radius: Radius::from(12.0),
            color: OryxisColors::t().border,
            width: 1.0,
        },
        ..Default::default()
    });

    // Solid scrim with no on_press, the modal is intentionally
    // non-dismissable on backdrop click. Clicking outside does nothing
    // so the user is forced to make an explicit save/discard choice and
    // can't lose their edits to a misclick.
    let scrim: Element<'_, Message> = container(Space::new())
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5))),
            ..Default::default()
        })
        .into();

    // Wrap the dialog in a MouseArea that swallows clicks via `NoOp`,
    // otherwise events fall through the Stack to the scrim underneath
    // and the modal closes on every click inside the dialog body.
    let centered = container(MouseArea::new(dialog).on_press(Message::NoOp))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill);

    iced::widget::Stack::new()
        .push(scrim)
        .push(centered)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Floating preview shown next to the cursor while a row is being
/// dragged across panes. Solid pill with the dragged item's label or
/// "N items"; non-interactive so the eventual mouse-release still hits
/// the underlying drop target.
pub(crate) fn drag_ghost<'a>(label: &str) -> Element<'a, Message> {
    container(
        row![
            iced_fonts::lucide::file().size(12).color(Color::WHITE),
            Space::new().width(8),
            text(label.to_string())
                .size(12)
                .color(Color::WHITE)
                .font(iced::Font {
                    weight: iced::font::Weight::Medium,
                    ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                }),
        ]
        .align_y(iced::Alignment::Center),
    )
    .padding(Padding { top: 6.0, right: 12.0, bottom: 6.0, left: 12.0 })
    .style(|_| container::Style {
        background: Some(Background::Color(Color { a: 0.92, ..OryxisColors::t().accent })),
        border: Border {
            radius: Radius::from(8.0),
            color: OryxisColors::t().accent,
            width: 1.0,
        },
        shadow: iced::Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
            offset: iced::Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
        ..Default::default()
    })
    .into()
}

/// Properties dialog, shows the standard file metadata (path, size,
/// mtime, owner) and a 3×3 grid of permission checkboxes for r/w/x
/// across owner / group / others. Apply runs chmod; bits the dialog
/// doesn't render (setuid/setgid/sticky) are preserved verbatim from
/// the original mode.
fn properties_modal<'a>(
    props: &crate::state::PropertiesView,
) -> Element<'a, Message> {
    let basename = props
        .path
        .rsplit(['/', '\\'])
        .find(|s| !s.is_empty())
        .unwrap_or(&props.path)
        .to_string();
    let kind = if props.is_dir { t("kind_folder") } else { t("kind_file") };
    let mtime_str = props
        .mtime
        .and_then(|secs| chrono::DateTime::<chrono::Utc>::from_timestamp(secs as i64, 0))
        .map(|dt| dt.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "-".to_string());
    let owner_str = match (props.owner_uid, props.owner_gid) {
        (Some(u), Some(g)) => format!("uid {u} · gid {g}"),
        (Some(u), None) => format!("uid {u}"),
        _ => "-".to_string(),
    };
    let mode_octal = format!("{:o}", (props.original_mode & !0o777) | props.bits.to_mode());

    let info_row = |label: &str, value: String| -> Element<'a, Message> {
        row![
            text(label.to_string())
                .size(11)
                .color(OryxisColors::t().text_muted)
                .width(Length::Fixed(80.0)),
            text(value).size(12).color(OryxisColors::t().text_primary),
        ]
        .align_y(iced::Alignment::Center)
        .into()
    };

    let header_row = |label: &str| -> Element<'a, Message> {
        text(label.to_string())
            .size(11)
            .color(OryxisColors::t().text_muted)
            .into()
    };

    let perm_check = |checked: bool, bit: crate::state::PermBit| -> Element<'a, Message> {
        let mark = if checked {
            iced_fonts::lucide::circle_check()
                .size(14)
                .color(OryxisColors::t().accent)
        } else {
            iced_fonts::lucide::circle_minus()
                .size(14)
                .color(OryxisColors::t().text_muted)
        };
        button(mark)
            .on_press(Message::SftpPropertiesToggleBit(bit))
            .padding(Padding { top: 4.0, right: 6.0, bottom: 4.0, left: 6.0 })
            .style(|_, status| {
                let bg = match status {
                    BtnStatus::Hovered => OryxisColors::t().bg_hover,
                    _ => Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(4.0), ..Default::default() },
                    ..Default::default()
                }
            })
            .into()
    };

    let perm_row = |label: &str, r: (bool, crate::state::PermBit), w: (bool, crate::state::PermBit), x: (bool, crate::state::PermBit)| -> Element<'a, Message> {
        row![
            text(label.to_string())
                .size(12)
                .color(OryxisColors::t().text_secondary)
                .width(Length::Fixed(80.0)),
            perm_check(r.0, r.1),
            Space::new().width(8),
            perm_check(w.0, w.1),
            Space::new().width(8),
            perm_check(x.0, x.1),
        ]
        .align_y(iced::Alignment::Center)
        .into()
    };

    let perm_grid = column![
        row![
            Space::new().width(Length::Fixed(80.0)),
            container(text("R").size(11).color(OryxisColors::t().text_muted))
                .width(Length::Fixed(26.0))
                .center_x(Length::Fixed(26.0)),
            Space::new().width(8),
            container(text("W").size(11).color(OryxisColors::t().text_muted))
                .width(Length::Fixed(26.0))
                .center_x(Length::Fixed(26.0)),
            Space::new().width(8),
            container(text("X").size(11).color(OryxisColors::t().text_muted))
                .width(Length::Fixed(26.0))
                .center_x(Length::Fixed(26.0)),
        ],
        Space::new().height(4),
        perm_row(
            t("perm_owner"),
            (props.bits.user_r, crate::state::PermBit::UserR),
            (props.bits.user_w, crate::state::PermBit::UserW),
            (props.bits.user_x, crate::state::PermBit::UserX),
        ),
        Space::new().height(2),
        perm_row(
            t("perm_group"),
            (props.bits.group_r, crate::state::PermBit::GroupR),
            (props.bits.group_w, crate::state::PermBit::GroupW),
            (props.bits.group_x, crate::state::PermBit::GroupX),
        ),
        Space::new().height(2),
        perm_row(
            t("perm_others"),
            (props.bits.other_r, crate::state::PermBit::OtherR),
            (props.bits.other_w, crate::state::PermBit::OtherW),
            (props.bits.other_x, crate::state::PermBit::OtherX),
        ),
    ];

    let mut content = column![
        text(basename)
            .size(15)
            .font(iced::Font {
                weight: iced::font::Weight::Semibold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            })
            .color(OryxisColors::t().text_primary),
        Space::new().height(4),
        text(props.path.clone())
            .size(11)
            .color(OryxisColors::t().text_muted),
        Space::new().height(14),
        info_row(t("type_label"), kind.to_string()),
        Space::new().height(4),
        info_row(t("col_size"), format_size(props.size)),
        Space::new().height(4),
        info_row(t("col_modified"), mtime_str),
        Space::new().height(4),
        info_row(t("perm_owner"), owner_str),
        Space::new().height(4),
        info_row(t("info_mode"), format!("0{}", mode_octal)),
        Space::new().height(16),
        header_row(t("permissions")),
        Space::new().height(8),
    ];
    content = content.push(perm_grid);
    if let Some(err) = &props.error {
        content = content.push(Space::new().height(10));
        content = content.push(
            text(err.clone()).size(11).color(OryxisColors::t().error),
        );
    }
    content = content.push(Space::new().height(18));
    let apply_label = if props.applying { t("applying") } else { t("apply") };
    content = content.push(
        row![
            ghost_button(t("close"), Message::SftpPropertiesClose),
            Space::new().width(Length::Fill),
            primary_button(apply_label, Message::SftpPropertiesApply, OryxisColors::t().accent),
        ]
        .align_y(iced::Alignment::Center),
    );

    let dialog = container(content.padding(22).width(440))
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border {
                radius: Radius::from(12.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            shadow: iced::Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.30),
                offset: iced::Vector::new(0.0, 8.0),
                blur_radius: 24.0,
            },
            ..Default::default()
        });

    let scrim: Element<'_, Message> = MouseArea::new(
        container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5))),
                ..Default::default()
            }),
    )
    .on_press(Message::SftpPropertiesClose)
    .into();

    // Wrap the dialog in a MouseArea that swallows clicks via `NoOp`,
    // otherwise events fall through the Stack to the scrim underneath
    // and the modal closes on every click inside the dialog body.
    let centered = container(MouseArea::new(dialog).on_press(Message::NoOp))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill);

    iced::widget::Stack::new()
        .push(scrim)
        .push(centered)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Modal shown when an upload would clobber an existing remote file.
/// Lays out the choices as a single horizontal row of buttons
/// destructive primary on the right, secondary outlined options in the
/// middle, ghost-style cancel on the left, so the modal stays compact
/// instead of stacking four heavy buttons vertically. The scrim is
/// non-dismissable: the user must pick something explicitly.
fn overwrite_modal<'a>(
    prompt: &crate::state::OverwritePrompt,
) -> Element<'a, Message> {
    let size_hint = if prompt.src_size == prompt.dst_size {
        t("size_both_identical")
            .replacen("{size}", &format_size(prompt.src_size), 1)
    } else {
        t("size_local_remote")
            .replacen("{local}", &format_size(prompt.src_size), 1)
            .replacen("{remote}", &format_size(prompt.dst_size), 1)
    };

    let mut content = column![
        text(t("already_exists").replacen("{name}", &prompt.basename, 1))
            .size(15)
            .font(iced::Font {
                weight: iced::font::Weight::Semibold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            })
            .color(OryxisColors::t().text_primary),
        Space::new().height(4),
        text(prompt.dst_dir.clone())
            .size(11)
            .color(OryxisColors::t().text_muted),
        Space::new().height(2),
        text(size_hint).size(11).color(OryxisColors::t().text_muted),
    ]
    .width(Length::Fill);

    if prompt.multi {
        // Sticky decision lets the user clear out a long upload's
        // collisions in one click instead of answering N times.
        content = content.push(Space::new().height(14));
        content = content.push(overwrite_apply_to_all_checkbox(prompt.apply_to_all));
    }

    content = content.push(Space::new().height(18));
    content = content.push(
        row![
            ghost_button(
                t("cancel"),
                Message::SftpResolveOverwrite(crate::state::OverwriteAction::Cancel),
            ),
            Space::new().width(Length::Fill),
            outlined_button(
                t("replace_if_different"),
                Message::SftpResolveOverwrite(crate::state::OverwriteAction::ReplaceIfDifferent),
            ),
            Space::new().width(8),
            outlined_button(
                t("duplicate"),
                Message::SftpResolveOverwrite(crate::state::OverwriteAction::Duplicate),
            ),
            Space::new().width(8),
            primary_button(
                t("replace"),
                Message::SftpResolveOverwrite(crate::state::OverwriteAction::Replace),
                OryxisColors::t().error,
            ),
        ]
        .align_y(iced::Alignment::Center),
    );

    let dialog = container(content.padding(22).width(560))
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border {
                radius: Radius::from(12.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            shadow: iced::Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.30),
                offset: iced::Vector::new(0.0, 8.0),
                blur_radius: 24.0,
            },
            ..Default::default()
        });

    // Non-dismissable scrim, clicking outside is not a valid answer
    // (users could lose data by deciding the wrong way), so we swallow
    // the press without doing anything. The user must pick a button.
    let scrim: Element<'_, Message> = container(Space::new())
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5))),
            ..Default::default()
        })
        .into();

    // Wrap the dialog in a MouseArea that swallows clicks via `NoOp`,
    // otherwise events fall through the Stack to the scrim underneath
    // and the modal closes on every click inside the dialog body.
    let centered = container(MouseArea::new(dialog).on_press(Message::NoOp))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill);

    iced::widget::Stack::new()
        .push(scrim)
        .push(centered)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Filled primary action button, destructive variants pass red, neutral
/// pass accent. Slightly more compact than `widgets::styled_button` so it
/// sits well in a horizontal modal footer row.
fn primary_button<'a>(label: &'a str, msg: Message, color: Color) -> Element<'a, Message> {
    // Accent CTAs share the theme's `button_text`; semantic colors
    // (success/error/etc.) auto-pick by luminance.
    let fg = if color == OryxisColors::t().accent {
        OryxisColors::t().button_text
    } else {
        crate::theme::contrast_text_for(color)
    };
    button(
        text(label.to_owned())
            .size(12)
            .font(iced::Font {
                weight: iced::font::Weight::Semibold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            })
            .color(fg),
    )
    .on_press(msg)
    .padding(Padding { top: 6.0, right: 16.0, bottom: 6.0, left: 16.0 })
    .style(move |_, status| {
        let bg = match status {
            BtnStatus::Hovered => Color {
                a: 1.0,
                r: (color.r + 0.06).min(1.0),
                g: (color.g + 0.06).min(1.0),
                b: (color.b + 0.06).min(1.0),
            },
            _ => color,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    })
    .into()
}

/// Outlined secondary button, transparent fill with a subtle border.
/// Hover fills with a faint accent tint to communicate clickability
/// without competing with the primary action visually.
fn outlined_button<'a>(label: &'a str, msg: Message) -> Element<'a, Message> {
    button(
        text(label.to_owned())
            .size(12)
            .color(OryxisColors::t().text_primary),
    )
    .on_press(msg)
    .padding(Padding { top: 6.0, right: 14.0, bottom: 6.0, left: 14.0 })
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => Color { a: 0.10, ..OryxisColors::t().accent },
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                radius: Radius::from(6.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        }
    })
    .into()
}

/// Ghost button, pure text on transparent, hover-only background tint.
/// Right for the lowest-emphasis action (Cancel here).
fn ghost_button<'a>(label: &'a str, msg: Message) -> Element<'a, Message> {
    button(
        text(label.to_owned())
            .size(12)
            .color(OryxisColors::t().text_secondary),
    )
    .on_press(msg)
    .padding(Padding { top: 6.0, right: 14.0, bottom: 6.0, left: 14.0 })
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().bg_hover,
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    })
    .into()
}

/// Small click-to-toggle row with a square indicator + label. iced 0.14
/// has a built-in checkbox widget, but matching it to the rest of the
/// modal's chrome takes more code than just rolling a button-like row.
fn overwrite_apply_to_all_checkbox<'a>(checked: bool) -> Element<'a, Message> {
    let mark = if checked {
        iced_fonts::lucide::circle_check()
            .size(14)
            .color(OryxisColors::t().accent)
    } else {
        iced_fonts::lucide::circle_minus()
            .size(14)
            .color(OryxisColors::t().text_muted)
    };
    button(
        row![
            mark,
            Space::new().width(8),
            text(t("apply_to_remaining"))
                .size(12)
                .color(OryxisColors::t().text_secondary),
        ]
        .align_y(iced::Alignment::Center),
    )
    .on_press(Message::SftpToggleApplyToAll)
    .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 4.0 })
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().bg_hover,
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(4.0), ..Default::default() },
            ..Default::default()
        }
    })
    .into()
}

/// Per-file progress panel that rises above the transfer strip when the
/// user clicks it. Lists finished items, the one in flight, and what's
/// still queued, so a multi-file (or slow single-file) transfer shows
/// exactly where it is.
fn transfer_file_panel<'a>(
    transfer: &crate::state::TransferState,
    done_log: &[String],
) -> Element<'a, Message> {
    fn marker_row<'b>(
        glyph: &str,
        glyph_color: Color,
        label: String,
        label_color: Color,
    ) -> Element<'b, Message> {
        crate::widgets::dir_row(vec![
            text(glyph.to_string()).size(12).color(glyph_color).into(),
            Space::new().width(8).into(),
            text(label).size(12).color(label_color).into(),
        ])
        .align_y(iced::Alignment::Center)
        .into()
    }

    let theme = OryxisColors::t();
    let mut list = column![].spacing(3);
    for label in done_log {
        list = list.push(marker_row("\u{2713}", theme.success, label.clone(), theme.text_secondary));
    }
    if let Some(cur) = &transfer.current {
        list = list.push(marker_row("\u{25B8}", theme.accent, cur.clone(), theme.text_primary));
    }
    for item in &transfer.queue {
        let label = crate::sftp_helpers::transfer_item_label(item);
        list = list.push(marker_row("\u{2022}", theme.text_muted, label, theme.text_muted));
    }
    container(scrollable(list).height(Length::Fixed(180.0)))
        .padding(10)
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border {
                radius: Radius::from(8.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        })
        .into()
}

/// Bottom-of-view strip that surfaces an in-progress folder transfer:
/// kind label, current item, count, slim progress bar, and a cancel
/// button. Stays compact so the file panes lose as little vertical
/// space as possible.
fn transfer_progress_strip<'a>(
    transfer: &crate::state::TransferState,
    bytes_done: u64,
    bytes_total: u64,
) -> Element<'a, Message> {
    let label = match transfer.kind {
        crate::state::TransferKind::Upload => t("transfer_uploading"),
        crate::state::TransferKind::Download => t("transfer_downloading"),
        crate::state::TransferKind::DuplicateLocal => t("transfer_duplicating"),
        crate::state::TransferKind::Relay => t("transfer_relaying"),
    };
    let current = transfer
        .current
        .clone()
        .unwrap_or_else(|| t("transfer_preparing").to_string());
    // When the total byte size is known, drive the bar + count by bytes so a
    // single large file shows live movement; otherwise fall back to item
    // counts (e.g. all-directory transfers, where sizes are 0).
    let (count, pct) = if bytes_total > 0 {
        let done = bytes_done.min(bytes_total);
        (
            format!("{} / {}", format_size(done), format_size(bytes_total)),
            (done as f32 / bytes_total as f32).clamp(0.0, 1.0),
        )
    } else if transfer.total == 0 {
        (format!("{} / {}", transfer.completed, transfer.total), 0.0)
    } else {
        (
            format!("{} / {}", transfer.completed, transfer.total),
            (transfer.completed as f32 / transfer.total as f32).clamp(0.0, 1.0),
        )
    };
    // Ratio-based progress bar: a filled portion + a remaining portion in
    // a row so the `FillPortion` weights actually divide the track. (A lone
    // FillPortion child fills 100% regardless of its weight, which made the
    // bar look full even at 0%.)
    let filled = (pct * 1000.0) as u16;
    let remaining = 1000u16.saturating_sub(filled);
    let bar = container(
        row![
            container(Space::new())
                .width(Length::FillPortion(filled))
                .height(Length::Fixed(4.0))
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().accent)),
                    border: Border { radius: Radius::from(2.0), ..Default::default() },
                    ..Default::default()
                }),
            container(Space::new())
                .width(Length::FillPortion(remaining))
                .height(Length::Fixed(4.0)),
        ]
        .width(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fixed(4.0))
    .style(|_| container::Style {
        background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.05))),
        border: Border { radius: Radius::from(2.0), ..Default::default() },
        ..Default::default()
    });

    let info = column![
        row![
            text(format!("{} {}", label, transfer.root_label))
                .size(11)
                .color(OryxisColors::t().text_secondary),
            Space::new().width(Length::Fill),
            text(count).size(11).color(OryxisColors::t().text_muted),
        ]
        .align_y(iced::Alignment::Center),
        Space::new().height(2),
        text(current)
            .size(10)
            .color(OryxisColors::t().text_muted),
        Space::new().height(6),
        bar,
    ]
    .width(Length::Fill);

    let cancel_btn = button(
        text(t("cancel")).size(11).color(OryxisColors::t().text_secondary),
    )
    .on_press(Message::SftpCancelTransfer)
    .padding(Padding { top: 4.0, right: 10.0, bottom: 4.0, left: 10.0 })
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().bg_hover,
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                radius: Radius::from(6.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        }
    });

    container(
        row![info, Space::new().width(12), cancel_btn]
            .align_y(iced::Alignment::Center),
    )
    .padding(Padding { top: 8.0, right: 14.0, bottom: 8.0, left: 14.0 })
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
        border: Border {
            width: 1.0,
            color: OryxisColors::t().border,
            radius: Radius::from(0.0),
        },
        ..Default::default()
    })
    .into()
}

/// The always-visible footer bar carrying the message-log toggle. Shows
/// the log entry count and highlights when the panel is open.
fn sftp_log_bar<'a>(open: bool, count: usize) -> Element<'a, Message> {
    let tint = if open {
        OryxisColors::t().accent
    } else {
        OryxisColors::t().text_muted
    };
    let label_color = if open {
        OryxisColors::t().text_primary
    } else {
        OryxisColors::t().text_muted
    };
    let chevron = if open {
        iced_fonts::lucide::chevron_down()
    } else {
        iced_fonts::lucide::chevron_up()
    };
    let toggle = button(
        row![
            iced_fonts::lucide::list().size(13).color(tint),
            Space::new().width(6),
            text(format!("{} ({})", t("sftp_log"), count))
                .size(11)
                .color(label_color),
            Space::new().width(6),
            chevron.size(10).color(tint),
        ]
        .align_y(iced::Alignment::Center),
    )
    .on_press(Message::SftpToggleLog)
    .padding(Padding { top: 3.0, right: 10.0, bottom: 3.0, left: 10.0 })
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().bg_hover,
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(4.0), ..Default::default() },
            ..Default::default()
        }
    });
    container(
        row![toggle, Space::new().width(Length::Fill)].align_y(iced::Alignment::Center),
    )
    .width(Length::Fill)
    .padding(Padding { top: 2.0, right: 8.0, bottom: 2.0, left: 8.0 })
    .style(|_| container::Style {
        background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
        border: Border {
            width: 1.0,
            color: OryxisColors::t().border,
            radius: Radius::from(0.0),
        },
        ..Default::default()
    })
    .into()
}

/// FileZilla-style message-log panel: a fixed-height scrollable list of
/// timestamped events, colour-coded by level. Newest entries sit at the
/// bottom. Strings are cloned so the element doesn't borrow the log.
fn sftp_log_panel<'a>(log: &[crate::state::SftpLogEntry]) -> Element<'a, Message> {
    use crate::state::SftpLogLevel;
    let mut col = column![]
        .spacing(1)
        .padding(Padding { top: 6.0, right: 10.0, bottom: 6.0, left: 10.0 })
        .width(Length::Fill);
    if log.is_empty() {
        col = col.push(
            text(t("sftp_log_empty"))
                .size(11)
                .color(OryxisColors::t().text_muted),
        );
    } else {
        for e in log {
            let color = match e.level {
                SftpLogLevel::Info => OryxisColors::t().text_secondary,
                SftpLogLevel::Ok => OryxisColors::t().success,
                SftpLogLevel::Warn => OryxisColors::t().warning,
                SftpLogLevel::Error => OryxisColors::t().error,
            };
            col = col.push(
                row![
                    text(e.time.clone())
                        .size(11)
                        .color(OryxisColors::t().text_muted)
                        .width(Length::Fixed(64.0)),
                    text(e.text.clone()).size(11).color(color).width(Length::Fill),
                ]
                .spacing(8),
            );
        }
    }
    container(scrollable(col).width(Length::Fill).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fixed(160.0))
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border {
                width: 1.0,
                color: OryxisColors::t().border,
                radius: Radius::from(0.0),
            },
            ..Default::default()
        })
        .into()
}

fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut idx = 0;
    while value >= 1024.0 && idx < UNITS.len() - 1 {
        value /= 1024.0;
        idx += 1;
    }
    if idx == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else {
        format!("{:.1} {}", value, UNITS[idx])
    }
}
