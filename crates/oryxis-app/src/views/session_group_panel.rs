//! Session-group editor side panel: name, parent group, color, and a
//! per-pane startup script for each pane in the saved arrangement. Rendered
//! in the same right-hand slot as the host editor (from `view_terminal` when
//! opened from a tab, from `view_dashboard` when opened from a card).
//!
//! Interactive controls use `MouseArea` rather than `button`: a `button`
//! rendered beside the terminal canvas has its clicks eaten by an iced
//! widget-tree quirk (the chat sidebar hit the same thing, see
//! `view_terminal.rs::sidebar_tab_btn`). `MouseArea` sidesteps it.

use iced::border::Radius;
use iced::widget::{column, container, scrollable, text, text_input, MouseArea, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis, PANEL_WIDTH};
use crate::i18n::t;
use crate::os_icon::BrandIcon;
use crate::theme::OryxisColors;
use crate::widgets::{bounds_reporter, dir_align_x, dir_row, panel_field, panel_section};

/// A pressable element built on `MouseArea` (see module docs for why not
/// `button`). The pointer cursor matches the rest of the chrome.
fn press<'a>(content: impl Into<Element<'a, Message>>, msg: Message) -> Element<'a, Message> {
    MouseArea::new(content)
        .on_press(msg)
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}

impl Oryxis {
    pub(crate) fn view_session_group_panel(&self) -> Element<'_, Message> {
        let form = &self.editor_session_group;
        let is_editing = form.editing_id.is_some();
        let title = if is_editing {
            t("session_group_edit_title")
        } else {
            t("session_group_new_title")
        };

        // ── Header ──
        let close_btn = press(
            container(
                text("\u{00D7}")
                    .size(20)
                    .color(OryxisColors::t().text_muted),
            )
            .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 }),
            Message::SessionGroupFormCancel,
        );
        let panel_header = container(
            dir_row(vec![
                text(title)
                    .size(16)
                    .color(OryxisColors::t().text_primary)
                    .into(),
                Space::new().width(Length::Fill).into(),
                close_btn,
            ])
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 16.0, right: 16.0, bottom: 12.0, left: 16.0 });

        // ── Parent-group combo: typeable text input (creates a new group on
        // save) + chevron opening the shared group-picker popover. Same
        // component as the host editor's Parent Group field. ──
        const COMBO_HEIGHT: f32 = 36.0;
        let folder_input = text_input(t("group_placeholder"), &form.group_name)
            .on_input(Message::SessionGroupFormGroupChanged)
            .on_submit(Message::SessionGroupFormSave)
            .padding(10)
            .width(Length::Fill)
            .style(crate::widgets::rounded_input_style)
            .align_x(dir_align_x());
        let folder_chevron = press(
            container(
                iced_fonts::lucide::chevron_down::<iced::Theme, iced::Renderer>()
                    .size(12)
                    .color(OryxisColors::t().text_muted),
            )
            .center_x(Length::Fixed(32.0))
            .center_y(Length::Fixed(COMBO_HEIGHT))
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    radius: Radius::from(6.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            }),
            Message::ToggleGroupPicker(crate::state::GroupPickerTarget::SessionGroupFolder),
        );
        let folder_combo: Element<'_, Message> = bounds_reporter(
            dir_row(vec![
                container(folder_input)
                    .width(Length::Fill)
                    .height(Length::Fixed(COMBO_HEIGHT))
                    .into(),
                Space::new().width(6).into(),
                container(folder_chevron)
                    .height(Length::Fixed(COMBO_HEIGHT))
                    .into(),
            ])
            .align_y(iced::Alignment::Center),
            self.session_group_folder_combo_bounds.clone(),
        );

        // Icon + color badge. Clicking opens the shared host icon/color
        // picker (swatches + custom hex + icon grid), seeded from the form.
        let badge_bg = form
            .color
            .as_deref()
            .and_then(crate::os_icon::parse_hex_color)
            .unwrap_or_else(|| OryxisColors::t().accent);
        let badge_glyph = form
            .icon_style
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(crate::os_icon::custom_icon_glyph)
            .unwrap_or(BrandIcon::Glyph(iced_fonts::lucide::boxes()));
        let icon_badge = press(
            container(badge_glyph.view(18.0, Color::WHITE))
                .width(Length::Fixed(36.0))
                .height(Length::Fixed(36.0))
                .center_x(Length::Fixed(36.0))
                .center_y(Length::Fixed(36.0))
                .style(move |_| container::Style {
                    background: Some(Background::Color(badge_bg)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                }),
            Message::ShowSessionGroupIconPicker,
        );

        // ── Section: General ──
        let general_section = panel_section(column![
            panel_field(
                t("session_group_label"),
                text_input(t("session_group_label_placeholder"), &form.label)
                    .id(iced::widget::Id::new("session-group-name"))
                    .on_input(Message::SessionGroupFormLabelChanged)
                    .on_submit(Message::SessionGroupFormSave)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x())
                    .into(),
            ),
            Space::new().height(10),
            panel_field(t("parent_group"), folder_combo),
            Space::new().height(10),
            panel_field(t("session_group_color"), icon_badge),
        ]);

        // ── Section: Panes (one pane shown at a time; chevrons step) ──
        let panes_section: Element<'_, Message> = if form.pane_rows.is_empty() {
            Space::new().height(0).into()
        } else {
            let total = form.pane_rows.len();
            let cur = form.current_pane.min(total - 1);
            let current_label = form
                .pane_rows
                .get(cur)
                .map(|r| r.label.clone())
                .unwrap_or_default();

            // Chevron nav. Pressable only when there's somewhere to go; the
            // dimmed end-state is a plain container with no handler.
            let nav = |glyph: iced::widget::Text<'static>, enabled: bool, msg: Message| {
                let color = if enabled {
                    OryxisColors::t().text_secondary
                } else {
                    OryxisColors::t().text_muted
                };
                let inner = container(glyph.size(14).color(color))
                    .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 });
                if enabled {
                    press(inner, msg)
                } else {
                    inner.into()
                }
            };

            let counter = column![
                text(format!("{} / {}", cur + 1, total))
                    .size(11)
                    .color(OryxisColors::t().text_muted),
                text(current_label)
                    .size(13)
                    .color(OryxisColors::t().text_primary)
                    .wrapping(iced::widget::text::Wrapping::None),
            ]
            .spacing(2)
            .align_x(iced::Alignment::Center)
            .width(Length::Fill);

            let header = dir_row(vec![
                nav(
                    iced_fonts::lucide::chevron_left(),
                    cur > 0,
                    Message::SessionGroupPaneNav(false),
                ),
                counter.into(),
                nav(
                    iced_fonts::lucide::chevron_right(),
                    cur + 1 < total,
                    Message::SessionGroupPaneNav(true),
                ),
            ])
            .align_y(iced::Alignment::Center)
            .width(Length::Fill);

            let editor = container(
                iced::widget::text_editor(&self.session_group_script_editor)
                    .placeholder(t("session_group_pane_script_placeholder"))
                    .on_action(Message::SessionGroupScriptAction)
                    .padding(10)
                    .height(Length::Shrink)
                    .style(crate::widgets::rounded_editor_style),
            )
            .max_height(200.0);

            panel_section(
                column![
                    text(t("session_group_panes"))
                        .size(12)
                        .color(OryxisColors::t().text_muted),
                    Space::new().height(2),
                    text(t("session_group_panes_hint"))
                        .size(10)
                        .color(OryxisColors::t().text_muted),
                    Space::new().height(8),
                    header,
                    Space::new().height(8),
                    editor,
                ]
                .width(Length::Fill)
                .align_x(dir_align_x()),
            )
        };

        // ── Error ──
        let panel_error: Element<'_, Message> = if let Some(err) = &self.session_group_panel_error {
            container(Element::from(
                text(err.clone()).size(11).color(OryxisColors::t().error),
            ))
            .padding(Padding { top: 4.0, right: 16.0, bottom: 4.0, left: 16.0 })
            .into()
        } else {
            Space::new().height(0).into()
        };

        // ── Bottom actions ──
        let save_btn_bg = if form.label.trim().is_empty() {
            OryxisColors::t().bg_surface
        } else {
            OryxisColors::t().accent
        };
        let save_btn = press(
            container(
                text(t("save"))
                    .size(14)
                    .color(OryxisColors::t().text_primary),
            )
            .padding(Padding { top: 12.0, right: 0.0, bottom: 12.0, left: 0.0 })
            .width(Length::Fill)
            .center_x(Length::Fill)
            .style(move |_| container::Style {
                background: Some(Background::Color(save_btn_bg)),
                border: Border { radius: Radius::from(8.0), ..Default::default() },
                ..Default::default()
            }),
            Message::SessionGroupFormSave,
        );

        let bottom = column![panel_error, save_btn].spacing(8);

        let form_scroll = scrollable(
            column![general_section, Space::new().height(8), panes_section]
                .padding(Padding { top: 0.0, right: 16.0, bottom: 16.0, left: 16.0 }),
        )
        .height(Length::Fill);

        let panel_content = column![
            panel_header,
            form_scroll,
            container(bottom).padding(Padding {
                top: 8.0,
                right: 16.0,
                bottom: 16.0,
                left: 16.0,
            }),
        ]
        .height(Length::Fill);

        container(panel_content)
            .width(PANEL_WIDTH)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    color: OryxisColors::t().border,
                    width: 1.0,
                    radius: Radius::from(0.0),
                },
                ..Default::default()
            })
            .into()
    }
}
