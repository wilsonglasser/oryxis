//! Terminal view + AI chat sidebar.

use std::sync::Arc;

use iced::border::Radius;
use iced::widget::{
    button, canvas, column, container, row, scrollable, text, text_input, MouseArea, Space,
};
use iced::widget::button::Status as BtnStatus;
use iced::{Background, Border, Color, Element, Length, Padding};

use oryxis_terminal::widget::TerminalView;

use crate::app::{SettingsMessage, TerminalMessage, ZmodemMessage, AiMessage, Message, Oryxis};
use crate::i18n::t;
use crate::state::TerminalTab;
use crate::theme::OryxisColors;
use crate::widgets::dir_row;

use crate::util::truncate_middle;

impl Oryxis {
    pub(crate) fn view_terminal(&self) -> Element<'_, Message> {
        // Hybrid tab in Files mode: the tab's whole content area is the
        // dual-pane SFTP surface (its state hoisted into `self.sftp` by
        // the toggle / SelectTab; the owner check keeps a broken
        // invariant from ever rendering another surface's data). The
        // PTY keeps running underneath and output keeps processing.
        if let Some(tab) = self.active_tab.and_then(|idx| self.tabs.get(idx))
            && tab.files_mode
            && self.hybrid_sftp_owner == Some(tab._id)
        {
            // The ZMODEM card still floats on top: the hidden PTY keeps
            // processing output, so a remote `sz` can seize the pane
            // while the files surface is up and must stay visible.
            let mut stack = iced::widget::Stack::new().push(self.view_sftp());
            if let Some(zm) = self.transfer_overlay() {
                stack = stack.push(zm);
            }
            return stack.into();
        }
        // Which sidebar regions are on screen (issue #102): each side
        // is open on the tab AND has at least one available tab. Both
        // recordings reset here, before either region renders, so a
        // frame with one region open never leaves the other's stale
        // rows behind.
        use crate::state::SidebarSide;
        let (sidebar_left, sidebar_right) = self
            .active_tab
            .and_then(|idx| self.tabs.get(idx))
            .map(|tab| {
                (
                    self.sidebar_region_shown(tab, SidebarSide::Left),
                    self.sidebar_region_shown(tab, SidebarSide::Right),
                )
            })
            .unwrap_or((false, false));
        self.sidebar_nav_reset();

        let terminal_area: Element<'_, Message> = if let Some(tab_idx) = self.active_tab {
            if let Some(tab) = self.tabs.get(tab_idx) {
                // Render the tab's panes through a `pane_grid`. With one
                // pane this is visually identical to the old single canvas;
                // splits add cells. Each cell gets a focus border (only
                // visible once there's more than one pane) and the grid
                // wires click-to-focus + drag-to-resize.
                let focused = tab.focused;
                let multipane = tab.pane_grid.panes.len() > 1;
                // Which edges of each pane border a sibling. The panes sit
                // flush (no gutter), so the only way to grab a divider is
                // for the pane to hand that strip back to the grid; doing
                // it only on shared edges keeps the grid's outer border
                // fully selectable. Relative coordinates are enough, so
                // the regions are laid out at an arbitrary size.
                // A visible gutter belongs to no pane, so it IS the handle
                // and the panes keep all their pixels. Only when the panes
                // sit flush does each one have to hand a strip back.
                let gap = self.prefs.pane_gap.parse::<f32>().unwrap_or(0.0).clamp(0.0, 24.0);
                let neighbours: std::collections::HashMap<_, _> = if multipane && gap <= 0.0 {
                    const UNIT: f32 = 1000.0;
                    let regions = tab.pane_grid.layout().pane_regions(
                        0.0,
                        0.0,
                        iced::Size::new(UNIT, UNIT),
                    );
                    regions
                        .iter()
                        .map(|(handle, r)| {
                            const EPS: f32 = 0.5;
                            const GRAB: f32 = 4.0;
                            let edge = |touching: bool| if touching { 0.0 } else { GRAB };
                            (
                                *handle,
                                (
                                    edge(r.y <= EPS),
                                    edge(r.x + r.width >= UNIT - EPS),
                                    edge(r.y + r.height >= UNIT - EPS),
                                    edge(r.x <= EPS),
                                ),
                            )
                        })
                        .collect()
                } else {
                    std::collections::HashMap::new()
                };
                // Broadcast input (C2): while armed, every participating pane
                // wears a 2px warning-tinted border so it is unmistakable that
                // keystrokes fan out to all of them at once.
                let broadcast = tab.broadcast;
                let grid = iced::widget::pane_grid(&tab.pane_grid, move |pane, pane_data, _max| {
                    let is_focused = pane == focused;
                    // The outline (focus accent, or the warning tint while
                    // broadcasting) is drawn INSIDE `render_pane_canvas`, as a
                    // layer above the terminal canvas. It used to live here as
                    // this container's border and was invisible: the canvas
                    // fills the container and paints over it (#113). Gated on
                    // `multipane` there, since a lone pane has nothing to be
                    // distinguished from and broadcast is inert on it.
                    iced::widget::pane_grid::Content::new(
                        container(self.render_pane_canvas(
                            pane_data,
                            is_focused,
                            broadcast,
                            multipane,
                            neighbours.get(&pane).copied().unwrap_or((0.0, 0.0, 0.0, 0.0)),
                        ))
                        .width(Length::Fill)
                        .height(Length::Fill),
                    )
                })
                .on_click(|v| Message::Terminal(TerminalMessage::FocusPane(v)))
                // The panes sit FLUSH: no gutter at all (owner call). The
                // divider stays grabbable because each pane declines
                // presses in a 4 px strip along the edges it shares with a
                // sibling (`with_resize_margins`), so the grid gets them
                // and no text selection starts. The leeway is back at 8
                // now that nothing competes for those pixels.
                .on_resize(8, |v| Message::Terminal(TerminalMessage::ResizePane(v)))
                .spacing(if multipane { gap } else { 0.0 })
                .width(Length::Fill)
                .height(Length::Fill);

                // The AI/sidebar toggle now lives in the tab bar (panel
                // button right of `+`), so the terminal canvas no longer
                // carries its own floating sparkle overlay.
                //
                // `spacing` only puts air BETWEEN panes, so a gap alone
                // left the outer panes flush against the window while
                // their shared edges breathed. Matching the outer padding
                // to the gap makes every boundary the same width, which
                // is what "gap" reads as; without it the setting looks
                // half-applied (field report).
                let term_with_toggle: Element<'_, Message> =
                    if multipane && gap > 0.0 {
                        container(grid).padding(gap).into()
                    } else {
                        grid.into()
                    };

                // The session-group editor renders here, as a sibling of the
                // grid inside the terminal area, the same way the chat sidebar
                // does. Wrapping the whole terminal container from outside
                // (view_content) instead left the canvas eating clicks meant
                // for the panel, so keep it inside.
                if sidebar_left || sidebar_right || self.panels.session_group_panel {
                    // Region sides are explicit physical edges (issues
                    // #85 / #102), like the #87 tab-bar dock, so a plain
                    // Row (not dir_row) places them, RTL must not flip a
                    // side the user chose. The session-group editor
                    // keeps its trailing position, outside even the
                    // right region.
                    let mut children: Vec<Element<'_, Message>> = Vec::new();
                    if sidebar_left {
                        children.push(self.view_terminal_sidebar(tab, SidebarSide::Left));
                    }
                    children.push(term_with_toggle);
                    if sidebar_right {
                        children.push(self.view_terminal_sidebar(tab, SidebarSide::Right));
                    }
                    if self.panels.session_group_panel {
                        children.push(self.view_session_group_panel());
                    }
                    iced::widget::Row::with_children(children)
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .into()
                } else {
                    term_with_toggle
                }
            } else {
                container(text(t("no_active_session")).size(14).color(OryxisColors::t().text_muted))
                    .center(Length::Fill).into()
            }
        } else {
            container(text(t("no_active_session")).size(14).color(OryxisColors::t().text_muted))
                .center(Length::Fill).into()
        };

        // The terminal's own backdrop, and the only layer that carries
        // the opacity: the canvas hands its full-bounds fill over to
        // this container (`with_transparent_bg`) precisely so the colour
        // is painted once. Panes, split gutters and the empty
        // no-session area all sit on it, so they fade together instead
        // of one translucent rectangle floating on an opaque plate.
        let backdrop_alpha = self.terminal_backdrop_alpha();
        let base = container(terminal_area)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_| container::Style {
                background: Some(Background::Color(Color {
                    a: backdrop_alpha.unwrap_or(1.0),
                    ..OryxisColors::t().terminal_bg
                })),
                ..Default::default()
            });
        // Floating overlay over the terminal area (shown whether or not
        // the chat sidebar is open): the ZMODEM transfer card. The toast
        // chip is NOT layered here: it mounts at the window root
        // (`root_view.rs`), so notifications raised while the user sits
        // in the Dashboard / Settings views are visible too.
        let mut stack = iced::widget::Stack::new().push(base);
        if let Some(zm) = self.transfer_overlay() {
            stack = stack.push(zm);
        }
        stack.into()
    }

    /// Bottom-center transfer card over the terminal while the active
    /// pane is moving files: a ZMODEM transfer (in-band, PTY diverted)
    /// or an OS-drop SFTP upload (out-of-band, terminal stays live).
    /// One card for both: direction verb, entry name, byte progress, a
    /// bar (when the size is known) and Cancel. `None` when idle. The
    /// two never coexist on a pane (the drop router refuses a second
    /// transfer), so ZMODEM being checked first is not a preference.
    fn transfer_overlay(&self) -> Option<Element<'_, Message>> {
        let pane = self.active_tab.and_then(|i| self.tabs.get(i)).map(|t| t.active())?;
        let pane_id = pane.id;
        // (verb, name, batch, transferred, total, cancel message)
        let (verb, name, batch, transferred, total, cancel_msg) =
            if let Some(zm) = pane.zmodem.as_ref() {
                let verb = match zm.direction {
                    oryxis_zmodem::Direction::Download => t("zmodem_downloading"),
                    oryxis_zmodem::Direction::Upload => t("zmodem_uploading"),
                };
                (
                    verb,
                    zm.file_name.as_deref().unwrap_or("…"),
                    zm.batch,
                    zm.transferred,
                    zm.total,
                    Message::Zmodem(ZmodemMessage::ZmodemCancel(pane_id)),
                )
            } else {
                let up = pane.drop_upload.as_ref()?;
                (
                    t("zmodem_uploading"),
                    up.file_name.as_deref().unwrap_or("…"),
                    up.batch,
                    up.transferred,
                    up.total,
                    Message::Terminal(TerminalMessage::TerminalDropCancel(pane_id)),
                )
            };

        // Multi-file position; numeric, so no i18n needed.
        let batch = batch
            .map(|(k, n)| format!(" ({k}/{n})"))
            .unwrap_or_default();
        let bytes_line = match total {
            Some(total) => format!("{} / {}", fmt_bytes(transferred), fmt_bytes(total)),
            None => fmt_bytes(transferred),
        };
        let header = dir_row(vec![
            text(format!("{verb} {name}{batch}"))
                .size(12)
                .color(OryxisColors::t().text_primary)
                .into(),
            Space::new().width(Length::Fill).into(),
            text(bytes_line).size(11).color(OryxisColors::t().text_muted).into(),
        ])
        .align_y(iced::Alignment::Center);

        let mut body = column![header].spacing(6).width(Length::Fixed(320.0));
        if let Some(total) = total.filter(|t| *t > 0) {
            let frac = (transferred as f32 / total as f32).clamp(0.0, 1.0);
            body = body.push(iced::widget::progress_bar(0.0..=1.0, frac));
        }
        let cancel = button(text(t("cancel")).size(11).color(OryxisColors::t().text_primary))
            .on_press(cancel_msg)
            .padding(Padding { top: 4.0, right: 10.0, bottom: 4.0, left: 10.0 })
            .style(|_, status| {
                let bg = match status {
                    iced::widget::button::Status::Hovered
                    | iced::widget::button::Status::Pressed => OryxisColors::t().bg_hover,
                    _ => OryxisColors::t().bg_surface,
                };
                iced::widget::button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border {
                        radius: Radius::from(6.0),
                        color: OryxisColors::t().border,
                        width: 1.0,
                    },
                    ..Default::default()
                }
            });
        body = body.push(
            container(cancel)
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Right),
        );

        let card = container(body)
            .padding(Padding { top: 10.0, right: 12.0, bottom: 10.0, left: 12.0 })
            .style(|_| container::Style {
                background: Some(Background::Color(Color {
                    a: 0.97,
                    ..OryxisColors::t().bg_selected
                })),
                border: Border {
                    radius: Radius::from(8.0),
                    color: OryxisColors::t().accent,
                    width: 1.0,
                },
                ..Default::default()
            });
        Some(
            container(
                column![
                    Space::new().height(Length::Fill),
                    container(card)
                        .width(Length::Fill)
                        .align_x(iced::alignment::Horizontal::Center),
                    Space::new().height(Length::Fixed(84.0)),
                ]
                .width(Length::Fill)
                .height(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        )
    }

    /// Bottom-center toast chip, or `None` when no toast is pending.
    /// Mounted at the window root (`root_view.rs`) so it floats over
    /// every unlocked view, not just the terminal; the chat sidebar no
    /// longer renders its own copy (that only showed while it was open).
    pub(crate) fn toast_overlay(&self) -> Option<Element<'_, Message>> {
        let text_ = self.toast.as_ref()?;
        let chip = container(
            text(text_.clone()).size(11).color(OryxisColors::t().text_primary),
        )
        .padding(Padding { top: 5.0, right: 12.0, bottom: 5.0, left: 12.0 })
        .style(|_| container::Style {
            background: Some(Background::Color(Color {
                a: 0.95,
                ..OryxisColors::t().bg_selected
            })),
            border: Border {
                radius: Radius::from(8.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        });
        // Clicking the chip dismisses it immediately. Only the chip is
        // interactive; the surrounding Fill stays transparent to clicks so it
        // never steals input from the terminal underneath.
        let chip = MouseArea::new(chip)
            .on_press(Message::ToastDismiss)
            .interaction(iced::mouse::Interaction::Pointer);
        Some(
            container(
                column![
                    Space::new().height(Length::Fill),
                    container(chip)
                        .width(Length::Fill)
                        .align_x(iced::alignment::Horizontal::Center),
                    Space::new().height(Length::Fixed(48.0)),
                ]
                .width(Length::Fill)
                .height(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        )
    }

    /// Build the terminal canvas for one pane, applying the global font /
    /// rendering settings. Shared by every `pane_grid` cell. `is_focused`
    /// gates mouse-tracking reports so a focus-click on an inactive pane
    /// doesn't inject a stray report.
    /// The user's chords for the gestures the terminal widget performs
    /// itself (copy / select-all / scrollback paging, all of which need
    /// canvas state the dispatcher can't reach).
    ///
    /// Hands the widget a matcher instead of a chord table so the
    /// binding model stays in one place: `HotkeyBindings::match_event`
    /// is the same code the router runs, so a rebind can't mean one
    /// thing here and another there. The lists are cloned because
    /// `TerminalView` carries no lifetime; they hold one or two chords
    /// each, so it costs nothing per pane per frame.
    pub(crate) fn terminal_chord_resolver(&self) -> oryxis_terminal::widget::ChordResolver {
        use crate::hotkeys::HotkeyAction::*;
        use oryxis_terminal::widget::TerminalChordAction;
        let get = |a| self.hotkey_bindings.get(&a).cloned().unwrap_or_default();
        let copy = get(TerminalCopy);
        // Shift+Insert by default (the xterm / kitty / Alacritty PRIMARY
        // paste chord). Checked after Copy on purpose: if someone binds
        // both onto the same chord, the non-destructive one should win
        // the tie.
        let paste_selection = get(TerminalPasteSelection);
        let select_all = get(TerminalSelectAll);
        let page_up = get(ScrollbackPageUp);
        let page_down = get(ScrollbackPageDown);
        Box::new(move |key, mods| {
            // Mirror the router's terminal gate: a chord that is a bare
            // control sequence (a user rebinding Copy to Ctrl+C, say)
            // must reach the PTY as its control byte, NOT trigger the
            // widget gesture. Without this the router lets ^C fall
            // through to the PTY (SIGINT) while the widget ALSO copies,
            // so one keystroke both copies and kills the process. The
            // widget only runs with the PTY owning keys, so the gate is
            // unconditional here (no find-bar exemption to weigh).
            let live = |b: &crate::hotkeys::HotkeyBinding| !b.is_terminal_control_sequence();
            if copy.match_event_where(key, mods, live).is_some() {
                Some(TerminalChordAction::Copy)
            } else if paste_selection.match_event_where(key, mods, live).is_some() {
                Some(TerminalChordAction::PasteSelection)
            } else if select_all.match_event_where(key, mods, live).is_some() {
                Some(TerminalChordAction::SelectAll)
            } else if page_up.match_event_where(key, mods, live).is_some() {
                Some(TerminalChordAction::ScrollPageUp)
            } else if page_down.match_event_where(key, mods, live).is_some() {
                Some(TerminalChordAction::ScrollPageDown)
            } else {
                None
            }
        })
    }

    /// The user's MOUSE bindings for the terminal canvas (middle-click
    /// paste out of the box).
    ///
    /// Same contract as `terminal_chord_resolver`: the widget gets a
    /// matcher, not a table, so `HotkeyBinding::match_mouse` stays the
    /// single implementation.
    ///
    /// Which pairs belong here is `HotkeyAction::mouse_binding_owner`,
    /// shared with `shortcuts::dispatch_mouse_binding` so the two can't
    /// both claim a press (it would fire twice) or both decline it.
    /// Declining here leaves the press uncaptured, which is exactly
    /// what lets the global handler pick it up.
    pub(crate) fn terminal_mouse_resolver(&self) -> oryxis_terminal::widget::MouseResolver<Message> {
        use crate::hotkeys::{HotkeyAction, HotkeyBinding, MouseButton};
        use oryxis_terminal::widget::{MouseGesture, TerminalChordAction};
        // Flattened at build time so the closure does one linear scan of
        // (usually) a single entry per press instead of walking the whole
        // action table.
        let bound: Vec<(HotkeyBinding, HotkeyAction)> = HotkeyAction::all()
            .iter()
            .filter_map(|a| self.hotkey_bindings.get(a).map(|binds| (*a, binds)))
            .flat_map(|(a, binds)| binds.mouse_chords().map(move |b| (*b, a)))
            .collect();
        Box::new(move |button, mods| {
            let button = MouseButton::from_iced(button)?;
            let action = bound
                .iter()
                .find(|(b, _)| b.match_mouse(button, mods))
                .map(|(_, a)| *a)?;
            if action.mouse_binding_owner(button) != crate::hotkeys::MouseBindingOwner::Widget {
                return None;
            }
            // The split is `HotkeyAction::widget_dispatched`: those five
            // need canvas state, everything else is the app's to run.
            Some(match action {
                HotkeyAction::TerminalCopy => MouseGesture::Widget(TerminalChordAction::Copy),
                HotkeyAction::TerminalPasteSelection => {
                    MouseGesture::Widget(TerminalChordAction::PasteSelection)
                }
                HotkeyAction::TerminalSelectAll => {
                    MouseGesture::Widget(TerminalChordAction::SelectAll)
                }
                HotkeyAction::ScrollbackPageUp => {
                    MouseGesture::Widget(TerminalChordAction::ScrollPageUp)
                }
                HotkeyAction::ScrollbackPageDown => {
                    MouseGesture::Widget(TerminalChordAction::ScrollPageDown)
                }
                other => MouseGesture::Publish(Message::Tabs(
                    crate::messages::TabsMessage::RunHotkeyAction(other),
                )),
            })
        })
    }

    fn render_pane_canvas<'a>(
        &'a self,
        pane: &'a crate::state::Pane,
        is_focused: bool,
        tab_broadcast: bool,
        multipane: bool,
        // `(top, right, bottom, left)` strips handed back to the grid so
        // its dividers stay grabbable with the panes flush.
        resize_margins: (f32, f32, f32, f32),
    ) -> Element<'a, Message> {
        // Resolved once for this pane's tab: the picture (if any) and the
        // translucent-backdrop alpha both decide who paints the base fill.
        let appearance = self.active_terminal_appearance();
        let mut term_view = TerminalView::new(Arc::clone(&pane.terminal))
            .focused(is_focused)
            .with_bell_flash(pane.bell_flash)
            .with_font_size(self.terminal_font_size)
            .with_font_name(&self.terminal_font_name)
            .with_font_weight(self.terminal_font_weight.font_weight())
            .with_text_dilation(self.terminal_text_thickness.px())
            .with_copy_on_select(self.prefs.copy_on_select)
            .with_right_click_copy(self.prefs.right_click_copy)
            .with_terminal_chords(self.terminal_chord_resolver())
            .with_mouse_bindings(self.terminal_mouse_resolver())
            .with_right_click_action(self.prefs.terminal_right_click.to_widget())
            // The keypress half of the pair is queued on the input funnel
            // (`write_bytes_to_pane`), not here: see issue #111.
            .with_reset_scroll_on_output(self.prefs.scrollback_reset_output)
            .with_bold_is_bright(self.prefs.bold_is_bright)
            .with_keyword_highlight(self.prefs.keyword_highlight)
            // Resolved for this pane's host, the same set its backend
            // watches with: a rule must never colour one pattern while
            // firing on another.
            .with_highlight_rules(self.highlight_rules_for(pane.saved_conn_id()))
            .with_performance(self.prefs.performance_mode)
            .with_perf_overlay(self.prefs.perf_overlay)
            .with_privacy(self.privacy_active_for_label(&pane.label))
            .with_privacy_terms(&self.privacy_terms())
            .with_privacy_classes(self.privacy_classes())
            .with_smart_contrast(self.prefs.smart_contrast)
            // The backdrop is painted once, by whoever sits behind this
            // canvas: the container in `view_terminal` (translucent
            // terminal) or the per-pane `Backdrop` canvas stacked below
            // (background picture; it must be a separate canvas because
            // images always render above every fill within one layer, so
            // a picture drawn in the grid's own frame would bury the
            // selection, the cursor and every cell background).
            .with_transparent_bg(appearance.alpha.is_some() || appearance.image.is_some())
            // C5: a host with `disable_mouse_reporting` keeps clicks local
            // even when the remote turns on mouse tracking.
            .with_mouse_reporting(!pane.quirks.disable_mouse_reporting)
            .with_word_delimiters(&self.prefs.word_delimiters)
            .with_resize_margins(resize_margins)
            .on_font_size_increase(Message::Settings(SettingsMessage::TerminalFontSizeIncrease))
            .on_font_size_decrease(Message::Settings(SettingsMessage::TerminalFontSizeDecrease))
            .on_paste_request(Message::Terminal(TerminalMessage::TerminalPasteFromClipboard))
            // Captures THIS pane's id so the paste can't land in another
            // pane if focus moves between the keystroke and the update.
            .on_paste_selection({
                let pane_id = pane.id;
                move |text| {
                    Message::Terminal(TerminalMessage::TerminalPasteSelection(pane_id, text.into()))
                }
            })
            .on_terminal_input(|v| Message::Terminal(TerminalMessage::TerminalInput(v)))
            // Ctrl+click hands the target here rather than opening it in
            // the widget: only the app knows whether this pane is remote
            // (so the link is confirmed first) and which SSH connection a
            // loopback callback in it has to be tunnelled through. Carries
            // THIS pane's id for the same reason the paste hooks do.
            .on_link_activate({
                let pane_id = pane.id;
                move |url| {
                    Message::Terminal(TerminalMessage::TerminalLinkActivated(pane_id, url))
                }
            });
        // The perf HUD's `net` row: link quality from the SSH session's
        // RTT probe window. Only sampled while the HUD can render it, so
        // the per-frame snapshot lock costs nothing when it's off. The
        // env-var check mirrors the widget's own `ORYXIS_TERM_PERF`
        // force-on so the forced HUD isn't missing its net row.
        let hud_on = self.prefs.perf_overlay
            || std::env::var("ORYXIS_TERM_PERF").is_ok_and(|v| !v.is_empty() && v != "0");
        if hud_on && let Some(ssh) = pane.session.as_ref().and_then(|t| t.ssh()) {
            let q = ssh.net_quality();
            let ms = |d: std::time::Duration| d.as_secs_f32() * 1000.0;
            term_view = term_view.with_net_hud(Some(oryxis_terminal::NetHud {
                rtt_ms: q.last_rtt.map(ms),
                avg_rtt_ms: q.avg_rtt.map(ms),
                peak_rtt_ms: q.peak_rtt.map(ms),
                jitter_ms: q.jitter.map(ms),
                lost: q.timeouts,
                silent_for_secs: q.silent_for.map(|d| d.as_secs_f32()),
            }));
        }
        // Context menu (right-click scheme = Menu): carry the clicked
        // pane's id so Copy All / Clear Scrollback target the right pane,
        // not just the focused one.
        if self.prefs.terminal_right_click == crate::util::RightClickMode::Menu {
            let pane_id = pane.id;
            term_view = term_view.on_context_menu(move |x, y, sel| {
                Message::Terminal(TerminalMessage::ShowTerminalContextMenu(pane_id, x, y, sel))
            });
        }
        // Wire the teaching hints only while they should still show for
        // this pane, so the widget stops emitting once HintMode::Once has
        // retired them (and never emits under Never).
        if self.prefs.hint_mode.should_show(pane.mouse_hint_shown) {
            term_view = term_view.on_mouse_capture_hint(|| Message::Terminal(TerminalMessage::TerminalMouseCaptureHint));
        }
        if self.prefs.hint_mode.should_show(pane.link_hint_shown) {
            term_view = term_view.on_link_click_hint(|| Message::Terminal(TerminalMessage::TerminalLinkClickHint));
        }
        // Wrap the canvas so the focused pane asks the OS to enable its IME.
        // The terminal is a canvas (not a text_input), so without this winit
        // keeps the IME disabled and CJK input can't be switched on.
        let term_canvas = canvas(term_view)
            .width(Length::Fill)
            .height(Length::Fill);
        // Background picture: its own canvas UNDER the grid, per pane so a
        // split lays out one copy in each half. `Stack` gives the grid its
        // own render layer above it, which is what keeps the grid's fills
        // (selection, cursor, cell backgrounds, fade) over the picture and
        // lets the fade actually show (see `oryxis_terminal::Backdrop`).
        let term_canvas: Element<'a, Message> = if let Some(image) = appearance.image {
            iced::widget::Stack::new()
                .push(
                    canvas(oryxis_terminal::Backdrop::new(
                        Arc::clone(&pane.terminal),
                        image,
                    ))
                    .width(Length::Fill)
                    .height(Length::Fill),
                )
                .push(term_canvas)
                .into()
        } else {
            term_canvas.into()
        };
        let host = crate::widgets::ime_host(
            term_canvas,
            is_focused,
            Arc::clone(&pane.terminal),
            self.terminal_font_size,
            self.terminal_font_name.clone(),
            self.terminal_font_weight.font_weight(),
        );
        // Report this pane's drawn rect so the OS-drop router can find
        // the pane under the cursor (a split tab can host different
        // hosts, so "the focused pane" is not always the drop target).
        let host = crate::widgets::bounds_reporter(host, pane.bounds.clone());
        // Top-right overlays over the live canvas. The find-bar (C1) takes
        // the corner while open; otherwise a broadcast chip (C2) sits there
        // whenever the tab is armed, showing this pane's participate / muted
        // state and toggling it on click.
        let overlay: Option<Element<'a, Message>> = if pane.search_open && is_focused {
            Some(self.terminal_find_bar(pane))
        } else if tab_broadcast {
            Some(self.broadcast_chip(pane))
        } else {
            None
        };
        // Bottom-leading link-reveal chip (C3): the OSC 8 target under the
        // pointer, exposed before Ctrl+click so a spoofed label (target !=
        // visible text) can't phish the click. Read non-blocking, a PTY
        // output burst holds this same lock, and a blocking read would hitch
        // the paint for every pane every frame; a skipped frame is invisible.
        let link_chip: Option<Element<'a, Message>> = pane
            .terminal
            .try_lock()
            .ok()
            .and_then(|s| s.hovered_link.clone())
            .map(|link| self.link_reveal_chip(&pane.label, link));
        // Pane outline (#113). It has to be drawn ON TOP of the canvas,
        // not as the enclosing container's border: the canvas fills its
        // parent and paints its own background across the whole rect, so
        // a border on the container underneath is painted over and the
        // focused pane ends up with no marking at all, which is exactly
        // what the report says. Padding the container instead would
        // inset the canvas, and that recomputes the terminal's rows and
        // columns, so merely FOCUSING a pane would resize the PTY.
        //
        // Non-interactive (a plain container over a Space), so it cannot
        // eat the press that moves focus to this pane.
        // Off means the FOCUSED pane still draws (it is the only marker
        // left once the panes sit flush); only the siblings go bare, so
        // the ring is still built whenever the tab is split.
        let ring: Option<Element<'a, Message>> = multipane.then(|| {
            let participating = tab_broadcast && !pane.broadcast_opt_out;
            // Broadcast is the louder signal and wins: keystrokes going
            // to several hosts at once matters more than which pane the
            // caret is in.
            let (color, width) = if participating {
                (OryxisColors::t().warning, 2.0)
            } else if is_focused {
                (OryxisColors::t().accent, 2.0)
            } else if self.prefs.pane_border_inactive {
                (OryxisColors::t().border, 1.0)
            } else {
                // Setting off: the focused pane still gets its accent
                // (with the panes flush, nothing else says where the
                // focused one ends), the rest go bare.
                (Color::TRANSPARENT, 0.0)
            };
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(move |_| container::Style {
                    border: Border { color, width, radius: Radius::from(0.0) },
                    ..Default::default()
                })
                .into()
        });
        // The end-of-session card (issue #208). Not gated on `multipane`:
        // a local shell in a tab of its own raises `ended` too, and it is
        // the pane with the least other recourse, since no relabel or
        // auto-reconnect covers a local tab. `note_pane_ended` is the one
        // that decides which panes get here.
        let ended_card: Option<Element<'a, Message>> =
            pane.ended.then(|| self.pane_ended_card(pane));
        if overlay.is_none() && link_chip.is_none() && ring.is_none() && ended_card.is_none() {
            return host;
        }
        let mut stack = iced::widget::Stack::new().push(host);
        // Under the chips: a 2 px ring at the edges and a padded chip in
        // the corner do not overlap, and keeping the chips last means a
        // future wider ring can never cover the find bar.
        if let Some(ring) = ring {
            stack = stack.push(ring);
        }
        if let Some(top) = overlay {
            stack = stack.push(
                container(top)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Right)
                    .align_y(iced::alignment::Vertical::Top)
                    .padding(Padding::from([6.0, 10.0])),
            );
        }
        if let Some(chip) = link_chip {
            stack = stack.push(
                container(chip)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(crate::widgets::dir_align_x())
                    .align_y(iced::alignment::Vertical::Bottom)
                    .padding(Padding::from([6.0, 10.0])),
            );
        }
        // Last, so nothing layers over the only controls a dead pane has.
        if let Some(card) = ended_card {
            stack = stack.push(
                container(card)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            );
        }
        stack.into()
    }

    /// Bottom-leading reveal chip (C3) for the OSC 8 hyperlink under the
    /// pointer. An OSC 8 link's visible label need not match its target, so
    /// the chip exposes the actual destination before the user Ctrl+clicks
    /// (browser status-bar convention). A blocked-scheme link shows a
    /// "link type not allowed" notice instead of the target, matching the
    /// widget's suppressed pointer / underline. Under Privacy Mode the target
    /// is redacted like session logs are, since a URI can embed `user@host`
    /// or an IP. Display-only (no button), so it stays click-through over the
    /// canvas.
    fn link_reveal_chip<'a>(
        &self,
        pane_label: &str,
        link: oryxis_terminal::HoveredLink,
    ) -> Element<'a, Message> {
        let colors = OryxisColors::t();
        let (label, fg) = if link.allowed {
            let shown = if self.privacy_active_for_label(pane_label) {
                crate::widgets::redact_for_display(&link.target, &self.privacy_terms(), self.privacy_classes())
            } else {
                link.target.clone()
            };
            (truncate_middle(&shown, 80), colors.text_primary)
        } else {
            // Attacker-controlled scheme, shown as-is but capped so a hostile
            // server can't blow up the chip; the target itself is withheld.
            let scheme: String =
                link.target.split(':').next().unwrap_or("").chars().take(16).collect();
            (t("link_target_blocked").replace("{scheme}", &scheme), colors.warning)
        };
        container(
            text(label)
                .size(12)
                .color(fg)
                .align_x(iced::alignment::Horizontal::Left),
        )
        .padding(Padding::from([4.0, 8.0]))
        .style(move |_| container::Style {
            background: Some(Background::Color(colors.bg_surface)),
            border: Border {
                radius: Radius::from(6.0),
                width: 1.0,
                color: colors.border,
            },
            ..Default::default()
        })
        .into()
    }

    /// The card a pane wears once its session has ended (issue #208):
    /// what happened, and the two answers a tab-wide reconnect cannot
    /// give one pane. `note_pane_ended` decides which panes get here.
    ///
    /// Restart is offered only where there is something to restart: a
    /// saved host, a quick-connect entry or a local shell, all of which
    /// the pane's `PaneOrigin` names. A cloud pane (`Ephemeral`) gets
    /// Close alone rather than a button that would toast an apology.
    ///
    /// It sits over the pane's own canvas, so the scrollback the user
    /// disconnected on stays readable behind it, and it is deliberately
    /// not full-bleed for the same reason.
    fn pane_ended_card<'a>(&self, pane: &'a crate::state::Pane) -> Element<'a, Message> {
        let colors = OryxisColors::t();
        let pane_id = pane.id;
        let restartable = match &pane.origin {
            crate::state::PaneOrigin::Host(id) => {
                self.connections.iter().any(|c| c.id == *id)
            }
            crate::state::PaneOrigin::QuickHost(id) => self.quick_connects.contains_key(id),
            crate::state::PaneOrigin::Local(_) => true,
            crate::state::PaneOrigin::Ephemeral => false,
        };
        // Collected first and handed to `dir_row` in one call: it
        // reverses its children AT CONSTRUCTION, so a row built empty
        // and pushed into afterwards keeps physical order and never
        // mirrors under RTL.
        let mut actions: Vec<Element<'a, Message>> = Vec::with_capacity(2);
        if restartable {
            actions.push(crate::widgets::styled_button(
                t("pane_ended_restart"),
                Message::Terminal(TerminalMessage::RestartPane(pane_id)),
                colors.accent,
            ));
        }
        actions.push(crate::widgets::styled_button(
            t("pane_ended_close"),
            Message::Terminal(TerminalMessage::ClosePane(Some(pane_id))),
            colors.bg_hover,
        ));
        let buttons = crate::widgets::dir_row(actions).spacing(8);
        let card = container(
            iced::widget::column![
                text(t("pane_ended")).size(13).color(colors.text_primary),
                Space::new().height(10),
                buttons,
            ]
            .align_x(iced::Alignment::Center)
            .padding(Padding::from([14.0, 18.0])),
        )
        .style(move |_| container::Style {
            background: Some(Background::Color(colors.bg_surface)),
            border: Border {
                radius: Radius::from(10.0),
                width: 1.0,
                color: colors.border,
            },
            ..Default::default()
        });
        // Swallow presses that miss the buttons, so a click aimed at the
        // card never falls through to the canvas and starts a selection
        // in the dead pane's scrollback.
        MouseArea::new(card).on_press(Message::NoOp).into()
    }

    /// Broadcast opt-out chip (C2): a small button in the pane's top-right
    /// corner while its tab is armed. Shows `radio` (participating, warning
    /// tint) or `volume_x` (muted, dimmed) and toggles this pane's
    /// participation on click.
    fn broadcast_chip<'a>(&self, pane: &'a crate::state::Pane) -> Element<'a, Message> {
        let muted = pane.broadcast_opt_out;
        let (glyph, color, tip) = if muted {
            (iced_fonts::lucide::volume_x(), OryxisColors::t().text_muted, t("broadcast_pane_unmute"))
        } else {
            (iced_fonts::lucide::radio(), OryxisColors::t().warning, t("broadcast_pane_mute"))
        };
        let pane_id = pane.id;
        let btn = button(
            container(glyph.size(13).color(color))
                .center_x(Length::Fixed(26.0))
                .center_y(Length::Fixed(22.0)),
        )
        .padding(0)
        .on_press(Message::Terminal(TerminalMessage::TogglePaneBroadcastOptOut(pane_id)))
        .style(move |_, status| {
            let bg = match status {
                BtnStatus::Hovered | BtnStatus::Pressed => OryxisColors::t().bg_hover,
                _ => OryxisColors::t().bg_surface,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border {
                    radius: Radius::from(6.0),
                    width: 1.0,
                    color: OryxisColors::t().border,
                },
                ..Default::default()
            }
        });
        icon_tooltip(btn.into(), tip)
    }

    /// The scrollback find-bar row (C1): needle input + `N / M` counter +
    /// prev / next / close. The match set lives on the widget's
    /// `TerminalState.search`; the counter is read without blocking here.
    fn terminal_find_bar<'a>(&'a self, pane: &'a crate::state::Pane) -> Element<'a, Message> {
        let colors = OryxisColors::t();
        // Read non-blocking like the link chip: a PTY output burst holds
        // this same lock, and a blocking read would hitch the paint. On
        // contention skip the counter this frame (a skipped frame is
        // invisible) rather than showing a misleading "no matches".
        let count = pane.terminal.try_lock().ok().map(|s| s.search_count());
        let count_label = match count {
            Some(Some((cur, total))) if total > 0 => t("terminal_search_count")
                .replace("{current}", &cur.to_string())
                .replace("{total}", &total.to_string()),
            Some(_) if !pane.search_query.is_empty() => t("terminal_search_no_matches").to_string(),
            _ => String::new(),
        };
        // Fit the bar to the pane it floats in. It used to be all fixed
        // widths, so a narrow pane clipped it from the trailing edge and
        // took the step arrows and the CLOSE button with it, leaving the
        // search stuck open with no way out but the hotkey (field report).
        //
        // The pane's real width comes from the same `bounds` cell the
        // OS-drop router reads, so this needs no layout guessing. Order of
        // sacrifice, least useful first: the match counter, then the step
        // arrows, then the input shrinks to a stub. Close is never dropped
        // -- it is the way out.
        const BTN: f32 = 32.0;
        const COUNTER_W: f32 = 70.0;
        const CHROME: f32 = 44.0; // overlay padding + container padding + gaps
        let pane_w = pane.bounds.get().width;
        // A pane that has never drawn reports 0; assume there is room
        // rather than rendering a stub on the first frame.
        let budget = if pane_w > 0.0 { pane_w - CHROME } else { f32::MAX };
        let show_steps = budget >= 80.0 + COUNTER_W + BTN * 3.0;
        let show_counter = budget >= 120.0 + COUNTER_W + BTN * 3.0;
        let buttons_w = if show_steps { BTN * 3.0 } else { BTN };
        let input_w = (budget - buttons_w - if show_counter { COUNTER_W } else { 0.0 })
            .clamp(60.0, 200.0);
        let input = text_input(t("terminal_search_placeholder"), &pane.search_query)
            .id(iced::widget::Id::new("terminal-buffer-search"))
            .on_input(|v| Message::Terminal(TerminalMessage::TerminalSearchInput(v)))
            .width(Length::Fixed(input_w))
            .padding(6);
        let mut items: Vec<Element<'_, Message>> = vec![input.into()];
        if show_counter {
            items.push(
                container(
                    text(count_label)
                        .size(12)
                        .color(colors.text_muted)
                        .width(Length::Fixed(COUNTER_W)),
                )
                .center_y(Length::Fixed(28.0))
                .into(),
            );
        }
        if show_steps {
            items.push(icon_tooltip(
                chat_header_btn(iced_fonts::lucide::chevron_up(), Message::Terminal(TerminalMessage::TerminalSearchStep(false))),
                t("terminal_search_prev"),
            ));
            items.push(icon_tooltip(
                chat_header_btn(
                    iced_fonts::lucide::chevron_down(),
                    Message::Terminal(TerminalMessage::TerminalSearchStep(true)),
                ),
                t("terminal_search_next"),
            ));
        }
        items.push(icon_tooltip(
            chat_header_btn(iced_fonts::lucide::x(), Message::Terminal(TerminalMessage::TerminalSearchClose)),
            t("terminal_search_close"),
        ));
        let controls = dir_row(items)
            .spacing(4)
            .align_y(iced::Alignment::Center);
        container(controls)
            .padding(6)
            .style(move |_| container::Style {
                background: Some(Background::Color(colors.bg_surface)),
                border: Border {
                    radius: Radius::from(8.0),
                    width: 1.0,
                    color: colors.border,
                },
                ..Default::default()
            })
            .into()
    }

    pub(crate) fn view_terminal_sidebar<'a>(
        &'a self,
        tab: &'a TerminalTab,
        side: crate::state::SidebarSide,
    ) -> Element<'a, Message> {
        use crate::state::TerminalSidebarTab as STab;
        // The per-frame recording reset happens once in `view_terminal`,
        // BEFORE either region renders: each tab body records its
        // keyboard rows while rendering, so a stale list from the
        // previous frame (or the other region's reset) must never wipe
        // rows recorded this frame.
        //
        // The strip shows the tabs docked to THIS region that pass
        // their gates (Chat needs AI enabled; Files / Monitor / Tmux
        // need a live SSH session plus their feature toggles; an
        // unavailable tab is hidden rather than disabled, mirroring
        // the pre-#102 strip). `sidebar_region_shown` gates the call
        // on a non-empty region, and `sidebar_region_tab` re-resolves
        // the remembered active tab against the same offers.
        let region_tabs = self.sidebar_region_tabs(side);
        let Some(active) = self.sidebar_region_tab(side) else {
            return Space::new().into();
        };

        // ── Tab strip ──
        // Icon tabs on the leading edge; contextual Reset (Chat only) and
        // the Close X on the trailing edge, same affordance as the chrome.
        let mut strip: Vec<Element<'_, Message>> = Vec::new();
        for region_tab in region_tabs {
            strip.push(sidebar_tab_btn(
                sidebar_tab_icon(region_tab),
                active == region_tab,
                Message::Ai(AiMessage::SelectTerminalSidebarTab(region_tab)),
                t(region_tab.label_key()),
            ));
        }
        strip.push(Space::new().width(Length::Fill).into());
        // The trailing header actions (Reset on Chat, Close always) join
        // the Tab walk, recorded FIRST (the strip renders above every tab
        // body) under the active tab's tag. The tab icons stay off
        // the walk: the FocusSidebarList hotkey already cycles them.
        if active == STab::Chat {
            strip.push(self.sidebar_nav_slot(
                crate::keynav::SidebarRow::button(Message::Ai(AiMessage::ChatResetConversation))
                    .chrome(),
                active,
                6.0,
                icon_tooltip(
                    chat_header_btn(
                        iced_fonts::lucide::rotate_ccw(),
                        Message::Ai(AiMessage::ChatResetConversation),
                    ),
                    t("chat_reset_tip"),
                ),
            ));
            strip.push(Space::new().width(4).into());
        }
        strip.push(self.sidebar_nav_slot(
            crate::keynav::SidebarRow::button(Message::Ai(AiMessage::ToggleSidebarRegion(side)))
                .chrome(),
            active,
            6.0,
            icon_tooltip(
                chat_header_btn(
                    iced_fonts::lucide::x(),
                    Message::Ai(AiMessage::ToggleSidebarRegion(side)),
                ),
                t("close"),
            ),
        ));

        let header = container(
            dir_row(strip)
                .width(Length::Fill)
                .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 8.0, right: 8.0, bottom: 8.0, left: 8.0 })
        .width(Length::Fill);

        let header_separator = container(Space::new().height(1))
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().border)),
                ..Default::default()
            });

        // 4 px draggable handle on the left edge, clicking starts a
        // resize, the global mouse-move handler in app.rs follows the
        // cursor, and the global mouse-up stops the drag.
        let resize_handle: Element<'_, Message> = MouseArea::new(
            container(Space::new().width(Length::Fixed(4.0)).height(Length::Fill))
                .width(Length::Fixed(4.0))
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().border)),
                    ..Default::default()
                }),
        )
        .on_press(Message::Ai(AiMessage::ChatSidebarResizeStart(side)))
        .interaction(iced::mouse::Interaction::ResizingHorizontally)
        .into();

        // ── Assemble sidebar ──
        // Tab bodies are built lazily inside the match: building an
        // inactive tab's body would record its keyboard rows into the
        // per-frame sidebar recording (see `sidebar_nav_reset` above).
        let content: Element<'_, Message> = match active {
            STab::Chat => self.chat_tab_body(tab),
            STab::Snippets => self.snippets_tab_content(),
            STab::History => self.history_tab_content(),
            STab::Files => self.files_tab_content(tab),
            STab::Monitor => self.monitor_tab_content(),
            STab::Tmux => self.tmux_tab_content(tab),
            STab::HostConfig => self.host_config_tab_content(tab),
            STab::HostsTree => self.hosts_tree_tab_content(),
        };
        let panel_column = column![header, header_separator, content]
            .width(Length::Fill)
            .height(Length::Fill);

        // The toast now floats over the whole terminal view (see
        // `view_terminal` / `toast_overlay`), not just this sidebar, so it
        // shows even when the chat panel is closed.
        let panel = container(panel_column)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_primary)),
            ..Default::default()
        });

        // The 4 px drag handle sits on the INNER edge (the one facing
        // the terminal): the left region's right, the right region's
        // left (issues #85 / #102). A physical placement, like the
        // region itself: plain row!, never dir_row.
        let handle_and_panel: iced::widget::Row<'_, Message> = match side {
            crate::state::SidebarSide::Left => row![panel, resize_handle],
            crate::state::SidebarSide::Right => row![resize_handle, panel],
        };
        container(handle_and_panel.width(Length::Fill).height(Length::Fill))
            .width(Length::Fixed(self.chat_ui.sidebar_width[side.idx()]))
            .height(Length::Fill)
            .into()
    }

    /// Chat tab body: the message list, the floating Stop pill, the
    /// Plan / Ask / Auto mode picker and the message editor. Split out
    /// of `view_terminal_sidebar` so it only renders (and records its
    /// keyboard rows) when the Chat tab is the active one.
    fn chat_tab_body<'a>(&'a self, tab: &'a TerminalTab) -> Element<'a, Message> {
        // ── Messages list ──
        let mut messages_col = column![].spacing(8).padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 });

        if tab.chat_history.is_empty() {
            messages_col = messages_col.push(
                container(
                    column![
                        iced_fonts::lucide::sparkles().size(24).color(OryxisColors::t().text_muted),
                        Space::new().height(8),
                        text(t("ask_ai_session")).size(12).color(OryxisColors::t().text_muted),
                    ]
                    .align_x(iced::Alignment::Center),
                )
                .center_x(Length::Fill)
                .padding(Padding { top: 40.0, right: 0.0, bottom: 0.0, left: 0.0 }),
            );
        } else {
            // Markdown settings are identical for every assistant
            // bubble, so build them once per sidebar render instead of
            // re-deriving the style from the theme per message.
            let md_settings = self.chat_markdown_settings();
            for msg in &tab.chat_history {
                // Skip empty assistant placeholders, they exist as
                // staging slots for streaming chunks; an empty one is
                // either pre-first-token (covered by the "Thinking..."
                // bubble below) or a stream that ended before any text
                // arrived (e.g. straight to a tool call). Either way,
                // an empty padded box would just look like a glitch.
                if msg.role == crate::state::ChatRole::Assistant
                    && msg.content.is_empty()
                {
                    continue;
                }
                let bubble = self.view_chat_message(msg, md_settings);
                messages_col = messages_col.push(bubble);
            }
        }

        // Hide the "Thinking..." indicator once the model has started
        // streaming visible text, the streaming bubble itself is the
        // signal of activity, and showing both reads as a stutter.
        let actively_streaming = tab
            .chat_history
            .last()
            .map(|m| m.role == crate::state::ChatRole::Assistant && !m.content.is_empty())
            .unwrap_or(false);
        if tab.chat_loading && !actively_streaming {
            messages_col = messages_col.push(
                container(
                    text(t("thinking")).size(12).color(OryxisColors::t().text_muted),
                )
                .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().bg_surface)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                }),
            );
        }

        let messages_scroll = scrollable(messages_col)
            .id(iced::widget::Id::new("chat-scroll"))
            .on_scroll(|viewport| Message::Ai(AiMessage::ChatScrolled(viewport.relative_offset().y)))
            .width(Length::Fill)
            .height(Length::Fill);

        // ── Input area ──
        let input_separator = container(Space::new().height(1))
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().border)),
                ..Default::default()
            });

        // Multi-line input, grows with content up to ~6 lines (~150 px),
        // then scrolls internally. Enter sends the message; Shift+Enter
        // inserts a newline. No send button, every chat-style UI uses
        // Enter today, so the arrow was just visual noise.
        let chat_editor = iced::widget::text_editor(&self.chat_ui.input)
            // Programmatic focus target for the FocusSidebarList hotkey's
            // Chat stop (the fork's text_editor is operation::Focusable).
            .id(iced::widget::Id::new("chat-input"))
            .placeholder(t("ask_ai"))
            .on_action(|v| Message::Ai(AiMessage::ChatInputAction(v)))
            .padding(10)
            .height(Length::Shrink)
            .key_binding(|key_press| {
                use iced::keyboard::{key::Named, Key};
                use iced::widget::text_editor::{Binding, KeyPress};
                let KeyPress { key, modifiers, .. } = &key_press;
                if matches!(key, Key::Named(Named::Enter)) && !modifiers.shift() {
                    return Some(Binding::Custom(Message::Ai(AiMessage::SendChat)));
                }
                Binding::from_key_press(key_press)
            })
            .style(|_theme, status| {
                let c = OryxisColors::t();
                let (border_color, border_width) = match status {
                    iced::widget::text_editor::Status::Focused { .. } => (c.accent, 1.5),
                    _ => (c.border, 1.0),
                };
                iced::widget::text_editor::Style {
                    background: Background::Color(c.bg_surface),
                    border: Border {
                        radius: Radius::from(crate::widgets::INPUT_RADIUS),
                        width: border_width,
                        color: border_color,
                    },
                    placeholder: c.text_muted,
                    value: c.text_primary,
                    selection: c.accent,
                }
            });

        // Plan / Ask / Auto picker, sitting just above the input so the
        // active mode is visible while typing. Reflects (and sets) THIS
        // tab's mode. Recorded as a picker row (Left/Right cycle the
        // modes) BEFORE the editor, matching the display order.
        let mode_row = {
            use crate::state::ChatMode;
            let (prev, next) = crate::keynav::slots::cycle_pair(
                &[ChatMode::Plan, ChatMode::Ask, ChatMode::Auto],
                &tab.chat_mode,
                |v| Message::Ai(AiMessage::ChatModeChanged(v)),
            );
            container(
                dir_row(vec![self.sidebar_nav_slot(
                    crate::keynav::SidebarRow::picker(prev, next),
                    crate::state::TerminalSidebarTab::Chat,
                    6.0,
                    crate::views::sidebar_chat::chat_mode_picker(tab.chat_mode),
                )])
                .width(Length::Fill)
                .align_y(iced::Alignment::Center),
            )
            .padding(Padding { top: 6.0, right: 12.0, bottom: 0.0, left: 12.0 })
            .width(Length::Fill)
            .align_x(crate::widgets::dir_align_x())
        };

        // The editor is an input row in the sidebar Tab walk (real
        // focus via its "chat-input" id; its own key_binding keeps
        // Enter = send).
        let input_row = container(
            self.sidebar_nav_slot(
                crate::keynav::SidebarRow::input(iced::widget::Id::new("chat-input")),
                crate::state::TerminalSidebarTab::Chat,
                crate::widgets::INPUT_RADIUS,
                container(chat_editor).height(Length::Shrink.max(150.0)).into(),
            ),
        )
        .padding(Padding { top: 8.0, right: 12.0, bottom: 12.0, left: 12.0 })
        .width(Length::Fill);

        // Persistent reminder that the assistant runs commands on the
        // live server (some auto-execute), sitting just above the input.
        let chat_disclaimer = container(
            text(t("ai_chat_disclaimer"))
                .size(10)
                .color(OryxisColors::t().text_muted),
        )
        .padding(Padding { top: 6.0, right: 12.0, bottom: 0.0, left: 12.0 })
        .width(Length::Fill)
        .align_x(crate::widgets::dir_align_x());

        // While a chat task is in flight (streaming a reply or auto-running
        // a tool chain) offer an explicit Stop, floating over the bottom of
        // the message list (not inline) so it stays reachable without pushing
        // the conversation around. It aborts the live task so a runaway tool
        // loop can be halted by hand, without closing the panel. Per-tab:
        // shown only when THIS tab has work in flight.
        let stop_overlay: Option<Element<'_, Message>> = tab.chat_task.is_some().then(|| {
            let pill = button(
                dir_row(vec![
                    iced_fonts::lucide::circle_stop()
                        .size(12)
                        .color(OryxisColors::t().text_primary)
                        .into(),
                    text(t("chat_stop"))
                        .size(11)
                        .color(OryxisColors::t().text_primary)
                        .into(),
                ])
                .spacing(6)
                .align_y(iced::Alignment::Center),
            )
            .padding(Padding { top: 5.0, right: 14.0, bottom: 5.0, left: 14.0 })
            .on_press(Message::Ai(AiMessage::ChatStop))
            .style(|_, status| {
                let c = OryxisColors::t();
                let bg = match status {
                    BtnStatus::Hovered => c.button_bg_hover,
                    _ => c.button_bg,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    text_color: c.text_primary,
                    border: Border {
                        radius: Radius::from(16.0),
                        width: 1.0,
                        color: c.border,
                    },
                    // A soft shadow lifts the pill off the messages behind it.
                    shadow: iced::Shadow {
                        color: Color { a: 0.25, ..Color::BLACK },
                        offset: iced::Vector::new(0.0, 2.0),
                        blur_radius: 8.0,
                    },
                    ..Default::default()
                }
            });
            // Pin to bottom-center of the message area, floating above the
            // separator/input.
            container(pill)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Bottom)
                .padding(Padding { top: 0.0, right: 0.0, bottom: 10.0, left: 0.0 })
                .into()
        });

        // Base is the scrollable message list; the Stop pill (when present)
        // floats over its bottom edge via a Stack.
        let messages_area: Element<'_, Message> = match stop_overlay {
            Some(overlay) => iced::widget::Stack::new()
                .push(messages_scroll)
                .push(overlay)
                .into(),
            None => messages_scroll.into(),
        };

        column![messages_area, input_separator, mode_row, chat_disclaimer, input_row]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

/// One icon tab in the sidebar's tab strip. Active tab gets an accent
/// glyph on a faint accent wash; inactive is muted and transparent.
fn sidebar_tab_btn<'a>(
    icon: iced::widget::Text<'a>,
    active: bool,
    msg: Message,
    tip: &'a str,
) -> Element<'a, Message> {
    let color = if active { OryxisColors::t().accent } else { OryxisColors::t().text_muted };
    let btn = button(
        container(icon.size(15).color(color))
            .center_x(Length::Fixed(34.0))
            .center_y(Length::Fixed(28.0)),
    )
    .padding(0)
    .on_press(msg)
    .style(move |_, status| {
        // Selected tab keeps its accent tint; an unselected tab fills with
        // bg_hover on hover/press for clear pointer feedback.
        let bg = if active {
            Color { a: 0.15, ..OryxisColors::t().accent }
        } else {
            match status {
                BtnStatus::Hovered | BtnStatus::Pressed => OryxisColors::t().bg_hover,
                _ => Color::TRANSPARENT,
            }
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    });
    icon_tooltip(btn.into(), tip)
}

/// Lucide glyph for a sidebar tab's strip button. One table, so a
/// tab's icon can't drift between the two region strips.
fn sidebar_tab_icon<'a>(tab: crate::state::TerminalSidebarTab) -> iced::widget::Text<'a> {
    use crate::state::TerminalSidebarTab as STab;
    match tab {
        STab::Chat => iced_fonts::lucide::sparkles(),
        STab::Snippets => iced_fonts::lucide::code(),
        STab::History => iced_fonts::lucide::history(),
        STab::Files => iced_fonts::lucide::folder(),
        STab::Monitor => iced_fonts::lucide::activity(),
        STab::Tmux => iced_fonts::lucide::layout_grid(),
        STab::HostConfig => iced_fonts::lucide::cog(),
        STab::HostsTree => iced_fonts::lucide::folder_tree(),
    }
}

/// Wrap an icon control in a small bottom-anchored tooltip, the shared
/// affordance for the sidebar tab strip and close affordances.
/// `icon_tooltip` for a tip built at render time (a formatted figure, a
/// path) rather than a borrowed `t(...)` literal. Same look; the owned
/// String is what lets the element outlive the caller's frame-local.
pub(crate) fn icon_tooltip_owned<'a>(
    inner: Element<'a, Message>,
    tip: String,
) -> Element<'a, Message> {
    iced::widget::tooltip(
        inner,
        container(text(tip).size(11).color(OryxisColors::t().text_primary))
            .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    radius: Radius::from(6.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            }),
        iced::widget::tooltip::Position::Bottom,
    )
    .into()
}

pub(crate) fn icon_tooltip<'a>(inner: Element<'a, Message>, tip: &'a str) -> Element<'a, Message> {
    iced::widget::tooltip(
        inner,
        container(text(tip).size(11).color(OryxisColors::t().text_primary))
            .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    radius: Radius::from(6.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            }),
        iced::widget::tooltip::Position::Bottom,
    )
    .into()
}


/// Compact human-readable byte count for the transfer overlay
/// (1 decimal past KB; integers stay integers).
fn fmt_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{n} {}", UNITS[0])
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

pub(crate) fn chat_header_btn<'a>(
    icon: iced::widget::Text<'a>,
    msg: Message,
) -> Element<'a, Message> {
    button(
        container(icon.size(13).color(OryxisColors::t().text_muted))
            .center_x(Length::Fixed(28.0))
            .center_y(Length::Fixed(24.0)),
    )
    .padding(0)
    .on_press(msg)
    .style(|_, status| {
        // Fill with bg_hover on hover/press so close/reset/action icons
        // give the same pointer feedback as the rest of the chrome.
        let bg = match status {
            BtnStatus::Hovered | BtnStatus::Pressed => OryxisColors::t().bg_hover,
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(4.0), ..Default::default() },
            ..Default::default()
        }
    })
    .into()
}
