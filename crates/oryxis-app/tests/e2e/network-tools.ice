viewport: 1240x1400
mode: Zen
-----
# The optional network tools panel: the toggle-hidden gate, the panel
# opening as its own tab, one real run, and the tab going away with the
# feature. The probes that need a network (DNS, WHOIS, the blocklists)
# stay out of CI; the port test below is a loopback connect, so it
# answers the same offline.
click "Skip"
click "Continue without password"
settle
# Off by default means no door at all, not a disabled row. Checked with
# the burger open over the vault surface, which is the only screen where
# nothing else spells the words.
click (19, 20)
settle
absent "Network tools"
type escape
settle
# Settings > Features & Plugins carries the toggle, next to the other
# optional features: that list is what a user reads to find out what the
# app can be made to do, and a feature reachable only from Advanced does
# not exist to them (it lived there first, and the owner could not find
# it). The toggler is hit by position (clicking a toggle row's label
# does not flip it); this file declares its own viewport so the
# coordinate is deterministic, and the `expect` above it is the loud
# failure if a row is ever inserted between it and the top of the card.
click (19, 20)
settle
click "Settings"
settle
click "Features & Plugins"
settle
expect "Network tools panel"
click (1183, 395)
settle
# Now the menu has the entry, and it opens as a tab of its own next to
# Settings rather than taking over the vault surface.
click (19, 20)
settle
expect "Network tools"
click "Network tools"
settle
expect "Enter a target and run the tool."
# Switch to the port test: pick_list options are not reachable by text
# selectors, so the picker and its row are clicked by position.
click (174, 184)
settle
click (174, 348)
settle
expect "Ports"
# A loopback connect to a port nothing is listening on. Refused and
# filtered are different findings and either one lands in the same card,
# which is what this asserts.
click (305, 257)
type "127.0.0.1"
click (679, 257)
type "1"
click "Run"
settle
expect "No port answered."
# Switching the feature off closes the panel's tab with it: a chip that
# reopens a surface the user can no longer reach would be the
# optional-features rule broken in the one state nobody tests.
click "Settings"
settle
click (1183, 395)
settle
click (57, 20)
settle
absent "Network tools"
