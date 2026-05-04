//! `Oryxis::handle_editor` — match arms for the connection editor:
//! field changes, save/cancel/duplicate/delete, port-forwarding edits,
//! identity selection, MCP-enabled toggle, OS detection.

#![allow(clippy::result_large_err)]

use iced::Task;

use oryxis_core::models::connection::{AuthMethod, Connection, ProxyType};
use oryxis_core::models::group::Group;

use crate::app::{Message, Oryxis};
use crate::state::{ConnectionForm, PortForwardForm};

impl Oryxis {
    pub(crate) fn handle_editor(
        &mut self,
        message: Message,
    ) -> Result<Task<Message>, Message> {
        match message {
            // -- Connection editor --
            Message::ShowNewConnection => {
                self.show_host_panel = true;
                self.editor_form = ConnectionForm::default();
                if let Some(gid) = self.active_group
                    && let Some(g) = self.groups.iter().find(|g| g.id == gid)
                {
                    self.editor_form.group_name = g.label.clone();
                }
                self.host_panel_error = None;
            }
            Message::EditConnection(idx) => {
                self.card_context_menu = None;
                self.overlay = None;
                if let Some(conn) = self.connections.get(idx) {
                    self.show_host_panel = true;
                    self.host_panel_error = None;
                    let has_pw = self.vault.as_ref()
                        .and_then(|v| v.get_connection_password(&conn.id).ok())
                        .flatten()
                        .is_some();
                    self.editor_form = ConnectionForm {
                        label: conn.label.clone(),
                        hostname: conn.hostname.clone(),
                        port: conn.port.to_string(),
                        username: conn.username.clone().unwrap_or_default(),
                        password: String::new(),
                        auth_method: conn.auth_method.clone(),
                        group_name: conn
                            .group_id
                            .and_then(|gid| {
                                self.groups.iter().find(|g| g.id == gid).map(|g| g.label.clone())
                            })
                            .unwrap_or_default(),
                        selected_key: conn.key_id.and_then(|kid| {
                            self.keys.iter().find(|k| k.id == kid).map(|k| k.label.clone())
                        }),
                        jump_host: conn.jump_chain.first().and_then(|jid| {
                            self.connections.iter().find(|c| c.id == *jid).map(|c| c.label.clone())
                        }),
                        selected_identity: conn.identity_id.and_then(|iid| {
                            self.identities.iter().find(|i| i.id == iid).map(|i| i.label.clone())
                        }),
                        editing_id: Some(conn.id),
                        has_existing_password: has_pw,
                        password_touched: false,
                        password_visible: false,
                        username_focused: false,
                        port_forwards: conn.port_forwards.iter().map(|pf| PortForwardForm {
                            local_port: pf.local_port.to_string(),
                            remote_host: pf.remote_host.clone(),
                            remote_port: pf.remote_port.to_string(),
                        }).collect(),
                        mcp_enabled: conn.mcp_enabled,
                        agent_forwarding: conn.agent_forwarding,
                        proxy_type: conn.proxy.as_ref().map(|p| match &p.proxy_type {
                            ProxyType::Socks5 => "Socks5".into(),
                            ProxyType::Socks4 => "Socks4".into(),
                            ProxyType::Http => "Http".into(),
                            ProxyType::Command(_) => "Command".into(),
                        }).unwrap_or_else(|| "(none)".into()),
                        proxy_host: conn.proxy.as_ref().map(|p| p.host.clone()).unwrap_or_default(),
                        proxy_port: conn.proxy.as_ref().map(|p| p.port.to_string()).unwrap_or_default(),
                        proxy_username: conn.proxy.as_ref().and_then(|p| p.username.clone()).unwrap_or_default(),
                        proxy_password: conn.proxy.as_ref().and_then(|p| p.password.clone()).unwrap_or_default(),
                        proxy_command: conn.proxy.as_ref().and_then(|p| match &p.proxy_type {
                            ProxyType::Command(cmd) => Some(cmd.clone()),
                            _ => None,
                        }).unwrap_or_default(),
                    };
                }
            }
            Message::EditorLabelChanged(v) => { self.editor_form.label = v; self.editor_form.username_focused = false; }
            Message::EditorHostnameChanged(v) => { self.editor_form.hostname = v; self.editor_form.username_focused = false; }
            Message::EditorPortChanged(v) => { self.editor_form.port = v; self.editor_form.username_focused = false; }
            Message::EditorUsernameChanged(v) => {
                self.editor_form.username = v;
                self.editor_form.username_focused = true;
            }
            Message::EditorPasswordChanged(v) => {
                self.editor_form.password_touched = true;
                self.editor_form.username_focused = false;
                self.editor_form.password = v;
            }
            Message::EditorTogglePasswordVisibility => {
                self.editor_form.password_visible = !self.editor_form.password_visible;
            }
            Message::EditorAuthMethodChanged(v) => {
                self.editor_form.auth_method = match v.as_str() {
                    "Password" => AuthMethod::Password,
                    "Key" => AuthMethod::Key,
                    "Agent" => AuthMethod::Agent,
                    "Interactive" => AuthMethod::Interactive,
                    _ => AuthMethod::Auto,
                };
            }
            Message::EditorGroupChanged(v) => self.editor_form.group_name = v,
            Message::EditorKeyChanged(v) => {
                self.editor_form.selected_key = if v == "(none)" { None } else { Some(v) };
            }
            Message::EditorJumpHostChanged(v) => {
                self.editor_form.jump_host = if v == "(none)" { None } else { Some(v) };
            }
            Message::EditorProxyTypeChanged(v) => { self.editor_form.proxy_type = v; }
            Message::EditorProxyHostChanged(v) => { self.editor_form.proxy_host = v; }
            Message::EditorProxyPortChanged(v) => { self.editor_form.proxy_port = v; }
            Message::EditorProxyUsernameChanged(v) => { self.editor_form.proxy_username = v; }
            Message::EditorProxyPasswordChanged(v) => { self.editor_form.proxy_password = v; }
            Message::EditorProxyCommandChanged(v) => { self.editor_form.proxy_command = v; }
            Message::EditorSave => {
                if self.editor_form.label.is_empty() || self.editor_form.hostname.is_empty() {
                    self.host_panel_error = Some("Label and hostname are required".into());
                    return Ok(Task::none());
                }
                let port: u16 = self.editor_form.port.parse().unwrap_or(22);

                // Find or create group
                let group_id = if !self.editor_form.group_name.is_empty() {
                    let existing = self
                        .groups
                        .iter()
                        .find(|g| g.label == self.editor_form.group_name);
                    match existing {
                        Some(g) => Some(g.id),
                        None => {
                            let g = Group::new(&self.editor_form.group_name);
                            let gid = g.id;
                            if let Some(vault) = &self.vault {
                                let _ = vault.save_group(&g);
                            }
                            self.groups.push(g);
                            Some(gid)
                        }
                    }
                } else {
                    None
                };

                let mut conn = if let Some(id) = self.editor_form.editing_id {
                    // Editing existing
                    self.connections
                        .iter()
                        .find(|c| c.id == id)
                        .cloned()
                        .unwrap_or_else(|| Connection::new("", ""))
                } else {
                    Connection::new("", "")
                };

                conn.label = self.editor_form.label.clone();
                conn.hostname = self.editor_form.hostname.clone();
                conn.port = port;
                conn.username = if self.editor_form.username.is_empty() {
                    None
                } else {
                    Some(self.editor_form.username.clone())
                };
                conn.auth_method = self.editor_form.auth_method.clone();
                conn.group_id = group_id;
                conn.key_id = self.editor_form.selected_key.as_ref().and_then(|label| {
                    self.keys.iter().find(|k| k.label == *label).map(|k| k.id)
                });
                conn.identity_id = self.editor_form.selected_identity.as_ref().and_then(|label| {
                    self.identities.iter().find(|i| i.label == *label).map(|i| i.id)
                });
                conn.jump_chain = self.editor_form.jump_host.as_ref()
                    .and_then(|label| {
                        self.connections.iter().find(|c| c.label == *label).map(|c| vec![c.id])
                    })
                    .unwrap_or_default();
                conn.port_forwards = self.editor_form.port_forwards.iter().filter_map(|pf| {
                    let local_port = pf.local_port.parse::<u16>().ok()?;
                    let remote_port = pf.remote_port.parse::<u16>().ok()?;
                    if pf.remote_host.is_empty() { return None; }
                    Some(oryxis_core::models::connection::PortForward {
                        local_port,
                        remote_host: pf.remote_host.clone(),
                        remote_port,
                    })
                }).collect();
                conn.mcp_enabled = self.editor_form.mcp_enabled;
                conn.agent_forwarding = self.editor_form.agent_forwarding;
                // Proxy mapping from editor form into the saved Connection
                if self.editor_form.proxy_type == "(none)" || self.editor_form.proxy_type.is_empty() {
                    conn.proxy = None;
                } else {
                    let proxy_port = self.editor_form.proxy_port.parse::<u16>().unwrap_or(0);
                    let proxy_type = match self.editor_form.proxy_type.as_str() {
                        "Socks5" => ProxyType::Socks5,
                        "Socks4" => ProxyType::Socks4,
                        "Http" => ProxyType::Http,
                        "Command" => ProxyType::Command(self.editor_form.proxy_command.clone()),
                        _ => ProxyType::Socks5,
                    };
                    conn.proxy = Some(oryxis_core::models::connection::ProxyConfig {
                        proxy_type,
                        host: self.editor_form.proxy_host.clone(),
                        port: proxy_port,
                        username: if self.editor_form.proxy_username.is_empty() { None } else { Some(self.editor_form.proxy_username.clone()) },
                        password: if self.editor_form.proxy_password.is_empty() { None } else { Some(self.editor_form.proxy_password.clone()) },
                    });
                }
                conn.updated_at = chrono::Utc::now();

                let password = if !self.editor_form.password_touched {
                    None // User didn't touch the field — preserve existing password
                } else if self.editor_form.password.is_empty() {
                    Some("") // User cleared the password — remove it
                } else {
                    Some(self.editor_form.password.as_str())
                };

                if let Some(vault) = &self.vault {
                    match vault.save_connection(&conn, password) {
                        Ok(()) => {
                            self.show_host_panel = false;
                            self.host_panel_error = None;
                            self.load_data_from_vault();
                        }
                        Err(e) => {
                            self.host_panel_error = Some(e.to_string());
                        }
                    }
                }
            }
            Message::EditorCancel => {
                self.show_host_panel = false;
                self.host_panel_error = None;
            }
            Message::DeleteConnection(idx) => {
                self.card_context_menu = None;
                self.overlay = None;
                if let Some(conn) = self.connections.get(idx) {
                    let id = conn.id;
                    if let Some(vault) = &self.vault {
                        let _ = vault.delete_connection(&id);
                        self.show_host_panel = false;
                        self.load_data_from_vault();
                    }
                }
            }
            Message::DuplicateConnection(idx) => {
                self.card_context_menu = None;
                self.overlay = None;
                if let Some(conn) = self.connections.get(idx).cloned() {
                    let mut dup = Connection::new(
                        format!("{} (copy)", conn.label),
                        &conn.hostname,
                    );
                    dup.port = conn.port;
                    dup.username = conn.username.clone();
                    dup.auth_method = conn.auth_method.clone();
                    dup.key_id = conn.key_id;
                    dup.group_id = conn.group_id;
                    dup.jump_chain = conn.jump_chain.clone();
                    dup.port_forwards = conn.port_forwards.clone();
                    dup.proxy = conn.proxy.clone();
                    dup.tags = conn.tags.clone();
                    dup.notes = conn.notes.clone();
                    dup.color = conn.color.clone();
                    dup.agent_forwarding = conn.agent_forwarding;
                    if let Some(vault) = &self.vault {
                        // Copy password too
                        let pw = vault.get_connection_password(&conn.id).ok().flatten();
                        let _ = vault.save_connection(&dup, pw.as_deref());
                        self.load_data_from_vault();
                    }
                }
            }
            // ── Connection identity ──
            Message::EditorIdentityChanged(v) => {
                self.editor_form.username_focused = false;
                if v == "(none)" {
                    self.editor_form.selected_identity = None;
                } else {
                    self.editor_form.selected_identity = Some(v);
                }
            }

            m => return Err(m),
        }
        Ok(Task::none())
    }
}
