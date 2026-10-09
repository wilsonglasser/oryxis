viewport: 1200x750
mode: Zen
-----
# Settings > Shortcuts: the menu verbs that ship with NO factory chord
# ("Move tab to new window" and its ten siblings, `ships_unbound`) are
# rows whose only chip reads "(unbound)". This pins the row's whole
# life for the one the owner asked for:
#
#   * it lists, unbound, beside the factory rows;
#   * a click on its chip starts a capture, the chord lands and the row
#     grows a "Reset" (a row with no factory entry is overridden by ANY
#     chord);
#   * after "Reset" the chord is gone: on a shell tab it moves nothing,
#     so the strip keeps the Settings tab;
#   * bound again, the chord sends the tab menu's own message: the shell
#     tab leaves for a window of its own, which the emulator adopts and
#     draws with that tab alone (tab-move-windows.ice's shape).
#
# `expect` is exact, which is what makes "Reset" usable here ("Reset all
# to defaults" and "Reset font zoom" never match it); `absent` is not,
# so the "no Reset after Reset" half is asserted through the chord not
# firing instead. The chip is clicked by position: eleven chips carry
# the same "(unbound)" text, and this is the second one at this
# viewport with the list at the top.
expect "Welcome to Oryxis"
click "Skip"
click "Continue without password"
settle 250
# Toolbar gear (icon only, no text selector).
click (1175, 64)
settle
click "Shortcuts"
settle
expect "Duplicate in New Window"
expect "Move tab to new window"
expect "Pin / unpin tab"
expect "Copy host address"
expect "Close Other Tabs"
expect "Close All Tabs"
expect "Copy All"
expect "Copy Screen"
expect "Clear Scrollback"
expect "Lock Vault"
expect "(unbound)"
# Bind Ctrl+Alt+M to "Move tab to new window", then take it back.
click (251, 682)
settle 250
expect "Press a key or mouse button…"
type ctrl+alt+m
settle 250
expect "Reset"
click "Reset"
settle 250
# Unbound again: on a shell tab the chord moves nothing.
type ctrl+shift+l
settle 900
expect "bash (default)"
type ctrl+alt+m
settle 600
expect "Settings"
expect "bash (default)"
# Bound again: the chord is the tab menu's "Move to New Window".
click "Settings"
settle 250
click (251, 682)
settle 250
expect "Press a key or mouse button…"
type ctrl+alt+m
settle 250
expect "Reset"
click "bash (default)"
settle 250
type ctrl+alt+m
settle 900
expect "bash (default)"
absent "Settings"
