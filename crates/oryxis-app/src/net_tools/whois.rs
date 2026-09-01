//! WHOIS over the port 43 text protocol.
//!
//! There is no discovery service for "which server knows about this
//! name", so the lookup walks the referral chain the way the protocol
//! intends: ask IANA, follow its `refer:` line to the registry, and
//! follow the registry's `Registrar WHOIS Server:` to the registrar that
//! holds the contact data. Thin registries (`.com` is the famous one)
//! answer with almost nothing until that last hop is taken, which is why
//! two referrals are followed rather than one.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{CardStatus, NetToolCard};
use crate::i18n::t;

/// Where every lookup starts. IANA's server knows which registry owns a
/// TLD and which RIR owns an address block, so both target kinds enter
/// the chain here.
const ROOT_SERVER: &str = "whois.iana.org";
/// Total budget for the whole chain. Each hop gets what is left, so a
/// slow registry cannot spend the registrar's share.
const TOTAL_BUDGET: Duration = Duration::from_secs(10);
/// Cap on one server's answer. Registries return kilobytes; a server
/// that streams without end must not be allowed to fill memory.
const MAX_RESPONSE: u64 = 256 * 1024;
/// Referral hops after the root: registry, then registrar.
const MAX_REFERRALS: usize = 2;

pub(crate) async fn probe(target: &str) -> Result<Vec<NetToolCard>, String> {
    let query = super::host_of(target).trim_end_matches('.').to_string();
    if query.is_empty() {
        return Err(t("net_err_no_target").to_string());
    }
    let deadline = tokio::time::Instant::now() + TOTAL_BUDGET;
    let mut cards = Vec::new();
    let mut server = ROOT_SERVER.to_string();
    let mut hops = 0;

    loop {
        let body = match ask(&server, &query, deadline).await {
            Ok(body) => body,
            Err(e) => {
                cards.push(
                    NetToolCard::new(server.clone(), vec![e]).status(CardStatus::Bad),
                );
                break;
            }
        };
        let next = next_server(&body).filter(|s| *s != server);
        cards.push(card_for(&server, &body));
        hops += 1;
        match next {
            Some(n) if hops <= MAX_REFERRALS => server = n,
            _ => break,
        }
    }
    Ok(cards)
}

/// One server's answer, rendered: the fields worth reading at a glance,
/// with the server's own text behind the copy action.
fn card_for(server: &str, body: &str) -> NetToolCard {
    if is_not_found(body) {
        return NetToolCard::new(server.to_string(), vec![t("net_whois_not_found").to_string()])
            .status(CardStatus::Warn)
            .raw(body.to_string());
    }
    let fields = extract_fields(body);
    let lines = if fields.is_empty() {
        // Some registries answer in a format nobody standardized. The
        // body is still the answer, so the card shows it rather than
        // claiming the lookup found nothing.
        body.lines()
            .map(str::trim_end)
            .filter(|l| !l.is_empty() && !l.starts_with('%') && !l.starts_with('#'))
            .take(40)
            .map(str::to_string)
            .collect()
    } else {
        fields.iter().map(|(k, v)| format!("{k}: {v}")).collect()
    };
    NetToolCard::new(server.to_string(), lines)
        .status(CardStatus::Ok)
        .raw(body.to_string())
}

/// Send one query and read the whole reply. WHOIS has no framing: the
/// server closes the connection when it is done, so reading to EOF is
/// the protocol.
async fn ask(
    server: &str,
    query: &str,
    deadline: tokio::time::Instant,
) -> Result<String, String> {
    let now = tokio::time::Instant::now();
    if deadline <= now {
        return Err(t("net_err_timeout").to_string());
    }
    let budget = deadline - now;
    let work = async {
        let mut stream = tokio::net::TcpStream::connect((server, 43u16))
            .await
            .map_err(|e| e.to_string())?;
        stream
            .write_all(format!("{query}\r\n").as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        let mut buf = Vec::new();
        tokio::io::AsyncReadExt::take(&mut stream, MAX_RESPONSE)
            .read_to_end(&mut buf)
            .await
            .map_err(|e| e.to_string())?;
        Ok::<String, String>(String::from_utf8_lossy(&buf).into_owned())
    };
    match tokio::time::timeout(budget, work).await {
        Ok(res) => res,
        Err(_) => Err(t("net_err_timeout").to_string()),
    }
}

/// The next server in the chain: IANA's `refer:` first, then a registry's
/// `Registrar WHOIS Server:`. Returns `None` at the end of the chain.
pub(crate) fn next_server(body: &str) -> Option<String> {
    extract_refer(body).or_else(|| extract_registrar_server(body))
}

/// IANA's referral line: `refer:        whois.verisign-grs.com`.
pub(crate) fn extract_refer(body: &str) -> Option<String> {
    value_for(body, "refer").filter(|v| is_hostname(v))
}

/// A registry's pointer at the registrar holding the real record:
/// `Registrar WHOIS Server: whois.example-registrar.com`.
pub(crate) fn extract_registrar_server(body: &str) -> Option<String> {
    value_for(body, "registrar whois server").filter(|v| is_hostname(v))
}

/// The first value for `key`, matched case-insensitively on the label
/// before the colon. WHOIS bodies use both `key: value` and
/// `key:          value`, and arrive with either line ending.
fn value_for(body: &str, key: &str) -> Option<String> {
    for line in body.lines() {
        let line = line.trim();
        let Some((label, value)) = line.split_once(':') else {
            continue;
        };
        if label.trim().eq_ignore_ascii_case(key) {
            let value = value.trim().trim_end_matches('.');
            if !value.is_empty() {
                return Some(value.to_ascii_lowercase());
            }
        }
    }
    None
}

/// Labels worth lifting out of the body, matched case-insensitively.
/// Several spellings per fact because registries never agreed on one:
/// `.com` says "Registry Expiry Date", `.org` says "Registry Expiry
/// Date" too, `.uk` says "Expiry date", RIPE says "created".
const INTERESTING: [&str; 12] = [
    "domain name",
    "registrar",
    "creation date",
    "created",
    "registered on",
    "updated date",
    "last updated",
    "registry expiry date",
    "expiry date",
    "expires on",
    "name server",
    "domain status",
];

/// Pull the recognizable fields out, keeping the body's own order and
/// allowing repeats (a domain has several name servers, and dropping all
/// but one would misreport its delegation).
pub(crate) fn extract_fields(body: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if line.starts_with('%') || line.starts_with('#') {
            continue;
        }
        let Some((label, value)) = line.split_once(':') else {
            continue;
        };
        let label = label.trim();
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        let lower = label.to_ascii_lowercase();
        if !INTERESTING.contains(&lower.as_str()) {
            continue;
        }
        // The same fact repeated verbatim (registries echo the registrar
        // block per contact) adds nothing; a second name server does.
        if out.iter().any(|(k, v)| k.eq_ignore_ascii_case(label) && v == value) {
            continue;
        }
        out.push((label.to_string(), value.to_string()));
    }
    out
}

/// Whether the server said the name is not registered. Every registry
/// phrases this differently, and all of them start the sentence with one
/// of these.
pub(crate) fn is_not_found(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    ["no match for", "not found", "no entries found", "no data found", "domain not found"]
        .iter()
        .any(|marker| lower.contains(marker))
}

/// A value is only followed if it is plausibly a host name: a referral is
/// an address the client is about to connect to, so a body that puts
/// prose after `refer:` must not send us anywhere.
fn is_hostname(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && value.contains('.')
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    const IANA_BODY: &str = "% IANA WHOIS server\r\n\
domain:       COM\r\n\
organisation: VeriSign Global Registry Services\r\n\
\r\n\
whois:        whois.verisign-grs.com\r\n\
refer:        whois.verisign-grs.com\r\n\
\r\n\
created:      1985-01-01\r\n";

    const VERISIGN_BODY: &str = "   Domain Name: EXAMPLE.COM\n\
   Registrar WHOIS Server: whois.example-registrar.com\n\
   Registrar: Example Registrar, Inc.\n\
   Creation Date: 1995-08-14T04:00:00Z\n\
   Registry Expiry Date: 2026-08-13T04:00:00Z\n\
   Name Server: A.IANA-SERVERS.NET\n\
   Name Server: B.IANA-SERVERS.NET\n\
   Domain Status: clientTransferProhibited\n";

    #[test]
    fn iana_referral_is_followed_first() {
        assert_eq!(extract_refer(IANA_BODY).as_deref(), Some("whois.verisign-grs.com"));
        assert_eq!(next_server(IANA_BODY).as_deref(), Some("whois.verisign-grs.com"));
    }

    #[test]
    fn registrar_server_is_the_second_hop() {
        // No `refer:` in a registry answer, so the chain continues
        // through the registrar pointer instead.
        assert!(extract_refer(VERISIGN_BODY).is_none());
        assert_eq!(
            next_server(VERISIGN_BODY).as_deref(),
            Some("whois.example-registrar.com")
        );
    }

    #[test]
    fn a_body_with_no_referral_ends_the_chain() {
        assert!(next_server("Domain Name: EXAMPLE.TEST\nRegistrar: Someone\n").is_none());
        assert!(next_server("").is_none());
    }

    #[test]
    fn prose_after_refer_is_not_a_server() {
        assert!(extract_refer("refer: see https://example.com/whois please").is_none());
        assert!(extract_refer("refer:\n").is_none());
    }

    #[test]
    fn fields_keep_order_and_repeats() {
        let fields = extract_fields(VERISIGN_BODY);
        let keys: Vec<&str> = fields.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys[0], "Domain Name");
        assert_eq!(
            fields.iter().filter(|(k, _)| k == "Name Server").count(),
            2,
            "both name servers survive"
        );
        assert!(fields.iter().any(|(k, v)| k == "Registry Expiry Date" && v.starts_with("2026")));
    }

    #[test]
    fn crlf_and_bare_lf_parse_the_same() {
        let crlf = "Registrar: Example\r\nCreation Date: 2020-01-01\r\n";
        let lf = "Registrar: Example\nCreation Date: 2020-01-01\n";
        assert_eq!(extract_fields(crlf), extract_fields(lf));
    }

    #[test]
    fn comment_lines_are_not_fields() {
        // IANA's own body opens with `%` comments that contain colons.
        let fields = extract_fields(IANA_BODY);
        assert!(fields.iter().all(|(k, _)| !k.starts_with('%')));
    }

    #[test]
    fn unregistered_names_are_recognized() {
        assert!(is_not_found("No match for \"NOPE.COM\"."));
        assert!(is_not_found("Domain not found."));
        assert!(is_not_found("NOT FOUND\n"));
        assert!(!is_not_found(VERISIGN_BODY));
    }
}
