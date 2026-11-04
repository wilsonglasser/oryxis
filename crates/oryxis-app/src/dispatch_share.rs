//! `Oryxis::handle_share`, match arms for the export/import dialogs
//! and the share dialog (vault export with optional keys, file pick,
//! password gating).

#![allow(clippy::result_large_err)]

use std::sync::Arc;

use iced::futures::SinkExt;
use iced::Task;
use oryxis_ssh::SshEngine;

use crate::app::{SshMessage, ShareMessage, Message, Oryxis, Toast};

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
    ProxyCommand(oryxis_ssh::ProxyCommandQuery),
    /// A security key waiting on a person, said in a toast.
    SecurityKey(oryxis_ssh::SecurityKeyNotice),
    Done(Result<BackupOutcome, String>),
    NoCommonAlgo {
        category: oryxis_ssh::NegCategory,
        server_offers: Vec<String>,
    },
}

impl Oryxis {
    /// Whether the vault can actually decrypt right now: absent
    /// (pre-creation) and soft-locked (`vault.lock()` zeroizes the
    /// key, the store stays `Some`) both answer false. Every
    /// export / import / share confirm gates on this rather than
    /// `self.vaults.cur().store.is_some()`: a locked store still answers `list_*`
    /// calls while every per-field decrypt inside `export_vault`
    /// degrades to `None`, so an export run against it would quietly
    /// write a file with every password missing. Reachable in
    /// practice: `SharePathChosen` arrives from the native save
    /// dialog's blocking task, and browsing that dialog feeds iced no
    /// input events, so the idle auto-lock can fire mid-pick.
    fn vault_usable(&self) -> bool {
        self.vaults.cur().store.as_ref().is_some_and(|v| !v.is_locked())
    }

    /// The shared "can't act on the vault right now" status line for
    /// the guards above; also traced so a declined confirm is never
    /// invisible in the debug log (issue #151's complaint).
    fn vault_locked_status(context: &str) -> Result<String, String> {
        tracing::warn!("{context}: declined, vault absent or locked");
        Err(crate::i18n::t("vault_locked_error").to_string())
    }

    pub(crate) fn handle_share(
        &mut self,
        message: ShareMessage,
    ) -> Task<Message> {
        match message {
            // ── Export / Import ──
            ShareMessage::ExportVault => {
                self.vaults.cur_mut().panels.export_dialog = true;
                self.vaults.cur_mut().export_password = String::new();
                self.vaults.cur_mut().export_include_keys = true;
                self.vaults.cur_mut().export_selection = oryxis_vault::ExportSelection::all();
                self.vaults.cur_mut().export_status = None;
            }
            ShareMessage::ExportPasswordChanged(v) => {
                self.vaults.cur_mut().export_password = v.into_inner();
            }
            ShareMessage::ExportToggleKeys => {
                self.vaults.cur_mut().export_include_keys = !self.vaults.cur_mut().export_include_keys;
            }
            ShareMessage::ExportToggleCategory(cat) => {
                self.vaults.cur_mut().export_selection.toggle(cat);
            }
            ShareMessage::ExportConfirm => {
                if self.vaults.cur().export_password.is_empty() {
                    self.vaults.cur_mut().export_status = Some(Err("Password is required".into()));
                    return Task::none();
                }
                if !self.vault_usable() {
                    self.vaults.cur_mut().export_status = Some(Self::vault_locked_status("vault export"));
                    return Task::none();
                }
                if let Some(vault) = &self.vaults.cur().store {
                    let options = oryxis_vault::ExportOptions {
                        include_private_keys: self.vaults.cur().export_include_keys,
                        filter: oryxis_vault::ExportFilter::All,
                        selection: self.vaults.cur().export_selection,
                    };
                    match oryxis_vault::export_vault(vault, &self.vaults.cur().export_password, options) {
                        Ok(data) => {
                            // The native save dialog blocks its thread for as
                            // long as the user browses; run it (and the write)
                            // off the event loop so the UI keeps painting.
                            return Task::perform(
                                tokio::task::spawn_blocking(move || {
                                    let path = rfd::FileDialog::new()
                                        .set_title(crate::i18n::t("export_vault"))
                                        .add_filter("Oryxis Export", &["oryxis"])
                                        .set_file_name("vault.oryxis")
                                        .save_file()?;
                                    Some(write_export_file(&path, &data))
                                }),
                                |res| match res {
                                    Ok(Some(status)) => Message::Share(ShareMessage::ExportCompleted(status)),
                                    // Dialog cancelled or task panicked: leave
                                    // the status untouched.
                                    _ => Message::NoOp,
                                },
                            );
                        }
                        Err(e) => {
                            self.vaults.cur_mut().export_status = Some(Err(e.to_string()));
                        }
                    }
                }
            }
            ShareMessage::ExportCompleted(result) => {
                self.vaults.cur_mut().export_status = Some(result);
            }
            ShareMessage::ExportHostsCsv => {
                // Rendered here, on live state: only plaintext fields
                // go in (`importers::csv::render` cannot receive
                // secrets), so no vault access and no password step.
                // Folder, identity and key go out as LABELS: the
                // importer reads them back the same way.
                let ov = self.vaults.cur();
                let rows: Vec<crate::importers::csv::ExportRow<'_>> = ov
                    .connections
                    .iter()
                    .map(|c| crate::importers::csv::ExportRow {
                        conn: c,
                        group: c
                            .group_id
                            .map(|gid| oryxis_core::models::Group::path_of(&ov.groups, gid))
                            .unwrap_or_default(),
                        identity: c
                            .identity_id
                            .and_then(|id| ov.identities.iter().find(|i| i.id == id))
                            .map(|i| i.label.clone())
                            .unwrap_or_default(),
                        key: c
                            .key_id
                            .and_then(|id| ov.keys.iter().find(|k| k.id == id))
                            .map(|k| k.label.clone())
                            .unwrap_or_default(),
                    })
                    .collect();
                let data = crate::importers::csv::render(&rows);
                return Task::perform(
                    tokio::task::spawn_blocking(move || {
                        // Same off-the-event-loop rule as the vault
                        // export: the native dialog blocks its thread.
                        let path = rfd::FileDialog::new()
                            .set_title(crate::i18n::t("export_hosts_csv"))
                            .add_filter("CSV", &["csv"])
                            .set_file_name("oryxis-hosts.csv")
                            .save_file()?;
                        Some(
                            write_export_file(&path, data.as_bytes())
                                .map(|_| path.display().to_string()),
                        )
                    }),
                    |res| match res {
                        Ok(Some(status)) => {
                            Message::Share(ShareMessage::ExportHostsCsvCompleted(status))
                        }
                        // Dialog cancelled or task panicked: nothing to say.
                        _ => Message::NoOp,
                    },
                );
            }
            ShareMessage::ExportHostsCsvCompleted(result) => match result {
                Ok(path) => {
                    let msg = crate::i18n::t("csv_export_done")
                        .replace("{count}", &self.vaults.cur().connections.len().to_string())
                        .replace("{path}", &path);
                    self.set_toast(msg);
                }
                Err(e) => self.set_toast(e),
            },
            ShareMessage::ImportHostsCsv => {
                self.overlay = None;
                self.vaults.cur_mut().ssh_config_import_status = None;
                // Started from Settings > Security: no folder is open
                // there, whatever `active_group` still remembers. A row
                // that names a folder still goes there.
                self.vaults.cur_mut().import_target_group = None;
                return Task::perform(
                    tokio::task::spawn_blocking(|| {
                        let path = rfd::FileDialog::new()
                            .set_title(crate::i18n::t("import_hosts_csv"))
                            .add_filter("CSV", &["csv", "tsv", "txt"])
                            .pick_file()?;
                        // The hub's ceiling: a host table is kilobytes.
                        if let Ok(meta) = std::fs::metadata(&path)
                            && meta.len() > crate::importers::detect::MAX_FILE_BYTES
                        {
                            return Some(Err(crate::i18n::t("import_hub_file_too_large").to_string()));
                        }
                        Some(std::fs::read(&path).map_err(|e| format!("Read failed: {e}")))
                    }),
                    |res| match res {
                        Ok(Some(loaded)) => Message::Share(ShareMessage::ImportHostsCsvLoaded(loaded)),
                        _ => Message::NoOp,
                    },
                );
            }
            ShareMessage::ImportHostsCsvLoaded(loaded) => {
                // Same reader and the same preview as the hub's CSV
                // path, only without the format guess: this door was
                // asked for a table. A spreadsheet saved as UTF-16 (an
                // Excel "Unicode text") decodes the way the hub reads it.
                let parsed = loaded.and_then(|bytes| {
                    let text = crate::importers::regfile::decode_reg_bytes(&bytes);
                    crate::importers::csv::parse(&text)
                        .ok_or_else(|| crate::i18n::t("import_csv_unrecognized").to_string())
                });
                return match parsed {
                    Ok(import) => self.open_direct_preview(import),
                    Err(e) => {
                        self.vaults.cur_mut().ssh_config_import_status = Some(Err(e.clone()));
                        self.show_toast(e)
                    }
                };
            }
            ShareMessage::ImportSshConfig => {
                self.overlay = None;
                self.vaults.cur_mut().ssh_config_import_status = None;
                // Started from Settings > Security: no folder is open
                // there, whatever `active_group` still remembers.
                self.vaults.cur_mut().import_target_group = None;
                return Task::perform(
                    tokio::task::spawn_blocking(|| {
                        let mut dialog = rfd::FileDialog::new()
                            .set_title(crate::i18n::t("import_ssh_config_title"))
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
                        Ok(Some(text)) => Message::Share(ShareMessage::SshConfigFileLoaded(text)),
                        _ => Message::NoOp,
                    },
                );
            }
            ShareMessage::ShowImportHub => {
                self.overlay = None;
                self.vaults.cur_mut().import_hub_error = None;
                self.vaults.cur_mut().import_hub_pending = None;
                self.vaults.cur_mut().import_hub_password = String::new();
                // The hub only opens from the dashboard (its add menu
                // and empty state) and from the onboarding, where no
                // folder exists yet, so `active_group` IS the folder
                // the user is looking at. Only a manual folder can
                // take hosts (the same guard the host editor's group
                // prefill applies): a dynamic group's contents come
                // from its query.
                self.vaults.cur_mut().import_target_group = self.cur_nav().active_group.filter(|gid| {
                    self.vaults.cur().groups
                        .iter()
                        .any(|g| g.id == *gid && g.cloud_query.is_none())
                });
                self.vaults.cur_mut().panels.import_hub = true;
            }
            ShareMessage::ImportHubDismiss => {
                self.vaults.cur_mut().panels.import_hub = false;
                self.vaults.cur_mut().import_hub_pending = None;
                self.vaults.cur_mut().import_hub_password = String::new();
                self.vaults.cur_mut().import_target_group = None;
            }
            ShareMessage::ImportHubPasswordChanged(v) => {
                self.vaults.cur_mut().import_hub_password = v.into_inner();
            }
            ShareMessage::ImportHubUnlock => {
                let Some(bytes) = self.vaults.cur().import_hub_pending.clone() else {
                    return Task::none();
                };
                return self.import_hub_try_mremoteng(
                    bytes,
                    Some(self.vaults.cur().import_hub_password.clone()),
                );
            }
            ShareMessage::ImportHubPick => {
                return Task::perform(
                    tokio::task::spawn_blocking(|| {
                        let path = rfd::FileDialog::new()
                            .set_title(crate::i18n::t("import_hub_title"))
                            .pick_file()?;
                        let stem = path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("session")
                            .to_string();
                        // Same ceiling the folder scan applies per file:
                        // a config export is kilobytes, so reject a
                        // multi-GB pick before it is read fully into
                        // memory and parsed on the UI thread.
                        if let Ok(meta) = std::fs::metadata(&path)
                            && meta.len() > crate::importers::detect::MAX_FILE_BYTES
                        {
                            return Some(Err(crate::i18n::t("import_hub_file_too_large")
                                .to_string()));
                        }
                        Some(
                            std::fs::read(&path)
                                .map(|bytes| (bytes, stem))
                                .map_err(|e| format!("Read failed: {e}")),
                        )
                    }),
                    |res| match res {
                        Ok(Some(loaded)) => {
                            Message::Share(ShareMessage::ImportHubLoaded(loaded))
                        }
                        _ => Message::NoOp,
                    },
                );
            }
            ShareMessage::ImportHubPickFolder => {
                return Task::perform(
                    tokio::task::spawn_blocking(|| {
                        let dir = rfd::FileDialog::new()
                            .set_title(crate::i18n::t("import_sessions_folder_title"))
                            .pick_folder()?;
                        Some(crate::importers::detect::scan_folder(&dir))
                    }),
                    |res| match res {
                        Ok(Some(import)) => Message::Share(
                            ShareMessage::ImportHubFolderScanned(Box::new(import)),
                        ),
                        _ => Message::NoOp,
                    },
                );
            }
            ShareMessage::ImportHubFolderScanned(import) => {
                let import = *import;
                if import.hosts.is_empty() {
                    self.vaults.cur_mut().import_hub_error =
                        Some(crate::i18n::t("import_hub_folder_empty").to_string());
                    return Task::none();
                }
                self.vaults.cur_mut().panels.import_hub = false;
                return self.open_direct_preview(import);
            }
            ShareMessage::ImportHubMrngParsed(parsed, had_password) => {
                return self.handle_import_hub_mrng_parsed(*parsed, had_password);
            }
            ShareMessage::ImportHubLoaded(Err(e)) => {
                self.vaults.cur_mut().import_hub_error = Some(e);
            }
            ShareMessage::ImportHubLoaded(Ok((bytes, stem))) => {
                use crate::importers::detect::Detected;
                match crate::importers::detect::detect(&bytes, &stem) {
                    Detected::OryxisExport => {
                        // Hand off to the vault-import dialog with the
                        // same field resets its own picker path does.
                        self.vaults.cur_mut().panels.import_hub = false;
                        self.vaults.cur_mut().vault_import.status = None;
                        self.vaults.cur_mut().vault_import.password = String::new();
                        self.vaults.cur_mut().vault_import.summary = None;
                        self.vaults.cur_mut().vault_import.selection =
                            oryxis_vault::ExportSelection::all();
                        self.vaults.cur_mut().vault_import.file_data = Some(bytes);
                        self.vaults.cur_mut().panels.import_dialog = true;
                        // The vault-import dialog renders inline in
                        // Settings > Security ONLY, while the hub opens
                        // from the dashboard and the onboarding
                        // follow-up: without navigating there the flag
                        // is invisible and the pick silently does
                        // nothing (issue #151). RevealSetting rides the
                        // palette path: view + section switch plus the
                        // scroll that brings the dialog into view (the
                        // export/import card sits far down the section).
                        return self.update(Message::Settings(
                            crate::app::SettingsMessage::RevealSetting(
                                crate::state::SettingsSection::Security,
                                "import_vault",
                            ),
                        ));
                    }
                    Detected::SshConfig(text) => {
                        // The ssh_config flow keeps its alias-linking
                        // pass; enter it exactly as its own loader does.
                        self.vaults.cur_mut().panels.import_hub = false;
                        return self.update(Message::Share(
                            ShareMessage::SshConfigFileLoaded(Ok(text)),
                        ));
                    }
                    Detected::Foreign(parsed) => {
                        if parsed.hosts.is_empty() {
                            // Recognized but nothing importable: say so
                            // inside the hub (named skips beat a shrug).
                            self.vaults.cur_mut().import_hub_error =
                                Some(if parsed.skipped.is_empty() {
                                    crate::i18n::t("ssh_import_none_found").to_string()
                                } else {
                                    format!(
                                        "{} {}",
                                        crate::i18n::t("import_skipped"),
                                        parsed.skipped.join(", ")
                                    )
                                });
                            return Task::none();
                        }
                        self.vaults.cur_mut().panels.import_hub = false;
                        return self.open_direct_preview(parsed);
                    }
                    Detected::MRemoteNg => {
                        return self.import_hub_try_mremoteng(bytes, None);
                    }
                    Detected::Unknown => {
                        self.vaults.cur_mut().import_hub_error =
                            Some(crate::i18n::t("import_hub_unrecognized").to_string());
                    }
                }
            }
            ShareMessage::SshConfigFileLoaded(Err(e)) => {
                self.vaults.cur_mut().ssh_config_import_status = Some(Err(e));
            }
            ShareMessage::SshConfigFileLoaded(Ok(text)) => {
                let parsed = crate::ssh_config::parse(&text);
                if parsed.is_empty() {
                    let msg = crate::i18n::t("ssh_import_none_found").to_string();
                    self.vaults.cur_mut().ssh_config_import_status = Some(Err(msg.clone()));
                    return self.show_toast(msg);
                }
                // Flag aliases that already exist as a connection label so
                // the preview can surface them and default them to
                // unticked, re-importing the same config shouldn't pile
                // up duplicates. Lossy de-dup, exact label match.
                let existing_labels: std::collections::HashSet<String> = self
                    .vaults.cur().connections
                    .iter()
                    .map(|c| c.label.clone())
                    .collect();
                let kinds: Vec<crate::importers::ImportRowKind> = parsed
                    .iter()
                    .map(|h| {
                        if existing_labels.contains(&h.alias) {
                            crate::importers::ImportRowKind::LabelExists
                        } else {
                            crate::importers::ImportRowKind::New
                        }
                    })
                    .collect();
                // New hosts start ticked; known ones start unticked.
                self.vaults.cur_mut().ssh_import_selected =
                    kinds.iter().map(|k| k.starts_ticked()).collect();
                self.vaults.cur_mut().ssh_import_kinds = kinds;
                self.vaults.cur_mut().ssh_import_new_folders.clear();
                self.vaults.cur_mut().ssh_import_hosts = parsed;
                self.vaults.cur_mut().ssh_import_direct = None;
                self.vaults.cur_mut().ssh_config_import_status = None;
                self.vaults.cur_mut().panels.ssh_import_dialog = true;
            }
            ShareMessage::SshImportToggle(i) => {
                if let Some(sel) = self.vaults.cur_mut().ssh_import_selected.get_mut(i) {
                    *sel = !*sel;
                }
                self.refresh_import_new_folders();
            }
            ShareMessage::SshImportSelectAll(on) => {
                self.vaults.cur_mut().ssh_import_selected.fill(on);
                self.refresh_import_new_folders();
            }
            ShareMessage::SshImportDismiss => self.close_ssh_import(),
            ShareMessage::SshImportConfirm if self.direct_import_removes_any() => {
                // A removal the file asks for goes through the same
                // confirm every other removal does (Cancel the default
                // row), naming what goes; the preview stays open under
                // it, so Cancel lands back on the ticks.
                return self.confirm_direct_import_removals();
            }
            confirm @ (ShareMessage::SshImportConfirm | ShareMessage::SshImportConfirmed) => {
                if !self.vault_usable() {
                    self.vaults.cur_mut().panels.ssh_import_dialog = false;
                    self.vaults.cur_mut().ssh_config_import_status =
                        Some(Self::vault_locked_status("ssh config import"));
                    return Task::none();
                }
                if self.vaults.cur().store.is_none() {
                    return Task::none();
                }
                // Where the batch lands (issue #230): the folder that
                // was open when the hub was opened, consumed here so it
                // cannot outlive this import. Stamped just before the
                // save, and the stamped copy is what joins the in-memory
                // list, so the dashboard shows the host where the vault
                // has it. Only a row that names no folder of its own
                // takes it: `~/.ssh/config` names none, mRemoteNG's
                // folders ride `notes`, and a CSV row with a `group`
                // cell goes where the cell says.
                //
                // A confirmed removal arrives after a dialog: a batch
                // dismissed meanwhile is not there to save.
                if matches!(confirm, ShareMessage::SshImportConfirmed)
                    && self.vaults.cur().ssh_import_direct.is_none()
                {
                    return Task::none();
                }
                let target_group = self.take_import_target_group();
                // Third-party batch (PuTTY, ..., a CSV): planned rows,
                // saved in one transaction (`dispatch_direct_import`).
                if self.vaults.cur().ssh_import_direct.is_some() {
                    return self.save_direct_import(target_group);
                }
                let ov = self.vaults.cur_mut();
                let Some(vault) = &ov.store else {
                    return Task::none();
                };
                // Ticked hosts, in original order so `link_proxy_jumps`
                // can resolve sibling aliases to freshly-assigned ids.
                let picked: Vec<crate::ssh_config::SshConfigHost> = ov
                    .ssh_import_hosts
                    .iter()
                    .zip(ov.ssh_import_selected.iter())
                    .filter(|(_, sel)| **sel)
                    .map(|(h, _)| h.clone())
                    .collect();
                let total = picked.len();
                let mut to_save: Vec<oryxis_core::models::connection::Connection> =
                    picked.iter().map(crate::ssh_config::to_connection).collect();
                crate::ssh_config::link_proxy_jumps(&picked, &mut to_save);
                for conn in &mut to_save {
                    if conn.group_id.is_none() {
                        conn.group_id = target_group;
                    }
                }
                // One transaction for the batch, and patch the in-memory
                // list with the rows that saved instead of re-reading the
                // whole vault.
                let _ = vault.begin_batch();
                let mut saved: Vec<oryxis_core::models::connection::Connection> =
                    Vec::new();
                let mut errors: Vec<String> = Vec::new();
                for (host, conn) in picked.iter().zip(to_save.iter()) {
                    // No password yet, `~/.ssh/config` doesn't carry
                    // credentials. The user can add one later in the host
                    // editor; for now save without it.
                    match vault.save_connection(conn, None) {
                        Ok(()) => saved.push(conn.clone()),
                        Err(e) => errors.push(format!("{}: {e}", host.alias)),
                    }
                }
                if let Err(e) = vault.commit_batch() {
                    vault.rollback_batch();
                    saved.clear();
                    errors.push(format!("commit: {e}"));
                }
                let imported = saved.len();
                self.vaults.cur_mut().connections.extend(saved);
                self.vaults.cur_mut().panels.ssh_import_dialog = false;
                self.vaults.cur_mut().ssh_import_hosts.clear();
                self.vaults.cur_mut().ssh_import_selected.clear();
                self.vaults.cur_mut().ssh_import_kinds.clear();
                let mut summary = format!(
                    "{} {} / {}",
                    crate::i18n::t("import_summary_imported"),
                    imported,
                    total,
                );
                if errors.is_empty() {
                    self.vaults.cur_mut().ssh_config_import_status = Some(Ok(summary.clone()));
                } else {
                    summary.push_str("; ");
                    summary.push_str(&errors.join("; "));
                    self.vaults.cur_mut().ssh_config_import_status = Some(Err(summary.clone()));
                }
                return self.show_toast(summary);
            }
            ShareMessage::ImportVault => {
                // Close the "+ Host ▾" add menu when reached from there.
                self.overlay = None;
                // Started from Settings > Security: no folder is open
                // there, whatever `active_group` still remembers.
                self.vaults.cur_mut().import_target_group = None;
                self.vaults.cur_mut().vault_import.status = None;
                self.vaults.cur_mut().vault_import.password = String::new();
                self.vaults.cur_mut().vault_import.file_data = None;
                self.vaults.cur_mut().vault_import.summary = None;
                self.vaults.cur_mut().vault_import.selection = oryxis_vault::ExportSelection::all();
                self.vaults.cur_mut().vault_import.as_new_vault = false;
                self.vaults.cur_mut().vault_import.new_vault_name = String::new();
                // Picker + read off the event loop; the follow-up
                // messages route back into the dialog state.
                return Task::perform(
                    tokio::task::spawn_blocking(|| {
                        let path = rfd::FileDialog::new()
                            .set_title(crate::i18n::t("import_vault"))
                            .add_filter("Oryxis Export", &["oryxis"])
                            .pick_file()?;
                        Some(match std::fs::read(&path) {
                            Ok(data) if oryxis_vault::is_valid_export(&data) => Ok(data),
                            Ok(_) => Err("Invalid export file".to_string()),
                            Err(e) => Err(format!("Read failed: {}", e)),
                        })
                    }),
                    |res| match res {
                        Ok(Some(Ok(data))) => Message::Share(ShareMessage::ImportFileLoaded(data)),
                        Ok(Some(Err(e))) => Message::Share(ShareMessage::ImportCompleted(Err(e))),
                        _ => Message::NoOp,
                    },
                );
            }
            ShareMessage::ImportAsNewVaultToggled => {
                let import = &mut self.vaults.cur_mut().vault_import;
                import.as_new_vault = !import.as_new_vault;
                import.status = None;
            }
            ShareMessage::ImportNewVaultNameChanged(name) => {
                let import = &mut self.vaults.cur_mut().vault_import;
                import.new_vault_name = name;
                import.status = None;
            }
            ShareMessage::ImportFileLoaded(data) => {
                self.vaults.cur_mut().vault_import.file_data = Some(data);
                self.vaults.cur_mut().panels.import_dialog = true;
            }
            ShareMessage::ImportPasswordChanged(v) => {
                self.vaults.cur_mut().vault_import.password = v.into_inner();
            }
            ShareMessage::ImportInspect => {
                if self.vaults.cur().vault_import.password.is_empty() {
                    self.vaults.cur_mut().vault_import.status = Some(Err(crate::i18n::t("password_required").to_string()));
                    return Task::none();
                }
                if let Some(data) = &self.vaults.cur().vault_import.file_data {
                    match oryxis_vault::inspect_export(data, &self.vaults.cur().vault_import.password) {
                        Ok(summary) => {
                            // Pre-check every category the file carries;
                            // the user unchecks to narrow.
                            self.vaults.cur_mut().vault_import.selection = summary.default_selection();
                            self.vaults.cur_mut().vault_import.summary = Some(summary);
                            self.vaults.cur_mut().vault_import.status = None;
                        }
                        Err(oryxis_vault::VaultError::InvalidPassword) => {
                            self.vaults.cur_mut().vault_import.status = Some(Err(crate::i18n::t("import_wrong_password").to_string()));
                        }
                        Err(e) => {
                            self.vaults.cur_mut().vault_import.status = Some(Err(e.to_string()));
                        }
                    }
                }
            }
            ShareMessage::ImportToggleCategory(cat) => {
                // Only categories present in the file are interactive in
                // the UI, but guard anyway, toggling an absent one is a
                // no-op since it stays empty in the payload.
                self.vaults.cur_mut().vault_import.selection.toggle(cat);
            }
            ShareMessage::ImportConfirm => {
                if self.vaults.cur().vault_import.password.is_empty() {
                    self.vaults.cur_mut().vault_import.status = Some(Err(crate::i18n::t("password_required").to_string()));
                    return Task::none();
                }
                // Confirm only acts after a successful inspection, the UI
                // hides the button until then, this guards the message path.
                if self.vaults.cur().vault_import.summary.is_none() {
                    return Task::none();
                }
                // As a vault of its own: made now, filled once its setup
                // step (the master password, or none) has made its key.
                if self.vaults.cur().vault_import.as_new_vault {
                    return self.import_as_new_vault();
                }
                if !self.vault_usable() {
                    self.vaults.cur_mut().vault_import.status = Some(Self::vault_locked_status("vault import"));
                    return Task::none();
                }
                if let (Some(vault), Some(data)) = (&self.vaults.cur().store, &self.vaults.cur().vault_import.file_data) {
                    // The folder the hub was opened in, when the file
                    // arrived through it (issue #230); `None` from the
                    // Security cards. Consumed only by an import that
                    // LANDED: a wrong password or a write error leaves the
                    // dialog up for another try, which must still land
                    // where the first one would have. Every dismiss of the
                    // dialog clears it, so a later import cannot inherit
                    // it.
                    let target_group = self.live_import_target_group();
                    match oryxis_vault::import_vault(vault, data, &self.vaults.cur().vault_import.password, &self.vaults.cur().vault_import.selection, target_group) {
                        Ok(result) => {
                            self.vaults.cur_mut().import_target_group = None;
                            let msg = import_summary(&result);
                            self.vaults.cur_mut().vault_import.status = Some(Ok(msg));
                            self.vaults.cur_mut().panels.import_dialog = false;
                            self.vaults.cur_mut().vault_import.file_data = None;
                            self.vaults.cur_mut().vault_import.summary = None;
                            self.load_data_from_vault();
                        }
                        Err(oryxis_vault::VaultError::InvalidPassword) => {
                            self.vaults.cur_mut().vault_import.status = Some(Err(crate::i18n::t("import_wrong_password").to_string()));
                        }
                        Err(e) => {
                            self.vaults.cur_mut().vault_import.status = Some(Err(e.to_string()));
                        }
                    }
                } else {
                    // Vault presence was guarded above, so the only way
                    // here is a confirm with no file loaded, which the
                    // dialog flag makes unreachable by construction. A
                    // bare return is exactly how issue #151 stayed
                    // invisible, so leave a trace.
                    tracing::warn!("vault import: confirm with no file loaded");
                }
            }
            ShareMessage::ImportCompleted(result) => {
                self.vaults.cur_mut().vault_import.status = Some(result);
                if self.vaults.cur().vault_import.status.as_ref().is_some_and(|r| r.is_ok()) {
                    self.vaults.cur_mut().panels.import_dialog = false;
                    self.vaults.cur_mut().vault_import.file_data = None;
                    self.load_data_from_vault();
                }
            }
            ShareMessage::ExportImportDismiss => {
                self.vaults.cur_mut().panels.export_dialog = false;
                self.vaults.cur_mut().panels.import_dialog = false;
                self.vaults.cur_mut().import_target_group = None;
                self.vaults.cur_mut().export_status = None;
                self.vaults.cur_mut().vault_import.status = None;
                self.vaults.cur_mut().vault_import.file_data = None;
                self.vaults.cur_mut().vault_import.summary = None;
                self.vaults.cur_mut().sftp_backup.open = false;
            }

            // ── Backup / Restore over SFTP ──
            ShareMessage::ExportToSftp => {
                if self.vaults.cur().export_password.is_empty() {
                    self.vaults.cur_mut().export_status =
                        Some(Err(crate::i18n::t("password_required").to_string()));
                    return Task::none();
                }
                self.open_sftp_backup_picker(false);
            }
            ShareMessage::ImportFromSftp => {
                // Close the "+ Host ▾" add menu when reached from there,
                // and reset the import dialog state the loaded blob feeds.
                self.overlay = None;
                // Started from Settings > Security: no folder is open
                // there, whatever `active_group` still remembers.
                self.vaults.cur_mut().import_target_group = None;
                self.vaults.cur_mut().vault_import.status = None;
                self.vaults.cur_mut().vault_import.password = String::new();
                self.vaults.cur_mut().vault_import.file_data = None;
                self.vaults.cur_mut().vault_import.summary = None;
                self.vaults.cur_mut().vault_import.selection = oryxis_vault::ExportSelection::all();
                self.open_sftp_backup_picker(true);
            }
            ShareMessage::SftpBackupHostSelected(idx) => {
                self.vaults.cur_mut().sftp_backup.host = Some(idx);
            }
            ShareMessage::SftpBackupPathChanged(v) => {
                self.vaults.cur_mut().sftp_backup.path = v;
            }
            ShareMessage::SftpBackupCancel => {
                self.vaults.cur_mut().sftp_backup.open = false;
                self.vaults.cur_mut().sftp_backup.busy = false;
                self.vaults.cur_mut().sftp_backup.status = None;
            }
            ShareMessage::SftpBackupConfirm => {
                return self.run_sftp_backup();
            }
            ShareMessage::SftpBackupExportDone(res) => {
                self.vaults.cur_mut().sftp_backup.busy = false;
                self.host_key_response_tx = None;
                match res {
                    Ok(msg) => self.vaults.cur_mut().sftp_backup.status = Some(Ok(msg)),
                    Err(e) => self.vaults.cur_mut().sftp_backup.status = Some(Err(e)),
                }
            }
            ShareMessage::SftpBackupImportDone(res) => {
                self.vaults.cur_mut().sftp_backup.busy = false;
                self.host_key_response_tx = None;
                match res {
                    Ok(data) => {
                        // The decrypt password was already entered in the
                        // picker, so open the import dialog and inspect the
                        // blob straight away (jumps to category selection;
                        // a wrong password surfaces its error there).
                        self.vaults.cur_mut().sftp_backup.open = false;
                        self.vaults.cur_mut().sftp_backup.status = None;
                        self.vaults.cur_mut().vault_import.file_data = Some(data);
                        self.vaults.cur_mut().panels.import_dialog = true;
                        return Task::done(Message::Share(ShareMessage::ImportInspect));
                    }
                    Err(e) => self.vaults.cur_mut().sftp_backup.status = Some(Err(e)),
                }
            }

            // ── Share ──
            ShareMessage::ShareConnection(idx) => {
                self.overlay = None;
                let ov = self.vaults.cur_mut();
                if let Some(conn) = ov.connections.get(idx) {
                    ov.share.group_mode = false;
                    ov.share.filter = Some(oryxis_vault::ExportFilter::Hosts(vec![conn.id]));
                    ov.share.suggested_name = Some(share_file_name(&conn.label));
                    ov.panels.share_dialog = true;
                    ov.share.password = String::new();
                    ov.share.include_keys = false;
                    ov.share.status = None;
                }
            }
            ShareMessage::ShowExportHosts(scope) => {
                self.overlay = None;
                self.vaults.cur_mut().share.group_mode = true;
                // Pre-tick the in-scope folders. Inside a folder, tick it
                // and its descendants (mirroring the old group + subgroup
                // export); at root, tick every folder plus the ungrouped
                // hosts so a no-op confirm exports everything.
                match scope {
                    Some(gid) => {
                        self.vaults.cur_mut().share.groups = self.group_with_descendants(gid);
                        self.vaults.cur_mut().share.include_ungrouped = false;
                        self.vaults.cur_mut().share.suggested_name = self
                            .vaults.cur().groups
                            .iter()
                            .find(|g| g.id == gid)
                            .map(|g| share_file_name(&g.label));
                    }
                    None => {
                        self.vaults.cur_mut().share.groups =
                            self.vaults.cur().groups.iter().map(|g| g.id).collect();
                        self.vaults.cur_mut().share.include_ungrouped = true;
                        self.vaults.cur_mut().share.suggested_name = Some(share_file_name("hosts"));
                    }
                }
                self.vaults.cur_mut().share.filter = None;
                self.vaults.cur_mut().panels.share_dialog = true;
                self.vaults.cur_mut().share.password = String::new();
                self.vaults.cur_mut().share.include_keys = false;
                self.vaults.cur_mut().share.status = None;
            }
            ShareMessage::ShareToggleGroup(gid) => {
                if !self.vaults.cur_mut().share.groups.remove(&gid) {
                    self.vaults.cur_mut().share.groups.insert(gid);
                }
            }
            ShareMessage::ShareToggleUngrouped => {
                self.vaults.cur_mut().share.include_ungrouped = !self.vaults.cur_mut().share.include_ungrouped;
            }
            ShareMessage::SharePasswordChanged(v) => {
                self.vaults.cur_mut().share.password = v.into_inner();
            }
            ShareMessage::ShareToggleKeys => {
                self.vaults.cur_mut().share.include_keys = !self.vaults.cur_mut().share.include_keys;
            }
            ShareMessage::ShareConfirm => {
                // In group mode the filter is derived from the ticked
                // folders just before export, so a mid-dialog tick is
                // always reflected.
                if self.vaults.cur().share.group_mode {
                    let ids: Vec<uuid::Uuid> = self
                        .vaults.cur().connections
                        .iter()
                        .filter(|c| match c.group_id {
                            Some(g) => self.vaults.cur().share.groups.contains(&g),
                            None => self.vaults.cur().share.include_ungrouped,
                        })
                        .map(|c| c.id)
                        .collect();
                    if ids.is_empty() {
                        self.vaults.cur_mut().share.status = Some(Err(
                            crate::i18n::t("export_nothing_selected").to_string(),
                        ));
                        return Task::none();
                    }
                    self.vaults.cur_mut().share.filter =
                        Some(oryxis_vault::ExportFilter::Hosts(ids));
                }
                if self.vaults.cur().share.password.is_empty() {
                    self.vaults.cur_mut().share.status = Some(Err("Password is required".into()));
                    return Task::none();
                }
                if !self.vault_usable() {
                    self.vaults.cur_mut().share.status = Some(Self::vault_locked_status("share export"));
                    return Task::none();
                }
                if self.vaults.cur().share.filter.is_some() {
                    // Open the save dialog FIRST (off the event loop), then
                    // encrypt on the follow-up message. Argon2 takes tens of
                    // ms and the dialog can block for as long as the user
                    // browses; picking the path first also skips the work
                    // entirely when the user cancels.
                    let default_name = self
                        .vaults.cur().share.suggested_name
                        .clone()
                        .unwrap_or_else(|| "shared.oryxis".to_string());
                    // The save dialog is titled the way the modal that
                    // opened it is: a group export reads "Export hosts",
                    // a single host reads "Share".
                    let title = if self.vaults.cur().share.group_mode {
                        crate::i18n::t("export_hosts")
                    } else {
                        crate::i18n::t("share")
                    };
                    return Task::perform(
                        tokio::task::spawn_blocking(move || {
                            rfd::FileDialog::new()
                                .set_title(title)
                                .add_filter("Oryxis Export", &["oryxis"])
                                .set_file_name(&default_name)
                                .save_file()
                        }),
                        |res| match res {
                            Ok(Some(path)) => Message::Share(ShareMessage::SharePathChosen(path)),
                            _ => Message::NoOp,
                        },
                    );
                }
            }
            ShareMessage::SharePathChosen(path) => {
                // This message arrives from the blocking save-dialog
                // task, so unlike the click-driven confirms the idle
                // auto-lock can genuinely have fired in between (the
                // native dialog feeds iced no input events): without
                // the guard a soft-locked store would still list every
                // record while decrypting nothing, and the share would
                // be written with every password silently missing.
                if !self.vault_usable() {
                    self.vaults.cur_mut().share.status = Some(Self::vault_locked_status("share export"));
                    return Task::none();
                }
                if let (Some(vault), Some(filter)) = (&self.vaults.cur().store, &self.vaults.cur().share.filter) {
                    let options = oryxis_vault::ExportOptions {
                        include_private_keys: self.vaults.cur().share.include_keys,
                        filter: filter.clone(),
                        // A host/group share carries everything in scope,
                        // settings + cross-cutting families are withheld
                        // anyway because the filter is not `All`.
                        selection: oryxis_vault::ExportSelection::all(),
                    };
                    match oryxis_vault::export_vault(vault, &self.vaults.cur().share.password, options) {
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
                                    self.vaults.cur_mut().share.status = Some(Ok(format!("Saved to {}", path.display())));
                                    self.vaults.cur_mut().panels.share_dialog = false;
                                    // Count exported hosts for the toast.
                                    // `Hosts` covers the per-host share and
                                    // the group-mode export (the only ways
                                    // the dialog opens); other variants fall
                                    // back to a generic confirmation.
                                    let n = match &self.vaults.cur().share.filter {
                                        Some(oryxis_vault::ExportFilter::Hosts(ids)) => Some(ids.len()),
                                        _ => None,
                                    };
                                    let toast = match n {
                                        Some(n) => format!(
                                            "{} {} {}",
                                            crate::i18n::t("export_done"),
                                            n,
                                            crate::i18n::t("cat_connections"),
                                        ),
                                        None => crate::i18n::t("export_done").to_string(),
                                    };
                                    return self.show_toast(toast);
                                }
                                Err(e) => {
                                    self.vaults.cur_mut().share.status = Some(Err(format!("Write failed: {}", e)));
                                }
                            }
                        }
                        Err(e) => {
                            self.vaults.cur_mut().share.status = Some(Err(e.to_string()));
                        }
                    }
                }
            }
            ShareMessage::ShareDismiss => {
                self.vaults.cur_mut().panels.share_dialog = false;
                self.vaults.cur_mut().share.filter = None;
                self.vaults.cur_mut().share.status = None;
                self.vaults.cur_mut().share.suggested_name = None;
                self.vaults.cur_mut().share.group_mode = false;
                self.vaults.cur_mut().share.groups.clear();
                self.vaults.cur_mut().share.include_ungrouped = false;
            }
        }
        Task::none()
    }
}

impl Oryxis {
    /// Show a transient toast chip. Auto-dismissal is deadline-driven
    /// (see [`Oryxis::set_toast`]); the returned `Task` is empty and kept
    /// only so the many `return Ok(self.show_toast(..))` call sites stay
    /// unchanged. Used for feedback that should be visible from any screen.
    pub(crate) fn show_toast(&mut self, msg: String) -> Task<Message> {
        self.set_toast(msg);
        Task::none()
    }

    /// Like [`show_toast`] but with an explicit dwell in whole seconds, for
    /// hints that are a sentence to read rather than a one-word confirmation.
    pub(crate) fn show_toast_secs(&mut self, msg: String, secs: u64) -> Task<Message> {
        self.set_toast_secs(msg, secs);
        Task::none()
    }

    /// [`set_toast`] with an explicit dwell, for a caller that returns a
    /// real `Task` of its own and so cannot go through [`show_toast_secs`]
    /// (whose empty `Task` would swallow it).
    pub(crate) fn set_toast_secs(&mut self, msg: String, secs: u64) {
        self.set_toast_millis(msg, secs * 1000, None);
    }

    /// Show a toast that carries one clickable action, rendered as a
    /// link inside the chip (the sz download's "Show in folder").
    pub(crate) fn set_toast_action(
        &mut self,
        msg: String,
        secs: u64,
        label: String,
        action: Message,
    ) {
        self.set_toast_millis(msg, secs * 1000, Some((label, action)));
    }

    /// Set the toast chip and stamp its auto-dismiss deadline (default
    /// dwell). Every toast goes through [`set_toast_millis`]: the
    /// `ToastTick` subscription clears the chip once the deadline passes,
    /// so no toast is ever stranded and the newest one always wins its
    /// full dwell.
    pub(crate) fn set_toast(&mut self, msg: String) {
        self.set_toast_millis(msg, 2600, None);
    }

    /// Replace the chip wholesale: the newest text owns the deadline and
    /// the action too, so nothing from a previous toast survives into
    /// this one.
    fn set_toast_millis(&mut self, msg: String, millis: u64, action: Option<(String, Message)>) {
        self.toast = Some(Toast {
            text: msg,
            deadline: std::time::Instant::now()
                .checked_add(std::time::Duration::from_millis(millis)),
            action,
        });
    }

    /// A folder id together with every folder nested beneath it. Drives
    /// the group-mode export so picking a folder also picks its subfolders
    /// (matching the old `ExportFilter::Group` reach).
    pub(crate) fn group_with_descendants(
        &self,
        root: uuid::Uuid,
    ) -> std::collections::HashSet<uuid::Uuid> {
        let mut out = std::collections::HashSet::new();
        out.insert(root);
        // Repeated passes until no new child is added; group counts are
        // small, so the quadratic walk is cheaper than building an index.
        loop {
            let mut grew = false;
            for g in &self.vaults.cur().groups {
                if let Some(parent) = g.parent_id
                    && out.contains(&parent)
                    && out.insert(g.id)
                {
                    grew = true;
                }
            }
            if !grew {
                break;
            }
        }
        out
    }

    /// Reset and open the SFTP backup-target picker. `is_import` flips it
    /// between writing the blob (export) and reading it back (import).
    /// The host defaults to the first connection and the path to a plain
    /// `vault.oryxis` so a one-host user can confirm immediately.
    /// Breadcrumb path of the folder the current import will land in
    /// (`import_target_group`), for the preview dialogs to state it.
    /// `None` at the root, and for a folder that vanished meanwhile
    /// (deleted on another device between the pick and the confirm):
    /// the vault side ignores an unknown target too, so the dialog
    /// must not promise a folder the rows will not reach.
    pub(crate) fn import_target_folder_path(&self) -> Option<String> {
        let gid = self.live_import_target_group()?;
        Some(oryxis_core::models::Group::path_of(&self.vaults.cur().groups, gid))
    }

    /// The import's target folder as it stands NOW: the snapshot taken
    /// when the hub opened, re-checked against the folders that exist at
    /// this moment. A folder deleted (or turned dynamic) in between, by
    /// sync or on this device, resolves to the top level rather than
    /// being stamped as a dangling `group_id` that would leave the
    /// imported hosts invisible on the dashboard. The dialogs' "Into
    /// folder" line and the confirm read the same answer.
    pub(crate) fn live_import_target_group(&self) -> Option<uuid::Uuid> {
        self.vaults.cur().import_target_group.filter(|gid| {
            self.vaults.cur().groups
                .iter()
                .any(|g| g.id == *gid && g.cloud_query.is_none())
        })
    }

    /// Consume the import's target folder, re-checked
    /// (`live_import_target_group`).
    fn take_import_target_group(&mut self) -> Option<uuid::Uuid> {
        let live = self.live_import_target_group();
        self.vaults.cur_mut().import_target_group = None;
        live
    }

    fn open_sftp_backup_picker(&mut self, is_import: bool) {
        self.vaults.cur_mut().sftp_backup.is_import = is_import;
        self.vaults.cur_mut().sftp_backup.open = true;
        self.vaults.cur_mut().sftp_backup.busy = false;
        self.vaults.cur_mut().sftp_backup.status = None;
        if self.vaults.cur().sftp_backup.path.trim().is_empty() {
            self.vaults.cur_mut().sftp_backup.path = "vault.oryxis".to_string();
        }
        if self.vaults.cur().sftp_backup.host.is_none() && !self.vaults.cur().connections.is_empty() {
            self.vaults.cur_mut().sftp_backup.host = Some(0);
        }
    }

    /// Validate the picker, then connect to the chosen host (reusing an
    /// open tab session when one exists, else a fresh SFTP-only connect
    /// with the shared host-key modal) and transfer the encrypted blob.
    fn run_sftp_backup(&mut self) -> Task<Message> {
        // Guard against a second confirm while a transfer is in flight.
        if self.vaults.cur().sftp_backup.busy {
            return Task::none();
        }
        let Some(mut conn) = self
            .vaults.cur().sftp_backup.host
            .and_then(|i| self.vaults.cur().connections.get(i))
            .cloned()
        else {
            self.vaults.cur_mut().sftp_backup.status =
                Some(Err(crate::i18n::t("sftp_backup_pick_host").to_string()));
            return Task::none();
        };
        // Same working copy every connect path dials: group inheritance
        // (D4) and the effective proxy, so the backup host authenticates
        // exactly like a terminal tab to it would.
        self.apply_group_inheritance(&mut conn);
        let path = self.vaults.cur().sftp_backup.path.trim().to_string();
        if path.is_empty() {
            self.vaults.cur_mut().sftp_backup.status =
                Some(Err(crate::i18n::t("sftp_backup_path_required").to_string()));
            return Task::none();
        }
        let is_import = self.vaults.cur().sftp_backup.is_import;
        // Restore needs the decrypt password up front (mirrors export, which
        // collects the encrypt password before the picker opens). The fetched
        // blob is inspected with it as soon as it lands.
        if is_import && self.vaults.cur().vault_import.password.is_empty() {
            self.vaults.cur_mut().sftp_backup.status =
                Some(Err(crate::i18n::t("password_required").to_string()));
            return Task::none();
        }
        let label = conn.label.clone();

        // For export, encrypt the blob now from the open dialog's state so
        // the async task only has to write bytes.
        let export_data: Option<Vec<u8>> = if is_import {
            None
        } else {
            if !self.vault_usable() {
                self.vaults.cur_mut().sftp_backup.status =
                    Some(Self::vault_locked_status("sftp backup export"));
                return Task::none();
            }
            let Some(vault) = &self.vaults.cur().store else {
                return Task::none();
            };
            let options = oryxis_vault::ExportOptions {
                include_private_keys: self.vaults.cur().export_include_keys,
                filter: oryxis_vault::ExportFilter::All,
                selection: self.vaults.cur().export_selection,
            };
            match oryxis_vault::export_vault(vault, &self.vaults.cur().export_password, options) {
                Ok(d) => Some(d),
                Err(e) => {
                    self.vaults.cur_mut().sftp_backup.status = Some(Err(e.to_string()));
                    return Task::none();
                }
            }
        };

        self.vaults.cur_mut().sftp_backup.busy = true;
        self.vaults.cur_mut().sftp_backup.status = None;

        // Status formatter shared by both connect paths. Captures clones so
        // `path` stays owned for the remote-path bindings below.
        let path_msg = path.clone();
        let done_ok = move |outcome: BackupOutcome| match outcome {
            BackupOutcome::Export(n) => Message::Share(ShareMessage::SftpBackupExportDone(Ok(crate::i18n::t(
                "sftp_backup_export_ok",
            )
            .replace("{host}", &label)
            .replace("{path}", &path_msg)
            .replace("{n}", &n.to_string())))),
            BackupOutcome::Import(data) => Message::Share(ShareMessage::SftpBackupImportDone(Ok(data))),
        };

        // Reuse a live session when a terminal tab already points at this
        // host, saves a second auth dance (mirrors the SFTP mount path).
        let existing = self.tabs.iter().find_map(|t| {
            let base = t.label.trim_end_matches(" (disconnected)");
            if base == conn.label {
                // SSH handles only: the export upload rides SFTP.
                t.active().session.as_ref().and_then(|s| s.ssh()).cloned()
            } else {
                None
            }
        });

        if let Some(session) = existing {
            let remote = self.vaults.cur().sftp_backup.path.trim().to_string();
            let data = export_data;
            return Task::perform(
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
                    Err(e) if is_import => Message::Share(ShareMessage::SftpBackupImportDone(Err(e))),
                    Err(e) => Message::Share(ShareMessage::SftpBackupExportDone(Err(e))),
                },
            );
        }

        // No open tab: connect a fresh SFTP-only session. Same credential
        // /resolver pipeline as the terminal connect, with the host-key
        // ask channel wired to the shared verification modal.
        let (password, private_key, certificate) = self.resolve_credentials(&conn);
        // Agent-auth pin (B3), same rule as the tab connect.
        let pinned_agent = self.pinned_agent_public(&conn);
        let resolver = self.make_jump_resolver(&mut conn);
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

        // Command-proxy approval, same bridge shape as the host key.
        // The user confirmed this backup, so the prompt may be raised.
        let (pc_ask_tx, mut pc_ask_rx) = tokio::sync::mpsc::channel::<(
            oryxis_ssh::ProxyCommandQuery,
            tokio::sync::oneshot::Sender<bool>,
        )>(1);
        let (pc_resp_tx, mut pc_resp_rx) = tokio::sync::mpsc::channel::<bool>(1);
        self.proxy_command_response_tx = Some(pc_resp_tx);

        let totp_secret = self
            .vaults.cur().store
            .as_ref()
            .and_then(|v| v.get_connection_totp_secret(&conn.id).ok().flatten());

        let remote = path;
        // Captured for the map (conn moves into the producer); the retry
        // re-runs this backup transfer.
        let backup_conn_id = conn.id;
        let stream = iced::stream::channel::<BackupConnectMsg>(
            8,
            move |mut sender: iced::futures::channel::mpsc::Sender<BackupConnectMsg>| async move {
                let (sk_notice_tx, mut sk_notice_rx) =
                    tokio::sync::mpsc::unbounded_channel::<oryxis_ssh::SecurityKeyNotice>();
                let engine = SshEngine::new()
                    .with_host_key_check(host_key_check)
                    .with_host_key_ask(hk_ask_tx)
                    .with_proxy_command_ask(pc_ask_tx)
                    .with_totp_secret(totp_secret.as_deref())
                    // The user confirmed this backup: a touch may be asked
                    // for (no PIN prompt surface here, as for SFTP).
                    .with_security_key_prompts(crate::connect_methods::security_key_prompts(Some(
                        sk_notice_tx,
                    )))
                    .with_keepalive(keepalive)
                    .with_address_family(conn.address_family)
                    .with_rekey_limit_mb(conn.rekey_limit_mb)
                    .with_pinned_agent_key(pinned_agent.as_deref())
                    .with_algorithm_overrides(
                        conn.ciphers.clone(),
                        conn.kex.clone(),
                        conn.macs.clone(),
                        conn.host_key_algorithms.clone(),
                    )
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

                let mut sk_sender = sender.clone();
                let _sk_bridge = tokio::spawn(async move {
                    while let Some(notice) = sk_notice_rx.recv().await {
                        let _ = sk_sender.send(BackupConnectMsg::SecurityKey(notice)).await;
                    }
                });

                let mut pc_sender = sender.clone();
                let _pc_bridge = tokio::spawn(async move {
                    while let Some((query, resp_tx)) = pc_ask_rx.recv().await {
                        let _ = pc_sender.send(BackupConnectMsg::ProxyCommand(query)).await;
                        let approved = pc_resp_rx.recv().await.unwrap_or(false);
                        let _ = resp_tx.send(approved);
                    }
                });

                // Transport handshake first so a "no common algorithm"
                // failure routes to the legacy fallback dialog.
                let session = match engine
                    .connect_with_resolver(
                        &conn,
                        password.as_deref(),
                        private_key
                            .as_deref()
                            .map(|pem| oryxis_ssh::KeyMaterial::new(pem, certificate.as_deref())),
                        80,
                        24,
                        resolver.as_ref(),
                    )
                    .await
                {
                    Ok((s, _rx)) => Arc::new(s),
                    Err(e) => {
                        if let Some(nf) = e.negotiation_failure() {
                            let _ = sender
                                .send(BackupConnectMsg::NoCommonAlgo {
                                    category: nf.category,
                                    server_offers: nf.server_offers,
                                })
                                .await;
                        } else {
                            let _ = sender.send(BackupConnectMsg::Done(Err(e.to_string()))).await;
                        }
                        return;
                    }
                };
                let result = async {
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
        Task::stream(stream).map(move |m| match m {
            BackupConnectMsg::HostKey(q) => Message::Ssh(SshMessage::SshHostKeyVerify(q)),
            BackupConnectMsg::SecurityKey(notice) => Message::ToastShow(
                crate::connect_methods::security_key_notice_text(notice).to_string(),
            ),
            BackupConnectMsg::ProxyCommand(q) => Message::Ssh(SshMessage::SshProxyCommandVerify(
                Box::new(q),
                crate::state::ProxyConsentMode::Ask,
            )),
            BackupConnectMsg::Done(Ok(outcome)) => done_ok(outcome),
            BackupConnectMsg::Done(Err(e)) if is_import => Message::Share(ShareMessage::SftpBackupImportDone(Err(e))),
            BackupConnectMsg::Done(Err(e)) => Message::Share(ShareMessage::SftpBackupExportDone(Err(e))),
            BackupConnectMsg::NoCommonAlgo { category, server_offers } => {
                Message::Ssh(SshMessage::SshNoCommonAlgo {
                    conn_id: backup_conn_id,
                    category,
                    server_offers,
                    retry: Box::new(Message::Share(ShareMessage::SftpBackupConfirm)),
                })
            }
        })
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

impl Oryxis {
    /// Try a confCons.xml with the given file password (`None` = the
    /// mRemoteNG default). A protected file parks its bytes in the
    /// hub, which grows a password row and retries via
    /// `ImportHubUnlock`; success sweeps the parked state and opens
    /// the shared preview.
    /// Kick the confCons.xml parse OFF the UI thread: its PBKDF2 key
    /// stretching (once per password blob) runs under a file-controlled
    /// iteration count, so a hostile file could otherwise freeze the app
    /// for the whole parse. The result comes back as
    /// `ImportHubMrngParsed`. The held bytes are stashed so an
    /// `ImportHubUnlock` retry has them without a second file read.
    fn import_hub_try_mremoteng(
        &mut self,
        bytes: Vec<u8>,
        password: Option<String>,
    ) -> Task<Message> {
        let had_password = password.is_some();
        // Keep the bytes for a possible password retry; cleared on a
        // Ready / Invalid result in the parsed handler.
        self.vaults.cur_mut().import_hub_pending = Some(bytes.clone());
        Task::perform(
            tokio::task::spawn_blocking(move || {
                crate::importers::mremoteng::parse(&bytes, password.as_deref())
            }),
            move |res| {
                let parsed = res.unwrap_or(crate::importers::mremoteng::MrngParse::Invalid);
                Message::Share(ShareMessage::ImportHubMrngParsed(
                    Box::new(parsed),
                    had_password,
                ))
            },
        )
    }

    fn handle_import_hub_mrng_parsed(
        &mut self,
        parsed: crate::importers::mremoteng::MrngParse,
        had_password: bool,
    ) -> Task<Message> {
        use crate::importers::mremoteng::MrngParse;
        match parsed {
            MrngParse::Ready(parsed) => {
                self.vaults.cur_mut().import_hub_pending = None;
                self.vaults.cur_mut().import_hub_password = String::new();
                if parsed.hosts.is_empty() {
                    self.vaults.cur_mut().import_hub_error = Some(if parsed.skipped.is_empty() {
                        crate::i18n::t("ssh_import_none_found").to_string()
                    } else {
                        format!(
                            "{} {}",
                            crate::i18n::t("import_skipped"),
                            parsed.skipped.join(", ")
                        )
                    });
                    return Task::none();
                }
                self.vaults.cur_mut().panels.import_hub = false;
                self.open_direct_preview(parsed)
            }
            MrngParse::NeedsPassword => {
                // Second miss (a wrong typed password) reads different
                // from the first (the silent default-password try). The
                // held bytes stay in `import_hub_pending` (stashed at
                // dispatch) so the unlock retry can reuse them.
                self.vaults.cur_mut().import_hub_error = if had_password {
                    Some(crate::i18n::t("import_hub_wrong_password").to_string())
                } else {
                    None
                };
                Task::none()
            }
            MrngParse::Invalid => {
                self.vaults.cur_mut().import_hub_pending = None;
                self.vaults.cur_mut().import_hub_error =
                    Some(crate::i18n::t("import_hub_unrecognized").to_string());
                Task::none()
            }
        }
    }

    /// Put a parsed foreign batch into the shared preview dialog:
    /// dedup ticks against existing labels, clear the ssh_config half
    /// (the two are mutually exclusive) and open the dialog. Empty
    /// batches turn into a toast naming the skipped sessions, so a
    /// file full of unimportable sites never opens an empty picker.
    fn open_direct_preview(
        &mut self,
        mut parsed: crate::importers::DirectImport,
    ) -> Task<Message> {
        // Every foreign format hands its host string over verbatim, and
        // several of them let the user put a whole `user@host` in it:
        // PuTTY's "Host Name (or IP address)" accepts one and does the
        // split itself at connect time, and WinSCP / SecureCRT inherit
        // the habit. Splitting HERE (not per parser) is what keeps the
        // eight of them from each needing to remember, and it runs
        // before the preview so the dialog shows what will actually be
        // saved rather than what the file said (issue #171).
        //
        // A CSV row is planned against the vault first: it may name a
        // host here to update or remove, and its planner already split
        // the host string it was given. Only a NEW host is split here,
        // so a known host's address is never rewritten by a preview.
        {
            let ov = self.vaults.cur();
            let known = crate::importers::csv_plan::Known {
                connections: &ov.connections,
                groups: &ov.groups,
                identities: &ov.identities,
                keys: &ov.keys,
            };
            parsed = crate::importers::csv_plan::plan(parsed, &known);
        }
        for host in &mut parsed.hosts {
            if host.op == crate::importers::HostOp::Add {
                crate::importers::split_host_field(&mut host.conn);
            }
        }
        if parsed.hosts.is_empty() {
            let msg = if parsed.skipped.is_empty() {
                crate::i18n::t("ssh_import_none_found").to_string()
            } else {
                format!(
                    "{} {}",
                    crate::i18n::t("import_skipped"),
                    parsed.skipped.join(", ")
                )
            };
            self.vaults.cur_mut().ssh_config_import_status = Some(Err(msg.clone()));
            return self.show_toast(msg);
        }
        let existing_labels: std::collections::HashSet<String> = self
            .vaults.cur().connections
            .iter()
            .map(|c| c.label.clone())
            .collect();
        let kinds: Vec<crate::importers::ImportRowKind> = parsed
            .hosts
            .iter()
            .map(|h| crate::importers::ImportRowKind::of(h, &existing_labels))
            .collect();
        self.vaults.cur_mut().ssh_import_selected =
            kinds.iter().map(|k| k.starts_ticked()).collect();
        self.vaults.cur_mut().ssh_import_kinds = kinds;
        self.vaults.cur_mut().ssh_import_hosts.clear();
        self.vaults.cur_mut().ssh_import_direct = Some(parsed);
        self.refresh_import_new_folders();
        self.vaults.cur_mut().ssh_config_import_status = None;
        self.vaults.cur_mut().panels.ssh_import_dialog = true;
        Task::none()
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

impl Oryxis {
    /// "Import as a new vault": make the vault (named in the dialog),
    /// park the inspected file on it and move this window there, onto
    /// its setup step; [`Self::take_pending_import`] brings the file in
    /// once that step made the vault's key.
    fn import_as_new_vault(&mut self) -> Task<Message> {
        let name = self.vaults.cur().vault_import.new_vault_name.trim().to_string();
        if name.is_empty() {
            self.vaults.cur_mut().vault_import.status = Some(Err(crate::i18n::t("vault_name_required").to_string()));
            return Task::none();
        }
        let Some(data) = self.vaults.cur().vault_import.file_data.clone() else {
            tracing::warn!("vault import: confirm with no file loaded");
            return Task::none();
        };
        let pending = crate::state::PendingImport {
            data,
            password: self.vaults.cur().vault_import.password.clone(),
            selection: self.vaults.cur().vault_import.selection,
        };
        let id = match self.make_vault(&name, None) {
            Ok(id) => id,
            Err(e) => {
                self.vaults.cur_mut().vault_import.status = Some(Err(e));
                return Task::none();
            }
        };
        // The dialog is done here; the file now belongs to the new vault.
        let import = &mut self.vaults.cur_mut().vault_import;
        *import = crate::state::VaultImportState::default();
        self.vaults.cur_mut().panels.import_dialog = false;
        self.vaults.cur_mut().import_target_group = None;
        let task = self.enter_new_vault(id);
        self.vaults.cur_mut().pending_import = Some(pending);
        task
    }

    /// Bring in the export a vault was made for, now that it is set up
    /// (both ends of the setup step call this); said in a toast.
    pub(crate) fn take_pending_import(&mut self) -> Task<Message> {
        let Some(pending) = self.vaults.cur_mut().pending_import.take() else {
            return Task::none();
        };
        let Some(store) = self.vaults.cur().store.as_ref() else {
            return Task::none();
        };
        let outcome = oryxis_vault::import_vault(store, &pending.data, &pending.password, &pending.selection, None);
        match outcome {
            Ok(result) => {
                let msg = import_summary(&result);
                self.load_data_from_vault();
                self.show_toast(msg)
            }
            Err(e) => self.show_toast(crate::i18n::t("import_into_new_vault_failed").replacen("{error}", &e.to_string(), 1)),
        }
    }
}

/// What an import brought in, as the toast and the dialog say it: the
/// non-empty families, fully translated, and the forwards whose
/// auto-start was cleared on the way in.
fn import_summary(result: &oryxis_vault::ImportResult) -> String {
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
    // Every record skipped (re-import of a file whose content already
    // exists) would render "Imported:" with an empty tail, which reads as
    // a silent failure; say what happened instead.
    let mut msg = if body.is_empty() {
        crate::i18n::t("import_nothing_new").to_string()
    } else {
        format!("{} {}", crate::i18n::t("import_done"), body)
    };
    // Auto-start was cleared on the way in (an imported rule must not
    // dial on its own at the next launch). Say so, or the forwards look
    // broken when they don't come up.
    if result.port_forward_rules_disarmed > 0 {
        msg.push_str(" \u{2022} ");
        msg.push_str(&format!(
            "{} {}",
            result.port_forward_rules_disarmed,
            crate::i18n::t("import_forwards_disarmed")
        ));
    }
    msg
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
