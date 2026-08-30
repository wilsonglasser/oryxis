//! Host-icon picker save / reset handlers split out of
//! `dispatch_tabs`. Called from `handle_tabs`.

#![allow(clippy::result_large_err)]

use iced::Task;

use crate::app::{Message, Oryxis};

impl Oryxis {
    pub(super) fn handle_icon_picker_save(&mut self) -> Task<Message> {
        if self.icon_picker.for_local_terminal {
            // Deferred save: flow the choice into the local-terminal
            // add / edit form; the modal's own Add / Save persists it.
            self.local_terminal_form.icon = self.icon_picker.icon.clone();
            self.local_terminal_form.color = self.icon_picker.color.clone();
        } else if self.icon_picker.for_session_group {
            // Deferred save: flow the choice into the session-group
            // editor form; the form's own Save persists it.
            self.editor_session_group.icon_style = self.icon_picker.icon.clone();
            self.editor_session_group.color = self.icon_picker.color.clone();
        } else if self.icon_picker.for_group_edit {
            // Deferred save: flow into the manual group editor; the
            // panel's own Save persists to the vault.
            self.group_edit.icon = self.icon_picker.icon.clone().unwrap_or_default();
            self.group_edit.color = self.icon_picker.color.clone().unwrap_or_default();
        } else if let Some(conn_id) = self.icon_picker.for_id {
            let icon = self.icon_picker.icon.clone();
            let color = self.icon_picker.color.clone();
            if let Some(conn) = self.connections.iter_mut().find(|c| c.id == conn_id) {
                conn.custom_icon = icon.clone();
                conn.custom_color = color.clone();
                // Full save so the row persists (and other fields
                // aren't accidentally overwritten).
                if let Some(vault) = &self.vault {
                    let _ = vault.save_connection(conn, None);
                }
            }
        }
        self.panels.icon_picker = false;
        self.icon_picker.for_id = None;
        self.icon_picker.for_session_group = false;
        self.icon_picker.for_group_edit = false;
        self.icon_picker.for_local_terminal = false;
        self.icon_picker.icon_search.clear();
        self.icon_color_popover = None;
        Task::none()
    }

    pub(super) fn handle_icon_picker_reset_auto(&mut self) -> Task<Message> {
        // Clears the icon/color override, letting OS detection
        // drive the icon again on the next successful connect.
        // (Terminal-theme override is edited separately in the
        // host editor and is not touched here.)
        if self.icon_picker.for_local_terminal {
            self.local_terminal_form.icon = None;
            self.local_terminal_form.color = None;
        } else if self.icon_picker.for_session_group {
            self.editor_session_group.icon_style = None;
            self.editor_session_group.color = None;
        } else if self.icon_picker.for_group_edit {
            self.group_edit.icon = String::new();
            self.group_edit.color = String::new();
        } else if let Some(conn_id) = self.icon_picker.for_id
            && let Some(conn) = self.connections.iter_mut().find(|c| c.id == conn_id) {
            conn.custom_icon = None;
            conn.custom_color = None;
            if let Some(vault) = &self.vault {
                let _ = vault.save_connection(conn, None);
            }
        }
        self.panels.icon_picker = false;
        self.icon_picker.for_id = None;
        self.icon_picker.for_session_group = false;
        self.icon_picker.for_group_edit = false;
        self.icon_picker.for_local_terminal = false;
        self.icon_color_popover = None;
        Task::none()
    }

}
