//! On-demand font download + runtime load (CJK scripts + the
//! terminal font pack).
//!
//! Noto Sans (Latin / Cyrillic / Greek / Vietnamese) plus the Noto
//! Sans Arabic, Hebrew, Thai and Devanagari script fonts ship inside
//! the binary (see `main.rs`). The CJK scripts (Simplified and
//! Traditional Chinese, Japanese, Korean) are large (9-18 MB each) and
//! most users never need them, so they are fetched the first time the
//! user selects one of those languages, cached under
//! `~/.oryxis/fonts/`, integrity checked against a baked-in SHA-256,
//! then handed to the iced font system with `iced::font::load` so
//! cosmic-text falls back to them per codepoint.
//!
//! The terminal font pack (issue #109) rides the same machinery: a
//! curated short list of popular Nerd Font builds, downloaded the
//! first time the user picks one in the terminal font picker instead
//! of being bundled in the installers. Same cache directory, same
//! sha256 + byte-length pins, same mirror routing.
//!
//! A failed download degrades to the previous font (system CJK font /
//! whatever the terminal was rendering with) and never surfaces as a
//! hard error.

use std::path::PathBuf;

use futures_util::StreamExt;
use sha2::{Digest, Sha256};

use crate::i18n::Language;
use crate::messages::{Message, SettingsMessage};

/// Every font baked into the binary, in load order. `main.rs` feeds
/// these to the iced application builder one `.font()` call at a
/// time; the headless harness (`harness.rs`, feature `harness`) loads
/// the same list straight into the global font system, since the
/// emulator path never runs the shell's boot-time font loading.
///
/// The set, and why each entry is bundled:
/// - Lucide: the app's icon glyphs. Codicon: window chrome glyphs
///   (chrome-minimize/maximize/restore/close) matching the native
///   Windows title bar look that VS Code uses. Brand glyphs are
///   per-brand SVGs (`os_icon::BRAND_ICONS`), no font needed.
/// - Noto Sans (Regular / SemiBold / Bold): the single bundled UI
///   font across every platform, one standard look instead of per-OS
///   system fonts. Covers Latin, Latin Extended, Cyrillic, Greek and
///   Vietnamese, so most shipped languages render from the bundle
///   with no system font dependency. The three weights share the
///   "Noto Sans" typographic family (name ID 16), so weight selection
///   resolves to the right file. SIL OFL 1.1 (resources/fonts/OFL.txt).
/// - Noto Sans Arabic / Hebrew / Thai / Devanagari: small script
///   fonts (17-185 KB per weight) bundled so Arabic, Persian, Hebrew,
///   Thai and Hindi render offline; cosmic-text falls back to them
///   per-codepoint. CJK is the genuinely large script set and is
///   downloaded on demand instead (see this module).
/// - MenuCJK: tiny (~4 KB) subset holding only the glyphs of the
///   language-picker names (한국어 / 简体中文 / 繁體中文 / 日本語) so those
///   entries always render before the full CJK font is downloaded.
///   Distinct family ("Oryxis Menu CJK"), pure per-codepoint fallback.
/// - SauceCodePro Nerd Font (Regular / Medium): default terminal
///   font, Source Code Pro patched with the full Nerd Font glyph set
///   so Starship / Powerline prompts render out of the box. System
///   mono fonts lacking the PUA glyphs fall back to it per-codepoint
///   via the terminal widget's symbol_map.
/// - Symbols Nerd Font: same PUA set with no Latin coverage,
///   fallback-only so proportional text (chat, host labels, snippets)
///   can show Powerline/Devicon glyphs without going monospace.
pub static BUNDLED_FONTS: &[&[u8]] = &[
    iced_fonts::LUCIDE_FONT_BYTES,
    iced_fonts::CODICON_FONT_BYTES,
    include_bytes!("../../../resources/fonts/NotoSans-Regular.ttf"),
    include_bytes!("../../../resources/fonts/NotoSans-SemiBold.ttf"),
    include_bytes!("../../../resources/fonts/NotoSans-Bold.ttf"),
    include_bytes!("../../../resources/fonts/NotoSansArabic-Regular.ttf"),
    include_bytes!("../../../resources/fonts/NotoSansArabic-SemiBold.ttf"),
    include_bytes!("../../../resources/fonts/NotoSansArabic-Bold.ttf"),
    include_bytes!("../../../resources/fonts/NotoSansHebrew-Regular.ttf"),
    include_bytes!("../../../resources/fonts/NotoSansHebrew-SemiBold.ttf"),
    include_bytes!("../../../resources/fonts/NotoSansHebrew-Bold.ttf"),
    include_bytes!("../../../resources/fonts/NotoSansThai-Regular.ttf"),
    include_bytes!("../../../resources/fonts/NotoSansThai-SemiBold.ttf"),
    include_bytes!("../../../resources/fonts/NotoSansThai-Bold.ttf"),
    include_bytes!("../../../resources/fonts/NotoSansDevanagari-Regular.ttf"),
    include_bytes!("../../../resources/fonts/NotoSansDevanagari-SemiBold.ttf"),
    include_bytes!("../../../resources/fonts/NotoSansDevanagari-Bold.ttf"),
    include_bytes!("../../../resources/fonts/MenuCJK.ttf"),
    include_bytes!("../../../resources/fonts/SauceCodeProNerdFont-Regular.ttf"),
    include_bytes!("../../../resources/fonts/SauceCodeProNerdFont-Medium.ttf"),
    include_bytes!("../../../resources/fonts/SymbolsNerdFont-Regular.ttf"),
];

/// The monospace families that ship inside the binary, with the CSS
/// weight of every face bundled for each. The terminal font picker
/// reads this to know which weights a bundled family can serve
/// without touching the system font database (which never sees the
/// bundled faces: they are loaded straight into the iced font
/// system). Bundling another weight file in `BUNDLED_FONTS` means
/// adding it here too, or the picker will keep calling it missing.
pub static BUNDLED_MONO_WEIGHTS: &[(&str, &[u16])] =
    &[("SauceCodePro Nerd Font", &[400, 500])];

/// The terminal font weights the picker offers (issue #155).
///
/// Four values rather than the full CSS ladder: these are the ones
/// monospace families actually ship, and offering Thin / Black would
/// mostly be offering faces nobody has. The stored setting is the CSS
/// number as a string ("400".."700") so it reads the same way the
/// issue, the font files (`usWeightClass`) and every other terminal's
/// config do; an unknown value degrades to Regular, the same
/// forward-compatible posture as `TerminalAppearance::fit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TerminalFontWeight {
    #[default]
    Regular,
    Medium,
    SemiBold,
    Bold,
}

impl TerminalFontWeight {
    /// Picker order, lightest first.
    pub const ALL: [Self; 4] = [Self::Regular, Self::Medium, Self::SemiBold, Self::Bold];

    /// CSS numeric weight; what the settings row stores and what the
    /// pinned faces declare.
    pub fn css(self) -> u16 {
        match self {
            Self::Regular => 400,
            Self::Medium => 500,
            Self::SemiBold => 600,
            Self::Bold => 700,
        }
    }

    /// Parse a stored setting value. Anything unrecognized (an older
    /// build's value, a hand-edited row, a newer weight synced from a
    /// future version) reads as Regular rather than failing the boot.
    pub fn from_setting(value: &str) -> Self {
        match value.trim() {
            "500" => Self::Medium,
            "600" => Self::SemiBold,
            "700" => Self::Bold,
            _ => Self::Regular,
        }
    }

    /// The value written to the `terminal_font_weight` setting.
    pub fn setting_value(self) -> &'static str {
        match self {
            Self::Regular => "400",
            Self::Medium => "500",
            Self::SemiBold => "600",
            Self::Bold => "700",
        }
    }

    /// What the terminal widget asks cosmic-text for.
    pub fn font_weight(self) -> iced::font::Weight {
        match self {
            Self::Regular => iced::font::Weight::Normal,
            Self::Medium => iced::font::Weight::Medium,
            Self::SemiBold => iced::font::Weight::Semibold,
            Self::Bold => iced::font::Weight::Bold,
        }
    }

    /// i18n key of the weight's name.
    fn label_key(self) -> &'static str {
        match self {
            Self::Regular => "font_weight_regular",
            Self::Medium => "font_weight_medium",
            Self::SemiBold => "font_weight_semibold",
            Self::Bold => "font_weight_bold",
        }
    }
}

impl std::fmt::Display for TerminalFontWeight {
    /// Picker label: the translated name plus the CSS number, because
    /// the number is what font files, other terminals and the issue
    /// itself use, and it is the same in every language.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", crate::i18n::t(self.label_key()), self.css())
    }
}

/// How much extra stroke width the terminal paints on every glyph.
///
/// Our glyphs are rasterized to raw coverage (swash, 8-bit alpha) and
/// composited as-is. Every platform text stack widens strokes before
/// compositing and ours does not: on macOS the default
/// `AppleFontSmoothing` runs glyphs through Core Graphics smoothing,
/// which in the words of crossfont (the font crate alacritty uses)
/// "increases the stroke width". That is why the same font file at the
/// same size reads lighter here than in a terminal that rasterizes
/// through the OS, which is what issue #155 reported before it became
/// a request for heavier weights.
///
/// The values are logical pixels, not device pixels, so the widening
/// stays proportional to the glyph on a HiDPI display instead of
/// vanishing as the pixels get smaller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextThickness {
    /// Raw coverage, what every build before this one drew.
    None,
    Light,
    #[default]
    Medium,
    Strong,
}

impl TextThickness {
    /// Picker order, lightest first.
    pub const ALL: [Self; 4] = [Self::None, Self::Light, Self::Medium, Self::Strong];

    /// Stroke widening in logical pixels.
    pub fn px(self) -> f32 {
        match self {
            Self::None => 0.0,
            Self::Light => 0.2,
            Self::Medium => 0.3,
            Self::Strong => 0.45,
        }
    }

    /// The value written to the `terminal_text_thickness` setting. A
    /// token rather than the number so the pixel amounts stay tunable
    /// without migrating anybody's vault.
    pub fn setting_value(self) -> &'static str {
        match self {
            Self::None => "off",
            Self::Light => "light",
            Self::Medium => "medium",
            Self::Strong => "strong",
        }
    }

    /// Parse a stored setting value; anything unrecognized reads as the
    /// default, the same forward-compatible posture as the font weight.
    pub fn from_setting(value: &str) -> Self {
        match value.trim() {
            "off" => Self::None,
            "light" => Self::Light,
            "strong" => Self::Strong,
            _ => Self::Medium,
        }
    }

    fn label_key(self) -> &'static str {
        match self {
            Self::None => "text_thickness_off",
            Self::Light => "text_thickness_light",
            Self::Medium => "text_thickness_medium",
            Self::Strong => "text_thickness_strong",
        }
    }
}

impl std::fmt::Display for TextThickness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(crate::i18n::t(self.label_key()))
    }
}

/// One downloadable font file: pinned to an immutable commit URL
/// *and* to its SHA-256. To re-pin or move to a self-hosted mirror,
/// change `url` and `sha256` together. Shared by the CJK assets and
/// the terminal font pack.
struct FontAsset {
    /// Cache file name under `~/.oryxis/fonts/`.
    file: &'static str,
    /// Immutable (commit-pinned) download URL.
    url: &'static str,
    /// Expected SHA-256 of the bytes, lowercase hex.
    sha256: &'static str,
    /// Expected byte length. A cheap pre-check before hashing and the
    /// cache-hit validity test (guards against a truncated file).
    len: u64,
}

/// One downloadable CJK font, keyed by language. Each is a Noto Sans
/// regional variable TTF (all weights in one file).
struct CjkAsset {
    /// Short language code used as the in-memory "already loaded" guard
    /// key and the cache file stem.
    code: &'static str,
    /// The family this file is registered under, which is NOT the family
    /// the file declares (issue #189). cosmic-text resolves a Han
    /// codepoint the UI font can't draw by naming one family per locale,
    /// and on Linux that name is `Noto Sans CJK <region>`, never
    /// `Noto Sans <region>`. See `font_family` for why claiming it is
    /// honest and why the rename is in memory only.
    family: &'static str,
    asset: FontAsset,
}

// The pinned URLs below resolve against `google/fonts` commit
// `c89741abbf4eeabce432c3ed2fd7dc28b022701e`. A raw `githubusercontent`
// URL at a fixed commit is content-addressed, so the bytes can never
// change under the SHA-256 pin.

/// The four regional CJK fonts. Han unification means each regional
/// font only covers its own language's full alphabet (KR has Hangul,
/// JP has kana, SC has the simplified Han set, TC the traditional
/// set), so they are downloaded per language rather than as one
/// shared file.
static ASSETS: &[CjkAsset] = &[
    CjkAsset {
        code: "ko",
        family: "Noto Sans CJK KR",
        asset: FontAsset {
            file: "NotoSansKR.ttf",
            url: "https://raw.githubusercontent.com/google/fonts/c89741abbf4eeabce432c3ed2fd7dc28b022701e/ofl/notosanskr/NotoSansKR%5Bwght%5D.ttf",
            sha256: "194018e6b2b293a7964f037b25c0249ce1418bc9ab3c971060a03aa57861e252",
            len: 10_414_588,
        },
    },
    CjkAsset {
        code: "zh",
        family: "Noto Sans CJK SC",
        asset: FontAsset {
            file: "NotoSansSC.ttf",
            url: "https://raw.githubusercontent.com/google/fonts/c89741abbf4eeabce432c3ed2fd7dc28b022701e/ofl/notosanssc/NotoSansSC%5Bwght%5D.ttf",
            sha256: "a3041811a78c361b1de50f953c805e0244951c21c5bd412f7232ef0d899af0da",
            len: 17_772_300,
        },
    },
    CjkAsset {
        code: "ja",
        family: "Noto Sans CJK JP",
        asset: FontAsset {
            file: "NotoSansJP.ttf",
            url: "https://raw.githubusercontent.com/google/fonts/c89741abbf4eeabce432c3ed2fd7dc28b022701e/ofl/notosansjp/NotoSansJP%5Bwght%5D.ttf",
            sha256: "c2f3b4d463500a2ddcd3849cded1fceeb9fd6d1c32e6cbecd568453ba50fc68f",
            len: 9_589_900,
        },
    },
    CjkAsset {
        code: "zh-TW",
        family: "Noto Sans CJK TC",
        asset: FontAsset {
            file: "NotoSansTC.ttf",
            url: "https://raw.githubusercontent.com/google/fonts/c89741abbf4eeabce432c3ed2fd7dc28b022701e/ofl/notosanstc/NotoSansTC%5Bwght%5D.ttf",
            sha256: "864727d210d54f2537bbe23b3a839436c3992af72de9322af5270897246bd44f",
            len: 11_941_968,
        },
    },
];

// The pinned URLs below resolve against `ryanoasis/nerd-fonts` commit
// `fa7b859994228a9c8759f99c55a8d31ee92a1b5e` (the v3.4.0 tag), the
// last release whose patched TTFs are committed in-repo (v3.5.0
// removed them from the tree, leaving only the mutable release-zip
// assets, which a sha256 pin can't ride). Same content-addressed
// contract as the CJK pins above.

/// One pinned face of a pack family: the file that carries a single
/// weight. Kept separate from the family so a weight the user never
/// picks is never downloaded (issue #155).
pub struct PackFace {
    /// CSS weight this face declares (`usWeightClass`): 400 Regular,
    /// 500 Medium, 600 SemiBold, 700 Bold. Must match the file's own
    /// value, that is what cosmic-text matches the request against.
    pub weight: u16,
    asset: FontAsset,
}

impl PackFace {
    /// Stable per-face key for the app's "already requested" guard.
    /// The cache file name is unique across the whole catalog (the
    /// `download_pins_are_well_formed` test enforces it), so it keys
    /// a face without a second identifier to keep in sync.
    pub fn key(&self) -> &'static str {
        self.asset.file
    }
}

/// The terminal font pack (issue #109): a curated list of popular
/// Nerd Font builds, offered in the terminal font picker and
/// downloaded individually on first selection (a catalog, not a
/// bundle: the user only ever pays for the families they pick).
/// `family` is the exact typographic family name inside each TTF
/// (the in-repo builds are inconsistent about it: some use the short
/// "NF" suffix, most spell out "Nerd Font") - it is what the picker
/// stores in `terminal_font_name` and what cosmic-text resolves, so
/// the three must always agree per entry.
///
/// The three "NF" families are named after a name record every one of
/// their faces carries: the patched builds put the long form
/// ("JetBrainsMono Nerd Font") in the en-US typographic family and
/// the short form in the en-GB one, and fontdb keeps BOTH in
/// `FaceInfo::families` and matches a query against any of them. So
/// the family string below groups the whole weight set, whichever
/// spelling it uses. The non-Mono variant matches the bundled
/// SauceCodePro Nerd Font build.
///
/// `faces` is Regular first, then whatever heavier weights the pinned
/// commit actually ships for that family (three of them stop at Bold,
/// nothing invents a face upstream doesn't have): a weight with no
/// face is a request cosmic-text answers with the closest one it has,
/// and the picker says so rather than pretending the pick landed.
pub struct PackFont {
    /// Typographic family name (name ID 1/16) inside the TTF.
    pub family: &'static str,
    /// Every pinned face of the family, lightest first.
    pub faces: &'static [PackFace],
}

impl PackFont {
    /// The pinned face for exactly `weight`, if the family has one.
    /// Deliberately exact: a near miss is what cosmic-text's own
    /// matching is for, and downloading a 700 for a 500 request would
    /// spend the user's bandwidth on a face they did not ask for.
    pub fn face(&self, weight: u16) -> Option<&'static PackFace> {
        self.faces.iter().find(|f| f.weight == weight)
    }
}

pub static PACK_FONTS: &[PackFont] = &[
    PackFont {
        family: "JetBrainsMono NF",
        faces: &[
            PackFace {
                weight: 400,
                asset: FontAsset {
                    file: "JetBrainsMonoNerdFont-Regular.ttf",
                    url: "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/fa7b859994228a9c8759f99c55a8d31ee92a1b5e/patched-fonts/JetBrainsMono/Ligatures/Regular/JetBrainsMonoNerdFont-Regular.ttf",
                    sha256: "0ec29a68b539ece7078fc714cebff0c0accb2f4948f8f7963d9f5e86633b12d9",
                    len: 2_469_104,
                },
            },
            PackFace {
                weight: 500,
                asset: FontAsset {
                    file: "JetBrainsMonoNerdFont-Medium.ttf",
                    url: "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/fa7b859994228a9c8759f99c55a8d31ee92a1b5e/patched-fonts/JetBrainsMono/Ligatures/Medium/JetBrainsMonoNerdFont-Medium.ttf",
                    sha256: "04a099702e3e808a922c28c4a4da656e9ea783d6fa6bed33ae67f6f4e0afb937",
                    len: 2_468_976,
                },
            },
            PackFace {
                weight: 600,
                asset: FontAsset {
                    file: "JetBrainsMonoNerdFont-SemiBold.ttf",
                    url: "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/fa7b859994228a9c8759f99c55a8d31ee92a1b5e/patched-fonts/JetBrainsMono/Ligatures/SemiBold/JetBrainsMonoNerdFont-SemiBold.ttf",
                    sha256: "1d28a687259870de46378bf83e511d9c85136c597db678e6c4953b2731e55c72",
                    len: 2_472_212,
                },
            },
            PackFace {
                weight: 700,
                asset: FontAsset {
                    file: "JetBrainsMonoNerdFont-Bold.ttf",
                    url: "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/fa7b859994228a9c8759f99c55a8d31ee92a1b5e/patched-fonts/JetBrainsMono/Ligatures/Bold/JetBrainsMonoNerdFont-Bold.ttf",
                    sha256: "e82e27a7f37c9a0a13cc4e417503a149c6a0280586930772d2ebed803159c864",
                    len: 2_472_872,
                },
            },
        ],
    },
    // Cascadia ships SemiLight / Light / ExtraLight below Regular and
    // stops at Bold above it: no Medium exists upstream, so 500 is the
    // one weight this family answers with a neighbour.
    PackFont {
        family: "CaskaydiaCove NF",
        faces: &[
            PackFace {
                weight: 400,
                asset: FontAsset {
                    file: "CaskaydiaCoveNerdFont-Regular.ttf",
                    url: "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/fa7b859994228a9c8759f99c55a8d31ee92a1b5e/patched-fonts/CascadiaCode/CaskaydiaCoveNerdFont-Regular.ttf",
                    sha256: "701d7ec08f58f07251c1758361c5d1ab57ba0a867dd378cbb0fa52e1d2beccad",
                    len: 2_892_532,
                },
            },
            PackFace {
                weight: 600,
                asset: FontAsset {
                    file: "CaskaydiaCoveNerdFont-SemiBold.ttf",
                    url: "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/fa7b859994228a9c8759f99c55a8d31ee92a1b5e/patched-fonts/CascadiaCode/CaskaydiaCoveNerdFont-SemiBold.ttf",
                    sha256: "66522b4e54ab36e71e9b15817d74b9cb593e0b06a064adc876d4994f881d134e",
                    len: 2_893_648,
                },
            },
            PackFace {
                weight: 700,
                asset: FontAsset {
                    file: "CaskaydiaCoveNerdFont-Bold.ttf",
                    url: "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/fa7b859994228a9c8759f99c55a8d31ee92a1b5e/patched-fonts/CascadiaCode/CaskaydiaCoveNerdFont-Bold.ttf",
                    sha256: "d38b2e9461f52c70ef9f18c5c79f869d8b084432416ea6e850855c5222fbdc38",
                    len: 2_894_232,
                },
            },
        ],
    },
    PackFont {
        family: "FiraCode Nerd Font",
        faces: &[
            PackFace {
                weight: 400,
                asset: FontAsset {
                    file: "FiraCodeNerdFont-Regular.ttf",
                    url: "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/fa7b859994228a9c8759f99c55a8d31ee92a1b5e/patched-fonts/FiraCode/Regular/FiraCodeNerdFont-Regular.ttf",
                    sha256: "29b619655612cb273e034737408b9508a04beb63c1ddbdfaa9a6846c409c7a2e",
                    len: 2_642_616,
                },
            },
            PackFace {
                weight: 500,
                asset: FontAsset {
                    file: "FiraCodeNerdFont-Medium.ttf",
                    url: "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/fa7b859994228a9c8759f99c55a8d31ee92a1b5e/patched-fonts/FiraCode/Medium/FiraCodeNerdFont-Medium.ttf",
                    sha256: "1c8bd9c2949d924e138d3fe867680ec7a9009c741697260365e2de53f4b38b2d",
                    len: 2_636_736,
                },
            },
            PackFace {
                weight: 600,
                asset: FontAsset {
                    file: "FiraCodeNerdFont-SemiBold.ttf",
                    url: "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/fa7b859994228a9c8759f99c55a8d31ee92a1b5e/patched-fonts/FiraCode/SemiBold/FiraCodeNerdFont-SemiBold.ttf",
                    sha256: "2f27a15f24ebc756b7800b09cbdedf65065e3a1036bbbaa97ac3fb75eebd6ffb",
                    len: 2_657_272,
                },
            },
            PackFace {
                weight: 700,
                asset: FontAsset {
                    file: "FiraCodeNerdFont-Bold.ttf",
                    url: "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/fa7b859994228a9c8759f99c55a8d31ee92a1b5e/patched-fonts/FiraCode/Bold/FiraCodeNerdFont-Bold.ttf",
                    sha256: "594a51c86afb58c1160df286e45dc551eeeba4d5f0e6edb1c304fc43ab8a0a09",
                    len: 2_672_432,
                },
            },
        ],
    },
    PackFont {
        family: "Hack Nerd Font",
        faces: &[
            PackFace {
                weight: 400,
                asset: FontAsset {
                    file: "HackNerdFont-Regular.ttf",
                    url: "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/fa7b859994228a9c8759f99c55a8d31ee92a1b5e/patched-fonts/Hack/Regular/HackNerdFont-Regular.ttf",
                    sha256: "7e6b5d86baee613984b10cef14c8d6aee86c976a3d1cbd87abffd424d6ec4c64",
                    len: 2_685_912,
                },
            },
            PackFace {
                weight: 700,
                asset: FontAsset {
                    file: "HackNerdFont-Bold.ttf",
                    url: "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/fa7b859994228a9c8759f99c55a8d31ee92a1b5e/patched-fonts/Hack/Bold/HackNerdFont-Bold.ttf",
                    sha256: "7fb835cbd3273d509868dcd4e03eab3dc98679ac0bdffd52ef23411244396082",
                    len: 2_694_312,
                },
            },
        ],
    },
    // The nerd-fonts build of Meslo LG S, the face powerlevel10k made
    // the de-facto prompt standard (p10k's own "MesloLGS NF" is a
    // separate hand-tuned build; this official one names itself
    // "MesloLGS Nerd Font").
    PackFont {
        family: "MesloLGS Nerd Font",
        faces: &[
            PackFace {
                weight: 400,
                asset: FontAsset {
                    file: "MesloLGSNerdFont-Regular.ttf",
                    url: "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/fa7b859994228a9c8759f99c55a8d31ee92a1b5e/patched-fonts/Meslo/S/Regular/MesloLGSNerdFont-Regular.ttf",
                    sha256: "44ae9b687639c1529ecd01e5d0ae8d98f3b30cb20b02bbe4e6d9fb474c8dee36",
                    len: 2_853_324,
                },
            },
            PackFace {
                weight: 700,
                asset: FontAsset {
                    file: "MesloLGSNerdFont-Bold.ttf",
                    url: "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/fa7b859994228a9c8759f99c55a8d31ee92a1b5e/patched-fonts/Meslo/S/Bold/MesloLGSNerdFont-Bold.ttf",
                    sha256: "9799788a96066045c3f53f2e5bcbe376c6b9adb9514e3276cc84ac42d8b176d0",
                    len: 2_870_436,
                },
            },
        ],
    },
    PackFont {
        family: "RobotoMono Nerd Font",
        faces: &[
            PackFace {
                weight: 400,
                asset: FontAsset {
                    file: "RobotoMonoNerdFont-Regular.ttf",
                    url: "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/fa7b859994228a9c8759f99c55a8d31ee92a1b5e/patched-fonts/RobotoMono/Regular/RobotoMonoNerdFont-Regular.ttf",
                    sha256: "09605f6c29dcb12c007cbddc22170ce771d746fde4ed05b4b5d4dfc251e595fb",
                    len: 2_454_524,
                },
            },
            PackFace {
                weight: 500,
                asset: FontAsset {
                    file: "RobotoMonoNerdFont-Medium.ttf",
                    url: "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/fa7b859994228a9c8759f99c55a8d31ee92a1b5e/patched-fonts/RobotoMono/Medium/RobotoMonoNerdFont-Medium.ttf",
                    sha256: "d34887e9f09dcdea84efcc305f52632b27b86f9764318e9ebf265fee93d876f2",
                    len: 2_454_572,
                },
            },
            PackFace {
                weight: 600,
                asset: FontAsset {
                    file: "RobotoMonoNerdFont-SemiBold.ttf",
                    url: "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/fa7b859994228a9c8759f99c55a8d31ee92a1b5e/patched-fonts/RobotoMono/SemiBold/RobotoMonoNerdFont-SemiBold.ttf",
                    sha256: "c0503aea5692fbf67b348976e53538a18a8dfe75b9ce50aef26603e76bd3b820",
                    len: 2_454_884,
                },
            },
            PackFace {
                weight: 700,
                asset: FontAsset {
                    file: "RobotoMonoNerdFont-Bold.ttf",
                    url: "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/fa7b859994228a9c8759f99c55a8d31ee92a1b5e/patched-fonts/RobotoMono/Bold/RobotoMonoNerdFont-Bold.ttf",
                    sha256: "af99096cade7f4f84e201341b79f569ae730f6e78562526e2f160b6285ab7fe6",
                    len: 2_454_668,
                },
            },
        ],
    },
    PackFont {
        family: "UbuntuMono Nerd Font",
        faces: &[
            PackFace {
                weight: 400,
                asset: FontAsset {
                    file: "UbuntuMonoNerdFont-Regular.ttf",
                    url: "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/fa7b859994228a9c8759f99c55a8d31ee92a1b5e/patched-fonts/UbuntuMono/Regular/UbuntuMonoNerdFont-Regular.ttf",
                    sha256: "06492cae7c6b268ac5dccacfb0677e40f0f1377852b4d22689d4105ec862d7a4",
                    len: 2_367_832,
                },
            },
            PackFace {
                weight: 700,
                asset: FontAsset {
                    file: "UbuntuMonoNerdFont-Bold.ttf",
                    url: "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/fa7b859994228a9c8759f99c55a8d31ee92a1b5e/patched-fonts/UbuntuMono/Bold/UbuntuMonoNerdFont-Bold.ttf",
                    sha256: "99108aad07b983283b9acbdd8b41bf840a6bebe569b0fcbd0553045b21b33c54",
                    len: 2_351_976,
                },
            },
        ],
    },
    // 13 MB per face: Iosevka is a superfamily with very wide
    // coverage. Fine for an opt-in download (the CJK fonts run
    // 9-18 MB), would never fly bundled. Only the weight actually
    // picked is ever fetched.
    PackFont {
        family: "Iosevka NF",
        faces: &[
            PackFace {
                weight: 400,
                asset: FontAsset {
                    file: "IosevkaNerdFont-Regular.ttf",
                    url: "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/fa7b859994228a9c8759f99c55a8d31ee92a1b5e/patched-fonts/Iosevka/IosevkaNerdFont-Regular.ttf",
                    sha256: "48dad582909322164f40892e4e27eaa497346ab046b450b5c23c754ac35b53d2",
                    len: 13_233_516,
                },
            },
            PackFace {
                weight: 500,
                asset: FontAsset {
                    file: "IosevkaNerdFont-Medium.ttf",
                    url: "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/fa7b859994228a9c8759f99c55a8d31ee92a1b5e/patched-fonts/Iosevka/IosevkaNerdFont-Medium.ttf",
                    sha256: "49acc2d2644de43a4663949487f89d91069c5c31e3e340775da8aacd58fcf2c7",
                    len: 13_276_024,
                },
            },
            PackFace {
                weight: 600,
                asset: FontAsset {
                    file: "IosevkaNerdFont-SemiBold.ttf",
                    url: "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/fa7b859994228a9c8759f99c55a8d31ee92a1b5e/patched-fonts/Iosevka/IosevkaNerdFont-SemiBold.ttf",
                    sha256: "666f54dcadfa2545e1a1cc54a35ff0bce668cfbb24f693c42de8aaf12d1fef82",
                    len: 13_265_424,
                },
            },
            PackFace {
                weight: 700,
                asset: FontAsset {
                    file: "IosevkaNerdFont-Bold.ttf",
                    url: "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/fa7b859994228a9c8759f99c55a8d31ee92a1b5e/patched-fonts/Iosevka/IosevkaNerdFont-Bold.ttf",
                    sha256: "d13814ca9b4d51909ab151a47b2087915ceb91c244901b7d3497e27826ba090c",
                    len: 13_281_980,
                },
            },
        ],
    },
];

/// The pack entry for a picker family name, if it is one.
pub fn pack_font(family: &str) -> Option<&'static PackFont> {
    PACK_FONTS.iter().find(|p| p.family == family)
}

/// The face to fetch so a pack family renders at `weight`: the one
/// pinned at exactly that weight, or the family's Regular when it has
/// none there.
///
/// The fallback is the whole point of returning something: without a
/// single face loaded, the family the user picked is not in the font
/// system at all and the terminal falls back to a system font, which
/// is a worse answer than the same family at a nearby weight. Every
/// caller resolves through here so the boot path and the picker can't
/// disagree about which file a setting needs.
pub fn pack_face_for(
    family: &str,
    weight: TerminalFontWeight,
) -> Option<&'static PackFace> {
    let font = pack_font(family)?;
    font.face(weight.css()).or_else(|| font.faces.first())
}

/// True when the face's file is already on disk at the expected size
/// (same cheap validity test as [`is_language_cached`]).
pub fn is_face_cached(face: &PackFace) -> bool {
    cached_path(&face.asset)
        .and_then(|p| std::fs::metadata(p).ok())
        .map(|m| m.len() == face.asset.len)
        .unwrap_or(false)
}

/// The CJK asset a language needs, if any.
fn asset_for(lang: Language) -> Option<&'static CjkAsset> {
    let code = match lang {
        Language::Korean => "ko",
        Language::Chinese => "zh",
        Language::Japanese => "ja",
        Language::ChineseTraditional => "zh-TW",
        _ => return None,
    };
    ASSETS.iter().find(|a| a.code == code)
}

/// The CJK language code this language needs (`"ko"`/`"zh"`/`"ja"`), or
/// `None` for languages whose scripts are already bundled.
pub fn asset_code(lang: Language) -> Option<&'static str> {
    asset_for(lang).map(|a| a.code)
}

/// `~/.oryxis/fonts/`, the same `~/.oryxis` root the vault and plugin
/// cache use. Not created here; `download` creates it on demand.
fn cache_dir() -> Option<PathBuf> {
    Some(oryxis_core::paths::oryxis_dir()?.join("fonts"))
}

fn cached_path(asset: &FontAsset) -> Option<PathBuf> {
    Some(cache_dir()?.join(asset.file))
}

/// True when the language's font is already on disk at the expected
/// size. Used to decide whether to show a "downloading" hint; the byte
/// length is a cheap validity check (a half-written file fails it) so a
/// boot existence test can't load a truncated download.
pub fn is_language_cached(lang: Language) -> bool {
    let Some(asset) = asset_for(lang) else {
        return false;
    };
    cached_path(&asset.asset)
        .and_then(|p| std::fs::metadata(p).ok())
        .map(|m| m.len() == asset.asset.len)
        .unwrap_or(false)
}

/// Read the cached font if present and the right size, otherwise
/// download it (size-capped + SHA-256 verified, written atomically),
/// and return the bytes ready for `iced::font::load`.
async fn ensure_and_read(asset: &'static FontAsset) -> Result<Vec<u8>, String> {
    let path = cached_path(asset).ok_or_else(|| "no home directory".to_string())?;

    if let Ok(meta) = tokio::fs::metadata(&path).await
        && meta.len() == asset.len
        && let Ok(bytes) = tokio::fs::read(&path).await
    {
        return Ok(bytes);
    }

    let client = reqwest::Client::builder()
        .user_agent(concat!("Oryxis/", env!("CARGO_PKG_VERSION")))
        // Bound the request so a stalled connection becomes the Err
        // path (system-font fallback + retry) instead of leaving the
        // "downloading" toast and the in-memory guard stuck forever.
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(90))
        // Never let a redirect move the font fetch to plaintext http.
        // The sha256 pin already guards integrity; https keeps the
        // fetch itself confidential.
        .https_only(true)
        .build()
        .map_err(|e| e.to_string())?;
    // Direct fetch of the pinned canonical URL only; there is no mirror
    // layer anymore. The SHA-256 gate below is the integrity contract,
    // so the host serving the bytes never needs to be trusted.
    let resp = client
        .get(asset.url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?;

    // Cap a little above the pinned length so a wrong/redirected body
    // can't exhaust memory; the SHA-256 below is the real gate.
    let max = asset.len + 64 * 1024;
    let mut buf = Vec::with_capacity(asset.len as usize);
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        if (buf.len() as u64).saturating_add(chunk.len() as u64) > max {
            return Err(format!("font body exceeds {max} byte ceiling"));
        }
        buf.extend_from_slice(&chunk);
    }

    // sha2 0.11 returns a `hybrid_array::Array`, which no longer
    // implements LowerHex; format the bytes directly.
    let digest: String = Sha256::digest(&buf).iter().map(|b| format!("{b:02x}")).collect();
    if !digest.eq_ignore_ascii_case(asset.sha256) {
        return Err(format!(
            "sha256 mismatch for {}: expected {}, got {digest}",
            asset.file, asset.sha256
        ));
    }

    // Atomic install: write a sibling .tmp, fsync it, then rename into
    // place so an interrupted download or a power loss never leaves a
    // partial file the cache-hit path would trust.
    if let Some(dir) = cache_dir() {
        let _ = tokio::fs::create_dir_all(&dir).await;
        let tmp = dir.join(format!("{}.tmp", asset.file));
        let wrote = async {
            use tokio::io::AsyncWriteExt as _;
            let mut f = tokio::fs::File::create(&tmp).await.ok()?;
            f.write_all(&buf).await.ok()?;
            f.sync_all().await.ok()?;
            Some(())
        }
        .await;
        if wrote.is_some() {
            let _ = tokio::fs::rename(&tmp, &path).await;
            // fsync the directory so the rename itself survives a power
            // loss (the durability step download.rs documents as required).
            if let Ok(d) = tokio::fs::File::open(&dir).await {
                let _ = d.sync_all().await;
            }
        } else {
            let _ = tokio::fs::remove_file(&tmp).await;
        }
    }

    Ok(buf)
}

/// A task that ensures the CJK font for `lang` is available and loads
/// it into the iced font system. `Task::none()` for languages that
/// don't need a downloaded font. The resulting `CjkFontReady` message
/// carries the bytes (or an error) back to the update loop, which calls
/// `iced::font::load` on the main side.
///
/// The bytes handed on are the cached file's with its family rewritten
/// (see `registered_bytes`), never the file on disk: the cache is
/// validated by byte length against the pinned `len`, so a renamed file
/// there would re-download on every boot.
pub fn ensure_task(lang: Language) -> iced::Task<Message> {
    let Some(asset) = asset_for(lang) else {
        return iced::Task::none();
    };
    let code = asset.code.to_string();
    iced::Task::perform(
        async move { ensure_and_read(&asset.asset).await.map(|b| registered_bytes(b, asset)) },
        move |res| Message::Settings(SettingsMessage::CjkFontReady(code.clone(), res)),
    )
}

/// The bytes to register with the font system for a CJK asset.
///
/// On Linux, cosmic-text's per-script fallback names `Noto Sans CJK
/// <region>` for Han and nothing else, so the file we download under its
/// own `Noto Sans <region>` family is only ever reached by the sweep
/// over every remaining face, which it loses (issue #189). Answering to
/// the name that is actually asked for is what puts the font we shipped
/// in front of whatever the machine happens to have installed.
///
/// macOS and Windows name their own system faces (PingFang, Microsoft
/// YaHei), which are always present and render correctly, so the rename
/// would be claiming a family we are not. They keep the file as it is.
fn registered_bytes(mut bytes: Vec<u8>, asset: &'static CjkAsset) -> Vec<u8> {
    if CLAIMS_FALLBACK_FAMILY
        && !crate::font_family::set_family(&mut bytes, asset.family)
    {
        tracing::warn!(
            target = "oryxis::fonts",
            lang = %asset.code,
            "could not rewrite the CJK font family; system CJK fallback stays in charge"
        );
    }
    bytes
}

/// Whether this platform's per-script fallback names a family only we
/// can supply. A `cfg!` rather than a `cfg`, so the rewrite and its
/// tests keep compiling on every target: the sfnt surgery has nothing
/// platform-specific in it, and hiding it behind a `cfg` would leave the
/// macOS and Windows CI jobs building a file they never check.
const CLAIMS_FALLBACK_FAMILY: bool =
    cfg!(all(unix, not(any(target_os = "android", target_os = "macos"))));

/// A task that ensures one pack face is available (cache read or
/// download) and reports back as `PackFontReady`, which registers the
/// bytes with the iced font system. The terminal widget resolves the
/// family by name per frame, so a load that lands mid-session applies
/// to live panes with no restart.
pub fn ensure_pack_task(face: &'static PackFace) -> iced::Task<Message> {
    let key = face.key().to_string();
    iced::Task::perform(ensure_and_read(&face.asset), move |res| {
        Message::Settings(SettingsMessage::PackFontReady(key.clone(), res))
    })
}

/// Boot-time pack loading: one ensure task per pack face that is
/// either already cached (pure disk read, loads before the first
/// terminal renders) or is the face the picked terminal font +
/// weight needs on a machine that doesn't have it yet (settings
/// arrived via sync / portable import; the silent download heals the
/// tofu without the user re-picking).
pub fn boot_pack_tasks(
    selected_family: &str,
    selected_weight: TerminalFontWeight,
) -> Vec<(&'static str, iced::Task<Message>)> {
    let wanted = pack_face_for(selected_family, selected_weight);
    PACK_FONTS
        .iter()
        .flat_map(|f| f.faces.iter())
        .filter(|face| {
            is_face_cached(face)
                || wanted.is_some_and(|w| std::ptr::eq(w, *face))
        })
        .map(|face| (face.key(), ensure_pack_task(face)))
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::i18n::Language;
    use fontdb::{Database, Family, Query, Stretch, Style, Weight};

    fn db_with_noto() -> Database {
        let mut db = Database::new();
        db.load_font_data(
            include_bytes!("../../../resources/fonts/NotoSans-Regular.ttf").to_vec(),
        );
        db.load_font_data(
            include_bytes!("../../../resources/fonts/NotoSans-SemiBold.ttf").to_vec(),
        );
        db.load_font_data(
            include_bytes!("../../../resources/fonts/NotoSans-Bold.ttf").to_vec(),
        );
        db
    }

    fn resolve_weight(db: &Database, weight: Weight) -> Weight {
        let id = db
            .query(&Query {
                families: &[Family::Name("Noto Sans")],
                weight,
                stretch: Stretch::Normal,
                style: Style::Normal,
            })
            .expect("the \"Noto Sans\" family must resolve");
        db.face(id).expect("face for id").weight
    }

    /// The three bundled Noto Sans files must group under one family
    /// ("Noto Sans") so the UI's Regular (400), SemiBold (600) and Bold
    /// (700) each resolve to the right file. SemiBold's legacy family-1
    /// name is "Noto Sans SemiBold"; the grouping relies on fontdb
    /// reading the typographic family (name ID 16 = "Noto Sans"). If
    /// that breaks, weight 600 would fall back to 400/700 and every
    /// heading / tab / active chip would render at the wrong weight on
    /// every platform, the exact regression this guards headless.
    #[test]
    fn noto_sans_weights_resolve_distinctly() {
        let db = db_with_noto();
        assert_eq!(resolve_weight(&db, Weight::NORMAL), Weight::NORMAL);
        assert_eq!(resolve_weight(&db, Weight::SEMIBOLD), Weight::SEMIBOLD);
        assert_eq!(resolve_weight(&db, Weight::BOLD), Weight::BOLD);
    }

    /// Every downloadable font pin must be well-formed: lowercase-hex
    /// 64-char sha256, a nonzero length, an https URL on the one host
    /// the mirror layer rewrites to `fonts/<file>` (a pin on any other
    /// host would silently lose the China-mirror fallback), and a URL
    /// that actually ends in the cache file name (the mirror bucket
    /// key is derived from the URL's last segment, so a mismatch would
    /// 404 only on the fallback leg, the least-tested path).
    #[test]
    fn download_pins_are_well_formed() {
        let all: Vec<(&str, &super::FontAsset)> = super::ASSETS
            .iter()
            .map(|a| (a.code, &a.asset))
            .chain(
                super::PACK_FONTS
                    .iter()
                    .flat_map(|p| p.faces.iter().map(|f| (p.family, &f.asset))),
            )
            .collect();
        for (key, asset) in &all {
            assert_eq!(asset.sha256.len(), 64, "{key}: sha256 length");
            assert!(
                asset.sha256.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
                "{key}: sha256 must be lowercase hex"
            );
            assert!(asset.len > 0, "{key}: zero pinned length");
            assert!(
                asset.url.starts_with("https://raw.githubusercontent.com/"),
                "{key}: pin host must be raw.githubusercontent.com"
            );
            let last = asset.url.rsplit('/').next().unwrap_or_default();
            let decoded = last.replace("%5B", "[").replace("%5D", "]");
            assert!(
                decoded.contains(asset.file.trim_end_matches(".ttf"))
                    || asset.file.contains(decoded.trim_end_matches(".ttf")),
                "{key}: URL tail {decoded:?} does not match cache file {:?}",
                asset.file
            );
        }
        // Cache file names double as bucket keys; a collision would
        // make two different pins overwrite each other on disk.
        let mut files: Vec<&str> = all.iter().map(|(_, a)| a.file).collect();
        files.sort_unstable();
        files.dedup();
        assert_eq!(files.len(), all.len(), "duplicate cache file names");
    }

    /// The pack family names are what the picker stores and what
    /// cosmic-text resolves against the loaded TTF, so they must be
    /// unique and stable. `enumerate_terminal_fonts` prepends them by
    /// name; a rename here without a picker migration would strand
    /// saved `terminal_font_name` values.
    #[test]
    fn pack_families_are_unique() {
        let mut fams: Vec<&str> = super::PACK_FONTS.iter().map(|p| p.family).collect();
        fams.sort_unstable();
        fams.dedup();
        assert_eq!(fams.len(), super::PACK_FONTS.len());
    }

    /// Every pack family must start at Regular and climb, with no
    /// weight pinned twice and nothing outside the four the picker
    /// offers. A duplicate would make `PackFont::face` pick by table
    /// order (silently downloading one file while the other is what
    /// the user sees named), and a weight the picker can't request
    /// would be a file nothing ever fetches.
    #[test]
    fn pack_faces_are_ordered_and_offerable() {
        use super::TerminalFontWeight as W;
        let offered: Vec<u16> = W::ALL.iter().map(|w| w.css()).collect();
        for font in super::PACK_FONTS {
            let weights: Vec<u16> = font.faces.iter().map(|f| f.weight).collect();
            assert_eq!(
                weights.first(),
                Some(&400),
                "{}: the Regular face is what an un-weighted request lands on",
                font.family
            );
            let mut sorted = weights.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted, weights, "{}: faces must be unique, lightest first", font.family);
            for w in &weights {
                assert!(
                    offered.contains(w),
                    "{}: weight {w} is pinned but the picker never asks for it",
                    font.family
                );
            }
        }
    }

    /// A weight the family does not pin resolves to its Regular, not
    /// to nothing. Both the boot loader and the picker fetch through
    /// this, and a `None` there would leave the picked family absent
    /// from the font system entirely: the terminal would quietly
    /// render in a system font instead of at a nearby weight of the
    /// font that was actually chosen.
    #[test]
    fn a_weight_the_family_lacks_falls_back_to_its_regular() {
        use super::TerminalFontWeight as W;
        let weight_of = |family, w| super::pack_face_for(family, w).map(|f| f.weight);
        // Hack pins Regular and Bold; upstream has no Medium.
        assert_eq!(weight_of("Hack Nerd Font", W::Medium), Some(400));
        assert_eq!(weight_of("Hack Nerd Font", W::Bold), Some(700));
        // JetBrainsMono pins all four, so each is served exactly.
        assert_eq!(weight_of("JetBrainsMono NF", W::SemiBold), Some(600));
        assert_eq!(weight_of("JetBrainsMono NF", W::Regular), Some(400));
        // The bundled family is not downloadable, so nothing to fetch.
        assert!(super::pack_face_for("SauceCodePro Nerd Font", W::Medium).is_none());
    }

    /// The stored setting round-trips, and an unknown value degrades
    /// to Regular instead of poisoning the boot: the row travels
    /// through sync and portable import, so a newer build's weight can
    /// legitimately arrive at an older one.
    #[test]
    fn font_weight_settings_round_trip() {
        use super::TerminalFontWeight as W;
        for w in W::ALL {
            assert_eq!(W::from_setting(w.setting_value()), w);
            assert_eq!(w.setting_value().parse::<u16>().unwrap(), w.css());
        }
        assert_eq!(W::from_setting("800"), W::Regular);
        assert_eq!(W::from_setting(""), W::Regular);
        assert_eq!(W::from_setting("bold"), W::Regular);
    }

    /// The bundled terminal family ships Regular AND Medium under one
    /// typographic family, which is what lets the weight picker mean
    /// something out of the box, with no download and no system font.
    /// The two files spell their name-1 family differently
    /// ("SauceCodePro NF" vs "SauceCodePro NF Medium"), so the
    /// grouping rests entirely on fontdb reading the typographic
    /// family (name ID 16); if a re-pin breaks that, weight 500 would
    /// silently collapse back onto 400 and the setting would look
    /// broken for every user who never installed a font.
    #[test]
    fn bundled_terminal_family_serves_regular_and_medium() {
        let mut db = Database::new();
        db.load_font_data(
            include_bytes!("../../../resources/fonts/SauceCodeProNerdFont-Regular.ttf")
                .to_vec(),
        );
        db.load_font_data(
            include_bytes!("../../../resources/fonts/SauceCodeProNerdFont-Medium.ttf")
                .to_vec(),
        );
        let resolve = |weight: Weight| {
            let id = db
                .query(&Query {
                    families: &[Family::Name("SauceCodePro Nerd Font")],
                    weight,
                    stretch: Stretch::Normal,
                    style: Style::Normal,
                })
                .expect("the bundled terminal family must resolve");
            db.face(id).expect("face for id").weight
        };
        assert_eq!(resolve(Weight::NORMAL), Weight::NORMAL);
        assert_eq!(resolve(Weight::MEDIUM), Weight::MEDIUM);
        // And the table the picker reads must agree with the files.
        assert_eq!(
            super::BUNDLED_MONO_WEIGHTS
                .iter()
                .find(|(f, _)| *f == "SauceCodePro Nerd Font")
                .map(|(_, w)| *w),
            Some([400u16, 500].as_slice()),
        );
    }

    /// The bundled MenuCJK subset must cover every glyph of the CJK
    /// language-picker names, so those entries always render from the
    /// binary even before the full on-demand CJK font is downloaded. If
    /// a `Language::name()` for a CJK language gains a character the
    /// subset doesn't carry, this fails (re-generate the subset, see the
    /// fonts memory note).
    #[test]
    fn menu_cjk_covers_picker_names() {
        let data =
            include_bytes!("../../../resources/fonts/MenuCJK.ttf").as_slice();
        let face = ttf_parser::Face::parse(data, 0).expect("MenuCJK parses");
        for lang in [
            Language::Chinese,
            Language::ChineseTraditional,
            Language::Japanese,
            Language::Korean,
        ] {
            for ch in lang.name().chars() {
                assert!(
                    face.glyph_index(ch).is_some(),
                    "MenuCJK is missing glyph {ch:?} from {} name {:?}",
                    lang.code(),
                    lang.name(),
                );
            }
        }
    }
}
