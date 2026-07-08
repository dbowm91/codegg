#!/usr/bin/env bash
#
# capture-nextest-timing.sh
#
# Run nextest and produce a human-readable summary of the slowest tests.
# Requires cargo-nextest (install once with: cargo install cargo-nextest).
#
# Usage:
#   scripts/capture-nextest-timing.sh [PROFILE] [--all-features] [-- NEXTEST_ARGS...]
#
# Profiles (defined in .config/nextest.toml):
#   default    – 30s timeout, auto parallelism
#   ci-fast    – 20s timeout, auto parallelism
#   ci-heavy   – 60s timeout, serial
#   ci-release – 120s timeout, serial
#
# Options:
#   --top N          Number of slowest tests to show (default: 20)
#   --all-features   Pass --all-features to cargo nextest
#   --               Pass remaining args directly to cargo nextest
#
# Examples:
#   scripts/capture-nextest-timing.sh
#   scripts/capture-nextest-timing.sh ci-heavy
#   scripts/capture-nextest-timing.sh ci-heavy --all-features
#   scripts/capture-nextest-timing.sh default --top 10 -- -p codegg-core

set -euo pipefail

TOP_N=20
ALL_FEATURES=""
PROFILE="default"
EXTRA_ARGS=()

# ── Parse arguments ──────────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
    case "$1" in
        --top)
            TOP_N="$2"
            shift 2
            ;;
        --all-features)
            ALL_FEATURES="--all-features"
            shift
            ;;
        --)
            shift
            EXTRA_ARGS=("$@")
            break
            ;;
        -*)
            echo "Error: unknown flag '$1'" >&2
            exit 1
            ;;
        *)
            PROFILE="$1"
            shift
            ;;
    esac
done

# ── Preflight checks ────────────────────────────────────────────────────────
if ! command -v cargo &>/dev/null; then
    echo "Error: cargo not found. Install Rust via https://rustup.rs" >&2
    exit 1
fi

if ! cargo nextest --version &>/dev/null 2>&1; then
    echo "Error: cargo-nextest not found." >&2
    echo "Install it with: cargo install cargo-nextest" >&2
    exit 1
fi

echo "==> Profile: ${PROFILE}"
echo "==> Top slowest tests to show: ${TOP_N}"
echo ""

# ── Build first (--no-run) to separate compile time from test time ──────────
echo "==> Building tests (--no-run)..."
cargo nextest run \
    --workspace \
    --profile "${PROFILE}" \
    ${ALL_FEATURES} \
    --no-run \
    "${EXTRA_ARGS[@]}" \
    2>&1 | tail -1
echo ""

# ── Run tests and capture JSON timing data ───────────────────────────────────
TMPJSON="$(mktemp /tmp/nextest-timing-XXXXXX.json)"

echo "==> Running tests with timing..."
if ! cargo nextest run \
    --workspace \
    --profile "${PROFILE}" \
    ${ALL_FEATURES} \
    --json \
    "${EXTRA_ARGS[@]}" \
    > "${TMPJSON}" \
    2>/dev/null; then
    echo "==> Some tests failed — showing timing for completed tests."
fi

# ── Parse JSON and print summary ────────────────────────────────────────────
python3 - "${TMPJSON}" "${TOP_N}" <<'PYEOF'
import json
import sys

path = sys.argv[1]
top_n = int(sys.argv[2])

with open(path) as f:
    data = json.load(f)

# nextest JSON output: top-level "test" key with "executed" list
tests = data.get("test", {}).get("executed", [])

if not tests:
    # Fallback: older nextest versions use a flat "events" list
    tests = [
        e for e in data.get("events", [])
        if e.get("type") == "test" and e.get("event") in ("passed", "failed")
    ]

def duration(t):
    """Extract duration in seconds from a test event."""
    time_obj = t.get("time", {})
    if "duration" in time_obj:
        return time_obj["duration"]
    if "duration" in t:
        d = t["duration"]
        if isinstance(d, dict) and "secs" in d:
            return d["secs"] + d.get("nanos", 0) / 1e9
        return float(d)
    return 0.0

tests.sort(key=duration, reverse=True)

passed = sum(1 for t in tests if t.get("event", t.get("status", "")) in ("passed", "ok"))
failed = sum(1 for t in tests if t.get("event", t.get("status", "")) in ("failed", "fail"))
skipped = sum(1 for t in tests if t.get("event", t.get("status", "")) in ("skipped", "ignore"))
total_time = sum(duration(t) for t in tests)

print("=" * 72)
print(f"  Nextest Timing Summary")
print(f"  Total: {len(tests)}  |  Passed: {passed}  |  Failed: {failed}  |  Skipped: {skipped}")
print(f"  Aggregate time: {total_time:.1f}s")
print("=" * 72)
print()
print(f"  Top {min(top_n, len(tests))} slowest tests:")
print(f"  {'Duration':>10}  {'Test Name'}")
print(f"  {'-' * 10}  {'-' * 56}")

for t in tests[:top_n]:
    d = duration(t)
    name = t.get("name", t.get("test", "unknown"))
    if len(name) > 56:
        name = name[:53] + "..."
    print(f"  {d:>8.2f}s  {name}")

print()
PYEOF

rm -f "${TMPJSON}"
echo "==> Done."
