#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
EXPECTED_VERSION="0.4.4"
EXPECTED_COMMIT="13fcac0b7ac0125d38e4e9775ebe1d62028fda72"
TEMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/codegg-eggsearch-pair.XXXXXX")"
SOURCE_ROOT=""
cleanup() {
    if [[ "${CODEGG_EGGSEARCH_KEEP_TEMP:-0}" == "1" ]]; then
        printf 'pair temporary files retained at %s\n' "$TEMP_ROOT"
    else
        rm -rf -- "$TEMP_ROOT"
    fi
}
trap cleanup EXIT INT TERM

run_stage() {
    stage="$1"
    shift
    printf 'stage=%s\n' "$stage"
    "$@"
}

if [[ -n "${CODEGG_EGGSEARCH_BIN:-}" && -n "${CODEGG_EGGSEARCH_SOURCE:-}" ]]; then
    printf '%s\n' "error: set only one of CODEGG_EGGSEARCH_BIN or CODEGG_EGGSEARCH_SOURCE" >&2
    exit 2
fi

if [[ -n "${CODEGG_EGGSEARCH_SOURCE:-}" ]]; then
    source_root="$(cd "$CODEGG_EGGSEARCH_SOURCE" && pwd -P)"
    source_commit="$(git -C "$source_root" rev-parse "$EXPECTED_COMMIT^{commit}")"
    [[ "$source_commit" == "$EXPECTED_COMMIT" ]] || {
        printf 'error: Eggsearch source revision resolves to %s, expected immutable commit %s\n' "$source_commit" "$EXPECTED_COMMIT" >&2
        exit 2
    }
    SOURCE_ROOT="$TEMP_ROOT/eggsearch-source"
    mkdir -p "$SOURCE_ROOT"
    git -C "$source_root" archive "$EXPECTED_COMMIT" | tar -x -C "$SOURCE_ROOT"
    printf 'building eggsearch %s from immutable commit %s, with Cargo offline\n' "$EXPECTED_VERSION" "$EXPECTED_COMMIT"
    run_stage source_build cargo build --offline --locked --release --manifest-path "$SOURCE_ROOT/Cargo.toml" \
        --target-dir "$TEMP_ROOT/eggsearch-target" --bin eggsearch || exit $?
    CODEGG_EGGSEARCH_BIN="$TEMP_ROOT/eggsearch-target/release/eggsearch"
    CODEGG_EGGSEARCH_TAG="commit:$EXPECTED_COMMIT"
elif [[ -n "${CODEGG_EGGSEARCH_BIN:-}" ]]; then
    CODEGG_EGGSEARCH_BIN="$(cd "$(dirname "$CODEGG_EGGSEARCH_BIN")" && pwd -P)/$(basename "$CODEGG_EGGSEARCH_BIN")"
    CODEGG_EGGSEARCH_TAG="v$EXPECTED_VERSION (binary-supplied)"
else
    printf '%s\n' "error: set CODEGG_EGGSEARCH_BIN to an explicit binary or CODEGG_EGGSEARCH_SOURCE to a checkout containing immutable Eggsearch commit $EXPECTED_COMMIT" >&2
    exit 2
fi
export CODEGG_EGGSEARCH_BIN CODEGG_EGGSEARCH_TAG
export CODEGG_EGGSEARCH_REAL_BIN="$CODEGG_EGGSEARCH_BIN"

[[ -f "$CODEGG_EGGSEARCH_BIN" && -x "$CODEGG_EGGSEARCH_BIN" ]] || {
    printf 'error: stage=version_identity eggsearch binary is missing or not executable: %s\n' "$CODEGG_EGGSEARCH_BIN" >&2
    exit 2
}
version_line="$("$CODEGG_EGGSEARCH_BIN" --version)"
version_token="$(printf '%s\n' "$version_line" | awk 'NR == 1 { for (i = 1; i < NF; i++) if (tolower($i) == "eggsearch") { print $(i + 1); exit } }')"
version_token="${version_token#v}"
version_token="${version_token#V}"
[[ "$version_token" == "$EXPECTED_VERSION" ]] || {
    printf 'error: stage=version_identity expected exact eggsearch %s, got: %s\n' "$EXPECTED_VERSION" "$version_line" >&2
    exit 2
}
if command -v sha256sum >/dev/null 2>&1; then
    binary_digest="$(sha256sum "$CODEGG_EGGSEARCH_BIN" | awk '{print $1}')"
else
    binary_digest="$(shasum -a 256 "$CODEGG_EGGSEARCH_BIN" | awk '{print $1}')"
fi
CODEGG_REVISION="$(git -C "$ROOT" rev-parse HEAD)"
export CODEGG_REVISION
printf 'pair input: codegg=%s eggsearch=%s tag=%s sha256=%s\n' \
    "$CODEGG_REVISION" "$version_line" "$CODEGG_EGGSEARCH_TAG" "$binary_digest"

cd "$ROOT"
export CARGO_NET_OFFLINE=true
run_stage format cargo fmt --all -- --check
run_stage provider_contract_tests cargo test -p codegg --lib search_backend::eggsearch -- --test-threads=1
run_stage bootstrap_tests cargo test -p codegg --lib search_backend::bootstrap -- --test-threads=1
run_stage research_adapter_tests cargo test -p codegg --lib research::sources::eggsearch -- --test-threads=1
run_stage search_backend_tests cargo test -p codegg --test search_backend_eggsearch -- --test-threads=1
run_stage provider_mapping_tests cargo test -p codegg --test search_backend_arg_mapping -- --test-threads=1
run_stage fake_mcp_tests cargo test -p codegg --test fake_eggsearch_mcp -- --test-threads=1
run_stage mcp_child_lifecycle_tests cargo test -p codegg --lib mcp::local::tests -- --test-threads=1
printf 'stage=real_wrapper_request child_environment structured_fidelity teardown\n'
export BRAVE_API_KEY=parent-only-test-sentinel
export HTTP_PROXY=http://127.0.0.1:9
export http_proxy=http://127.0.0.1:9
export CODEGG_PAIR_PARENT_SECRET_SENTINEL=parent-only-test-sentinel
cargo test -p codegg --test eggsearch_pair_compat -- --ignored --exact pinned_eggsearch_process_pair_is_compatible_offline --nocapture
run_stage agent_loop_scoped_test cargo test -p codegg --test agent_loop_harness -- --ignored --exact pinned_eggsearch_agent_loop_uses_each_accepted_workspace_root --nocapture
unset BRAVE_API_KEY HTTP_PROXY http_proxy CODEGG_PAIR_PARENT_SECRET_SENTINEL
run_stage verify_quick scripts/verify.sh quick

printf 'eggsearch pair compatibility passed (CodeGG %s + eggsearch %s)\n' "$CODEGG_REVISION" "$version_line"
