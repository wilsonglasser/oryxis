//! What a link printed in the terminal means, before it is opened.
//!
//! Two questions the dispatcher asks about a URL that a REMOTE host put
//! on the wire. Whether it should be confirmed first, which is a matter
//! of where the pane's bytes come from and is decided by the caller. And
//! whether it carries a LOOPBACK CALLBACK: a `redirect_uri` pointing at
//! `127.0.0.1:<port>`, which is the shape every CLI OAuth login has
//! (`aws sso login`, `gcloud auth login`, ...). That address means "the
//! machine running the CLI", and over SSH that machine is the remote one,
//! so the browser we hand the link to lands on the wrong loopback unless
//! the port is tunnelled first.
//!
//! Everything here is pure text work, so the parsing is unit-tested
//! without a live session in the loop.

/// A loopback callback that a link expects to be able to reach on the
/// machine that printed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoopbackCallback {
    /// The host as written (`127.0.0.1`, `localhost`, `[::1]`), for the
    /// confirmation dialog and for the FAMILY the tunnel binds: a
    /// callback written at `[::1]` is one the browser will dial over
    /// IPv6, and an IPv4-only listener leaves it reaching nothing. A
    /// name (`localhost`) stays on IPv4, which is the address every
    /// resolver has for it and the one a browser falls back to.
    pub host: String,
    /// The port the remote process is listening on, and therefore the
    /// port that has to be bound here too. Never remapped: the
    /// authorization server redirects the browser to the exact
    /// `redirect_uri` the CLI registered, port included.
    pub port: u16,
}

/// Query keys whose value is a URL the browser will be sent to after
/// the user authorizes. `redirect_uri` is the OAuth 2.0 spelling (RFC
/// 6749) and covers the AWS / Google / Azure CLIs; `redirect_url` is the
/// variant a handful of providers use.
const REDIRECT_KEYS: [&str; 2] = ["redirect_uri", "redirect_url"];

/// The loopback callback a link carries, if any.
///
/// It has to be carried IN THE QUERY, under a redirect key: an authorize
/// URL on the provider's domain whose `redirect_uri` comes back to
/// loopback. A link that merely IS a loopback URL does not count, even
/// with a port written out, because that is the shape of a dev server a
/// remote `npm run dev` announced (`http://localhost:3000`) far more
/// often than it is a login. Treating that as a callback tunnelled a
/// port nobody asked about, and, when the same port was already busy
/// here, refused to open the link at all - a browser that opens on a
/// URL nobody would have tunnelled anything for.
pub(crate) fn loopback_callback(url: &str) -> Option<LoopbackCallback> {
    let query = url.split_once('?').map(|(_, q)| q)?;
    // A fragment is not part of the query.
    let query = query.split('#').next().unwrap_or(query);
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .filter(|(key, _)| {
            REDIRECT_KEYS
                .iter()
                .any(|k| key.eq_ignore_ascii_case(k))
        })
        .find_map(|(_, value)| as_loopback(&percent_decode(value)))
}

/// A redirect value as a loopback callback: an `http(s)` URL whose host
/// is a loopback address AND whose port is written out.
///
/// The explicit port is required rather than defaulted. A callback URL
/// always carries one (the CLI binds an ephemeral port and puts it in
/// the `redirect_uri`), and there is nothing to bind without it.
///
/// The authority is delimited the way a BROWSER delimits it, because
/// what this answers is used to describe, in the confirmation, where the
/// browser is about to go. A special scheme ends its authority at `\`
/// too (WHATWG URL), so `http://evil.com\@127.0.0.1:1234/` has host
/// `evil.com` there; reading it as loopback here would put a port in the
/// dialog that has nothing to do with the page that opens.
fn as_loopback(url: &str) -> Option<LoopbackCallback> {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))?;
    let authority = rest
        .split(['/', '?', '#', '\\'])
        .next()
        .filter(|a| !a.is_empty())?;
    // Userinfo is not part of the host.
    let authority = authority.rsplit('@').next()?;
    let (host, port) = split_host_port(authority)?;
    is_loopback_host(host).then(|| LoopbackCallback {
        host: host.to_string(),
        port,
    })
}

/// Split an authority into host and explicit port, keeping an IPv6
/// literal's brackets on the host (`[::1]:1234` -> `[::1]`, `1234`).
/// `None` when no port is written, or when what follows the colon is not
/// a port.
fn split_host_port(authority: &str) -> Option<(&str, u16)> {
    let (host, port) = if let Some(end) = authority.find(']') {
        let (bracketed, rest) = authority.split_at(end + 1);
        (bracketed, rest.strip_prefix(':')?)
    } else {
        authority.rsplit_once(':')?
    };
    let port: u16 = port.parse().ok()?;
    (port != 0).then_some((host, port))
}

/// Whether a host names this machine's loopback interface. All of
/// `127.0.0.0/8` counts: a callback is not always on `.0.1` (macOS
/// tooling reaches for `127.0.0.53` and friends).
fn is_loopback_host(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => false,
    }
}

/// Decode `%XX` escapes, leaving everything else (including `+`) alone.
///
/// Enough for the one job here: a `redirect_uri` travels percent-encoded
/// in the query, and a `+` inside one would itself be `%2B`. Invalid
/// escapes are passed through verbatim rather than dropped, so a
/// malformed value stays visibly malformed instead of turning into a
/// different URL.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Some(hi) = (bytes[i + 1] as char).to_digit(16)
            && let Some(lo) = (bytes[i + 2] as char).to_digit(16)
        {
            out.push((hi * 16 + lo) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The target as the confirmation may SHOW it.
///
/// That dialog is the one place where what a link claims and where it
/// points are put side by side, so it exists to be read. A URL is remote
/// text, and text can be made to read as something it is not: a bidi
/// override reverses the run it opens, so a trusted-looking name can be
/// drawn where the real host should be, and a control byte simply
/// vanishes. Neither reaches the dialog. `?` per character, the way the
/// SFTP console's listing answers the same problem, because it is one
/// column wide and leaves the elision counting what is drawn.
///
/// Sanitized BEFORE the cut, so the cut can never drop the pop that
/// closes an override it kept.
///
/// Display only: what gets opened, tunnelled and copied is the real
/// string.
pub(crate) fn display_target(url: &str, max: usize) -> String {
    let shown: String = url
        .chars()
        .map(|c| if c.is_control() || is_bidi_control(c) { '?' } else { c })
        .collect();
    elide_middle(&shown, max)
}

/// Characters that reorder the text around them instead of drawing
/// anything: the explicit embeddings, overrides and isolates
/// (U+202A..U+202E, U+2066..U+2069) plus the two marks. `is_control`
/// covers C0 and C1 and none of these.
fn is_bidi_control(c: char) -> bool {
    matches!(
        c,
        '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
    )
}

/// Shorten a URL for display, keeping both ends.
///
/// An authorize URL runs to several hundred characters, and the half
/// that identifies it (the host) and the half that betrays a spoof (the
/// tail) are at opposite ends, so a plain truncation hides exactly the
/// part worth reading. Cuts on char boundaries.
fn elide_middle(url: &str, max: usize) -> String {
    let count = url.chars().count();
    if count <= max || max < 8 {
        return url.to_string();
    }
    // The gap marker costs one char, so head + tail is `max - 1`.
    let head = max.div_ceil(2) - 1;
    let tail = max - 1 - head;
    let start: String = url.chars().take(head).collect();
    let end: String = url.chars().skip(count - tail).collect();
    format!("{start}\u{2026}{end}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cb(url: &str) -> Option<(String, u16)> {
        loopback_callback(url).map(|c| (c.host, c.port))
    }

    #[test]
    fn aws_sso_authorize_url_yields_its_callback_port() {
        // The shape `aws sso login` prints once it uses the auth-code
        // flow with PKCE: a provider URL whose redirect comes back to
        // the CLI's own loopback listener.
        let url = "https://oidc.ap-southeast-2.amazonaws.com/authorize?\
                   response_type=code&client_id=abc&\
                   redirect_uri=http%3A%2F%2F127.0.0.1%3A55341%2Foauth%2Fcallback&\
                   state=xyz";
        assert_eq!(cb(url), Some(("127.0.0.1".to_string(), 55341)));
    }

    #[test]
    fn a_bare_loopback_url_is_not_a_callback() {
        // The shape a remote `npm run dev` announces. It carries a port,
        // it is loopback, and it is still nobody's login: tunnelling it
        // would bind a port on this machine unasked, and refuse to open
        // the link at all whenever that port was already busy here.
        assert_eq!(cb("http://localhost:3000/"), None);
        assert_eq!(cb("http://127.0.0.1:8080/admin"), None);
        // Even carrying a query, as long as no redirect key names it.
        assert_eq!(cb("http://127.0.0.1:1410/?code=4/x"), None);
    }

    #[test]
    fn ipv6_loopback_keeps_its_brackets_and_finds_its_port() {
        assert_eq!(
            cb("https://p.test/a?redirect_uri=http%3A%2F%2F%5B%3A%3A1%5D%3A9000%2Fcb"),
            Some(("[::1]".to_string(), 9000))
        );
    }

    #[test]
    fn a_backslash_ends_the_authority_the_way_a_browser_ends_it() {
        // A special scheme's authority ends at `\` too, so the browser
        // reads this as host `evil.com` with `/@127.0.0.1:1234/` for a
        // path. Answering "loopback, port 1234" would put a port in the
        // confirmation that has nothing to do with the page that opens.
        assert_eq!(
            cb("https://p.test/a?redirect_uri=http%3A%2F%2Fevil.com%5C%40127.0.0.1%3A1234%2Fx"),
            None
        );
    }

    #[test]
    fn a_public_link_has_no_callback() {
        assert_eq!(cb("https://oryxis.app/themes"), None);
        assert_eq!(cb("https://example.com/?redirect_uri=https%3A%2F%2Fevil.com%2Fcb"), None);
        // A public host that merely CONTAINS the loopback text.
        assert_eq!(
            cb("https://p.test/a?redirect_uri=http%3A%2F%2F127.0.0.1.evil.com%3A8080%2Fcb"),
            None
        );
    }

    #[test]
    fn a_callback_without_a_written_port_is_not_tunnelled() {
        // Nothing to bind: port 80 would be a guess.
        assert_eq!(
            cb("https://provider.test/auth?redirect_uri=http%3A%2F%2Flocalhost%2Fcb"),
            None
        );
        assert_eq!(
            cb("https://provider.test/auth?redirect_uri=http%3A%2F%2F127.0.0.1%3A0%2Fcb"),
            None
        );
    }

    #[test]
    fn userinfo_does_not_pass_for_a_host() {
        // `evil.com` is the host here, not the loopback in the userinfo.
        assert_eq!(
            cb("https://p.test/a?redirect_uri=http%3A%2F%2F127.0.0.1%3A1234%40evil.com%3A80%2Fx"),
            None
        );
    }

    #[test]
    fn the_redirect_key_is_matched_whole_and_case_insensitively() {
        assert_eq!(
            cb("https://p.test/a?Redirect_URI=http%3A%2F%2F127.0.0.1%3A70%2Fc"),
            Some(("127.0.0.1".to_string(), 70))
        );
        // A key that merely ends in the same letters is not the one.
        assert_eq!(
            cb("https://p.test/a?not_redirect_uri=http%3A%2F%2F127.0.0.1%3A70%2Fc"),
            None
        );
    }

    #[test]
    fn a_fragment_is_not_searched_for_a_callback() {
        assert_eq!(
            cb("https://p.test/a?x=1#redirect_uri=http%3A%2F%2F127.0.0.1%3A70%2Fc"),
            None
        );
    }

    #[test]
    fn percent_decoding_passes_malformed_escapes_through() {
        assert_eq!(percent_decode("a%2Fb"), "a/b");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
        assert_eq!(percent_decode("%2"), "%2");
    }

    #[test]
    fn elision_keeps_both_ends_and_stays_on_char_boundaries() {
        let url = "https://oidc.example.com/authorize?state=0123456789";
        let short = elide_middle(url, 20);
        assert_eq!(short.chars().count(), 20);
        assert!(short.starts_with("https://o"));
        assert!(short.ends_with("456789"));
        // Short enough already: returned untouched.
        assert_eq!(elide_middle("https://a.co", 20), "https://a.co");
        // Multi-byte chars must not be split mid-sequence.
        let wide = "https://例.example.com/authorize?state=0123456789";
        assert_eq!(elide_middle(wide, 20).chars().count(), 20);
    }

    #[test]
    fn a_shown_target_cannot_reorder_or_hide_itself() {
        // The dialog is read, so the characters that reorder the run
        // around them never reach it: with the override intact this
        // draws as `https://moc.live/x`, a host the user never agreed
        // to. Non-ASCII text is not a control and must survive.
        assert_eq!(
            display_target("https://\u{202E}evil.com/x", 80),
            "https://?evil.com/x"
        );
        assert_eq!(display_target("https://a.co/\u{2066}b\u{2069}", 80), "https://a.co/?b?");
        assert_eq!(display_target("https://a.co/\u{7}x\ty", 80), "https://a.co/?x?y");
        assert_eq!(display_target("https://例.com/relatório", 80), "https://例.com/relatório");
    }
}
