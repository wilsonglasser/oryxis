#!/bin/bash
# Offline-compliance rehearsal for Linux (iptables + LOG). Default-drop
# OUTPUT except: loopback, established, UDP broadcast :9 (WoL), and
# optionally raw.githubusercontent.com + one AI provider host.
set -euo pipefail

FONT_IPS="${FONT_IPS:-$(dig +short raw.githubusercontent.com | grep -E '^[0-9.]+$' | tr '\n' ' ')}"
AI_IPS="${AI_IPS:-}"
TAG=ORYXIS_REHEARSAL

case "${1:-}" in
start)
  sudo iptables -N $TAG 2>/dev/null || sudo iptables -F $TAG
  sudo iptables -A OUTPUT -j $TAG
  sudo iptables -A $TAG -o lo -j RETURN
  sudo iptables -A $TAG -m state --state ESTABLISHED -j RETURN
  for ip in $FONT_IPS; do
    sudo iptables -A $TAG -p tcp -d "$ip" --dport 443 -j RETURN
  done
  for ip in $AI_IPS; do
    sudo iptables -A $TAG -p tcp -d "$ip" --dport 443 -j RETURN
  done
  sudo iptables -A $TAG -p udp -d 255.255.255.255 --dport 9 -j RETURN
  sudo iptables -A $TAG -m limit --limit 10/min -j LOG --log-prefix "$TAG DROP: "
  sudo iptables -A $TAG -j REJECT
  echo "default-drop OUTPUT armed ($TAG). Exercise the app, then: $0 report"
  ;;
report)
  sudo dmesg | grep "$TAG DROP" | tail -100 || journalctl -k | grep "$TAG DROP" | tail -100
  ;;
stop)
  sudo iptables -D OUTPUT -j $TAG 2>/dev/null || true
  sudo iptables -F $TAG 2>/dev/null || true
  sudo iptables -X $TAG 2>/dev/null || true
  echo "rehearsal rules removed"
  ;;
*)
  echo "usage: $0 start|report|stop   (env: FONT_IPS, AI_IPS space-separated)" >&2
  exit 2
  ;;
esac
