#!/usr/bin/env bash
set -euo pipefail

manifest="crates/codegg-client/Cargo.toml"
forbidden='codegg =|codegg-core|codegg-providers|ratatui|crossterm|axum|tower_http|wasmtime|tauri'
if rg -n "$forbidden" "$manifest"; then
  echo "codegg-client has a forbidden root, domain, UI, server, or desktop dependency" >&2
  exit 1
fi

if rg -n 'crate::(tui|server|agent|provider|tool|storage|scheduler)|codegg::(tui|server|agent|provider|tool|storage|scheduler)' crates/codegg-client/src; then
  echo "codegg-client imports a root-owned runtime or presentation module" >&2
  exit 1
fi

echo "codegg-client dependency boundary passed"
