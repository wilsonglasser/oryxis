//! SFTP browser view, dual-pane (local | remote) file manager.

pub(crate) use iced::border::Radius;
pub(crate) use iced::widget::button::Status as BtnStatus;
pub(crate) use iced::widget::{button, container, scrollable, text, text_input, MouseArea, Space};
use iced::widget::{column, row};
pub(crate) use iced::{Background, Border, Color, Element, Length, Padding};

pub(crate) use crate::app::{Message, Oryxis};
pub(crate) use crate::i18n::t;
pub(crate) use crate::state::{SftpEntryKind, SftpPaneSide};
pub(crate) use crate::theme::OryxisColors;
pub(crate) use crate::widgets::dir_align_x;

pub(crate) const ROW_HEIGHT: f32 = 28.0;

/// Which pane-anchored popover is open (layered at the view level so its
/// dismiss scrim covers the whole view).
#[derive(Clone, Copy)]
enum PanePopover {
    Actions,
    Filter,
}


// Themed view-helper submodules, split out of this file.
mod breadcrumb;
mod columns;
mod formatting;
mod menus;
mod modals;
mod rows;
mod transfers;

pub(crate) use breadcrumb::*;
pub(crate) use columns::*;
pub(crate) use formatting::*;
pub(crate) use menus::*;
pub(crate) use modals::*;
pub(crate) use rows::*;
pub(crate) use transfers::*;

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
            body_col = body_col.push(sftp_log_divider());
            body_col = body_col.push(sftp_log_panel(&self.sftp.log, self.sftp.log_height));
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
        let cols_total = cols_total_width(&ordered_cols, col_widths);
        let layout = ColLayout::resolve(cols_total, pane_avail);
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
            let mut col = column![].spacing(0);
            if pane.local_path.parent().is_some() {
                let parent_selected =
                    self.sftp.parent_cursor && self.sftp.focused_side == side;
                col = col.push(parent_row(
                    side,
                    parent_selected,
                    self.sftp.suppress_hover,
                    &ordered_cols,
                    col_widths,
                    layout,
                ));
            }
            if let Some(err) = &pane.error {
                // Keep the ".." row above so a permission-denied (or any
                // other read) failure can't trap the user in the folder;
                // the error stands in for the entries we couldn't list.
                col = col.push(
                    container(text(err.clone()).size(12).color(OryxisColors::t().error))
                        .padding(12),
                );
            } else {
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
                        self.sftp.suppress_hover,
                        &ordered_cols,
                        col_widths,
                        layout,
                    ));
                }
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
                let parent_selected =
                    self.sftp.parent_cursor && self.sftp.focused_side == side;
                col = col.push(parent_row(
                    side,
                    parent_selected,
                    self.sftp.suppress_hover,
                    &ordered_cols,
                    col_widths,
                    layout,
                ));
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
                    self.sftp.suppress_hover,
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
