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
#   Broad Cargo commands use CARGO_BUILD_JOBS=2 by default (local
#   developer machines). CI overrides this per-runner (see
#   .github/workflows/ci.yml). Test execution stays serial within each
#   binary; cross-binary parallelism comes from nextest profile `ci`.
#   Callers may override any env var before invoking this script.
#
# The script stops at the first failing command and returns its status.

set -euo pipefail

# ── Resolve repository root from script location ────────────────────────────
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# ── Broad-test resource contract (matches CI) ───────────────────────────────
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"

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
  Both modes set CARGO_BUILD_JOBS=2 by default.
  Test execution uses nextest profile `ci` (serial within each binary,
  parallel across binaries); plain `cargo test` broad runs keep
  --test-threads=1.
  Callers may override via environment variables.
  cargo-nextest is required for full mode.
EOF
}

# ── Quick tier ──────────────────────────────────────────────────────────────
run_quick() {
    echo "==> Quick verification"

    echo "==> cargo fmt --check --all"
    (cd "$REPO_ROOT" && cargo fmt --check --all)

    echo "==> python3 scripts/generate_builtin_agents.py --check"
    (cd "$REPO_ROOT" && python3 scripts/generate_builtin_agents.py --check)

    echo "==> ./scripts/check-core-boundary.sh"
    (cd "$REPO_ROOT" && ./scripts/check-core-boundary.sh)

    echo "==> python3 scripts/check_sandbox_contract.py"
    (cd "$REPO_ROOT" && python3 scripts/check_sandbox_contract.py)

    echo "==> python3 scripts/check_execution_ownership.py"
    (cd "$REPO_ROOT" && python3 scripts/check_execution_ownership.py)

    echo "==> python3 scripts/check_tui_project_authority.py"
    (cd "$REPO_ROOT" && python3 scripts/check_tui_project_authority.py)

    echo "==> python3 scripts/check_http_route_disposition.py"
    (cd "$REPO_ROOT" && python3 scripts/check_http_route_disposition.py)

    echo "==> python3 scripts/check_audit_coverage.py"
    (cd "$REPO_ROOT" && python3 scripts/check_audit_coverage.py)

    echo "==> python3 scripts/check_scheduler_bypass.py"
    (cd "$REPO_ROOT" && python3 scripts/check_scheduler_bypass.py)

    echo "==> python3 scripts/check_eggwork_target_routing.py"
    (cd "$REPO_ROOT" && python3 scripts/check_eggwork_target_routing.py)

    echo "==> CARGO_BUILD_JOBS=$CARGO_BUILD_JOBS cargo check --workspace --all-targets --locked"
    (cd "$REPO_ROOT" && cargo check --workspace --all-targets --locked)

    echo "==> Quick verification passed."
}

# ── Full tier ───────────────────────────────────────────────────────────────
run_full() {
    echo "==> Full verification"
    echo "==> Broad-test environment: CARGO_BUILD_JOBS=$CARGO_BUILD_JOBS nextest profile ci"

    if ! command -v cargo-nextest >/dev/null 2>&1; then
        echo "Error: full mode requires cargo-nextest (cargo install cargo-nextest --locked)" >&2
        exit 1
    fi

    # Quick checks first
    run_quick

    echo "==> CARGO_BUILD_JOBS=$CARGO_BUILD_JOBS cargo clippy --workspace --all-targets --locked -- -D warnings"
    (cd "$REPO_ROOT" && cargo clippy --workspace --all-targets --locked -- -D warnings)

    echo "==> CARGO_BUILD_JOBS=$CARGO_BUILD_JOBS cargo nextest run --workspace --locked --profile ci"
    (cd "$REPO_ROOT" && cargo nextest run --workspace --locked --profile ci)

    echo "==> CARGO_BUILD_JOBS=$CARGO_BUILD_JOBS cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support --profile ci"
    (cd "$REPO_ROOT" && cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support --profile ci)

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
