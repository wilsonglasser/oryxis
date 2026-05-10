//! Dashboard — folders/hosts grid.

use iced::border::Radius;
use iced::widget::{button, column, container, scrollable, text, text_input, MouseArea, Space};
use iced::widget::button::Status as BtnStatus;
use iced::{Background, Border, Color, Element, Length, Padding};

use oryxis_core::models::connection::AuthMethod;

use crate::app::{Message, Oryxis, CARD_WIDTH};
use crate::i18n::t;
use crate::os_icon::BrandIcon;
use crate::theme::OryxisColors;
use crate::widgets::{card_grid_columns, dir_row, distribute_card_grid};

impl Oryxis {
    pub(crate) fn view_dashboard(&self) -> Element<'_, Message> {
        // ── Toolbar ──
        let toolbar_left: Element<'_, Message> = if let Some(gid) = self.active_group {
            let group_name = self.groups.iter()
                .find(|g| g.id == gid)
                .map(|g| g.label.as_str())
                .unwrap_or(t("group_fallback_label"));
            dir_row(vec![
                button(
                    dir_row(vec![
                        iced_fonts::lucide::arrow_left().size(14).color(OryxisColors::t().accent).into(),
                        Space::new().width(6).into(),
                        text(t("all_hosts")).size(14).color(OryxisColors::t().accent).into(),
                    ]).align_y(iced::Alignment::Center),
                )
                .on_press(Message::BackToRoot)
                .padding(Padding { top: 4.0, right: 10.0, bottom: 4.0, left: 10.0 })
                .style(|_, _| button::Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    border: Border::default(),
                    ..Default::default()
                }).into(),
                text("/").size(16).color(OryxisColors::t().text_muted).into(),
                Space::new().width(8).into(),
                iced_fonts::lucide::folder().size(16).color(OryxisColors::t().accent).into(),
                Space::new().width(6).into(),
                text(group_name).size(16).color(OryxisColors::t().text_primary).into(),
            ]).align_y(iced::Alignment::Center).into()
        } else {
            text(t("hosts")).size(20).color(OryxisColors::t().text_primary).into()
        };

        // "+ Host [▾]" split button — primary half opens the manual
        // SSH editor (unchanged), the chevron half opens a cloud
        // provider picker overlay so discovery launches from the
        // Hosts view (where the user naturally goes to add hosts).
        // Layout mirrors the keychain "+ ADD ▼" split exactly so both
        // toolbars stay visually consistent. The chevron half is only
        // emitted when at least one cloud profile is configured —
        // when there's no chevron, the primary button takes back its
        // full corner radius so it doesn't look "cut" on the right.
        let has_chevron = !self.cloud_profiles.is_empty();
        let rtl = crate::i18n::is_rtl_layout();
        // Pre-compute the rounded-corner radii so the leading half
        // always rounds the leading edge and the chevron always
        // rounds the trailing edge — flipped under RTL.
        let label_radius = if !has_chevron {
            Radius::from(6.0)
        } else if rtl {
            Radius { top_left: 0.0, bottom_left: 0.0, top_right: 6.0, bottom_right: 6.0 }
        } else {
            Radius { top_left: 6.0, bottom_left: 6.0, top_right: 0.0, bottom_right: 0.0 }
        };
        let chevron_radius = if rtl {
            Radius { top_left: 6.0, bottom_left: 6.0, top_right: 0.0, bottom_right: 0.0 }
        } else {
            Radius { top_left: 0.0, bottom_left: 0.0, top_right: 6.0, bottom_right: 6.0 }
        };

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
            .padding(Padding { top: 0.0, right: 14.0, bottom: 0.0, left: 14.0 }),
        )
        .on_press(Message::ShowNewConnection)
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

        let action_group: Element<'_, Message> = if has_chevron {
            // 1px divider between the two halves — same alpha-tinted
            // black the keychain split uses.
            let separator = container(Space::new().width(1).height(16))
                .style(|_| container::Style {
                    background: Some(Background::Color(Color { a: 0.3, ..Color::BLACK })),
                    ..Default::default()
                });
            let chevron_btn = button(
                container(
                    iced_fonts::lucide::chevron_down::<iced::Theme, iced::Renderer>()
                        .size(12)
                        .color(OryxisColors::t().button_text),
                )
                .center_y(Length::Fixed(24.0))
                .padding(Padding { top: 0.0, right: 4.0, bottom: 0.0, left: 4.0 }),
            )
            .on_press(Message::ShowCloudProviderPicker)
            .style(move |_, status| {
                let bg = match status {
                    BtnStatus::Hovered => OryxisColors::t().button_bg_hover,
                    _ => OryxisColors::t().button_bg,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: chevron_radius, ..Default::default() },
                    ..Default::default()
                }
            });
            dir_row(vec![primary_btn.into(), separator.into(), chevron_btn.into()])
                .align_y(iced::Alignment::Center)
                .into()
        } else {
            primary_btn.into()
        };

        let toolbar = container(
            dir_row(vec![
                toolbar_left,
                Space::new().width(Length::Fill).into(),
                action_group,
            ]).align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 24.0, bottom: 16.0, left: 24.0 })
        .width(Length::Fill);

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
        // above the Save button — duplicating it here put the message
        // floating in the listing area, which read as a list-level
        // error rather than form feedback. Keep this slot reserved
        // for future list-level statuses.
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
            // Termius-style empty state — centered "Create host" with input
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

            if self.show_host_panel {
                let panel = self.view_host_panel();
                return dir_row(vec![main_content.into(), panel])
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into();
            } else {
                return main_content.into();
            }
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

            let placeholder = container(
                column![
                    container(
                        iced_fonts::lucide::cloud()
                            .size(36)
                            .color(OryxisColors::t().text_muted),
                    )
                    .padding(20)
                    .style(|_| container::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_surface)),
                        border: Border {
                            radius: Radius::from(12.0),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                    Space::new().height(20),
                    text(group.label.clone())
                        .size(18)
                        .color(OryxisColors::t().text_primary),
                    Space::new().height(6),
                    text(detail)
                        .size(12)
                        .color(OryxisColors::t().text_muted),
                    Space::new().height(20),
                    text(t("cloud_dynamic_group_pending"))
                        .size(13)
                        .color(OryxisColors::t().text_secondary),
                ]
                .align_x(iced::Alignment::Center),
            )
            .center(Length::Fill);
            let main_content = column![toolbar, search_bar, status, placeholder]
                .width(Length::Fill)
                .height(Length::Fill);
            return main_content.into();
        }

        if self.active_group.is_none() {
            // Root view: show folder cards for groups that have connections
            let mut shown_groups = std::collections::HashSet::new();
            for conn in &self.connections {
                if let Some(gid) = conn.group_id
                    && shown_groups.insert(gid)
                        && let Some(group) = self.groups.iter().find(|g| g.id == gid) {
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
                                    oryxis_core::models::cloud::CloudQueryKind::EcsTasks { .. } => "aws",
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

                            // ⋮ button — only rendered while the folder
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
                // own cloud_query (ECS → AWS, K8s → Kubernetes). We
                // don't expose a UI to set group icons, so honouring
                // `group.icon` here is moot — derive from the query.
                let brand: &str = match query.kind {
                    oryxis_core::models::cloud::CloudQueryKind::EcsTasks { .. } => "aws",
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

                group_cards.push(container(folder_card).width(Length::Fill).clip(true).into());
            }
        } else if let Some(active_gid) = self.active_group {
            // Inside a folder: render its nested dynamic groups (e.g.
            // ECS service / K8s deployment dynamic groups whose
            // `parent_id` points at this folder). Same card style as
            // the root pass, just filtered by parent.
            for group in &self.groups {
                let Some(query) = group.cloud_query.as_ref() else { continue };
                if group.parent_id != Some(active_gid) { continue }
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
                group_cards.push(container(folder_card).width(Length::Fill).clip(true).into());
            }
        }

        // Show host cards — filtered by active group and search
        let search_lower = self.host_search.to_lowercase();
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

            // Vertical ellipsis (⋮) — always occupies the same space so the
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
                // Invisible placeholder of identical width — reserves the
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
        // slack to align inside — without it the column shrinks to
        // content and the rows still hug the leading edge.
        let grid = scrollable(
            column(content_rows)
                .width(Length::Fill)
                .padding(Padding { top: 0.0, right: 24.0, bottom: 24.0, left: 24.0 })
                .align_x(crate::widgets::dir_align_x()),
        ).height(Length::Fill);

        // ── Main + side panel ──
        let main_content = column![toolbar, search_bar, status, grid]
            .width(Length::Fill)
            .height(Length::Fill);

        // Discovery panel takes priority over the host editor — opening
        // it from the "+ Host [▾]" picker fully replaces any open
        // editor visually until the user dismisses or imports.
        if self.cloud_discover_visible {
            let panel = self.view_cloud_discover_panel();
            dir_row(vec![main_content.into(), panel])
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else if self.show_host_panel {
            let panel = self.view_host_panel();
            dir_row(vec![main_content.into(), panel])
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            main_content.into()
        }
    }
}
