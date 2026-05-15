#!/usr/bin/env bash
# Plugin-signing key helper.
#
# The *development* signing key isn't a separate file: it's derived
# from a constant seed committed in
# `oryxis_plugin_protocol::DEV_PLUGIN_SIGNING_SEED`, the app's debug
# build trusts whatever pubkey that seed produces. So there's nothing
# to keygen for dev. To sign a binary with the dev key:
#
#   cargo run --release -p oryxis-plugin-signer -- sign <path> --dev
#
# This script generates a fresh *production* keypair (one-shot, run
# it once when bootstrapping the prod signing identity). Bake the
# public half into `PROD_PUBKEY` in
# crates/oryxis-app/src/plugins/verify.rs, and store the private half
# in the `ORYXIS_SIGNING_KEY` CI secret. Keep the private half OUT of
# source control.

set -euo pipefail
cd "$(dirname "$0")/.."

echo "Generating a fresh production plugin-signing keypair..."
echo
cargo run --quiet --release -p oryxis-plugin-signer -- keygen
