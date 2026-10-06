#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
    echo "usage: $0 <sdm-checkout> <expected-full-commit-sha>" >&2
    exit 2
fi

sdm_repo=$1
expected_revision=$2
if [[ ! "$expected_revision" =~ ^[0-9a-f]{40}$ ]]; then
    echo "expected revision must be a full 40-character commit SHA" >&2
    exit 2
fi
actual_revision=$(git -C "$sdm_repo" rev-parse HEAD)
if [[ "$actual_revision" != "$expected_revision" ]]; then
    echo "SDM checkout revision mismatch: expected $expected_revision, got $actual_revision" >&2
    exit 1
fi

cargo run --locked --manifest-path "$sdm_repo/Cargo.toml" \
    -p sdm-training --bin sdm-check-compat -- \
    "$sdm_repo/fixtures/codegg-compatibility-v1.jsonl" \
    "$sdm_repo/artifacts/compatibility-rank-v1.json"
