//! Root layout: toolbar. Split out of views/layout/mod.rs.

use super::*;
impl Oryxis {
    /// Horizontal space the active view's toolbar Row actually has, after
    /// the nav rail, any open side panel, and the toolbar's own 24px side
    /// padding. Drives the responsive collapse tiers.
    pub(crate) fn toolbar_content_width(&self) -> f32 {
        let panel = if self.vault_panel_open() {
            crate::app::PANEL_WIDTH
        } else {
            0.0
        };
        (self.window_size.width - self.vault_rail_width() - panel - 48.0).max(0.0)
    }

    // ── Responsive-toolbar geometry constants ──
    // Toolbar action buttons are 24px of content inside iced's default
    // 10px horizontal button padding, so a square icon button is 44px and
    // a labelled button is (content + 20). These are the *real* rendered
    // widths, used so the collapse tiers fire at the right window size
    // (estimating low made the search collapse while space was still free).
    const TB_ICON: f32 = 44.0; // square icon button (sort, view toggle, search/overflow icon)
    const TB_SEARCH_MIN: f32 = 180.0; // smallest inline search worth keeping
    const TB_GAP_SC: f32 = 10.0; // gap between the search slot and the cluster
    const TB_GAP_BS: f32 = 12.0; // gap between breadcrumb and search (Hosts only)
    const TB_BC_FLOOR: f32 = 50.0; // breadcrumb never shrinks below this before the search collapses

    /// Full natural width of the active view's trailing button cluster
    /// (every action, inline, with the gaps between them). Single source
    /// of truth for both the collapse tiers and the floating-field width.
    pub(crate) fn toolbar_cluster_width(&self) -> f32 {
        match self.active_view {
            View::Dashboard => {
                // Grid/list toggle only shows above a single grid column.
                let nav_width = self.vault_rail_width();
                let panel = if self.vault_panel_open() {
                    crate::app::PANEL_WIDTH
                } else {
                    0.0
                };
                let available =
                    (self.window_size.width - nav_width - panel - 48.0).max(0.0);
                let cols = crate::widgets::card_grid_columns(
                    available,
                    crate::app::CARD_WIDTH,
                    12.0,
                );
                let toggle = if cols > 1 { Self::TB_ICON + 6.0 } else { 0.0 };
                // Action button: none inside a dynamic group, "Discover"
                // inside a cloud-linked folder, else the "+ Host" split.
                let action = match self.active_group {
                    Some(gid) => {
                        let dynamic = self
                            .groups
                            .iter()
                            .find(|g| g.id == gid)
                            .and_then(|g| g.cloud_query.as_ref())
                            .is_some();
                        if dynamic {
                            0.0
                        } else {
                            115.0
                        }
                    }
                    None => 113.0,
                };
                toggle + Self::TB_ICON + 8.0 + action
            }
            // sort(44) + gap(8) + the "+ Add" split(~113).
            View::Keys => Self::TB_ICON + 8.0 + 113.0,
            // sort(44) + gap(8) + "+ Snippet"(~92).
            View::Snippets => Self::TB_ICON + 8.0 + 92.0,
            View::Cloud => 95.0,            // "+ Account"
            View::PortForwarding => 92.0,   // "+ Port Forward"
            View::Proxies => {
                if self.proxy_identity_form.visible {
                    0.0
                } else {
                    113.0 // "+ Add" split
                }
            }
            // range label + prev/next pager(48 each) + "Clear all"(~98).
            View::History => 330.0,
            _ => 0.0,
        }
    }

    /// Estimated natural width of the leading breadcrumb (Hosts inside a
    /// group). 0 for every other view / the root host list. Overestimated
    /// per char so a long / CJK folder name yields the search to its min
    /// before the name itself clips.
    pub(crate) fn toolbar_leading_width(&self) -> f32 {
        if self.active_view != View::Dashboard {
            return 0.0;
        }
        let Some(gid) = self.active_group else {
            return 0.0;
        };
        let mut w = 0.0_f32;
        let mut cursor = Some(gid);
        let mut first = true;
        for _ in 0..5 {
            let Some(id) = cursor else { break };
            let Some(g) = self.groups.iter().find(|g| g.id == id) else {
                break;
            };
            if !first {
                w += 22.0; // "/" separator + its surrounding spacing
            }
            first = false;
            w += 18.0 + 6.0 + g.label.chars().count() as f32 * 12.0;
            cursor = g.parent_id;
        }
        w
    }

    /// Width of the floating search field popped when the search collapses
    /// to its icon. Because the search only ever collapses *after* the
    /// buttons have folded into the `…` (see `toolbar_tiers`), the sole
    /// inline trailing widget then is the 44px `…`, so the field spans the
    /// whole leading + search zone up to it. Shared by `overlay_menu_width`
    /// (rendering) and the toggle handler (anchor math) so both agree.
    pub(crate) fn toolbar_search_width(&self) -> f32 {
        (self.toolbar_content_width() - Self::TB_ICON - Self::TB_GAP_SC).clamp(200.0, 720.0)
    }

    /// Responsive toolbar tiers for the active view: whether the whole
    /// button cluster folds into a single `…` overflow menu, and (only at
    /// the very narrowest) whether the search itself collapses to a
    /// floating-field icon.
    ///
    /// Priority is to keep the search a real field at a usable min-width:
    /// when space runs out the *buttons* fold first (into the `…`) and the
    /// breadcrumb clips, so the search field survives; the search only
    /// becomes an icon when even a min-width field plus the `…` won't fit.
    /// Both thresholds come from real rendered widths (never a
    /// post-decision measurement), so the result is a monotonic step
    /// function of window width and can't oscillate: the overflow
    /// threshold sits above the collapse one because `cluster_w > TB_ICON`,
    /// so a collapsed search always implies overflowed buttons.
    pub(crate) fn toolbar_tiers(&self) -> (bool, bool) {
        let leading_w = self.toolbar_leading_width();
        let cluster_w = self.toolbar_cluster_width();
        let in_group = leading_w > 0.0;
        let gap_bs = if in_group { Self::TB_GAP_BS } else { 0.0 };
        let bc_floor = if in_group { Self::TB_BC_FLOOR } else { 0.0 };
        let w = self.toolbar_content_width();
        // Room a min-width search field needs alongside the breadcrumb floor.
        let base = bc_floor + gap_bs + Self::TB_SEARCH_MIN + Self::TB_GAP_SC;
        // Fold the buttons once the full cluster won't fit beside that field.
        let buttons_overflow = w < base + cluster_w;
        // Collapse the search only once even the field + the lone `…` won't fit.
        let search_collapsed = w < base + Self::TB_ICON;
        (search_collapsed, buttons_overflow)
    }

    /// The toolbar search element: the inline field, or (when `collapsed`,
    /// or whenever the floating field is already open) a search icon that
    /// pops the floating field. Shared by every vault view's toolbar. The
    /// icon hugs the trailing edge of the search zone via a leading Fill
    /// spacer, so the button cluster after it stays pinned right.
    pub(crate) fn vault_search_slot(&self, collapsed: bool) -> Element<'_, Message> {
        if self.active_view_search_empty() {
            // No search at all: keep a Fill spacer so the action cluster
            // stays trailing, exactly as `vault_search_field` does.
            return Space::new().width(Length::Fill).height(0).into();
        }
        // Force the icon whenever the floating field owns the input id, so
        // the inline field is never mounted at the same time (duplicate
        // `Id`) if a panel/resize flips `collapsed` while it's open.
        let overlay_open = matches!(
            self.overlay.as_ref().map(|o| &o.content),
            Some(crate::state::OverlayContent::ToolbarSearch)
        );
        if collapsed || overlay_open {
            // The slot must carry the toolbar row's Fill so the icon hugs
            // the trailing edge of the search zone and the button cluster
            // stays pinned right. A bare `dir_row` is a `Shrink` Row, which
            // would swallow the inner Fill spacer, so force Fill here.
            return crate::widgets::dir_row(vec![
                Space::new().width(Length::Fill).into(),
                crate::widgets::toolbar_search_icon(overlay_open),
            ])
            .align_y(iced::Alignment::Center)
            .width(Length::Fill)
            .into();
        }
        self.vault_search_field()
    }

    pub(crate) fn vault_search_field(&self) -> Element<'_, Message> {
        // Nothing to search → no search box (the empty state covers it).
        // Return a Fill spacer (not zero-width) so the toolbar slot keeps
        // stretching and the action cluster stays pinned to the trailing
        // edge, exactly as it does when the field is present.
        if self.active_view_search_empty() {
            return Space::new().width(Length::Fill).height(0).into();
        }
        // `ph_key` / `id` are static; only `value` borrows from `self`,
        // so they're kept in separate bindings (a shared tuple lifetime
        // would force the static `id` down to `self`'s lifetime, and
        // `Id::new` needs `'static`).
        let (ph_key, value, on_input): (&'static str, &str, fn(String) -> Message) =
            match self.active_view {
                View::Dashboard => (
                    "search_hosts",
                    self.host_search.as_str(),
                    Message::HostSearchChanged,
                ),
                View::Keys => (
                    "search_keys_identities",
                    self.key_search.as_str(),
                    Message::KeySearchChanged,
                ),
                View::Snippets => (
                    "search_snippets",
                    self.snippet_search.as_str(),
                    Message::SnippetSearchChanged,
                ),
                View::PortForwarding => (
                    "search_port_forwards",
                    self.port_forward_search.as_str(),
                    Message::PortForwardSearchChanged,
                ),
                View::History => (
                    "search_logs",
                    self.history_search.as_str(),
                    Message::HistorySearchChanged,
                ),
                View::Cloud => (
                    "search_cloud_accounts",
                    self.cloud_search.as_str(),
                    Message::CloudSearchChanged,
                ),
                View::Proxies => (
                    "search_proxies",
                    self.proxy_search.as_str(),
                    Message::ProxySearchChanged,
                ),
                _ => return Space::new().width(0).height(0).into(),
            };
        let id: &'static str = match self.active_view {
            View::Dashboard => "search-dashboard",
            View::Keys => "search-keys",
            View::Snippets => "search-snippets",
            View::PortForwarding => "search-port-forwards",
            View::History => "search-history",
            View::Cloud => "search-cloud",
            View::Proxies => "search-proxies",
            _ => "search-vault-subnav",
        };
        // Vertical padding tuned so the field's height matches the
        // toolbar action buttons beside it (24px content + 5px default
        // button padding top/bottom = 34px).
        iced::widget::text_input(crate::i18n::t(ph_key), value)
            .id(iced::widget::Id::new(id))
            .on_input(on_input)
            .padding(Padding { top: 9.0, right: 12.0, bottom: 9.0, left: 12.0 })
            .size(13)
            .width(Length::Fill)
            .style(crate::widgets::rounded_input_style)
            .align_x(dir_align_x())
            .into()
    }

    /// Workspace mode contextual sub-nav: horizontal pill row with
    /// the vault sub-sections. Search now lives in each view's own
    /// toolbar (see `vault_search_field`); this row is just the vault
    /// chip, the section pills, the "…" overflow and the settings gear.
    pub(crate) fn view_vault_sub_nav(&self) -> Element<'_, Message> {
        let pill = |label_key: &'static str, view: View| -> Element<'_, Message> {
            let is_active = self.active_view == view;
            let fg = if is_active {
                OryxisColors::t().accent
            } else {
                OryxisColors::t().text_muted
            };
            let bg = if is_active {
                Color { a: 0.15, ..OryxisColors::t().accent }
            } else {
                Color::TRANSPARENT
            };
            // Fixed-height inner container (28px) so every sub-nav
            // control lines up; button padding zeroed so its default
            // doesn't stack on top.
            button(
                container(
                    text(crate::i18n::t(label_key))
                        .size(12)
                        .color(fg),
                )
                .center_y(Length::Fixed(28.0))
                .padding(Padding { top: 0.0, right: 8.0, bottom: 0.0, left: 8.0 }),
            )
            .padding(0)
            .on_press(Message::ChangeView(view))
            .style(move |_, status| {
                let hover_bg = match status {
                    iced::widget::button::Status::Hovered if !is_active => {
                        Color { a: 0.08, ..OryxisColors::t().text_secondary }
                    }
                    _ => bg,
                };
                button::Style {
                    background: Some(Background::Color(hover_bg)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                }
            })
            .into()
        };
        // Priority+ overflow: the pills that fit render inline; the rest
        // live behind the "…" menu (see `subnav_pill_split`). No
        // horizontal scroll, the menu is the overflow affordance.
        let (inline_defs, overflow_defs) = self.subnav_pill_split();
        let pill_items: Vec<Element<'_, Message>> =
            inline_defs.iter().map(|(k, v)| pill(k, *v)).collect();
        let pills = dir_row(pill_items)
            .spacing(3)
            .align_y(iced::Alignment::Center);
        // Search moved out of this row into each view's toolbar
        // (see `vault_search_field`).
        // Static "Personal" vault chip on the leading edge: the active
        // vault's identity. Multi-vault switching isn't wired yet, so
        // this is a non-interactive placeholder (no dropdown). "Personal"
        // is the default vault's name (data, not a translated string).
        let vault_chip: Element<'_, Message> = container(
            dir_row(vec![
                iced_fonts::lucide::lock()
                    .size(13)
                    .color(OryxisColors::t().accent)
                    .into(),
                Space::new().width(6).into(),
                text("Personal")
                    .size(12)
                    .color(OryxisColors::t().text_primary)
                    .into(),
                Space::new().width(6).into(),
                iced_fonts::lucide::chevron_down()
                    .size(12)
                    .color(OryxisColors::t().text_muted)
                    .into(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .center_y(Length::Fixed(28.0))
        .padding(Padding { top: 0.0, right: 9.0, bottom: 0.0, left: 9.0 })
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border {
                radius: Radius::from(6.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        })
        .into();

        // Settings gear pinned at the trailing edge, outside the
        // scrollable, so it never scrolls out of reach (mirrors how the
        // top strip docks the "+").
        let settings_active = self.active_view == View::Settings;
        let settings_gear: Element<'_, Message> = button(
            container(
                iced_fonts::lucide::settings()
                    .size(15)
                    .color(if settings_active {
                        OryxisColors::t().accent
                    } else {
                        OryxisColors::t().text_muted
                    }),
            )
            .center_y(Length::Fixed(28.0))
            .padding(Padding { top: 0.0, right: 8.0, bottom: 0.0, left: 8.0 }),
        )
        .padding(0)
        .on_press(Message::ChangeView(View::Settings))
        .style(move |_, status| {
            let bg = if settings_active {
                Color { a: 0.15, ..OryxisColors::t().accent }
            } else if matches!(status, iced::widget::button::Status::Hovered) {
                Color { a: 0.08, ..OryxisColors::t().text_secondary }
            } else {
                Color::TRANSPARENT
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(6.0), ..Default::default() },
                ..Default::default()
            }
        })
        .into();

        // "…" overflow button (priority+): opens the menu with the
        // destinations that didn't fit. Sits right after the visible
        // pills. U+22EF is the midline ellipsis (vertically centered,
        // unlike the baseline-sitting U+2026).
        let overflow_btn: Element<'_, Message> = if overflow_defs.is_empty() {
            Space::new().width(0).into()
        } else {
            let open = self.show_subnav_overflow;
            button(
                container(
                    text("\u{22EF}")
                        .size(16)
                        .color(if open {
                            OryxisColors::t().accent
                        } else {
                            OryxisColors::t().text_muted
                        }),
                )
                .center_y(Length::Fixed(28.0))
                .padding(Padding { top: 0.0, right: 7.0, bottom: 0.0, left: 7.0 }),
            )
            .padding(0)
            .on_press(Message::ToggleSubnavOverflow)
            .style(move |_, status| {
                let bg = if open {
                    Color { a: 0.15, ..OryxisColors::t().accent }
                } else if matches!(status, iced::widget::button::Status::Hovered) {
                    Color { a: 0.08, ..OryxisColors::t().text_secondary }
                } else {
                    Color::TRANSPARENT
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                }
            })
            .into()
        };

        let mut row_items: Vec<Element<'_, Message>> = Vec::new();
        // Vault switcher chip only when there's more than one vault.
        if self.show_vault_switcher() {
            row_items.push(vault_chip);
            row_items.push(Space::new().width(8).into());
        }
        row_items.push(pills.into());
        row_items.push(overflow_btn);
        row_items.push(Space::new().width(Length::Fill).into());
        row_items.push(settings_gear);
        let row_inner = dir_row(row_items).align_y(iced::Alignment::Center);
        // Vertical padding equals the left padding so the chip sits with
        // equal breathing room on all sides. Background is a vertical
        // gradient from the top-bar color down into the content color,
        // so the row melts into the content below with no hard seam (no
        // bottom separator line).
        let row_content = container(row_inner)
            .padding(Padding { top: 8.0, right: 8.0, bottom: 8.0, left: 8.0 })
            .width(Length::Fill)
            .style(|_| container::Style {
                // Flat, same as the content below: the sub-nav reads as
                // a toolbar sitting on the content surface. The accent
                // hairline above it is the only chrome boundary (the
                // distinct-bg / gradient experiments all read worse).
                background: Some(Background::Color(OryxisColors::t().bg_primary)),
                ..Default::default()
            });
        // No bottom separator: the gradient already blends into content.
        row_content.into()
    }

    /// Estimated rendered width of one sub-nav pill, by label key.
    /// iced exposes no pre-render measurement, so this is a heuristic:
    /// ~7.5px per glyph + 21px padding + 2px inter-pill spacing.
    pub(crate) fn subnav_pill_width(key: &str) -> f32 {
        // ~6.3 px/char matches Noto Sans at 12 px; the old 7.5 over-
        // estimated and tripped the "…" collapse a pill or two early.
        // 16 px is the pill's horizontal padding (8+8), +6 for the row gap.
        crate::i18n::t(key).chars().count() as f32 * 6.3 + 16.0 + 6.0
    }

    /// Full ordered list of vault sub-nav destinations (Logs auto-hides
    /// until the feature is real for this user).
    pub(crate) fn subnav_pill_defs(&self) -> Vec<SubnavPill> {
        let mut defs: Vec<SubnavPill> = vec![
            ("hosts", View::Dashboard),
            ("keychain", View::Keys),
            ("snippets", View::Snippets),
            ("port_forwards", View::PortForwarding),
        ];
        if self.logs_surface_visible() {
            defs.push(("logs", View::History));
        }
        // Cloud Accounts / Proxies / Known Hosts were Settings sections
        // in v0.7; they're now first-class vault surfaces.
        defs.push(("cloud_accounts", View::Cloud));
        defs.push(("proxies", View::Proxies));
        defs.push(("known_hosts", View::KnownHosts));
        defs
    }

    /// Split the destinations into the pills that fit inline and the
    /// ones that overflow into the "…" menu (priority+ navigation). The
    /// active destination is always kept inline so its highlight stays
    /// visible in the row.
    pub(crate) fn subnav_pill_split(&self) -> (Vec<SubnavPill>, Vec<SubnavPill>) {
        // Reserve only the pinned flanks of THIS row: chip (~115, only
        // when the vault switcher shows), gear (~31), the "…" button
        // (~28), plus row padding and gaps. Search no longer lives here
        // (it moved to each view's toolbar), so nothing is reserved for
        // it, otherwise the "…" triggered way too early.
        let chip = if self.show_vault_switcher() { 115.0 } else { 0.0 };
        let flank = chip + 31.0 + 28.0 + 24.0;
        // An open side panel sits over the sub-nav's trailing edge now,
        // so subtract its width from the budget or pills would render
        // under the panel before the "…" kicks in.
        let panel_reserve = if self.side_panel_open() {
            crate::app::PANEL_WIDTH
        } else {
            0.0
        };
        let avail = (self.window_size.width - flank - panel_reserve).max(0.0);

        let mut inline: Vec<SubnavPill> = Vec::new();
        let mut overflow: Vec<SubnavPill> = Vec::new();
        let mut used = 0.0;
        for (k, v) in self.subnav_pill_defs() {
            let w = Self::subnav_pill_width(k);
            if overflow.is_empty() && used + w <= avail {
                used += w;
                inline.push((k, v));
            } else {
                overflow.push((k, v));
            }
        }
        // If the active view spilled into the overflow, swap it back in
        // (demoting the last inline pill) so the current location stays
        // highlighted in the strip.
        if let Some(pos) = overflow.iter().position(|(_, v)| *v == self.active_view) {
            let active = overflow.remove(pos);
            if let Some(last) = inline.pop() {
                overflow.insert(0, last);
            }
            inline.push(active);
        }
        (inline, overflow)
    }
}
