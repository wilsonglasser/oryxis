viewport: 1200x750
mode: Zen
-----
# Issue #186, second half: the reopen was reachable by hotkey, from a
# chip's own context menu and from the command palette, and the reporter
# answered that people look for it with the mouse, on the bar itself.
# So the strip's empty area answers a right-click with the strip's own
# menu, the way every browser does.
#
# What the menu holds is the assertion that matters: New Tab always,
# the reopen only once something has been closed. An entry that is
# always there and usually does nothing reads as broken the first time
# it is tried, and the same pixels are the window-drag handle, which is
# why nothing destructive is offered here at all.
settle 250
click "Skip"
settle 250
click "Continue without password"
settle 250
# Empty strip, right of the `+`. Coordinates because empty space has no
# text to aim a selector at, which is the whole point of the surface.
click right (400.00, 18.00)
settle 250
expect "New Tab"
absent "Reopen Closed Tab"
type escape
settle 250
absent "New Tab"
# A live PTY never lets the emulator quiesce, so cap the per-instruction
# wait instead of burning the full timeout on every line.
timeout 500
type ctrl+shift+l
settle 250
expect "bash (default)"
# Closed through the chip's menu, one of the user close paths that feed
# the stack. Coordinates again: the chip's own label is the string the
# assertions below are about.
click right (150.00, 20.00)
settle 250
click "Close Tab"
settle 250
absent "bash (default)"
# Now the strip has something to offer back, and it is the mouse path
# the issue asked for.
click right (400.00, 18.00)
settle 250
expect "Reopen Closed Tab"
click "Reopen Closed Tab"
settle 250
expect "bash (default)"
