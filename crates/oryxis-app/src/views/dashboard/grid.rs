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
use iced::{Background, Border, Color, Element, Length, Padding};

use oryxis_core::models::connection::AuthMethod;

use crate::app::{Message, Oryxis, CARD_WIDTH};
use crate::i18n::t;
use crate::os_icon::BrandIcon;
use crate::theme::OryxisColors;
use crate::widgets::{card_grid_columns, dir_row, distribute_card_grid};

impl Oryxis {
    pub(super) fn dashboard_main_content(&self) -> Element<'_, Message> {
        let toolbar = self.dashboard_toolbar();

        // ── Search bar ──
        let search_bar = container(
            text_input(t("search_hosts"), &self.host_search)
                .on_input(Message::HostSearchChanged)
                .padding(10)
                .size(13)
                .style(crate::widgets::rounded_input_style),
        )
        .padding(Padding { top: 0.0, right: 24.0, bottom: 12.0, left: 24.0 })
        .width(Length::Fill);

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

        if self.connections.is_empty() {
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
                        .style(crate::widgets::rounded_input_style),
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
                        let mut items: Vec<Element<'_, Message>> = Vec::new();
                        for h in hosts {
                            // Click → ECS Exec into this task. Handler
                            // calls AWS, spawns session-manager-plugin
                            // in a PTY, and opens a new terminal tab.
                            let task_id = h.resource_id.clone();
                            let task_label = h.label.clone();
                            items.push(
                                button(
                                    dir_row(vec![
                                        iced_fonts::lucide::container()
                                            .size(14)
                                            .color(OryxisColors::t().text_muted)
                                            .into(),
                                        Space::new().width(8).into(),
                                        text(h.label.clone())
                                            .size(12)
                                            .color(OryxisColors::t().text_primary)
                                            .into(),
                                    ])
                                    .align_y(iced::Alignment::Center),
                                )
                                .on_press(Message::ConnectEcsExecTask {
                                    group_id: gid,
                                    task_id,
                                    task_label,
                                })
                                .padding(Padding {
                                    top: 8.0,
                                    right: 12.0,
                                    bottom: 8.0,
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
            // Root view: show folder cards for groups that have connections
            let mut shown_groups = std::collections::HashSet::new();
            for conn in &self.connections {
                if let Some(gid) = conn.group_id
                    && shown_groups.insert(gid)
                        && let Some(group) = self.groups.iter().find(|g| g.id == gid)
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
                            let icon_box = container(folder_glyph.view(18.0, Color::WHITE))
                                .width(Length::Fixed(32.0))
                                .height(Length::Fixed(32.0))
                                .center_x(Length::Fixed(32.0))
                                .center_y(Length::Fixed(32.0))
                                .style(move |_| container::Style {
                                    background: Some(Background::Color(folder_bg)),
                                    border: Border {
                                        radius: Radius::from(8.0),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                });

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
                                        icon_box.into(),
                                        Space::new().width(8).into(),
                                        column![
                                            text(label).size(13).color(OryxisColors::t().text_primary),
                                            Space::new().height(2),
                                            text(count_text).size(10).color(OryxisColors::t().text_muted),
                                        ]
                                        .width(Length::Fill)
                                        .align_x(crate::widgets::dir_align_x())
                                        .into(),
                                        actions_btn,
                                    ]).align_y(iced::Alignment::Center),
                                )
                                .padding(Padding { top: 8.0, right: 6.0, bottom: 8.0, left: 8.0 }),
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
            // manual nested groups would.
            for group in &self.groups {
                let Some(query) = group.cloud_query.as_ref() else { continue };
                if group.parent_id.is_some() { continue }
                if !search_lower.is_empty()
                    && !group.label.to_lowercase().contains(&search_lower)
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

                // Dynamic groups always have a known brand from their
                // own cloud_query (ECS hexagon-box, K8s wheel). We
                // don't expose a UI to set group icons, so honouring
                // `group.icon` here is moot, derive from the query.
                let brand: &str = match query.kind {
                    oryxis_core::models::cloud::CloudQueryKind::EcsTasks { .. } => "ecs",
                    oryxis_core::models::cloud::CloudQueryKind::K8sPods { .. } => "kubernetes",
                };
                let folder_glyph = crate::os_icon::custom_icon_glyph(brand);
                let folder_bg = crate::os_icon::provider_icon(
                    brand,
                    OryxisColors::t().accent,
                )
                .1;
                let icon_box = container(folder_glyph.view(18.0, Color::WHITE))
                    .width(Length::Fixed(32.0))
                    .height(Length::Fixed(32.0))
                    .center_x(Length::Fixed(32.0))
                    .center_y(Length::Fixed(32.0))
                    .style(move |_| container::Style {
                        background: Some(Background::Color(folder_bg)),
                        border: Border {
                            radius: Radius::from(8.0),
                            ..Default::default()
                        },
                        ..Default::default()
                    });

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
                            icon_box.into(),
                            Space::new().width(8).into(),
                            column![
                                text(group.label.clone())
                                    .size(13)
                                    .color(OryxisColors::t().text_primary),
                                Space::new().height(2),
                                text(subtitle)
                                    .size(10)
                                    .color(OryxisColors::t().text_muted),
                            ]
                            .width(Length::Fill)
                            .align_x(crate::widgets::dir_align_x())
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
            // the root pass, just filtered by parent.
            for group in &self.groups {
                let Some(query) = group.cloud_query.as_ref() else { continue };
                if group.parent_id != Some(active_gid) { continue }
                if !search_lower.is_empty()
                    && !group.label.to_lowercase().contains(&search_lower)
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

                let (folder_glyph, folder_bg): (BrandIcon, Color) =
                    match group.icon.as_deref() {
                    Some(custom) if !custom.is_empty() => {
                        let glyph = crate::os_icon::custom_icon_glyph(custom);
                        let brand = crate::os_icon::provider_icon(
                            custom.strip_prefix("si:").unwrap_or(custom),
                            OryxisColors::t().accent,
                        )
                        .1;
                        (glyph, brand)
                    }
                    _ => (
                        BrandIcon::Glyph(iced_fonts::lucide::cloud()),
                        OryxisColors::t().accent,
                    ),
                };
                let icon_box = container(folder_glyph.view(18.0, Color::WHITE))
                    .width(Length::Fixed(32.0))
                    .height(Length::Fixed(32.0))
                    .center_x(Length::Fixed(32.0))
                    .center_y(Length::Fixed(32.0))
                    .style(move |_| container::Style {
                        background: Some(Background::Color(folder_bg)),
                        border: Border { radius: Radius::from(8.0), ..Default::default() },
                        ..Default::default()
                    });

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
                            icon_box.into(),
                            Space::new().width(8).into(),
                            column![
                                text(group.label.clone())
                                    .size(13)
                                    .color(OryxisColors::t().text_primary),
                                Space::new().height(2),
                                text(subtitle).size(10).color(OryxisColors::t().text_muted),
                            ]
                            .width(Length::Fill)
                            .align_x(crate::widgets::dir_align_x())
                            .into(),
                            dyn_actions_btn,
                        ])
                        .align_y(iced::Alignment::Center),
                    )
                    .padding(Padding { top: 8.0, right: 6.0, bottom: 8.0, left: 8.0 }),
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

        // Show host cards, filtered by active group and search
        for (idx, conn) in self.connections.iter().enumerate() {
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
            // Fixed 32x32 square box centered on the glyph (Termius-style).
            let icon_box = container(os_glyph.view(18.0, Color::WHITE))
                .width(Length::Fixed(32.0))
                .height(Length::Fixed(32.0))
                .center_x(Length::Fixed(32.0))
                .center_y(Length::Fixed(32.0))
                .style(move |_| container::Style {
                    background: Some(Background::Color(icon_color)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                });

            // Vertical ellipsis (⋮), always occupies the same space so the
            // card's geometry is stable; the button itself is only interactive
            // (and visible) on hover or when its context menu is open. A
            // transparent placeholder keeps the subtitle width budget constant.
            let show_dots = self.hovered_card == Some(idx) || self.card_context_menu == Some(idx);
            const DOTS_SLOT_W: f32 = 22.0;
            let dots_btn: Element<'_, Message> = if show_dots {
                button(
                    text("\u{22EE}").size(14).color(OryxisColors::t().text_muted),
                )
                .on_press(Message::ShowCardMenu(idx))
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
                // Invisible placeholder of identical width, reserves the
                // horizontal slot so the subtitle wrap budget never changes.
                Space::new().width(Length::Fixed(DOTS_SLOT_W)).height(Length::Fixed(1.0)).into()
            };

            // Card body: icon + labels + (dots or placeholder). Subtitle is
            // clamped to a single line via `wrapping::None` so the card's
            // height is identical for every host, regardless of how long the
            // "user@host:port · Auth" string is.
            let card_btn = button(
                container(
                    dir_row(vec![
                        icon_box.into(),
                        Space::new().width(8).into(),
                        column![
                            text(&conn.label)
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
                        dots_btn,
                    ]).align_y(iced::Alignment::Center),
                )
                .padding(Padding { top: 8.0, right: 2.0, bottom: 8.0, left: 8.0 }),
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

            // Wrap in MouseArea for hover tracking and right-click
            let wrapped = MouseArea::new(card_btn)
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
        let section_header = |label_key: &'static str| -> Element<'_, Message> {
            text(t(label_key))
                .size(14)
                .color(OryxisColors::t().text_muted)
                .into()
        };

        let mut content_rows: Vec<Element<'_, Message>> = Vec::new();
        if flatten {
            if !group_cards.is_empty() {
                content_rows.push(section_header("groups_section"));
                content_rows.push(Space::new().height(8).into());
                content_rows.push(distribute_card_grid(group_cards, cols, 12.0, 12.0));
                content_rows.push(Space::new().height(20).into());
            }
            if !host_cards.is_empty() {
                content_rows.push(section_header("hosts_section"));
                content_rows.push(Space::new().height(8).into());
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

        let main_content = column![toolbar, search_bar, status, grid]
            .width(Length::Fill)
            .height(Length::Fill);
        main_content.into()
    }
}
