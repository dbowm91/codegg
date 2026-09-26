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
#   ./scripts/detect-live-eggwork-changes.sh <event-name> [<base-sha> [<base-ref>]]
#
# In CI, pass `"${{ github.event_name }}"`,
# `"${{ github.event.pull_request.base.sha }}"`, and
# `"${{ github.event.pull_request.base.ref }}"`. The ref is the primary
# fetch source: GitHub does not serve arbitrary SHAs over the fetch
# protocol, so fetching the base SHA alone fails and would fail every
# PR open. The SHA is still used for the diff once objects are present.
#
# Prints `live_required=true|...` progress to stdout for the CI log and
# emits exactly `live_required=<true|false>` on the last line for
# `$GITHUB_OUTPUT` capture. Exit status is always 0 (the decision, not
# the detection health, is the output; failures resolve to true).
#
# Local simulation (objects already exist locally, no fetch needed):
#   ./scripts/detect-live-eggwork-changes.sh pull_request <base-sha>

set -u

EVENT_NAME="${1:-push}"
BASE_SHA="${2:-}"
BASE_REF="${3:-}"

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

# Shallow CI clones usually lack the base objects: fetch the base *ref*
# (branch tip), which the protocol serves. A raw-SHA fetch is not
# attempted: GitHub rejects it, and treating that rejection as fatal
# would fail every PR open. If objects are already present (local
# simulation, full clone), skip the fetch and diff directly.
BASE_REV="$BASE_SHA"
if git cat-file -e "$BASE_SHA^{commit}" >/dev/null 2>&1; then
    echo "detect-live-eggwork: base objects present locally, no fetch needed"
elif [ -n "$BASE_REF" ] && git fetch --no-tags --depth=1 origin "$BASE_REF" >/dev/null 2>&1; then
    BASE_REV="FETCH_HEAD"
    echo "detect-live-eggwork: fetched base ref $BASE_REF"
else
    fail_open "could not fetch base ref ${BASE_REF:-<none>}"
    exit 0
fi

# Actions checks out pull requests at a synthetic merge commit, while its
# shallow fetch of the base ref may omit the common ancestor needed by a
# three-dot diff. The merge tree already includes the base tree, so compare
# the fetched base tree directly to HEAD; on a PR this yields the changed
# paths without requiring history beyond the two available commits.
CHANGED="$(git diff --name-only "$BASE_REV" HEAD 2>/dev/null)" || {
    fail_open "could not diff $BASE_REV HEAD"
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
