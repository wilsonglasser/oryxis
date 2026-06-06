//! New-tab picker, centered modal overlay with a search bar and a
//! drill-down list: top level shows groups (folders) + the recent
//! connections, and clicking a group drills into it. Manual groups reveal
//! their sub-groups and member connections; cloud-query groups (ECS / K8s)
//! resolve their live resources on enter so the user can pick a task / pod
//! to exec into. Triggered from the `+` button in the tab bar, or from a
//! pane split (which targets an SSH-only pane, so cloud groups are hidden).
//!
//! Visually modeled on Termius' "New Tab" screen: big rounded search at the
//! top, then a grouped list with host-icon badges and a "Personal / Group"
//! breadcrumb on the right.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, scrollable, text, text_input, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use oryxis_core::models::Group;

use crate::app::{Message, Oryxis};
use crate::i18n::t;
use crate::state::DynamicGroupState;
use crate::theme::OryxisColors;
use crate::widgets::{dir_align_x, dir_row};

impl Oryxis {
    /// Build the new-tab picker modal. The caller is responsible for checking
    /// `self.show_new_tab_picker` before rendering and stacking it on top of
    /// the base view.
    pub(crate) fn view_new_tab_picker(&self) -> Element<'_, Message> {
        // Internal right-padding leaves room for the floating "Ctrl+K"
        // affordance so the typed value never slides under the hint.
        let search = text_input(t("search_hosts_or_tabs"), &self.new_tab_picker_search)
            .on_input(Message::NewTabPickerSearchChanged)
            .padding(Padding {
                top: 14.0,
                right: 64.0,
                bottom: 14.0,
                left: 14.0,
            })
            .size(14)
            .style(crate::widgets::rounded_input_style).align_x(dir_align_x());

        // Right-anchored "Ctrl+K" hint inside a styled chip so it reads
        // as a keyboard affordance rather than placeholder text. Lives
        // in a Stack on top of the input, `text` has no click handler,
        // so focus-on-click still works on the wider left portion.
        let ctrl_k_chip = container(
            text("Ctrl+K").size(11).color(OryxisColors::t().text_muted),
        )
        .padding(Padding {
            top: 2.0,
            right: 6.0,
            bottom: 2.0,
            left: 6.0,
        })
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_hover)),
            border: Border {
                radius: Radius::from(4.0),
                ..Default::default()
            },
            ..Default::default()
        });
        let ctrl_k_overlay = container(ctrl_k_chip)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Right)
            .align_y(iced::alignment::Vertical::Center)
            .padding(Padding {
                top: 0.0,
                right: 12.0,
                bottom: 0.0,
                left: 0.0,
            });

        let search_block = iced::widget::Stack::new()
            .push(search)
            .push(ctrl_k_overlay)
            .width(Length::Fill);

        // Split panes are SSH-only (ECS Exec / kubectl exec open full tabs),
        // so when the picker is filling a pane we hide every cloud-query
        // group and never surface a path to an ECS task / K8s pod.
        let filling_pane = self.pending_pane_split.is_some();
        let needle = self.new_tab_picker_search.to_lowercase();

        // Resolve the drilled-into group (if any) up front so the body
        // builder and the back-header agree on the level being shown.
        let drilled = self
            .new_tab_picker_group
            .and_then(|gid| self.groups.iter().find(|g| g.id == gid));

        let list_inner: Vec<Element<'_, Message>> = match drilled {
            Some(group) => self.picker_group_rows(group, &needle, filling_pane),
            None => self.picker_top_level_rows(&needle, filling_pane),
        };

        let list_panel = container(column(list_inner).spacing(2))
            .padding(Padding { top: 14.0, right: 16.0, bottom: 14.0, left: 16.0 })
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    radius: Radius::from(10.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            });

        let list_scroll = scrollable(list_panel).height(Length::Fill);

        let body = container(
            column![
                search_block,
                Space::new().height(16),
                list_scroll,
            ],
        )
        .padding(24)
        .width(Length::Fixed(780.0))
        .height(Length::Fixed(640.0))
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_primary)),
            border: Border {
                radius: Radius::from(12.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        });

        // Bare card; `widgets::modal_overlay` (the caller) owns centering,
        // the absorbing scrim, and the click-trap.
        body.into()
    }

    /// Top-level rows: a "Groups" section (root groups as drillable
    /// folders) followed by the flat "Recent connections" list. Cloud-query
    /// groups are hidden while filling a pane.
    fn picker_top_level_rows(
        &self,
        needle: &str,
        filling_pane: bool,
    ) -> Vec<Element<'_, Message>> {
        let mut rows: Vec<Element<'_, Message>> = Vec::new();

        // Local shell, always first. Routes into the pending pane (split)
        // or a fresh tab, handled by `Message::PickLocalShell`.
        if needle.is_empty() || t("local_shell").to_lowercase().contains(needle) {
            rows.push(local_shell_row());
            rows.push(Space::new().height(14).into());
        }

        // Root groups (parent_id == None). Sub-groups surface when the user
        // drills into their parent, mirroring the dashboard hierarchy.
        let mut group_rows: Vec<Element<'_, Message>> = Vec::new();
        for g in self.groups.iter().filter(|g| g.parent_id.is_none()) {
            if filling_pane && g.cloud_query.is_some() {
                continue;
            }
            // Hide empty manual folders (no hosts, no sub-groups): there's
            // nothing to open inside, so they'd just be dead rows. Mirrors
            // the dashboard, which only renders a root folder with a direct
            // connection or a sub-group. Cloud-query groups always show:
            // they resolve their hosts dynamically and carry an ECS/K8S tag
            // instead of a count.
            if g.cloud_query.is_none() && self.picker_group_child_count(g.id) == 0 {
                continue;
            }
            if !needle.is_empty() && !g.label.to_lowercase().contains(needle) {
                continue;
            }
            group_rows.push(self.picker_group_row(g));
        }
        if !group_rows.is_empty() {
            rows.push(section_header(t("groups_section")));
            rows.push(Space::new().height(8).into());
            rows.extend(group_rows);
            rows.push(Space::new().height(14).into());
        }

        // Recent connections: every saved host, most-recently-used first.
        let mut idxs: Vec<usize> = (0..self.connections.len())
            .filter(|&i| {
                if needle.is_empty() {
                    return true;
                }
                let c = &self.connections[i];
                c.label.to_lowercase().contains(needle)
                    || c.hostname.to_lowercase().contains(needle)
            })
            .collect();
        idxs.sort_by(|a, b| {
            let la = self.connections[*a].last_used;
            let lb = self.connections[*b].last_used;
            lb.cmp(&la)
        });

        rows.push(section_header(t("recent_connections")));
        rows.push(Space::new().height(8).into());
        if idxs.is_empty() {
            rows.push(info_row(if needle.is_empty() {
                t("no_connections_yet")
            } else {
                t("no_matches")
            }));
        } else {
            for (pos, ci) in idxs.iter().enumerate() {
                rows.push(self.connection_row(*ci, pos));
            }
        }
        rows
    }

    /// Rows for a drilled-into group: a back header, then either the
    /// group's sub-groups + member connections (manual group) or its
    /// resolved cloud resources (ECS tasks / K8s pods).
    fn picker_group_rows<'a>(
        &'a self,
        group: &'a Group,
        needle: &str,
        filling_pane: bool,
    ) -> Vec<Element<'a, Message>> {
        let mut rows: Vec<Element<'a, Message>> = vec![back_header(&group.label)];
        rows.push(Space::new().height(8).into());

        if group.cloud_query.is_some() {
            rows.extend(self.picker_cloud_resource_rows(group, needle));
            return rows;
        }

        // Manual group: sub-groups first, then member connections.
        let mut any = false;
        for g in self.groups.iter().filter(|g| g.parent_id == Some(group.id)) {
            if filling_pane && g.cloud_query.is_some() {
                continue;
            }
            // Same empty-folder hiding as the top level (see there).
            if g.cloud_query.is_none() && self.picker_group_child_count(g.id) == 0 {
                continue;
            }
            if !needle.is_empty() && !g.label.to_lowercase().contains(needle) {
                continue;
            }
            rows.push(self.picker_group_row(g));
            any = true;
        }
        let mut member_idxs: Vec<usize> = (0..self.connections.len())
            .filter(|&i| self.connections[i].group_id == Some(group.id))
            .filter(|&i| {
                if needle.is_empty() {
                    return true;
                }
                let c = &self.connections[i];
                c.label.to_lowercase().contains(needle)
                    || c.hostname.to_lowercase().contains(needle)
            })
            .collect();
        member_idxs.sort_by(|a, b| {
            self.connections[*b].last_used.cmp(&self.connections[*a].last_used)
        });
        for (pos, ci) in member_idxs.iter().enumerate() {
            rows.push(self.connection_row(*ci, pos));
            any = true;
        }
        if !any {
            rows.push(info_row(if needle.is_empty() {
                t("no_connections_yet")
            } else {
                t("no_matches")
            }));
        }
        rows
    }

    /// Rows for a cloud-query group, driven off the per-group resolve cache.
    /// Renders all four `DynamicGroupState` cases (incl. the not-yet-resolved
    /// `None`) so the picker never looks dead mid-fetch, plus a retry button
    /// on failure.
    fn picker_cloud_resource_rows<'a>(
        &'a self,
        group: &'a Group,
        needle: &str,
    ) -> Vec<Element<'a, Message>> {
        let gid = group.id;
        // Per-group connect coordinates, derived once. ECS rows carry the
        // query container as a fallback; K8s rows carry the namespace and
        // route through `kubectl exec` instead of the ECS Exec transport.
        let (ecs_container, k8s_namespace) = match group.cloud_query.as_ref().map(|q| &q.kind) {
            Some(oryxis_core::models::cloud::CloudQueryKind::EcsTasks { container, .. }) => {
                (container.clone(), None)
            }
            Some(oryxis_core::models::cloud::CloudQueryKind::K8sPods { namespace, .. }) => {
                (String::new(), Some(namespace.clone()))
            }
            None => (String::new(), None),
        };

        match self.cloud_dynamic_group_state.get(&gid) {
            None => vec![info_row(t("cloud_dynamic_group_pending"))],
            Some(DynamicGroupState::Loading) => vec![info_row(t("cloud_discover_running"))],
            Some(DynamicGroupState::Failed(msg)) => vec![
                info_row(&format!("{}: {msg}", t("cloud_test_failed"))),
                Space::new().height(8).into(),
                retry_row(gid),
            ],
            Some(DynamicGroupState::Loaded { hosts, .. }) => {
                let mut rows: Vec<Element<'a, Message>> = Vec::new();
                for h in hosts {
                    // Primary label: container name when set (ECS task with
                    // N containers), else the bare resource id.
                    let primary = match &h.container_name {
                        Some(name) if !name.is_empty() => name.clone(),
                        _ => h.resource_id.clone(),
                    };
                    if !needle.is_empty()
                        && !primary.to_lowercase().contains(needle)
                        && !h.resource_id.to_lowercase().contains(needle)
                    {
                        continue;
                    }
                    let task_id = h.resource_id.clone();
                    let task_label = h.label.clone();
                    let status_upper: Option<String> =
                        h.status.as_deref().map(|s| s.to_ascii_uppercase());
                    // Only RUNNING (or unknown) tasks can be exec'd into; a
                    // PENDING / STOPPED one yields an opaque error on click.
                    let connectable =
                        matches!(status_upper.as_deref(), Some("RUNNING") | None);
                    // Connect message construction mirrors the dashboard grid
                    // verbatim (the container fallback is subtle): the per-row
                    // container under a wildcard query, else the query's.
                    let msg = match &k8s_namespace {
                        Some(ns) => Message::ConnectKubectlExecPod {
                            group_id: gid,
                            namespace: ns.clone(),
                            pod: task_id.clone(),
                            container: h.container_name.clone().unwrap_or_default(),
                        },
                        None => Message::ConnectEcsExecTask {
                            group_id: gid,
                            task_id: task_id.clone(),
                            task_label,
                            container: h
                                .container_name
                                .clone()
                                .unwrap_or_else(|| ecs_container.clone()),
                        },
                    };
                    // Secondary line shows the task / pod id, but only when
                    // the primary label isn't already that id (bare resources
                    // would otherwise print it twice).
                    let secondary = if primary == h.resource_id {
                        String::new()
                    } else {
                        h.resource_id.clone()
                    };
                    rows.push(resource_row(
                        msg,
                        primary,
                        secondary,
                        status_upper.as_deref(),
                        connectable,
                    ));
                }
                if rows.is_empty() {
                    rows.push(info_row(if needle.is_empty() {
                        t("cloud_dynamic_group_no_tasks")
                    } else {
                        t("no_matches")
                    }));
                }
                rows
            }
        }
    }

    /// Direct child count of a manual folder: its own connections plus its
    /// immediate sub-groups. Drives both the trailing count badge and the
    /// empty-folder hiding, so the number shown always matches whether the
    /// folder is shown at all (count 0 -> hidden).
    fn picker_group_child_count(&self, gid: uuid::Uuid) -> usize {
        let conns = self
            .connections
            .iter()
            .filter(|c| c.group_id == Some(gid))
            .count();
        let subs = self
            .groups
            .iter()
            .filter(|g| g.parent_id == Some(gid))
            .count();
        conns + subs
    }

    /// A drillable folder row for `group`, emitting `NewTabPickerOpenGroup`.
    /// Cloud-query groups get a cloud glyph + a kind tag (ECS / K8S); manual
    /// groups get a folder glyph + a child count.
    fn picker_group_row<'a>(&self, group: &'a Group) -> Element<'a, Message> {
        let is_cloud = group.cloud_query.is_some();
        let glyph: Element<'a, Message> = if is_cloud {
            iced_fonts::lucide::cloud()
                .size(15)
                .color(OryxisColors::t().accent)
                .into()
        } else {
            iced_fonts::lucide::folder()
                .size(15)
                .color(OryxisColors::t().accent)
                .into()
        };

        let subtitle = match group.cloud_query.as_ref().map(|q| &q.kind) {
            Some(oryxis_core::models::cloud::CloudQueryKind::EcsTasks { .. }) => "ECS".to_string(),
            Some(oryxis_core::models::cloud::CloudQueryKind::K8sPods { .. }) => "K8S".to_string(),
            None => self.picker_group_child_count(group.id).to_string(),
        };

        // Trailing chevron points into the group; mirror it under RTL.
        let chevron: Element<'a, Message> = if crate::i18n::is_rtl_layout() {
            iced_fonts::lucide::chevron_left()
        } else {
            iced_fonts::lucide::chevron_right()
        }
        .size(15)
        .color(OryxisColors::t().text_muted)
        .into();

        let inner = dir_row(vec![
            glyph,
            Space::new().width(12).into(),
            text(group.label.clone())
                .size(13)
                .font(iced::Font {
                    weight: iced::font::Weight::Semibold,
                    ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                })
                .color(OryxisColors::t().text_primary)
                .into(),
            Space::new().width(Length::Fill).into(),
            text(subtitle).size(12).color(OryxisColors::t().text_muted).into(),
            Space::new().width(10).into(),
            chevron,
        ])
        .align_y(iced::Alignment::Center);

        button(
            container(inner)
                .padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 })
                .width(Length::Fill),
        )
        .on_press(Message::NewTabPickerOpenGroup(group.id))
        .width(Length::Fill)
        .style(hover_row_style)
        .into()
    }

    /// A saved-connection row (mirrors the host card badge + breadcrumb),
    /// emitting `ConnectSsh`. `pos` drives the zebra stripe.
    fn connection_row(&self, ci: usize, pos: usize) -> Element<'_, Message> {
        let conn = &self.connections[ci];
        let group_name = conn.group_id.and_then(|gid| {
            self.groups.iter().find(|g| g.id == gid).map(|g| g.label.clone())
        });
        let breadcrumb = match group_name {
            Some(g) => format!("{} / {}", t("personal"), g),
            None => t("personal").to_string(),
        };
        let zebra_bg = if pos % 2 == 1 {
            OryxisColors::t().bg_hover
        } else {
            Color::TRANSPARENT
        };
        let badge_style = crate::widgets::resolve_host_icon_style(
            conn.icon_style.as_deref(),
            &self.setting_default_host_icon,
        );
        let (glyph, default_color) = crate::os_icon::resolve_icon(
            conn.detected_os.as_deref(),
            OryxisColors::t().accent,
        );
        let badge_color = conn
            .custom_color
            .as_deref()
            .or(conn.color.as_deref())
            .and_then(crate::widgets::parse_hex_color)
            .unwrap_or(default_color);
        let glyph_el: Element<'_, Message> = glyph.view(12.0, Color::WHITE);
        let badge = crate::widgets::host_icon(badge_style, badge_color, &conn.label, Some(glyph_el), 26.0);
        picker_row(ci, &conn.label, breadcrumb, zebra_bg, badge)
    }
}

/// "Local Shell" entry, emitting `PickLocalShell` (fills the pending split
/// pane, or opens a local shell in a new tab).
fn local_shell_row<'a>() -> Element<'a, Message> {
    let inner = dir_row(vec![
        iced_fonts::lucide::terminal()
            .size(15)
            .color(OryxisColors::t().accent)
            .into(),
        Space::new().width(12).into(),
        text(t("local_shell"))
            .size(13)
            .font(iced::Font {
                weight: iced::font::Weight::Semibold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            })
            .color(OryxisColors::t().text_primary)
            .into(),
    ])
    .align_y(iced::Alignment::Center);
    button(
        container(inner)
            .padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 })
            .width(Length::Fill),
    )
    .on_press(Message::PickLocalShell)
    .width(Length::Fill)
    .style(hover_row_style)
    .into()
}

/// Bold section label ("Groups", "Recent connections").
fn section_header<'a>(label: &str) -> Element<'a, Message> {
    dir_row(vec![
        text(label.to_string())
            .size(13)
            .font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            })
            .color(OryxisColors::t().text_primary)
            .into(),
        Space::new().width(Length::Fill).into(),
    ])
    .align_y(iced::Alignment::Center)
    .into()
}

/// Back-navigation header shown when drilled into a group. The leading
/// arrow + the group label form one click target returning to the top.
fn back_header<'a>(label: &str) -> Element<'a, Message> {
    let arrow: Element<'a, Message> = iced_fonts::lucide::arrow_left()
        .size(16)
        .color(OryxisColors::t().text_primary)
        .into();
    let inner = dir_row(vec![
        arrow,
        Space::new().width(10).into(),
        text(label.to_string())
            .size(14)
            .font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            })
            .color(OryxisColors::t().text_primary)
            .into(),
    ])
    .align_y(iced::Alignment::Center);
    button(
        container(inner)
            .padding(Padding { top: 6.0, right: 8.0, bottom: 6.0, left: 4.0 })
            .width(Length::Fill),
    )
    .on_press(Message::NewTabPickerBack)
    .width(Length::Fill)
    .style(hover_row_style)
    .into()
}

/// Muted, centered informational row (empty / loading / error states).
fn info_row<'a>(msg: &str) -> Element<'a, Message> {
    container(text(msg.to_string()).size(13).color(OryxisColors::t().text_muted))
        .padding(Padding { top: 18.0, right: 16.0, bottom: 18.0, left: 16.0 })
        .center_x(Length::Fill)
        .into()
}

/// Retry button for a failed cloud resolve. Dispatches `DynamicGroupResolve`
/// directly (the TTL gate would refuse to restart a `Failed` cache).
fn retry_row<'a>(gid: uuid::Uuid) -> Element<'a, Message> {
    let inner = dir_row(vec![
        iced_fonts::lucide::refresh_cw()
            .size(13)
            .color(OryxisColors::t().text_primary)
            .into(),
        Space::new().width(8).into(),
        text(t("cloud_discover_refresh"))
            .size(13)
            .color(OryxisColors::t().text_primary)
            .into(),
    ])
    .align_y(iced::Alignment::Center);
    container(
        button(container(inner).padding(Padding { top: 8.0, right: 14.0, bottom: 8.0, left: 14.0 }))
            .on_press(Message::DynamicGroupResolve(gid))
            .style(hover_row_style),
    )
    .center_x(Length::Fill)
    .into()
}

/// A live cloud-resource row (ECS task / K8s pod). `on_press` is omitted
/// when the task isn't connectable, which renders the row inert + muted.
fn resource_row<'a>(
    msg: Message,
    primary: String,
    secondary: String,
    status_upper: Option<&str>,
    connectable: bool,
) -> Element<'a, Message> {
    let primary_color = if connectable {
        OryxisColors::t().text_primary
    } else {
        OryxisColors::t().text_muted
    };
    let mut text_col: Vec<Element<'a, Message>> = vec![text(primary)
        .size(13)
        .color(primary_color)
        .wrapping(iced::widget::text::Wrapping::None)
        .into()];
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

    let status_pill: Element<'a, Message> = match status_upper {
        Some("RUNNING") => status_pill_widget("RUNNING", OryxisColors::t().success),
        Some("PENDING") | Some("PROVISIONING") => {
            status_pill_widget(status_upper.unwrap(), OryxisColors::t().warning)
        }
        Some("STOPPED") | Some("DEACTIVATING") => {
            status_pill_widget(status_upper.unwrap(), OryxisColors::t().error)
        }
        Some(other) => status_pill_widget(other, OryxisColors::t().text_muted),
        None => Space::new().width(0).into(),
    };

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
    .on_press_maybe(connectable.then_some(msg))
    .padding(Padding { top: 10.0, right: 12.0, bottom: 10.0, left: 12.0 })
    .width(Length::Fill)
    .style(|_, status| {
        let (bg, bc) = match status {
            BtnStatus::Hovered => (OryxisColors::t().bg_hover, OryxisColors::t().accent),
            BtnStatus::Pressed => (OryxisColors::t().bg_selected, OryxisColors::t().accent),
            BtnStatus::Disabled => (OryxisColors::t().bg_primary, OryxisColors::t().border),
            _ => (OryxisColors::t().bg_surface, OryxisColors::t().border),
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), color: bc, width: 1.0 },
            ..Default::default()
        }
    })
    .into()
}

/// Small colour-coded status chip (RUNNING / PENDING / ...).
fn status_pill_widget<'a>(label: &str, color: Color) -> Element<'a, Message> {
    container(text(label.to_string()).size(10).color(color))
        .padding(Padding { top: 2.0, right: 8.0, bottom: 2.0, left: 8.0 })
        .style(move |_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_hover)),
            border: Border { radius: Radius::from(4.0), color, width: 1.0 },
            ..Default::default()
        })
        .into()
}

/// Shared button style: transparent until hover, used by group / back /
/// retry rows that don't carry their own zebra stripe.
fn hover_row_style(_: &iced::Theme, status: BtnStatus) -> button::Style {
    let bg = match status {
        BtnStatus::Hovered => OryxisColors::t().bg_hover,
        _ => Color::TRANSPARENT,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        border: Border { radius: Radius::from(6.0), ..Default::default() },
        ..Default::default()
    }
}

fn picker_row<'a>(
    conn_idx: usize,
    label: &'a str,
    breadcrumb: String,
    zebra_bg: Color,
    badge: Element<'a, Message>,
) -> Element<'a, Message> {
    let label_text = text(label.to_string()).size(13).font(iced::Font {
        weight: iced::font::Weight::Semibold,
        ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
    }).color(OryxisColors::t().text_primary);

    let breadcrumb_text = text(breadcrumb).size(12).color(OryxisColors::t().accent);

    let inner = dir_row(vec![
        badge,
        Space::new().width(12).into(),
        label_text.into(),
        Space::new().width(Length::Fill).into(),
        breadcrumb_text.into(),
    ])
    .align_y(iced::Alignment::Center);

    button(
        container(inner)
            .padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 })
            .width(Length::Fill),
    )
    .on_press(Message::ConnectSsh(conn_idx))
    .width(Length::Fill)
    .style(move |_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().bg_hover,
            _ => zebra_bg,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    })
    .into()
}
