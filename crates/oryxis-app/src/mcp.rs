//! MCP (Model Context Protocol) setup helpers, command path resolution, config
//! JSON generation, installation into `~/.claude/.mcp.json`, and the info panel
//! widget that walks the user through the setup in the Security settings.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, row, text, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::Message;
use crate::theme::OryxisColors;

/// Binary command for each platform.
pub(crate) fn mcp_binary_command() -> &'static str {
    if cfg!(target_os = "windows") {
        "C:\\\\Program Files\\\\Oryxis\\\\oryxis-mcp.exe"
    } else if cfg!(target_os = "macos") {
        "/Applications/Oryxis.app/Contents/MacOS/oryxis-mcp"
    } else {
        "/usr/local/bin/oryxis-mcp"
    }
}

/// WSL command for Windows users whose AI client runs inside WSL.
pub(crate) fn mcp_wsl_command() -> &'static str {
    "/mnt/c/Program Files/Oryxis/oryxis-mcp.exe"
}

/// Config file path hint per platform.
pub(crate) fn mcp_config_path() -> &'static str {
    if cfg!(target_os = "windows") {
        "%APPDATA%\\Claude\\claude_desktop_config.json  or  ~/.claude/settings.json (WSL)"
    } else if cfg!(target_os = "macos") {
        "~/Library/Application Support/Claude/claude_desktop_config.json  or  ~/.claude/settings.json"
    } else {
        "~/.claude/settings.json"
    }
}

/// The JSON snippet users need to copy. When `token` is non-empty
/// the snippet includes an `env` block that passes
/// `ORYXIS_MCP_TOKEN` to the spawned MCP server; the server refuses
/// every call when the token mismatches the value stored in the
/// vault. Empty token keeps the legacy unauth path.
pub(crate) fn mcp_config_json(token: &str) -> String {
    let cmd = mcp_binary_command();
    if token.is_empty() {
        format!(
            "{{\n  \"mcpServers\": {{\n    \"oryxis\": {{\n      \"command\": \"{cmd}\"\n    }}\n  }}\n}}"
        )
    } else {
        format!(
            "{{\n  \"mcpServers\": {{\n    \"oryxis\": {{\n      \"command\": \"{cmd}\",\n      \"env\": {{\n        \"ORYXIS_MCP_TOKEN\": \"{token}\"\n      }}\n    }}\n  }}\n}}"
        )
    }
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
    let entry = if token.is_empty() {
        serde_json::json!({ "command": cmd })
    } else {
        serde_json::json!({
            "command": cmd,
            "env": { "ORYXIS_MCP_TOKEN": token },
        })
    };
    servers_map.insert("oryxis".to_string(), entry);

    let output = serde_json::to_string_pretty(&root)
        .map_err(|e| format!("Failed to serialize: {e}"))?;
    std::fs::write(&mcp_path, &output)
        .map_err(|e| format!("Failed to write {}: {e}", mcp_path.display()))?;

    Ok(mcp_path.display().to_string())
}

/// Monospaced code block widget.
pub(crate) fn code_block<'a>(content: &str) -> Element<'a, Message> {
    container(
        text(content.to_owned()).size(12).color(OryxisColors::t().text_primary),
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
) -> Element<'a, Message> {
    let json_text = mcp_config_json(token);

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

    let mut token_row = row![
        text(crate::i18n::t("mcp_token_label"))
            .size(11)
            .color(OryxisColors::t().text_muted),
        Space::new().width(8),
        container(
            text(token_display)
                .size(11)
                .font(iced::Font::MONOSPACE)
                .color(token_color),
        )
        .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_primary)),
            border: Border { radius: Radius::from(4.0), ..Default::default() },
            ..Default::default()
        }),
    ]
    .align_y(iced::Alignment::Center);
    if !token.is_empty() {
        token_row = token_row.push(Space::new().width(8)).push(token_action_btn(
            toggle_label,
            OryxisColors::t().text_secondary,
            Message::ToggleMcpTokenVisibility,
        ));
        token_row = token_row.push(Space::new().width(6)).push(token_action_btn(
            crate::i18n::t("mcp_token_copy"),
            OryxisColors::t().accent,
            Message::CopyMcpToken,
        ));
    }
    token_row = token_row.push(Space::new().width(6)).push(token_action_btn(
        crate::i18n::t("mcp_token_regenerate"),
        OryxisColors::t().warning,
        Message::RegenerateMcpToken,
    ));

    let mut info_col = column![
        text(crate::i18n::t("mcp_info_title")).size(14).color(OryxisColors::t().text_primary),
        Space::new().height(8),
        text(crate::i18n::t("mcp_info_desc")).size(12).color(OryxisColors::t().text_secondary),
        Space::new().height(12),
        token_row,
        Space::new().height(4),
        text(crate::i18n::t("mcp_token_desc"))
            .size(10).color(OryxisColors::t().text_muted),
        Space::new().height(12),
        code_block(&json_text),
        Space::new().height(8),
        text(format!("{} {}", crate::i18n::t("mcp_info_path_label"), mcp_config_path()))
            .size(11).color(OryxisColors::t().text_muted),
    ];

    if cfg!(target_os = "windows") {
        let wsl_json = format!(
            "{{\n  \"mcpServers\": {{\n    \"oryxis\": {{\n      \"command\": \"{}\"\n    }}\n  }}\n}}",
            mcp_wsl_command()
        );
        info_col = info_col
            .push(Space::new().height(12))
            .push(text(crate::i18n::t("mcp_info_note_wsl")).size(12).color(OryxisColors::t().warning))
            .push(Space::new().height(4))
            .push(code_block(&wsl_json));
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
        .push(row![install_btn, Space::new().width(8), copy_btn, Space::new().width(8), close_btn]);

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
