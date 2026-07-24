#!/usr/bin/env bash
set -euo pipefail

bad_imports=$(rg -n "crate::(agent|tool[^_]|permission|mcp|plugin|tui|server|client|auth|crypto|search|search_backend|research|theme|tts|upgrade)" crates/codegg-core/src || true)
if [[ -n "$bad_imports" ]]; then
  echo "codegg-core has forbidden root-domain imports:"
  echo "$bad_imports"
  exit 1
fi

bad_deps=$(rg -n "ratatui|crossterm|ratatui_textarea|axum|tower_http|tokio_tungstenite|wasmtime|wasmtime_wasi" crates/codegg-core/Cargo.toml || true)
if [[ -n "$bad_deps" ]]; then
  echo "codegg-core appears to reference forbidden UI/server/plugin dependencies:"
  echo "$bad_deps"
  exit 1
fi

echo "codegg-core boundary check passed"
