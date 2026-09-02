viewport: 1200x750
mode: Zen
-----
# Issue #208 / #209: a pane whose session ends says so and offers the
# two answers a tab-wide reconnect cannot give one pane. Driven on a
# LOCAL shell because that is the half nothing used to notice at all:
# a pty's read side cannot reach EOF while a writer is open, so the
# exit is reported by the child-exit signal, not by the byte stream.
# A lone tab on purpose: the card must not be gated on a split, and
# the origin that opened the shell must not decide whether it appears.
# `timeout 500` once the PTY is live, like terminal-search.ice: the
# zen emulator never quiesces with a running shell.
expect "Welcome to Oryxis"
click "Skip"
click "Continue without password"
expect "Create host"
click (91, 20)
expect "Local Shell"
timeout 500
click "Local Shell"
settle 400
# Nothing is over the grid while the shell is live.
absent "Session ended"
type "exit"
type enter
settle 800
expect "Session ended"
expect "Restart"
expect "Close pane"
screenshot pane-end-card
# Restart re-spawns into the SAME pane, so the card goes and the shell
# is live again rather than the tab being rebuilt around it.
click "Restart"
settle 800
absent "Session ended"
# And the second exit raises it again: the generation guard discards
# the dead PTY's signal instead of letting it mark the live one.
type "exit"
type enter
settle 800
expect "Session ended"
