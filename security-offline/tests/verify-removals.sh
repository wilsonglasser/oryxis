#!/bin/bash
# Offline-edition removal verification. Run from the repository root.
# Exits non-zero if any removed capability still has live code.
set -u
cd "$(dirname "$0")/../.."

fail=0
check() { # <pattern> <description>
  local hits
  hits=$(rg -n "$1" crates/ --type rust -g '!crates/oryxis-app/src/i18n/*' 2>/dev/null | wc -l | tr -d ' ')
  if [ "$hits" != "0" ]; then
    echo "FAIL: $2 ($hits hits for '$1')"; fail=1
  else
    echo "ok:   $2"
  fi
}

check 'net_mirror::'               "vendor mirror routing"
check 'CloudMessage'               "cloud message plumbing"
check 'View::Cloud'                "cloud view"
check 'oryxis_sync|oryxis-sync'    "sync crate references"
check 'oryxis_relay|oryxis-relay'  "relay crate references"
# `plugins/cache.rs` keeps the historical `oryxis-cloud-{id}-plugin` cache
# naming convention so existing on-disk caches keep resolving; it is a
# string format, not a crate reference.
hits=$(rg -n 'oryxis-cloud' crates/ --type rust -g '!crates/oryxis-app/src/i18n/*' -g '!crates/oryxis-app/src/plugins/cache.rs' 2>/dev/null | wc -l | tr -d ' ')
if [ "$hits" != "0" ]; then echo "FAIL: cloud crate references ($hits hits)"; fail=1; else echo "ok:   cloud crate references"; fi
check 'dl\.oryxis\.app'            "vendor mirror endpoint"
check 'api\.github\.com'           "release lookup endpoint"
check 'derive_sync_secret'         "vault sync secret"
check 'SyncPeerRow|record_tombstone' "vault sync API"
check 'dispatch_update|UpdateMessage' "auto-updater"
check 'PluginProvider|plugins::download' "remote plugin fetch"

# crates that must not exist
for d in oryxis-cloud oryxis-cloud-aws oryxis-cloud-azure oryxis-cloud-gcp \
         oryxis-cloud-k8s oryxis-sync oryxis-relay; do
  if [ -d "crates/$d" ]; then echo "FAIL: crates/$d still present"; fail=1
  else echo "ok:   crates/$d absent"; fi
done
[ -d signaling-worker ] && { echo "FAIL: signaling-worker present"; fail=1; } || echo "ok:   signaling-worker absent"

# workspace membership
if grep -q 'oryxis-sync\|oryxis-relay\|oryxis-cloud' Cargo.toml; then
  echo "FAIL: removed crates still referenced in Cargo.toml"; fail=1
else echo "ok:   workspace manifest clean"; fi

exit $fail
