#!/usr/bin/env bash
# build-e2e-app.sh — Build the desktop app binary for the C003 built-app E2E harness.
#
# Usage:
#   e2e/build-e2e-app.sh build   Install the test-only capability and build the
#                                app binary with the `desktop-e2e` feature.
#   e2e/build-e2e-app.sh clean   Remove the generated test-only capability so
#                                the tree is production-clean again.
#
# The generated src-tauri/capabilities/e2e.json is gitignored: the Tauri build
# script resolves every capabilities/*.json at compile time, and
# `wdio-webdriver:default` only exists when the desktop-e2e feature compiles
# the test-only embedded WebDriver plugin. Ordinary production builds
# (`cargo build`, `npm run tauri build`) never see this file and never
# contain automation capabilities.
set -euo pipefail
app_root="$(cd "$(dirname "$0")/.." && pwd)"
generated="$app_root/src-tauri/capabilities/e2e.json"
template="$app_root/e2e/capabilities/e2e.json"

case "${1:-}" in
  build)
    install -m 0644 "$template" "$generated"
    rustup run 1.90.0 cargo build --manifest-path "$app_root/src-tauri/Cargo.toml" \
      --locked --features desktop-e2e
    echo "e2e app binary: $app_root/src-tauri/target/debug/codegg-desktop"
    ;;
  clean)
    rm -f "$generated"
    echo "removed generated test-only capability"
    ;;
  *)
    echo "usage: build-e2e-app.sh {build|clean}" >&2
    exit 2
    ;;
esac
