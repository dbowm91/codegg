#!/usr/bin/env bash
#
# verify.sh — Canonical local verification entry point for CodeGG.
#
# Usage:
#   scripts/verify.sh quick    — cheap repository sanity for ordinary iteration
#   scripts/verify.sh full     — broad maintainer/developer verification before handoff or release
#   scripts/verify.sh help     — print usage
#
# Resource policy:
#   Broad Cargo commands use CARGO_BUILD_JOBS=1 and --test-threads=1.
#   No optional external tools are required in either canonical mode.
#
# The script stops at the first failing command and returns its status.

set -euo pipefail

# ── Resolve repository root from script location ────────────────────────────
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# ── Usage ───────────────────────────────────────────────────────────────────
usage() {
    cat <<EOF
Usage: scripts/verify.sh <mode>

Modes:
  quick   Cheap repository sanity for ordinary iteration.
          Developers run focused tests for changed code separately.
  full    Broad maintainer verification before handoff or release.
  help    Print this message.

Resource policy:
  Both modes set CARGO_BUILD_JOBS=1. Full mode passes --test-threads=1
  to broad workspace tests. No optional external tools are required.
EOF
}

# ── Quick tier ──────────────────────────────────────────────────────────────
run_quick() {
    echo "==> Quick verification"

    echo "==> cargo fmt --check --all"
    (cd "$REPO_ROOT" && cargo fmt --check --all)

    echo "==> python3 scripts/generate_builtin_agents.py --check"
    (cd "$REPO_ROOT" && python3 scripts/generate_builtin_agents.py --check)

    echo "==> python3 scripts/check_builtin_agents.py"
    (cd "$REPO_ROOT" && python3 scripts/check_builtin_agents.py)

    echo "==> python3 scripts/check-tokio-test-flavors.py"
    (cd "$REPO_ROOT" && python3 scripts/check-tokio-test-flavors.py)

    echo "==> ./scripts/check-core-boundary.sh"
    (cd "$REPO_ROOT" && ./scripts/check-core-boundary.sh)

    echo "==> CARGO_BUILD_JOBS=1 cargo check --workspace --all-targets --locked"
    (cd "$REPO_ROOT" && CARGO_BUILD_JOBS=1 cargo check --workspace --all-targets --locked)

    echo "==> Quick verification passed."
}

# ── Full tier ───────────────────────────────────────────────────────────────
run_full() {
    echo "==> Full verification"

    # Quick checks first
    run_quick

    echo "==> CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --locked -- -D warnings"
    (cd "$REPO_ROOT" && CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --locked -- -D warnings)

    echo "==> CARGO_BUILD_JOBS=1 cargo test --workspace --locked -- --test-threads=1"
    (cd "$REPO_ROOT" && CARGO_BUILD_JOBS=1 cargo test --workspace --locked -- --test-threads=1)

    echo "==> CARGO_BUILD_JOBS=1 cargo check -p codegg --locked --features server,plugins,lsp-test-support"
    (cd "$REPO_ROOT" && CARGO_BUILD_JOBS=1 cargo check -p codegg --locked --features server,plugins,lsp-test-support)

    echo "==> Full verification passed."
}

# ── Main ────────────────────────────────────────────────────────────────────
case "${1:-}" in
    quick)
        run_quick
        ;;
    full)
        run_full
        ;;
    help|--help|-h)
        usage
        ;;
    *)
        echo "Error: unknown mode '${1:-}'" >&2
        echo "" >&2
        usage >&2
        exit 1
        ;;
esac
