#!/usr/bin/env bash
# run-e2e.sh — Canonical built-app E2E entry point for C003 qualification.
#
# Usage (from apps/desktop):
#   CODEGG_DAEMON_EXECUTABLE=/path/to/codegg ./e2e/run-e2e.sh
#
# Builds the fixture helper and the desktop-e2e feature app binary, runs the
# WebdriverIO trajectory against an isolated daemon home, then restores the
# production-clean tree (removes the generated test-only capability).
# Exit status is the WebdriverIO result; screenshots/logs are supplementary —
# the machine assertions in e2e/specs are the closure authority.
set -euo pipefail
app_root="$(cd "$(dirname "$0")/.." && pwd)"
repo_root="$(cd "$app_root/../.." && pwd)"

if [[ -z "${CODEGG_DAEMON_EXECUTABLE:-}" ]]; then
  candidate="$repo_root/target/debug/codegg"
  if [[ -x "$candidate" ]]; then
    export CODEGG_DAEMON_EXECUTABLE="$candidate"
  else
    echo "CODEGG_DAEMON_EXECUTABLE is required (build the root codegg binary first)" >&2
    exit 2
  fi
fi
if [[ ! -x "$CODEGG_DAEMON_EXECUTABLE" ]]; then
  echo "daemon executable is missing or not executable: $CODEGG_DAEMON_EXECUTABLE" >&2
  exit 2
fi

cleanup() {
  "$app_root/e2e/build-e2e-app.sh" clean >/dev/null 2>&1 || true
}
trap cleanup EXIT

echo "==> building E2E fixture helper"
rustup run 1.90.0 cargo build --manifest-path "$app_root/src-tauri/Cargo.toml" \
  --locked --bin desktop_e2e_fixture
echo "==> building desktop app with the desktop-e2e feature"
"$app_root/e2e/build-e2e-app.sh" build
echo "==> running WebdriverIO trajectory (CODEGG_DAEMON_EXECUTABLE=$CODEGG_DAEMON_EXECUTABLE)"
# The debug app binary resolves its frontendDist (../dist) relative to the
# working directory, so the runner executes from src-tauri. (A bundled
# `tauri build` artifact embeds assets instead; the debug binary is the same
# compiled app without packaging.)
(
  cd "$app_root/src-tauri"
  npx wdio run "$app_root/e2e/wdio.conf.ts"
)
