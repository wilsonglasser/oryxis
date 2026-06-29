//! Dashboard grid: group cards. Split out of views/dashboard/grid/mod.rs.

use super::*;
use iced::widget::column;
impl Oryxis {
    /// Folder + provider group cards for the dashboard grid.
    pub(crate) fn dashboard_group_cards(&self) -> Vec<(Element<'_, Message>, Color, DashNavItem)> {
        let search_lower = self.host_search.to_lowercase();
        let hidden_profiles = self.hidden_cloud_profile_ids();
        let hidden_groups: std::collections::HashSet<Uuid> = if hidden_profiles.is_empty() {
            std::collections::HashSet::new()
        } else {
            let mut has_visible_conn: std::collections::HashSet<Uuid> =
                std::collections::HashSet::new();
            for c in &self.connections {
                if let Some(gid) = c.group_id
                    && !c
                        .cloud_ref
                        .as_ref()
                        .is_some_and(|r| hidden_profiles.contains(&r.profile_id))
                {
                    has_visible_conn.insert(gid);
                }
            }
            let mut memo: std::collections::HashMap<Uuid, bool> =
                std::collections::HashMap::new();
            let mut set: std::collections::HashSet<Uuid> = std::collections::HashSet::new();
            for g in &self.groups {
                let hide = if let Some(q) = g.cloud_query.as_ref() {
                    hidden_profiles.contains(&q.profile_id)
                } else {
                    !group_has_visible_content(
                        g.id,
                        &self.groups,
                        &has_visible_conn,
                        &hidden_profiles,
                        &mut memo,
                    )
                };
                if hide {
                    set.insert(g.id);
                }
            }
            set
        };
        let mut group_cards: Vec<(Element<'_, Message>, Color, DashNavItem)> = Vec::new();
        if self.active_group.is_none() {
            // Root view: show folder cards for manual groups that have
            // either direct connections or nested children (e.g. an
            // AWS profile folder whose only child is an ECS dynamic
            // sub-group, with no EC2 connection imported alongside).
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
                if g.cloud_query.is_some() || g.parent_id.is_some() { continue }
                if shown_groups.contains(&g.id) { continue }
                if self.groups.iter().any(|c| c.parent_id == Some(g.id)) {
                    shown_groups.insert(g.id);
                    roots_to_render.push(g.id);
                }
            }
            // Pre-pass over connections + groups, one scan each per
            // view call. The per-card lookups below (group resolve,
            // host / nested counts, brand inference) all hit these maps
            // in O(1) instead of rescanning the full lists for every
            // folder card on every frame.
            let group_by_id: std::collections::HashMap<Uuid, _> =
                self.groups.iter().map(|g| (g.id, g)).collect();
            let mut direct_host_count: std::collections::HashMap<Uuid, usize> =
                std::collections::HashMap::new();
            // First cloud_ref profile seen per group (connections
            // order), feeding the brand-inference fallback below.
            let mut first_cloud_profile: std::collections::HashMap<Uuid, Uuid> =
                std::collections::HashMap::new();
            for conn in &self.connections {
                if let Some(cgid) = conn.group_id {
                    // Hidden cloud hosts don't count toward the folder's
                    // host total or its brand inference.
                    if conn
                        .cloud_ref
                        .as_ref()
                        .is_some_and(|r| hidden_profiles.contains(&r.profile_id))
                    {
                        continue;
                    }
                    *direct_host_count.entry(cgid).or_insert(0) += 1;
                    if let Some(cref) = conn.cloud_ref.as_ref() {
                        first_cloud_profile.entry(cgid).or_insert(cref.profile_id);
                    }
                }
            }
            let mut nested_group_count: std::collections::HashMap<Uuid, usize> =
                std::collections::HashMap::new();
            // First nested cloud-query brand per parent (groups order),
            // the primary brand-inference source.
            let mut child_query_brand: std::collections::HashMap<Uuid, &'static str> =
                std::collections::HashMap::new();
            for g in &self.groups {
                if let Some(pgid) = g.parent_id {
                    // Hidden cloud sub-groups don't count toward the
                    // parent folder's nested-group total.
                    if hidden_groups.contains(&g.id) {
                        continue;
                    }
                    *nested_group_count.entry(pgid).or_insert(0) += 1;
                    if let Some(q) = g.cloud_query.as_ref() {
                        child_query_brand.entry(pgid).or_insert(match q.kind {
                            oryxis_core::models::cloud::CloudQueryKind::EcsTasks { .. } => "ecs",
                            oryxis_core::models::cloud::CloudQueryKind::K8sPods { .. } => "kubernetes",
                        });
                    }
                }
            }
            // Subtree-match set for the cloud-profile filter chip,
            // built once per view call (None when the filter is off).
            let cloud_filter_groups: Option<std::collections::HashSet<Uuid>> =
                self.host_filter_cloud_profile
                    .map(|pid| self.groups_containing_cloud_profile(pid));

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
                // Provider folder that went empty after its plugin was
                // removed (every host / dynamic group inside it is from
                // an uninstalled provider). Hidden until the plugin is
                // reinstalled.
                if hidden_groups.contains(&gid) {
                    continue;
                }
                // Cloud-profile filter, hide folders whose subtree has
                // no host or dynamic group matching the active profile.
                // Active filter intentionally hides every manual,
                // non-cloud folder at root, the chip is the user's
                // explicit "show me only this provider" lens.
                if let Some(visible) = cloud_filter_groups.as_ref()
                    && !visible.contains(&gid)
                {
                    continue;
                }
                if let Some(&group) = group_by_id.get(&gid)
                        && (search_lower.is_empty()
                            || group.label.to_lowercase().contains(&search_lower)) {
                            // Count = direct connections + nested groups
                            // (each nested dynamic group is a record,
                            // even if its tasks are resolved on expand).
                            let direct_hosts =
                                direct_host_count.get(&gid).copied().unwrap_or(0);
                            let nested_groups =
                                nested_group_count.get(&gid).copied().unwrap_or(0);
                            let count = direct_hosts + nested_groups;
                            let label = group.label.clone();
                            let count_text = crate::i18n::host_count(count);

                            // Folder card icon precedence:
                            //   1. Explicit BRAND icon on the group
                            //      (`aws`, `kubernetes`, `ubuntu`, etc.).
                            //   2. Inferred brand from children (nested
                            //      cloud-query group, direct connection's
                            //      `cloud_ref`).
                            //   3. Explicit non-brand icon (Lucide UI
                            //      placeholder like `cloud`, `server`).
                            //   4. Generic Lucide `boxes` cube.
                            // Inference (#2) wins over generic Lucide
                            // icons (#3) so a group containing AWS
                            // resources shows the AWS chip even if the
                            // user / legacy data left `icon = "cloud"`.
                            // Visual: brand-colour chip with a white
                            // glyph on top.
                            let explicit_brand = group
                                .icon
                                .as_deref()
                                .filter(|s| !s.is_empty())
                                .and_then(crate::os_icon::canonical_brand_id);
                            let inferred_brand = child_query_brand
                                .get(&gid)
                                .copied()
                                .or_else(|| {
                                    first_cloud_profile.get(&gid).and_then(|pid| {
                                        self.cloud_profiles.iter()
                                            .find(|p| p.id == *pid)
                                            .map(|p| match p.provider.as_str() {
                                                "aws" => "aws",
                                                "k8s" | "kubernetes" => "kubernetes",
                                                _ => "cloud",
                                            })
                                    })
                                });

                            let (folder_glyph, folder_bg): (BrandIcon, Color) =
                                if let Some(brand) = explicit_brand.or(inferred_brand) {
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
                                    // Non-brand explicit icon (e.g. user
                                    // picked Lucide `key` / `lock` for a
                                    // group). Honour it with the user's
                                    // colour or the accent fallback.
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
                            // Render through host_icon so the group
                            // folder respects the global default shape
                            // (Circular / Square / Outline / Initials)
                            // the user picked in Settings -> Interface.
                            let host_style = crate::widgets::resolve_host_icon_style(
                                None,
                                &self.setting_default_host_icon,
                            );
                            let icon_box = crate::widgets::host_icon(
                                host_style,
                                folder_bg,
                                &group.label,
                                Some(folder_glyph.view(18.0, Color::WHITE)),
                                32.0,
                            );

                            // Trailing affordance lives in a Stack overlay on
                            // the trailing corner, exactly like the host card's
                            // kebab, so a group's ⋮ lines up pixel-for-pixel
                            // with a host's. The card reserves the same fixed
                            // trailing pad; the overlay shows the ⋮ on hover and
                            // a muted chevron otherwise (the chevron is the
                            // group affordance that distinguishes folder cards
                            // from host cards at a glance, issue #38 polish).
                            let folder_rtl = crate::i18n::is_rtl_layout();
                            let folder_show_dots = self.hovered_folder_card == Some(gid);
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
                                            text(label)
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
                            .on_press(Message::OpenGroup(gid))
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

                            // ⋮ on hover, chevron otherwise. Both sit in the
                            // same right-aligned overlay slot as the host kebab.
                            let folder_trailing: Element<'_, Message> = if folder_show_dots {
                                crate::widgets::card_kebab_button(
                                    OryxisColors::t().text_muted,
                                    true,
                                    Message::ShowFolderActions(gid),
                                )
                                .into()
                            } else {
                                let chevron = if folder_rtl {
                                    iced_fonts::lucide::chevron_left()
                                } else {
                                    iced_fonts::lucide::chevron_right()
                                };
                                // Center the idle chevron in the same 22×22 box
                                // the hover ⋮ uses, so idle and hover share a
                                // center (no x/y jitter on hover).
                                container(
                                    chevron
                                        .size(14)
                                        .color(OryxisColors::t().text_muted),
                                )
                                .center_x(Length::Fixed(22.0))
                                .center_y(Length::Fixed(22.0))
                                .into()
                            };
                            let folder_dots_align = if folder_rtl {
                                iced::alignment::Horizontal::Left
                            } else {
                                iced::alignment::Horizontal::Right
                            };
                            let folder_dots_pad = if folder_rtl {
                                Padding { top: 0.0, right: 0.0, bottom: 0.0, left: 4.0 }
                            } else {
                                Padding { top: 0.0, right: 4.0, bottom: 0.0, left: 0.0 }
                            };
                            let folder_trailing_overlay = container(folder_trailing)
                                .width(Length::Fill)
                                .height(Length::Fill)
                                .align_x(folder_dots_align)
                                .align_y(iced::alignment::Vertical::Center)
                                .padding(folder_dots_pad);
                            let folder_element: Element<'_, Message> = iced::widget::Stack::new()
                                .push(folder_card)
                                .push(folder_trailing_overlay)
                                .into();

                            // Wrap in MouseArea so hover events drive the
                            // dots-button visibility (same UX as host cards).
                            let wrapped = MouseArea::new(folder_element)
                                .on_enter(Message::FolderCardHovered(gid))
                                .on_exit(Message::FolderCardUnhovered);
                            group_cards.push((Element::from(container(wrapped).width(Length::Fill).clip(true)), folder_bg, DashNavItem::Group(gid)));
                        }
            }

            // ── Dynamic (cloud-query) groups ──
            // Manual groups only render above when at least one
            // Connection points at them. Dynamic groups carry their
            // contents through `cloud_query` (ECS tasks resolved on
            // expand) and would otherwise stay invisible. At root we
            // only show dynamic groups WITHOUT a `parent_id`; nested
            // ones (auto-imported under their cloud-profile folder)
            // surface when the user opens that folder, just like
            // manual nested groups would. Sorted indices keep dynamic
            // groups interleaved with manual folders by the same rule
            // (label / created_at) instead of vault-insertion order.
            // Filter first so the sort only touches the cards that
            // actually render instead of the whole group list.
            let mut dyn_group_order: Vec<usize> = (0..self.groups.len())
                .filter(|&i| {
                    let g = &self.groups[i];
                    let Some(query) = g.cloud_query.as_ref() else {
                        return false;
                    };
                    g.parent_id.is_none()
                        // Hidden when the provider plugin isn't installed.
                        && !hidden_profiles.contains(&query.profile_id)
                        && (search_lower.is_empty()
                            || g.label.to_lowercase().contains(&search_lower))
                        && self
                            .host_filter_cloud_profile
                            .is_none_or(|pid| query.profile_id == pid)
                })
                .collect();
            self.hosts_sort.sort_items(
                &mut dyn_group_order,
                |&i| self.groups[i].label.clone(),
                |&i| self.groups[i].created_at,
            );
            for dyn_i in dyn_group_order {
                let group = &self.groups[dyn_i];
                let Some(query) = group.cloud_query.as_ref() else { continue };
                let gid = group.id;
                let subtitle = match &query.kind {
                    oryxis_core::models::cloud::CloudQueryKind::EcsTasks {
                        cluster, ..
                    } => format!("ECS · {cluster}"),
                    oryxis_core::models::cloud::CloudQueryKind::K8sPods {
                        context, namespace, ..
                    } => format!("K8s · {context}/{namespace}"),
                };

                // Icon precedence (matches manual-folder cards):
                //   1. Explicit `group.icon` set by the user via the
                //      dynamic-group editor (Phase 4): wins so a
                //      renamed/recustomised ECS group reflects the
                //      user's choice.
                //   2. Otherwise the query-derived brand (`ecs` for
                //      ECS tasks, `kubernetes` for K8s pods) so a
                //      fresh import still shows the right glyph.
                // Background precedence:
                //   1. Explicit `group.color` (hex) wins.
                //   2. Otherwise the icon's brand colour from
                //      `provider_icon` (orange for ecs/aws, blue for
                //      kubernetes, ...).
                let query_brand: &str = match query.kind {
                    oryxis_core::models::cloud::CloudQueryKind::EcsTasks { .. } => "ecs",
                    oryxis_core::models::cloud::CloudQueryKind::K8sPods { .. } => "kubernetes",
                };
                let icon_id: &str = group
                    .icon
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(query_brand);
                let folder_glyph = crate::os_icon::custom_icon_glyph(icon_id);
                let folder_bg = group
                    .color
                    .as_deref()
                    .and_then(crate::os_icon::parse_hex_color)
                    .unwrap_or_else(|| {
                        crate::os_icon::provider_icon(
                            icon_id,
                            OryxisColors::t().accent,
                        )
                        .1
                    });
                // Render via host_icon so the dynamic-group folder
                // mirrors the global shape preference, same as the
                // manual-folder card above.
                let host_style = crate::widgets::resolve_host_icon_style(
                    None,
                    &self.setting_default_host_icon,
                );
                let icon_box = crate::widgets::host_icon(
                    host_style,
                    folder_bg,
                    &group.label,
                    Some(folder_glyph.view(18.0, Color::WHITE)),
                    32.0,
                );

                // Kebab + hover state, same convention as host /
                // manual-folder cards. Edit + Delete via the overlay
                // menu wired in `dispatch_cloud`.
                const DG_DOTS_SLOT_W: f32 = 22.0;
                let show_dots = self.hovered_dynamic_group_card == Some(gid);
                let dyn_actions_btn: Element<'_, Message> = if show_dots {
                    crate::widgets::card_kebab_button(
                        OryxisColors::t().text_muted,
                        true,
                        Message::ShowDynamicGroupCardMenu(gid),
                    )
                    .into()
                } else {
                    // Same trailing chevron affordance as manual folder
                    // cards (group cards read as "openable" at a glance).
                    let chevron = if crate::i18n::is_rtl_layout() {
                        iced_fonts::lucide::chevron_left()
                    } else {
                        iced_fonts::lucide::chevron_right()
                    };
                    container(chevron.size(14).color(OryxisColors::t().text_muted))
                        .center_x(Length::Fixed(DG_DOTS_SLOT_W))
                        .center_y(Length::Fixed(DG_DOTS_SLOT_W))
                        .into()
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
                                text(subtitle)
                                    .size(10)
                                    .color(OryxisColors::t().text_muted)
                                    .wrapping(iced::widget::text::Wrapping::None),
                            ]
                            .width(Length::Fill)
                            .align_x(crate::widgets::dir_align_x())
                            .clip(true)
                            .into(),
                            dyn_actions_btn,
                        ])
                        .align_y(iced::Alignment::Center),
                    )
                    .padding(Padding {
                        top: 8.0,
                        right: 6.0,
                        bottom: 8.0,
                        left: 8.0,
                    }),
                )
                .on_press(Message::OpenGroup(gid))
                .width(Length::Fill)
                .style(|_, status| {
                    let (bg, bc, bw) = match status {
                        BtnStatus::Hovered => (OryxisColors::t().bg_hover, OryxisColors::t().accent, 1.5),
                        BtnStatus::Pressed => (OryxisColors::t().bg_selected, OryxisColors::t().accent, 2.0),
                        _ => (OryxisColors::t().bg_surface, OryxisColors::t().border, 1.0),
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border {
                            radius: Radius::from(10.0),
                            color: bc,
                            width: bw,
                        },
                        ..Default::default()
                    }
                });

                let wrapped = MouseArea::new(folder_card)
                    .on_enter(Message::DynamicGroupCardHovered(gid))
                    .on_exit(Message::DynamicGroupCardUnhovered)
                    .on_right_press(Message::ShowDynamicGroupCardMenu(gid));
                group_cards.push((Element::from(container(wrapped).width(Length::Fill).clip(true)), folder_bg, DashNavItem::Group(gid)));
            }
        } else if let Some(active_gid) = self.active_group {
            // Inside a folder: render its nested dynamic groups (e.g.
            // ECS service / K8s deployment dynamic groups whose
            // `parent_id` points at this folder). Same card style as
            // the root pass, just filtered by parent. Same sort rule
            // too so the nested view stays consistent with the root.
            // Filter first, same as the root pass, so the sort only
            // covers the cards that actually render.
            let mut nested_dyn_order: Vec<usize> = (0..self.groups.len())
                .filter(|&i| {
                    let g = &self.groups[i];
                    let Some(query) = g.cloud_query.as_ref() else {
                        return false;
                    };
                    g.parent_id == Some(active_gid)
                        // Hidden when the provider plugin isn't installed.
                        && !hidden_profiles.contains(&query.profile_id)
                        && (search_lower.is_empty()
                            || g.label.to_lowercase().contains(&search_lower))
                        && self
                            .host_filter_cloud_profile
                            .is_none_or(|pid| query.profile_id == pid)
                })
                .collect();
            self.hosts_sort.sort_items(
                &mut nested_dyn_order,
                |&i| self.groups[i].label.clone(),
                |&i| self.groups[i].created_at,
            );
            for nested_i in nested_dyn_order {
                let group = &self.groups[nested_i];
                let Some(query) = group.cloud_query.as_ref() else { continue };
                let gid = group.id;
                let subtitle = match &query.kind {
                    oryxis_core::models::cloud::CloudQueryKind::EcsTasks {
                        cluster, ..
                    } => format!("ECS · {cluster}"),
                    oryxis_core::models::cloud::CloudQueryKind::K8sPods {
                        context, namespace, ..
                    } => format!("K8s · {context}/{namespace}"),
                };

                // Mirror the root-level dynamic-group precedence so a
                // nested ECS group reacts to the user's icon / colour
                // edits in the Edit Cloud Group panel. Icon falls back
                // to the query brand (`ecs` / `kubernetes`), colour
                // falls back to the icon's brand colour. Previously
                // the nested path looked at `group.icon` alone and
                // ignored `group.color` entirely, so colour edits
                // never reached the card.
                let query_brand: &str = match query.kind {
                    oryxis_core::models::cloud::CloudQueryKind::EcsTasks { .. } => "ecs",
                    oryxis_core::models::cloud::CloudQueryKind::K8sPods { .. } => "kubernetes",
                };
                let icon_id: &str = group
                    .icon
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .map(|s| s.strip_prefix("si:").unwrap_or(s))
                    .unwrap_or(query_brand);
                let folder_glyph = crate::os_icon::custom_icon_glyph(icon_id);
                let folder_bg = group
                    .color
                    .as_deref()
                    .and_then(crate::os_icon::parse_hex_color)
                    .unwrap_or_else(|| {
                        crate::os_icon::provider_icon(
                            icon_id,
                            OryxisColors::t().accent,
                        )
                        .1
                    });
                let host_style = crate::widgets::resolve_host_icon_style(
                    None,
                    &self.setting_default_host_icon,
                );
                let icon_box = crate::widgets::host_icon(
                    host_style,
                    folder_bg,
                    &group.label,
                    Some(folder_glyph.view(18.0, Color::WHITE)),
                    32.0,
                );

                const DG_DOTS_SLOT_W: f32 = 22.0;
                let show_dots = self.hovered_dynamic_group_card == Some(gid);
                let dyn_actions_btn: Element<'_, Message> = if show_dots {
                    crate::widgets::card_kebab_button(
                        OryxisColors::t().text_muted,
                        true,
                        Message::ShowDynamicGroupCardMenu(gid),
                    )
                    .into()
                } else {
                    // Same trailing chevron affordance as manual folder
                    // cards (group cards read as "openable" at a glance).
                    let chevron = if crate::i18n::is_rtl_layout() {
                        iced_fonts::lucide::chevron_left()
                    } else {
                        iced_fonts::lucide::chevron_right()
                    };
                    container(chevron.size(14).color(OryxisColors::t().text_muted))
                        .center_x(Length::Fixed(DG_DOTS_SLOT_W))
                        .center_y(Length::Fixed(DG_DOTS_SLOT_W))
                        .into()
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
                                text(subtitle)
                                    .size(10)
                                    .color(OryxisColors::t().text_muted)
                                    .wrapping(iced::widget::text::Wrapping::None),
                            ]
                            .width(Length::Fill)
                            .align_x(crate::widgets::dir_align_x())
                            .clip(true)
                            .into(),
                            dyn_actions_btn,
                        ])
                        .align_y(iced::Alignment::Center),
                    )
                    // Match the host-card padding so dynamic-group
                    // cards line up at the same height when they sit
                    // beside hosts in the same grid row.
                    .padding(Padding { top: 8.0, right: 6.0, bottom: 8.0, left: 2.0 }),
                )
                .on_press(Message::OpenGroup(gid))
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
                let wrapped = MouseArea::new(folder_card)
                    .on_enter(Message::DynamicGroupCardHovered(gid))
                    .on_exit(Message::DynamicGroupCardUnhovered)
                    .on_right_press(Message::ShowDynamicGroupCardMenu(gid));
                group_cards.push((Element::from(container(wrapped).width(Length::Fill).clip(true)), folder_bg, DashNavItem::Group(gid)));
            }
        }
        group_cards
    }
}
