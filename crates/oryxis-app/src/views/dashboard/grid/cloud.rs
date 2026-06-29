//! Dashboard grid: cloud-query group view. Split out of views/dashboard/grid/mod.rs.

use super::*;
use iced::widget::column;
impl Oryxis {
    /// Live view for a cloud-query (dynamic) group: its resolved children
    /// or the resolver's pending / loading / failed state.
    pub(crate) fn dashboard_cloud_group_view<'a>(
        &'a self,
        gid: Uuid,
        query: &'a oryxis_core::models::CloudQuery,
    ) -> Element<'a, Message> {
        let toolbar = self.dashboard_toolbar();
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
                    // K8s namespace, set only for `K8sPods` groups so
                    // each pod row dispatches `kubectl exec` instead of
                    // the ECS Exec transport.
                    let k8s_namespace = match &query.kind {
                        oryxis_core::models::cloud::CloudQueryKind::K8sPods {
                            namespace,
                            ..
                        } => Some(namespace.clone()),
                        _ => None,
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

                        // Only RUNNING tasks can be exec'd into. A
                        // PROVISIONING container has no `runtimeId`
                        // yet and a STOPPED one is gone, so clicking
                        // either just yields an opaque AWS error.
                        // Disable the row (no on_press) and mute its
                        // label for any known non-RUNNING state.
                        // Unknown / absent status stays clickable so
                        // we never block a task we simply couldn't
                        // classify; the API gives a clear error then.
                        let connectable = matches!(
                            status_upper.as_deref(),
                            Some("RUNNING") | None
                        );
                        let primary_color = if connectable {
                            OryxisColors::t().text_primary
                        } else {
                            OryxisColors::t().text_muted
                        };

                        let mut text_col: Vec<Element<'_, Message>> = vec![
                            text(primary)
                                .size(13)
                                .color(primary_color)
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
                            .on_press_maybe(connectable.then_some(
                                match &k8s_namespace {
                                    // K8s pod row: open `kubectl exec`.
                                    Some(ns) => Message::ConnectKubectlExecPod {
                                        group_id: gid,
                                        namespace: ns.clone(),
                                        pod: task_id.clone(),
                                        container: h
                                            .container_name
                                            .clone()
                                            .unwrap_or_default(),
                                    },
                                    // ECS task row: SSM-backed Exec.
                                    None => Message::ConnectEcsExecTask {
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
                                    },
                                },
                            ))
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
                                    // Non-RUNNING rows arrive here
                                    // (no on_press). Flatten to the
                                    // page background so they read as
                                    // inert next to the live cards.
                                    BtnStatus::Disabled => (
                                        OryxisColors::t().bg_primary,
                                        OryxisColors::t().border,
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
        main_content.into()
    }
}
