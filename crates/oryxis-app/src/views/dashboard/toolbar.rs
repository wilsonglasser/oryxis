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

            // Leading crumb: just "Hosts" (no arrow, no "All"), styled
            // to match the homepage Hosts header so the breadcrumb feels
            // like a continuation of that title rather than a separate
            // nav element. Accent color marks it as clickable; routes
            // back to the root view.
            // Zero padding on every crumb button so the clickable
            // ancestors render at the same x footprint as the
            // unstyled-text current crumb. Gaps between crumbs come
            // from explicit `Space::new().width(...)` separators
            // below, not from button chrome.
            // No leading "Hosts"/home crumb: the top Home tab already
            // returns to the root host list (it clears the active group),
            // so a second home here was redundant. The breadcrumb starts
            // straight at the folder path.
            let mut crumbs: Vec<Element<'_, Message>> = Vec::new();
            for (idx, g) in chain.iter().enumerate() {
                let is_last = idx == chain.len() - 1;
                // Separator only between crumbs, not before the first.
                // Space on both sides so the "/" never glues to the
                // preceding crumb under RTL (`dir_row` reverses the
                // slice).
                if idx > 0 {
                    crumbs.push(Space::new().width(4).into());
                    crumbs.push(text("/").size(20).color(OryxisColors::t().text_muted).into());
                    crumbs.push(Space::new().width(8).into());
                }
                crumbs.push(
                    iced_fonts::lucide::folder().size(18).color(OryxisColors::t().accent).into(),
                );
                crumbs.push(Space::new().width(6).into());
                if is_last {
                    // Current group, plain text, no nav action.
                    crumbs.push(
                        text(g.label.clone())
                            .size(20)
                            .wrapping(iced::widget::text::Wrapping::None)
                            .color(OryxisColors::t().text_primary)
                            .into(),
                    );
                } else {
                    // Ancestor, clickable: navigates back up. Zero
                    // padding mirrors the leading "Hosts" button and
                    // the unstyled current-crumb text so the row's
                    // glyph baseline stays consistent across mixed
                    // clickable + non-clickable crumbs.
                    let parent_id = g.id;
                    crumbs.push(
                        button(
                            text(g.label.clone())
                                .size(20)
                                .wrapping(iced::widget::text::Wrapping::None)
                                .color(OryxisColors::t().accent),
                        )
                        .on_press(Message::OpenGroup(parent_id))
                        .padding(Padding::ZERO)
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
            // Title dropped (redundant with the section nav); the search
            // field fills this slot in the toolbar instead.
            Space::new().width(0).into()
        };

        // "+ Host [▾]" split button, primary half opens the manual
        // SSH editor (unchanged), the chevron half opens the add menu
        // overlay: import a `.oryxis` file (vault or shared host) plus
        // cloud discovery per configured profile. Launching from the
        // Hosts view keeps every "add a host" path in one place (the
        // user naturally goes here to add hosts). Layout mirrors the
        // keychain "+ ADD ▼" split exactly so both toolbars stay
        // visually consistent. The chevron is always emitted so import
        // stays reachable even before any cloud profile exists.
        let rtl = crate::i18n::is_rtl_layout();
        // Pre-compute the rounded-corner radii so the leading half
        // rounds the leading edge and the chevron rounds the trailing
        // edge, flipped under RTL.
        let label_radius = if rtl {
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
            .center_x(Length::Fixed(72.0)),
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

        // 1px divider between the two halves, same alpha-tinted black
        // the keychain split uses.
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
        let action_group: Element<'_, Message> =
            dir_row(vec![primary_btn.into(), separator.into(), chevron_btn.into()])
                .align_y(iced::Alignment::Center)
                .into();

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
                // Dynamic group → no "+ host" button. Reserve the
                // same vertical slot the visible button would occupy
                // so the breadcrumb row keeps its height. Iced's
                // button widget adds its own DEFAULT_PADDING (5 top
                // + 5 bottom) on top of the inner container's
                // `center_y(Length::Fixed(24.0))`, so the rendered
                // button is 24 + 10 = 34 px tall. Anchoring the
                // slot to 34 keeps the breadcrumb glyph baseline at
                // the same y-position across views; iced's Space
                // also ignores `height` when `width == 0`, so use a
                // 1 px-wide sliver to actually force the height.
                Space::new()
                    .width(Length::Fixed(1.0))
                    .height(Length::Fixed(34.0))
                    .into()
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

        // Sort dropdown trigger, sits just before the "+ Host" /
        // "+ Discover" action. Glyph reflects the active sort so the
        // current mode is readable without opening the menu.
        let sort_btn = crate::widgets::sort_toolbar_button(
            crate::state::SortMenuKind::Hosts,
            self.hosts_sort,
        );

        // Grid/List toggle, hidden once the window is so narrow that the
        // grid already renders as a single column (list == grid there).
        let nav_width = self.vault_rail_width();
        let panel_open = self.cloud_discover_visible || self.show_host_panel;
        let panel_width = if panel_open { crate::app::PANEL_WIDTH } else { 0.0 };
        let available = (self.window_size.width - nav_width - panel_width - 48.0).max(0.0);
        let responsive_cols =
            crate::widgets::card_grid_columns(available, crate::app::CARD_WIDTH, 12.0);
        let show_view_toggle = responsive_cols > 1;
        let view_toggle: Element<'_, Message> = if show_view_toggle {
            dir_row(vec![
                crate::widgets::host_view_toggle_button(self.setting_host_list_view),
                Space::new().width(6).into(),
            ])
            .align_y(iced::Alignment::Center)
            .into()
        } else {
            Space::new().width(0).into()
        };

        // ── Responsive collapse ──
        // #1: search yields before the folder name. #2: but the search
        // keeps a usable min-width, so once it hits that the breadcrumb
        // clips instead; only when the min won't fit at all does the search
        // fold to a floating-field icon. #3: when the whole button cluster
        // can't fit alongside the icon, every action folds into a single
        // `…` overflow menu (so the toolbar shows just the search + `…`).
        const SEARCH_MIN: f32 = 180.0;
        const ICON: f32 = 44.0;
        const GAP_SC: f32 = 10.0; // search ↔ cluster
        const GAP_BS: f32 = 12.0; // breadcrumb ↔ search
        const BC_FLOOR: f32 = 50.0;
        let in_group = self.active_group.is_some();
        let leading_w = self.toolbar_leading_width();
        let cluster_w = self.toolbar_cluster_width();
        let toolbar_w = self.toolbar_content_width();
        let (search_collapsed, buttons_overflow) = self.toolbar_tiers();
        let overflow_open = matches!(
            self.overlay.as_ref().map(|o| &o.content),
            Some(crate::state::OverlayContent::ToolbarOverflow)
        );

        // Breadcrumb width. The inline trailing is the full cluster, or
        // just the 44px `…` once the buttons have folded. While the search
        // is a field, cap the breadcrumb so the Fill search keeps at least
        // SEARCH_MIN (the name clips before the search shrinks past
        // usable). Once the search is an icon, the breadcrumb takes
        // whatever the icon + `…` leave.
        let trailing_w = if buttons_overflow { ICON } else { cluster_w };
        let left_el: Element<'_, Message> = if in_group {
            let (cap, clip_to_cap) = if search_collapsed {
                let c = (toolbar_w - ICON - GAP_SC - GAP_BS - trailing_w).max(0.0);
                (c, true)
            } else {
                let zone = toolbar_w - trailing_w - GAP_SC - GAP_BS;
                let c = (zone - SEARCH_MIN).max(BC_FLOOR);
                (c, leading_w > c)
            };
            if clip_to_cap {
                container(toolbar_left)
                    .width(Length::Fixed(cap))
                    .clip(true)
                    .into()
            } else {
                container(toolbar_left).clip(true).into()
            }
        } else {
            toolbar_left
        };

        let mut row_items: Vec<Element<'_, Message>> = vec![left_el];
        if in_group {
            row_items.push(Space::new().width(12).into());
        }
        row_items.push(self.vault_search_slot(search_collapsed));
        row_items.push(Space::new().width(10).into());
        if buttons_overflow {
            // Every action folds into the one `…` menu.
            row_items.push(crate::widgets::toolbar_overflow_icon(overflow_open));
        } else {
            row_items.push(view_toggle);
            row_items.push(sort_btn);
            row_items.push(Space::new().width(8).into());
            row_items.push(resolved_action);
        }

        // Let the row size to its natural height (button chrome included)
        // so the action button keeps its true visual size.
        let toolbar = container(dir_row(row_items).align_y(iced::Alignment::Center))
            // Top padding matches the 24px side padding so the page's inner
            // spacing is uniform on the X and Y axes.
            .padding(Padding { top: 16.0, right: 24.0, bottom: 16.0, left: 24.0 })
            .width(Length::Fill);
        toolbar.into()
    }
}
