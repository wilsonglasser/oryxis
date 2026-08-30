#!/bin/bash
# Offline-compliance rehearsal for macOS (pf). Blocks all IPv4/IPv6
# egress except: loopback, established state, ICMP/UDP to :9 (WoL),
# and (optionally) raw.githubusercontent.com + an AI provider host you
# pass as env vars. Logs drops to /var/log/pf-oryxis.log via pflog0.
set -euo pipefail

ANCHOR=oryxis_rehearsal
LOGIF=pflog0
ALLOW_FONTS="${ALLOW_FONTS:-1}"
AI_HOST="${AI_HOST:-}"   # e.g. api.anthropic.com — only if you configured AI

cmd="${1:-}"

case "$cmd" in
start)
  FONT_RULE=""
  if [ "$ALLOW_FONTS" = "1" ]; then
    FONT_RULE="pass out proto tcp to raw.githubusercontent.com port 443"
  fi
  AI_RULE=""
  if [ -n "$AI_HOST" ]; then
    AI_RULE="pass out proto tcp to $AI_HOST port 443"
  fi
  sudo pfctl -a "$ANCHOR" -f - <<EOF
set skip on lo0
block out log all
pass out proto tcp to 127.0.0.0/8
$FONT_RULE
$AI_RULE
pass out proto udp from any to 255.255.255.255 port 9
pass in all
EOF
  sudo pfctl -E >/dev/null 2>&1 || true
  echo "anchor $ANCHOR loaded; drops log to $LOGIF. Exercise the app now, then: $0 report"
  ;;
report)
  if sudo ifconfig "$LOGIF" >/dev/null 2>&1; then
    sudo tcpdump -n -e -i "$LOGIF" 2>/dev/null | head -100
  else
    echo "(pflog interface not present; drops were still enforced)"
  fi
  ;;
stop)
  sudo pfctl -a "$ANCHOR" -F all 2>/dev/null || true
  echo "anchor $ANCHOR flushed"
  ;;
*)
  echo "usage: $0 start|report|stop   (env: ALLOW_FONTS=0, AI_HOST=host)" >&2
  exit 2
  ;;
esac
