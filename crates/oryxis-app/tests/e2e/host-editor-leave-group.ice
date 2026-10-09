viewport: 1200x750
mode: Zen
-----
# A host leaves its folder from its own editor. The Parent Group field
# is a text input + the shared group picker, which offers "Top level
# (no group)" for this target; a clear (×) inside the field is the
# one-click door. The native combo_box it replaced blanked itself on
# focus without telling the form, so the drawer's closing flush wrote
# the old folder back and the host could never leave. The folder
# card's own count is the evidence either way: a text_input VALUE is
# invisible to `expect`, a card count is real text.
#
# Picking "Prod" in the picker goes through its search box: the folder
# card on the dashboard carries the same text, so a text click would
# be ambiguous.
click "Skip"
click "Continue without password"
settle 250
click "Type IP or Hostname"
type "web01"
click "Continue"
settle 250
click "My Server"
type "web01"
click "Production, Staging..."
type "Prod"
settle 250
click "Save"
settle 250
expect "1 host"
# Door one: the picker's "Top level" row.
click "Prod"
settle 250
click right "web01"
settle 250
click "Edit"
settle 250
expect "Edit Host"
click (1150.00, 251.00)
settle 250
expect "Top level (no group)"
click "Top level (no group)"
settle 250
click (1169.00, 72.00)
settle 400
absent "web01"
click (31.00, 110.00)
settle 250
expect "0 hosts"
expect "web01"
# Back into the folder through the same picker.
click right "web01"
settle 250
click "Edit"
settle 250
expect "Edit Host"
click (1150.00, 251.00)
settle 250
click "Search groups…"
type "Prod"
settle 200
type down
type enter
settle 250
click (1169.00, 72.00)
settle 400
expect "1 host"
# Door two: the clear (×) inside the field.
click "Prod"
settle 250
click right "web01"
settle 250
click "Edit"
settle 250
expect "Edit Host"
click (1115.00, 251.00)
settle 250
click (1169.00, 72.00)
settle 400
absent "web01"
click (31.00, 110.00)
settle 250
expect "0 hosts"
expect "web01"
