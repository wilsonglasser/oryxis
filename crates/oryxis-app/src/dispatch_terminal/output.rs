//! `PtyOutput` handling + session-log recording, split out of
//! `dispatch_terminal`: the per-batch PTY firehose (zmodem
//! interception, OSC title/cwd/notification sniffing, command
//! capture, smart-tab activity, bell), the batched vault flush of
//! recorded output with replay timing marks, and its segment /
//! UTF-8 alignment helpers. Called from `handle_terminal`.

#![allow(clippy::result_large_err)]

use iced::Task;

use crate::app::{TerminalMessage, Message, Oryxis};
use oryxis_core::models::TriggerAction;

/// Flush a pane's recorded-output buffer to the vault once it reaches
/// this size, so a burst (e.g. an `apt upgrade` dump) doesn't sit in
/// RAM unbounded between the periodic flush ticks.
const SESSION_LOG_FLUSH_BYTES: usize = 64 * 1024;

/// Free space below which session recording stops. The vault sits on
/// the user's home volume, so a recording that runs it dry takes every
/// other application (and the OS's own logging) down with it, and the
/// remote peer is the one deciding how many bytes arrive. Not a
/// setting: nobody switches on "do not fill my disk".
const SESSION_LOG_MIN_FREE_BYTES: u64 = 1024 * 1024 * 1024;

/// How often the capacity guard actually measures. The flush runs every
/// 2 s (and on a 64 KiB burst); a `statvfs` plus a `SUM(LENGTH(data))`
/// over every chunk at that cadence would cost more than the recording
/// does. At 30 s the worst case between checks is bounded by the
/// redaction pass's own throughput, well inside the 1 GiB floor.
const SESSION_LOG_CAPACITY_INTERVAL: std::time::Duration =
    std::time::Duration::from_secs(30);

/// Minimum gap between arrival marks for the flush to cut a separate
/// timed chunk (asciicast replay step). Bursty output (a compile, a
/// `find /`) coalesces into a few chunks per second instead of one
/// vault row per PTY read; interactive pauses stay visible.
const SESSION_LOG_SEGMENT_MS: i64 = 250;

/// Split a flushed buffer into timed replay segments: `(byte offset,
/// offset_ms)` per chunk, driven by the arrival marks. Cuts happen
/// ONLY at line boundaries, either the byte before the mark is `\n` /
/// `\r`, or the mark itself sits on a `\r` (a CR-prefixed progress-bar
/// redraw, wget style). Secret runs never contain `\n` or `\r`, so a
/// cut there can't split one across redaction chunks. And only marks
/// whose arrival gap is worth a replay step cut; bursty output
/// coalesces instead of producing one vault row per PTY read.
fn session_log_segments(head: &[u8], marks: &[(usize, i64)]) -> Vec<(usize, i64)> {
    let mut segs: Vec<(usize, i64)> = Vec::new();
    for &(pos, ms) in marks {
        match segs.last() {
            None => segs.push((0, ms)),
            Some(&(_, prev_ms)) => {
                let line_bounded = pos > 0
                    && pos < head.len()
                    && (matches!(head[pos - 1], b'\n' | b'\r') || head[pos] == b'\r');
                if line_bounded && ms - prev_ms >= SESSION_LOG_SEGMENT_MS {
                    segs.push((pos, ms));
                }
            }
        }
    }
    if segs.is_empty() {
        segs.push((0, 0));
    }
    segs
}

/// One vault row waiting in a flush batch, kept in stream order so the
/// table's insertion order (which replay reads back) matches the order
/// the live grid saw things happen.
enum PendingSessionRow {
    /// Output chunk (`kind='o'`): replay offset (`None` in simple
    /// mode) + bytes.
    Chunk(Option<i64>, Vec<u8>),
    /// Grid geometry change (`kind='r'`) at this point of the stream.
    Resize(i64, u16, u16),
}

/// A replay row produced by [`session_log_rows`], before the bytes are
/// materialized: chunk ranges index into the flushed head.
#[derive(Debug, PartialEq)]
enum SessionRow {
    Chunk(i64, std::ops::Range<usize>),
    Resize(i64, u16, u16),
}

/// Largest position `<= pos` that is a line boundary (start of buffer,
/// or right after a `\n`/`\r`). Chunk cuts must stay line-bounded so a
/// secret run (which contains neither byte) is never split across
/// redaction chunks; a resize cut snaps back here.
fn line_bounded_floor(head: &[u8], pos: usize) -> usize {
    let mut p = pos.min(head.len());
    while p > 0 && !matches!(head[p - 1], b'\n' | b'\r') {
        p -= 1;
    }
    p
}

/// Interleave a flushed head's replay rows: the timed output chunks
/// (cut at the arrival marks, see [`session_log_segments`]) plus the
/// recorded resize marks as `'r'` rows at the stream position where
/// the grid actually changed. Each resize forces an extra cut at its
/// line-bounded floor so the row can sit between the chunks; bytes on
/// the same partial line as the resize replay right after it, which is
/// what the live grid's reflow showed anyway. Without the interleave,
/// resizes landed at flush time after a whole window of chunks, so
/// replay processed up to 64 KiB of bytes on a stale grid; the connect
/// window (MOTD + prompt-setup echo formatted for the PTY's initial
/// size) rendered garbled and the OSC 7 setup's self-erasing echo
/// (DECSC/DECRC) misfired, leaving the setup block on screen.
fn session_log_rows(
    head: &[u8],
    marks: &[(usize, i64)],
    resizes: &[(usize, i64, u16, u16)],
) -> Vec<SessionRow> {
    let mut cuts = if head.is_empty() {
        Vec::new()
    } else {
        session_log_segments(head, marks)
    };
    for &(pos, _, _, _) in resizes {
        let p = line_bounded_floor(head, pos);
        if p < head.len() && !cuts.iter().any(|&(start, _)| start == p) {
            // Stamp the forced cut with the arrival time of the bytes
            // at it: the last mark at or before the position.
            let ms = marks
                .iter()
                .rev()
                .find(|&&(mp, _)| mp <= p)
                .map(|&(_, ms)| ms)
                .unwrap_or(0);
            cuts.push((p, ms));
        }
    }
    cuts.sort_unstable_by_key(|&(start, _)| start);
    let mut rows = Vec::new();
    let mut ri = 0;
    for (i, &(start, ms)) in cuts.iter().enumerate() {
        let end = cuts.get(i + 1).map(|c| c.0).unwrap_or(head.len());
        // Every resize whose floor is at or before this chunk's start
        // applies before its bytes (floors are monotonic, so the walk
        // stays in order).
        while ri < resizes.len()
            && line_bounded_floor(head, resizes[ri].0) <= start
        {
            let (_, ms, cols, rows_n) = resizes[ri];
            rows.push(SessionRow::Resize(ms, cols, rows_n));
            ri += 1;
        }
        if start < end {
            rows.push(SessionRow::Chunk(ms, start..end));
        }
    }
    // Resizes past the last chunk (or with no chunks at all) land at
    // the end of the batch: the flush-cadence fallback records exactly
    // this shape for a resize with no output after it.
    while ri < resizes.len() {
        let (_, ms, cols, rows_n) = resizes[ri];
        rows.push(SessionRow::Resize(ms, cols, rows_n));
        ri += 1;
    }
    rows
}

/// Largest cut `<= take` that doesn't split a trailing multi-byte
/// UTF-8 sequence (its continuation bytes may not have arrived yet).
/// Walks back over at most 3 continuation bytes; anything that doesn't
/// parse as UTF-8 (raw binary output) keeps the original cut, this is
/// a best-effort alignment, not a validator.
fn utf8_aligned(buf: &[u8], take: usize) -> usize {
    let mut cut = take;
    let floor = take.saturating_sub(3);
    while cut > floor && buf[cut - 1] & 0xC0 == 0x80 {
        cut -= 1;
    }
    if cut == 0 {
        return take;
    }
    let lead = buf[cut - 1];
    let need = match lead {
        b if b & 0x80 == 0x00 => 1,
        b if b & 0xE0 == 0xC0 => 2,
        b if b & 0xF0 == 0xE0 => 3,
        b if b & 0xF8 == 0xF0 => 4,
        // Stray continuation or invalid lead: not UTF-8, cut as-is.
        _ => return take,
    };
    if need > take - (cut - 1) {
        // The sequence is incomplete at the cut: flush up to its start.
        cut - 1
    } else {
        take
    }
}

impl Oryxis {
    /// Drain every pane's recorded-output buffer into the vault, one
    /// append per pane. Driven by the size threshold, the flush tick,
    /// disconnect, and window close, so the vault sees batched writes
    /// instead of one per SSH chunk (the old per-chunk path rewrote the
    /// whole growing blob and hammered the disk).
    ///
    /// Secrets/PII are scrubbed per flushed chunk (`session_redact`).
    /// Patterns can't match across chunk boundaries, so the periodic
    /// (non-final) flush holds back everything after the buffer's last
    /// line boundary (`\n` or `\r`; secret runs contain neither, so a
    /// cut there can't split one); the partial line rides along to the
    /// next flush unless the buffer is oversized anyway.
    pub(crate) fn flush_session_logs(&mut self) {
        self.flush_session_logs_inner(false);
    }

    /// Flush including trailing partial lines. Use when the pane, tab,
    /// session, or window is going away (or the log is about to be
    /// read), so the recorded tail isn't lost.
    pub(crate) fn flush_session_logs_final(&mut self) {
        self.flush_session_logs_inner(true);
    }

    /// Why recording stopped, so the toast can say which limit was hit.
    fn session_log_capacity_stop(&mut self) -> Option<&'static str> {
        if self.last_session_log_capacity_check.elapsed() < SESSION_LOG_CAPACITY_INTERVAL {
            return None;
        }
        self.last_session_log_capacity_check = std::time::Instant::now();
        let vault = self.vault.as_ref()?;

        // The size cap is the user's own quota, so reaching it drops the
        // oldest FINISHED recordings first (retention by size) and
        // recording continues. Only when there is nothing left to drop,
        // i.e. one live session is the whole total, does it stop.
        if let Some(cap) = self.prefs.session_log_max_bytes {
            match vault.prune_session_logs_to_fit(cap) {
                Ok(n) => {
                    if n > 0 {
                        tracing::info!("session log size cap pruned {n} recordings");
                    }
                }
                Err(e) => tracing::warn!("session log size-cap prune failed: {e}"),
            }
            if vault.session_logs_total_bytes().unwrap_or(0) > cap {
                return Some("session_log_stopped_cap");
            }
        }

        // The free-space floor is NOT a setting: nobody switches on "do
        // not fill my disk", and the vault lives on the user's home
        // volume, so running it dry takes every other application down
        // with the recording. Best-effort by construction, a platform
        // that will not answer leaves recording alone.
        let vault_dir = oryxis_core::paths::oryxis_dir()?;
        let free = oryxis_core::disk::available_space(&vault_dir)?;
        (free < SESSION_LOG_MIN_FREE_BYTES).then_some("session_log_stopped_disk")
    }

    /// Stop every live recording, flag each one as cut short, and say so
    /// once. Marking matters as much as stopping: an audit feature that
    /// hands back a partial stream presenting itself as the whole
    /// session fails worse than one that stops.
    fn stop_all_session_logs(&mut self, reason: &'static str) {
        let mut stopped: Vec<uuid::Uuid> = Vec::new();
        for tab in &mut self.tabs {
            for pane in tab.pane_grid.panes.values_mut() {
                if let Some(log_id) = pane.session_log_id.take() {
                    pane.session_log_buf.clear();
                    pane.session_log_marks.clear();
                    pane.session_log_resizes.clear();
                    stopped.push(log_id);
                }
            }
        }
        if stopped.is_empty() {
            return;
        }
        if let Some(vault) = &self.vault {
            for log_id in &stopped {
                if let Err(e) = vault.mark_session_log_truncated(log_id) {
                    tracing::warn!("marking session log {log_id} truncated failed: {e}");
                }
            }
        }
        tracing::warn!(
            "session recording stopped for {} pane(s): {reason}",
            stopped.len()
        );
        // A sentence to read, not a one-word confirmation.
        self.set_toast_secs(crate::i18n::t(reason).to_string(), 8);
    }

    fn flush_session_logs_inner(&mut self, final_flush: bool) {
        // Full detail = timed segments + resize events (.cast export);
        // simple = one untimed chunk per flush, the plain log of old.
        let full = self.prefs.session_log_full;
        let compress = self.prefs.session_log_compress;
        // Replay rows per pane, in stream order (chunks interleaved
        // with resizes; see `session_log_rows`).
        let mut pending: Vec<(uuid::Uuid, PendingSessionRow)> = Vec::new();
        for tab in &mut self.tabs {
            for pane in tab.pane_grid.panes.values_mut() {
                let Some(log_id) = pane.session_log_id else {
                    continue;
                };
                // Geometry fallback on the flush cadence: catches a
                // resize with NO output after it (the primary capture
                // point is the output batch itself, which stamps the
                // resize at its exact stream position). Replay
                // metadata, so full-detail mode only.
                if full {
                    let size = pane
                        .terminal
                        .lock()
                        .ok()
                        .map(|s| (s.cols(), s.rows()))
                        .filter(|&(c, r)| c > 0 && r > 0);
                    if let Some((cols, rows)) = size
                        && pane.session_log_last_size != Some((cols, rows))
                    {
                        pane.session_log_last_size = Some((cols, rows));
                        let now_ms = pane
                            .session_log_t0
                            .map(|t| t.elapsed().as_millis() as i64)
                            .unwrap_or(0);
                        pane.session_log_resizes.push((
                            pane.session_log_buf.len(),
                            now_ms,
                            cols,
                            rows,
                        ));
                    }
                }
                if pane.session_log_buf.is_empty()
                    && pane.session_log_resizes.is_empty()
                {
                    continue;
                }
                let buf = &mut pane.session_log_buf;
                let take = if final_flush {
                    buf.len()
                } else if buf.len() >= SESSION_LOG_FLUSH_BYTES {
                    // Oversized burst: flush everything, but never
                    // split a multi-byte UTF-8 sequence across chunks
                    // (the .cast export decodes per chunk, so both
                    // halves of a split char would render as U+FFFD).
                    utf8_aligned(buf, buf.len())
                } else {
                    // Hold back the partial trailing line so a secret
                    // mid-echo isn't split across redaction chunks.
                    // `\r` ends a line here too: progress-bar redraws
                    // are CR-delimited and would otherwise sit in RAM
                    // until a newline or the size threshold.
                    match buf.iter().rposition(|&b| b == b'\n' || b == b'\r') {
                        Some(pos) => pos + 1,
                        None => 0,
                    }
                };
                // Nothing flushable yet; pending resize marks ride
                // along with the bytes they precede on a later flush.
                // A final flush drains even an empty head so a trailing
                // resize isn't lost with the session.
                if take == 0 && !final_flush {
                    continue;
                }
                let tail = buf.split_off(take);
                let head = std::mem::replace(buf, tail);
                // Partition the arrival marks: the head's marks
                // drive the timed segments below; the tail's are
                // rebased, keeping a mark at 0 so carried-over
                // bytes don't lose their arrival time.
                let mut head_marks: Vec<(usize, i64)> = Vec::new();
                let mut tail_marks: Vec<(usize, i64)> = Vec::new();
                for (pos, ms) in pane.session_log_marks.drain(..) {
                    if pos < take {
                        head_marks.push((pos, ms));
                    } else {
                        tail_marks.push((pos - take, ms));
                    }
                }
                if !pane.session_log_buf.is_empty()
                    && tail_marks.first().is_none_or(|m| m.0 > 0)
                {
                    let carry_ms = head_marks.last().map(|m| m.1).unwrap_or(0);
                    tail_marks.insert(0, (0, carry_ms));
                }
                pane.session_log_marks = tail_marks;
                // Same partition for the resize marks. A mark exactly
                // at the cut belongs to the held-back tail (it applies
                // before bytes that flush later), except on a final
                // flush, where nothing follows.
                let mut head_resizes: Vec<(usize, i64, u16, u16)> = Vec::new();
                let mut tail_resizes: Vec<(usize, i64, u16, u16)> = Vec::new();
                for (pos, ms, cols, rows) in pane.session_log_resizes.drain(..) {
                    if pos < take || final_flush {
                        head_resizes.push((pos, ms, cols, rows));
                    } else {
                        tail_resizes.push((pos - take, ms, cols, rows));
                    }
                }
                pane.session_log_resizes = tail_resizes;
                if full {
                    for row in session_log_rows(&head, &head_marks, &head_resizes) {
                        pending.push((
                            log_id,
                            match row {
                                SessionRow::Chunk(ms, range) => PendingSessionRow::Chunk(
                                    Some(ms),
                                    head[range].to_vec(),
                                ),
                                SessionRow::Resize(ms, cols, rows) => {
                                    PendingSessionRow::Resize(ms, cols, rows)
                                }
                            },
                        ));
                    }
                } else if !head.is_empty() {
                    // Simple mode: one untimed chunk, no replay
                    // metadata (NULL offset = the legacy shape).
                    pending.push((log_id, PendingSessionRow::Chunk(None, head)));
                }
            }
        }
        if pending.is_empty() {
            return;
        }
        // Capacity is checked at the WRITE, not at the capture: the
        // buffers above are already drained, so the bytes this flush
        // holds are written either way and the stop takes effect from
        // the next one. Recording is what stops; the session keeps
        // running, untouched.
        if let Some(reason) = self.session_log_capacity_stop() {
            if let Some(vault) = &self.vault {
                for (log_id, row) in pending {
                    if let PendingSessionRow::Chunk(offset_ms, bytes) = row {
                        let scrubbed = crate::session_redact::redact_secrets(&bytes);
                        let _ = vault.append_session_data(
                            &log_id, &scrubbed, offset_ms, compress,
                        );
                    }
                }
            }
            self.stop_all_session_logs(reason);
            return;
        }
        // A failing append is almost always the disk filling underneath
        // us (the guard above measures on an interval, so a fast enough
        // peer can cross the line between two checks). It used to end
        // in a `tracing::warn!` and nothing else: recording kept being
        // attempted, every later byte was dropped on the floor, and the
        // user was never told their recording had stopped working.
        let mut append_failed = false;
        if let Some(vault) = &self.vault {
            for (log_id, row) in pending {
                match row {
                    PendingSessionRow::Chunk(offset_ms, bytes) => {
                        let scrubbed = crate::session_redact::redact_secrets(&bytes);
                        if let Err(e) = vault.append_session_data(
                            &log_id, &scrubbed, offset_ms, compress,
                        ) {
                            tracing::warn!("session log append failed for {log_id}: {e}");
                            append_failed = true;
                        }
                    }
                    PendingSessionRow::Resize(offset_ms, cols, rows) => {
                        if let Err(e) =
                            vault.append_session_resize(&log_id, offset_ms, cols, rows)
                        {
                            tracing::warn!("session resize append failed for {log_id}: {e}");
                            append_failed = true;
                        }
                    }
                }
            }
        }
        if append_failed {
            self.stop_all_session_logs("session_log_stopped_disk");
        }
    }

    /// Handle the `PtyOutput` firehose. Returns `Err(message)` for
    /// every other variant so `handle_terminal`'s chain falls through.
    pub(super) fn handle_terminal_output(
        &mut self,
        message: TerminalMessage,
    ) -> Result<Task<Message>, TerminalMessage> {
        match message {
            // -- Terminal I/O --
            TerminalMessage::PtyOutput(pane_id, mut bytes) => {
                // A local host's startup command waits for the shell to
                // speak and then go quiet (there is no session-ready
                // event to hang it on). Each batch re-arms the timer;
                // for every other pane this is one hash lookup on an
                // empty map.
                let startup_task = match self.pending_local_startup.is_empty() {
                    true => Task::none(),
                    false => self.note_local_output(pane_id),
                };
                // ── ZMODEM interception (before any emulator processing) ──
                // While a transfer owns the pane, output is protocol wire:
                // hand it to the driver and stop. Otherwise the initiation
                // detector runs; a detected `sz`/`rz` starts a transfer and
                // the clean prefix still flows to the emulator below.
                let mut zmodem_start: Option<(oryxis_zmodem::Direction, Vec<u8>)> = None;
                if let Some(pane) = self.pane_by_id_mut(pane_id) {
                    if let Some(zm) = pane.zmodem.as_mut() {
                        if let Err(unsent) = zm.wire_tx.send(std::mem::take(&mut bytes)) {
                            // The driver already ended; its terminal
                            // Progress is still in flight. Hold the
                            // bytes for the teardown to replay in order
                            // instead of dropping a fast prompt.
                            zm.late.extend_from_slice(&unsent.0);
                        }
                        // Return the startup timer even here: a transfer
                        // owns the BYTES, not the pane's pending
                        // command, and dropping it on the batch that
                        // happens to be the last one before silence
                        // would lose the command outright.
                        return Ok(startup_task);
                    }
                    let scan = pane.zmodem_detector.feed(&bytes);
                    // Only divert when the pane has a transport to run the
                    // protocol on; a local shell keeps the bytes on screen.
                    if let Some(direction) = scan.detection
                        && pane.session.is_some()
                    {
                        zmodem_start = Some((direction, scan.wire));
                        bytes = scan.clean;
                    } else if scan.detection.is_some() {
                        bytes = scan.clean;
                        bytes.extend(scan.wire);
                    } else {
                        bytes = scan.clean;
                    }
                }
                // Login automation (issue #122) reads the same cleaned
                // bytes the emulator is about to get, BEFORE the pane
                // borrow below, because a fired step writes back through
                // `self`. It is a no-op for every pane without an armed
                // script, which is nearly all of them.
                //
                // Whether a script was armed is captured BEFORE the feed:
                // the batch that finishes a run clears `login_script`
                // inside it, and the autofill gate below must still count
                // that batch as script-owned. The grid at its end shows
                // the bastion's password prompt in front of the cursor
                // (the runner's answer is not echoed), so reading it
                // would raise a popup for the prompt the script just
                // answered.
                let had_login_script = self
                    .pane_by_id(pane_id)
                    .is_some_and(|p| p.login_script.is_some());
                self.feed_login_script(pane_id, &bytes);
                // Route to the specific pane (a tab may have several, each
                // with its own PTY). Scan is trivial at these counts.
                let mut over_threshold = false;
                let mut schedule_flush: Option<std::time::Duration> = None;
                // Snapshot the (Copy) bell mode before borrowing self.tabs; the
                // bell action runs while the pane is borrowed.
                let bell_mode = self.prefs.bell_mode;
                // The compiled highlight rules, installed on the pane's
                // backend just before it processes the batch. Handing
                // them over HERE rather than at pane creation is what
                // makes them universal: every pane, whatever created it,
                // passes through this funnel, and the setter is a
                // pointer comparison when the set has not changed.
                // Resolved for the PANE'S HOST: the global list plus (or
                // replaced by) that host's own rules. Cached and keyed by
                // a signature of its inputs, so this is a lookup rather
                // than a recompile.
                let rules_conn_id = self.pane_by_id(pane_id).and_then(|p| p.saved_conn_id());
                let highlight_rules = self.highlight_rules_for(rules_conn_id);
                // How wide this host measures Unicode "Ambiguous" width
                // characters (J4), installed here for the same reason the
                // rules are: it is the one place every pane passes
                // through, whichever of the creation paths made it. The
                // setter is a bool comparison when the answer has not
                // changed, so the first batch pays for it and no other.
                // A mosh pane answers from the value PINNED at handover
                // instead: the screen inside the protocol was built with
                // that one and cannot be reconfigured, and a pane that
                // re-read the setting would end up disagreeing with the
                // model whose diff it is drawing.
                let ambiguous_width_wide = self
                    .pane_by_id(pane_id)
                    .and_then(|p| p.mosh_ambiguous_width)
                    .unwrap_or_else(|| {
                        rules_conn_id
                            .and_then(|id| self.connections.iter().find(|c| c.id == id))
                            .is_some_and(|c| c.ambiguous_width_effective())
                    });
                // What each action-bearing rule should DO, resolved
                // before the borrow because the actions themselves need
                // the whole app (a toast, a snippet, a confirmation).
                // Empty unless some rule carries an action, which is the
                // normal case and keeps this a no-op allocation.
                let trigger_actions: std::collections::HashMap<String, TriggerAction> =
                    if highlight_rules.any_triggers() {
                        self.prefs
                            .highlight_rules
                            .iter()
                            .filter(|r| r.enabled && r.action.is_trigger())
                            .map(|r| (r.id.clone(), r.action.clone()))
                            .collect()
                    } else {
                        std::collections::HashMap::new()
                    };
                // Rules that fired on this batch and cleared their
                // cooldown: (rule id, rule name, the matching line).
                let mut fired_triggers: Vec<(String, String, String)> = Vec::new();
                // Notification policy + focus snapshot before the tabs borrow.
                let notif_mode = self.prefs.notification_mode;
                let win_focused = self.window_focused;
                let mut flash_pane: Option<uuid::Uuid> = None;
                // (pane label, OSC 9 body). The label rides along so the
                // body can be redacted under Privacy Mode at delivery time
                // (resolved after the tabs borrow ends, like smart tabs).
                let mut pending_notification: Option<(String, String)> = None;
                let capture_enabled = self.prefs.command_history;
                let log_full = self.prefs.session_log_full;
                // Smart tabs: policy snapshots taken before the tabs borrow.
                let smart_enabled = self.prefs.smart_tabs;
                let smart_long = self.prefs.smart_long_secs;
                let active_tab = self.active_tab;
                // "Watched" needs the terminal on screen: an active tab is
                // invisible while the user sits in the Dashboard /
                // Settings, so it must still collect attention there.
                // Asked of the helper `view_content` renders by, NOT of
                // `active_view`: opening a host from the Dashboard pushes
                // a tab without assigning the view, so a view check reads
                // the tab the user is watching as unwatched and notifies
                // about it.
                let in_terminal_view = self.terminal_surface_visible();
                // (pane label, full body, redacted body) triples raised by
                // smart tabs this batch, delivered after the borrow ends
                // (Privacy Mode is resolved per pane at delivery).
                // (tab index, pane label, body, redacted body). The tab
                // rides along so DELIVERY can drop in-app toasts about the
                // tab already on screen (see below).
                let mut smart_notifications: Vec<(usize, String, String, String)> = Vec::new();
                let mut captured_cmds: Vec<(uuid::Uuid, String)> = Vec::new();
                // (log id, offset_ms, command) rows for the session
                // recording's 'c' chunks, written after the borrow ends.
                let mut session_cmds: Vec<(uuid::Uuid, Option<i64>, String)> = Vec::new();
                // Set when this batch carried an OSC 7 cwd; feeds the
                // sidebar Files follow after the pane borrow ends.
                let mut cwd_changed = false;
                // Alternate-screen edge this batch produced, if any:
                // `Some(entered)`. Attaching a tmux session draws the
                // alternate screen and detaching restores the primary,
                // so this is the tmux tab's auto-refresh signal (#158)
                // and the detach retire for the "attached here" hint
                // (#159). Handled after the pane borrow ends.
                let mut alt_edge: Option<bool> = None;
                // Password autofill (issue #117). The pane a popup may
                // open on is resolved BEFORE the borrow (it needs the
                // whole app: the visible view, the active tab, the
                // overlay slot), and the popup itself is raised AFTER
                // it, since showing it mutates `self.overlay`.
                let autofill_pane = self.password_suggest_target();
                // The open popup's pane, if any. The read must KEEP
                // running there even though the overlay slot is taken:
                // "the prompt is gone" is the popup's own dismissal
                // signal, and without it a popup outlived its prompt.
                // Field case: Ctrl+C cancels the prompt, the shell
                // prompt returns, and a pick on the leftover popup would
                // type the password into an ECHOING shell.
                let suggest_open_on = self.password_suggest_pane();
                let mut raise_password_suggest = false;
                let mut dismiss_stale_suggest = false;
                if let Some((tab_idx, pane)) = self
                    .tabs
                    .iter_mut()
                    .enumerate()
                    .flat_map(|(ti, t)| {
                        t.pane_grid.panes.values_mut().map(move |p| (ti, p))
                    })
                    .find(|(_, p)| p.id == pane_id)
                {
                    let mut sync_deadline = None;
                    let mut new_title = None;
                    let mut bell_rang = false;
                    let mut new_cwd = None;
                    let mut new_notification = None;
                    let mut new_progress = None;
                    let mut size_now = None;
                    // Read inside the lock, compared against the pane's
                    // remembered signature just after it.
                    let mut prompt_now: Option<oryxis_terminal::PasswordPrompt> = None;
                    // Never while a login script owns the prompts: the
                    // runner exists to answer exactly these (issue #122),
                    // and two answers on one PTY is one too many. Both
                    // halves matter: `had_login_script` covers the batch
                    // whose feed FINISHED the run (the grid still shows
                    // the prompt the script just answered), the live
                    // check covers every batch in between.
                    let autofill_read = (autofill_pane == Some(pane_id)
                        || suggest_open_on == Some(pane_id))
                        && !had_login_script
                        && pane.login_script.is_none();
                    if let Ok(mut state) = pane.terminal.lock() {
                        state.set_highlight_rules(highlight_rules);
                        state.set_ambiguous_width_wide(ambiguous_width_wide);
                        state.process(&bytes);
                        // Highlight rules that fired on this batch (C6).
                        // Drained inside the lock (the scanner lives in
                        // the backend), filtered by the per-pane cooldown
                        // here, and ACTED ON after the borrow: an action
                        // is a notification, a beep or a snippet, and all
                        // three need the whole app.
                        if !trigger_actions.is_empty() {
                            let now = std::time::Instant::now();
                            for hit in state.take_trigger_hits() {
                                if !trigger_actions.contains_key(&hit.rule_id) {
                                    continue;
                                }
                                let runtime = pane.triggers.entry(hit.rule_id.clone()).or_default();
                                if !runtime.take_turn(now) {
                                    continue;
                                }
                                fired_triggers.push((hit.rule_id, hit.rule_name, hit.line));
                            }
                        }
                        // Grid size this batch was processed at, for the
                        // recording's resize marks below.
                        size_now = Some((state.cols(), state.rows()));
                        // A buffering DEC ?2026 update reports its abort
                        // deadline here; read it while still locked.
                        sync_deadline = state.sync_timeout();
                        // OSC 0/2 title set by the shell this batch (or an
                        // empty string for ResetTitle). Captured unconditionally;
                        // the auto-title setting only gates display.
                        new_title = state.take_title();
                        bell_rang = state.take_bell();
                        // OSC 7 working directory.
                        new_cwd = state.take_cwd();
                        // OSC 133 shell-integration marks drive the pane's
                        // prompt state (the command-history capture gate) and
                        // resolve captures deferred until the echo arrived.
                        // Applied inside the lock: the marks' grid positions
                        // refer to rows this very batch drew.
                        let new_marks = state.take_shell_marks();
                        if !new_marks.is_empty() {
                            // Drained in the same breath as the marks: a
                            // `CommandLine` mark resolves its text by id
                            // against this very batch.
                            let new_texts = state.take_shell_command_lines();
                            let cmds = crate::command_capture::observe_output_marks(
                                &mut pane.prompt,
                                &mut pane.pending_capture,
                                &mut pane.inband,
                                &state,
                                &new_marks,
                                &new_texts,
                            );
                            // A capture resolved at this batch's OutputStart
                            // (paste with trailing newline) is the command
                            // that just started: label its run with it.
                            if smart_enabled && let Some(cmd) = cmds.last() {
                                pane.last_submitted = Some(cmd.clone());
                            }
                            // A recording session stores the resolved
                            // captures as 'c' chunks too (input-only
                            // export), regardless of the history setting.
                            if let Some(log_id) = pane.session_log_id {
                                let off = pane
                                    .session_log_t0
                                    .map(|t| t.elapsed().as_millis() as i64);
                                session_cmds.extend(
                                    cmds.iter().map(|c| (log_id, off, c.clone())),
                                );
                            }
                            // The per-host command HISTORY takes shell
                            // commands only. An SFTP console emits the same
                            // OSC 133 marks a shell with integration does,
                            // deliberately, because the marks are what tell
                            // the tab a command is running and give a
                            // recording its per-command boundaries. But its
                            // vocabulary is `sftp(1)`'s, and the history
                            // exists to be re-inserted into a shell, where
                            // `get access.log` is not a command. So the two
                            // other consumers above keep the captures and
                            // this one declines them.
                            if capture_enabled
                                && pane.purpose != crate::state::PanePurpose::SftpConsole
                                && let crate::state::PaneOrigin::Host(hid) = &pane.origin
                            {
                                captured_cmds.extend(cmds.into_iter().map(|c| (*hid, c)));
                            }
                            // Smart tabs: the same marks drive command
                            // start/end timing. A command that ran past the
                            // threshold and finished on a tab the user was
                            // not watching earns an attention dot + a
                            // notification.
                            if smart_enabled {
                                let now = std::time::Instant::now();
                                let watched = win_focused
                                    && in_terminal_view
                                    && active_tab == Some(tab_idx);
                                for f in crate::smart_tabs::observe_marks(
                                    &mut pane.running_cmd,
                                    &mut pane.last_submitted,
                                    &new_marks,
                                    now,
                                ) {
                                    if smart_long > 0
                                        && f.elapsed.as_secs() >= u64::from(smart_long)
                                        && !watched
                                    {
                                        crate::smart_tabs::raise_attention(
                                            &mut pane.attention,
                                            if f.failed() {
                                                crate::smart_tabs::TabAttention::FinishedFail
                                            } else {
                                                crate::smart_tabs::TabAttention::FinishedOk
                                            },
                                        );
                                        smart_notifications.push((
                                            tab_idx,
                                            pane.label.clone(),
                                            crate::smart_tabs::finished_body(&f, true),
                                            crate::smart_tabs::finished_body(&f, false),
                                        ));
                                    }
                                }
                            }
                        }
                        // OSC 9 notification text + OSC 9;4 progress.
                        new_notification = state.take_notification();
                        new_progress = state.progress();
                        // Password autofill (issue #117): read the grid
                        // AFTER `process`, so what is in front of the
                        // cursor is what this batch just painted.
                        if autofill_read {
                            prompt_now = state.password_prompt_at_cursor();
                        }
                        // Alternate-screen edge detection, against the
                        // pane's remembered side (updated below, the
                        // pane borrow outlives this lock).
                        let alt_now = state.is_alt_screen();
                        if alt_now != pane.alt_screen {
                            alt_edge = Some(alt_now);
                        }
                    }
                    if let Some(entered) = alt_edge {
                        pane.alt_screen = entered;
                    }
                    // Edge-trigger, and only when the read actually
                    // ran: a gated batch knows nothing about the screen,
                    // and letting it clear the signature would resurrect
                    // a dismissed popup the moment its own gate (an open
                    // overlay) went away.
                    if autofill_read {
                        let prompt_present = prompt_now.is_some();
                        raise_password_suggest =
                            crate::dispatch_password_suggest::observe_password_prompt(
                                &mut pane.password_prompt_sig,
                                prompt_now,
                            );
                        // The popup follows its prompt: when the thing it
                        // was raised for is no longer waiting (answered,
                        // cancelled with Ctrl+C, redrawn, alt screen),
                        // the suggestion is stale and a pick would type a
                        // password into whatever replaced it.
                        dismiss_stale_suggest =
                            !prompt_present && suggest_open_on == Some(pane_id);
                    }
                    // OSC 9;4 progress (state 0 = clear) drives the tab border.
                    pane.progress = new_progress.filter(|p| p.state != 0 && p.value > 0);
                    // Smart tabs, quiet-period half: runs on EVERY batch
                    // (marks or not) so the silence clock stays honest, and
                    // covers hosts without shell integration. Output after
                    // [`QUIET_PERIOD`] on an unwatched pane is "activity";
                    // the notification fires only on the dot's rising edge
                    // so a chatty background pane can't spam.
                    if smart_enabled {
                        let now = std::time::Instant::now();
                        let watched = win_focused
                            && in_terminal_view
                            && active_tab == Some(tab_idx);
                        let was_quiet =
                            crate::smart_tabs::quiet_activity(&mut pane.last_output, now);
                        if watched {
                            // Viewing the tab consumes its attention; this
                            // lazy clear backs up the explicit ones on
                            // SelectTab / window refocus, catching every
                            // other activation path as soon as bytes flow.
                            pane.attention = None;
                        } else if was_quiet
                            && crate::smart_tabs::raise_attention(
                                &mut pane.attention,
                                crate::smart_tabs::TabAttention::Activity,
                            )
                        {
                            // Activity carries no command text; only the
                            // pane label differs under Privacy Mode.
                            let body = crate::i18n::t("smart_activity").to_string();
                            smart_notifications.push((
                                tab_idx,
                                pane.label.clone(),
                                body.clone(),
                                body,
                            ));
                        }
                    }
                    pending_notification =
                        new_notification.map(|body| (pane.label.clone(), body));
                    if let Some(cwd) = new_cwd {
                        cwd_changed = pane.cwd.as_deref() != Some(cwd.as_str());
                        pane.cwd = Some(cwd);
                        pane.cwd_from_osc7 = true;
                    }
                    // C5: a host with `disable_title_change` ignores remote
                    // OSC 0/2 title updates entirely (the tab keeps its
                    // connection label / manual rename), and the OSC-title cwd
                    // fallback is suppressed with it.
                    if let Some(title) = new_title.filter(|_| !pane.quirks.disable_title_change) {
                        // Stored raw: when auto-title is on it's opt-in emulator
                        // behavior, so the tab shows exactly what the shell set
                        // (`user@host: ~`, `vim file`, …), like gnome-terminal /
                        // iTerm / Windows Terminal do.
                        let trimmed = title.trim();
                        // Cwd fallback for shells WITHOUT OSC 7 integration:
                        // the stock Debian/Ubuntu/Fedora PS1 titles the
                        // window `\u@\h: \w`, so the title carries the cwd
                        // (possibly `~`-relative; the sidebar Files browser
                        // expands it against the session home). For remote
                        // panes and local shells alike (the local browser
                        // follows it too, issue #145), and only until a
                        // real OSC 7 shows up, which is exact and takes
                        // over for good.
                        if !pane.cwd_from_osc7
                            && (pane.session.as_ref().and_then(|s| s.ssh()).is_some()
                                || matches!(
                                    pane.origin,
                                    crate::state::PaneOrigin::Local(_)
                                ))
                            && let Some(dir) = crate::dispatch_sidebar_files::title_cwd(trimmed)
                            && pane.cwd.as_deref() != Some(dir)
                        {
                            pane.cwd = Some(dir.to_string());
                            cwd_changed = true;
                        }
                        pane.osc_title = (!trimmed.is_empty()).then(|| trimmed.to_string());
                    }
                    if bell_rang {
                        match bell_mode {
                            crate::util::BellMode::Off => {}
                            crate::util::BellMode::Beep => crate::util::play_system_beep(),
                            crate::util::BellMode::Flash => {
                                pane.bell_flash = true;
                                flash_pane = Some(pane_id);
                            }
                        }
                    }
                    // Rising edge only: arm one flush timer per update, not
                    // one per coalesced output batch. The flag clears when the
                    // update closes normally (deadline gone) or when the
                    // `TerminalSyncFlush` handler fires.
                    match sync_deadline {
                        Some(deadline) if !pane.sync_flush_scheduled => {
                            pane.sync_flush_scheduled = true;
                            schedule_flush = Some(deadline.saturating_duration_since(
                                std::time::Instant::now(),
                            ));
                        }
                        None => pane.sync_flush_scheduled = false,
                        _ => {}
                    }
                    // Buffer the bytes; the vault write is batched (see
                    // `flush_session_logs`). Flush early once the buffer
                    // grows large so a burst doesn't balloon in RAM.
                    // Each batch leaves an arrival mark so the flush can
                    // stamp real replay timing onto the stored chunks.
                    if pane.session_log_id.is_some() {
                        // Arrival marks are replay metadata: full-detail
                        // recording only. Simple mode just buffers bytes
                        // (the flush stores one untimed chunk).
                        if log_full {
                            let t0 = *pane
                                .session_log_t0
                                .get_or_insert_with(std::time::Instant::now);
                            let now_ms = t0.elapsed().as_millis() as i64;
                            // A grid size change since the last recorded
                            // geometry lands as a resize mark at the
                            // current stream position: this batch's bytes
                            // were processed at the new size, so replay
                            // must resize before feeding them. The first
                            // batch records the initial geometry through
                            // the same path (last size starts `None`).
                            // Stamping resizes only at flush time built
                            // the replay grid at the first flush's size,
                            // so the connect window's bytes (MOTD +
                            // prompt-setup echo formatted for the PTY's
                            // initial 120x40) rendered garbled and the
                            // OSC 7 setup's self-erasing echo survived on
                            // screen in the player / .cast / GIF.
                            if let Some((cols, rows)) =
                                size_now.filter(|&(c, r)| c > 0 && r > 0)
                                && pane.session_log_last_size != Some((cols, rows))
                            {
                                pane.session_log_last_size = Some((cols, rows));
                                pane.session_log_resizes.push((
                                    pane.session_log_buf.len(),
                                    now_ms,
                                    cols,
                                    rows,
                                ));
                            }
                            pane.session_log_marks
                                .push((pane.session_log_buf.len(), now_ms));
                        }
                        pane.session_log_buf.extend_from_slice(&bytes);
                        over_threshold =
                            pane.session_log_buf.len() >= SESSION_LOG_FLUSH_BYTES;
                    }
                }
                for (host, cmd) in captured_cmds {
                    self.record_command_history(host, cmd);
                }
                for (log_id, off, cmd) in session_cmds {
                    self.record_session_command(&log_id, off, &cmd);
                }
                if over_threshold {
                    self.flush_session_logs();
                }
                // OSC 9 notification. The in-app toast is a gentle cue and
                // fires regardless of focus (also useful for a background tab);
                // the OS notification only fires while the window is unfocused
                // (a native popup for the thing you're already watching is just
                // noise) and falls back to a toast if the native call fails (no
                // daemon / no AppUserModelID on a non-installed Windows build).
                // Highlight-rule actions (C6). After the pane borrow,
                // because every one of them needs the whole app; the
                // cooldown has already been paid inside it.
                for (rule_id, rule_name, line) in fired_triggers {
                    if let Some(action) = trigger_actions.get(&rule_id) {
                        self.run_trigger_action(pane_id, &rule_id, &rule_name, &line, action);
                    }
                }
                let mut toast_shown = false;
                if let Some((label, text)) = pending_notification {
                    let trimmed = text.trim();
                    // The OSC 9 body is server-supplied. Under Privacy Mode
                    // the OS notification center keeps plaintext around and
                    // the terminal's masking is render-only, so redact it
                    // here before it leaves, exactly like the smart-tab
                    // bodies below. The title stays the generic "Oryxis"
                    // (no host identity), so only the body needs it.
                    let body_owned = if self.privacy_active_for_label(&label) {
                        crate::widgets::redact_for_display(
                            trimmed,
                            &self.privacy_terms(),
                            self.privacy_classes(),
                        )
                    } else {
                        trimmed.to_string()
                    };
                    let body = body_owned.as_str();
                    if !body.is_empty() {
                        let show_toast = match notif_mode {
                            crate::util::NotificationMode::Off => false,
                            crate::util::NotificationMode::Toast => true,
                            crate::util::NotificationMode::Os => {
                                !win_focused
                                    && !crate::util::show_os_notification("Oryxis", body)
                            }
                        };
                        if show_toast {
                            self.set_toast(body.to_string());
                            // Auto-dismiss on a timer only when the window is
                            // focused (you see it now). A toast raised while
                            // unfocused is left up and cleared shortly after you
                            // return (WindowFocusChanged), so it isn't gone
                            // before you look.
                            toast_shown = win_focused;
                        }
                    }
                }
                // Smart-tab notifications ride the same delivery policy, with
                // one twist: they only ever fire for a tab the user was NOT
                // watching, so in OS mode a focused window (background tab)
                // still gets the in-app toast; the native popup is reserved
                // for an unfocused window, exactly like OSC 9 above.
                // Privacy Mode (per pane) surfaces the redacted body and
                // drops the pane identity: the OS notification center keeps
                // plaintext around, and the terminal's masking is
                // render-only, so it must not be sidestepped here.
                for (notif_tab, label, body, redacted) in smart_notifications {
                    // Delivery-time gate (owner report): an IN-APP toast
                    // about the tab currently on screen is never useful.
                    // Looking at the app, the output itself is the signal;
                    // away from it, the toast is invisible. It also closes
                    // the alt-tab race: output buffered while away is
                    // processed in the same batch as (or just before) the
                    // refocus event, so the watched gate above still saw
                    // focused=false and raised a notification for the tab
                    // the user is already reading. OS notifications keep
                    // covering the active tab: telling the user their
                    // long command finished WHILE they are in another app
                    // is that mode's whole point.
                    let about_active_tab = active_tab == Some(notif_tab)
                        && self.active_view == crate::state::View::Terminal;
                    let private = self.privacy_active_for_label(&label);
                    let (title, text) = if private {
                        ("Oryxis".to_string(), redacted)
                    } else {
                        (label, body)
                    };
                    let show_toast = match notif_mode {
                        crate::util::NotificationMode::Off => false,
                        crate::util::NotificationMode::Toast => !about_active_tab,
                        crate::util::NotificationMode::Os => {
                            if win_focused {
                                // In the app: an in-app toast beats a system
                                // banner (unchanged), except about the tab on
                                // screen, where neither is needed.
                                !about_active_tab
                            } else {
                                // Away: the system banner is the point,
                                // active tab included. Toast only as the
                                // failure fallback, and the active-tab gate
                                // still applies to it, "away" may already be
                                // stale in the alt-tab race.
                                !crate::util::show_os_notification(&title, &text)
                                    && !about_active_tab
                            }
                        }
                    };
                    if show_toast {
                        self.set_toast(if private {
                            text
                        } else {
                            format!("{title} \u{b7} {text}")
                        });
                        toast_shown = win_focused;
                    }
                }
                // Session-group per-pane startup script for LOCAL panes. SSH
                // panes inject on `SshConnected`, but a local shell has no
                // such ready event, so we gate on its first output (the
                // prompt) to be sure the shell is reading stdin.
                if self.pane_script_overrides.contains_key(&pane_id) {
                    let is_local = self
                        .tabs
                        .iter()
                        .flat_map(|t| t.pane_grid.panes.values())
                        .find(|p| p.id == pane_id)
                        .map(|p| matches!(p.origin, crate::state::PaneOrigin::Local(_)))
                        .unwrap_or(false);
                    if is_local
                        && let Some(script) = self.pane_script_overrides.remove(&pane_id)
                        && let Some(pane) = self
                            .tabs
                            .iter()
                            .flat_map(|t| t.pane_grid.panes.values())
                            .find(|p| p.id == pane_id)
                        && let Ok(mut state) = pane.terminal.lock()
                    {
                        state.write(format!("{script}\n").as_bytes());
                    }
                }
                // Password autofill (issue #117): the pane borrow is
                // gone, so the overlay slot is writable again. The gates
                // were checked before the borrow; the vault lookup
                // inside decides whether there is anything to offer at
                // all.
                if raise_password_suggest {
                    self.show_password_suggest(pane_id);
                } else if dismiss_stale_suggest {
                    self.dismiss_password_suggest_for(pane_id);
                }
                // Arm the one-shot flush for a synchronized update that
                // stalled with output buffered. Fires `flush_sync` at the
                // 150 ms deadline so a never-closed `?2026` can't leave the
                // screen frozen (see `TerminalSyncFlush`).
                let mut tasks: Vec<iced::Task<Message>> = Vec::new();
                // A detected ZMODEM transfer starts after the clean prefix
                // has been drawn: it seizes the pane and streams progress.
                if let Some((direction, wire)) = zmodem_start {
                    tasks.push(self.begin_zmodem_transfer(pane_id, direction, wire));
                }
                if let Some(remaining) = schedule_flush {
                    tasks.push(Task::perform(
                        async move {
                            tokio::time::sleep(remaining).await;
                        },
                        move |_| Message::Terminal(TerminalMessage::TerminalSyncFlush(pane_id)),
                    ));
                }
                if let Some(fp) = flash_pane {
                    // Clear the visual-bell flash after a brief window.
                    tasks.push(Task::perform(
                        async move {
                            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                        },
                        move |_| Message::Terminal(TerminalMessage::TerminalBellFlashEnd(fp)),
                    ));
                }
                if toast_shown {
                    // Auto-dismiss the fallback notification toast.
                    tasks.push(Task::perform(
                        async {
                            tokio::time::sleep(std::time::Duration::from_millis(3000)).await;
                        },
                        |_| Message::ToastClear,
                    ));
                }
                if cwd_changed {
                    // Follow-cwd for the sidebar Files browser. The sync
                    // no-ops unless the browser is visible, following,
                    // and this pane is the focused one.
                    tasks.push(self.sidebar_files_sync());
                }
                if let Some(entered) = alt_edge {
                    // tmux attach/detach signal: refresh a visible tmux
                    // tab, retire the pane's "attached here" hint on
                    // the falling edge. No-ops for everyone else.
                    tasks.push(self.tmux_alt_screen_edge(pane_id, entered));
                }
                // The local-startup timer rides along, so it survives
                // the no-other-tasks path below too (the zmodem early
                // return above carries its own copy).
                tasks.push(startup_task);
                return Ok(Task::batch(tasks));
            }
            TerminalMessage::LocalStartupDue(pane_id, armed_at) => {
                self.fire_local_startup(pane_id, armed_at);
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        session_log_rows, session_log_segments, utf8_aligned, SessionRow,
        SESSION_LOG_SEGMENT_MS,
    };

    /// Arrival marks for consecutive batches: each batch starts where
    /// the previous one ended, mirroring how the PTY handler records.
    fn marks_for(batches: &[(&[u8], i64)]) -> (Vec<u8>, Vec<(usize, i64)>) {
        let mut head = Vec::new();
        let mut marks = Vec::new();
        for &(bytes, ms) in batches {
            marks.push((head.len(), ms));
            head.extend_from_slice(bytes);
        }
        (head, marks)
    }

    #[test]
    fn cr_prefixed_progress_updates_get_their_own_timed_segments() {
        // wget style: every redraw starts with `\r`. Before the CR fix
        // these coalesced into one untimed lump.
        let gap = SESSION_LOG_SEGMENT_MS;
        let (head, marks) = marks_for(&[
            (b"GET file HTTP/1.1\n", 0),
            (b"\r 10% [==>      ]", gap),
            (b"\r 55% [=====>   ]", gap * 2),
            (b"\r100% [=========]", gap * 3),
        ]);
        let segs = session_log_segments(&head, &marks);
        assert_eq!(segs.len(), 4, "each CR redraw is a replay step: {segs:?}");
        assert_eq!(segs[1], (18, gap));
        assert!(segs.iter().skip(1).all(|&(p, _)| head[p] == b'\r'));
    }

    #[test]
    fn cr_terminated_progress_updates_cut_after_the_cr() {
        // apt style: every redraw ends with `\r`, the next batch starts
        // with printable text right after it.
        let gap = SESSION_LOG_SEGMENT_MS;
        let (head, marks) = marks_for(&[
            (b"Reading... 10%\r", 0),
            (b"Reading... 55%\r", gap),
            (b"Done\n", gap * 2),
        ]);
        let segs = session_log_segments(&head, &marks);
        assert_eq!(segs.len(), 3, "{segs:?}");
        assert!(segs.iter().skip(1).all(|&(p, _)| head[p - 1] == b'\r'));
    }

    #[test]
    fn bursty_marks_coalesce_and_mid_line_marks_never_cut() {
        let gap = SESSION_LOG_SEGMENT_MS;
        // Marks inside one replay step (gap measured from the last CUT,
        // not the last mark) coalesce even at line boundaries.
        let (head, marks) =
            marks_for(&[(b"a\n", 0), (b"b\n", gap / 3), (b"c\n", gap - 1)]);
        assert_eq!(session_log_segments(&head, &marks).len(), 1);
        // A big gap mid-line (no `\n`/`\r` anywhere near) still can't
        // cut: chunk boundaries must stay line-bounded for redaction.
        let (head, marks) = marks_for(&[(b"export TOKEN=abc", 0), (b"def123\n", gap * 4)]);
        assert_eq!(session_log_segments(&head, &marks).len(), 1);
        // No marks at all: a single segment at t=0.
        assert_eq!(session_log_segments(b"x", &[]), vec![(0, 0)]);
    }

    #[test]
    fn initial_geometry_resize_lands_before_the_first_chunk() {
        // The first output batch records the grid it was processed at
        // as a resize mark at position 0: replay must build the grid
        // BEFORE feeding the connect window's bytes (the player and
        // the .cast header read their initial geometry from it).
        let (head, marks) = marks_for(&[(b"Welcome\n", 0)]);
        let rows = session_log_rows(&head, &marks, &[(0, 0, 120, 40)]);
        assert_eq!(
            rows,
            vec![
                SessionRow::Resize(0, 120, 40),
                SessionRow::Chunk(0, 0..head.len()),
            ]
        );
    }

    #[test]
    fn mid_stream_resize_cuts_the_chunk_and_sits_between() {
        // Bytes before the resize replay on the old grid, bytes after
        // it on the new one, even when the arrival marks alone would
        // have coalesced everything into a single chunk.
        let (head, marks) = marks_for(&[(b"before\n", 0), (b"after\n", 10)]);
        let rows = session_log_rows(&head, &marks, &[(7, 10, 90, 30)]);
        assert_eq!(
            rows,
            vec![
                SessionRow::Chunk(0, 0..7),
                SessionRow::Resize(10, 90, 30),
                SessionRow::Chunk(10, 7..head.len()),
            ]
        );
    }

    #[test]
    fn mid_line_resize_snaps_back_to_a_line_boundary() {
        // Chunk cuts must stay line-bounded (secret runs contain no
        // `\n`/`\r`, so a cut there can't split one across redaction
        // chunks): a resize mark mid-line snaps its cut back to the
        // previous boundary, and the partial line replays after the
        // resize, matching the live grid's reflow.
        let (head, marks) = marks_for(&[(b"line\n", 0), (b"par", 5), (b"tial\n", 20)]);
        let rows = session_log_rows(&head, &marks, &[(8, 20, 100, 50)]);
        assert_eq!(
            rows,
            vec![
                SessionRow::Chunk(0, 0..5),
                SessionRow::Resize(20, 100, 50),
                SessionRow::Chunk(5, 5..head.len()),
            ]
        );
    }

    #[test]
    fn trailing_resize_lands_after_the_last_chunk() {
        // The flush-cadence fallback records a resize with no output
        // after it at the buffer's end; with no bytes at all (final
        // flush of an idle pane) the row still flushes on its own.
        let (head, marks) = marks_for(&[(b"done\n", 0)]);
        let rows = session_log_rows(&head, &marks, &[(5, 30, 80, 24)]);
        assert_eq!(
            rows,
            vec![
                SessionRow::Chunk(0, 0..head.len()),
                SessionRow::Resize(30, 80, 24),
            ]
        );
        let rows = session_log_rows(&[], &[], &[(0, 40, 80, 24)]);
        assert_eq!(rows, vec![SessionRow::Resize(40, 80, 24)]);
    }

    #[test]
    fn consecutive_resizes_at_one_position_all_replay_in_order() {
        // Two size changes with no bytes between them (a live drag
        // sampled across batches) keep both rows, in order.
        let (head, marks) = marks_for(&[(b"x\n", 0)]);
        let rows = session_log_rows(
            &head,
            &marks,
            &[(2, 10, 100, 40), (2, 20, 110, 42)],
        );
        assert_eq!(
            rows,
            vec![
                SessionRow::Chunk(0, 0..head.len()),
                SessionRow::Resize(10, 100, 40),
                SessionRow::Resize(20, 110, 42),
            ]
        );
    }

    #[test]
    fn utf8_aligned_backs_off_split_sequences_only() {
        // "é" = C3 A9. A cut between the bytes moves before the lead.
        let buf = b"abc\xC3\xA9";
        assert_eq!(utf8_aligned(buf, 4), 3);
        assert_eq!(utf8_aligned(buf, 5), 5);
        // 4-byte emoji (F0 9F 92 96) split at every interior position.
        let buf = b"ok\xF0\x9F\x92\x96";
        for cut in 3..6 {
            assert_eq!(utf8_aligned(buf, cut), 2, "cut at {cut}");
        }
        assert_eq!(utf8_aligned(buf, 6), 6);
        // Pure ASCII and raw binary keep the requested cut.
        assert_eq!(utf8_aligned(b"hello", 5), 5);
        assert_eq!(utf8_aligned(&[0xFF; 8], 8), 8);
        assert_eq!(utf8_aligned(&[0x80; 8], 8), 8);
    }
}
