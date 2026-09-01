//! The terminal sidebar's Host config tab.
//!
//! Same four settings as the editor's Terminal section, applied to a
//! LIVE session instead of a saved host, which is why they are separate
//! messages rather than a reuse of the editor's.

use super::*;

impl Oryxis {
    pub(super) fn handle_editor_host_config(&mut self, message: EditorMessage) -> Task<Message> {
        match message {
            EditorMessage::HostConfigThemeChanged(name) => {
                // Empty sentinel = follow the global terminal theme (None).
                let value = if name.is_empty() { None } else { Some(name) };
                self.host_config_apply(|c| c.terminal_theme = value, true);
            }
            EditorMessage::HostConfigEncodingChanged(v) => {
                let value = if v == "UTF-8" { None } else { Some(v) };
                self.host_config_apply(|c| c.encoding = value, false);
            }
            EditorMessage::HostConfigAmbiguousWidthChanged(v) => {
                // Unlike encoding and TERM, this one does not wait for a
                // reconnect: the output funnel installs it on the pane's
                // next batch, so a redraw is enough to see it.
                self.host_config_apply(|c| c.ambiguous_width = v, false);
            }
            EditorMessage::HostConfigTerminalTypeChanged(v) => {
                let value = if v == "xterm-256color" { None } else { Some(v) };
                self.host_config_apply(|c| c.terminal_type = value, false);
            }
            EditorMessage::HostConfigAutoTitleChanged(v) => {
                use crate::i18n::t;
                let value = if v == t("host_auto_title_show") {
                    Some(true)
                } else if v == t("host_auto_title_hide") {
                    Some(false)
                } else {
                    None
                };
                self.host_config_apply(|c| c.auto_title = value, false);
            }
            // Routed here by the parent; anything else is a
            // grouping mistake, not a runtime case.
            m => return crate::dispatch::unrouted(m),
        }
        Task::none()
    }
}
