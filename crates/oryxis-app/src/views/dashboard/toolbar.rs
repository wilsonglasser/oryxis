//! Dashboard toolbar, the breadcrumb on the left, and the trailing
//! action button (`+ host` for manual folders, `⬇ Discover` for
//! cloud-linked ones, nothing for dynamic groups).

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, container, text, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis};
use crate::i18n::t;
use crate::theme::OryxisColors;
use crate::widgets::dir_row;

impl Oryxis {
    pub(super) fn dashboard_toolbar(&self) -> Element<'_, Message> {
        // ── Toolbar ──
        let toolbar_left: Element<'_, Message> = if let Some(gid) = self.active_group {
            // Build the parent → child breadcrumb chain so a deeply
            // nested view (root → prod-aws → tbl-sis-web ECS) shows
            // both ancestors. Walk parent_id pointers up; cap at 5
            // levels to keep the layout sane and break any cycles
            // legacy data could carry.
            let mut chain: Vec<&oryxis_core::models::group::Group> = Vec::new();
            let mut cursor = Some(gid);
            for _ in 0..5 {
                let Some(id) = cursor else { break };
                let Some(g) = self.groups.iter().find(|g| g.id == id) else { break };
                chain.push(g);
                cursor = g.parent_id;
            }
            chain.reverse();

            let mut crumbs: Vec<Element<'_, Message>> = vec![
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
            ];
            for (idx, g) in chain.iter().enumerate() {
                let is_last = idx == chain.len() - 1;
                crumbs.push(text("/").size(16).color(OryxisColors::t().text_muted).into());
                crumbs.push(Space::new().width(8).into());
                crumbs.push(
                    iced_fonts::lucide::folder().size(16).color(OryxisColors::t().accent).into(),
                );
                crumbs.push(Space::new().width(6).into());
                if is_last {
                    // Current group, plain text, no nav action.
                    crumbs.push(
                        text(g.label.clone())
                            .size(16)
                            .color(OryxisColors::t().text_primary)
                            .into(),
                    );
                } else {
                    // Ancestor, clickable: navigates back up.
                    let parent_id = g.id;
                    crumbs.push(
                        button(
                            text(g.label.clone())
                                .size(16)
                                .color(OryxisColors::t().accent),
                        )
                        .on_press(Message::OpenGroup(parent_id))
                        .padding(Padding {
                            top: 0.0,
                            right: 4.0,
                            bottom: 0.0,
                            left: 4.0,
                        })
                        .style(|_, _| button::Style {
                            background: Some(Background::Color(Color::TRANSPARENT)),
                            border: Border::default(),
                            ..Default::default()
                        })
                        .into(),
                    );
                }
            }
            dir_row(crumbs).align_y(iced::Alignment::Center).into()
        } else {
            text(t("hosts")).size(20).color(OryxisColors::t().text_primary).into()
        };

        // "+ Host [▾]" split button, primary half opens the manual
        // SSH editor (unchanged), the chevron half opens a cloud
        // provider picker overlay so discovery launches from the
        // Hosts view (where the user naturally goes to add hosts).
        // Layout mirrors the keychain "+ ADD ▼" split exactly so both
        // toolbars stay visually consistent. The chevron half is only
        // emitted when at least one cloud profile is configured
        // when there's no chevron, the primary button takes back its
        // full corner radius so it doesn't look "cut" on the right.
        let has_chevron = !self.cloud_profiles.is_empty();
        let rtl = crate::i18n::is_rtl_layout();
        // Pre-compute the rounded-corner radii so the leading half
        // always rounds the leading edge and the chevron always
        // rounds the trailing edge, flipped under RTL.
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
            // 1px divider between the two halves, same alpha-tinted
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

        // Context-aware toolbar action: inside a dynamic group there
        // is no "+ host", tasks come from the cloud resolver. Inside
        // a provider folder (= a manual folder linked to a cloud
        // profile via its children's `cloud_ref`/`cloud_query`),
        // "+ HOST" turns into "+ DISCOVER" so the user lands directly
        // in the right import flow.
        let resolved_action: Element<'_, Message> = if let Some(gid) = self.active_group {
            // Is this a dynamic group?
            let dynamic_query_profile = self
                .groups
                .iter()
                .find(|g| g.id == gid)
                .and_then(|g| g.cloud_query.as_ref())
                .map(|q| q.profile_id);
            if dynamic_query_profile.is_some() {
                // Dynamic group → no "+ host" button. The Refresh
                // icon already lives in the sub-header.
                Space::new().width(0).into()
            } else {
                // Manual folder: derive the linked profile from any
                // child host's cloud_ref or any child dynamic group's
                // cloud_query.
                let linked_profile = self
                    .connections
                    .iter()
                    .filter(|c| c.group_id == Some(gid))
                    .find_map(|c| c.cloud_ref.as_ref().map(|r| r.profile_id))
                    .or_else(|| {
                        self.groups
                            .iter()
                            .filter(|g| g.parent_id == Some(gid))
                            .find_map(|g| g.cloud_query.as_ref().map(|q| q.profile_id))
                    });
                match linked_profile {
                    Some(pid) => {
                        let fg = OryxisColors::t().button_text;
                        button(
                            container(
                                dir_row(vec![
                                    iced_fonts::lucide::download()
                                        .size(13)
                                        .color(fg)
                                        .into(),
                                    Space::new().width(4).into(),
                                    text(t("cloud_discover"))
                                        .size(11)
                                        .font(iced::Font {
                                            weight: iced::font::Weight::Bold,
                                            ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                                        })
                                        .color(fg)
                                        .into(),
                                ])
                                .align_y(iced::Alignment::Center),
                            )
                            .center_y(Length::Fixed(24.0))
                            .padding(Padding {
                                top: 0.0,
                                right: 14.0,
                                bottom: 0.0,
                                left: 14.0,
                            }),
                        )
                        .on_press(Message::ShowCloudDiscover(pid))
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
                        })
                        .into()
                    }
                    None => action_group,
                }
            }
        } else {
            action_group
        };

        let toolbar = container(
            dir_row(vec![
                toolbar_left,
                Space::new().width(Length::Fill).into(),
                resolved_action,
            ]).align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 24.0, bottom: 16.0, left: 24.0 })
        .width(Length::Fill);
        toolbar.into()
    }
}
