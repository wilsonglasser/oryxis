//! Who the host is and how you authenticate to it.
//!
//! Label, address, credentials, TOTP, the key or identity reference, and
//! the auth method that decides which of those the form even shows.

use super::*;

impl Oryxis {
    pub(super) fn handle_editor_identity(&mut self, message: EditorMessage) -> Task<Message> {
        match message {
            EditorMessage::EditorLabelChanged(v) => { self.editor_form.label = v; self.editor_form.username_focused = false; }
            EditorMessage::EditorTagsChanged(v) => { self.editor_form.tags_text = v; }
            EditorMessage::EditorHostnameChanged(v) => { self.editor_form.hostname = v; self.editor_form.username_focused = false; }
            EditorMessage::EditorPortChanged(v) => { self.editor_form.port = v; self.editor_form.username_focused = false; }
            EditorMessage::EditorUsernameChanged(v) => {
                self.editor_form.username = v;
                self.editor_form.username_focused = true;
            }
            EditorMessage::EditorPasswordChanged(v) => {
                self.editor_form.username_focused = false;
                self.editor_form.password.set(v.into_inner());
            }
            EditorMessage::EditorTogglePasswordVisibility => {
                if self.editor_form.password_visible {
                    // Hide: drop a prefilled (untouched) stored plaintext
                    // right away; typed edits stay masked in the buffer.
                    if !self.editor_form.password.touched() {
                        self.editor_form.password.clear();
                    }
                    self.editor_form.password_visible = false;
                } else {
                    // Reveal on demand: a stored password is decrypted
                    // into the buffer only for the moment it is shown
                    // (prefill stays untouched, so an unedited field
                    // still preserves the stored value on save).
                    if !self.editor_form.password.touched()
                        && let Some(id) = self.editor_form.editing_id
                        && let Some(pw) = self.vault.as_ref()
                            .and_then(|v| v.get_connection_password(&id).ok().flatten())
                    {
                        self.editor_form.password.prefill(pw);
                    }
                    self.editor_form.password_visible = true;
                }
            }
            EditorMessage::EditorTotpChanged(v) => {
                self.editor_form.username_focused = false;
                self.editor_form.totp_secret.set(v.into_inner());
            }
            EditorMessage::EditorToggleTotpVisibility => {
                if self.editor_form.totp_visible {
                    // Hide: drop a prefilled (untouched) stored secret
                    // right away; typed edits stay masked in the buffer.
                    if !self.editor_form.totp_secret.touched() {
                        self.editor_form.totp_secret.clear();
                    }
                    self.editor_form.totp_visible = false;
                } else {
                    // Reveal on demand, same lazy decrypt as the password.
                    if !self.editor_form.totp_secret.touched()
                        && let Some(id) = self.editor_form.editing_id
                        && let Some(secret) = self.vault.as_ref()
                            .and_then(|v| v.get_connection_totp_secret(&id).ok().flatten())
                    {
                        self.editor_form.totp_secret.prefill(secret);
                    }
                    self.editor_form.totp_visible = true;
                }
            }
            EditorMessage::EditorUseTotpToggled => {
                self.editor_form.use_totp = !self.editor_form.use_totp;
            }
            EditorMessage::EditorAuthMethodChanged(v) => {
                // Localized (or English) label -> enum, shared with the
                // Settings default-auth picker.
                self.editor_form.auth_method = crate::util::auth_method_from_label(&v);
                // Certificate lists only keys that carry a cert: drop a
                // selection that is no longer offerable and rebuild the
                // combo with the filtered (or restored) option list.
                if self.editor_form.auth_method == AuthMethod::Certificate
                    && let Some(sel) = self.editor_form.selected_key.as_deref()
                    && !self
                        .keys
                        .iter()
                        .any(|k| k.label == sel && k.certificate.is_some())
                {
                    self.editor_form.selected_key = None;
                }
                self.reset_editor_key_combo();
            }
            EditorMessage::EditorGroupChanged(v) => self.editor_form.group_name = v,
            EditorMessage::EditorKeyChanged(v) => {
                self.editor_form.selected_key = if v == "(none)" { None } else { Some(v) };
            }
            EditorMessage::EditorKeyComboOpened => {
                // Focus clears the typed value so the dropdown opens on
                // the full key list, not pre-filtered by the current pick.
                self.reset_editor_key_combo();
            }
            EditorMessage::EditorIdentityChanged(v) => {
                self.editor_form.username_focused = false;
                if v == "(none)" {
                    self.editor_form.selected_identity = None;
                } else {
                    self.editor_form.selected_identity = Some(v);
                }
            }
            EditorMessage::EditorIconStyleChanged(v) => {
                // "" clears the override; anything else is normalized to
                // the known set so a stale UI value can't smuggle in a
                // string the renderer doesn't understand.
                self.editor_form.icon_style = match v.as_str() {
                    "circular" | "square" | "rounded" | "outline" | "initials" => Some(v),
                    _ => None,
                };
            }
            EditorMessage::EditorProtocolChanged(protocol) => {
                let prev = self.editor_form.protocol;
                if prev != protocol {
                    // Retarget the numeric port only when both protocols
                    // use one AND the field still holds the old default,
                    // so a user-typed port survives the switch untouched.
                    // Serial has no numeric port (`None`), so switching
                    // to/from it leaves the field alone (it's hidden).
                    if let (Some(prev_port), Some(new_port)) =
                        (prev.default_port(), protocol.default_port())
                        && self.editor_form.port.trim() == prev_port.to_string()
                    {
                        self.editor_form.port = new_port.to_string();
                    }
                    // Materialize serial defaults the first time a host
                    // becomes Serial so the reduced form has values to
                    // show (9600 8N1).
                    if protocol == oryxis_core::models::connection::ConnectionProtocol::Serial
                        && self.editor_form.serial.is_none()
                    {
                        self.editor_form.serial =
                            Some(oryxis_core::models::serial::SerialParams::default());
                    }
                    self.editor_form.protocol = protocol;
                }
                self.editor_form.username_focused = false;
            }
            EditorMessage::EditorAddressFamilyChanged(family) => {
                self.editor_form.address_family = family;
            }
            // Routed here by the parent; anything else is a
            // grouping mistake, not a runtime case.
            m => return crate::dispatch::unrouted(m),
        }
        Task::none()
    }
}
