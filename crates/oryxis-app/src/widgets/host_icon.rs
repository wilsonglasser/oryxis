//! UI helper widgets: host_icon. Split out of widgets/mod.rs.

use super::*;
/// Resolve a `Color` from a `#RRGGBB` hex string. Returns `None` for any
/// other input so callers can fall through to the global accent.
pub(crate) fn parse_hex_color(s: &str) -> Option<Color> {
    let trimmed = s.trim_start_matches('#');
    if trimmed.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&trimmed[0..2], 16).ok()?;
    let g = u8::from_str_radix(&trimmed[2..4], 16).ok()?;
    let b = u8::from_str_radix(&trimmed[4..6], 16).ok()?;
    Some(Color::from_rgb8(r, g, b))
}

/// Effective host icon style: resolve the per-host override, fall back to
/// the global default, then default to "circular" if both are missing
/// or contain an unknown value.
pub(crate) fn resolve_host_icon_style(per_host: Option<&str>, global: &str) -> HostIconStyle {
    let candidate = per_host.unwrap_or(global);
    match candidate {
        "square" => HostIconStyle::Square,
        "rounded" => HostIconStyle::Rounded,
        "outline" => HostIconStyle::Outline,
        "initials" => HostIconStyle::Initials,
        _ => HostIconStyle::Circular,
    }
}

/// Host icon shape, resolved by `resolve_host_icon_style`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostIconStyle {
    Circular,
    /// Sharp-cornered square (radius 0). The earlier "square" value
    /// was actually rounded, which is now `Rounded` below.
    Square,
    /// Soft-cornered square (~25 % radius). This was the original
    /// `Square` rendering before user feedback split the two.
    Rounded,
    Outline,
    Initials,
}

/// Render a host badge in the chosen style. The badge is a fixed
/// `size x size` square; the inner geometry adapts to `style`:
///
/// - `Circular`: filled disc with the glyph centered (radius = size/2)
/// - `Square`: filled rounded square, same shape as the OS badge in
///   tab strips (compatibility look)
/// - `Outline`: transparent fill with a 1.5 px colored border + glyph
///   in the border color
/// - `Initials`: filled disc with the first one or two characters of
///   `label` instead of the OS glyph, using a contrasting foreground
///
/// `color` is the badge background / outline color (typically the
/// resolved per-host accent). `label` is the source for the initials
/// when style == Initials; for the other styles a caller supplies an
/// `Element` glyph via `glyph` (e.g. an OS lucide icon). Pass `None`
/// for `glyph` to render a blank circle when no OS could be detected.
pub(crate) fn host_icon<'a>(
    style: HostIconStyle,
    color: Color,
    label: &str,
    glyph: Option<Element<'a, Message>>,
    size: f32,
) -> Element<'a, Message> {
    let half = size / 2.0;
    match style {
        HostIconStyle::Circular | HostIconStyle::Square | HostIconStyle::Rounded => {
            let radius = match style {
                HostIconStyle::Circular => half,
                HostIconStyle::Square => 0.0,
                HostIconStyle::Rounded => size * 0.25,
                _ => 0.0,
            };
            let inner: Element<'a, Message> = glyph
                .unwrap_or_else(|| Space::new().width(0).height(0).into());
            container(inner)
                .center_x(Length::Fixed(size))
                .center_y(Length::Fixed(size))
                .style(move |_| container::Style {
                    background: Some(Background::Color(color)),
                    border: Border { radius: Radius::from(radius), ..Default::default() },
                    ..Default::default()
                })
                .into()
        }
        HostIconStyle::Outline => {
            let inner: Element<'a, Message> = glyph
                .unwrap_or_else(|| Space::new().width(0).height(0).into());
            container(inner)
                .center_x(Length::Fixed(size))
                .center_y(Length::Fixed(size))
                .style(move |_| container::Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    border: Border {
                        radius: Radius::from(half),
                        color,
                        width: 1.5,
                    },
                    ..Default::default()
                })
                .into()
        }
        HostIconStyle::Initials => {
            // Take up to two leading alphanumeric chars, uppercased.
            // "Saúde e Vida" -> "SE", "api-prod-1" -> "AP", "x" -> "X".
            let initials: String = label
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| !w.is_empty())
                .take(2)
                .filter_map(|w| w.chars().next())
                .map(|c| c.to_ascii_uppercase())
                .collect();
            let display = if initials.is_empty() {
                "?".to_string()
            } else {
                initials
            };
            // Pick a foreground that reads against the filled color.
            // Cheap luminance heuristic: dark backgrounds get white,
            // light backgrounds get the app's text_primary.
            let lum = 0.299 * color.r + 0.587 * color.g + 0.114 * color.b;
            let fg = if lum < 0.55 {
                Color::WHITE
            } else {
                OryxisColors::t().text_primary
            };
            container(text(display).size((size * 0.45).max(8.0)).color(fg))
                .center_x(Length::Fixed(size))
                .center_y(Length::Fixed(size))
                .style(move |_| container::Style {
                    background: Some(Background::Color(color)),
                    border: Border { radius: Radius::from(half), ..Default::default() },
                    ..Default::default()
                })
                .into()
        }
    }
}
