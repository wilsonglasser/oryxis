//! Tab bar + window chrome.
//!
//! Tabs render as pill-shaped chips with an OS-coloured icon badge on the
//! left that morphs into an X on hover/active (Termius-style close
//! affordance). The right-hand cluster, `[+]`, `[⋯]`, then the window
//! chrome (minimize / maximize / close), is pinned to the window edge and
//! never gets pushed off when many tabs are open. Tabs themselves shrink
//! uniformly to a minimum width as the bar fills, while the active tab
//! keeps its natural width so its label stays fully readable.

pub(crate) use iced::border::Radius;
pub(crate) use iced::widget::button::Status as BtnStatus;
pub(crate) use iced::widget::{button, container, row, scrollable, text, MouseArea, Space};
pub(crate) use iced::{Background, Border, Color, Element, Length, Padding};

pub(crate) use crate::app::{SftpMessage, TabsMessage, NavigationMessage, Message, Oryxis, AiMessage};
pub(crate) use crate::state::View;
pub(crate) use crate::tab_conn_state::TabConnState;
pub(crate) use crate::theme::{OryxisColors, SYSTEM_UI_SEMIBOLD};

/// One rendered slot of the unified strip, resolved from `tab_order`
/// against the live storage. A typed enum rather than the old
/// `(is_sftp, index)` pair so a new tab kind is a compile error at every
/// site that renders, measures or hit-tests the strip instead of silently
/// falling into the terminal branch (issue #120 added the third kind).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StripEntry {
    /// Index into `Oryxis::tabs`.
    Terminal(usize),
    /// Index into `Oryxis::sftp_tabs`.
    Sftp(usize),
    /// A panel tab (Settings, network tools); it has no storage vec to
    /// index, so it carries the kind itself.
    Panel(crate::state::PanelKind),
}

pub(crate) const TAB_HEIGHT: f32 = 26.0;
/// Height a tab chip actually RENDERS at: the `TAB_HEIGHT` content box
/// plus the button's default 5 px top/bottom padding. Anything that
/// wraps a chip (the `Stack` overlays for the progress border and the
/// underline rule) or measures a strip row (the vertical strip's row
/// pitch, its overflow math) must use this, not `TAB_HEIGHT`: a `Stack`
/// sized to the content box squeezes the button back down to 26 px, so
/// the chip silently loses its padding and every row-height estimate
/// drifts by 10 px.
pub(crate) const TAB_ROW_HEIGHT: f32 = TAB_HEIGHT + 10.0;
pub(crate) const TAB_ICON_SLOT: f32 = 16.0;
/// Diameter of the split pane-count pill ("2" on a grouped tab). A fixed
/// square so a single digit renders as a true circle rather than an oval.
pub(crate) const COUNT_DISC: f32 = 15.0;
/// Gap between the pane-count pill and the label that follows it.
///
/// Both of these are shared rather than local because the pill's footprint
/// has to be subtracted in TWO places that used to carry it as separate
/// magic numbers: `tab_content_width` (sizing the chip) and the label
/// truncation (fitting text inside it). When only the first knew about the
/// pill, grouped tabs spilled their label past the chip edge (#108).
pub(crate) const COUNT_GAP: f32 = 4.0;
/// Fixed width of a compact (Chrome-style) pinned tab chip.
pub(crate) const CHIP_W: f32 = 38.0;

/// Maximum width a tab claims when it has the room. Sized to fit a typical
/// label like "user@hostname.example.com" without truncation. The active
/// tab always uses this; inactives only when there's space.
pub(crate) const TAB_NATURAL_WIDTH: f32 = 240.0;
/// Floor below which we don't shrink, once a tab gets this narrow the
/// label is mostly ellipses anyway, and going lower kills hit-target ergonomics.
/// Picked to fit "OS-badge + ~8 chars + ellipsis" comfortably.
pub(crate) const TAB_MIN_WIDTH: f32 = 110.0;

/// Approximate per-character width at the tab label's font/size combo
/// (12 px SemiBold). Used to figure out how many chars fit in a compact
/// tab before the truncation kicks in.
pub(crate) const TAB_CHAR_WIDTH: f32 = 7.0;

/// Spacing between tabs, extracted into a constant so the width math
/// can subtract it without drifting from the actual `row.spacing()`.
// Tabs separate by their own internal padding; the strip adds only a hairline
// gap so adjacent hover/active fills don't visually merge. (padding + 6px gap +
// padding read as too much space, especially between compact pinned chips.)
pub(crate) const TAB_SPACING: f32 = 1.0;

/// Total height of the tab bar. Sized to comfortably fit a session tab
/// (whose inner row already includes the OS-icon badge at 18 px plus
/// padding) without feeling cramped, and tall enough that the chrome
/// buttons read as proper hit targets when filled corner-to-corner.
pub(crate) const BAR_HEIGHT: f32 = 40.0;

pub(crate) const SIDEBAR_TOGGLE_WIDTH: f32 = 28.0;
// `+` and `⋯` (jump-to) live in the right cluster next to the chrome
// buttons, so they share the chrome width, gives the whole strip a
// uniform 46×BAR_HEIGHT cell rhythm.
pub(crate) const PLUS_BUTTON_WIDTH: f32 = 46.0;
pub(crate) const DOTS_BUTTON_WIDTH: f32 = 46.0;
pub(crate) const SIDEBAR_BUTTON_WIDTH: f32 = 46.0;
pub(crate) const CHROME_BUTTON_WIDTH: f32 = 46.0;
pub(crate) const CHROME_TOTAL_WIDTH: f32 = CHROME_BUTTON_WIDTH * 3.0;

impl Oryxis {
    /// The default top bar: burger + tab strip + `+`/`⋯` + side-panel
    /// toggle + window chrome, all in one row.
    pub(crate) fn view_tab_bar(&self) -> Element<'_, Message> {
        self.tab_strip_bar(false)
    }

    /// Bottom-docked variant of the strip (Settings -> Interface -> Tab
    /// bar position): just the tabs, the `+` and the `⋯` jump button.
    /// The window chrome stays in `view_top_chrome_bar`.
    pub(crate) fn view_bottom_tab_strip(&self) -> Element<'_, Message> {
        self.tab_strip_bar(true)
    }

    /// Slim top bar for the docked layouts: burger, an empty
    /// window-drag area, the side-panel toggle and the chrome buttons.
    /// Keeps the titlebar affordances (drag, double-click maximize,
    /// minimize / maximize / close) at the top of the window where every
    /// OS puts them, while the tabs live at the bottom or on a side.
    /// In side-dock mode the Home area tab joins it next to the burger
    /// (the vertical strip carries session tabs only), and with
    /// `pinned_tabs_top_bar` the pinned tabs dock here too, their strip
    /// doubling as the drag area like the horizontal tab strip does.
    pub(crate) fn view_top_chrome_bar(&self) -> Element<'_, Message> {
        let side = tab_bar_pos().is_side();
        let solid_fill =
            self.prefs.tab_fill_style == "solid" || self.prefs.performance_mode;
        let pins_here = side && self.prefs.pinned_tabs_top_bar;

        let mut cluster_items: Vec<Element<'_, Message>> = Vec::new();
        if self.active_tab.is_some() {
            for toggle_side in self.sidebar_toggle_sides() {
                cluster_items.push(sidebar_btn(toggle_side, SIDEBAR_BUTTON_WIDTH, BAR_HEIGHT));
                cluster_items.push(Space::new().width(2).into());
            }
        }
        cluster_items.push(self.window_chrome_row(CHROME_BUTTON_WIDTH, BAR_HEIGHT).into());
        let right_cluster: Element<'_, Message> = crate::widgets::dir_row(cluster_items)
            .align_y(iced::Alignment::Center)
            .into();

        let mut leading: Vec<Element<'_, Message>> = vec![
            burger_menu_btn(self.panels.burger_menu),
            Space::new().width(1).height(TAB_HEIGHT).into(),
        ];
        if side {
            leading.push(self.home_area_tab(solid_fill));
        }
        // The middle Fill slot: the pinned-tab strip (which keeps the
        // window-drag / double-click contract on its empty area) or the
        // plain drag area.
        let ghost_ctx: Option<StripCtx> = pins_here.then(|| self.chrome_bar_pins_ctx());
        if let Some(ref ctx) = ghost_ctx {
            leading.push(self.chrome_bar_pins(ctx));
        } else {
            leading.push(
                MouseArea::new(
                    container(Space::new())
                        .width(Length::Fill)
                        .height(Length::Fixed(BAR_HEIGHT)),
                )
                .on_press(Message::Tabs(TabsMessage::WindowDrag))
                .on_double_click(Message::Tabs(TabsMessage::WindowMaximizeToggle))
                .into(),
            );
        }
        leading.push(right_cluster);
        let bar_bg = self.tab_bar_background();
        let bar: Element<'_, Message> = container(
            crate::widgets::dir_row(leading).align_y(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fixed(BAR_HEIGHT))
        .style(move |_| container::Style {
            background: Some(bar_bg),
            ..Default::default()
        })
        .into();

        // Floating ghost while a PINNED tab is being dragged: the pins
        // live on this bar, so the ghost tracks the cursor's x here;
        // the vertical strip draws the unpinned ghosts. Once the cursor
        // leaves the strip the window-level layer takes over, because
        // this Stack is clipped to the bar and the ghost has to follow
        // the cursor's y down into the content (issue #112).
        if let Some(ref ctx) = ghost_ctx
            && self.cursor_in_tab_strip()
            && self.dragged_tab_pinned()
            && let Some((ghost, ghost_w)) = self.strip_drag_ghost_el(
                ctx.drag_uniform_w,
                ctx.compact_pins,
                &ctx.privacy_terms,
            )
        {
            let gx = (self.mouse_position.x - ghost_w / 2.0).max(0.0);
            let positioned: Element<'_, Message> = iced::widget::Column::new()
                .push(Space::new().height(7.0))
                .push(
                    iced::widget::Row::new()
                        .push(Space::new().width(gx))
                        .push(ghost),
                )
                .into();
            return iced::widget::Stack::new()
                .push(bar)
                .push(positioned)
                .width(Length::Fill)
                .height(Length::Fixed(BAR_HEIGHT))
                .into();
        }
        bar
    }

    /// One width for every tab in the horizontal strip, or `None` when
    /// the adaptive default is in force.
    ///
    /// Adaptive sizing gives the ACTIVE tab its natural width and hugs
    /// the rest to their labels, so selecting a differently-named tab
    /// relays the whole bar and the chip under the pointer moves. That
    /// reflow is what the request is about (#112), not the widths
    /// themselves.
    ///
    /// The width is the widest label in the strip, so no tab ellipsizes
    /// while there is room, then shrunk to fit when there is not (the
    /// label truncates, the geometry stays put). Terminal AND SFTP tabs
    /// are measured: they share one strip, so measuring half of it would
    /// let an SFTP label clip at a width the terminal tabs agreed on.
    /// Compact pinned chips keep `CHIP_W` and sit outside this, exactly
    /// as they sit outside the adaptive allocation.
    fn uniform_tab_width(
        &self,
        close_on_right: bool,
        compact_pins: bool,
        approx_strip_width: f32,
    ) -> Option<f32> {
        if self.prefs.tab_width_mode != "uniform" {
            return None;
        }
        let mut widest = TAB_MIN_WIDTH;
        let mut flexible = 0.0f32;
        let mut pinned_chips = 0.0f32;
        let number_px = self.tab_number_px();
        for entry in self.strip_order() {
            let content = match entry {
                StripEntry::Sftp(idx) => {
                    let Some(tab) = self.sftp_tabs.get(idx) else {
                        continue;
                    };
                    if compact_pins && tab.pinned {
                        pinned_chips += 1.0;
                        continue;
                    }
                    tab_content_width(tab.display_label(), close_on_right, false, number_px)
                }
                StripEntry::Terminal(idx) => {
                    let Some(tab) = self.tabs.get(idx) else {
                        continue;
                    };
                    if compact_pins && tab.pinned {
                        pinned_chips += 1.0;
                        continue;
                    }
                    tab_content_width(
                        tab.display_label(self.tab_auto_title(tab)),
                        close_on_right,
                        tab.pane_count() > 1,
                        number_px,
                    )
                }
                StripEntry::Panel(kind) => {
                    panel_tab_width(crate::i18n::t(kind.label_key()), number_px)
                }
            };
            widest = widest.max(content);
            flexible += 1.0;
        }
        if flexible == 0.0 {
            return None;
        }
        // The user's ceiling, not the natural width: uniform exists so
        // the strip stops reshuffling, and letting one long label set 200
        // for everyone is the complaint that asked for this. The widest
        // label still decides below the cap.
        let cap = match self.prefs.tab_uniform_size.as_str() {
            "small" => 140.0,
            "large" => 260.0,
            _ => TAB_NATURAL_WIDTH,
        };
        let widest = widest.clamp(TAB_MIN_WIDTH, cap);
        // Fit check. Overflow shrinks every tab equally rather than
        // singling any out: uniform that stops being uniform under
        // pressure would bring back the reflow this mode exists to
        // remove. Below the minimum the scrollable takes over, as it
        // does for the adaptive mode.
        let spacing = TAB_SPACING * (flexible + pinned_chips - 1.0).max(0.0);
        let budget = approx_strip_width - pinned_chips * CHIP_W - spacing;
        let fitted = (budget / flexible).clamp(TAB_MIN_WIDTH, widest);
        Some(fitted)
    }

    /// Shared per-frame context for the pinned tabs rendered into the
    /// slim chrome bar: horizontal widths (active natural, inactives
    /// content-hugged; the scrollable is the overflow safety net).
    fn chrome_bar_pins_ctx(&self) -> StripCtx {
        let close_on_right = self.prefs.tab_close_button_side == "right";
        let number_px = self.tab_number_px();
        let mut session_widths = vec![TAB_MIN_WIDTH; self.tabs.len()];
        let mut max_inactive_content = TAB_MIN_WIDTH;
        for (i, tab) in self.tabs.iter().enumerate() {
            if self.active_tab == Some(i) {
                session_widths[i] = TAB_NATURAL_WIDTH;
            } else {
                let cw = tab_content_width(
                    tab.display_label(self.tab_auto_title(tab)),
                    close_on_right,
                    tab.pane_count() > 1,
                    number_px,
                );
                session_widths[i] = cw;
                max_inactive_content = max_inactive_content.max(cw);
            }
        }
        StripCtx {
            privacy_terms: self.privacy_terms(),
            close_on_right,
            close_armed: self.hover.tab_close_armed,
            compact_pins: self.prefs.pinned_tab_style == "compact",
            solid_fill: self.prefs.tab_fill_style == "solid"
                || self.prefs.performance_mode,
            dragging_any: self.tab_drag.map(|d| d.active).unwrap_or(false),
            drag_uniform_w: max_inactive_content.clamp(TAB_MIN_WIDTH, TAB_NATURAL_WIDTH),
            uniform_w: None,
            session_widths,
            number_px,
        }
    }

    /// The pinned tabs as a horizontal strip inside the slim chrome bar
    /// (side dock + `pinned_tabs_top_bar`): the same chips the strips
    /// render, in a hidden-scrollbar scrollable whose empty area keeps
    /// the window-drag / double-click-maximize titlebar contract.
    fn chrome_bar_pins<'a>(&'a self, ctx: &StripCtx) -> Element<'a, Message> {
        let mut items: Vec<Element<'a, Message>> = Vec::new();
        // Numbering counts the FULL strip, so the pinned chips the
        // chrome bar shows keep the numbers they have in the strip
        // proper instead of restarting at 1 here.
        for (slot, entry) in self.strip_order().into_iter().enumerate() {
            if !self.strip_entry_pinned(entry) {
                continue;
            }
            items.push(self.strip_tab_element(ctx, entry, slot));
        }
        let strip = scrollable(
            row(items)
                .spacing(TAB_SPACING)
                .align_y(iced::Alignment::Center),
        )
        .id(iced::widget::Id::new("chrome-pin-scroll"))
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new().width(0.0).scroller_width(0.0),
        ))
        .width(Length::Fill)
        .height(Length::Fixed(BAR_HEIGHT));
        MouseArea::new(
            container(strip)
                .width(Length::Fill)
                .padding(Padding { top: 4.0, right: 0.0, bottom: 4.0, left: 4.0 }),
        )
        .on_press(Message::Tabs(TabsMessage::WindowDrag))
        .on_double_click(Message::Tabs(TabsMessage::WindowMaximizeToggle))
        // A strip of chips is a strip, wherever it is docked: the pins
        // parked on the chrome bar answer a right-click the same way
        // the tab strip proper does (issue #186).
        .on_right_press(Message::Tabs(TabsMessage::ShowTabBarMenu))
        .into()
    }

    /// Window controls (minimize / maximize-restore / close) in their own
    /// dir_row so the close button ends up on the leading edge under RTL,
    /// matching how macOS and GNOME flip traffic-light buttons when the
    /// locale flips. Shared by the combined top bar, the slim chrome bar
    /// of the docked layouts (standard 46 x BAR_HEIGHT cells) and the
    /// side strip's header when the top bar is hidden (compact cells).
    pub(crate) fn window_chrome_row(&self, cell_w: f32, cell_h: f32) -> iced::widget::Row<'_, Message> {
        let max_icon = if self.window_maximized {
            iced_fonts::codicon::chrome_restore()
        } else {
            iced_fonts::codicon::chrome_maximize()
        };
        crate::widgets::dir_row(vec![
            window_btn(
                iced_fonts::codicon::chrome_minimize(),
                Message::Tabs(TabsMessage::WindowMinimize),
                OryxisColors::t().text_secondary,
                cell_w,
                cell_h,
            ),
            window_btn(
                max_icon,
                Message::Tabs(TabsMessage::WindowMaximizeToggle),
                OryxisColors::t().text_secondary,
                cell_w,
                cell_h,
            ),
            window_btn(
                iced_fonts::codicon::chrome_close(),
                Message::Tabs(TabsMessage::WindowClose),
                OryxisColors::t().error,
                cell_w,
                cell_h,
            ),
        ])
        .align_y(iced::Alignment::Center)
    }

    /// Shared bar background: the accent wash (tinted leading edge fading
    /// to the bar surface) when enabled, else the flat sidebar surface.
    /// Used by the combined top bar, the slim chrome bar and the
    /// bottom-docked strip so the chrome reads as one material.
    fn tab_bar_background(&self) -> Background {
        let bar_base = OryxisColors::t().bg_sidebar;
        if self.prefs.tab_accent_wash {
            let washed = crate::theme::mix(bar_base, self.top_accent_tint(), 0.16);
            Background::Gradient(iced::Gradient::Linear(
                iced::gradient::Linear::new(iced::Radians(std::f32::consts::FRAC_PI_2))
                    .add_stop(0.0, washed)
                    .add_stop(0.9, bar_base),
            ))
        } else {
            Background::Color(bar_base)
        }
    }

    /// Rough width available to the horizontal tab strip: the window
    /// minus the burger, the right cluster (the per-region sidebar
    /// toggles with the `+` / `⋯` / chrome buttons, matching what the
    /// cluster actually renders, issue #102) and the Workspace-mode
    /// area tabs. The exact value isn't critical (`scrollable` is the
    /// safety net); what matters is that the strip renderer and the
    /// center-active-tab scroll math read the SAME estimate, which is
    /// why this is one method instead of the two mirrored copies it
    /// used to be. `bottom` is the bottom-docked strip, whose burger
    /// and chrome live in the slim top bar.
    fn approx_strip_width(&self, bottom: bool) -> f32 {
        let toggle_count = if self.active_tab.is_some() {
            self.sidebar_toggle_sides().len() as f32
        } else {
            0.0
        };
        let right_cluster_width: f32 = toggle_count * (SIDEBAR_BUTTON_WIDTH + 2.0)
            + PLUS_BUTTON_WIDTH
            + 2.0
            + DOTS_BUTTON_WIDTH
            + 2.0
            + CHROME_TOTAL_WIDTH;
        // Workspace mode prepends area tabs (Hosts, SFTP) that consume
        // strip width before the connection tabs even start. Each is
        // roughly icon(16) + gap(6) + label(~50) + padding(20) ~= 90 px.
        let area_tab_count = 1 + (self.sftp_enabled as u32);
        const AREA_TAB_APPROX_WIDTH: f32 = 100.0;
        let area_tabs_total =
            area_tab_count as f32 * (AREA_TAB_APPROX_WIDTH + TAB_SPACING);
        let reserved = if bottom {
            PLUS_BUTTON_WIDTH + 2.0 + DOTS_BUTTON_WIDTH
        } else {
            SIDEBAR_TOGGLE_WIDTH + right_cluster_width
        };
        (self.window_size.width - reserved - area_tabs_total - 12.0).max(120.0)
    }

    /// Build the tab strip bar. `bottom == false` is the classic combined
    /// top bar (burger + tabs + right cluster with chrome); `bottom ==
    /// true` renders only the strip pieces (tabs, `+`, `⋯`), the chrome
    /// half living in `view_top_chrome_bar` instead.
    fn tab_strip_bar(&self, bottom: bool) -> Element<'_, Message> {
        let n_tabs = self.tabs.len();
        let active_idx = self.active_tab;

        // For compaction we need a rough estimate of the strip's width
        // (active tab natural, inactives shrink to fit).
        let approx_strip_width = self.approx_strip_width(bottom);

        // Per-tab width allocation. Inactive tabs hug their own label
        // (clamped to [MIN, NATURAL]); the active tab claims the full
        // NATURAL width so focusing it visibly "fattens" the chip
        // (JetBrains-style). When the combined widths overflow the strip
        // the inactive tabs shrink proportionally toward MIN (the
        // scrollable is the final safety net). Compact pinned chips are
        // fixed at CHIP_W and don't participate in the flexible sizing.
        let close_on_right = self.prefs.tab_close_button_side == "right";
        let compact_pins = self.prefs.pinned_tab_style == "compact";
        let number_px = self.tab_number_px();
        let mut session_widths = vec![TAB_MIN_WIDTH; n_tabs];
        let mut max_inactive_content = TAB_MIN_WIDTH;
        for (i, tab) in self.tabs.iter().enumerate() {
            if tab.pinned && compact_pins {
                session_widths[i] = CHIP_W;
            } else if active_idx == Some(i) {
                session_widths[i] = TAB_NATURAL_WIDTH;
            } else {
                let cw = tab_content_width(
                    tab.display_label(self.tab_auto_title(tab)),
                    close_on_right,
                    tab.pane_count() > 1,
                    number_px,
                );
                session_widths[i] = cw;
                max_inactive_content = max_inactive_content.max(cw);
            }
        }
        let n_f = n_tabs as f32;
        let total_spacing = TAB_SPACING * (n_f - 1.0).max(0.0);
        let desired_total: f32 = session_widths.iter().sum::<f32>() + total_spacing;
        // Scroll-mode trigger (brings in the `⋯` jump button): the tabs
        // at their desired widths plus spacing wouldn't fit the strip.
        // Computed from the same per-tab widths the strip actually
        // renders, so the button doesn't pop in while everything still
        // fits.
        let scroll_mode = desired_total > approx_strip_width;
        // Overflow shrink: pull the inactive tabs proportionally toward
        // MIN so the strip stays packed before the scrollable has to
        // scroll. The active tab keeps its NATURAL width; compact pins
        // keep CHIP_W.
        if desired_total > approx_strip_width {
            let overflow = desired_total - approx_strip_width;
            let shrinkable: f32 = (0..n_tabs)
                .filter(|&i| {
                    !(self.tabs[i].pinned && compact_pins) && active_idx != Some(i)
                })
                .map(|i| (session_widths[i] - TAB_MIN_WIDTH).max(0.0))
                .sum();
            if shrinkable > 0.0 {
                let ratio = ((shrinkable - overflow) / shrinkable).clamp(0.0, 1.0);
                for (i, w) in session_widths.iter_mut().enumerate().take(n_tabs) {
                    if (self.tabs[i].pinned && compact_pins) || active_idx == Some(i) {
                        continue;
                    }
                    *w = TAB_MIN_WIDTH + (*w - TAB_MIN_WIDTH) * ratio;
                }
            }
        }
        // Uniform width used while a tab is mid-drag, so the strip
        // geometry stays stable as the dragged slot slides (the
        // active/inactive width difference otherwise bounces the seam).
        // Sized to the widest inactive content so no label clips.
        let drag_uniform_w = max_inactive_content.clamp(TAB_MIN_WIDTH, TAB_NATURAL_WIDTH);
        // True overflow: even at TAB_MIN_WIDTH (compact pins at CHIP_W)
        // the tabs don't fit, so the scrollable actually scrolls. This
        // is the trigger that docks the "+" at the strip edge; the
        // softer `scroll_mode` above (tabs merely compressed below
        // natural) only brings in the `⋯` jump button. Without the
        // distinction the "+" jumped to the right cluster as soon as
        // three tabs compressed, long before anything scrolled.
        let pin_n = if compact_pins {
            self.tabs.iter().filter(|t| t.pinned).count()
        } else {
            0
        } as f32;
        let reg_n = n_f - pin_n;
        let min_total = pin_n * (CHIP_W + TAB_SPACING)
            + reg_n * TAB_MIN_WIDTH
            + (reg_n - 1.0).max(0.0) * TAB_SPACING;
        let strip_overflow = min_total > approx_strip_width;

        let mut tab_items: Vec<Element<'_, Message>> = Vec::new();
        // Active-tab fill style: gradient (default) or a flat accent tint.
        // Computed once and threaded into every tab/chip renderer so the
        // choice applies uniformly across session, SFTP and area tabs.
        // Performance mode forces the flat tint: the gradient is a
        // per-pixel shader in the software renderer, the flat tint a
        // single solid fill.
        let solid_fill = self.prefs.tab_fill_style == "solid" || self.prefs.performance_mode;

        // Terminal and SFTP tabs share one strip, pinned-first across BOTH
        // kinds (so an unpinned SFTP tab never jumps ahead of a pinned
        // terminal). `false` = terminal index into `self.tabs`, `true` = SFTP
        // index into `self.sftp_tabs`. Within a pin partition, terminals come
        // before SFTP tabs (cross-type drag-interleave is a later refinement).
        // While a drag is active every tab renders at the inactive width so
        // the strip geometry is uniform. The active-vs-inactive width
        // difference otherwise shifts positions on each live-slide swap and
        // bounces the dragged tab back and forth over a seam.
        let dragging_any = self.tab_drag.map(|d| d.active).unwrap_or(false);
        // One terms pass for the whole strip: Privacy Mode redacts the
        // rendered tab labels (issue #78) and must not rebuild the
        // hostname list per tab.
        let privacy_terms = self.privacy_terms();
        // Display order follows `tab_order` (the authoritative, drag-reorderable
        // unified order), partitioned pinned-first across both kinds. Each
        // `TabRef` maps to its current storage index. SFTP refs are skipped
        // when the SFTP feature is off. The per-entry element (session /
        // SFTP tab, compact pinned chip, or the drag gap) is built by the
        // shared `strip_tab_element` so the side-docked vertical strip
        // renders exactly the same chips.
        let ctx = StripCtx {
            privacy_terms,
            close_on_right,
            close_armed: self.hover.tab_close_armed,
            compact_pins,
            solid_fill,
            dragging_any,
            drag_uniform_w,
            uniform_w: self.uniform_tab_width(close_on_right, compact_pins, approx_strip_width),
            session_widths,
            number_px,
        };
        for (slot, entry) in self.strip_order().into_iter().enumerate() {
            tab_items.push(self.strip_tab_element(&ctx, entry, slot));
        }
        // "+" trails the last tab, browser-style (issue #38). Only when
        // the strip TRULY overflows (tabs at min width still don't fit,
        // so the scrollable scrolls) it docks at the strip's trailing
        // edge instead, just before the right cluster, so it can never
        // scroll out of reach with the tabs.
        // Wrapped in a MouseArea so entering the `+` during an active tab
        // drag drops the dragged tab at the end of its partition (the trailing
        // slot the live-slide can't reach). The handler no-ops when no drag is
        // in flight, so normal `+` clicks are unaffected.
        let plus_btn: Element<'_, Message> = MouseArea::new(crate::widgets::bounds_reporter(
            new_tab_btn(!strip_overflow),
            self.plus_btn_bounds.clone(),
        ))
        .on_enter(Message::Tabs(TabsMessage::TabDragToEnd))
        .into();
        let mut docked_plus: Option<Element<'_, Message>> = None;
        if strip_overflow {
            docked_plus = Some(plus_btn);
        } else {
            tab_items.push(plus_btn);
        }

        // The tab strip lives in an auto-width container, Length::Fill
        // so the row gives it whatever's left after the sidebar toggle
        // and right cluster claim their Shrink widths. The scrollable
        // inside is the safety net: tabs that don't fit at min width
        // overflow into a horizontal scroll (mouse-wheel works, the
        // scrollbar itself is zeroed out so it's invisible). The
        // surrounding MouseArea makes the empty area of the strip a
        // window-drag handle, since we no longer have a separate drag
        // sibling in the row.
        let tab_strip_inner = scrollable(
            row(tab_items)
                .spacing(TAB_SPACING)
                .align_y(iced::Alignment::Center),
        )
        .id(iced::widget::Id::new("tab-scroll"))
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new()
                .width(0.0)
                .scroller_width(0.0),
        ))
        .width(Length::Fill)
        .height(Length::Fixed(BAR_HEIGHT));

        // No leading padding in either mode: the burger (top) or the
        // Home tab's own gutter (bottom) already claims the leading
        // edge out in the fixed row. Bottom mode keeps one extra px on
        // top so the tabs don't hug the accent hairline that sits
        // directly above them.
        let strip_padding = if bottom {
            Padding { top: 5.0, right: 0.0, bottom: 4.0, left: 0.0 }
        } else {
            Padding { top: 4.0, right: 0.0, bottom: 4.0, left: 0.0 }
        };
        let tab_strip: Element<'_, Message> = MouseArea::new(
            container(tab_strip_inner)
                .width(Length::Fill)
                .padding(strip_padding),
        )
        .on_press(Message::Tabs(TabsMessage::WindowDrag))
        // Native title-bar convention: double-click the drag area to
        // toggle maximize.
        .on_double_click(Message::Tabs(TabsMessage::WindowMaximizeToggle))
        // Right-click the strip's own space: the strip menu (issue
        // #186). A chip captures its right press before this widget
        // sees it, so the two menus can't both open.
        .on_right_press(Message::Tabs(TabsMessage::ShowTabBarMenu))
        // Vertical wheel translates to horizontal scroll on the tab
        // strip. The horizontal scrollable inside doesn't capture a
        // pure-y wheel event (iced only steers wheel along the
        // direction the scrollable can actually scroll), so this
        // MouseArea picks it up and routes a scroll_by command.
        .on_scroll(|delta| {
            let y = match delta {
                iced::mouse::ScrollDelta::Lines { y, .. } => y * 60.0,
                iced::mouse::ScrollDelta::Pixels { y, .. } => y,
            };
            Message::Tabs(TabsMessage::TabBarWheel(y))
        })
        .into();

        // `⋯` jump-to button, shown only when the strip is compressed
        // (scroll mode). In the combined bar it heads the right cluster;
        // in the bottom-docked strip it trails the tabs directly.
        let dots_btn: Option<Element<'_, Message>> =
            if scroll_mode { Some(tab_jump_btn()) } else { None };

        // Row composition. Combined top bar: [burger] [tab_strip(Fill)]
        // [docked +?] [right cluster: ⋯? / side-panel / chrome]. Bottom-
        // docked strip: [tab_strip(Fill)] [docked +?] [⋯?], the burger and
        // chrome live in the slim top bar. Burger / cluster are
        // Length::Shrink so iced gives them their content width first;
        // tab_strip is the remaining Fill area in between. `dir_row` flips
        // the row under RTL so the leading-edge controls always sit next
        // to the sidebar (which the outer layout also flips).
        let mut leading: Vec<Element<'_, Message>> = Vec::new();
        if !bottom {
            // Burger menu on the far leading edge: its dropdown lists every
            // vault destination + global actions. Leading breathing space is
            // the burger button's own left padding (not a margin), so the gap
            // is part of its clickable / hover area.
            leading.push(burger_menu_btn(self.panels.burger_menu));
            // 1 px breather between the burger and the first area tab (home).
            leading.push(Space::new().width(1).height(TAB_HEIGHT).into());
        } else {
            // Bottom mode has no burger, so the Home tab leads the row;
            // give it a small gutter off the window edge. `dir_row`
            // flips the whole row under RTL, which carries the gutter
            // to the mirrored leading edge with no manual branch.
            leading.push(Space::new().width(8).height(TAB_HEIGHT).into());
        }
        // The navigation areas live as fixed top-level tabs before the
        // scrollable connection strip (see `home_area_tab` for the
        // selection family and why Settings stays out), so Home stays
        // reachable no matter how far the strip overflows.
        leading.push(self.home_area_tab(solid_fill));
        leading.push(Space::new().width(TAB_SPACING).height(TAB_HEIGHT).into());
        leading.push(tab_strip);
        if let Some(plus) = docked_plus {
            leading.push(plus);
        }
        if bottom {
            if let Some(dots) = dots_btn {
                leading.push(dots);
            }
        } else {
            // The right cluster sits on the trailing edge of the tab bar.
            // Build it in reading order ([extras] then chrome) and let
            // `dir_row` flip the order in RTL so chrome lands on the
            // outer edge there too.
            let mut cluster_items: Vec<Element<'_, Message>> = Vec::new();
            if let Some(dots) = dots_btn {
                cluster_items.push(dots);
                cluster_items.push(Space::new().width(2).into());
            }
            // The side-panel toggles (one per non-empty region, issue
            // #102) only make sense inside a connection tab, so skip
            // them on the navigation views where there's no terminal
            // session to attach a panel to.
            if self.active_tab.is_some() {
                for toggle_side in self.sidebar_toggle_sides() {
                    cluster_items
                        .push(sidebar_btn(toggle_side, SIDEBAR_BUTTON_WIDTH, BAR_HEIGHT));
                    cluster_items.push(Space::new().width(2).into());
                }
            }
            cluster_items.push(self.window_chrome_row(CHROME_BUTTON_WIDTH, BAR_HEIGHT).into());
            let right_cluster: Element<'_, Message> = crate::widgets::dir_row(cluster_items)
                .align_y(iced::Alignment::Center)
                .into();
            leading.push(right_cluster);
        }

        // Whole-bar accent wash: a tinted leading edge fading back to
        // the bar surface, same direction as the card accent wash + the
        // bottom hairline. Gated on `setting_tab_accent_wash`, and
        // breathes the active tab's colour via `top_accent_tint`. Both
        // gradient stops are opaque, so the tab buttons render normally.
        let bar_bg = self.tab_bar_background();
        let bar: Element<'_, Message> = container(
            crate::widgets::dir_row(leading)
                .align_y(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fixed(BAR_HEIGHT))
        .style(move |_| container::Style {
            background: Some(bar_bg),
            ..Default::default()
        })
        .into();

        // Floating ghost of the tab being dragged. The bar spans the window's
        // top-left, so window-space cursor x maps directly to bar-local x. The
        // ghost is a plain (non-interactive) container, so the tab MouseAreas
        // underneath still receive the hover events that drive the live-slide.
        // Only while the cursor is still on the strip: past that the
        // window-level layer draws it, free in both axes (issue #112).
        let drag_ghost_el = self
            .cursor_in_tab_strip()
            .then(|| self.strip_drag_ghost_el(drag_uniform_w, compact_pins, &ctx.privacy_terms))
            .flatten();
        if let Some((ghost, ghost_w)) = drag_ghost_el {
            let gx = (self.mouse_position.x - ghost_w / 2.0).max(0.0);
            let positioned: Element<'_, Message> = iced::widget::Column::new()
                .push(Space::new().height(7.0))
                .push(
                    iced::widget::Row::new()
                        .push(Space::new().width(gx))
                        .push(ghost),
                )
                .into();
            return iced::widget::Stack::new()
                .push(bar)
                .push(positioned)
                .width(Length::Fill)
                .height(Length::Fixed(BAR_HEIGHT))
                .into();
        }
        bar
    }
}

impl Oryxis {
    /// Build a task that snaps the tab strip's scrollable so the active
    /// tab is roughly centered in the visible area. Called whenever a
    /// new tab gets focused (manual select, opening a local shell,
    /// connecting an SSH session, etc.), without this the new tab
    /// can land off-screen when the strip is in scroll mode.
    /// Resolve the OS / brand icon hint for a tab from its (de-suffixed)
    /// label: a saved connection's detected OS, else a local-shell hint, else
    /// the cloud brand parsed from an `ECS · ...` / `K8s · ...` prefix.
    /// Effective auto-title (OSC 0/2) decision for a tab: the focused host's
    /// per-host `Connection.auto_title` override wins over the global
    /// `terminal_auto_title` setting; local shells and hosts with no override
    /// fall back to the global. Resolved live so editing a host updates its
    /// open tabs without a reconnect.
    pub(crate) fn tab_auto_title(&self, tab: &crate::state::TerminalTab) -> bool {
        if let crate::state::PaneOrigin::Host(id) = &tab.active().origin
            && let Some(conn) = self.connections.iter().find(|c| c.id == *id)
            && let Some(over) = conn.auto_title
        {
            return over;
        }
        crate::state::auto_title_enabled()
    }

    pub(crate) fn tab_detected_os(&self, base_label: &str) -> Option<String> {
        // Saved hosts first, then quick-connect entries (their detection
        // result lives in memory only), then the local/cloud hints.
        self.any_connection_by_label(base_label)
            .and_then(|c| c.detected_os.clone())
            .or_else(|| crate::os_icon::local_shell_os_hint(base_label))
            .or_else(|| {
                crate::os_icon::tab_label_cloud_brand(base_label).map(|s| s.to_string())
            })
    }

    /// Display order of the unified tab strip: pinned-first over the
    /// drag-reorderable `tab_order`, across both terminal and SFTP kinds.
    /// Each entry is `(is_sftp, storage_index)`. SFTP refs are dropped when
    /// the SFTP feature is off. Shared by `view_tab_bar` (rendering) and
    /// `tab_scroll_to_active` (offset math) so the two can't drift.
    /// The `tab_number_style` setting, parsed.
    pub(crate) fn tab_number_style(&self) -> TabNumberStyle {
        TabNumberStyle::from_setting(&self.prefs.tab_number_style)
    }

    /// Room every chip reserves for the number prefix, sized to the
    /// WIDEST number in the strip so the labels stay aligned instead of
    /// stepping right when the strip crosses ten tabs. Zero when the
    /// number is off or drawn in the badge slot, which costs no width.
    pub(crate) fn tab_number_px(&self) -> f32 {
        if self.tab_number_style() != TabNumberStyle::Prefix {
            return 0.0;
        }
        let digits = self.strip_order().len().max(1).to_string().len();
        label_px_width(&format!("{}. ", "9".repeat(digits)))
    }

    /// The tab's number for `slot`, or `None` when numbering is off.
    pub(crate) fn tab_number_at(&self, slot: usize) -> Option<TabNumber> {
        match self.tab_number_style() {
            TabNumberStyle::Off => None,
            style => Some(TabNumber {
                value: slot + 1,
                in_icon: style == TabNumberStyle::Icon,
            }),
        }
    }

    pub(crate) fn strip_order(&self) -> Vec<StripEntry> {
        let pinned_of = |r: &crate::state::TabRef| -> bool {
            match r {
                crate::state::TabRef::Terminal(id) => {
                    self.tabs.iter().find(|t| t._id == *id).map(|t| t.pinned).unwrap_or(false)
                }
                crate::state::TabRef::Sftp(id) => {
                    self.sftp_tabs.iter().find(|t| t.id == *id).map(|t| t.pinned).unwrap_or(false)
                }
                crate::state::TabRef::Panel(_) => false,
            }
        };
        let to_entry = |r: &crate::state::TabRef| -> Option<StripEntry> {
            match r {
                crate::state::TabRef::Terminal(id) => {
                    self.tabs.iter().position(|t| t._id == *id).map(StripEntry::Terminal)
                }
                crate::state::TabRef::Sftp(id) => {
                    if !self.sftp_enabled {
                        return None;
                    }
                    self.sftp_tabs.iter().position(|t| t.id == *id).map(StripEntry::Sftp)
                }
                crate::state::TabRef::Panel(kind) => {
                    self.panel_tab_open(*kind).then_some(StripEntry::Panel(*kind))
                }
            }
        };
        let mut order: Vec<StripEntry> = Vec::new();
        order.extend(self.tab_order.iter().filter(|r| pinned_of(r)).filter_map(to_entry));
        order.extend(self.tab_order.iter().filter(|r| !pinned_of(r)).filter_map(to_entry));
        order
    }

    pub(crate) fn tab_scroll_to_active(&self) -> iced::Task<Message> {
        let Some(active_idx) = self.active_tab else {
            return iced::Task::none();
        };
        if self.tabs.is_empty() {
            return iced::Task::none();
        }
        // Side-docked vertical strip: scroll along y instead. Rows are
        // uniform (TAB_HEIGHT + the button's 5px paddings + spacing), so
        // the offset is the active tab's display position times the row
        // pitch, centered in the strip's viewport. Compact pinned chips
        // pack several per row, which makes this slightly overshoot;
        // like the horizontal math below, approximate is fine, the
        // scrollable clamps.
        if tab_bar_pos().is_side() {
            let row_pitch = TAB_ROW_HEIGHT + TAB_SPACING;
            // With `pinned_tabs_top_bar` the pinned entries live with
            // the chrome, outside the scrollable, so they don't count
            // toward the offset. Compact pinned chips otherwise pack
            // several per row, which makes this slightly overshoot;
            // like the horizontal math below, approximate is fine, the
            // scrollable clamps.
            let pins_top = self.prefs.pinned_tabs_top_bar;
            let preceding = self
                .strip_order()
                .iter()
                .filter(|&&e| !(pins_top && self.strip_entry_pinned(e)))
                .position(|&e| e == StripEntry::Terminal(active_idx))
                .unwrap_or(active_idx) as f32;
            let viewport_h = (self.window_size.height - BAR_HEIGHT - 40.0).max(120.0);
            let y = (preceding * row_pitch - viewport_h / 2.0 + row_pitch / 2.0).max(0.0);
            return iced::widget::operation::scroll_to(
                iced::widget::Id::new("tab-scroll"),
                iced::widget::scrollable::AbsoluteOffset { x: 0.0, y },
            );
        }
        // The same estimate the strip renderer uses, so the offsets
        // line up with the layout.
        let approx_strip_width =
            self.approx_strip_width(tab_bar_pos() == TabBarPos::Bottom);
        let (active_w, inactive_w) =
            allocate_tab_widths(self.tabs.len(), approx_strip_width);
        // Sum widths of all tabs that come before the active one, plus
        // the spacing between them. The strip renders pinned-first over
        // the drag-reorderable tab_order, not in storage order, so use
        // the active tab's actual display position (else a reorder or pin
        // would center the wrong tab). Width is still approximated
        // uniformly here, as before; only the ordering is corrected.
        let preceding = self
            .strip_order()
            .iter()
            .position(|&e| e == StripEntry::Terminal(active_idx))
            .unwrap_or(active_idx) as f32;
        let mut x = preceding * (inactive_w + TAB_SPACING);
        // Center active in viewport instead of left-aligning so the
        // user has context (the previous + next tabs visible too).
        x = (x - approx_strip_width / 2.0 + active_w / 2.0).max(0.0);
        iced::widget::operation::scroll_to(
            iced::widget::Id::new("tab-scroll"),
            iced::widget::scrollable::AbsoluteOffset { x, y: 0.0 },
        )
    }
}

// Tab-bar helper fns split into themed sibling files.
mod buttons;
mod entry;
mod ghosts;
mod side_strip;
mod sizing;
mod tabs;

pub(crate) use buttons::*;
pub(crate) use entry::*;
pub(crate) use ghosts::*;
pub(crate) use side_strip::*;
pub(crate) use sizing::*;
pub(crate) use tabs::*;
