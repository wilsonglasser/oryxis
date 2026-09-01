viewport: 1200x750
mode: Zen
-----
# B1 phase 4: enable the agent from Settings > Features & Plugins,
# then walk into the Settings > SSH Agent sidebar section (the config
# rows moved there in 8228a694) and flip the new opt-in "Accept keys
# from other apps" row; toggling it on restarts the runtime and the
# section stays up (a bind error would surface inline). The OpenSSH
# pipe alias row is Windows-only and never renders on this platform.
# The toggle coordinates are stable under the fixed viewport (togglers
# sit outside the text selector). The "SSH Agent" click resolves to
# the sidebar entry (first match in tree order; the Features row label
# comes later).
expect "Welcome to Oryxis"
click "Skip"
click "Continue without password"
expect "Create host"
click (1175, 64)
expect "Features & Plugins"
click "Features & Plugins"
expect "SSH Agent"
# The SSH Agent toggle sits last in the Features list, so every new
# feature row above it pushes this y down: 305 -> 350 with Host
# monitoring (#83), 350 -> 395 with tmux sessions, 395 -> 440 with the
# network tools panel. Read the row's own
# bounds with `texts` when it drifts again; the label sits ~16 px above
# the toggle's centre.
click (1141, 440)
# The agent runtime is a LISTENER, so from here on the boot task never
# reports done and every instruction waits out the per-instruction
# timeout instead. Same reason terminal-primary-selection.ice drops it
# once a PTY is live; declared here rather than in the header so the
# steps above keep the patient default. Measured: 63s -> 18s.
timeout 5000
settle 300
click "SSH Agent"
expect "Confirm each use"
expect "Accept keys from other apps"
expect "Agent socket"
click (1141, 145)
settle 300
expect "Accept keys from other apps"
screenshot agent-allow-add-on
