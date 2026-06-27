//! Host config sidebar tab: per-host (and local-terminal) appearance
//! settings, edited live from the terminal sidebar with the terminal in
//! view. Split out of `views/terminal.rs` so that file stays focused on
//! the terminal pane + the sidebar shell.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, scrollable, text, Space};
use iced::{Background, Border, Element, Length, Padding};

use crate::app::{Message, Oryxis};
use crate::i18n::t;
use crate::state::TerminalTab;
use crate::theme::OryxisColors;
use crate::widgets::dir_row;

impl Oryxis {
    /// Swatch-preview theme cards for a sidebar config tab (a compact,
    /// single-column version of the global terminal-theme picker). The
    /// first card is the "follow app theme" sentinel (empty pick).
    /// `on_pick` routes to the per-host or local-session theme message.
    fn sidebar_theme_cards<'a>(
        &'a self,
        selected: Option<&str>,
        on_pick: fn(String) -> Message,
    ) -> Vec<Element<'a, Message>> {
        let mut cards: Vec<Element<'a, Message>> = Vec::new();
        let app_theme_name = crate::theme::AppTheme::active().name();
        let follow_palette = self
            .terminal_palette_for_name(app_theme_name)
            .unwrap_or_default();
        let follow_label = format!("{} ({})", t("terminal_theme_follow_app"), app_theme_name);
        cards.push(crate::widgets::terminal_theme_card(
            follow_palette,
            &follow_label,
            selected.is_none(),
            on_pick(String::new()),
        ));
        for theme in oryxis_terminal::TerminalTheme::ALL.iter() {
            cards.push(crate::widgets::terminal_theme_card(
                theme.palette(),
                theme.name(),
                selected == Some(theme.name()),
                on_pick(theme.name().to_string()),
            ));
        }
        for ct in self.custom_terminal_themes.iter() {
            let palette = self
                .terminal_palette_for_name(&ct.name)
                .unwrap_or_default();
            cards.push(crate::widgets::terminal_theme_card(
                palette,
                &ct.name,
                selected == Some(ct.name.as_str()),
                on_pick(ct.name.clone()),
            ));
        }
        cards
    }

    /// Global terminal appearance controls (font size, font, render
    /// toggles) surfaced in the sidebar config tabs so they can be tuned
    /// on-demand against the live terminal. These dispatch the same global
    /// settings messages, so changes persist and repaint immediately.
    /// Plain `button`/`toggle` widgets (no tooltip) so clicks work here.
    fn terminal_appearance_controls(&self) -> Element<'_, Message> {
        let c = OryxisColors::t();
        let size_row = dir_row(vec![
            text(t("terminal_font_size")).size(12).color(c.text_secondary).into(),
            Space::new().width(Length::Fill).into(),
            step_btn("\u{2212}", Message::TerminalFontSizeDecrease),
            Space::new().width(8).into(),
            text(format!("{:.0}", self.terminal_font_size)).size(13).color(c.text_primary).into(),
            Space::new().width(8).into(),
            step_btn("+", Message::TerminalFontSizeIncrease),
        ])
        .align_y(iced::Alignment::Center);

        let fonts = crate::app::enumerate_terminal_fonts();
        let font_pick = iced::widget::pick_list(
            Some(self.terminal_font_name.clone()),
            fonts,
            |s: &String| s.clone(),
        )
        .on_select(Message::TerminalFontChanged)
        .width(Length::Fill)
        .padding(8)
        .style(crate::widgets::rounded_pick_list_style);

        column![
            text(t("host_config_global_appearance")).size(13).color(c.text_primary),
            Space::new().height(2),
            text(t("host_config_global_note")).size(11).color(c.text_muted),
            Space::new().height(10),
            size_row,
            Space::new().height(10),
            text(t("terminal_font")).size(12).color(c.text_secondary),
            Space::new().height(4),
            font_pick,
            Space::new().height(12),
            crate::widgets::toggle_row(t("bold_bright"), self.setting_bold_is_bright, Message::ToggleBoldIsBright),
            Space::new().height(8),
            crate::widgets::toggle_row(t("keyword_highlight"), self.setting_keyword_highlight, Message::ToggleKeywordHighlight),
            Space::new().height(8),
            crate::widgets::toggle_row(t("smart_contrast"), self.setting_smart_contrast, Message::ToggleSmartContrast),
        ]
        .width(Length::Fill)
        .into()
    }

    /// Host config sidebar tab: per-host appearance/behavior for the
    /// focused pane's connection, edited live (theme repaints the running
    /// terminal instantly; encoding/TERM apply on the next connection).
    pub(crate) fn host_config_tab_content<'a>(&'a self, tab: &'a TerminalTab) -> Element<'a, Message> {
        use iced::widget::pick_list;
        let c = OryxisColors::t();

        // Local/ephemeral panes have no saved host: offer a session-only
        // theme for the open local terminals instead.
        let conn = match &tab.active().origin {
            crate::state::PaneOrigin::Host(id) => self.connections.iter().find(|cn| cn.id == *id),
            _ => None,
        };
        let Some(conn) = conn else {
            return self.local_terminal_config_content();
        };

        let pl_style = crate::widgets::rounded_pick_list_style;

        // Theme: swatch-preview cards (live repaint on pick). The
        // "follow app theme" card is the None sentinel.
        let mut theme_col = column![].spacing(8).width(Length::Fill);
        for card in self.sidebar_theme_cards(conn.terminal_theme.as_deref(), Message::HostConfigThemeChanged) {
            theme_col = theme_col.push(card);
        }

        let encoding_opts: Vec<String> = [
            "UTF-8", "Big5", "GBK", "gb18030", "Shift_JIS", "EUC-JP", "EUC-KR",
            "ISO-8859-1", "ISO-8859-15", "windows-1251", "windows-1252", "KOI8-R",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let encoding_selected = conn.encoding.clone().unwrap_or_else(|| "UTF-8".to_string());
        let encoding_pick = pick_list(Some(encoding_selected), encoding_opts, |s: &String| s.clone())
            .on_select(Message::HostConfigEncodingChanged)
            .width(Length::Fill)
            .padding(8)
            .style(pl_style);

        let term_opts: Vec<String> = [
            "xterm-256color", "xterm", "screen-256color", "tmux-256color", "screen",
            "linux", "vt220", "vt100", "ansi",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let term_selected = conn
            .terminal_type
            .clone()
            .unwrap_or_else(|| "xterm-256color".to_string());
        let term_pick = pick_list(Some(term_selected), term_opts, |s: &String| s.clone())
            .on_select(Message::HostConfigTerminalTypeChanged)
            .width(Length::Fill)
            .padding(8)
            .style(pl_style);

        let title_opts = vec![
            t("host_auto_title_default").to_string(),
            t("host_auto_title_show").to_string(),
            t("host_auto_title_hide").to_string(),
        ];
        let title_selected = match conn.auto_title {
            Some(true) => t("host_auto_title_show"),
            Some(false) => t("host_auto_title_hide"),
            None => t("host_auto_title_default"),
        }
        .to_string();
        let title_pick = pick_list(Some(title_selected), title_opts, |s: &String| s.clone())
            .on_select(Message::HostConfigAutoTitleChanged)
            .width(Length::Fill)
            .padding(8)
            .style(pl_style);

        let label = |key: &str| text(t(key)).size(12).color(OryxisColors::t().text_secondary);

        let body = column![
            text(conn.label.clone()).size(13).color(c.text_primary),
            Space::new().height(2),
            text(t("host_config_subtitle")).size(11).color(c.text_muted),
            Space::new().height(14),
            text(t("host_config_this_terminal")).size(13).color(c.text_primary),
            Space::new().height(10),
            label("host_config_encoding"),
            Space::new().height(4),
            encoding_pick,
            Space::new().height(12),
            label("host_config_terminal_type"),
            Space::new().height(4),
            term_pick,
            Space::new().height(12),
            label("host_auto_title"),
            Space::new().height(4),
            title_pick,
            Space::new().height(8),
            text(t("host_config_reconnect_note")).size(11).color(c.text_muted),
            Space::new().height(18),
            self.terminal_appearance_controls(),
            // Theme cards last: the swatch list is long, so keep the
            // compact selects/toggles above it.
            Space::new().height(18),
            label("terminal_theme"),
            Space::new().height(4),
            theme_col,
        ]
        .width(Length::Fill)
        .padding(Padding { top: 12.0, right: 14.0, bottom: 12.0, left: 14.0 });

        scrollable(body).height(Length::Fill).into()
    }

    /// Host config tab body for local/ephemeral panes: a session-only
    /// terminal theme for the open local shells, with a one-click promote
    /// to the persisted global default.
    fn local_terminal_config_content(&self) -> Element<'_, Message> {
        let c = OryxisColors::t();

        // Theme: swatch cards (applied to the open local panes on pick).
        let mut theme_col = column![].spacing(8).width(Length::Fill);
        for card in
            self.sidebar_theme_cards(self.local_terminal_theme.as_deref(), Message::LocalConfigThemeChanged)
        {
            theme_col = theme_col.push(card);
        }

        // Promote is only meaningful once a session theme is chosen.
        let save_btn = crate::widgets::styled_button_opt(
            t("local_terminal_save_global"),
            self.local_terminal_theme
                .as_ref()
                .map(|_| Message::LocalConfigSaveGlobal),
            c.accent,
        );

        let body = column![
            text(t("local_terminal_config_title")).size(13).color(c.text_primary),
            Space::new().height(2),
            text(t("local_terminal_config_subtitle")).size(11).color(c.text_muted),
            Space::new().height(14),
            self.terminal_appearance_controls(),
            // Theme cards last (long swatch list).
            Space::new().height(18),
            text(t("terminal_theme")).size(12).color(c.text_secondary),
            Space::new().height(4),
            theme_col,
            Space::new().height(12),
            save_btn,
            Space::new().height(6),
            text(t("local_terminal_config_note")).size(11).color(c.text_muted),
        ]
        .width(Length::Fill)
        .padding(Padding { top: 12.0, right: 14.0, bottom: 12.0, left: 14.0 });

        scrollable(body).height(Length::Fill).into()
    }

}

/// Small +/- stepper button (font size). Plain `button` (no tooltip), so
/// it clicks fine inside the sidebar.
fn step_btn<'a>(glyph: &'a str, msg: Message) -> Element<'a, Message> {
    button(
        container(text(glyph.to_owned()).size(14).color(OryxisColors::t().text_primary))
            .padding(Padding { top: 4.0, right: 10.0, bottom: 4.0, left: 10.0 }),
    )
    .on_press(msg)
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().bg_hover,
            _ => OryxisColors::t().bg_selected,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(4.0), ..Default::default() },
            ..Default::default()
        }
    })
    .into()
}
