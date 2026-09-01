//! DNS lookups for the network tools panel.
//!
//! The system resolver behind `getaddrinfo` answers A / AAAA and nothing
//! else, which is exactly the half of a DNS problem that is never the
//! interesting one. This asks the host's own configured resolvers
//! (`/etc/resolv.conf`, the Windows registry) for each record type in
//! turn and reports every answer, including the "no records" ones: a
//! domain with no MX is a finding, not an absence of output.

use std::net::IpAddr;

use hickory_resolver::TokioResolver;
use hickory_resolver::proto::rr::RecordType;

use super::{CardStatus, NetToolCard};
use crate::i18n::t;

/// Record types queried for a name target, in the order the cards show.
const NAME_TYPES: [RecordType; 6] = [
    RecordType::A,
    RecordType::AAAA,
    RecordType::CNAME,
    RecordType::MX,
    RecordType::NS,
    RecordType::TXT,
];

/// A resolver built from the machine's own DNS configuration. Built per
/// run rather than cached: the panel is used precisely while the network
/// is being changed, and a resolver holding the nameservers from before
/// the VPN came up would answer the question the user is trying to
/// escape.
pub(crate) fn resolver() -> Result<TokioResolver, String> {
    TokioResolver::builder_tokio()
        .map_err(|e| format!("{}: {e}", t("net_err_resolver")))?
        .build()
        .map_err(|e| format!("{}: {e}", t("net_err_resolver")))
}

pub(crate) async fn probe(target: &str) -> Result<Vec<NetToolCard>, String> {
    let host = super::host_of(target);
    let resolver = resolver()?;
    let mut cards = Vec::new();

    // An IP target has exactly one DNS question worth asking, and it is
    // the reverse one. Querying A for "1.2.3.4" would ask about a name
    // that only exists as a typo.
    if let Ok(ip) = host.parse::<IpAddr>() {
        let name = reverse_ptr_name(ip);
        cards.push(lookup_card(&resolver, &name, RecordType::PTR, "PTR").await);
        return Ok(cards);
    }

    for rtype in NAME_TYPES {
        cards.push(lookup_card(&resolver, host, rtype, &rtype.to_string()).await);
    }
    // SOA lives on the zone, so a lookup for `www.example.com` normally
    // answers from the authority section rather than as an answer. Asked
    // last because it is the least often the reason someone opened this.
    cards.push(soa_card(&resolver, host).await);
    // SPF is a TXT record by convention, and the one TXT record people
    // actually come looking for; promoting it out of the TXT pile is the
    // whole reason it gets a card.
    if let Some(card) = spf_card(&resolver, host).await {
        cards.push(card);
    }
    Ok(cards)
}

/// One record type, rendered as a card. Every outcome is a card: answers,
/// an empty set, NXDOMAIN, or the resolver failing to answer at all.
async fn lookup_card(
    resolver: &TokioResolver,
    name: &str,
    rtype: RecordType,
    title: &str,
) -> NetToolCard {
    match resolver.lookup(name, rtype).await {
        Ok(lookup) => {
            let lines: Vec<String> = lookup
                .answers()
                .iter()
                .filter(|r| r.record_type() == rtype)
                .map(|r| format!("{}   ttl {}s", r.data, r.ttl))
                .collect();
            if lines.is_empty() {
                NetToolCard::new(title.to_string(), vec![t("net_dns_no_records").to_string()])
            } else {
                NetToolCard::new(title.to_string(), lines).status(CardStatus::Ok)
            }
        }
        Err(e) if e.is_nx_domain() => {
            NetToolCard::new(title.to_string(), vec![t("net_dns_nxdomain").to_string()])
                .status(CardStatus::Bad)
        }
        Err(e) if e.is_no_records_found() => {
            NetToolCard::new(title.to_string(), vec![t("net_dns_no_records").to_string()])
        }
        Err(e) => NetToolCard::new(title.to_string(), vec![e.to_string()]).status(CardStatus::Bad),
    }
}

/// SOA, read from wherever the resolver put it. A query for a name below
/// the zone apex answers with the SOA in the AUTHORITY section (that is
/// how a negative answer proves itself), so a card that only read
/// `answers()` would report "no SOA" for every `www.` name on earth.
async fn soa_card(resolver: &TokioResolver, host: &str) -> NetToolCard {
    let title = "SOA".to_string();
    match resolver.lookup(host, RecordType::SOA).await {
        Ok(lookup) => {
            let lines: Vec<String> = lookup
                .answers()
                .iter()
                .chain(lookup.authorities())
                .filter(|r| r.record_type() == RecordType::SOA)
                .map(|r| format!("{}   {}   ttl {}s", r.name, r.data, r.ttl))
                .collect();
            if lines.is_empty() {
                NetToolCard::new(title, vec![t("net_dns_no_records").to_string()])
            } else {
                NetToolCard::new(title, lines).status(CardStatus::Ok)
            }
        }
        Err(e) => {
            // The SOA that proves a negative answer rides the error type
            // here, which is why this arm digs it out instead of
            // reporting a failure the way `lookup_card` does.
            if let Some(soa) = e.into_soa() {
                return NetToolCard::new(
                    title,
                    vec![format!("{}   {}   ttl {}s", soa.name, soa.data, soa.ttl)],
                )
                .status(CardStatus::Ok);
            }
            NetToolCard::new(title, vec![t("net_dns_no_records").to_string()])
        }
    }
}

/// The SPF policy, extracted from TXT. `None` when the domain publishes
/// none, so no card is added: an empty SPF card would read as a finding
/// on the many domains that legitimately send no mail.
async fn spf_card(resolver: &TokioResolver, host: &str) -> Option<NetToolCard> {
    let lookup = resolver.lookup(host, RecordType::TXT).await.ok()?;
    let lines: Vec<String> = lookup
        .answers()
        .iter()
        .filter(|r| r.record_type() == RecordType::TXT)
        .map(|r| unquote_txt(&r.data.to_string()))
        .filter(|s| is_spf(s))
        .collect();
    if lines.is_empty() {
        return None;
    }
    // More than one SPF record is a misconfiguration RFC 7208 calls a
    // permerror, so the card says so rather than quietly listing both.
    let status = if lines.len() > 1 { CardStatus::Warn } else { CardStatus::Ok };
    let mut card = NetToolCard::new("SPF".to_string(), lines).status(status);
    if status == CardStatus::Warn {
        card.lines.push(t("net_dns_spf_multiple").to_string());
    }
    Some(card)
}

/// The reverse-lookup name for an address: `1.2.3.4` becomes
/// `4.3.2.1.in-addr.arpa.`, and an IPv6 address becomes its 32 nibbles in
/// reverse under `ip6.arpa.` (RFC 3596). Pure, so the shape is tested
/// without a resolver.
pub(crate) fn reverse_ptr_name(ip: IpAddr) -> String {
    let suffix = match ip {
        IpAddr::V4(_) => "in-addr.arpa.",
        IpAddr::V6(_) => "ip6.arpa.",
    };
    format!("{}.{suffix}", reversed_labels(ip))
}

/// An address as reversed DNS labels, with no zone suffix:
/// `1.2.3.4` -> `4.3.2.1`, and an IPv6 address -> its 32 nibbles in
/// reverse. Shared with the DNSBL lookups, which append their own zone
/// to exactly this (that is what a blocklist query IS), so the two
/// cannot disagree about nibble order.
pub(crate) fn reversed_labels(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            format!("{}.{}.{}.{}", o[3], o[2], o[1], o[0])
        }
        IpAddr::V6(v6) => {
            let mut out = String::with_capacity(64);
            for byte in v6.octets().iter().rev() {
                // Low nibble first: the name is the address read
                // backwards one nibble at a time.
                out.push_str(&format!("{:x}.{:x}.", byte & 0x0f, byte >> 4));
            }
            out.pop();
            out
        }
    }
}

/// Strip the quoting hickory renders TXT rdata with, so a policy shows as
/// the string the zone published. Long TXT records arrive as several
/// quoted chunks that concatenate with no separator (RFC 1035 character
/// strings), which is why this joins rather than takes the first.
pub(crate) fn unquote_txt(rendered: &str) -> String {
    if !rendered.contains('"') {
        return rendered.to_string();
    }
    let mut out = String::with_capacity(rendered.len());
    let mut inside = false;
    for ch in rendered.chars() {
        match ch {
            '"' => inside = !inside,
            c if inside => out.push(c),
            _ => {}
        }
    }
    out
}

/// Whether a TXT string is an SPF policy. Case-insensitive on the version
/// tag because the RFC is, and a policy written `V=SPF1` still governs the
/// domain.
pub(crate) fn is_spf(txt: &str) -> bool {
    let t = txt.trim_start();
    t.len() >= 6 && t[..6].eq_ignore_ascii_case("v=spf1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv4_reverse_name() {
        assert_eq!(
            reverse_ptr_name("1.2.3.4".parse().unwrap()),
            "4.3.2.1.in-addr.arpa."
        );
        assert_eq!(
            reverse_ptr_name("8.8.8.8".parse().unwrap()),
            "8.8.8.8.in-addr.arpa."
        );
    }

    #[test]
    fn ipv6_reverse_name_is_nibbles_backwards() {
        // RFC 3596's own example shape: 32 nibbles, least significant
        // first, under ip6.arpa.
        let name = reverse_ptr_name("2001:db8::1".parse().unwrap());
        assert!(name.ends_with("ip6.arpa."));
        assert!(name.starts_with("1.0.0.0."));
        let labels = name.trim_end_matches("ip6.arpa.").trim_end_matches('.');
        assert_eq!(labels.split('.').count(), 32);
    }

    #[test]
    fn txt_unquoting_joins_chunks() {
        assert_eq!(unquote_txt("\"v=spf1 -all\""), "v=spf1 -all");
        assert_eq!(unquote_txt("\"part one \" \"part two\""), "part one part two");
        assert_eq!(unquote_txt("no quotes here"), "no quotes here");
    }

    #[test]
    fn spf_detection_ignores_case_and_leading_space() {
        assert!(is_spf("v=spf1 include:_spf.example.com ~all"));
        assert!(is_spf("  V=SPF1 -all"));
        assert!(!is_spf("v=DMARC1; p=none"));
        assert!(!is_spf("v=spf"));
        assert!(!is_spf(""));
    }
}
