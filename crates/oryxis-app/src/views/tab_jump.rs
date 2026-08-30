//! Termius-style "Jump to" modal, invoked from the `⋯` button in the
//! tab bar or via `Ctrl+J`. Lists every open tab plus the same Quick
//! connect entries that the new-tab picker offers (Local Terminal,
//! Serial, etc.), and includes a search box so the user can filter
//! down to a target tab without reaching for the mouse.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, scrollable, text, text_input, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{SettingsMessage, TabsMessage, SshMessage, Message, Oryxis};
use crate::i18n::t;
use crate::theme::{OryxisColors, SYSTEM_UI_SEMIBOLD};
use crate::widgets::{dir_align_x, dir_row};

impl Oryxis {
    pub(crate) fn view_tab_jump_modal(&self) -> Element<'_, Message> {
        // Keyboard rows are recorded by `jump_row` in visual order;
        // Up/Down move a selection, Enter jumps to it (or to the top
        // match while searching).
        self.modal_nav_reset();
        let needle = self.tab_jump_search.to_lowercase();
        // One terms pass for every row: Privacy Mode redacts the
        // rendered tab labels below (issue #78).
        let privacy_terms = self.privacy_terms();

        // ── Tabs section ───────────────────────────────────────────────
        // Every open tab is a row; current one gets the accent bg.
        let mut tabs_col = column![].spacing(2);
        let mut had_match = false;
        for (idx, tab) in self.tabs.iter().enumerate() {
            // Show (and search) what the strip shows, custom rename
            // included; the badge lookup keys on the automatic label so a
            // rename doesn't lose the OS / brand icon. The search matches
            // the RAW label (the needle is the user's own typing), the
            // rendered row is redacted under Privacy Mode (issue #78).
            let raw_label = tab
                .display_label(self.tab_auto_title(tab))
                .trim_end_matches(" (disconnected)")
                .to_string();
            if !needle.is_empty() && !raw_label.to_lowercase().contains(&needle) {
                continue;
            }
            let label = self.privacy_display_label(
                tab.auto_label(self.tab_auto_title(tab)),
                &raw_label,
                &privacy_terms,
            );
            had_match = true;
            let is_active = self.active_tab == Some(idx);
            // Match the tab-bar's OS-coloured badge so users recognise
            // the same visual cue from the strip up here, including the
            // local-shell fallbacks
            // their brand icon instead of the generic fallback).
            let lookup = tab.label.trim_end_matches(" (disconnected)").to_string();
            let detected_os = self.tab_detected_os(&lookup);
            let fallback = if tab.label.ends_with(" (disconnected)") {
                OryxisColors::t().text_muted
            } else {
                OryxisColors::t().accent
            };
            let (glyph, mut badge_color) =
                crate::os_icon::resolve_icon(detected_os.as_deref(), fallback);
            if tab.label.ends_with(" (disconnected)") {
                badge_color = OryxisColors::t().text_muted;
            }
            let badge: Element<'_, Message> =
                container(glyph.view(14.0, Color::WHITE))
                    .center_x(Length::Fixed(20.0))
                    .center_y(Length::Fixed(20.0))
                    .style(move |_| container::Style {
                        background: Some(Background::Color(badge_color)),
                        border: Border {
                            radius: Radius::from(4.0),
                            ..Default::default()
                        },
                        ..Default::default()
                    })
                    .into();

            tabs_col = tabs_col.push(self.jump_row(
                badge,
                label,
                is_active,
                Message::Tabs(TabsMessage::SelectTab(idx)),
            ));
        }
        // Inline "New Tab" entry, shortcut to the existing new-tab
        // picker without leaving this modal first.
        let new_tab_badge: Element<'_, Message> = container(
            iced_fonts::lucide::circle_check()
                .size(13)
                .color(OryxisColors::t().text_muted),
        )
        .center_x(Length::Fixed(20.0))
        .center_y(Length::Fixed(20.0))
        .into();
        let new_tab_label = t("new_tab").to_string();
        if new_tab_label.to_lowercase().contains(&needle) || needle.is_empty() {
            had_match = true;
            tabs_col = tabs_col.push(self.jump_row(
                new_tab_badge,
                new_tab_label,
                false,
                Message::Tabs(TabsMessage::ShowNewTabPicker),
            ));
        }

        let tabs_section: Element<'_, Message> = column![
            section_header(t("tabs")),
            Space::new().height(4),
            tabs_col,
        ]
        .into();

        // ── Quick connect section ──────────────────────────────────────
        // Mirrors the "categories" of the new-tab picker so the user
        // can also kick off a fresh session from this modal.
        let quick_local: Element<'_, Message> = container(
            iced_fonts::lucide::monitor()
                .size(13)
                .color(OryxisColors::t().accent),
        )
        .center_x(Length::Fixed(20.0))
        .center_y(Length::Fixed(20.0))
        .into();
        let mut quick_col = column![self.jump_row(
            quick_local,
            t("local_terminal").to_string(),
            false,
            Message::Settings(SettingsMessage::OpenLocalShell),
        )];
        // Search text parsing as `user@host[:port]` offers an immediate
        // ad-hoc connect, mirroring the new-tab picker's top row.
        if let Some(conn) = self.quick_connect_target(&self.tab_jump_search) {
            let quick_target: Element<'_, Message> = container(
                iced_fonts::lucide::zap()
                    .size(13)
                    .color(OryxisColors::t().accent),
            )
            .center_x(Length::Fixed(20.0))
            .center_y(Length::Fixed(20.0))
            .into();
            quick_col = quick_col.push(self.jump_row(
                quick_target,
                match conn.protocol
                    == oryxis_core::models::connection::ConnectionProtocol::Ssh
                {
                    true => format!("{}: {}", t("quick_connect"), conn.label),
                    false => {
                        format!("{} ({}): {}", t("quick_connect"), conn.protocol, conn.label)
                    }
                },
                false,
                Message::Ssh(SshMessage::QuickConnect(Box::new(
                    crate::state::QuickConnectEntry::bare(conn),
                ))),
            ));
        }
        let quick_section: Element<'_, Message> = column![
            section_header(t("quick_connect")),
            Space::new().height(4),
            quick_col,
        ]
        .into();

        // ── Search header ──────────────────────────────────────────────
        let search_input = text_input(t("search_tabs"), &self.tab_jump_search)
            .id(iced::widget::Id::new(crate::state::TAB_JUMP_SEARCH_ID))
            .on_input(|v| Message::Tabs(TabsMessage::TabJumpSearchChanged(v)))
            .padding(Padding { top: 8.0, right: 10.0, bottom: 8.0, left: 10.0 })
            .size(13)
            .style(crate::widgets::rounded_input_style).align_x(dir_align_x());

        // "Jump to" pill on the left of the search row gives the modal
        // its identity; the live shortcut hint on the right reinforces
        // it so users learn it (resolved from the binding table, never
        // hard-coded: the default changed once already, issue #100).
        let pill: Element<'_, Message> = container(
            text(t("jump_to"))
                .size(11)
                .color(OryxisColors::t().accent)
                .font(SYSTEM_UI_SEMIBOLD),
        )
        .padding(Padding { top: 3.0, right: 8.0, bottom: 3.0, left: 8.0 })
        .style(|_| container::Style {
            background: Some(Background::Color(Color {
                a: 0.15,
                ..OryxisColors::t().accent
            })),
            border: Border {
                radius: Radius::from(10.0),
                ..Default::default()
            },
            ..Default::default()
        })
        .into();
        let shortcut_hint: Element<'_, Message> = text(
            self.hotkey_label_for_action(crate::hotkeys::HotkeyAction::ShowTabJump)
                .unwrap_or_default(),
        )
        .size(11)
        .color(OryxisColors::t().text_muted)
        .into();

        let search_header = container(
            dir_row(vec![
                iced_fonts::lucide::globe()
                    .size(13)
                    .color(OryxisColors::t().text_muted)
                    .into(),
                Space::new().width(8).into(),
                pill,
                Space::new().width(8).into(),
                container(search_input).width(Length::Fill).into(),
                Space::new().width(12).into(),
                shortcut_hint,
            ])
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 4.0, right: 14.0, bottom: 4.0, left: 14.0 })
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_hover)),
            border: Border {
                radius: Radius::from(8.0),
                ..Default::default()
            },
            ..Default::default()
        });

        // Empty state, when search filters out everything.
        let body: Element<'_, Message> = if !had_match {
            container(
                text(t("no_matching_tabs"))
                    .size(12)
                    .color(OryxisColors::t().text_muted),
            )
            .padding(20)
            .into()
        } else {
            scrollable(
                column![
                    Space::new().height(8),
                    tabs_section,
                    Space::new().height(12),
                    quick_section,
                    Space::new().height(8),
                ]
                .padding(Padding {
                    top: 0.0,
                    right: 6.0,
                    bottom: 0.0,
                    left: 0.0,
                }),
            )
            // Stable id so the keyboard selection can be kept in view.
            .id(iced::widget::Id::new("tab-jump-scroll"))
            .height(Length::Fixed(420.0))
            .into()
        };

        let dialog = container(
            column![search_header, Space::new().height(4), body]
                .padding(12)
                .width(Length::Fixed(540.0)),
        )
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border {
                radius: Radius::from(12.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            shadow: iced::Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.30),
                offset: iced::Vector::new(0.0, 8.0),
                blur_radius: 24.0,
            },
            ..Default::default()
        });

        // Bare card; `widgets::modal_overlay` (the caller) owns centering,
        // the absorbing scrim, and the click-trap.
        dialog.into()
    }
}

fn section_header<'a>(label: &'a str) -> Element<'a, Message> {
    text(label.to_owned())
        .size(11)
        .color(OryxisColors::t().text_muted)
        .into()
}

impl Oryxis {
    fn jump_row<'a>(
        &self,
        icon: Element<'a, Message>,
        label: String,
        is_active: bool,
        on_select: Message,
    ) -> Element<'a, Message> {
        let bg = if is_active {
            Color { a: 0.15, ..OryxisColors::t().accent }
        } else {
            Color::TRANSPARENT
        };
        let label_color = if is_active {
            OryxisColors::t().accent
        } else {
            OryxisColors::t().text_primary
        };
        let select = on_select.clone();
        let row: Element<'a, Message> = button(
            dir_row(vec![
                icon,
                Space::new().width(8).into(),
                text(label).size(13).color(label_color).into(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .on_press_with(move || {
            // Two-step dispatch: select first, then close, keeps the
            // modal from flashing closed before the select handler runs.
            // SequencedSelect is wired in app.rs to fire both messages.
            Message::Tabs(TabsMessage::TabJumpSelect(Box::new(on_select.clone())))
        })
        .padding(Padding { top: 6.0, right: 12.0, bottom: 6.0, left: 12.0 })
        .width(Length::Fill)
        .style(move |_, status| {
            let hover_bg = match status {
                BtnStatus::Hovered if !is_active => OryxisColors::t().bg_hover,
                _ => bg,
            };
            button::Style {
                background: Some(Background::Color(hover_bg)),
                border: Border { radius: Radius::from(6.0), ..Default::default() },
                ..Default::default()
            }
        })
        .into();
        // Keyboard row: Enter mirrors the click (same two-step
        // TabJumpSelect dispatch).
        self.modal_nav_slot(
            crate::keynav::RowAction::activate(Message::Tabs(TabsMessage::TabJumpSelect(Box::new(select)))),
            6.0,
            false,
            row,
        )
    }
}
