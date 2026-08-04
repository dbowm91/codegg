#!/usr/bin/env bash
set -euo pipefail

match_boundary() {
  local pattern="$1"
  shift

  local output status=0
  output=$(grep -rInE "$pattern" "$@") || status=$?
  if [[ "$status" -eq 0 ]]; then
    printf '%s\n' "$output"
    return 0
  fi

  if [[ "$status" -eq 1 ]]; then
    return 1
  fi

  echo "codegg-core boundary matcher failed (grep exit status $status)" >&2
  return "$status"
}

bad_imports=""
if bad_imports=$(match_boundary "crate::(agent|tool[^_]|permission|mcp|plugin|tui|server|client|auth|crypto|search|search_backend|research|theme|tts|upgrade)" crates/codegg-core/src); then
  :
else
  status=$?
  if [[ "$status" -ne 1 ]]; then
    exit "$status"
  fi
fi
if [[ -n "$bad_imports" ]]; then
  echo "codegg-core has forbidden root-domain imports:"
  echo "$bad_imports"
  exit 1
fi

bad_deps=""
if bad_deps=$(match_boundary "ratatui|crossterm|ratatui_textarea|axum|tower_http|tokio_tungstenite|wasmtime|wasmtime_wasi" crates/codegg-core/Cargo.toml); then
  :
else
  status=$?
  if [[ "$status" -ne 1 ]]; then
    exit "$status"
  fi
fi
if [[ -n "$bad_deps" ]]; then
  echo "codegg-core appears to reference forbidden UI/server/plugin dependencies:"
  echo "$bad_deps"
  exit 1
fi

echo "codegg-core boundary check passed"
