viewport: 1200x750
mode: Zen
-----
# "Copy Screen" copies the viewport as drawn, scroll position included and
# scrollback left out. It belongs to the terminal's own context menu, which
# exists only under the Menu right-click scheme: under the other two that
# menu never opens, so the tab chip's menu carries the entry instead. What
# this test holds is the exclusivity, in both directions: whichever menu the
# scheme leaves reachable is the one, and the only one, that offers it.
settle 250
click "Skip"
settle 250
click "Continue without password"
settle 250
# A live PTY never lets the emulator quiesce, so cap the per-instruction
# wait instead of burning the full timeout on every line.
timeout 500
type ctrl+shift+l
settle 250
expect "bash (default)"
# Default scheme is Paste, so the chip's menu is the only door there is.
# Coordinates because the chip's own label is the string being asserted.
click right (150.00, 20.00)
settle 250
expect "Copy Screen"
type escape
settle 250
# Flip the scheme through the palette row that owns it. The screenshot is
# not decoration: the emulator only draws on one, and the pick_list is
# clicked by coordinate because a dropdown's options are not text
# selectable.
type ctrl+shift+p
settle 400
type "right-click"
settle 400
expect "Terminal Settings: Right-click"
type enter
settle 500
screenshot copy-screen-scheme
click (1058.00, 79.00)
settle 400
click (1022.00, 121.00)
settle 400
# Back on the shell: the chip's menu still opens (Close Tab is there) and
# no longer carries an entry the terminal's own menu now answers for.
click (150.00, 20.00)
settle 400
timeout 500
click right (150.00, 20.00)
settle 250
absent "Copy Screen"
expect "Close Tab"
type escape
settle 250
click right (600.00, 400.00)
settle 250
expect "Copy Screen"
expect "Copy All"
