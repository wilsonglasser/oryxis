//! MCP (Model Context Protocol) setup helpers — command path resolution, config
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

/// The JSON snippet users need to copy.
pub(crate) fn mcp_config_json() -> String {
    let cmd = mcp_binary_command();
    format!(
        "{{\n  \"mcpServers\": {{\n    \"oryxis\": {{\n      \"command\": \"{cmd}\"\n    }}\n  }}\n}}"
    )
}

/// Write/merge the oryxis MCP entry into `~/.claude/.mcp.json`.
pub(crate) fn install_mcp_config_to_file() -> Result<String, String> {
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
    servers_map.insert(
        "oryxis".to_string(),
        serde_json::json!({ "command": cmd }),
    );

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
pub(crate) fn mcp_info_panel(
    copied: bool,
    install_status: &Option<Result<String, String>>,
) -> Element<'_, Message> {
    let json_text = mcp_config_json();

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

    let mut info_col = column![
        text(crate::i18n::t("mcp_info_title")).size(14).color(OryxisColors::t().text_primary),
        Space::new().height(8),
        text(crate::i18n::t("mcp_info_desc")).size(12).color(OryxisColors::t().text_secondary),
        Space::new().height(8),
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
