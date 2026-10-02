#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
metadata="$(cargo metadata --no-deps --format-version 1 --manifest-path "$root/Cargo.toml")"
if printf '%s' "$metadata" | rg -q '"name":"(tauri|tauri-build|codegg-desktop)"'; then
  echo 'desktop package leaked into root Cargo metadata' >&2
  exit 1
fi
if ! rg -q 'rust-version = "1\.89"' "$root/Cargo.toml"; then
  echo 'root Rust MSRV changed from 1.89' >&2
  exit 1
fi
config="$root/apps/desktop/src-tauri/tauri.conf.json"
capability="$root/apps/desktop/src-tauri/capabilities/main.json"
if rg -ni 'unsafe-eval|https://(?!schema\.tauri\.app)' "$config" --pcre2; then
  echo 'unexpected remote renderer origin or unsafe-eval in Tauri config' >&2
  exit 1
fi
if rg -ni 'shell|filesystem|fs:|http:default|process:|updater|clipboard|global-shortcut' "$capability"; then
  echo 'forbidden renderer permission in main capability' >&2
  exit 1
fi
if rg -n 'core_request|read_file|invoke.*shell' "$root/apps/desktop/src" "$root/apps/desktop/src-tauri/src"; then
  echo 'generic machine-authority bridge found' >&2
  exit 1
fi
python3 - "$config" "$capability" <<'PY'
import json
import sys

config = json.load(open(sys.argv[1], encoding="utf-8"))
capability = json.load(open(sys.argv[2], encoding="utf-8"))
assert capability["windows"] == ["main"], "capability must be limited to the main window"
assert capability["permissions"] == [], "renderer must receive no Tauri plugin commands"
assert config["app"]["windows"][0]["url"] == "index.html", "main window must load a local asset"
csp = config["app"]["security"]["csp"]
assert "unsafe-eval" not in csp, "unsafe-eval is forbidden"
assert "https://" not in csp, "remote origins are forbidden in renderer CSP"
PY
echo 'desktop dependency and authority boundaries verified'
