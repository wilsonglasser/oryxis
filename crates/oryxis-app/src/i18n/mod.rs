use std::sync::atomic::{AtomicUsize, Ordering};

static ACTIVE_LANG: AtomicUsize = AtomicUsize::new(0);
static ACTIVE_LAYOUT_DIR: AtomicUsize = AtomicUsize::new(0);

/// User-facing setting controlling the visual layout direction. `Auto` follows
/// the active language (so Persian flips automatically); the explicit values
/// override regardless of the chosen language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutDirection {
    Auto,
    LeftToRight,
    RightToLeft,
}

impl LayoutDirection {
    pub const ALL: &[LayoutDirection] = &[
        Self::Auto,
        Self::LeftToRight,
        Self::RightToLeft,
    ];

    pub fn code(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::LeftToRight => "ltr",
            Self::RightToLeft => "rtl",
        }
    }

    pub fn from_code(code: &str) -> Self {
        match code {
            "ltr" => Self::LeftToRight,
            "rtl" => Self::RightToLeft,
            _ => Self::Auto,
        }
    }

    /// i18n key used for the dropdown label of this option.
    pub fn label_key(&self) -> &'static str {
        match self {
            Self::Auto => "layout_dir_auto",
            Self::LeftToRight => "layout_dir_ltr",
            Self::RightToLeft => "layout_dir_rtl",
        }
    }

    pub fn set_active(dir: LayoutDirection) {
        let idx = Self::ALL.iter().position(|d| *d == dir).unwrap_or(0);
        ACTIVE_LAYOUT_DIR.store(idx, Ordering::Relaxed);
    }

    pub fn active() -> LayoutDirection {
        let idx = ACTIVE_LAYOUT_DIR.load(Ordering::Relaxed);
        Self::ALL.get(idx).copied().unwrap_or(LayoutDirection::Auto)
    }
}

/// True when the active *language* uses right-to-left script. Drives text
/// alignment, text-input direction, BiDi hints. Independent of the user's
/// layout-direction setting, Persian text is always RTL regardless of
/// whether the user kept the sidebar on the left.
///
/// Currently unused at call sites, cosmic-text's BiDi shaping handles
/// glyph-level rendering automatically. Exposed for future per-widget
/// alignment overrides (e.g. right-aligning RTL `text_input`s).
#[allow(dead_code)]
pub fn is_rtl_text() -> bool {
    Language::active().is_rtl()
}

/// True when the *layout* should be physically mirrored (sidebar swaps
/// sides, row children reverse). Resolves the user's `LayoutDirection`
/// setting; `Auto` defers to the language. Override `Auto` with explicit
/// `Left`/`Right` if the user wants Persian text but a familiar layout.
pub fn is_rtl_layout() -> bool {
    match LayoutDirection::active() {
        LayoutDirection::Auto => Language::active().is_rtl(),
        LayoutDirection::LeftToRight => false,
        LayoutDirection::RightToLeft => true,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    English,
    PortugueseBR,
    Spanish,
    French,
    German,
    Italian,
    Chinese,
    Japanese,
    Russian,
    Persian,
    Arabic,
    Korean,
    Polish,
    Turkish,
    Indonesian,
    Vietnamese,
    Ukrainian,
}

impl Language {
    pub const ALL: &[Language] = &[
        Self::English,
        Self::PortugueseBR,
        Self::Spanish,
        Self::French,
        Self::German,
        Self::Italian,
        Self::Chinese,
        Self::Japanese,
        Self::Russian,
        Self::Persian,
        Self::Arabic,
        Self::Korean,
        Self::Polish,
        Self::Turkish,
        Self::Indonesian,
        Self::Vietnamese,
        Self::Ukrainian,
    ];

    pub fn code(&self) -> &'static str {
        match self {
            Self::English => "en",
            Self::PortugueseBR => "pt-BR",
            Self::Spanish => "es",
            Self::French => "fr",
            Self::German => "de",
            Self::Italian => "it",
            Self::Chinese => "zh",
            Self::Japanese => "ja",
            Self::Russian => "ru",
            Self::Persian => "fa",
            Self::Arabic => "ar",
            Self::Korean => "ko",
            Self::Polish => "pl",
            Self::Turkish => "tr",
            Self::Indonesian => "id",
            Self::Vietnamese => "vi",
            Self::Ukrainian => "uk",
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::English => "English",
            Self::PortugueseBR => "Português (Brasil)",
            Self::Spanish => "Español",
            Self::French => "Français",
            Self::German => "Deutsch",
            Self::Italian => "Italiano",
            Self::Chinese => "中文",
            Self::Japanese => "日本語",
            Self::Russian => "Русский",
            Self::Persian => "فارسی",
            Self::Arabic => "العربية",
            Self::Korean => "한국어",
            Self::Polish => "Polski",
            Self::Turkish => "Türkçe",
            Self::Indonesian => "Bahasa Indonesia",
            Self::Vietnamese => "Tiếng Việt",
            Self::Ukrainian => "Українська",
        }
    }

    /// Whether this language is written right-to-left. Used by the
    /// `LayoutDirection::Auto` setting to decide if the UI should mirror.
    pub fn is_rtl(&self) -> bool {
        matches!(self, Self::Persian | Self::Arabic)
    }

    pub fn from_code(code: &str) -> Self {
        match code {
            "pt-BR" => Self::PortugueseBR,
            "es" => Self::Spanish,
            "fr" => Self::French,
            "de" => Self::German,
            "it" => Self::Italian,
            "zh" => Self::Chinese,
            "ja" => Self::Japanese,
            "ru" => Self::Russian,
            "fa" => Self::Persian,
            "ar" => Self::Arabic,
            "ko" => Self::Korean,
            "pl" => Self::Polish,
            "tr" => Self::Turkish,
            "id" => Self::Indonesian,
            "vi" => Self::Vietnamese,
            "uk" => Self::Ukrainian,
            _ => Self::English,
        }
    }

    pub fn set_active(lang: Language) {
        let idx = Self::ALL.iter().position(|l| *l == lang).unwrap_or(0);
        ACTIVE_LANG.store(idx, Ordering::Relaxed);
    }

    pub fn active() -> Language {
        let idx = ACTIVE_LANG.load(Ordering::Relaxed);
        Self::ALL.get(idx).copied().unwrap_or(Language::English)
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

mod en;
mod pt_br;
mod es;
mod fr;
mod de;
mod it;
mod zh;
mod ja;
mod ru;
mod fa;
mod ar;
mod ko;
mod pl;
mod tr;
mod id;
mod vi;
mod uk;

/// Get a translated string. Usage: `t("hosts")` or `t("create_host")`
pub fn t(key: &str) -> &'static str {
    let lang = Language::active();
    translate(key, lang)
}

/// "1 host" / "N hosts" with the count inlined. One/other is an
/// approximation (Slavic languages have richer plural classes), good
/// enough for a count label, and it fixes the "1 hosts" card subtitle.
pub fn host_count(n: usize) -> String {
    if n == 1 {
        t("host_count_one").to_string()
    } else {
        format!("{} {}", n, t("host_count_other"))
    }
}

fn translate(key: &str, lang: Language) -> &'static str {
    match lang {
        Language::English => en::lookup(key),
        Language::PortugueseBR => pt_br::lookup(key).unwrap_or_else(|| en::lookup(key)),
        Language::Spanish => es::lookup(key).unwrap_or_else(|| en::lookup(key)),
        Language::French => fr::lookup(key).unwrap_or_else(|| en::lookup(key)),
        Language::German => de::lookup(key).unwrap_or_else(|| en::lookup(key)),
        Language::Italian => it::lookup(key).unwrap_or_else(|| en::lookup(key)),
        Language::Chinese => zh::lookup(key).unwrap_or_else(|| en::lookup(key)),
        Language::Japanese => ja::lookup(key).unwrap_or_else(|| en::lookup(key)),
        Language::Russian => ru::lookup(key).unwrap_or_else(|| en::lookup(key)),
        Language::Persian => fa::lookup(key).unwrap_or_else(|| en::lookup(key)),
        Language::Arabic => ar::lookup(key).unwrap_or_else(|| en::lookup(key)),
        Language::Korean => ko::lookup(key).unwrap_or_else(|| en::lookup(key)),
        Language::Polish => pl::lookup(key).unwrap_or_else(|| en::lookup(key)),
        Language::Turkish => tr::lookup(key).unwrap_or_else(|| en::lookup(key)),
        Language::Indonesian => id::lookup(key).unwrap_or_else(|| en::lookup(key)),
        Language::Vietnamese => vi::lookup(key).unwrap_or_else(|| en::lookup(key)),
        Language::Ukrainian => uk::lookup(key).unwrap_or_else(|| en::lookup(key)),
    }
}
