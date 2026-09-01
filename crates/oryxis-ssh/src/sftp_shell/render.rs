//! Formatting the console's output: listings, the progress meter, sizes.
//!
//! Everything here is pure, so the awkward cases (an entry the server
//! described without permissions, a transfer whose total is unknown, a
//! name twice as wide as it is long) are covered by tests rather than by
//! looking at a screen.
//!
//! Output is English and does NOT go through `i18n::t`. That is a
//! deliberate departure from the repo's rule, decided with the owner: the
//! console is a clone of `sftp(1)`, and a localized `ls -l` breaks muscle
//! memory, breaks anything pasted from a tutorial, and breaks the eye
//! that scans a listing by shape. The UI labels that OPEN the console are
//! translated like everything else.

use unicode_width::UnicodeWidthStr;

use crate::sftp::SftpEntry;

use super::parser::LsOpts;

/// A console line ends with CRLF, not LF: the emulator is in the state a
/// PTY leaves it in, where a bare LF moves down without returning to
/// column zero and the output walks off to the right.
pub const CRLF: &str = "\r\n";

/// OSC 133, the FinalTerm semantic-prompt marks, which the console emits
/// around its own prompt and commands exactly as a shell with
/// integration installed would.
///
/// It costs four short strings and buys the things that read a session's
/// STRUCTURE rather than its text: the tab's activity indicator knows a
/// command is running and whether it failed, and a recording carries the
/// boundaries a per-command transcript needs. The console is in the rare
/// position of being able to emit these perfectly, because unlike a
/// shell it is not guessing where its own prompt ends.
///
/// `E` (the command line as parsed, OSC 633) is deliberately NOT emitted.
/// It feeds the command HISTORY, which is per host and exists to be
/// re-inserted into a shell, where `get access.log` is not a command.
/// See `command_capture` for the matching gate on the reading side.
pub mod marks {
    /// `A`: a prompt is about to be drawn.
    pub const PROMPT_START: &str = "\x1b]133;A\x1b\\";
    /// `B`: the prompt is drawn and the command line starts here.
    pub const PROMPT_END: &str = "\x1b]133;B\x1b\\";
    /// `C`: the command is running and what follows is its output.
    pub const OUTPUT_START: &str = "\x1b]133;C\x1b\\";

    /// `D`: the command finished, with its exit status.
    pub fn command_end(failed: bool) -> String {
        // A console command either worked or reported why. There is no
        // richer status to pass on, so the two values a reader acts on
        // are the two it gets.
        format!("\x1b]133;D;{}\x1b\\", i32::from(failed))
    }
}

/// Format a size the way `ls -h` does: three significant digits and a
/// unit suffix. Below 1K there is no suffix, matching `sftp(1)`.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["", "K", "M", "G", "T", "P"];
    if bytes < 1024 {
        return bytes.to_string();
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    // Under 10 the tenth matters (1.5G), above it the digit does not
    // (234M), which is what keeps the column narrow and readable.
    if value < 10.0 {
        format!("{value:.1}{}", UNITS[unit])
    } else {
        format!("{value:.0}{}", UNITS[unit])
    }
}

/// The `drwxr-xr-x` column.
///
/// `None` permissions come from a server that did not send them; the
/// answer is `?` per position rather than a guess, because a listing that
/// invents `rw-` is worse than one that admits it does not know.
pub fn mode_string(entry: &SftpEntry) -> String {
    let kind = if entry.is_symlink {
        'l'
    } else if entry.is_dir {
        'd'
    } else {
        '-'
    };
    let Some(mode) = entry.permissions else {
        return format!("{kind}?????????");
    };
    let mut s = String::with_capacity(10);
    s.push(kind);
    for shift in [6, 3, 0] {
        let bits = (mode >> shift) & 0o7;
        s.push(if bits & 0o4 != 0 { 'r' } else { '-' });
        s.push(if bits & 0o2 != 0 { 'w' } else { '-' });
        s.push(if bits & 0o1 != 0 { 'x' } else { '-' });
    }
    // setuid / setgid / sticky replace the matching execute bit, which is
    // what `ls` does and what makes a 4755 visibly different from a 0755.
    if mode & 0o4000 != 0 {
        replace_at(&mut s, 3, if mode & 0o100 != 0 { 's' } else { 'S' });
    }
    if mode & 0o2000 != 0 {
        replace_at(&mut s, 6, if mode & 0o010 != 0 { 's' } else { 'S' });
    }
    if mode & 0o1000 != 0 {
        replace_at(&mut s, 9, if mode & 0o001 != 0 { 't' } else { 'T' });
    }
    s
}

fn replace_at(s: &mut String, idx: usize, c: char) {
    let mut chars: Vec<char> = s.chars().collect();
    if idx < chars.len() {
        chars[idx] = c;
        *s = chars.into_iter().collect();
    }
}

/// `Aug 26 03:11` for something within the last six months, `Aug 26
/// 2024` for anything older. Same rule as `ls`, and the reason is the
/// same: the recent half of a listing is where the time matters.
pub fn format_mtime(mtime: Option<u32>, now: i64) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let Some(secs) = mtime else {
        return "            ".to_string();
    };
    let (year, month, day, hour, minute) = civil_from_epoch(i64::from(secs));
    let month_name = MONTHS[(month as usize).saturating_sub(1).min(11)];
    // Six months, give or take: the exact boundary does not matter, the
    // switch from clock to year does.
    const SIX_MONTHS: i64 = 182 * 24 * 60 * 60;
    if (now - i64::from(secs)).abs() < SIX_MONTHS {
        format!("{month_name} {day:>2} {hour:02}:{minute:02}")
    } else {
        format!("{month_name} {day:>2}  {year}")
    }
}

/// Days-from-epoch to a civil date, Howard Hinnant's algorithm. Written
/// out rather than pulled from `chrono` because this crate has no date
/// dependency and one listing column does not justify adding one.
fn civil_from_epoch(secs: i64) -> (i64, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let hour = (rem / 3600) as u32;
    let minute = ((rem % 3600) / 60) as u32;

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d, hour, minute)
}

/// A remote name with everything that is not text replaced by `?`.
///
/// A file name is remote input and it is about to be written to a
/// terminal that interprets escape sequences. Left raw, a name can
/// clear the screen, retitle the window, forge the OSC 133 marks this
/// console emits around its own prompt, or reach the clipboard through
/// OSC 52, which is on by default. None of that is the listing the user
/// asked for.
///
/// `?` rather than a placeholder glyph because that is what `ls -q`
/// does, and because it is one column wide, so the column arithmetic
/// downstream keeps agreeing with what lands on screen.
///
/// Display only. The name used to BUILD a path is the real one, or a
/// file whose name contains a tab would stop being downloadable.
pub fn display_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

/// Render a directory listing.
///
/// `now` is passed in rather than read, so the six-month boundary is
/// testable.
///
/// **Owner and group are numeric, always.** The SFTP v3 listing carries a
/// ready-made `ls -l` line in its `longname` field, with names already
/// resolved by the server, and `russh-sftp`'s `ReadDir` discards it
/// before we ever see it (`client/fs/dir.rs`). So the console formats
/// from the numeric attributes, which is exactly what `ls -n` shows. The
/// fix is upstream, not a fork.
pub fn render_listing(entries: &[SftpEntry], opts: &LsOpts, now: i64, cols: u16) -> String {
    // Sanitized up front, once, so the sort, the width arithmetic and
    // the bytes written can never be looking at different names.
    let entries: Vec<SftpEntry> = entries
        .iter()
        .map(|e| SftpEntry {
            name: display_name(&e.name),
            ..e.clone()
        })
        .collect();
    let mut visible: Vec<&SftpEntry> = entries
        .iter()
        .filter(|e| opts.all || !e.name.starts_with('.'))
        .collect();

    if !opts.unsorted {
        // Sort keys are compared, then the tie broken by name, so a
        // listing is stable rather than reordering between calls.
        if opts.by_size {
            visible.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.name.cmp(&b.name)));
        } else if opts.by_time {
            visible.sort_by(|a, b| {
                b.mtime
                    .unwrap_or(0)
                    .cmp(&a.mtime.unwrap_or(0))
                    .then_with(|| a.name.cmp(&b.name))
            });
        } else {
            visible.sort_by(|a, b| a.name.cmp(&b.name));
        }
        if opts.reverse {
            visible.reverse();
        }
    }

    if opts.long {
        return long_listing(&visible, opts.human, now);
    }
    if opts.one_per_line {
        let mut out = String::new();
        for e in &visible {
            out.push_str(&e.name);
            out.push_str(CRLF);
        }
        return out;
    }
    columnize(&visible, cols)
}

fn long_listing(entries: &[&SftpEntry], human: bool, now: i64) -> String {
    // Widths are measured over the rows about to be printed, not fixed,
    // so a listing of small files does not carry a column of padding
    // sized for a file that is not there.
    let size_strings: Vec<String> = entries
        .iter()
        .map(|e| {
            if human {
                human_size(e.size)
            } else {
                e.size.to_string()
            }
        })
        .collect();
    let owner_strings: Vec<String> = entries
        .iter()
        .map(|e| e.uid.map(|u| u.to_string()).unwrap_or_else(|| "?".into()))
        .collect();
    let group_strings: Vec<String> = entries
        .iter()
        .map(|e| e.gid.map(|g| g.to_string()).unwrap_or_else(|| "?".into()))
        .collect();

    let size_w = size_strings.iter().map(|s| s.len()).max().unwrap_or(0);
    let owner_w = owner_strings.iter().map(|s| s.len()).max().unwrap_or(0);
    let group_w = group_strings.iter().map(|s| s.len()).max().unwrap_or(0);

    let mut out = String::new();
    for (i, e) in entries.iter().enumerate() {
        out.push_str(&format!(
            "{} {:>owner_w$} {:>group_w$} {:>size_w$} {} {}",
            mode_string(e),
            owner_strings[i],
            group_strings[i],
            size_strings[i],
            format_mtime(e.mtime, now),
            e.name,
        ));
        out.push_str(CRLF);
    }
    out
}

/// Lay names out in columns that fit `cols`, the way a bare `ls` does.
///
/// Widths are display widths, so a listing of CJK names lines up instead
/// of drifting one column per character.
fn columnize(entries: &[&SftpEntry], cols: u16) -> String {
    if entries.is_empty() {
        return String::new();
    }
    const GAP: usize = 2;
    let width = cols.max(1) as usize;
    let widest = entries.iter().map(|e| e.name.width()).max().unwrap_or(1);
    let col_w = widest + GAP;
    let per_row = (width / col_w).max(1);
    let rows = entries.len().div_ceil(per_row);

    let mut out = String::new();
    for row in 0..rows {
        // Column-major, like `ls`: reading down a column is what a user
        // does with an alphabetical listing.
        let mut line = String::new();
        for col in 0..per_row {
            let Some(entry) = entries.get(col * rows + row) else {
                continue;
            };
            let name_w = entry.name.width();
            line.push_str(&entry.name);
            // No padding after the last name on a row, so the line has no
            // invisible trailing run for a selection to pick up.
            if col + 1 < per_row && (col + 1) * rows + row < entries.len() {
                line.push_str(&" ".repeat(col_w.saturating_sub(name_w)));
            }
        }
        out.push_str(line.trim_end());
        out.push_str(CRLF);
    }
    out
}

/// One frame of the transfer progress meter, drawn in place with a
/// leading CR so it overwrites itself.
///
/// `total` of `None` means the size is unknown, which happens on a stream
/// the server would not stat. The bar is dropped in that case rather than
/// invented: a bar that does not track anything is a lie the user reads
/// as progress.
///
/// **The line never exceeds `cols`.** A meter that overflows wraps, and a
/// wrapped meter redrawn with a bare CR paints its second row over the
/// output above it, so a long transfer slowly eats the scrollback. That
/// is why the layout is built by DROPPING fields in reverse order of
/// importance rather than by assuming a comfortable window: percentage
/// and bytes always survive, then the rate, then the ETA, and the bar
/// takes whatever is left only if there is enough of it to mean
/// something.
pub fn progress_line(
    name: &str,
    done: u64,
    total: Option<u64>,
    rate: f64,
    elapsed_secs: f64,
    cols: u16,
) -> String {
    /// Below this the bar carries no information worth its columns.
    const MIN_BAR: usize = 6;

    let width = cols.max(1) as usize;
    let rate_s = format!("{}/s", human_size(rate as u64));
    let done_s = human_size(done);

    let Some(total) = total.filter(|t| *t > 0) else {
        // Unknown total: report what is certain (bytes moved and how
        // fast) and nothing else.
        let text = format!("{name} {done_s} {rate_s}");
        return format!("\r{}\x1b[K", truncate_to(&text, width));
    };

    let pct = ((done as f64 / total as f64) * 100.0).min(100.0);
    let eta = if done >= total {
        format!(
            "{:02}:{:02}",
            (elapsed_secs as u64) / 60,
            (elapsed_secs as u64) % 60
        )
    } else if rate > 1.0 {
        let remaining = (total - done) as f64 / rate;
        format!(
            "{:02}:{:02} ETA",
            (remaining as u64) / 60,
            (remaining as u64) % 60
        )
    } else {
        "--:-- ETA".to_string()
    };

    // Built longest-first; the first one that fits wins. The name is
    // budgeted a third of the window in the roomy layouts and dropped
    // entirely in the tightest, because "which file" matters less than
    // "how far along" when there is only one line to say it in.
    let name_budget = (width / 3).max(4);
    let short_name = truncate_to(name, name_budget.min(name.width()));
    let candidates = [
        format!(" {pct:>3.0}% {done_s:>8} {rate_s:>10} {eta:>12}"),
        format!(" {pct:>3.0}% {done_s:>8} {rate_s:>10}"),
        format!(" {pct:>3.0}% {done_s:>8}"),
        format!(" {pct:>3.0}%"),
    ];

    for tail in &candidates {
        // 3 columns of overhead: the space after the name and the two
        // brackets around the bar.
        let fixed = short_name.width() + tail.width() + 3;
        if fixed + MIN_BAR <= width {
            let bar_w = width - fixed;
            let filled = ((pct / 100.0) * bar_w as f64).round() as usize;
            let bar = "#".repeat(filled.min(bar_w));
            let pad = " ".repeat(bar_w - bar.width());
            return format!("\r{short_name} [{bar}{pad}]{tail}\x1b[K");
        }
    }
    // No room for a bar at any tail: drop the bar, then the name, then
    // truncate whatever is left. Something always renders.
    for tail in &candidates {
        let text = format!("{short_name}{tail}");
        if text.width() <= width {
            return format!("\r{text}\x1b[K");
        }
    }
    let bare = format!("{pct:.0}%");
    format!("\r{}\x1b[K", truncate_to(&bare, width))
}

/// Cut a string to `width` DISPLAY columns, not bytes and not characters.
fn truncate_to(s: &str, width: usize) -> String {
    if s.width() <= width {
        return s.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if used + w > width {
            break;
        }
        out.push(c);
        used += w;
    }
    out
}

/// The `help` table. Ordered by what a person reaches for, not
/// alphabetically: navigation, then listing, then transfer, then the
/// rest.
pub fn help_text() -> String {
    const ROWS: &[(&str, &str)] = &[
        (
            "cd [path]",
            "change the remote directory (no argument: home)",
        ),
        ("lcd [path]", "change the local directory"),
        ("pwd", "print the remote directory"),
        ("lpwd", "print the local directory"),
        ("ls [-1afhlnrSt] [path]", "list the remote directory"),
        ("lls [-1afhlnrSt] [path]", "list the local directory"),
        (
            "get [-afpr] remote [local]",
            "download; remote may be a glob",
        ),
        ("mget remote", "download matching files (get with a glob)"),
        ("reget remote [local]", "resume an interrupted download"),
        ("put [-afpr] local [remote]", "upload; local may be a glob"),
        ("mput local", "upload matching files (put with a glob)"),
        ("reput local [remote]", "resume an interrupted upload"),
        ("rm path [path ...]", "delete remote files"),
        ("mkdir path", "create a remote directory"),
        ("lmkdir path", "create a local directory"),
        ("rmdir path", "remove a remote directory"),
        ("rename old new", "rename a remote file"),
        ("chmod mode path", "change remote permissions (octal)"),
        ("progress", "toggle the transfer progress meter"),
        ("version", "show the SFTP protocol version"),
        ("help, ?", "this list"),
        ("bye, quit, exit", "close the console"),
    ];
    let widest = ROWS.iter().map(|(cmd, _)| cmd.len()).max().unwrap_or(0);
    let mut out = String::new();
    for (cmd, description) in ROWS {
        out.push_str(&format!("{cmd:<widest$}  {description}"));
        out.push_str(CRLF);
    }
    out.push_str(CRLF);
    out.push_str("Owner and group are shown numerically: the server's own");
    out.push_str(CRLF);
    out.push_str("formatted listing is not available through this client.");
    out.push_str(CRLF);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, is_dir: bool, size: u64, mode: u32) -> SftpEntry {
        SftpEntry {
            name: name.to_string(),
            is_dir,
            is_symlink: false,
            size,
            mtime: Some(1_700_000_000),
            permissions: Some(mode),
            uid: Some(1000),
            gid: Some(1000),
        }
    }

    // --- sizes ------------------------------------------------------

    #[test]
    fn human_sizes_follow_ls_h() {
        assert_eq!(human_size(0), "0");
        assert_eq!(human_size(999), "999");
        assert_eq!(human_size(1024), "1.0K");
        assert_eq!(human_size(1536), "1.5K");
        assert_eq!(human_size(10 * 1024), "10K");
        assert_eq!(human_size(1024 * 1024), "1.0M");
        assert_eq!(human_size(1024 * 1024 * 1024), "1.0G");
    }

    #[test]
    fn human_sizes_do_not_overflow_the_unit_table() {
        // Well past petabytes: the unit must clamp rather than index off
        // the end of the table.
        assert!(human_size(u64::MAX).ends_with('P'));
    }

    // --- mode -------------------------------------------------------

    #[test]
    fn mode_renders_the_usual_shapes() {
        assert_eq!(mode_string(&entry("f", false, 0, 0o644)), "-rw-r--r--");
        assert_eq!(mode_string(&entry("d", true, 0, 0o755)), "drwxr-xr-x");
        assert_eq!(mode_string(&entry("x", false, 0, 0o000)), "----------");
        assert_eq!(mode_string(&entry("x", false, 0, 0o777)), "-rwxrwxrwx");
    }

    #[test]
    fn setuid_setgid_and_sticky_replace_the_execute_bit() {
        assert_eq!(mode_string(&entry("f", false, 0, 0o4755)), "-rwsr-xr-x");
        assert_eq!(mode_string(&entry("f", false, 0, 0o4644)), "-rwSr--r--");
        assert_eq!(mode_string(&entry("f", false, 0, 0o2755)), "-rwxr-sr-x");
        assert_eq!(mode_string(&entry("d", true, 0, 0o1777)), "drwxrwxrwt");
    }

    #[test]
    fn a_symlink_is_marked_even_when_it_points_at_a_directory() {
        let mut e = entry("link", true, 0, 0o777);
        e.is_symlink = true;
        assert!(mode_string(&e).starts_with('l'));
    }

    /// A server that sent no permissions gets question marks, not an
    /// invented `rw-`.
    #[test]
    fn missing_permissions_are_admitted() {
        let mut e = entry("f", false, 0, 0o644);
        e.permissions = None;
        assert_eq!(mode_string(&e), "-?????????");
    }

    // --- time -------------------------------------------------------

    #[test]
    fn recent_times_show_a_clock_and_old_ones_a_year() {
        let now = 1_700_000_000;
        assert!(format_mtime(Some(now as u32), now).contains(':'));
        let two_years_ago = now - 2 * 365 * 24 * 3600;
        let old = format_mtime(Some(two_years_ago as u32), now);
        assert!(old.contains("2021"), "got {old:?}");
    }

    #[test]
    fn a_known_epoch_renders_the_right_date() {
        // 2023-11-14 22:13:20 UTC
        assert_eq!(
            format_mtime(Some(1_700_000_000), 1_700_000_000),
            "Nov 14 22:13"
        );
    }

    #[test]
    fn an_unknown_time_takes_the_column_without_inventing_one() {
        let s = format_mtime(None, 1_700_000_000);
        assert!(s.trim().is_empty());
        assert_eq!(s.len(), 12);
    }

    // --- listing ----------------------------------------------------

    #[test]
    fn a_short_listing_hides_dotfiles_unless_asked() {
        let entries = vec![
            entry(".hidden", false, 1, 0o644),
            entry("shown", false, 1, 0o644),
        ];
        let opts = LsOpts::default();
        let out = render_listing(&entries, &opts, 0, 80);
        assert!(!out.contains(".hidden"));
        assert!(out.contains("shown"));

        let opts = LsOpts {
            all: true,
            ..Default::default()
        };
        assert!(render_listing(&entries, &opts, 0, 80).contains(".hidden"));
    }

    #[test]
    fn sorting_by_size_time_and_name_with_reverse() {
        let entries = vec![
            entry("b", false, 10, 0o644),
            entry("a", false, 30, 0o644),
            entry("c", false, 20, 0o644),
        ];
        let by_name = render_listing(
            &entries,
            &LsOpts {
                one_per_line: true,
                ..Default::default()
            },
            0,
            80,
        );
        assert_eq!(by_name, "a\r\nb\r\nc\r\n");

        let by_size = render_listing(
            &entries,
            &LsOpts {
                one_per_line: true,
                by_size: true,
                ..Default::default()
            },
            0,
            80,
        );
        assert_eq!(by_size, "a\r\nc\r\nb\r\n");

        let reversed = render_listing(
            &entries,
            &LsOpts {
                one_per_line: true,
                reverse: true,
                ..Default::default()
            },
            0,
            80,
        );
        assert_eq!(reversed, "c\r\nb\r\na\r\n");
    }

    /// `-f` means "do not sort", and honouring it is how a user inspects
    /// the order the server actually answered in.
    #[test]
    fn unsorted_keeps_the_server_order() {
        let entries = vec![entry("z", false, 1, 0o644), entry("a", false, 1, 0o644)];
        let out = render_listing(
            &entries,
            &LsOpts {
                one_per_line: true,
                unsorted: true,
                ..Default::default()
            },
            0,
            80,
        );
        assert_eq!(out, "z\r\na\r\n");
    }

    #[test]
    fn the_long_format_has_every_column() {
        let entries = vec![entry("access.log", false, 2847362, 0o644)];
        let opts = LsOpts {
            long: true,
            ..Default::default()
        };
        let out = render_listing(&entries, &opts, 1_700_000_000, 80);
        assert!(out.starts_with("-rw-r--r-- 1000 1000 2847362 Nov 14 22:13 access.log"));
        assert!(out.ends_with(CRLF));
    }

    #[test]
    fn the_long_format_honours_human_sizes() {
        let entries = vec![entry("big", false, 2 * 1024 * 1024, 0o644)];
        let opts = LsOpts {
            long: true,
            human: true,
            ..Default::default()
        };
        assert!(render_listing(&entries, &opts, 0, 80).contains("2.0M"));
    }

    /// Columns are sized from the rows being printed, so a listing of
    /// small files does not carry padding for a file that is not there.
    #[test]
    fn long_columns_size_themselves_to_the_content() {
        let entries = vec![entry("a", false, 1, 0o644), entry("b", false, 22, 0o644)];
        let opts = LsOpts {
            long: true,
            ..Default::default()
        };
        let out = render_listing(&entries, &opts, 0, 80);
        // Sizes right-align in a two-wide column: " 1" and "22".
        assert!(out.contains(" 1 "), "got {out:?}");
        assert!(out.contains("22 "), "got {out:?}");
    }

    #[test]
    fn missing_owner_shows_a_question_mark() {
        let mut e = entry("f", false, 1, 0o644);
        e.uid = None;
        e.gid = None;
        let opts = LsOpts {
            long: true,
            ..Default::default()
        };
        let out = render_listing(&[e], &opts, 0, 80);
        assert!(out.contains("? ?"), "got {out:?}");
    }

    #[test]
    fn an_empty_directory_renders_nothing() {
        assert_eq!(render_listing(&[], &LsOpts::default(), 0, 80), "");
        let opts = LsOpts {
            long: true,
            ..Default::default()
        };
        assert_eq!(render_listing(&[], &opts, 0, 80), "");
    }

    #[test]
    fn short_listings_fill_columns() {
        let entries: Vec<SftpEntry> = ('a'..='f')
            .map(|c| entry(&c.to_string(), false, 1, 0o644))
            .collect();
        let out = render_listing(&entries, &LsOpts::default(), 0, 20);
        // 20 columns / (1 + 2) = 6 per row, so one row holds them all.
        assert_eq!(out.lines().count(), 1);
        assert!(out.starts_with("a  b  c"));
    }

    #[test]
    fn a_narrow_window_still_produces_one_name_per_row() {
        let entries: Vec<SftpEntry> = (0..3)
            .map(|i| entry(&format!("a_very_long_name_{i}"), false, 1, 0o644))
            .collect();
        let out = render_listing(&entries, &LsOpts::default(), 0, 10);
        assert_eq!(out.lines().count(), 3);
    }

    /// Column widths are DISPLAY widths: a CJK name is twice as wide as
    /// it is long, and counting characters drifts the alignment.
    #[test]
    fn columns_measure_display_width_not_characters() {
        let entries = vec![entry("文档", false, 1, 0o644), entry("ab", false, 1, 0o644)];
        let out = render_listing(&entries, &LsOpts::default(), 0, 80);
        // Widest is 4 columns, so the first name is followed by 2 spaces
        // of gap and the shorter one by 4 to reach the same stop.
        assert!(
            out.starts_with("ab    文档") || out.starts_with("文档  ab"),
            "got {out:?}"
        );
    }

    #[test]
    fn every_listing_line_ends_with_crlf() {
        let entries = vec![entry("a", false, 1, 0o644)];
        for opts in [
            LsOpts::default(),
            LsOpts {
                long: true,
                ..Default::default()
            },
            LsOpts {
                one_per_line: true,
                ..Default::default()
            },
        ] {
            let out = render_listing(&entries, &opts, 0, 80);
            assert!(out.ends_with(CRLF), "{opts:?} produced {out:?}");
            assert!(!out.contains("\n\r"), "{opts:?} produced {out:?}");
        }
    }

    // --- progress ---------------------------------------------------

    #[test]
    fn progress_draws_in_place_and_clears_the_rest() {
        let line = progress_line("f.txt", 50, Some(100), 1024.0, 1.0, 80);
        assert!(line.starts_with('\r'), "got {line:?}");
        assert!(line.ends_with("\x1b[K"), "got {line:?}");
    }

    #[test]
    fn progress_shows_the_percentage_it_is_at() {
        let line = progress_line("f", 0, Some(100), 0.0, 0.0, 80);
        assert!(line.contains("0%"), "got {line:?}");
        let line = progress_line("f", 68, Some(100), 4096.0, 1.0, 80);
        assert!(line.contains("68%"), "got {line:?}");
        let line = progress_line("f", 100, Some(100), 4096.0, 1.0, 80);
        assert!(line.contains("100%"), "got {line:?}");
    }

    #[test]
    fn a_finished_transfer_has_a_full_bar_and_no_eta() {
        let line = progress_line("f", 100, Some(100), 4096.0, 3.0, 80);
        assert!(!line.contains("ETA"), "got {line:?}");
        assert!(line.contains("00:03"), "got {line:?}");
    }

    /// The drawn body, with the leading CR and the trailing erase-to-end
    /// stripped. Both contain characters (`[`) that the assertions below
    /// are looking for in the CONTENT, so comparing against the raw line
    /// would pass or fail for the wrong reason.
    fn body(line: &str) -> &str {
        line.trim_start_matches('\r').trim_end_matches("\x1b[K")
    }

    /// A total the server would not report means no bar. Drawing one
    /// anyway would be a progress indicator that indicates nothing.
    #[test]
    fn an_unknown_total_drops_the_bar_rather_than_faking_it() {
        let line = progress_line("f", 4096, None, 1024.0, 1.0, 80);
        assert!(!body(&line).contains('['), "got {line:?}");
        assert!(line.contains("4.0K"), "got {line:?}");
    }

    #[test]
    fn a_zero_total_is_treated_as_unknown_not_as_a_division() {
        // The assertion is that this does not divide by zero.
        let line = progress_line("f", 0, Some(0), 0.0, 0.0, 80);
        assert!(!body(&line).contains('['), "got {line:?}");
    }

    /// A meter that overflows the window wraps, and a wrapped meter
    /// redrawn with a bare CR paints its second row over the output
    /// above, so a long transfer slowly eats the scrollback. Every width
    /// down to absurd ones has to stay within budget.
    #[test]
    fn progress_never_exceeds_the_window() {
        for cols in [1u16, 5, 10, 20, 40, 80, 200] {
            for (done, total) in [
                (0u64, Some(100u64)),
                (50, Some(100)),
                (100, Some(100)),
                (4096, None),
            ] {
                let line =
                    progress_line("some_long_file_name.tar.gz", done, total, 1024.0, 1.0, cols);
                assert!(
                    body(&line).width() <= cols as usize,
                    "cols={cols} done={done} produced {} columns: {:?}",
                    body(&line).width(),
                    body(&line)
                );
            }
        }
    }

    /// The layout degrades in stages, and the percentage is what survives
    /// longest: it is the field the user is actually reading. At 20
    /// columns the rate and the ETA are gone but a bar still fits; at 12
    /// even the bar goes.
    #[test]
    fn a_tight_window_sheds_fields_in_order() {
        let narrow = progress_line("some_long_file_name.tar.gz", 50, Some(100), 1024.0, 1.0, 20);
        assert!(body(&narrow).contains("50%"), "got {narrow:?}");
        assert!(
            body(&narrow).contains('['),
            "a bar still fits at 20: {narrow:?}"
        );
        assert!(
            !body(&narrow).contains("ETA"),
            "eta should be gone: {narrow:?}"
        );

        let tiny = progress_line("some_long_file_name.tar.gz", 50, Some(100), 1024.0, 1.0, 12);
        assert!(body(&tiny).contains("50%"), "got {tiny:?}");
        assert!(
            !body(&tiny).contains('['),
            "no room for a bar at 12: {tiny:?}"
        );
    }

    /// A roomy window gets the whole layout, so the degradation above is
    /// not silently the only path anyone ever sees.
    #[test]
    fn a_roomy_window_gets_the_bar_and_every_field() {
        let line = progress_line("f.txt", 50, Some(100), 1024.0, 1.0, 80);
        let body = body(&line);
        assert!(body.contains('['), "no bar: {body:?}");
        assert!(body.contains('#'), "empty bar: {body:?}");
        assert!(body.contains("50%"), "no percentage: {body:?}");
        assert!(body.contains("1.0K/s"), "no rate: {body:?}");
        assert!(body.contains("ETA"), "no eta: {body:?}");
    }

    // --- OSC 133 ----------------------------------------------------

    /// The marks are the FinalTerm sequences verbatim, terminated with
    /// ST. A mark the emulator does not recognise is invisible: it
    /// simply never fires, and nothing anywhere says why.
    #[test]
    fn the_marks_are_well_formed_osc_133() {
        for (mark, kind) in [
            (marks::PROMPT_START, 'A'),
            (marks::PROMPT_END, 'B'),
            (marks::OUTPUT_START, 'C'),
        ] {
            assert_eq!(mark, format!("\x1b]133;{kind}\x1b\\"));
        }
        assert_eq!(marks::command_end(false), "\x1b]133;D;0\x1b\\");
        assert_eq!(marks::command_end(true), "\x1b]133;D;1\x1b\\");
    }

    /// Every mark is invisible: a sequence that printed would leave
    /// debris on the line the user is typing.
    #[test]
    fn the_marks_print_nothing() {
        for mark in [
            marks::PROMPT_START,
            marks::PROMPT_END,
            marks::OUTPUT_START,
            &marks::command_end(false),
        ] {
            assert!(mark.starts_with("\x1b]"), "not an OSC: {mark:?}");
            assert!(mark.ends_with("\x1b\\"), "unterminated: {mark:?}");
            // Nothing between the introducer and the terminator that a
            // terminal would draw.
            let body = &mark[2..mark.len() - 2];
            assert!(
                body.chars().all(|c| c.is_ascii_graphic() || c == ';'),
                "unexpected payload: {body:?}"
            );
        }
    }

    /// `E` (the parsed command line) is deliberately absent: it feeds
    /// the per-host command history, which exists to be re-inserted into
    /// a shell, where `get access.log` is not a command. The reading
    /// side declines console captures too; this is the other half.
    #[test]
    fn no_mark_carries_a_command_line() {
        for mark in [
            marks::PROMPT_START,
            marks::PROMPT_END,
            marks::OUTPUT_START,
            &marks::command_end(true),
        ] {
            assert!(!mark.contains("633"), "633 is the command-line family");
            assert!(!mark.contains(";E"), "E carries a command line");
        }
    }

    #[test]
    fn help_lists_every_command_and_ends_cleanly() {
        let help = help_text();
        for cmd in [
            "cd", "lcd", "get", "put", "mget", "rm", "chmod", "progress", "bye",
        ] {
            assert!(help.contains(cmd), "help is missing {cmd}");
        }
        assert!(help.ends_with(CRLF));
    }

    /// The `longname` limitation is documented where a user will hit it,
    /// which is the point of putting it in `help` rather than in a
    /// comment nobody reads.
    #[test]
    fn help_admits_the_numeric_owner_limitation() {
        assert!(help_text().contains("numerically"));
    }

    /// A file name is remote input on its way to a terminal that acts on
    /// escape sequences. Left raw it can clear the screen, forge the OSC
    /// 133 marks this console emits, or reach the clipboard through OSC
    /// 52, which is on by default.
    #[test]
    fn a_name_cannot_smuggle_an_escape_sequence_into_the_terminal() {
        assert_eq!(display_name("plain.txt"), "plain.txt");
        assert_eq!(display_name("a\u{1b}[2Jb"), "a?[2Jb");
        assert_eq!(display_name("t\tab"), "t?ab");
        assert_eq!(display_name("line\r\nbreak"), "line??break");
        assert_eq!(display_name("\u{7}bell"), "?bell");
        // Non-ASCII text is not a control sequence and must survive.
        assert_eq!(display_name("relatório-日本語.txt"), "relatório-日本語.txt");
    }

    /// The sanitizer runs before the widths are measured, so a name full
    /// of escapes cannot push the column arithmetic off what is drawn.
    #[test]
    fn a_listing_carries_no_control_bytes() {
        let entry = SftpEntry {
            name: "ev\u{1b}]0;pwned\u{7}il".to_string(),
            is_dir: false,
            is_symlink: false,
            size: 1,
            mtime: None,
            permissions: None,
            uid: None,
            gid: None,
        };
        for opts in [
            LsOpts { long: true, ..Default::default() },
            LsOpts { one_per_line: true, ..Default::default() },
            LsOpts::default(),
        ] {
            let out = render_listing(std::slice::from_ref(&entry), &opts, 0, 80);
            assert!(
                !out.chars().any(|c| c.is_control() && c != '\r' && c != '\n'),
                "control byte survived into the listing: {out:?}"
            );
        }
    }
}
