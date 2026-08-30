//! Dashboard grid: group cards. Split out of views/dashboard/grid/mod.rs.

use super::*;
use iced::widget::column;
impl Oryxis {
    /// Folder + provider group cards for the dashboard grid.
    pub(crate) fn dashboard_group_cards(&self) -> Vec<(Element<'_, Message>, Color, DashNavItem)> {
        let search_lower = self.host_search.to_lowercase();
        // Counts and the filter chips: the shared pre-pass (one scan
        // over connections + groups per view call; the tree view runs
        // the same one), so the per-card lookups below all hit maps
        // in O(1).
        let pre = self.dash_grid_pre_pass();
        let direct_host_count = &pre.direct_host_count;
        let nested_group_count = &pre.nested_group_count;
        let tag_filter_groups = &pre.tag_filter_groups;
        let mut group_cards: Vec<(Element<'_, Message>, Color, DashNavItem)> = Vec::new();
        let group_by_id: std::collections::HashMap<Uuid, _> =
            self.groups.iter().map(|g| (g.id, g)).collect();
        if self.active_group.is_none() {
            // Root view: show folder cards for manual groups that have
            // either direct connections or nested children. Groups
            // nested under a live parent are excluded below: they
            // render inside their parent folder instead.
            let mut shown_groups = std::collections::HashSet::new();
            let mut roots_to_render: Vec<uuid::Uuid> = Vec::new();
            for conn in &self.connections {
                if let Some(gid) = conn.group_id
                    && shown_groups.insert(gid)
                {
                    roots_to_render.push(gid);
                }
            }
            for g in &self.groups {
                if shown_groups.contains(&g.id) { continue }
                // Render a manual folder at root when it has nested
                // children OR when it's an empty container that belongs
                // at root: a top-level folder (no parent), or a subgroup
                // whose ancestry is broken (dangling / cyclic parent,
                // e.g. the parent was deleted on another device). Without
                // this, an empty manual folder (created via "New
                // subgroup" with the parent cleared, or orphaned by a
                // remote delete) would render nowhere yet still be a
                // pickable combo destination, a phantom folder. Empty
                // WELL-NESTED subgroups aren't added here: they render
                // inside their parent (which shows via its own children),
                // so there's no double-render.
                let has_children =
                    self.groups.iter().any(|c| c.parent_id == Some(g.id));
                let belongs_at_root = g.parent_id.is_none()
                    || !oryxis_core::models::Group::is_reachable_from_root(
                        &self.groups,
                        g.id,
                    );
                if has_children || belongs_at_root {
                    shown_groups.insert(g.id);
                    roots_to_render.push(g.id);
                }
            }

            // Apply the toolbar sort to folder cards. Hidden groups (no
            // direct match) just fall through the search filter below.
            self.hosts_sort.sort_items(
                &mut roots_to_render,
                |gid| {
                    group_by_id
                        .get(gid)
                        .map(|g| g.label.clone())
                        .unwrap_or_default()
                },
                |gid| {
                    group_by_id
                        .get(gid)
                        .map(|g| g.created_at)
                        .unwrap_or_else(chrono::Utc::now)
                },
            );
            for gid in roots_to_render {
                // Tag filter: hide folders whose subtree holds no host
                // with a selected tag (owner QA: the filter must narrow
                // the Groups section too, not only the loose hosts).
                if let Some(visible) = tag_filter_groups.as_ref()
                    && !visible.contains(&gid)
                {
                    continue;
                }
                let Some(&group) = group_by_id.get(&gid) else {
                    continue;
                };
                // A subgroup renders inside its parent folder, not at
                // root, but ONLY when its ancestry is well-formed (the
                // parent chain leads to a real root). It degrades to
                // rendering at root whenever the chain is broken:
                //   - dangling parent (a folder deleted on another
                //     device before this one synced), or
                //   - a parent CYCLE (two devices concurrently
                //     re-parenting G1<->G2, LWW-merged into a loop): the
                //     old `contains_key` check only saw that the parent
                //     EXISTS, so every group in the loop had a live
                //     parent and none rendered at root, hiding the whole
                //     cycle (and its hosts) with no path to edit or
                //     delete it.
                // Rendering the broken group at root keeps it (and its
                // hosts) reachable, editable and deletable, matching the
                // dangling-parent degradation. `is_reachable_from_root`
                // walks the chain with a visited-set guard so cyclic
                // data can't loop here.
                if group.parent_id.is_some()
                    && oryxis_core::models::Group::is_reachable_from_root(
                        &self.groups,
                        gid,
                    )
                {
                    continue;
                }
                if !search_lower.is_empty()
                    && !group.label.to_lowercase().contains(&search_lower)
                {
                    continue;
                }
                // Count = direct connections + nested groups (each
                // nested group is a record, even if its contents are
                // resolved on expand).
                let direct_hosts =
                    direct_host_count.get(&gid).copied().unwrap_or(0);
                let nested_groups =
                    nested_group_count.get(&gid).copied().unwrap_or(0);
                let count_text =
                    crate::i18n::host_count(direct_hosts + nested_groups);
                let (element, folder_bg) =
                    self.manual_folder_card(group, count_text);
                group_cards.push((element, folder_bg, DashNavItem::Group(gid)));
            }

        } else if let Some(active_gid) = self.active_group {
            // Inside a folder: manual subgroups render first as folder
            // cards (same builder as the root pass), then the nested
            // dynamic groups, mirroring the root view's manual-then-
            // dynamic order. All manual children render, including
            // empty ones, so a freshly created subgroup is visible
            // immediately.
            let mut nested_manual_order: Vec<usize> = (0..self.groups.len())
                .filter(|&i| {
                    let g = &self.groups[i];
                    g.parent_id == Some(active_gid)
                        && tag_filter_groups
                            .as_ref()
                            .is_none_or(|v| v.contains(&g.id))
                        && (search_lower.is_empty()
                            || g.label.to_lowercase().contains(&search_lower))
                })
                .collect();
            self.hosts_sort.sort_items(
                &mut nested_manual_order,
                |&i| self.groups[i].label.clone(),
                |&i| self.groups[i].created_at,
            );
            for nested_i in nested_manual_order {
                let group = &self.groups[nested_i];
                let gid = group.id;
                let direct_hosts =
                    direct_host_count.get(&gid).copied().unwrap_or(0);
                let nested_groups =
                    nested_group_count.get(&gid).copied().unwrap_or(0);
                let count_text =
                    crate::i18n::host_count(direct_hosts + nested_groups);
                let (element, folder_bg) =
                    self.manual_folder_card(group, count_text);
                group_cards.push((element, folder_bg, DashNavItem::Group(gid)));
            }

        }
        group_cards
    }

    /// One manual-folder card (icon chip + label + record count +
    /// hover kebab / idle chevron), shared by the root pass and the
    /// nested-subgroup pass inside an open folder. Returns the wrapped
    /// element plus the folder's accent colour for the glass wash.
    /// Tree mode does NOT use this: its rows are the dense
    /// `tree_folder_row` chassis in `tree.rs`.
    pub(crate) fn manual_folder_card<'a>(
        &'a self,
        group: &'a oryxis_core::models::Group,
        count_text: String,
    ) -> (Element<'a, Message>, Color) {
        let gid = group.id;
        // Folder card icon precedence:
        //   1. Explicit BRAND icon on the group
        //      (`aws`, `kubernetes`, `ubuntu`, etc.).
        //   2. Explicit non-brand icon (Lucide UI placeholder like
        //      `server`).
        //   3. Generic Lucide `boxes` cube.
        // Visual: brand-colour chip with a white glyph on top.
        let explicit_brand = group
            .icon
            .as_deref()
            .filter(|s| !s.is_empty())
            .and_then(crate::os_icon::canonical_brand_id);

        let (folder_glyph, folder_bg): (BrandIcon, Color) =
            if let Some(brand) = explicit_brand {
                let glyph = crate::os_icon::custom_icon_glyph(brand);
                let bg = group
                    .color
                    .as_deref()
                    .and_then(crate::os_icon::parse_hex_color)
                    .unwrap_or_else(|| {
                        crate::os_icon::provider_icon(
                            brand,
                            OryxisColors::t().accent,
                        )
                        .1
                    });
                (glyph, bg)
            } else if let Some(custom) = group
                .icon
                .as_deref()
                .filter(|s| !s.is_empty())
            {
                // Non-brand explicit icon (e.g. user picked Lucide
                // `key` / `lock` for a group). Honour it with the
                // user's colour or the accent fallback.
                let glyph = crate::os_icon::custom_icon_glyph(custom);
                let bg = group
                    .color
                    .as_deref()
                    .and_then(crate::os_icon::parse_hex_color)
                    .unwrap_or_else(|| OryxisColors::t().accent);
                (glyph, bg)
            } else {
                (
                    BrandIcon::Glyph(iced_fonts::lucide::boxes()),
                    OryxisColors::t().accent,
                )
            };
        // Render through host_icon so the group folder respects the
        // global default shape (Circular / Square / Outline /
        // Initials) the user picked in Settings -> Interface.
        let host_style = crate::widgets::resolve_host_icon_style(
            None,
            &self.prefs.default_host_icon,
        );
        let icon_box = crate::widgets::host_icon(
            host_style,
            folder_bg,
            &group.label,
            Some(folder_glyph.view(18.0, Color::WHITE)),
            32.0,
        );

        // Trailing affordance lives in a Stack overlay on the trailing
        // corner, exactly like the host card's kebab, so a group's ⋮
        // lines up pixel-for-pixel with a host's. The card reserves
        // the same fixed trailing pad; the overlay shows the ⋮ on
        // hover and a muted chevron otherwise (the chevron is the
        // group affordance that distinguishes folder cards from host
        // cards at a glance, issue #38 polish).
        let folder_rtl = crate::i18n::is_rtl_layout();
        let folder_show_dots = self.hover.folder_card == Some(gid);
        let folder_pad_trailing = 24.0_f32;
        let folder_padding = if folder_rtl {
            Padding { top: 8.0, right: 2.0, bottom: 8.0, left: folder_pad_trailing }
        } else {
            Padding { top: 8.0, right: folder_pad_trailing, bottom: 8.0, left: 2.0 }
        };

        let folder_card = button(
            container(
                dir_row(vec![
                    icon_box,
                    Space::new().width(8).into(),
                    column![
                        text(group.label.clone())
                            .size(13)
                            .color(OryxisColors::t().text_primary)
                            .wrapping(iced::widget::text::Wrapping::None),
                        Space::new().height(2),
                        text(count_text)
                            .size(10)
                            .color(OryxisColors::t().text_muted)
                            .wrapping(iced::widget::text::Wrapping::None),
                    ]
                    .width(Length::Fill)
                    .align_x(crate::widgets::dir_align_x())
                    .clip(true)
                    .into(),
                ]).align_y(iced::Alignment::Center),
            )
            .padding(folder_padding),
        )
        .on_press(Message::Navigation(NavigationMessage::OpenGroup(gid)))
        .width(Length::Fill)
        .style(|_, status| {
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

        // ⋮ on hover, chevron otherwise. Both sit in the same
        // right-aligned overlay slot as the host kebab.
        let folder_trailing: Element<'_, Message> = if folder_show_dots {
            crate::widgets::card_kebab_button(
                OryxisColors::t().text_muted,
                true,
                Message::Tabs(TabsMessage::ShowFolderActions(gid)),
            )
            .into()
        } else {
            let chevron = if folder_rtl {
                iced_fonts::lucide::chevron_left()
            } else {
                iced_fonts::lucide::chevron_right()
            };
            // Center the idle chevron in the same 22×22 box the hover
            // ⋮ uses, so idle and hover share a center (no x/y jitter
            // on hover).
            container(
                chevron
                    .size(14)
                    .color(OryxisColors::t().text_muted),
            )
            .center_x(Length::Fixed(22.0))
            .center_y(Length::Fixed(22.0))
            .into()
        };
        let folder_element =
            crate::widgets::card_trailing_overlay(folder_card.into(), folder_trailing);

        // Wrap in MouseArea so hover events drive the dots-button
        // visibility, and right-click opens the kebab menu (app-wide
        // card convention).
        let wrapped = MouseArea::new(folder_element)
            .on_enter(Message::Tabs(TabsMessage::FolderCardHovered(gid)))
            .on_exit(Message::Tabs(TabsMessage::FolderCardUnhovered(gid)))
            .on_right_press(Message::Tabs(TabsMessage::ShowFolderActions(gid)));
        (
            Element::from(container(wrapped).width(Length::Fill).clip(true)),
            folder_bg,
        )
    }
}
