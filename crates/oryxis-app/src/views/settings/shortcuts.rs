//! Settings -> Shortcuts section view. Split out of views/settings/mod.rs.

use super::*;
use iced::widget::column;

impl Oryxis {
    pub(crate) fn view_settings_shortcuts(&self) -> Element<'_, Message> {
        use crate::hotkeys::{default_bindings, HotkeyAction};
        // Keyboard rows are recorded in visual order.
        self.keynav_settings_reset();
        let defaults = default_bindings();

        // Header: title + hint + global reset button.
        let header = column![
                                text(crate::i18n::t("hotkey_edit_hint"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(10),
            self.settings_nav_slot_labeled(
                crate::i18n::t("hotkey_reset_all"),
                crate::keynav::RowAction::activate(Message::Settings(SettingsMessage::ResetAllHotkeys)),
                6.0,
                styled_button(
                    crate::i18n::t("hotkey_reset_all"),
                    Message::Settings(SettingsMessage::ResetAllHotkeys),
                    OryxisColors::t().bg_selected,
                ),
            ),
            Space::new().height(16),
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());

        // Ctrl+digit slot mapping. Not a binding (the chord is editable
        // in the table below like any other), but what the slots COUNT,
        // which is the only part of this action a user can be surprised
        // by: on vaults from before the change the Home tab holds slot 1,
        // so the third tab answers to Ctrl+4 and the tab numbers read one
        // ahead of their chords.
        let slot_row = column![
            self.nav_toggle_row(
                crate::i18n::t("tab_slot_includes_home"),
                self.prefs.tab_slot_includes_home,
                Message::Settings(SettingsMessage::SettingToggleTabSlotIncludesHome),
            ),
            Space::new().height(4),
            text(crate::i18n::t("tab_slot_includes_home_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(16),
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());

        let mut rows_col = column![header, slot_row]
            .spacing(8)
            .width(Length::Fill)
            .align_x(dir_align_x());

        // Every gesture is an editable row here, the wheel included:
        // Ctrl+wheel zoom is a chord on the font zoom actions (#225),
        // the way terminal copy / paste / select-all became ordinary
        // actions in #75. There is no read-only footer left.
        for action in HotkeyAction::all() {
            // The row is not one nav slot: it records a slot per chord
            // chip, plus the add chip and the reset button. Enter on a
            // chip starts a capture for THAT chord.
            rows_col = rows_col.push(self.hotkey_editor_row(*action, defaults.get(action)));
        }

        scrollable(
            container(rows_col)
                .padding(Padding { top: 24.0, right: 24.0, bottom: 24.0, left: 24.0 }),
        )
        // Stable id so the keyboard router can keep the selected row
        // in view.
        .id(iced::widget::Id::new("settings-shortcuts-scroll"))
        .on_scroll(|s| Message::Settings(SettingsMessage::SectionScrolled(s.viewport.relative_offset().y)))
        .height(Length::Fill)
        .boxed()
    }

    /// One chord chip in the Shortcuts editor: the badge cluster on a
    /// clickable surface that starts a capture for `slot`. `chord` is
    /// `None` for the trailing add button.
    ///
    /// Records its own keynav slot, so callers must build chips in
    /// display order (build order is record order).
    fn hotkey_chip(
        &self,
        action: crate::hotkeys::HotkeyAction,
        slot: crate::hotkeys::HotkeySlot,
        chord: Option<crate::hotkeys::HotkeyBinding>,
        recording: bool,
        empty_row: bool,
    ) -> Element<'_, Message> {
        let idx = self.settings_nav_record(crate::keynav::RowAction::activate(
            Message::Settings(SettingsMessage::StartEditingHotkey(action, slot)),
        ));
        let inner: Element<'_, Message> = if recording {
            // Capture state: paint with the high-contrast `button_text`
            // foreground, the readable pairing for the `button_bg`
            // surface this button already uses. Painting accent-on-bg
            // here washed the placeholder out against the dark button.
            // Rows that take a mouse button say so up front: finding out
            // by clicking and having nothing happen reads as broken.
            let placeholder = if action.accepts_mouse() {
                "hotkey_press_a_key_or_mouse"
            } else {
                "hotkey_press_a_key"
            };
            text(crate::i18n::t(placeholder))
                .size(12)
                .color(OryxisColors::t().button_text)
                .boxed()
        } else if let Some(b) = chord {
            // For family actions the suffix badge is rendered with a
            // distinct muted style so the user sees at a glance which
            // slot is fixed.
            let labels = b.badges();
            let n = labels.len();
            let primary_editable = action.primary_editable();
            let badges: Vec<Element<'_, Message>> = labels
                .into_iter()
                .enumerate()
                .map(|(i, lbl)| {
                    let is_suffix = i == n - 1;
                    if is_suffix && !primary_editable {
                        // Fixed-suffix badge: same solid pill as the
                        // modifiers so it stays legible, but with a
                        // dashed-feel via a tinted border + muted
                        // text. The earlier alpha-40 background
                        // washed out completely against the dark
                        // button surface; this keeps the visual
                        // distinction without losing contrast.
                        container(
                            text(lbl)
                                .size(11)
                                .color(OryxisColors::t().text_secondary),
                        )
                        .padding(Padding {
                            top: 3.0,
                            right: 6.0,
                            bottom: 3.0,
                            left: 6.0,
                        })
                        .style(|_| container::Style {
                            background: Some(Background::Color(OryxisColors::t().bg_selected)),
                            border: Border {
                                radius: Radius::from(4.0),
                                color: OryxisColors::t().border,
                                width: 1.0,
                            },
                            ..Default::default()
                        })
                        .boxed()
                    } else {
                        key_badge_owned(lbl)
                    }
                })
                .collect();
            iced::widget::Row::with_children(badges)
                .spacing(4)
                .align_y(iced::Alignment::Center)
                .boxed()
        } else if empty_row {
            // Nothing bound at all: the add chip carries the unbound
            // placeholder, so the row still reads as one affordance
            // rather than a bare "+" next to nothing.
            // Same contrast rule as the capture placeholder above: the
            // chip surface is `button_bg` (an accent fill on most
            // themes), so the muted foreground washed out on it.
            text(crate::i18n::t("hotkey_unbound"))
                .size(11)
                .color(OryxisColors::t().button_text)
                .boxed()
        } else {
            text("+")
                .size(13)
                .color(OryxisColors::t().button_text)
                .boxed()
        };

        let btn = button(inner)
            .on_press(Message::Settings(SettingsMessage::StartEditingHotkey(action, slot)))
            .style(move |_, status| {
                let bg = match status {
                    BtnStatus::Hovered => OryxisColors::t().button_bg_hover,
                    _ => OryxisColors::t().button_bg,
                };
                // The chip being recorded gets an accent border so it
                // reads "pending input" against its siblings.
                let border_color = if recording {
                    OryxisColors::t().accent
                } else {
                    OryxisColors::t().border
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border {
                        radius: Radius::from(6.0),
                        color: border_color,
                        width: 1.0,
                    },
                    ..Default::default()
                }
            });
        self.settings_nav_ring_at(idx, 6.0, btn.boxed())
    }

    /// Single row in the Shortcuts editor list. Renders one chip per
    /// bound chord (click to re-record it, Delete while recording to
    /// drop it), a trailing add chip, and a reset button only when the
    /// chords differ from the factory ones, so the user can spot
    /// overrides at a glance.
    ///
    /// Actions carry a LIST of chords, not one: `Ctrl+Shift+V` and
    /// `Shift+Insert` are both factory paste chords. Each chord gets
    /// its own bordered chip precisely so two chords never read as one
    /// long run of badges.
    pub(crate) fn hotkey_editor_row(
        &self,
        action: crate::hotkeys::HotkeyAction,
        default: Option<&crate::hotkeys::HotkeyBindings>,
    ) -> Element<'_, Message> {
        use crate::hotkeys::{HotkeyBindings, HotkeySlot};
        let fallback = HotkeyBindings::default();
        let binds = self.hotkey_bindings.get(&action).unwrap_or(&fallback);
        // An action that ships unbound (`ships_unbound`, no entry in the
        // factory table) is overridden by ANY chord: `None` there means
        // "nothing", not "unknown", or its Reset would never show and a
        // chord bound to it could not be taken back from here.
        let is_overridden = default.map_or(!binds.is_empty(), |d| d != binds);
        let editing = self
            .editing_hotkey
            .filter(|(a, _)| *a == action)
            .map(|(_, s)| s);

        // Mouse bindings are ordinary chords in this list (middle-click
        // paste is one of them out of the box), so they render as
        // editable chips like any other rather than as a read-only
        // gesture badge.
        let mut chips: Vec<Element<'_, Message>> = Vec::with_capacity(binds.len() + 1);
        chips.extend(binds.iter().enumerate().map(|(i, chord)| {
            let slot = HotkeySlot::Replace(i);
            self.hotkey_chip(action, slot, Some(*chord), editing == Some(slot), false)
        }));
        chips.push(self.hotkey_chip(
            action,
            HotkeySlot::Add,
            None,
            editing == Some(HotkeySlot::Add),
            binds.is_empty(),
        ));

        // The chip run WRAPS inside its fixed column: an action can carry
        // a gesture badge plus several chords, and a single line would
        // squeeze the trailing add chip into an unreadable sliver (or
        // push it out of the column entirely). The column keeps its fixed
        // width so the label beside it never jitters as chords are added;
        // the row simply grows taller. `spacing` / `align_y` must be set
        // before `wrap()`, which consumes the `Row`.
        let pills_box = container(
            iced::widget::Row::with_children(chips)
                .spacing(6)
                .align_y(iced::Alignment::Center)
                .wrap()
                .vertical_spacing(4)
                .align_x(dir_align_x()),
        )
        .width(260)
        .align_x(dir_align_x());

        let label = text(crate::i18n::t(action.label_key()))
            .size(13)
            .color(OryxisColors::t().text_secondary);

        // Recorded after the chips: build order is record order, and
        // reset sits at the trailing edge of the row.
        let reset_el: Element<'_, Message> = if is_overridden {
            let btn = button(
                text(crate::i18n::t("hotkey_reset"))
                    .size(11)
                    .color(OryxisColors::t().text_muted),
            )
            .on_press(Message::Settings(SettingsMessage::ResetHotkey(action)))
            .style(|_, status| {
                let bg = match status {
                    BtnStatus::Hovered => Some(Background::Color(OryxisColors::t().button_bg_hover)),
                    _ => None,
                };
                button::Style {
                    background: bg,
                    border: Border {
                        radius: Radius::from(4.0),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            });
            self.settings_nav_slot(
                crate::keynav::RowAction::activate(Message::Settings(SettingsMessage::ResetHotkey(action))),
                4.0,
                btn.boxed(),
            )
        } else {
            Space::new().boxed()
        };

        dir_row(vec![
            pills_box.boxed(),
            label.boxed(),
            Space::new().width(Length::Fill).boxed(),
            reset_el,
        ])
        .align_y(iced::Alignment::Center)
        .boxed()
    }
}

/// One key badge of a chord chip. The editor builds labels at runtime
/// from `HotkeyBinding::badges()`, hence the owned `String`.
fn key_badge_owned(label: String) -> Element<'static, Message> {
    container(text(label).size(11).color(OryxisColors::t().text_primary))
        .padding(Padding {
            top: 3.0,
            right: 6.0,
            bottom: 3.0,
            left: 6.0,
        })
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_selected)),
            border: Border {
                radius: Radius::from(4.0),
                ..Default::default()
            },
            ..Default::default()
        })
        .boxed()
}
