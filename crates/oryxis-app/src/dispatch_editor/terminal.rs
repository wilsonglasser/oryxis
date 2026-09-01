//! What the session looks and behaves like once it is up.
//!
//! Theme, encoding, `TERM`, the startup command, and the compatibility
//! quirks (backspace, Home/End, function keys, mouse reporting, OSC 52,
//! Option-as-Meta) plus the algorithm overrides.

use super::*;

impl Oryxis {
    /// Open the OS picture picker off the event loop and deliver the
    /// path through `on_pick`. Shared by the host editor's Browse button
    /// and by choosing "Custom picture" in the mode row, which is the
    /// same request phrased two ways. `Err` is a cancel and every
    /// consumer ignores it, leaving the previous choice in place.
    pub(crate) fn pick_background_image(
        on_pick: impl Fn(Result<String, String>) -> Message + Send + 'static,
    ) -> Task<Message> {
        Task::perform(
            tokio::task::spawn_blocking(move || {
                rfd::FileDialog::new()
                    .set_title(crate::i18n::t("terminal_bg_image"))
                    // The formats iced's image pipeline decodes. SVG is
                    // deliberately absent: it goes through a different
                    // renderer path than raster handles, so offering it
                    // would pick a file that silently never draws.
                    .add_filter(
                        crate::i18n::t("terminal_bg_filter"),
                        &["png", "jpg", "jpeg", "webp", "bmp", "gif", "tiff", "tif"],
                    )
                    .pick_file()
                    .map(|p| p.to_string_lossy().to_string())
                    .ok_or_else(|| "cancelled".to_string())
            }),
            move |result| {
                let r = match result {
                    Ok(r) => r,
                    Err(e) => Err(format!("Thread error: {e}")),
                };
                on_pick(r)
            },
        )
    }

    pub(super) fn handle_editor_terminal(&mut self, message: EditorMessage) -> Task<Message> {
        match message {
            EditorMessage::EditorOpenThemePicker => {
                self.panels.theme_picker = true;
                // A stale filter from the last visit would open the
                // picker on a mysteriously short list.
                self.theme_ui.picker_filter.clear();
            }
            EditorMessage::EditorCloseThemePicker => {
                self.panels.theme_picker = false;
            }
            EditorMessage::EditorThemePickerFilterChanged(v) => {
                self.theme_ui.picker_filter = v;
            }
            EditorMessage::EditorTerminalThemeChanged(name) => {
                // Empty string == "inherit the global pick".
                self.editor_form.terminal_theme =
                    if name.is_empty() { None } else { Some(name) };
                self.panels.theme_picker = false;
            }
            EditorMessage::EditorOpacityChanged(label) => {
                // The sentinel label clears the override; everything
                // else is one of the "85%" steps.
                self.editor_form.terminal_appearance.opacity =
                    if label == crate::i18n::t("appearance_inherit") {
                        None
                    } else {
                        label.trim_end_matches('%').parse::<u8>().ok()
                    };
            }
            EditorMessage::EditorBgImageModeChanged(label) => {
                // Three states, because "inherit" and "none" are
                // genuinely different answers once a global picture
                // exists: inherit shows it, none is this host opting
                // out.
                if label == crate::i18n::t("appearance_inherit") {
                    self.editor_form.terminal_appearance.image = None;
                } else if label == crate::i18n::t("none") {
                    self.editor_form.terminal_appearance.image = Some(String::new());
                } else {
                    // "Custom picture" IS the request to choose one, so
                    // it opens the dialog rather than parking the row on
                    // a state with no file behind it. Cancelling leaves
                    // the previous choice untouched, which is why the
                    // field is not cleared first.
                    return Self::pick_background_image(|r| {
                        Message::Editor(EditorMessage::EditorBgImagePicked(r))
                    });
                }
            }
            EditorMessage::EditorBgImageBrowse => {
                return Self::pick_background_image(|r| {
                    Message::Editor(EditorMessage::EditorBgImagePicked(r))
                });
            }
            EditorMessage::EditorBgImagePicked(result) => {
                if let Ok(path) = result {
                    self.editor_form.terminal_appearance.image = Some(path);
                }
            }
            EditorMessage::EditorBgFitChanged(label) => {
                self.editor_form.terminal_appearance.fit = if label
                    == crate::i18n::t("appearance_inherit")
                {
                    None
                } else {
                    oryxis_terminal::BgFit::ALL
                        .iter()
                        .copied()
                        .find(|f| {
                            crate::i18n::t(crate::terminal_appearance::bg_fit_label_key(*f))
                                == label
                        })
                        .map(|f| f.as_str().to_string())
                };
            }
            EditorMessage::EditorBgDimChanged(label) => {
                self.editor_form.terminal_appearance.dim =
                    if label == crate::i18n::t("appearance_inherit") {
                        None
                    } else {
                        label.trim_end_matches('%').parse::<u8>().ok()
                    };
            }
            EditorMessage::EditorEncodingChanged(v) => {
                // "UTF-8" is the implicit default, stored as None so the
                // SSH engine skips transcoding entirely.
                self.editor_form.encoding = if v == "UTF-8" { None } else { Some(v) };
            }
            EditorMessage::EditorAmbiguousWidthChanged(v) => {
                self.editor_form.ambiguous_width = v;
            }
            EditorMessage::EditorTerminalTypeChanged(v) => {
                // "xterm-256color" is the implicit default, stored as None.
                self.editor_form.terminal_type =
                    if v == "xterm-256color" { None } else { Some(v) };
            }
            EditorMessage::EditorAutoTitleChanged(v) => {
                use crate::i18n::t;
                // Map the localized pick label back to the tri-state override.
                self.editor_form.auto_title = if v == t("host_auto_title_show") {
                    Some(true)
                } else if v == t("host_auto_title_hide") {
                    Some(false)
                } else {
                    None
                };
            }
            EditorMessage::EditorPrivacyModeChanged(v) => {
                use crate::i18n::t;
                // Map the localized pick label back to the tri-state override.
                self.editor_form.privacy_mode = if v == t("host_privacy_mode_on") {
                    Some(true)
                } else if v == t("host_privacy_mode_off") {
                    Some(false)
                } else {
                    None
                };
            }
            EditorMessage::EditorSidebarAutoOpenChanged(v) => {
                use crate::i18n::t;
                // Same localized-label mapping as the privacy row above.
                self.editor_form.sidebar_auto_open = if v == t("host_privacy_mode_on") {
                    Some(true)
                } else if v == t("host_privacy_mode_off") {
                    Some(false)
                } else {
                    None
                };
            }
            EditorMessage::EditorSftpInitialPathChanged(v) => {
                // Free text: the path lives on the remote host, so there is
                // nothing to validate locally. Newlines are stripped because
                // a paste can carry one and a remote path never contains it.
                self.editor_form.sftp_initial_path =
                    v.chars().filter(|c| *c != '\n' && *c != '\r').collect();
            }
            EditorMessage::EditorStartupComboOpened => {
                // Same rule as the key combo: the widget clears its own
                // input on focus, so this only picks up a snippet added
                // while the editor was open, and only when the list
                // really changed (see `refresh_combo`).
                let options = self.editor_startup_options();
                Self::refresh_combo(&mut self.editor_startup_combo, options);
            }
            EditorMessage::EditorStartupChoiceChanged(label) => {
                use crate::state::StartupChoice;
                // Map the picker label back to a source. The None / Custom
                // sentinels come from i18n; anything else is a snippet
                // label. A snippet is stored as a live reference (its id),
                // resolved to the snippet body at connect time, so we
                // don't copy the body into the custom text editor here.
                if label == crate::i18n::t("startup_none") {
                    self.editor_startup_choice = StartupChoice::None;
                    self.editor_initial_command =
                        iced::widget::text_editor::Content::new();
                } else if label == crate::i18n::t("startup_custom") {
                    self.editor_startup_choice = StartupChoice::Custom;
                } else if let Some(s) =
                    self.snippets.iter().find(|s| s.label == label)
                {
                    self.editor_startup_choice = StartupChoice::Snippet(s.id);
                }
            }
            EditorMessage::EditorInitialCommandChanged(action) => {
                self.editor_initial_command.perform(action);
            }
            EditorMessage::EditorQuirkBackspaceChanged(v) => {
                self.editor_form.quirks.backspace = crate::util::quirk_backspace_from_label(&v);
            }
            EditorMessage::EditorQuirkHomeEndChanged(v) => {
                self.editor_form.quirks.home_end = crate::util::quirk_home_end_from_label(&v);
            }
            EditorMessage::EditorQuirkFnKeysChanged(v) => {
                self.editor_form.quirks.function_keys = crate::util::quirk_fn_keys_from_label(&v);
            }
            EditorMessage::EditorQuirkMouseReportingChanged(on) => {
                // Toggle shows the positive "report mouse"; off disables it.
                self.editor_form.quirks.disable_mouse_reporting = !on;
            }
            EditorMessage::EditorQuirkTitleChangeChanged(on) => {
                self.editor_form.quirks.disable_title_change = !on;
            }
            EditorMessage::EditorQuirkOsc52Changed(v) => {
                self.editor_form.quirks.osc52 = crate::util::quirk_osc52_from_label(&v);
            }
            EditorMessage::EditorQuirkOptionAsMetaChanged(v) => {
                self.editor_form.quirks.option_as_meta =
                    crate::util::quirk_option_as_meta_from_label(&v);
            }
            EditorMessage::EditorQuirkRekeyChanged(v) => {
                // Digits only; empty allowed (= default). Clamp to russh's
                // 1 GiB cap (1024 MB) so the field can't exceed it.
                self.editor_form.rekey_limit_mb = if v.trim().is_empty() {
                    String::new()
                } else {
                    crate::util::sanitize_uint(&v, 1024)
                };
            }
            EditorMessage::EditorAlgoSetAuto(cat, auto) => {
                // Auto = None (russh defaults). Switching to custom seeds the
                // list with the safe defaults so the user adds legacy entries
                // (or trims) from a working set rather than from nothing.
                *self.editor_form.algo_list_mut(cat) = if auto {
                    None
                } else {
                    Some(cat.defaults())
                };
            }
            EditorMessage::EditorAlgoToggle(cat, name) => {
                let list = self.editor_form.algo_list_mut(cat).get_or_insert_with(Vec::new);
                if let Some(pos) = list.iter().position(|n| n == &name) {
                    list.remove(pos);
                } else {
                    list.push(name);
                }
            }
            // Routed here by the parent; anything else is a
            // grouping mistake, not a runtime case.
            m => return crate::dispatch::unrouted(m),
        }
        Task::none()
    }
}
