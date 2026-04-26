//! Map detected OS id → Simple Icons glyph + brand color. Used by host
//! cards / tab badges / the new-tab picker / editor to visually identify a
//! server's distribution.
//!
//! The OS id comes from `/etc/os-release` (`ID=` field) or `uname -s` for
//! non-Linux systems — see `oryxis_ssh::SshSession::detect_os`.

use iced::Color;
use iced::widget::Text;
use iced::widget::text;

/// Font family name as declared inside the bundled `SimpleIcons.ttf` name
/// table. Matched by iced/cosmic-text when we build a Text with this font.
const SI_FAMILY: &str = "Simple Icons";

fn si_text<'a>(codepoint: u32) -> Text<'a> {
    let c = char::from_u32(codepoint).unwrap_or('\u{25A1}');
    text(c.to_string()).font(iced::Font::new(SI_FAMILY))
}

fn parse_hex_color(s: &str) -> Option<Color> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 { return None; }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some(Color::from_rgb8(r, g, b))
}

/// Distro / OS → (Simple Icons codepoint, official brand color).
/// Keys are the lowercase ids we get from `/etc/os-release` or `uname -s`.
fn distro_entry(os: &str) -> Option<(u32, Color)> {
    Some(match os {
        "ubuntu"      => (0xF622, Color::from_rgb8(0xE9, 0x54, 0x20)),
        "debian"      => (0xECCF, Color::from_rgb8(0xA8, 0x1D, 0x33)),
        "arch" | "archlinux" => (0xEAD2, Color::from_rgb8(0x17, 0x93, 0xD1)),
        "fedora"      => (0xEDBB, Color::from_rgb8(0x51, 0xA2, 0xDA)),
        "centos"      => (0xEBDC, Color::from_rgb8(0x26, 0x25, 0x77)),
        "rhel" | "redhat" => (0xF3A6, Color::from_rgb8(0xEE, 0x00, 0x00)),
        "alpine"      => (0xEA6E, Color::from_rgb8(0x0D, 0x59, 0x7F)),
        "rocky" | "rockylinux" => (0xF3F0, Color::from_rgb8(0x10, 0xB9, 0x81)),
        "alma" | "almalinux"   => (0xEA6D, Color::from_rgb8(0x00, 0x00, 0x00)),
        "suse" | "opensuse" | "opensuse-leap" | "opensuse-tumbleweed"
                      => (0xF23A, Color::from_rgb8(0x73, 0xBA, 0x25)),
        // "amzn" / "amazon" handled separately in `resolve_icon` and
        // `custom_icon_glyph` — Simple Icons v16.17 has no Amazon glyph, so
        // we fall back to the Lucide `bird` icon in Amazon orange.
        "freebsd"     => (0xEE13, Color::from_rgb8(0xAB, 0x2B, 0x28)),
        "openbsd" | "netbsd" => (0xF092, Color::from_rgb8(0xFA, 0xDA, 0x64)),
        "darwin" | "macos" => (0xEAC1, Color::from_rgb8(0x30, 0x30, 0x30)),
        "gentoo"      => (0xEE42, Color::from_rgb8(0x54, 0x48, 0x7A)),
        "manjaro"     => (0xF0E5, Color::from_rgb8(0x35, 0xBF, 0xA4)),
        // Simple Icons brand value for Kali is #557C94 (washed-out blue
        // gray) — visually it lands almost the same as the dark sidebar
        // bg and the dragon glyph disappears. Bumped to Kali's primary
        // documentation blue (closer to what Termius renders) so the
        // host card actually has the recognisable navy chip.
        "kali"        => (0xEFF1, Color::from_rgb8(0x19, 0x76, 0xD2)),
        "raspbian" | "raspberry_pi_os" => (0xF38B, Color::from_rgb8(0xA2, 0x28, 0x46)),
        "nixos"       => (0xF1CC, Color::from_rgb8(0x52, 0x77, 0xC3)),
        "deepin"      => (0xECD5, Color::from_rgb8(0x00, 0x7C, 0xFF)),
        "linuxmint" | "mint" => (0xF095, Color::from_rgb8(0x86, 0xBE, 0x43)),
        "zorin"       => (0xF75D, Color::from_rgb8(0x15, 0xA6, 0xF0)),
        "pop" | "pop_os" | "popos" => (0xF2F9, Color::from_rgb8(0x48, 0xB9, 0xC7)),
        "elementary"  => (0xED61, Color::from_rgb8(0x64, 0xBA, 0xFF)),
        "linux"       => (0xF092, Color::from_rgb8(0xFC, 0xC6, 0x24)),
        _ => return None,
    })
}

/// Resolves (icon widget, brand color) for an OS id. Falls back to the
/// generic Lucide `server` glyph in `fallback_color` when unknown.
const AMAZON_ORANGE: Color = Color::from_rgb(0xFF as f32 / 255.0, 0x99 as f32 / 255.0, 0.0);

pub(crate) fn resolve_icon<'a>(
    os: Option<&str>,
    fallback_color: Color,
) -> (Text<'a>, Color) {
    if let Some(id) = os {
        let lower = id.to_lowercase();
        if matches!(lower.as_str(), "amzn" | "amazon" | "amazonlinux") {
            return (iced_fonts::lucide::bird(), AMAZON_ORANGE);
        }
        if let Some((cp, color)) = distro_entry(&lower) {
            return (si_text(cp), color);
        }
    }
    (iced_fonts::lucide::server(), fallback_color)
}

// ---------------------------------------------------------------------------
// Custom icon picker — library of user-selectable icons.
// ---------------------------------------------------------------------------

/// Entries shown in the icon picker grid. `id` is persisted in
/// `Connection.custom_icon`. IDs prefixed `si:` resolve to a Simple Icons
/// codepoint; everything else falls back to Lucide.
pub(crate) const CUSTOM_ICONS: &[(&str, &str)] = &[
    // Distros (Simple Icons brand marks)
    ("si:ubuntu", "Ubuntu"),
    ("si:debian", "Debian"),
    ("si:archlinux", "Arch"),
    ("si:fedora", "Fedora"),
    ("si:centos", "CentOS"),
    ("si:redhat", "Red Hat"),
    ("si:alpine", "Alpine"),
    ("si:rocky", "Rocky"),
    ("si:alma", "Alma"),
    ("si:opensuse", "openSUSE"),
    ("si:linux", "Linux"),
    ("si:amazon", "Amazon"),
    ("si:freebsd", "FreeBSD"),
    ("si:apple", "Apple"),
    ("si:gentoo", "Gentoo"),
    ("si:manjaro", "Manjaro"),
    ("si:kali", "Kali"),
    ("si:raspbian", "Raspberry Pi"),
    ("si:nixos", "NixOS"),
    ("si:mint", "Mint"),
    // Cloud
    ("si:aws", "AWS"),
    ("si:gcp", "GCP"),
    ("si:azure", "Azure"),
    ("si:digitalocean", "DigitalOcean"),
    ("si:cloudflare", "Cloudflare"),
    ("si:oraclecloud", "Oracle"),
    ("si:vercel", "Vercel"),
    ("si:netlify", "Netlify"),
    // Infra generic (Lucide)
    ("server", "Server"),
    ("terminal", "Terminal"),
    ("database", "Database"),
    ("cloud", "Cloud"),
    ("container", "Container"),
    ("cpu", "CPU"),
    ("hard_drive", "Disk"),
    ("network", "Network"),
    ("globe", "Globe"),
    ("wifi", "Wi-Fi"),
    ("monitor", "Monitor"),
    ("router", "Router"),
    // Security
    ("key_round", "Key"),
    ("lock", "Lock"),
    ("shield", "Shield"),
    ("fingerprint", "Fingerprint"),
    ("user_cog", "Admin"),
    ("eye", "Watch"),
    // Utility
    ("zap", "Bolt"),
    ("rocket", "Rocket"),
    ("package", "Package"),
    ("boxes", "Boxes"),
    ("archive", "Archive"),
    ("wrench", "Wrench"),
    ("cog", "Cog"),
    ("flame", "Flame"),
    ("bot", "Bot"),
    // Data / Dev
    ("code", "Code"),
    ("file_code", "Code File"),
    ("git_branch", "Branch"),
    ("bug", "Bug"),
    ("activity", "Activity"),
    ("brain", "AI"),
    // Work / Org
    ("briefcase", "Business"),
    ("building", "Building"),
    ("factory", "Factory"),
    ("store", "Store"),
    ("warehouse", "Warehouse"),
    ("home", "Home"),
    // Fun
    ("star", "Star"),
    ("heart", "Heart"),
    ("tag", "Tag"),
    ("flag", "Flag"),
    ("bookmark", "Bookmark"),
    ("folder", "Folder"),
];

/// Additional Simple Icons codepoints used by `CUSTOM_ICONS` entries that
/// don't correspond to an OS probe (clouds etc.). Keys match the substring
/// after `si:` in the picker id.
fn si_codepoint_for(id: &str) -> Option<u32> {
    // OS / distro first (share with `distro_entry`).
    if let Some((cp, _)) = distro_entry(id) { return Some(cp); }
    Some(match id {
        "aws" | "amazonec2" | "amazonwebservices" => 0xF0CB, // Amazon Web Services wordmark
        "gcp" | "googlecloud" => 0xEE91,
        "azure" | "microsoftazure" => 0xF1D9,
        "digitalocean" => 0xECFC,
        "cloudflare" => 0xEC57,
        "oraclecloud" | "oracle" => 0xF24E,
        "vercel" => 0xF65B,
        "netlify" => 0xF1C5,
        _ => return None,
    })
}

/// Preset color palette shown in the picker.
pub(crate) const PRESET_COLORS: &[&str] = &[
    "#E95420", "#A81D33", "#1793D1", "#51A2DA", "#262577",
    "#EE0000", "#0D597F", "#10B981", "#73BA25", "#FCC624",
    "#FF9900", "#AB2B28", "#35BFA4", "#5277C3", "#86BE43",
    "#00B4D8", "#F472B6", "#BD93F9", "#F5A623", "#5FD365",
    "#E86262", "#888888", "#202020", "#FFFFFF",
];

/// Resolve a picker id into a Text glyph. `si:<name>` goes through Simple
/// Icons; bare ids fall through to Lucide.
pub(crate) fn custom_icon_glyph<'a>(id: &str) -> Text<'a> {
    if let Some(rest) = id.strip_prefix("si:") {
        if matches!(rest, "amazon" | "amzn" | "amazonlinux") {
            return iced_fonts::lucide::bird();
        }
        if let Some(cp) = si_codepoint_for(rest) {
            return si_text(cp);
        }
    }
    match id {
        // Infra
        "terminal" => iced_fonts::lucide::terminal(),
        "database" => iced_fonts::lucide::database(),
        "cloud" => iced_fonts::lucide::cloud(),
        "container" => iced_fonts::lucide::container(),
        "cpu" => iced_fonts::lucide::cpu(),
        "hard_drive" => iced_fonts::lucide::hard_drive(),
        "network" => iced_fonts::lucide::network(),
        "globe" => iced_fonts::lucide::globe(),
        "wifi" => iced_fonts::lucide::wifi(),
        "monitor" => iced_fonts::lucide::monitor(),
        "router" => iced_fonts::lucide::router(),
        // Security
        "key_round" => iced_fonts::lucide::key_round(),
        "lock" => iced_fonts::lucide::lock(),
        "shield" => iced_fonts::lucide::shield(),
        "fingerprint" => iced_fonts::lucide::fingerprint(),
        "user_cog" => iced_fonts::lucide::user_cog(),
        "eye" => iced_fonts::lucide::eye(),
        // Utility
        "zap" => iced_fonts::lucide::zap(),
        "rocket" => iced_fonts::lucide::rocket(),
        "package" => iced_fonts::lucide::package(),
        "boxes" => iced_fonts::lucide::boxes(),
        "archive" => iced_fonts::lucide::archive(),
        "wrench" => iced_fonts::lucide::wrench(),
        "cog" => iced_fonts::lucide::cog(),
        "flame" => iced_fonts::lucide::flame(),
        "bot" => iced_fonts::lucide::bot(),
        // Data / Dev
        "code" => iced_fonts::lucide::code(),
        "file_code" => iced_fonts::lucide::file_code(),
        "git_branch" => iced_fonts::lucide::git_branch(),
        "bug" => iced_fonts::lucide::bug(),
        "activity" => iced_fonts::lucide::activity(),
        "brain" => iced_fonts::lucide::brain(),
        // Work / Org
        "briefcase" => iced_fonts::lucide::briefcase(),
        "building" => iced_fonts::lucide::building(),
        "factory" => iced_fonts::lucide::factory(),
        "store" => iced_fonts::lucide::store(),
        "warehouse" => iced_fonts::lucide::warehouse(),
        "home" => iced_fonts::lucide::house(),
        // Fun
        "star" => iced_fonts::lucide::star(),
        "heart" => iced_fonts::lucide::heart(),
        "tag" => iced_fonts::lucide::tag(),
        "flag" => iced_fonts::lucide::flag(),
        "bookmark" => iced_fonts::lucide::bookmark(),
        "folder" => iced_fonts::lucide::folder(),
        _ => iced_fonts::lucide::server(),
    }
}

/// Resolves (icon widget, brand color) for a connection. Precedence:
///   1. `custom_color` / `custom_icon` overrides.
///   2. `detected_os` → Simple Icons brand glyph + color.
///   3. `fallback_color` with generic server glyph.
pub(crate) fn resolve_for<'a>(
    detected_os: Option<&str>,
    custom_icon: Option<&str>,
    custom_color: Option<&str>,
    fallback_color: Color,
) -> (Text<'a>, Color) {
    if custom_icon.is_some() || custom_color.is_some() {
        let glyph = match custom_icon {
            Some(id) => custom_icon_glyph(id),
            None => resolve_icon(detected_os, fallback_color).0,
        };
        let color = custom_color
            .and_then(parse_hex_color)
            .unwrap_or_else(|| resolve_icon(detected_os, fallback_color).1);
        return (glyph, color);
    }
    resolve_icon(detected_os, fallback_color)
}
