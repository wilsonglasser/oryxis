//! Ping and traceroute: spoken natively where the OS allows it without
//! privileges, and through the system binary where it does not.
//!
//! The native path (`super::icmp`) comes first, because an SSH client
//! that ships as a single binary should not stop working because a
//! container image left `traceroute` out, which is exactly what happens
//! on a stock WSL install. It is a datagram ICMP socket on Linux and
//! `IcmpSendEcho2` on Windows; macOS, the BSDs and an IPv6 target on
//! Windows have no wired native path and go straight to the binary,
//! which is part of the base system on those platforms.
//!
//! The FALLBACK is not a lesser answer: when the native socket is
//! refused (`net.ipv4.ping_group_range` on a locked-down kernel) the
//! system binary is usually setuid and works, so the panel ends up
//! showing what the user would have seen in their terminal. On that
//! path the RAW OUTPUT is always shown alongside the summary, because
//! `ping` and `traceroute` phrase themselves differently across
//! iputils, BSD and Windows (which also translates its output) and a
//! summary that guessed wrong must never be the only thing on screen.
//! The native path has no raw output to show: the cards ARE the
//! reading, with nothing in between to disagree with.

use std::time::Duration;

use super::icmp::{self, Outcome, Unavailable};
use super::{CardStatus, NetToolCard};
use crate::i18n::t;

/// Echo requests per run: four is what every platform's default flag
/// count means to a person reading the output.
const COUNT: u8 = 4;
/// Wall-clock ceiling per run. Ping's own budget is bounded by `-c`;
/// traceroute's is not, so the longer one covers 20 hops timing out.
const PING_BUDGET: Duration = Duration::from_secs(25);
const TRACEROUTE_BUDGET: Duration = Duration::from_secs(90);
/// Hops a traceroute walks before giving up. The default is 30 or 64
/// depending on the implementation, which is a long wait for a path that
/// is not going to complete.
const MAX_HOPS: u8 = 20;

pub(crate) async fn probe_ping(target: &str) -> Result<Vec<NetToolCard>, String> {
    let host = super::host_of(target);
    if super::is_flag_like(host) {
        return Err(t("net_err_flag_target").to_string());
    }
    if let Some(cards) = native_ping(host).await {
        return Ok(cards);
    }
    let (program, args) = if cfg!(windows) {
        ("ping", vec!["-n".to_string(), COUNT.to_string(), host.to_string()])
    } else {
        ("ping", vec!["-c".to_string(), COUNT.to_string(), host.to_string()])
    };
    let output = match run_tool(program, &args, PING_BUDGET).await {
        Ok(o) => o,
        Err(card) => return Ok(vec![card]),
    };
    let mut cards = Vec::new();
    if let Some(summary) = parse_ping(&output) {
        let mut lines = vec![
            t("net_ping_summary")
                .replacen("{recv}", &summary.received.to_string(), 1)
                .replacen("{sent}", &summary.transmitted.to_string(), 1)
                .replacen("{loss}", &format!("{:.0}", summary.loss_pct), 1),
        ];
        if let Some((min, avg, max)) = summary.rtt_ms {
            lines.push(rtt_line(min, avg, max));
        }
        let status = match summary.received {
            0 => CardStatus::Bad,
            r if r < summary.transmitted => CardStatus::Warn,
            _ => CardStatus::Ok,
        };
        cards.push(NetToolCard::new(host.to_string(), lines).status(status).raw(output.clone()));
    }
    cards.push(raw_card(&output));
    Ok(cards)
}

pub(crate) async fn probe_traceroute(target: &str) -> Result<Vec<NetToolCard>, String> {
    let host = super::host_of(target);
    if super::is_flag_like(host) {
        return Err(t("net_err_flag_target").to_string());
    }
    if let Some(cards) = native_traceroute(host).await {
        return Ok(cards);
    }
    let (program, args) = if cfg!(windows) {
        (
            "tracert",
            vec![
                "-d".to_string(),
                "-h".to_string(),
                MAX_HOPS.to_string(),
                "-w".to_string(),
                "2000".to_string(),
                host.to_string(),
            ],
        )
    } else {
        (
            "traceroute",
            vec![
                "-n".to_string(),
                "-q".to_string(),
                "1".to_string(),
                "-w".to_string(),
                "2".to_string(),
                "-m".to_string(),
                MAX_HOPS.to_string(),
                host.to_string(),
            ],
        )
    };
    let output = match run_tool(program, &args, TRACEROUTE_BUDGET).await {
        Ok(o) => o,
        Err(card) => return Ok(vec![card]),
    };
    let mut cards = Vec::new();
    let hops = parse_traceroute(&output);
    if !hops.is_empty() {
        let unanswered = hops.iter().filter(|h| h.hosts.is_empty()).count();
        let lines: Vec<String> = hops.iter().map(Hop::render).collect();
        // Every hop silent means the path is invisible from here, which
        // is a finding; a few silent hops in the middle is ordinary
        // (plenty of routers simply do not answer).
        let status = if unanswered == hops.len() {
            CardStatus::Bad
        } else if unanswered > 0 {
            CardStatus::Warn
        } else {
            CardStatus::Ok
        };
        cards.push(
            NetToolCard::new(
                format!(
                    "{host}   {}",
                    t("net_trace_hops").replacen("{n}", &hops.len().to_string(), 1)
                ),
                lines,
            )
            .status(status)
            .raw(output.clone()),
        );
    }
    cards.push(raw_card(&output));
    Ok(cards)
}

/// Per-probe budget for the native path. A router that has not answered
/// in two seconds is not going to, and a traceroute pays this per hop.
const NATIVE_TIMEOUT: Duration = Duration::from_secs(2);

/// Ping over the native socket. `None` when that path is unavailable on
/// this machine, which is the caller's cue to shell out.
///
/// Runs on a blocking task: the socket is a blocking one on purpose (the
/// ICMP error a traceroute reads arrives on the socket's error queue,
/// which signals through `POLLERR`, and driving that through an async
/// readiness layer buys nothing for a probe that lasts a second).
async fn native_ping(host: &str) -> Option<Vec<NetToolCard>> {
    let ip = resolve_for_native(host).await?;
    let outcomes = match tokio::task::spawn_blocking(move || {
        icmp::ping(ip, COUNT, NATIVE_TIMEOUT)
    })
    .await
    {
        Ok(Ok(outcomes)) => outcomes,
        Ok(Err(reason)) => return native_declined(reason),
        // The blocking task panicked or was cancelled; the binary is a
        // better answer than nothing.
        Err(_) => return None,
    };

    let tally = tally_echoes(&outcomes);
    let mut lines = vec![
        t("net_ping_summary")
            .replacen("{recv}", &tally.received.to_string(), 1)
            .replacen("{sent}", &tally.sent.to_string(), 1)
            .replacen("{loss}", &format!("{:.0}", tally.loss_pct()), 1),
    ];
    let times: Vec<f32> = outcomes.iter().filter_map(Outcome::rtt_ms).collect();
    if !times.is_empty() {
        let min = times.iter().copied().fold(f32::INFINITY, f32::min);
        let max = times.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let avg = times.iter().sum::<f32>() / times.len() as f32;
        lines.push(rtt_line(min, avg, max));
    }
    // A reply from somewhere other than the target (an unreachable
    // reported by a router on the way) is the interesting half of a
    // failed ping, so it is named rather than counted as silence.
    for outcome in &outcomes {
        if let Outcome::Unreachable { from, .. } = outcome {
            lines.push(format!("{}: {from}", t("net_ping_unreachable")));
        }
    }
    Some(vec![NetToolCard::new(host.to_string(), lines).status(tally.status())])
}

/// What a run of echo probes amounts to.
///
/// Pure, and separate from the card, because the counting is the part
/// that can be WRONG in a way nobody notices: only an echo reply is the
/// target answering. An `Unreachable` ends the walk (that is
/// `Outcome::is_final`) but it is a router saying the target cannot be
/// had, so counting it as a reply would report "4 of 4 answered, 0%
/// lost" in green for a host that is not there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EchoTally {
    sent: u32,
    received: u32,
    /// Someone reported the target unreachable, which is a different
    /// failure from silence and gets the worst status even if another
    /// probe did come back.
    unreachable: bool,
}

impl EchoTally {
    fn loss_pct(&self) -> f32 {
        if self.sent == 0 {
            return 100.0;
        }
        (self.sent - self.received) as f32 / self.sent as f32 * 100.0
    }

    fn status(&self) -> CardStatus {
        if self.received == 0 || self.unreachable {
            CardStatus::Bad
        } else if self.received < self.sent {
            CardStatus::Warn
        } else {
            CardStatus::Ok
        }
    }
}

fn tally_echoes(outcomes: &[Outcome]) -> EchoTally {
    EchoTally {
        sent: outcomes.len() as u32,
        received: outcomes
            .iter()
            .filter(|o| matches!(o, Outcome::Reply { .. }))
            .count() as u32,
        unreachable: outcomes.iter().any(|o| matches!(o, Outcome::Unreachable { .. })),
    }
}

/// Traceroute over the native socket, same contract as [`native_ping`].
async fn native_traceroute(host: &str) -> Option<Vec<NetToolCard>> {
    let ip = resolve_for_native(host).await?;
    let outcomes = match tokio::task::spawn_blocking(move || {
        icmp::traceroute(ip, MAX_HOPS, NATIVE_TIMEOUT)
    })
    .await
    {
        Ok(Ok(outcomes)) => outcomes,
        Ok(Err(reason)) => return native_declined(reason),
        Err(_) => return None,
    };
    if outcomes.is_empty() {
        return None;
    }
    let silent = outcomes.iter().filter(|o| matches!(o, Outcome::Timeout)).count();
    // Arriving means the TARGET answered. A walk that ends on a router
    // reporting the target unreachable stopped for the opposite reason,
    // and reporting it green would say the path works.
    let reached = outcomes.last().is_some_and(|o| matches!(o, Outcome::Reply { .. }));
    let lines: Vec<String> = outcomes
        .iter()
        .enumerate()
        .map(|(i, outcome)| render_hop(i + 1, outcome))
        .collect();
    // Reaching the target is the good answer; a path that ran out of
    // hops, or one where nothing answered at all, is not.
    let status = if !reached || silent == outcomes.len() {
        CardStatus::Bad
    } else if silent > 0 {
        CardStatus::Warn
    } else {
        CardStatus::Ok
    };
    Some(vec![NetToolCard::new(
        format!(
            "{host}   {}",
            t("net_trace_hops").replacen("{n}", &outcomes.len().to_string(), 1)
        ),
        lines,
    )
    .status(status)])
}

/// One hop line: the number, who answered, and how long it took.
fn render_hop(index: usize, outcome: &Outcome) -> String {
    match (outcome.source(), outcome.rtt_ms()) {
        (Some(from), Some(rtt)) => {
            let unreachable = matches!(outcome, Outcome::Unreachable { .. });
            let suffix = if unreachable {
                format!("   {}", t("net_ping_unreachable"))
            } else {
                String::new()
            };
            format!("{index:>2}   {from}   {}{suffix}", one_rtt(rtt))
        }
        _ => format!("{index:>2}   *"),
    }
}

/// The summary's round-trip line, at a scale the numbers deserve.
/// Sub-millisecond times are ordinary on loopback and on a fast LAN, and
/// one decimal renders all three of them as "0.0 ms", which reads as a
/// broken measurement rather than a fast one.
fn rtt_line(min: f32, avg: f32, max: f32) -> String {
    if max < 10.0 {
        format!("{}: {min:.3} / {avg:.3} / {max:.3} ms", t("net_ping_rtt"))
    } else {
        format!("{}: {min:.1} / {avg:.1} / {max:.1} ms", t("net_ping_rtt"))
    }
}

/// One time, at the same scale rule.
fn one_rtt(rtt: f32) -> String {
    if rtt < 10.0 {
        format!("{rtt:.3} ms")
    } else {
        format!("{rtt:.1} ms")
    }
}

/// What to do when the native path declined. A platform with no wired
/// native path and a kernel that refused the socket both mean the same
/// thing here (fall back to the binary); a setup failure is reported,
/// because falling back would hide a bug behind a working command.
fn native_declined(reason: Unavailable) -> Option<Vec<NetToolCard>> {
    match reason {
        Unavailable::Platform | Unavailable::Denied => None,
        Unavailable::Failed(e) => {
            tracing::debug!(error = %e, "native ICMP probe unavailable, falling back");
            None
        }
    }
}

/// The address the native probe aims at. Resolved here rather than
/// inside the socket code so the whole native path takes an `IpAddr` and
/// the fallback keeps taking the name (the binary does its own lookup,
/// and a name that only IT can resolve should still work).
async fn resolve_for_native(host: &str) -> Option<std::net::IpAddr> {
    super::port::resolve_one(host).await.ok()
}

/// The tool's own output, verbatim. Always present, and always the thing
/// the copy action yields for these two tools.
fn raw_card(output: &str) -> NetToolCard {
    NetToolCard::new(
        t("net_raw_output").to_string(),
        output.lines().map(str::to_string).collect(),
    )
    .raw(output.to_string())
}

/// Run a system tool and return its combined output. The `Err` arm is a
/// finished card rather than a message, because "traceroute is not
/// installed" is an answer worth rendering like any other.
async fn run_tool(program: &str, args: &[String], budget: Duration) -> Result<String, NetToolCard> {
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args);
    #[cfg(windows)]
    {
        // No console window for the spawned tool: this is a GUI app, and
        // a flashing black box next to the panel is not the output.
        // `tokio::process::Command` carries `creation_flags` itself, so
        // the std extension trait is not imported here.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let child = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();
    let child = match child {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(NetToolCard::new(
                t("net_tool_missing_binary").to_string(),
                vec![
                    format!("{program}: {}", t("net_tool_missing_binary_desc")),
                    missing_hint(program).to_string(),
                ],
            )
            .status(CardStatus::Bad));
        }
        Err(e) => {
            return Err(NetToolCard::new(program.to_string(), vec![e.to_string()])
                .status(CardStatus::Bad));
        }
    };
    let out = match tokio::time::timeout(budget, child.wait_with_output()).await {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => {
            return Err(NetToolCard::new(program.to_string(), vec![e.to_string()])
                .status(CardStatus::Bad));
        }
        // The child is dropped here, which kills it: tokio's Command
        // defaults to kill_on_drop(false), so the explicit drop of the
        // future is what has to end the process. `wait_with_output`
        // consumed the child, so the timeout arm owns nothing to kill,
        // and the process ends when its pipes close with the future.
        Err(_) => {
            return Err(NetToolCard::new(
                program.to_string(),
                vec![format!("{} ({}s)", t("net_err_timeout"), budget.as_secs())],
            )
            .status(CardStatus::Warn));
        }
    };
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    let err = String::from_utf8_lossy(&out.stderr);
    if !err.trim().is_empty() {
        // Unreachable-host messages arrive on stderr in several
        // implementations, so dropping it would blank the card in
        // exactly the failing case the user is looking at.
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(err.trim_end());
    }
    Ok(text)
}

/// Where to get the missing binary. Named per tool because the packages
/// differ (and on Windows both ship with the OS, so the message there is
/// about PATH rather than an install).
fn missing_hint(program: &str) -> &'static str {
    if cfg!(windows) {
        return t("net_tool_missing_windows");
    }
    match program {
        "traceroute" => t("net_tool_missing_traceroute"),
        _ => t("net_tool_missing_ping"),
    }
}

/// What a ping run reported.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PingSummary {
    pub transmitted: u32,
    pub received: u32,
    pub loss_pct: f32,
    /// min / avg / max, when the tool printed a statistics line.
    pub rtt_ms: Option<(f32, f32, f32)>,
}

/// Read the statistics block. Handles iputils ("4 received"), BSD and
/// macOS ("4 packets received") and Windows (whose counters are
/// localized, so only the percentage and the numeric fields are read).
/// Returns `None` when nothing recognizable is there, which is what
/// keeps the raw card from being contradicted by a made-up summary.
pub(crate) fn parse_ping(output: &str) -> Option<PingSummary> {
    let mut summary: Option<PingSummary> = None;
    for line in output.lines() {
        let l = line.trim();
        if let Some(s) = parse_posix_stats(l) {
            summary = Some(s);
        } else if summary.is_none()
            && let Some(s) = parse_windows_stats(l)
        {
            summary = Some(s);
        }
        if let Some(rtt) = parse_rtt_line(l)
            && let Some(s) = summary.as_mut()
        {
            s.rtt_ms = Some(rtt);
        }
    }
    // Windows prints its timing block after the counters, on its own
    // localized line, so it is read in a second pass over the same text.
    if let Some(s) = summary.as_mut()
        && s.rtt_ms.is_none()
        && let Some(rtt) = parse_windows_rtt(output)
    {
        s.rtt_ms = Some(rtt);
    }
    summary
}

/// `4 packets transmitted, 4 received, 0% packet loss, time 3005ms` and
/// its BSD spelling `4 packets transmitted, 4 packets received, 0.0% packet loss`.
fn parse_posix_stats(line: &str) -> Option<PingSummary> {
    if !line.contains("transmitted") {
        return None;
    }
    let transmitted = number_before(line, "packets transmitted")
        .or_else(|| number_before(line, "transmitted"))?;
    let received = number_before(line, "packets received")
        .or_else(|| number_before(line, "received"))?;
    let loss_pct = percent_before(line, "packet loss").unwrap_or_else(|| {
        if transmitted == 0.0 {
            0.0
        } else {
            (transmitted - received) / transmitted * 100.0
        }
    });
    Some(PingSummary {
        transmitted: transmitted as u32,
        received: received as u32,
        loss_pct,
        rtt_ms: None,
    })
}

/// Windows: `Packets: Sent = 4, Received = 4, Lost = 0 (0% loss),`. The
/// words are translated on a localized install, the `= n` shape and the
/// `(n% ...)` are not, so the three numbers are read positionally from
/// the `=` assignments and the percentage from the parentheses.
fn parse_windows_stats(line: &str) -> Option<PingSummary> {
    if !line.contains('=') || !line.contains('%') {
        return None;
    }
    let numbers: Vec<f32> = line
        .split('=')
        .skip(1)
        .filter_map(|piece| {
            let digits: String =
                piece.trim_start().chars().take_while(char::is_ascii_digit).collect();
            digits.parse::<f32>().ok()
        })
        .collect();
    if numbers.len() < 3 {
        return None;
    }
    let loss_pct = line
        .split('(')
        .nth(1)
        .and_then(|rest| rest.split('%').next())
        .and_then(|n| n.trim().parse::<f32>().ok())?;
    Some(PingSummary {
        transmitted: numbers[0] as u32,
        received: numbers[1] as u32,
        loss_pct,
        rtt_ms: None,
    })
}

/// `rtt min/avg/max/mdev = 11.155/11.402/11.717/0.205 ms` (iputils) and
/// `round-trip min/avg/max/stddev = ...` (BSD).
fn parse_rtt_line(line: &str) -> Option<(f32, f32, f32)> {
    if !line.contains("min/avg/max") {
        return None;
    }
    let values = line.rsplit_once('=')?.1;
    let mut parts = values.trim().trim_end_matches("ms").trim().split('/');
    let min = parts.next()?.trim().parse().ok()?;
    let avg = parts.next()?.trim().parse().ok()?;
    let max = parts.next()?.trim().parse().ok()?;
    Some((min, avg, max))
}

/// Windows: `Minimum = 11ms, Maximum = 12ms, Average = 11ms`, in
/// whatever language the install speaks. Read positionally like the
/// counters, and only from a line whose numbers all carry `ms`, so the
/// counters line above cannot be mistaken for it.
fn parse_windows_rtt(output: &str) -> Option<(f32, f32, f32)> {
    for line in output.lines() {
        let l = line.trim();
        if l.matches("ms").count() < 3 || !l.contains('=') {
            continue;
        }
        let values: Vec<f32> = l
            .split('=')
            .skip(1)
            .filter_map(|piece| {
                let digits: String =
                    piece.trim_start().chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
                digits.parse::<f32>().ok()
            })
            .collect();
        if values.len() >= 3 {
            // Windows prints minimum, maximum, average, in that order.
            return Some((values[0], values[2], values[1]));
        }
    }
    None
}

/// The number immediately before `marker` on the line.
fn number_before(line: &str, marker: &str) -> Option<f32> {
    let head = line.split(marker).next()?;
    head.split_whitespace().next_back()?.parse().ok()
}

/// The percentage immediately before `marker`, tolerating both `0%` and
/// `0.0%`.
fn percent_before(line: &str, marker: &str) -> Option<f32> {
    let head = line.split(marker).next()?;
    head.split_whitespace()
        .next_back()?
        .trim_end_matches('%')
        .parse()
        .ok()
}

/// One traceroute hop.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Hop {
    pub index: u32,
    /// Addresses that answered. More than one when the probes at this
    /// distance took different paths, which load-balanced networks do.
    pub hosts: Vec<String>,
    pub times_ms: Vec<f32>,
}

impl Hop {
    fn render(&self) -> String {
        if self.hosts.is_empty() {
            return format!("{:>2}   *", self.index);
        }
        let times = if self.times_ms.is_empty() {
            String::new()
        } else {
            format!(
                "   {}",
                self.times_ms
                    .iter()
                    .map(|t| format!("{t:.1} ms"))
                    .collect::<Vec<_>>()
                    .join("  ")
            )
        };
        format!("{:>2}   {}{}", self.index, self.hosts.join(", "), times)
    }
}

/// Read hop lines from either shape:
///
/// - POSIX: `` 1  192.168.0.1  0.512 ms  0.480 ms `` (a silent hop is `*`)
/// - Windows: `  1     1 ms    <1 ms     1 ms  192.168.0.1`
///
/// Both begin with the hop number, which is what anchors the parse; a
/// line that does not is header or trailer and is skipped.
pub(crate) fn parse_traceroute(output: &str) -> Vec<Hop> {
    let mut hops = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        let mut tokens = trimmed.split_whitespace();
        let Some(index) = tokens.next().and_then(|t| t.parse::<u32>().ok()) else {
            continue;
        };
        let mut hosts: Vec<String> = Vec::new();
        let mut times_ms: Vec<f32> = Vec::new();
        let rest: Vec<&str> = tokens.collect();
        let mut i = 0;
        while i < rest.len() {
            let token = rest[i];
            if token == "*" {
                i += 1;
                continue;
            }
            // `1 ms` and `0.512 ms` (value and unit split), `<1` (a
            // Windows sub-millisecond reply), and `1ms` all mean a time.
            if let Some(v) = parse_time_token(token, rest.get(i + 1).copied()) {
                times_ms.push(v.0);
                i += v.1;
                continue;
            }
            // Anything left that looks like an address is one, possibly
            // parenthesized in `name (1.2.3.4)` form when the tool
            // resolved names. Prose is skipped rather than collected:
            // Windows writes `Request timed out.` on a silent hop, and
            // three English words in the address column would read as
            // three routers.
            let host = token.trim_matches(|c| c == '(' || c == ')');
            if looks_like_address(host) && !hosts.iter().any(|h| h == host) {
                hosts.push(host.to_string());
            }
            i += 1;
        }
        hops.push(Hop { index, hosts, times_ms });
    }
    hops
}

/// Whether a token is a router address rather than part of a sentence.
/// An IP literal always is; a name has to carry a dot and end in a label
/// that could be a TLD, which is what keeps `out.` (the tail of
/// `Request timed out.`) from being read as a host.
fn looks_like_address(token: &str) -> bool {
    let token = token.trim_end_matches('.');
    if token.is_empty() {
        return false;
    }
    if token.parse::<std::net::IpAddr>().is_ok() {
        return true;
    }
    if !token
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' || c == ':')
    {
        return false;
    }
    let mut labels = token.split('.');
    let Some(tld) = labels.next_back() else {
        return false;
    };
    // A single label is a bare word, not a host: traceroute never prints
    // an unqualified name in the address column.
    labels.next().is_some() && tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphanumeric())
}

/// A time token and how many tokens it consumed. `<1` is Windows for
/// "under a millisecond" and is read as zero rather than dropped, so the
/// hop keeps its answered/silent distinction.
fn parse_time_token(token: &str, next: Option<&str>) -> Option<(f32, usize)> {
    // `<1` is an upper bound, not a measurement: the hop answered in
    // less than the clock can resolve. It reads as 0, because rendering
    // it as 1 would claim a millisecond the router never spent.
    let (value, below_resolution) = match token.strip_prefix('<') {
        Some(rest) => (rest, true),
        None => (token, false),
    };
    let floor = |v: f32| if below_resolution { 0.0 } else { v };
    if let Some(stripped) = value.strip_suffix("ms")
        && let Ok(v) = stripped.parse::<f32>()
    {
        return Some((floor(v), 1));
    }
    let parsed: f32 = floor(value.parse().ok()?);
    // A bare number is only a time when `ms` follows it; otherwise it is
    // part of an address (or an AS number) and must not be eaten.
    if next == Some("ms") {
        return Some((parsed, 2));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::net::{IpAddr, Ipv4Addr};

    fn reply() -> Outcome {
        Outcome::Reply { from: IpAddr::V4(Ipv4Addr::LOCALHOST), rtt_ms: 1.0 }
    }

    fn unreachable() -> Outcome {
        Outcome::Unreachable { from: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), rtt_ms: 1.0 }
    }

    #[test]
    fn every_reply_is_a_clean_run() {
        let tally = tally_echoes(&[reply(), reply()]);
        assert_eq!((tally.sent, tally.received), (2, 2));
        assert_eq!(tally.loss_pct(), 0.0);
        assert_eq!(tally.status(), CardStatus::Ok);
    }

    #[test]
    fn silence_is_partial_loss() {
        let tally = tally_echoes(&[reply(), Outcome::Timeout]);
        assert_eq!(tally.received, 1);
        assert_eq!(tally.loss_pct(), 50.0);
        assert_eq!(tally.status(), CardStatus::Warn);
    }

    #[test]
    fn an_unreachable_report_is_not_an_answer() {
        // A router saying the target cannot be reached ENDS the probe,
        // but it is not the target replying: counting it as one reported
        // "answered, 0% lost" in green for a host that is not there.
        let tally = tally_echoes(&[unreachable(), unreachable()]);
        assert_eq!(tally.received, 0);
        assert_eq!(tally.loss_pct(), 100.0);
        assert_eq!(tally.status(), CardStatus::Bad);
    }

    #[test]
    fn one_unreachable_beside_a_reply_still_fails() {
        // Half a route answering and the other half being refused is a
        // broken path, not a slightly lossy one.
        let tally = tally_echoes(&[reply(), unreachable()]);
        assert_eq!(tally.received, 1);
        assert_eq!(tally.status(), CardStatus::Bad);
    }

    #[test]
    fn nothing_sent_is_total_loss_rather_than_a_divide_by_zero() {
        let tally = tally_echoes(&[]);
        assert_eq!(tally.loss_pct(), 100.0);
        assert_eq!(tally.status(), CardStatus::Bad);
    }


    const LINUX_PING: &str = "PING example.com (93.184.216.34) 56(84) bytes of data.\n\
64 bytes from 93.184.216.34: icmp_seq=1 ttl=54 time=11.2 ms\n\
\n--- example.com ping statistics ---\n\
4 packets transmitted, 4 received, 0% packet loss, time 3005ms\n\
rtt min/avg/max/mdev = 11.155/11.402/11.717/0.205 ms\n";

    const MACOS_PING: &str = "--- example.com ping statistics ---\n\
4 packets transmitted, 3 packets received, 25.0% packet loss\n\
round-trip min/avg/max/stddev = 11.155/11.402/11.717/0.205 ms\n";

    const WINDOWS_PING: &str = "Pinging example.com [93.184.216.34] with 32 bytes of data:\r\n\
Reply from 93.184.216.34: bytes=32 time=11ms TTL=54\r\n\
\r\nPing statistics for 93.184.216.34:\r\n\
    Packets: Sent = 4, Received = 4, Lost = 0 (0% loss),\r\n\
Approximate round trip times in milli-seconds:\r\n\
    Minimum = 11ms, Maximum = 13ms, Average = 12ms\r\n";

    const UNREACHABLE_PING: &str = "--- 10.0.0.1 ping statistics ---\n\
4 packets transmitted, 0 received, 100% packet loss, time 3068ms\n";

    #[test]
    fn linux_ping_stats() {
        let s = parse_ping(LINUX_PING).expect("summary");
        assert_eq!((s.transmitted, s.received), (4, 4));
        assert_eq!(s.loss_pct, 0.0);
        let (min, avg, max) = s.rtt_ms.expect("rtt");
        assert!((min - 11.155).abs() < 0.001);
        assert!((avg - 11.402).abs() < 0.001);
        assert!((max - 11.717).abs() < 0.001);
    }

    #[test]
    fn macos_ping_stats() {
        let s = parse_ping(MACOS_PING).expect("summary");
        assert_eq!((s.transmitted, s.received), (4, 3));
        assert_eq!(s.loss_pct, 25.0);
        assert!(s.rtt_ms.is_some());
    }

    #[test]
    fn windows_ping_stats() {
        let s = parse_ping(WINDOWS_PING).expect("summary");
        assert_eq!((s.transmitted, s.received), (4, 4));
        assert_eq!(s.loss_pct, 0.0);
        // Windows prints min, max, average; the summary reports
        // min / avg / max like every other platform.
        let (min, avg, max) = s.rtt_ms.expect("rtt");
        assert_eq!((min, avg, max), (11.0, 12.0, 13.0));
    }

    #[test]
    fn total_loss_is_reported_not_dropped() {
        let s = parse_ping(UNREACHABLE_PING).expect("summary");
        assert_eq!(s.received, 0);
        assert_eq!(s.loss_pct, 100.0);
        assert!(s.rtt_ms.is_none());
    }

    #[test]
    fn unparseable_output_yields_no_summary() {
        assert!(parse_ping("ping: unknown host nope.invalid").is_none());
        assert!(parse_ping("").is_none());
    }

    #[test]
    fn posix_traceroute_hops() {
        let out = "traceroute to example.com (93.184.216.34), 20 hops max\n\
 1  192.168.0.1  0.512 ms\n\
 2  * \n\
 3  93.184.216.34  11.402 ms\n";
        let hops = parse_traceroute(out);
        assert_eq!(hops.len(), 3);
        assert_eq!(hops[0].hosts, vec!["192.168.0.1"]);
        assert!((hops[0].times_ms[0] - 0.512).abs() < 0.001);
        assert!(hops[1].hosts.is_empty(), "a silent hop keeps its slot");
        assert_eq!(hops[2].hosts, vec!["93.184.216.34"]);
    }

    #[test]
    fn windows_traceroute_hops() {
        let out = "Tracing route to example.com [93.184.216.34]\r\n\
over a maximum of 20 hops:\r\n\r\n\
  1     1 ms    <1 ms     1 ms  192.168.0.1\r\n\
  2     *        *        *     Request timed out.\r\n\
  3    11 ms    12 ms    11 ms  93.184.216.34\r\n";
        let hops = parse_traceroute(out);
        assert_eq!(hops.len(), 3);
        assert_eq!(hops[0].hosts, vec!["192.168.0.1"]);
        assert_eq!(hops[0].times_ms, vec![1.0, 0.0, 1.0]);
        // "Request timed out." is prose, not three routers.
        assert!(hops[1].hosts.is_empty());
        assert_eq!(hops[2].times_ms.len(), 3);
        assert_eq!(hops[2].hosts, vec!["93.184.216.34"]);
    }

    #[test]
    fn a_hop_that_resolved_a_name_keeps_both() {
        let out = " 1  gw.example.com (192.168.0.1)  0.512 ms\n";
        let hops = parse_traceroute(out);
        assert_eq!(hops[0].hosts, vec!["gw.example.com", "192.168.0.1"]);
    }

    #[test]
    fn header_lines_are_not_hops() {
        let out = "traceroute to example.com (93.184.216.34), 20 hops max, 60 byte packets\n";
        assert!(parse_traceroute(out).is_empty());
    }
}
