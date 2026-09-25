#!/usr/bin/env bash
#
# detect-live-eggwork-changes.sh — Decide whether the real live-Eggwork
# qualification must run for this change (M002 WP4).
#
# Policy (see plans/implementation/.../002-*.md WP4):
#   - pushes to `main` always qualify (authoritative Linux signal);
#   - pull requests qualify only when changed paths can affect Eggwork
#     remote execution, scheduler integration, sandbox/helper behavior,
#     or the live fixture itself;
#   - any detection error fails OPEN (runs live) so coverage can never
#     silently disappear because a selector matched zero tests.
#
# Usage:
#   ./scripts/detect-live-eggwork-changes.sh <event-name> [<base-sha>]
#
# Prints `live_required=true|...` progress to stdout for the CI log and
# emits exactly `live_required=<true|false>` on the last line for
# `$GITHUB_OUTPUT` capture. Exit status is always 0 (the decision, not
# the detection health, is the output; failures resolve to true).
#
# Local simulation:
#   ./scripts/detect-live-eggwork-changes.sh pull_request <base-sha>

set -u

EVENT_NAME="${1:-push}"
BASE_SHA="${2:-}"

# Paths whose changes can affect live Eggwork qualification.
LIVE_PATTERN='^(src/scheduler/|src/security/sandbox|src/bin/codegg-sandbox-helper\.rs|crates/eggwork-test-node/|tests/eggwork_remote_execution|\.github/workflows/ci\.yml$|\.config/nextest\.toml$|scripts/prebuild-eggwork-fixtures\.sh$|scripts/detect-live-eggwork-changes\.sh$|Cargo\.toml$|Cargo\.lock$)'

fail_open() {
    echo "detect-live-eggwork: $1; failing open (live_required=true)" >&2
    echo "live_required=true"
}

if [ "$EVENT_NAME" != "pull_request" ]; then
    echo "detect-live-eggwork: event=$EVENT_NAME (authoritative main/push signal)"
    echo "live_required=true"
    exit 0
fi

if [ -z "$BASE_SHA" ]; then
    fail_open "pull_request without base sha"
    exit 0
fi

if ! git fetch --no-tags --depth=1 origin "$BASE_SHA" >/dev/null 2>&1; then
    fail_open "could not fetch base $BASE_SHA"
    exit 0
fi

CHANGED="$(git diff --name-only "$BASE_SHA...HEAD" 2>/dev/null)" || {
    fail_open "could not diff $BASE_SHA...HEAD"
    exit 0
}

if [ -z "$CHANGED" ]; then
    fail_open "empty changed-file set"
    exit 0
fi

MATCHED="$(printf '%s\n' "$CHANGED" | grep -E "$LIVE_PATTERN" || true)"
if [ -n "$MATCHED" ]; then
    echo "detect-live-eggwork: live-relevant changes:"
    printf '%s\n' "$MATCHED" | sed 's/^/  /'
    echo "live_required=true"
else
    echo "detect-live-eggwork: no live-relevant changes in $(printf '%s\n' "$CHANGED" | wc -l | tr -d ' ') files (ordinary-PR path; live stays qualified on main)"
    echo "live_required=false"
fi
exit 0
