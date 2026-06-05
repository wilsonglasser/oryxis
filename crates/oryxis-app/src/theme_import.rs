//! Import popular terminal color schemes into a `CustomTerminalTheme`.
//!
//! Three formats, auto-detected from the pasted content:
//! - **Windows Terminal** JSON scheme object (`background`, `foreground`,
//!   `black`..`brightWhite`).
//! - **base16** YAML (`base00`..`base0F`), mapped to the 16 ANSI slots by
//!   the standard base16 shell-template convention.
//! - **iTerm2** `.itermcolors` (XML plist with float 0..1 components).
//!
//! Each parser is pure (`&str -> Result<CustomTerminalTheme, String>`) so it
//! can be unit-tested without a vault or UI.

use oryxis_core::models::custom_terminal_theme::CustomTerminalTheme;

/// Detect the format from the content and parse it. `name` is the
/// user-provided theme name.
pub(crate) fn parse_theme(content: &str, name: &str) -> Result<CustomTerminalTheme, String> {
    let trimmed = content.trim_start();
    if trimmed.starts_with('{') {
        parse_windows_terminal(content, name)
    } else if trimmed.starts_with("<?xml") || trimmed.contains("<plist") {
        parse_iterm(content, name)
    } else if content.contains("base00") {
        parse_base16(content, name)
    } else {
        Err("Unrecognized format (expected Windows Terminal JSON, iTerm \
             .itermcolors, or base16 YAML).".to_string())
    }
}

fn build(
    name: &str,
    fg: String,
    bg: String,
    cursor: String,
    ansi: [String; 16],
) -> CustomTerminalTheme {
    let mut t = CustomTerminalTheme::new_default(name.to_string());
    t.foreground = fg;
    t.background = bg;
    t.cursor = cursor;
    t.ansi = ansi;
    t
}

/// Normalize a hex string to `#rrggbb` (accepts a leading `#` or not).
fn norm_hex(s: &str) -> Option<String> {
    let h = s.trim().trim_matches('"').trim_matches('\'').trim_start_matches('#');
    if h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(format!("#{}", h.to_lowercase()))
    } else {
        None
    }
}

fn float_to_hex(r: f32, g: f32, b: f32) -> String {
    let q = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", q(r), q(g), q(b))
}

// ---- Windows Terminal ------------------------------------------------------

fn parse_windows_terminal(s: &str, name: &str) -> Result<CustomTerminalTheme, String> {
    let v: serde_json::Value =
        serde_json::from_str(s).map_err(|e| format!("Invalid JSON: {e}"))?;
    let get = |k: &str| v.get(k).and_then(|x| x.as_str()).and_then(norm_hex);
    let req = |k: &str| get(k).ok_or_else(|| format!("Missing or invalid '{k}'"));

    let bg = req("background")?;
    let fg = req("foreground")?;
    let cursor = get("cursorColor").unwrap_or_else(|| fg.clone());
    // Windows Terminal names magenta "purple".
    let keys = [
        "black", "red", "green", "yellow", "blue", "purple", "cyan", "white",
        "brightBlack", "brightRed", "brightGreen", "brightYellow", "brightBlue",
        "brightPurple", "brightCyan", "brightWhite",
    ];
    let mut ansi: [String; 16] = std::array::from_fn(|_| String::new());
    for (i, k) in keys.iter().enumerate() {
        ansi[i] = req(k)?;
    }
    Ok(build(name, fg, bg, cursor, ansi))
}

// ---- base16 ----------------------------------------------------------------

fn parse_base16(s: &str, name: &str) -> Result<CustomTerminalTheme, String> {
    let mut bases: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for line in s.lines() {
        if let Some((k, val)) = line.split_once(':') {
            let k = k.trim();
            if k.len() == 6
                && k.starts_with("base")
                && let Some(hex) = norm_hex(val)
            {
                bases.insert(k.to_string(), hex);
            }
        }
    }
    let b = |k: &str| bases.get(k).cloned().ok_or_else(|| format!("Missing '{k}'"));
    let bg = b("base00")?;
    let fg = b("base05")?;
    // Standard base16 -> ANSI mapping (shell template).
    let ansi = [
        b("base00")?, b("base08")?, b("base0B")?, b("base0A")?,
        b("base0D")?, b("base0E")?, b("base0C")?, b("base05")?,
        b("base03")?, b("base08")?, b("base0B")?, b("base0A")?,
        b("base0D")?, b("base0E")?, b("base0C")?, b("base07")?,
    ];
    Ok(build(name, fg.clone(), bg, fg, ansi))
}

// ---- iTerm2 .itermcolors ---------------------------------------------------

fn parse_iterm(s: &str, name: &str) -> Result<CustomTerminalTheme, String> {
    // For a `<key>NAME</key>` color entry, read the Red/Green/Blue float
    // components from the dict that follows it.
    let color_for = |key: &str| -> Option<String> {
        let start = s.find(&format!("<key>{key}</key>"))?;
        let rest = &s[start..];
        let comp = |name: &str| -> Option<f32> {
            let ci = rest.find(&format!("<key>{name} Component</key>"))?;
            let after = &rest[ci..];
            let ri = after.find("<real>")? + "<real>".len();
            let end = after[ri..].find("</real>")?;
            after[ri..ri + end].trim().parse::<f32>().ok()
        };
        Some(float_to_hex(comp("Red")?, comp("Green")?, comp("Blue")?))
    };

    let bg = color_for("Background Color").ok_or("Missing Background Color")?;
    let fg = color_for("Foreground Color").ok_or("Missing Foreground Color")?;
    let cursor = color_for("Cursor Color").unwrap_or_else(|| fg.clone());
    let mut ansi: [String; 16] = std::array::from_fn(|_| String::new());
    for (i, slot) in ansi.iter_mut().enumerate() {
        *slot = color_for(&format!("Ansi {i} Color"))
            .ok_or_else(|| format!("Missing Ansi {i} Color"))?;
    }
    Ok(build(name, fg, bg, cursor, ansi))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_terminal_round_trips() {
        let json = r##"{
            "name": "Sample",
            "background": "#1e1e1e", "foreground": "#d4d4d4",
            "cursorColor": "#ffffff",
            "black": "#000000", "red": "#cd3131", "green": "#0dbc79",
            "yellow": "#e5e510", "blue": "#2472c8", "purple": "#bc3fbc",
            "cyan": "#11a8cd", "white": "#e5e5e5",
            "brightBlack": "#666666", "brightRed": "#f14c4c",
            "brightGreen": "#23d18b", "brightYellow": "#f5f543",
            "brightBlue": "#3b8eea", "brightPurple": "#d670d6",
            "brightCyan": "#29b8db", "brightWhite": "#ffffff"
        }"##;
        let t = parse_theme(json, "My WT").unwrap();
        assert_eq!(t.name, "My WT");
        assert_eq!(t.background, "#1e1e1e");
        assert_eq!(t.ansi[1], "#cd3131"); // red
        assert_eq!(t.ansi[5], "#bc3fbc"); // purple -> magenta slot
        assert_eq!(t.ansi[15], "#ffffff");
    }

    #[test]
    fn base16_maps_to_ansi() {
        let yaml = "scheme: \"Test\"\nbase00: \"1d1f21\"\nbase01: \"282a2e\"\n\
            base02: \"373b41\"\nbase03: \"969896\"\nbase04: \"b4b7b4\"\n\
            base05: \"c5c8c6\"\nbase06: \"e0e0e0\"\nbase07: \"ffffff\"\n\
            base08: \"cc6666\"\nbase09: \"de935f\"\nbase0A: \"f0c674\"\n\
            base0B: \"b5bd68\"\nbase0C: \"8abeb7\"\nbase0D: \"81a2be\"\n\
            base0E: \"b294bb\"\nbase0F: \"a3685a\"\n";
        let t = parse_theme(yaml, "B16").unwrap();
        assert_eq!(t.background, "#1d1f21"); // base00
        assert_eq!(t.foreground, "#c5c8c6"); // base05
        assert_eq!(t.ansi[1], "#cc6666"); // red = base08
        assert_eq!(t.ansi[2], "#b5bd68"); // green = base0B
        assert_eq!(t.ansi[15], "#ffffff"); // bright white = base07
    }

    #[test]
    fn iterm_floats_to_hex() {
        let xml = r#"<?xml version="1.0"?>
        <plist version="1.0"><dict>
        <key>Background Color</key>
        <dict><key>Red Component</key><real>0.0</real>
        <key>Green Component</key><real>0.0</real>
        <key>Blue Component</key><real>0.0</real></dict>
        <key>Foreground Color</key>
        <dict><key>Red Component</key><real>1.0</real>
        <key>Green Component</key><real>1.0</real>
        <key>Blue Component</key><real>1.0</real></dict>
        <key>Ansi 1 Color</key>
        <dict><key>Red Component</key><real>1.0</real>
        <key>Green Component</key><real>0.0</real>
        <key>Blue Component</key><real>0.0</real></dict>
        </dict></plist>"#;
        // Needs all 16 ANSI keys; this minimal sample should fail cleanly.
        let err = parse_theme(xml, "iT").unwrap_err();
        assert!(err.contains("Ansi"), "expected a missing-Ansi error, got {err}");
    }

    #[test]
    fn iterm_full_parses() {
        let mut xml = String::from("<?xml version=\"1.0\"?>\n<plist><dict>\n");
        let comp = |r: f32, g: f32, b: f32| {
            format!(
                "<dict><key>Red Component</key><real>{r}</real>\
                 <key>Green Component</key><real>{g}</real>\
                 <key>Blue Component</key><real>{b}</real></dict>"
            )
        };
        xml.push_str(&format!("<key>Background Color</key>{}\n", comp(0.1, 0.1, 0.1)));
        xml.push_str(&format!("<key>Foreground Color</key>{}\n", comp(1.0, 1.0, 1.0)));
        for i in 0..16 {
            xml.push_str(&format!("<key>Ansi {i} Color</key>{}\n", comp(1.0, 0.0, 0.0)));
        }
        xml.push_str("</dict></plist>");
        let t = parse_theme(&xml, "iT").unwrap();
        assert_eq!(t.foreground, "#ffffff");
        assert_eq!(t.ansi[0], "#ff0000");
    }

    #[test]
    fn unknown_format_errors() {
        assert!(parse_theme("hello world", "x").is_err());
    }
}
