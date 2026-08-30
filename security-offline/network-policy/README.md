# Network policy rehearsal scripts

The dynamic phase of this audit could not run a disposable VM with packet
capture, so offline compliance is proven statically (code-level egress
inventory + removal greps in `12-VALIDATION.md`) plus by these operator
rehearsals. Run the one for your platform after installing the
transformed build; any packet, DNS query, or connection attempt outside
the allowlist below is a failed gate.

## Expected allowlist (post-transformation)

| Destination | When | Protocol |
|---|---|---|
| your SSH/SFTP/mosh/telnet hosts | you connect | TCP (UDP for mosh) |
| your AI provider (ONLY if configured) | you invoke the assistant | HTTPS |
| raw.githubusercontent.com | you pick a CJK language / Nerd Font pack face, or boot heals the configured font | HTTPS |
| 255.255.255.255:9 | you click Wake-on-LAN | UDP broadcast |
| loopback (127.0.0.1, ::1) | ssh-agent socket use is a file socket/pipe, not TCP; dev harness only in dev builds | — |

Everything else — DNS included during idle — must be silent. In
particular: no `api.github.com`, no `dl.oryxis.app` / `dl-cn.oryxis.app`,
no signaling endpoints, no cloud-provider APIs, nothing at boot or idle.

## macOS (pf)

```sh
sudo ./rehearsal-macos.sh start   # block all egress except the allowlist holes
# … exercise: boot, idle 5 min, connect to a host, open settings, SFTP browse …
sudo ./rehearsal-macos.sh report  # pf log since start
sudo ./rehearsal-macos.sh stop
```

## Linux (nftables/iptables)

```sh
sudo ./rehearsal-linux.sh start   # default-drop OUTPUT with the same holes
# … same exercise …
sudo ./rehearsal-linux.sh report
sudo ./rehearsal-linux.sh stop
```

Both scripts log every dropped egress packet with the owning UID; match
it to the oryxis process (`pgrep -f 'target/(debug|release)/oryxis'`).

## Hard mode (fully air-gapped smoke test)

1. `cargo build --release` on a networked machine, then copy the tree
   and `~/.cargo` cache over.
2. On the offline machine: `cargo test --workspace --offline` must pass.
3. Run the binary with networking unavailable: core flows (vault unlock,
   host list, local terminal, SFTP to LAN hosts if the "network" is a
   stub) must work; the font picker must fall back to bundled/system
   fonts with a local error, never hang.
