//! Dashboard main content, the responsive grid of folder cards and
//! host cards plus the zero-connections early-return path. The
//! biggest chunk of `view_dashboard`, lifted here so the orchestrator
//! stays thin.
//!
//! Returns the full `main_content` (toolbar + search + status + body).
//! The mod-level `view_dashboard` only wraps it with the right-side
//! panel slot.

pub(crate) use iced::border::Radius;
pub(crate) use iced::widget::button::Status as BtnStatus;
pub(crate) use iced::widget::{button, container, scrollable, text, text_input, MouseArea, Space};
use iced::widget::column;
pub(crate) use uuid::Uuid;
pub(crate) use iced::{Background, Border, Color, Element, Length, Padding};

pub(crate) use crate::app::{TabsMessage, SshMessage, NavigationMessage, DashNavItem, Message, Oryxis, SessionGroupMessage, CARD_WIDTH};
pub(crate) use crate::i18n::t;
pub(crate) use crate::os_icon::BrandIcon;
pub(crate) use crate::theme::OryxisColors;
pub(crate) use crate::widgets::{card_grid_columns, dir_align_x, dir_row, distribute_card_grid};

/// Count the leaf panes in a saved session-group layout (for the card
/// subtitle).
pub(crate) fn count_leaves(layout: &oryxis_core::models::PaneLayout) -> usize {
    match layout {
        oryxis_core::models::PaneLayout::Split { a, b, .. } => {
            count_leaves(a) + count_leaves(b)
        }
        oryxis_core::models::PaneLayout::Leaf(_) => 1,
    }
}

/// Map (card, accent-colour, nav-item) tuples to renderable cards: apply
/// the shared `widgets::card_accent_wash` when `glass` is on, then draw
/// the keyboard-selection ring on the item matching `selected`. A free fn
/// (not a closure) so the input/output lifetimes stay tied.
pub(crate) fn apply_card_wash<'a>(
    cards: Vec<(Element<'a, Message>, Color, DashNavItem)>,
    glass: bool,
    selected: Option<DashNavItem>,
    ring_bounds: crate::widgets::BoundsCell,
) -> Vec<Element<'a, Message>> {
    cards
        .into_iter()
        .map(|(el, c, nav)| {
            let el = if glass {
                crate::widgets::card_accent_wash(el, c)
            } else {
                el
            };
            // Ring wrapper on EVERY card (transparent when unringed,
            // see select_ring_opt); the bounds_reporter, invisible to
            // the widget tree, wraps only the ringed card so the Menu
            // key can anchor the context menu at its kebab corner.
            let ringed = selected == Some(nav);
            let el = crate::widgets::select_ring_opt(
                el,
                10.0,
                ringed.then(|| crate::theme::OryxisColors::t().accent),
            );
            if ringed {
                crate::widgets::bounds_reporter(el, ring_bounds.clone())
            } else {
                el
            }
        })
        .collect()
}

/// The pre-pass both dashboard host surfaces share (the folder-card
/// grid and the tree view): one scan over connections + groups per
/// view call, so the per-card lookups (host / nested counts, filter
/// chips) all hit maps in O(1). Grew up inside
/// `dashboard_group_cards` and was then copied whole into the tree -
/// this struct is that copy, deduplicated.
pub(crate) struct DashGridPrePass {
    /// Direct connections per group.
    pub(crate) direct_host_count: std::collections::HashMap<Uuid, usize>,
    /// Child groups per parent.
    pub(crate) nested_group_count: std::collections::HashMap<Uuid, usize>,
    /// Subtree-match set for the tag filter (None when off).
    pub(crate) tag_filter_groups: Option<std::collections::HashSet<Uuid>>,
}

impl Oryxis {
    pub(crate) fn dash_grid_pre_pass(&self) -> DashGridPrePass {
        let mut direct_host_count: std::collections::HashMap<Uuid, usize> =
            std::collections::HashMap::new();
        for conn in &self.connections {
            if let Some(cgid) = conn.group_id {
                *direct_host_count.entry(cgid).or_insert(0) += 1;
            }
        }
        let mut nested_group_count: std::collections::HashMap<Uuid, usize> =
            std::collections::HashMap::new();
        for g in &self.groups {
            if let Some(pgid) = g.parent_id {
                *nested_group_count.entry(pgid).or_insert(0) += 1;
            }
        }
        let tag_filter_groups: Option<std::collections::HashSet<Uuid>> =
            self.groups_containing_filtered_tags();
        DashGridPrePass {
            direct_host_count,
            nested_group_count,
            tag_filter_groups,
        }
    }
}

// Card/section view methods, split into sibling files.
mod empty;
mod group;
mod host;
mod session;
mod tree;

impl Oryxis {
    /// Subtree-match set for the dashboard tag filter: every group
    /// whose descendants include a host carrying at least one selected
    /// tag (ancestors marked in one upward walk). `None` while the
    /// filter is off so callers can skip the check entirely.
    pub(crate) fn groups_containing_filtered_tags(
        &self,
    ) -> Option<std::collections::HashSet<Uuid>> {
        if self.host_filter_tags.is_empty() {
            return None;
        }
        let parent_of: std::collections::HashMap<Uuid, Option<Uuid>> =
            self.groups.iter().map(|g| (g.id, g.parent_id)).collect();
        let mut set = std::collections::HashSet::new();
        for conn in &self.connections {
            let matches = conn.tags.iter().any(|tg| {
                self.host_filter_tags.iter().any(|f| f.eq_ignore_ascii_case(tg))
            });
            if !matches {
                continue;
            }
            let mut cur = conn.group_id;
            while let Some(g) = cur {
                if !set.insert(g) {
                    break;
                }
                cur = parent_of.get(&g).copied().flatten();
            }
        }
        Some(set)
    }

    /// Manual group ids eligible for the parent-group pickers (host
    /// editor combo, Settings default-group). Every folder qualifies,
    /// including empty ones (a freshly created subgroup must be a
    /// pickable destination before anything lives in it).
    pub(crate) fn visible_group_ids(&self) -> std::collections::HashSet<Uuid> {
        self.groups.iter().map(|g| g.id).collect()
    }

    /// The host cards currently shown on the dashboard, as absolute
    /// indices into `self.connections`, in display order (group +
    /// search filters applied, then the user's sort). Shared by the
    /// grid renderer and the keyboard-selection navigation so Tab /
    /// arrows move through exactly what's on screen.
    pub(crate) fn dashboard_host_order(&self) -> Vec<usize> {
        let at_root = self.active_group.is_none();
        let flatten = self.flatten_hosts && at_root;
        let search_lower = self.host_search.to_lowercase();
        let mut host_order: Vec<usize> = (0..self.connections.len())
            .filter(|&i| {
                let conn = &self.connections[i];
                if let Some(gid) = self.active_group {
                    if conn.group_id != Some(gid) {
                        return false;
                    }
                } else if conn.group_id.is_some() && !flatten {
                    return false;
                }
                if !crate::util::host_matches_search(conn, &search_lower) {
                    return false;
                }
                if !self.host_filter_tags.is_empty()
                    && !conn.tags.iter().any(|tg| {
                        self.host_filter_tags.iter().any(|f| f.eq_ignore_ascii_case(tg))
                    })
                {
                    return false;
                }
                true
            })
            .collect();
        self.hosts_sort.sort_items(
            &mut host_order,
            |&i| self.connections[i].label.clone(),
            |&i| self.connections[i].created_at,
        );
        host_order
    }

    /// True on a first-run vault: nothing saved anywhere, so the
    /// dashboard renders `dashboard_empty_state` (no toolbar, no
    /// search field). Read outside the view too, by
    /// `active_view_search_id`, so the keyboard never tries to focus a
    /// search field this screen doesn't build.
    pub(crate) fn dashboard_is_empty(&self) -> bool {
        self.connections.is_empty() && self.groups.is_empty() && self.session_groups.is_empty()
    }

    pub(super) fn dashboard_main_content(&self) -> Element<'_, Message> {
        if self.dashboard_is_empty() {
            // Nothing navigable; keep the keyboard order in sync. The
            // empty state builds no toolbar, so it resets that
            // recording itself.
            self.keynav_clear_content();
            return self.dashboard_empty_state();
        }

        let toolbar = self.dashboard_toolbar();

        // ── Search bar ──
        // The host search lives in the dashboard toolbar
        // (`vault_search_field`) now, so the legacy full-width bar here
        // collapses to a zero-height spacer.
        let search_bar: Element<'_, Message> = Space::new().into();

        // The host editor's validation error renders inside the
        // editor panel itself (`host_panel::view_host_panel`) right
        // above the Save button. Slot reserved for future list-level
        // statuses.
        let status: Element<'_, Message> = Space::new().into();
        let at_root = self.active_group.is_none();
        let flatten = self.flatten_hosts && at_root;

        // Tree mode        // Tree mode builds its own depth-aware walk (grid/tree.rs);
        // the flat collectors stay empty so nothing below double
        // renders.
        let tree_mode = self.prefs.host_view_mode == crate::state::HostViewMode::Tree;
        let (group_cards, host_cards) = if tree_mode {
            (Vec::new(), Vec::new())
        } else {
            (self.dashboard_group_cards(), self.dashboard_host_cards())
        };

        // Column count adapts to current window width minus the visible
        // chrome (left nav + optional right panel + horizontal padding).
        // Re-derived on every view() so resizing the window or toggling
        // the side panel reflows the cards into the new column count.
        let nav_width = self.vault_rail_width();
        let panel_open = self.panels.host_panel;
        let panel_width = if panel_open { self.panel_width } else { 0.0 };
        // A side-docked tab strip (issue #87) narrows the content band
        // like the other grids; without it the math yields one column
        // too many and the card row clips at the edge.
        let available = (self.window_size.width
            - nav_width
            - panel_width
            - self.side_strip_reserve()
            - 48.0)
            .max(0.0);
        // List and tree modes force a single column; otherwise the
        // grid reflows responsively to the available width.
        let cols = if self.prefs.host_view_mode == crate::state::HostViewMode::Grid {
            card_grid_columns(available, CARD_WIDTH, 12.0)
        } else {
            1
        };

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

        // Saved session groups that live in the current folder. The
        // enumerate index is absolute (into `self.session_groups`), which is
        // what Open/Edit/Delete expect. Tree mode emits them inside
        // its own walk, at their folder's level.
        let session_group_cards: Vec<(Element<'_, Message>, Color, DashNavItem)> = if tree_mode
        {
            Vec::new()
        } else {
            self.session_groups
                .iter()
                .enumerate()
                .filter(|(_, g)| g.group_id == self.active_group)
                .map(|(i, g)| {
                    let (el, color) = self.session_group_card(i, g);
                    (el, color, DashNavItem::SessionGroup(i))
                })
                .collect()
        };

        // Per the `card_accent_glass` setting: glass on → each card gets
        // the soft per-colour wash; off → cards stay pure (just the
        // element, no overlay).
        let glass = self.prefs.card_accent_glass;
        let selected = match self.keynav.selected_in(crate::keynav::FocusZone::Content) {
            Some(crate::keynav::NavItem::Dash(d)) => Some(d),
            _ => None,
        };

        // List mode (cols == 1) renders History-style rows: full-width
        // rounded cards with a small gap. Grid mode keeps the roomier
        // 12px gutters. Tree mode is dense on purpose - a hairline gap
        // keeps its indent guide lines reading as near-continuous.
        let gap = match self.prefs.host_view_mode {
            crate::state::HostViewMode::Grid => 12.0,
            crate::state::HostViewMode::List => 8.0,
            crate::state::HostViewMode::Tree => 2.0,
        };

        let mut content_rows: Vec<Element<'_, Message>> = Vec::new();
        let tree_cards = if tree_mode { self.dashboard_tree_cards() } else { Vec::new() };
        // Record the keyboard-navigation order as visual rows (groups rows
        // then hosts rows, each chunked to the column count) so the key
        // handler can move the selection in 2-D without re-deriving the
        // group order. Groups + session groups share the groups section.
        // Tree mode is one linear section instead: one item per row,
        // in exactly the walk's display order.
        if tree_mode {
            self.keynav_set_content_sections(vec![tree_cards
                .iter()
                .map(|(_, _, n)| vec![crate::keynav::NavItem::Dash(*n)])
                .collect()]);
        } else {
            let cw = cols.max(1);
            let group_nav: Vec<DashNavItem> = group_cards
                .iter()
                .chain(session_group_cards.iter())
                .map(|(_, _, n)| *n)
                .collect();
            let host_nav: Vec<DashNavItem> =
                host_cards.iter().map(|(_, _, n)| *n).collect();
            let dash_row = |c: &[DashNavItem]| {
                c.iter().map(|&n| crate::keynav::NavItem::Dash(n)).collect()
            };
            // Two Tab sections (Groups, then Hosts); arrows still flow
            // continuously across both.
            self.keynav_set_content_sections(vec![
                group_nav.chunks(cw).map(dash_row).collect(),
                host_nav.chunks(cw).map(dash_row).collect(),
            ]);
        }

        // Search filtered everything out but the query parses as an
        // ad-hoc `user@host[:port]` target: offer a centered quick-connect
        // card instead of a bare no-results gap (mirrors the picker row
        // and the toolbar's "Enter to connect" hint).
        if group_cards.is_empty()
            && session_group_cards.is_empty()
            && host_cards.is_empty()
            && tree_cards.is_empty()
            && !self.host_search.trim().is_empty()
            && let Some(conn) = self.dashboard_quick_connect_target(&self.host_search)
        {
            content_rows.push(self.quick_connect_card(conn));
        }
        if tree_mode {
            let washed =
                apply_card_wash(tree_cards, glass, selected, self.keynav.ring_bounds.clone());
            content_rows.push(distribute_card_grid(washed, 1, gap, gap));
        } else if flatten {
            // Session groups live under the same "Groups" section as host
            // groups (they're both group-shaped entries), instead of a
            // separate "Session Groups" section. Host groups come first.
            if !group_cards.is_empty() || !session_group_cards.is_empty() {
                // `section_header` already carries its own 4/8 vertical
                // padding (mirroring Keychain), so no extra Space below.
                content_rows.push(section_header("groups_section"));
                let mut grouped = group_cards;
                grouped.extend(session_group_cards);
                let grouped = apply_card_wash(grouped, glass, selected, self.keynav.ring_bounds.clone());
                content_rows.push(distribute_card_grid(grouped, cols, gap, gap));
                content_rows.push(Space::new().height(20).into());
            }
            if !host_cards.is_empty() {
                content_rows.push(section_header("hosts_section"));
                let host_cards = apply_card_wash(host_cards, glass, selected, self.keynav.ring_bounds.clone());
                content_rows.push(distribute_card_grid(host_cards, cols, gap, gap));
            }
        } else {
            // Legacy: groups, then session groups, then hosts, in one grid.
            let mut combined = group_cards;
            combined.extend(session_group_cards);
            combined.extend(host_cards);
            let combined = apply_card_wash(combined, glass, selected, self.keynav.ring_bounds.clone());
            content_rows.push(distribute_card_grid(combined, cols, gap, gap));
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
        )
        .id(iced::widget::Id::new("dashboard-grid-scroll"))
        .height(Length::Fill);

        let main_content = column![toolbar, search_bar, status, grid]
            .width(Length::Fill)
            .height(Length::Fill);
        main_content.into()
    }

    /// Centered ad-hoc quick-connect card, shown when the host search
    /// matches nothing but parses as a `user@host[:port]` target. The
    /// whole card is a button (hover + press feedback per convention)
    /// dispatching `QuickConnect`; nothing is saved to the vault.
    fn quick_connect_card(
        &self,
        conn: oryxis_core::models::Connection,
    ) -> Element<'_, Message> {
        let label = conn.label.clone();
        // Protocol badges, for a line that named no `scheme://`. They
        // live OUTSIDE the card button (a button inside a button never
        // gets its own press), directly under it.
        let badges: Option<Element<'_, Message>> = self
            .quick_connect_badges(&self.host_search)
            .map(|(options, selected)| {
                let chips: Vec<Element<'_, Message>> = options
                    .into_iter()
                    .map(|p| {
                        let on = p == selected;
                        let bg = if on {
                            OryxisColors::t().accent
                        } else {
                            OryxisColors::t().bg_hover
                        };
                        let fg = if on {
                            crate::theme::contrast_text_for(bg)
                        } else {
                            OryxisColors::t().text_secondary
                        };
                        button(text(p.to_string()).size(11).color(fg))
                            .on_press(Message::Ssh(SshMessage::QuickConnectProtocolPicked(p)))
                            .padding(Padding { top: 3.0, right: 10.0, bottom: 3.0, left: 10.0 })
                            .style(move |_, status| button::Style {
                                background: Some(Background::Color(match status {
                                    BtnStatus::Hovered | BtnStatus::Pressed => {
                                        OryxisColors::t().bg_selected
                                    }
                                    _ => bg,
                                })),
                                border: Border {
                                    radius: Radius::from(4.0),
                                    ..Default::default()
                                },
                                text_color: fg,
                                ..Default::default()
                            })
                            .into()
                    })
                    .collect();
                container(crate::widgets::dir_row(chips).spacing(6))
                    .padding(Padding { top: 10.0, right: 0.0, bottom: 0.0, left: 0.0 })
                    .center_x(Length::Fill)
                    .into()
            });
        let card = button(
            iced::widget::Column::with_children(vec![
                iced_fonts::lucide::zap()
                    .size(28)
                    .color(OryxisColors::t().accent)
                    .into(),
                Space::new().height(10).into(),
                // The protocol is named whenever it is not the default
                // one, so a Telnet or Serial dial says so before Enter
                // rather than after it opens.
                text(
                    match conn.protocol
                        == oryxis_core::models::connection::ConnectionProtocol::Ssh
                    {
                        true => format!("{}: {}", t("quick_connect"), label),
                        false => format!(
                            "{} ({}): {}",
                            t("quick_connect"),
                            conn.protocol,
                            label
                        ),
                    },
                )
                .size(15)
                .color(OryxisColors::t().text_primary)
                .into(),
                Space::new().height(4).into(),
                text(t("quick_connect_not_saved"))
                    .size(12)
                    .color(OryxisColors::t().text_muted)
                    .into(),
                Space::new().height(8).into(),
                container(
                    text(t("quick_connect_hint"))
                        .size(11)
                        .color(OryxisColors::t().accent),
                )
                .padding(Padding { top: 2.0, right: 8.0, bottom: 2.0, left: 8.0 })
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().bg_hover)),
                    border: Border {
                        radius: Radius::from(4.0),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .into(),
            ])
            .align_x(iced::alignment::Horizontal::Center),
        )
        .on_press(Message::Ssh(SshMessage::QuickConnect(Box::new(
            crate::state::QuickConnectEntry::bare(conn),
        ))))
        .padding(Padding { top: 24.0, right: 32.0, bottom: 24.0, left: 32.0 })
        .style(|_, status| {
            let (bg, bc) = match status {
                BtnStatus::Hovered => (OryxisColors::t().bg_hover, OryxisColors::t().accent),
                BtnStatus::Pressed => {
                    (OryxisColors::t().bg_selected, OryxisColors::t().accent)
                }
                _ => (OryxisColors::t().bg_surface, OryxisColors::t().border),
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border {
                    radius: Radius::from(10.0),
                    color: bc,
                    width: 1.0,
                },
                ..Default::default()
            }
        });
        let mut stack = iced::widget::Column::new().align_x(iced::alignment::Horizontal::Center);
        stack = stack.push(card);
        if let Some(badges) = badges {
            stack = stack.push(badges);
        }
        container(stack)
            .width(Length::Fill)
            .center_x(Length::Fill)
            .padding(Padding { top: 32.0, right: 0.0, bottom: 0.0, left: 0.0 })
            .into()
    }
}


