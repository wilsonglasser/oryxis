//! Host editor: the Terminal card (theme / icon / encoding / TERM
//! appearance tile, session logging, privacy-mode override).
use super::*;
use iced::widget::column;

impl Oryxis {
    pub(super) fn hp_appearance_items(&self) -> Element<'_, Message> {
        // ── Section: Terminal appearance ──
        // A single "click to open picker" tile that mirrors the
        // current pick (palette swatches if a specific theme is set,
        // a plain "inherit" row otherwise). The full picker lives in
        // its own modal so this section stays compact.
        // Themed preview tile: shows the chosen per-host palette, or the
        // inherited global theme when there's no override, so the row is
        // always a real preview instead of a bare "use global" dropdown.
        // Click opens the full picker modal.
        // Resolve the override (built-in OR custom) to a palette for the
        // preview swatch; fall back to the inherited global when there's no
        // override (or the named custom theme was deleted).
        let override_name = self
            .editor_form
            .terminal_theme
            .as_deref()
            .filter(|name| self.terminal_palette_for_name(name).is_some());
        let (preview_palette, theme_label) = match override_name {
            Some(name) => (
                self.terminal_palette_for_name(name).unwrap(),
                name.to_string(),
            ),
            // No host override: the group chain may still answer (D4),
            // and naming the GROUP is more useful than saying "global"
            // when the global is not actually what will be used.
            None => {
                let inherited = self
                    .editor_inherited()
                    .terminal_theme
                    .filter(|(name, _)| self.terminal_palette_for_name(name).is_some());
                match inherited {
                    Some((name, group)) => (
                        self.terminal_palette_for_name(&name).unwrap(),
                        crate::i18n::t("inherited_from")
                            .replace("{value}", &name)
                            .replace("{group}", &group),
                    ),
                    None => (
                        self.resolve_global_terminal_palette(),
                        format!(
                            "{} ({})",
                            crate::i18n::t("terminal_theme_inherit_global"),
                            self.resolve_global_terminal_theme_name()
                        ),
                    ),
                }
            }
        };
        let theme_trigger: Element<'_, Message> = self.panel_nav_slot(
            crate::keynav::RowAction::activate(Message::Editor(EditorMessage::EditorOpenThemePicker)),
            8.0,
            terminal_theme_trigger(preview_palette, theme_label),
        );

        // Per-host icon shape override. The "Use default" entry maps to
        // an empty string which clears the override (resolved to the
        // global default_host_icon at render time).
        // Tokens drive the picker value (same pattern as Settings
        // -> Interface). Empty string is the "use default" token; the
        // dispatcher treats it as a None override on the form field.
        let icon_options = vec![
            String::new(),
            "circular".to_string(),
            "square".to_string(),
            "rounded".to_string(),
            "outline".to_string(),
            "initials".to_string(),
        ];
        let icon_selected = self.editor_form.icon_style.clone().unwrap_or_default();
        let icon_picker = pick_list(
            Some(icon_selected),
            icon_options,
            |s: &String| {
                let key = match s.as_str() {
                    "circular" => "icon_circular",
                    "square" => "icon_square",
                    "rounded" => "icon_rounded",
                    "outline" => "icon_outline",
                    "initials" => "icon_initials",
                    _ => "icon_use_default",
                };
                crate::i18n::t(key).to_string()
            },
        )
        .on_select(|v| Message::Editor(EditorMessage::EditorIconStyleChanged(v)))
        .id(iced::widget::Id::new("editor-pick-icon-style"))
        .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
        .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
        .width(170)
        .padding(10)
        .style(crate::widgets::rounded_pick_list_style);
        // Focusable select (Tab + Enter/Space, widget-owned keys).
        let icon_row: Element<'_, Message> = dir_row(vec![
            text(crate::i18n::t("host_icon_style")).size(13).color(OryxisColors::t().text_secondary).into(),
            Space::new().width(Length::Fill).into(),
            self.panel_nav_slot(
                crate::keynav::RowAction::input(iced::widget::Id::new("editor-pick-icon-style")),
                crate::widgets::INPUT_RADIUS,
                icon_picker.into(),
            ),
        ]).align_y(iced::Alignment::Center).into();

        // Per-host terminal encoding. "UTF-8" is the default (stored as
        // None); the rest are encoding_rs labels the SSH engine transcodes.
        let encoding_options: Vec<String> = [
            "UTF-8", "Big5", "GBK", "gb18030", "Shift_JIS", "EUC-JP",
            "EUC-KR", "ISO-8859-1", "ISO-8859-15", "windows-1251",
            "windows-1252", "KOI8-R",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let encoding_selected = self
            .editor_form
            .encoding
            .clone()
            .unwrap_or_else(|| "UTF-8".to_string());
        let encoding_picker = pick_list(Some(encoding_selected), encoding_options, |s: &String| s.clone())
            .on_select(|v| Message::Editor(EditorMessage::EditorEncodingChanged(v)))
            .id(iced::widget::Id::new("editor-pick-encoding"))
            .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
            .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
            .width(170)
            .padding(10)
            .style(crate::widgets::rounded_pick_list_style);
        // Focusable select, same treatment as the icon row.
        let encoding_row: Element<'_, Message> = dir_row(vec![
            text(crate::i18n::t("host_encoding")).size(13).color(OryxisColors::t().text_secondary).into(),
            Space::new().width(Length::Fill).into(),
            self.panel_nav_slot(
                crate::keynav::RowAction::input(iced::widget::Id::new("editor-pick-encoding")),
                crate::widgets::INPUT_RADIUS,
                encoding_picker.into(),
            ),
        ]).align_y(iced::Alignment::Center).into();

        // Per-host ambiguous width (J4). Next to encoding because that is
        // what `Auto` reads: a legacy CJK charset is the only per-host
        // signal that the remote measures these characters as two cells.
        // The explicit answers exist for the larger cohort that CANNOT be
        // read off anything, a CJK environment on UTF-8.
        use oryxis_core::models::connection::AmbiguousWidth;
        let ambiguous_options = vec![
            AmbiguousWidth::Auto,
            AmbiguousWidth::Narrow,
            AmbiguousWidth::Wide,
        ];
        let ambiguous_picker = pick_list(
            Some(self.editor_form.ambiguous_width),
            ambiguous_options,
            |w: &AmbiguousWidth| crate::i18n::t(crate::util::ambiguous_width_key(*w)).to_string(),
        )
        .on_select(|v| Message::Editor(EditorMessage::EditorAmbiguousWidthChanged(v)))
        .id(iced::widget::Id::new("editor-pick-ambiguous-width"))
        .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
        .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
        // Wider than its neighbours because the default value names what
        // it follows, and a truncated "Auto (follow enco..." is exactly
        // the half of the answer nobody needs.
        .width(210)
        .padding(10)
        .style(crate::widgets::rounded_pick_list_style);
        let ambiguous_row: Element<'_, Message> = dir_row(vec![
            text(crate::i18n::t("host_ambiguous_width")).size(13).color(OryxisColors::t().text_secondary).into(),
            Space::new().width(Length::Fill).into(),
            self.panel_nav_slot(
                crate::keynav::RowAction::input(iced::widget::Id::new("editor-pick-ambiguous-width")),
                crate::widgets::INPUT_RADIUS,
                ambiguous_picker.into(),
            ),
        ]).align_y(iced::Alignment::Center).into();
        // The honest half: this side decides how to DRAW, the remote's
        // wcwidth decides where its programs PUT things, and only the
        // pair being equal makes a TUI line up.
        let ambiguous_hint: Element<'_, Message> = container(
            text(crate::i18n::t("ambiguous_width_hint"))
                .size(11)
                .color(OryxisColors::t().text_muted),
        )
        .width(Length::Fill)
        .align_x(dir_align_x())
        .into();

        // Per-host TERM. "xterm-256color" is the default (stored as None);
        // the rest are fallbacks for hosts whose terminfo trips on it.
        let term_options: Vec<String> = [
            "xterm-256color", "xterm", "screen-256color", "tmux-256color",
            "screen", "linux", "vt220", "vt100", "ansi",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let term_selected = self
            .editor_form
            .terminal_type
            .clone()
            .unwrap_or_else(|| "xterm-256color".to_string());
        let term_picker = pick_list(Some(term_selected), term_options, |s: &String| s.clone())
            .on_select(|v| Message::Editor(EditorMessage::EditorTerminalTypeChanged(v)))
            .id(iced::widget::Id::new("editor-pick-term"))
            .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
            .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
            .width(170)
            .padding(10)
            .style(crate::widgets::rounded_pick_list_style);
        // Focusable select, same treatment as the icon row.
        let term_row: Element<'_, Message> = dir_row(vec![
            text(crate::i18n::t("host_terminal_type")).size(13).color(OryxisColors::t().text_secondary).into(),
            Space::new().width(Length::Fill).into(),
            self.panel_nav_slot(
                crate::keynav::RowAction::input(iced::widget::Id::new("editor-pick-term")),
                crate::widgets::INPUT_RADIUS,
                term_picker.into(),
            ),
        ]).align_y(iced::Alignment::Center).into();

        // Terminal card body: the theme keeps its full-width preview tile
        // (it's a live swatch, not a plain dropdown); icon and encoding
        // are compact inline rows (label left, picker right) like Auth
        // Method, so the section reads tight instead of three stacked
        // label+description blocks.
        let mut appearance_items = column![
            text(crate::i18n::t("terminal_theme"))
                .size(13)
                .color(OryxisColors::t().text_secondary),
            Space::new().height(8),
            theme_trigger,
            Space::new().height(14),
            icon_row,
            Space::new().height(12),
            encoding_row,
            Space::new().height(12),
            ambiguous_row,
            Space::new().height(4),
            ambiguous_hint,
            Space::new().height(12),
            term_row,
        ];
        // Backdrop overrides. Built here, after the rows above, because
        // the keyboard walk records in build order and must match what
        // the eye sees.
        for row in self.hp_backdrop_rows() {
            appearance_items = appearance_items.push(Space::new().height(12)).push(row);
        }
        // This host's own highlight rules, last for the same reason:
        // build order is record order for the keyboard walk. The block
        // itself is the one from Settings, pointed at the host's list.
        appearance_items = appearance_items
            .push(Space::new().height(18))
            .push(self.highlight_rules_block(
                crate::state::RuleScope::Host,
                &self.editor_form.highlight_rules.rules,
            ));
        appearance_items.into()
    }

    /// This host's backdrop overrides: opacity, background picture,
    /// and (once a picture applies here) its fit and fade.
    ///
    /// Every picker carries an "Inherit" entry that reads as what it
    /// resolves to right now, so the row answers "what will this host
    /// actually look like" without making the user open Settings to
    /// find out. The picture row has three states rather than two:
    /// with a global picture set, "inherit" and "none" are different
    /// answers, and a host that wants a clean terminal needs to be able
    /// to say the second one.
    fn hp_backdrop_rows(&self) -> Vec<Element<'_, Message>> {
        let inherit = crate::i18n::t("appearance_inherit");
        let app = &self.editor_form.terminal_appearance;
        let mut rows: Vec<Element<'_, Message>> = Vec::new();

        // Opacity: inherit + the same steps Settings offers.
        let mut opacity_options = vec![format!(
            "{inherit} ({}%)",
            self.prefs.terminal_opacity
        )];
        opacity_options.extend(crate::theme::OPACITY_STEPS.iter().map(|p| format!("{p}%")));
        let opacity_selected = match app.opacity {
            Some(p) => format!("{p}%"),
            None => opacity_options[0].clone(),
        };
        rows.push(self.hp_backdrop_pick(
            crate::i18n::t("terminal_opacity"),
            "editor-pick-bg-opacity",
            opacity_options,
            opacity_selected,
            |v| Message::Editor(EditorMessage::EditorOpacityChanged(v)),
        ));

        // Picture: inherit / none / this host's own file.
        let global_name = picture_name(&self.prefs.terminal_bg_image);
        let custom = crate::i18n::t("appearance_custom_image");
        let image_options = vec![
            format!("{inherit} ({global_name})"),
            crate::i18n::t("none").to_string(),
            custom.to_string(),
        ];
        let image_selected = match app.image.as_deref() {
            None => image_options[0].clone(),
            Some("") => image_options[1].clone(),
            Some(_) => custom.to_string(),
        };
        rows.push(self.hp_backdrop_pick(
            crate::i18n::t("terminal_bg_image"),
            "editor-pick-bg-image",
            image_options,
            image_selected,
            |v| Message::Editor(EditorMessage::EditorBgImageModeChanged(v)),
        ));

        // The file itself, only while this host carries its own.
        if let Some(path) = app.image.as_deref().filter(|p| !p.is_empty()) {
            rows.push(
                dir_row(vec![
                    text(picture_name(path))
                        .size(12)
                        .color(OryxisColors::t().text_muted)
                        .into(),
                    Space::new().width(Length::Fill).into(),
                    self.panel_nav_slot(
                        crate::keynav::RowAction::activate(Message::Editor(
                            EditorMessage::EditorBgImageBrowse,
                        )),
                        8.0,
                        crate::widgets::styled_button_opt(
                            crate::i18n::t("browse"),
                            Some(Message::Editor(EditorMessage::EditorBgImageBrowse)),
                            OryxisColors::t().accent,
                        ),
                    ),
                ])
                .align_y(iced::Alignment::Center)
                .into(),
            );
        }
        // No Browse row in the other two states on purpose: picking
        // "Custom picture" in the row above opens the dialog itself, so
        // a second entry point would only ever be a button that repeats
        // the choice the user just made.

        // Fit and fade matter only when a picture actually applies to
        // this host, whether its own or the inherited one.
        let picture_applies = match app.image.as_deref() {
            Some("") => false,
            Some(_) => true,
            None => !self.prefs.terminal_bg_image.trim().is_empty(),
        };
        if picture_applies {
            let global_fit = oryxis_terminal::BgFit::from_str_or_default(
                &self.prefs.terminal_bg_fit,
            );
            let mut fit_options = vec![format!(
                "{inherit} ({})",
                crate::i18n::t(crate::terminal_appearance::bg_fit_label_key(global_fit))
            )];
            fit_options.extend(oryxis_terminal::BgFit::ALL.iter().map(|f| {
                crate::i18n::t(crate::terminal_appearance::bg_fit_label_key(*f)).to_string()
            }));
            let fit_selected = match app.fit.as_deref() {
                Some(f) => crate::i18n::t(crate::terminal_appearance::bg_fit_label_key(
                    oryxis_terminal::BgFit::from_str_or_default(f),
                ))
                .to_string(),
                None => fit_options[0].clone(),
            };
            rows.push(self.hp_backdrop_pick(
                crate::i18n::t("terminal_bg_fit"),
                "editor-pick-bg-fit",
                fit_options,
                fit_selected,
                |v| Message::Editor(EditorMessage::EditorBgFitChanged(v)),
            ));

            let mut dim_options =
                vec![format!("{inherit} ({}%)", self.prefs.terminal_bg_dim)];
            dim_options.extend(
                crate::terminal_appearance::DIM_STEPS
                    .iter()
                    .map(|p| format!("{p}%")),
            );
            let dim_selected = match app.dim {
                Some(p) => format!("{p}%"),
                None => dim_options[0].clone(),
            };
            rows.push(self.hp_backdrop_pick(
                crate::i18n::t("terminal_bg_dim"),
                "editor-pick-bg-dim",
                dim_options,
                dim_selected,
                |v| Message::Editor(EditorMessage::EditorBgDimChanged(v)),
            ));
        }

        rows
    }

    /// One label-left / picker-right row in the backdrop block, wired
    /// into the panel's keyboard walk like every other host-editor
    /// select.
    fn hp_backdrop_pick<'a>(
        &'a self,
        label: &'a str,
        id: &'static str,
        options: Vec<String>,
        selected: String,
        on_select: impl Fn(String) -> Message + 'a,
    ) -> Element<'a, Message> {
        let picker = pick_list(Some(selected), options, |s: &String| s.clone())
            .on_select(on_select)
            .id(iced::widget::Id::new(id))
            .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
            .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
            .width(200)
            .padding(10)
            .style(crate::widgets::rounded_pick_list_style);
        dir_row(vec![
            text(label)
                .size(13)
                .color(OryxisColors::t().text_secondary)
                .into(),
            Space::new().width(Length::Fill).into(),
            self.panel_nav_slot(
                crate::keynav::RowAction::input(iced::widget::Id::new(id)),
                crate::widgets::INPUT_RADIUS,
                picker.into(),
            ),
        ])
        .align_y(iced::Alignment::Center)
        .into()
    }

    pub(super) fn hp_row_session_logging(&self) -> Element<'_, Message> {
        // Session logging (universal -> Terminal). Tri-state: Default
        // (inherit global) / On / Off. Enter/Space cycles the state.
        let row_session_logging: Element<'_, Message> = self.panel_nav_slot(
            crate::keynav::RowAction::activate(Message::Editor(EditorMessage::EditorCycleSessionLogging)),
            8.0,
            container(
                dir_row(vec![
                    iced_fonts::lucide::file_text().size(14).color(OryxisColors::t().text_muted).into(),
                    Space::new().width(10).into(),
                    text(t("session_logging")).size(13).color(OryxisColors::t().text_secondary).into(),
                    Space::new().width(Length::Fill).into(),
                    {
                        let (label_key, bg) = match self.editor_form.session_logging {
                            None => ("session_log_default", OryxisColors::t().bg_hover),
                            Some(true) => ("session_log_on", OryxisColors::t().success),
                            Some(false) => ("session_log_off", OryxisColors::t().error),
                        };
                        let fg = crate::theme::contrast_text_for(bg);
                        button(text(t(label_key)).size(12).color(fg))
                            .on_press(Message::Editor(EditorMessage::EditorCycleSessionLogging))
                            .style(move |_theme, _status| button::Style {
                                background: Some(Background::Color(bg)),
                                border: Border { radius: Radius::from(4.0), ..Default::default() },
                                text_color: fg,
                                ..Default::default()
                            })
                            .into()
                    },
                ]).align_y(iced::Alignment::Center)
            )
            .padding(Padding { top: 8.0, right: 0.0, bottom: 8.0, left: 0.0 }).into(),
        );
        row_session_logging
    }

    pub(super) fn hp_row_privacy_mode(&self) -> Element<'_, Message> {
        // Per-host Privacy Mode override: Default (inherit global) / On
        // (always hide sensitive data for this host) / Off (never hide).
        let privacy_mode_selected = match self.editor_form.privacy_mode {
            Some(true) => t("host_privacy_mode_on"),
            Some(false) => t("host_privacy_mode_off"),
            None => t("host_privacy_mode_default"),
        }
        .to_string();
        let privacy_mode_options = vec![
            t("host_privacy_mode_default").to_string(),
            t("host_privacy_mode_on").to_string(),
            t("host_privacy_mode_off").to_string(),
        ];
        // Focusable select (Tab + Enter/Space, widget-owned keys).
        let row_privacy_mode: Element<'_, Message> = panel_option_row(
            iced_fonts::lucide::eye_off(),
            t("host_privacy_mode"),
            self.panel_nav_slot(
                crate::keynav::RowAction::input(iced::widget::Id::new("editor-pick-privacy-mode")),
                crate::widgets::INPUT_RADIUS,
                pick_list(Some(privacy_mode_selected), privacy_mode_options, |s: &String| s.clone())
                    .on_select(|v| Message::Editor(EditorMessage::EditorPrivacyModeChanged(v)))
                    .id(iced::widget::Id::new("editor-pick-privacy-mode"))
                    .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                    .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                    .width(120)
                    .padding(10)
                    .style(crate::widgets::rounded_pick_list_style)
                    .into(),
            ),
        );
        row_privacy_mode
    }

    pub(super) fn hp_row_sidebar_auto_open(&self) -> Element<'_, Message> {
        // Per-host sidebar auto-open override: Default (inherit the
        // global setting) / On (always open on connect) / Off (never).
        // Reuses the tri-state option labels of the privacy row.
        let selected = match self.editor_form.sidebar_auto_open {
            Some(true) => t("host_privacy_mode_on"),
            Some(false) => t("host_privacy_mode_off"),
            None => t("host_privacy_mode_default"),
        }
        .to_string();
        let options = vec![
            t("host_privacy_mode_default").to_string(),
            t("host_privacy_mode_on").to_string(),
            t("host_privacy_mode_off").to_string(),
        ];
        let row: Element<'_, Message> = panel_option_row(
            iced_fonts::lucide::panel_left_open(),
            t("sidebar_auto_open"),
            self.panel_nav_slot(
                crate::keynav::RowAction::input(iced::widget::Id::new(
                    "editor-pick-sidebar-auto-open",
                )),
                crate::widgets::INPUT_RADIUS,
                pick_list(Some(selected), options, |s: &String| s.clone())
                    .on_select(|v| Message::Editor(EditorMessage::EditorSidebarAutoOpenChanged(v)))
                    .id(iced::widget::Id::new("editor-pick-sidebar-auto-open"))
                    .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                    .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                    .width(120)
                    .padding(10)
                    .style(crate::widgets::rounded_pick_list_style)
                    .into(),
            ),
        );
        row
    }

    /// A keyboard-navigable pick_list row for the Advanced-terminal
    /// section (icon + label + widget-owned select), mirroring the
    /// privacy-mode row.
    fn hp_quirk_pick_row<'a>(
        &self,
        icon: iced::widget::Text<'a>,
        label: &'a str,
        id: &'static str,
        selected: String,
        options: Vec<String>,
        on_select: fn(String) -> Message,
    ) -> Element<'a, Message> {
        panel_option_row(
            icon,
            label,
            self.panel_nav_slot(
                crate::keynav::RowAction::input(iced::widget::Id::new(id)),
                crate::widgets::INPUT_RADIUS,
                pick_list(Some(selected), options, |s: &String| s.clone())
                    .on_select(on_select)
                    .id(iced::widget::Id::new(id))
                    .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                    .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                    // Wide enough for the longest label ("Control-? (127)").
                    .width(160.0)
                    .padding(10)
                    .style(crate::widgets::rounded_pick_list_style)
                    .into(),
            ),
        )
    }

    /// An on/off toggle row for the Advanced-terminal section (mirrors
    /// the agent-forwarding toggle: click / Enter flips it).
    fn hp_quirk_toggle_row<'a>(
        &self,
        icon: iced::widget::Text<'a>,
        label: &'a str,
        on: bool,
        msg: fn(bool) -> Message,
    ) -> Element<'a, Message> {
        let toggle_msg = msg(!on);
        self.panel_nav_slot(
            crate::keynav::RowAction::activate(toggle_msg.clone()),
            8.0,
            container(
                dir_row(vec![
                    icon.size(14).color(OryxisColors::t().text_muted).into(),
                    Space::new().width(10).into(),
                    text(label).size(13).color(OryxisColors::t().text_secondary).into(),
                    Space::new().width(Length::Fill).into(),
                    {
                        let bg = if on { OryxisColors::t().success } else { OryxisColors::t().bg_hover };
                        let fg = crate::theme::contrast_text_for(bg);
                        button(
                            text(if on { t("toggle_on") } else { t("toggle_off") })
                                .size(12)
                                .color(fg),
                        )
                        .on_press(toggle_msg)
                        .style(move |_theme, _status| button::Style {
                            background: Some(Background::Color(bg)),
                            border: Border { radius: Radius::from(4.0), ..Default::default() },
                            text_color: fg,
                            ..Default::default()
                        })
                        .into()
                    },
                ])
                .align_y(iced::Alignment::Center),
            )
            .padding(Padding { top: 8.0, right: 0.0, bottom: 8.0, left: 0.0 })
            .into(),
        )
    }

    /// C5 "Advanced terminal" section: per-host legacy keyboard modes
    /// (backspace / home-end / function keys) and feature toggles
    /// (mouse reporting, title changes, OSC 52 clipboard, SSH rekey
    /// limit). Only rendered for terminal protocols (`is_terminal`);
    /// RDP/VNC hosts never call this.
    pub(super) fn hp_advanced_terminal_items(&self) -> Element<'_, Message> {
        use oryxis_core::models::terminal_quirks::{
            BackspaceMode, FunctionKeyMode, HomeEndMode, OptionAsMeta,
        };
        let q = &self.editor_form.quirks;

        let backspace_row = self.hp_quirk_pick_row(
            iced_fonts::lucide::delete(),
            t("quirks_backspace"),
            "editor-pick-quirk-backspace",
            crate::util::quirk_backspace_label(q.backspace),
            vec![
                crate::util::quirk_backspace_label(BackspaceMode::Del127),
                crate::util::quirk_backspace_label(BackspaceMode::CtrlH),
            ],
            |v| Message::Editor(EditorMessage::EditorQuirkBackspaceChanged(v)),
        );

        let home_end_row = self.hp_quirk_pick_row(
            iced_fonts::lucide::move_horizontal(),
            t("quirks_home_end"),
            "editor-pick-quirk-homeend",
            crate::util::quirk_home_end_label(q.home_end),
            vec![
                crate::util::quirk_home_end_label(HomeEndMode::Standard),
                crate::util::quirk_home_end_label(HomeEndMode::Rxvt),
            ],
            |v| Message::Editor(EditorMessage::EditorQuirkHomeEndChanged(v)),
        );

        let fn_keys_row = self.hp_quirk_pick_row(
            iced_fonts::lucide::keyboard(),
            t("quirks_fn_keys"),
            "editor-pick-quirk-fnkeys",
            crate::util::quirk_fn_keys_label(q.function_keys),
            vec![
                crate::util::quirk_fn_keys_label(FunctionKeyMode::Xterm),
                crate::util::quirk_fn_keys_label(FunctionKeyMode::LinuxConsole),
                crate::util::quirk_fn_keys_label(FunctionKeyMode::Vt400),
                crate::util::quirk_fn_keys_label(FunctionKeyMode::Rxvt),
            ],
            |v| Message::Editor(EditorMessage::EditorQuirkFnKeysChanged(v)),
        );

        let mouse_row = self.hp_quirk_toggle_row(
            iced_fonts::lucide::mouse_pointer_click(),
            t("quirks_mouse_reporting"),
            !q.disable_mouse_reporting,
            |v| Message::Editor(EditorMessage::EditorQuirkMouseReportingChanged(v)),
        );
        let title_row = self.hp_quirk_toggle_row(
            iced_fonts::lucide::r#type(),
            t("quirks_title_change"),
            !q.disable_title_change,
            |v| Message::Editor(EditorMessage::EditorQuirkTitleChangeChanged(v)),
        );

        let osc52_selected = match q.osc52 {
            Some(oryxis_core::models::terminal_quirks::Osc52Override::On) => t("quirks_osc52_on"),
            Some(oryxis_core::models::terminal_quirks::Osc52Override::Off) => t("quirks_osc52_off"),
            None => t("quirks_osc52_default"),
        }
        .to_string();
        let osc52_row = self.hp_quirk_pick_row(
            iced_fonts::lucide::clipboard(),
            t("quirks_osc52"),
            "editor-pick-quirk-osc52",
            osc52_selected,
            vec![
                t("quirks_osc52_default").to_string(),
                t("quirks_osc52_on").to_string(),
                t("quirks_osc52_off").to_string(),
            ],
            |v| Message::Editor(EditorMessage::EditorQuirkOsc52Changed(v)),
        );

        // macOS Option-as-Meta (issue #80). Shown on every platform: the
        // vault syncs across OSes, so a host edited on Linux/Windows must
        // still be able to carry the quirk its Mac replica applies; the
        // "(macOS)" in the label says where it takes effect.
        let option_meta_row = self.hp_quirk_pick_row(
            iced_fonts::lucide::option(),
            t("quirks_option_meta"),
            "editor-pick-quirk-optionmeta",
            crate::util::quirk_option_as_meta_label(q.option_as_meta),
            vec![
                crate::util::quirk_option_as_meta_label(OptionAsMeta::None),
                crate::util::quirk_option_as_meta_label(OptionAsMeta::OnlyLeft),
                crate::util::quirk_option_as_meta_label(OptionAsMeta::OnlyRight),
                crate::util::quirk_option_as_meta_label(OptionAsMeta::Both),
            ],
            |v| Message::Editor(EditorMessage::EditorQuirkOptionAsMetaChanged(v)),
        );

        // Rekey limit: a small numeric text input (empty = russh default).
        let rekey_input: Element<'_, Message> = self.panel_nav_slot(
            crate::keynav::RowAction::input(iced::widget::Id::new("editor-quirk-rekey")),
            crate::widgets::INPUT_RADIUS,
            text_input(t("quirks_rekey_hint"), &self.editor_form.rekey_limit_mb)
                .id(iced::widget::Id::new("editor-quirk-rekey"))
                .on_input(|v| Message::Editor(EditorMessage::EditorQuirkRekeyChanged(v)))
                .width(120)
                .padding(8)
                .style(crate::widgets::rounded_input_style)
                .into(),
        );
        let rekey_row = panel_option_row(
            iced_fonts::lucide::refresh_cw(),
            t("quirks_rekey_limit"),
            rekey_input,
        );

        column![
            section_header(t("quirks_section_title")),
            Space::new().height(2),
            text(t("quirks_applies_next_connect"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(4),
            backspace_row,
            home_end_row,
            fn_keys_row,
            mouse_row,
            title_row,
            osc52_row,
            option_meta_row,
            rekey_row,
        ]
        .spacing(2)
        .into()
    }
}

/// File name of a picture path for display, or the localized "None"
/// when there is no path. The full path would wrap the row and is
/// recoverable from the picker, which reopens where it left off.
fn picture_name(path: &str) -> String {
    let path = path.trim();
    if path.is_empty() {
        return crate::i18n::t("none").to_string();
    }
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}
