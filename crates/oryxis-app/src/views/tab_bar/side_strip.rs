//! Side-docked (left / right) vertical tab strip (issue #87).
//!
//! When `tab_bar_position` is `left` or `right` the tabs stack as a
//! vertical list on that window edge. Every chip is rendered by the same
//! `strip_tab_element` the horizontal strips use, at one uniform row
//! width; compact pinned chips pack several per row (Edge-style pinned
//! grid). Left / right are physical edges by user choice, so RTL never
//! flips the strip; the chips' inner rows still mirror through
//! `dir_row` like everywhere else.
//!
//! Three side-only options compose on top (Settings -> Interface):
//! `pinned_tabs_top_bar` docks the pinned tabs with the window chrome
//! (the slim top bar, or a fixed group under this strip's header when
//! that bar is hidden); `side_hide_top_bar` removes the top bar and
//! moves the whole titlebar contract in here (header row with burger +
//! Home + compact window buttons, empty areas drag the window,
//! double-click maximizes); `side_full_height` is layout-only
//! (`main_layout` keeps the status bar off this strip's column).

use super::*;

/// Total width of the side-docked strip, gutters included.
pub(crate) const SIDE_STRIP_WIDTH: f32 = 216.0;
/// Uniform row width of every tab chip inside the strip (the strip
/// minus its 8px side gutters).
pub(crate) const SIDE_TAB_WIDTH: f32 = SIDE_STRIP_WIDTH - 16.0;
/// Rendered height of one strip row: a chip renders at `TAB_ROW_HEIGHT`
/// (content box + the button's default 5px top/bottom paddings).
const SIDE_ROW_HEIGHT: f32 = TAB_ROW_HEIGHT;
/// Compact window-chrome cell used by the strip header when the top
/// bar is hidden (the standard 46px cells would eat half the strip).
const HEADER_CHROME_W: f32 = 28.0;
const HEADER_CHROME_H: f32 = 32.0;

impl Oryxis {
    /// The vertical tab strip for the left / right docked layout: an
    /// optional titlebar header (hidden-top-bar mode), an optional
    /// fixed pinned group, then the scrolling tab list with `+`; `⋯`
    /// joins a docked footer once the list overflows the viewport.
    pub(crate) fn view_side_tab_strip(&self) -> Element<'_, Message> {
        let hide_top_bar = self.setting_side_hide_top_bar;
        let pins_top = self.setting_pinned_tabs_top_bar;
        let compact_pins = self.setting_pinned_tab_style == "compact";
        let solid_fill =
            self.setting_tab_fill_style == "solid" || self.setting_performance_mode;
        let dragging_any = self.tab_drag.map(|d| d.active).unwrap_or(false);
        let ctx = StripCtx {
            privacy_terms: self.privacy_terms(),
            close_on_right: self.setting_tab_close_button_side == "right",
            compact_pins,
            solid_fill,
            dragging_any,
            // Uniform rows: the drag width IS the row width, so the
            // live-slide never changes the strip geometry.
            drag_uniform_w: SIDE_TAB_WIDTH,
            uniform_w: Some(SIDE_TAB_WIDTH),
            session_widths: Vec::new(),
            number_px: self.tab_number_px(),
        };
        let chips_per_row = ((SIDE_TAB_WIDTH + TAB_SPACING)
            / (CHIP_W + TAB_SPACING))
            .floor()
            .max(1.0) as usize;

        // Fixed head (never scrolls). Hidden-top-bar mode: the titlebar
        // contract lives here: burger + Home + drag gap + side-panel
        // toggle + compact window chrome. `pinned_tabs_top_bar` then
        // docks the pinned tabs as an always-visible group right under
        // it (Zen-style essentials), since there is no top bar to host
        // them.
        let mut head: Vec<Element<'_, Message>> = Vec::new();
        if hide_top_bar {
            let mut header: Vec<Element<'_, Message>> = vec![
                burger_menu_btn(self.show_burger_menu),
                self.home_area_tab(solid_fill),
                Space::new().width(Length::Fill).into(),
            ];
            if self.active_tab.is_some() {
                header.push(sidebar_btn(SIDEBAR_TOGGLE_WIDTH, HEADER_CHROME_H));
            }
            header.push(self.window_chrome_row(HEADER_CHROME_W, HEADER_CHROME_H).into());
            head.push(
                crate::widgets::dir_row(header)
                    .align_y(iced::Alignment::Center)
                    .into(),
            );
        }
        let pins_docked_here = hide_top_bar && pins_top;
        let mut pins_row_count = 0usize;
        if pins_docked_here {
            let rows = self.side_pins_rows(&ctx, chips_per_row, compact_pins);
            pins_row_count = rows.len();
            head.extend(rows);
        }

        // Scrolling list. With `pinned_tabs_top_bar` the pinned entries
        // live with the chrome (top bar or the fixed head above), so
        // they are skipped here; otherwise consecutive compact pinned
        // chips pack into rows at the top of the list (`strip_order` is
        // pinned-first) and everything else stacks one chip per row.
        let mut items: Vec<Element<'_, Message>> = Vec::new();
        let mut chip_row: Vec<Element<'_, Message>> = Vec::new();
        let mut row_count = 0usize;
        // Slots count the full strip order (see `strip_tab_element`):
        // with the pins docked in the top bar this list skips them, and
        // a local counter would renumber the rest from 1.
        for (slot, entry) in self.strip_order().into_iter().enumerate() {
            let pinned = self.strip_entry_pinned(entry);
            if pins_top && pinned {
                continue;
            }
            let el = self.strip_tab_element(&ctx, entry, slot);
            if compact_pins && pinned {
                chip_row.push(el);
                if chip_row.len() == chips_per_row {
                    items.push(
                        row(std::mem::take(&mut chip_row))
                            .spacing(TAB_SPACING)
                            .into(),
                    );
                    row_count += 1;
                }
            } else {
                if !chip_row.is_empty() {
                    items.push(
                        row(std::mem::take(&mut chip_row))
                            .spacing(TAB_SPACING)
                            .into(),
                    );
                    row_count += 1;
                }
                items.push(el);
                row_count += 1;
            }
        }
        if !chip_row.is_empty() {
            items.push(row(chip_row).spacing(TAB_SPACING).into());
            row_count += 1;
        }

        // Overflow: the rows (plus the trailing `+`) don't fit the
        // strip's viewport, so the `+` docks into a fixed footer with
        // the `⋯` jump button and the list alone scrolls. Mirrors the
        // horizontal strip's docked-plus / scroll-mode pair; vertical
        // rows never compress, so one trigger covers both.
        let strip_top = if hide_top_bar { 0.0 } else { BAR_HEIGHT + 1.0 };
        // The pinned dock is capped at ~40% of the window: an always-
        // visible head that outgrew the viewport would push the list
        // (and every unpinned tab) clean off screen with no way to
        // reach them. Past the cap the dock scrolls on its own.
        let pins_h_raw = pins_row_count as f32 * (SIDE_ROW_HEIGHT + TAB_SPACING);
        let pins_h_max =
            (self.window_size.height * 0.4).max(3.0 * (SIDE_ROW_HEIGHT + TAB_SPACING));
        let pins_overflow = pins_h_raw > pins_h_max;
        let head_h = if hide_top_bar { BAR_HEIGHT } else { 0.0 }
            + pins_h_raw.min(pins_h_max);
        let viewport_h =
            (self.window_size.height - strip_top - head_h - 40.0).max(120.0);
        let content_h = (row_count as f32 + 1.0) * (SIDE_ROW_HEIGHT + TAB_SPACING);
        let overflow = content_h > viewport_h;

        // Same `+` affordance as the horizontal strip: bounds reported
        // for the split-menu anchor, drag-to-end drop target.
        let plus_btn: Element<'_, Message> = MouseArea::new(crate::widgets::bounds_reporter(
            new_tab_btn(!overflow),
            self.plus_btn_bounds.clone(),
        ))
        .on_enter(Message::Tabs(TabsMessage::TabDragToEnd))
        .into();
        let mut footer: Option<Element<'_, Message>> = None;
        if overflow {
            footer = Some(
                row(vec![plus_btn, Space::new().width(2).into(), tab_jump_btn()])
                    .align_y(iced::Alignment::Center)
                    .into(),
            );
        } else {
            items.push(plus_btn);
        }

        // Vertical scrollable, scrollbar zeroed out like the horizontal
        // strip (the wheel still scrolls it natively).
        let strip_scroll = scrollable(
            iced::widget::Column::with_children(items).spacing(TAB_SPACING),
        )
        .id(iced::widget::Id::new("tab-scroll"))
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::new().width(0.0).scroller_width(0.0),
        ))
        .width(Length::Fill)
        .height(Length::Fill);

        // Constant-shape column: the header, pinned dock, list and
        // footer live in FIXED child slots (zero-sized Spaces when
        // absent). iced pairs widget state by child position and every
        // stateless widget shares one tag, so a slot that appears or
        // disappears would silently shift the scrollable one position
        // over and hand its state (the scroll offset) to a sibling.
        // The placeholder must be a SHRINK Space: this fork's
        // Column::push void-filters any zero-FIXED child, which would
        // silently drop the slot (see the skeleton note in
        // `main_layout.rs`).
        let empty = || -> Element<'_, Message> { Space::new().into() };
        let mut head_slots = head.into_iter();
        let header_slot = head_slots.next().unwrap_or_else(empty);
        let pins_slot: Element<'_, Message> = {
            let rest: Vec<Element<'_, Message>> = head_slots.collect();
            if rest.is_empty() {
                empty()
            } else {
                let dock = iced::widget::Column::with_children(rest).spacing(TAB_SPACING);
                if pins_overflow {
                    scrollable(dock)
                        .id(iced::widget::Id::new("side-pins-scroll"))
                        .direction(scrollable::Direction::Vertical(
                            scrollable::Scrollbar::new().width(0.0).scroller_width(0.0),
                        ))
                        .width(Length::Fill)
                        .height(Length::Fixed(pins_h_max))
                        .into()
                } else {
                    dock.into()
                }
            }
        };
        let footer_slot = footer.unwrap_or_else(empty);
        let inner = iced::widget::Column::with_children(vec![
            header_slot,
            pins_slot,
            strip_scroll.into(),
            footer_slot,
        ])
        .spacing(TAB_SPACING);

        // Strip surface: the accent wash runs top -> bottom here (the
        // horizontal bars wash along their leading edge), fading toward
        // the status bar; same gate and tint as `tab_bar_background`.
        let bar_base = OryxisColors::t().bg_sidebar;
        let bar_bg = if self.setting_tab_accent_wash {
            let washed = crate::theme::mix(bar_base, self.top_accent_tint(), 0.16);
            Background::Gradient(iced::Gradient::Linear(
                iced::gradient::Linear::new(iced::Radians(std::f32::consts::PI))
                    .add_stop(0.0, washed)
                    .add_stop(0.9, bar_base),
            ))
        } else {
            Background::Color(bar_base)
        };
        let mut bar: Element<'_, Message> = container(inner)
            .padding(Padding { top: 6.0, right: 8.0, bottom: 6.0, left: 8.0 })
            .width(Length::Fixed(SIDE_STRIP_WIDTH))
            .height(Length::Fill)
            .style(move |_| container::Style {
                background: Some(bar_bg),
                ..Default::default()
            })
            .into();
        // With no top bar this strip IS the titlebar: its empty areas
        // (header gap, space below the tabs) drag the window and
        // double-click maximizes, exactly like the horizontal strips.
        // Tab buttons consume their own presses, so clicks on chips
        // never start a window move. The MouseArea wrapper is ALWAYS
        // present (handlers only bound in hidden-bar mode) so the
        // hide toggle never changes this subtree's widget type.
        let mut area = MouseArea::new(bar);
        if hide_top_bar {
            area = area
                .on_press(Message::Tabs(TabsMessage::WindowDrag))
                .on_double_click(Message::Tabs(TabsMessage::WindowMaximizeToggle));
        }
        bar = area.into();

        // Floating drag ghost, tracking the cursor's y (the horizontal
        // bars track x). Non-interactive so the tab MouseAreas below
        // keep receiving the hover events that drive the live-slide.
        // With the pins docked in the (visible) top bar, that bar draws
        // the pinned ghosts instead.
        // Leaving the strip hands the ghost to the window-level layer,
        // which is the only one that can follow the cursor into the
        // content area (issue #112).
        let ghost_elsewhere = (pins_top && !hide_top_bar && self.dragged_tab_pinned())
            || !self.cursor_in_tab_strip();
        if !ghost_elsewhere
            && let Some((ghost, _ghost_w)) =
                self.strip_drag_ghost_el(SIDE_TAB_WIDTH, compact_pins, &ctx.privacy_terms)
        {
            let gy = (self.mouse_position.y - strip_top - 6.0 - SIDE_ROW_HEIGHT / 2.0)
                .max(0.0);
            let positioned: Element<'_, Message> = iced::widget::Column::new()
                .push(Space::new().height(gy))
                .push(
                    iced::widget::Row::new()
                        .push(Space::new().width(8.0))
                        .push(ghost),
                )
                .into();
            return iced::widget::Stack::new()
                .push(bar)
                .push(positioned)
                .width(Length::Fixed(SIDE_STRIP_WIDTH))
                .height(Length::Fill)
                .into();
        }
        bar
    }

    /// The pinned `strip_order` entries as fixed strip rows: compact
    /// chips packed `chips_per_row` per row, full-style pins one per
    /// row. Used by the hidden-top-bar head when `pinned_tabs_top_bar`
    /// docks the pins on this strip.
    fn side_pins_rows(
        &self,
        ctx: &StripCtx,
        chips_per_row: usize,
        compact_pins: bool,
    ) -> Vec<Element<'_, Message>> {
        let mut rows: Vec<Element<'_, Message>> = Vec::new();
        let mut chip_row: Vec<Element<'_, Message>> = Vec::new();
        for (slot, entry) in self.strip_order().into_iter().enumerate() {
            if !self.strip_entry_pinned(entry) {
                continue;
            }
            let el = self.strip_tab_element(ctx, entry, slot);
            if compact_pins {
                chip_row.push(el);
                if chip_row.len() == chips_per_row {
                    rows.push(
                        row(std::mem::take(&mut chip_row))
                            .spacing(TAB_SPACING)
                            .into(),
                    );
                }
            } else {
                rows.push(el);
            }
        }
        if !chip_row.is_empty() {
            rows.push(row(chip_row).spacing(TAB_SPACING).into());
        }
        rows
    }
}
