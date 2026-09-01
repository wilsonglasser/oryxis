viewport: 1400x2200
mode: Zen
-----
# Terminal font weight (#155): the picker under the font family, and
# the honesty line that fires when the picked family has no face at the
# picked weight.
#
# The bundled default (SauceCodePro Nerd Font) ships Regular and Medium
# and nothing heavier, so SemiBold is a request it cannot serve
# exactly: the hint must appear. That single assertion pins the whole
# availability path (BUNDLED_MONO_WEIGHTS -> the font scan ->
# terminal_font_serves_weight -> the view), which no unit test can
# reach, plus the i18n key behind the line.
#
# Nothing here downloads: a pack family would, so the flow never leaves
# the bundled font.
#
# The assertion reads the machine as well as the code: a runner with
# its own "SauceCodePro Nerd Font" installed, carrying a face at 600 or
# heavier, would legitimately have nothing to warn about and this line
# would fail. CI runners ship no such font.
click "Skip"
click "Continue without password"
settle
click (19, 20)
settle
click "Settings"
settle
click "Terminal Settings"
settle
expect "Terminal Font Weight"
# The picker sits ~45 px under its label (label 17 high + 8 gap + the
# 40 px row), which puts its centre at label_y + 45. Read the label's
# own bounds with `find "Terminal Font Weight"` and recompute BOTH
# clicks below when the cards above it grow: the row drifted 48 px once
# already, then 1202 -> 1310 when the two link rows (confirm + callback
# tunnel, each with a description line) joined the behaviour card above
# it. A miss lands on the card instead of the picker, leaves the
# dropdown shut, and fails at the assertion rather than at the click
# that actually went wrong.
click (300, 1310)
settle 300
# Dropdown options, top to bottom: Regular / Medium / SemiBold / Bold.
# It opens UPWARDS over the label, so SemiBold lands above the picker
# (~82 px up from the picker's centre). They live in an overlay no text
# selector can see, hence the coordinate.
click (300, 1228)
settle 300
expect "This font has no face at the selected weight, so the terminal uses the closest one it has."
# The stroke-widening row lives in the same block, right under the
# weight. Its effect is canvas-only (a second stamp per glyph, which no
# text selector can see), so this pins the row and its i18n key; the
# pixels are measured by hand against the 41-stem sample in the commit.
expect "Text Thickness"
screenshot font-weight-semibold
