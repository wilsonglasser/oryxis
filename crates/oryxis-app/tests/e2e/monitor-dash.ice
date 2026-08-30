viewport: 1240x3400
mode: Zen
-----
# The multi-host monitor dashboard (#95): the optional-features gate
# and the empty state. The feature master toggle is OFF by default, so
# no trace of the surface may exist until it is turned on; flipping it
# in Settings > Features & Plugins makes the Monitoring pill appear,
# and the view without opted-in hosts explains how to add some. The
# dial/probe machinery needs a reachable host and stays out of CI (it
# was exercised live: dead-endpoint dial -> Failed card -> detail
# panel Retry, and real vitals against a local sshd).
click "Skip"
click "Continue without password"
settle
absent "Monitoring"
# Through the burger so the flow doesn't depend on the gear's pixel
# position (same reasoning as settings-tab.ice).
click (19, 20)
settle
click "Settings"
settle
click "Features & Plugins"
settle
expect "Host monitoring"
# The toggler is hit by position (clicking a toggle row's label does
# not flip it); this file declares its own viewport so the coordinate
# is deterministic, and the `expect "Monitoring"` below is the loud
# failure if a row is ever inserted above this one. 298 -> 260 when the
# Sync feature row above was removed with the offline transformation.
click (1181, 260)
settle
# Back to the vault area: the pill strip now carries the entry.
click (57, 20)
settle
expect "Monitoring"
click "Monitoring"
settle
expect "No hosts are opted in to monitoring."
expect "Turn on Monitoring in a host's editor, or monitor all hosts from Settings."
