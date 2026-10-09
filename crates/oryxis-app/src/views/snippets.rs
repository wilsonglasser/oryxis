//! Snippets (saved commands) list and editor panel.

use iced::Widget as _;
use iced::border::Radius;
use iced::widget::{
    button, column, container, scrollable, text, text_editor, text_input, MouseArea, Space,
};
use iced::widget::button::Status as BtnStatus;
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{TabsMessage, SnippetMessage, Message, Oryxis, CARD_WIDTH};
use crate::i18n::t;
use crate::theme::OryxisColors;
use crate::widgets::{card_grid_columns, dir_align_x, dir_row, distribute_card_grid};

impl Oryxis {
    pub(crate) fn view_snippets(&self) -> Element<'_, Message> {
        let sort_btn = crate::widgets::bounds_reporter(
            crate::widgets::sort_toolbar_button(
                crate::state::SortMenuKind::Snippets,
                self.snippets_sort,
            ),
            self.toolbar_sort_btn_bounds.clone(),
        );
        let primary: Element<'_, Message> = {
            let fg = OryxisColors::t().button_text;
            button(
                container(
                    dir_row(vec![
                        text("+").size(13).font(iced::Font {
                            weight: iced::font::Weight::Bold,
                            ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                        }).color(fg).boxed(),
                        Space::new().width(4).boxed(),
                        text(t("snippet_btn")).size(11).font(iced::Font {
                            weight: iced::font::Weight::Bold,
                            ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                        }).color(fg).boxed(),
                    ]).align_y(iced::Alignment::Center),
                )
                .center_y(Length::Fixed(24.0))
                .center_x(Length::Fixed(72.0)),
            )
            .on_press(Message::Snippet(SnippetMessage::ShowSnippetPanel))
            .style(|_, status| {
                let bg = match status {
                    BtnStatus::Hovered => OryxisColors::t().button_bg_hover,
                    _ => OryxisColors::t().button_bg,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                }
            }).boxed()
        };
        // Responsive collapse: search yields first, then folds to an
        // icon; when the buttons can't fit they all move into a `…` menu.
        // `keynav_toolbar_slot` records each rendered action for the
        // keyboard router (push order == visual order here).
        let (search_collapsed, buttons_overflow) = self.toolbar_tiers();
        self.keynav_toolbar_reset();
        let mut row_items: Vec<Element<'_, Message>> = Vec::new();
        // Breadcrumb while inside a snippet group (dashboard-style):
        // "Snippets" routes back to the root, then the open group.
        if let Some(open_group) = &self.cur_nav().active_snippet_group {
            row_items.push(
                dir_row(vec![
                    iced_fonts::lucide::folder()
                        .size(16)
                        .color(OryxisColors::t().accent)
                        .boxed(),
                    Space::new().width(6).boxed(),
                    button(
                        text(t("snippets"))
                            .size(16)
                            .wrapping(iced::widget::text::Wrapping::None)
                            .color(OryxisColors::t().accent),
                    )
                    .on_press(Message::Snippet(SnippetMessage::CloseSnippetGroup))
                    .padding(Padding::ZERO)
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(Color::TRANSPARENT)),
                        border: Border::default(),
                        ..Default::default()
                    })
                    .boxed(),
                    Space::new().width(6).boxed(),
                    text("/").size(16).color(OryxisColors::t().text_muted).boxed(),
                    Space::new().width(6).boxed(),
                    text(open_group.clone())
                        .size(16)
                        .wrapping(iced::widget::text::Wrapping::None)
                        .color(OryxisColors::t().text_primary)
                        .boxed(),
                ])
                .align_y(iced::Alignment::Center)
                .boxed(),
            );
            row_items.push(Space::new().width(12).boxed());
        }
        let search_slot = self.vault_search_slot(search_collapsed);
        row_items.push(if search_collapsed {
            self.keynav_toolbar_slot(crate::keynav::ToolbarItem::SearchIcon, search_slot)
        } else {
            search_slot
        });
        row_items.push(Space::new().width(10).boxed());
        let show_tag_filter = !self.distinct_snippet_tags().is_empty()
            || !self.cur_nav().snippet_filter_tags.is_empty();
        if buttons_overflow {
            // The sort trigger is off screen: blank the anchor cells so
            // the menu falls back cleanly.
            self.keynav_toolbar_zero_trigger_bounds();
            row_items.push(self.keynav_toolbar_slot(
                crate::keynav::ToolbarItem::Overflow,
                crate::widgets::bounds_reporter(
                    crate::widgets::toolbar_overflow_icon(matches!(
                        self.cur_overlay().map(|o| &o.content),
                        Some(crate::state::OverlayContent::ToolbarOverflow)
                    )),
                    self.toolbar_overflow_btn_bounds.clone(),
                ),
            ));
        } else {
            if show_tag_filter {
                row_items.push(self.keynav_toolbar_slot(
                    crate::keynav::ToolbarItem::TagFilter,
                    // Report the button's bounds so the dropdown anchors
                    // under it instead of at the cursor.
                    crate::widgets::bounds_reporter(
                        crate::widgets::tag_filter_toolbar_button(
                            self.cur_nav().snippet_filter_tags.len(),
                            Message::Snippet(SnippetMessage::ShowSnippetTagFilterMenu),
                        ),
                        self.snippet_tag_filter_btn_bounds.clone(),
                    ),
                ));
                row_items.push(Space::new().width(6).boxed());
            }
            row_items.push(self.keynav_toolbar_slot(crate::keynav::ToolbarItem::Sort, sort_btn));
            row_items.push(Space::new().width(8).boxed());
            row_items.push(self.keynav_toolbar_slot(crate::keynav::ToolbarItem::Primary, primary));
        }
        let toolbar = container(dir_row(row_items).align_y(iced::Alignment::Center))
            .padding(Padding { top: 16.0, right: 24.0, bottom: 16.0, left: 24.0 })
            .width(Length::Fill);

        let status: Element<'_, Message> = if let Some(err) = &self.snippet_form.error {
            container(text(err.clone()).size(12).color(OryxisColors::t().error).boxed())
                .padding(Padding { top: 0.0, right: 24.0, bottom: 8.0, left: 24.0 }).boxed()
        } else {
            Space::new().boxed()
        };

        if self.snippets.is_empty() {
            let empty_state = crate::widgets::empty_state(
                iced_fonts::lucide::code()
                    .size(32)
                    .color(OryxisColors::t().text_muted)
                    .boxed(),
                crate::i18n::t("create_snippet_title").to_string(),
                crate::i18n::t("create_snippet_desc").to_string(),
                Some((
                    crate::i18n::t("new_snippet").to_string(),
                    Message::Snippet(SnippetMessage::ShowSnippetPanel),
                )),
            );

            // No toolbar when empty: search is hidden and the "+ New" lives
            // in the empty-state CTA (avoids an orphaned action button).
            // Side panel is hoisted to `view_main` (active_side_panel).
            // Un-record the toolbar items; none of them render here.
            // Same for content rows.
            self.keynav_toolbar_reset();
            self.keynav_clear_content();
            let main_content = column![status, empty_state]
                .width(Length::Fill)
                .height(Length::Fill);
            return main_content.boxed();
        }

        let snippet_needle = self.cur_nav().snippet_search.to_lowercase();
        // Apply the toolbar sort by reordering an index list, the source
        // collection stays in insertion order (its index is what the
        // EditSnippet / RunSnippet messages carry). The needle also
        // matches tags and the group name; the toolbar tag filter keeps
        // snippets carrying ANY selected tag.
        let mut snippet_order: Vec<usize> = (0..self.snippets.len()).collect();
        self.snippets_sort.sort_items(
            &mut snippet_order,
            |&i| self.snippets[i].label.clone(),
            |&i| self.snippets[i].created_at,
        );
        let visible: Vec<usize> = snippet_order
            .into_iter()
            .filter(|&idx| {
                let snip = &self.snippets[idx];
                let needle_ok = snippet_needle.is_empty()
                    || snip.label.to_lowercase().contains(&snippet_needle)
                    || snip.command.to_lowercase().contains(&snippet_needle)
                    || snip
                        .tags
                        .iter()
                        .any(|tg| tg.to_lowercase().contains(&snippet_needle))
                    || snip
                        .group
                        .as_ref()
                        .is_some_and(|g| g.to_lowercase().contains(&snippet_needle));
                let tags_ok = self.cur_nav().snippet_filter_tags.is_empty()
                    || snip.tags.iter().any(|tg| {
                        self.cur_nav().snippet_filter_tags
                            .iter()
                            .any(|f| f.eq_ignore_ascii_case(tg))
                    });
                needle_ok && tags_ok
            })
            .collect();

        // Layout mirrors the Hosts dashboard: at root, group FOLDER
        // CARDS (click to drill in) above the ungrouped snippets;
        // inside a group, that group's snippets with a breadcrumb in
        // the toolbar. An active search flattens everything (the
        // matches are the answer, folders would hide them).
        let group_names = self.snippet_group_names();
        let searching = !snippet_needle.is_empty();
        let mut snippet_nav_sections: Vec<Vec<crate::keynav::NavItem>> = Vec::new();
        let mut section_blocks: Vec<(Option<&'static str>, Vec<Element<'_, Message>>)> =
            Vec::new();

        if let Some(open_group) = self.cur_nav().active_snippet_group.as_ref().filter(|_| !searching) {
            let items: Vec<usize> = visible
                .iter()
                .copied()
                .filter(|&i| {
                    self.snippets[i]
                        .group
                        .as_ref()
                        .is_some_and(|g| g.eq_ignore_ascii_case(open_group))
                })
                .collect();
            snippet_nav_sections.push(
                items.iter().map(|&i| crate::keynav::NavItem::Snippet(i)).collect(),
            );
            section_blocks.push((None, items.iter().map(|&i| self.snippet_card(i)).collect()));
        } else if searching {
            // Flat filtered list, grouping suspended.
            snippet_nav_sections.push(
                visible.iter().map(|&i| crate::keynav::NavItem::Snippet(i)).collect(),
            );
            section_blocks
                .push((None, visible.iter().map(|&i| self.snippet_card(i)).collect()));
        } else {
            // Root: folder cards first (absolute indices into
            // `group_names` so the keyboard id survives filtering),
            // hidden while the tag filter leaves them empty.
            let mut folder_nav: Vec<crate::keynav::NavItem> = Vec::new();
            let mut folder_cards: Vec<Element<'_, Message>> = Vec::new();
            for (gi, name) in group_names.iter().enumerate() {
                let count = visible
                    .iter()
                    .filter(|&&i| {
                        self.snippets[i]
                            .group
                            .as_ref()
                            .is_some_and(|g| g.eq_ignore_ascii_case(name))
                    })
                    .count();
                if count == 0 {
                    continue;
                }
                folder_nav.push(crate::keynav::NavItem::SnippetGroup(gi));
                folder_cards.push(self.snippet_group_card(gi, name, count));
            }
            if !folder_cards.is_empty() {
                snippet_nav_sections.push(folder_nav);
                section_blocks.push((Some("groups"), folder_cards));
            }
            let ungrouped: Vec<usize> = visible
                .iter()
                .copied()
                .filter(|&i| self.snippets[i].group.is_none())
                .collect();
            if !ungrouped.is_empty() || section_blocks.is_empty() {
                snippet_nav_sections.push(
                    ungrouped
                        .iter()
                        .map(|&i| crate::keynav::NavItem::Snippet(i))
                        .collect(),
                );
                let header = section_blocks.is_empty().then_some(()).map_or(
                    Some("snippets"),
                    |_| None,
                );
                section_blocks.push((
                    header,
                    ungrouped.iter().map(|&i| self.snippet_card(i)).collect(),
                ));
            }
        }

        let nav_width = self.vault_rail_width();
        let panel_width = if self.panels.snippet_panel { self.panel_width } else { 0.0 };
        let available = (self.cur_window_size().width
            - nav_width
            - self.side_strip_reserve()
            - panel_width
            - 48.0)
            .max(0.0);
        let cols = card_grid_columns(available, CARD_WIDTH, 12.0);
        // Chunk each section's keyboard order to the grid's column
        // count; multi-section recording makes Tab step between the
        // folder cards and the loose snippets, dashboard-style.
        self.keynav_set_content_sections(
            snippet_nav_sections
                .iter()
                .map(|nav| nav.chunks(cols.max(1)).map(|c| c.to_vec()).collect())
                .collect(),
        );
        let mut grid_col = iced::widget::Column::<iced::Element<'_, _>>::new().spacing(12);
        for (header_key, cards) in section_blocks {
            if let Some(key) = header_key {
                // Same section-header treatment as the keychain's
                // "Keys" / "Identities" (size 14, regular weight,
                // muted, 4/8 vertical padding), so every vault view
                // titles its sections identically.
                grid_col = grid_col.push(
                    container(
                        container(
                            text(t(key)).size(14).color(OryxisColors::t().text_muted),
                        )
                        .width(Length::Fill)
                        .align_x(dir_align_x()),
                    )
                    .padding(Padding { top: 4.0, right: 0.0, bottom: 8.0, left: 0.0 }).boxed(),
                );
            }
            grid_col = grid_col.push(distribute_card_grid(cards, cols, 12.0, 12.0));
        }

        let grid = scrollable(
            column![grid_col].padding(Padding { top: 0.0, right: 24.0, bottom: 24.0, left: 24.0 }),
        )
        // Stable id so the keyboard router can keep the selected card
        // scrolled into view.
        .id(iced::widget::Id::new("snippets-grid-scroll"))
        .height(Length::Fill);

        // Inline search bar in Classic mode (Workspace puts it on
        // the contextual sub-nav). Collapses to zero height in
        // Workspace so we don't render the input twice.
        // Search now lives in the toolbar (`vault_search_field`); the
        // legacy below-toolbar search bar collapses to nothing.
        let search_bar: Element<'_, Message> = Space::new().boxed();

        // Side panel hoisted to `view_main` (active_side_panel).
        column![toolbar, search_bar, status, grid]
            .width(Length::Fill)
            .height(Length::Fill)
            .boxed()
    }

    /// Shortcut recorder row shared by both snippet editors: the
    /// current binding (or a dash), a Record button (shows "press a
    /// key" while armed) and a Clear button when set. `panel` records
    /// the buttons into the side-panel keyboard walk.
    pub(crate) fn snippet_hotkey_row(&self, panel: bool) -> Element<'_, Message> {
        let c = OryxisColors::t();
        let current: Element<'_, Message> = match &self.snippet_form.hotkey {
            Some(b) => text(b.badges().join(" + "))
                .size(12)
                .color(c.text_primary)
                .boxed(),
            None => text("\u{2014}").size(12).color(c.text_muted).boxed(),
        };
        let record_label = if self.snippet_form.hotkey_capturing {
            t("hotkey_press_a_key")
        } else {
            t("snippet_hotkey_record")
        };
        let record_btn: Element<'_, Message> = button(
            container(text(record_label).size(11).color(c.text_primary))
                .padding(Padding { top: 4.0, right: 10.0, bottom: 4.0, left: 10.0 }),
        )
        .on_press(Message::Snippet(SnippetMessage::SnippetHotkeyCaptureStart))
        .style(move |_, status| {
            let bg = if self.snippet_form.hotkey_capturing {
                Color { a: 0.15, ..OryxisColors::t().accent }
            } else {
                match status {
                    BtnStatus::Hovered | BtnStatus::Pressed => OryxisColors::t().bg_hover,
                    _ => OryxisColors::t().bg_selected,
                }
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(6.0), ..Default::default() },
                ..Default::default()
            }
        })
        .boxed();
        let record_btn = if panel {
            self.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::Snippet(SnippetMessage::SnippetHotkeyCaptureStart)),
                6.0,
                record_btn,
            )
        } else {
            record_btn
        };
        let mut items: Vec<Element<'_, Message>> = vec![
            current,
            Space::new().width(Length::Fill).boxed(),
            record_btn,
        ];
        if self.snippet_form.hotkey.is_some() {
            let clear: Element<'_, Message> = button(
                container(iced_fonts::lucide::x().size(11).color(c.text_muted))
                    .padding(Padding { top: 5.0, right: 7.0, bottom: 5.0, left: 7.0 }),
            )
            .on_press(Message::Snippet(SnippetMessage::SnippetHotkeyClear))
            .style(|_, status| {
                let bg = match status {
                    BtnStatus::Hovered | BtnStatus::Pressed => OryxisColors::t().bg_hover,
                    _ => Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                }
            })
            .boxed();
            let clear = if panel {
                self.panel_nav_slot(
                    crate::keynav::RowAction::activate(Message::Snippet(SnippetMessage::SnippetHotkeyClear)),
                    6.0,
                    clear,
                )
            } else {
                clear
            };
            items.push(Space::new().width(4).boxed());
            items.push(clear);
        }
        dir_row(items).align_y(iced::Alignment::Center).boxed()
    }

    /// One snippet card for the vault grid (badge + label + preview +
    /// hashtag line + hover kebab + selection ring). Extracted so the
    /// root and in-group layouts share it.
    fn snippet_card(&self, idx: usize) -> Element<'_, Message> {
        let snip = &self.snippets[idx];
            let kb_selected = self.kn().selected_in(crate::keynav::FocusZone::Content)
                == Some(crate::keynav::NavItem::Snippet(idx));
            // Use host_icon so the snippet badge follows the global
            // `default_host_icon` shape (Circular by default in v0.7)
            // and the rest of the cards on this screen look the same.
            let snip_style = crate::widgets::resolve_host_icon_style(
                None,
                &self.prefs.default_host_icon,
            );
            // `line_height(1.0)` collapses the default text padding so
            // the glyph sits at the optical centre of the badge; the
            // default ~1.2 multiplier pushed it visually upward and
            // the badge looked misaligned next to the label column.
            let glyph_el: Element<'_, Message> = iced_fonts::lucide::code()
                .size(14)
                .line_height(1.0)
                .color(Color::WHITE)
                .boxed();
            let icon_box = crate::widgets::host_icon(
                snip_style,
                OryxisColors::t().accent,
                &snip.label,
                Some(glyph_el),
                32.0,
            );

            // Vertical ellipsis (⋮) only when the row is hovered, so it
            // matches the host-card / keychain affordance. A fixed
            // placeholder reserves the slot in the unhovered state so
            // the label column width stays constant.
            const SNIP_DOTS_SLOT_W: f32 = 22.0;
            // Keep the kebab mounted while its context menu is open, even
            // if the pointer drifts off the card, mirroring the host cards.
            let show_dots = self.cur_nav().hover.snippet_card == Some(idx)
                || self.snippet_context_menu == Some(idx)
                || kb_selected;
            let edit_btn: Element<'_, Message> = if show_dots {
                crate::widgets::card_kebab_button(
                    OryxisColors::t().text_muted,
                    true,
                    Message::Snippet(SnippetMessage::ShowSnippetMenu(idx)),
                )
                .boxed()
            } else {
                Space::new()
                    .width(Length::Fixed(SNIP_DOTS_SLOT_W))
                    .height(Length::Fixed(22.0))
                    .boxed()
            };

            let cmd_preview = if snip.command.len() > 30 {
                format!("{}...", &snip.command[..30])
            } else {
                snip.command.clone()
            };

            // Tags read as a compact hashtag line under the preview;
            // display-only (editing stays in the comma field).
            let mut info_col = column![
                text(&snip.label)
                    .size(13)
                    .color(OryxisColors::t().text_primary)
                    .wrapping(iced::widget::text::Wrapping::None),
                Space::new().height(2),
                text(cmd_preview)
                    .size(10)
                    .color(OryxisColors::t().text_muted)
                    .font(iced::Font::MONOSPACE)
                    .wrapping(iced::widget::text::Wrapping::None),
            ];
            // Install-script category marker (issue #147); the per-host
            // "installed here" state lives on the terminal sidebar rows,
            // which know which host is focused.
            if snip.install {
                info_col = info_col.push(Space::new().height(2).boxed()).push(
                    text(t("snippet_install_badge"))
                        .size(10)
                        .color(OryxisColors::t().warning)
                        .wrapping(iced::widget::text::Wrapping::None).boxed(),
                );
            }
            if !snip.tags.is_empty() {
                let hashtags = snip
                    .tags
                    .iter()
                    .map(|tg| format!("#{tg}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                info_col = info_col.push(Space::new().height(2).boxed()).push(
                    text(hashtags)
                        .size(10)
                        .color(Color { a: 0.8, ..OryxisColors::t().accent })
                        .wrapping(iced::widget::text::Wrapping::None).boxed(),
                );
            }
            if let Some(binding) = snip
                .hotkey
                .as_deref()
                .and_then(crate::hotkeys::HotkeyBinding::parse)
            {
                info_col = info_col.push(Space::new().height(2).boxed()).push(
                    text(binding.badges().join(" + "))
                        .size(10)
                        .color(OryxisColors::t().text_muted)
                        .wrapping(iced::widget::text::Wrapping::None).boxed(),
                );
            }

            let card_btn = button(
                container(
                    dir_row(vec![
                        icon_box,
                        Space::new().width(8).boxed(),
                        info_col.width(Length::Fill).boxed(),
                        edit_btn,
                    ]).align_y(iced::Alignment::Center),
                )
                // Match the host card padding (top/bottom 8, left 2,
                // right reserved for the trailing slot) so the row
                // height + indent line up with the rest of the grid.
                .padding(Padding { top: 8.0, right: 2.0, bottom: 8.0, left: 2.0 }),
            )
            .on_press(Message::Snippet(SnippetMessage::RunSnippet(idx)))
            .width(Length::Fill)
            .style(move |_, status| {
                let (bg, bc, bw) = match status {
                    BtnStatus::Hovered => (OryxisColors::t().bg_hover, OryxisColors::t().accent, 1.5),
                    BtnStatus::Pressed => (OryxisColors::t().bg_selected, OryxisColors::t().accent, 2.0),
                    _ => (OryxisColors::t().bg_surface, OryxisColors::t().border, 1.0),
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(10.0), color: bc, width: bw },
                    ..Default::default()
                }
            });

            // Wrap the button in a MouseArea so we can track hover
            // for the kebab show/hide affordance, same trick the host
            // cards use; right-click opens the kebab menu (app-wide
            // card convention).
            let wrapped: Element<'_, Message> = MouseArea::new(card_btn)
                .on_enter(Message::Tabs(TabsMessage::SnippetCardHovered(idx)))
                .on_exit(Message::Tabs(TabsMessage::SnippetCardUnhovered(idx)))
                .on_right_press(Message::Snippet(SnippetMessage::ShowSnippetMenu(idx)))
                .boxed();
            let card_el: Element<'_, Message> =
                container(wrapped).width(Length::Fill).clip(true).boxed();
            let card_el = self.card_wash(card_el, OryxisColors::t().accent);
self.keynav_ring_content(kb_selected, card_el)
    }

    /// Folder card for a snippet group, mirroring the dashboard's
    /// manual-folder cards: badge + name + count + trailing chevron;
    /// click (or Enter) drills into the group.
    fn snippet_group_card(&self, gi: usize, name: &str, count: usize) -> Element<'_, Message> {
        let kb_selected = self.kn().selected_in(crate::keynav::FocusZone::Content)
            == Some(crate::keynav::NavItem::SnippetGroup(gi));
        let style = crate::widgets::resolve_host_icon_style(
            None,
            &self.prefs.default_host_icon,
        );
        let glyph: Element<'_, Message> = iced_fonts::lucide::folder()
            .size(16)
            .line_height(1.0)
            .color(Color::WHITE)
            .boxed();
        let icon_box = crate::widgets::host_icon(
            style,
            OryxisColors::t().accent,
            name,
            Some(glyph),
            32.0,
        );
        let card_btn = button(
            container(
                dir_row(vec![
                    icon_box,
                    Space::new().width(8).boxed(),
                    column![
                        text(name.to_string())
                            .size(13)
                            .color(OryxisColors::t().text_primary)
                            .wrapping(iced::widget::text::Wrapping::None),
                        Space::new().height(2),
                        text(crate::i18n::snippet_count(count))
                            .size(10)
                            .color(OryxisColors::t().text_muted)
                            .wrapping(iced::widget::text::Wrapping::None),
                    ]
                    .width(Length::Fill)
                    .boxed(),
                    iced_fonts::lucide::chevron_right()
                        .size(14)
                        .color(OryxisColors::t().text_muted)
                        .boxed(),
                ])
                .align_y(iced::Alignment::Center),
            )
            .padding(Padding { top: 8.0, right: 8.0, bottom: 8.0, left: 2.0 }),
        )
        .on_press(Message::Snippet(SnippetMessage::OpenSnippetGroup(name.to_string())))
        .width(Length::Fill)
        .style(move |_, status| {
            let (bg, bc, bw) = match status {
                BtnStatus::Hovered => (OryxisColors::t().bg_hover, OryxisColors::t().accent, 1.5),
                BtnStatus::Pressed => (OryxisColors::t().bg_selected, OryxisColors::t().accent, 2.0),
                _ => (OryxisColors::t().bg_surface, OryxisColors::t().border, 1.0),
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(10.0), color: bc, width: bw },
                ..Default::default()
            }
        });
        let card_el: Element<'_, Message> =
            container(card_btn).width(Length::Fill).clip(true).boxed();
        let card_el = self.card_wash(card_el, OryxisColors::t().accent);
        self.keynav_ring_content(kb_selected, card_el)
    }

    pub(crate) fn view_snippet_panel(&self) -> Element<'_, Message> {
        // Keyboard rows are recorded in visual order (row mode: Up/Down from any input).
        self.panel_nav_reset();
        let is_editing = self.snippet_form.editing_id.is_some();
        let title = if is_editing { t("edit_snippet") } else { t("new_snippet") };

        let panel_header = container(
            dir_row(vec![
                text(title).size(18).color(OryxisColors::t().text_primary).boxed(),
                Space::new().width(Length::Fill).boxed(),
                button(text("\u{00D7}").size(14).color(OryxisColors::t().text_muted))
                    .on_press(Message::Snippet(SnippetMessage::HideSnippetPanel))
                    .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_surface)),
                        border: Border { radius: Radius::from(6.0), ..Default::default() },
                        ..Default::default()
                    }).boxed(),
            ]).align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 20.0, bottom: 16.0, left: 20.0 });

        let form = column![
            text(t("name")).size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            self.panel_nav_slot(
                crate::keynav::RowAction::input(iced::widget::Id::new("panel-snippet-name")),
                10.0,
                text_input("restart-nginx", &self.snippet_form.label)
                    .id(iced::widget::Id::new("panel-snippet-name"))
                    .on_input(|v| Message::Snippet(SnippetMessage::SnippetLabelChanged(v)))
                    .padding(10)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                    .boxed(),
            ),
            Space::new().height(14),
            text(t("group")).size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            // Same type-ahead combo as the host editor's Parent Group:
            // filters the existing snippet groups as you type and still
            // accepts a brand new name. Keyboard row: Left/Right cycle
            // the known groups (the fork combo_box has no id hook, so
            // Enter cannot focus it; free-text entry stays typing).
            {
                let (group_prev, group_next) = crate::keynav::slots::cycle_pair(
                    self.snippet_form.group_combo.options(),
                    &self.snippet_form.group,
                    |v| Message::Snippet(SnippetMessage::SnippetGroupChanged(v)),
                );
                let selection = (!self.snippet_form.group.is_empty())
                    .then_some(&self.snippet_form.group);
                self.panel_nav_slot(
                    crate::keynav::RowAction::picker(group_prev, group_next),
                    10.0,
                    iced::widget::combo_box(
                        &self.snippet_form.group_combo,
                        t("group_optional_placeholder"),
                        selection,
                        |v| Message::Snippet(SnippetMessage::SnippetGroupChanged(v)),
                    )
                    .on_input(|v| Message::Snippet(SnippetMessage::SnippetGroupChanged(v)))
                    // The fork's combo clears its own input on focus and
                    // restores the committed value on blur ONLY inside
                    // its `on_close` branch; without one a focused-then-
                    // abandoned field stays blank while the form keeps
                    // the group. The message itself does nothing.
                    .on_close(Message::NoOp)
                    .padding(10)
                    .input_style(crate::widgets::rounded_input_style)
                    .menu_style(crate::widgets::combo_menu_style)
                    .width(Length::Fill)
                    .boxed(),
                )
            },
            Space::new().height(14),
            text(t("tags")).size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            self.panel_nav_slot(
                crate::keynav::RowAction::input(iced::widget::Id::new("panel-snippet-tags")),
                10.0,
                text_input(t("tags_placeholder"), &self.snippet_form.tags_input)
                    .id(iced::widget::Id::new("panel-snippet-tags"))
                    .on_input(|v| Message::Snippet(SnippetMessage::SnippetTagsChanged(v)))
                    .padding(10)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                    .boxed(),
            ),
            Space::new().height(14),
            text(t("snippet_hotkey")).size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            self.snippet_hotkey_row(true),
            Space::new().height(14),
            // Install-script category (issue #147): flips this snippet
            // into the one-time host-setup affordances (a confirmation
            // showing the full body before anything is sent, and the
            // per-host "installed here" memory).
            self.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::Snippet(
                    SnippetMessage::ToggleSnippetInstall,
                )),
                10.0,
                crate::widgets::toggle_row_desc(
                    t("snippet_install_toggle"),
                    t("snippet_install_desc"),
                    self.snippet_form.install,
                    Message::Snippet(SnippetMessage::ToggleSnippetInstall),
                ),
            ),
            Space::new().height(14),
            text(t("command_label")).size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            // Multi-line, auto-grows with content; container caps the height
            // (~10 lines) and then it scrolls internally.
            self.panel_nav_slot(
                crate::keynav::RowAction::input(iced::widget::Id::new("panel-snippet-command")),
                10.0,
                container(
                    text_editor(&self.snippet_form.command)
                        .id(iced::widget::Id::new("panel-snippet-command"))
                        .placeholder("sudo systemctl restart nginx")
                        .on_action(|v| Message::Snippet(SnippetMessage::SnippetCommandAction(v)))
                        .padding(10)
                        .height(Length::Shrink)
                        .style(crate::widgets::rounded_editor_style),
                )
                .height(Length::Shrink.max(240.0))
                .boxed(),
            ),
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());

        // Shared form chrome. Deleting a snippet lives on the card's ⋮
        // context menu (Edit / Delete), so the destructive action isn't
        // buried inside the editor form.
        let panel_error = crate::widgets::form_error(self.snippet_form.error.as_deref());
        let footer = crate::widgets::form_footer(
            self.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::Snippet(SnippetMessage::HideSnippetPanel)),
                6.0,
                crate::widgets::form_cancel_button(Message::Snippet(SnippetMessage::HideSnippetPanel)),
            ),
            self.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::Snippet(SnippetMessage::SaveSnippet)),
                6.0,
                crate::widgets::form_save_button(
                    crate::i18n::t("save"),
                    Some(Message::Snippet(SnippetMessage::SaveSnippet)),
                ),
            ),
        );

        let panel_content = column![
            panel_header,
            container(
                column![form].width(Length::Fill).align_x(dir_align_x()),
            )
            .padding(Padding { top: 0.0, right: 20.0, bottom: 0.0, left: 20.0 })
            .height(Length::Fill),
            panel_error,
            footer,
        ].height(Length::Fill);

        crate::widgets::side_panel_frame(panel_content.boxed(), OryxisColors::t().bg_sidebar, self.panel_width)
    }
}
