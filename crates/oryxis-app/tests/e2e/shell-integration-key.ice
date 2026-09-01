viewport: 1400x2600
mode: Zen
-----
# The shell-integration key (Settings > Terminal). Command history's
# in-band path only accepts a reported command line that carries this
# vault's key, so the two controls that put the key on a host are the
# whole usable surface of that gate: if Copy stops working, capture
# stops working and nothing else says so.
#
# Tall viewport on purpose: the Terminal section is long and the row
# sits under the command-history toggle it belongs to.
click "Skip"
click "Continue without password"
settle
# Through the burger rather than the toolbar gear, so the flow does not
# depend on the gear's pixel position at this width (same reasoning as
# settings-tab.ice).
click (19, 20)
settle
click "Settings"
settle
click "Terminal Settings"
settle
# The row is nested under the capture toggle and only exists while
# capture is on, which is the default.
expect "Capture command history"
expect "Copy shell integration snippet"
expect "Rotate key"
# Copy: the click itself asserts the control is reachable, the toast
# asserts the handler ran. The snippet's own content is pinned by unit
# tests (it carries the key, keeps no placeholder, is LF-only), which is
# where a randomly generated key can actually be asserted.
clipboard "not the snippet"
click "Copy shell integration snippet"
expect "Snippet copied. Paste it into your shell config on the host."
settle
# Rotate: mints a new key and installs it immediately, so every copy of
# the old snippet stops reporting from that moment.
click "Rotate key"
expect "New key in use. Hosts still running the old snippet stop reporting until you copy the new one."
settle
# Turning capture off takes the row with it: a key that nothing consults
# is a control that cannot matter.
#
# The toggler is hit by position, not by its label: clicking the text of
# a `nav_toggle_row` does not flip it (the toggler is the control, the
# label is a label). The coordinate is deterministic because this file
# declares its own viewport, and if a row is ever inserted above this
# one the failure is the explicit `absent` below rather than a silent
# pass. It has caught that six times already: 1784 -> 1830 when the
# #117 password-autofill row joined the section above, 1830 -> 1858 when
# the #109 font-pack hint line landed under the font picker, 1858 ->
# 1927 when the background-opacity row joined Appearance, 1927 -> 1991
# when the background-image row joined it, 1991 -> 2091 when the
# highlight-rules block landed after Appearance, 2091 -> 2276 when the
# font-weight and text-thickness rows joined the font block (which also
# pushed the copy row past the old 2200 viewport), 2276 -> 2227 when the
# #143 polish shortened the section above, and 2227 -> 2334 when the two
# link rows (confirm + callback tunnel) joined the behaviour card.
#
# That last one sat broken for a while because this
# file runs late in the alphabet and the batch aborts on the FIRST
# failure, so three earlier stale tests hid it. Read the row back
# with `find "Capture command history"` and use label_y + 8 rather
# than guessing a delta.
click (1340, 2334)
settle
absent "Copy shell integration snippet"
absent "Rotate key"
