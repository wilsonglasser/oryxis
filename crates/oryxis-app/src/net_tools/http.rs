//! HTTP / HTTPS reachability, the redirect chain, and the certificate.
//!
//! Redirects are followed BY HAND (`Policy::none()` plus a loop) rather
//! than by reqwest, because the chain is the answer here: "it returns
//! 200" hides the http -> https -> www hop that is usually what broke.
//! The chain is also where a redirect loop shows itself, which an
//! automatic follower reports only as a generic error.

use std::error::Error as _;
use std::time::{Duration, Instant};

use super::{CardStatus, NetToolCard};
use crate::i18n::t;

/// Per-request budget. The panel is diagnosing a server that may be
/// hanging, so this has to end well before the user assumes the app did.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// Redirect hops followed. Browsers stop around 20; a chain longer than
/// this is a loop, and reporting it as one is more useful than walking
/// it to the end.
const MAX_HOPS: usize = 10;
/// Response headers worth a line of their own, lower-cased for matching.
/// The rest are still in the raw copy, this is what the card leads with.
const INTERESTING_HEADERS: [&str; 7] = [
    "server",
    "content-type",
    "content-length",
    "location",
    "strict-transport-security",
    "cache-control",
    "x-powered-by",
];

pub(crate) async fn probe(target: &str) -> Result<Vec<NetToolCard>, String> {
    let url = normalize_url(target)?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(REQUEST_TIMEOUT)
        .user_agent(concat!("Oryxis/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;

    let mut cards = Vec::new();
    let mut chain: Vec<String> = Vec::new();
    let mut current = url.clone();
    let mut tls_error: Option<String> = None;
    let mut final_response: Option<(reqwest::Response, u128)> = None;

    for hop in 0..=MAX_HOPS {
        let started = Instant::now();
        // HEAD first: it is the polite way to ask, and it is exactly
        // what a health check does. Servers that mishandle it (405, 501,
        // or a bare error) get the GET they understand.
        let mut response = client.head(current.clone()).send().await;
        if response.as_ref().is_ok_and(|r| {
            matches!(r.status().as_u16(), 400 | 403 | 404 | 405 | 501)
        }) || response.is_err()
        {
            let via_get = client.get(current.clone()).send().await;
            if via_get.is_ok() || response.is_err() {
                response = via_get;
            }
        }
        let elapsed = started.elapsed().as_millis();
        let response = match response {
            Ok(r) => r,
            Err(e) => {
                let msg = describe_error(&e);
                if is_tls_error(&e) {
                    tls_error = Some(msg.clone());
                }
                cards.push(
                    NetToolCard::new(current.to_string(), vec![msg]).status(CardStatus::Bad),
                );
                break;
            }
        };
        let status = response.status();
        if status.is_redirection()
            && let Some(location) = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        {
            chain.push(format!(
                "{} {}   -> {location}   ({elapsed} ms)",
                status.as_u16(),
                current
            ));
            // A relative Location resolves against the URL it came from,
            // which is why the base is joined rather than parsed alone.
            match current.join(&location) {
                Ok(next) if hop < MAX_HOPS => {
                    current = next;
                    continue;
                }
                Ok(_) => {
                    cards.push(
                        NetToolCard::new(
                            t("net_http_redirects").to_string(),
                            vec![t("net_http_too_many_redirects").to_string()],
                        )
                        .status(CardStatus::Bad),
                    );
                    break;
                }
                Err(e) => {
                    chain.push(format!("{}: {e}", t("net_http_bad_location")));
                    break;
                }
            }
        }
        final_response = Some((response, elapsed));
        break;
    }

    if !chain.is_empty() {
        cards.insert(
            0,
            NetToolCard::new(t("net_http_redirects").to_string(), chain)
                .status(CardStatus::Warn),
        );
    }

    if let Some((response, elapsed)) = final_response {
        let status = response.status();
        let card_status = match status.as_u16() {
            200..=299 => CardStatus::Ok,
            300..=399 => CardStatus::Warn,
            _ => CardStatus::Bad,
        };
        let mut lines = vec![
            format!("{} {}", status.as_u16(), status.canonical_reason().unwrap_or("")),
            format!("{}   {elapsed} ms", response.url()),
            format!("{:?}", response.version()),
        ];
        lines.extend(header_lines(response.headers()));
        cards.insert(
            0,
            NetToolCard::new(t("net_http_response").to_string(), lines).status(card_status),
        );
    }

    // The certificate is inspected for any https target, including one
    // whose request just failed: a chain the trust store rejects is the
    // most likely reason it failed, and showing it is the point.
    if current.scheme() == "https"
        && let Some(host) = current.host_str()
    {
        let port = current.port().unwrap_or(443);
        cards.push(certificate_card(host, port, tls_error).await);
    }
    Ok(cards)
}

/// The certificate card, with the trust verdict coming from whether the
/// ordinary request succeeded rather than from the inspecting handshake
/// (which trusts everything by construction; see `net_tools::tls`).
async fn certificate_card(host: &str, port: u16, tls_error: Option<String>) -> NetToolCard {
    let summary = match super::tls::inspect(host, port).await {
        Ok(s) => s,
        Err(e) => {
            return NetToolCard::new(t("net_http_certificate").to_string(), vec![e])
                .status(CardStatus::Bad);
        }
    };
    let mut lines = vec![
        format!("{}: {}", t("net_cert_subject"), summary.subject),
        format!("{}: {}", t("net_cert_issuer"), summary.issuer),
        format!("{}: {}", t("net_cert_valid_from"), summary.not_before),
        format!("{}: {}", t("net_cert_valid_until"), summary.not_after),
    ];
    lines.push(if summary.expired() {
        t("net_cert_expired_ago").replacen("{n}", &(-summary.days_left).to_string(), 1)
    } else {
        t("net_cert_expires_in").replacen("{n}", &summary.days_left.to_string(), 1)
    });
    if summary.chain_len == 1 {
        // No intermediate presented. Browsers often paper over this by
        // fetching the issuer themselves; many clients (and every
        // command-line tool) do not.
        lines.push(t("net_cert_chain_incomplete").to_string());
    } else {
        lines.push(format!("{}: {}", t("net_cert_chain"), summary.chain_len));
    }
    if !summary.sans.is_empty() {
        lines.push(format!("{}: {}", t("net_cert_sans"), summary.sans.join(", ")));
    }
    let status = match &tls_error {
        Some(e) => {
            lines.push(format!("{}: {e}", t("net_cert_untrusted")));
            CardStatus::Bad
        }
        None if summary.expired() => CardStatus::Bad,
        // A month is the usual "renew now" threshold, and the usual
        // amount of notice an operator wants.
        None if summary.days_left < 30 => CardStatus::Warn,
        None => CardStatus::Ok,
    };
    NetToolCard::new(t("net_http_certificate").to_string(), lines).status(status)
}

/// The headers the card leads with, in the order [`INTERESTING_HEADERS`]
/// lists them so two runs against different servers line up.
fn header_lines(headers: &reqwest::header::HeaderMap) -> Vec<String> {
    INTERESTING_HEADERS
        .iter()
        .filter_map(|name| {
            let value = headers.get(*name)?;
            Some(format!("{name}: {}", value.to_str().unwrap_or("<binary>")))
        })
        .collect()
}

/// Give the target a scheme when the user typed a bare host, which is
/// how everyone types a URL. `https` rather than `http`: it is what a
/// browser tries first, and the certificate card is half the tool.
pub(crate) fn normalize_url(target: &str) -> Result<reqwest::Url, String> {
    let target = target.trim();
    if target.is_empty() {
        return Err(t("net_err_no_target").to_string());
    }
    let candidate = if target.contains("://") {
        target.to_string()
    } else {
        format!("https://{target}")
    };
    let url = reqwest::Url::parse(&candidate)
        .map_err(|e| format!("{}: {e}", t("net_err_bad_url")))?;
    if !matches!(url.scheme(), "http" | "https") {
        // Any other scheme would send the request somewhere reqwest
        // cannot go, and reporting that as a failed HTTP check would
        // blame the server for the user's typo.
        return Err(format!("{}: {}", t("net_err_bad_scheme"), url.scheme()));
    }
    if url.host_str().is_none() {
        return Err(t("net_err_bad_url").to_string());
    }
    Ok(url)
}

/// Whether the request failed inside TLS. reqwest wraps the rustls error
/// several layers down, so the chain is walked and matched on text: the
/// verdict only decides how the certificate card is LABELLED, never
/// whether a connection is made.
fn is_tls_error(e: &reqwest::Error) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(e);
    while let Some(err) = source {
        let text = err.to_string().to_ascii_lowercase();
        if text.contains("certificate")
            || text.contains("tls")
            || text.contains("handshake")
            || text.contains("ssl")
        {
            return true;
        }
        source = err.source();
    }
    false
}

/// Flatten a reqwest error into one line, keeping the innermost cause:
/// the outer message is usually just "error sending request".
fn describe_error(e: &reqwest::Error) -> String {
    let mut parts = vec![e.to_string()];
    let mut source = e.source();
    while let Some(err) = source {
        parts.push(err.to_string());
        source = err.source();
    }
    parts.dedup();
    parts.join(": ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_host_becomes_https() {
        assert_eq!(normalize_url("example.com").unwrap().as_str(), "https://example.com/");
        assert_eq!(
            normalize_url("example.com:8443/health").unwrap().as_str(),
            "https://example.com:8443/health"
        );
    }

    #[test]
    fn an_explicit_scheme_is_kept() {
        assert_eq!(normalize_url("http://example.com").unwrap().scheme(), "http");
        assert_eq!(
            normalize_url("https://example.com/a?b=c").unwrap().as_str(),
            "https://example.com/a?b=c"
        );
    }

    #[test]
    fn other_schemes_are_refused_by_name() {
        let err = normalize_url("ftp://example.com").unwrap_err();
        assert!(err.contains("ftp"), "{err}");
        assert!(normalize_url("ssh://example.com").is_err());
        assert!(normalize_url("").is_err());
    }

    #[test]
    fn a_relative_location_resolves_against_its_hop() {
        // The join the redirect loop performs, which is what makes
        // `Location: /login` work.
        let base = normalize_url("https://example.com/app/page").unwrap();
        assert_eq!(base.join("/login").unwrap().as_str(), "https://example.com/login");
        assert_eq!(base.join("next").unwrap().as_str(), "https://example.com/app/next");
        assert_eq!(
            base.join("https://other.example/x").unwrap().as_str(),
            "https://other.example/x"
        );
    }
}
