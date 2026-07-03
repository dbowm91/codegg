#!/usr/bin/env bash
# Manual validation harness for the plugin UI stack.
#
# This script exercises the same checks the GitHub Actions plugin-focused
# job runs. It is intended as a local fallback when CI is unavailable or
# when a contributor wants to validate the plugin UI before pushing.
#
# Usage:
#   ./scripts/validate_plugin_ui.sh
#
# Requires: cargo, rustfmt, clippy, ripgrep, and (optionally) rustup with the
# wasm32-unknown-unknown target for the WASM example checks.

set -euo pipefail

cd "$(dirname "$0")/.."

step() {
    printf '\n=== %s ===\n' "$1"
}

fail() {
    printf '\nFAILED: %s\n' "$1" >&2
    exit 1
}

step "cargo fmt --check --all"
cargo fmt --check --all || fail "rustfmt reported formatting changes; run 'cargo fmt' to fix."

step "cargo check --workspace --all-features --all-targets"
cargo check --workspace --all-features --all-targets || fail "cargo check failed."

step "cargo clippy --workspace --all-targets --all-features -- -D warnings"
cargo clippy --workspace --all-targets --all-features -- -D warnings \
    || fail "clippy reported warnings."

step "cargo test --workspace --all-features"
cargo test --workspace --all-features -- --test-threads=1 || fail "workspace tests failed."

step "Plugin install path / policy tests"
cargo test -p codegg --lib plugin::install:: --all-features \
    || fail "plugin::install tests failed."

step "Plugin management tests"
cargo test -p codegg --lib plugin::management:: --all-features \
    || fail "plugin::management tests failed."

step "Plugin registry tests"
cargo test -p codegg --lib plugin::registry:: --all-features \
    || fail "plugin::registry tests failed."

step "Plugin TUI command tests"
cargo test -p codegg --lib tui::commands::plugin_management:: --all-features \
    || fail "plugin TUI command tests failed."

step "codegg-core boundary check"
./scripts/check-core-boundary.sh || fail "codegg-core boundary check failed."

if command -v rustup >/dev/null 2>&1; then
    step "Rust SDK plugin tests"
    cargo test --manifest-path examples/plugins/sdk-rust/Cargo.toml \
        || fail "Rust plugin SDK tests failed."

    if command -v python3 >/dev/null 2>&1; then
        step "Python SDK plugin tests"
        PYTHONPATH=examples/plugins/sdk-python \
            python3 -m unittest discover examples/plugins/sdk-python/tests -v \
            || fail "Python plugin SDK tests failed."
    else
        echo "Skipping Python SDK tests: python3 not on PATH."
    fi

    if rustup target list --installed 2>/dev/null | grep -q wasm32-unknown-unknown; then
        step "WASM example: command-table"
        cargo check \
            --manifest-path examples/plugins/wasm-command-table/Cargo.toml \
            --target wasm32-unknown-unknown \
            || fail "wasm-command-table check failed."

        step "WASM example: hook-message-transform"
        cargo check \
            --manifest-path examples/plugins/wasm-hook-message-transform/Cargo.toml \
            --target wasm32-unknown-unknown \
            || fail "wasm-hook-message-transform check failed."

        step "WASM example: status-widget"
        cargo check \
            --manifest-path examples/plugins/wasm-status-widget/Cargo.toml \
            --target wasm32-unknown-unknown \
            || fail "wasm-status-widget check failed."
    else
        echo "Skipping WASM example checks: target wasm32-unknown-unknown not installed."
        echo "Run 'rustup target add wasm32-unknown-unknown' to enable."
    fi
else
    echo "Skipping SDK / WASM example checks: rustup not on PATH."
fi

printf '\nAll plugin UI validation checks passed.\n'