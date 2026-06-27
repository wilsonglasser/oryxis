//! `Oryxis::handle_share`, match arms for the export/import dialogs
//! and the share dialog (vault export with optional keys, file pick,
//! password gating).

#![allow(clippy::result_large_err)]

use std::sync::Arc;

use iced::futures::SinkExt;
use iced::Task;
use oryxis_ssh::SshEngine;

use crate::app::{Message, Oryxis};

/// Result of an SFTP backup transfer once the session is up: either the
/// byte count written (export) or the validated blob read back (import).
enum BackupOutcome {
    Export(usize),
    Import(Vec<u8>),
}

/// Stream messages for a fresh-connect SFTP backup: host-key prompts are
/// forwarded to the shared verification modal, then the terminal `Done`
/// carries the transfer outcome.
enum BackupConnectMsg {
    HostKey(oryxis_ssh::HostKeyQuery),
    Done(Result<BackupOutcome, String>),
}

impl Oryxis {
    pub(crate) fn handle_share(
        &mut self,
        message: Message,
    ) -> Result<Task<Message>, Message> {
        match message {
            // ── Export / Import ──
            Message::ExportVault => {
                self.show_export_dialog = true;
                self.export_password = String::new();
                self.export_include_keys = true;
                self.export_selection = oryxis_vault::ExportSelection::all();
                self.export_status = None;
            }
            Message::ExportPasswordChanged(v) => {
                self.export_password = v;
            }
            Message::ExportToggleKeys => {
                self.export_include_keys = !self.export_include_keys;
            }
            Message::ExportToggleCategory(cat) => {
                self.export_selection.toggle(cat);
            }
            Message::ExportConfirm => {
                if self.export_password.is_empty() {
                    self.export_status = Some(Err("Password is required".into()));
                    return Ok(Task::none());
                }
                if let Some(vault) = &self.vault {
                    let options = oryxis_vault::ExportOptions {
                        include_private_keys: self.export_include_keys,
                        filter: oryxis_vault::ExportFilter::All,
                        selection: self.export_selection,
                    };
                    match oryxis_vault::export_vault(vault, &self.export_password, options) {
                        Ok(data) => {
                            // The native save dialog blocks its thread for as
                            // long as the user browses; run it (and the write)
                            // off the event loop so the UI keeps painting.
                            return Ok(Task::perform(
                                tokio::task::spawn_blocking(move || {
                                    let path = rfd::FileDialog::new()
                                        .set_title("Export Vault")
                                        .add_filter("Oryxis Export", &["oryxis"])
                                        .set_file_name("vault.oryxis")
                                        .save_file()?;
                                    Some(write_export_file(&path, &data))
                                }),
                                |res| match res {
                                    Ok(Some(status)) => Message::ExportCompleted(status),
                                    // Dialog cancelled or task panicked: leave
                                    // the status untouched.
                                    _ => Message::NoOp,
                                },
                            ));
                        }
                        Err(e) => {
                            self.export_status = Some(Err(e.to_string()));
                        }
                    }
                }
            }
            Message::ExportCompleted(result) => {
                self.export_status = Some(result);
            }
            Message::ImportSshConfig => {
                self.ssh_config_import_status = None;
                return Ok(Task::perform(
                    tokio::task::spawn_blocking(|| {
                        let mut dialog = rfd::FileDialog::new()
                            .set_title("Import SSH config")
                            .add_filter("SSH config", &["", "config"]);
                        if let Some(default) = crate::ssh_config::default_config_path()
                            && let Some(parent) = default.parent()
                        {
                            dialog = dialog.set_directory(parent);
                        }
                        let path = dialog.pick_file()?;
                        Some(
                            std::fs::read_to_string(&path)
                                .map_err(|e| format!("Read failed: {e}")),
                        )
                    }),
                    |res| match res {
                        Ok(Some(text)) => Message::SshConfigFileLoaded(text),
                        _ => Message::NoOp,
                    },
                ));
            }
            Message::SshConfigFileLoaded(Err(e)) => {
                self.ssh_config_import_status = Some(Err(e));
            }
            Message::SshConfigFileLoaded(Ok(text)) => {
                let parsed = crate::ssh_config::parse(&text);
                if parsed.is_empty() {
                    self.ssh_config_import_status =
                        Some(Err("No host blocks found in this file".into()));
                    return Ok(Task::none());
                }
                let Some(vault) = &self.vault else {
                    self.ssh_config_import_status =
                        Some(Err("Vault not unlocked".into()));
                    return Ok(Task::none());
                };
                // Skip aliases that already exist as a connection label
                //, re-importing the same config shouldn't pile up
                // duplicates. Lossy de-dup, exact label match.
                let existing_labels: std::collections::HashSet<String> = self
                    .connections
                    .iter()
                    .map(|c| c.label.clone())
                    .collect();
                let mut imported = 0usize;
                let mut skipped = 0usize;
                let mut errors: Vec<String> = Vec::new();
                // Build all connections first so `link_proxy_jumps` can
                // resolve sibling aliases to their freshly-assigned ids.
                // `parsed_to_save` and `to_save` keep matching indices.
                let mut parsed_to_save: Vec<&crate::ssh_config::SshConfigHost> = Vec::new();
                let mut to_save: Vec<oryxis_core::models::connection::Connection> = Vec::new();
                for host in &parsed {
                    if existing_labels.contains(&host.alias) {
                        skipped += 1;
                        continue;
                    }
                    parsed_to_save.push(host);
                    to_save.push(crate::ssh_config::to_connection(host));
                }
                crate::ssh_config::link_proxy_jumps(&parsed_to_save.iter().map(|p| (*p).clone()).collect::<Vec<_>>(), &mut to_save);
                // One transaction for the batch, and patch the in-memory
                // list with the rows that saved instead of re-reading
                // the whole vault.
                let _ = vault.begin_batch();
                let mut saved: Vec<oryxis_core::models::connection::Connection> = Vec::new();
                for (host, conn) in parsed_to_save.iter().zip(to_save.iter()) {
                    // No password yet, `~/.ssh/config` doesn't carry
                    // credentials. The user can add one later in the
                    // host editor; for now save without it.
                    match vault.save_connection(conn, None) {
                        Ok(()) => {
                            imported += 1;
                            saved.push(conn.clone());
                        }
                        Err(e) => errors.push(format!("{}: {e}", host.alias)),
                    }
                }
                if let Err(e) = vault.commit_batch() {
                    vault.rollback_batch();
                    saved.clear();
                    imported = 0;
                    errors.push(format!("commit: {e}"));
                }
                self.connections.extend(saved);
                let mut summary = format!(
                    "{} {} / {}",
                    crate::i18n::t("import_summary_imported"),
                    imported,
                    parsed.len(),
                );
                if skipped > 0 {
                    summary.push_str(&format!(
                        " ({} {})",
                        skipped,
                        crate::i18n::t("import_summary_skipped"),
                    ));
                }
                if errors.is_empty() {
                    self.ssh_config_import_status = Some(Ok(summary));
                } else {
                    summary.push_str("; errors: ");
                    summary.push_str(&errors.join("; "));
                    self.ssh_config_import_status = Some(Err(summary));
                }
            }
            Message::ImportVault => {
                // Close the "+ Host ▾" add menu when reached from there.
                self.overlay = None;
                self.import_status = None;
                self.import_password = String::new();
                self.import_file_data = None;
                self.import_summary = None;
                self.import_selection = oryxis_vault::ExportSelection::all();
                // Picker + read off the event loop; the follow-up
                // messages route back into the dialog state.
                return Ok(Task::perform(
                    tokio::task::spawn_blocking(|| {
                        let path = rfd::FileDialog::new()
                            .set_title("Import Vault")
                            .add_filter("Oryxis Export", &["oryxis"])
                            .pick_file()?;
                        Some(match std::fs::read(&path) {
                            Ok(data) if oryxis_vault::is_valid_export(&data) => Ok(data),
                            Ok(_) => Err("Invalid export file".to_string()),
                            Err(e) => Err(format!("Read failed: {}", e)),
                        })
                    }),
                    |res| match res {
                        Ok(Some(Ok(data))) => Message::ImportFileLoaded(data),
                        Ok(Some(Err(e))) => Message::ImportCompleted(Err(e)),
                        _ => Message::NoOp,
                    },
                ));
            }
            Message::ImportFileLoaded(data) => {
                self.import_file_data = Some(data);
                self.show_import_dialog = true;
            }
            Message::ImportPasswordChanged(v) => {
                self.import_password = v;
            }
            Message::ImportInspect => {
                if self.import_password.is_empty() {
                    self.import_status = Some(Err(crate::i18n::t("password_required").to_string()));
                    return Ok(Task::none());
                }
                if let Some(data) = &self.import_file_data {
                    match oryxis_vault::inspect_export(data, &self.import_password) {
                        Ok(summary) => {
                            // Pre-check every category the file carries;
                            // the user unchecks to narrow.
                            self.import_selection = summary.default_selection();
                            self.import_summary = Some(summary);
                            self.import_status = None;
                        }
                        Err(oryxis_vault::VaultError::InvalidPassword) => {
                            self.import_status = Some(Err(crate::i18n::t("import_wrong_password").to_string()));
                        }
                        Err(e) => {
                            self.import_status = Some(Err(e.to_string()));
                        }
                    }
                }
            }
            Message::ImportToggleCategory(cat) => {
                // Only categories present in the file are interactive in
                // the UI, but guard anyway, toggling an absent one is a
                // no-op since it stays empty in the payload.
                self.import_selection.toggle(cat);
            }
            Message::ImportConfirm => {
                if self.import_password.is_empty() {
                    self.import_status = Some(Err(crate::i18n::t("password_required").to_string()));
                    return Ok(Task::none());
                }
                // Confirm only acts after a successful inspection, the UI
                // hides the button until then, this guards the message path.
                if self.import_summary.is_none() {
                    return Ok(Task::none());
                }
                if let (Some(vault), Some(data)) = (&self.vault, &self.import_file_data) {
                    match oryxis_vault::import_vault(vault, data, &self.import_password, &self.import_selection) {
                        Ok(result) => {
                            // Fully translated summary, built from the
                            // same category labels the dialog uses. Only
                            // non-zero families are listed to keep it short.
                            let parts: Vec<(usize, &str)> = vec![
                                (result.connections_added + result.connections_updated, "cat_connections"),
                                (result.keys_added, "cat_keys"),
                                (result.groups_added, "cat_groups"),
                                (result.identities_added + result.identities_updated, "cat_identities"),
                                (result.proxy_identities_added + result.proxy_identities_updated, "cat_proxies"),
                                (result.cloud_profiles_added + result.cloud_profiles_updated, "cat_cloud_profiles"),
                                (result.snippets_added, "cat_snippets"),
                                (result.known_hosts_added, "cat_known_hosts"),
                                (result.port_forward_rules_added, "cat_port_forwards"),
                                (result.session_groups_added, "cat_session_layouts"),
                                (result.settings_imported, "cat_settings"),
                            ];
                            let body = parts
                                .iter()
                                .filter(|(n, _)| *n > 0)
                                .map(|(n, key)| format!("{n} {}", crate::i18n::t(key)))
                                .collect::<Vec<_>>()
                                .join(", ");
                            let msg = format!("{} {}", crate::i18n::t("import_done"), body);
                            self.import_status = Some(Ok(msg));
                            self.show_import_dialog = false;
                            self.import_file_data = None;
                            self.import_summary = None;
                            self.load_data_from_vault();
                        }
                        Err(oryxis_vault::VaultError::InvalidPassword) => {
                            self.import_status = Some(Err(crate::i18n::t("import_wrong_password").to_string()));
                        }
                        Err(e) => {
                            self.import_status = Some(Err(e.to_string()));
                        }
                    }
                }
            }
            Message::ImportCompleted(result) => {
                self.import_status = Some(result);
                if self.import_status.as_ref().is_some_and(|r| r.is_ok()) {
                    self.show_import_dialog = false;
                    self.import_file_data = None;
                    self.load_data_from_vault();
                }
            }
            Message::ExportImportDismiss => {
                self.show_export_dialog = false;
                self.show_import_dialog = false;
                self.export_status = None;
                self.import_status = None;
                self.import_file_data = None;
                self.import_summary = None;
                self.sftp_backup_open = false;
            }

            // ── Backup / Restore over SFTP ──
            Message::ExportToSftp => {
                if self.export_password.is_empty() {
                    self.export_status =
                        Some(Err(crate::i18n::t("password_required").to_string()));
                    return Ok(Task::none());
                }
                self.open_sftp_backup_picker(false);
            }
            Message::ImportFromSftp => {
                // Close the "+ Host ▾" add menu when reached from there,
                // and reset the import dialog state the loaded blob feeds.
                self.overlay = None;
                self.import_status = None;
                self.import_password = String::new();
                self.import_file_data = None;
                self.import_summary = None;
                self.import_selection = oryxis_vault::ExportSelection::all();
                self.open_sftp_backup_picker(true);
            }
            Message::SftpBackupHostSelected(idx) => {
                self.sftp_backup_host = Some(idx);
            }
            Message::SftpBackupPathChanged(v) => {
                self.sftp_backup_path = v;
            }
            Message::SftpBackupCancel => {
                self.sftp_backup_open = false;
                self.sftp_backup_busy = false;
                self.sftp_backup_status = None;
            }
            Message::SftpBackupConfirm => {
                return self.run_sftp_backup();
            }
            Message::SftpBackupExportDone(res) => {
                self.sftp_backup_busy = false;
                self.host_key_response_tx = None;
                match res {
                    Ok(msg) => self.sftp_backup_status = Some(Ok(msg)),
                    Err(e) => self.sftp_backup_status = Some(Err(e)),
                }
            }
            Message::SftpBackupImportDone(res) => {
                self.sftp_backup_busy = false;
                self.host_key_response_tx = None;
                match res {
                    Ok(data) => {
                        // The decrypt password was already entered in the
                        // picker, so open the import dialog and inspect the
                        // blob straight away (jumps to category selection;
                        // a wrong password surfaces its error there).
                        self.sftp_backup_open = false;
                        self.sftp_backup_status = None;
                        self.import_file_data = Some(data);
                        self.show_import_dialog = true;
                        return Ok(Task::done(Message::ImportInspect));
                    }
                    Err(e) => self.sftp_backup_status = Some(Err(e)),
                }
            }

            // ── Share ──
            Message::ShareConnection(idx) => {
                self.overlay = None;
                if let Some(conn) = self.connections.get(idx) {
                    self.share_filter = Some(oryxis_vault::ExportFilter::Hosts(vec![conn.id]));
                    self.share_suggested_name = Some(share_file_name(&conn.label));
                    self.show_share_dialog = true;
                    self.share_password = String::new();
                    self.share_include_keys = false;
                    self.share_status = None;
                }
            }
            Message::ShareGroup(group_id) => {
                self.overlay = None;
                self.share_filter = Some(oryxis_vault::ExportFilter::Group(group_id));
                self.share_suggested_name = self
                    .groups
                    .iter()
                    .find(|g| g.id == group_id)
                    .map(|g| share_file_name(&g.label));
                self.show_share_dialog = true;
                self.share_password = String::new();
                self.share_include_keys = false;
                self.share_status = None;
            }
            Message::SharePasswordChanged(v) => {
                self.share_password = v;
            }
            Message::ShareToggleKeys => {
                self.share_include_keys = !self.share_include_keys;
            }
            Message::ShareConfirm => {
                if self.share_password.is_empty() {
                    self.share_status = Some(Err("Password is required".into()));
                    return Ok(Task::none());
                }
                if self.vault.is_some() && self.share_filter.is_some() {
                    // Open the save dialog FIRST (off the event loop), then
                    // encrypt on the follow-up message. Argon2 takes tens of
                    // ms and the dialog can block for as long as the user
                    // browses; picking the path first also skips the work
                    // entirely when the user cancels.
                    let default_name = self
                        .share_suggested_name
                        .clone()
                        .unwrap_or_else(|| "shared.oryxis".to_string());
                    return Ok(Task::perform(
                        tokio::task::spawn_blocking(move || {
                            rfd::FileDialog::new()
                                .set_title("Share")
                                .add_filter("Oryxis Export", &["oryxis"])
                                .set_file_name(&default_name)
                                .save_file()
                        }),
                        |res| match res {
                            Ok(Some(path)) => Message::SharePathChosen(path),
                            _ => Message::NoOp,
                        },
                    ));
                }
            }
            Message::SharePathChosen(path) => {
                if let (Some(vault), Some(filter)) = (&self.vault, &self.share_filter) {
                    let options = oryxis_vault::ExportOptions {
                        include_private_keys: self.share_include_keys,
                        filter: filter.clone(),
                        // A host/group share carries everything in scope,
                        // settings + cross-cutting families are withheld
                        // anyway because the filter is not `All`.
                        selection: oryxis_vault::ExportSelection::all(),
                    };
                    match oryxis_vault::export_vault(vault, &self.share_password, options) {
                        Ok(data) => {
                            match std::fs::write(&path, &data) {
                                Ok(()) => {
                                    // Lock the file to 0600. Even though the
                                    // share is encrypted, defense in depth
                                    // keeps a stranger from the easy first
                                    // step of copy/exfiltrate, matching the
                                    // full-vault export path.
                                    #[cfg(unix)]
                                    {
                                        use std::os::unix::fs::PermissionsExt as _;
                                        let _ = std::fs::set_permissions(
                                            &path,
                                            std::fs::Permissions::from_mode(0o600),
                                        );
                                    }
                                    self.share_status = Some(Ok(format!("Saved to {}", path.display())));
                                    self.show_share_dialog = false;
                                }
                                Err(e) => {
                                    self.share_status = Some(Err(format!("Write failed: {}", e)));
                                }
                            }
                        }
                        Err(e) => {
                            self.share_status = Some(Err(e.to_string()));
                        }
                    }
                }
            }
            Message::ShareDismiss => {
                self.show_share_dialog = false;
                self.share_filter = None;
                self.share_status = None;
                self.share_suggested_name = None;
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }
}

impl Oryxis {
    /// Reset and open the SFTP backup-target picker. `is_import` flips it
    /// between writing the blob (export) and reading it back (import).
    /// The host defaults to the first connection and the path to a plain
    /// `vault.oryxis` so a one-host user can confirm immediately.
    fn open_sftp_backup_picker(&mut self, is_import: bool) {
        self.sftp_backup_is_import = is_import;
        self.sftp_backup_open = true;
        self.sftp_backup_busy = false;
        self.sftp_backup_status = None;
        if self.sftp_backup_path.trim().is_empty() {
            self.sftp_backup_path = "vault.oryxis".to_string();
        }
        if self.sftp_backup_host.is_none() && !self.connections.is_empty() {
            self.sftp_backup_host = Some(0);
        }
    }

    /// Validate the picker, then connect to the chosen host (reusing an
    /// open tab session when one exists, else a fresh SFTP-only connect
    /// with the shared host-key modal) and transfer the encrypted blob.
    fn run_sftp_backup(&mut self) -> Result<Task<Message>, Message> {
        // Guard against a second confirm while a transfer is in flight.
        if self.sftp_backup_busy {
            return Ok(Task::none());
        }
        let Some(conn) = self
            .sftp_backup_host
            .and_then(|i| self.connections.get(i))
            .cloned()
        else {
            self.sftp_backup_status =
                Some(Err(crate::i18n::t("sftp_backup_pick_host").to_string()));
            return Ok(Task::none());
        };
        let path = self.sftp_backup_path.trim().to_string();
        if path.is_empty() {
            self.sftp_backup_status =
                Some(Err(crate::i18n::t("sftp_backup_path_required").to_string()));
            return Ok(Task::none());
        }
        let is_import = self.sftp_backup_is_import;
        // Restore needs the decrypt password up front (mirrors export, which
        // collects the encrypt password before the picker opens). The fetched
        // blob is inspected with it as soon as it lands.
        if is_import && self.import_password.is_empty() {
            self.sftp_backup_status =
                Some(Err(crate::i18n::t("password_required").to_string()));
            return Ok(Task::none());
        }
        let label = conn.label.clone();

        // For export, encrypt the blob now from the open dialog's state so
        // the async task only has to write bytes.
        let export_data: Option<Vec<u8>> = if is_import {
            None
        } else {
            let Some(vault) = &self.vault else {
                return Ok(Task::none());
            };
            let options = oryxis_vault::ExportOptions {
                include_private_keys: self.export_include_keys,
                filter: oryxis_vault::ExportFilter::All,
                selection: self.export_selection,
            };
            match oryxis_vault::export_vault(vault, &self.export_password, options) {
                Ok(d) => Some(d),
                Err(e) => {
                    self.sftp_backup_status = Some(Err(e.to_string()));
                    return Ok(Task::none());
                }
            }
        };

        self.sftp_backup_busy = true;
        self.sftp_backup_status = None;

        // Status formatter shared by both connect paths. Captures clones so
        // `path` stays owned for the remote-path bindings below.
        let path_msg = path.clone();
        let done_ok = move |outcome: BackupOutcome| match outcome {
            BackupOutcome::Export(n) => Message::SftpBackupExportDone(Ok(crate::i18n::t(
                "sftp_backup_export_ok",
            )
            .replace("{host}", &label)
            .replace("{path}", &path_msg)
            .replace("{n}", &n.to_string()))),
            BackupOutcome::Import(data) => Message::SftpBackupImportDone(Ok(data)),
        };

        // Reuse a live session when a terminal tab already points at this
        // host, saves a second auth dance (mirrors the SFTP mount path).
        let existing = self.tabs.iter().find_map(|t| {
            let base = t.label.trim_end_matches(" (disconnected)");
            if base == conn.label {
                t.active().ssh_session.clone()
            } else {
                None
            }
        });

        if let Some(session) = existing {
            let remote = self.sftp_backup_path.trim().to_string();
            let data = export_data;
            return Ok(Task::perform(
                async move {
                    let client = session.open_sftp().await.map_err(|e| e.to_string())?;
                    if is_import {
                        let bytes = client.read_file(&remote).await.map_err(|e| e.to_string())?;
                        if !oryxis_vault::is_valid_export(&bytes) {
                            return Err(crate::i18n::t("sftp_backup_not_export").to_string());
                        }
                        Ok(BackupOutcome::Import(bytes))
                    } else {
                        let blob = data.expect("export bytes prepared above");
                        client
                            .write_file(&remote, &blob)
                            .await
                            .map_err(|e| e.to_string())?;
                        Ok(BackupOutcome::Export(blob.len()))
                    }
                },
                move |res: Result<BackupOutcome, String>| match res {
                    Ok(outcome) => done_ok(outcome),
                    Err(e) if is_import => Message::SftpBackupImportDone(Err(e)),
                    Err(e) => Message::SftpBackupExportDone(Err(e)),
                },
            ));
        }

        // No open tab: connect a fresh SFTP-only session. Same credential
        // /resolver pipeline as the terminal connect, with the host-key
        // ask channel wired to the shared verification modal.
        let (password, private_key) = self.resolve_credentials(&conn);
        let resolver = self.make_jump_resolver(&conn);
        let host_key_check = self.make_host_key_check();
        let keepalive = self.effective_keepalive(&conn);
        let connect_to = self.sftp_connect_timeout();
        let auth_to = self.sftp_auth_timeout();
        let session_to = self.sftp_session_timeout();

        let (hk_ask_tx, mut hk_ask_rx) = tokio::sync::mpsc::channel::<(
            oryxis_ssh::HostKeyQuery,
            tokio::sync::oneshot::Sender<bool>,
        )>(1);
        let (hk_resp_tx, mut hk_resp_rx) = tokio::sync::mpsc::channel::<bool>(1);
        self.host_key_response_tx = Some(hk_resp_tx);

        let remote = path;
        let stream = iced::stream::channel::<BackupConnectMsg>(
            8,
            move |mut sender: iced::futures::channel::mpsc::Sender<BackupConnectMsg>| async move {
                let engine = SshEngine::new()
                    .with_host_key_check(host_key_check)
                    .with_host_key_ask(hk_ask_tx)
                    .with_keepalive(keepalive)
                    .with_connect_timeout(connect_to)
                    .with_auth_timeout(auth_to)
                    .with_session_timeout(session_to);

                let mut sender_clone = sender.clone();
                let _bridge = tokio::spawn(async move {
                    while let Some((query, resp_tx)) = hk_ask_rx.recv().await {
                        let _ = sender_clone.send(BackupConnectMsg::HostKey(query)).await;
                        let accepted = hk_resp_rx.recv().await.unwrap_or(false);
                        let _ = resp_tx.send(accepted);
                    }
                });

                let result = async {
                    let (session, _rx) = engine
                        .connect_with_resolver(
                            &conn,
                            password.as_deref(),
                            private_key.as_deref(),
                            80,
                            24,
                            resolver.as_ref(),
                        )
                        .await
                        .map_err(|e| e.to_string())?;
                    let session = Arc::new(session);
                    let client = session.open_sftp().await.map_err(|e| e.to_string())?;
                    if is_import {
                        let bytes =
                            client.read_file(&remote).await.map_err(|e| e.to_string())?;
                        if !oryxis_vault::is_valid_export(&bytes) {
                            return Err(crate::i18n::t("sftp_backup_not_export").to_string());
                        }
                        Ok(BackupOutcome::Import(bytes))
                    } else {
                        let blob = export_data.expect("export bytes prepared above");
                        client
                            .write_file(&remote, &blob)
                            .await
                            .map_err(|e| e.to_string())?;
                        Ok(BackupOutcome::Export(blob.len()))
                    }
                }
                .await;
                let _ = sender.send(BackupConnectMsg::Done(result)).await;
            },
        );
        Ok(Task::stream(stream).map(move |m| match m {
            BackupConnectMsg::HostKey(q) => Message::SshHostKeyVerify(q),
            BackupConnectMsg::Done(Ok(outcome)) => done_ok(outcome),
            BackupConnectMsg::Done(Err(e)) if is_import => Message::SftpBackupImportDone(Err(e)),
            BackupConnectMsg::Done(Err(e)) => Message::SftpBackupExportDone(Err(e)),
        }))
    }
}

/// Write an export payload to the chosen path, tightening permissions
/// to 0600 on Unix (the export is encrypted, but defense in depth
/// keeps a stranger from the easy first step of copy/exfiltrate).
/// Returns the status line for the dialog.
fn write_export_file(path: &std::path::Path, data: &[u8]) -> Result<String, String> {
    match std::fs::write(path, data) {
        Ok(()) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
            }
            Ok(format!("Exported to {}", path.display()))
        }
        Err(e) => Err(format!("Write failed: {}", e)),
    }
}

/// Build a filesystem-safe `*.oryxis` default file name from a connection
/// or group label. Strips path separators, control characters and other
/// reserved bytes so the suggestion can't escape the picked directory or
/// produce an unusable name. Falls back to `shared.oryxis` when nothing
/// printable survives.
fn share_file_name(label: &str) -> String {
    let cleaned: String = label
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    if trimmed.is_empty() {
        "shared.oryxis".to_string()
    } else {
        format!("{trimmed}.oryxis")
    }
}

#[cfg(test)]
mod tests {
    use super::share_file_name;

    #[test]
    fn share_file_name_uses_label() {
        assert_eq!(share_file_name("my-server"), "my-server.oryxis");
        assert_eq!(share_file_name("Prod DB"), "Prod DB.oryxis");
    }

    #[test]
    fn share_file_name_strips_path_and_reserved_chars() {
        // No separator survives, so the suggestion can't escape the
        // directory the user picks in the save dialog. A leftover ".."
        // with no separator is just a harmless filename component.
        let name = share_file_name("../../etc/passwd");
        assert!(!name.contains('/'));
        assert!(!name.contains('\\'));
        assert_eq!(share_file_name("a:b*c?"), "a_b_c_.oryxis");
    }

    #[test]
    fn share_file_name_falls_back_when_empty() {
        assert_eq!(share_file_name(""), "shared.oryxis");
        assert_eq!(share_file_name("   "), "shared.oryxis");
        assert_eq!(share_file_name("..."), "shared.oryxis");
    }
}
