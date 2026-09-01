//! TCP port reachability.
//!
//! A full connect, never a half-open SYN probe: raw sockets need
//! privileges the app deliberately does not ask for, and the question
//! being answered ("can I reach this service from here") is the one a
//! completed handshake answers exactly.

use std::net::IpAddr;
use std::time::{Duration, Instant};

use futures_util::StreamExt;

use super::{CardStatus, NetToolCard};
use crate::i18n::t;

/// Per-port connect budget. Long enough that a slow path across an
/// ocean still answers, short enough that a filtered port does not hold
/// the panel for the TCP stack's own multi-minute retry schedule.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// How many connects are in flight at once. Ordered output (`buffered`),
/// so the cards list ports the way the user typed them.
const CONCURRENCY: usize = 16;
/// Ceiling on one run. This is a diagnostic panel, not a port scanner:
/// the cap keeps a pasted `1-65535` from turning the app into something
/// a network owner would read as hostile.
pub(crate) const MAX_PORTS: usize = 64;

/// What one port answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PortVerdict {
    Open(u128),
    Refused,
    Filtered,
}

pub(crate) async fn probe(target: &str, ports: &str) -> Result<Vec<NetToolCard>, String> {
    let host = super::host_of(target);
    let ports = parse_ports(ports)?;
    let ip = resolve_one(host).await?;

    let results: Vec<(u16, PortVerdict)> = futures_util::stream::iter(ports.iter().copied())
        .map(|port| async move { (port, connect(ip, port).await) })
        .buffered(CONCURRENCY)
        .collect()
        .await;

    let mut cards = vec![NetToolCard::new(
        t("net_target").to_string(),
        vec![
            format!("{host} -> {ip}"),
            t("net_port_count").replacen("{n}", &ports.len().to_string(), 1),
        ],
    )];

    let open: Vec<String> = results
        .iter()
        .filter_map(|(p, v)| match v {
            PortVerdict::Open(ms) => Some(format!("{p}/tcp   {} ({ms} ms)", t("net_port_open"))),
            _ => None,
        })
        .collect();
    let shut: Vec<String> = results
        .iter()
        .filter_map(|(p, v)| match v {
            PortVerdict::Refused => Some(format!("{p}/tcp   {}", t("net_port_refused"))),
            PortVerdict::Filtered => Some(format!("{p}/tcp   {}", t("net_port_filtered"))),
            PortVerdict::Open(_) => None,
        })
        .collect();

    if open.is_empty() {
        cards.push(
            NetToolCard::new(t("net_port_open_title").to_string(), vec![t("net_port_none").to_string()])
                .status(CardStatus::Bad),
        );
    } else {
        cards.push(NetToolCard::new(t("net_port_open_title").to_string(), open).status(CardStatus::Ok));
    }
    if !shut.is_empty() {
        cards.push(NetToolCard::new(t("net_port_closed_title").to_string(), shut));
    }
    Ok(cards)
}

/// One connect attempt, timed. A refusal and a timeout are different
/// findings (a service that is down versus a firewall that swallows the
/// packet), so they stay distinct all the way to the card.
async fn connect(ip: IpAddr, port: u16) -> PortVerdict {
    let started = Instant::now();
    match tokio::time::timeout(CONNECT_TIMEOUT, tokio::net::TcpStream::connect((ip, port))).await {
        Ok(Ok(_stream)) => PortVerdict::Open(started.elapsed().as_millis()),
        Ok(Err(_)) => PortVerdict::Refused,
        Err(_) => PortVerdict::Filtered,
    }
}

/// The address the ports are tried against, resolved once for the whole
/// run so 64 connects do not each pay for their own lookup (and so every
/// card reports the SAME host, which a round-robin name would otherwise
/// spread across several servers).
pub(crate) async fn resolve_one(host: &str) -> Result<IpAddr, String> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(ip);
    }
    let resolver = super::dns::resolver()?;
    let lookup = resolver
        .lookup_ip(host)
        .await
        .map_err(|e| format!("{}: {e}", t("net_err_resolve_failed")))?;
    lookup
        .iter()
        .next()
        .ok_or_else(|| t("net_err_resolve_failed").to_string())
}

/// Parse the port field: comma-separated singles and `a-b` ranges, in any
/// mix. Returns them in the order typed, de-duplicated, capped at
/// [`MAX_PORTS`].
pub(crate) fn parse_ports(input: &str) -> Result<Vec<u16>, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err(t("net_err_no_ports").to_string());
    }
    let mut out: Vec<u16> = Vec::new();
    for piece in input.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        match piece.split_once('-') {
            Some((a, b)) => {
                let (a, b) = (parse_port(a.trim())?, parse_port(b.trim())?);
                // A reversed range is an obvious intent, so it is read
                // rather than refused: `443-80` scans 80 through 443.
                let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                for p in lo..=hi {
                    push_capped(&mut out, p)?;
                }
            }
            None => push_capped(&mut out, parse_port(piece)?)?,
        }
    }
    if out.is_empty() {
        return Err(t("net_err_no_ports").to_string());
    }
    Ok(out)
}

fn push_capped(out: &mut Vec<u16>, port: u16) -> Result<(), String> {
    if out.contains(&port) {
        return Ok(());
    }
    if out.len() >= MAX_PORTS {
        return Err(format!("{} ({MAX_PORTS})", t("net_err_too_many_ports")));
    }
    out.push(port);
    Ok(())
}

/// One port number. Port 0 is rejected: it means "any port" to bind() and
/// nothing at all to connect(), so accepting it would produce a card
/// about a question nobody asked.
fn parse_port(s: &str) -> Result<u16, String> {
    match s.parse::<u16>() {
        Ok(0) | Err(_) => Err(format!("{}: {s}", t("net_err_bad_port"))),
        Ok(p) => Ok(p),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn singles_and_ranges_mix() {
        assert_eq!(parse_ports("22").unwrap(), vec![22]);
        assert_eq!(parse_ports("22, 80,443").unwrap(), vec![22, 80, 443]);
        assert_eq!(parse_ports("80-83").unwrap(), vec![80, 81, 82, 83]);
        assert_eq!(parse_ports("22,80-82").unwrap(), vec![22, 80, 81, 82]);
    }

    #[test]
    fn reversed_range_is_read_not_refused() {
        assert_eq!(parse_ports("83-80").unwrap(), vec![80, 81, 82, 83]);
    }

    #[test]
    fn duplicates_collapse_and_order_is_kept() {
        assert_eq!(parse_ports("443,22,443,22").unwrap(), vec![443, 22]);
        // The overlap is dropped, not counted twice against the cap.
        assert_eq!(parse_ports("80-82,81-83").unwrap(), vec![80, 81, 82, 83]);
    }

    #[test]
    fn garbage_and_out_of_range_are_named() {
        assert!(parse_ports("http").is_err());
        assert!(parse_ports("65536").is_err());
        assert!(parse_ports("0").is_err());
        assert!(parse_ports("22,,80").is_ok(), "empty pieces are skipped");
        assert!(parse_ports("").is_err());
        assert!(parse_ports("   ").is_err());
    }

    #[test]
    fn the_cap_holds() {
        let ok = parse_ports("1-64").unwrap();
        assert_eq!(ok.len(), MAX_PORTS);
        assert!(parse_ports("1-65").is_err());
        assert!(parse_ports("1-65535").is_err());
    }
}
