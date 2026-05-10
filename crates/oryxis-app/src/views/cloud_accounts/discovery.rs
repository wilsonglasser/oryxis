//! Discovery panel + the per-result body that lists EC2 instances and
//! ECS services. The panel houses the title bar, search input, the
//! results list (split into EC2 / ECS sections), and the import action
//! footer. Already-imported entries are greyed out so the user
//! doesn't dupe them.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis, PANEL_WIDTH};
use crate::i18n::t;
use crate::state::CloudDiscoverState;
use crate::theme::OryxisColors;
use crate::widgets::dir_row;

use super::section_header;

impl Oryxis {
    pub(crate) fn view_cloud_discover_panel(&self) -> Element<'_, Message> {
        // Header: title + small refresh-icon button + close (X). The
        // refresh icon lives to the left of the close so the layout
        // mirrors the "title, actions, close" idiom of the host
        // editor panel; both header buttons share the same square chip
        // style so they read as a paired action group.
        let icon_btn_style = |_: &iced::Theme, status: BtnStatus| {
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
        };
        let refresh_icon_btn = button(
            iced_fonts::lucide::refresh_cw()
                .size(13)
                .color(OryxisColors::t().text_muted),
        )
        .on_press(Message::CloudDiscoverRefresh)
        .padding(Padding {
            top: 4.0,
            right: 8.0,
            bottom: 4.0,
            left: 8.0,
        })
        .style(icon_btn_style);
        let close_btn = button(text("\u{00D7}").size(14).color(OryxisColors::t().text_muted))
            .on_press(Message::HideCloudDiscover)
            .padding(Padding {
                top: 4.0,
                right: 8.0,
                bottom: 4.0,
                left: 8.0,
            })
            .style(icon_btn_style);
        let title = container(
            dir_row(vec![
                text(t("cloud_discover"))
                    .size(18)
                    .color(OryxisColors::t().text_primary)
                    .into(),
                Space::new().width(Length::Fill).into(),
                refresh_icon_btn.into(),
                Space::new().width(6).into(),
                close_btn.into(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding {
            top: 20.0,
            right: 20.0,
            bottom: 8.0,
            left: 20.0,
        });

        // Search bar, only meaningful when results are loaded, but
        // we render it always so the panel layout doesn't shift when
        // the state transitions.
        let search = container(
            text_input(t("cloud_discover_search_ph"), &self.cloud_discover_filter)
                .on_input(Message::CloudDiscoverFilterChanged)
                .padding(Padding {
                    top: 8.0,
                    right: 10.0,
                    bottom: 8.0,
                    left: 10.0,
                })
                .style(crate::widgets::rounded_input_style),
        )
        .padding(Padding {
            top: 0.0,
            right: 20.0,
            bottom: 12.0,
            left: 20.0,
        });

        // Body content varies by state, keep each branch self-
        // contained so the layout above stays readable.
        let body: Element<'_, Message> = match &self.cloud_discover_state {
            CloudDiscoverState::Idle => Space::new().height(0).into(),
            CloudDiscoverState::Running => container(
                text(t("cloud_discover_running"))
                    .size(13)
                    .color(OryxisColors::t().text_muted),
            )
            .center(Length::Fill)
            .into(),
            CloudDiscoverState::Failed(msg) => container(
                column![
                    text(format!("{}: {msg}", t("cloud_test_failed")))
                        .size(13)
                        .color(OryxisColors::t().error),
                    Space::new().height(12),
                    button(
                        container(
                            text(t("cloud_discover_refresh"))
                                .size(12)
                                .color(OryxisColors::t().text_primary),
                        )
                        .padding(Padding {
                            top: 6.0,
                            right: 12.0,
                            bottom: 6.0,
                            left: 12.0,
                        }),
                    )
                    .on_press(Message::CloudDiscoverRefresh)
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_surface)),
                        border: Border {
                            radius: Radius::from(6.0),
                            color: OryxisColors::t().border,
                            width: 1.0,
                        },
                        ..Default::default()
                    }),
                ]
                .align_x(iced::Alignment::Center),
            )
            .center(Length::Fill)
            .into(),
            CloudDiscoverState::Loaded(result) => self.view_discover_result_body(result),
        };

        // Footer: action buttons. Disabled / enabled depending on what
        // the current state allows. We re-render every frame so the
        // selection counter stays live.
        let import_count = self.cloud_discover_selected_ec2.len()
            + self.cloud_discover_selected_ecs.len();
        let can_import = matches!(
            self.cloud_discover_state,
            CloudDiscoverState::Loaded(_)
        ) && import_count > 0;

        let import_btn = {
            let label = if import_count == 0 {
                t("cloud_discover_import_none").to_string()
            } else {
                format!("{} {import_count}", t("cloud_discover_import_n"))
            };
            let mut b = button(
                container(
                    text(label)
                        .size(13)
                        .color(OryxisColors::t().text_primary),
                )
                .padding(Padding {
                    top: 10.0,
                    right: 0.0,
                    bottom: 10.0,
                    left: 0.0,
                })
                .width(Length::Fill)
                .center_x(Length::Fill),
            )
            .width(Length::Fill)
            .style(move |_, _| button::Style {
                background: Some(Background::Color(if can_import {
                    OryxisColors::t().accent
                } else {
                    OryxisColors::t().bg_surface
                })),
                border: Border {
                    radius: Radius::from(8.0),
                    ..Default::default()
                },
                ..Default::default()
            });
            if can_import {
                b = b.on_press(Message::CloudDiscoverImport);
            }
            b
        };

        // Refresh moved to a header icon. Transport pick was moved to
        // a confirmation modal that fires on click of the Import
        // button (only when EC2 hosts are selected; ECS-only imports
        // skip the modal). Footer is just the Import button.
        let footer = column![import_btn];

        let panel_content = column![
            title,
            search,
            container(body).height(Length::Fill).padding(Padding {
                top: 0.0,
                right: 20.0,
                bottom: 8.0,
                left: 20.0,
            }),
            container(footer).padding(Padding {
                top: 0.0,
                right: 20.0,
                bottom: 20.0,
                left: 20.0,
            }),
        ]
        .height(Length::Fill);

        container(panel_content)
            .width(PANEL_WIDTH)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                border: Border {
                    color: OryxisColors::t().border,
                    width: 1.0,
                    radius: Radius::from(0.0),
                },
                ..Default::default()
            })
            .into()
    }

    /// Render the EC2 (and future ECS / K8s) section of the loaded
    /// discovery result. Already-imported instances are shown but
    /// disabled, the user can tell what's new at a glance.
    fn view_discover_result_body(
        &self,
        result: &oryxis_cloud::DiscoveryResult,
    ) -> Element<'_, Message> {
        if result.ec2.is_empty() && result.ecs_services.is_empty() {
            return container(
                text(t("cloud_discover_no_results"))
                    .size(13)
                    .color(OryxisColors::t().text_muted),
            )
            .center(Length::Fill)
            .into();
        }

        // Index of currently-imported (profile, instance_id) pairs so
        // we can grey out duplicates instead of letting the user
        // re-import them.
        let already: std::collections::HashSet<String> = self
            .connections
            .iter()
            .filter_map(|c| {
                let cr = c.cloud_ref.as_ref()?;
                if Some(cr.profile_id) == self.cloud_discover_profile_id {
                    Some(cr.resource_id.clone())
                } else {
                    None
                }
            })
            .collect();

        // Apply the live filter, case-insensitive substring match
        // across name, instance-id, region, public/private DNS+IP.
        // The total count above the section reflects unfiltered size
        // so the user sees how much got hidden vs. the raw discovery
        // total.
        let needle = self.cloud_discover_filter.trim().to_lowercase();
        let matches_filter = |e: &oryxis_cloud::DiscoveredEc2| -> bool {
            if needle.is_empty() {
                return true;
            }
            let mut hay = String::new();
            if let Some(n) = &e.name { hay.push_str(n); hay.push(' '); }
            hay.push_str(&e.instance_id);
            hay.push(' ');
            hay.push_str(&e.region);
            for v in [&e.public_dns, &e.public_ip, &e.private_dns, &e.private_ip]
                .iter()
                .copied()
                .flatten()
            {
                hay.push(' ');
                hay.push_str(v);
            }
            hay.to_lowercase().contains(&needle)
        };

        // Group EC2 by region so the user sees the cloud's natural
        // boundary instead of an undifferentiated flat list.
        let mut by_region: std::collections::BTreeMap<String, Vec<&oryxis_cloud::DiscoveredEc2>> =
            std::collections::BTreeMap::new();
        let mut filtered_count = 0usize;
        for e in &result.ec2 {
            if matches_filter(e) {
                by_region.entry(e.region.clone()).or_default().push(e);
                filtered_count += 1;
            }
        }

        let mut sections: Vec<Element<'_, Message>> = Vec::new();
        // Hide the EC2 section entirely when zero entries match
        // showing an empty header reads as broken / loading. Same
        // policy applies to ECS below. The "no matches" hint at the
        // very bottom catches the case where every section is empty.
        let ec2_collapsed = self.cloud_discover_collapsed.contains("ec2");
        let show_ec2_section = filtered_count > 0;
        if show_ec2_section {
            let header_text = if needle.is_empty() {
                format!("EC2 ({})", result.ec2.len())
            } else {
                format!("EC2 ({} / {})", filtered_count, result.ec2.len())
            };
            sections.push(section_header("ec2", &header_text, ec2_collapsed));
            sections.push(Space::new().height(6).into());
        }

        if !show_ec2_section || ec2_collapsed {
            // Skip rendering EC2 rows entirely, header alone wraps
            // the section, the rest of the panel reflows to give the
            // collapsed state real space-saving value.
        } else {
        for (region, items) in by_region {
            sections.push(
                text(format!("📍 {region}"))
                    .size(11)
                    .color(OryxisColors::t().text_muted)
                    .into(),
            );
            sections.push(Space::new().height(4).into());
            for e in items {
                let is_imported = already.contains(&e.instance_id);
                let checked = self.cloud_discover_selected_ec2.contains(&e.instance_id);
                let id_for_msg = e.instance_id.clone();
                let label_text = match (&e.name, e.public_dns.as_deref().or(e.public_ip.as_deref()))
                {
                    (Some(name), Some(addr)) => format!("{name}  ({})  {addr}", e.instance_id),
                    (Some(name), None) => format!("{name}  ({})", e.instance_id),
                    (None, Some(addr)) => format!("{}  {addr}", e.instance_id),
                    (None, None) => e.instance_id.clone(),
                };
                let label_text = if is_imported {
                    format!("{label_text}  ·  {}", t("cloud_discover_already_imported"))
                } else {
                    label_text
                };
                let row_el: Element<'_, Message> = if is_imported {
                    text(label_text)
                        .size(11)
                        .color(OryxisColors::t().text_muted)
                        .into()
                } else {
                    let mark = if checked {
                        iced_fonts::lucide::circle_check()
                            .size(13)
                            .color(OryxisColors::t().accent)
                    } else {
                        iced_fonts::lucide::circle_minus()
                            .size(13)
                            .color(OryxisColors::t().text_muted)
                    };
                    button(
                        row![
                            mark,
                            Space::new().width(8),
                            text(label_text)
                                .size(11)
                                .color(OryxisColors::t().text_secondary),
                        ]
                        .align_y(iced::Alignment::Center),
                    )
                    .on_press(Message::CloudDiscoverToggleEc2(id_for_msg))
                    .padding(Padding {
                        top: 3.0,
                        right: 6.0,
                        bottom: 3.0,
                        left: 4.0,
                    })
                    .style(|_, status| {
                        let bg = match status {
                            BtnStatus::Hovered => OryxisColors::t().bg_hover,
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
                    .into()
                };
                sections.push(row_el);
                sections.push(Space::new().height(2).into());
            }
            sections.push(Space::new().height(8).into());
        }
        } // end `if !ec2_collapsed` block

        // ── ECS section ──
        // ECS services are imported as *dynamic groups* (one per
        // service+container) rather than individual hosts, since
        // task IDs are ephemeral. Already-imported services greyed
        // out so the user doesn't dupe them.
        let already_ecs: std::collections::HashSet<String> = self
            .groups
            .iter()
            .filter_map(|g| {
                let q = g.cloud_query.as_ref()?;
                if q.profile_id != self.cloud_discover_profile_id? {
                    return None;
                }
                match &q.kind {
                    oryxis_core::models::cloud::CloudQueryKind::EcsTasks {
                        cluster,
                        service,
                        container,
                    } => Some(format!("{cluster}/{service}/{container}")),
                    _ => None,
                }
            })
            .collect();

        let ecs_match_filter = |s: &oryxis_cloud::DiscoveredEcsService| -> bool {
            if needle.is_empty() {
                return true;
            }
            let hay = format!(
                "{} {} {} {}",
                s.cluster, s.service, s.container, s.region
            )
            .to_lowercase();
            hay.contains(&needle)
        };

        let ecs_filtered: Vec<&oryxis_cloud::DiscoveredEcsService> = result
            .ecs_services
            .iter()
            .filter(|s| ecs_match_filter(s))
            .collect();

        // Same auto-hide policy as EC2: only emit the ECS section if
        // there's at least one entry surviving the filter.
        if !ecs_filtered.is_empty() {
            sections.push(Space::new().height(8).into());
            let ecs_header = if needle.is_empty() {
                format!("ECS Services ({})", result.ecs_services.len())
            } else {
                format!(
                    "ECS Services ({} / {})",
                    ecs_filtered.len(),
                    result.ecs_services.len()
                )
            };
            let ecs_collapsed = self.cloud_discover_collapsed.contains("ecs");
            sections.push(section_header("ecs", &ecs_header, ecs_collapsed));
            sections.push(Space::new().height(6).into());

            if ecs_collapsed {
                // collapsed, skip body
            } else {

            // Group by region → cluster so the user reads
            // `📍 region / 🗂 cluster` then services. Tasks are
            // ephemeral; the import unit is the (service, container)
            // pair, which becomes a dynamic Group.
            let mut by_region_cluster: std::collections::BTreeMap<
                (String, String),
                Vec<&oryxis_cloud::DiscoveredEcsService>,
            > = std::collections::BTreeMap::new();
            for s in &ecs_filtered {
                by_region_cluster
                    .entry((s.region.clone(), s.cluster.clone()))
                    .or_default()
                    .push(s);
            }

            for ((region, cluster), items) in by_region_cluster {
                sections.push(
                    text(format!("📍 {region}  ·  {cluster}"))
                        .size(11)
                        .color(OryxisColors::t().text_muted)
                        .into(),
                );
                sections.push(Space::new().height(4).into());
                for s in items {
                    let key = format!("{}/{}/{}", s.cluster, s.service, s.container);
                    let is_imported = already_ecs.contains(&key);
                    let checked = self.cloud_discover_selected_ecs.contains(&key);
                    let label_text = format!(
                        "{} / {}  ·  {} task(s)",
                        s.service, s.container, s.running_task_count
                    );
                    let label_text = if is_imported {
                        format!("{label_text}  ·  {}", t("cloud_discover_already_imported"))
                    } else {
                        label_text
                    };
                    let row_el: Element<'_, Message> = if is_imported {
                        text(label_text)
                            .size(11)
                            .color(OryxisColors::t().text_muted)
                            .into()
                    } else {
                        let mark = if checked {
                            iced_fonts::lucide::circle_check()
                                .size(13)
                                .color(OryxisColors::t().accent)
                        } else {
                            iced_fonts::lucide::circle_minus()
                                .size(13)
                                .color(OryxisColors::t().text_muted)
                        };
                        let key_for_msg = key.clone();
                        button(
                            row![
                                mark,
                                Space::new().width(8),
                                text(label_text)
                                    .size(11)
                                    .color(OryxisColors::t().text_secondary),
                            ]
                            .align_y(iced::Alignment::Center),
                        )
                        .on_press(Message::CloudDiscoverToggleEcs(key_for_msg))
                        .padding(Padding {
                            top: 3.0,
                            right: 6.0,
                            bottom: 3.0,
                            left: 4.0,
                        })
                        .style(|_, status| {
                            let bg = match status {
                                BtnStatus::Hovered => OryxisColors::t().bg_hover,
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
                        .into()
                    };
                    sections.push(row_el);
                    sections.push(Space::new().height(2).into());
                }
                sections.push(Space::new().height(8).into());
            }
            } // end `if !ecs_collapsed` block
        }

        // Both sections hid themselves under the active filter, show
        // a friendly hint instead of an empty scroll area so the
        // panel doesn't read as "broken".
        if !show_ec2_section && ecs_filtered.is_empty() && !needle.is_empty() {
            sections.push(
                container(
                    text(t("cloud_discover_no_matches"))
                        .size(12)
                        .color(OryxisColors::t().text_muted),
                )
                .center_x(Length::Fill)
                .padding(Padding {
                    top: 24.0,
                    right: 0.0,
                    bottom: 0.0,
                    left: 0.0,
                })
                .into(),
            );
        }

        scrollable(column(sections)).height(Length::Fill).into()
    }
}
