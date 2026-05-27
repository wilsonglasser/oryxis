//! MCP (Model Context Protocol) setup helpers, command path resolution, config
//! JSON generation, installation into `~/.claude/.mcp.json`, and the info panel
//! widget that walks the user through the setup in the Security settings.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, text, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::Message;
use crate::mcp_install;
use crate::theme::OryxisColors;

/// Binary command external MCP clients (Claude Desktop / Code,
/// Cursor) should spawn. Resolves to the stable launcher path the
/// plugin install layer maintains (`~/.oryxis/bin/oryxis-mcp[.exe]`),
/// so the JSON snippet the user copies stays valid across plugin
/// updates. Falls back to the launcher path even when no plugin is
/// installed yet, the install flow gates the surface, so the user
/// shouldn't see this snippet with a missing binary.
pub(crate) fn mcp_binary_command() -> String {
    mcp_install::launcher_path()
        .map(|p| {
            if cfg!(target_os = "windows") {
                // JSON in the snippet needs `\\` to escape backslashes
                // when rendered into a `command` string. The display
                // form embeds them as-is; the JSON builder doubles
                // them.
                p.display().to_string()
            } else {
                p.display().to_string()
            }
        })
        .unwrap_or_else(|_| "oryxis-mcp".to_string())
}

/// WSL-side path for Windows users whose AI client runs inside WSL.
/// Translates the Windows launcher path (`C:\Users\<user>\.oryxis\bin\
/// oryxis-mcp.exe`) into its WSL mount equivalent
/// (`/mnt/c/Users/<user>/.oryxis/bin/oryxis-mcp.exe`). Returns an
/// empty string when `USERPROFILE` isn't available; the WSL block in
/// the info panel only renders on Windows, where it always is.
pub(crate) fn mcp_wsl_command() -> String {
    // The launcher path is computed against `dirs::home_dir`, which
    // reads `USERPROFILE` on Windows. We post-process the result into
    // the WSL form rather than going through `USERPROFILE` again so
    // both helpers stay in lockstep.
    let Ok(path) = mcp_install::launcher_path() else {
        return String::new();
    };
    let s = path.to_string_lossy();
    // Drive-letter form: `C:\Users\...` -> `/mnt/c/Users/...`.
    if let Some(rest) = s.strip_prefix("C:\\").or_else(|| s.strip_prefix("c:\\")) {
        return format!("/mnt/c/{}", rest.replace('\\', "/"));
    }
    // Any other layout (network share, non-C drive) is too unusual to
    // guess at; fall back to the bare Windows path so the user can fix
    // it by hand.
    s.into_owned()
}

/// Config file path hint for the native client per platform. The WSL
/// target has its own hint, built inline in the info panel, so this no
/// longer needs to mention WSL on Windows.
pub(crate) fn mcp_config_path() -> &'static str {
    if cfg!(target_os = "windows") {
        "%APPDATA%\\Claude\\claude_desktop_config.json  or  ~/.claude/.mcp.json"
    } else if cfg!(target_os = "macos") {
        "~/Library/Application Support/Claude/claude_desktop_config.json  or  ~/.claude/settings.json"
    } else {
        "~/.claude/settings.json"
    }
}

/// JSON entry for the `oryxis` MCP server: the `command` path plus
/// the optional `env` block carrying the auth token. Shared between
/// the copy-to-clipboard snippet and the on-disk merge so backslash
/// escaping stays consistent on Windows.
fn oryxis_mcp_entry(cmd: &str, token: &str) -> serde_json::Value {
    if token.is_empty() {
        serde_json::json!({ "command": cmd })
    } else {
        serde_json::json!({
            "command": cmd,
            "env": { "ORYXIS_MCP_TOKEN": token },
        })
    }
}

/// The JSON snippet users need to copy. When `token` is non-empty
/// the snippet includes an `env` block that passes
/// `ORYXIS_MCP_TOKEN` to the spawned MCP server; the server refuses
/// every call when the token mismatches the value stored in the
/// vault. Empty token keeps the legacy unauth path.
pub(crate) fn mcp_config_json(token: &str) -> String {
    let cmd = mcp_binary_command();
    let root = serde_json::json!({
        "mcpServers": {
            "oryxis": oryxis_mcp_entry(&cmd, token),
        }
    });
    serde_json::to_string_pretty(&root).unwrap_or_else(|_| String::from("{}"))
}

/// Same as [`mcp_config_json`] but with the binary expressed as its
/// WSL mount path (`/mnt/c/...`), for an AI client (Claude Code,
/// Cursor) running *inside* a WSL distro on a Windows host. The
/// Windows app produces this so the user doesn't have to translate the
/// `C:\...` path into `/mnt/c/...` by hand.
pub(crate) fn mcp_config_json_wsl(token: &str) -> String {
    let cmd = mcp_wsl_command();
    let root = serde_json::json!({
        "mcpServers": {
            "oryxis": oryxis_mcp_entry(&cmd, token),
        }
    });
    serde_json::to_string_pretty(&root).unwrap_or_else(|_| String::from("{}"))
}

/// Write/merge the oryxis MCP entry into `~/.claude/.mcp.json`.
/// Threads `token` through so the on-disk config always carries
/// whatever the vault setting holds.
pub(crate) fn install_mcp_config_to_file(token: &str) -> Result<String, String> {
    let home_str = if cfg!(target_os = "windows") {
        std::env::var("USERPROFILE").map_err(|_| "USERPROFILE not set")?
    } else {
        std::env::var("HOME").map_err(|_| "HOME not set")?
    };
    let home = std::path::PathBuf::from(home_str);
    let claude_dir = home.join(".claude");
    let mcp_path = claude_dir.join(".mcp.json");

    std::fs::create_dir_all(&claude_dir)
        .map_err(|e| format!("Failed to create ~/.claude/: {e}"))?;

    let mut root: serde_json::Map<String, serde_json::Value> = if mcp_path.exists() {
        let content = std::fs::read_to_string(&mcp_path)
            .map_err(|e| format!("Failed to read {}: {e}", mcp_path.display()))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse {}: {e}", mcp_path.display()))?
    } else {
        serde_json::Map::new()
    };

    let servers = root
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}));
    let servers_map = servers
        .as_object_mut()
        .ok_or("mcpServers is not an object")?;

    let cmd = mcp_binary_command();
    servers_map.insert("oryxis".to_string(), oryxis_mcp_entry(&cmd, token));

    let output = serde_json::to_string_pretty(&root)
        .map_err(|e| format!("Failed to serialize: {e}"))?;
    std::fs::write(&mcp_path, &output)
        .map_err(|e| format!("Failed to write {}: {e}", mcp_path.display()))?;

    Ok(mcp_path.display().to_string())
}

/// Write/merge the oryxis MCP entry into the WSL distro's
/// `~/.claude/.mcp.json`, for a Claude Code / Cursor instance running
/// inside WSL on a Windows host. Shells out to `wsl.exe` (default
/// distro): reads the current config, merges in Rust so the JSON stays
/// well-formed, and writes the result back through stdin so the
/// payload never has to survive shell quoting. The `command` field
/// uses the `/mnt/c/...` mount path from [`mcp_wsl_command`].
///
/// Only meaningful on Windows; returns an error elsewhere, where there
/// is no `wsl.exe` to talk to.
pub(crate) fn install_mcp_config_to_wsl(token: &str) -> Result<String, String> {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = token;
        Err("WSL install is only available on the Windows build".to_string())
    }
    #[cfg(target_os = "windows")]
    {
        use std::io::Write;
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};

        // CREATE_NO_WINDOW keeps wsl.exe from flashing a console over
        // the app.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        // Read the current config (empty when the file is absent). A
        // non-login bash keeps rc-file noise out of stdout while still
        // expanding `~` via HOME. The trailing `|| true` keeps the exit
        // code at 0 when the file doesn't exist yet (first install),
        // otherwise `cat`'s failure would look like a WSL error.
        let read = Command::new("wsl.exe")
            .args([
                "--",
                "bash",
                "-c",
                "mkdir -p ~/.claude && cat ~/.claude/.mcp.json 2>/dev/null || true",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("Could not run wsl.exe ({e}). Is WSL installed?"))?;
        if !read.status.success() {
            let err = String::from_utf8_lossy(&read.stderr);
            return Err(format!("wsl.exe failed: {}", err.trim()));
        }

        let existing = String::from_utf8_lossy(&read.stdout);
        let mut root: serde_json::Map<String, serde_json::Value> = if existing.trim().is_empty() {
            serde_json::Map::new()
        } else {
            serde_json::from_str(existing.trim())
                .map_err(|e| format!("Failed to parse WSL ~/.claude/.mcp.json: {e}"))?
        };

        let servers = root
            .entry("mcpServers")
            .or_insert_with(|| serde_json::json!({}));
        let servers_map = servers
            .as_object_mut()
            .ok_or("mcpServers is not an object")?;
        let cmd = mcp_wsl_command();
        servers_map.insert("oryxis".to_string(), oryxis_mcp_entry(&cmd, token));

        let output =
            serde_json::to_string_pretty(&root).map_err(|e| format!("Failed to serialize: {e}"))?;

        // Pipe the merged JSON back through stdin so it never has to be
        // escaped into a shell argument.
        let mut child = Command::new("wsl.exe")
            .args(["--", "bash", "-c", "mkdir -p ~/.claude && cat > ~/.claude/.mcp.json"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("Could not run wsl.exe ({e})."))?;
        child
            .stdin
            .take()
            .ok_or("failed to open wsl.exe stdin")?
            .write_all(output.as_bytes())
            .map_err(|e| format!("Failed to write to WSL: {e}"))?;
        let status = child
            .wait()
            .map_err(|e| format!("wsl.exe did not finish: {e}"))?;
        if !status.success() {
            return Err("wsl.exe could not write ~/.claude/.mcp.json".to_string());
        }

        Ok("~/.claude/.mcp.json (WSL)".to_string())
    }
}

/// Monospaced code block widget.
pub(crate) fn code_block<'a>(content: &str) -> Element<'a, Message> {
    container(
        // `selectable(true)` lets the user drag-highlight the snippet
        // and copy it with Ctrl+C, instead of being forced through the
        // Copy button.
        text(content.to_owned()).size(12).selectable(true).color(OryxisColors::t().text_primary),
    )
    .padding(12)
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(Background::Color(OryxisColors::t().bg_primary)),
        border: Border { radius: Radius::from(6.0), color: OryxisColors::t().border, width: 1.0 },
        ..Default::default()
    })
    .into()
}

/// The expandable MCP info panel shown inside the Security settings.
pub(crate) fn mcp_info_panel<'a>(
    copied: bool,
    install_status: &'a Option<Result<String, String>>,
    token: &'a str,
    token_visible: bool,
    target_wsl: bool,
) -> Element<'a, Message> {
    // `target_wsl` switches the snippet (and the Copy / Install button
    // targets, handled in dispatch) between the native client and a
    // Claude Code / Cursor running inside WSL. The toggle that flips it
    // is Windows-only, so on other platforms this stays false.
    let json_text = if target_wsl {
        mcp_config_json_wsl(token)
    } else {
        mcp_config_json(token)
    };
    let path_hint: &str = if target_wsl {
        "~/.claude/.mcp.json (WSL)"
    } else {
        mcp_config_path()
    };

    let copy_label = if copied {
        crate::i18n::t("mcp_copied")
    } else {
        crate::i18n::t("mcp_info_copy")
    };
    let copy_color = if copied { OryxisColors::t().success } else { OryxisColors::t().accent };

    let copy_btn = button(
        container(text(copy_label).size(12).color(copy_color))
            .padding(Padding { top: 6.0, right: 16.0, bottom: 6.0, left: 16.0 }),
    )
    .on_press(Message::CopyMcpConfig)
    .style(move |_, status| {
        let bg = match status {
            BtnStatus::Hovered => Color { a: 0.1, ..copy_color },
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), color: copy_color, width: 1.0 },
            ..Default::default()
        }
    });

    let (install_label, install_color) = match install_status {
        Some(Ok(_)) => (crate::i18n::t("mcp_installed"), OryxisColors::t().success),
        Some(Err(_)) => (crate::i18n::t("mcp_install_failed"), OryxisColors::t().error),
        None => (crate::i18n::t("mcp_install_claude"), OryxisColors::t().success),
    };
    let install_btn = button(
        container(text(install_label).size(12).color(install_color))
            .padding(Padding { top: 6.0, right: 16.0, bottom: 6.0, left: 16.0 }),
    )
    .on_press(Message::InstallMcpConfig)
    .style(move |_, status| {
        let bg = match status {
            BtnStatus::Hovered => Color { a: 0.1, ..install_color },
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), color: install_color, width: 1.0 },
            ..Default::default()
        }
    });

    let close_btn = button(
        container(text(crate::i18n::t("mcp_info_close")).size(12).color(OryxisColors::t().text_muted))
            .padding(Padding { top: 6.0, right: 16.0, bottom: 6.0, left: 16.0 }),
    )
    .on_press(Message::HideMcpInfo)
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().bg_hover,
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), color: OryxisColors::t().border, width: 1.0 },
            ..Default::default()
        }
    });

    // Token row: shows the active MCP token (masked by default), with
    // show/hide, copy, and regenerate affordances. The token also
    // ends up inside the `env` block of the JSON snippet rendered
    // below, this row is the place where the user actually sees it
    // and rotates it.
    let token_display: String = if token.is_empty() {
        crate::i18n::t("mcp_token_unset").to_string()
    } else if token_visible {
        token.to_string()
    } else {
        "\u{2022}".repeat(token.chars().count().min(48))
    };
    let token_color = if token.is_empty() {
        OryxisColors::t().warning
    } else {
        OryxisColors::t().text_primary
    };
    let toggle_label = if token_visible {
        crate::i18n::t("mcp_token_hide")
    } else {
        crate::i18n::t("mcp_token_show")
    };

    fn token_action_btn<'a>(
        label: &'a str,
        color: Color,
        msg: Message,
    ) -> Element<'a, Message> {
        button(
            container(text(label).size(11).color(color))
                .padding(Padding { top: 4.0, right: 10.0, bottom: 4.0, left: 10.0 }),
        )
        .on_press(msg)
        .style(move |_, status| {
            let bg = match status {
                BtnStatus::Hovered => Color { a: 0.12, ..color },
                _ => Color::TRANSPARENT,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(6.0), color, width: 1.0 },
                ..Default::default()
            }
        })
        .into()
    }

    let mut token_items: Vec<Element<'_, Message>> = vec![
        text(crate::i18n::t("mcp_token_label"))
            .size(11)
            .color(OryxisColors::t().text_muted)
            .into(),
        Space::new().width(8).into(),
        container(
            text(token_display)
                .size(11)
                .selectable(true)
                .font(iced::Font::MONOSPACE)
                .color(token_color),
        )
        .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_primary)),
            border: Border { radius: Radius::from(4.0), ..Default::default() },
            ..Default::default()
        })
        .into(),
    ];
    if !token.is_empty() {
        token_items.push(Space::new().width(8).into());
        token_items.push(token_action_btn(
            toggle_label,
            OryxisColors::t().text_secondary,
            Message::ToggleMcpTokenVisibility,
        ));
        token_items.push(Space::new().width(6).into());
        token_items.push(token_action_btn(
            crate::i18n::t("mcp_token_copy"),
            OryxisColors::t().accent,
            Message::CopyMcpToken,
        ));
    }
    token_items.push(Space::new().width(6).into());
    token_items.push(token_action_btn(
        crate::i18n::t("mcp_token_regenerate"),
        OryxisColors::t().warning,
        Message::RegenerateMcpToken,
    ));
    let token_row = crate::widgets::dir_row(token_items)
        .align_y(iced::Alignment::Center);

    let mut info_col = column![
        text(crate::i18n::t("mcp_info_title")).size(14).color(OryxisColors::t().text_primary),
        Space::new().height(8),
        text(crate::i18n::t("mcp_info_desc")).size(12).color(OryxisColors::t().text_secondary),
    ];

    // Target toggle (Native / WSL): only relevant on Windows, where the
    // binary is an `.exe` a WSL-resident client reaches via `/mnt/c`.
    // On other platforms there is a single target, so the toggle is
    // omitted and `target_wsl` stays false.
    #[cfg(target_os = "windows")]
    {
        fn target_btn<'a>(label: &'a str, selected: bool, msg: Message) -> Element<'a, Message> {
            let text_color = if selected {
                OryxisColors::t().bg_primary
            } else {
                OryxisColors::t().text_secondary
            };
            button(
                container(text(label).size(11).color(text_color))
                    .padding(Padding { top: 4.0, right: 14.0, bottom: 4.0, left: 14.0 }),
            )
            .on_press(msg)
            .style(move |_, status| {
                let bg = if selected {
                    OryxisColors::t().accent
                } else if matches!(status, BtnStatus::Hovered) {
                    OryxisColors::t().bg_hover
                } else {
                    Color::TRANSPARENT
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(6.0), color: OryxisColors::t().border, width: 1.0 },
                    ..Default::default()
                }
            })
            .into()
        }

        let target_row = crate::widgets::dir_row(vec![
            text(crate::i18n::t("mcp_target_label"))
                .size(11)
                .color(OryxisColors::t().text_muted)
                .into(),
            Space::new().width(8).into(),
            target_btn(crate::i18n::t("mcp_target_native"), !target_wsl, Message::SetMcpTarget(false)),
            Space::new().width(6).into(),
            target_btn(crate::i18n::t("mcp_target_wsl"), target_wsl, Message::SetMcpTarget(true)),
        ])
        .align_y(iced::Alignment::Center);

        info_col = info_col.push(Space::new().height(12)).push(target_row);
    }

    info_col = info_col
        .push(Space::new().height(12))
        .push(token_row)
        .push(Space::new().height(4))
        .push(
            text(crate::i18n::t("mcp_token_desc"))
                .size(10)
                .color(OryxisColors::t().text_muted),
        )
        .push(Space::new().height(12))
        .push(code_block(&json_text))
        .push(Space::new().height(8))
        .push(
            text(format!("{} {}", crate::i18n::t("mcp_info_path_label"), path_hint))
                .size(11)
                .color(OryxisColors::t().text_muted),
        );

    // Explain that the WSL snippet targets a client living inside the
    // distro, shown only while that target is selected.
    #[cfg(target_os = "windows")]
    if target_wsl {
        info_col = info_col
            .push(Space::new().height(8))
            .push(
                text(crate::i18n::t("mcp_info_note_wsl"))
                    .size(11)
                    .color(OryxisColors::t().warning),
            );
    }

    if let Some(Err(e)) = install_status {
        info_col = info_col
            .push(Space::new().height(4))
            .push(text(e.clone()).size(11).color(OryxisColors::t().error));
    } else if let Some(Ok(path)) = install_status {
        info_col = info_col
            .push(Space::new().height(4))
            .push(text(format!("{} {path}", crate::i18n::t("mcp_installed_to"))).size(11).color(OryxisColors::t().success));
    }

    info_col = info_col
        .push(Space::new().height(8))
        .push(text(crate::i18n::t("mcp_info_vault_password_note")).size(11).color(OryxisColors::t().text_muted))
        .push(Space::new().height(12))
        .push(crate::widgets::dir_row(vec![
            install_btn.into(),
            Space::new().width(8).into(),
            copy_btn.into(),
            Space::new().width(8).into(),
            close_btn.into(),
        ]));

    container(info_col)
        .padding(16)
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border { radius: Radius::from(8.0), color: OryxisColors::t().accent, width: 1.0 },
            ..Default::default()
        })
        .into()
}
