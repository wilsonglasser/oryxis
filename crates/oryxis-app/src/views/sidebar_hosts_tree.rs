//! Hosts sidebar tab (issue #102): an mRemoteNG-style tree of the
//! vault's groups and hosts. Folders expand/collapse in place (nested
//! to any depth, the #102 sub-group work), a click on a host opens a
//! session in a new tab, and the search shows every match with its
//! ancestor chain force-expanded. Session-independent by design: the
//! tab needs no live transport, so a region holding only this tab is
//! always available.

use iced::border::Radius;
use iced::widget::{column, container, text, Space};
use iced::{Background, Border, Color, Element, Length, Padding};
use uuid::Uuid;

use oryxis_core::models::Group;

use crate::app::{AiMessage, Message, Oryxis, SessionGroupMessage, SshMessage};
use crate::i18n::t;
use crate::theme::OryxisColors;
use crate::widgets::dir_row;

const STAB: crate::state::TerminalSidebarTab = crate::state::TerminalSidebarTab::HostsTree;

/// Per-group subtree aggregates (hosts, saved arrangements), built
/// once per frame by `tree_subtree_counts`.
#[derive(Default)]
struct TreeSubtreeCounts {
    hosts: std::collections::HashMap<Uuid, usize>,
    sessions: std::collections::HashMap<Uuid, usize>,
}

/// Indent per tree level, applied on the leading edge (via `dir_row`,
/// so it mirrors under RTL).
const INDENT: f32 = 14.0;

impl Oryxis {
    pub(crate) fn hosts_tree_tab_content(&self) -> Element<'_, Message> {
        // Focus target for the sidebar hotkey / Ctrl+F (entering the
        // tree lands the keyboard here), and an input row in the Tab
        // walk.
        let search = self.sidebar_nav_slot(
            crate::keynav::SidebarRow::input(iced::widget::Id::new("sidebar-hosts-search")),
            STAB,
            crate::widgets::INPUT_RADIUS,
            iced::widget::text_input(t("search"), &self.hosts_tree_search)
                .id(iced::widget::Id::new("sidebar-hosts-search"))
                .on_input(|v| Message::Ai(AiMessage::HostsTreeSearchChanged(v)))
                .padding(8)
                .size(13)
                .style(crate::widgets::rounded_input_style)
                .into(),
        );
        let header = container(
            dir_row(vec![search]).align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 10.0, right: 12.0, bottom: 8.0, left: 12.0 })
        .width(Length::Fill);

        // Saved arrangements count as content: a vault holding only
        // session groups still has rows to show (the empty-state gate
        // once hid them behind the placeholder).
        if self.connections.is_empty()
            && self.groups.is_empty()
            && self.session_groups.is_empty()
        {
            return column![header, placeholder(t("hosts_tree_empty"))]
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
        }

        let needle = self.hosts_tree_search.trim().to_lowercase();
        let mut rows: Vec<Element<'_, Message>> = Vec::new();
        // Which groups earn a row this frame, decided BEFORE any row
        // is built: the keynav recording happens at construction time,
        // so rows must be built strictly in display order (a subtree
        // materialised early, or built-then-discarded for a collapsed
        // folder, records phantom indices the keyboard then acts on -
        // that shipped as Enter connecting a host that wasn't even on
        // screen).
        let counts = self.tree_subtree_counts();
        let visible = self.tree_visibility(&needle, &counts);
        // The needle-free visibility, for subtrees under a folder that
        // MATCHED the search: a matching folder shows everything it
        // would show without a search (the ancestor-match rule both
        // trees follow), so its descendants gate on content alone.
        // With no needle the two maps are identical; skip the rebuild.
        let visible_base = if needle.is_empty() {
            None
        } else {
            Some(self.tree_visibility("", &counts))
        };
        let visible_base = visible_base.as_ref().unwrap_or(&visible);
        // Sync LWW merges can leave dangling parents and cycles; a
        // group whose chain doesn't reach a root degrades to root
        // (same policy as the dashboard), and the visited set keeps a
        // cycle from recursing forever.
        let mut visited: std::collections::HashSet<Uuid> = std::collections::HashSet::new();
        let mut roots: Vec<&Group> = self
            .groups
            .iter()
            .filter(|g| {
                g.parent_id.is_none() || !Group::is_reachable_from_root(&self.groups, g.id)
            })
            .collect();
        self.hosts_sort.sort_items(&mut roots, |g| g.label.clone(), |g| g.created_at);
        for group in roots {
            self.tree_group_rows(
                &mut rows,
                group,
                0,
                &needle,
                false,
                &visible,
                visible_base,
                &counts,
                &mut visited,
            );
        }
        // Root session groups (saved split-pane arrangements): no
        // folder, or a folder id that no longer resolves. The root has
        // no matching ancestor, so the needle applies (a `true` here
        // once let every root arrangement bypass the search).
        self.tree_session_rows(&mut rows, None, 0, &needle, false);
        // Root hosts: no group, or a group id that no longer resolves.
        let group_exists =
            |gid: Uuid| self.groups.iter().any(|g| g.id == gid);
        let mut root_hosts: Vec<(usize, &oryxis_core::models::Connection)> = self
            .connections
            .iter()
            .enumerate()
            .filter(|(_, c)| c.group_id.filter(|gid| group_exists(*gid)).is_none())
            .collect();
        self.hosts_sort.sort_items(
            &mut root_hosts,
            |(_, c)| c.label.clone(),
            |(_, c)| c.created_at,
        );
        for (idx, conn) in root_hosts {
            if crate::util::host_matches_search(conn, &needle) {
                rows.push(self.tree_host_row(idx, conn, 0));
            }
        }

        if rows.is_empty() {
            rows.push(placeholder(t("no_matches")));
        }

        let list = column(rows)
            .spacing(2)
            .padding(Padding { top: 0.0, right: 12.0, bottom: 12.0, left: 12.0 });
        let body = iced::widget::scrollable(list)
            .id(crate::keynav::sidebar_scroll_id(STAB))
            .width(Length::Fill)
            .height(Length::Fill);
        column![header, body]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Append one group's row (and, when expanded, its subtree) to
    /// `rows`, strictly in DISPLAY order: the keynav layer records a
    /// row the moment it is built, so nothing is ever built ahead of
    /// its on-screen position and nothing built is ever discarded.
    /// Whether a branch shows at all was decided up front by
    /// `tree_visibility`; `ancestor_match` carries a search hit on any
    /// ancestor folder down the recursion (a matching folder shows its
    /// WHOLE subtree, so its descendants gate on the needle-free
    /// `visible_base` instead).
    #[allow(clippy::too_many_arguments)]
    fn tree_group_rows<'a>(
        &'a self,
        rows: &mut Vec<Element<'a, Message>>,
        group: &'a Group,
        depth: usize,
        needle: &str,
        ancestor_match: bool,
        visible: &std::collections::HashMap<Uuid, bool>,
        visible_base: &std::collections::HashMap<Uuid, bool>,
        counts: &TreeSubtreeCounts,
        visited: &mut std::collections::HashSet<Uuid>,
    ) {
        if !visited.insert(group.id) {
            return;
        }
        let gate = if ancestor_match { visible_base } else { visible };
        if !gate.get(&group.id).copied().unwrap_or(false) {
            return;
        }
        let searching = !needle.is_empty();
        let expanded = searching || self.hosts_tree_expanded.contains(&group.id);
        rows.push(self.tree_group_row(group, depth, expanded, counts));
        if !expanded {
            return;
        }

        let label_match = ancestor_match
            || !searching
            || group.label.to_lowercase().contains(needle);
        let mut children: Vec<&Group> = self
            .groups
            .iter()
            .filter(|g| g.parent_id == Some(group.id))
            .collect();
        self.hosts_sort.sort_items(&mut children, |g| g.label.clone(), |g| g.created_at);
        for child in children {
            self.tree_group_rows(
                rows,
                child,
                depth + 1,
                needle,
                searching && label_match,
                visible,
                visible_base,
                counts,
                visited,
            );
        }

        // Saved split-pane arrangements filed under this folder, after
        // the subfolders and before the hosts (they open a whole tab,
        // like a folder of sessions in one click).
        self.tree_session_rows(rows, Some(group.id), depth + 1, needle, label_match);
        let mut hosts: Vec<(usize, &oryxis_core::models::Connection)> = self
            .connections
            .iter()
            .enumerate()
            .filter(|(_, c)| c.group_id == Some(group.id))
            .collect();
        self.hosts_sort.sort_items(&mut hosts, |(_, c)| c.label.clone(), |(_, c)| c.created_at);
        for (idx, conn) in hosts {
            // A matching group shows its whole host list; otherwise
            // only the hosts that match themselves.
            if label_match || crate::util::host_matches_search(conn, needle) {
                rows.push(self.tree_host_row(idx, conn, depth + 1));
            }
        }

    }

    /// Which groups earn a row for this frame's needle, computed
    /// WITHOUT building any widget (see `tree_group_rows` for why
    /// construction must follow display order). Memoised recursion in
    /// the `group_has_visible_content` style; the pre-seeded `false`
    /// doubles as the cycle guard.
    ///
    /// The rules, per group: needs a saved host or a saved
    /// arrangement somewhere below (an empty folder has nothing to
    /// connect to, owner ask); under a search additionally its label,
    /// one of its hosts, or a descendant must match.
    fn tree_visibility(
        &self,
        needle: &str,
        counts: &TreeSubtreeCounts,
    ) -> std::collections::HashMap<Uuid, bool> {
        fn visible(
            app: &Oryxis,
            gid: Uuid,
            needle: &str,
            counts: &TreeSubtreeCounts,
            memo: &mut std::collections::HashMap<Uuid, bool>,
        ) -> bool {
            if let Some(&v) = memo.get(&gid) {
                return v;
            }
            memo.insert(gid, false);
            let Some(group) = app.groups.iter().find(|g| g.id == gid) else {
                return false;
            };
            let searching = !needle.is_empty();
            let has_content = counts.hosts.get(&gid).copied().unwrap_or(0) > 0
                || counts.sessions.get(&gid).copied().unwrap_or(0) > 0;
            let v = if !has_content {
                false
            } else if !searching {
                true
            } else {
                group.label.to_lowercase().contains(needle)
                    || app.connections.iter().any(|c| {
                        c.group_id == Some(gid)
                            && crate::util::host_matches_search(c, needle)
                    })
                    || app
                        .session_groups
                        .iter()
                        .any(|sg| {
                            sg.group_id == Some(gid)
                                && sg.label.to_lowercase().contains(needle)
                        })
                    || app
                        .groups
                        .iter()
                        .filter(|g| g.parent_id == Some(gid))
                        .any(|g| visible(app, g.id, needle, counts, memo))
            };
            memo.insert(gid, v);
            v
        }
        let mut memo = std::collections::HashMap::new();
        for g in &self.groups {
            visible(self, g.id, needle, counts, &mut memo);
        }
        memo
    }

    /// One folder row: chevron + folder glyph (the group's custom icon
    /// and colour when set) + label + subtree host count. Click (or
    /// Enter on the ring) toggles the expansion.
    fn tree_group_row<'a>(
        &'a self,
        group: &'a Group,
        depth: usize,
        expanded: bool,
        counts: &TreeSubtreeCounts,
    ) -> Element<'a, Message> {
        let c = OryxisColors::t();
        let chevron = if expanded {
            iced_fonts::lucide::chevron_down()
        } else if crate::i18n::is_rtl_layout() {
            iced_fonts::lucide::chevron_left()
        } else {
            iced_fonts::lucide::chevron_right()
        };
        // Icon precedence mirrors the dashboard folder cards (owner
        // ask: the group's REAL badge, not a generic glyph): an
        // explicit user icon wins. Background: explicit group colour,
        // else the brand colour, else a plain folder glyph with no
        // badge at all - an all-accent badge on every folder would
        // turn the tree into a colour wall.
        let icon_id = group.icon.as_deref().filter(|s| !s.is_empty());
        let group_color = group.color.as_deref().and_then(crate::os_icon::parse_hex_color);
        let folder: Element<'a, Message> = match icon_id {
            Some(icon_id) => {
                let glyph = crate::os_icon::custom_icon_glyph(icon_id);
                let bg = group_color.unwrap_or_else(|| {
                    crate::os_icon::provider_icon(icon_id, c.accent).1
                });
                crate::widgets::host_icon(
                    crate::widgets::resolve_host_icon_style(
                        None,
                        &self.prefs.default_host_icon,
                    ),
                    bg,
                    &group.label,
                    Some(glyph.view(10.0, Color::WHITE)),
                    18.0,
                )
            }
            None => {
                let tint = group_color.unwrap_or(c.text_muted);
                if expanded {
                    iced_fonts::lucide::folder_open().size(14).color(tint).into()
                } else {
                    iced_fonts::lucide::folder().size(14).color(tint).into()
                }
            }
        };
        // Folders count their subtree's saved hosts.
        let subtree_hosts = counts.hosts.get(&group.id).copied().unwrap_or(0);
        let mut items: Vec<Element<'a, Message>> = vec![
            Space::new().width(depth as f32 * INDENT).into(),
            chevron.size(12).color(c.text_muted).into(),
            Space::new().width(4).into(),
            folder,
            Space::new().width(6).into(),
            text(group.label.as_str())
                .size(12)
                .color(c.text_primary)
                .width(Length::Fill)
                .wrapping(iced::widget::text::Wrapping::None)
                .into(),
        ];
        if subtree_hosts > 0 {
            items.push(Space::new().width(6).into());
            items.push(text(subtree_hosts.to_string()).size(11).color(c.text_muted).into());
        }
        let msg = Message::Ai(AiMessage::HostsTreeToggleGroup(group.id));
        self.sidebar_nav_slot(
            crate::keynav::SidebarRow::list_button(msg.clone()),
            STAB,
            6.0,
            tree_row_button(items, msg),
        )
    }

    /// One host row: the host's OWN icon badge (per-host icon / color
    /// / shape overrides, OS glyph, global shape default - the exact
    /// resolution the dashboard card uses, at tree size), a live dot
    /// when a tab is connected to this host, the label, and the
    /// address when the global "show host address" preference is on
    /// (masked by Privacy Mode, like the card subtitle). Click (or
    /// Enter on the ring) opens a session, the same message as the
    /// dashboard card.
    fn tree_host_row<'a>(
        &'a self,
        idx: usize,
        conn: &'a oryxis_core::models::Connection,
        depth: usize,
    ) -> Element<'a, Message> {
        let c = OryxisColors::t();
        let live = self.tabs.iter().any(|t| {
            t.pane_grid
                .panes
                .values()
                .any(|p| p.saved_conn_id() == Some(conn.id) && p.session.is_some())
        });
        // Same icon resolution as the dashboard host card, minus the
        // connected-green fallback: the live DOT is the tree's only
        // "connected" signal (owner call: colour on the badge AND a
        // bullet reads twice).
        let (os_glyph, icon_color) = crate::os_icon::resolve_for(
            conn.detected_os.as_deref(),
            conn.custom_icon.as_deref(),
            conn.custom_color.as_deref(),
            conn.username.as_deref(),
            c.accent,
        );
        let host_style = crate::widgets::resolve_host_icon_style(
            conn.icon_style.as_deref(),
            &self.prefs.default_host_icon,
        );
        let badge_color = conn
            .custom_color
            .as_deref()
            .or(conn.color.as_deref())
            .and_then(crate::widgets::parse_hex_color)
            .unwrap_or(icon_color);
        let icon_box = crate::widgets::host_icon(
            host_style,
            badge_color,
            &conn.label,
            Some(os_glyph.view(10.0, Color::WHITE)),
            18.0,
        );
        let mut items: Vec<Element<'a, Message>> = vec![
            Space::new().width(depth as f32 * INDENT + 16.0).into(),
            icon_box,
            Space::new().width(6).into(),
            text(conn.label.as_str())
                .size(12)
                .color(c.text_primary)
                .wrapping(iced::widget::text::Wrapping::None)
                .into(),
        ];
        if live {
            items.push(Space::new().width(5).into());
            items.push(
                container(Space::new().width(6).height(6))
                    .style(|_| container::Style {
                        background: Some(Background::Color(OryxisColors::t().success)),
                        border: Border { radius: Radius::from(3.0), ..Default::default() },
                        ..Default::default()
                    })
                    .into(),
            );
        }
        items.push(Space::new().width(Length::Fill).into());
        if self.prefs.show_host_address {
            // Privacy Mode masks the address behind blocks, same as
            // the card subtitle (no hover reveal here: tree rows are
            // click-to-connect, a reveal gesture would sit one pixel
            // from a connect).
            let address = crate::util::host_address_label(conn);
            let address = if self.privacy_active(conn) {
                crate::widgets::mask_blocks(&address)
            } else {
                address
            };
            items.push(
                text(address)
                    .size(11)
                    .color(c.text_muted)
                    .wrapping(iced::widget::text::Wrapping::None)
                    .into(),
            );
        }
        let msg = Message::Ssh(SshMessage::ConnectSsh(idx));
        // Right-click (and the Menu key on the ringed row, via
        // `with_menu`) opens the reduced card menu: the action set
        // minus Remove, per the right-click-opens-the-kebab
        // convention.
        let menu_msg = Message::Tabs(crate::app::TabsMessage::ShowTreeHostMenu(idx));
        let row: Element<'a, Message> =
            iced::widget::MouseArea::new(tree_row_button(items, msg.clone()))
                .on_right_press(menu_msg.clone())
                .into();
        self.sidebar_nav_slot(
            crate::keynav::SidebarRow::list_button(msg).with_menu(menu_msg),
            STAB,
            6.0,
            row,
        )
    }

    /// The saved split-pane arrangements filed under `folder` (`None`
    /// = root, including dangling folder ids), one row each: the
    /// session group's own badge (custom icon / colour, `boxes`
    /// default - the dashboard card at tree size), the label, and the
    /// pane count. Click (or Enter on the ring) opens the whole
    /// arrangement, the same message as the card. `parent_matched`
    /// short-circuits the search filter, like hosts under a matching
    /// folder.
    fn tree_session_rows<'a>(
        &'a self,
        rows: &mut Vec<Element<'a, Message>>,
        folder: Option<Uuid>,
        depth: usize,
        needle: &str,
        parent_matched: bool,
    ) {
        let group_exists = |gid: Uuid| self.groups.iter().any(|g| g.id == gid);
        let mut sessions: Vec<(usize, &oryxis_core::models::SessionGroup)> = self
            .session_groups
            .iter()
            .enumerate()
            .filter(|(_, sg)| match folder {
                Some(gid) => sg.group_id == Some(gid),
                None => sg.group_id.filter(|gid| group_exists(*gid)).is_none(),
            })
            .collect();
        self.hosts_sort.sort_items(
            &mut sessions,
            |(_, sg)| sg.label.clone(),
            |(_, sg)| sg.created_at,
        );
        let c = OryxisColors::t();
        for (idx, sg) in sessions {
            if !parent_matched
                && !needle.is_empty()
                && !sg.label.to_lowercase().contains(needle)
            {
                continue;
            }
            let bg = sg
                .color
                .as_deref()
                .and_then(crate::os_icon::parse_hex_color)
                .unwrap_or(c.accent);
            let glyph = sg
                .icon_style
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(crate::os_icon::custom_icon_glyph)
                .unwrap_or(crate::os_icon::BrandIcon::Glyph(iced_fonts::lucide::boxes()));
            let icon_box = crate::widgets::host_icon(
                crate::widgets::resolve_host_icon_style(None, &self.prefs.default_host_icon),
                bg,
                &sg.label,
                Some(glyph.view(10.0, Color::WHITE)),
                18.0,
            );
            let items: Vec<Element<'a, Message>> = vec![
                Space::new().width(depth as f32 * INDENT + 16.0).into(),
                icon_box,
                Space::new().width(6).into(),
                text(sg.label.as_str())
                    .size(12)
                    .color(c.text_primary)
                    .wrapping(iced::widget::text::Wrapping::None)
                    .into(),
                Space::new().width(Length::Fill).into(),
                text(format!(
                    "{} {}",
                    crate::views::dashboard::grid::count_leaves(&sg.layout),
                    t("session_group_panes")
                ))
                    .size(11)
                    .color(c.text_muted)
                    .wrapping(iced::widget::text::Wrapping::None)
                    .into(),
            ];
            let msg = Message::SessionGroup(SessionGroupMessage::OpenSessionGroup(idx));
            rows.push(self.sidebar_nav_slot(
                crate::keynav::SidebarRow::list_button(msg.clone()),
                STAB,
                6.0,
                tree_row_button(items, msg),
            ));
        }
    }

    /// Subtree aggregates for EVERY group in one pass over the vault:
    /// each host / saved arrangement marks its whole ancestor chain
    /// walking up (cycle-guarded). Replaces the per-folder
    /// `Group::subtree_ids` scans, which made every frame quadratic
    /// in the group count.
    fn tree_subtree_counts(&self) -> TreeSubtreeCounts {
        let parent_of: std::collections::HashMap<Uuid, Option<Uuid>> =
            self.groups.iter().map(|g| (g.id, g.parent_id)).collect();
        let mut counts = TreeSubtreeCounts::default();
        let up_chain = |start: Uuid, mark: &mut dyn FnMut(Uuid)| {
            let mut seen: std::collections::HashSet<Uuid> = std::collections::HashSet::new();
            let mut cur = Some(start);
            while let Some(g) = cur {
                if !seen.insert(g) || !parent_of.contains_key(&g) {
                    break;
                }
                mark(g);
                cur = parent_of.get(&g).copied().flatten();
            }
        };
        for c in &self.connections {
            if let Some(gid) = c.group_id {
                up_chain(gid, &mut |g| *counts.hosts.entry(g).or_insert(0) += 1);
            }
        }
        for sg in &self.session_groups {
            if let Some(gid) = sg.group_id {
                up_chain(gid, &mut |g| *counts.sessions.entry(g).or_insert(0) += 1);
            }
        }
        counts
    }

}

/// Shared row chrome: full-width flat button with hover / press
/// feedback (the button-feedback convention; no flat rows).
fn tree_row_button<'a>(
    items: Vec<Element<'a, Message>>,
    msg: Message,
) -> Element<'a, Message> {
    iced::widget::button(
        dir_row(items)
            .align_y(iced::Alignment::Center)
            .width(Length::Fill),
    )
    .on_press(msg)
    .padding(Padding { top: 5.0, right: 6.0, bottom: 5.0, left: 6.0 })
    .width(Length::Fill)
    .style(|_, status| {
        let bg = match status {
            iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed => {
                OryxisColors::t().bg_hover
            }
            _ => Color::TRANSPARENT,
        };
        iced::widget::button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    })
    .into()
}

/// Centered muted text for the empty / no-matches states.
fn placeholder(label: &str) -> Element<'_, Message> {
    container(text(label).size(12).color(OryxisColors::t().text_muted))
        .center_x(Length::Fill)
        .padding(Padding { top: 40.0, right: 12.0, bottom: 0.0, left: 12.0 })
        .width(Length::Fill)
        .into()
}

