//! `Oryxis::handle_known_hosts`: settings-panel-independent dispatch arms for the
//! known_hosts area, split out of dispatch.rs. Returns `Err(message)` for anything
//! it doesn't claim so the try_handler! chain falls through.
#![allow(clippy::result_large_err)]

use iced::Task;

use crate::app::{Message, Oryxis};

impl Oryxis {
    pub(crate) fn handle_known_hosts(
        &mut self,
        message: Message,
    ) -> Result<Task<Message>, Message> {
        match message {
            // -- Known hosts --
            Message::RequestDeleteKnownHost(idx) => {
                let label = self
                    .known_hosts
                    .get(idx)
                    .map(|kh| format!("{}:{}", kh.hostname, kh.port))
                    .unwrap_or_default();
                self.error_dialog = Some(crate::state::ErrorDialog {
                    title: crate::i18n::t("known_host_remove_confirm_title").to_string(),
                    body: format!(
                        "{label}: {}",
                        crate::i18n::t("known_host_remove_confirm_body")
                    ),
                    link: None,
                    action: Some(crate::state::ErrorDialogAction {
                        label: crate::i18n::t("remove").to_string(),
                        message: Box::new(Message::DeleteKnownHost(idx)),
                        danger: true,
                    }),
                });
            }
            Message::DeleteKnownHost(idx) => {
                if let Some(kh) = self.known_hosts.get(idx) {
                    let id = kh.id;
                    if let Some(vault) = &self.vault {
                        let _ = vault.delete_known_host(&id);
                        self.load_data_from_vault();
                    }
                }
            }
            Message::RequestClearAllKnownHosts => {
                self.error_dialog = Some(crate::state::ErrorDialog {
                    title: crate::i18n::t("known_hosts_clear_confirm_title").to_string(),
                    body: crate::i18n::t("known_hosts_clear_confirm_body").to_string(),
                    link: None,
                    action: Some(crate::state::ErrorDialogAction {
                        label: crate::i18n::t("re_verify_all").to_string(),
                        message: Box::new(Message::ClearAllKnownHosts),
                        danger: true,
                    }),
                });
            }
            Message::ClearAllKnownHosts => {
                if let Some(vault) = &self.vault {
                    for kh in self.known_hosts.clone() {
                        let _ = vault.delete_known_host(&kh.id);
                    }
                    self.load_data_from_vault();
                }
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }
}
