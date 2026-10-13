#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
EXPECTED_VERSION="0.4.1"
EXPECTED_COMMIT="32769d684417567dfb30fc5adea1475cee7f0ed0"
EXPECTED_TAG="v0.4.1"
TEMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/codegg-eggsearch-pair.XXXXXX")"
SOURCE_ROOT=""
cleanup() {
    rm -rf -- "$TEMP_ROOT"
}
trap cleanup EXIT INT TERM

if [[ -n "${CODEGG_EGGSEARCH_BIN:-}" && -n "${CODEGG_EGGSEARCH_SOURCE:-}" ]]; then
    printf '%s\n' "error: set only one of CODEGG_EGGSEARCH_BIN or CODEGG_EGGSEARCH_SOURCE" >&2
    exit 2
fi

if [[ -n "${CODEGG_EGGSEARCH_SOURCE:-}" ]]; then
    source_root="$(cd "$CODEGG_EGGSEARCH_SOURCE" && pwd -P)"
    tag_commit="$(git -C "$source_root" rev-parse "refs/tags/$EXPECTED_TAG^{commit}")"
    [[ "$tag_commit" == "$EXPECTED_COMMIT" ]] || {
        printf 'error: %s resolves to %s, expected immutable commit %s\n' "$EXPECTED_TAG" "$tag_commit" "$EXPECTED_COMMIT" >&2
        exit 2
    }
    SOURCE_ROOT="$TEMP_ROOT/eggsearch-source"
    mkdir -p "$SOURCE_ROOT"
    git -C "$source_root" archive "$EXPECTED_COMMIT" | tar -x -C "$SOURCE_ROOT"
    printf 'building eggsearch %s from immutable %s (%s), with Cargo offline\n' "$EXPECTED_VERSION" "$EXPECTED_TAG" "$EXPECTED_COMMIT"
    CARGO_NET_OFFLINE=true cargo build --locked --release --manifest-path "$SOURCE_ROOT/Cargo.toml" \
        --target-dir "$TEMP_ROOT/eggsearch-target" --bin eggsearch
    CODEGG_EGGSEARCH_BIN="$TEMP_ROOT/eggsearch-target/release/eggsearch"
    CODEGG_EGGSEARCH_TAG="$EXPECTED_TAG"
elif [[ -n "${CODEGG_EGGSEARCH_BIN:-}" ]]; then
    CODEGG_EGGSEARCH_BIN="$(cd "$(dirname "$CODEGG_EGGSEARCH_BIN")" && pwd -P)/$(basename "$CODEGG_EGGSEARCH_BIN")"
    CODEGG_EGGSEARCH_TAG="v$EXPECTED_VERSION (binary-supplied)"
else
    printf '%s\n' "error: set CODEGG_EGGSEARCH_BIN to an explicit binary or CODEGG_EGGSEARCH_SOURCE to a checkout containing the immutable v0.4.1 tag" >&2
    exit 2
fi
export CODEGG_EGGSEARCH_BIN CODEGG_EGGSEARCH_TAG

[[ -f "$CODEGG_EGGSEARCH_BIN" && -x "$CODEGG_EGGSEARCH_BIN" ]] || {
    printf 'error: eggsearch binary is missing or not executable: %s\n' "$CODEGG_EGGSEARCH_BIN" >&2
    exit 2
}
version_line="$("$CODEGG_EGGSEARCH_BIN" --version)"
version_token="$(printf '%s\n' "$version_line" | awk 'NR == 1 { for (i = 1; i < NF; i++) if (tolower($i) == "eggsearch") { print $(i + 1); exit } }')"
version_token="${version_token#v}"
version_token="${version_token#V}"
[[ "$version_token" == "$EXPECTED_VERSION" ]] || {
    printf 'error: expected exact eggsearch %s, got: %s\n' "$EXPECTED_VERSION" "$version_line" >&2
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
cargo fmt --all -- --check
cargo test -p codegg --lib search_backend::eggsearch::tests::provider_hint -- --test-threads=1
cargo test -p codegg --lib search_backend::bootstrap::tests -- --test-threads=1
cargo test -p codegg --lib research::sources::eggsearch -- --test-threads=1
cargo test -p codegg --test search_backend_eggsearch -- --test-threads=1
cargo test -p codegg --test search_backend_arg_mapping -- --test-threads=1
cargo test -p codegg --test fake_eggsearch_mcp -- --test-threads=1
cargo test -p codegg --test eggsearch_pair_compat -- --ignored --exact pinned_eggsearch_process_pair_is_compatible_offline --nocapture
scripts/verify.sh quick

printf 'eggsearch pair compatibility passed (CodeGG %s + eggsearch %s)\n' "$CODEGG_REVISION" "$version_line"
