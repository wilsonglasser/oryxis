//! Manual host-group editor side panel: label + parent group + icon +
//! color. Rendered in the same right-hand slot as the host /
//! session-group editors (from `view_main::active_side_panel` when
//! `group_edit.visible`). Doubles as the "New subgroup" creation form
//! when `group_edit.id` is `None` (opened from the folder kebab with
//! the parent prefilled).

use iced::border::Radius;
use iced::widget::{button, column, container, scrollable, text, text_input, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{TabsMessage, Message, NavigationMessage, Oryxis};
use crate::os_icon::BrandIcon;
use crate::theme::OryxisColors;
use crate::widgets::{dir_align_x, dir_row, panel_field, panel_section};

impl Oryxis {
    pub(crate) fn view_group_edit_panel(&self) -> Element<'_, Message> {
        // Keyboard rows are recorded in visual order (row mode: Up/Down from any input).
        self.panel_nav_reset();

        // ── Header ──
        // The close (×) is not a keyboard row: Esc already owns panel
        // close, and recording it would make the header the first Down
        // target instead of the form.
        // `id = None` is create mode. The title tracks the Parent Group
        // field live: an empty parent creates a top-level folder ("New
        // group", the toolbar/empty-state entry), a filled one a child
        // ("New subgroup", the folder kebab entry). Self-correcting if
        // the user edits the parent field either way.
        let header_key = if self.group_edit.id.is_some() {
            "edit_group"
        } else if self.group_edit.parent_label.trim().is_empty() {
            "new_group"
        } else {
            "new_subgroup"
        };
        let panel_header = container(
            dir_row(vec![
                text(crate::i18n::t(header_key))
                    .size(16)
                    .color(OryxisColors::t().text_primary)
                    .into(),
                Space::new().width(Length::Fill).into(),
                button(text("\u{00D7}").size(20).color(OryxisColors::t().text_muted))
                    .on_press(Message::Tabs(TabsMessage::CancelGroupEdit))
                    .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(Color::TRANSPARENT)),
                        border: Border::default(),
                        ..Default::default()
                    })
                    .into(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 12.0, right: 16.0, bottom: 12.0, left: 16.0 });

        // Icon + color badge. Clicking opens the shared icon/color picker,
        // seeded from the in-memory form (deferred save).
        let badge_bg = crate::os_icon::parse_hex_color(&self.group_edit.color)
            .unwrap_or_else(|| OryxisColors::t().accent);
        let badge_glyph = if self.group_edit.icon.is_empty() {
            BrandIcon::Glyph(iced_fonts::lucide::boxes())
        } else {
            crate::os_icon::custom_icon_glyph(&self.group_edit.icon)
        };
        let icon_badge = button(
            container(badge_glyph.view(18.0, Color::WHITE))
                .center_x(Length::Fixed(36.0))
                .center_y(Length::Fixed(36.0)),
        )
        .on_press(Message::Tabs(TabsMessage::ShowGroupEditIconPicker))
        .padding(0)
        .style(move |_, _| button::Style {
            background: Some(Background::Color(badge_bg)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });

        // Keyboard rows record in build order == display order (panel
        // contract), so the Name field is built before the parent
        // combo and the icon badge row is recorded last inside the
        // section column below.
        let name_field = panel_field(
            crate::i18n::t("name"),
            self.panel_nav_slot(
                crate::keynav::RowAction::input(iced::widget::Id::new("group-edit-name")),
                10.0,
                text_input(crate::i18n::t("group_placeholder"), &self.group_edit.label)
                    .id(iced::widget::Id::new("group-edit-name"))
                    .on_input(|v| Message::Tabs(TabsMessage::GroupEditLabelChanged(v)))
                    .on_submit(Message::Tabs(TabsMessage::SaveGroupEdit))
                    .padding(10)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x())
                    .into(),
            ),
        );

        // Parent Group combo, same shape as the dynamic-group editor:
        // text input + chevron that opens the shared group picker
        // popover (which excludes this group's own subtree). Typing a
        // label works too; empty / unmatched = root.
        const PARENT_COMBO_HEIGHT: f32 = 36.0;
        let parent_input = self.panel_nav_slot(
            crate::keynav::RowAction::input(iced::widget::Id::new("group-edit-parent")),
            10.0,
            text_input(
                crate::i18n::t("group_placeholder"),
                &self.group_edit.parent_label,
            )
            .id(iced::widget::Id::new("group-edit-parent"))
            .on_input(|v| Message::Tabs(TabsMessage::GroupEditParentChanged(v)))
            .padding(10)
            .style(crate::widgets::rounded_input_style)
            .align_x(dir_align_x())
            .into(),
        );
        let picker_toggle = Message::Navigation(NavigationMessage::ToggleGroupPicker(
            crate::state::GroupPickerTarget::GroupEditParent,
        ));
        let parent_chevron = self.panel_nav_slot(
            crate::keynav::RowAction::activate(picker_toggle.clone()),
            8.0,
            button(
                container(
                    iced_fonts::lucide::chevron_down::<iced::Theme, iced::Renderer>()
                        .size(12)
                        .color(OryxisColors::t().text_muted),
                )
                .center_x(Length::Fixed(32.0))
                .center_y(Length::Fixed(PARENT_COMBO_HEIGHT)),
            )
            .on_press(picker_toggle)
            .padding(0)
            .style(|_, status| {
                let bg = match status {
                    button::Status::Hovered => OryxisColors::t().bg_hover,
                    _ => OryxisColors::t().bg_surface,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border {
                        radius: Radius::from(6.0),
                        color: OryxisColors::t().border,
                        width: 1.0,
                    },
                    ..Default::default()
                }
            })
            .into(),
        );
        let parent_combo: Element<'_, Message> = crate::widgets::bounds_reporter(
            dir_row(vec![
                container(parent_input)
                    .width(Length::Fill)
                    .height(Length::Fixed(PARENT_COMBO_HEIGHT))
                    .into(),
                Space::new().width(6).into(),
                container(parent_chevron)
                    .height(Length::Fixed(PARENT_COMBO_HEIGHT))
                    .into(),
            ])
            .align_y(iced::Alignment::Center),
            self.group_edit_parent_combo_bounds.clone(),
        );

        // ── Section: General ──
        let general_section = panel_section(column![
            name_field,
            Space::new().height(10),
            panel_field(crate::i18n::t("parent_group"), parent_combo),
            Space::new().height(10),
            panel_field(
                crate::i18n::t("group_icon_color"),
                self.panel_nav_slot(
                    crate::keynav::RowAction::activate(Message::Tabs(TabsMessage::ShowGroupEditIconPicker)),
                    8.0,
                    icon_badge.into(),
                ),
            ),
        ]);

        // ── Section: Defaults (D4) ──
        // What every host inside this group inherits unless it says
        // otherwise. Its own section rather than more rows in General:
        // the fields above describe the FOLDER, these describe the
        // hosts in it, and mixing the two reads as one long form where
        // nothing signals that half of it reaches other records.
        let defaults_section: Element<'_, Message> =
            panel_section(column![self.group_defaults_section()]);

        let form_scroll = scrollable(
            container(column![general_section, Space::new().height(12), defaults_section])
                .padding(Padding {
                    top: 0.0,
                    right: 16.0,
                    bottom: 16.0,
                    left: 16.0,
                }),
        )
        // Shared id: the keyboard router keeps the selected row in view.
        .id(iced::widget::Id::new("side-panel-scroll"))
        .height(Length::Fill);

        // Full-width Save, standardized with the host editor's footer
        // (the header × acts as Cancel).
        let save_btn = self.panel_nav_slot(
            crate::keynav::RowAction::activate(Message::Tabs(TabsMessage::SaveGroupEdit)),
            8.0,
            button(
                container(text(crate::i18n::t("save")).size(14).color(OryxisColors::t().text_primary))
                    .padding(Padding { top: 12.0, right: 0.0, bottom: 12.0, left: 0.0 })
                    .width(Length::Fill)
                    .center_x(Length::Fill),
            )
            .on_press(Message::Tabs(TabsMessage::SaveGroupEdit))
            .width(Length::Fill)
            .style(|_, _| button::Style {
                background: Some(Background::Color(OryxisColors::t().accent)),
                border: Border { radius: Radius::from(8.0), ..Default::default() },
                ..Default::default()
            })
            .into(),
        );

        let footer = container(save_btn)
            .padding(Padding { top: 8.0, right: 16.0, bottom: 16.0, left: 16.0 });

        let panel_content = column![panel_header, form_scroll, footer].height(Length::Fill);

        crate::widgets::side_panel_frame(panel_content.into(), OryxisColors::t().bg_surface, self.panel_width)
    }
}
