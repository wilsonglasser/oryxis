//! Dashboard toolbar, the breadcrumb on the left, and the trailing
//! `+ host` action button.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, container, text, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{EditorMessage, NavigationMessage, Message, Oryxis};
use crate::i18n::t;
use crate::theme::OryxisColors;
use crate::widgets::dir_row;

impl Oryxis {
    pub(super) fn dashboard_toolbar(&self) -> Element<'_, Message> {
        // ── Toolbar ──
        let toolbar_left: Element<'_, Message> = if let Some(gid) = self.active_group {
            // Compact folder header: a back arrow (one level up, root
            // when the folder is top-level) + folder glyph + the
            // current group's label. Replaced the full breadcrumb
            // chain (owner call 2026-07-23): with nested subgroups the
            // chain ate the toolbar's width; the arrow covers the same
            // navigation one hop at a time, SFTP-style.
            let current = self.groups.iter().find(|g| g.id == gid);
            let label = current.map(|g| g.label.clone()).unwrap_or_default();
            // A dangling parent (deleted on another device) backs out
            // to root, matching how the grid re-homes the subtree.
            let parent = current
                .and_then(|g| g.parent_id)
                .filter(|pid| self.groups.iter().any(|g| g.id == *pid));
            let back_msg = match parent {
                Some(pid) => Message::Navigation(NavigationMessage::OpenGroup(pid)),
                // Top level: ChangeView(Dashboard) clears the active
                // group (the Home-tab path), landing on the root list.
                None => Message::Navigation(NavigationMessage::ChangeView(
                    crate::state::View::Dashboard,
                )),
            };
            // Physical direction flips under RTL ("back" points at the
            // trailing edge there).
            let back_glyph = if crate::i18n::is_rtl_layout() {
                iced_fonts::lucide::arrow_right()
            } else {
                iced_fonts::lucide::arrow_left()
            };
            let back_btn = button(
                container(back_glyph.size(16).color(OryxisColors::t().text_primary))
                    .center_x(Length::Fixed(28.0))
                    .center_y(Length::Fixed(28.0)),
            )
            .on_press(back_msg)
            .padding(0)
            .style(|_, status| {
                let bg = match status {
                    BtnStatus::Hovered => OryxisColors::t().bg_hover,
                    BtnStatus::Pressed => OryxisColors::t().bg_selected,
                    _ => Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                }
            });
            dir_row(vec![
                crate::views::terminal::icon_tooltip(back_btn.into(), t("back")),
                Space::new().width(8).into(),
                iced_fonts::lucide::folder().size(18).color(OryxisColors::t().accent).into(),
                Space::new().width(6).into(),
                text(label)
                    .size(20)
                    .wrapping(iced::widget::text::Wrapping::None)
                    .color(OryxisColors::t().text_primary)
                    .into(),
            ])
            .align_y(iced::Alignment::Center)
            .into()
        } else {
            // Title dropped (redundant with the section nav); the search
            // field fills this slot in the toolbar instead.
            Space::new().into()
        };

        // "+ Host" button: opens the manual SSH editor. The other
        // "add a host" paths (import / export / new group) live in the
        // empty state's action buttons, so the toolbar keeps the one
        // primary verb.
        let label_radius = Radius::from(6.0);

        let primary_btn = button(
            container(
                dir_row(vec![
                    text("+").size(13).font(iced::Font {
                        weight: iced::font::Weight::Bold,
                        ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                    }).color(OryxisColors::t().button_text).into(),
                    Space::new().width(4).into(),
                    text(t("host_btn")).size(11).font(iced::Font {
                        weight: iced::font::Weight::Bold,
                        ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                    }).color(OryxisColors::t().button_text).into(),
                ]).align_y(iced::Alignment::Center),
            )
            .center_y(Length::Fixed(24.0))
            .center_x(Length::Fixed(72.0)),
        )
        .on_press(Message::Editor(EditorMessage::ShowNewConnection))
        .style(move |_, status| {
            let bg = match status {
                BtnStatus::Hovered => OryxisColors::t().button_bg_hover,
                _ => OryxisColors::t().button_bg,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: label_radius, ..Default::default() },
                ..Default::default()
            }
        });

        let resolved_action = self
            .keynav_toolbar_ring(crate::keynav::ToolbarItem::Primary, primary_btn.into());
        let resolved_items = vec![crate::keynav::ToolbarItem::Primary];

        // Sort dropdown trigger, sits just before the "+ Host"
        // action. Glyph reflects the active sort so the
        // current mode is readable without opening the menu.
        let sort_btn = self.keynav_toolbar_ring(
            crate::keynav::ToolbarItem::Sort,
            crate::widgets::bounds_reporter(
                crate::widgets::sort_toolbar_button(
                    crate::state::SortMenuKind::Hosts,
                    self.hosts_sort,
                ),
                self.toolbar_sort_btn_bounds.clone(),
            ),
        );

        // Tag filter, only rendered once at least one host is tagged
        // (or a filter is active and needs clearing). Accent-filled
        // while active so a narrowed list is visibly narrowed.
        let show_tag_filter = self.host_tag_filter_available();
        let tag_filter_btn: Element<'_, Message> = if show_tag_filter {
            dir_row(vec![
                self.keynav_toolbar_ring(
                    crate::keynav::ToolbarItem::TagFilter,
                    // Report the button's bounds so the dropdown anchors
                    // under it (like the "+ Host" split menu) instead of
                    // at the cursor.
                    crate::widgets::bounds_reporter(
                        crate::widgets::tag_filter_toolbar_button(
                            self.host_filter_tags.len(),
                            Message::Navigation(NavigationMessage::ShowHostTagFilterMenu),
                        ),
                        self.host_tag_filter_btn_bounds.clone(),
                    ),
                ),
                Space::new().width(6).into(),
            ])
            .align_y(iced::Alignment::Center)
            .into()
        } else {
            Space::new().into()
        };

        // Grid/List toggle, hidden once the window is so narrow that the
        // grid already renders as a single column (list == grid there).
        let nav_width = self.vault_rail_width();
        let panel_open = self.panels.host_panel;
        let panel_width = if panel_open { self.panel_width } else { 0.0 };
        let available = (self.window_size.width
            - nav_width
            - self.side_strip_reserve()
            - panel_width
            - 48.0)
            .max(0.0);
        let responsive_cols =
            crate::widgets::card_grid_columns(available, crate::app::CARD_WIDTH, 12.0);
        // Narrow windows used to hide the toggle (grid == list at one
        // column), but the TREE mode is meaningful at any width - and
        // hiding the button would strand a user who cycled into tree
        // with no way back (issue #102).
        let show_view_toggle = responsive_cols > 1
            || self.prefs.host_view_mode != crate::state::HostViewMode::Grid;
        let view_toggle: Element<'_, Message> = if show_view_toggle {
            dir_row(vec![
                self.keynav_toolbar_ring(
                    crate::keynav::ToolbarItem::ViewToggle,
                    crate::widgets::host_view_toggle_button(self.prefs.host_view_mode),
                ),
                Space::new().width(6).into(),
            ])
            .align_y(iced::Alignment::Center)
            .into()
        } else {
            Space::new().into()
        };

        // ── Responsive collapse ──
        // #1: search yields before the folder name. #2: but the search
        // keeps a usable min-width, so once it hits that the breadcrumb
        // clips instead; only when the min won't fit at all does the search
        // fold to a floating-field icon. #3: when the whole button cluster
        // can't fit alongside the icon, every action folds into a single
        // `…` overflow menu (so the toolbar shows just the search + `…`).
        const SEARCH_MIN: f32 = 180.0;
        const ICON: f32 = 44.0;
        const GAP_SC: f32 = 10.0; // search ↔ cluster
        const GAP_BS: f32 = 12.0; // breadcrumb ↔ search
        const BC_FLOOR: f32 = 50.0;
        let in_group = self.active_group.is_some();
        let leading_w = self.toolbar_leading_width();
        let cluster_w = self.toolbar_cluster_width();
        let toolbar_w = self.toolbar_content_width();
        let (search_collapsed, buttons_overflow) = self.toolbar_tiers();
        let overflow_open = matches!(
            self.overlay.as_ref().map(|o| &o.content),
            Some(crate::state::OverlayContent::ToolbarOverflow)
        );

        // Breadcrumb width. The inline trailing is the full cluster, or
        // just the 44px `…` once the buttons have folded. While the search
        // is a field, cap the breadcrumb so the Fill search keeps at least
        // SEARCH_MIN (the name clips before the search shrinks past
        // usable). Once the search is an icon, the breadcrumb takes
        // whatever the icon + `…` leave.
        let trailing_w = if buttons_overflow { ICON } else { cluster_w };
        let left_el: Element<'_, Message> = if in_group {
            let (cap, clip_to_cap) = if search_collapsed {
                let c = (toolbar_w - ICON - GAP_SC - GAP_BS - trailing_w).max(0.0);
                (c, true)
            } else {
                let zone = toolbar_w - trailing_w - GAP_SC - GAP_BS;
                let c = (zone - SEARCH_MIN).max(BC_FLOOR);
                (c, leading_w > c)
            };
            if clip_to_cap {
                container(toolbar_left)
                    .width(Length::Fixed(cap))
                    .clip(true)
                    .into()
            } else {
                container(toolbar_left).clip(true).into()
            }
        } else {
            toolbar_left
        };

        // Record the rendered actions for the keyboard router, in
        // visual (leading-to-trailing) order; the focus rings were
        // applied at each build site above.
        self.keynav_toolbar_reset();
        if search_collapsed {
            self.keynav_toolbar_record(crate::keynav::ToolbarItem::SearchIcon);
        }
        if buttons_overflow {
            self.keynav_toolbar_record(crate::keynav::ToolbarItem::Overflow);
        } else {
            if show_view_toggle {
                self.keynav_toolbar_record(crate::keynav::ToolbarItem::ViewToggle);
            }
            if show_tag_filter {
                self.keynav_toolbar_record(crate::keynav::ToolbarItem::TagFilter);
            }
            self.keynav_toolbar_record(crate::keynav::ToolbarItem::Sort);
            for it in &resolved_items {
                self.keynav_toolbar_record(*it);
            }
        }

        let mut row_items: Vec<Element<'_, Message>> = vec![left_el];
        if in_group {
            row_items.push(Space::new().width(12).into());
        }
        let search_slot = self.vault_search_slot(search_collapsed);
        row_items.push(if search_collapsed {
            self.keynav_toolbar_ring(crate::keynav::ToolbarItem::SearchIcon, search_slot)
        } else {
            search_slot
        });
        row_items.push(Space::new().width(10).into());
        if buttons_overflow {
            // Every action folds into the one `…` menu; the split/sort
            // triggers are off screen, so blank their anchor cells.
            self.keynav_toolbar_zero_trigger_bounds();
            row_items.push(self.keynav_toolbar_ring(
                crate::keynav::ToolbarItem::Overflow,
                crate::widgets::bounds_reporter(
                    crate::widgets::toolbar_overflow_icon(overflow_open),
                    self.toolbar_overflow_btn_bounds.clone(),
                ),
            ));
        } else {
            row_items.push(view_toggle);
            row_items.push(tag_filter_btn);
            row_items.push(sort_btn);
            row_items.push(Space::new().width(8).into());
            row_items.push(resolved_action);
        }

        // Let the row size to its natural height (button chrome included)
        // so the action button keeps its true visual size.
        let toolbar = container(dir_row(row_items).align_y(iced::Alignment::Center))
            // Top padding matches the 24px side padding so the page's inner
            // spacing is uniform on the X and Y axes.
            .padding(Padding { top: 16.0, right: 24.0, bottom: 16.0, left: 24.0 })
            .width(Length::Fill);
        toolbar.into()
    }
}
