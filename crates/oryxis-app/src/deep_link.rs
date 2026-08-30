//! OS-registered URL schemes and their in-app routing: `oryxis://`
//! (issue #118's theme sharing follow-up) and `ssh://` (quick connect).
//!
//! Two halves:
//!
//! - **Parsing** ([`parse`]): pure, size-capped, and strict. A link
//!   that doesn't parse is dropped (with a log line), never "best
//!   effort" handled: these URLs arrive from browsers, i.e. from any
//!   web page the user clicked.
//! - **Routing** ([`Oryxis::handle_deep_link`]): every route lands on
//!   an EXISTING confirm surface with the payload prefilled, never on
//!   a side effect. A theme link opens the import panel (Apply ->
//!   editor -> Save stays the user's call); an `ssh://` link
//!   opens the ad-hoc host editor with the target filled in. Nothing
//!   installs, joins or DIALS on its own, so a hostile link can at
//!   worst open a screen.
//!
//! The `oryxis user@host` CLI form deliberately does NOT ride that
//! routing: it lands on [`Oryxis::handle_connect_target`] and connects,
//! because the user typed it in their own shell. The two travel in
//! separate `tray_ipc` inboxes so a claiming window never has to infer
//! which launcher wrote a payload.
//!
//! Delivery paths into this module:
//!
//! - Cold start: the OS hands the URL on argv; `main.rs` stashes it in
//!   [`crate::app::PENDING_DEEP_LINK`] and boot routes it (post-unlock
//!   via `pending_deep_link`, mirroring `--connect`).
//! - Running instance: the OS spawns a second process, which forwards
//!   the URL through `tray_ipc::write_deeplink` and exits; the
//!   `deep_link_stream` subscription in every window claims and routes
//!   it (`TrayMessage::DeepLink`).
//!
//! macOS is NOT wired yet: LaunchServices delivers URLs as Apple
//! Events (`kAEGetURL`), not argv, so it needs an event handler whose
//! interaction with winit's NSApplication delegate is unverified from
//! this machine. The scheme is therefore not declared in Info.plist
//! either; both land together when they can be QA'd on hardware.

use base64::Engine as _;

use crate::messages::Message;
use iced::Task;

/// Hard cap on an incoming URL. A theme payload is ~1 KiB of base64;
/// anything near this size is hostile or corrupt.
const MAX_URL_LEN: usize = 128 * 1024;

/// A parsed, shape-validated deep link. Payload validation beyond the
/// shape (does the theme import?) stays
/// with the flows the link routes into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeepLink {
    /// `oryxis://theme/<base64url JSON>`: a theme file to import, the
    /// same bytes the gallery's Copy button yields. `ui` mirrors the
    /// `oryxis_ui_theme` marker and picks which import panel opens.
    ThemeInstall { json: String, ui: bool },
    /// `ssh://user@host:port`: the standard SSH URL scheme, which the
    /// OS hands us when the user clicks one in a browser or a document.
    /// Carried as the canonical target string; the route PREFILLS the
    /// ad-hoc host editor rather than dialing, because a web page picks
    /// this payload (see the CLI positional for the path that connects).
    SshTarget(String),
}

/// Parse a raw `oryxis://` URL. `None` means "not ours / malformed":
/// callers log and drop, they never surface parse errors to the user
/// (the user didn't type this, a web page did).
pub fn parse(url: &str) -> Option<DeepLink> {
    if url.len() > MAX_URL_LEN {
        return None;
    }
    // Browsers and the Windows shell like to append a trailing slash
    // to protocol launches. Neither route's payload can legitimately
    // end in one (base64url has no `/`), so
    // strip it before the strict parsers see the link.
    let url = url.trim().trim_end_matches('/');
    if let Some(authority) = url.strip_prefix("ssh://") {
        return parse_ssh_authority(authority).map(DeepLink::SshTarget);
    }
    let rest = url.strip_prefix("oryxis://")?;
    let payload = rest.strip_prefix("theme/")?;
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let bytes = engine.decode(payload).ok()?;
    let json = String::from_utf8(bytes).ok()?;
    // Shape gate only: is it a JSON object, and which kind of theme.
    // `parse_theme` / `parse_ui_theme` do the real validation when the
    // user presses Apply in the import panel this link opens.
    let value: serde_json::Value = serde_json::from_str(&json).ok()?;
    let obj = value.as_object()?;
    let ui = obj.contains_key("oryxis_ui_theme");
    Some(DeepLink::ThemeInstall { json, ui })
}

/// The authority of an `ssh://` URL (everything after the scheme) as a
/// canonical target string, or `None` when it isn't a plain
/// `[user@]host[:port]`.
///
/// Deliberately stricter than a URL library would be, because the input
/// comes from web pages:
///
/// - a path, query or fragment REJECTS the whole URL rather than being
///   stripped. `ssh://host/../../x` must never be quietly read as
///   `host`, and OpenSSH's own `ssh://` form has no path anyway.
/// - percent-encoding rejects too. Decoding could re-shape the target
///   after validation (`%40` minting a username separator, `%00` and
///   friends smuggling control characters), and nothing legitimate in a
///   hostname or username needs it.
pub fn parse_ssh_authority(authority: &str) -> Option<String> {
    if authority.is_empty() || authority.contains('%') {
        return None;
    }
    // Reject the delimiters that would start a path / query / fragment.
    // An IPv6 literal's own brackets and colons are handled by the
    // target parser; none of these three can appear inside one.
    if authority.contains('/') || authority.contains('?') || authority.contains('#') {
        return None;
    }
    let target = oryxis_core::ssh_target::SshTarget::parse(authority)?;
    Some(target.canonical())
}

impl crate::app::Oryxis {
    /// Route a parsed deep link. Locked vault stashes the link in
    /// `pending_deep_link` (the `--connect` pattern): the unlock
    /// handler and boot both drain it, so a link clicked at the lock
    /// screen lands right after the master password.
    pub(crate) fn handle_deep_link(&mut self, link: DeepLink) -> Task<Message> {
        use crate::messages::TabsMessage;
        if self.vault_ui.state != crate::state::VaultState::Unlocked {
            self.pending_deep_link = Some(link);
            return Task::none();
        }
        match link {
            DeepLink::ThemeInstall { json, ui } => {
                // Mirror the ThemeImportOpen / UiThemeImportOpen
                // handlers (which reset these fields) and then prefill
                // the pasted-content editor, so the panel comes up as
                // if the user had pasted the file: Apply -> editor ->
                // Save keeps every existing validation and confirm.
                let section = if ui {
                    self.panels.ui_theme_import = true;
                    self.ui_theme_import_content =
                        iced::widget::text_editor::Content::with_text(&json);
                    self.ui_theme_import_name.clear();
                    self.ui_theme_import_error = None;
                    crate::state::SettingsSection::Interface
                } else {
                    self.panels.theme_import = true;
                    self.theme_ui.import_content =
                        iced::widget::text_editor::Content::with_text(&json);
                    self.theme_ui.import_name.clear();
                    self.theme_ui.import_error = None;
                    crate::state::SettingsSection::Terminal
                };
                Task::done(Message::Tabs(TabsMessage::OpenSettingsSection(section)))
            }
            DeepLink::SshTarget(target) => {
                // A web page chose this host, so the link lands on the
                // ad-hoc host editor with the target prefilled and the
                // Connect / Save footer, never on a dial. Registering
                // the ephemeral entry first is what `EditQuickHost`
                // needs; it is the same surface the quick-connect
                // failure screen's "Edit Host" opens, so no new panel,
                // i18n or keynav is involved.
                let Some(conn) = self.quick_connect_target(&target) else {
                    tracing::warn!("deep link: ssh target no longer offerable");
                    return Task::none();
                };
                let id = conn.id;
                self.quick_connects
                    .insert(id, crate::state::QuickConnectEntry::bare(conn));
                Task::done(Message::Editor(
                    crate::messages::EditorMessage::EditQuickHost(id),
                ))
            }
        }
    }

    /// Connect straight to a target the USER named locally: the
    /// `oryxis user@host` CLI form. Unlike the `ssh://` route above,
    /// the provenance here is the user's own shell, so this dials the
    /// way `ssh user@host` would; the two paths deliberately never
    /// share a transport (see `tray_ipc`'s separate inboxes).
    pub(crate) fn handle_connect_target(&mut self, target: &str) -> Task<Message> {
        if self.vault_ui.state != crate::state::VaultState::Unlocked {
            self.pending_connect_target = Some(target.to_string());
            return Task::none();
        }
        let Some(conn) = self.quick_connect_target(target) else {
            tracing::warn!("cli connect: target not offerable");
            return Task::none();
        };
        Task::done(Message::Ssh(crate::messages::SshMessage::QuickConnect(
            Box::new(crate::state::QuickConnectEntry::bare(conn)),
        )))
    }

    /// Handle a raw URL claimed from the cross-process inbox while
    /// this window is already running. On Windows the window may be
    /// hidden to the tray, so surface it first; the link's own route
    /// then decides what to show.
    pub(crate) fn handle_deep_link_url(&mut self, url: &str) -> Task<Message> {
        let Some(link) = parse(url) else {
            tracing::warn!("deep link: ignoring malformed forwarded URL");
            return Task::none();
        };
        let route = self.handle_deep_link(link);
        #[cfg(target_os = "windows")]
        let route = Task::batch([
            Task::done(Message::Tray(crate::messages::TrayMessage::Show)),
            route,
        ]);
        route
    }
}

#[cfg(test)]
mod tests {
    use super::{DeepLink, parse};

    /// Encode a theme file the way the site's future Install button
    /// will: the inverse of the `theme/` arm of [`parse`].
    fn format_theme_link(json: &str) -> String {
        use base64::Engine as _;
        let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        format!("oryxis://theme/{}", engine.encode(json.as_bytes()))
    }

    const TERMINAL_JSON: &str =
        r##"{"name":"Night","author":"a","license":"MIT","background":"#000000"}"##;
    const UI_JSON: &str =
        r#"{"oryxis_ui_theme":1,"name":"Night","author":"a","license":"MIT","colors":{}}"#;

    /// The canonical shapes OpenSSH's own `ssh://` URLs take. A missing
    /// user resolves against the local account (so the parse must
    /// succeed even though the canonical string then varies by machine)
    /// and a missing port means 22.
    #[test]
    fn ssh_urls_parse_to_targets() {
        assert_eq!(
            parse("ssh://wilson@10.0.0.5:2222"),
            Some(DeepLink::SshTarget("wilson@10.0.0.5:2222".into()))
        );
        assert_eq!(
            parse("ssh://wilson@example.com"),
            Some(DeepLink::SshTarget("wilson@example.com".into()))
        );
        // IPv6 literals keep their brackets through canonicalization.
        assert_eq!(
            parse("ssh://wilson@[::1]:2222"),
            Some(DeepLink::SshTarget("wilson@[::1]:2222".into()))
        );
        // Trailing slash: browsers and the Windows shell add one.
        assert!(matches!(
            parse("ssh://wilson@example.com/"),
            Some(DeepLink::SshTarget(_))
        ));
        // No user at all still parses (the target keeps it empty and
        // the route fills in the local account).
        assert!(matches!(parse("ssh://example.com"), Some(DeepLink::SshTarget(_))));
    }

    /// These URLs come from web pages, so anything that could re-shape
    /// the target after validation is rejected outright rather than
    /// sanitized: a path must never fold into the hostname, and
    /// percent-encoding must never mint a `@` or a control character.
    #[test]
    fn hostile_ssh_urls_are_rejected() {
        for url in [
            "ssh://host/../../etc/passwd",
            "ssh://host/path",
            "ssh://user@host?x=1",
            "ssh://user@host#frag",
            "ssh://user%40evil.com@host",
            "ssh://user@host%00",
            "ssh://%2e%2e/host",
            "ssh://",
        ] {
            assert_eq!(parse(url), None, "should reject: {url}");
        }
    }

    /// The structural invariant behind the two-path design: a `ssh://`
    /// URL parses to the PREFILL route, never to the CLI's dialing one.
    /// A refactor that collapsed the two would let any web page start
    /// an outbound SSH connection, which is exactly what the separate
    /// `tray_ipc` inboxes and this variant exist to prevent.
    #[test]
    fn an_ssh_url_never_becomes_a_connect() {
        let link = parse("ssh://root@evil.example:22").expect("valid shape");
        assert!(
            matches!(link, DeepLink::SshTarget(_)),
            "ssh:// must route to the confirm surface, not a dial"
        );
    }

    #[test]
    fn theme_link_round_trips() {
        let link = format_theme_link(TERMINAL_JSON);
        assert_eq!(
            parse(&link),
            Some(DeepLink::ThemeInstall {
                json: TERMINAL_JSON.to_string(),
                ui: false,
            })
        );
    }

    #[test]
    fn ui_marker_selects_the_ui_panel() {
        let link = format_theme_link(UI_JSON);
        assert_eq!(
            parse(&link),
            Some(DeepLink::ThemeInstall { json: UI_JSON.to_string(), ui: true })
        );
    }

    #[test]
    fn browser_trailing_slash_is_tolerated() {
        let link = format!("{}/", format_theme_link(TERMINAL_JSON));
        assert!(parse(&link).is_some());
    }

    #[test]
    fn hostile_payloads_are_dropped() {
        // Wrong scheme / route.
        assert_eq!(parse("https://oryxis.app/themes"), None);
        assert_eq!(parse("oryxis://themes/abc"), None);
        // Not base64url.
        assert_eq!(parse("oryxis://theme/%%%"), None);
        // Valid base64 of invalid UTF-8.
        assert_eq!(parse("oryxis://theme/_w"), None);
        // Valid base64 of non-object JSON.
        let engine = {
            use base64::Engine as _;
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"[1,2]")
        };
        assert_eq!(parse(&format!("oryxis://theme/{engine}")), None);
        // Oversized URL.
        let huge = format!("oryxis://theme/{}", "A".repeat(200 * 1024));
        assert_eq!(parse(&huge), None);
    }
}
