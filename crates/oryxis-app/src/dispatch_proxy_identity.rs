//! `Oryxis::handle_proxy_identity` — match arms for the Proxy
//! Identity inline form lives under `Settings → Proxies`. CRUD over
//! reusable proxy configurations referenced from connections via
//! `Connection.proxy_identity_id`.

#![allow(clippy::result_large_err)]

use iced::Task;

use oryxis_core::models::connection::ProxyType;
use oryxis_core::models::proxy_identity::ProxyIdentity;

use crate::app::{Message, Oryxis};
use crate::state::ProxyKind;

impl Oryxis {
    pub(crate) fn handle_proxy_identity(
        &mut self,
        message: Message,
    ) -> Result<Task<Message>, Message> {
        match message {
            Message::ShowProxyIdentityForm(maybe_id) => {
                self.proxy_identity_form_visible = true;
                self.proxy_identity_form_error = None;

                if let Some(id) = maybe_id
                    && let Some(pi) = self.proxy_identities.iter().find(|p| p.id == id)
                {
                    let has_pw = self
                        .vault
                        .as_ref()
                        .and_then(|v| v.get_proxy_identity_password(&id).ok())
                        .flatten()
                        .is_some();
                    self.editing_proxy_identity_id = Some(id);
                    self.proxy_identity_form_label = pi.label.clone();
                    self.proxy_identity_form_kind = match &pi.proxy_type {
                        ProxyType::Socks5 => ProxyKind::Socks5,
                        ProxyType::Socks4 => ProxyKind::Socks4,
                        ProxyType::Http => ProxyKind::Http,
                        ProxyType::Command(_) => ProxyKind::Command,
                    };
                    self.proxy_identity_form_host = pi.host.clone();
                    self.proxy_identity_form_port = if pi.port == 0 {
                        String::new()
                    } else {
                        pi.port.to_string()
                    };
                    self.proxy_identity_form_username = pi.username.clone().unwrap_or_default();
                    // Mirror the connection-password UX — never pre-fill the
                    // encrypted password, just flag that one exists so the
                    // user can leave it untouched to preserve.
                    self.proxy_identity_form_password = String::new();
                    self.proxy_identity_form_password_touched = false;
                    self.proxy_identity_form_has_existing_password = has_pw;
                } else {
                    self.editing_proxy_identity_id = None;
                    self.proxy_identity_form_label = String::new();
                    self.proxy_identity_form_kind = ProxyKind::Socks5;
                    self.proxy_identity_form_host = String::new();
                    self.proxy_identity_form_port = "1080".into();
                    self.proxy_identity_form_username = String::new();
                    self.proxy_identity_form_password = String::new();
                    self.proxy_identity_form_password_touched = false;
                    self.proxy_identity_form_has_existing_password = false;
                }
                self.proxy_identity_form_password_visible = false;
            }
            Message::HideProxyIdentityForm => {
                self.proxy_identity_form_visible = false;
                self.proxy_identity_form_error = None;
            }
            Message::ProxyIdentityFormLabelChanged(v) => {
                self.proxy_identity_form_label = v;
            }
            Message::ProxyIdentityFormKindChanged(kind) => {
                // The picker only ever feeds back the four wire types
                // (SOCKS5/SOCKS4/HTTP/Command); guarding here keeps the
                // form coherent if a future caller passes None/Identity.
                if matches!(
                    kind,
                    ProxyKind::Socks5 | ProxyKind::Socks4 | ProxyKind::Http | ProxyKind::Command
                ) {
                    self.proxy_identity_form_kind = kind;
                    if let Some(p) = kind.default_port()
                        && self.proxy_identity_form_port.is_empty()
                    {
                        self.proxy_identity_form_port = p.to_string();
                    }
                }
            }
            Message::ProxyIdentityFormHostChanged(v) => {
                self.proxy_identity_form_host = v;
            }
            Message::ProxyIdentityFormPortChanged(v) => {
                self.proxy_identity_form_port = v;
            }
            Message::ProxyIdentityFormUsernameChanged(v) => {
                self.proxy_identity_form_username = v;
            }
            Message::ProxyIdentityFormPasswordChanged(v) => {
                self.proxy_identity_form_password_touched = true;
                self.proxy_identity_form_password = v;
            }
            Message::SaveProxyIdentity => {
                let label = self.proxy_identity_form_label.trim().to_string();
                if label.is_empty() {
                    self.proxy_identity_form_error =
                        Some(crate::i18n::t("proxy_identity_err_label_required").into());
                    return Ok(Task::none());
                }

                // Build the ProxyType based on the chosen kind.
                let proxy_type = match self.proxy_identity_form_kind {
                    ProxyKind::Socks5 => ProxyType::Socks5,
                    ProxyKind::Socks4 => ProxyType::Socks4,
                    ProxyKind::Http => ProxyType::Http,
                    ProxyKind::Command => {
                        // The Command variant in a *saved* identity needs a
                        // command string; we don't expose a separate field
                        // for it here yet, so reject early instead of
                        // silently storing an empty command.
                        self.proxy_identity_form_error =
                            Some(crate::i18n::t("proxy_identity_err_command_unsupported").into());
                        return Ok(Task::none());
                    }
                    ProxyKind::None | ProxyKind::Identity(_) => {
                        // These can't be the kind of a saved identity itself.
                        self.proxy_identity_form_error =
                            Some(crate::i18n::t("proxy_identity_err_invalid_kind").into());
                        return Ok(Task::none());
                    }
                };

                let port: u16 = match self.proxy_identity_form_port.parse() {
                    Ok(p) if p > 0 => p,
                    _ => {
                        self.proxy_identity_form_error =
                            Some(crate::i18n::t("proxy_err_port_invalid").into());
                        return Ok(Task::none());
                    }
                };

                if self.proxy_identity_form_host.trim().is_empty() {
                    self.proxy_identity_form_error =
                        Some(crate::i18n::t("proxy_err_host_required").into());
                    return Ok(Task::none());
                }

                let now = chrono::Utc::now();
                let mut identity = if let Some(id) = self.editing_proxy_identity_id {
                    self.proxy_identities
                        .iter()
                        .find(|p| p.id == id)
                        .cloned()
                        .unwrap_or_else(|| ProxyIdentity::new(label.clone()))
                } else {
                    ProxyIdentity::new(label.clone())
                };
                identity.label = label;
                identity.proxy_type = proxy_type;
                identity.host = self.proxy_identity_form_host.clone();
                identity.port = port;
                identity.username = if self.proxy_identity_form_username.is_empty() {
                    None
                } else {
                    Some(self.proxy_identity_form_username.clone())
                };
                identity.updated_at = now;

                // Only forward the password to the vault when the user
                // actually edited the field — preserves the existing
                // encrypted value otherwise (mirrors `save_identity`).
                let password_arg = if self.proxy_identity_form_password_touched {
                    if self.proxy_identity_form_password.is_empty() {
                        Some("")
                    } else {
                        Some(self.proxy_identity_form_password.as_str())
                    }
                } else {
                    None
                };

                if let Some(vault) = &self.vault {
                    match vault.save_proxy_identity(&identity, password_arg) {
                        Ok(()) => {
                            self.proxy_identity_form_visible = false;
                            self.proxy_identity_form_error = None;
                            self.load_data_from_vault();
                        }
                        Err(e) => {
                            self.proxy_identity_form_error = Some(e.to_string());
                        }
                    }
                }
            }
            Message::DeleteProxyIdentity(id) => {
                if let Some(vault) = &self.vault {
                    let _ = vault.delete_proxy_identity(&id);
                    self.load_data_from_vault();
                }
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }
}
