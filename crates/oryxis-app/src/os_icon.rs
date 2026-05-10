//! Brand icon resolution: map OS distros and cloud providers to embedded
//! mono SVG glyphs.
//!
//! Each brand icon is a normalized mono SVG (currentColor fills) bundled
//! at compile time via `include_bytes!`. At render time the chip's
//! background takes the brand colour and the SVG path is painted on top
//! in white via `Svg::style()`. Unknown OS ids fall through to the
//! generic Tux glyph.
//!
//! The id taxonomy:
//!   - **Canonical**: matches the SVG filename (e.g. `ubuntu`, `archlinux`,
//!     `aws`, `kubernetes`). Stored in `BRAND_ICONS` and `brand_color`.
//!   - **Aliases**: every flavour we receive from the wild
//!     (`/etc/os-release`'s `ID=`, SSH usernames, custom-icon picker
//!     legacy ids with `si:` prefix). All routed through
//!     `canonical_brand_id`.

use std::borrow::Cow;

use iced::widget::svg::Handle;
use iced::widget::Text;
use iced::Color;

// ---------------------------------------------------------------------------
// Brand icon registry
// ---------------------------------------------------------------------------

/// Embedded brand-icon SVG bytes. The id is the canonical brand slug,
/// matching the filename in `resources/icons/brand/`. Lookup is linear:
/// 43 entries, called only on render.
macro_rules! brand_svg {
    ($name:literal) => {
        (
            $name,
            include_bytes!(concat!(
                "../../../resources/icons/brand/",
                $name,
                ".svg"
            )) as &[u8],
        )
    };
}

const BRAND_ICONS: &[(&str, &[u8])] = &[
    brand_svg!("ubuntu"),
    brand_svg!("debian"),
    brand_svg!("archlinux"),
    brand_svg!("fedora"),
    brand_svg!("centos"),
    brand_svg!("redhat"),
    brand_svg!("alpinelinux"),
    brand_svg!("rockylinux"),
    brand_svg!("almalinux"),
    brand_svg!("opensuse"),
    brand_svg!("suse"),
    brand_svg!("freebsd"),
    brand_svg!("openbsd"),
    brand_svg!("netbsd"),
    brand_svg!("macos"),
    brand_svg!("gentoo"),
    brand_svg!("manjaro"),
    brand_svg!("kalilinux"),
    brand_svg!("raspberrypi"),
    brand_svg!("nixos"),
    brand_svg!("deepin"),
    brand_svg!("linuxmint"),
    brand_svg!("zorin"),
    brand_svg!("popos"),
    brand_svg!("elementary"),
    brand_svg!("linux"),
    brand_svg!("docker"),
    brand_svg!("openwrt"),
    brand_svg!("truenas"),
    brand_svg!("openmediavault"),
    brand_svg!("googlechrome"),
    brand_svg!("mxlinux"),
    brand_svg!("endeavouros"),
    brand_svg!("proxmox"),
    brand_svg!("vmware"),
    brand_svg!("unraid"),
    brand_svg!("pfsense"),
    brand_svg!("opnsense"),
    brand_svg!("talos"),
    brand_svg!("aws"),
    brand_svg!("amazonlinux"),
    brand_svg!("ecs"),
    brand_svg!("windows"),
    brand_svg!("oracle"),
    brand_svg!("kubernetes"),
];

fn brand_handle(id: &str) -> Option<Handle> {
    BRAND_ICONS
        .iter()
        .find(|(k, _)| *k == id)
        .map(|(_, bytes)| Handle::from_memory(Cow::Borrowed(*bytes)))
}

/// Brand colour for a canonical brand id. Used as the chip background.
/// Returns `None` for ids we know about but didn't pick a colour for
/// (caller should use the supplied fallback colour in that case).
fn brand_color(id: &str) -> Option<Color> {
    Some(match id {
        "ubuntu" => Color::from_rgb8(0xE9, 0x54, 0x20),
        "debian" => Color::from_rgb8(0xA8, 0x1D, 0x33),
        "archlinux" => Color::from_rgb8(0x17, 0x93, 0xD1),
        "fedora" => Color::from_rgb8(0x51, 0xA2, 0xDA),
        "centos" => Color::from_rgb8(0x26, 0x25, 0x77),
        "redhat" => Color::from_rgb8(0xEE, 0x00, 0x00),
        "alpinelinux" => Color::from_rgb8(0x0D, 0x59, 0x7F),
        "rockylinux" => Color::from_rgb8(0x10, 0xB9, 0x81),
        "almalinux" => Color::from_rgb8(0x00, 0x00, 0x00),
        "opensuse" | "suse" => Color::from_rgb8(0x73, 0xBA, 0x25),
        "freebsd" => Color::from_rgb8(0xAB, 0x2B, 0x28),
        "openbsd" | "netbsd" => Color::from_rgb8(0xFA, 0xDA, 0x64),
        "macos" => Color::from_rgb8(0x30, 0x30, 0x30),
        "gentoo" => Color::from_rgb8(0x54, 0x48, 0x7A),
        "manjaro" => Color::from_rgb8(0x35, 0xBF, 0xA4),
        // Kali's brand grey-blue (#557C94) blends into dark sidebars,
        // so we bump it to the documentation blue Termius uses.
        "kalilinux" => Color::from_rgb8(0x19, 0x76, 0xD2),
        "raspberrypi" => Color::from_rgb8(0xA2, 0x28, 0x46),
        "nixos" => Color::from_rgb8(0x52, 0x77, 0xC3),
        "deepin" => Color::from_rgb8(0x00, 0x7C, 0xFF),
        "linuxmint" => Color::from_rgb8(0x86, 0xBE, 0x43),
        "zorin" => Color::from_rgb8(0x15, 0xA6, 0xF0),
        "popos" => Color::from_rgb8(0x48, 0xB9, 0xC7),
        "elementary" => Color::from_rgb8(0x64, 0xBA, 0xFF),
        "linux" => Color::from_rgb8(0xFC, 0xC6, 0x24),
        "docker" => Color::from_rgb8(0x24, 0x96, 0xED),
        "openwrt" => Color::from_rgb8(0x00, 0xB5, 0xE2),
        "truenas" => Color::from_rgb8(0x0E, 0x4E, 0xA8),
        "openmediavault" => Color::from_rgb8(0x5A, 0xAF, 0xC6),
        "googlechrome" => Color::from_rgb8(0x44, 0x85, 0xF4),
        "mxlinux" => Color::from_rgb8(0x00, 0x69, 0x97),
        "endeavouros" => Color::from_rgb8(0x7F, 0x3F, 0xBF),
        "proxmox" => Color::from_rgb8(0xE5, 0x70, 0x00),
        "vmware" => Color::from_rgb8(0x60, 0x70, 0x78),
        "unraid" => Color::from_rgb8(0xF1, 0x52, 0x2D),
        "pfsense" => Color::from_rgb8(0x21, 0x2C, 0x53),
        "opnsense" => Color::from_rgb8(0xD9, 0x42, 0x22),
        "talos" => Color::from_rgb8(0x36, 0x46, 0x6A),
        // Cloud / corporate
        "aws" => Color::from_rgb8(0xFF, 0x99, 0x00),
        // Amazon Linux mascot bird, same orange family as AWS but kept
        // separate so the brand reads as the OS, not the cloud provider.
        "amazonlinux" => Color::from_rgb8(0xF3, 0x99, 0x1D),
        // ECS shares AWS orange so the chip reads "AWS family" but
        // the glyph (the hexagonal-box logo) tells you which service.
        "ecs" => Color::from_rgb8(0xFF, 0x99, 0x00),
        "windows" => Color::from_rgb8(0x00, 0x78, 0xD4),
        "oracle" => Color::from_rgb8(0xC7, 0x4A, 0x3D),
        "kubernetes" => Color::from_rgb8(0x32, 0x6C, 0xE5),
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Alias resolution
// ---------------------------------------------------------------------------

/// Map any raw OS / brand id to a canonical brand slug present in
/// `BRAND_ICONS`. Strips the legacy `si:` prefix that early storage
/// formats carried over from the Simple-Icons era. Returns `None` when
/// no brand entry matches; callers should fall back to the Tux glyph.
pub(crate) fn canonical_brand_id(id: &str) -> Option<&'static str> {
    let id = id.strip_prefix("si:").unwrap_or(id);
    Some(match id {
        // OS distros (canonical = filename)
        "ubuntu" => "ubuntu",
        "debian" => "debian",
        "archlinux" | "arch" => "archlinux",
        "fedora" => "fedora",
        "centos" => "centos",
        "redhat" | "rhel" => "redhat",
        "alpinelinux" | "alpine" => "alpinelinux",
        "rockylinux" | "rocky" => "rockylinux",
        "almalinux" | "alma" => "almalinux",
        "opensuse" | "opensuse-leap" | "opensuse-tumbleweed" => "opensuse",
        "suse" | "sles" | "suselinuxenterprise" => "suse",
        "freebsd" => "freebsd",
        "openbsd" => "openbsd",
        "netbsd" => "netbsd",
        "macos" | "darwin" => "macos",
        "gentoo" => "gentoo",
        "manjaro" => "manjaro",
        "kalilinux" | "kali" => "kalilinux",
        "raspberrypi" | "raspbian" | "raspberry_pi_os" => "raspberrypi",
        "nixos" => "nixos",
        "deepin" => "deepin",
        "linuxmint" | "mint" => "linuxmint",
        "zorin" | "zorinos" => "zorin",
        "popos" | "pop" | "pop_os" => "popos",
        "elementary" | "elementaryos" => "elementary",
        "linux" => "linux",
        "docker" | "docker-desktop" => "docker",
        "openwrt" => "openwrt",
        "truenas" | "freenas" => "truenas",
        "openmediavault" | "omv" => "openmediavault",
        "googlechrome" | "chrome" | "chromeos" | "chromeosflex" => "googlechrome",
        "mxlinux" => "mxlinux",
        "endeavouros" => "endeavouros",
        "proxmox" | "proxmoxve" | "pve" => "proxmox",
        "vmware" | "esxi" | "vmwareesxi" | "vsphere" => "vmware",
        "unraid" => "unraid",
        "pfsense" => "pfsense",
        "opnsense" => "opnsense",
        "talos" | "talosos" => "talos",
        // Amazon Linux distros land on the dedicated bird mascot. The
        // generic `aws` smile-arrow is reserved for cases where the
        // host is just *hosted on* AWS (EC2-bootstrapped images
        // without a detected OS, or username-only hints like
        // `ec2-user` before the silent OS probe runs).
        "amzn" | "amazonlinux" | "amazonlinux2" | "amazonlinux2023" => "amazonlinux",
        "aws" | "amazon" | "amazonec2" | "amazonwebservices"
        | "ec2-user" | "ssm-user" => "aws",
        "ecs" | "amazonecs" | "ecstask" | "ecstasks"
        | "elasticcontainerservice" => "ecs",
        "windows" | "windows10" | "windows11" | "powershell" | "pwsh"
        | "cmd" | "command_prompt" | "microsoftwindows" => "windows",
        "oracle" | "oraclelinux" | "ol" => "oracle",
        "kubernetes" | "k8s" => "kubernetes",
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Username inference (Termius-style: ec2-user → AWS chip even before the
// silent OS probe runs).
// ---------------------------------------------------------------------------

pub(crate) fn infer_from_username(username: &str) -> Option<&'static str> {
    let u = username.trim().to_ascii_lowercase();
    Some(match u.as_str() {
        "ec2-user" | "ssm-user" => "aws",
        "ubuntu" => "ubuntu",
        "debian" => "debian",
        "fedora" => "fedora",
        "centos" => "centos",
        "rocky" => "rockylinux",
        "almalinux" | "alma" => "almalinux",
        "alpine" => "alpinelinux",
        "arch" => "archlinux",
        "opensuse" | "suse" => "opensuse",
        "freebsd" => "freebsd",
        "openbsd" => "openbsd",
        "netbsd" => "netbsd",
        "kali" => "kalilinux",
        "manjaro" => "manjaro",
        "pi" => "raspberrypi",
        "core" => "alpinelinux",
        "bitnami" => "debian",
        _ => return None,
    })
}

/// Derive an OS hint from a Local Shell tab label so the tab chip can
/// pick a brand-correct icon. Handles `"<distro> (WSL)"` (the shape
/// `wsl --list --quiet` produces) plus the PowerShell / cmd label we
/// surface for native Windows shells. Returns `None` for labels that
/// don't look like local-shell entries.
pub(crate) fn local_shell_os_hint(label: &str) -> Option<String> {
    if let Some(distro) = label.strip_suffix(" (WSL)") {
        return Some(distro.to_ascii_lowercase());
    }
    let lower = label.to_ascii_lowercase();
    if lower.contains("powershell") || lower == "command prompt" || lower == "cmd" {
        return Some("windows".into());
    }
    None
}

// ---------------------------------------------------------------------------
// Color helpers
// ---------------------------------------------------------------------------

pub(crate) fn parse_hex_color(s: &str) -> Option<Color> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some(Color::from_rgb8(r, g, b))
}

// ---------------------------------------------------------------------------
// Custom icon picker, library of user-selectable icons.
// ---------------------------------------------------------------------------

/// Entries shown in the icon picker grid. `id` is persisted in
/// `Connection.custom_icon`. Brand ids resolve via `canonical_brand_id`
/// to a real SVG; bare ids fall through to a Lucide UI glyph.
pub(crate) const CUSTOM_ICONS: &[(&str, &str)] = &[
    // Distros
    ("ubuntu", "Ubuntu"),
    ("debian", "Debian"),
    ("archlinux", "Arch"),
    ("fedora", "Fedora"),
    ("centos", "CentOS"),
    ("redhat", "Red Hat"),
    ("alpinelinux", "Alpine"),
    ("rockylinux", "Rocky"),
    ("almalinux", "Alma"),
    ("opensuse", "openSUSE"),
    ("linux", "Linux"),
    ("aws", "Amazon"),
    ("amazonlinux", "Amazon Linux"),
    ("freebsd", "FreeBSD"),
    ("macos", "macOS"),
    ("gentoo", "Gentoo"),
    ("manjaro", "Manjaro"),
    ("kalilinux", "Kali"),
    ("raspberrypi", "Raspberry Pi"),
    ("nixos", "NixOS"),
    ("linuxmint", "Mint"),
    // Cloud + corporate
    ("aws", "AWS"),
    ("windows", "Windows"),
    ("oracle", "Oracle"),
    ("kubernetes", "Kubernetes"),
    ("docker", "Docker"),
    ("googlechrome", "Chrome"),
    ("vmware", "VMware"),
    ("proxmox", "Proxmox"),
    // Infra generic (Lucide UI fallback)
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

/// Preset color palette shown in the picker.
pub(crate) const PRESET_COLORS: &[&str] = &[
    "#E95420", "#A81D33", "#1793D1", "#51A2DA", "#262577",
    "#EE0000", "#0D597F", "#10B981", "#73BA25", "#FCC624",
    "#FF9900", "#AB2B28", "#35BFA4", "#5277C3", "#86BE43",
    "#00B4D8", "#F472B6", "#BD93F9", "#F5A623", "#5FD365",
    "#E86262", "#888888", "#202020", "#FFFFFF",
];

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Resolved brand icon. Brand ids return `Svg(handle)`; non-brand custom
/// picker ids (server, lock, rocket, ...) return `Glyph(text)` from the
/// Lucide UI font.
pub(crate) enum BrandIcon {
    Glyph(Text<'static>),
    Svg(Handle),
}

impl From<Text<'static>> for BrandIcon {
    /// Lucide UI glyphs (`iced_fonts::lucide::*()`) coerce into the
    /// `Glyph` variant automatically, so call sites that historically
    /// passed a `Text` can keep passing one without manual wrapping.
    fn from(text: Text<'static>) -> Self {
        BrandIcon::Glyph(text)
    }
}

impl BrandIcon {
    /// Render the icon at `size` px painted in `color`. For SVG the
    /// `color` is applied via `Svg::style()` (the SVG's `currentColor`
    /// fills resolve to this colour at raster time). For the Lucide
    /// glyph it's plain `Text::color()`.
    pub(crate) fn view<'a, Message>(
        self,
        size: f32,
        color: Color,
    ) -> iced::Element<'a, Message>
    where
        Message: 'a,
    {
        match self {
            BrandIcon::Glyph(t) => t.size(size).color(color).into(),
            BrandIcon::Svg(handle) => iced::widget::svg(handle)
                .width(iced::Length::Fixed(size))
                .height(iced::Length::Fixed(size))
                .style(move |_theme: &iced::Theme, _| {
                    iced::widget::svg::Style { color: Some(color) }
                })
                .into(),
        }
    }
}

/// Map a custom-icon picker id to a `BrandIcon`. Brand entries return
/// the embedded SVG; everything else falls through to the Lucide UI
/// glyph table.
pub(crate) fn custom_icon_glyph(id: &str) -> BrandIcon {
    if let Some(canonical) = canonical_brand_id(id)
        && let Some(handle) = brand_handle(canonical)
    {
        return BrandIcon::Svg(handle);
    }
    BrandIcon::Glyph(match id {
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
        "key_round" => iced_fonts::lucide::key_round(),
        "lock" => iced_fonts::lucide::lock(),
        "shield" => iced_fonts::lucide::shield(),
        "fingerprint" => iced_fonts::lucide::fingerprint(),
        "user_cog" => iced_fonts::lucide::user_cog(),
        "eye" => iced_fonts::lucide::eye(),
        "zap" => iced_fonts::lucide::zap(),
        "rocket" => iced_fonts::lucide::rocket(),
        "package" => iced_fonts::lucide::package(),
        "boxes" => iced_fonts::lucide::boxes(),
        "archive" => iced_fonts::lucide::archive(),
        "wrench" => iced_fonts::lucide::wrench(),
        "cog" => iced_fonts::lucide::cog(),
        "flame" => iced_fonts::lucide::flame(),
        "bot" => iced_fonts::lucide::bot(),
        "code" => iced_fonts::lucide::code(),
        "file_code" => iced_fonts::lucide::file_code(),
        "git_branch" => iced_fonts::lucide::git_branch(),
        "bug" => iced_fonts::lucide::bug(),
        "activity" => iced_fonts::lucide::activity(),
        "brain" => iced_fonts::lucide::brain(),
        "briefcase" => iced_fonts::lucide::briefcase(),
        "building" => iced_fonts::lucide::building(),
        "factory" => iced_fonts::lucide::factory(),
        "store" => iced_fonts::lucide::store(),
        "warehouse" => iced_fonts::lucide::warehouse(),
        "home" => iced_fonts::lucide::house(),
        "star" => iced_fonts::lucide::star(),
        "heart" => iced_fonts::lucide::heart(),
        "tag" => iced_fonts::lucide::tag(),
        "flag" => iced_fonts::lucide::flag(),
        "bookmark" => iced_fonts::lucide::bookmark(),
        "folder" => iced_fonts::lucide::folder(),
        _ => iced_fonts::lucide::server(),
    })
}

/// (BrandIcon, brand colour) for a cloud provider id. Used on cloud
/// account cards. AWS and Kubernetes hit the SVG registry; unknown
/// providers get the Lucide cloud glyph at the caller's fallback colour.
pub(crate) fn provider_icon(provider: &str, fallback_color: Color) -> (BrandIcon, Color) {
    match provider {
        "aws" => (
            BrandIcon::Svg(brand_handle("aws").expect("aws SVG bundled")),
            brand_color("aws").expect("aws color set"),
        ),
        "ecs" => (
            BrandIcon::Svg(brand_handle("ecs").expect("ecs SVG bundled")),
            brand_color("ecs").expect("ecs color set"),
        ),
        "k8s" | "kubernetes" => (
            BrandIcon::Svg(brand_handle("kubernetes").expect("kubernetes SVG bundled")),
            brand_color("kubernetes").expect("kubernetes color set"),
        ),
        _ => (
            BrandIcon::Glyph(iced_fonts::lucide::cloud()),
            fallback_color,
        ),
    }
}

/// Resolve an OS id to (BrandIcon, bg_color). Falls back to the bundled
/// Tux SVG painted at `fallback_color` when the id matches no known
/// alias.
pub(crate) fn resolve_icon(os: Option<&str>, fallback_color: Color) -> (BrandIcon, Color) {
    if let Some(id) = os
        && let Some(canonical) = canonical_brand_id(id)
        && let Some(handle) = brand_handle(canonical)
    {
        return (
            BrandIcon::Svg(handle),
            brand_color(canonical).unwrap_or(fallback_color),
        );
    }
    // Tux fallback, painted at the caller's fallback colour so
    // disconnected hosts read in accent-colour and connected ones in
    // the success-green chip.
    (
        BrandIcon::Svg(brand_handle("linux").expect("linux Tux SVG bundled")),
        fallback_color,
    )
}

/// Resolves (BrandIcon, brand colour) for a connection. Precedence:
///   1. `custom_color` / `custom_icon` overrides.
///   2. `detected_os` → brand SVG + brand colour.
///   3. Username hint (`ec2-user` → aws, `ubuntu` → ubuntu, ...).
///   4. Tux SVG painted at `fallback_color`.
pub(crate) fn resolve_for(
    detected_os: Option<&str>,
    custom_icon: Option<&str>,
    custom_color: Option<&str>,
    username: Option<&str>,
    fallback_color: Color,
) -> (BrandIcon, Color) {
    // Custom overrides take priority.
    if custom_icon.is_some() || custom_color.is_some() {
        let (icon, brand_col) = match custom_icon {
            Some(id) => {
                let icon = custom_icon_glyph(id);
                let col = canonical_brand_id(id).and_then(brand_color);
                (icon, col)
            }
            None => {
                let (icon, col) = resolve_icon(detected_os, fallback_color);
                (icon, Some(col))
            }
        };
        let color = custom_color
            .and_then(parse_hex_color)
            .or(brand_col)
            .unwrap_or(fallback_color);
        return (icon, color);
    }

    // Auto path: detected_os wins, username is a fallback hint.
    let inferred = match detected_os {
        Some(os) => Some(os.to_string()),
        None => username.and_then(infer_from_username).map(String::from),
    };
    resolve_icon(inferred.as_deref(), fallback_color)
}
