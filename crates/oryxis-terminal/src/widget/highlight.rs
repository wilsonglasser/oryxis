use super::*;

#[derive(Clone, Copy, PartialEq)]
enum HighlightKind {
    Url,
    Ip,
    Path,
    Number,
    /// `user@host` prompt token (`root@web`). Detected only when Privacy
    /// Mode is on; never colored (see [`highlight_color_at`]), it exists
    /// solely so the draw pass can mask it. Also catches emails / typed
    /// `ssh user@host` targets, which are sensitive too.
    HostUser,
    /// The `<name>` segment of a home-directory path (`C:\Users\<name>`,
    /// `/home/<name>`, `/Users/<name>`). It identifies the local account
    /// just like a prompt `user@host` does, so Privacy Mode masks it.
    /// Same contract as [`HighlightKind::HostUser`]: privacy-only, never
    /// colored.
    UserDir,
    /// An exact occurrence of a saved connection's hostname (passed in by
    /// the app as a privacy term). Plain DNS names have no detectable
    /// shape (file extensions collide with ccTLDs: `main.rs`,
    /// `install.sh` are FQDN-shaped), so the known values are matched
    /// literally instead. Privacy-only, never colored.
    KnownHost,
    /// An address-shaped token whose privacy class is DISABLED: colors
    /// exactly like [`HighlightKind::Ip`] but is excluded from Privacy
    /// Mode masking. (The name is historical: issue #53 once exempted
    /// version-shaped quads from masking; that heuristic was removed
    /// 2026-07-19 because hostile output could abuse it to display a
    /// real IP unmasked, and only the class-off demotion remains.)
    VersionQuad,
    /// A user-defined highlight rule's match. Produced by
    /// [`detect_rule_highlights`], which the draw pass keeps in a list of
    /// its own so an explicit rule always wins over the automatic
    /// detectors and neither pass perturbs the other's overlap math.
    Rule,
}

impl HighlightKind {
    /// Privacy-Mode-only markers: masked by the draw pass, never used as
    /// a keyword-highlight color.
    fn privacy_only(self) -> bool {
        matches!(self, Self::HostUser | Self::UserDir | Self::KnownHost)
    }
}

/// Whether a hex-digit/colon run is IPv6-shaped: the full 8-group form
/// (exactly 7 colons) or the `::`-compressed form, groups of 1-4 hex
/// digits, at most one `::`, no `:::`. A run without `::` and without the
/// full form's 7 colons is rejected, which keeps timestamps (`12:34:56`)
/// and MAC addresses (`aa:bb:cc:dd:ee:ff`) out. Shared by the terminal
/// highlighter and the app-side session-log redaction so both agree on
/// what gets masked; callers are responsible for context (a run glued to
/// a word, like `std::io`, is theirs to reject).
pub fn looks_like_ipv6(run: &str) -> bool {
    let bytes = run.as_bytes();
    if bytes.is_empty() || !bytes.iter().all(|b| b.is_ascii_hexdigit() || *b == b':') {
        return false;
    }
    if run.contains(":::") || run.matches("::").count() > 1 {
        return false;
    }
    let groups: Vec<&str> = run.split(':').filter(|g| !g.is_empty()).collect();
    if groups.is_empty() || groups.iter().any(|g| g.len() > 4) {
        return false;
    }
    if run.contains("::") {
        // `::` stands for at least one zero group, so at most 7 explicit
        // groups remain; a single leading/trailing colon that isn't part
        // of the `::` is malformed.
        groups.len() <= 7
            && (!run.starts_with(':') || run.starts_with("::"))
            && (!run.ends_with(':') || run.ends_with("::"))
    } else {
        bytes.iter().filter(|b| **b == b':').count() == 7 && groups.len() == 8
    }
}

/// Byte spans (`start..end`, end exclusive) of range-valid IPv4-shaped
/// tokens in a row: exactly 4 dot-separated groups of 1-3 digits, each
/// `<= 255`, not glued to an alphanumeric or `.` on either side. This is
/// the syntactic candidate set of the IPv4 highlight pass; whether a
/// candidate masks as an address or stays readable as a version string is
/// decided per candidate by the version classifier and its overrides.
pub fn scan_quad_dot_candidates(row: &str) -> Vec<(usize, usize)> {
    let bytes = row.as_bytes();
    let len = bytes.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i < len {
        if bytes[i].is_ascii_digit() {
            if i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'.') {
                i += 1;
                continue;
            }
            let start = i;
            let mut groups = 0u8;
            let mut j = i;
            loop {
                let group_start = j;
                while j < len && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                let group_len = j - group_start;
                if group_len == 0 || group_len > 3 {
                    break;
                }
                if let Ok(val) = row[group_start..j].parse::<u16>() {
                    if val > 255 { break; }
                } else {
                    break;
                }
                groups += 1;
                if groups == 4 { break; }
                if j < len && bytes[j] == b'.' {
                    j += 1;
                } else {
                    break;
                }
            }
            if groups == 4 {
                if j < len && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'.') {
                    i += 1;
                    continue;
                }
                out.push((start, j));
                i = j;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Whether an IPv4 candidate sits in private/loopback/link-local space
/// (10/8, 127/8, 169.254/16, 172.16/12, 192.168/16). Privacy Mode always
/// masks these even in a version-like row: a version string colliding
/// with RFC1918 space is rare and masking is the safe error.
pub fn ipv4_is_private_or_loopback(candidate: &str) -> bool {
    let mut octets = candidate.split('.').map(|g| g.parse::<u8>().ok());
    let (Some(Some(a)), Some(Some(b))) = (octets.next(), octets.next()) else {
        return false;
    };
    a == 10
        || a == 127
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
}

pub(crate) struct Highlight {
    row: u16,
    start_col: u16,
    end_col: u16, // inclusive
    color: Color,
    kind: HighlightKind,
}

/// Per-class Privacy Mode gates (issue #78): each `false` stops that
/// class from MASKING. Detection still runs for address spans (they
/// double as keyword highlights), a disabled class just classifies
/// them as the never-masked keyword kind instead of `Ip`. The
/// saved-hostname / saved-username classes have no flag here: those
/// are literal terms the app filters out of `privacy_terms` before
/// they reach this crate. All on by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrivacyClasses {
    /// Public IP addresses (v4 + v6).
    pub public_ips: bool,
    /// Private / loopback / link-local addresses (10/8, 127/8,
    /// 169.254/16, 172.16/12, 192.168/16, `::1`, `fe80::`, ULA).
    pub private_ips: bool,
    /// The username shape heuristics: `user@host` prompt tokens and
    /// home-directory names (`/home/<u>`, `C:\Users\<u>`).
    pub usernames: bool,
}

impl Default for PrivacyClasses {
    fn default() -> Self {
        Self { public_ips: true, private_ips: true, usernames: true }
    }
}

/// Whether an IPv6 literal is machine-local: loopback (`::1`),
/// link-local (`fe80::/10`) or a unique-local address (`fc00::/7`).
/// The counterpart of [`ipv4_is_private_or_loopback`] for the
/// private-IPs privacy class.
///
/// Parses the address rather than matching text prefixes: the prefix
/// form misclassified short hextets (`fe8::` is the group `0fe8`, a
/// GLOBAL address, not `fe80::/10`; `fc::` is `00fc`, not `fc00::/7`)
/// and missed an uncompressed loopback (`0:0:0:0:0:0:0:1`). A privacy
/// class deciding what to mask must get the range right.
pub fn ipv6_is_local(s: &str) -> bool {
    // Strip a `[...]` wrapper and a `%zone` suffix the scanner may
    // include before parsing the address itself.
    let core = s.trim().trim_start_matches('[').trim_end_matches(']');
    let core = core.split('%').next().unwrap_or(core);
    let Ok(addr) = core.parse::<std::net::Ipv6Addr>() else {
        return false;
    };
    if addr.is_loopback() {
        return true;
    }
    let seg0 = addr.segments()[0];
    // link-local fe80::/10 (top 10 bits 1111111010) or unique-local
    // fc00::/7 (top 7 bits 1111110).
    (seg0 & 0xffc0) == 0xfe80 || (seg0 & 0xfe00) == 0xfc00
}

/// One row's text (one char per column, blanks filled in) plus, for a
/// row that is not pure ASCII, the map from byte offset back to column.
/// Both scanners work in byte offsets and convert at the end; see the
/// comment at the call site for why.
/// One row as the scanners see it.
///
/// `wraps_at` is what lets a token be recognised across a soft wrap: a
/// row that ended by running into the margin is the same LOGICAL line as
/// the row below it, and a URL printed there has no scheme on its tail
/// row to match on. Callers with no grid to ask (the privacy helper, the
/// smart-select probe, tests) pass `None` and get the row-local
/// behaviour.
pub(crate) struct ScanRow {
    /// Visible row index, the key every `Highlight` is reported under.
    pub row: u16,
    /// Non-blank cells, `(column, char)`.
    pub cols: Vec<(u16, char)>,
    /// `Some(last column)` when this row soft-wraps into the next one
    /// (alacritty's WRAPLINE), `None` when it ends its line.
    pub wraps_at: Option<u16>,
}

fn row_text(cols: &[(u16, char)]) -> (String, Option<Vec<u16>>) {
    let max_col = cols.iter().map(|(c, _)| *c).max().unwrap_or(0) as usize;
    let mut chars = vec![' '; max_col + 1];
    for &(col, ch) in cols {
        if (col as usize) <= max_col {
            chars[col as usize] = ch;
        }
    }
    let row_str: String = chars.iter().collect();
    let byte_col: Option<Vec<u16>> = (!row_str.is_ascii()).then(|| {
        let mut map = vec![0u16; row_str.len()];
        for (col, (b, ch)) in row_str.char_indices().enumerate() {
            for off in 0..ch.len_utf8() {
                map[b + off] = col as u16;
            }
        }
        map
    });
    (row_str, byte_col)
}

/// Spans matched by the user's own highlight rules.
///
/// Kept apart from [`detect_highlights`] rather than folded into it, for
/// two reasons. The automatic detectors resolve overlaps against each
/// other (a path inside a URL is not a second highlight), and a user
/// rule must not join that negotiation: it was asked for explicitly, so
/// it wins outright, which the draw pass implements by consulting this
/// list first. And the two passes are gated independently: rules paint
/// whether or not the automatic "Keyword highlighting" toggle is on.
pub(crate) fn detect_rule_highlights(
    row_chars: &[ScanRow],
    rules: &[crate::highlight_rules::CompiledRule],
) -> Vec<Highlight> {
    let mut highlights = Vec::new();
    let mut spans = Vec::new();
    for ScanRow { row, cols, .. } in row_chars {
        let (row_str, byte_col) = row_text(cols);
        // A row is blanks-padded to its last printable column, so the
        // trailing run of spaces is not text the user can see. Matching
        // against the trimmed view keeps a rule like `\s+$` from
        // painting the whole rest of the line.
        let trimmed = row_str.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        for rule in rules {
            spans.clear();
            rule.find_spans(trimmed, &mut spans);
            for &(start, end) in &spans {
                let (start_col, end_col) = match &byte_col {
                    Some(map) => (map[start], map[end - 1]),
                    None => (start as u16, (end - 1) as u16),
                };
                highlights.push(Highlight {
                    row: *row,
                    start_col,
                    end_col,
                    color: rule.color,
                    kind: HighlightKind::Rule,
                });
            }
        }
    }
    highlights
}

/// Scan row text for IPv4/IPv6 addresses, URLs, and Unix file paths (no
/// regex). Takes `(row, non-blank cells)` pairs; rows with no printable
/// chars are simply absent (the draw pass builds this per frame, so a
/// dense Vec beats re-hashing every row into a map). `privacy_terms` are
/// extra strings (saved-connection hostnames, lowercase) masked wherever
/// they appear, Privacy Mode only.
pub(crate) fn detect_highlights(
    row_chars: &[ScanRow],
    palette: &TerminalPalette,
    privacy: bool,
    privacy_terms: &[String],
    classes: PrivacyClasses,
) -> Vec<Highlight> {
    let ip_color = palette.ansi[5];   // magenta
    let url_color = palette.ansi[4];  // blue
    let path_color = palette.ansi[6]; // cyan
    let num_color = palette.ansi[5];  // magenta, same as IP, easy scan

    let mut highlights = Vec::new();
    // Set by a row whose URL ran into the wrap margin, read by the row
    // below it. A URL whose HEAD is scrolled off the top of the viewport
    // starts with no carry and so keeps the old row-local colour: the
    // scan is per-frame and viewport-local by design, and Ctrl+click and
    // the hover underline both follow the wrap regardless
    // (`url_run_at_cell` walks the grid, not this).
    let mut carry_from: Option<u16> = None;

    for ScanRow { row, cols, wraps_at } in row_chars {
        let row = *row;
        let wraps_at = *wraps_at;
        // The scanners below all walk `bytes` and record BYTE offsets in
        // `start_col`/`end_col`. The row string holds exactly one char per
        // column, so a char's index IS its column; on a pure-ASCII row the
        // two units coincide, but one multi-byte char shifts every later
        // byte offset past its column, which slid highlight and privacy
        // spans to the right (a masked IP after a CJK char leaked its
        // leading cells). Internal span math stays in bytes (the
        // `dominated` overlap checks compare spans of the same row), and
        // the finished row's spans are remapped to columns at the end of
        // this loop body via `byte_col`.
        let (row_str, byte_col) = row_text(cols);
        let bytes = row_str.as_bytes();
        let len = bytes.len();
        let row_first_span = highlights.len();

        // --- URLs: "http://" or "https://" followed by non-whitespace ---
        //
        // A URL that runs into the wrap margin continues on the next row,
        // where it has no scheme left to match on, so that tail is picked
        // up from `carry_from` instead of by the scan. Without it the head
        // of a wrapped link was blue and the rest of it was not, even
        // though Ctrl+click opens the whole thing and the hover underline
        // covers every row it touches.
        {
            // Column of a byte, for comparing a span's end against the
            // wrap margin. Byte offsets and columns coincide on an ASCII
            // row; the map exists only when they do not.
            let col_of = |byte: usize| -> u16 {
                byte_col.as_ref().map_or(byte as u16, |m| m[byte])
            };
            // Trailing sentence punctuation belongs to the prose around
            // the link. Trimmed only where the URL actually ENDS: at a
            // wrap those bytes are interior text, and cutting them there
            // would break the span and lose the carry with it.
            let trim_tail = |mut end: usize, start: usize| -> usize {
                while end > start
                    && matches!(bytes[end - 1], b')' | b']' | b'>' | b',' | b'.' | b';')
                {
                    end -= 1;
                }
                end
            };
            let mut i = 0;
            // Only the row IMMEDIATELY below the one that carried out may
            // claim the carry. A row with nothing printable on it never
            // reaches this loop, and a stale carry must not leap over it
            // onto unrelated text.
            if carry_from.take() == Some(row.wrapping_sub(1)) {
                let mut end = 0;
                for ch in row_str.chars() {
                    if ch.is_whitespace() || ch == '\0' {
                        break;
                    }
                    end += ch.len_utf8();
                }
                if end > 0 {
                    let carries_on = wraps_at == Some(col_of(end - 1));
                    let cut = if carries_on { end } else { trim_tail(end, 0) };
                    highlights.push(Highlight {
                        row,
                        start_col: 0,
                        end_col: (cut - 1) as u16,
                        color: url_color,
                        kind: HighlightKind::Url,
                    });
                    if carries_on {
                        carry_from = Some(row);
                    }
                    // Resume past the tail, not past the trim, so a second
                    // URL later on the same row is still found.
                    i = end;
                }
            }
            while i < len {
                // Only slice at ASCII 'h', guaranteed char boundary. Skipping this
                // guard panics when i lands mid-UTF-8 (e.g. typing "ç" crashed the app).
                if bytes[i] != b'h' {
                    i += 1;
                    continue;
                }
                let rest = &row_str[i..];
                if rest.starts_with("http://") || rest.starts_with("https://") {
                    let start = i;
                    let mut end = i;
                    for ch in row_str[i..].chars() {
                        if ch.is_whitespace() || ch == '\0' {
                            break;
                        }
                        end += ch.len_utf8();
                    }
                    if end > start {
                        // Reaching the margin of a wrapped row means the
                        // link goes on below.
                        let carries_on = wraps_at == Some(col_of(end - 1));
                        let cut = if carries_on { end } else { trim_tail(end, start) };
                        highlights.push(Highlight {
                            row,
                            start_col: start as u16,
                            end_col: (cut - 1) as u16,
                            color: url_color,
                            kind: HighlightKind::Url,
                        });
                        if carries_on {
                            carry_from = Some(row);
                        }
                        i = end;
                        continue;
                    }
                }
                i += 1;
            }
        }

        // --- IPv4: digit groups separated by dots (4 groups, each 0-255).
        // EVERY range-valid quad masks under Privacy Mode (per its class
        // gate): version-shaped tokens ("version 1.2.3.4") used to be
        // exempted (issue #53), but a valid quad is byte-for-byte
        // indistinguishable from an address, so hostile output could use
        // the version prefix to display a real IP unmasked. Owner call
        // 2026-07-19: masking an actual version string accidentally is
        // the acceptable error. VersionQuad survives only as the
        // "colored, never masked" kind for candidates whose privacy
        // class is off.
        {
            let candidates = scan_quad_dot_candidates(&row_str);
            for &(start, end) in &candidates {
                let dominated = highlights.iter().any(|h| {
                    h.row == row && start as u16 >= h.start_col && (start as u16) <= h.end_col
                });
                let text = &row_str[start..end];
                // Per-class privacy gates (issue #78): a disabled IP
                // class demotes the span to the never-masked VersionQuad
                // kind so keyword coloring survives. A vault-term hit
                // always masks: the terms list is already class-filtered
                // app-side.
                let term_hit = privacy_terms.iter().any(|t| t == text);
                let private = ipv4_is_private_or_loopback(text);
                let class_on = if private { classes.private_ips } else { classes.public_ips };
                let masked = term_hit || class_on;
                // A candidate already owned by an earlier span (a scraped
                // URL) is normally skipped. But the app-side redactor is
                // not URL-aware and masks an address inside a URL host, so
                // under Privacy Mode push an OVERLAPPING Ip span for that
                // one case (the draw pass masks any cell a privacy_only
                // span covers, and leaves the URL's color on the rest). A
                // non-masked candidate stays skipped so it does not fight
                // the URL's color.
                if dominated && !(privacy && masked) {
                    continue;
                }
                let kind = if masked {
                    HighlightKind::Ip
                } else {
                    HighlightKind::VersionQuad
                };
                highlights.push(Highlight {
                    row,
                    start_col: start as u16,
                    end_col: (end - 1) as u16,
                    color: ip_color,
                    kind,
                });
            }
        }

        // --- IPv6: hex-digit groups separated by colons, validated by
        // `looks_like_ipv6` (needs `::` or the full form's 7 colons, so
        // timestamps and MACs stay out). Runs glued to a word on either
        // side (std::io, Vec::new, beef42) are identifiers, not
        // addresses. A single leading/trailing colon is prose
        // punctuation and is trimmed off first. Same kind as IPv4:
        // colored by keyword highlighting, masked by Privacy Mode. An
        // embedded dotted-quad tail (`::ffff:192.0.2.1`) is already
        // covered by the IPv4 pass above; the two spans sit side by side.
        {
            let mut i = 0;
            while i < len {
                if !bytes[i].is_ascii_hexdigit() && bytes[i] != b':' {
                    i += 1;
                    continue;
                }
                // Take the whole run up front so a rejected start skips it
                // entirely instead of re-matching at every inner offset.
                let start = i;
                let mut j = i;
                while j < len && (bytes[j].is_ascii_hexdigit() || bytes[j] == b':') {
                    j += 1;
                }
                let glued = (start > 0
                    && (is_word_byte(bytes[start - 1]) || bytes[start - 1] == b'.'))
                    || (j < len && is_word_byte(bytes[j]));
                let mut s2 = start;
                let mut e2 = j;
                if e2 - s2 >= 2 && bytes[s2] == b':' && bytes[s2 + 1] != b':' {
                    s2 += 1;
                }
                if e2 - s2 >= 2 && bytes[e2 - 1] == b':' && bytes[e2 - 2] != b':' {
                    e2 -= 1;
                }
                let dominated = highlights.iter().any(|h| {
                    h.row == row && s2 as u16 >= h.start_col && (s2 as u16) <= h.end_col
                });
                if !glued && looks_like_ipv6(&row_str[s2..e2]) {
                    // Per-class gate (issue #78): a disabled class
                    // keeps the keyword color but never masks, same
                    // demotion the IPv4 arm applies.
                    let class_on = if ipv6_is_local(&row_str[s2..e2]) {
                        classes.private_ips
                    } else {
                        classes.public_ips
                    };
                    // Same URL-host exception as the IPv4 arm: a candidate
                    // dominated by a scraped URL is skipped unless Privacy
                    // Mode would mask it, in which case an overlapping Ip
                    // span masks the host inside the URL.
                    if !dominated || (privacy && class_on) {
                        highlights.push(Highlight {
                            row,
                            start_col: s2 as u16,
                            end_col: (e2 - 1) as u16,
                            color: ip_color,
                            kind: if class_on {
                                HighlightKind::Ip
                            } else {
                                HighlightKind::VersionQuad
                            },
                        });
                    }
                }
                i = j;
            }
        }

        // --- user@host prompt tokens (Privacy Mode only): word@word ---
        // Anchored on '@' with token chars on both sides. Token chars are
        // alnum plus `. _ -` (host labels, usernames, email locals). This
        // catches the unix prompt (`root@web`), emails, and typed
        // `ssh user@host` targets, all of which are sensitive. Never
        // colored, only masked, so it runs solely under Privacy Mode
        // and only while the usernames class is on (issue #78).
        if privacy && classes.usernames {
            let is_tok = |b: u8| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-';
            let mut i = 0;
            while i < len {
                if bytes[i] == b'@'
                    && i > 0
                    && i + 1 < len
                    && is_tok(bytes[i - 1])
                    && is_tok(bytes[i + 1])
                {
                    let mut start = i;
                    while start > 0 && is_tok(bytes[start - 1]) {
                        start -= 1;
                    }
                    let mut end = i + 1;
                    while end < len && is_tok(bytes[end]) {
                        end += 1;
                    }
                    highlights.push(Highlight {
                        row,
                        start_col: start as u16,
                        end_col: (end - 1) as u16,
                        color: ip_color,
                        kind: HighlightKind::HostUser,
                    });
                    i = end;
                    continue;
                }
                i += 1;
            }
        }

        // --- Home-directory usernames (Privacy Mode only): the `<name>` in
        // `C:\Users\<name>`, `/home/<name>` or `/Users/<name>` (Windows /
        // Linux / macOS prompts and paths). Only the name segment is
        // masked, the rest of the path stays readable. Markers compare
        // case-insensitively (`c:\users\` prompts exist too). Gated by
        // the usernames class like the prompt tokens (issue #78).
        if privacy && classes.usernames {
            let is_tok = |b: u8| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-';
            const MARKERS: [&[u8]; 3] = [b"\\users\\", b"/home/", b"/users/"];
            let mut i = 0;
            while i < len {
                if bytes[i] != b'\\' && bytes[i] != b'/' {
                    i += 1;
                    continue;
                }
                let Some(mlen) = MARKERS
                    .iter()
                    .find(|m| i + m.len() <= len && bytes[i..i + m.len()].eq_ignore_ascii_case(m))
                    .map(|m| m.len())
                else {
                    i += 1;
                    continue;
                };
                let start = i + mlen;
                let mut end = start;
                while end < len && is_tok(bytes[end]) {
                    end += 1;
                }
                // A `/home/` or `/users/` inside a detected URL is a web
                // path (`https://cdn.io/users/42`), not this machine's
                // account name; leave those alone.
                let inside_url = highlights.iter().any(|h| {
                    h.kind == HighlightKind::Url
                        && h.row == row
                        && start as u16 >= h.start_col
                        && (start as u16) <= h.end_col
                });
                if end > start && !inside_url {
                    highlights.push(Highlight {
                        row,
                        start_col: start as u16,
                        end_col: (end - 1) as u16,
                        color: ip_color,
                        kind: HighlightKind::UserDir,
                    });
                }
                i = end.max(i + 1);
            }
        }

        // --- Saved-connection hostnames (Privacy Mode only): exact,
        // case-insensitive, token-bounded occurrences of the vault's host
        // addresses, provided by the app in `privacy_terms` (lowercase).
        // Plain DNS names have no detectable shape, file extensions
        // collide with ccTLDs (`main.rs`, `install.sh` are FQDN-shaped),
        // so the known values are matched literally instead of guessed.
        if privacy && !privacy_terms.is_empty() {
            let is_tok = |b: u8| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-';
            let lower = row_str.to_ascii_lowercase();
            for term in privacy_terms {
                let mut from = 0;
                while let Some(pos) = lower[from..].find(term.as_str()) {
                    let s0 = from + pos;
                    let e0 = s0 + term.len();
                    let bounded = (s0 == 0 || !is_tok(bytes[s0 - 1]))
                        && (e0 >= len || !is_tok(bytes[e0]));
                    if bounded {
                        highlights.push(Highlight {
                            row,
                            start_col: s0 as u16,
                            end_col: (e0 - 1) as u16,
                            color: ip_color,
                            kind: HighlightKind::KnownHost,
                        });
                    }
                    from = e0;
                }
            }
        }

        // --- Unix file paths: "/" followed by alphanumeric/dot/dash/underscore/slash ---
        {
            let mut i = 0;
            while i < len {
                if bytes[i] == b'/' {
                    if i > 0 {
                        let prev = bytes[i - 1];
                        if prev.is_ascii_alphanumeric() || prev == b'_' || prev == b'-' || prev == b'.' {
                            i += 1;
                            continue;
                        }
                    }
                    let start = i;
                    let mut j = i + 1;
                    while j < len {
                        let b = bytes[j];
                        if b.is_ascii_alphanumeric()
                            || b == b'.' || b == b'-' || b == b'_' || b == b'/' || b == b'~'
                        {
                            j += 1;
                        } else {
                            break;
                        }
                    }
                    if j - start >= 3 {
                        while j > start + 1 && (bytes[j - 1] == b'.' || bytes[j - 1] == b'/') {
                            j -= 1;
                        }
                        let dominated = highlights.iter().any(|h| {
                            h.row == row && start as u16 >= h.start_col && (start as u16) <= h.end_col
                        });
                        if !dominated && j - start >= 3 {
                            highlights.push(Highlight {
                                row,
                                start_col: start as u16,
                                end_col: (j - 1) as u16,
                                color: path_color,
                                kind: HighlightKind::Path,
                            });
                        }
                        i = j;
                        continue;
                    }
                }
                i += 1;
            }
        }

        // --- Standalone numbers: int/float, optional minus, optional %.
        // Examples: 1634, -273.1, 23.3%, 0.0. Skipped when the run is part
        // of an existing highlight (IP/path/URL) or is inside a word.
        {
            let mut i = 0;
            while i < len {
                let b = bytes[i];
                let is_start = b.is_ascii_digit()
                    || (b == b'-'
                        && i + 1 < len
                        && bytes[i + 1].is_ascii_digit()
                        && (i == 0 || !is_word_byte(bytes[i - 1])));
                if !is_start {
                    i += 1;
                    continue;
                }
                // Reject when prefixed by a word character (e.g. "abc123",
                // version strings), those should keep the surrounding fg.
                if i > 0 && b.is_ascii_digit() && is_word_byte(bytes[i - 1]) {
                    i += 1;
                    continue;
                }
                let start = i;
                let mut j = i;
                if bytes[j] == b'-' {
                    j += 1;
                }
                while j < len && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                // Optional decimal part, must be `.<digit>+`.
                if j + 1 < len && bytes[j] == b'.' && bytes[j + 1].is_ascii_digit() {
                    j += 1;
                    while j < len && bytes[j].is_ascii_digit() {
                        j += 1;
                    }
                }
                // Optional trailing percent.
                if j < len && bytes[j] == b'%' {
                    j += 1;
                }
                // Reject when followed by a letter (e.g. "10.0.0.1",
                // "v1.2-rc", the IP path already handled the first; we
                // also avoid colouring "rc" parts).
                if j < len && is_word_byte(bytes[j]) {
                    i = j;
                    continue;
                }
                let dominated = highlights.iter().any(|h| {
                    h.row == row
                        && start as u16 >= h.start_col
                        && (start as u16) <= h.end_col
                });
                if !dominated && j > start {
                    highlights.push(Highlight {
                        row,
                        start_col: start as u16,
                        end_col: (j - 1) as u16,
                        color: num_color,
                        kind: HighlightKind::Number,
                    });
                }
                i = j;
            }
        }

        // Remap this row's spans from byte offsets to columns (see the
        // `byte_col` comment above). ASCII rows skip this: byte == column.
        if let Some(map) = &byte_col {
            for h in &mut highlights[row_first_span..] {
                h.start_col = map[h.start_col as usize];
                h.end_col = map[h.end_col as usize];
            }
        }
    }

    highlights
}

/// WCAG 2.x relative luminance for an sRGB colour in `[0, 1]`. Used by
/// the smart-contrast fallback to decide whether a too-close cell
/// should flip its foreground to white or near-black.
pub(crate) fn relative_luminance(c: Color) -> f32 {
    fn channel(v: f32) -> f32 {
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b)
}

/// WCAG contrast ratio between two opaque colours: 1.0 = identical,
/// 21.0 = white-on-black. We trip the smart-contrast fallback below
/// `2.5`, well under the AA-body threshold of `4.5` so we only act
/// on visually disappearing pairs and leave merely-low-contrast
/// styling alone.
pub(crate) fn contrast_ratio(a: Color, b: Color) -> f32 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    let (lighter, darker) = if la >= lb { (la, lb) } else { (lb, la) };
    (lighter + 0.05) / (darker + 0.05)
}

#[inline]
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Check if a cell position falls within any highlight, returning the color.
#[inline]
pub(crate) fn highlight_color_at(highlights: &[Highlight], row: u16, col: u16) -> Option<Color> {
    for h in highlights {
        // HostUser / UserDir are Privacy-Mode-only markers, never a syntax
        // color: skip them so prompts aren't tinted when keyword
        // highlighting is on.
        if !h.kind.privacy_only() && h.row == row && col >= h.start_col && col <= h.end_col {
            return Some(h.color);
        }
    }
    None
}

/// Privacy span extents with overlapping or directly adjacent spans on the
/// same row merged into one `(row, start_col, end_col)`. The detectors
/// overlap by design (`user@host` from the prompt scan plus the saved
/// hostname inside it from the term scan): drawing one bar per raw
/// highlight painted two bars, and so two eye-slashes, over what reads as
/// a single redaction. Merging here is canonical, every consumer
/// (bar drawing, hover reveal, pin-by-text) sees the same unified spans,
/// so a reveal always uncovers exactly what one bar covers.
fn merged_privacy_extents(highlights: &[Highlight]) -> Vec<(u16, u16, u16)> {
    let mut exts: Vec<(u16, u16, u16)> = highlights
        .iter()
        .filter(|h| h.kind == HighlightKind::Ip || h.kind.privacy_only())
        .map(|h| (h.row, h.start_col, h.end_col))
        .collect();
    exts.sort_unstable();
    let mut merged: Vec<(u16, u16, u16)> = Vec::with_capacity(exts.len());
    for (row, start, end) in exts {
        match merged.last_mut() {
            Some((mrow, _, mend)) if *mrow == row && start <= mend.saturating_add(1) => {
                *mend = (*mend).max(end);
            }
            _ => merged.push((row, start, end)),
        }
    }
    merged
}

/// Find the IP / `user@host` privacy span covering a cell, returning its
/// `(row, start_col, end_col)` (inclusive). Used by the draw pass to
/// reveal the span the cursor is over while the rest stay masked, the same
/// hover-reveal mechanic as [`hovered_url_range`].
#[inline]
pub(crate) fn privacy_span_at(
    highlights: &[Highlight],
    row: u16,
    col: u16,
) -> Option<(u16, u16, u16)> {
    merged_privacy_extents(highlights)
        .into_iter()
        .find(|(r, sc, ec)| *r == row && col >= *sc && col <= *ec)
}

/// Whether a cell falls inside any IP / `user@host` privacy span. The draw
/// pass masks such cells (block glyph + muted color) unless they're in the
/// currently revealed span.
#[inline]
pub(crate) fn is_privacy_cell(highlights: &[Highlight], row: u16, col: u16) -> bool {
    highlights.iter().any(|h| {
        (h.kind == HighlightKind::Ip || h.kind.privacy_only())
            && h.row == row
            && col >= h.start_col
            && col <= h.end_col
    })
}

/// All privacy spans with their text, resolved from the same per-frame row
/// data the draw pass uses. The draw pass matches these against the
/// click-pinned value set so every occurrence of a pinned value stays
/// revealed, wherever it appears.
/// All privacy span extents in the frame's highlight set, revealed or
/// not. The draw pass turns each non-revealed one into a single
/// span-level redaction bar (rounded rect + eye-slash, issue #78)
/// instead of per-cell fills.
pub(crate) fn privacy_extents(highlights: &[Highlight]) -> Vec<(u16, u16, u16)> {
    merged_privacy_extents(highlights)
}

pub(crate) fn privacy_spans_with_text(
    highlights: &[Highlight],
    row_chars: &[ScanRow],
) -> Vec<((u16, u16, u16), String)> {
    merged_privacy_extents(highlights)
        .into_iter()
        .filter_map(|(row, start_col, end_col)| {
            let cells = &row_chars.iter().find(|sr| sr.row == row)?.cols;
            let mut text = String::with_capacity((end_col - start_col + 1) as usize);
            for col in start_col..=end_col {
                text.push(
                    cells
                        .iter()
                        .find(|(c, _)| *c == col)
                        .map(|(_, ch)| *ch)
                        .unwrap_or(' '),
                );
            }
            Some(((row, start_col, end_col), text))
        })
        .collect()
}

/// Text of the privacy span covering a cell, scroll-aware. Rebuilds the
/// one grid row (the way `smart_span_at` does), reruns the privacy
/// detection on it, and returns the covered span's text. Drives the
/// click-to-pin reveal: the returned value keys the pinned set.
pub(crate) fn privacy_value_at_cell(
    term: &alacritty_terminal::Term<crate::backend::EventProxy>,
    palette: &TerminalPalette,
    privacy_terms: &[String],
    classes: PrivacyClasses,
    line: i32,
    col: u16,
) -> Option<String> {
    use alacritty_terminal::grid::Dimensions;
    use alacritty_terminal::index::{Column, Line};
    let grid = term.grid();
    let l = Line(line);
    if l < grid.topmost_line() || l > grid.bottommost_line() {
        return None;
    }
    let row = &grid[l];
    let ncols = grid.columns();
    let mut cols: Vec<(u16, char)> = Vec::new();
    for ci in 0..ncols {
        let c = row[Column(ci)].c;
        if c != ' ' && c != '\0' {
            cols.push((ci as u16, c));
        }
    }
    if cols.is_empty() {
        return None;
    }
    let rows = [ScanRow { row: 0, cols, wraps_at: None }];
    let highlights = detect_highlights(&rows, palette, true, privacy_terms, classes);
    privacy_spans_with_text(&highlights, &rows)
        .into_iter()
        .find(|((_, sc, ec), _)| col >= *sc && col <= *ec)
        .map(|(_, text)| text)
}

/// Returns true when the given cell is part of a URL highlight, used by the
/// draw pass to paint an underline under clickable links.
#[inline]
/// Find the URL highlight that contains a specific cell, used by the
/// draw pass to underline only the URL the cursor is over (instead of
/// every URL in the viewport, which made even un-hovered links look
/// "linkable" with no Ctrl-click feedback).
pub(crate) fn hovered_url_range(
    highlights: &[Highlight],
    row: u16,
    col: u16,
) -> Option<(u16, u16, u16)> {
    highlights
        .iter()
        .find(|h| {
            h.kind == HighlightKind::Url
                && h.row == row
                && col >= h.start_col
                && col <= h.end_col
        })
        .map(|h| (h.row, h.start_col, h.end_col))
}

/// How many rows a soft-wrap chain is followed across when a logical
/// line is reassembled, IN EACH DIRECTION from the clicked row.
///
/// The bound is what keeps a pathological grid (every row carrying
/// WRAPLINE) from walking the whole scrollback on a single hover; 64
/// rows of a wide pane is several thousand characters, far past any URL
/// a program actually prints. Same bound the prompt walk in `state.rs`
/// uses, for the same reason.
const MAX_WRAP_ROWS: usize = 64;

/// A logical line reassembled across soft-wrapped rows, carrying the
/// provenance the row-local scan used to get for free.
struct LogicalLine {
    text: String,
    /// One entry per BYTE of `text`: the `(grid line, column)` that
    /// produced it. A multi-byte char repeats its cell across its
    /// bytes, so any byte offset maps back to a cell.
    cells: Vec<(i32, u16)>,
}

impl LogicalLine {
    /// Byte offset of a given cell, or `None` when the cell is past the
    /// end of the line (a trimmed trailing blank).
    fn byte_of(&self, line: i32, col: u16) -> Option<usize> {
        self.cells.iter().position(|&c| c == (line, col))
    }

    /// The `cells` slice `[start, end)` as one segment per grid row:
    /// `(line, first_col, last_col)`, inclusive columns. A chain is
    /// contiguous and monotonic, so a run of equal lines is one segment.
    fn segments(&self, start: usize, end: usize) -> Vec<LinkSegment> {
        let mut segs: Vec<LinkSegment> = Vec::new();
        for &(line, col) in &self.cells[start..end] {
            match segs.last_mut() {
                Some((l, _, last)) if *l == line => *last = col,
                _ => segs.push((line, col, col)),
            }
        }
        segs
    }
}

/// Reassemble the logical line that `target_line` belongs to, following
/// the soft-wrap chain in both directions.
///
/// A row that ends in WRAPLINE is continued by the next one, and the
/// text of the two is one line with no break between them - the same
/// rule the selection copy path follows so a wrapped URL copies out
/// intact. Reading only the clicked row (what this used to do) truncated
/// every link long enough to wrap, which is exactly the shape an OAuth
/// authorize URL has.
fn logical_line_at(
    term: &alacritty_terminal::Term<crate::backend::EventProxy>,
    target_line: i32,
) -> Option<LogicalLine> {
    use alacritty_terminal::index::{Column, Line};
    use alacritty_terminal::term::cell::Flags as CellFlags;

    // Index grid rows directly (the way `smart_span_at` does) instead of
    // walking the viewport display iterator. `target_line` is a grid
    // line (scroll adjusted, negative for scrollback), not an on-screen
    // row, so Ctrl+click and hover stay correct when scrolled into
    // history.
    let grid = term.grid();
    if Line(target_line) < grid.topmost_line() || Line(target_line) > grid.bottommost_line() {
        return None;
    }
    let ncols = grid.columns();
    let last_col = Column(ncols.saturating_sub(1));
    let wraps = |line: i32| -> bool {
        grid[Line(line)][last_col]
            .flags
            .contains(CellFlags::WRAPLINE)
    };

    let topmost = grid.topmost_line().0;
    let bottommost = grid.bottommost_line().0;
    let mut first = target_line;
    // A budget PER DIRECTION, never one shared between them: the cell
    // that was clicked is somewhere inside the chain, and a shared count
    // spent going up leaves nothing to go down with, so a link long
    // enough to fill the walk would open truncated at exactly the cell
    // the user aimed at. That is the bug this walk exists to fix.
    let mut up = 0;
    while first > topmost && up < MAX_WRAP_ROWS && wraps(first - 1) {
        first -= 1;
        up += 1;
    }
    let mut last = target_line;
    let mut down = 0;
    while last < bottommost && down < MAX_WRAP_ROWS && wraps(last) {
        last += 1;
        down += 1;
    }

    let mut text = String::new();
    let mut cells: Vec<(i32, u16)> = Vec::new();
    for line in first..=last {
        let row = &grid[Line(line)];
        // A soft-wrapped row is full to the margin, so its trailing
        // cells are interior content of the logical line; only the row
        // that ENDS the chain gets its blank tail dropped.
        let width = if line < last {
            ncols
        } else {
            (0..ncols)
                .rev()
                .find(|&c| !matches!(row[Column(c)].c, ' ' | '\0'))
                .map_or(0, |c| c + 1)
        };
        for ci in 0..width {
            // `\0` is an unwritten cell; it reads as a blank, which ends
            // a URL token the same way a printed space does.
            let ch = match row[Column(ci)].c {
                '\0' => ' ',
                c => c,
            };
            text.push(ch);
            cells.extend(std::iter::repeat_n((line, ci as u16), ch.len_utf8()));
        }
    }
    (!text.trim().is_empty()).then_some(LogicalLine { text, cells })
}

/// Extract the URL string at a given cell, if any. Returns `None` when
/// the click lands outside any URL.
pub(crate) fn url_at_cell(
    term: &alacritty_terminal::Term<crate::backend::EventProxy>,
    target_line: i32,
    target_col: u16,
) -> Option<String> {
    url_run_at_cell(term, target_line, target_col).map(|(url, _)| url)
}

/// The scraped URL at a cell together with the rows it occupies.
///
/// The counterpart of [`osc8_link_run`] for links a program printed as
/// plain text: both hand the hover path a target plus the segments to
/// underline, so a URL that soft-wraps is drawn (and opened) as the one
/// link it is rather than as its first row.
pub(crate) fn url_run_at_cell(
    term: &alacritty_terminal::Term<crate::backend::EventProxy>,
    target_line: i32,
    target_col: u16,
) -> Option<(String, Vec<LinkSegment>)> {
    let logical = logical_line_at(term, target_line)?;
    let target = logical.byte_of(target_line, target_col)?;
    let text = &logical.text;
    let bytes = text.as_bytes();
    let len = bytes.len();

    let mut i = 0;
    while i < len {
        if bytes[i] != b'h' {
            i += 1;
            continue;
        }
        let rest = &text[i..];
        if rest.starts_with("http://") || rest.starts_with("https://") {
            let start = i;
            let mut end = i;
            for ch in rest.chars() {
                if ch.is_whitespace() || ch == '\0' {
                    break;
                }
                end += ch.len_utf8();
            }
            if end > start {
                while end > start {
                    let last = bytes[end - 1];
                    if last == b')' || last == b']' || last == b'>'
                        || last == b',' || last == b'.' || last == b';'
                    {
                        end -= 1;
                    } else {
                        break;
                    }
                }
                if (start..end).contains(&target) {
                    return Some((text[start..end].to_string(), logical.segments(start, end)));
                }
                i = end;
                continue;
            }
        }
        i += 1;
    }
    None
}

/// Explicit OSC 8 hyperlink at a cell, with the column run on this row that
/// shares the same link. Returns `(uri, start_col, end_col)` (inclusive cols).
///
/// Unlike [`url_at_cell`], which scrapes a literal `http(s)://` token out of
/// the rendered text, the URI here is an attribute alacritty parsed from the
/// OSC 8 escape, so it works when the displayed label differs from the target
/// (e.g. `\e]8;;https://example.com\e\\click here\e]8;;\e\\`). The run is
/// grouped by alacritty's hyperlink id (which ties the cells of one logical
/// link together) plus the uri.
pub(crate) fn osc8_link_at_cell(
    term: &alacritty_terminal::Term<crate::backend::EventProxy>,
    target_line: i32,
    target_col: u16,
) -> Option<(String, u16, u16)> {
    use alacritty_terminal::index::{Column, Line};
    let grid = term.grid();
    let line = Line(target_line);
    if line < grid.topmost_line() || line > grid.bottommost_line() {
        return None;
    }
    let row = &grid[line];
    let ncols = grid.columns();
    let col = target_col as usize;
    if col >= ncols {
        return None;
    }
    let link = row[Column(col)].hyperlink()?;
    let uri = link.uri().to_string();
    let id = link.id().to_string();
    let same = |c: usize| -> bool {
        row[Column(c)]
            .hyperlink()
            .is_some_and(|h| h.id() == id && h.uri() == uri)
    };
    let mut start = col;
    while start > 0 && same(start - 1) {
        start -= 1;
    }
    let mut end = col;
    while end + 1 < ncols && same(end + 1) {
        end += 1;
    }
    Some((uri, start as u16, end as u16))
}

/// The full run of an OSC 8 hyperlink at a cell, following a wrapped link
/// across grid rows. Returns `(uri, segments)` where each segment is
/// `(grid_line, start_col, end_col)` (inclusive cols), ordered top to bottom.
///
/// Unlike [`osc8_link_at_cell`] (which clamps to the hovered row and drives
/// the open / hint paths), this powers the hover underline, which must
/// cover every row a long link wraps onto. The walk only crosses a row
/// boundary on a genuine wrap: the current row's run must be flush against
/// the far edge AND the adjacent row's near edge must carry the same
/// hyperlink `id + uri`. This never merges two same-`id` but disjoint
/// regions (an explicit `id=` can repeat), only a contiguous wrap. Capped at
/// `MAX_ROWS` so a pathologically long link can't walk the whole scrollback
/// on the draw hot path (it keeps a partial underline past the cap).
/// One row's slice of a link run: `(grid_line, start_col, end_col)`.
/// Shared by the OSC 8 and scraped-URL paths, both of which can span
/// rows (an explicit hyperlink over a wrapped label, a plain URL over a
/// soft wrap) and both of which underline every row they cover.
pub(crate) type LinkSegment = (i32, u16, u16);

pub(crate) fn osc8_link_run(
    term: &alacritty_terminal::Term<crate::backend::EventProxy>,
    target_line: i32,
    target_col: u16,
) -> Option<(String, Vec<LinkSegment>)> {
    use alacritty_terminal::grid::Dimensions;
    use alacritty_terminal::index::{Column, Line};
    let grid = term.grid();
    let topmost = grid.topmost_line().0;
    let bottommost = grid.bottommost_line().0;
    let ncols = grid.columns();
    if target_line < topmost || target_line > bottommost || ncols == 0 {
        return None;
    }
    let col = target_col as usize;
    if col >= ncols {
        return None;
    }
    let anchor = grid[Line(target_line)][Column(col)].hyperlink()?;
    let uri = anchor.uri().to_string();
    let id = anchor.id().to_string();
    let same_on = |line: i32, c: usize| -> bool {
        grid[Line(line)][Column(c)]
            .hyperlink()
            .is_some_and(|h| h.id() == id && h.uri() == uri)
    };
    // The contiguous run on one row around a known-matching column.
    let seg = |line: i32, from: usize| -> LinkSegment {
        let mut s = from;
        while s > 0 && same_on(line, s - 1) {
            s -= 1;
        }
        let mut e = from;
        while e + 1 < ncols && same_on(line, e + 1) {
            e += 1;
        }
        (line, s as u16, e as u16)
    };
    const MAX_ROWS: usize = 8;
    let last_col = ncols - 1;
    let mut segments = vec![seg(target_line, col)];
    // Walk up: cross only when the current top segment starts at col 0
    // (i.e. it wrapped from above) and the previous row's last cell is the
    // same link.
    let mut line = target_line;
    while segments.len() < MAX_ROWS
        && line > topmost
        && segments[0].1 == 0
        && same_on(line - 1, last_col)
    {
        line -= 1;
        segments.insert(0, seg(line, last_col));
    }
    // Walk down: cross only when the current bottom segment ends at the last
    // col (wraps onward) and the next row's first cell is the same link.
    let mut line = target_line;
    while segments.len() < MAX_ROWS
        && line < bottommost
        && segments[segments.len() - 1].2 as usize == last_col
        && same_on(line + 1, 0)
    {
        line += 1;
        segments.push(seg(line, 0));
    }
    Some((uri, segments))
}

/// The scheme of a URI, lowercased, iff the URI begins with a well-formed
/// `scheme:` per RFC 3986 (`ALPHA *( ALPHA / DIGIT / "+" / "-" / "." ) ":"`).
///
/// Runs on attacker-controlled OSC 8 bytes, so it is strict: the run before
/// the first `:` must start with a letter and contain only scheme characters.
/// A leading space, a control char or a newline anywhere in that run
/// (`java\nscript:`, ` javascript:`) fails to parse, so it can never be
/// mistaken for an allowed scheme.
pub(crate) fn osc8_scheme(uri: &str) -> Option<String> {
    let (scheme, _rest) = uri.split_once(':')?;
    let mut chars = scheme.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    chars
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        .then(|| scheme.to_ascii_lowercase())
}

/// Whether an OSC 8 target may be followed (pointer cursor + Ctrl+click open).
///
/// OSC 8 URIs are attacker-controlled server output and, unlike the scraped
/// URLs (which only ever match `http(s)://`), can name any scheme, including
/// `javascript:`, `file:` and OS-handler schemes with side effects. Allow only
/// the safe, widely-understood web-facing schemes; everything else gets a
/// "link type not allowed" chip with no pointer, underline or open affordance,
/// so a hostile server cannot phish a click into an arbitrary handler.
///
/// `ssh://` is intentionally NOT allowed here yet: it should route to the
/// in-app quick-connect path (v1.0 `ssh://` handler follow-up), not open
/// blindly through the OS.
pub(crate) fn osc8_scheme_allowed(uri: &str) -> bool {
    matches!(
        osc8_scheme(uri).as_deref(),
        Some("http" | "https" | "mailto" | "ftp")
    )
}

/// Smart-select span for double-click: if the cell at grid-line `line`,
/// column `col` falls inside a detected URL / IP / path token, return its
/// `(start_col, end_col)` (inclusive). Returns `None` otherwise (caller
/// falls back to delimiter-word selection). Numbers are excluded, they are
/// too granular to be a useful "word" target. Reads the grid directly by
/// line so it stays correct when scrolled into history (unlike
/// `url_at_cell`, which indexes by on-screen row number and so only
/// matches the live screen).
pub(crate) fn smart_span_at(
    term: &alacritty_terminal::Term<crate::backend::EventProxy>,
    palette: &TerminalPalette,
    line: i32,
    col: u16,
) -> Option<(u16, u16)> {
    use alacritty_terminal::grid::Dimensions;
    use alacritty_terminal::index::{Column, Line};
    let grid = term.grid();
    let l = Line(line);
    if l < grid.topmost_line() || l > grid.bottommost_line() {
        return None;
    }
    let row = &grid[l];
    let ncols = grid.columns();
    let mut present = vec![false; ncols];
    let mut cols: Vec<(u16, char)> = Vec::new();
    for ci in 0..ncols {
        let c = row[Column(ci)].c;
        if c != ' ' && c != '\0' {
            present[ci] = true;
            cols.push((ci as u16, c));
        }
    }
    if cols.is_empty() || !present.get(col as usize).copied().unwrap_or(false) {
        return None;
    }
    // Expand to the whitespace-bounded token containing the click.
    let mut left = col;
    while left > 0 && present[left as usize - 1] {
        left -= 1;
    }
    let mut right = col;
    while (right as usize + 1) < ncols && present[right as usize + 1] {
        right += 1;
    }
    // Trigger only when that token overlaps a detected URL / IP / path
    // highlight, so plain prose words still fall through to delimiter-word
    // selection. The highlighter's own URL span may be shorter than the
    // token (its matcher is loose), hence the overlap test rather than a
    // containment test. `detect_highlights` takes (row, cells) pairs; a
    // single synthetic row 0 is enough as long as we match on the same key.
    let rows = [ScanRow { row: 0, cols, wraps_at: None }];
    let hit = detect_highlights(&rows, palette, false, &[], PrivacyClasses::default()).into_iter().any(|h| {
        h.row == 0
            && h.kind != HighlightKind::Number
            && h.start_col <= right
            && h.end_col >= left
    });
    hit.then_some((left, right))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv6_local_classification_uses_real_ranges() {
        // Loopback in both forms.
        assert!(ipv6_is_local("::1"));
        assert!(ipv6_is_local("0:0:0:0:0:0:0:1"));
        // Genuine link-local / ULA.
        assert!(ipv6_is_local("fe80::1"));
        assert!(ipv6_is_local("febf::abcd"));
        assert!(ipv6_is_local("fc00::1"));
        assert!(ipv6_is_local("fd12:3456::1"));
        assert!(ipv6_is_local("[fe80::1]"));
        assert!(ipv6_is_local("fe80::1%eth0"));
        // Short hextets that the old prefix check wrongly called local:
        // fe8 = 0fe8 (global), fc = 00fc (global).
        assert!(!ipv6_is_local("fe8::1"));
        assert!(!ipv6_is_local("fc::1"));
        // fec0::/10 (site-local, deprecated) is NOT fe80::/10.
        assert!(!ipv6_is_local("fec0::1"));
        // Global and mapped-public stay public.
        assert!(!ipv6_is_local("2001:db8::1"));
        assert!(!ipv6_is_local("::ffff:8.8.8.8"));
        assert!(!ipv6_is_local("not-an-address"));
    }

    /// One unwrapped row, the shape most of these tests want.
    fn rows_from(s: &str) -> Vec<ScanRow> {
        vec![scan_row(0, s, None)]
    }

    /// A row at `row`, soft-wrapping into the next one when `wraps_at` is
    /// set (the column its text ran into, i.e. the grid's last column).
    fn scan_row(row: u16, s: &str, wraps_at: Option<u16>) -> ScanRow {
        ScanRow {
            row,
            cols: s
                .chars()
                .enumerate()
                .filter(|(_, c)| *c != ' ')
                .map(|(i, c)| (i as u16, c))
                .collect(),
            wraps_at,
        }
    }

    /// `(start, end)` column spans of the UserDir highlights detected in `s`.
    fn user_dir_spans(s: &str, privacy: bool) -> Vec<(usize, usize)> {
        let rows = rows_from(s);
        detect_highlights(&rows, &TerminalPalette::default(), privacy, &[], PrivacyClasses::default())
            .into_iter()
            .filter(|h| h.kind == HighlightKind::UserDir)
            .map(|h| (h.start_col as usize, h.end_col as usize))
            .collect()
    }

    fn masked_text(s: &str, spans: &[(usize, usize)]) -> Vec<String> {
        spans.iter().map(|&(a, b)| s[a..=b].to_string()).collect()
    }

    #[test]
    fn windows_prompt_masks_username_only() {
        let s = r"PS C:\Users\koobs> winget upgrade";
        let spans = user_dir_spans(s, true);
        assert_eq!(masked_text(s, &spans), vec!["koobs"]);
    }

    #[test]
    fn windows_deep_path_masks_username_only() {
        let s = r"C:\Users\koobs\AppData\Local\Oryxis";
        let spans = user_dir_spans(s, true);
        assert_eq!(masked_text(s, &spans), vec!["koobs"]);
    }

    #[test]
    fn windows_marker_is_case_insensitive() {
        let s = r"c:\users\bob>";
        let spans = user_dir_spans(s, true);
        assert_eq!(masked_text(s, &spans), vec!["bob"]);
    }

    #[test]
    fn linux_home_masks_username_only() {
        let s = "drwxr-xr-x /home/wilson/dev";
        let spans = user_dir_spans(s, true);
        assert_eq!(masked_text(s, &spans), vec!["wilson"]);
    }

    #[test]
    fn macos_users_masks_username_only() {
        let s = "/Users/wilson/Library/Logs";
        let spans = user_dir_spans(s, true);
        assert_eq!(masked_text(s, &spans), vec!["wilson"]);
    }

    #[test]
    fn user_dir_requires_privacy_mode() {
        let s = r"PS C:\Users\koobs>";
        assert!(user_dir_spans(s, false).is_empty());
    }

    #[test]
    fn url_paths_are_not_user_dirs() {
        let s = "GET https://cdn.example.com/users/42/avatar.png";
        assert!(user_dir_spans(s, true).is_empty());
    }

    #[test]
    fn user_dir_cells_are_privacy_cells() {
        let s = r"PS C:\Users\koobs> ";
        let rows = rows_from(s);
        let hs = detect_highlights(&rows, &TerminalPalette::default(), true, &[], PrivacyClasses::default());
        let name_start = s.find("koobs").unwrap() as u16;
        // Every cell of the name is masked; the separators around it are not.
        for col in name_start..name_start + 5 {
            assert!(is_privacy_cell(&hs, 0, col), "col {col} should be masked");
        }
        assert!(!is_privacy_cell(&hs, 0, name_start - 1));
        assert!(!is_privacy_cell(&hs, 0, name_start + 5));
        // Hover-reveal resolves the same span.
        assert_eq!(
            privacy_span_at(&hs, 0, name_start),
            Some((0, name_start, name_start + 4))
        );
    }

    #[test]
    fn user_dir_is_never_a_syntax_color() {
        let s = r"cd C:\Users\koobs";
        let rows = rows_from(s);
        let hs = detect_highlights(&rows, &TerminalPalette::default(), true, &[], PrivacyClasses::default());
        let name_start = s.find("koobs").unwrap() as u16;
        // The Windows path isn't a Unix-path highlight and UserDir itself
        // must not tint cells, so the name carries no keyword color.
        assert_eq!(highlight_color_at(&hs, 0, name_start), None);
    }

    /// `(start, end)` spans of Ip-kind highlights detected in `s`.
    fn ip_spans(s: &str) -> Vec<(usize, usize)> {
        let rows = rows_from(s);
        detect_highlights(&rows, &TerminalPalette::default(), false, &[], PrivacyClasses::default())
            .into_iter()
            .filter(|h| h.kind == HighlightKind::Ip)
            .map(|h| (h.start_col as usize, h.end_col as usize))
            .collect()
    }

    #[test]
    fn an_ip_inside_a_url_is_masked_under_privacy() {
        // The app-side redactor (redact_for_display) is not URL-aware and
        // masks a bare address inside a URL host; the terminal must agree.
        // Under Privacy Mode the host's cells are privacy cells even though
        // a Url span also covers them, while the scheme / path are not.
        let s = "see https://8.8.8.8/admin now";
        let rows = rows_from(s);
        let hs = detect_highlights(
            &rows,
            &TerminalPalette::default(),
            true,
            &[],
            PrivacyClasses::default(),
        );
        let ip_at = s.find("8.8.8.8").unwrap() as u16;
        let ip_len = "8.8.8.8".len() as u16;
        for col in ip_at..ip_at + ip_len {
            assert!(is_privacy_cell(&hs, 0, col), "col {col} in the URL host should mask");
        }
        // The '/' just before and after the host stay part of the URL, not
        // masked.
        assert!(!is_privacy_cell(&hs, 0, ip_at - 1));
        assert!(!is_privacy_cell(&hs, 0, ip_at + ip_len));

        // With Privacy Mode off, the address stays owned by the URL span
        // and is not separately highlighted (no masking to do).
        assert!(ip_spans(s).is_empty());
    }

    fn ip_texts(s: &str) -> Vec<String> {
        ip_spans(s).into_iter().map(|(a, b)| s[a..=b].to_string()).collect()
    }

    #[test]
    fn multibyte_prefix_keeps_ip_mask_on_its_cells() {
        // Two CJK chars (3 UTF-8 bytes each, one column each in the test
        // row model) precede the address: byte offsets and columns
        // diverge, and the mask must land on the address CELLS. Before
        // the byte->column remap the span sat 4 cells to the right,
        // leaking the leading half of the address on screen.
        let s = "网速 8.8.8.8 ok";
        let rows = rows_from(s);
        let hs = detect_highlights(
            &rows,
            &TerminalPalette::default(),
            true,
            &[],
            PrivacyClasses::default(),
        );
        let ip_cols: Vec<(u16, u16, u16)> = privacy_extents(&hs);
        // Columns: 网=0, 速=1, space=2, address=3..=9.
        assert_eq!(ip_cols, vec![(0, 3, 9)]);
        assert!(is_privacy_cell(&hs, 0, 3));
        assert!(is_privacy_cell(&hs, 0, 9));
        assert!(!is_privacy_cell(&hs, 0, 2));
        assert!(!is_privacy_cell(&hs, 0, 10));
        // The span text extraction (column-keyed) agrees.
        let texts = privacy_spans_with_text(&hs, &rows);
        assert_eq!(texts[0].1, "8.8.8.8");
    }

    #[test]
    fn multibyte_prefix_keeps_url_span_on_its_cells() {
        // Same unit split for the scraped-URL pass: the span must cover
        // the URL's columns, not its byte offsets.
        let s = "ü http://ex.com x";
        let rows = rows_from(s);
        let hs = detect_highlights(
            &rows,
            &TerminalPalette::default(),
            false,
            &[],
            PrivacyClasses::default(),
        );
        let url: Vec<(u16, u16)> = hs
            .iter()
            .filter(|h| h.kind == HighlightKind::Url)
            .map(|h| (h.start_col, h.end_col))
            .collect();
        // Columns: ü=0, space=1, url=2..=14 ("http://ex.com" is 13 chars).
        assert_eq!(url, vec![(2, 14)]);
    }

    #[test]
    fn ipv6_full_form_detected() {
        assert_eq!(
            ip_texts("addr 2001:0db8:85a3:0000:0000:8a2e:0370:7334 up"),
            vec!["2001:0db8:85a3:0000:0000:8a2e:0370:7334"]
        );
    }

    #[test]
    fn ipv6_compressed_forms_detected() {
        assert_eq!(ip_texts("ping ::1 ok"), vec!["::1"]);
        assert_eq!(ip_texts("via 2001:db8::1 dev"), vec!["2001:db8::1"]);
        assert_eq!(ip_texts("prefix 2001:db8:: len"), vec!["2001:db8::"]);
        assert_eq!(ip_texts("inet6 fe80::215:5dff:fe10:a3b1 scope"),
            vec!["fe80::215:5dff:fe10:a3b1"]);
    }

    #[test]
    fn ipv6_bracketed_leaves_port_visible() {
        // `[::1]:22`: the address is detected; the `:22` after the bracket
        // is a lone-colon run and stays visible.
        assert_eq!(ip_texts("connect [2001:db8::1]:8080"), vec!["2001:db8::1"]);
    }

    #[test]
    fn ipv6_trailing_prose_colon_is_trimmed() {
        assert_eq!(ip_texts("gateway 2001:db8::1: unreachable"), vec!["2001:db8::1"]);
    }

    #[test]
    fn timestamps_and_macs_are_not_ipv6() {
        assert!(ip_texts("12:34:56 log line").is_empty());
        assert!(ip_texts("mac aa:bb:cc:dd:ee:ff up").is_empty());
    }

    #[test]
    fn rust_paths_are_not_ipv6() {
        assert!(ip_texts("use std::io and Vec::new()").is_empty());
        assert!(ip_texts("err at core::fmt::Debug").is_empty());
    }

    #[test]
    fn ipv6_with_embedded_ipv4_is_fully_covered() {
        // Two side-by-side spans (hex part + dotted-quad tail from the
        // IPv4 pass); together they must cover the whole address.
        let s = "nat ::ffff:192.0.2.1 ok";
        let spans = ip_spans(s);
        let addr_start = s.find("::ffff").unwrap();
        let addr_end = s.find(" ok").unwrap() - 1;
        for col in addr_start..=addr_end {
            assert!(
                spans.iter().any(|&(a, b)| col >= a && col <= b),
                "col {col} ({}) uncovered", &s[col..=col]
            );
        }
    }

    #[test]
    fn looks_like_ipv6_validator_edges() {
        assert!(looks_like_ipv6("::1"));
        assert!(looks_like_ipv6("2001:db8::"));
        assert!(looks_like_ipv6("1:2:3:4:5:6:7:8"));
        assert!(!looks_like_ipv6("12:34:56"));
        assert!(!looks_like_ipv6("1:2:3:4:5:6:7:8:9"));
        assert!(!looks_like_ipv6("12345::1"));
        assert!(!looks_like_ipv6("1::2::3"));
        assert!(!looks_like_ipv6(":::"));
        assert!(!looks_like_ipv6(":1:2:3"));
        assert!(!looks_like_ipv6("1:2:3:"));
        assert!(!looks_like_ipv6("::")); // path separator, needs a group
        assert!(!looks_like_ipv6("g::1")); // non-hex
    }

    /// `(start, end)` spans of KnownHost-kind highlights.
    fn term_spans(s: &str, terms: &[&str]) -> Vec<String> {
        let rows = rows_from(s);
        let terms: Vec<String> = terms.iter().map(|t| t.to_string()).collect();
        detect_highlights(&rows, &TerminalPalette::default(), true, &terms, PrivacyClasses::default())
            .into_iter()
            .filter(|h| h.kind == HighlightKind::KnownHost)
            .map(|h| s[h.start_col as usize..=h.end_col as usize].to_string())
            .collect()
    }

    #[test]
    fn known_host_terms_masked_case_insensitively() {
        assert_eq!(
            term_spans("ping WEB01.prod.internal ok", &["web01.prod.internal"]),
            vec!["WEB01.prod.internal"]
        );
    }

    #[test]
    fn known_host_terms_are_token_bounded() {
        // "web01" inside "web01-backup" is a different token; only the
        // standalone occurrence matches.
        assert_eq!(
            term_spans("host web01 and web01-backup", &["web01"]),
            vec!["web01"]
        );
    }

    #[test]
    fn known_host_terms_require_privacy_mode() {
        let rows = rows_from("ping web01 ok");
        let terms = vec!["web01".to_string()];
        let hs = detect_highlights(&rows, &TerminalPalette::default(), false, &terms, PrivacyClasses::default());
        assert!(hs.iter().all(|h| h.kind != HighlightKind::KnownHost));
    }

    #[test]
    fn overlapping_privacy_spans_merge_into_one_extent() {
        // The FreeBSD prompt regression: `wilson@web01` matches the
        // prompt-token scan (HostUser) AND `web01` is a saved hostname
        // (KnownHost), two overlapping raw highlights. The extent set the
        // draw pass consumes must be ONE merged span, one bar with one
        // eye-slash, and hover / pin must resolve that same span.
        let s = "[wilson@web01 ~]$ ";
        let rows = rows_from(s);
        let terms = vec!["web01".to_string()];
        let hs = detect_highlights(&rows, &TerminalPalette::default(), true, &terms, PrivacyClasses::default());
        // Both raw detectors fired.
        assert!(hs.iter().any(|h| h.kind == HighlightKind::HostUser));
        assert!(hs.iter().any(|h| h.kind == HighlightKind::KnownHost));
        let start = s.find("wilson@web01").unwrap() as u16;
        let end = start + "wilson@web01".len() as u16 - 1;
        assert_eq!(privacy_extents(&hs), vec![(0, start, end)]);
        // Hover anywhere in the merged span (including the KnownHost
        // sub-span) reveals the whole span, matching the single bar.
        for col in start..=end {
            assert_eq!(privacy_span_at(&hs, 0, col), Some((0, start, end)));
        }
        // Pin-by-text keys the merged text, what the reveal shows.
        let texts = privacy_spans_with_text(&hs, &rows);
        assert_eq!(texts, vec![((0, start, end), "wilson@web01".to_string())]);
    }

    #[test]
    fn separate_privacy_spans_stay_separate() {
        // A gap of at least one unmasked cell keeps spans distinct: two
        // addresses separated by a space remain two bars / two reveals.
        let s = "8.8.8.8 9.9.9.9";
        let rows = rows_from(s);
        let hs = detect_highlights(&rows, &TerminalPalette::default(), true, &[], PrivacyClasses::default());
        assert_eq!(privacy_extents(&hs), vec![(0, 0, 6), (0, 8, 14)]);
    }

    /// Texts of VersionQuad-kind highlights detected in `s`.
    fn version_quad_texts(s: &str, terms: &[&str]) -> Vec<String> {
        let rows = rows_from(s);
        let terms: Vec<String> = terms.iter().map(|t| t.to_string()).collect();
        detect_highlights(&rows, &TerminalPalette::default(), true, &terms, PrivacyClasses::default())
            .into_iter()
            .filter(|h| h.kind == HighlightKind::VersionQuad)
            .map(|h| s[h.start_col as usize..=h.end_col as usize].to_string())
            .collect()
    }

    /// Ip-kind texts with privacy on and vault terms, the maskable set.
    fn masked_ip_texts(s: &str, terms: &[&str]) -> Vec<String> {
        let rows = rows_from(s);
        let terms: Vec<String> = terms.iter().map(|t| t.to_string()).collect();
        detect_highlights(&rows, &TerminalPalette::default(), true, &terms, PrivacyClasses::default())
            .into_iter()
            .filter(|h| h.kind == HighlightKind::Ip)
            .map(|h| s[h.start_col as usize..=h.end_col as usize].to_string())
            .collect()
    }

    #[test]
    fn unmarked_quad_table_masks_in_privacy() {
        // Issue #53 narrowed (per-candidate scoping): a bare four-octet
        // all-<=255 quad with NO marker glued to it is byte-for-byte an
        // IP, so Privacy Mode masks it. A winget version table and an
        // `ip route` address list are the same shape; the safe error is
        // to mask. Locally-marked versions stay VersionQuad (next test).
        // The 3-part `3.13.0` is not a quad candidate, so it never masks.
        let s = "Python 3  Python.3  3.9.0.2  3.13.0  winget";
        assert_eq!(masked_ip_texts(s, &[]), vec!["3.9.0.2"]);
        assert!(version_quad_texts(s, &[]).is_empty());

        let s2 = "Visual Studio Code  1.96.0.0  1.96.0.1";
        assert_eq!(masked_ip_texts(s2, &[]), vec!["1.96.0.0", "1.96.0.1"]);
    }

    #[test]
    fn version_marked_quads_mask_like_any_address() {
        // The issue #53 version-context exemption is gone (owner call
        // 2026-07-19): a hostile server could print `version 203.0.113.7`
        // to display a real address unmasked, and a range-valid quad is
        // byte-for-byte indistinguishable from one. Masking an actual
        // version string accidentally is the accepted error.
        assert_eq!(
            masked_ip_texts("pandoc version 3.9.0.2 installed", &[]),
            vec!["3.9.0.2"]
        );
        assert_eq!(masked_ip_texts("agent curl/8.4.0.1 sent", &[]), vec!["8.4.0.1"]);
        assert_eq!(masked_ip_texts("ver 1.2.3.4", &[]), vec!["1.2.3.4"]);
        assert_eq!(
            masked_ip_texts("version 203.0.113.7", &[]),
            vec!["203.0.113.7"]
        );
        assert!(version_quad_texts("ver 1.2.3.4", &[]).is_empty());
        assert_eq!(masked_ip_texts("rustc 1.96.0.0 available", &[]), vec!["1.96.0.0"]);
    }

    #[test]
    fn sibling_ip_not_unmasked_by_a_row_version() {
        // A genuine version token on the line must not unmask an
        // unrelated public IP sharing it (the per-candidate leak class).
        assert_eq!(
            masked_ip_texts("app 5.6.7 listening on 8.8.8.8", &[]),
            vec!["8.8.8.8"]
        );
        assert_eq!(
            masked_ip_texts("default via 203.0.113.1 dev eth0 src 203.0.113.55", &[]),
            vec!["203.0.113.1", "203.0.113.55"]
        );
    }

    #[test]
    fn oversized_octet_never_matched() {
        // `2365` fails the 3-digit/255 caps: no Ip and no VersionQuad
        // span, the current escape stays locked in.
        let s = "Microsoft Edge 122.0.2365.106 here";
        assert!(masked_ip_texts(s, &[]).is_empty());
        assert!(version_quad_texts(s, &[]).is_empty());
    }

    #[test]
    fn real_ips_still_masked() {
        assert_eq!(masked_ip_texts("ping 8.8.8.8", &[]), vec!["8.8.8.8"]);
        assert_eq!(masked_ip_texts("ssh 203.0.113.7", &[]), vec!["203.0.113.7"]);
        // Private/loopback ranges override version context.
        assert_eq!(
            masked_ip_texts("update available at 192.168.1.10", &[]),
            vec!["192.168.1.10"]
        );
        assert_eq!(masked_ip_texts("upgrade via 10.0.0.1 now", &[]), vec!["10.0.0.1"]);
    }

    #[test]
    fn repeated_address_is_not_a_version_table() {
        // `PING 8.8.8.8 (8.8.8.8)` carries two quad-dots but only one
        // DISTINCT value: an echoed endpoint, not an installed/available
        // pair, so it must stay masked (found by harness QA).
        assert_eq!(
            masked_ip_texts("PING 8.8.8.8 (8.8.8.8) 56(84) bytes of data.", &[]),
            vec!["8.8.8.8", "8.8.8.8"]
        );
    }

    #[test]
    fn vault_hostname_quad_dot_always_masked() {
        // A saved connection address wins over any version context.
        assert_eq!(
            masked_ip_texts("installed 3.9.0.2 available", &["3.9.0.2"]),
            vec!["3.9.0.2"]
        );
        assert!(version_quad_texts("installed 3.9.0.2 available", &["3.9.0.2"]).is_empty());
    }

    #[test]
    fn class_off_quads_keep_the_keyword_color_and_skip_privacy() {
        // With the public-IPs class disabled the span demotes to
        // VersionQuad: colored like an Ip span, never a privacy cell.
        // This is the only remaining path to that kind now that the
        // version-context exemption is gone.
        let s = "ping 203.0.113.7 ok";
        let rows = rows_from(s);
        let classes = PrivacyClasses {
            public_ips: false,
            ..PrivacyClasses::default()
        };
        let hs = detect_highlights(&rows, &TerminalPalette::default(), true, &[], classes);
        let start = s.find("203.0.113.7").unwrap() as u16;
        assert_eq!(
            highlight_color_at(&hs, 0, start),
            Some(TerminalPalette::default().ansi[5])
        );
        for col in start..start + 11 {
            assert!(!is_privacy_cell(&hs, 0, col), "col {col} must not mask");
        }
        assert_eq!(privacy_span_at(&hs, 0, start), None);
    }

    #[test]
    fn version_classification_is_privacy_flag_independent() {
        // With Privacy Mode off the same span exists with the same color,
        // so toggling privacy changes masking only, never coloring.
        let s = "version 3.9.0.2 ok";
        let rows = rows_from(s);
        let start = s.find("3.9.0.2").unwrap() as u16;
        let on = detect_highlights(&rows, &TerminalPalette::default(), true, &[], PrivacyClasses::default());
        let off = detect_highlights(&rows, &TerminalPalette::default(), false, &[], PrivacyClasses::default());
        assert_eq!(
            highlight_color_at(&on, 0, start),
            highlight_color_at(&off, 0, start)
        );
    }

    #[test]
    fn private_or_loopback_validator_edges() {
        assert!(ipv4_is_private_or_loopback("10.1.2.3"));
        assert!(ipv4_is_private_or_loopback("127.0.0.1"));
        assert!(ipv4_is_private_or_loopback("169.254.0.5"));
        assert!(ipv4_is_private_or_loopback("172.16.0.1"));
        assert!(ipv4_is_private_or_loopback("172.31.255.1"));
        assert!(ipv4_is_private_or_loopback("192.168.0.4"));
        assert!(!ipv4_is_private_or_loopback("172.32.0.1"));
        assert!(!ipv4_is_private_or_loopback("8.8.8.8"));
        assert!(!ipv4_is_private_or_loopback("169.253.0.1"));
        assert!(!ipv4_is_private_or_loopback("not.an.ip.at.all"));
    }

    // ── OSC 8 hyperlinks (C3) ──

    #[test]
    fn osc8_scheme_parses_only_well_formed_schemes() {
        assert_eq!(osc8_scheme("https://a.com").as_deref(), Some("https"));
        // Case-folded.
        assert_eq!(osc8_scheme("HTTPS://a.com").as_deref(), Some("https"));
        assert_eq!(osc8_scheme("mailto:x@y").as_deref(), Some("mailto"));
        // Scheme chars per RFC 3986 (`+`, `-`, `.`) are allowed in the run.
        assert_eq!(osc8_scheme("view-source:http://a").as_deref(), Some("view-source"));
        // A leading space, a control char or a digit-first run is not a scheme.
        assert_eq!(osc8_scheme(" javascript:alert(1)"), None);
        assert_eq!(osc8_scheme("java\nscript:alert(1)"), None);
        assert_eq!(osc8_scheme("1http://a"), None);
        // No colon at all.
        assert_eq!(osc8_scheme("example.com/path"), None);
    }

    #[test]
    fn osc8_scheme_allowlist_blocks_dangerous_handlers() {
        for ok in ["http://a", "https://a", "mailto:a@b", "ftp://a/f"] {
            assert!(osc8_scheme_allowed(ok), "{ok} should be allowed");
        }
        for bad in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "vscode://x",
            "ssh://host",          // deliberately not allowed yet (quick-connect follow-up)
            " https://spoof",      // leading space defeats the scheme parse
            "data:text/html,<x>",
        ] {
            assert!(!osc8_scheme_allowed(bad), "{bad} should be blocked");
        }
    }

    /// Build a term, feed OSC 8 escapes, and return its grid for the run
    /// queries. `TerminalBackend` owns the alacritty `Term` the widget reads.
    fn osc8_term(bytes: &[u8]) -> crate::backend::TerminalBackend {
        let mut backend = crate::backend::TerminalBackend::new(80, 4);
        backend.process(bytes);
        backend
    }

    #[test]
    fn osc8_run_covers_the_whole_label() {
        // `\e]8;;URI\e\\LABEL\e]8;;\e\\`; the label is 10 cells ("click here").
        let b = osc8_term(b"\x1b]8;;https://example.com\x1b\\click here\x1b]8;;\x1b\\");
        let hit = osc8_link_at_cell(&b.term, 0, 3).expect("cell inside the label is a link");
        assert_eq!(hit, ("https://example.com".to_string(), 0, 9));
        // A cell past the label carries no link.
        assert!(osc8_link_at_cell(&b.term, 0, 20).is_none());
    }

    #[test]
    fn osc8_adjacent_distinct_links_do_not_merge() {
        // Two back-to-back links with different ids AND uris must stay
        // separate runs, never a single run spanning both.
        let b = osc8_term(
            b"\x1b]8;id=1;https://a.com\x1b\\AAA\x1b]8;;\x1b\\\x1b]8;id=2;https://b.com\x1b\\BBB\x1b]8;;\x1b\\",
        );
        let first = osc8_link_at_cell(&b.term, 0, 1).expect("first link");
        assert_eq!(first, ("https://a.com".to_string(), 0, 2));
        let second = osc8_link_at_cell(&b.term, 0, 4).expect("second link");
        assert_eq!(second, ("https://b.com".to_string(), 3, 5));
    }

    #[test]
    fn osc8_link_at_cell_ignores_plain_text() {
        // Bare text (no OSC 8) has no hyperlink attribute, even when it looks
        // like a URL, that path is the scraped `url_at_cell`, not this one.
        let b = osc8_term(b"visit https://plain.example.com now");
        assert!(osc8_link_at_cell(&b.term, 0, 10).is_none());
    }

    /// Build a narrow term so a long label wraps across rows.
    fn osc8_narrow_term(bytes: &[u8]) -> crate::backend::TerminalBackend {
        let mut backend = crate::backend::TerminalBackend::new(10, 4);
        backend.process(bytes);
        backend
    }

    #[test]
    fn osc8_run_follows_a_wrapped_link_across_rows() {
        // A 13-char label on a 10-col grid wraps: row 0 cols 0..9, row 1
        // cols 0..2. alacritty carries the same hyperlink across the wrap.
        let b = osc8_narrow_term(b"\x1b]8;;https://example.com\x1b\\ABCDEFGHIJKLM\x1b]8;;\x1b\\");
        let (uri, segs) = osc8_link_run(&b.term, 0, 5).expect("run from the top row");
        assert_eq!(uri, "https://example.com");
        assert_eq!(segs, vec![(0, 0, 9), (1, 0, 2)]);
        // Hovering the tail row resolves the identical full run.
        let (_, from_tail) = osc8_link_run(&b.term, 1, 1).expect("run from the tail row");
        assert_eq!(from_tail, vec![(0, 0, 9), (1, 0, 2)]);
    }

    #[test]
    fn osc8_run_does_not_merge_stacked_distinct_links() {
        // Link A exactly fills row 0 (10 chars, flush to the edge); link B
        // starts row 1. Different ids, so the walk must NOT treat B as A's
        // wrap even though A is flush-right and B is flush-left.
        let b = osc8_narrow_term(
            b"\x1b]8;id=1;https://a.com\x1b\\AAAAAAAAAA\x1b]8;;\x1b\\\x1b]8;id=2;https://b.com\x1b\\BBB\x1b]8;;\x1b\\",
        );
        let (uri_a, segs_a) = osc8_link_run(&b.term, 0, 4).expect("link A");
        assert_eq!(uri_a, "https://a.com");
        assert_eq!(segs_a, vec![(0, 0, 9)]);
        let (uri_b, segs_b) = osc8_link_run(&b.term, 1, 1).expect("link B");
        assert_eq!(uri_b, "https://b.com");
        assert_eq!(segs_b, vec![(1, 0, 2)]);
    }

    fn user_rule(pattern: &str, color: Color) -> crate::highlight_rules::CompiledRule {
        crate::highlight_rules::CompiledRule::new("r", "n", pattern, false, false, color, false)
            .unwrap()
    }

    #[test]
    fn a_user_rule_paints_every_occurrence() {
        let rows = rows_from("ERROR here and ERROR there");
        let red = Color::from_rgb8(255, 0, 0);
        let hs = detect_rule_highlights(&rows, &[user_rule("ERROR", red)]);
        assert_eq!(hs.len(), 2);
        assert_eq!((hs[0].start_col, hs[0].end_col), (0, 4));
        assert_eq!((hs[1].start_col, hs[1].end_col), (15, 19));
        assert_eq!(highlight_color_at(&hs, 0, 2), Some(red));
        // Between the two matches nothing is painted.
        assert_eq!(highlight_color_at(&hs, 0, 8), None);
    }

    #[test]
    fn a_user_rule_after_a_wide_character_lands_on_the_right_columns() {
        // Same trap the automatic detectors hit: a multi-byte char shifts
        // every later BYTE offset past its column, so an unremapped span
        // would paint the wrong cells.
        let rows = rows_from("日本ERROR");
        let red = Color::from_rgb8(255, 0, 0);
        let hs = detect_rule_highlights(&rows, &[user_rule("ERROR", red)]);
        assert_eq!(hs.len(), 1);
        assert_eq!((hs[0].start_col, hs[0].end_col), (2, 6));
    }

    #[test]
    fn a_user_rule_cannot_paint_the_blank_padding_after_the_text() {
        // Rows are padded with blanks out to the last printable column;
        // a whitespace rule must not treat that padding as content.
        let rows = rows_from("done");
        let hs = detect_rule_highlights(
            &rows,
            &[crate::highlight_rules::CompiledRule::new(
                "r",
                "n",
                r"\s+",
                true,
                false,
                Color::WHITE,
                false,
            )
            .unwrap()],
        );
        assert!(hs.is_empty());
    }

    #[test]
    fn rule_spans_are_not_privacy_spans() {
        // A rule matching an address must not make it maskable, and must
        // not stop the address's own detector from masking it either:
        // the two passes are separate lists.
        let rows = rows_from("host 8.8.8.8 up");
        let rule_hs = detect_rule_highlights(&rows, &[user_rule("8.8.8.8", Color::WHITE)]);
        assert!(!rule_hs.is_empty());
        assert!(!is_privacy_cell(&rule_hs, 0, 6));
        let auto = detect_highlights(
            &rows,
            &TerminalPalette::default(),
            true,
            &[],
            PrivacyClasses::default(),
        );
        assert!(is_privacy_cell(&auto, 0, 6));
    }

    // ── Scraped URLs across soft wraps ──

    /// A grid narrow enough that a realistic URL wraps, the way an OAuth
    /// authorize URL does in any normal pane.
    fn wrapped_url_term(text: &[u8]) -> crate::backend::TerminalBackend {
        let mut backend = crate::backend::TerminalBackend::new(20, 6);
        backend.process(text);
        backend
    }

    #[test]
    fn scraped_url_run_joins_a_soft_wrapped_url() {
        // 45 chars on a 20-col grid: rows 0 and 1 full, row 2 holds the
        // 5-char tail. Every row of the run resolves the same whole URL.
        let url = "https://example.com/authorize?code=abcdefgh12";
        let b = wrapped_url_term(url.as_bytes());
        for (line, col) in [(0, 0), (0, 19), (1, 5), (2, 3)] {
            let (hit, segs) = url_run_at_cell(&b.term, line, col)
                .unwrap_or_else(|| panic!("no link at {line},{col}"));
            assert_eq!(hit, url);
            assert_eq!(segs, vec![(0, 0, 19), (1, 0, 19), (2, 0, 4)]);
        }
    }

    #[test]
    fn scraped_url_run_stops_at_a_hard_line_break() {
        // Row 0 ends in a real newline, so row 1 is a new logical line:
        // the URL must not swallow the text under it (the failure that
        // a naive "join the next row" would produce).
        let b = wrapped_url_term(b"see https://a.co
not-part-of-it");
        let (hit, segs) = url_run_at_cell(&b.term, 0, 6).expect("link on row 0");
        assert_eq!(hit, "https://a.co");
        assert_eq!(segs, vec![(0, 4, 15)]);
        assert!(url_run_at_cell(&b.term, 1, 2).is_none());
    }

    #[test]
    fn scraped_url_run_keeps_the_row_local_rules() {
        // Trailing sentence punctuation is trimmed, a cell outside the
        // token is not a link, and the space still ends the token.
        let b = wrapped_url_term(b"go https://a.co/x. now");
        assert_eq!(
            url_at_cell(&b.term, 0, 5).as_deref(),
            Some("https://a.co/x")
        );
        assert!(url_at_cell(&b.term, 0, 1).is_none());
        assert!(url_at_cell(&b.term, 0, 19).is_none());
    }

    #[test]
    fn scraped_url_run_picks_the_token_under_the_cell() {
        // Two URLs on one wrapped logical line: the hit is the one the
        // cell sits in, not the first one in the line.
        let b = wrapped_url_term(b"https://a.co/one https://b.co/two");
        assert_eq!(
            url_at_cell(&b.term, 0, 2).as_deref(),
            Some("https://a.co/one")
        );
        assert_eq!(
            url_at_cell(&b.term, 1, 2).as_deref(),
            Some("https://b.co/two")
        );
    }

    #[test]
    fn a_chain_longer_than_the_walk_still_opens_whole_from_its_middle() {
        // `MAX_WRAP_ROWS` rows above the click and more below it: with
        // one budget shared between the two directions, the walk up
        // spends all of it and the walk down never runs, so the click
        // opens the link cut off at its own row. Which is the truncation
        // the whole reassembly exists to stop, just further along the
        // link than the row-local scan used to stop at.
        let url = format!("https://a.co/{}", "x".repeat(1387));
        let mut b = crate::backend::TerminalBackend::new(20, 80);
        b.process(url.as_bytes());
        // 1400 chars at 20 columns is 70 rows; row 64 is exactly the
        // budget below the head, with 5 rows of tail under it.
        assert_eq!(url_at_cell(&b.term, 64, 5).as_deref(), Some(url.as_str()));
    }

    // -- Wrapped-URL colouring (the carry) --

    /// `(row, start_col, end_col)` of every URL highlight, in order.
    fn url_spans(rows: &[ScanRow]) -> Vec<(u16, u16, u16)> {
        detect_highlights(rows, &TerminalPalette::default(), false, &[], PrivacyClasses::default())
            .into_iter()
            .filter(|h| h.kind == HighlightKind::Url)
            .map(|h| (h.row, h.start_col, h.end_col))
            .collect()
    }

    #[test]
    fn a_wrapped_url_is_coloured_on_every_row_it_covers() {
        // Row 0 is full to column 9 and wraps; row 1 holds the tail. Both
        // rows belong to the one link, so both are coloured (this is what
        // the hover underline already did and the colour did not).
        let rows = vec![
            scan_row(0, "http://a.co", Some(10)),
            scan_row(1, "/deep/path x", None),
        ];
        assert_eq!(url_spans(&rows), vec![(0, 0, 10), (1, 0, 9)]);
    }

    #[test]
    fn a_wrapped_url_carries_across_three_rows() {
        let rows = vec![
            scan_row(0, "http://a.co", Some(10)),
            scan_row(1, "0123456789", Some(9)),
            scan_row(2, "tail", None),
        ];
        assert_eq!(url_spans(&rows), vec![(0, 0, 10), (1, 0, 9), (2, 0, 3)]);
    }

    #[test]
    fn a_hard_line_break_carries_nothing() {
        // Row 0 ends its logical line (`wraps_at` is None), so row 1 is
        // unrelated text and must keep its own colour.
        let rows = vec![
            scan_row(0, "http://a.co", None),
            scan_row(1, "/not-part-of-it", None),
        ];
        assert_eq!(url_spans(&rows), vec![(0, 0, 10)]);
    }

    #[test]
    fn a_url_that_stops_before_the_margin_carries_nothing() {
        // The row wraps, but the link ended at column 10 with blanks
        // after it: those blanks are real content of the logical line, so
        // they end the URL and row 1 is not part of it.
        let rows = vec![
            scan_row(0, "http://a.co", Some(20)),
            scan_row(1, "still-not-part", None),
        ];
        assert_eq!(url_spans(&rows), vec![(0, 0, 10)]);
    }

    #[test]
    fn a_carry_reaches_only_the_next_row() {
        // Row 2 does not follow row 0, so a carry that outlived its row
        // must not paint it (a blank row never reaches the scanner, which
        // is exactly how a stale carry could have skipped one).
        let rows = vec![
            scan_row(0, "http://a.co", Some(10)),
            scan_row(2, "unrelated", None),
        ];
        assert_eq!(url_spans(&rows), vec![(0, 0, 10)]);
    }

    #[test]
    fn punctuation_is_trimmed_at_the_end_but_not_at_a_wrap() {
        // A `.` at the wrap margin is interior text: trimming it there
        // would break the span and drop the carry. At the real end of the
        // link it is prose again and comes off.
        let rows = vec![
            scan_row(0, "http://a.co.", Some(11)),
            scan_row(1, "au/x.", None),
        ];
        assert_eq!(url_spans(&rows), vec![(0, 0, 11), (1, 0, 3)]);
    }

    #[test]
    fn a_second_url_after_a_carried_tail_is_still_found() {
        // The scan resumes past the tail, so the row's own link is not
        // swallowed by the carry.
        let rows = vec![
            scan_row(0, "http://a.co", Some(10)),
            scan_row(1, "/x http://b.co", None),
        ];
        assert_eq!(url_spans(&rows), vec![(0, 0, 10), (1, 0, 1), (1, 3, 13)]);
    }
}
