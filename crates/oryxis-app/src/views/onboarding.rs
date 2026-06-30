//! Welcome / onboarding carousel.
//!
//! The first-run flow only: rendered off `VaultState::NeedSetup` as the
//! base view (full screen, wrapped in window chrome by `root_view`). The
//! final slide creates the vault via `VaultSetup` / `VaultSkipPassword`,
//! replacing the old dry `view_vault_setup` screen.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, svg, text, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis};
use crate::dispatch_onboarding::ONBOARDING_LAST_SLIDE;
use crate::i18n::t;
use crate::theme::OryxisColors;
use crate::widgets::{dir_row, password_input_with_eye, styled_button};

impl Oryxis {
    /// Build the full-screen first-run onboarding page (the caller wraps
    /// it in window chrome). The carousel IS the screen: feature slides
    /// then a final master-password slide that creates the vault.
    pub(crate) fn view_onboarding_page(&self) -> Element<'_, Message> {
        let slide = self.onboarding_slide.min(ONBOARDING_LAST_SLIDE);

        let content: Element<'_, Message> = if slide == ONBOARDING_LAST_SLIDE {
            self.onboarding_password_slide()
        } else {
            self.onboarding_feature_slide(slide)
        };

        let card_inner = column![
            content,
            Space::new().height(34),
            // Pagination dots share the action row, centered between Back
            // (left) and Skip / Next (right).
            self.onboarding_nav(slide),
        ]
        .width(Length::Fill)
        .align_x(iced::Alignment::Center);

        let card = container(card_inner)
            .padding(Padding {
                top: 48.0,
                right: 48.0,
                bottom: 34.0,
                left: 48.0,
            })
            .width(Length::Fixed(600.0))
            .style(|_| {
                let base = OryxisColors::t().bg_primary;
                let accent = OryxisColors::t().accent;
                container::Style {
                    // A subtle accent wash diagonally from the top-left
                    // corner, settling into the surface toward the bottom.
                    background: Some(onboarding_accent_gradient(
                        blend(base, accent, 0.12),
                        base,
                    )),
                    border: Border {
                        radius: Radius::from(18.0),
                        color: OryxisColors::t().border,
                        width: 1.0,
                    },
                    // A soft drop shadow lifts the card off the page so the
                    // panel-in-page reads as deliberate, not a stray box.
                    shadow: iced::Shadow {
                        color: Color { a: 0.32, ..Color::BLACK },
                        offset: iced::Vector::new(0.0, 12.0),
                        blur_radius: 40.0,
                    },
                    ..Default::default()
                }
            });

        // The card centered on a full-fill background carrying a stronger
        // accent gradient, top-left to bottom, like the active-tab wash in
        // the bar. The user leaves by creating the vault or continuing
        // without a password on the final slide (no close affordance).
        container(card)
            .center(Length::Fill)
            .style(|_| {
                let base = OryxisColors::t().bg_sidebar;
                let accent = OryxisColors::t().accent;
                container::Style {
                    background: Some(onboarding_accent_gradient(
                        blend(base, accent, 0.22),
                        base,
                    )),
                    ..Default::default()
                }
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// One of the four feature slides (welcome / vault / connect / sync).
    /// Slide 0 is a hero: logo + headline + tagline. Slides 1..3 lead with
    /// a tinted glyph, a headline, and a short bulleted highlight list so
    /// the value reads at a glance. All copy comes from i18n (17 languages).
    fn onboarding_feature_slide(&self, slide: usize) -> Element<'_, Message> {
        let accent = OryxisColors::t().accent;
        let badge: Element<'_, Message> = if slide == 0 {
            svg(self.logo_handle.clone()).width(84).height(84).into()
        } else {
            let glyph = match slide {
                1 => iced_fonts::lucide::shield(),
                2 => iced_fonts::lucide::terminal(),
                _ => iced_fonts::lucide::sparkles(),
            };
            onboarding_icon_badge(glyph.size(38).color(accent).into(), accent)
        };

        let title_key = match slide {
            0 => "onboarding_welcome_title",
            1 => "onboarding_vault_title",
            2 => "onboarding_connect_title",
            _ => "onboarding_sync_title",
        };

        // Slide 0 stays a single tagline; the feature slides become a
        // left-aligned highlight list (three bullets) for scannability.
        let body: Element<'_, Message> = if slide == 0 {
            text(t("onboarding_welcome_body"))
                .size(16)
                .color(OryxisColors::t().text_secondary)
                .align_x(iced::alignment::Horizontal::Center)
                .into()
        } else {
            let bullets = match slide {
                1 => ["onboarding_vault_b1", "onboarding_vault_b2", "onboarding_vault_b3"],
                2 => ["onboarding_connect_b1", "onboarding_connect_b2", "onboarding_connect_b3"],
                _ => ["onboarding_sync_b1", "onboarding_sync_b2", "onboarding_sync_b3"],
            };
            column![
                onboarding_bullet(t(bullets[0])),
                Space::new().height(12),
                onboarding_bullet(t(bullets[1])),
                Space::new().height(12),
                onboarding_bullet(t(bullets[2])),
            ]
            .width(Length::Fixed(440.0))
            .align_x(crate::widgets::dir_align_x())
            .into()
        };

        column![
            badge,
            Space::new().height(24),
            text(t(title_key)).size(30).font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            }).color(OryxisColors::t().text_primary),
            Space::new().height(18),
            body,
        ]
        .align_x(iced::Alignment::Center)
        .into()
    }

    /// The final slide: master-password setup. Creates the vault via
    /// `VaultSetup` (or `VaultSkipPassword`). The "why a password helps"
    /// copy is reused from the Settings security strings.
    fn onboarding_password_slide(&self) -> Element<'_, Message> {
        let accent = OryxisColors::t().accent;
        let badge = onboarding_icon_badge(
            iced_fonts::lucide::key_round().size(38).color(accent).into(),
            accent,
        );

        let header = column![
            badge,
            Space::new().height(24),
            text(t("onboarding_password_title")).size(30).font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            }).color(OryxisColors::t().text_primary),
            Space::new().height(14),
            text(t("vault_importance_desc"))
                .size(15)
                .color(OryxisColors::t().text_secondary)
                .align_x(iced::alignment::Horizontal::Center),
        ]
        .align_x(iced::Alignment::Center);

        let fields = container(password_input_with_eye(
            t("master_password_optional"),
            &self.vault_ui.password_input,
            Message::VaultPasswordChanged,
            Some(Message::VaultSetup),
            self.vault_ui.password_visible,
            Message::VaultTogglePasswordVisibility,
            12.0,
        ))
        .width(300);

        let error: Element<'_, Message> = if let Some(e) = &self.vault_ui.error {
            column![
                Space::new().height(8),
                text(e.clone()).size(12).color(OryxisColors::t().error),
            ]
            .into()
        } else {
            Space::new().height(0).into()
        };

        column![
            header,
            Space::new().height(8),
            text(t("vault_set_password_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted)
                .align_x(iced::alignment::Horizontal::Center),
            Space::new().height(14),
            fields,
            Space::new().height(14),
            styled_button(t("create_vault"), Message::VaultSetup, accent),
            Space::new().height(6),
            onboarding_text_button(t("continue_without_password"), Message::VaultSkipPassword),
            error,
        ]
        .align_x(iced::Alignment::Center)
        .into()
    }

    /// The single action row: Back on the leading edge, the pagination
    /// dots centered, and Skip + Next on the trailing edge. The final
    /// slide drops Skip / Next (its primary actions live in the slide
    /// body), keeping Back + dots. `dir_row` mirrors the order under RTL.
    fn onboarding_nav(&self, slide: usize) -> Element<'_, Message> {
        let mut items: Vec<Element<'_, Message>> = Vec::new();

        if slide > 0 {
            items.push(onboarding_text_button(t("onboarding_back"), Message::OnboardingBack));
        }
        items.push(Space::new().width(Length::Fill).into());
        items.push(onboarding_dots(slide));
        items.push(Space::new().width(Length::Fill).into());

        if slide < ONBOARDING_LAST_SLIDE {
            items.push(onboarding_text_button(t("onboarding_skip"), Message::OnboardingSkipToEnd));
            items.push(Space::new().width(10).into());
            items.push(styled_button(
                t("onboarding_next"),
                Message::OnboardingNext,
                OryxisColors::t().accent,
            ));
        }

        dir_row(items).align_y(iced::Alignment::Center).into()
    }
}

/// A single highlight bullet: an accent check glyph and a line of copy,
/// `dir_row` so the glyph sits on the leading edge under RTL too.
fn onboarding_bullet(label: &str) -> Element<'_, Message> {
    dir_row(vec![
        container(
            iced_fonts::lucide::check()
                .size(15)
                .color(OryxisColors::t().accent),
        )
        .padding(Padding { top: 2.0, right: 0.0, bottom: 0.0, left: 0.0 })
        .into(),
        Space::new().width(12).into(),
        text(label.to_string())
            .size(15)
            .color(OryxisColors::t().text_secondary)
            .into(),
    ])
    .align_y(iced::Alignment::Start)
    .into()
}

/// Opaque diagonal accent wash: `glow` at the top-left corner fading to
/// `base` toward the bottom-right (angle ~135 deg, the same family as the
/// active-tab gradient in the bar). Both stops are opaque so the surface
/// reads cleanly whether or not anything sits behind the container.
fn onboarding_accent_gradient(glow: Color, base: Color) -> Background {
    Background::Gradient(iced::Gradient::Linear(
        iced::gradient::Linear::new(iced::Radians(std::f32::consts::PI * 0.75))
            .add_stop(0.0, glow)
            .add_stop(0.55, blend(base, glow, 0.25))
            .add_stop(1.0, base),
    ))
}

/// Linear interpolate two colors, forcing an opaque result. Used to build
/// the accent-tinted "glow" end of the onboarding gradient from the theme's
/// base surface and accent so it tracks custom themes.
fn blend(a: Color, b: Color, t: f32) -> Color {
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: 1.0,
    }
}

/// A circular, tinted badge framing a slide's glyph: a soft accent disc so
/// the icon reads as a deliberate hero element, not a stray symbol.
fn onboarding_icon_badge<'a>(glyph: Element<'a, Message>, accent: Color) -> Element<'a, Message> {
    container(glyph)
        .center(Length::Fixed(84.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(Color { a: 0.12, ..accent })),
            border: Border {
                radius: Radius::from(42.0),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

/// The slide-position dots (filled for the current slide, muted otherwise).
fn onboarding_dots<'a>(active: usize) -> Element<'a, Message> {
    let mut items: Vec<Element<'a, Message>> = Vec::new();
    for i in 0..=ONBOARDING_LAST_SLIDE {
        if i > 0 {
            items.push(Space::new().width(7).into());
        }
        let on = i == active;
        let color = if on {
            OryxisColors::t().accent
        } else {
            OryxisColors::t().border
        };
        let dia = if on { 9.0 } else { 7.0 };
        items.push(
            container(Space::new())
                .width(Length::Fixed(dia))
                .height(Length::Fixed(dia))
                .style(move |_| container::Style {
                    background: Some(Background::Color(color)),
                    border: Border {
                        radius: Radius::from(dia / 2.0),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .into(),
        );
    }
    dir_row(items).align_y(iced::Alignment::Center).into()
}

/// A subtle text-style button (muted label, hover/press tint) for the
/// secondary affordances: Back, Skip, "continue without password", Later.
fn onboarding_text_button(label: &str, msg: Message) -> Element<'_, Message> {
    button(text(label.to_string()).size(13).color(OryxisColors::t().text_secondary))
        .on_press(msg)
        .padding(Padding { top: 8.0, right: 16.0, bottom: 8.0, left: 16.0 })
        .style(|_, status| {
            let bg = match status {
                BtnStatus::Hovered => OryxisColors::t().bg_hover,
                BtnStatus::Pressed => OryxisColors::t().bg_selected,
                _ => Color::TRANSPARENT,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(6.0), ..Default::default() },
                ..Default::default()
            }
        })
        .into()
}
