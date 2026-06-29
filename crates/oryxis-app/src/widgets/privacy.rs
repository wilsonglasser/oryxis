//! UI helper widgets: privacy. Split out of widgets/mod.rs.

use super::*;
/// Mask a sensitive string for Privacy Mode: every alphanumeric char
/// becomes a muted block, separators (`.`, `:`, `@`, `-`, `/`, space, ...)
/// stay so the value keeps a recognizable shape
/// (`192.168.0.4` -> `███.███.█.█`, `deploy@web` -> `██████@███`). Used on
/// host cards and in session logs; the terminal does its own per-cell
/// masking against the same block glyph.
pub(crate) fn mask_blocks(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { '█' } else { c })
        .collect()
}

/// Redact IPv4 addresses and `user@host` prompt tokens in arbitrary text
/// for Privacy Mode, replacing each match with muted blocks via
/// [`mask_blocks`]. Used by the session-log viewer (which renders recorded
/// terminal output) so a recording hides the same things the live terminal
/// does. `user@host` also catches emails and typed `ssh user@host` targets,
/// which are sensitive too. Returns the input unchanged when nothing matches.
pub(crate) fn redact_for_display(s: &str) -> String {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        // `user@host` first so an email/prompt token wins over a bare IP
        // that might sit inside it. IPv4 is shape-only (octet range isn't
        // validated): display masking is reversible via Reveal, so erring
        // toward hiding a version-shaped number is acceptable.
        regex::Regex::new(
            r"[A-Za-z0-9._-]+@[A-Za-z0-9._-]+|\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b",
        )
        .expect("privacy display pattern")
    });
    if !re.is_match(s) {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    for m in re.find_iter(s) {
        out.push_str(&s[last..m.start()]);
        out.push_str(&mask_blocks(m.as_str()));
        last = m.end();
    }
    out.push_str(&s[last..]);
    out
}

/// Privacy Mode reveal toggle (the eye icon). Shows an open eye while
/// revealed (accent tint) and a struck-through eye while masked, with a
/// tooltip describing the action. Shared by the Logs view, the session-log
/// viewer header and the Known Hosts view so the reveal affordance is the
/// same everywhere. Drives `Message::TogglePrivacyReveal`.
pub(crate) fn privacy_reveal_btn<'a>(revealed: bool) -> Element<'a, Message> {
    let (glyph, tip_key) = if revealed {
        (iced_fonts::lucide::eye(), "privacy_hide")
    } else {
        (iced_fonts::lucide::eye_off(), "privacy_reveal")
    };
    let icon = glyph.size(13).color(if revealed {
        OryxisColors::t().accent
    } else {
        OryxisColors::t().text_secondary
    });
    let b = button(
        container(icon)
            .center(Length::Fixed(24.0))
            .height(Length::Fixed(24.0))
            .width(Length::Fixed(28.0)),
    )
    .on_press(Message::TogglePrivacyReveal)
    .style(move |_, status| {
        let bg = match status {
            BtnStatus::Hovered => Color::from_rgba(1.0, 1.0, 1.0, 0.08),
            BtnStatus::Pressed => Color::from_rgba(1.0, 1.0, 1.0, 0.12),
            _ if revealed => Color { a: 0.12, ..OryxisColors::t().accent },
            _ => Color::TRANSPARENT,
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
    });
    iced::widget::tooltip(
        b,
        container(text(crate::i18n::t(tip_key)).size(11))
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
