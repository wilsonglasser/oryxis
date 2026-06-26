//! Lightweight OSC sniffer for sequences `alacritty_terminal` does not surface
//! as events: OSC 7 (working directory), OSC 133 (shell-integration / semantic
//! prompt marks), and OSC 9 (notifications + progress).
//!
//! It scans the same byte stream fed to the emulator and extracts only those
//! sequences; everything else passes through untouched. alacritty still parses
//! the full stream and harmlessly ignores the OSC numbers it does not know, so
//! this only ever *reads* the bytes, it never strips or rewrites them. The
//! scanner is resumable: an OSC split across two `feed` calls is reassembled.

/// A shell-integration mark (OSC 133, the FinalTerm semantic-prompt protocol).
/// Captured as groundwork for a future command-history feature; nothing
/// consumes them yet beyond being stored per pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellMark {
    /// `OSC 133 ; A` prompt start.
    PromptStart,
    /// `OSC 133 ; B` prompt end (the command line begins).
    PromptEnd,
    /// `OSC 133 ; C` command output begins.
    OutputStart,
    /// `OSC 133 ; D` command finished, with the exit code when the shell
    /// reports one (`D;<code>`).
    CommandEnd(Option<i32>),
}

/// OSC 9;4 progress report (ConEmu / Windows Terminal). `state`: 0 = clear,
/// 1 = normal, 2 = error, 3 = indeterminate, 4 = warning. `value`: 0..=100.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Progress {
    pub state: u8,
    pub value: u8,
}

#[derive(Default)]
enum Scan {
    #[default]
    Normal,
    /// Saw `ESC`.
    Esc,
    /// Inside an OSC, accumulating the payload.
    Osc,
    /// Inside an OSC and saw `ESC` (a possible `ST` terminator `ESC \`).
    OscEsc,
}

/// Hard cap on a single OSC payload so malformed input can't grow the buffer
/// without bound. Real OSC 7/133/9 payloads are tiny.
const MAX_OSC: usize = 8192;

#[derive(Default)]
pub struct OscSniffer {
    scan: Scan,
    buf: Vec<u8>,
    cwd: Option<String>,
    notification: Option<String>,
    progress: Option<Progress>,
    marks: Vec<ShellMark>,
}

impl OscSniffer {
    /// Feed a chunk of PTY bytes. Extracts any complete OSC 7/133/9 sequences
    /// into the pending fields, drained by the `take_*` accessors.
    pub fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            match self.scan {
                Scan::Normal => {
                    if b == 0x1b {
                        self.scan = Scan::Esc;
                    }
                }
                Scan::Esc => match b {
                    b']' => {
                        self.scan = Scan::Osc;
                        self.buf.clear();
                    }
                    0x1b => {} // back-to-back ESC, stay armed
                    _ => self.scan = Scan::Normal,
                },
                Scan::Osc => match b {
                    0x07 => {
                        // BEL terminator
                        self.finish();
                        self.scan = Scan::Normal;
                    }
                    0x1b => self.scan = Scan::OscEsc,
                    _ => {
                        if self.buf.len() < MAX_OSC {
                            self.buf.push(b);
                        } else {
                            // Overflow: abandon this sequence.
                            self.buf.clear();
                            self.scan = Scan::Normal;
                        }
                    }
                },
                Scan::OscEsc => match b {
                    b'\\' => {
                        // ST terminator (ESC \)
                        self.finish();
                        self.scan = Scan::Normal;
                    }
                    0x1b => {} // another ESC, keep waiting for the backslash
                    _ => {
                        // ESC then non-backslash: not a terminator, the OSC is
                        // aborted by a new escape. Drop it.
                        self.buf.clear();
                        self.scan = Scan::Normal;
                    }
                },
            }
        }
    }

    /// Parse a completed OSC payload (`buf`) and route it.
    fn finish(&mut self) {
        let content = std::mem::take(&mut self.buf);
        let Ok(s) = std::str::from_utf8(&content) else {
            return;
        };
        let (num, rest) = s.split_once(';').unwrap_or((s, ""));
        match num {
            "7" => {
                if let Some(path) = parse_osc7(rest) {
                    self.cwd = Some(path);
                }
            }
            "133" => {
                if let Some(mark) = parse_osc133(rest) {
                    // Bound the buffer; a future history reader drains it.
                    if self.marks.len() >= 4096 {
                        self.marks.remove(0);
                    }
                    self.marks.push(mark);
                }
            }
            "9" => {
                if let Some(p) = rest.strip_prefix("4;") {
                    if let Some(progress) = parse_progress(p) {
                        self.progress = Some(progress);
                    }
                } else if !rest.is_empty() {
                    self.notification = Some(rest.to_string());
                }
            }
            _ => {}
        }
    }

    pub fn take_cwd(&mut self) -> Option<String> {
        self.cwd.take()
    }

    pub fn take_notification(&mut self) -> Option<String> {
        self.notification.take()
    }

    pub fn progress(&self) -> Option<Progress> {
        self.progress
    }

    pub fn take_marks(&mut self) -> Vec<ShellMark> {
        std::mem::take(&mut self.marks)
    }
}

/// Parse an OSC 7 payload (`file://host/path`, percent-encoded) into a local
/// filesystem path. A bare `/path` (some shells omit the `file://` host) is
/// accepted too. Returns `None` for anything that isn't an absolute path.
fn parse_osc7(rest: &str) -> Option<String> {
    let after_scheme = rest.strip_prefix("file://").unwrap_or(rest);
    // Drop the authority (host) component: the path starts at the first '/'.
    let path = match after_scheme.find('/') {
        Some(i) => &after_scheme[i..],
        None if after_scheme.starts_with('/') => after_scheme,
        None => return None,
    };
    if !path.starts_with('/') {
        return None;
    }
    Some(percent_decode(path))
}

/// Minimal percent-decoder for OSC 7 paths (spaces arrive as `%20`, etc.).
/// Invalid escapes are kept literally.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parse an OSC 133 payload (`A`, `B`, `C`, `D`, or `D;<code>`).
fn parse_osc133(rest: &str) -> Option<ShellMark> {
    let kind = rest.as_bytes().first()?;
    match kind {
        b'A' => Some(ShellMark::PromptStart),
        b'B' => Some(ShellMark::PromptEnd),
        b'C' => Some(ShellMark::OutputStart),
        b'D' => {
            // `D` or `D;<exit code>` (further `;k=v` params ignored).
            let code = rest
                .split_once(';')
                .and_then(|(_, tail)| tail.split(';').next())
                .and_then(|c| c.parse::<i32>().ok());
            Some(ShellMark::CommandEnd(code))
        }
        _ => None,
    }
}

/// Parse an OSC 9;4 progress body (`<state>;<value>`).
fn parse_progress(p: &str) -> Option<Progress> {
    let (st, val) = p.split_once(';').unwrap_or((p, "0"));
    let state: u8 = st.trim().parse().ok()?;
    let value: u8 = val.trim().parse().unwrap_or(0).min(100);
    Some(Progress { state, value })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sniff(input: &[u8]) -> OscSniffer {
        let mut s = OscSniffer::default();
        s.feed(input);
        s
    }

    #[test]
    fn osc7_cwd_bel_and_st_terminators() {
        // BEL-terminated, file:// with host.
        let mut s = sniff(b"\x1b]7;file://host/home/wilson\x07");
        assert_eq!(s.take_cwd().as_deref(), Some("/home/wilson"));
        // ST-terminated (ESC \), empty host, percent-encoded space.
        let mut s = sniff(b"\x1b]7;file:///home/my%20dir\x1b\\");
        assert_eq!(s.take_cwd().as_deref(), Some("/home/my dir"));
    }

    #[test]
    fn osc_split_across_feeds_is_reassembled() {
        let mut s = OscSniffer::default();
        s.feed(b"\x1b]7;file://h/ho");
        s.feed(b"me/w\x07");
        assert_eq!(s.take_cwd().as_deref(), Some("/home/w"));
    }

    #[test]
    fn osc133_marks() {
        let mut s = sniff(b"\x1b]133;A\x07\x1b]133;B\x07\x1b]133;C\x07\x1b]133;D;7\x07");
        assert_eq!(
            s.take_marks(),
            vec![
                ShellMark::PromptStart,
                ShellMark::PromptEnd,
                ShellMark::OutputStart,
                ShellMark::CommandEnd(Some(7)),
            ]
        );
    }

    #[test]
    fn osc9_notification_and_progress() {
        let mut s = sniff(b"\x1b]9;build done\x07");
        assert_eq!(s.take_notification().as_deref(), Some("build done"));
        let s = sniff(b"\x1b]9;4;1;42\x07");
        assert_eq!(s.progress(), Some(Progress { state: 1, value: 42 }));
        // Clamp out-of-range progress.
        let s = sniff(b"\x1b]9;4;1;250\x07");
        assert_eq!(s.progress(), Some(Progress { state: 1, value: 100 }));
    }

    #[test]
    fn unrelated_osc_and_text_ignored() {
        // OSC 0 (title, alacritty's job) and plain text leave no signals.
        let mut s = sniff(b"hello \x1b]0;a title\x07 world");
        assert!(s.take_cwd().is_none());
        assert!(s.take_notification().is_none());
        assert!(s.progress().is_none());
        assert!(s.take_marks().is_empty());
    }
}
