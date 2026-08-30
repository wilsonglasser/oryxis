//! The Settings search and the section it reveals.
//!
//! Matching runs against the active-language label AND the English one,
//! so an English query works in any UI language. Activating a result
//! opens the section and rings + scrolls the row through the keynav
//! handshake, which is why the reveal is three messages rather than one.

use super::*;

impl Oryxis {
    pub(super) fn handle_settings_search(
        &mut self,
        message: SettingsMessage,
    ) -> Result<Task<Message>, SettingsMessage> {
        match message {
            SettingsMessage::SettingsSearchChanged(v) => {
                self.settings_search = v;
                self.settings_active_match = 0;
                if self.settings_search.trim().is_empty() {
                    return Ok(Task::none());
                }
                let ordered = self.settings_ordered_matches(&self.settings_search);
                if ordered.is_empty() {
                    return Ok(Task::none());
                }
                // Land the cursor on the first match in the OPEN section
                // if it has one (don't yank the user's section); else
                // open the document-first matching section.
                match ordered.iter().position(|(s, _)| *s == self.settings_section) {
                    Some(idx) => self.settings_active_match = idx,
                    None => {
                        self.settings_active_match = 0;
                        self.switch_settings_section_for_search(ordered[0].0);
                    }
                }
                // Keep the active match in view as the query narrows
                // (JetBrains-style). Scrolling the content pane doesn't
                // touch the search input's caret, so this is safe on
                // every change.
                return Ok(self.schedule_settings_scroll());
            }
            SettingsMessage::SettingsSearchStep(forward) => {
                let ordered = self.settings_ordered_matches(&self.settings_search);
                if ordered.is_empty() {
                    return Ok(Task::none());
                }
                let n = ordered.len();
                self.settings_active_match = if forward {
                    (self.settings_active_match + 1) % n
                } else {
                    (self.settings_active_match + n - 1) % n
                };
                let section = ordered[self.settings_active_match].0;
                if section != self.settings_section {
                    self.switch_settings_section_for_search(section);
                }
                return Ok(self.schedule_settings_scroll());
            }
            SettingsMessage::RevealSetting(section, label_key) => {
                // Palette entry point: put the setting's label in the
                // search box and open its section, so it lands on the
                // exact same highlight + scroll path as typing the query.
                self.settings_search = crate::i18n::t(label_key).to_string();
                self.keynav.pick_open = false;
                let t1 = self.update(Message::Navigation(
                    crate::app::NavigationMessage::ChangeView(View::Settings),
                ));
                let t2 = self.update(Message::Settings(
                    SettingsMessage::ChangeSettingsSection(section),
                ));
                let t3 = self.schedule_settings_scroll();
                return Ok(Task::batch([t1, t2, t3]));
            }
            SettingsMessage::RevealSettingScroll => {
                // Scroll the top matched row (tagged with
                // SETTINGS_SCROLL_TARGET_ID by the render) into view.
                // The operation reads real layout positions during
                // `operate`, so it works for rows scrolled far off the
                // bottom (which `draw` culls) - the whole reason the
                // old fixed-height / bounds-cell estimate mis-fired.
                if !self.settings_search.trim().is_empty() {
                    return Ok(crate::widgets::scroll_into_view_task(
                        self.settings_section.scroll_id(),
                        crate::keynav::SETTINGS_SCROLL_TARGET_ID,
                        16.0,
                    ));
                }
            }
            SettingsMessage::ChangeSettingsSection(section) => {
                // Leaving the Shortcuts editor cancels any pending
                // capture; otherwise the next keystroke on the new
                // section would silently rebind the action.
                if self.settings_section == crate::state::SettingsSection::Shortcuts
                    && section != crate::state::SettingsSection::Shortcuts
                {
                    self.editing_hotkey = None;
                }
                self.settings_section = section;
                // A pick_list dropdown open on the old section unmounts
                // WITHOUT firing on_close when the section swaps, and a
                // stuck `pick_open` swallows Enter/Space/Esc/arrows
                // process-wide (live-QA bug: Enter dead in every
                // terminal after fiddling with the renderer dropdown).
                self.keynav.pick_open = false;
                // Keyboard navigation: the old section's rows are gone;
                // keep a sidebar (SubNav) selection alive through the
                // switch (keynav's own Enter path sets the flag) so
                // repeated Up/Down + Enter keep walking sections.
                let keep = self.keynav.keep_focus_through_change_view;
                self.keynav.keep_focus_through_change_view = false;
                if !keep {
                    self.keynav.focus = None;
                }
                self.keynav_clear_content();
                self.keynav.settings_row_actions.borrow_mut().clear();
                let mut tasks = vec![self.renderer_info_task()];
                // Clicking another matching section while a search is
                // active scrolls that section's first match into view.
                // Otherwise sections remember where you left them (issue
                // #120), so hopping out to check a change and back lands
                // on the same row instead of at the top.
                tasks.push(if !self.settings_search.trim().is_empty() {
                    self.schedule_settings_scroll()
                } else {
                    self.settings_restore_scroll()
                });
                return Ok(Task::batch(tasks));
            }
            SettingsMessage::SectionScrolled(offset) => {
                self.settings_scroll.insert(self.settings_section, offset);
            }
            SettingsMessage::SectionScrollTo(id, y) => {
                return Ok(iced::widget::operation::snap_to(
                    id,
                    iced::widget::operation::RelativeOffset { x: None, y: Some(y) },
                ));
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }
}
