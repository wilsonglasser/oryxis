//! Row interaction arms split out of `dispatch_sftp`: right-click
//! menus, path copying, hover tracking, drag arming, click selection
//! (single / ctrl / shift / double / slow-rename), the SFTP keyboard
//! handling and type-ahead. Called from `handle_sftp`.

#![allow(clippy::result_large_err)]

use iced::Task;

use std::time::Duration;

use crate::app::{SftpMessage, Message, Oryxis};
use crate::state::SftpPaneSide;

/// Max gap between two clicks on the same folder to count as a double-click.
/// Matches the Windows system default so slow double-clickers still land an
/// "open" instead of falling through to selection (or worse, rename).
const DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(500);
/// Slow-click-to-rename arms no earlier than this after the previous click on
/// the same row. The gap between DOUBLE_CLICK_WINDOW and here is a dead zone
/// where a second click only re-selects: a sluggish double-click (intent:
/// open) lands there and must never be read as an edit request. Explorer and
/// Finder have no dead zone and famously do misread it, hence keeping ours.
/// There is deliberately no upper bound, matching those two: a row that is
/// still the lone selection stays renameable, and the other two gates (the
/// name hit test and the cancellable deferral) are what keep the affordance
/// from firing by accident.
const SLOW_RENAME_MIN: Duration = Duration::from_millis(900);
/// How long an armed slow-click rename waits before it actually opens the
/// inline editor. A further click inside this window bumps `click_gen` and
/// cancels it, which is what makes a sluggish double-click open the folder
/// instead of renaming it (the whole complaint behind this tuning).
const SLOW_RENAME_DEFER: Duration = Duration::from_millis(500);
/// Idle gap after which type-ahead starts a fresh search instead of
/// appending to the previous one.
const TYPE_AHEAD_RESET: Duration = Duration::from_millis(900);
/// Debounce before type-ahead actually searches, so fast typing resolves
/// once with the full buffer instead of on every keystroke.
const TYPE_AHEAD_DEBOUNCE: Duration = Duration::from_millis(150);

impl Oryxis {
    /// Arm a pending internal drag for a pressed SFTP row. Stays
    /// `active = false` until the cursor reaches the other pane, so a plain
    /// click still flows through. Called both from the global left-press
    /// (via `hovered_row`) and from the row button's own `on_press`: the
    /// latter is the reliable path for a truncated row, whose hover tooltip
    /// can drop `hovered_row` before the press lands (issue: truncated names
    /// wouldn't drag). No-op if a drag is already armed for this press.
    fn arm_sftp_row_drag(&mut self, side: SftpPaneSide, path: String, is_dir: bool) {
        if self.sftp.drag.is_some() {
            return;
        }
        // Rows inside a browsed archive carry synthetic paths the
        // transfer queue can't read; copy-out goes through the context
        // menu instead of drag-and-drop.
        if self.sftp.pane(side).zip.is_some() {
            return;
        }
        // Drag the entire same-pane selection if the pressed row is part of
        // it; otherwise drag just this row.
        let same_side: Vec<(String, bool)> = self
            .sftp
            .selected_rows
            .iter()
            .filter(|(s, _)| *s == side)
            .map(|(_, p)| {
                let is_dir = self.row_is_dir_in_pane(side, p);
                (p.clone(), is_dir)
            })
            .collect();
        let pressed_in_selection = same_side.iter().any(|(p, _)| p == &path);
        let items: Vec<(String, bool)> = if pressed_in_selection {
            same_side
        } else {
            vec![(path.clone(), is_dir)]
        };
        let label = if items.len() > 1 {
            format!("{} items", items.len())
        } else {
            path.rsplit(['/', '\\'])
                .find(|s| !s.is_empty())
                .unwrap_or(&path)
                .to_string()
        };
        // The same press also arms a drag-out (issue #167). The two
        // gestures share the press and separate on where the cursor
        // goes: into the opposite pane it is the internal transfer,
        // out of the window it is the OS drag. Only the FILES of the
        // selection travel: a directory needs recursive descriptors
        // the data object doesn't build yet, and offering a folder
        // that arrives empty is worse than not offering it.
        self.arm_drag_out_from_sftp(side, &items, &label);
        self.sftp.drag = Some(crate::state::SftpInternalDrag {
            origin_side: side,
            items,
            label,
            press_pos: self.mouse_position,
            active: false,
        });
    }

    /// Build the drag-out payload for a pressed SFTP row (or the whole
    /// same-pane selection it belongs to), the SFTP surface's half of
    /// the arm `dispatch_sidebar_files::navigate` does for the sidebar
    /// browser. The pane is in hand here, so the listing's sizes ride
    /// along as the floor `prepare`'s own `stat` is measured against.
    fn arm_drag_out_from_sftp(
        &mut self,
        side: SftpPaneSide,
        items: &[(String, bool)],
        label: &str,
    ) {
        self.drag_out_arm = None;
        if !crate::drag_out::supported() {
            return;
        }
        let files: Vec<&String> = items
            .iter()
            .filter(|(_, is_dir)| !is_dir)
            .map(|(path, _)| path)
            .collect();
        // A press that ends up being an ordinary cross-pane drag still
        // paid for the payload, so the arm stays off for selections
        // where that price stops being invisible: `prepare` stats every
        // file, and past a point those round trips are a queue in front
        // of the pane's own listings. Dragging hundreds of files OUT of
        // the window is not the gesture anyway; the context menu's
        // download is.
        const MAX_DRAG_OUT_FILES: usize = 64;
        if files.is_empty() || files.len() > MAX_DRAG_OUT_FILES {
            return;
        }
        let pane = self.sftp.pane(side);
        let payload = if pane.is_remote {
            let Some(client) = pane.client.clone() else {
                return;
            };
            let files = files
                .into_iter()
                .map(|path| {
                    let name = crate::dispatch_sidebar_files::files_basename(path);
                    let size = pane
                        .remote_entries
                        .iter()
                        .find(|e| e.name == name)
                        .map_or(0, |e| e.size);
                    crate::drag_out::RemoteDragFile {
                        path: path.clone(),
                        name,
                        size,
                    }
                })
                .collect();
            crate::drag_out::DragOutPayload::Remote { client, files }
        } else {
            crate::drag_out::DragOutPayload::Local(
                files.into_iter().map(std::path::PathBuf::from).collect(),
            )
        };
        self.drag_out_arm = Some(crate::drag_out::DragOutArm {
            press: self.mouse_position,
            label: label.to_string(),
            stage: crate::drag_out::DragOutStage::Armed(payload),
        });
    }

    /// Turn a slow-click rename armed on the press (and not cancelled by a
    /// drag) into a DEFERRED fire: the inline editor only opens once the
    /// deferral elapses with no further click, so the second click of a
    /// sluggish double-click still opens the folder. Called from the global
    /// left-release handler; no-op when nothing is armed.
    pub(crate) fn defer_slow_rename(&mut self) -> Task<Message> {
        let Some((side, path)) = self.sftp.pending_rename.take() else {
            return Task::none();
        };
        let generation = self.sftp_click_gen;
        Task::perform(
            async move {
                tokio::time::sleep(SLOW_RENAME_DEFER).await;
            },
            move |_| {
                Message::Sftp(SftpMessage::SftpSlowRenameFire(
                    side,
                    path.clone(),
                    generation,
                ))
            },
        )
    }

    pub(super) fn handle_sftp_selection(
        &mut self,
        message: SftpMessage,
    ) -> Result<Task<Message>, SftpMessage> {
        match message {
            SftpMessage::SftpRowRightClick(side, path, is_dir) => {
                // A right-click retires any deferred rename too: the user
                // asked for the menu, not for an editor to pop open behind it.
                self.sftp_click_gen = self.sftp_click_gen.wrapping_add(1);
                // If the user right-clicks a row that wasn't part of the
                // current selection, treat the right-click as a fresh
                // single-select, matches Finder/Explorer behaviour and
                // means menu actions never silently target a different
                // set of rows than the visual selection suggests.
                let target = (side, path.clone());
                let in_selection = self.sftp.selected_rows.contains(&target);
                if !in_selection {
                    self.sftp.selected_rows = vec![target.clone()];
                    self.sftp.selection_anchor = Some(target);
                }
                self.sftp.row_menu = Some(crate::state::SftpRowMenu {
                    open_group: false,
                    side,
                    path,
                    is_dir,
                    is_background: false,
                    x: self.mouse_position.x,
                    y: self.mouse_position.y,
                });
            }
            SftpMessage::SftpBackgroundRightClick(side) => {
                // Empty-area right-click: `path` carries the pane's current
                // directory so the directory-level actions act on it.
                let pane = self.sftp.pane(side);
                let dir = if pane.is_remote {
                    pane.remote_path.clone()
                } else {
                    pane.local_path.to_string_lossy().into_owned()
                };
                self.sftp.row_menu = Some(crate::state::SftpRowMenu {
                    open_group: false,
                    side,
                    path: dir,
                    is_dir: true,
                    is_background: true,
                    x: self.mouse_position.x,
                    y: self.mouse_position.y,
                });
            }
            SftpMessage::SftpRowMenuClose => {
                self.sftp.row_menu = None;
            }
            SftpMessage::SftpCopyPath(path) => {
                // The string arrives already side-formatted (POSIX for a
                // remote entry, OS-native for a local one), so this is a
                // straight clipboard write via the shared toast path.
                // Fired from the SFTP row menu, the pane's actions (kebab)
                // menu AND the sidebar Files row menu; dismiss whichever
                // is open.
                self.sftp.close_menus();
                self.overlay = None;
                return Ok(self.update(Message::CopyToClipboard(path)));
            }
            SftpMessage::SftpCopySelectionPaths(side) => {
                // Bulk variant: every selected path in the menu's pane,
                // one per line. Selection is left intact, copying is
                // not an action "on" the rows the way duplicate is.
                self.sftp.row_menu = None;
                let paths: Vec<String> = self
                    .sftp
                    .selected_rows
                    .iter()
                    .filter(|(s, _)| *s == side)
                    .map(|(_, p)| p.clone())
                    .collect();
                if paths.is_empty() {
                    return Ok(Task::none());
                }
                return Ok(self.update(Message::CopyToClipboard(paths.join("\n"))));
            }
            SftpMessage::SftpRowEnter(side, path, is_dir) => {
                // With the right-click menu open, the pixels in the gaps
                // between its items still sit over the list rows behind it,
                // so a bare on_enter would light up a row under the menu.
                // The list is inert while the menu is up (no drag is in
                // flight then either), so ignore the hover entirely.
                if self.sftp.row_menu.is_some() {
                    return Ok(Task::none());
                }
                self.sftp.hovered_row = Some((side, path, is_dir));
                // Promote a pending drag to active once the cursor reaches a
                // row in the *other* pane. This is a secondary trigger: the
                // primary one is the cursor-geometry crossing in the
                // MouseMoved handler (reliable during a button-hold, same as
                // the divider drags). Row-hover can be disrupted by tooltips
                // / row gaps, so it can't be the sole signal. Activating
                // lights up the destination pane outline as drag feedback.
                if let Some(drag) = self.sftp.drag.as_mut()
                    && !drag.active
                    && drag.origin_side != side
                {
                    drag.active = true;
                }
            }
            SftpMessage::SftpRowExit(side, path) => {
                // Only if this row is still the one recorded. Moving the
                // pointer between rows delivers `enter` for the new row and
                // `exit` for the old one in TREE order, not in the order
                // they happened, so walking UP the list used to publish
                // enter(above) then exit(below) and the unconditional clear
                // wiped the row that had just been entered. `hovered_row`
                // is what the left-press reads to arm a drag, so that lost
                // value is a drag that never starts: the reported "works
                // maybe one time in ten" when grabbing a row.
                if self
                    .sftp
                    .hovered_row
                    .as_ref()
                    .is_some_and(|(s, p, _)| *s == side && *p == path)
                {
                    self.sftp.hovered_row = None;
                }
            }
            SftpMessage::SftpNameHovered(side, path) => {
                // Same menu guard as SftpRowEnter: the gaps between the open
                // context menu's items sit over the rows behind it.
                if self.sftp.row_menu.is_some() {
                    return Ok(Task::none());
                }
                self.sftp.hovered_name = Some((side, path));
            }
            SftpMessage::SftpNameUnhovered(side, path) => {
                self.sftp.leave_name((side, path));
            }
            SftpMessage::SftpSlowRenameFire(side, path, generation) => {
                if !slow_rename_still_valid(
                    generation,
                    self.sftp_click_gen,
                    &self.sftp.selected_rows,
                    (side, &path),
                ) {
                    return Ok(Task::none());
                }
                return Ok(Task::done(Message::Sftp(SftpMessage::SftpStartRename(side, path))));
            }
            SftpMessage::SftpMouseLeftPressed => {
                // Consume the row identity this press's `press_hit_reporter`
                // wrapper recorded (if any) FIRST, before any early return
                // below: this message fires exactly once per press, so a
                // value left behind by a bailed-out press would otherwise
                // be mistaken for the next press's hit.
                let pressed_row = self.sftp.row_press.borrow_mut().take();
                // Any physical click leaves keyboard-selection mode: the
                // mouse took over, a lingering ring would just be noise.
                // Also drops the modal-layer selection so a menu closed
                // by an outside click (or reopened on another card)
                // starts fresh at its default row.
                self.keynav.focus = None;
                self.keynav.modal.selected = None;
                // Same rule for the terminal-sidebar ring: clicking into a
                // sidebar input (or any row) must not leave the ring parked
                // on whatever the Tab walk last visited (live QA: ring stuck
                // on the Close button while the Files path input had focus).
                self.keynav.sidebar_selected = None;
                // A physical left press over a tab arms a potential reorder
                // drag. Armed here (on the real button press) rather than in
                // SelectTab, so programmatic SelectTab dispatches (the
                // tab-jump modal, etc.) can't trigger a phantom drag.
                // Geometric guard on top of the hover flag: `hovered_tab`
                // comes from MouseArea enter/exit and the exit can be lost
                // (cursor sliding straight into the terminal canvas), after
                // which ANY press, e.g. starting a terminal text selection,
                // armed a phantom drag whose ghost chip then chased the
                // cursor (field report). The band must follow the strip's
                // actual dock: a hard-coded top `y <= BAR_HEIGHT` guard
                // silently disabled reorder on every non-top dock (issue
                // #87, "can't move tabs on the left side").
                let in_tab_strip = self.cursor_in_tab_strip();
                if !in_tab_strip {
                    self.hover.tab = None;
                    self.hover.sftp_tab = None;
                    self.hover.panel_tab = None;
                }
                if let Some(idx) = self.hover.tab.filter(|_| in_tab_strip)
                    && let Some(tab) = self.tabs.get(idx)
                {
                    self.tab_drag = Some(crate::state::TabDrag {
                        from_id: tab._id,
                        start: self.mouse_position,
                        active: false,
                    });
                } else if let Some(idx) = self.hover.sftp_tab.filter(|_| in_tab_strip)
                    && let Some(tab) = self.sftp_tabs.get(idx)
                {
                    // SFTP tabs arm the same unified reorder drag.
                    self.tab_drag = Some(crate::state::TabDrag {
                        from_id: tab.id,
                        start: self.mouse_position,
                        active: false,
                    });
                } else if let Some(kind) = self.hover.panel_tab.filter(|_| in_tab_strip) {
                    // So do the panel tabs, under their synthetic ids
                    // (issue #120): the reorder machinery is uuid-keyed
                    // and `TabRef::strip_id` answers with the same value.
                    self.tab_drag = Some(crate::state::TabDrag {
                        from_id: kind.tab_id(),
                        start: self.mouse_position,
                        active: false,
                    });
                }
                // A click outside the sidebar (i.e. into the terminal or
                // the vault area) cancels any in-progress inline edit in
                // the sidebar Files browser (path / rename / new entry),
                // mirroring the SFTP pane's click-outside-commits rule.
                // Clicks that land ON the sidebar keep the edit (the
                // text_input handles its own caret placement).
                if self.sidebar_tab_shown(crate::state::TerminalSidebarTab::Files)
                    && !self.cursor_over_sidebar()
                    && let Some(idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(idx)
                {
                    let files = &mut tab.active_mut().files;
                    if files.path_editing.is_some()
                        || files.rename.is_some()
                        || files.new_entry.is_some()
                    {
                        files.path_editing = None;
                        files.rename = None;
                        files.new_entry = None;
                    }
                    // The path-history dropdown's scrim only covers the
                    // sidebar; an outside click closes it here.
                    files.path_history_open = false;
                }
                // Begin a potential internal drag if the cursor is
                // currently on a row in the SFTP view. The drag stays
                // pending (active=false) until the user moves past the
                // threshold, that way plain clicks still flow to the
                // button's on_press handler.
                if !self.sftp_surface_visible() {
                    return Ok(Task::none());
                }
                // A press outside the inline-rename input commits the rename
                // (click any other row, empty area, or the other pane). A
                // press *inside* the input doesn't fire SftpSelectRow and
                // keeps `hovered_row` on the rename's own row, so we leave it
                // be and let the user keep editing.
                if let Some(rn) = self.sftp.rename.as_ref() {
                    let on_rename_row = self.sftp.hovered_row.as_ref().is_some_and(
                        |(s, p, _)| *s == rn.side && *p == rn.original_path,
                    );
                    if !on_rename_row {
                        return Ok(self.commit_rename());
                    }
                }
                // Which row the press landed on, recorded by the row's
                // `press_hit_reporter` wrapper at press time. `hovered_row`
                // is only the fallback: it is hover state, so a truncated
                // name's tooltip overlay drops it (the reason an arm was
                // once bolted onto the row button's on_press, which fires
                // on RELEASE and so can never help a drag), and iced
                // publishes enter / exit in tree order, which reorders it.
                // Draw-time rects were no good either: under a scrollable
                // they are content-space while the mouse is screen-space,
                // so a scrolled list dragged the wrong row by exactly the
                // scroll offset (issue #127). The press-time test happens
                // where iced itself translates the cursor, so it depends
                // on neither hover nor tracked offsets.
                if let Some((side, path, is_dir)) =
                    pressed_row.or_else(|| self.sftp.hovered_row.clone())
                {
                    self.arm_sftp_row_drag(side, path, is_dir);
                }
            }
            SftpMessage::SftpSelectRow(side, path, is_dir) => {
                // Arm a potential drag from the button's own press, before the
                // selection below collapses, using the exact pressed row. A
                // second arm path alongside the global left-press; no-op if
                // that already armed it. (Cross-pane activation itself happens
                // later via cursor geometry in MouseMoved.)
                self.arm_sftp_row_drag(side, path.clone(), is_dir);
                // Keyboard focus follows the mouse: a clicked row's pane
                // becomes the focused pane and the cursor leaves the ".." row.
                self.sftp.focused_side = side;
                self.sftp.parent_cursor = false;
                let target = (side, path.clone());
                let ctrl = self.modifiers.control() || self.modifiers.command();
                let shift = self.modifiers.shift();
                // Every click retires any rename armed by an earlier one (see
                // SftpSlowRenameFire), so a second click always wins over a
                // pending edit.
                self.sftp_click_gen = self.sftp_click_gen.wrapping_add(1);
                // Slow-click-to-rename, Explorer / Finder semantics: a plain
                // click that lands ON THE NAME of the row that is already the
                // lone selection, no sooner than a double-click after the
                // previous click, arms an inline rename. Three gates keep it
                // off real open attempts: the name hit test (a click on the
                // Size / Modified columns or the slack past a short name is
                // just a click), the release check below (a drag isn't a
                // rename), and the deferred fire (a following click cancels).
                let now = std::time::Instant::now();
                let already_sole = self.sftp.selected_rows.as_slice() == [target.clone()];
                let on_name = self
                    .sftp
                    .hovered_name
                    .as_ref()
                    .is_some_and(|(s, p)| *s == side && p == &path);
                let slow_second = !ctrl
                    && !shift
                    && already_sole
                    && on_name
                    && self.sftp.last_click.as_ref().is_some_and(|(s, p, t)| {
                        *s == side && p == &path && now.duration_since(*t) >= SLOW_RENAME_MIN
                    });
                self.sftp.pending_rename =
                    slow_second.then(|| (side, path.clone()));
                if shift {
                    // Range select within same pane. If the anchor lives
                    // in the other pane (or doesn't exist), fall through
                    // to a single-select to avoid silent cross-pane jumps.
                    if let Some(anchor) = self.sftp.selection_anchor.clone()
                        && anchor.0 == side
                    {
                        let entries = self.visible_entry_paths_in_pane(side);
                        let a = entries.iter().position(|p| p == &anchor.1);
                        let t = entries.iter().position(|p| p == &path);
                        if let (Some(ai), Some(ti)) = (a, t) {
                            let (lo, hi) = if ai <= ti { (ai, ti) } else { (ti, ai) };
                            self.sftp.selected_rows = entries[lo..=hi]
                                .iter()
                                .map(|p| (side, p.clone()))
                                .collect();
                            return Ok(Task::none());
                        }
                    }
                    self.sftp.selected_rows = vec![target.clone()];
                    self.sftp.selection_anchor = Some(target);
                } else if ctrl {
                    // Ctrl-click toggle. Anchor follows the click so a
                    // subsequent shift-click extends from here.
                    if let Some(pos) = self
                        .sftp
                        .selected_rows
                        .iter()
                        .position(|x| x == &target)
                    {
                        self.sftp.selected_rows.remove(pos);
                    } else {
                        self.sftp.selected_rows.push(target.clone());
                    }
                    self.sftp.selection_anchor = Some(target);
                } else if is_dir {
                    // Single click selects the folder (so it can be the
                    // type-ahead focus); a quick double click on the same
                    // folder opens it.
                    let now = std::time::Instant::now();
                    let is_double = self.sftp.last_click.as_ref().is_some_and(|(s, p, t)| {
                        *s == side
                            && p == &path
                            && now.duration_since(*t) < DOUBLE_CLICK_WINDOW
                    });
                    if is_double {
                        self.sftp.last_click = None;
                        self.sftp.selected_rows.clear();
                        self.sftp.selection_anchor = None;
                        return Ok(if self.sftp.pane(side).is_remote {
                            Task::done(Message::Sftp(SftpMessage::SftpNavigateRemote(side, path)))
                        } else {
                            Task::done(Message::Sftp(SftpMessage::SftpNavigateLocal(
                                side,
                                std::path::PathBuf::from(path),
                            )))
                        });
                    }
                    self.sftp.last_click = Some((side, path, now));
                    self.sftp.selected_rows = vec![target.clone()];
                    self.sftp.selection_anchor = Some(target);
                } else {
                    // Double-clicking a zip file enters it as a virtual
                    // directory (browse without extracting). Only when
                    // not already inside an archive: nested zips would
                    // need decompressing the outer entry first, which
                    // defeats the ranged-read model.
                    let now = std::time::Instant::now();
                    let is_double = self.sftp.last_click.as_ref().is_some_and(|(s, p, t)| {
                        *s == side
                            && p == &path
                            && now.duration_since(*t) < DOUBLE_CLICK_WINDOW
                    });
                    if is_double
                        && self.sftp.pane(side).zip.is_none()
                        && matches!(
                            oryxis_archive::names::ArchiveKind::from_name(
                                &crate::dispatch_sftp_archive::base_name(&path)
                            ),
                            Some(oryxis_archive::names::ArchiveKind::Zip)
                        )
                    {
                        self.sftp.last_click = None;
                        self.sftp.selected_rows.clear();
                        self.sftp.selection_anchor = None;
                        return Ok(Task::done(Message::Sftp(SftpMessage::SftpZipOpen(side, path))));
                    }
                    self.sftp.last_click = Some((side, path, now));
                    self.sftp.selected_rows = vec![target.clone()];
                    self.sftp.selection_anchor = Some(target);
                }
            }

            SftpMessage::SftpTypeAheadFire(generation) => {
                // A newer keystroke superseded this fire: skip it.
                if generation != self.sftp.type_ahead_gen {
                    return Ok(Task::none());
                }
                // On the ".." row there's no selected row, so fall back to
                // the focused pane (type-ahead works from the parent cursor).
                let side = self
                    .sftp
                    .selected_rows
                    .last()
                    .map(|(s, _)| *s)
                    .unwrap_or(self.sftp.focused_side);
                let prefix = self.sftp.type_ahead.clone();
                if prefix.is_empty() {
                    return Ok(Task::none());
                }
                // Cycle when the last keystroke repeated a single character
                // (Windows-style) or the user re-typed the whole previous
                // search after a pause: advance past the current selection
                // instead of restarting at the top.
                let cycle =
                    self.sftp.type_ahead_cycle || prefix == self.sftp.type_ahead_committed;

                // Snapshot the displayed entries as (name, full_path) in
                // display order (same hidden + filter rules as the view).
                let (visible, cur_path) = {
                    let pane = self.sftp.pane(side);
                    let filter = pane.filter.to_lowercase();
                    let show_hidden = pane.show_hidden;
                    let cur_path = if pane.is_remote {
                        pane.remote_path.clone()
                    } else {
                        pane.local_path.to_string_lossy().into_owned()
                    };
                    let base_remote = cur_path.trim_end_matches('/').to_string();
                    let raw: Vec<String> = if pane.is_remote {
                        pane.remote_entries.iter().map(|e| e.name.clone()).collect()
                    } else {
                        pane.local_entries.iter().map(|e| e.name.clone()).collect()
                    };
                    let mut visible: Vec<(String, String)> = Vec::new();
                    for n in raw {
                        if !show_hidden && n.starts_with('.') {
                            continue;
                        }
                        if !filter.is_empty() && !n.to_lowercase().contains(&filter) {
                            continue;
                        }
                        let full = if pane.is_remote {
                            if base_remote.is_empty() {
                                format!("/{n}")
                            } else {
                                format!("{base_remote}/{n}")
                            }
                        } else {
                            std::path::Path::new(&cur_path)
                                .join(&n)
                                .to_string_lossy()
                                .into_owned()
                        };
                        visible.push((n, full));
                    }
                    (visible, cur_path)
                };
                let total = visible.len();
                if total == 0 {
                    return Ok(Task::none());
                }
                // Cycling starts just after the current selection; otherwise
                // from the top.
                let start = if cycle {
                    let cur = self.sftp.selected_rows.last().map(|(_, p)| p.clone());
                    cur.and_then(|c| visible.iter().position(|(_, f)| *f == c))
                        .map(|i| i + 1)
                        .unwrap_or(0)
                } else {
                    0
                };
                let Some(idx) = (0..total)
                    .map(|off| (start + off) % total)
                    .find(|&i| visible[i].0.to_lowercase().starts_with(&prefix))
                else {
                    // No match; keep the buffer so the next key extends it.
                    return Ok(Task::none());
                };
                let full = visible[idx].1.clone();
                self.sftp.selected_rows = vec![(side, full.clone())];
                self.sftp.selection_anchor = Some((side, full));
                self.sftp.focused_side = side;
                self.sftp.parent_cursor = false;
                // Scroll the match into view via the pane's per-directory
                // scroll id (must match the one the view builds).
                let side_key = match side {
                    crate::state::SftpPaneSide::Left => "left",
                    crate::state::SftpPaneSide::Right => "right",
                };
                let scroll_id = format!("sftp-list-{side_key}-{cur_path}");
                let ratio = if total > 1 {
                    idx as f32 / (total - 1) as f32
                } else {
                    0.0
                };
                return Ok(iced::widget::operation::snap_to(
                    iced::widget::Id::from(scroll_id),
                    iced::widget::scrollable::RelativeOffset {
                        x: None,
                        y: Some(ratio),
                    },
                ));
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }
}

impl Oryxis {
    /// SFTP type-ahead / list-nav peek for keyboard events, lifted out of
    /// `handle_sftp_selection` when the SFTP handlers became type-safe over
    /// `SftpMessage` (a `TerminalMessage::KeyboardEvent` can no longer be a
    /// match arm there). `dispatch_message` calls this before routing a
    /// keyboard event to the terminal: `Ok(task)` consumes it (type-ahead,
    /// list nav, inline-edit Esc); `Err(ke)` declines and the event falls
    /// through to `handle_terminal`. Preserves the old chain ordering.
    pub(crate) fn sftp_type_ahead(
        &mut self,
        ke: iced::keyboard::Event,
    ) -> Result<Task<Message>, iced::keyboard::Event> {
                // Type-ahead: while a row is selected in the SFTP view,
                // typing letters jumps the selection to the first entry whose
                // name starts with what's been typed. Only plain printable
                // keys are intercepted here; modifiers, named keys, hotkeys,
                // and typing inside text fields all forward to the terminal
                // handler (which owns that logic) via `Err`. Gated on the
                // visible surface (standalone view OR a hybrid tab's Files
                // mode), where the PTY byte routing is disabled.
                if !self.sftp_surface_visible() {
                    return Err(ke);
                }
                // While the right-click row context menu is open it owns the
                // keyboard through the modal keynav router (arrows move its
                // rows, Enter fires, Esc closes). Decline every key here so
                // list nav / type-ahead / Ctrl+A don't steal them; this
                // handler runs before the modal router in the chain.
                if self.sftp.row_menu.is_some() {
                    return Err(ke);
                }
                // Consume the activation-swallow flag on the first keyboard
                // event after an inline-input commit: the trailing Enter from
                // that submit must not activate the still-selected row.
                let swallow = std::mem::take(&mut self.sftp.swallow_next_activate);
                if let iced::keyboard::Event::KeyPressed { key, .. } = &ke {
                    // Escape cancels an in-progress inline rename / new-entry
                    // instead of falling through to the terminal handler.
                    if matches!(key, iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape)) {
                        if self.sftp.rename.take().is_some() {
                            return Ok(Task::none());
                        }
                        if self.sftp.new_entry.take().is_some() {
                            return Ok(Task::none());
                        }
                        // Path-bar editing: Esc reverts to the breadcrumb
                        // discarding the typed text (Enter, via on_submit,
                        // is the only commit path).
                        if self.sftp.left.path_editing.is_some()
                            || self.sftp.right.path_editing.is_some()
                        {
                            self.sftp.left.path_editing = None;
                            self.sftp.right.path_editing = None;
                            return Ok(Task::none());
                        }
                    }
                    if swallow
                        && matches!(key, iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter))
                    {
                        return Ok(Task::none());
                    }
                }
                let editing = self.sftp.rename.is_some()
                    || self.sftp.new_entry.is_some()
                    || self.sftp.overwrite_prompt.is_some()
                    || !self.sftp.delete_confirm.is_empty()
                    || self.sftp.properties.is_some()
                    || self.sftp.picker_open
                    || self.sftp.left.path_editing.is_some()
                    || self.sftp.right.path_editing.is_some();
                // Ctrl+A / Cmd+A: select every visible row in the focused
                // pane (post filter/sort, the same list shift-click range
                // extension walks). Anchored on the first entry so a
                // follow-up shift-click keeps well-defined semantics.
                if !editing
                    && let iced::keyboard::Event::KeyPressed {
                        key: iced::keyboard::Key::Character(s),
                        modifiers,
                        ..
                    } = &ke
                    && (modifiers.control() || modifiers.command())
                    && !modifiers.alt()
                    && s.as_str() == "a"
                {
                    let side = self.sftp.focused_side;
                    let entries = self.visible_entry_paths_in_pane(side);
                    if !entries.is_empty() {
                        self.sftp.selection_anchor = Some((side, entries[0].clone()));
                        self.sftp.selected_rows =
                            entries.into_iter().map(|p| (side, p)).collect();
                        self.sftp.parent_cursor = false;
                        self.sftp.suppress_hover = true;
                    }
                    return Ok(Task::none());
                }
                // Arrow / Enter navigation: move the focused row or open
                // it (folder -> navigate, file -> open). These are Named
                // keys, so they never reach the type-ahead char extraction
                // below; handle them here before that returns `Err` and
                // forwards the keypress to the terminal/PTY. Suppressed
                // while a modal/input owns the keyboard, and when a
                // modifier is held (those belong to hotkeys / the PTY).
                if !editing
                    && let iced::keyboard::Event::KeyPressed {
                        key: iced::keyboard::Key::Named(named),
                        modifiers,
                        ..
                    } = &ke
                    && !modifiers.control()
                    && !modifiers.command()
                    && !modifiers.alt()
                {
                    use iced::keyboard::key::Named;
                    // Any of these takes the keyboard cursor over, so mute the
                    // mouse-hover highlight until the mouse moves again.
                    if matches!(
                        named,
                        Named::ArrowDown
                            | Named::ArrowUp
                            | Named::ArrowLeft
                            | Named::ArrowRight
                            | Named::Enter
                            | Named::Tab
                    ) {
                        self.sftp.suppress_hover = true;
                    }
                    match named {
                        Named::ArrowDown => return Ok(self.sftp_move_focus(true)),
                        Named::ArrowUp => return Ok(self.sftp_move_focus(false)),
                        // Right descends into a folder (or up via ".."); on a
                        // file it does nothing. Enter additionally opens files.
                        Named::ArrowRight => return Ok(self.sftp_activate_focused(false)),
                        Named::Enter => return Ok(self.sftp_activate_focused(true)),
                        // Left goes to the parent directory.
                        Named::ArrowLeft => return Ok(self.sftp_focus_parent()),
                        // Tab switches the focused pane.
                        Named::Tab => return Ok(self.sftp_toggle_pane_focus()),
                        // F2 renames the selected row, the standard shortcut
                        // in Explorer / Finder / FileZilla and the reliable
                        // path now that the slow click is deliberately hard
                        // to trigger by accident. A multi-selection has no
                        // single target, so it stays inert.
                        Named::F2 => {
                            if let [(side, path)] = self.sftp.selected_rows.as_slice() {
                                let (side, path) = (*side, path.clone());
                                return Ok(Task::done(Message::Sftp(
                                    SftpMessage::SftpStartRename(side, path),
                                )));
                            }
                            return Ok(Task::none());
                        }
                        // Delete asks about the SELECTION, which is what the
                        // keyboard cursor is here (`sftp_move_focus` reads
                        // and writes `selected_rows`), so it covers a
                        // multi-selection for free. Through the confirm
                        // dialog, never straight to the delete: the same
                        // rule the sidebar's Delete follows.
                        //
                        // Inert on the `..` row. `parent_cursor` CLEARS the
                        // selection when it is set, so the message would
                        // find nothing to do, but relying on that would
                        // mean a future change to that invariant silently
                        // turns Delete-on-`..` into deleting whatever was
                        // selected before.
                        Named::Delete => {
                            if self.sftp.parent_cursor {
                                return Ok(Task::none());
                            }
                            return Ok(Task::done(Message::Sftp(
                                SftpMessage::SftpAskDeleteSelection,
                            )));
                        }
                        // The dedicated Menu key (and Shift+F10, its
                        // keyboard equivalent on boards without one) opens
                        // the row context menu on the focused row, the
                        // keyboard peer of a right-click. Plain F10 keeps
                        // falling through to the PTY.
                        Named::ContextMenu => {
                            return Ok(self.sftp_open_focus_row_menu());
                        }
                        Named::F10 if modifiers.shift() => {
                            return Ok(self.sftp_open_focus_row_menu());
                        }
                        _ => {}
                    }
                }
                let ch = if editing {
                    None
                } else if let iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Character(s),
                    modifiers,
                    ..
                } = &ke
                {
                    if modifiers.control() || modifiers.command() || modifiers.alt() {
                        None
                    } else {
                        s.chars().next().filter(|c| !c.is_control())
                    }
                } else {
                    None
                };
                let Some(ch) = ch else {
                    return Err(ke);
                };
                // Type-ahead works from any keyboard cursor: a selected row
                // (the selection's pane is the focus) or the ".." parent row
                // (which clears selected_rows but sets parent_cursor on the
                // focused pane).
                if self.sftp.selected_rows.last().is_none() && !self.sftp.parent_cursor {
                    return Ok(Task::none());
                }
                let now = std::time::Instant::now();
                let elapsed = self
                    .sftp
                    .type_ahead_at
                    .map(|t| now.duration_since(t) > TYPE_AHEAD_RESET)
                    .unwrap_or(true);
                if elapsed {
                    // A pause completes the previous sequence: remember it so
                    // re-typing the same search cycles to the next match.
                    self.sftp.type_ahead_committed = std::mem::take(&mut self.sftp.type_ahead);
                }
                let lc: String = ch.to_lowercase().collect();
                // Windows-Explorer single-character cycling: while a search is
                // live, pressing the SAME single character again advances to the
                // next match instead of narrowing the buffer to "aa". A
                // different character narrows; a pause (elapsed) starts fresh.
                let repeat = !elapsed && self.sftp.type_ahead == lc;
                if !repeat {
                    self.sftp.type_ahead.push_str(&lc);
                }
                self.sftp.type_ahead_cycle = repeat;
                self.sftp.type_ahead_at = Some(now);
                // Type-ahead moves the keyboard cursor too; mute mouse hover.
                self.sftp.suppress_hover = true;
                self.sftp.type_ahead_gen = self.sftp.type_ahead_gen.wrapping_add(1);
                let generation = self.sftp.type_ahead_gen;
                if repeat {
                    // Cycle immediately so each repeat of the key advances one
                    // step (debouncing would collapse a fast "eee" into one jump).
                    Ok(Task::done(Message::Sftp(SftpMessage::SftpTypeAheadFire(
                        generation,
                    ))))
                } else {
                    // Debounce narrowing so fast typing ("cla") resolves once
                    // with the full buffer instead of jumping on every key.
                    Ok(Task::perform(
                        async move {
                            tokio::time::sleep(TYPE_AHEAD_DEBOUNCE).await;
                        },
                        move |_| Message::Sftp(SftpMessage::SftpTypeAheadFire(generation)),
                    ))
                }
    }
}

/// Whether a deferred slow-click rename should still open the inline editor
/// when its timer fires. Two conditions, both about the click that armed it
/// still being the user's last word:
///
/// - it must be the NEWEST click (`armed_gen == click_gen`). Every click,
///   right-click and navigation bumps the generation, so the second click of
///   a sluggish double-click, a click on another row, or descending into a
///   folder all retire the pending rename.
/// - its row must still be the lone selection, which covers the paths that
///   move the selection without a click (keyboard cursor, type-ahead, a
///   listing that reloaded).
fn slow_rename_still_valid(
    armed_gen: u64,
    click_gen: u64,
    selection: &[(SftpPaneSide, String)],
    target: (SftpPaneSide, &str),
) -> bool {
    armed_gen == click_gen
        && matches!(selection, [(side, path)] if *side == target.0 && path == target.1)
}

#[cfg(test)]
mod tests {
    use super::slow_rename_still_valid;
    use crate::state::SftpPaneSide::{Left, Right};

    fn sel(rows: &[(crate::state::SftpPaneSide, &str)]) -> Vec<(crate::state::SftpPaneSide, String)> {
        rows.iter().map(|(s, p)| (*s, p.to_string())).collect()
    }

    #[test]
    fn fires_when_nothing_happened_since_the_arming_click() {
        assert!(slow_rename_still_valid(
            7,
            7,
            &sel(&[(Right, "/srv/a.conf")]),
            (Right, "/srv/a.conf"),
        ));
    }

    #[test]
    fn a_later_click_cancels() {
        // The second click of a slow double-click (or any other click) bumps
        // the generation: the folder opens, no editor pops open behind it.
        assert!(!slow_rename_still_valid(
            7,
            8,
            &sel(&[(Right, "/srv/a.conf")]),
            (Right, "/srv/a.conf"),
        ));
    }

    #[test]
    fn selection_must_still_be_exactly_that_row() {
        // Moved on (keyboard cursor, type-ahead, reloaded listing).
        assert!(!slow_rename_still_valid(
            7,
            7,
            &sel(&[(Right, "/srv/b.conf")]),
            (Right, "/srv/a.conf"),
        ));
        // Grew into a multi-selection: no single rename target.
        assert!(!slow_rename_still_valid(
            7,
            7,
            &sel(&[(Right, "/srv/a.conf"), (Right, "/srv/b.conf")]),
            (Right, "/srv/a.conf"),
        ));
        // Emptied.
        assert!(!slow_rename_still_valid(7, 7, &[], (Right, "/srv/a.conf")));
        // Same path, other pane (both panes can show one host).
        assert!(!slow_rename_still_valid(
            7,
            7,
            &sel(&[(Left, "/srv/a.conf")]),
            (Right, "/srv/a.conf"),
        ));
    }
}
