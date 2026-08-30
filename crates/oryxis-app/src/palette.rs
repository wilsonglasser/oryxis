//! Command palette (C4): `Ctrl+Shift+P` opens a fuzzy-searchable list of
//! every action Oryxis can perform, doubling as hotkey discovery.
//!
//! The action rows are GENERATED, not hand-copied: every editable
//! `HotkeyAction` (all but the three digit/arrow family actions) becomes
//! one row whose activation replays `dispatch_hotkey_action`, so "every
//! hotkey has a palette row" holds by construction and the per-action
//! context gating is reused verbatim. On top of that a tiny table of
//! non-hotkey extras (Lock vault, Privacy mode) plus two dynamic
//! providers (saved hosts, Settings sections) fill out the catalog.
//!
//! Activation carries the resolved `Message` inside the `RowAction`
//! (mirroring `TabJumpSelect`), never a list index, so a query change
//! between the recording frame and the keypress can't misfire.

use crate::app::{SettingsMessage, TabsMessage, SshMessage, VaultMessage, Message, Oryxis};
use crate::hotkeys::HotkeyAction;
use crate::i18n::t;
use crate::state::View;

/// The `text_input` id the palette focuses on open and records as its
/// keyboard input row.
pub(crate) const PALETTE_INPUT_ID: &str = "command-palette-input";

/// Grouping for a palette row: drives the leading glyph and the
/// category-order tiebreak when two rows score equally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaletteCategory {
    Tabs,
    Vault,
    Terminal,
    Settings,
    Session,
}

impl PaletteCategory {
    /// Rank order for the equal-score tiebreak (lower sorts first).
    pub(crate) fn order(self) -> u8 {
        match self {
            PaletteCategory::Tabs => 0,
            PaletteCategory::Vault => 1,
            PaletteCategory::Terminal => 2,
            PaletteCategory::Settings => 3,
            PaletteCategory::Session => 4,
        }
    }

    /// i18n key for the category chip / section label.
    pub(crate) fn label_key(self) -> &'static str {
        match self {
            PaletteCategory::Tabs => "palette_cat_tabs",
            PaletteCategory::Vault => "palette_cat_vault",
            PaletteCategory::Terminal => "palette_cat_terminal",
            PaletteCategory::Settings => "palette_cat_settings",
            PaletteCategory::Session => "palette_cat_session",
        }
    }
}

/// One rendered palette entry, after filtering + ranking.
pub(crate) struct PaletteRow {
    pub(crate) label: String,
    /// The bound chord, if this row mirrors a hotkey (rendered live from
    /// the current binding map so rebinds show correctly).
    pub(crate) hotkey: Option<HotkeyAction>,
    pub(crate) category: PaletteCategory,
    /// The real message this row dispatches (carried into the
    /// `RowAction`, not re-derived by index).
    pub(crate) message: Message,
    /// Evaluated per frame: a disabled row lists (for discoverability)
    /// but records no keynav slot and drops its `on_press`.
    pub(crate) enabled: bool,
}

/// A pre-ranking candidate: the label plus the extra fuzzy fodder its
/// score is also tested against (the hotkey id, so "split" matches
/// `split_pane_vertical`).
struct Candidate {
    label: String,
    keywords: &'static str,
    hotkey: Option<HotkeyAction>,
    category: PaletteCategory,
    message: Message,
    enabled: bool,
}

/// Category for a generated hotkey row.
fn hotkey_category(action: HotkeyAction) -> PaletteCategory {
    use HotkeyAction::*;
    match action {
        ShowNewTabPicker | ShowTabJump | OpenLocalShell | NewWindow | CloseActiveTab
        | ReopenClosedTab | ReconnectTab | OpenSftp | OpenSftpConsole => PaletteCategory::Tabs,
        OpenPortForwards | FocusViewSearch | NewHost | ShowQuickConnect | NewKey | NewIdentity
        | VaultSectionPrev | VaultSectionNext | VaultSectionSlot => PaletteCategory::Vault,
        FontZoomIn | FontZoomOut | FontZoomReset | SplitPaneVertical | SplitPaneHorizontal
        | FocusPaneLeft | FocusPaneRight | FocusPaneUp | FocusPaneDown | ToggleMaximizePane
        | FocusSidebarList
        | ToggleSidebar | ToggleSidebarOther | ToggleTabFiles | ToggleBroadcastInput | TerminalCopy
        | TerminalPaste | TerminalPasteSelection | TerminalSelectAll | ScrollbackPageUp
        | ScrollbackPageDown => {
            PaletteCategory::Terminal
        }
        OpenSettings => PaletteCategory::Settings,
        ToggleFullscreen | ShowCommandPalette | SwitchToTabSlot | CycleTabs
        | TogglePrivacyMode => PaletteCategory::Session,
    }
}

/// Case-insensitive subsequence fuzzy match. `None` when `needle` is not
/// a subsequence of `haystack`; otherwise a score where higher is
/// better, favouring consecutive runs, word-boundary starts and earlier
/// matches (so exact-prefix beats mid-string beats scattered).
pub(crate) fn fuzzy_score(haystack: &str, needle: &str) -> Option<i32> {
    if needle.is_empty() {
        return Some(0);
    }
    let hay: Vec<char> = haystack.chars().flat_map(|c| c.to_lowercase()).collect();
    let need: Vec<char> = needle.chars().flat_map(|c| c.to_lowercase()).collect();
    let mut ni = 0usize;
    let mut score = 0i32;
    let mut prev_match = false;
    let mut first_match: Option<usize> = None;
    for (hi, &hc) in hay.iter().enumerate() {
        if ni >= need.len() {
            break;
        }
        if hc == need[ni] {
            if first_match.is_none() {
                first_match = Some(hi);
            }
            score += 1;
            if prev_match {
                score += 5; // consecutive-run bonus
            }
            // Word-boundary bonus: matching the first char of a word.
            if hi == 0 || !hay[hi - 1].is_alphanumeric() {
                score += 3;
            }
            ni += 1;
            prev_match = true;
        } else {
            prev_match = false;
        }
    }
    if ni < need.len() {
        return None; // not every needle char matched
    }
    // Earlier-match bonus: a prefix hit outranks a mid-string hit.
    if let Some(fm) = first_match {
        score += 10 - (fm.min(10) as i32);
    }
    Some(score)
}

/// Whitespace-tokenized AND over [`fuzzy_score`]: every query word
/// must match somewhere (scores summed), in any order. A raw
/// subsequence match treats the query's spaces as literal characters
/// to find IN ORDER, so "copy on select" could never hit a label
/// whose words appear differently ordered; per-token matching is
/// what a human means by a multi-word query. Single-word queries are
/// exactly [`fuzzy_score`].
pub(crate) fn tokenized_fuzzy_score(haystack: &str, needle: &str) -> Option<i32> {
    let mut total = 0i32;
    let mut any = false;
    for tok in needle.split_whitespace() {
        total += fuzzy_score(haystack, tok)?;
        any = true;
    }
    if any { Some(total) } else { Some(0) }
}

impl Oryxis {
    /// The visible section list, verbatim from the Settings sidebar
    /// (feature toggles hide sections), so the palette's "Settings: X"
    /// rows track exactly what the sidebar shows.
    pub(crate) fn settings_section_items(
        &self,
    ) -> Vec<(&'static str, crate::state::SettingsSection)> {
        use crate::state::SettingsSection as S;
        let mut items: Vec<(&'static str, S)> = vec![
            (t("interface"), S::Interface),
            (t("terminal_settings"), S::Terminal),
            (t("connection"), S::Connection),
            (t("shortcuts"), S::Shortcuts),
            (t("security_privacy"), S::Security),
            (t("features_and_plugins"), S::Plugins),
        ];
        if self.ai.enabled {
            items.push((t("ai_assistant"), S::AI));
        }
        if self.plugin_installed("mcp") {
            items.push((t("mcp_server"), S::Mcp));
        }
        if self.sftp_enabled {
            items.push(("SFTP", S::Sftp));
        }
        if self.prefs.host_monitoring {
            items.push((t("settings_section_monitoring"), S::Monitoring));
        }
        // Same gate as the Features toggle: no listener on this
        // platform means no section either.
        if self.agent.enabled && crate::agent_server::listener_socket_display().is_some() {
            items.push((t("agent_server"), S::Agent));
        }
        items.push((t("settings_advanced"), S::Advanced));
        items.push((t("about"), S::About));
        items
    }

    /// Build, filter and rank the palette rows for `query`. Empty query
    /// lists everything in category order (extension point for MRU);
    /// otherwise fuzzy-scores each label (and its keyword fodder), drops
    /// non-matches, ranks (score desc, then category, then label) and
    /// caps the visible list.
    pub(crate) fn palette_rows(&self, query: &str) -> Vec<PaletteRow> {
        const MAX_ROWS: usize = 12;
        let needle = query.trim().to_lowercase();
        let mut candidates: Vec<Candidate> = Vec::new();

        // ── Generated hotkey rows ──────────────────────────────────────
        // Every editable action (drops the 3 digit/arrow families) minus
        // the palette's own opener (activating it would just re-open).
        let in_terminal = self.active_view == View::Terminal || self.active_tab.is_some();
        for &action in HotkeyAction::all() {
            if !action.primary_editable() || action == HotkeyAction::ShowCommandPalette {
                continue;
            }
            // Widget-dispatched actions (copy / select-all / scrollback
            // paging) only fire from a keystroke reaching the terminal
            // canvas; RunHotkeyAction just swallows them, so a palette
            // click would be a dead row. Leave them out.
            if action.widget_dispatched() {
                continue;
            }
            // Mirror the exact gate the keyboard loop applies before it
            // calls dispatch_hotkey_action (shortcuts.rs): terminal_only
            // needs a focused terminal, vault_only needs the vault area.
            let enabled = (!action.terminal_only() || in_terminal)
                && (!action.vault_only() || self.in_vault_area());
            candidates.push(Candidate {
                label: t(action.label_key()).to_string(),
                keywords: action.id(),
                hotkey: Some(action),
                category: hotkey_category(action),
                message: Message::Tabs(TabsMessage::RunHotkeyAction(action)),
                enabled,
            });
        }

        // ── Non-hotkey extras ──────────────────────────────────────────
        candidates.push(Candidate {
            label: t("lock_vault").to_string(),
            keywords: "lock_vault",
            hotkey: None,
            category: PaletteCategory::Session,
            // Asks first: Lock Vault tears every live session down, so
            // the row opens the confirm dialog, not the teardown.
            message: Message::Vault(VaultMessage::LockVaultConfirm),
            enabled: self.vault_ui.has_user_password,
        });
        candidates.push(Candidate {
            label: t("privacy_mode_label").to_string(),
            keywords: "privacy_mode",
            hotkey: None,
            category: PaletteCategory::Session,
            message: Message::Settings(SettingsMessage::TogglePrivacyMode),
            enabled: true,
        });

        // ── Dynamic: saved hosts ───────────────────────────────────────
        let connect = t("connect");
        for (ci, conn) in self.connections.iter().enumerate() {
            candidates.push(Candidate {
                label: format!("{connect}: {}", conn.label),
                keywords: "connect_host_ssh",
                hotkey: None,
                category: PaletteCategory::Vault,
                message: Message::Ssh(SshMessage::ConnectSsh(ci)),
                enabled: true,
            });
        }

        // ── Dynamic: Settings sections ─────────────────────────────────
        let settings = t("settings");
        for (label, section) in self.settings_section_items() {
            candidates.push(Candidate {
                label: format!("{settings}: {label}"),
                keywords: "settings_section",
                hotkey: None,
                category: PaletteCategory::Settings,
                message: Message::Tabs(TabsMessage::OpenSettingsSection(section)),
                enabled: true,
            });
        }

        // ── Dynamic: individual settings (search index) ────────────────
        // Only under a query: the empty-query listing stays the curated
        // action catalog, the whole settings index would drown it.
        // Activation (RevealSetting) opens the section and drops the
        // setting's label into the sidebar search so it highlights +
        // scrolls into view.
        if !needle.is_empty() {
            for (entry, section_label) in self.settings_search_results(query) {
                candidates.push(Candidate {
                    label: format!("{section_label}: {}", t(entry.label_key)),
                    keywords: entry.keywords,
                    hotkey: None,
                    category: PaletteCategory::Settings,
                    message: Message::Settings(SettingsMessage::RevealSetting(
                        entry.section,
                        entry.label_key,
                    )),
                    enabled: true,
                });
            }
        }

        // ── Score + rank ───────────────────────────────────────────────
        let mut scored: Vec<(i32, Candidate)> = candidates
            .into_iter()
            .filter_map(|c| {
                if needle.is_empty() {
                    return Some((0, c));
                }
                // Label match wins; a keyword-only match still qualifies
                // but scores lower (its own score minus a penalty).
                let label_score = tokenized_fuzzy_score(&c.label, &needle);
                let kw_score = tokenized_fuzzy_score(c.keywords, &needle).map(|s| s - 20);
                let best = match (label_score, kw_score) {
                    (Some(a), Some(b)) => Some(a.max(b)),
                    (Some(a), None) => Some(a),
                    (None, Some(b)) => Some(b),
                    (None, None) => None,
                };
                best.map(|s| (s, c))
            })
            .collect();

        scored.sort_by(|(sa, a), (sb, b)| {
            sb.cmp(sa)
                .then(a.category.order().cmp(&b.category.order()))
                .then_with(|| a.label.cmp(&b.label))
        });

        scored
            .into_iter()
            .take(MAX_ROWS)
            .map(|(_, c)| PaletteRow {
                label: c.label,
                hotkey: c.hotkey,
                category: c.category,
                message: c.message,
                enabled: c.enabled,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_beats_prefix_beats_subsequence() {
        let exact = fuzzy_score("split", "split").unwrap();
        let prefix = fuzzy_score("split pane", "split").unwrap();
        let subseq = fuzzy_score("severe plight", "split").unwrap();
        assert!(exact >= prefix, "{exact} >= {prefix}");
        assert!(prefix > subseq, "{prefix} > {subseq}");
    }

    #[test]
    fn prefix_word_outranks_mid_string() {
        // "local" at the start of "Local terminal" beats "local" buried
        // in "Open local shell".
        let head = fuzzy_score("Local terminal", "local").unwrap();
        let mid = fuzzy_score("Open local shell", "local").unwrap();
        assert!(head > mid, "{head} > {mid}");
    }

    #[test]
    fn no_match_returns_none() {
        assert!(fuzzy_score("Lock vault", "zzzz").is_none());
        assert!(fuzzy_score("abc", "abcd").is_none());
    }

    #[test]
    fn empty_needle_scores_zero() {
        assert_eq!(fuzzy_score("anything", ""), Some(0));
    }

    #[test]
    fn keyword_id_matches_snake_case_fragment() {
        // The hotkey id doubles as fuzzy fodder: "pane" hits
        // "split_pane_vertical".
        assert!(fuzzy_score("split_pane_vertical", "pane").is_some());
        assert!(fuzzy_score("split_pane_vertical", "vertical").is_some());
    }

    #[test]
    fn tokenized_matches_words_in_any_order() {
        // Every word must hit, order-free; the raw subsequence matcher
        // would fail on the literal space + reordered words.
        assert!(tokenized_fuzzy_score("Select text to copy & Right click", "copy select").is_some());
        assert!(fuzzy_score("Select text to copy & Right click", "copy select").is_none());
        // A word with no match anywhere still kills the candidate.
        assert!(tokenized_fuzzy_score("Select text to copy", "copy zebra").is_none());
        // Single-word queries degrade to the plain scorer.
        assert_eq!(
            tokenized_fuzzy_score("Lock vault", "lock"),
            fuzzy_score("Lock vault", "lock"),
        );
    }

    #[test]
    fn cjk_matches_by_contiguous_substring() {
        assert!(fuzzy_score("锁定保险库", "保险").is_some());
        assert!(fuzzy_score("锁定保险库", "关闭").is_none());
    }

    #[test]
    fn every_generated_label_key_resolves_in_en() {
        // Every editable action's label key must resolve (no "???").
        for &action in HotkeyAction::all() {
            if !action.primary_editable() {
                continue;
            }
            let label = crate::i18n::en_lookup(action.label_key());
            assert_ne!(label, "???", "missing i18n for {}", action.id());
        }
        // Category + chrome keys.
        for cat in [
            PaletteCategory::Tabs,
            PaletteCategory::Vault,
            PaletteCategory::Terminal,
            PaletteCategory::Settings,
            PaletteCategory::Session,
        ] {
            assert_ne!(crate::i18n::en_lookup(cat.label_key()), "???");
        }
        for key in [
            "command_palette_title",
            "command_palette_placeholder",
            "command_palette_no_results",
            "hotkey_show_command_palette",
        ] {
            assert_ne!(crate::i18n::en_lookup(key), "???", "missing {key}");
        }
    }
}
