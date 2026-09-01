viewport: 1400x2200
mode: Zen
-----
# J4: the per-host East Asian ambiguous-width pick lives in the host
# editor's Terminal card, right under Encoding, because Encoding is what
# `Auto` reads.
#
# What this can assert is the SURFACE. The grid half (a `│` flagged
# WIDE_CHAR and the cursor advancing two columns) is unit-tested in the
# fork and in `oryxis-terminal`, because the harness cannot see inside
# the terminal canvas, and pick_list VALUES are invisible to text
# selectors the same way text_input values are, so the row LABEL is the
# evidence that the row is wired and rendered.
#
# The viewport is tall because the Terminal card sits at the bottom of
# the editor's section list and a click needs it actually on screen (the
# emulator's wheel does not reach side-panel scrollables).
click "Skip"
click "Continue without password"
expect "Create host"
click "Continue"
settle 400
expect "New Host"
# The card is collapsed on a fresh form: the row must not be reachable
# before it opens, or this test would pass on a stray match elsewhere.
absent "Ambiguous width"
click "Terminal Settings"
settle 400
expect "Encoding"
expect "Ambiguous width"
screenshot ambiguous-width-row
