//! Terminal pane / PTY / input / search messages.

use iced::keyboard;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum TerminalMessage {
    PtyOutput(Uuid, Vec<u8>),  // (pane_id, bytes)
    /// A local host's startup command is due: its pane produced output
    /// and then went quiet. Carries the output-batch count the timer was
    /// armed at, so a shell still printing (a MOTD, a slow profile)
    /// re-arms instead of firing into the middle of the banner.
    LocalStartupDue(Uuid, u64),
    /// One-shot wake-up that force-flushes a stalled DEC `?2026`
    /// synchronized update on the given pane (`pane_id`). Armed by the
    /// `PtyOutput` handler when output stops mid-update; without it an app
    /// that opens a sync update and blocks on input freezes the screen.
    TerminalSyncFlush(Uuid),
    /// Scrollback find-bar (C1). All four act on the ACTIVE pane of the
    /// active terminal tab (the bar only ever shows there).
    /// Open the find-bar (Ctrl+F over the terminal) and focus its input.
    TerminalSearchOpen,
    /// The find-bar needle changed: rebuild matches and scroll the first
    /// hit into view.
    TerminalSearchInput(String),
    /// Step the active match forward (`true`, Enter) or backward
    /// (`false`, Shift+Enter), wrapping, and scroll it into view.
    TerminalSearchStep(bool),
    /// Close the find-bar (Esc) and drop the match set; the terminal keeps
    /// focus.
    TerminalSearchClose,
    /// Wake-up for a running login script (issue #122). The engine
    /// checks its per-step deadline on poll, and poll is driven by
    /// output, so a bastion that goes silent would never time out
    /// without this. The generation makes a stale tick from a finished
    /// run a no-op instead of an abort of the run after it.
    LoginScriptTick(uuid::Uuid, u64),
    /// Password-suggest popup (issue #117). Move the selection by
    /// `i32` rows, wrapping. The first move ENGAGES the popup: until
    /// then nothing is selected and Enter still belongs to the prompt.
    PasswordSuggestNavigate(i32),
    /// Send the credential at `usize` to the pane the popup belongs to,
    /// then close. Decrypt happens here, never at show time. The row
    /// rides in the message rather than being read back from the
    /// selection: a click must work even when the popup opened under a
    /// stationary cursor, where no hover ever fires.
    PasswordSuggestPick(usize),
    /// Close the popup without sending anything (Esc, a click on the
    /// terminal, a tab switch, a disconnect). The prompt's signature
    /// stays recorded, so it does not immediately reopen.
    ///
    /// There is deliberately no Hover message: hover highlighting comes
    /// from the row button's own `Status::Hovered` styling, and letting
    /// hover set the SELECTION would arm Enter, so brushing the mouse
    /// across the popup (which opens at the caret, right where the
    /// pointer sits) plus an Enter aimed at the prompt would send a
    /// secret nobody picked.
    PasswordSuggestDismiss,
    /// New scroll offset of the popup's row list, reported by its own
    /// `on_scroll`. Tracked so keyboard navigation scrolls only when the
    /// selection would leave the viewport, instead of yanking the list
    /// on every arrow press (same contract as the SFTP row list).
    PasswordSuggestScrolled(f32),
    /// Broadcast input (C2): arm / disarm fan-out of keystrokes, pastes and
    /// snippets to every pane of the tab at `usize`. Toggled by the status
    /// segment, the tab context menu and the `ToggleBroadcastInput` hotkey.
    /// Arming requires a split tab: on a single-pane tab the handler
    /// refuses with a hint toast (the segment and menu entry are not
    /// rendered there, so only the hotkey / palette reach it).
    ToggleTabBroadcast(usize),
    /// Broadcast input (C2): flip whether the pane at `Uuid` participates in
    /// its tab's broadcast (the per-pane observer opt-out).
    TogglePaneBroadcastOptOut(Uuid),
    KeyboardEvent(keyboard::Event),
    /// Text committed by the OS IME (e.g. a composed CJK character).
    /// Arrives separately from `KeyboardEvent`; forwarded to the active
    /// PTY in `dispatch_terminal` behind the same focus guards.
    TerminalImeCommit(String),
    /// IME preedit (composition) update, e.g. the pinyin syllables while a
    /// CJK input method is composing. Stored on the focused pane's
    /// `TerminalState` so the `ime_host` overlay can render it at the
    /// caret; an empty string clears it.
    TerminalImePreedit(String),
    /// Focus a pane (click). Routes keyboard / snippets / paste to it.
    FocusPane(iced::widget::pane_grid::Pane),
    /// Drag a pane divider to resize.
    ResizePane(iced::widget::pane_grid::ResizeEvent),
    /// Split the focused pane of the active tab along an axis, opening the
    /// connection picker to fill the new pane.
    SplitPane(iced::widget::pane_grid::Axis),
    /// Like `SplitPane` but targets a specific tab (from its right-click
    /// menu), so it works even when that tab isn't the active one.
    SplitTabPane(usize, iced::widget::pane_grid::Axis),
    /// Close a pane (closes its tab if it was the tab's last one).
    /// `Some(pane_id)` targets that exact pane, re-resolved at dispatch
    /// time (the context-menu row: focus and the active tab can change
    /// via hotkeys while the menu overlay is open, and the pane may be
    /// gone entirely, a safe no-op). `None` closes the focused pane of
    /// the active tab (the hotkey path).
    ClosePane(Option<Uuid>),
    /// Re-dial ONE pane in place, keeping its terminal and scrollback
    /// (issue #208). The pane-scoped counterpart of `ReconnectTab`,
    /// which is tab-wide and rebuilds a split tab's live siblings along
    /// with the dead pane. Raised by the ended-pane card's Restart
    /// button and by the Reconnect action when the focused pane of a
    /// split tab has ended.
    RestartPane(Uuid),
    /// A local pane's shell exited, reported by the child-exit signal
    /// `PtyHandle` hands out. Deliberately not driven by the output
    /// stream ending: a pty's reader cannot see a child die (see
    /// `PtyHandle::take_child_exit`), so the stream would only ever
    /// report teardown.
    ///
    /// The generation is the guard, like `LoginScriptTick`'s: swapping a
    /// pane's `TerminalState` drops the old PTY, so a restart-in-place
    /// ends the OLD session and this message arrives for a pane that is
    /// alive again. A stale generation is discarded.
    LocalPaneEnded(Uuid, u64),
    /// Move focus to the adjacent pane in a direction (keyboard nav).
    FocusPaneDir(iced::widget::pane_grid::Direction),
    /// Expand the focused pane to the whole tab, and back. `None` targets
    /// the active tab (hotkey); `Some(idx)` a specific one (tab menu).
    ToggleMaximizePane(Option<usize>),
    /// Expand a SPECIFIC pane to the whole tab, and back. Carries the
    /// right-clicked pane's id for the same reason `ClosePane` does: the
    /// menu overlay is not modal, so focus and the active tab can move
    /// out from under it.
    ToggleMaximizePaneAt(Uuid),
    /// Flip the orientation of the split that separates this pane from
    /// its neighbour (stacked <-> side by side).
    FlipPaneSplit(Uuid),
    /// Periodic flush of buffered session-log output to the vault.
    SessionLogFlushTick,
    /// Emitted by the terminal widget when the user right-clicks. The
    /// dispatcher reads the clipboard and routes the text to the SSH
    /// session (if active) or the local PTY, mirroring Ctrl+Shift+V.
    TerminalPasteFromClipboard,
    /// Clipboard text handed back by the runtime for a pending paste, with
    /// the tab index the paste was requested FROM (`None` text = empty or
    /// unavailable). Every paste path funnels through here: the runtime is
    /// the only clipboard reader in the process, so a second concurrent read
    /// can't corrupt the heap (see `oryxis_terminal::host_clipboard`). The
    /// tab rides along because the read resolves later and the user may have
    /// switched tabs in between.
    TerminalPasteResolved(Uuid, Option<super::Redacted>),
    /// PRIMARY selection handed back by the runtime for a middle-click /
    /// Shift+Insert paste, with the tab it was requested FROM and the text
    /// that pane last selected itself. The system buffer wins (the user may
    /// have highlighted in another window), the pane's own selection is the
    /// fallback, and an empty pair falls through to the clipboard, which is
    /// what the gesture did before a pane had ever been selected in.
    /// Only ever sent where `oryxis_terminal::has_primary_selection` holds.
    TerminalPasteSelectionResolved(Uuid, Option<super::Redacted>, super::Redacted),
    /// Careful-paste confirmation: send the multi-line text held in
    /// `pending_paste` to the tab it was captured for (not the currently
    /// active one, which may have changed since).
    ConfirmPendingPaste,
    /// Careful-paste confirmation dismissed: drop the held text.
    CancelPendingPaste,
    /// Raw input bytes synthesized by the terminal widget (mouse-tracking
    /// reports, wheel-to-arrow translation). Routed to the active SSH
    /// session, falling back to the local PTY.
    TerminalInput(Vec<u8>),
    /// The user left-dragged in a pane whose remote app has mouse tracking
    /// on, so the drag is being reported instead of selecting text. Shows
    /// the "hold Shift to select" toast. Fires at most once per pane.
    TerminalMouseCaptureHint,
    /// The user plain-clicked (no Ctrl) a link in the terminal, so it
    /// selected instead of opening. Shows the "hold Ctrl and click to
    /// open" toast; under `HintMode::Once` it fires at most once per pane.
    TerminalLinkClickHint,
    /// Open the terminal context menu for a pane at a window-absolute
    /// point (right-click scheme = Menu). `(pane_id, x, y, selection)`,
    /// where `selection` is the live selection's text captured by the
    /// widget (`None` when empty), so the menu can offer "Copy".
    ShowTerminalContextMenu(Uuid, f32, f32, Option<String>),
    /// Copy the captured selection text to the clipboard (context-menu
    /// "Copy").
    TerminalCopySelection(String),
    /// Paste the X11 PRIMARY selection into a pane: middle-click, the
    /// paste-selection action, or the context-menu row. `(pane_id, text)`;
    /// the pane is explicit because every sender knows it (the widget
    /// captures its own at build time, the menu carries the right-clicked
    /// one) and the focused pane can change before this is handled. Never
    /// touches the system clipboard.
    TerminalPasteSelection(Uuid, super::Redacted),
    /// Flush the buffered OS drop (a multi-file drop arrives as one
    /// FileDropped per file): resolve the target pane and route the
    /// batch to its transport. Fired by a short debounce after the
    /// first file of the gesture.
    TerminalDropFlush,
    /// The OS-drop SFTP upload task streamed a progress event for a
    /// pane. Terminal events (Done / Failed / Cancelled) clear the
    /// pane's card and toast the outcome.
    TerminalDropProgress(Uuid, crate::state::DropProgress),
    /// User asked to cancel the pane's in-flight OS-drop upload.
    TerminalDropCancel(Uuid),
    /// Copy the whole buffer (scrollback + screen) of a pane to the
    /// clipboard (context-menu "Copy All"). `pane_id`.
    TerminalCopyAll(Uuid),
    /// Copy only the pane's displayed viewport, scroll position included and
    /// off-screen history excluded (context-menu "Copy Screen"). `pane_id`.
    TerminalCopyScreen(Uuid),
    /// Drop a pane's scrollback history (context-menu "Clear
    /// Scrollback"). `pane_id`.
    TerminalClearScrollback(Uuid),
    /// Clear a pane's visual-bell flash after its short display window.
    TerminalBellFlashEnd(Uuid),
    /// The answer to "may this highlight rule run its snippet on this
    /// session" (C6). Remembered for the session either way.
    TriggerConfirmDecision(bool),
    /// Ctrl+click activated a link in a pane: `(pane_id, url)`. The
    /// widget hands the resolved target over instead of opening it,
    /// because what happens next (confirm, tunnel a loopback callback)
    /// depends on the pane's session.
    TerminalLinkActivated(Uuid, String),
    /// The answer to "open this link?". `false` opens nothing.
    TerminalLinkDecision(bool),
    /// Copy the pending link's target instead of opening it. Also an
    /// answer to the question, so it closes the dialog.
    TerminalLinkCopy,
    /// A link's callback tunnel settled: `(pane_id, port, url, result)`.
    /// The browser is launched from here on success, never before: the
    /// redirect can arrive a second after the user finishes at the
    /// provider, so the listener has to be up first.
    TerminalLinkTunnelReady(
        Uuid,
        u16,
        String,
        Result<std::sync::Arc<oryxis_ssh::ForwardSession>, String>,
    ),
    /// A callback tunnel closed itself (served its redirect, or waited
    /// out its unused timeout): `(pane_id, port)`. Drops the app's
    /// bookkeeping entry.
    TerminalLinkTunnelClosed(Uuid, u16),
}
