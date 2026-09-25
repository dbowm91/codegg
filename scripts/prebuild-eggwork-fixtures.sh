#!/usr/bin/env bash
#
# prebuild-eggwork-fixtures.sh — Deterministic prebuild of the Eggwork live
# qualification fixtures (M002).
#
# Builds, exactly once:
#   1. `codegg-eggwork-test-node` from `crates/eggwork-test-node`
#      (standalone workspace, own lockfile — kept separate so
#      `eggwork-server`'s sqlite linkage never collides with the root
#      workspace's `sqlx` sqlite linkage);
#   2. the pinned `eggwork-sandbox-helper` from the same immutable Eggwork
#      revision, with `--target-dir` pointed at the helper workspace
#      target dir (mirrors `ensure_helper_built` in
#      `tests/eggwork_remote_execution_live.rs`).
#
# CI calls this once before live execution and exports
# `CODEGG_EGGWORK_TEST_NODE` plus `CODEGG_EGGWORK_FIXTURES_PREBUILT=1`,
# so per-test processes assert on prebuilt binaries instead of each
# discovering/building the fixture independently. Test-side fallback
# remains for local direct invocation (the fixtures gate only skips
# work when `CODEGG_EGGWORK_FIXTURES_PREBUILT=1`).
#
# Linux-only: the helper links Linux-only `landlock`. On other
# platforms this script reports a skip and exits 0; the live test
# target compiles to zero tests there.
#
# Usage:
#   ./scripts/prebuild-eggwork-fixtures.sh
#   CODEGG_EGGWORK_TEST_NODE=/path/to/bin ./scripts/prebuild-eggwork-fixtures.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
HELPER_MANIFEST="$REPO_ROOT/crates/eggwork-test-node/Cargo.toml"
NODE_BIN="${CODEGG_EGGWORK_TEST_NODE:-$REPO_ROOT/crates/eggwork-test-node/target/debug/codegg-eggwork-test-node}"

if [ "$(uname -s)" != "Linux" ]; then
    echo "prebuild-eggwork-fixtures: skip (Linux-only fixtures; $(uname -s) builds zero live tests)"
    exit 0
fi

if [ ! -f "$HELPER_MANIFEST" ]; then
    echo "prebuild-eggwork-fixtures: missing helper manifest: $HELPER_MANIFEST" >&2
    exit 1
fi

if [ ! -x "$NODE_BIN" ]; then
    echo "prebuild-eggwork-fixtures: building codegg-eggwork-test-node ..."
    cargo build --locked --manifest-path "$HELPER_MANIFEST"
else
    echo "prebuild-eggwork-fixtures: test node already present: $NODE_BIN"
fi

if [ ! -x "$NODE_BIN" ]; then
    echo "prebuild-eggwork-fixtures: node binary still missing after build: $NODE_BIN" >&2
    exit 1
fi

# Resolve the pinned Eggwork runner manifest exactly like the live test
# does (cargo metadata over the helper workspace), then build the
# sandbox helper from the pinned Eggwork root into the helper target
# dir so both fixtures share one artifact tree.
RUNNER_MANIFEST="$(cargo metadata --locked --format-version 1 --manifest-path "$HELPER_MANIFEST" \
    | python3 -c 'import json,sys; print(next(p["manifest_path"] for p in json.load(sys.stdin)["packages"] if p["name"] == "eggwork-runner"))')"
EGGWORK_ROOT="$(dirname "$(dirname "$(dirname "$RUNNER_MANIFEST")")")"
TARGET_DIR="$(dirname "$(dirname "$NODE_BIN")")"
SANDBOX_HELPER="$TARGET_DIR/debug/eggwork-sandbox-helper"

echo "prebuild-eggwork-fixtures: building pinned eggwork-sandbox-helper into $TARGET_DIR ..."
cargo build --locked \
    --manifest-path "$EGGWORK_ROOT/Cargo.toml" \
    --package eggwork-sandbox-helper \
    --target-dir "$TARGET_DIR"

if [ ! -x "$SANDBOX_HELPER" ]; then
    echo "prebuild-eggwork-fixtures: sandbox helper still missing after build: $SANDBOX_HELPER" >&2
    exit 1
fi

echo "prebuild-eggwork-fixtures: ready"
echo "  CODEGG_EGGWORK_TEST_NODE=$NODE_BIN"
echo "  eggwork-sandbox-helper=$SANDBOX_HELPER"
