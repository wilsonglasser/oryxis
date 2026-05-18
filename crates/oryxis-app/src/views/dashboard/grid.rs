//! Dashboard main content, the responsive grid of folder cards, host
//! cards, and dynamic-group cards plus the two early-return paths
//! (zero connections, dynamic-group view). The biggest chunk of
//! `view_dashboard`, lifted here so the orchestrator stays thin.
//!
//! Returns the full `main_content` (toolbar + search + status + body).
//! The mod-level `view_dashboard` only wraps it with the right-side
//! panel slot.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, scrollable, text, text_input, MouseArea, Space};
use uuid::Uuid;
use iced::{Background, Border, Color, Element, Length, Padding};

use oryxis_core::models::connection::AuthMethod;

use crate::app::{Message, Oryxis, CARD_WIDTH};
use crate::i18n::t;
use crate::os_icon::BrandIcon;
use crate::theme::OryxisColors;
use crate::widgets::{card_grid_columns, dir_align_x, dir_row, distribute_card_grid};

impl Oryxis {
    /// Recursively check whether a group contains at least one host or
    /// nested dynamic group whose cloud origin matches `profile_id`.
    /// Used by the cloud-profile filter chip so a parent folder stays
    /// visible when only its descendants match. Bounded by the
    /// underlying acyclic group structure; if upstream data ever holds
    /// a cycle, the visited set would need to be carried explicitly.
    fn group_contains_cloud_profile(&self, gid: Uuid, profile_id: Uuid) -> bool {
        if self.connections.iter().any(|c| {
            c.group_id == Some(gid)
                && c.cloud_ref.as_ref().map(|r| r.profile_id) == Some(profile_id)
        }) {
            return true;
        }
        for child in self.groups.iter().filter(|g| g.parent_id == Some(gid)) {
            if let Some(q) = &child.cloud_query
                && q.profile_id == profile_id
            {
                return true;
            }
            if self.group_contains_cloud_profile(child.id, profile_id) {
                return true;
            }
        }
        false
    }

    pub(super) fn dashboard_main_content(&self) -> Element<'_, Message> {
        let toolbar = self.dashboard_toolbar();

        // ── Search bar ──
        // In Workspace mode the search lives on the contextual sub-nav
        // (`view_vault_sub_nav`) instead of taking a full content row,
        // so the wide bar collapses to a zero-height spacer here.
        let workspace_mode = self.setting_layout_mode == "workspace";
        let search_bar: Element<'_, Message> = if workspace_mode {
            Space::new().height(0).into()
        } else {
            container(
                text_input(t("search_hosts"), &self.host_search)
                    .id(iced::widget::Id::new("search-dashboard"))
                    .on_input(Message::HostSearchChanged)
                    .padding(10)
                    .size(13)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
            )
            .padding(Padding { top: 0.0, right: 24.0, bottom: 12.0, left: 24.0 })
            .width(Length::Fill)
            .into()
        };

        // The host editor's validation error renders inside the
        // editor panel itself (`host_panel::view_host_panel`) right
        // above the Save button. Slot reserved for future list-level
        // statuses.
        let status: Element<'_, Message> = Space::new().height(0).into();
        // ── Host cards grid ──
        // Cards are collected in two parallel buckets so the renderer
        // can choose between a flat single grid (legacy mode) or two
        // labelled sections (Termius-style "Groups" + "Hosts" headers
        // when `flatten_hosts` is on at root).
        let mut group_cards: Vec<Element<'_, Message>> = Vec::new();
        let mut host_cards: Vec<Element<'_, Message>> = Vec::new();
        let at_root = self.active_group.is_none();
        let flatten = self.flatten_hosts && at_root;

        if self.connections.is_empty() && self.groups.is_empty() {
            // Termius-style empty state, centered "Create host" with input
            let has_input = !self.quick_host_input.is_empty();
            let btn_bg = if has_input { OryxisColors::t().success } else { OryxisColors::t().bg_surface };

            let empty_state = container(
                column![
                    // Icon
                    container(
                        iced_fonts::lucide::server().size(32).color(OryxisColors::t().text_muted),
                    )
                    .padding(16)
                    .style(|_| container::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_surface)),
                        border: Border { radius: Radius::from(12.0), ..Default::default() },
                        ..Default::default()
                    }),
                    Space::new().height(20),
                    text(crate::i18n::t("create_host_title")).size(20).color(OryxisColors::t().text_primary),
                    Space::new().height(8),
                    text(crate::i18n::t("create_host_desc"))
                        .size(13).color(OryxisColors::t().text_muted),
                    Space::new().height(24),
                    // Hostname input
                    text_input(t("type_ip_or_hostname"), &self.quick_host_input)
                        .on_input(Message::QuickHostInput)
                        .on_submit(Message::QuickHostContinue)
                        .padding(14)
                        .width(380)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
                    Space::new().height(12),
                    // Continue button
                    button(
                        container(text(crate::i18n::t("continue_btn")).size(14).color(OryxisColors::t().text_primary))
                            .padding(Padding { top: 12.0, right: 0.0, bottom: 12.0, left: 0.0 })
                            .width(380)
                            .center_x(380),
                    )
                    .on_press(Message::QuickHostContinue)
                    .width(380)
                    .style(move |_, _| button::Style {
                        background: Some(Background::Color(btn_bg)),
                        border: Border { radius: Radius::from(8.0), ..Default::default() },
                        ..Default::default()
                    }),
                ]
                .align_x(iced::Alignment::Center),
            )
            .center(Length::Fill);

            let main_content = column![toolbar, search_bar, status, empty_state]
                .width(Length::Fill)
                .height(Length::Fill);

            return main_content.into();
        }

        // Dynamic-group early-return: if the user opened a cloud-query
        // group (ECS service / future K8s deployment), short-circuit
        // the regular host-card flow and show a placeholder explaining
        // that the live-task resolver isn't wired yet. Without this
        // the panel renders an empty grid and the user can't tell
        // whether the import worked.
        if let Some(gid) = self.active_group
            && let Some(group) = self.groups.iter().find(|g| g.id == gid)
            && let Some(query) = group.cloud_query.as_ref()
        {
            let detail = match &query.kind {
                oryxis_core::models::cloud::CloudQueryKind::EcsTasks {
                    cluster,
                    service,
                    container,
                } => format!("ECS · {cluster} / {service} / {container}"),
                oryxis_core::models::cloud::CloudQueryKind::K8sPods {
                    context,
                    namespace,
                    selector,
                } => format!("K8s · {context} / {namespace} / {selector:?}"),
            };

            // Sub-header row: provider/path detail + Refresh icon.
            // Sits below the standard toolbar (which already carries
            // the "← All hosts" back button + the breadcrumb), so the
            // user can always navigate out of a dynamic group view.
            let header = container(
                dir_row(vec![
                    text(detail.clone())
                        .size(11)
                        .color(OryxisColors::t().text_muted)
                        .into(),
                    Space::new().width(Length::Fill).into(),
                    button(
                        iced_fonts::lucide::refresh_cw()
                            .size(13)
                            .color(OryxisColors::t().text_muted),
                    )
                    .on_press(Message::DynamicGroupResolve(gid))
                    .padding(Padding {
                        top: 4.0,
                        right: 8.0,
                        bottom: 4.0,
                        left: 8.0,
                    })
                    .style(|_, status| {
                        let bg = match status {
                            BtnStatus::Hovered => OryxisColors::t().bg_hover,
                            _ => OryxisColors::t().bg_surface,
                        };
                        button::Style {
                            background: Some(Background::Color(bg)),
                            border: Border {
                                radius: Radius::from(6.0),
                                ..Default::default()
                            },
                            ..Default::default()
                        }
                    })
                    .into(),
                ])
                .align_y(iced::Alignment::Center),
            )
            .padding(Padding {
                top: 4.0,
                right: 24.0,
                bottom: 8.0,
                left: 24.0,
            })
            .width(Length::Fill);

            // Body, drives off the per-group resolve cache. Empty
            // state (no tasks running) is distinct from "not resolved
            // yet" so the user can tell the difference.
            let body: Element<'_, Message> = match self.cloud_dynamic_group_state.get(&gid) {
                None => container(
                    text(t("cloud_dynamic_group_pending"))
                        .size(13)
                        .color(OryxisColors::t().text_muted),
                )
                .center(Length::Fill)
                .into(),
                Some(crate::state::DynamicGroupState::Loading) => container(
                    text(t("cloud_discover_running"))
                        .size(13)
                        .color(OryxisColors::t().text_muted),
                )
                .center(Length::Fill)
                .into(),
                Some(crate::state::DynamicGroupState::Failed(msg)) => container(
                    text(format!("{}: {msg}", t("cloud_test_failed")))
                        .size(12)
                        .color(OryxisColors::t().error),
                )
                .center(Length::Fill)
                .into(),
                Some(crate::state::DynamicGroupState::Loaded { hosts, .. }) => {
                    if hosts.is_empty() {
                        container(
                            text(t("cloud_dynamic_group_no_tasks"))
                                .size(13)
                                .color(OryxisColors::t().text_muted),
                        )
                        .center(Length::Fill)
                        .into()
                    } else {
                        // Pull the ECS coordinates once per body
                        // render so each row can build its own
                        // `aws ecs execute-command` clipboard payload
                        // without re-matching the query kind. K8s
                        // pods leave these empty (no aws-cli copy
                        // makes sense there).
                        let (ecs_cluster, ecs_container) = match &query.kind {
                            oryxis_core::models::cloud::CloudQueryKind::EcsTasks {
                                cluster,
                                container,
                                ..
                            } => (cluster.clone(), container.clone()),
                            _ => (String::new(), String::new()),
                        };
                        let mut items: Vec<Element<'_, Message>> = Vec::new();
                        for h in hosts {
                            let task_id = h.resource_id.clone();
                            let task_label = h.label.clone();
                            let cli_region = h.region.clone().unwrap_or_default();
                            // Use the per-row container so wildcard
                            // queries (empty query.container) still
                            // produce a valid CLI string targeting
                            // the specific container the user
                            // clicked. Falls back to the query
                            // container for single-container imports.
                            let cli_container = h
                                .container_name
                                .clone()
                                .filter(|s| !s.is_empty())
                                .unwrap_or_else(|| ecs_container.clone());
                            let cli_command = if !ecs_cluster.is_empty()
                                && !cli_container.is_empty()
                                && !cli_region.is_empty()
                            {
                                Some(format!(
                                    "aws ecs execute-command --region {} --cluster {} --task {} --container {} --interactive --command /bin/bash",
                                    cli_region, ecs_cluster, task_id, cli_container,
                                ))
                            } else {
                                None
                            };

                            // Primary line: container name (when set,
                            // ECS task with N containers, today N=1
                            // since the query filters to the imported
                            // container) followed by the task id. For
                            // bare resources the task id is the
                            // primary label.
                            let primary = match &h.container_name {
                                Some(name) if !name.is_empty() => name.clone(),
                                _ => h.resource_id.clone(),
                            };
                            let secondary = match (&h.container_name, &h.task_definition) {
                                (Some(_), Some(td)) => {
                                    format!("{}  ·  {}", h.resource_id, td)
                                }
                                (Some(_), None) => h.resource_id.clone(),
                                (None, Some(td)) => td.clone(),
                                _ => String::new(),
                            };

                            // Meta line: IP · AZ · "5m ago". Skipped
                            // pieces collapse so a row with only an IP
                            // doesn't show "  ·  ·  ·  ".
                            let started_str = h
                                .started_at
                                .map(relative_time_ago)
                                .unwrap_or_default();
                            let mut meta_parts: Vec<String> = Vec::new();
                            if let Some(ip) = &h.private_ip
                                && !ip.is_empty()
                            {
                                meta_parts.push(ip.clone());
                            }
                            if let Some(az) = &h.availability_zone
                                && !az.is_empty()
                            {
                                meta_parts.push(az.clone());
                            }
                            if !started_str.is_empty() {
                                meta_parts.push(started_str);
                            }
                            let meta_line = meta_parts.join("  ·  ");

                            // Status pill, colour-coded so the user
                            // can scan RUNNING (green) vs PENDING /
                            // STOPPED at a glance. Unknown statuses
                            // fall through to muted grey.
                            let status_upper: Option<String> = h
                                .status
                                .as_deref()
                                .map(|s| s.to_ascii_uppercase());
                            let status_pill: Element<'_, Message> = match status_upper
                                .as_deref()
                            {
                                Some("RUNNING") => status_pill_widget(
                                    "RUNNING".into(),
                                    OryxisColors::t().success,
                                ),
                                Some("PENDING") | Some("PROVISIONING") => {
                                    status_pill_widget(
                                        status_upper.clone().unwrap(),
                                        OryxisColors::t().warning,
                                    )
                                }
                                Some("STOPPED") | Some("DEACTIVATING") => {
                                    status_pill_widget(
                                        status_upper.clone().unwrap(),
                                        OryxisColors::t().error,
                                    )
                                }
                                Some(_) => status_pill_widget(
                                    status_upper.clone().unwrap(),
                                    OryxisColors::t().text_muted,
                                ),
                                None => Space::new().width(0).into(),
                            };

                            let mut text_col: Vec<Element<'_, Message>> = vec![
                                text(primary)
                                    .size(13)
                                    .color(OryxisColors::t().text_primary)
                                    .wrapping(iced::widget::text::Wrapping::None)
                                    .into(),
                            ];
                            if !secondary.is_empty() {
                                text_col.push(Space::new().height(2).into());
                                text_col.push(
                                    text(secondary)
                                        .size(10)
                                        .color(OryxisColors::t().text_muted)
                                        .wrapping(iced::widget::text::Wrapping::None)
                                        .into(),
                                );
                            }
                            if !meta_line.is_empty() {
                                text_col.push(Space::new().height(2).into());
                                text_col.push(
                                    text(meta_line)
                                        .size(10)
                                        .color(OryxisColors::t().text_muted)
                                        .wrapping(iced::widget::text::Wrapping::None)
                                        .into(),
                                );
                            }

                            items.push(
                                button(
                                    dir_row(vec![
                                        iced_fonts::lucide::container()
                                            .size(16)
                                            .color(OryxisColors::t().text_muted)
                                            .into(),
                                        Space::new().width(10).into(),
                                        iced::widget::Column::with_children(text_col)
                                            .width(Length::Fill)
                                            .align_x(dir_align_x())
                                            .clip(true)
                                            .into(),
                                        Space::new().width(10).into(),
                                        status_pill,
                                    ])
                                    .align_y(iced::Alignment::Center),
                                )
                                .on_press(Message::ConnectEcsExecTask {
                                    group_id: gid,
                                    task_id: task_id.clone(),
                                    task_label,
                                    // Specific container the user
                                    // clicked. Under wildcard mode
                                    // each row is one container; the
                                    // connect path needs the name to
                                    // target the right one in the
                                    // task. Falls back to the query
                                    // container when the row didn't
                                    // populate (legacy hosts).
                                    container: h
                                        .container_name
                                        .clone()
                                        .unwrap_or_else(|| ecs_container.clone()),
                                })
                                .padding(Padding {
                                    top: 10.0,
                                    right: 12.0,
                                    bottom: 10.0,
                                    left: 12.0,
                                })
                                .width(Length::Fill)
                                .style(|_, status| {
                                    let (bg, bc) = match status {
                                        BtnStatus::Hovered => (
                                            OryxisColors::t().bg_hover,
                                            OryxisColors::t().accent,
                                        ),
                                        BtnStatus::Pressed => (
                                            OryxisColors::t().bg_selected,
                                            OryxisColors::t().accent,
                                        ),
                                        _ => (
                                            OryxisColors::t().bg_surface,
                                            OryxisColors::t().border,
                                        ),
                                    };
                                    button::Style {
                                        background: Some(Background::Color(bg)),
                                        border: Border {
                                            radius: Radius::from(6.0),
                                            color: bc,
                                            width: 1.0,
                                        },
                                        ..Default::default()
                                    }
                                })
                                .into(),
                            );
                            // Copy CLI overlay: small button on the
                            // trailing edge of the row that copies
                            // the matching `aws ecs execute-command`
                            // invocation. Lives in a Stack so the
                            // click doesn't leak into the underlying
                            // ConnectEcsExecTask button. Only mounted
                            // when we have enough context to build a
                            // valid command (ECS path, region known).
                            if let Some(cmd) = cli_command {
                                let last_idx = items.len() - 1;
                                let row_el = std::mem::replace(
                                    &mut items[last_idx],
                                    Space::new().height(0).into(),
                                );
                                let copy_btn: Element<'_, Message> = button(
                                    iced_fonts::lucide::clipboard_copy()
                                        .size(13)
                                        .color(OryxisColors::t().text_muted),
                                )
                                .on_press(Message::CopyToClipboard(cmd))
                                .padding(Padding {
                                    top: 4.0,
                                    right: 6.0,
                                    bottom: 4.0,
                                    left: 6.0,
                                })
                                .style(|_, status| {
                                    let bg = match status {
                                        BtnStatus::Hovered => {
                                            OryxisColors::t().bg_hover
                                        }
                                        _ => Color::TRANSPARENT,
                                    };
                                    button::Style {
                                        background: Some(Background::Color(bg)),
                                        border: Border {
                                            radius: Radius::from(4.0),
                                            ..Default::default()
                                        },
                                        ..Default::default()
                                    }
                                })
                                .into();
                                let overlay = container(copy_btn)
                                    .width(Length::Fill)
                                    .height(Length::Fill)
                                    .align_x(iced::alignment::Horizontal::Right)
                                    .align_y(iced::alignment::Vertical::Center)
                                    .padding(Padding {
                                        top: 0.0,
                                        right: 8.0,
                                        bottom: 0.0,
                                        left: 0.0,
                                    });
                                let stacked: Element<'_, Message> =
                                    iced::widget::Stack::new()
                                        .push(row_el)
                                        .push(overlay)
                                        .into();
                                items[last_idx] = stacked;
                            }
                            items.push(Space::new().height(6).into());
                        }
                        items.push(Space::new().height(8).into());
                        scrollable(
                            column(items).padding(Padding {
                                top: 0.0,
                                right: 24.0,
                                bottom: 24.0,
                                left: 24.0,
                            }),
                        )
                        .height(Length::Fill)
                        .into()
                    }
                }
            };

            let main_content = column![toolbar, header, body]
                .width(Length::Fill)
                .height(Length::Fill);
            return main_content.into();
        }

        // Search needle applies to groups and hosts alike; computed
        // once here so every loop below can short-circuit on it.
        let search_lower = self.host_search.to_lowercase();

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
            // Apply the toolbar sort to folder cards. Hidden groups (no
            // direct match) just fall through the search filter below.
            self.hosts_sort.sort_items(
                &mut roots_to_render,
                |gid| {
                    self.groups
                        .iter()
                        .find(|g| g.id == *gid)
                        .map(|g| g.label.clone())
                        .unwrap_or_default()
                },
                |gid| {
                    self.groups
                        .iter()
                        .find(|g| g.id == *gid)
                        .map(|g| g.created_at)
                        .unwrap_or_else(chrono::Utc::now)
                },
            );
            for gid in roots_to_render {
                // Cloud-profile filter, hide folders whose subtree has
                // no host or dynamic group matching the active profile.
                // Active filter intentionally hides every manual,
                // non-cloud folder at root, the chip is the user's
                // explicit "show me only this provider" lens.
                if let Some(filter_pid) = self.host_filter_cloud_profile
                    && !self.group_contains_cloud_profile(gid, filter_pid)
                {
                    continue;
                }
                if let Some(group) = self.groups.iter().find(|g| g.id == gid)
                        && (search_lower.is_empty()
                            || group.label.to_lowercase().contains(&search_lower)) {
                            // Count = direct connections + nested groups
                            // (each nested dynamic group is a record,
                            // even if its tasks are resolved on expand).
                            let direct_hosts = self.connections.iter()
                                .filter(|c| c.group_id == Some(gid)).count();
                            let nested_groups = self.groups.iter()
                                .filter(|g| g.parent_id == Some(gid)).count();
                            let count = direct_hosts + nested_groups;
                            let label = group.label.clone();
                            // Plural form differs across languages (English
                            // pluralizes, Persian/Chinese/Japanese don't); use
                            // the bare i18n word so every locale stays correct.
                            let count_text = format!("{} {}", count, t("hosts").to_lowercase());

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
                            let inferred_brand = self.groups.iter()
                                .filter(|g| g.parent_id == Some(gid))
                                .find_map(|g| g.cloud_query.as_ref())
                                .map(|q| match q.kind {
                                    oryxis_core::models::cloud::CloudQueryKind::EcsTasks { .. } => "ecs",
                                    oryxis_core::models::cloud::CloudQueryKind::K8sPods { .. } => "kubernetes",
                                })
                                .or_else(|| {
                                    self.connections.iter()
                                        .filter(|c| c.group_id == Some(gid))
                                        .find_map(|c| c.cloud_ref.as_ref())
                                        .and_then(|cref| {
                                            self.cloud_profiles.iter()
                                                .find(|p| p.id == cref.profile_id)
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

                            // ⋮ button, only rendered while the folder
                            // row is hovered, mirroring the host-card UX.
                            // A fixed-width placeholder reserves the slot
                            // so the label width budget never changes.
                            const FOLDER_DOTS_SLOT_W: f32 = 22.0;
                            let folder_show_dots = self.hovered_folder_card == Some(gid);
                            let actions_btn: Element<'_, Message> = if folder_show_dots {
                                button(
                                    text("\u{22EE}").size(14).color(OryxisColors::t().text_muted),
                                )
                                .on_press(Message::ShowFolderActions(gid))
                                .padding(Padding { top: 1.0, right: 6.0, bottom: 1.0, left: 6.0 })
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
                                Space::new()
                                    .width(Length::Fixed(FOLDER_DOTS_SLOT_W))
                                    .height(Length::Fixed(1.0))
                                    .into()
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
                                        actions_btn,
                                    ]).align_y(iced::Alignment::Center),
                                )
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

                            // Wrap in MouseArea so hover events drive the
                            // dots-button visibility (same UX as host cards).
                            let wrapped = MouseArea::new(folder_card)
                                .on_enter(Message::FolderCardHovered(gid))
                                .on_exit(Message::FolderCardUnhovered);
                            group_cards.push(container(wrapped).width(Length::Fill).clip(true).into());
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
            let mut dyn_group_order: Vec<usize> = (0..self.groups.len()).collect();
            self.hosts_sort.sort_items(
                &mut dyn_group_order,
                |&i| self.groups[i].label.clone(),
                |&i| self.groups[i].created_at,
            );
            for dyn_i in dyn_group_order {
                let group = &self.groups[dyn_i];
                let Some(query) = group.cloud_query.as_ref() else { continue };
                if group.parent_id.is_some() { continue }
                if !search_lower.is_empty()
                    && !group.label.to_lowercase().contains(&search_lower)
                {
                    continue;
                }
                if let Some(filter_pid) = self.host_filter_cloud_profile
                    && query.profile_id != filter_pid
                {
                    continue;
                }
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
                    button(text("\u{22EE}").size(14).color(OryxisColors::t().text_muted))
                        .on_press(Message::ShowDynamicGroupCardMenu(gid))
                        .padding(Padding { top: 1.0, right: 6.0, bottom: 1.0, left: 6.0 })
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
                    Space::new()
                        .width(Length::Fixed(DG_DOTS_SLOT_W))
                        .height(Length::Fixed(1.0))
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
                group_cards.push(container(wrapped).width(Length::Fill).clip(true).into());
            }
        } else if let Some(active_gid) = self.active_group {
            // Inside a folder: render its nested dynamic groups (e.g.
            // ECS service / K8s deployment dynamic groups whose
            // `parent_id` points at this folder). Same card style as
            // the root pass, just filtered by parent. Same sort rule
            // too so the nested view stays consistent with the root.
            let mut nested_dyn_order: Vec<usize> = (0..self.groups.len()).collect();
            self.hosts_sort.sort_items(
                &mut nested_dyn_order,
                |&i| self.groups[i].label.clone(),
                |&i| self.groups[i].created_at,
            );
            for nested_i in nested_dyn_order {
                let group = &self.groups[nested_i];
                let Some(query) = group.cloud_query.as_ref() else { continue };
                if group.parent_id != Some(active_gid) { continue }
                if !search_lower.is_empty()
                    && !group.label.to_lowercase().contains(&search_lower)
                {
                    continue;
                }
                if let Some(filter_pid) = self.host_filter_cloud_profile
                    && query.profile_id != filter_pid
                {
                    continue;
                }
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
                    button(text("\u{22EE}").size(14).color(OryxisColors::t().text_muted))
                        .on_press(Message::ShowDynamicGroupCardMenu(gid))
                        .padding(Padding { top: 1.0, right: 6.0, bottom: 1.0, left: 6.0 })
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
                    Space::new()
                        .width(Length::Fixed(DG_DOTS_SLOT_W))
                        .height(Length::Fixed(1.0))
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
                group_cards.push(container(wrapped).width(Length::Fill).clip(true).into());
            }
        }

        // Show host cards, filtered by active group and search.
        // Reorder by the toolbar sort first; `idx` still references the
        // vault position so downstream messages (EditConnection,
        // ConnectSsh, …) keep targeting the right row.
        let mut host_order: Vec<usize> = (0..self.connections.len()).collect();
        self.hosts_sort.sort_items(
            &mut host_order,
            |&i| self.connections[i].label.clone(),
            |&i| self.connections[i].created_at,
        );
        for idx in host_order {
            let conn = &self.connections[idx];
            // Filter: inside a folder always restrict to that group;
            // at root, hide grouped hosts only when not flattening
            // (flatten = on means show every host, including grouped
            // ones, in the Hosts section).
            if let Some(gid) = self.active_group {
                if conn.group_id != Some(gid) { continue; }
            } else if conn.group_id.is_some() && !flatten {
                continue;
            }

            // Filter by search query
            if !search_lower.is_empty() {
                let label_match = conn.label.to_lowercase().contains(&search_lower);
                let host_match = conn.hostname.to_lowercase().contains(&search_lower);
                if !label_match && !host_match { continue; }
            }

            // Cloud-profile filter, only hosts imported from the
            // active filter profile survive. Hosts without a cloud
            // origin are hidden too so the lens stays consistent.
            if let Some(filter_pid) = self.host_filter_cloud_profile {
                let matches =
                    conn.cloud_ref.as_ref().map(|r| r.profile_id) == Some(filter_pid);
                if !matches {
                    continue;
                }
            }

            let is_connected = self.tabs.iter().any(|t| t.label == conn.label);
            let auth_label = match conn.auth_method {
                AuthMethod::Auto => t("auth_auto"),
                AuthMethod::Password => t("auth_password"),
                AuthMethod::Key => t("auth_key"),
                AuthMethod::Agent => t("auth_agent"),
                AuthMethod::Interactive => t("auth_interactive"),
            };
            let subtitle = format!("{}@{}:{} · {}", conn.username.as_deref().unwrap_or("root"), conn.hostname, conn.port, auth_label);

            // Resolve icon + brand color from detected OS (if any). Disconnected
            // hosts use the app accent; connected ones use the brand color or
            // success green as fallback.
            let default_fallback = if is_connected {
                OryxisColors::t().success
            } else {
                OryxisColors::t().accent
            };
            let (os_glyph, icon_color) = crate::os_icon::resolve_for(
                conn.detected_os.as_deref(),
                conn.custom_icon.as_deref(),
                conn.custom_color.as_deref(),
                conn.username.as_deref(),
                default_fallback,
            );
            // Fixed 32x32 badge. Shape and color come from the per-host
            // override (icon_style + color) when set; otherwise fall back
            // to the global default_host_icon setting and the OS-derived
            // brand color. Initials style ignores the glyph and renders
            // the leading letters of the label instead.
            let host_style = crate::widgets::resolve_host_icon_style(
                conn.icon_style.as_deref(),
                &self.setting_default_host_icon,
            );
            let badge_color = conn.custom_color.as_deref()
                .or(conn.color.as_deref())
                .and_then(crate::widgets::parse_hex_color)
                .unwrap_or(icon_color);
            let glyph_el: Element<'_, Message> = os_glyph.view(18.0, Color::WHITE);
            let icon_box = crate::widgets::host_icon(
                host_style,
                badge_color,
                &conn.label,
                Some(glyph_el),
                32.0,
            );

            // Floating ⋮ kebab: lives in a Stack overlay on the trailing
            // corner so it doesn't take inline width inside the dir_row.
            // The card reserves a fixed trailing pad so subtitles never
            // collide with the overlay, geometry stays constant regardless
            // of hover state. The button itself is always mounted (so the
            // surrounding MouseArea sees stable child bounds, no hover
            // event loop) and just toggles its glyph color + hover bg.
            let show_dots = self.hovered_card == Some(idx) || self.card_context_menu == Some(idx);
            let rtl = crate::i18n::is_rtl_layout();
            let pad_trailing = 24.0_f32;
            let card_padding = if rtl {
                Padding { top: 8.0, right: 2.0, bottom: 8.0, left: pad_trailing }
            } else {
                Padding { top: 8.0, right: pad_trailing, bottom: 8.0, left: 2.0 }
            };

            // Cloud-origin badge: small brand glyph that used to sit
            // inline with the label (and got clipped on long names).
            // Moved to the LEADING edge of the subtitle row so it
            // never competes with the title for horizontal space.
            // Stored as (brand_key, badge_color, is_orphan) so the
            // glyph can be re-resolved at the use site instead of
            // moved out of a shared tuple (`BrandIcon::view` consumes
            // self and `BrandIcon` doesn't impl Clone).
            let cloud_decoration: Option<(&'static str, Color, bool)> =
                conn.cloud_ref.as_ref().map(|cr| {
                    let provider = self
                        .cloud_profiles
                        .iter()
                        .find(|p| p.id == cr.profile_id)
                        .map(|p| p.provider.as_str())
                        .unwrap_or("cloud");
                    let brand_key: &'static str = match provider {
                        "aws" => "aws",
                        "k8s" | "kubernetes" => "kubernetes",
                        _ => "cloud",
                    };
                    let is_orphan = cr.orphaned_at.is_some();
                    let (_brand_glyph, brand_color_default) = crate::os_icon::provider_icon(
                        brand_key,
                        OryxisColors::t().accent,
                    );
                    let badge_color = if is_orphan {
                        OryxisColors::t().text_muted
                    } else {
                        brand_color_default
                    };
                    (brand_key, badge_color, is_orphan)
                });

            let label_color = match &cloud_decoration {
                Some((_, _, true)) => OryxisColors::t().text_muted,
                _ => OryxisColors::t().text_primary,
            };
            let label_el: Element<'_, Message> = if let Some((_, _, true)) = &cloud_decoration {
                // Orphan: keep the pill next to the label so the user
                // sees it at the title's eye level.
                let muted = OryxisColors::t().text_muted;
                let pill = container(
                    text(t("host_orphan_label"))
                        .size(9)
                        .color(OryxisColors::t().text_muted),
                )
                .padding(Padding {
                    top: 1.0,
                    right: 6.0,
                    bottom: 1.0,
                    left: 6.0,
                })
                .style(move |_| container::Style {
                    background: Some(Background::Color(Color { a: 0.10, ..muted })),
                    border: Border {
                        radius: Radius::from(6.0),
                        color: Color { a: 0.30, ..muted },
                        width: 1.0,
                    },
                    ..Default::default()
                });
                dir_row(vec![
                    text(&conn.label)
                        .size(13)
                        .color(label_color)
                        .wrapping(iced::widget::text::Wrapping::None)
                        .into(),
                    Space::new().width(6).into(),
                    pill.into(),
                ])
                .align_y(iced::Alignment::Center)
                .into()
            } else {
                text(&conn.label)
                    .size(13)
                    .color(label_color)
                    .wrapping(iced::widget::text::Wrapping::None)
                    .into()
            };

            // Subtitle row carries the brand badge on its leading edge
            // when this host is cloud-sourced. Manual hosts get just
            // the subtitle text (no leading gap).
            let subtitle_el: Element<'_, Message> = match &cloud_decoration {
                Some((brand_key, color, _)) => {
                    let glyph = crate::os_icon::custom_icon_glyph(brand_key);
                    dir_row(vec![
                        glyph.view(10.0, *color),
                        Space::new().width(6).into(),
                        text(subtitle)
                            .size(10)
                            .color(OryxisColors::t().text_muted)
                            .wrapping(iced::widget::text::Wrapping::None)
                            .into(),
                    ])
                    .align_y(iced::Alignment::Center)
                    .into()
                }
                None => text(subtitle)
                    .size(10)
                    .color(OryxisColors::t().text_muted)
                    .wrapping(iced::widget::text::Wrapping::None)
                    .into(),
            };

            let card_btn = button(
                container(
                    dir_row(vec![
                        icon_box,
                        Space::new().width(8).into(),
                        iced::widget::Column::with_children(vec![
                            label_el,
                            Space::new().height(2).into(),
                            subtitle_el,
                        ])
                        .width(Length::Fill)
                        .align_x(crate::widgets::dir_align_x())
                        .clip(true)
                        .into(),
                    ]).align_y(iced::Alignment::Center),
                )
                .padding(card_padding),
            )
            .on_press(Message::ConnectSsh(idx))
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

            let dots_glyph_color = if show_dots {
                OryxisColors::t().text_muted
            } else {
                Color::TRANSPARENT
            };
            let dots_btn = button(
                text("\u{22EE}").size(14).color(dots_glyph_color),
            )
            .on_press(Message::ShowCardMenu(idx))
            .padding(Padding { top: 1.0, right: 6.0, bottom: 1.0, left: 6.0 })
            .style(move |_, status| {
                let bg = match status {
                    BtnStatus::Hovered if show_dots => OryxisColors::t().bg_hover,
                    _ => Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                }
            });
            let dots_align = if rtl {
                iced::alignment::Horizontal::Left
            } else {
                iced::alignment::Horizontal::Right
            };
            let dots_pad = if rtl {
                Padding { top: 0.0, right: 0.0, bottom: 0.0, left: 4.0 }
            } else {
                Padding { top: 0.0, right: 4.0, bottom: 0.0, left: 0.0 }
            };
            let dots_overlay = container(dots_btn)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(dots_align)
                .align_y(iced::alignment::Vertical::Center)
                .padding(dots_pad);
            let card_element: Element<'_, Message> = iced::widget::Stack::new()
                .push(card_btn)
                .push(dots_overlay)
                .into();

            // Wrap in MouseArea for hover tracking and right-click
            let wrapped = MouseArea::new(card_element)
                .on_enter(Message::CardHovered(idx))
                .on_exit(Message::CardUnhovered)
                .on_right_press(Message::ShowCardMenu(idx));

            host_cards.push(container(wrapped).width(Length::Fill).clip(true).into());
        }

        // Column count adapts to current window width minus the visible
        // chrome (left nav + optional right panel + horizontal padding).
        // Re-derived on every view() so resizing the window or toggling
        // the side panel reflows the cards into the new column count.
        let nav_width = if self.sidebar_collapsed {
            crate::app::SIDEBAR_WIDTH_COLLAPSED
        } else {
            crate::app::SIDEBAR_WIDTH
        };
        let panel_open = self.cloud_discover_visible || self.show_host_panel;
        let panel_width = if panel_open { crate::app::PANEL_WIDTH } else { 0.0 };
        let available = (self.window_size.width - nav_width - panel_width - 48.0).max(0.0);
        let cols = card_grid_columns(available, CARD_WIDTH, 12.0);

        // Section header (Termius-style "Groups" / "Hosts" labels).
        // Only rendered in flatten mode at root, where the user can
        // see both lists side-by-side.
        // Wrap the label in a width-fill container so it lines up
        // with the card grid's leading edge. The plain `text` widget
        // shrinks to content and the column's `align_x` pushes the
        // shrunk box around in a way that doesn't always coincide
        // with the card border; making the container Fill anchors it
        // explicitly to the leading edge of the row. Also mirrors
        // Keychain's section_title vertical padding (4 px top, 8 px
        // bottom) so the section labels sit at the same offset
        // relative to the search bar as they do in the Keychain.
        let section_header = |label_key: &'static str| -> Element<'_, Message> {
            container(
                container(
                    text(t(label_key))
                        .size(14)
                        .color(OryxisColors::t().text_muted),
                )
                .width(Length::Fill)
                .align_x(crate::widgets::dir_align_x()),
            )
            .padding(Padding { top: 4.0, right: 0.0, bottom: 8.0, left: 0.0 })
            .into()
        };

        let mut content_rows: Vec<Element<'_, Message>> = Vec::new();
        if flatten {
            if !group_cards.is_empty() {
                // `section_header` already carries its own 4/8 vertical
                // padding (mirroring Keychain), so no extra Space below.
                content_rows.push(section_header("groups_section"));
                content_rows.push(distribute_card_grid(group_cards, cols, 12.0, 12.0));
                content_rows.push(Space::new().height(20).into());
            }
            if !host_cards.is_empty() {
                content_rows.push(section_header("hosts_section"));
                content_rows.push(distribute_card_grid(host_cards, cols, 12.0, 12.0));
            }
        } else {
            // Legacy: groups (if any) first, then hosts, in one grid.
            let mut combined = group_cards;
            combined.extend(host_cards);
            content_rows.push(distribute_card_grid(combined, cols, 12.0, 12.0));
        }

        // Each grid row holds up to 3 fixed-width cards; once the row
        // is narrower than the available column width, the column's
        // cross-axis alignment decides whether the row sticks to the
        // leading or trailing edge. Use `dir_align_x()` so cards begin
        // from the trailing edge of the LTR layout (= leading edge of
        // the RTL layout), keeping them aligned with the toolbar title
        // / actions on the same side.
        // The column needs `Length::Fill` for `align_x` to have any
        // slack to align inside, without it the column shrinks to
        // content and the rows still hug the leading edge.
        let grid = scrollable(
            column(content_rows)
                .width(Length::Fill)
                .padding(Padding { top: 0.0, right: 24.0, bottom: 24.0, left: 24.0 })
                .align_x(crate::widgets::dir_align_x()),
        ).height(Length::Fill);

        // Cloud-profile filter chip, only rendered while a filter is
        // active. Sits between search and the grid so the user always
        // has a visible way to clear it. Picks the brand glyph and
        // colour from the active profile's provider so AWS reads
        // orange, K8s blue, etc.
        let filter_chip: Element<'_, Message> = if let Some(filter_pid) =
            self.host_filter_cloud_profile
        {
            let profile = self.cloud_profiles.iter().find(|p| p.id == filter_pid);
            let profile_label = profile.map(|p| p.label.clone()).unwrap_or_default();
            let provider = profile.map(|p| p.provider.as_str()).unwrap_or("cloud");
            let brand_key = match provider {
                "aws" => "aws",
                "k8s" | "kubernetes" => "kubernetes",
                _ => "cloud",
            };
            let (brand_glyph, brand_color) =
                crate::os_icon::provider_icon(brand_key, OryxisColors::t().accent);
            let bg_color = brand_color;
            let chip = container(
                dir_row(vec![
                    brand_glyph.view(12.0, brand_color),
                    Space::new().width(6).into(),
                    text(crate::i18n::t("host_filter_active"))
                        .size(11)
                        .color(OryxisColors::t().text_muted)
                        .into(),
                    Space::new().width(4).into(),
                    text(profile_label)
                        .size(11)
                        .color(OryxisColors::t().text_primary)
                        .into(),
                    Space::new().width(6).into(),
                    button(
                        text("\u{00D7}")
                            .size(13)
                            .color(OryxisColors::t().text_muted),
                    )
                    .on_press(Message::HostFilterByCloudProfile(None))
                    .padding(Padding {
                        top: 0.0,
                        right: 6.0,
                        bottom: 0.0,
                        left: 6.0,
                    })
                    .style(|_, _| button::Style {
                        background: None,
                        ..Default::default()
                    })
                    .into(),
                ])
                .align_y(iced::Alignment::Center),
            )
            .padding(Padding {
                top: 4.0,
                right: 4.0,
                bottom: 4.0,
                left: 10.0,
            })
            .style(move |_| container::Style {
                background: Some(Background::Color(Color {
                    a: 0.12,
                    ..bg_color
                })),
                border: Border {
                    radius: Radius::from(14.0),
                    color: Color { a: 0.30, ..bg_color },
                    width: 1.0,
                },
                ..Default::default()
            });
            container(chip)
                .padding(Padding {
                    top: 0.0,
                    right: 24.0,
                    bottom: 8.0,
                    left: 24.0,
                })
                .align_x(dir_align_x())
                .width(Length::Fill)
                .into()
        } else {
            Space::new().height(0).into()
        };

        let main_content = column![toolbar, search_bar, filter_chip, status, grid]
            .width(Length::Fill)
            .height(Length::Fill);
        main_content.into()
    }
}

/// Coloured pill rendering a short status string. Background uses the
/// caller-provided accent (success / warning / error / muted) at low
/// alpha so the pill reads as a chip on either light or dark surfaces
/// without fighting the row's own border.
fn status_pill_widget(label: String, accent: Color) -> Element<'static, Message> {
    container(text(label).size(10).color(accent))
        .padding(Padding { top: 2.0, right: 8.0, bottom: 2.0, left: 8.0 })
        .style(move |_| container::Style {
            background: Some(Background::Color(Color { a: 0.15, ..accent })),
            border: Border {
                radius: Radius::from(6.0),
                color: Color { a: 0.30, ..accent },
                width: 1.0,
            },
            ..Default::default()
        })
        .into()
}

/// Compact "5m ago" / "2h ago" / "3d ago" formatter. Negative or
/// zero-second deltas collapse to "now" so freshly-started tasks read
/// cleanly. Values past 30 days fall through to a plain ISO date so
/// older orphans don't claim impossibly large hour counts.
fn relative_time_ago(t: chrono::DateTime<chrono::Utc>) -> String {
    let now = chrono::Utc::now();
    let delta = now.signed_duration_since(t);
    let secs = delta.num_seconds();
    if secs < 5 {
        return "now".to_string();
    }
    if secs < 60 {
        return format!("{secs}s ago");
    }
    let mins = delta.num_minutes();
    if mins < 60 {
        return format!("{mins}m ago");
    }
    let hours = delta.num_hours();
    if hours < 48 {
        return format!("{hours}h ago");
    }
    let days = delta.num_days();
    if days < 30 {
        return format!("{days}d ago");
    }
    t.format("%Y-%m-%d").to_string()
}
