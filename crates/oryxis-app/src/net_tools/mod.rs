//! The network tools panel's engine: one probe per classic sysadmin
//! question ("does this name resolve", "is that port open", "why does
//! the browser complain about the certificate"), each returning result
//! CARDS the view renders without knowing which tool produced them.
//!
//! Two rules shape every module in here:
//!
//! - **Parsing is pure, probing is not.** Everything that turns bytes or
//!   another program's stdout into a result is a free function over
//!   `&str`, tested without a network. Only the thin `probe_*` wrapper
//!   around it opens a socket or spawns a process.
//! - **A failed probe is a CARD, never an error.** "No MX records",
//!   "connection refused", "traceroute is not installed" are answers to
//!   the question the user asked; collapsing them into the panel's error
//!   line would throw away which of the ten things being probed failed.
//!   `Err` is reserved for "this run could not start at all" (an
//!   unparseable target, no resolver on the machine).

pub(crate) mod dns;
pub(crate) mod http;
pub(crate) mod icmp;
pub(crate) mod ping;
pub(crate) mod port;
pub(crate) mod rbl;
pub(crate) mod tls;
pub(crate) mod whois;

/// The tools the panel offers, in the order the selector lists them.
/// Ordered by how often the question gets asked while debugging
/// reachability, which is also roughly cheapest-first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) enum NetTool {
    #[default]
    Dns,
    Ping,
    Traceroute,
    PortTest,
    HttpCheck,
    Whois,
    Rbl,
}

impl NetTool {
    pub(crate) const ALL: [NetTool; 7] = [
        NetTool::Dns,
        NetTool::Ping,
        NetTool::Traceroute,
        NetTool::PortTest,
        NetTool::HttpCheck,
        NetTool::Whois,
        NetTool::Rbl,
    ];

    /// i18n key for the selector label.
    pub(crate) fn label_key(&self) -> &'static str {
        match self {
            NetTool::Dns => "net_tool_dns",
            NetTool::Ping => "net_tool_ping",
            NetTool::Traceroute => "net_tool_traceroute",
            NetTool::PortTest => "net_tool_port",
            NetTool::HttpCheck => "net_tool_http",
            NetTool::Whois => "net_tool_whois",
            NetTool::Rbl => "net_tool_rbl",
        }
    }

    /// Whether the tool takes the port list next to the target. Only the
    /// port test does: an HTTP check carries its port in the URL, and
    /// every other tool asks a fixed one (53, 43, ICMP has none).
    pub(crate) fn needs_ports(&self) -> bool {
        matches!(self, NetTool::PortTest)
    }

    /// Placeholder for the target field, which is a different KIND of
    /// value per tool: a host name, a URL, a bare IPv4.
    pub(crate) fn target_placeholder_key(&self) -> &'static str {
        match self {
            NetTool::HttpCheck => "net_target_url_ph",
            NetTool::Whois => "net_target_domain_ph",
            NetTool::Rbl => "net_target_ip_ph",
            _ => "net_target_ph",
        }
    }

    /// One-line explanation under the selector: what this tool actually
    /// asks the network, so the panel teaches rather than just answers.
    pub(crate) fn hint_key(&self) -> &'static str {
        match self {
            NetTool::Dns => "net_hint_dns",
            NetTool::Ping => "net_hint_ping",
            NetTool::Traceroute => "net_hint_traceroute",
            NetTool::PortTest => "net_hint_port",
            NetTool::HttpCheck => "net_hint_http",
            NetTool::Whois => "net_hint_whois",
            NetTool::Rbl => "net_hint_rbl",
        }
    }
}

impl std::fmt::Display for NetTool {
    /// Backs the fork's 4-step `pick_list` mapper, so the selector shows
    /// the translated label without a lookup table at the call site.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(crate::i18n::t(self.label_key()))
    }
}

/// How a card reads at a glance: the view tints its left edge from this,
/// so a wall of DNSBL zones shows the one listing without being read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum CardStatus {
    /// The probe answered and the answer is the good one.
    Ok,
    /// The probe answered and the answer deserves attention (a
    /// certificate expiring soon, a redirect chain, a filtered port).
    Warn,
    /// The probe answered and the answer is bad (listed on a DNSBL, an
    /// expired certificate, a refused connection).
    Bad,
    /// Informational: raw output, a section header, a note.
    #[default]
    Neutral,
}

/// One result block. `lines` is what the panel renders; `raw` is what
/// the copy action puts on the clipboard, which for a shelled-out tool
/// is the program's own output rather than our rendering of it.
#[derive(Debug, Clone)]
pub(crate) struct NetToolCard {
    pub title: String,
    pub lines: Vec<String>,
    pub raw: String,
    pub status: CardStatus,
}

impl NetToolCard {
    pub(crate) fn new(title: impl Into<String>, lines: Vec<String>) -> Self {
        let title = title.into();
        let raw = if lines.is_empty() {
            title.clone()
        } else {
            format!("{title}\n{}", lines.join("\n"))
        };
        Self { title, lines, raw, status: CardStatus::Neutral }
    }

    pub(crate) fn status(mut self, status: CardStatus) -> Self {
        self.status = status;
        self
    }

    /// Replace what the copy action yields. Used by the tools that shell
    /// out or speak a text protocol: the user copying a WHOIS card wants
    /// the registry's own text, not our extracted five fields.
    pub(crate) fn raw(mut self, raw: impl Into<String>) -> Self {
        self.raw = raw.into();
        self
    }
}

/// Run one tool against one target. `ports` is only read by the port
/// test; the other tools ignore it, so the panel can keep the field's
/// contents while the user switches tools.
///
/// The `Err` arm means the run never started (see the module docs); a
/// probe that ran and failed returns its failure as a card.
pub(crate) async fn run(
    tool: NetTool,
    target: String,
    ports: String,
) -> Result<Vec<NetToolCard>, String> {
    let target = target.trim().to_string();
    if target.is_empty() {
        return Err(crate::i18n::t("net_err_no_target").to_string());
    }
    match tool {
        NetTool::Dns => dns::probe(&target).await,
        NetTool::Ping => ping::probe_ping(&target).await,
        NetTool::Traceroute => ping::probe_traceroute(&target).await,
        NetTool::PortTest => port::probe(&target, &ports).await,
        NetTool::HttpCheck => http::probe(&target).await,
        NetTool::Whois => whois::probe(&target).await,
        NetTool::Rbl => rbl::probe(&target).await,
    }
}

/// Strip a scheme and any path from a target the user pasted from a
/// browser, so `https://example.com/path` pings `example.com`. Shared by
/// every tool that wants a bare host, which is all of them except the
/// HTTP check (that one needs the URL intact).
///
/// A bare IPv6 literal is left alone: it is all colons, and splitting it
/// on the first one would turn `::1` into an empty host.
pub(crate) fn host_of(target: &str) -> &str {
    let t = target.trim();
    let t = match t.split_once("://") {
        Some((_, rest)) => rest,
        None => t,
    };
    let t = t.split('/').next().unwrap_or(t);
    let t = t.split('?').next().unwrap_or(t);
    // `[::1]:8080` -> `::1`; a bracketed literal is the only form where a
    // port can follow an IPv6 address unambiguously.
    if let Some(rest) = t.strip_prefix('[')
        && let Some((inner, _)) = rest.split_once(']')
    {
        return inner;
    }
    if t.parse::<std::net::Ipv6Addr>().is_ok() {
        return t;
    }
    // `host:port` -> `host`, but only when what follows is actually a
    // port: a trailing colon in a typo must not silently drop it.
    match t.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && port.parse::<u16>().is_ok() => host,
        _ => t,
    }
}

/// A target the ping and traceroute fallbacks would hand to the system
/// binary as one of ITS OWN flags rather than as a host.
///
/// Those two are the only tools here that spawn a process, and the host
/// is the last word of the argv. `-f` is a flood ping and `-I eth0`
/// picks an interface, so a leading dash has to be refused before the
/// spawn rather than explained after it. No hostname or address starts
/// with one, so nothing legitimate is lost; `--` is not the answer
/// because Windows `ping` does not parse it.
pub(crate) fn is_flag_like(host: &str) -> bool {
    host.starts_with('-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_of_strips_scheme_path_and_port() {
        assert_eq!(host_of("https://example.com/a/b?c=d"), "example.com");
        assert_eq!(host_of("example.com:8443"), "example.com");
        assert_eq!(host_of("  example.com  "), "example.com");
        assert_eq!(host_of("ssh://user@host.tld"), "user@host.tld");
    }

    #[test]
    fn host_of_keeps_ipv6_literals_whole() {
        assert_eq!(host_of("::1"), "::1");
        assert_eq!(host_of("2001:db8::1"), "2001:db8::1");
        assert_eq!(host_of("[2001:db8::1]:443"), "2001:db8::1");
        assert_eq!(host_of("https://[::1]:8080/x"), "::1");
    }

    #[test]
    fn host_of_leaves_a_trailing_colon_alone() {
        // Not a port, so dropping it would hide the typo from the user.
        assert_eq!(host_of("example.com:"), "example.com:");
        assert_eq!(host_of("example.com:http"), "example.com:http");
    }

    #[test]
    fn every_tool_has_distinct_i18n_keys() {
        let mut keys: Vec<&str> = NetTool::ALL.iter().map(|t| t.label_key()).collect();
        keys.sort_unstable();
        let count = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), count, "two tools share a label key");
        for tool in NetTool::ALL {
            assert_ne!(crate::i18n::en_lookup(tool.label_key()), "???");
            assert_ne!(crate::i18n::en_lookup(tool.hint_key()), "???");
            assert_ne!(crate::i18n::en_lookup(tool.target_placeholder_key()), "???");
        }
    }
}
